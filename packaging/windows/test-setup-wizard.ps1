# Her installer's wizard, driven as a person would click it (UI Automation), with a screenshot of every
# page (tools/shoot-win.ps1): her model ChatGPT (its sign-in, then her model, started by Finish;
# stopped: her model stays unset), then
# "Start her now" and her on the desktop; Setup again with "Choose later" (no sign-in), and again with
# a model on this PC (an Ollama stand-in); then the uninstall's questions (and "Remove Synaps too?":
# yes). CI's windows-setup-wizard job, on a clean runner, uploads the shots; it runs as is in the
# capri-win VM. Windows PowerShell 5.1 (UIAutomationClient).
#
#   powershell -File .\packaging\windows\test-setup-wizard.ps1 -Setup capri-syn-setup-....exe -Shots DIR
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
param([Parameter(Mandatory)] [string]$Setup, [Parameter(Mandatory)] [string]$Shots)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Wiz {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); }
    // Setup's VCL controls (TNewRadioButton, TNewComboBox) are Win32 buttons and combo boxes under
    // class names UI Automation doesn't know (a pane each, no patterns): read and set as Win32 ones.
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, System.Text.StringBuilder l);
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
    // BM_GETCHECK: BST_CHECKED.
    public static bool Checked(IntPtr h) { return SendMessage(h, 0x00F0, IntPtr.Zero, IntPtr.Zero).ToInt64() == 1; }
    // CB_GETCOUNT, CB_GETLBTEXTLEN, CB_GETLBTEXT: a combo box's items.
    public static string[] Items(IntPtr h) {
        int n = (int)SendMessage(h, 0x0146, IntPtr.Zero, IntPtr.Zero);
        var r = new string[Math.Max(n, 0)];
        for (int i = 0; i < n; i++) {
            int len = (int)SendMessage(h, 0x0149, (IntPtr)i, IntPtr.Zero);
            var sb = new System.Text.StringBuilder(Math.Max(len, 0) + 1);
            SendMessage(h, 0x0148, (IntPtr)i, sb);
            r[i] = sb.ToString();
        }
        return r;
    }
    // CB_SETCURSEL, then CBN_SELCHANGE to its parent (WM_COMMAND), as a pick of his sends it.
    public static int Pick(IntPtr h, int i) {
        SendMessage(h, 0x014E, (IntPtr)i, IntPtr.Zero);
        SendMessage(GetParent(h), 0x0111, (IntPtr)((1 << 16) | (GetDlgCtrlID(h) & 0xFFFF)), h);
        return (int)SendMessage(h, 0x0147, IntPtr.Zero, IntPtr.Zero);
    }
}
'@
[void][Wiz]::SetProcessDPIAware()
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Setup = (Resolve-Path $Setup).Path
New-Item -ItemType Directory -Force $Shots | Out-Null
$UIA = [Windows.Automation.AutomationElement]
$Tree = [Windows.Automation.TreeScope]
$app = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'
$synaps = Join-Path $(if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }) 'synaps.exe'
$n = 0

# Shots: spike's tools/shoot-win.ps1 (#82), dot-sourced: a window (by title glob or process),
# cropped (+ Pad), numbered in the order taken.
. (Join-Path $root 'tools\shoot-win.ps1')
# The wizard's window as a person uses it (caption, answering, enabled, dragging): test-wizard-window.ps1.
. (Join-Path $PSScriptRoot 'test-wizard-window.ps1')
function Get-Shot([string]$Name, [string]$Title = '', [int]$ProcessId = 0, [int]$Pad = 12) {
    $script:n++
    $f = Join-Path $Shots ('{0:D2}-{1}.png' -f $script:n, $Name)
    try { $r = Save-WindowShot -Path $f -Title $Title -ProcessId $ProcessId -Pad $Pad; Write-Host "  shot $(Split-Path -Leaf $f): $($r -join ',')" }
    catch { Write-Host "  (no shot $($Name): $($_.Exception.Message))" }
}
# The top-level windows whose title matches.
function Find-Windows([string]$Title) {
    @($UIA::RootElement.FindAll($Tree::Children, [Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.Name -match $Title })
}
function Find-Window([string]$Title) { Find-Windows $Title | Select-Object -First 1 }
function Wait-Window([string]$Title, [int]$Secs = 60) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Secs) { $w = Find-Window $Title; if ($w) { return $w }; Start-Sleep -Milliseconds 300 }
    throw "no window '$Title' in $Secs s"
}
# Every name shown in a window (its page's caption among them).
# Trimmed: a heading's name can end in a line break (the finish page's does).
function Get-WindowText($W) {
    # A window that closes under the read (the uninstall's questions do) says nothing: not an error.
    try { @($W.FindAll($Tree::Descendants, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object { "$($_.Current.Name)".Trim() } | Where-Object { $_ }) }
    catch { @() }
}
# A button by its caption, whatever UI Automation calls it (a VCL button can come through as a pane):
# Invoke when it has that, else a real click on its middle.
function Invoke-Button($W, [string]$Name) {
    $b = @($W.FindAll($Tree::Descendants, [Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.Name -match "^&?$Name( >)?$" -and $_.Current.IsEnabled -and $_.Current.BoundingRectangle.Width -gt 0 } |
            Sort-Object { $_.Current.IsOffscreen }) | Select-Object -First 1
    if (-not $b) { throw "no button '$Name' (it shows: $((Get-WindowText $W) -join ' | '))" }
    $o = $null
    if ($b.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$o)) { $o.Invoke(); return }
    $r = $b.Current.BoundingRectangle
    Write-Host "  ('$Name' is a $($b.Current.ControlType.ProgrammaticName) $($b.Current.ClassName) with no Invoke: clicked at its middle)"
    try { [void][Wiz]::SetForegroundWindow([IntPtr]$W.Current.NativeWindowHandle) } catch { $null = $_ }
    [Wiz]::Click([int]($r.Left + $r.Width / 2), [int]($r.Top + $r.Height / 2))
}
# Waits until the wizard shows a page whose caption matches; its texts.
function Wait-Page($W, [string]$Caption, [int]$Secs = 120) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Secs) {
        $t = Get-WindowText $W
        # The caption changes first; the page's own controls are in the tree a moment later.
        if ($t -match $Caption) { Start-Sleep -Milliseconds 400; return Get-WindowText $W }
        Start-Sleep -Milliseconds 300
    }
    throw "the wizard never showed '$Caption' (it shows: $((Get-WindowText $W) -join ' | '))"
}

# The control called Name (its caption), on screen.
function Find-Named($W, [string]$Name) {
    @($W.FindAll($Tree::Descendants, [Windows.Automation.Condition]::TrueCondition) | Where-Object {
            "$($_.Current.Name)".Trim() -eq $Name -and $_.Current.BoundingRectangle.Width -gt 0 }) | Select-Object -First 1
}
# A radio button picked: its check (BM_GETCHECK), else UI Automation's word for it.
function Test-Picked($E) {
    if (-not $E) { return $false }
    $h = [IntPtr]$E.Current.NativeWindowHandle
    if ($h -ne [IntPtr]::Zero) { return [Wiz]::Checked($h) }
    $o = $null
    if ($E.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$o)) { return $o.Current.IsSelected }
    return $false
}
# A radio button picked as he'd pick it: a click on its caption (its OnClick runs).
function Select-Radio($W, [string]$Name) {
    $e = Find-Named $W $Name
    if (-not $e) { throw "no '$Name' (it shows: $((Get-WindowText $W) -join ' | '))" }
    $r = $e.Current.BoundingRectangle
    try { [void][Wiz]::SetForegroundWindow([IntPtr]$W.Current.NativeWindowHandle) } catch { $null = $_ }
    [Wiz]::Click([int]($r.Left + 40), [int]($r.Top + $r.Height / 2))
    Start-Sleep -Milliseconds 400
    return (Test-Picked $e)
}
# In the page's one enabled list (a drop-down, TNewComboBox): the item whose words are like Like,
# picked. Its words.
function Select-Item($W, [string]$Like) {
    $seen = @()
    $lists = @($W.FindAll($Tree::Descendants, [Windows.Automation.Condition]::TrueCondition) | Where-Object {
            $_.Current.ClassName -match 'ComboBox' -and $_.Current.IsEnabled -and $_.Current.BoundingRectangle.Width -gt 0 })
    foreach ($c in $lists) {
        $h = [IntPtr]$c.Current.NativeWindowHandle
        $items = @([Wiz]::Items($h))
        $seen += $items
        for ($i = 0; $i -lt $items.Count; $i++) {
            if ($items[$i] -like $Like) {
                $at = [Wiz]::Pick($h, $i)
                Start-Sleep -Milliseconds 300
                if ($at -ne $i) { throw "picked '$($items[$i])' ($i), but the list says $at" }
                return $items[$i]
            }
        }
    }
    throw "no item like '$Like' in an enabled list ($($lists.Count) of them; they show: $($seen -join ' | '))"
}
# Waits until the wizard no longer shows a page whose caption matches (after a click on Next).
function Wait-Leave($W, [string]$Caption, [int]$Secs = 60) {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Secs) { if (-not ((Get-WindowText $W) -match $Caption)) { return }; Start-Sleep -Milliseconds 300 }
    throw "the wizard stayed on '$Caption' (it shows: $((Get-WindowText $W) -join ' | '))"
}
# Her settings' [session] model ('' when there's none).
function Get-HerModel {
    $f = Join-Path $env:APPDATA 'synaps-avatar\settings.toml'
    if (-not (Test-Path $f)) { return '' }
    $m = [regex]::Match((Get-Content -Raw $f), '(?ms)^\[session\][^\[]*?^model\s*=\s*"([^"]*)"')
    if ($m.Success) { return $m.Groups[1].Value } else { return '' }
}
# Synaps login processes, and the steps that run them.
function Get-SignIn([string]$Provider) { Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match "synaps(\.exe)?`"? login --provider $Provider" } | Select-Object -First 1 }
function Close-SignIn { Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'steps\.ps1" login|synaps(\.exe)?"? login' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } }

# The wizard, as he gets it: double-clicked, every default left as it is but her model (ChatGPT).
$log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-wizard-$PID.log"
# The shots' desktop quiet (the runner's console minimized, a flat backdrop), before the wizard.
try { Clear-ShotDesktop } catch { Write-Host "  (desktop stays as it is: $($_.Exception.Message))" }
$p = Start-Process $Setup -ArgumentList "/LOG=`"$log`"" -PassThru
# Its handle now, while it runs: else its ExitCode is $null once it's gone.
$null = $p.Handle
# Run elevated (CI's runners are), Setup asks first, No its default: he says Yes here. Not elevated
# (a double-click on his PC), it doesn't ask.
$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$askedAdmin = $false
$w = $null
$t0 = Get-Date
while (-not $w -and ((Get-Date) - $t0).TotalSeconds -lt 60) {
    foreach ($d in Find-Windows '^Setup$|^capri-syn') {
        $texts = Get-WindowText $d
        if (-not $askedAdmin -and ($texts -match 'Setup is running as administrator')) {
            $askedAdmin = $true
            Get-Shot 'asked-as-administrator' -ProcessId $d.Current.ProcessId
            Check (($texts -match 'Go on as administrator anyway').Count -gt 0) 'run as administrator, Setup asks first'
            Invoke-Button $d 'Yes'
        }
    }
    $w = Find-Window '^Setup - capri-syn'
    if (-not $w) { Start-Sleep -Milliseconds 300 }
}
if (-not $w) { throw "no window '^Setup - capri-syn' in 60 s" }
Check ($askedAdmin -eq $elevated) "it asks when run as administrator, and only then (elevated: $elevated, asked: $askedAdmin)"
foreach ($page in @(
        @('^Select Destination Location$', 'destination', 'Next'),
        @('^Select Additional Tasks$', 'tasks', 'Next'),
        @('^Her brain$', 'her-brain', 'Next'),
        @('^Her model$', 'her-model', 'Next'),
        @('^Her pack$', 'her-pack', 'Next'),
        @('^Her voice$', 'her-voice', 'Next'),
        @('^Ready to Install$', 'ready', 'Install'))) {
    $t = Wait-Page $w $page[0]
    if ($page[1] -eq 'her-model') {
        # Four choices at the same level, "Choose later" the one picked; no model server here.
        Check ((Test-Picked (Find-Named $w 'Choose later')) -and -not (Test-Picked (Find-Named $w 'Sign in to an account')) -and
            -not (Test-Picked (Find-Named $w 'An API key'))) 'her model page: "Choose later" is the default, nothing else picked'
        Check (($t -match '^A model on this PC$').Count -gt 0 -and ($t -match 'None running here').Count -gt 0) 'no model server here: it says so'
        Get-Shot 'her-model-later' -Title 'Setup - capri-syn*'
        # An account: ChatGPT, signed in to at the end (short of the real sign-in).
        Check (Select-Radio $w 'Sign in to an account') 'a click picks "Sign in to an account"'
        $picked = Select-Item $w 'ChatGPT*'
        Check ($picked -eq 'ChatGPT (your browser)') "ChatGPT, how it signs in: $picked"
    }
    Get-Shot $page[1] -Title 'Setup - capri-syn*'
    if ($page[1] -eq 'her-brain') { Check (($t -match 'Install Synaps \(her brain, open source\)').Count -gt 0) 'her brain page offers to install Synaps' }
    if ($page[1] -eq 'her-voice') { Check (($t -match '^Talk to her$').Count -gt 0 -and ($t -match 'Read replies aloud').Count -gt 0) 'her voice page: talk, microphone, "hey Capri", replies aloud' }
    if ($page[1] -eq 'her-pack') { Check (($t -match 'Download her art').Count -gt 0) 'her pack page offers the download' }
    if ($page[1] -in 'destination', 'ready') { Test-WizardWindow $w "the $($page[1]) page" }
    Invoke-Button $w $page[2]
    Wait-Leave $w $page[0]
}
# The steps, on their progress page (Synaps, her art, the daemon), then the finish page.
$null = Wait-Page $w '^Setting her up$' 180
Start-Sleep -Seconds 3
Get-Shot 'setting-her-up' -Title 'Setup - capri-syn*'
Test-WizardDuringStep $w
$t = Wait-Page $w '^Completing the capri-syn Setup Wizard$' 1200
Get-Shot 'finish' -Title 'Setup - capri-syn*'
Write-StepTime
Check (($t -match 'Synaps \d+\.\d+\.\d+, her brain: installed').Count -gt 0) 'the finish page: Synaps installed'
Check (($t -match 'Her art: downloaded, built and checked').Count -gt 0) 'the finish page: her art'
Check (($t -match 'Its daemon runs').Count -gt 0) 'the finish page: the daemon'
# An account: her model is set once he's signed in (the finish page's step), never before.
$m1 = Get-HerModel
Check ($m1 -eq '') "her model: not set at install, before the sign-in ($m1)"
Check (($t -match '(?m)^\+ Her model:').Count -eq 0) 'the finish page names no model of hers'
Check (($t -match 'Next, her brain: leave "Sign in to ChatGPT" ticked, and sign in in your browser\. Her model is ChatGPT''s once you''re signed in\.').Count -gt 0) 'the finish page: ChatGPT''s sign-in, ticked; her model once signed in'
# Finish: "Start her now" and "Sign in to ChatGPT", ticked. The sign-in starts with no terminal:
# steps.ps1 login model, then synaps login, then his browser (CI can't sign in: stopped after).
Invoke-Button $w 'Finish'
[void]$p.WaitForExit(60000)
Check ($p.ExitCode -eq 0) "the wizard exits 0 ($($p.ExitCode))"
$signIn = $null
$t0 = Get-Date
while (-not $signIn -and ((Get-Date) - $t0).TotalSeconds -lt 30) {
    $signIn = Get-SignIn 'openai-codex'
    if (-not $signIn) { Start-Sleep -Milliseconds 500 }
}
Check ([bool]$signIn) "Finish started ChatGPT's sign-in, no terminal: $(if ($signIn) { $signIn.CommandLine })"
$step = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'steps\.ps1" login model -Provider openai-codex -Exe "[^"]+\\synaps-avatar\.exe"' } | Select-Object -First 1
Check ([bool]$step) "by steps.ps1 login model (her model once signed in): $(if ($step) { $step.CommandLine })"
Start-Sleep -Seconds 4
$browser = @(Get-Process -Name msedge, chrome, firefox -ErrorAction SilentlyContinue)
Write-Host "  a browser runs: $(if ($browser) { ($browser | Select-Object -ExpandProperty Name -Unique) -join ', ' } else { 'none seen' })"
Close-SignIn
Start-Sleep -Seconds 2
Check ((Get-HerModel) -eq '') "the sign-in stopped: her model still unset ($(Get-HerModel))"

# Her, started by the finish page.
$her = $null
$t0 = Get-Date
while (-not $her -and ((Get-Date) - $t0).TotalSeconds -lt 30) { $her = Get-Process synaps-avatar -ErrorAction SilentlyContinue | Select-Object -First 1; Start-Sleep -Milliseconds 300 }
Check ([bool]$her) '"Start her now" started her'
Start-Sleep -Seconds 12
if ($her) { Get-Shot 'her-on-the-desktop' -ProcessId $her.Id -Pad 240; Get-Shot 'her' -ProcessId $her.Id -Pad 40 }
$herLog = Join-Path $env:LOCALAPPDATA 'synaps-avatar\synaps-avatar.log'
if (Test-Path $herLog) { Write-Output "--- her log (end) ---`n$((Get-Content $herLog -Tail 12) -join "`n")`n---" }

# Setup again over her (still no brain: the sign-in above was stopped), pack and voice skipped: every
# page as it comes (no destination, no "Her brain": Synaps is here), "Her model" as OnModel picks, up
# to the finish page. Its texts; the wizard in $script:w, its process in $script:p. She's quit first,
# as he'd quit her (else Setup's "Preparing to Install" asks to close her: Next there closes her).
function Invoke-Again([string]$Name, [scriptblock]$OnModel) {
    Get-Process synaps-avatar -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    $t0 = Get-Date
    while ((Get-Process synaps-avatar -ErrorAction SilentlyContinue) -and ((Get-Date) - $t0).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 300 }
    $log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-wizard-$Name-$PID.log"
    $script:p = Start-Process $Setup -ArgumentList "/LOG=`"$log`" /PACK=skip /VOICE=off" -PassThru
    $null = $script:p.Handle
    $script:w = $null
    $t0 = Get-Date
    while (-not $script:w -and ((Get-Date) - $t0).TotalSeconds -lt 60) {
        foreach ($d in Find-Windows '^Setup$|^capri-syn') { if ((Get-WindowText $d) -match 'Setup is running as administrator') { Invoke-Button $d 'Yes' } }
        $script:w = Find-Window '^Setup - capri-syn'
        if (-not $script:w) { Start-Sleep -Milliseconds 300 }
    }
    if (-not $script:w) { throw "$($Name): no window '^Setup - capri-syn' in 60 s" }
    $model = $false
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 900) {
        $t = Get-WindowText $script:w
        if ($t -match '^Completing the capri-syn Setup Wizard$') { Start-Sleep -Milliseconds 400; return Get-WindowText $script:w }
        # The progress page names its steps (RunPick's is "Her model"): no page to answer while it's up.
        if (@($t -ceq 'Setting her up').Count -gt 0) { Start-Sleep -Milliseconds 500; continue }
        $page = @($t -match '^(Select Destination Location|Select Additional Tasks|Her brain|Her model|Her pack|Her voice|Ready to Install|Preparing to Install)$') | Select-Object -First 1
        if ($page) {
            Start-Sleep -Milliseconds 400
            if ($page -eq 'Her model') { $model = $true; & $OnModel $script:w; Get-Shot "$Name-her-model" -Title 'Setup - capri-syn*' }
            Invoke-Button $script:w $(if ($page -eq 'Ready to Install') { 'Install' } else { 'Next' })
            Wait-Leave $script:w "^$page$"
        } else { Start-Sleep -Milliseconds 500 }
        if ($page -eq 'Ready to Install' -and -not $model) { throw "$($Name): no ""Her model"" page (no brain yet: it should ask)" }
    }
    throw "$($Name): no finish page in 900 s (it shows: $((Get-WindowText $script:w) -join ' | '))"
}

# Later: "Choose later", as it starts. No sign-in, her model as it was, and the finish page says so.
$t = Invoke-Again 'later' { param($W) Check (Test-Picked (Find-Named $W 'Choose later')) 'again: "Choose later" picked' }
Get-Shot 'later-finish' -Title 'Setup - capri-syn*'
Check (($t -match 'Her brain: later, as you chose\. She shows NO BRAIN YET until then; her Settings > Brain gives her one\.').Count -gt 0) 'later: the finish page says so'
Check (($t -match 'Sign in to').Count -eq 0) 'later: no sign-in on the finish page'
Invoke-Button $script:w 'Finish'
[void]$script:p.WaitForExit(60000)
Start-Sleep -Seconds 8
Check (-not (Get-SignIn '\S+')) 'later: no sign-in started'
Check ((Get-HerModel) -eq $m1) "later: her model as it was ($(Get-HerModel))"
Close-SignIn

# A model on this PC: an Ollama stand-in on its port (its model list, as Ollama's /v1/models gives it).
Add-Type -TypeDefinition @'
using System;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
public static class FakeOllama {
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
[FakeOllama]::Start(11434, '{"object":"list","data":[{"id":"qwen3:8b","object":"model","owned_by":"library"},{"id":"nomic-embed-text:latest","object":"model"}]}')
$t = Invoke-Again 'local' {
    param($W)
    Check (Select-Radio $W 'A model on this PC') 'local: a click picks "A model on this PC"'
    $picked = Select-Item $W 'qwen3:8b*'
    Check ($picked -eq 'qwen3:8b (Ollama)') "local: Ollama's model, no embedding one: $picked"
}
Get-Shot 'local-finish' -Title 'Setup - capri-syn*'
Check (($t -match '\+ Her model: qwen3:8b, on Ollama\.').Count -gt 0) 'local: the finish page says her model'
Check ((Get-HerModel) -eq 'local/qwen3:8b') "local: her model local/qwen3:8b ($(Get-HerModel))"
$cfg = Join-Path $env:USERPROFILE '.synaps-cli\config'
Check (-not ((Test-Path $cfg) -and (Select-String -LiteralPath $cfg -Pattern 'provider\.local\.url' -Quiet))) "local: Synaps' config untouched (Ollama's is its default)"
Check (($t -match 'Sign in to').Count -eq 0) 'local: no sign-in on the finish page'
Invoke-Button $script:w 'Finish'
[void]$script:p.WaitForExit(60000)
Check ($script:p.ExitCode -eq 0) "local: the wizard exits 0 ($($script:p.ExitCode))"
Start-Sleep -Seconds 4
Close-SignIn

# The uninstall, as he'd click it: sure? yes; Synaps too? yes (her installer installed it); done.
$bin = Split-Path -Parent $synaps
$hadHidden = Test-Path (Join-Path $bin 'hidden.exe')
Check $hadHidden "Synaps' installer put hidden.exe beside synaps.exe ($bin)"
$u = Start-Process (Join-Path $app 'unins000.exe') -PassThru
$d = Wait-Window '^capri-syn Uninstall$' 60
Get-Shot 'uninstall-sure' -Title 'capri-syn Uninstall'
Invoke-Button $d 'Yes'
$t0 = Get-Date
$asked = $false
$done = $false
while (-not $done -and ((Get-Date) - $t0).TotalSeconds -lt 180) {
    # Its progress window and its questions share the title: the one with the words.
    foreach ($d in Find-Windows '^capri-syn Uninstall$') {
        $texts = Get-WindowText $d
        if (-not $asked -and ($texts -match 'Remove Synaps too')) { $asked = $true; Get-Shot 'uninstall-synaps-too' -ProcessId $d.Current.ProcessId; Invoke-Button $d 'Yes' }
        elseif ($texts -match 'successfully removed') { Get-Shot 'uninstall-done' -ProcessId $d.Current.ProcessId; Invoke-Button $d 'OK'; $done = $true }
    }
    Start-Sleep -Milliseconds 400
}
Check $asked 'the uninstall asked to remove Synaps too'
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path (Join-Path $app 'unins000.exe')) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
Check (-not (Test-Path $app)) "$app is gone"
Check (-not (Test-Path (Join-Path $env:LOCALAPPDATA 'synaps-avatar'))) 'her data folder is gone'
Check (-not (Test-Path $synaps)) "Synaps is gone, as he said ($synaps)"
Check (-not (Test-Path (Join-Path $bin 'hidden.exe'))) 'and hidden.exe, its other binary (its installer''s receipt lists both)'
$rh = if ($env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME 'synaps' } else { Join-Path $env:LOCALAPPDATA 'synaps' }
Check (-not (Test-Path (Join-Path $rh 'synaps-receipt.json'))) "and its installer's receipt ($rh)"
$userPath = @((Get-Item -LiteralPath 'registry::HKEY_CURRENT_USER\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' |
        Where-Object { $_ } | ForEach-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') })
if (Test-Path $bin) { Check (@(Get-ChildItem -Force $bin).Count -gt 0) "$bin kept, with other programs' files in it: $((Get-ChildItem -Force $bin | Select-Object -First 5 | ForEach-Object { $_.Name }) -join ', ')" }
else { Check ($userPath -notcontains $bin.TrimEnd('\')) "$bin is gone, and off his user Path" }
$null = $u

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her wizard, clicked through: every page shot'
