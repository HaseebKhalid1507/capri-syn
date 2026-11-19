<#
.SYNOPSIS
  Screenshots on a Windows CI runner: the display pinned, a process's windows found and cropped,
  PNGs out. Never the whole desktop: a shot is the union of the windows asked for, padded.

.DESCRIPTION
  Dot-source it for the functions (the shots workflow does, and so can an installer job):
      . tools/shoot-win.ps1
      Set-ShotDisplay -Width 1920 -Height 1080 -Scale 100
      Save-WindowShot -Path out\setup.png -ProcessId $p.Id
      Save-WindowShot -Path out\wizard.png -Title 'Setup - *'
  or run it for one thing:
      pwsh tools/shoot-win.ps1 -Display 1920x1080@150
      pwsh tools/shoot-win.ps1 -Shot out\setup.png -ProcessId 1234 [-Pad 12]
      pwsh tools/shoot-win.ps1 -Shot out\setup.png -Title 'Setup - *'

  Also here: Send-ShotKeys (SendInput chords: 'ctrl+oem_comma', 'right', 'escape') and
  Send-ShotClick (a click at screen pixels), for what a program's own control can't do.
  Every rect is in physical pixels (this thread is made per-monitor DPI aware), so shots and clicks
  agree at any scaling. No param block: dot-sourcing it sets nothing in the caller's scope but its
  functions.
#>

Add-Type -AssemblyName System.Drawing
if (-not ('ShootWin.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace ShootWin {
public static class Native {
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    public static void Aware() { SetThreadDpiAwarenessContext(new IntPtr(-4)); }

    // ---- windows ------------------------------------------------------------------------------
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int v, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public class Win { public long Handle; public uint Pid; public string Title; public int X, Y, W, H; }

    /// Visible, uncloaked top-level windows (DWM's frame bounds where it has them), top first.
    public static List<Win> Windows() {
        Aware();
        var all = new List<Win>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            int cloaked;
            if (DwmGetWindowAttribute(h, 14, out cloaked, 4) == 0 && cloaked != 0) return true;
            RECT r;
            if (DwmGetWindowAttribute(h, 9, out r, 16) != 0 && !GetWindowRect(h, out r)) return true;
            if (r.R - r.L < 2 || r.B - r.T < 2) return true;
            uint pid; GetWindowThreadProcessId(h, out pid);
            var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
            all.Add(new Win { Handle = h.ToInt64(), Pid = pid, Title = sb.ToString(), X = r.L, Y = r.T, W = r.R - r.L, H = r.B - r.T });
            return true;
        }, IntPtr.Zero);
        return all;
    }

    // ---- the screen ---------------------------------------------------------------------------
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
    /// The virtual screen: x, y, w, h.
    public static int[] Screen() {
        Aware();
        return new[] { GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79) };
    }

    // ---- input --------------------------------------------------------------------------------
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct INPUTU { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public INPUTU u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    const uint KEYUP = 2, EXTENDED = 1;

    static INPUT Key(ushort vk, bool up) {
        var i = new INPUT { type = 1 };
        i.u.ki.vk = vk;
        // The arrows and their block are extended keys.
        bool ext = (vk >= 0x21 && vk <= 0x28) || vk == 0x2D || vk == 0x2E;
        i.u.ki.flags = (up ? KEYUP : 0) | (ext ? EXTENDED : 0);
        return i;
    }

    /// A chord: the keys down in order, then up in reverse.
    public static uint Chord(ushort[] vks) {
        var list = new List<INPUT>();
        foreach (var v in vks) list.Add(Key(v, false));
        for (int i = vks.Length - 1; i >= 0; i--) list.Add(Key(vks[i], true));
        return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
    }

    /// The pointer to screen pixels (off what a shot shows: no hover).
    public static void Move(int x, int y) { Aware(); SetCursorPos(x, y); }

    /// A click at screen pixels (right: the right button).
    public static uint Click(int x, int y, bool right) {
        Aware();
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(30);
        var down = new INPUT { type = 0 }; down.u.mi.flags = right ? 0x0008u : 0x0002u;
        var up = new INPUT { type = 0 }; up.u.mi.flags = right ? 0x0010u : 0x0004u;
        return SendInput(2, new[] { down, up }, Marshal.SizeOf(typeof(INPUT)));
    }

    // ---- the display: its mode and its scaling ------------------------------------------------
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
        public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);

    /// The modes the display offers (w x h), and the current one first.
    public static List<string> Modes() {
        var o = new List<string>();
        var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        if (EnumDisplaySettings(null, -1, ref dm)) o.Add("current " + dm.dmPelsWidth + "x" + dm.dmPelsHeight);
        for (int i = 0; EnumDisplaySettings(null, i, ref dm); i++) {
            var s = dm.dmPelsWidth + "x" + dm.dmPelsHeight;
            if (!o.Contains(s)) o.Add(s);
        }
        return o;
    }

    /// 0: done (DISP_CHANGE_SUCCESSFUL).
    public static int SetMode(int w, int h) {
        var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        if (!EnumDisplaySettings(null, -1, ref dm)) return -100;
        if (dm.dmPelsWidth == w && dm.dmPelsHeight == h) return 0;
        dm.dmPelsWidth = w; dm.dmPelsHeight = h; dm.dmFields = 0x80000 | 0x100000;
        return ChangeDisplaySettings(ref dm, 0);
    }

    [StructLayout(LayoutKind.Sequential)] public struct LUID { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] struct PATH_SOURCE { public LUID adapter; public uint id, modeIdx, status; }
    [StructLayout(LayoutKind.Sequential)] struct PATH_TARGET { public LUID adapter; public uint id, modeIdx, tech, rotation, scaling, refNum, refDen, scan; public int avail; public uint status; }
    [StructLayout(LayoutKind.Sequential)] struct PATH { public PATH_SOURCE src; public PATH_TARGET tgt; public uint flags; }
    [StructLayout(LayoutKind.Sequential, Size = 64)] struct MODE { public uint type, id; public LUID adapter; }
    [StructLayout(LayoutKind.Sequential)] struct HEADER { public int type; public uint size; public LUID adapter; public uint id; }
    [StructLayout(LayoutKind.Sequential)] struct DPI_GET { public HEADER h; public int min, cur, max; }
    [StructLayout(LayoutKind.Sequential)] struct DPI_SET { public HEADER h; public int rel; }
    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint np, out uint nm);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint np, [Out] PATH[] p, ref uint nm, [Out] MODE[] m, IntPtr topo);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref DPI_GET p);
    [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(ref DPI_SET p);
    static readonly int[] Steps = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };

    static PATH[] Paths() {
        uint np, nm;
        if (GetDisplayConfigBufferSizes(2, out np, out nm) != 0) return new PATH[0];
        var p = new PATH[np]; var m = new MODE[nm];
        if (QueryDisplayConfig(2, ref np, p, ref nm, m, IntPtr.Zero) != 0) return new PATH[0];
        Array.Resize(ref p, (int)np);
        return p;
    }

    /// Each active display's scaling: "current% (recommended%, up to max%)". The settings app's
    /// own call (undocumented: DisplayConfigGetDeviceInfo type -3, scale steps relative to the
    /// recommended one).
    public static List<string> Scales() {
        var o = new List<string>();
        foreach (var p in Paths()) {
            var g = new DPI_GET(); g.h.type = -3; g.h.size = (uint)Marshal.SizeOf(typeof(DPI_GET)); g.h.adapter = p.src.adapter; g.h.id = p.src.id;
            if (DisplayConfigGetDeviceInfo(ref g) != 0) { o.Add("unknown"); continue; }
            int rec = Math.Abs(g.min), cur = Math.Min(Math.Max(g.cur, g.min), g.max);
            o.Add(Steps[Math.Min(rec + cur, Steps.Length - 1)] + "% (recommended " + Steps[rec] + "%, up to " + Steps[Math.Min(rec + g.max, Steps.Length - 1)] + "%)");
        }
        return o;
    }

    /// Every active display to `pct` scaling, live (no sign-out). Throws where it can't.
    public static void SetScale(int pct) {
        int want = Array.IndexOf(Steps, pct);
        if (want < 0) throw new ArgumentException("no such scaling step: " + pct);
        var paths = Paths();
        if (paths.Length == 0) throw new InvalidOperationException("QueryDisplayConfig found no display");
        foreach (var p in paths) {
            var g = new DPI_GET(); g.h.type = -3; g.h.size = (uint)Marshal.SizeOf(typeof(DPI_GET)); g.h.adapter = p.src.adapter; g.h.id = p.src.id;
            int r = DisplayConfigGetDeviceInfo(ref g);
            if (r != 0) throw new InvalidOperationException("DisplayConfigGetDeviceInfo: " + r);
            int rec = Math.Abs(g.min), rel = want - rec;
            if (rel < g.min || rel > g.max) throw new InvalidOperationException(pct + "% is outside this display's " + Steps[rec + g.min] + "-" + Steps[Math.Min(rec + g.max, Steps.Length - 1)] + "%");
            var s = new DPI_SET(); s.h.type = -4; s.h.size = (uint)Marshal.SizeOf(typeof(DPI_SET)); s.h.adapter = p.src.adapter; s.h.id = p.src.id; s.rel = rel;
            r = DisplayConfigSetDeviceInfo(ref s);
            if (r != 0) throw new InvalidOperationException("DisplayConfigSetDeviceInfo: " + r);
        }
    }

    [DllImport("user32.dll")] static extern uint GetDpiForSystem();
    public static uint SystemDpi() { Aware(); return GetDpiForSystem(); }
}
}
'@
}

# ---- the display -------------------------------------------------------------------------------

<# The display pinned: its mode (w x h) and its scaling (100, 125, 150 ...), checked after. Says what
   it found and what it set; throws if it can't. #>
function Set-ShotDisplay {
    param([int]$Width = 1920, [int]$Height = 1080, [int]$Scale = 100)
    Write-Host "display: modes $(([ShootWin.Native]::Modes()) -join ', ')"
    $r = [ShootWin.Native]::SetMode($Width, $Height)
    if ($r -ne 0) {
        Write-Host "display: ChangeDisplaySettings ${Width}x${Height}: $r; trying Set-DisplayResolution"
        Set-DisplayResolution -Width $Width -Height $Height -Force
    }
    Start-Sleep -Milliseconds 800
    $was = ([ShootWin.Native]::Scales()) -join '; '
    Write-Host "display: scaling before: $was"
    if (-not $was.StartsWith("$Scale%")) {
        [ShootWin.Native]::SetScale($Scale)
        Start-Sleep -Seconds 2
    }
    $s = [ShootWin.Native]::Screen()
    $now = ([ShootWin.Native]::Scales()) -join '; '
    Write-Host "display: screen $($s[2])x$($s[3]) at $($s[0]),$($s[1]); scaling $now"
    if ($s[2] -ne $Width -or $s[3] -ne $Height) { throw "display: the screen is $($s[2])x$($s[3]), not ${Width}x${Height}" }
    if (-not $now.StartsWith("$Scale%")) { throw "display: scaling is $now, not $Scale%" }
}

# ---- windows and shots -------------------------------------------------------------------------

<# Visible top-level windows: of a process (and its children's, -Tree), or titled like -Title. #>
function Get-ShotWindows {
    param([int]$ProcessId, [string]$Title, [switch]$Tree)
    $pids = @()
    if ($ProcessId) {
        $pids = @($ProcessId)
        if ($Tree) {
            $kids = Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -in $pids } | ForEach-Object { [int]$_.ProcessId }
            $pids += $kids
        }
    }
    [ShootWin.Native]::Windows() | Where-Object {
        ((-not $ProcessId) -or ([int]$_.Pid -in $pids)) -and ((-not $Title) -or ($_.Title -like $Title))
    }
}

<# One rect around them all (x, y, w, h), padded and kept on the screen. Null: none. #>
function Get-ShotRect {
    param([object[]]$Windows, [int]$Pad = 12)
    if (-not $Windows) { return $null }
    $l = ($Windows | ForEach-Object { $_.X } | Measure-Object -Minimum).Minimum - $Pad
    $t = ($Windows | ForEach-Object { $_.Y } | Measure-Object -Minimum).Minimum - $Pad
    $r = ($Windows | ForEach-Object { $_.X + $_.W } | Measure-Object -Maximum).Maximum + $Pad
    $b = ($Windows | ForEach-Object { $_.Y + $_.H } | Measure-Object -Maximum).Maximum + $Pad
    $s = [ShootWin.Native]::Screen()
    $l = [Math]::Max($l, $s[0]); $t = [Math]::Max($t, $s[1])
    $r = [Math]::Min($r, $s[0] + $s[2]); $b = [Math]::Min($b, $s[1] + $s[3])
    if ($r -le $l -or $b -le $t) { return $null }
    return @([int]$l, [int]$t, [int]($r - $l), [int]($b - $t))
}

<# A PNG of a rect of the screen (physical px): BitBlt with CAPTUREBLT, so layered windows too. #>
function Save-RectShot {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][int[]]$Rect)
    $full = [IO.Path]::GetFullPath($Path)
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $full) | Out-Null
    [ShootWin.Native]::Aware()
    $bmp = [System.Drawing.Bitmap]::new($Rect[2], $Rect[3])
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $op = [Enum]::ToObject([System.Drawing.CopyPixelOperation], 0x40CC0020)
        $g.CopyFromScreen($Rect[0], $Rect[1], 0, 0, [System.Drawing.Size]::new($Rect[2], $Rect[3]), $op)
        $bmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $g.Dispose(); $bmp.Dispose()
    }
}

<# A PNG of a process's windows (or those titled like -Title), cropped to them. Returns the rect
   (x, y, w, h), or throws if nothing of theirs is on screen. #>
function Save-WindowShot {
    param([Parameter(Mandatory)][string]$Path, [int]$ProcessId, [string]$Title, [int]$Pad = 12, [switch]$Tree, [int[]]$Also)
    $wins = @(Get-ShotWindows -ProcessId $ProcessId -Title $Title -Tree:$Tree)
    if ($Also) { $wins += [pscustomobject]@{ X = $Also[0]; Y = $Also[1]; W = $Also[2]; H = $Also[3] } }
    $rect = Get-ShotRect -Windows $wins -Pad $Pad
    if (-not $rect) { throw "shot ${Path}: no window on screen (pid $ProcessId, title '$Title')" }
    Save-RectShot -Path $Path -Rect $rect
    return $rect
}

# ---- input (only where the program's own control can't) ---------------------------------------

<# A chord, 'ctrl+comma' or 'right': into whatever has the keyboard. #>
function Send-ShotKeys {
    param([Parameter(Mandatory)][string]$Chord)
    $vk = @{
        ctrl = 0x11; control = 0x11; shift = 0x10; alt = 0x12; win = 0x5B
        enter = 0x0D; return = 0x0D; escape = 0x1B; esc = 0x1B; tab = 0x09; space = 0x20; backspace = 0x08
        left = 0x25; up = 0x26; right = 0x27; down = 0x28; home = 0x24; end = 0x23; pageup = 0x21; pagedown = 0x22; delete = 0x2E
        comma = 0xBC; period = 0xBE
    }
    $vks = foreach ($k in $Chord.ToLower().Split('+')) {
        if ($vk.ContainsKey($k)) { [uint16]$vk[$k] }
        elseif ($k.Length -eq 1) { [uint16][char]$k.ToUpper() }
        else { throw "Send-ShotKeys: unknown key '$k'" }
    }
    $n = [ShootWin.Native]::Chord([uint16[]]$vks)
    if ($n -ne 2 * @($vks).Count) { throw "Send-ShotKeys ${Chord}: SendInput took $n of $(2 * @($vks).Count)" }
}

<# The pointer out of the way (a hover lights what it is over). #>
function Move-ShotPointer {
    param([int]$X = 0, [int]$Y = 0)
    [ShootWin.Native]::Move($X, $Y)
}

<# A click at screen pixels. #>
function Send-ShotClick {
    param([Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y, [switch]$Right)
    $n = [ShootWin.Native]::Click($X, $Y, [bool]$Right)
    if ($n -ne 2) { throw "Send-ShotClick ${X},${Y}: SendInput took $n of 2" }
}

# ---- run as a script: -Display WxH[@scale] | -Shot PATH (-ProcessId N | -Title GLOB) [-Pad N] ---

if ($args.Count -gt 0) {
    $cli = @{}
    for ($i = 0; $i -lt $args.Count; $i += 2) { $cli[([string]$args[$i]).TrimStart('-').ToLower()] = $args[$i + 1] }
    if ($cli.display) {
        if ($cli.display -notmatch '^(\d+)x(\d+)(?:@(\d+))?$') { throw "-Display WxH[@scale], e.g. 1920x1080@100" }
        $sc = if ($Matches[3]) { [int]$Matches[3] } else { 100 }
        Set-ShotDisplay -Width ([int]$Matches[1]) -Height ([int]$Matches[2]) -Scale $sc
    }
    if ($cli.shot) {
        $pad = if ($cli.pad) { [int]$cli.pad } else { 12 }
        $r = Save-WindowShot -Path $cli.shot -ProcessId ([int]$cli.processid) -Title ([string]$cli.title) -Pad $pad
        Write-Host "shot $($cli.shot): $($r -join ',')"
    }
}
