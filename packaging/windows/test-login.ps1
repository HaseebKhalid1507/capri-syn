# steps.ps1 login against a stand-in synaps.exe (compiled here: what Synaps 0.12's `login --provider
# claude` does on Windows). Its `cmd /c start URL` cuts the URL at the first &, so the step opens the
# URL from its stderr itself. Checked: the whole URL, with every & (CAPRI_NO_BROWSER: said, not
# opened); stdin held open until auth.json changes (the stand-in, as 0.12, fails with "Manual input
# channel closed" otherwise), then closed; `brain: yes`. And a login that fails says why.
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
        if (Environment.GetEnvironmentVariable("FAKE_LOGIN_FAIL") == "1") { Console.Error.WriteLine("Error: token exchange failed (400)"); return 1; }
        // As 0.12: stdin read from the start (the paste); its end ends the login.
        var reader = new Thread(() => { Console.In.ReadToEnd(); closed = true; });
        reader.IsBackground = true;
        reader.Start();
        var e = Console.Error;
        e.WriteLine("\n\u001b[1mOpening browser to sign in...\u001b[0m\n");
        e.WriteLine("\u001b[2mIf the browser didn't open, visit this URL:\u001b[0m");
        e.WriteLine("\u001b[36m$url\u001b[0m\n");
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

$env:FAKE_LOGIN_FAIL = '1'
$out = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps login -Synaps (Join-Path $bin 'synaps.exe') 2>&1 | Out-String
Write-Host ($out.Trim() -split "`n" | ForEach-Object { "  login: $_" } | Out-String)
Check ($LASTEXITCODE -ne 0) "a failed login fails the step ($LASTEXITCODE)"
Check ($out -match 'failed: synaps login exited 1: Error: token exchange failed \(400\)') 'and says why, in its words'
Remove-Item Env:\FAKE_LOGIN_FAIL, Env:\CAPRI_NO_BROWSER, Env:\SYNAPS_BASE_DIR
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her sign-in step works'
