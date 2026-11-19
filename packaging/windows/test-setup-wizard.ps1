# Her installer's wizard, driven as a person would click it (UI Automation), with a screenshot of every
# page (tools/shoot-win.ps1), then "Start her now" and her on the desktop, then the uninstall's
# questions (and "Remove Synaps too?": yes). CI's windows-setup-wizard job, on a clean runner, uploads
# the shots; it runs as is in the capri-win VM. Windows PowerShell 5.1 (UIAutomationClient).
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
        if ($t -match $Caption) { Start-Sleep -Milliseconds 400; return $t }
        Start-Sleep -Milliseconds 300
    }
    throw "the wizard never showed '$Caption' (it shows: $((Get-WindowText $W) -join ' | '))"
}

# The wizard, as he gets it: double-clicked, every default left as it is.
$log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-wizard-$PID.log"
# The shots' desktop quiet (the runner's console minimized, a flat backdrop), before the wizard.
try { Clear-ShotDesktop } catch { Write-Host "  (desktop stays as it is: $($_.Exception.Message))" }
$p = Start-Process $Setup -ArgumentList "/LOG=`"$log`"" -PassThru
$w = Wait-Window '^Setup - capri-syn'
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
    Invoke-Button $w $page[2]
}
# The steps, on their progress page (Synaps, her art, the daemon), then the finish page.
$null = Wait-Page $w '^Setting her up$' 180
Start-Sleep -Seconds 3
Get-Shot 'setting-her-up' -Title 'Setup - capri-syn*'
$t = Wait-Page $w '^Completing the capri-syn Setup Wizard$' 1200
Get-Shot 'finish' -Title 'Setup - capri-syn*'
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
$null = $u

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her wizard, clicked through: every page shot'
