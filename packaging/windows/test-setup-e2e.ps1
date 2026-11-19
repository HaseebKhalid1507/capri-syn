# Her installer end to end, as someone with nothing installed gets it: capri-syn-setup.exe unattended
# with its defaults, on a clean Windows (CI's windows-setup-e2e job, a fresh runner; it runs as is in
# the capri-win VM too). Then:
#   - Synaps: `synaps --version` works (its installer's ~\.cargo\bin), and it's on his user Path;
#   - the daemon: up, protocol 3, and at sign-in (Run\synaps-daemon through conhost --headless);
#   - her pack: downloaded, sprites.json, her --check passes; autostart and the Start-menu entry;
#   - she starts (as the finish page's "Start her now" does) and attaches: her control socket's `state`
#     has session.live, and her tag isn't OFFLINE (NO BRAIN YET is fine: CI can't sign in);
#   - she stays responsive for 60 s of clicks on her: every `state` is answered by her UI thread (a
#     hung one answers a bare "ok" after 3 s), her windows answer messages, her ticks go on, and Windows
#     logs no Application Hang for her;
#   - the uninstall (unattended) leaves nothing of hers: files, folders, keys, Run values, shortcut, her
#     socket. Synaps stays (unattended, it's kept; the wizard asks).
# -Shots DIR: her on the desktop (tools/shoot-win.ps1). Needs PowerShell 7 (AF_UNIX on Windows).
#
#   pwsh .\packaging\windows\test-setup-e2e.ps1 -Setup capri-syn-setup-....exe [-Shots DIR]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
param([Parameter(Mandatory)] [string]$Setup, [string]$Shots = '', [int]$Responsive = 60)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Setup = (Resolve-Path $Setup).Path
$app = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'
$exe = Join-Path $app 'synaps-avatar.exe'
$data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
$conf = Join-Path $env:APPDATA 'synaps-avatar'
$pack = Join-Path $data 'packs\capri'
$herLog = Join-Path $data 'synaps-avatar.log'
$sock = Join-Path ([IO.Path]::GetTempPath()) 'synaps-avatar.sock'
$cargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
$synaps = Join-Path $cargoBin 'synaps.exe'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar.lnk'
$inno = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1'
$aumid = 'HKCU:\Software\Classes\AppUserModelId\Synaps.Avatar'
function Get-RunValue([string]$Name) { (Get-ItemProperty -Path $runKey -Name $Name -ErrorAction SilentlyContinue).$Name }
if (Test-Path $app) { throw "$app exists already: not a clean machine" }
if (Test-Path $synaps) { throw "$synaps exists already: not a clean machine" }
if ($Shots) { New-Item -ItemType Directory -Force $Shots | Out-Null }

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class E2e {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public static List<IntPtr> Windows(int pid) {
        var l = new List<IntPtr>();
        EnumWindows((h, p) => { uint w; GetWindowThreadProcessId(h, out w); if (w == pid && IsWindowVisible(h)) l.Add(h); return true; }, IntPtr.Zero);
        return l;
    }
    // WM_NULL with SMTO_ABORTIFHUNG: false when the window's thread doesn't answer in ms.
    public static bool Answers(IntPtr h, uint ms) { IntPtr r; return SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 0x0002, ms, out r) != IntPtr.Zero; }
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); }
}
'@
[void][E2e]::SetProcessDPIAware()

# Her control socket: one command, its reply. $null when nothing answers.
function Send-Her([string]$Command, [int]$TimeoutMs = 5000) {
    $s = [Net.Sockets.Socket]::new([Net.Sockets.AddressFamily]::Unix, [Net.Sockets.SocketType]::Stream, [Net.Sockets.ProtocolType]::Unspecified)
    try {
        $s.ReceiveTimeout = $TimeoutMs; $s.SendTimeout = $TimeoutMs
        $s.Connect([Net.Sockets.UnixDomainSocketEndPoint]::new($sock))
        [void]$s.Send([Text.Encoding]::UTF8.GetBytes("$Command`n"))
        $buf = New-Object byte[] 65536
        $ms = [IO.MemoryStream]::new()
        do { $n = $s.Receive($buf); if ($n -gt 0) { $ms.Write($buf, 0, $n) } } while ($n -gt 0 -and $buf[$n - 1] -ne 10)
        return [Text.Encoding]::UTF8.GetString($ms.ToArray()).Trim()
    } catch { return $null } finally { $s.Dispose() }
}
# Her state, from her UI thread: $null when it didn't answer (a bare "ok" after 3 s) or nothing did.
function Get-HerState {
    $t = Send-Her 'state'
    if ($t -and $t.StartsWith('{')) { return ($t | ConvertFrom-Json) }
    return $null
}
# Shots: spike's tools/shoot-win.ps1 (#82), dot-sourced. A process's windows, cropped (+ Pad).
if ($Shots) {
    . (Join-Path $root 'tools\shoot-win.ps1')
    # The desktop quiet (the runner's console minimized, a flat backdrop), long before she's up:
    # MinimizeAll is the shell's, and late, and would take her with it.
    try { Clear-ShotDesktop } catch { Write-Host "  (desktop stays as it is: $($_.Exception.Message))" }
}
function Get-Shot([string]$Name, [int]$ProcessId, [int]$Pad = 12) {
    if (-not $Shots) { return }
    try { $r = Save-WindowShot -Path (Join-Path $Shots "$Name.png") -ProcessId $ProcessId -Pad $Pad; Write-Host "  shot $Name.png: $($r -join ',')" }
    catch { Write-Host "  (no shot $($Name): $($_.Exception.Message))" }
}

# 1. Her installer, unattended, its defaults (Synaps, her art, the daemon, autostart; no voice).
$log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-e2e-$PID.log"
$t0 = Get-Date
$p = Start-Process $Setup -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$log`"") -PassThru
$null = $p.Handle
if (-not $p.WaitForExit(20 * 60 * 1000)) { Stop-Process -Id $p.Id -Force; throw 'her installer still runs after 20 minutes' }
$setupLog = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
($setupLog -split "`r?`n" | Where-Object { $_ -match '(step |synaps|pack|daemon|finished|voice|brain): ' }) | ForEach-Object { Write-Host "  setup: $_" }
Check ($p.ExitCode -eq 0) "her installer, unattended with its defaults, exits 0 ($($p.ExitCode), $([int]((Get-Date) - $t0).TotalSeconds) s)"
Check ($setupLog -notmatch '(?m)^.*: failed: ') "no step failed$(if ($setupLog -match '(?m)^.*: failed: .*$') { ": $($Matches[0])" })"
Check (Test-Path $exe) "her exe in $app"

# 2. Synaps, and its daemon now and at sign-in.
$v = if (Test-Path $synaps) { (& $synaps --version) | Select-Object -First 1 } else { '' }
Check ($v -match '^synaps \d+\.\d+\.\d+') "synaps --version: $v"
$userPath = (Get-Item -LiteralPath 'registry::HKEY_CURRENT_USER\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames')
Check (@($userPath -split ';') -contains $cargoBin) "$cargoBin is on his user Path (Synaps' installer)"
$status = if (Test-Path $synaps) { (& $synaps daemon status 2>&1 | Out-String) } else { '' }
Check ($status -match 'daemon: running') "the Synaps daemon runs: $(($status -split "`n")[0])"
Check ($status -match 'protocol: 3') 'protocol 3 (her client)'
Check ((Get-RunValue 'synaps-daemon') -match '^"[^"]*\\conhost\.exe" --headless "[^"]*\\synaps\.exe" daemon --detach$') "and at sign-in, no console window: $(Get-RunValue 'synaps-daemon')"

# 3. Her pack, autostart, the Start menu.
Check (Test-Path (Join-Path $pack 'sprites.json')) "her pack in $pack"
Check (Test-Path (Join-Path $pack 'anim\lachy\LICENSE.md')) "with lachy's art and its licence"
Check ($setupLog -match 'Required Notice: ') "lachy's Required Notice lines were said"
$before = if (Test-Path $herLog) { (Get-Item $herLog).Length } else { 0 }
$c = Start-Process $exe -ArgumentList "--pack `"$pack`" --check" -Wait -PassThru -WindowStyle Hidden
$text = if (Test-Path $herLog) { (Get-Content -Raw $herLog).Substring([int]$before) } else { '' }
Check ($c.ExitCode -eq 0 -and $text -match 'pack `Capri` ok: \d+ frames') "her --check: $([regex]::Match($text, 'pack `Capri` ok: \d+ frames[^,]*').Value) (exit $($c.ExitCode))"
Check ((Get-RunValue 'synaps-avatar') -eq "`"$exe`"") 'she starts at sign-in (Run\synaps-avatar, the default)'
Check (Test-Path $lnk) 'her Start-menu entry'
# Her voice: on by default only with a microphone and somewhere to get it from (a CI runner has no
# microphone, and there's no release yet): off, the log says why, and no voice files.
Write-Host "  microphones: $([regex]::Match($setupLog, 'mics: mics: (\d+)').Groups[1].Value); $([regex]::Match($setupLog, 'voicefrom: [^\r\n]*').Value)"
$off = [regex]::Match($setupLog, 'voice: off \(.*?\)').Value
Check ([bool]$off) "her voice is off by default here, and the log says why: $off"
Check (-not (Test-Path (Join-Path $app 'voice')) -and -not (Test-Path (Join-Path $env:LOCALAPPDATA 'capri-voice'))) 'and no voice files'

# 4. She starts, from his profile folder (the finish page's "Start her now"), and attaches.
$from = (Get-Item $herLog -ErrorAction SilentlyContinue).Length
$t0 = Get-Date
$her = Start-Process $exe -WorkingDirectory $env:USERPROFILE -PassThru
$st = $null; $painted = $false
while (((Get-Date) - $t0).TotalSeconds -lt 60) {
    if ($her.HasExited) { break }
    if (-not $painted -and (Test-Path $herLog) -and ((Get-Content -Raw $herLog).Substring([int]$from) -match 'first-frame: painted (\d+) ms')) { $painted = $true; Write-Host "  her first frame: $($Matches[1]) ms" }
    $st = Get-HerState
    if ($st -and $st.session -and $st.session.live) { break }
    Start-Sleep -Milliseconds 500
}
Check (-not $her.HasExited) "she runs (pid $($her.Id))"
# Attached first (a fast daemon): her first frame's line may come after.
while (-not $painted -and ((Get-Date) - $t0).TotalSeconds -lt 60 -and -not $her.HasExited) {
    if ((Get-Content -Raw $herLog).Substring([int]$from) -match 'first-frame: painted (\d+) ms') { $painted = $true; Write-Host "  her first frame: $($Matches[1]) ms" }
    else { Start-Sleep -Milliseconds 500 }
}
Check $painted 'her first frame is painted'
Check ($st -and $st.session -and $st.session.live) "she attaches to the daemon (session.live) in $([int]((Get-Date) - $t0).TotalSeconds) s"
Check ($st -and "$($st.status)" -notmatch 'OFFLINE') "her tag isn't OFFLINE: '$($st.status)' (session: '$($st.session.label)')"
Get-Shot 'her-on-the-desktop' $her.Id 240
Get-Shot 'her' $her.Id 40

# 5. Responsive for 60 s, clicked on.
$t0 = Get-Date
$miss = 0; $hung = 0; $clicks = 0; $round = 0; $ticks0 = if ($st) { [long]$st.counters.ticks } else { 0 }; $last = $st
while (((Get-Date) - $t0).TotalSeconds -lt $Responsive -and -not $her.HasExited) {
    $s = Get-HerState
    if ($s) { $last = $s } else { $miss++; Write-Host "  $([int]((Get-Date) - $t0).TotalSeconds) s: her UI thread didn't answer state" }
    foreach ($h in [E2e]::Windows($her.Id)) { if (-not [E2e]::Answers($h, 2000)) { $hung++; Write-Host "  $([int]((Get-Date) - $t0).TotalSeconds) s: window $h doesn't answer" } }
    $round++
    if ($s -and $s.avatar -and ($round % 5 -eq 0)) {
        [E2e]::Click([int]($s.avatar.x + $s.avatar.w / 2), [int]($s.avatar.y + $s.avatar.h / 2))
        $clicks++
        if ($clicks -eq 2) { Get-Shot 'her-clicked' $her.Id 120 }
    }
    Start-Sleep -Milliseconds 1000
}
Check (-not $her.HasExited) "she still runs after $Responsive s ($clicks clicks on her)"
Check ($miss -eq 0) "her UI thread answered every state ($miss missed)"
Check ($hung -eq 0) "her windows answered every message ($hung didn't)"
Check ($last -and [long]$last.counters.ticks -gt $ticks0) "her ticks go on ($ticks0 -> $(if ($last) { $last.counters.ticks }))"
$wer = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Application Hang'; StartTime = $t0.AddSeconds(-120) } -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'synaps-avatar' })
Check ($wer.Count -eq 0) "no Application Hang for her in the event log$(if ($wer) { ': ' + ($wer[0].Message -split "`n")[0] })"
$tail = if (Test-Path $herLog) { (Get-Content $herLog -Tail 15) -join "`n" } else { '' }
Write-Output "--- her log (end) ---`n$tail`n---"

# 6. The uninstall, unattended: nothing of hers is left; Synaps stays.
$u = Start-Process (Join-Path $app 'unins000.exe') -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -PassThru
$null = $u.Handle
[void]$u.WaitForExit(180000)
# The uninstaller re-runs itself from %TEMP%: wait for it to finish.
$deadline = (Get-Date).AddSeconds(120)
while ((Test-Path $inno) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
Check ($u.ExitCode -eq 0) "uninstalls unattended (exit $($u.ExitCode))"
Check (-not (Get-Process synaps-avatar -ErrorAction SilentlyContinue)) 'she isn''t running'
Check (-not (Test-Path $app)) "$app is gone$(if (Test-Path $app) { ': ' + ((Get-ChildItem -Recurse -Force $app | Select-Object -First 5 | ForEach-Object { $_.FullName }) -join ', ') })"
Check (-not (Test-Path $data)) "$data is gone$(if (Test-Path $data) { ': ' + ((Get-ChildItem -Recurse -Force $data | Select-Object -First 5 | ForEach-Object { $_.FullName }) -join ', ') })"
Check (-not (Test-Path $conf)) "$conf is gone$(if (Test-Path $conf) { ': ' + ((Get-ChildItem -Recurse -Force $conf | ForEach-Object { $_.Name }) -join ', ') })"
Check (-not (Test-Path $sock)) 'her control socket is gone'
Check (-not (Test-Path $inno)) 'her Apps entry is gone'
Check (-not (Test-Path $aumid)) 'her AUMID key is gone'
Check (-not (Test-Path $lnk)) 'her Start-menu entry is gone'
Check ($null -eq (Get-RunValue 'synaps-avatar')) 'Run\synaps-avatar is gone'
Check ($null -eq (Get-RunValue 'synaps-daemon')) 'Run\synaps-daemon is gone'
Check (Test-Path $synaps) 'Synaps stays (unattended: kept)'

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her installer works end to end'
