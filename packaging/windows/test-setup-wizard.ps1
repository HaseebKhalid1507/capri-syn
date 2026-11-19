# Her installer's wizard, driven as a person would click it (UI Automation), with a screenshot of every
# page (tools/shoot-win.ps1), then "Start her now" and her on the desktop, then the uninstall's
# questions (and "Remove Synaps too?": yes). CI's windows-setup-wizard job, on a clean runner, uploads
# the shots; it runs as is in the capri-win VM. Windows PowerShell 5.1 (UIAutomationClient).
#
# And the window itself, as a person would use it: on a normal page, and again while a step runs
# (Synaps, her art, the daemon: it takes a while), its top-level window must have its caption and
# answer WM_NCHITTEST with HTCAPTION there, be responsive (not hung, WM_NULL answered within 1 s), be
# enabled, and drag: a real mouse drag by its title bar (SendInput) moves it. ("I can't drag the
# installer": Setup disables every window of its own while ExecAndLogOutput waits.)
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
}
'@
# The wizard's window as Windows sees it: style, rect, enabled, hung, answering, hit test, a drag.
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Threading;
public static class WizWin {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);

    public static int[] Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return new int[] { r.Left, r.Top, r.Right, r.Bottom }; }
    public static int Style(IntPtr h) { return GetWindowLong(h, -16); }
    public static bool Enabled(IntPtr h) { return IsWindowEnabled(h); }
    public static bool Hung(IntPtr h) { return IsHungAppWindow(h); }
    // WM_NULL: does it answer within ms?
    public static bool Answers(IntPtr h, uint ms) { IntPtr res; return SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 0, ms, out res) != IntPtr.Zero; }
    // WM_NCHITTEST at a screen point: HTCAPTION is 2; -1: no answer.
    public static int HitTest(IntPtr h, int x, int y) {
        IntPtr res;
        IntPtr lp = new IntPtr(((y & 0xFFFF) << 16) | (x & 0xFFFF));
        if (SendMessageTimeout(h, 0x84, IntPtr.Zero, lp, 0, 1000, out res) == IntPtr.Zero) return -1;
        return (int)res.ToInt64();
    }
    // The top-level window that is under a screen point.
    public static IntPtr RootAt(int x, int y) { POINT p; p.X = x; p.Y = y; IntPtr w = WindowFromPoint(p); return w == IntPtr.Zero ? w : GetAncestor(w, 2); }
    public static void MoveTo(IntPtr h, int x, int y) { SetWindowPos(h, IntPtr.Zero, x, y, 0, 0, 0x0001 | 0x0004 | 0x0010); } // NOSIZE | NOZORDER | NOACTIVATE
    static bool Mouse(int x, int y, uint buttons) {
        int w = GetSystemMetrics(0), h = GetSystemMetrics(1);
        INPUT[] a = new INPUT[1];
        a[0].type = 0; // INPUT_MOUSE
        a[0].mi.dx = (int)((long)x * 65535 / (w - 1));
        a[0].mi.dy = (int)((long)y * 65535 / (h - 1));
        a[0].mi.dwFlags = buttons | 0x8000 | 0x0001; // + ABSOLUTE | MOVE
        return SendInput(1, a, Marshal.SizeOf(typeof(INPUT))) == 1;
    }
    // A real drag: the pointer to (x, y), the left button down, a move of (dx, dy) in ten steps, up.
    public static bool Drag(int x, int y, int dx, int dy) {
        bool ok = Mouse(x, y, 0); Thread.Sleep(200);
        ok &= Mouse(x, y, 0x0002); Thread.Sleep(200);
        for (int i = 1; i <= 10; i++) { ok &= Mouse(x + dx * i / 10, y + dy * i / 10, 0); Thread.Sleep(40); }
        Thread.Sleep(150);
        ok &= Mouse(x + dx, y + dy, 0x0004); Thread.Sleep(400);
        return ok;
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
function Get-WindowText($W) { @($W.FindAll($Tree::Descendants, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object { "$($_.Current.Name)".Trim() } | Where-Object { $_ }) }
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

# The wizard's window, as Windows sees it (never UI Automation's word for it): one look at its caption,
# whether it answers, and, with -Drag, a real mouse drag by its title bar (SendInput: down, move, up;
# then it is put back where it was, for the shots).
function Get-WindowState([IntPtr]$H, [switch]$Drag) {
    $r = [WizWin]::Rect($H)
    $x = [int](($r[0] + $r[2]) / 2)
    # The first point down the title bar that answers WM_NCHITTEST with HTCAPTION (2).
    $capY = $null
    for ($y = $r[1] + 4; $y -lt $r[1] + 80 -and -not $capY; $y += 3) { if ([WizWin]::HitTest($H, $x, $y) -eq 2) { $capY = $y } }
    $o = [ordered]@{
        Caption = (([WizWin]::Style($H) -band 0xC00000) -eq 0xC00000); HitCaption = [bool]$capY
        Enabled = [WizWin]::Enabled($H); Hung = [WizWin]::Hung($H); Answers = [WizWin]::Answers($H, 1000)
        Moved = $null; Why = ''; From = "$($r[0]),$($r[1])"; To = ''
    }
    if ($Drag) {
        if (-not $capY) { $o.Moved = $false; $o.Why = 'no point on the title bar answers HTCAPTION' }
        elseif ([WizWin]::RootAt($x, $capY) -ne $H) { $o.Moved = $false; $o.Why = 'another window is over the title bar' }
        else {
            if (-not [WizWin]::Drag($x, $capY, 60, 40)) { $o.Why = 'SendInput took fewer events than sent' }
            $r2 = [WizWin]::Rect($H)
            $o.To = "$($r2[0]),$($r2[1])"
            $o.Moved = ($r2[0] - $r[0]) -ge 30 -and ($r2[1] - $r[1]) -ge 20
            [WizWin]::MoveTo($H, $r[0], $r[1])
            Start-Sleep -Milliseconds 300
        }
    }
    [pscustomobject]$o
}
function Format-WindowState($S) {
    "caption=$($S.Caption) HTCAPTION=$($S.HitCaption) enabled=$($S.Enabled) hung=$($S.Hung) answers=$($S.Answers)" +
    $(if ($null -ne $S.Moved) { " drag: $($S.From) -> $($S.To) moved=$($S.Moved) $($S.Why)" })
}
# On a page: caption, responsive, enabled, and a drag that moves it.
function Test-WizardWindow([IntPtr]$H, [string]$Where) {
    $st = Get-WindowState $H -Drag
    Write-Host "  $Where`: $(Format-WindowState $st)"
    Check ($st.Caption -and $st.HitCaption) "$Where`: the window has its caption (WS_CAPTION) and its title bar answers WM_NCHITTEST with HTCAPTION"
    Check ((-not $st.Hung) -and $st.Answers) "$Where`: the window is responsive (not hung, WM_NULL answered within 1 s)"
    Check $st.Enabled "$Where`: the window is enabled"
    Check ([bool]$st.Moved) "$Where`: dragging it by its title bar moves it ($($st.From) -> $($st.To))"
}
# The steps Setup ran, from its own log ("step NAME: ARGS" opens one, "step NAME: exit N" closes it).
function Get-StepRuns {
    $runs = [System.Collections.Generic.List[object]]::new()
    $lines = try { @(Get-Content -LiteralPath $log -ErrorAction Stop) } catch { @() }
    foreach ($l in $lines) {
        if ($l -notmatch '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d)\S*\s+step ([\w-]+): (.*)$') { continue }
        $at = [datetime]$Matches[1]; $name = $Matches[2]; $rest = $Matches[3]
        if ($rest -match '^exit (-?\d+)') {
            $code = $Matches[1]
            $open = $runs | Where-Object { $_.Name -eq $name -and -not $_.End } | Select-Object -Last 1
            if ($open) { $open.End = $at; $open.Exit = $code }
        } else { $runs.Add([pscustomobject]@{ Name = $name; Start = $at; End = $null; Exit = $null }) }
    }
    $runs
}
function Get-StepNow { Get-StepRuns | Where-Object { -not $_.End } | Select-Object -Last 1 }

# The wizard, as he gets it: double-clicked, every default left as it is.
$log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-wizard-$PID.log"
# The shots' desktop quiet (the runner's console minimized, a flat backdrop), before the wizard.
try { Clear-ShotDesktop } catch { Write-Host "  (desktop stays as it is: $($_.Exception.Message))" }
$p = Start-Process $Setup -ArgumentList "/LOG=`"$log`"" -PassThru
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
$h = [IntPtr][int]$w.Current.NativeWindowHandle
Write-Host "  the wizard's window: $h, class $($w.Current.ClassName), '$($w.Current.Name)'"
Check ($askedAdmin -eq $elevated) "it asks when run as administrator, and only then (elevated: $elevated, asked: $askedAdmin)"
foreach ($page in @(
        @('^Select Destination Location$', 'destination', 'Next'),
        @('^Select Additional Tasks$', 'tasks', 'Next'),
        @('^Her brain$', 'her-brain', 'Next'),
        @('^Her pack$', 'her-pack', 'Next'),
        @('^Her voice$', 'her-voice', 'Next'),
        @('^Ready to Install$', 'ready', 'Install'))) {
    $t = Wait-Page $w $page[0]
    Get-Shot $page[1] -Title 'Setup - capri-syn*'
    if ($page[1] -eq 'her-brain') { Check (($t -match 'Install Synaps \(her brain, open source\)').Count -gt 0) 'her brain page offers to install Synaps' }
    if ($page[1] -eq 'her-voice') { Check (($t -match '^Talk to her$').Count -gt 0 -and ($t -match 'Read replies aloud').Count -gt 0) 'her voice page: talk, microphone, "hey Capri", replies aloud' }
    if ($page[1] -eq 'her-pack') { Check (($t -match 'Download her art').Count -gt 0) 'her pack page offers the download' }
    # A normal page: it has its caption, answers, and drags.
    if ($page[1] -in 'destination', 'ready') { Test-WizardWindow $h "the $($page[1]) page" }
    Invoke-Button $w $page[2]
}
# The steps, on their progress page (Synaps, her art, the daemon), then the finish page.
$null = Wait-Page $w '^Setting her up$' 180
Start-Sleep -Seconds 3
Get-Shot 'setting-her-up' -Title 'Setup - capri-syn*'
# The steps are running (Setup's log says which): the same look again and again, with a drag in the
# first four, until ten looks were taken inside one step each or the finish page is up.
$during = [System.Collections.Generic.List[object]]::new()
$drags = 0
$t0 = Get-Date
while ($during.Count -lt 10 -and ((Get-Date) - $t0).TotalSeconds -lt 1200) {
    if (@(Get-WindowText $w) -match '^Completing the capri-syn Setup Wizard$') { break }
    $run = Get-StepNow
    if (-not $run) { Start-Sleep -Milliseconds 300; continue }
    $drag = $drags -lt 4
    $st = Get-WindowState $h -Drag:$drag
    if ($drag) { $drags++ }
    $now = Get-StepNow
    # Counted only when the same step was running before and after the look.
    if ($now -and $now.Name -eq $run.Name -and $now.Start -eq $run.Start) {
        Add-Member -InputObject $st -NotePropertyName Step -NotePropertyValue $run.Name
        $during.Add($st)
        Write-Host "  while step '$($run.Name)' runs (look $($during.Count)): $(Format-WindowState $st)"
    }
}
$stepNames = @($during | ForEach-Object { $_.Step } | Select-Object -Unique)
Check ($during.Count -ge 3) "the window was looked at $($during.Count) times while a step ran ($($stepNames -join ', ')): at least 3"
$bad = @($during | Where-Object { -not ($_.Caption -and $_.HitCaption) })
Check ($bad.Count -eq 0) "while a step runs: the window has its caption (WS_CAPTION) and its title bar answers WM_NCHITTEST with HTCAPTION ($($during.Count - $bad.Count) of $($during.Count) looks)"
$bad = @($during | Where-Object { $_.Hung -or -not $_.Answers })
Check ($bad.Count -eq 0) "while a step runs: the window is responsive (not hung, WM_NULL answered within 1 s) ($($during.Count - $bad.Count) of $($during.Count) looks)"
$bad = @($during | Where-Object { -not $_.Enabled })
Check ($bad.Count -eq 0) "while a step runs: the window is enabled ($($during.Count - $bad.Count) of $($during.Count) looks)"
$dragged = @($during | Where-Object { $null -ne $_.Moved })
$bad = @($dragged | Where-Object { -not $_.Moved })
Check ($dragged.Count -gt 0 -and $bad.Count -eq 0) "while a step runs: dragging it by its title bar moves it ($($dragged.Count - $bad.Count) of $($dragged.Count) drags did)"
$t = Wait-Page $w '^Completing the capri-syn Setup Wizard$' 1200
Get-Shot 'finish' -Title 'Setup - capri-syn*'
foreach ($r in Get-StepRuns) { Write-Host ("  step {0}: {1} s (exit {2})" -f $r.Name, $(if ($r.End) { [int]($r.End - $r.Start).TotalSeconds } else { '?' }), $r.Exit) }
Check (($t -match 'Synaps \d+\.\d+\.\d+, her brain: installed').Count -gt 0) 'the finish page: Synaps installed'
Check (($t -match 'Her art: downloaded, built and checked').Count -gt 0) 'the finish page: her art'
Check (($t -match 'Its daemon runs').Count -gt 0) 'the finish page: the daemon'
# Its defaults: "Start her now" and "Sign in to Claude" ticked. The sign-in starts with no
# terminal: steps.ps1 login, then synaps login, then his browser (CI can't sign in: stopped after).
Invoke-Button $w 'Finish'
[void]$p.WaitForExit(60000)
Check ($p.ExitCode -eq 0) "the wizard exits 0 ($($p.ExitCode))"
$signIn = $null
$t0 = Get-Date
while (-not $signIn -and ((Get-Date) - $t0).TotalSeconds -lt 30) {
    $signIn = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'synaps(\.exe)?"? login --provider claude' } | Select-Object -First 1
    if (-not $signIn) { Start-Sleep -Milliseconds 500 }
}
Check ([bool]$signIn) "Finish started the sign-in, no terminal: $(if ($signIn) { $signIn.CommandLine })"
Start-Sleep -Seconds 4
$browser = @(Get-Process -Name msedge, chrome, firefox -ErrorAction SilentlyContinue)
Write-Host "  a browser runs: $(if ($browser) { ($browser | Select-Object -ExpandProperty Name -Unique) -join ', ' } else { 'none seen' })"
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'steps\.ps1" login|synaps(\.exe)?"? login' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

# Her, started by the finish page.
$her = $null
$t0 = Get-Date
while (-not $her -and ((Get-Date) - $t0).TotalSeconds -lt 30) { $her = Get-Process synaps-avatar -ErrorAction SilentlyContinue | Select-Object -First 1; Start-Sleep -Milliseconds 300 }
Check ([bool]$her) '"Start her now" started her'
Start-Sleep -Seconds 12
if ($her) { Get-Shot 'her-on-the-desktop' -ProcessId $her.Id -Pad 240; Get-Shot 'her' -ProcessId $her.Id -Pad 40 }
$herLog = Join-Path $env:LOCALAPPDATA 'synaps-avatar\synaps-avatar.log'
if (Test-Path $herLog) { Write-Output "--- her log (end) ---`n$((Get-Content $herLog -Tail 12) -join "`n")`n---" }

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
