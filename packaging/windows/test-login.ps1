# steps.ps1's brain steps against stand-ins (compiled here): a synaps.exe that does what Synaps 0.12's
# `login --provider` does on Windows, her exe's --providers and --set, Ollama's and LM Studio's model
# lists. Synaps' `cmd /c start URL` cuts a sign-in URL at its first &, so the login step opens the URL
# from its stderr itself: checked whole, every & in it, on the provider's own site only
# (CAPRI_NO_BROWSER: written to that file, not opened), and never in the step's output (her
# installer logs it); only the line after Synaps' "If the browser didn't open, visit this URL:" (a bare
# one, another site's, another path's: never). Stdin held open until auth.json changes (the stand-in,
# as 0.12, fails with "Manual input channel closed" otherwise), then closed; `brain: yes`; `login
# model`: her model set once signed in, never when it fails. Copilot's code in a window of its own and
# on the clipboard (gone from it once signed in), never in the output; the window closed stops it and
# synaps. Grok isn't offered on Windows. A key goes on synaps' stdin only, exactly its bytes and a
# line's end (no BOM). brain-check's lists; her model an account's default
# from her own table; a local model (LM Studio's: provider.local.url, the daemon reloaded). A login
# that fails says why. The same stand-in's daemon, whose `daemon status`
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
# As Synaps 0.12: Claude's and ChatGPT's sign-in print the page's whole URL and wait on their
# callback, reading stdin (the paste) from the start, and once signed in wait on that read till stdin
# ends; Copilot's prints its code and waits on the device flow (FAKE_GO: he typed it); an API key's
# reads stdin. Every call's arguments go to FAKE_ARGS; a key's stdin (every byte, in hex, to its
# end), its command line and what it sees of CAPRI_BRAIN_KEY to FAKE_KEY. FAKE_LOGIN_BARE: its page
# on a line of its own, without "If the browser didn't open"; FAKE_LOGIN_GAP: a blank line between.
Add-Type -OutputType ConsoleApplication -OutputAssembly (Join-Path $bin 'synaps.exe') -TypeDefinition @"
using System;
using System.IO;
using System.Threading;
public static class Program {
    static volatile bool closed;
    static string Env(string k) { return Environment.GetEnvironmentVariable(k); }
    public static int Main(string[] a) {
        if (!string.IsNullOrEmpty(Env("FAKE_ARGS"))) File.AppendAllText(Env("FAKE_ARGS"), string.Join(" ", a) + Environment.NewLine);
        if (a.Length > 0 && a[0] == "--version") { Console.WriteLine("synaps 0.12.0"); return 0; }
        if (a.Length > 1 && a[0] == "daemon" && a[1] == "status") {
            // As 0.12's: "running" only when the daemon answers this synaps (the same protocol as
            // FAKE_CLIENT); else "running (not answering)", exit 1. The protocol from daemon.json.
            var p = Env("FAKE_PROTOCOL");
            var c = Env("FAKE_CLIENT") ?? "4";
            bool ok = (p == "none" || string.IsNullOrEmpty(p) || p == c) && Env("FAKE_HUNG") != "1";
            Console.WriteLine(ok ? "daemon: running" : "daemon: running (not answering)");
            if (p != "none") Console.WriteLine("pid: 4242  version: 0.0.0  protocol: " + (string.IsNullOrEmpty(p) ? "4" : p) + "  started: 0");
            return ok ? 0 : 1;
        }
        if (a.Length > 0 && a[0] == "daemon") { return 0; }
        string who = a.Length > 2 && a[1] == "--provider" ? a[2] : "claude";
        var e = Console.Error;
        var auth = Path.Combine(Env("SYNAPS_BASE_DIR"), "auth.json");
        if (Env("FAKE_LOGIN_FAIL") == "1") { e.WriteLine("Error: token exchange failed (400)"); return 1; }
        if (who == "groq") {
            e.WriteLine("Paste your Groq API key.");
            e.Write("provider.groq: ");
            e.Flush();
            var ms = new MemoryStream();
            Console.OpenStandardInput().CopyTo(ms);
            var got = ms.ToArray();
            File.WriteAllText(Env("FAKE_KEY"), BitConverter.ToString(got).Replace("-", "") + "|" + Environment.CommandLine + "|" + (Env("CAPRI_BRAIN_KEY") ?? ""));
            var k = System.Text.Encoding.UTF8.GetString(got).Trim();
            if (string.IsNullOrEmpty(k)) { e.WriteLine("Login failed: API key cannot be empty"); return 1; }
            e.WriteLine("API key saved!");
            File.WriteAllText(auth, "{\"groq\": {}}");
            return 0;
        }
        var reader = new Thread(() => { Console.In.ReadToEnd(); closed = true; });
        reader.IsBackground = true;
        reader.Start();
        if (who == "github-copilot") {
            e.WriteLine("\n\u001b[1mGitHub Copilot device login\u001b[0m");
            e.WriteLine("  Enter code: \u001b[36mWDJB-MJHT\u001b[0m");
            e.WriteLine("  At:         \u001b[36mhttps://github.com/login/device\u001b[0m\n");
            e.Flush();
            var go = Env("FAKE_GO");
            for (int i = 0; i < 1200 && !File.Exists(go); i++) Thread.Sleep(100);
            if (!File.Exists(go)) { e.WriteLine("Login failed: the code expired"); return 1; }
        } else {
            e.WriteLine("\n\u001b[1mOpening browser to sign in...\u001b[0m\n");
            if (Env("FAKE_LOGIN_BARE") != "1") e.WriteLine("\u001b[2mIf the browser didn't open, visit this URL:\u001b[0m");
            if (Env("FAKE_LOGIN_GAP") == "1") e.WriteLine("");
            var u = Env("FAKE_LOGIN_URL");
            e.WriteLine("\u001b[36m" + (string.IsNullOrEmpty(u) ? "$url" : u) + "\u001b[0m\n");
            e.WriteLine("\u001b[2mOr paste the redirect URL here (must include state):\u001b[0m");
            e.Flush();
            Thread.Sleep(1500);
            if (closed) { e.WriteLine("Error: Manual input channel closed"); return 1; }
        }
        File.WriteAllText(auth, "{\"" + who + "\": {\"type\": \"oauth\"}}");
        reader.Join(20000);
        return 0;
    }
}
"@
# Her, as the steps use her: --providers (her brains and their defaults: REAL_HER's, the exe this job
# built, through a pipe as her setup reads it; else a table of its own, in ALL's order) and --set
# (to FAKE_HER); FAKE_HER_OLD: a build before --providers.
Add-Type -OutputType ConsoleApplication -OutputAssembly (Join-Path $bin 'her.exe') -TypeDefinition @'
using System;
using System.IO;
public static class Her {
    public static int Main(string[] a) {
        File.AppendAllText(Environment.GetEnvironmentVariable("FAKE_HER"), string.Join(" ", a) + Environment.NewLine);
        if (Environment.GetEnvironmentVariable("FAKE_HER_OLD") == "1") { Console.Error.WriteLine("synaps-avatar: unknown argument '" + a[0] + "'"); return 1; }
        if (a.Length == 1 && a[0] == "--providers") {
            var real = Environment.GetEnvironmentVariable("REAL_HER");
            if (!string.IsNullOrEmpty(real)) {
                var pi = new System.Diagnostics.ProcessStartInfo(real, "--providers");
                pi.UseShellExecute = false;
                pi.RedirectStandardOutput = true;
                pi.CreateNoWindow = true;
                using (var p = System.Diagnostics.Process.Start(pi)) {
                    var o = p.StandardOutput.ReadToEnd();
                    p.WaitForExit();
                    Console.Write(o);
                    return p.ExitCode;
                }
            }
            Console.WriteLine("openai-codex\topenai-codex\taccount\topenai-codex/gpt-5.6-terra\tChatGPT\tbrowser\tfree account works");
            Console.WriteLine("anthropic\tclaude\taccount\tclaude-sonnet-4-6\tClaude\tbrowser");
            Console.WriteLine("github-copilot\tgithub-copilot\taccount\tgithub-copilot/claude-sonnet-4.6\tCopilot\tcode");
            Console.WriteLine("google-gemini\tgoogle-gemini\taccount\tgoogle-gemini/gemini-2.5-pro\tGemini\tbrowser");
            Console.WriteLine("xai-auth\txai-auth\taccount\txai-auth/grok-4.3\tGrok\tbrowser");
            Console.WriteLine("local\tlocal\tlocal\t\tLocal\tnone");
            Console.WriteLine("groq\tgroq\tkey\tgroq/llama-3.3-70b-versatile\tGroq\tkey");
            return 0;
        }
        if (a.Length == 2 && a[0] == "--set") { Console.WriteLine("settings: written"); return 0; }
        return 2;
    }
}
'@
# Ollama and LM Studio, as far as a look at them goes: their model lists on their own ports.
Add-Type -TypeDefinition @'
using System;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
public static class FakeServer {
    public static void Start(int port, string body) {
        var l = new TcpListener(IPAddress.Loopback, port);
        l.Start();
        var t = new Thread(() => {
            while (true) {
                try {
                    using (var c = l.AcceptTcpClient()) {
                        var s = c.GetStream();
                        s.ReadTimeout = 2000;
                        try { s.Read(new byte[8192], 0, 8192); } catch { }
                        var b = Encoding.UTF8.GetBytes(body);
                        var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: " + b.Length + "\r\nConnection: close\r\n\r\n");
                        s.Write(h, 0, h.Length);
                        s.Write(b, 0, b.Length);
                    }
                } catch { return; }
            }
        });
        t.IsBackground = true;
        t.Start();
    }
}
'@
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$env:SYNAPS_BASE_DIR = $base
$opened = Join-Path $tmp 'opened.txt'
$env:CAPRI_NO_BROWSER = $opened
$env:FAKE_ARGS = Join-Path $tmp 'args.txt'
$env:FAKE_HER = Join-Path $tmp 'her.txt'
$env:FAKE_KEY = Join-Path $tmp 'key.txt'
$env:FAKE_GO = Join-Path $tmp 'go'
$steps = Join-Path $root 'packaging\windows\steps.ps1'
$ps = (Get-Process -Id $PID).Path
$synaps = Join-Path $bin 'synaps.exe'
$her = Join-Path $bin 'her.exe'
$realHer = Join-Path $root 'target\release\synaps-avatar.exe'
if (Test-Path -LiteralPath $realHer) { $env:REAL_HER = $realHer; Write-Host "her table: $realHer --providers" }
else { Write-Host 'her table: the stand-in''s (no build of her here)' }
function Invoke-Step([string[]]$StepArgs) {
    $o = & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $steps @StepArgs -Synaps $synaps -Exe $her 2>&1 | Out-String
    $script:rc = $LASTEXITCODE
    Write-Host ($o.Trim() -split "`n" | ForEach-Object { "  $($StepArgs[0]): $_" } | Out-String)
    return $o
}
function Clear-Fake { Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $base 'auth.json'), (Join-Path $base 'config'), $opened, $env:FAKE_ARGS, $env:FAKE_HER, $env:FAKE_KEY, $env:FAKE_GO }
function Find-Top([string]$Name, [int]$Secs) {
    $t0 = Get-Date
    $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
    while (((Get-Date) - $t0).TotalSeconds -lt $Secs) {
        $w = [Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children, $cond)
        if ($w) { return $w }
        Start-Sleep -Milliseconds 300
    }
    return $null
}

# Claude (no -Provider: her installer's sign-in before there was a choice): the whole URL, every &.
$out = Invoke-Step @('login')
Check ($rc -eq 0) "steps.ps1 login exits 0 ($rc)"
Check ((Test-Path $opened) -and (Get-Content $opened) -eq $url) 'it opens the whole URL, every & in it (not cmd''s cut one)'
Check (-not $out.Contains('claude.ai')) 'and never says it (the step''s output goes to her installer''s log)'
Check ($out -match 'brain: yes') 'brain: yes once auth.json changes'
Check ((Get-Content $env:FAKE_ARGS) -contains 'login --provider claude') 'synaps login --provider claude'

# ChatGPT: its own page, on its own site; Claude's page given for it is not opened.
Clear-Fake
$env:FAKE_LOGIN_URL = 'https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_X&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&state=s9'
$out = Invoke-Step @('login', '-Provider', 'openai-codex')
Check ($rc -eq 0 -and (Get-Content $env:FAKE_ARGS) -contains 'login --provider openai-codex') "-Provider openai-codex: synaps login --provider openai-codex ($rc)"
Check ((Get-Content $opened) -eq $env:FAKE_LOGIN_URL -and -not $out.Contains('openai.com')) 'its whole page opened, never said'
Clear-Fake
$env:FAKE_LOGIN_URL = $url
$out = Invoke-Step @('login', '-Provider', 'openai-codex')
Check (-not (Test-Path $opened) -and $out.Contains("not opened: the page Synaps gave isn't ChatGPT's sign-in page") -and -not $out.Contains('claude.ai')) 'a page on another site than its own: not opened, not said'

# A synaps that prints another site's page, or one with a \ in it: not opened, not said.
foreach ($bad in @('https://claude.ai.evil.example/oauth/authorize?code=true&client_id=9d1c250a&state=s7', 'https://evil.example\.claude.ai/oauth/authorize?a=1&b=2')) {
    Clear-Fake
    $env:FAKE_LOGIN_URL = $bad
    $out = Invoke-Step @('login')
    Check (-not (Test-Path $opened) -and -not $out.Contains('evil.example') -and $out.Contains("not opened: the page Synaps gave isn't Claude's sign-in page")) "a page that isn't Claude's is never opened ($($bad.Substring(0, 30))...)"
}
Remove-Item Env:\FAKE_LOGIN_URL
# Its own page, but on a line of its own (not after "If the browser didn't open, visit this URL:"):
# never taken for the page.
Clear-Fake
$env:FAKE_LOGIN_BARE = '1'
$out = Invoke-Step @('login')
Remove-Item Env:\FAKE_LOGIN_BARE
Check ($rc -eq 0 -and -not (Test-Path $opened) -and -not $out.Contains('claude.ai')) "a bare line with a URL is never opened ($rc)"
# Not the very next line: never.
Clear-Fake
$env:FAKE_LOGIN_GAP = '1'
$out = Invoke-Step @('login')
Remove-Item Env:\FAKE_LOGIN_GAP
Check ($rc -eq 0 -and -not (Test-Path $opened) -and $out.Contains("not opened: the page Synaps gave isn't Claude's sign-in page")) "the page not on the line right after: never opened ($rc)"

# login model: her model set once he's signed in (her own table's default for it), never before.
Clear-Fake
$env:FAKE_LOGIN_URL = 'https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_X&state=s9'
$out = Invoke-Step @('login', 'model', '-Provider', 'openai-codex')
Remove-Item Env:\FAKE_LOGIN_URL
$her1 = @(Get-Content $env:FAKE_HER -ErrorAction SilentlyContinue)
Check ($rc -eq 0 -and $out -match 'brain: yes' -and ($her1 | Where-Object { $_ -like '--set session.model=openai-codex/*' })) "login model: signed in, then her model ChatGPT's default ($rc; $($her1 -join ', '))"

Clear-Fake
$env:FAKE_LOGIN_FAIL = '1'
$out = Invoke-Step @('login', 'model', '-Provider', 'claude')
Check ($rc -ne 0) "a failed login fails the step ($rc)"
Check ($out -match 'failed: synaps login exited 1: Error: token exchange failed \(400\)') 'and says why, in its words'
Check (-not (@(Get-Content $env:FAKE_HER -ErrorAction SilentlyContinue) -match '^--set')) 'and her model is never set (login model)'
Remove-Item Env:\FAKE_LOGIN_FAIL

# Grok: Synaps 0.12 never prints its page, and cmd cuts it: not offered on Windows.
Clear-Fake
$out = Invoke-Step @('login', '-Provider', 'xai-auth')
Check ($rc -ne 0 -and $out -match "failed: Synaps 0\.12 can't open xAI's sign-in page whole on Windows yet" -and -not (Test-Path $env:FAKE_ARGS)) "Grok: not here, said, synaps never run ($rc)"

# Copilot: its code in a window of its own (and on the clipboard), never in the step's output. Started
# hidden, as her installer's finish page starts it (login model). The test types it (FAKE_GO).
function Open-Copilot([string]$Out) {
    $c = Start-Process $ps -PassThru -WindowStyle Hidden -RedirectStandardOutput $Out -RedirectStandardError "$Out.err" -ArgumentList @(
        '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-File', "`"$steps`"",
        'login', 'model', '-Provider', 'github-copilot', '-Synaps', "`"$synaps`"", '-Exe', "`"$her`"")
    # Its handle now, while it runs: else its ExitCode is $null once it's gone.
    $null = $c.Handle
    return $c
}
function Get-SynapsProcess { @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $synaps }) }
Clear-Fake
Set-Clipboard -Value 'before'
$o = Join-Path $tmp 'copilot.txt'
$p = Open-Copilot $o
$w = Find-Top 'Sign in to Copilot' 60
Check ([bool]$w) 'Copilot: a window, "Sign in to Copilot"'
if ($w) {
    $edit = $w.FindFirst([Windows.Automation.TreeScope]::Descendants, (New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Edit)))
    $shown = if ($edit) { $edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value } else { '' }
    Check ($shown -eq 'WDJB-MJHT') "its code shown in it ($shown)"
    Check (-not $w.Current.IsOffscreen) 'on screen (started hidden, shown anyway)'
}
Check ((Get-Clipboard) -eq 'WDJB-MJHT') "and on the clipboard ($(Get-Clipboard))"
New-Item -ItemType File -Force $env:FAKE_GO | Out-Null
$exited = $p.WaitForExit(60000)
Check ($exited -and $p.ExitCode -eq 0) "signed in: the step exits 0 ($(if ($exited) { $p.ExitCode } else { 'still running after 60 s' }))"
$said = (Get-Content -Raw $o) + (Get-Content -Raw "$o.err")
Write-Host ($said.Trim() -split "`n" | ForEach-Object { "  copilot: $_" } | Out-String)
Check ($said -match 'brain: yes' -and -not $said.Contains('WDJB') -and -not $said.Contains('github.com')) 'brain: yes; the code and the page never in its output'
Check ((@(Get-Content $env:FAKE_HER -ErrorAction SilentlyContinue) -match '^--set session\.model=github-copilot/').Count -eq 1) 'then her model, Copilot''s default'
Check (-not (Find-Top 'Sign in to Copilot' 2)) 'its window gone once signed in'
Check ((Get-Clipboard) -ne 'WDJB-MJHT') "and the code gone from the clipboard ($(Get-Clipboard))"
# Closed by him: the sign-in stops (synaps too), and says so; her model never set. What he copied
# since stays on his clipboard.
Clear-Fake
$p = Open-Copilot $o
$w = Find-Top 'Sign in to Copilot' 60
Check ([bool]$w) 'Copilot again: its window'
Set-Clipboard -Value 'his own'
$t0 = Get-Date
if ($w) { $w.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close() }
$exited = $p.WaitForExit(30000)
Check ($exited -and $p.ExitCode -ne 0) "its window closed: the step stops ($(if ($exited) { "exit $($p.ExitCode) in $([int]((Get-Date) - $t0).TotalSeconds) s" } else { 'still running after 30 s' }))"
$said = (Get-Content -Raw $o -ErrorAction SilentlyContinue) + (Get-Content -Raw "$o.err" -ErrorAction SilentlyContinue)
Write-Host ("$said".Trim() -split "`n" | ForEach-Object { "  copilot closed: $_" } | Out-String)
Check ($said -match 'failed: the Copilot sign-in was stopped \(its window closed\)') 'and says so'
Check ((Get-SynapsProcess).Count -eq 0) 'synaps login stopped with it'
Check (-not (@(Get-Content $env:FAKE_HER -ErrorAction SilentlyContinue) -match '^--set')) 'her model never set'
Check ((Get-Clipboard) -eq 'his own') "his clipboard as he left it ($(Get-Clipboard))"
if (-not $exited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Get-SynapsProcess | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

# A key: from CAPRI_BRAIN_KEY, on synaps login's stdin; never on its command line, in its
# environment or in the step's output.
Clear-Fake
$env:CAPRI_BRAIN_KEY = 'gsk_not_a_real_key_42'
$out = Invoke-Step @('key', '-Provider', 'groq')
Remove-Item Env:\CAPRI_BRAIN_KEY -ErrorAction SilentlyContinue
$k = (Get-Content -Raw $env:FAKE_KEY) -split '\|'
Check ($rc -eq 0 -and $out -match 'brain: yes') "steps.ps1 key -Provider groq exits 0, brain: yes ($rc)"
# Every byte synaps got: the key's, then a line's end; no BOM (EF BB BF) before them.
$want = -join ([Text.Encoding]::ASCII.GetBytes("gsk_not_a_real_key_42`n") | ForEach-Object { $_.ToString('X2') })
Check ($k[0] -ceq $want) "the key on synaps login's stdin, exactly ($($k[0].Length / 2) bytes came, $($want.Length / 2) went$(if ($k[0].StartsWith('EFBBBF')) { '; a BOM first' }))"
Check (-not $k[1].Contains('gsk_') -and $k[1] -match 'login --provider groq') "never on its command line ($($k[1]))"
Check ($k[2] -eq '') 'nor in its environment'
Check (-not $out.Contains('gsk_')) 'nor in the step''s output'
Clear-Fake
$out = Invoke-Step @('key', '-Provider', 'groq')
Check ($rc -ne 0 -and $out -match 'failed: no Groq key was given' -and -not (Test-Path $env:FAKE_KEY)) "no key: the step fails, synaps never asked ($rc)"
$env:CAPRI_BRAIN_KEY = 'x'
$out = Invoke-Step @('key', '-Provider', 'openai-codex')
Remove-Item Env:\CAPRI_BRAIN_KEY -ErrorAction SilentlyContinue
Check ($rc -ne 0 -and $out -match 'ChatGPT is signed in to, not given a key') 'a key for an account: refused'

# What brain-check finds: no brain; the accounts (Gemini only with its Google client; Grok not here,
# with why), the key services, and the models on Ollama's and LM Studio's ports (no embedding one).
Clear-Fake
[FakeServer]::Start(11434, '{"object":"list","data":[{"id":"qwen3:8b","object":"model"},{"id":"nomic-embed-text:latest","object":"model"}]}')
[FakeServer]::Start(1234, '{"data": [{"id": "qwen/qwen3-8b", "object": "model"}]}')
$out = Invoke-Step @('brain-check')
$lines = @($out -split "`r?`n" | ForEach-Object { $_.Trim() })
Check ($rc -eq 0 -and $lines -contains 'brain: no') "brain-check: no brain yet ($rc)"
# Her table, as her setup gets it: the accounts and key services in its order, how each signs in.
$table = @(& $her --providers | Where-Object { $_ })
$want = @(foreach ($t in $table) {
        $f = $t -split "`t"
        if ($f[2] -notin @('account', 'key') -or $f[0] -eq 'google-gemini') { continue }
        $why = if ($f[0] -eq 'xai-auth') { "Synaps 0.12 can't open xAI's sign-in page whole on Windows yet" } else { '' }
        "brains: $($f[0])|$($f[1])|$($f[4])|$($f[5])|$why|$($f[6])"
    })
$got = @($lines | Where-Object { $_ -like 'brains: *' })
Check (($got -join "`n") -ceq ($want -join "`n")) "its accounts and key services: her table's, in its order ($($got.Count) of $($want.Count); first $($got | Select-Object -First 1))"
Check ($got -contains 'brains: anthropic|claude|Claude|browser||' -and $got -contains 'brains: github-copilot|github-copilot|Copilot|code||') 'Claude in the browser, Copilot with a code'
# A hint (her table's last field) beside ChatGPT alone: a free account works.
$hinted = @($got | Where-Object { ($_ -split '\|')[5] })
Check ($hinted.Count -eq 1 -and $hinted[0] -ceq 'brains: openai-codex|openai-codex|ChatGPT|browser||free account works') "ChatGPT's hint, and no other's: $($hinted -join ', ')"
Check (@($got | Where-Object { $_ -like 'brains: google-gemini|*' }).Count -eq 0) 'no Gemini without its Google client'
Check (@($got | Where-Object { $_ -match "^brains: xai-auth\|xai-auth\|Grok\|browser\|Synaps 0\.12 can't open" }).Count -eq 1) 'Grok, with why it is not here'
$keys = @($got | Where-Object { $_ -like 'brains: *|key||' }).Count
Check ($got -contains 'brains: groq|groq|Groq|key||' -and $keys -eq @($table | Where-Object { ($_ -split "`t")[2] -eq 'key' }).Count) "the key services ($keys)"
if ($env:REAL_HER) {
    # Her real table: ALL's 7 accounts, 19 key services (src/provider.rs; Gemini's needs its client).
    Check (@($table | Where-Object { ($_ -split "`t")[2] -eq 'account' }).Count -eq 7 -and $keys -eq 19) "her own table, through a pipe from her windowed exe: 7 accounts, $keys key services"
}
Check ($lines -contains 'local: Ollama|http://localhost:11434/v1|qwen3:8b' -and $lines -contains 'local: LM Studio|http://localhost:1234/v1|qwen/qwen3-8b') 'the models on Ollama and LM Studio'
Check (-not ($out -match 'embed')) 'no embedding model'
$env:SYNAPS_GOOGLE_GEMINI_CLIENT_ID = 'x'
$out = Invoke-Step @('brain-check')
Remove-Item Env:\SYNAPS_GOOGLE_GEMINI_CLIENT_ID
Check ($out -match '(?m)^brains: google-gemini\|google-gemini\|Gemini\|browser\|\|\s*$') 'Gemini with its Google client'

# Her model: an account's default from her own table, by her settings writer.
Clear-Fake
$codex = (@($table | Where-Object { $_ -like "openai-codex`t*" })[0] -split "`t")[3]
$out = Invoke-Step @('model', '-Provider', 'openai-codex')
Check ($rc -eq 0 -and $out.Contains("model: $codex")) "steps.ps1 model -Provider openai-codex: her model its default, $codex ($rc)"
Check ((Get-Content $env:FAKE_HER) -join '|' -eq "--providers|--set session.model=$codex") 'asked her table, then her settings writer'
Clear-Fake
$env:FAKE_HER_OLD = '1'
$out = Invoke-Step @('model', '-Provider', 'openai-codex')
Remove-Item Env:\FAKE_HER_OLD
Check ($rc -ne 0 -and $out -match "this build of her can't list her brains \(synaps-avatar --providers\)") "a build of her before --providers: the step says so ($rc)"

# A model on this PC: Ollama's (Synaps' default: its config untouched), then LM Studio's (its
# provider.local.url, his other lines kept; the daemon, running, reloaded).
Clear-Fake
$out = Invoke-Step @('local', '-Model', 'qwen3:8b', '-Url', 'http://localhost:11434/v1')
Check ($rc -eq 0 -and (Get-Content $env:FAKE_HER) -eq '--set session.model=local/qwen3:8b' -and -not (Test-Path (Join-Path $base 'config'))) "local qwen3:8b on Ollama: her model local/qwen3:8b, Synaps' config untouched ($rc)"
Clear-Fake
Set-Content -LiteralPath (Join-Path $base 'config') -Value "# his`nmodel = claude-opus-5-5`nprovider.local.url = http://old:9/v1" -NoNewline
$out = Invoke-Step @('local', '-Model', 'qwen/qwen3-8b', '-Url', 'http://localhost:1234/v1')
$cfg = [IO.File]::ReadAllText((Join-Path $base 'config'))
Check ($rc -eq 0 -and $cfg -eq "# his`nmodel = claude-opus-5-5`nprovider.local.url = http://localhost:1234/v1`n") "LM Studio: provider.local.url replaced, his lines kept ($cfg)"
Check ((Get-Content $env:FAKE_HER) -eq '--set session.model=local/qwen/qwen3-8b') 'her model local/qwen/qwen3-8b'
Check ((Get-Content $env:FAKE_ARGS) -contains 'daemon reload') 'the daemon, running, reloaded'
Clear-Fake

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
Remove-Item Env:\CAPRI_NO_BROWSER, Env:\SYNAPS_BASE_DIR, Env:\FAKE_ARGS, Env:\FAKE_HER, Env:\FAKE_KEY, Env:\FAKE_GO
Remove-Item Env:\REAL_HER -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her sign-in step works'
# The failed login above left $LASTEXITCODE at 1: the shell would report it.
exit 0
