# steps.ps1 login against a stand-in synaps.exe (compiled here: what Synaps 0.12's `login --provider
# claude` does on Windows). Its `cmd /c start URL` cuts the URL at the first &, so the step opens the
# URL from its stderr itself. Checked: the whole URL, with every & (CAPRI_NO_BROWSER: said, not
# opened); stdin held open until auth.json changes (the stand-in, as 0.12, fails with "Manual input
# channel closed" otherwise), then closed; `brain: yes`. A URL that isn't Claude's or Anthropic's is
# never opened. And a login that fails says why. The same stand-in's daemon, whose `daemon status`
# is 0.12's (another protocol than the synaps asking: "running (not answering)", exit 1): on
# protocol 4, steps.ps1 daemon fails and says she can't attach (her installer's finish page then
# never says it runs), whether that synaps answers it or not; one that doesn't say its protocol
# fails too; right after Synaps was installed or upgraded (-Fresh), one on another protocol is "an
# older one still running", with how to stop it; and one of hers that's hung is not running.
#
#   powershell -File .\packaging\windows\test-login.ps1
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
param()
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tmp = Join-Path ([IO.Path]::GetTempPath()) "capri-login-test-$PID"
$bin = Join-Path $tmp 'bin'
$base = Join-Path $tmp 'synaps-cli'
New-Item -ItemType Directory -Force $bin, $base | Out-Null
$url = 'https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&state=s7'
Add-Type -OutputType ConsoleApplication -OutputAssembly (Join-Path $bin 'synaps.exe') -TypeDefinition @"
using System;
using System.IO;
using System.Threading;
public static class Program {
    static volatile bool closed;
    public static int Main(string[] a) {
        if (a.Length > 0 && a[0] == "--version") { Console.WriteLine("synaps 0.12.0"); return 0; }
        if (a.Length > 1 && a[0] == "daemon" && a[1] == "status") {
            // As 0.12's: "running" only when the daemon answers this synaps (the same protocol as
            // FAKE_CLIENT); else "running (not answering)", exit 1. The protocol from daemon.json.
            var p = Environment.GetEnvironmentVariable("FAKE_PROTOCOL");
            var c = Environment.GetEnvironmentVariable("FAKE_CLIENT") ?? "4";
            bool ok = (p == "none" || string.IsNullOrEmpty(p) || p == c) && Environment.GetEnvironmentVariable("FAKE_HUNG") != "1";
            Console.WriteLine(ok ? "daemon: running" : "daemon: running (not answering)");
            if (p != "none") Console.WriteLine("pid: 4242  version: 0.0.0  protocol: " + (string.IsNullOrEmpty(p) ? "4" : p) + "  started: 0");
            return ok ? 0 : 1;
        }
        if (a.Length > 0 && a[0] == "daemon") { return 0; }
        if (Environment.GetEnvironmentVariable("FAKE_LOGIN_FAIL") == "1") { Console.Error.WriteLine("Error: token exchange failed (400)"); return 1; }
        // As 0.12: stdin read from the start (the paste); its end ends the login.
        var reader = new Thread(() => { Console.In.ReadToEnd(); closed = true; });
        reader.IsBackground = true;
        reader.Start();
        var e = Console.Error;
        e.WriteLine("\n\u001b[1mOpening browser to sign in...\u001b[0m\n");
        e.WriteLine("\u001b[2mIf the browser didn't open, visit this URL:\u001b[0m");
        var u = Environment.GetEnvironmentVariable("FAKE_LOGIN_URL");
        e.WriteLine("\u001b[36m" + (string.IsNullOrEmpty(u) ? "$url" : u) + "\u001b[0m\n");
        e.WriteLine("\u001b[2mOr paste the redirect URL here (must include ``state``):\u001b[0m");
        e.Flush();
        Thread.Sleep(1500);
        if (closed) { e.WriteLine("Error: Manual input channel closed"); return 1; }
        File.WriteAllText(Path.Combine(Environment.GetEnvironmentVariable("SYNAPS_BASE_DIR"), "auth.json"), "{\"anthropic\": {\"type\": \"oauth\"}}");
        reader.Join(20000);
        return 0;
    }
}
"@
$env:SYNAPS_BASE_DIR = $base
$env:CAPRI_NO_BROWSER = '1'
$steps = Join-Path $root 'packaging\windows\steps.ps1'
$ps = (Get-Process -Id $PID).Path

$out = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps login -Synaps (Join-Path $bin 'synaps.exe') 2>&1 | Out-String
Write-Host ($out.Trim() -split "`n" | ForEach-Object { "  login: $_" } | Out-String)
Check ($LASTEXITCODE -eq 0) "steps.ps1 login exits 0 ($LASTEXITCODE)"
Check ($out.Contains("would open: $url")) 'it opens the whole URL, every & in it (not cmd''s cut one)'
Check ($out -match 'brain: yes') 'brain: yes once auth.json changes'

# A synaps that prints another site's page: not opened.
Remove-Item -Force -LiteralPath (Join-Path $base 'auth.json')
$env:FAKE_LOGIN_URL = 'https://claude.ai.evil.example/oauth/authorize?code=true&client_id=9d1c250a&state=s7'
$out = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps login -Synaps (Join-Path $bin 'synaps.exe') 2>&1 | Out-String
Write-Host ($out.Trim() -split "`n" | ForEach-Object { "  login: $_" } | Out-String)
Check (-not $out.Contains('would open:') -and $out.Contains("not opened (not a Claude or Anthropic sign-in page): $($env:FAKE_LOGIN_URL)")) 'a page on another site is never opened'
Remove-Item Env:\FAKE_LOGIN_URL

$env:FAKE_LOGIN_FAIL = '1'
$out = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps login -Synaps (Join-Path $bin 'synaps.exe') 2>&1 | Out-String
Write-Host ($out.Trim() -split "`n" | ForEach-Object { "  login: $_" } | Out-String)
Check ($LASTEXITCODE -ne 0) "a failed login fails the step ($LASTEXITCODE)"
Check ($out -match 'failed: synaps login exited 1: Error: token exchange failed \(400\)') 'and says why, in its words'
Remove-Item Env:\FAKE_LOGIN_FAIL

# Its daemon on protocol 4 (hers is 3, exact match): the step fails, and says why. Run\synaps-daemon
# as it was before, after.
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$had = (Get-ItemProperty -Path $run -Name 'synaps-daemon' -ErrorAction SilentlyContinue).'synaps-daemon'
function Invoke-Daemon([string[]]$More) {
    try {
        $o = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps daemon -Synaps (Join-Path $bin 'synaps.exe') @More 2>&1 | Out-String
        $script:rc = $LASTEXITCODE
    } finally {
        if ($null -ne $had) { Set-ItemProperty -Path $run -Name 'synaps-daemon' -Value $had } else { Remove-ItemProperty -Path $run -Name 'synaps-daemon' -ErrorAction SilentlyContinue }
    }
    Write-Host ($o.Trim() -split "`n" | ForEach-Object { "  daemon: $_" } | Out-String)
    return $o
}
$out = Invoke-Daemon @()
Check ($rc -ne 0) "a daemon on another protocol fails the daemon step ($rc)"
Check ($out -match '(?m)^daemon: wrong-protocol') 'daemon: wrong-protocol (her installer: "she can''t use it")'
Check ($out -match 'failed: the daemon speaks protocol 4; she speaks 3 \(exact match\), so she can''t attach') 'and says why'
Check ($out -match 'Install Synaps \d+\.\d+\.\d+ \(irm https://github\.com/[^ ]+/releases/download/v\d+\.\d+\.\d+/synaps-installer\.ps1 \| iex\), stop the daemon, "[^"]+synaps\.exe" daemon stop') 'and how to fix it (the pin, then stop the daemon)'
Check ($out -notmatch '(?m)^daemon: running') 'never "daemon: running"'

# One that doesn't say which protocol it speaks: never let through as running.
$env:FAKE_PROTOCOL = 'none'
$out = Invoke-Daemon @()
Check ($rc -ne 0 -and $out -match '(?m)^daemon: wrong-protocol') "a daemon that doesn't say its protocol fails the step too ($rc)"
Check ($out -match "failed: the daemon doesn't say which protocol it speaks \(an older Synaps\?\); she speaks 3, exactly: stop it, `"[^`"]+synaps\.exe`" daemon stop \(--force if it won't\), then run this again") 'and says to stop it'
Check ($out -notmatch '(?m)^daemon: running') 'never "daemon: running (protocol ?)"'

# Right after Synaps was installed or upgraded (-Fresh): another protocol is an older daemon still up.
# This synaps speaks hers (3), so as 0.12 the daemon on 2 doesn't answer it: "running (not
# answering)", exit 1.
$env:FAKE_PROTOCOL = '2'
$env:FAKE_CLIENT = '3'
$out = Invoke-Daemon @('-Fresh')
Check ($rc -ne 0 -and $out -match '(?m)^daemon: wrong-protocol') "after an install, an older daemon fails the step ($rc)"
Check ($out -match "failed: synaps 0\.12\.0 is installed, but the daemon still running is an older one \(protocol 2; hers is 3\): stop it, `"[^`"]+synaps\.exe`" daemon stop \(--force if it won't\), then run this again") 'and says so: stop the old one (not "install Synaps" again)'
Check ($out -notmatch 'Install Synaps') 'no "Install Synaps" right after it was installed'

# Not -Fresh: a daemon on 4 left up, this synaps on 3 (exit 1, "not answering"): she can't attach.
$env:FAKE_PROTOCOL = '4'
$out = Invoke-Daemon @()
Check ($rc -ne 0 -and $out -match '(?m)^daemon: wrong-protocol') "a daemon on 4 that doesn't answer this synaps (3) fails the step ($rc)"
Check ($out -match 'failed: the daemon speaks protocol 4; she speaks 3 \(exact match\)') 'and says why'
Check ($out -notmatch '(?m)^daemon: (running|not-running)') 'never "running", never "not running"'

# Hers (3) but hung: "running (not answering)", exit 1. Not running, not a wrong protocol.
$env:FAKE_PROTOCOL = '3'
$env:FAKE_HUNG = '1'
$out = Invoke-Daemon @()
Check ($rc -eq 0 -and $out -match '(?m)^daemon: not-running') "a daemon of hers that doesn't answer: not running ($rc)"
Check ($out -match "the daemon runs \(protocol 3, hers\) but doesn't answer: stop it") 'and says to stop it and start it again'
Check ($out -notmatch '(?m)^daemon: (running|wrong-protocol)') 'never "running", never a wrong protocol'
Remove-Item Env:\FAKE_PROTOCOL, Env:\FAKE_CLIENT, Env:\FAKE_HUNG
Remove-Item Env:\CAPRI_NO_BROWSER, Env:\SYNAPS_BASE_DIR
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her sign-in step works'
# The failed login above left $LASTEXITCODE at 1: the shell would report it.
exit 0
