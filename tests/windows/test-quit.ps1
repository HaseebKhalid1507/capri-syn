# Her quit is bounded, and her exe is free after it. On Haseeb's box (2026-11-19, his desk locked)
# a `quit` on her control socket said "bye" and she stayed; a kill then left her process half-gone
# (HasExited, 1 thread, 707 handles) with her exe still mapped, and her installer couldn't replace
# it ("DeleteFile failed; code 5"). Here, offline as test-hang (no daemon):
#   - she starts and answers `state`; `quit` says bye; she's gone within 5 s (her bound is 2 s),
#     exit code 0;
#   - her exe can then be opened for writing: nothing maps it;
#   - stalled: her teardown held 10 s (SA_TEST_QUIT_STALL_MS, tests only): her bound ends her, her
#     log says "still here 2000 ms after her bye", gone within 5 s, exit code 0, her exe free;
#   - -Locked: again with the session locked (LockWorkStation; it stays locked: the job's last step).
# A round she outlives says how far she got (threads, handles, a kill, the rename the installer
# would do), so the next look starts from facts.
#
#   tests\windows\test-quit.ps1 [-Exe PATH] [-Pack DIR] [-Locked] [-Out DIR]
#
# -Exe: default target\release\synaps-avatar.exe (copied: the copy is the one checked). -Pack:
# default %TEMP%\sa-hang-pack (test-hang builds it). -Out: her logs; default %TEMP%\sa-quit-<pid>.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'CI log.')]
param([string]$Exe, [string]$Pack, [switch]$Locked, [string]$Out)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Exe) { $Exe = Join-Path $root 'target\release\synaps-avatar.exe' }
if (-not $Pack) { $Pack = Join-Path ([IO.Path]::GetTempPath()) 'sa-hang-pack' }
if (-not (Test-Path (Join-Path $Pack 'sprites.json'))) { throw "no pack in $Pack (test-hang.ps1 builds it)" }
if (-not $Out) { $Out = Join-Path ([IO.Path]::GetTempPath()) "sa-quit-$PID" }
New-Item -ItemType Directory -Force (Join-Path $Out 'bin') | Out-Null
$her = Join-Path $Out 'bin\synaps-avatar.exe'
if (Get-Process synaps-avatar -ErrorAction SilentlyContinue) { throw 'another synaps-avatar runs: not a clean machine' }
Copy-Item -Force (Resolve-Path $Exe).Path $her
$sock = Join-Path ([IO.Path]::GetTempPath()) 'synaps-avatar.sock'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
if (-not ('QuitDesk' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class QuitDesk {
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool LockWorkStation();
    // The input desktop out of reach: the secure desktop (locked, or a UAC prompt) has it.
    public static bool Locked() {
        IntPtr h = OpenInputDesktop(0, false, 0x0001);
        if (h == IntPtr.Zero) return true;
        CloseDesktop(h);
        return false;
    }
}
'@
}

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

# Her exe opened for writing, nothing shared: fails while anything maps it (a process of hers, gone
# or not).
function Test-Free([string]$Path) {
    try { [IO.File]::Open($Path, 'Open', 'ReadWrite', 'None').Dispose(); return $true } catch { return $false }
}

function Round([string]$Name, [switch]$Lock, [int]$StallMs = 0) {
    Write-Output "--- $Name"
    $log = Join-Path $Out "$Name.log"
    # Her teardown held (tests only): her bound has to end her.
    if ($StallMs) { $env:SA_TEST_QUIT_STALL_MS = "$StallMs" }
    try { $p = Start-Process -FilePath $her -ArgumentList @('--pack', "`"$Pack`"", '--socket', "\\.\pipe\sa-quit-nobody-$PID") -PassThru -RedirectStandardError $log -RedirectStandardOutput (Join-Path $Out "$Name.out") }
    finally { Remove-Item Env:\SA_TEST_QUIT_STALL_MS -ErrorAction SilentlyContinue }
    $null = $p.Handle
    $t0 = Get-Date
    $st = $null
    while (((Get-Date) - $t0).TotalSeconds -lt 60 -and -not $p.HasExited) {
        $st = Send-Her 'state' 2000
        if ($st -and $st.StartsWith('{')) { break }
        Start-Sleep -Milliseconds 500
    }
    Check ($st -and $st.StartsWith('{')) "$($Name): she runs and answers state (pid $($p.Id))"
    if (-not $st) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; return }
    Start-Sleep -Seconds 3
    if ($Lock) {
        $ok = [QuitDesk]::LockWorkStation()
        Start-Sleep -Seconds 3
        Check $ok "$($Name): LockWorkStation"
        Check ([QuitDesk]::Locked()) "$($Name): the input desktop is out of reach (locked)"
        $st = Send-Her 'state' 3000
        Write-Output "  locked, she answers state: $([bool]$st)"
    }
    $bye = Send-Her 'quit'
    $tq = Get-Date
    Check ($bye -eq 'bye') "$($Name): quit says bye ('$bye')"
    $gone = $p.WaitForExit(5000)
    $took = [int]((Get-Date) - $tq).TotalMilliseconds
    Check $gone "$($Name): she's gone within 5 s of her bye ($took ms)"
    if ($gone) { Check ($p.ExitCode -eq 0) "$($Name): exit code 0 ($($p.ExitCode))" }
    if (-not $gone) {
        $q = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        if ($q) { Write-Output "  still here: $($q.Threads.Count) thread(s), $($q.HandleCount) handles" }
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        $killed = $p.WaitForExit(10000)
        $q = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        Write-Output "  killed: exited $killed$(if ($q) { "; listed still: $($q.Threads.Count) thread(s), $($q.HandleCount) handles" })"
    }
    $tf = Get-Date
    while (-not (Test-Free $her) -and ((Get-Date) - $tf).TotalSeconds -lt 5) { Start-Sleep -Milliseconds 200 }
    $free = Test-Free $her
    Check $free "$($Name): her exe can be replaced after ($([int]((Get-Date) - $tf).TotalMilliseconds) ms)"
    if (-not $free) {
        # What her installer does then: the mapped exe renamed aside (Windows allows it).
        $aside = "$her.old-test"
        try { Move-Item -LiteralPath $her -Destination $aside; Write-Output "  renamed aside: ok"; Copy-Item -LiteralPath $aside $her } catch { Write-Output "  renamed aside: $($_.Exception.Message)" }
    }
    $said = if (Test-Path $log) { Get-Content $log } else { @() }
    $wd = $said | Where-Object { $_ -match 'quit: ' }
    Write-Output "  her log: $(if ($wd) { $wd -join ' / ' } else { 'no watchdog line (her teardown ended in time)' })"
    if ($StallMs) {
        Check (@($wd | Where-Object { $_ -match "quit: holding $StallMs ms" }).Count -eq 1) "$($Name): her teardown held (SA_TEST_QUIT_STALL_MS)"
        Check (@($wd | Where-Object { $_ -match 'quit: still here 2000 ms after her bye: ending now \(exit 0\)' }).Count -eq 1) "$($Name): her bound ended her: 'still here 2000 ms after her bye'"
        Check ($took -lt $StallMs) "$($Name): before her held teardown would have ended ($took ms < $StallMs ms)"
    }
}

Round 'unlocked'
Round 'stalled' -StallMs 10000
if ($Locked) { Round 'locked' -Lock }

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
Write-Output 'her quit is bounded and frees her exe'
