# She keeps answering through a minute of his clicks and carries, offline (no daemon, no Synaps).
# The hang of 2026-11-19 ("synaps-avatar.exe is not responding" seconds after a press on her) was
# her UI thread and her click-through poller waiting on each other; it takes his pointer crossing
# her outline while she animates. Here the pointer sweeps her window: each step a move, a short
# wait (the poller flips), then, where she takes the pointer, a click or a short carry. After every
# step a WM_NULL to her window must come back within 1 s. At the end she still runs, no WER hang
# or crash event names her, and her log has no panic.
#
#   tests\windows\test-hang.ps1 [-Exe PATH] [-Pack DIR] [-Seconds 60] [-With 'K=V', ...] [-ExpectHang]
#
# -Exe: default target\release\synaps-avatar.exe. -Pack: her pack; default %TEMP%\sa-hang-pack,
# built there as install.ps1 builds it (CapriCore at installer\manifest's pin + lachy, make-pack)
# unless it is there already (kept for the next run). -With: her environment. -ExpectHang: the run must catch a hang (the test bites: diag's SA_WIN_POLLER=old).
# Needs a desktop (the pointer, her windows): GitHub's Windows runners have one.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'CI log.')]
param(
    [string]$Exe,
    [string]$Pack,
    [int]$Seconds = 60,
    [string[]]$With = @(),
    [switch]$ExpectHang
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Exe) { $Exe = Join-Path $root 'target\release\synaps-avatar.exe' }
$Exe = (Resolve-Path $Exe).Path
$work = Join-Path ([IO.Path]::GetTempPath()) "sa-hang-$PID"
New-Item -ItemType Directory -Force $work | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class Her {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr res);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    // Her visible windows that take part in click-through (layered), biggest first is not
    // wanted: her body is the smallest one at least 64 px each way (the intro's is the screen's).
    public static IntPtr Body(int pid) {
        IntPtr best = IntPtr.Zero; long area = long.MaxValue;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            long ex = GetWindowLongPtr(h, -20).ToInt64();
            if ((ex & 0x80000) == 0) return true;  // WS_EX_LAYERED
            RECT r; GetWindowRect(h, out r);
            long w = r.R - r.L, hh = r.B - r.T, a = w * hh;
            if (w >= 64 && hh >= 64 && a < area) { area = a; best = h; }
            return true;
        }, IntPtr.Zero);
        return best;
    }
    public static int[] Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return new[] { r.L, r.T, r.R, r.B }; }
    // Whose window takes the pointer at x,y (her own: she takes it there).
    public static bool HersAt(int pid, int x, int y) {
        IntPtr h = WindowFromPoint(new POINT { X = x, Y = y });
        if (h == IntPtr.Zero) return false;
        uint p; GetWindowThreadProcessId(GetAncestor(h, 2), out p);
        return p == pid;
    }
    // A WM_NULL round trip, ms; -1: no answer within `ms` (or Windows calls her hung).
    public static long Ping(IntPtr h, uint ms) {
        var sw = Stopwatch.StartNew(); IntPtr res;
        IntPtr ok = SendMessageTimeoutW(h, 0, IntPtr.Zero, IntPtr.Zero, 2 /* SMTO_ABORTIFHUNG */, ms, out res);
        return ok == IntPtr.Zero ? -1 : sw.ElapsedMilliseconds;
    }
    public static void Down() { mouse_event(0x0002, 0, 0, 0, IntPtr.Zero); }
    public static void Up() { mouse_event(0x0004, 0, 0, 0, IntPtr.Zero); }
}
'@

# ---- her pack, as install.ps1 builds it ----------------------------------------------------------
if (-not $Pack) { $Pack = Join-Path ([IO.Path]::GetTempPath()) 'sa-hang-pack' }
if (-not (Test-Path (Join-Path $Pack 'sprites.json'))) {
    $m = @{}
    Get-Content (Join-Path $root 'installer\manifest') | Where-Object { $_ -match '^(\w+)=(.*)$' } | ForEach-Object { $m[$Matches[1]] = $Matches[2] }
    $cc = Join-Path $work 'CapriCore'
    New-Item -ItemType Directory -Force $cc | Out-Null
    $tgz = Join-Path $work 'capricore.tar.gz'
    Invoke-WebRequest "https://codeload.github.com/$($m.capricore_repo)/tar.gz/$($m.capricore_commit)" -OutFile $tgz
    tar.exe -xzf $tgz -C $cc --strip-components=1
    if ($LASTEXITCODE) { throw "tar failed ($LASTEXITCODE)" }
    python -m venv (Join-Path $work 'venv')
    $vpy = Join-Path $work 'venv\Scripts\python.exe'
    & $vpy -m pip install -q --disable-pip-version-check --no-cache-dir @($m.pip_pack -split '\s+' | Where-Object { $_ })
    if ($LASTEXITCODE) { throw "pip failed ($LASTEXITCODE)" }
    $env:XDG_CACHE_HOME = Join-Path $work 'xdg'
    & $vpy (Join-Path $root 'tools\make-pack') $cc $Pack --lachy auto
    if ($LASTEXITCODE) { throw "make-pack failed ($LASTEXITCODE)" }
    Remove-Item Env:\XDG_CACHE_HOME
}

# ---- her, offline --------------------------------------------------------------------------------
# Her log: her stderr, here (a console build would write to its console, a windowed one to its file).
$log = Join-Path $work 'her.log'
if (Get-Process synaps-avatar -ErrorAction SilentlyContinue) { throw 'another synaps-avatar runs: not a clean machine' }
$saved = @{}
foreach ($kv in $With) {
    $k, $v = $kv -split '=', 2
    $saved[$k] = [Environment]::GetEnvironmentVariable($k)
    [Environment]::SetEnvironmentVariable($k, $v)
}
$t0 = Get-Date
# Offline as on his box: a daemon socket nobody serves (a daemon running here is left alone).
$p = Start-Process -FilePath $Exe -ArgumentList @('--pack', "`"$Pack`"", '--socket', "\\.\pipe\sa-hang-nobody-$PID") -PassThru -RedirectStandardError $log -RedirectStandardOutput (Join-Path $work 'her.out')
foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
$null = $p.Handle
Write-Host "her pid $($p.Id) ($Exe; $($With -join ' '))"

# Ready: all her frames decoded, her body window up.
$deadline = (Get-Date).AddSeconds(120)
$body = [IntPtr]::Zero
while ((Get-Date) -lt $deadline) {
    if ($p.HasExited) { break }
    $ready = (Test-Path $log) -and (Select-String -LiteralPath $log -SimpleMatch 'all frames ready' -Quiet)
    $body = [Her]::Body($p.Id)
    if ($ready -and $body -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 250
}
if ($p.HasExited -or $body -eq [IntPtr]::Zero) {
    if (Test-Path $log) { Get-Content $log -Tail 40 | ForEach-Object { Write-Host "    | $_" } }
    throw "she didn't come up (exited: $($p.HasExited))"
}
Start-Sleep -Seconds 3
Write-Host "ready after $([int]((Get-Date) - $t0).TotalSeconds) s; her window 0x$($body.ToString('x')) at $([Her]::Rect($body) -join ',')"

# ---- a minute of his pointer ---------------------------------------------------------------------
$rng = [Random]::new(20261119)
$steps = 0; $presses = 0; $carries = 0; $worst = 0; $hung = $null
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end -and -not $p.HasExited) {
    $b = [Her]::Body($p.Id)
    if ($b -ne [IntPtr]::Zero) { $body = $b }
    $r = [Her]::Rect($body)
    $w = $r[2] - $r[0]; $h = $r[3] - $r[1]
    # Anywhere on her window: her outline is crossed both ways.
    $x = $r[0] + [int]($w * (0.15 + 0.7 * $rng.NextDouble())); $y = $r[1] + [int]($h * (0.1 + 0.8 * $rng.NextDouble()))
    [void][Her]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds (15 + $rng.Next(40))
    if ([Her]::HersAt($p.Id, $x, $y)) {
        [Her]::Down()
        if ($rng.Next(3) -eq 0) {
            # A short carry.
            for ($i = 1; $i -le 6; $i++) { [void][Her]::SetCursorPos($x + 8 * $i, $y - 5 * $i); Start-Sleep -Milliseconds 25 }
            $carries++
        } else {
            Start-Sleep -Milliseconds (40 + $rng.Next(120))
        }
        [Her]::Up()
        $presses++
        # Out of her and back in while she reacts: the poller flips mid-animation.
        [void][Her]::SetCursorPos($r[0] - 20, $r[1] - 20)
        Start-Sleep -Milliseconds (10 + $rng.Next(30))
        [void][Her]::SetCursorPos($x, $y)
    }
    $ms = [Her]::Ping($body, 1000)
    $steps++
    if ($ms -lt 0 -or [Her]::IsHungAppWindow($body)) {
        $hung = "step ${steps}: no answer to WM_NULL within 1 s (IsHungAppWindow: $([Her]::IsHungAppWindow($body)))"
        break
    }
    if ($ms -gt $worst) { $worst = $ms }
}
[void][Her]::SetCursorPos(5, 5)
Write-Host "$steps steps, $presses presses ($carries carries), slowest WM_NULL $worst ms"

# ---- what's left ---------------------------------------------------------------------------------
$fails = [System.Collections.Generic.List[string]]::new()
if ($hung) { $fails.Add("she stopped answering: $hung") }
if ($p.HasExited) { $fails.Add("she exited (code $($p.ExitCode))") }
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
$wer = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1000, 1002; StartTime = $t0 } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match 'synaps-avatar' })
foreach ($e in $wer) { $fails.Add("WER event $($e.Id) at $($e.TimeCreated): $(($e.Message -split "`n")[0])") }
$text = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
if ($text -match 'panicked') { $fails.Add('a panic in her log') }
foreach ($l in @($text -split "`r?`n" | Where-Object { $_ -match 'STALL' } | Select-Object -First 5)) { Write-Host "    her log: $l" }
if ($fails.Count -gt 0 -or $text -match 'STALL') { Get-Content $log -Tail 60 | ForEach-Object { Write-Host "    | $_" } }
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue

if ($ExpectHang) {
    if (-not $hung) { throw "no hang caught with $($With -join ' '): this test doesn't bite" }
    Write-Host "ok: the hang is caught ($hung)"
    exit 0
}
if ($presses -lt 20) { $fails.Add("only $presses presses landed on her: the run proves little") }
if ($fails.Count) { $fails | ForEach-Object { Write-Host "FAIL $_" }; exit 1 }
Write-Host 'ok: she answered every step'
