# A screenshot on Windows, for CI and the VM: the whole screen, or one program's window cropped to it.
# Used by her installer's wizard and desktop shots (packaging/windows/test-setup-wizard.ps1).
#
#   tools\shoot-win.ps1 -Out shot.png                          # every monitor
#   tools\shoot-win.ps1 -Out wizard.png -Process capri-syn-setup-0.0.1-abc*   # its window (the largest
#                                                              # visible one), cropped, + -Pad px
#   tools\shoot-win.ps1 -Out her.png -ProcessId 1234 -Pad 40 -WaitSec 20
#   tools\shoot-win.ps1 -Out x.png -Title '^Setup - capri-syn'
#
# Waits up to -WaitSec for the window to show. Prints the saved path. Windows PowerShell 5.1 and 7.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'A tool run by hand and in CI.')]
param(
    [Parameter(Mandatory)] [string]$Out,
    [string]$Process = '',
    [int]$ProcessId = 0,
    [string]$Title = '',
    [int]$Pad = 0,
    [int]$WaitSec = 10
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
if (-not ('ShootWin' -as [type])) {
    Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class ShootWin {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    public static RECT Bounds(IntPtr h) {
        RECT r;
        // The frame as drawn (no invisible resize borders), else the window rect.
        if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) != 0) GetWindowRect(h, out r);
        return r;
    }
    public static List<object[]> Windows() {
        var l = new List<object[]>();
        EnumWindows((h, p) => {
            if (!IsWindowVisible(h)) return true;
            uint pid; GetWindowThreadProcessId(h, out pid);
            var sb = new StringBuilder(512); GetWindowText(h, sb, 512);
            var r = Bounds(h);
            l.Add(new object[] { h, (int)pid, sb.ToString(), r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top });
            return true;
        }, IntPtr.Zero);
        return l;
    }
}
'@
}
[void][ShootWin]::SetProcessDPIAware()

# The target's largest visible window: X, Y, W, H, Title; $null when there's none (yet).
function Find-Window {
    $pids = @()
    if ($ProcessId) { $pids = @($ProcessId) }
    elseif ($Process) { $pids = @(Get-Process -Name $Process -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }) }
    $best = $null
    foreach ($w in [ShootWin]::Windows()) {
        if ($w[5] -le 0 -or $w[6] -le 0) { continue }
        if ($pids.Count -and $pids -notcontains $w[1]) { continue }
        if ($Title -and $w[2] -notmatch $Title) { continue }
        if (-not $best -or ($w[5] * $w[6]) -gt ($best.W * $best.H)) { $best = @{ X = $w[3]; Y = $w[4]; W = $w[5]; H = $w[6]; Title = $w[2] } }
    }
    $best
}

# The virtual screen (every monitor): SM_XVIRTUALSCREEN 76, SM_YVIRTUALSCREEN 77, SM_CXVIRTUALSCREEN 78, SM_CYVIRTUALSCREEN 79.
$vx = [ShootWin]::GetSystemMetrics(76); $vy = [ShootWin]::GetSystemMetrics(77)
$vw = [ShootWin]::GetSystemMetrics(78); $vh = [ShootWin]::GetSystemMetrics(79)
$r = @{ X = $vx; Y = $vy; W = $vw; H = $vh; Title = '(screen)' }
if ($Process -or $ProcessId -or $Title) {
    $w = $null
    $t0 = Get-Date
    while (-not $w -and ((Get-Date) - $t0).TotalSeconds -lt $WaitSec) {
        $w = Find-Window
        if (-not $w) { Start-Sleep -Milliseconds 250 }
    }
    if (-not $w) { throw "shoot-win: no visible window for $(if ($ProcessId) { "pid $ProcessId" } elseif ($Process) { $Process } else { $Title }) in $WaitSec s" }
    # Cropped to it (+ pad), and to the screen.
    $x0 = [Math]::Max($vx, $w.X - $Pad); $y0 = [Math]::Max($vy, $w.Y - $Pad)
    $x1 = [Math]::Min($vx + $vw, $w.X + $w.W + $Pad); $y1 = [Math]::Min($vy + $vh, $w.Y + $w.H + $Pad)
    $r = @{ X = $x0; Y = $y0; W = $x1 - $x0; H = $y1 - $y0; Title = $w.Title }
}
$bmp = New-Object Drawing.Bitmap $r.W, $r.H
$g = [Drawing.Graphics]::FromImage($bmp)
try {
    $g.CopyFromScreen($r.X, $r.Y, 0, 0, $bmp.Size)
    $dir = Split-Path -Parent ([IO.Path]::GetFullPath($Out))
    if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
    $bmp.Save([IO.Path]::GetFullPath($Out), [Drawing.Imaging.ImageFormat]::Png)
} finally { $g.Dispose(); $bmp.Dispose() }
Write-Host "shoot-win: $Out ($($r.W)x$($r.H) at $($r.X),$($r.Y): $($r.Title))"
