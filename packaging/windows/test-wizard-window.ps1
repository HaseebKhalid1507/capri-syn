# The wizard's window, as a person uses it, for test-setup-wizard.ps1 (dot-sourced): on a normal page, and
# again while a step runs (Synaps, her art, the daemon: it takes a while), its top-level window must
#   - have its caption (WS_CAPTION), and answer WM_NCHITTEST with HTCAPTION on its title bar;
#   - be responsive: not hung (IsHungAppWindow), WM_NULL answered within 1 s;
#   - be enabled (not WS_DISABLED);
#   - drag: a real mouse drag by its title bar (SendInput: down, move, up) moves it;
#   - while a step runs, with its "About Setup..." box open (system menu), stay disabled under it, and be
#     enabled again once it is closed.
# "I can't drag the installer": Setup disables every window of its own while ExecAndLogOutput waits, so
# the wizard answered and painted, but took no mouse. Uses Check, Get-WindowText and $log (Setup's own
# log: "step NAME: ARGS" opens a step, "step NAME: exit N" closes it) of the script that dot-sources it.
# Windows PowerShell 5.1.

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
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);

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
    // The enabled popup a window owns (GW_ENABLEDPOPUP: a modal box over it), 0 when it has none.
    public static IntPtr Popup(IntPtr h) { IntPtr p = GetWindow(h, 6); return p == h ? IntPtr.Zero : p; }
    public static bool Alive(IntPtr h) { return IsWindow(h); }
    // PostMessage: it returns at once (SendMessage waits for a modal box, and times out).
    public static bool Post(IntPtr h, uint msg, int w) { return PostMessage(h, msg, new IntPtr(w), IntPtr.Zero); }
    // The top-level window under a screen point.
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

# One look at a window (by its handle): its caption, whether it answers, and with -Drag a real drag by
# its title bar, after which it is put back where it was (the shots).
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

# The wizard $W (its UI Automation element) on a normal page: caption, responsive, enabled, and a drag.
function Test-WizardWindow {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
    param($W, [string]$Page)
    $st = Get-WindowState ([IntPtr][int]$W.Current.NativeWindowHandle) -Drag
    Write-Host "  $Page`: $(Format-WindowState $st)"
    Check ($st.Caption -and $st.HitCaption) "$Page`: the window has its caption (WS_CAPTION) and its title bar answers WM_NCHITTEST with HTCAPTION"
    Check ((-not $st.Hung) -and $st.Answers) "$Page`: the window is responsive (not hung, WM_NULL answered within 1 s)"
    Check $st.Enabled "$Page`: the window is enabled"
    Check ([bool]$st.Moved) "$Page`: dragging it by its title bar moves it ($($st.From) -> $($st.To))"
}

# The steps Setup ran, from its log.
function Get-StepRun {
    $runs = [System.Collections.Generic.List[object]]::new()
    $lines = try { @(Get-Content -LiteralPath $log -ErrorAction Stop) } catch { @() }
    foreach ($l in $lines) {
        if ($l -notmatch '^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d)\S*\s+step ([\w-]+): (.*)$') { continue }
        $at = [datetime]$Matches[1]; $name = $Matches[2]; $rest = $Matches[3]
        if ($rest -match '^exit (-?\d+)') {
            $code = $Matches[1]
            $open = $runs | Where-Object { $_.Name -eq $name -and -not $_.End } | Select-Object -Last 1
            if ($open) { $open.End = $at; $open.Exit = $code }
        } else {
            # One step runs at a time: one still open when the next starts never logged its exit (an
            # early exit of an older Setup's RunStep), so it is closed here, not left running for ever.
            foreach ($o in @($runs | Where-Object { -not $_.End })) { $o.End = $at; $o.Exit = '?' }
            $runs.Add([pscustomobject]@{ Name = $name; Start = $at; End = $null; Exit = $null })
        }
    }
    $runs
}
# The step running now, if one is.
function Get-StepNow { Get-StepRun | Where-Object { -not $_.End } | Select-Object -Last 1 }
function Write-StepTime {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
    param()
    $runs = @(Get-StepRun)
    foreach ($r in $runs) { Write-Host ("  step {0}: {1} s (exit {2})" -f $r.Name, $(if ($r.End) { [int]($r.End - $r.Start).TotalSeconds } else { '?' }), $r.Exit) }
    # Every "step NAME: ARGS" has its "step NAME: exit N", whichever way the step ended.
    $open = @($runs | Where-Object { $_.Exit -eq '?' -or -not $_.End })
    Check ($runs.Count -gt 0 -and $open.Count -eq 0) "Setup's log has an exit line for each of its $($runs.Count) steps$(if ($open) { " (none for: $(($open | ForEach-Object { $_.Name }) -join ', '))" })"
}

# The wizard's "About Setup..." (its system menu: the icon, Alt+Space, a right click on the title bar)
# opened while a step runs: a modal box inside Setup's wait for the step. The wizard must stay disabled
# under it (enabled, its title bar would open a second box), and be live again once the box is closed.
# $false when no step was running to open it in (try again later).
function Test-WizardAbout {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
    param($W)
    $h = [IntPtr][int]$W.Current.NativeWindowHandle
    $run = Get-StepNow
    if (-not $run) { return $false }
    [void][WizWin]::Post($h, 0x112, 9999) # WM_SYSCOMMAND, Inno's "About Setup..." item
    $box = $null
    $t0 = Get-Date
    while (-not $box -and ((Get-Date) - $t0).TotalSeconds -lt 15) { $box = Find-Window '^About Setup$'; if (-not $box) { Start-Sleep -Milliseconds 200 } }
    Check ([bool]$box) "while step '$($run.Name)' runs: the wizard's About box opens"
    if (-not $box) { return $true }
    $bh = [IntPtr][int]$box.Current.NativeWindowHandle
    # The box is up: the 50 ms timer that keeps the wizard live runs in its loop too, 24 ticks or so.
    $live = 0
    for ($i = 0; $i -lt 12; $i++) { Start-Sleep -Milliseconds 100; if ([WizWin]::Enabled($h)) { $live++ } }
    Write-Host "  while the About box is up: it is the wizard's enabled popup: $([WizWin]::Popup($h) -eq $bh); the wizard was enabled in $live of 12 looks"
    Check ($live -eq 0) "while the About box is up: the wizard stays disabled under it ($live of 12 looks enabled)"
    # Closed as he would: OK; the close message when that finds no button.
    try { Invoke-Button $box 'OK' } catch { Write-Host "  ($($_.Exception.Message))" }
    $t0 = Get-Date
    while ([WizWin]::Alive($bh) -and ((Get-Date) - $t0).TotalSeconds -lt 3) { Start-Sleep -Milliseconds 200 }
    if ([WizWin]::Alive($bh)) { [void][WizWin]::Post($bh, 0x10, 0) } # WM_CLOSE
    $t0 = Get-Date
    while ([WizWin]::Alive($bh) -and ((Get-Date) - $t0).TotalSeconds -lt 5) { Start-Sleep -Milliseconds 200 }
    Check (-not [WizWin]::Alive($bh)) 'the About box closes'
    if ([WizWin]::Alive($bh)) { throw 'the About box stays open: Setup waits behind it' }
    $t0 = Get-Date
    while (-not [WizWin]::Enabled($h) -and ((Get-Date) - $t0).TotalSeconds -lt 3) { Start-Sleep -Milliseconds 100 }
    Check ([WizWin]::Enabled($h)) 'after the About box is closed: the wizard is enabled again (a step is running)'
    $true
}

# The wizard $W while the steps run (Setup's log says which): the same look again and again, with a
# drag in the first four, until ten looks were taken inside one step each, or the finish page is up.
function Test-WizardDuringStep {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
    param($W)
    $h = [IntPtr][int]$W.Current.NativeWindowHandle
    $during = [System.Collections.Generic.List[object]]::new()
    $drags = 0
    $about = $false
    $t0 = Get-Date
    while ($during.Count -lt 10 -and ((Get-Date) - $t0).TotalSeconds -lt 1200) {
        if (@(Get-WindowText $W) -match '^Completing the capri-syn Setup Wizard$') { break }
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
        # Once, from the fifth look on, inside a step.
        if (-not $about -and $during.Count -ge 5) { $about = Test-WizardAbout $W }
    }
    Check $about 'the About box was opened and closed while a step ran'
    $names = @($during | ForEach-Object { $_.Step } | Select-Object -Unique)
    Check ($during.Count -ge 3) "the window was looked at $($during.Count) times while a step ran ($($names -join ', ')): at least 3"
    $bad = @($during | Where-Object { -not ($_.Caption -and $_.HitCaption) })
    Check ($bad.Count -eq 0) "while a step runs: the window has its caption (WS_CAPTION) and its title bar answers WM_NCHITTEST with HTCAPTION ($($during.Count - $bad.Count) of $($during.Count) looks)"
    $bad = @($during | Where-Object { $_.Hung -or -not $_.Answers })
    Check ($bad.Count -eq 0) "while a step runs: the window is responsive (not hung, WM_NULL answered within 1 s) ($($during.Count - $bad.Count) of $($during.Count) looks)"
    $bad = @($during | Where-Object { -not $_.Enabled })
    Check ($bad.Count -eq 0) "while a step runs: the window is enabled ($($during.Count - $bad.Count) of $($during.Count) looks)"
    $dragged = @($during | Where-Object { $null -ne $_.Moved })
    $bad = @($dragged | Where-Object { -not $_.Moved })
    Check ($dragged.Count -gt 0 -and $bad.Count -eq 0) "while a step runs: dragging it by its title bar moves it ($($dragged.Count - $bad.Count) of $($dragged.Count) drags did)"
}
