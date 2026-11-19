//! The Windows backend: plain translation between Win32/DWM and the core's types (her body, her
//! idle loop, perching, her global keys, the Line's keyboard, effect layers, the media flyout's
//! players, toasts, her tray icon; voice comes later). Every rule built on these answers is the
//! core's (`os.rs`).
//!
//! GPUI's Windows crate is vendored and patched (vendor/gpui-pre-windows/PATCHES.md): a `PopUp`
//! is a bare `WS_POPUP` (PW-1), a click on a window opened without focus doesn't activate it
//! (PW-2), a pure move doesn't re-render (PW-3), `activate` injects no input (PW-4), a pop-up
//! keeps one scale on every monitor (PW-5). `adopt` adds the rest: topmost tool window, DWM's
//! frame, border and corner rounding off, and click-through. GPUI composites through
//! DirectComposition (`WS_EX_NOREDIRECTIONBITMAP`, a premultiplied swap chain): per-pixel alpha
//! with no layered-window bitmap. Click-through: `WS_EX_LAYERED` + `WS_EX_TRANSPARENT`, the
//! latter toggled by a pointer poller from her input region (`WM_NCHITTEST` → `HTTRANSPARENT`
//! only passes clicks to windows of the same thread, measured in the P0 probe).
//!
//! The keyboard: Windows hands a process the foreground only right after input of his to it
//! (the foreground lock). She takes it only then: her key (`WM_HOTKEY`) or his press on one of
//! her windows, within `GRANT_MS`, or while hers have it (`Caps::keyboard_on_input`). Nothing
//! else moves it (no synthetic input, no `AttachThreadInput`).
//!
//! Coordinates: root px are device px (per-monitor DPI aware v2), from the virtual screen's
//! top-left.

use super::{Caps, Desktop, FxZ, Hint, KeyEv, Keys, Monitor, PointerRead, Role, SessionEv, ShapeRect, Stage, Visible, Win, WinId, WinRect};
use ::windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use ::windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};
use ::windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_BINARY, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW};
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WAIT_OBJECT_0, WPARAM};
use ::windows::Win32::Graphics::Dwm::{
    DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use ::windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DEVMODEW, DeleteObject, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC,
    HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint, MonitorFromWindow, RGN_OR, SetWindowRgn,
};
use ::windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, HOT_KEY_MODIFIERS, LASTINPUTINFO, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, VK_ESCAPE,
    VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VkKeyScanW,
};
use ::windows::Win32::UI::Shell::{
    DefSubclassProc, IVirtualDesktopManager, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, RemoveWindowSubclass, SHQueryUserNotificationState,
    SetWindowSubclass, VirtualDesktopManager,
};
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::{BOOL, PCWSTR};
use anyhow::{Result, bail};
use gpui_kit::{TitlebarOptions, Window, WindowKind, WindowOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod dirs;
mod fx;
mod media;
mod notifications;
#[cfg(test)]
mod testkit;
mod hold;
mod sidecar;
mod tray;
mod voice;

pub use dirs::{base_dir, home_dir};
pub use media::watch_media;
pub use notifications::notify;
pub use sidecar::voice_sidecar;
pub use voice::{VOICE_LINK_BLOCKS, VoiceLink, voice_link, voice_link_bg};

/// How long his input to her (her key, a press on one of her windows) lets her take the
/// keyboard: the Line opening on it takes well under this.
const GRANT_MS: u64 = 1000;

/// Milliseconds since boot (the clock `GRANT_MS` is counted on).
fn tick() -> u64 {
    unsafe { ::windows::Win32::System::SystemInformation::GetTickCount64() }
}

fn buttons_down() -> bool {
    unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) < 0) }
}

/// The window under the pointer is one of hers (a press there is his click on her). A
/// click-through surface letting the pointer by is not under it.
fn hers_at_pointer(mine: &Mutex<HashSet<WinId>>) -> bool {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return false;
    }
    let h = unsafe { WindowFromPoint(p) };
    !h.0.is_null() && mine.lock().is_ok_and(|m| m.contains(&id(unsafe { GetAncestor(h, GA_ROOT) })))
}

/// HWNDs are 32-bit values sign-extended (they cross 32/64-bit processes): a `WinId` holds one.
fn hwnd(w: WinId) -> HWND {
    HWND(w as i32 as isize as *mut _)
}

fn id(h: HWND) -> WinId {
    h.0 as isize as i32 as u32
}

/// The virtual screen's top-left (root px are counted from it).
fn origin() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) }
}

/// The primary monitor's scale (effective DPI ÷ 96): GPUI's primary display's, so her `sf`, and
/// the one her pop-ups keep on every monitor (PW-5).
fn primary_scale() -> f32 {
    use ::windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let m = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let (mut x, mut y) = (0u32, 0u32);
    match unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) } {
        Ok(()) if x > 0 => x as f32 / 96.0,
        _ => 1.0,
    }
}

thread_local! {
    /// The shell's virtual desktop manager, made once per thread that asks (None: there is none).
    static VDM: std::cell::OnceCell<Option<IVirtualDesktopManager>> = const { std::cell::OnceCell::new() };
}

/// On the virtual desktop he sees. A window on another one is cloaked as well (`showing` leaves
/// it out already); the shell's own answer settles a window it is moving. Unknown: yes.
fn on_current_desktop(h: HWND) -> bool {
    VDM.with(|v| {
        let m = v.get_or_init(|| unsafe {
            // COM on this thread if nothing set it up (GPUI's main thread already has it).
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL).ok()
        });
        m.as_ref().is_none_or(|m| !matches!(unsafe { m.IsWindowOnCurrentVirtualDesktop(h) }, Ok(b) if !b.as_bool()))
    })
}

/// Where the shell keeps the current virtual desktop: Windows 11's key, then Windows 10's (per
/// session), each that exists, with its path (for the log). The watcher listens to all of them:
/// with one desktop the shell writes no `CurrentVirtualDesktop` until his first switch (seen in
/// the VM, Windows 11 26300), and which key that first write goes to isn't known beforehand.
fn desktops_keys() -> Vec<(String, HKEY)> {
    let mut sid = 0u32;
    let _ = unsafe { ::windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(::windows::Win32::System::Threading::GetCurrentProcessId(), &mut sid) };
    [
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops".to_string(),
        format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\SessionInfo\{sid}\VirtualDesktops"),
    ]
    .into_iter()
    .filter_map(|p| {
        let mut key = HKEY::default();
        let path = ::windows::core::HSTRING::from(p.as_str());
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_READ, &mut key) }.ok().ok()?;
        Some((p, key))
    })
    .collect()
}

/// The current virtual desktop: the first key that holds it.
fn current_desktop_in(keys: &[HKEY]) -> Option<[u8; 16]> {
    keys.iter().find_map(|&k| current_desktop(k))
}

/// The current virtual desktop's id, as the shell stores it.
fn current_desktop(key: HKEY) -> Option<[u8; 16]> {
    let (mut buf, mut n) = ([0u8; 16], 16u32);
    unsafe { RegGetValueW(key, PCWSTR::null(), ::windows::core::w!("CurrentVirtualDesktop"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut n as *mut u32)) }
        .is_ok()
        .then_some(buf)
}

/// Monitors added, removed, rearranged or set to another mode, the work area changed (the
/// taskbar moved or resized), a DPI changed: `Hint::Layout`, on the watcher's thread.
unsafe extern "system" fn layout_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED || (msg == WM_SETTINGCHANGE && wp.0 as u32 == SPI_SETWORKAREA.0) {
        let why = match msg {
            WM_DISPLAYCHANGE => "monitors changed",
            WM_DPICHANGED => "a DPI changed",
            _ => "the work area changed",
        };
        elog!("synaps-avatar: windows: layout: {why}: re-read");
        HINT.with(|f| {
            if let Some(f) = f.borrow().as_ref() {
                f(Hint::Layout);
            }
        });
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

fn root_rect(r: &RECT, w: WinId) -> WinRect {
    let (ox, oy) = origin();
    WinRect { id: w, x: r.left - ox, y: r.top - oy, w: r.right - r.left, h: r.bottom - r.top }
}

fn cloaked(h: HWND) -> bool {
    let mut c: u32 = 0;
    unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut c as *mut u32 as *mut _, 4).is_ok() && c != 0 }
}

/// What shows of a top-level window: DWM's frame bounds (GetWindowRect counts the invisible
/// resize borders, 7 px left, right and bottom on Windows 11).
fn frame_rect(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32).is_err() {
            GetWindowRect(h, &mut r).ok()?;
        }
    }
    Some(r)
}

/// On screen: visible, not minimised, not cloaked (another virtual desktop, a suspended app).
fn showing(h: HWND) -> bool {
    unsafe { IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() && !cloaked(h) }
}

fn ex_style(h: HWND) -> u32 {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 }
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The desktop itself (the shell's wallpaper windows): never a window to sit on or be under.
fn is_shell(h: HWND) -> bool {
    h == unsafe { GetShellWindow() } || matches!(class_name(h).as_str(), "Progman" | "WorkerW")
}

/// The shell's processes: explorer and its out-of-process hosts (Start, Search, the
/// notification centre and Quick Settings, the touch keyboard, the lock screen).
const SHELL_EXES: &[&str] = &[
    "explorer.exe",
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "SearchApp.exe",
    "SearchUI.exe",
    "ShellExperienceHost.exe",
    "ShellHost.exe",
    "TextInputHost.exe",
    "LockApp.exe",
];

/// The shell's windows known by their class: the taskbars, Start's, Search's and the flyouts'
/// hosts, Alt+Tab and Task View.
const SHELL_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "XamlExplorerHostIslandWindow",
    "XamlExplorerHostIslandWindow_WASDK",
    "Windows.UI.Core.CoreWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "NotifyIconOverflowWindow",
    "MultitaskingViewFrame",
    "TaskSwitcherWnd",
    "ForegroundStaging",
];

/// A window of class `class` (a tool window or not) of process `exe` is the shell's own UI: the
/// shell's processes only (every UWP app has a `Windows.UI.Core.CoreWindow` too, top level when it
/// goes full screen), and of theirs a class known or any tool window (a flyout whose class a
/// Windows update renamed: class lists go stale, the tool-window rule doesn't).
fn shell_ui(class: &str, tool: bool, exe: &str) -> bool {
    SHELL_EXES.iter().any(|e| exe.eq_ignore_ascii_case(e)) && (tool || SHELL_CLASSES.contains(&class))
}

/// The shell's own UI: the desktop, the taskbars, and its flyouts and switchers (Start, Search,
/// the notification centre, Alt+Tab, Task View). In front, it is never his window nor a
/// fullscreen app: Start's host covers the whole monitor (on Haseeb's box she went
/// "fullscreen-hide" every time he opened Start or Search, and followed the Alt+Tab switcher).
/// The process is only asked for a window that could be the shell's (a known class or a tool
/// window).
fn is_shell_ui(h: HWND) -> bool {
    if is_shell(h) {
        return true;
    }
    let class = class_name(h);
    let tool = ex_style(h) & WS_EX_TOOLWINDOW.0 != 0;
    (tool || SHELL_CLASSES.contains(&class.as_str())) && shell_ui(&class, tool, &exe_of(h))
}

/// It covers its whole monitor (a borderless full screen). Tool windows never count (overlays,
/// the shell's hosts).
fn covers_monitor(h: HWND) -> bool {
    if ex_style(h) & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }
    frame_rect(h).zip(monitor_info(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) })).is_some_and(|(r, mi)| {
        let m = mi.monitorInfo.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    })
}

fn monitor_info(m: HMONITOR) -> Option<MONITORINFOEXW> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    unsafe { GetMonitorInfoW(m, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool().then_some(mi) }
}

unsafe extern "system" fn collect_monitor(m: HMONITOR, _dc: HDC, _r: *mut RECT, out: LPARAM) -> BOOL {
    let out = unsafe { &mut *(out.0 as *mut Vec<HMONITOR>) };
    out.push(m);
    BOOL(1)
}

fn hmonitors() -> Vec<HMONITOR> {
    let mut v: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut v as *mut Vec<HMONITOR> as isize));
    }
    v
}

unsafe extern "system" fn collect_window(h: HWND, out: LPARAM) -> BOOL {
    let out = unsafe { &mut *(out.0 as *mut Vec<HWND>) };
    out.push(h);
    BOOL(1)
}

/// Every top-level window, top → bottom.
fn toplevels() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_window), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

fn region(rects: &[ShapeRect]) -> Option<::windows::Win32::Graphics::Gdi::HRGN> {
    unsafe {
        let all = CreateRectRgn(0, 0, 0, 0);
        for &(x, y, w, h) in rects {
            let r = CreateRectRgn(x as i32, y as i32, x as i32 + w as i32, y as i32 + h as i32);
            CombineRgn(Some(all), Some(all), Some(r), RGN_OR);
            let _ = DeleteObject(r.into());
        }
        Some(all)
    }
}

/// The poller's wake-up to one of her click-through surfaces (`Pass`): a message of its own,
/// registered (no clash with GPUI's or anyone's `WM_APP` range).
fn pass_msg() -> u32 {
    static MSG: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MSG.get_or_init(|| match unsafe { RegisterWindowMessageW(::windows::core::w!("SynapsAvatar.Pass")) } {
        0 => {
            elog!("synaps-avatar: windows: no registered message for click-through; WM_APP+0x5A1 instead");
            WM_APP + 0x5A1
        }
        m => m,
    })
}
const PASS_SUBCLASS: usize = 0x5A1;

/// The poller's wake-up to her body when she has lost her place on top (`back_on_top`).
fn top_msg() -> u32 {
    static MSG: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MSG.get_or_init(|| match unsafe { RegisterWindowMessageW(::windows::core::w!("SynapsAvatar.Top")) } {
        0 => WM_APP + 0x5A2,
        m => m,
    })
}

/// A window came forward (the WinEvent hook): the poller checks her place on its next tick.
static Z_CHECK: AtomicBool = AtomicBool::new(false);
/// Her place on top is checked at least this often (and on every `Z_CHECK`, at this pace).
const Z_EVERY: Duration = Duration::from_millis(250);
/// The slowest the checks get while her puts don't hold (`ZPace`).
const Z_MAX: Duration = Duration::from_secs(8);
/// A window she couldn't get above is left out of the checks this long, then tried again (one
/// bad reading never leaves her under his window for good). Found over her again, its next stay
/// out doubles, to `Z_CANT_MAX` (`cant_for`).
const Z_CANT_FOR: Duration = Duration::from_secs(60);
const Z_CANT_MAX: Duration = Duration::from_secs(600);
/// A wake-up posted and not handled for this long was lost (its window never took it): posted
/// again.
const REPOST_MS: u64 = 500;

/// Posts `msg` to `h` unless one is in flight (`flag`): at most one at a time, and one left
/// unhandled for `REPOST_MS` posted again. None: one is in flight; else whether it went out.
fn post_once(h: HWND, msg: u32, flag: &AtomicBool, at: &AtomicU64) -> Option<bool> {
    let now = tick();
    if flag.swap(true, Ordering::SeqCst) && now.saturating_sub(at.load(Ordering::SeqCst)) < REPOST_MS {
        return None;
    }
    at.store(now, Ordering::SeqCst);
    let posted = unsafe { PostMessageW(Some(h), msg, WPARAM(0), LPARAM(0)) }.is_ok();
    if !posted {
        // Not delivered (gone, or its queue full): the next tick posts again.
        flag.store(false, Ordering::SeqCst);
    }
    Some(posted)
}

/// Whether `h` takes the pointer now (no `WS_EX_TRANSPARENT`). Reading a style never waits.
fn takes_pointer(h: HWND) -> bool {
    ex_style(h) & WS_EX_TRANSPARENT.0 == 0
}

/// Click-through, one surface: the input poller decides, the surface's own thread (her UI thread)
/// sets the style. The poller never sets it itself: from another thread,
/// `SetWindowLongPtrW(GWL_EXSTYLE)` is a synchronous send (WM_STYLECHANGING/-CHANGED) to her UI
/// thread, and the poller made it holding `surfaces` while her UI thread, mid-frame, waited for
/// that lock in `set_input`. Each waited for the other for good: "synaps-avatar.exe is not
/// responding" (Haseeb's box, four dumps alike, 2026-11-19).
///
/// The wanted state lives here; the message is only a wake-up. At most one is in flight
/// (`queued`) and the latest `take` wins. The poller compares the real style every tick (10 ms):
/// a style changed behind its back is posted again, and so is a wake-up left unhandled for
/// `REPOST_MS`.
#[derive(Default)]
struct Pass {
    /// Wanted: she takes the pointer here (no `WS_EX_TRANSPARENT`).
    take: AtomicBool,
    /// A wake-up is posted and not handled yet; since when (`tick`).
    queued: AtomicBool,
    queued_at: AtomicU64,
    /// Its window is gone (WM_NCDESTROY): the poller lets the surface go.
    gone: AtomicBool,
    /// Her body: a "back on top" wake-up is posted and not handled yet; since when (`tick`).
    top: AtomicBool,
    top_at: AtomicU64,
    /// Her body, right after she was put back: a plain window still over her (0: none). One
    /// she can't get above (a higher band).
    still_under: AtomicU32,
    /// Wake-ups that went out and wake-ups handled (the regression test reads them, per window).
    #[cfg(test)]
    posts: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    wakes: std::sync::atomic::AtomicUsize,
    /// "Back on top" wake-ups handled.
    #[cfg(test)]
    tops: std::sync::atomic::AtomicUsize,
}

impl Pass {
    /// The poller's side: `h` should take the pointer, or let it through. Never waits (a post).
    /// Some(posted) when a wake-up was due; None when the style is already so, or one is queued.
    fn want(&self, h: HWND, take: bool) -> Option<bool> {
        self.take.store(take, Ordering::SeqCst);
        if takes_pointer(h) == take {
            return None;
        }
        // Counted before it goes (its handling never runs ahead of the count), taken back if not.
        #[cfg(test)]
        self.posts.fetch_add(1, Ordering::SeqCst);
        let r = post_once(h, pass_msg(), &self.queued, &self.queued_at);
        #[cfg(test)]
        if r != Some(true) {
            self.posts.fetch_sub(1, Ordering::SeqCst);
        }
        r
    }

    /// The poller's side: her body back on top (at most one in flight, a lost one posted again).
    /// None: one is in flight; else whether it went out.
    fn want_top(&self, h: HWND) -> Option<bool> {
        post_once(h, top_msg(), &self.top, &self.top_at)
    }

    /// The surface's side, on its own thread: `queued` cleared first (a later `want` posts
    /// again), then the latest `take` set, only if the style differs.
    fn apply(&self, h: HWND) {
        self.queued.store(false, Ordering::SeqCst);
        #[cfg(test)]
        self.wakes.fetch_add(1, Ordering::SeqCst);
        let ex = ex_style(h);
        let want = if self.take.load(Ordering::SeqCst) { ex & !WS_EX_TRANSPARENT.0 } else { ex | WS_EX_TRANSPARENT.0 };
        if want != ex {
            unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, want as isize) };
        }
    }
}

/// The subclass on her click-through surfaces: `Pass::apply` on the poller's wake-up. `data` is
/// the surface's `Pass` (an `Arc` given up to it in `adopt_hwnd`, taken back at WM_NCDESTROY).
unsafe extern "system" fn pass_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, data: usize) -> LRESULT {
    if msg == top_msg() {
        if data != 0 {
            let pass = unsafe { &*(data as *const Pass) };
            #[cfg(test)]
            pass.tops.fetch_add(1, Ordering::SeqCst);
            back_on_top();
            // Her windows are all topmost now: any plain window over her is above the band.
            let still = under_plain_window(h, |_| false, |_| false).map_or(0, id);
            pass.still_under.store(still, Ordering::SeqCst);
            pass.top.store(false, Ordering::SeqCst);
        }
        return LRESULT(0);
    }
    if msg == pass_msg() {
        if data != 0 {
            unsafe { &*(data as *const Pass) }.apply(h);
        }
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(h, Some(pass_proc), PASS_SUBCLASS);
            if data != 0 {
                let pass = Arc::from_raw(data as *const Pass);
                pass.gone.store(true, Ordering::SeqCst);
            }
        }
    }
    unsafe { DefSubclassProc(h, msg, wp, lp) }
}

/// Her body's place on top is lost: the first window above her that isn't topmost (none above a
/// topmost window ever is), showing and not hers. Windows does this to topmost windows now and
/// then, keeping their `WS_EX_TOPMOST` bit: on Haseeb's box (2026-11-19, Windows 11 26200) she,
/// the second monitor's taskbar and the shell's WASDK host were all found under plain windows
/// after he opened an app from Explorer, with no call of hers in between.
/// `skip`: windows she can't get above (a higher band: Task Manager's "always on top").
fn under_plain_window(body: HWND, mine: impl Fn(WinId) -> bool, skip: impl Fn(WinId) -> bool) -> Option<HWND> {
    if !showing(body) {
        return None;
    }
    let mut w = body;
    for _ in 0..4096 {
        w = unsafe { GetWindow(w, GW_HWNDPREV) }.ok()?;
        if ex_style(w) & WS_EX_TOPMOST.0 == 0 && showing(w) && !skip(id(w)) && !mine(id(w)) {
            return Some(w);
        }
    }
    None
}

/// Her windows (this thread's surfaces: showing tool windows, as `adopt` makes every one; a
/// dialog of hers, were there one, is left alone) back to the top of the topmost band, in the
/// order they stand among themselves (her Line and bubble stay over her body). Not by her
/// topmost bit: Windows may have cleared it. On her UI thread, posted by the poller
/// (`top_msg`): her own windows, so nothing here waits on another thread.
fn back_on_top() {
    let me = unsafe { ::windows::Win32::System::Threading::GetCurrentThreadId() };
    let mut hers = Vec::new();
    let mut w = unsafe { GetTopWindow(None) }.ok();
    for _ in 0..8192 {
        let Some(h) = w else { break };
        if unsafe { GetWindowThreadProcessId(h, None) } == me && showing(h) && ex_style(h) & WS_EX_TOOLWINDOW.0 != 0 {
            hers.push(h);
        }
        w = unsafe { GetWindow(h, GW_HWNDNEXT) }.ok();
    }
    for &h in hers.iter().rev() {
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
    }
}

/// A process's file name (its exe), for her log.
fn exe_of(h: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&raw mut pid)) };
    let name = unsafe { ::windows::Win32::System::Threading::OpenProcess(::windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok().and_then(|p| {
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let ok = unsafe { ::windows::Win32::System::Threading::QueryFullProcessImageNameW(p, ::windows::Win32::System::Threading::PROCESS_NAME_WIN32, ::windows::core::PWSTR(buf.as_mut_ptr()), &mut n) }.is_ok();
        unsafe {
            let _ = ::windows::Win32::Foundation::CloseHandle(p);
        }
        ok.then(|| String::from_utf16_lossy(&buf[..n as usize]))
    });
    name.and_then(|n| n.rsplit('\\').next().map(str::to_string)).unwrap_or_else(|| format!("pid {pid}"))
}

/// A fullscreen app: showing, not the shell's UI, and covering its whole monitor (a borderless
/// full screen), or (`ask_shell`) in front while the shell reports a D3D full screen or
/// presentation mode (asked only when it doesn't cover its monitor). Never `ask_shell` from the
/// poller: that call is answered by explorer, and a hung explorer would stall the poller and her
/// click-through with it (Shady on #85).
fn fullscreen(h: HWND, ask_shell: bool) -> bool {
    if h.0.is_null() || is_shell_ui(h) || !showing(h) {
        return false;
    }
    if covers_monitor(h) {
        return true;
    }
    if !ask_shell {
        return false;
    }
    let state = unsafe { SHQueryUserNotificationState() };
    matches!(state, Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE) && unsafe { GetForegroundWindow() } == h
}

/// A fullscreen app in front (a game, a video, a slideshow): Windows may put it over topmost
/// windows, and the core hides her there anyway (`fullscreen-hide`). The poller's: the shell
/// isn't asked (`fullscreen`).
fn fullscreen_in_front() -> bool {
    fullscreen(unsafe { GetForegroundWindow() }, false)
}

/// How long a window she can't get above is left out, the `n`th time it's found over her:
/// `Z_CANT_FOR`, doubled each time, to `Z_CANT_MAX` (a window above the topmost band for good
/// isn't re-tried every minute all day).
fn cant_for(n: u32) -> Duration {
    Z_CANT_FOR.saturating_mul(1u32 << n.clamp(1, 8).saturating_sub(1)).min(Z_CANT_MAX)
}

/// One of her click-through surfaces: its input region (window px) and its click-through state.
struct Surface {
    input: Vec<ShapeRect>,
    pass: Arc<Pass>,
}

/// A lock even if a panic poisoned it (her locks hold plain values, whole after any panic): said
/// loudly, once a lock, and carried on (a silent end of the poller would leave her click-through stuck).
/// Release builds abort on a panic; this is for the rest.
fn relock<'a, T>(m: &'a Mutex<T>, what: &'static str) -> std::sync::MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| {
        // Once per lock (one list for every `T`).
        static SAID: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        let first = match SAID.lock() {
            Ok(said) if said.contains(&what) => false,
            Ok(mut said) => {
                said.push(what);
                true
            }
            Err(_) => true,
        };
        if first {
            elog!("synaps-avatar: windows: the {what} lock was poisoned (a panic while it was held); carrying on with its data");
        }
        m.clear_poison();
        e.into_inner()
    })
}

type Escapes = Arc<Mutex<Option<Box<dyn Fn() + Send>>>>;
/// How often the poller looks at her place on top, and whether her last put held: `ZWatch`'s
/// decisions, no Win32 in them (tested as they are).
#[derive(Clone, Copy, Debug, PartialEq)]
struct ZPace {
    every: Duration,
    /// A put went out and no check has found her on top since: under again means it didn't hold.
    unsettled: bool,
}

/// What a check does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZStep {
    /// She is on top (or a fullscreen app is in front): nothing to do.
    OnTop,
    /// Her UI thread hasn't taken the last put yet.
    Waiting,
    /// Put back, and still under that window right after: it is above the topmost band.
    Cant,
    /// Put her back on top.
    Put,
}

impl Default for ZPace {
    fn default() -> ZPace {
        ZPace { every: Z_EVERY, unsettled: false }
    }
}

impl ZPace {
    /// A check is due `since` the last one; `asked`: a window came forward. Backing off, that
    /// waits its turn too (else every menu, tooltip and toast would undo the backoff).
    fn due(&self, since: Duration, asked: bool) -> bool {
        since >= self.every || (asked && self.every <= Z_EVERY)
    }

    /// A check found her `under` a plain window, or not; `in_flight`: her UI thread hasn't taken
    /// the last put; `still_under_it`: right after its put, that very window was still over her.
    /// A put that didn't hold until this check doubles the wait (to `Z_MAX`); on top, the pace is
    /// back to `Z_EVERY`.
    fn found(&mut self, under: bool, in_flight: bool, still_under_it: bool) -> ZStep {
        if !under {
            *self = ZPace::default();
            return ZStep::OnTop;
        }
        if in_flight {
            return ZStep::Waiting;
        }
        if still_under_it {
            self.unsettled = false;
            return ZStep::Cant;
        }
        if self.unsettled {
            self.every = (self.every * 2).min(Z_MAX);
        }
        ZStep::Put
    }

    /// The put went out.
    fn put(&mut self) {
        self.unsettled = true;
    }
}

/// The poller's watch on her place on top (`ZPace` decides): a check every `Z_EVERY` and on every
/// window that comes forward (`Z_CHECK`); out of the topmost band, her UI thread is asked to put
/// her back (`back_on_top`) and her log says under what. While her puts don't hold, the checks
/// slow down (8 s at most) until one finds her on top. A window still over her right after she
/// was put back (her UI thread says, `Pass::still_under`) is above the topmost band itself, and
/// so is every plain window over it (bands stack): all left out for `Z_CANT_FOR`.
struct ZWatch {
    last: std::time::Instant,
    pace: ZPace,
    puts: u64,
    /// Windows she can't get above: since when, for how long (`cant_for`).
    cant: HashMap<WinId, (std::time::Instant, Duration)>,
    /// Those her log has named (once a window), and how often each was found over her.
    said: HashMap<WinId, u32>,
    /// A fullscreen app in front (`fullscreen_in_front`; a test's own).
    front_fullscreen: fn() -> bool,
}

impl Default for ZWatch {
    fn default() -> ZWatch {
        ZWatch {
            last: std::time::Instant::now(),
            pace: ZPace::default(),
            puts: 0,
            cant: HashMap::new(),
            said: HashMap::new(),
            front_fullscreen: fullscreen_in_front,
        }
    }
}

impl ZWatch {
    fn tick(&mut self, body: &AtomicU32, surfaces: &Mutex<HashMap<WinId, Surface>>, mine: &Mutex<HashSet<WinId>>) {
        let asked = Z_CHECK.swap(false, Ordering::Relaxed);
        if !self.pace.due(self.last.elapsed(), asked) {
            return;
        }
        self.last = std::time::Instant::now();
        let b = body.load(Ordering::Relaxed);
        if b == 0 {
            return;
        }
        let Some(pass) = relock(surfaces, "surfaces").get(&b).map(|s| s.pass.clone()) else { return };
        let alive = |w: WinId| unsafe { IsWindow(Some(hwnd(w))) }.as_bool();
        self.cant.retain(|&w, (at, stay)| alive(w) && at.elapsed() < *stay);
        self.said.retain(|&w, _| alive(w));
        let is_mine = |w: WinId| mine.lock().is_ok_and(|m| m.contains(&w));
        let front = self.front_fullscreen;
        let over = under_plain_window(hwnd(b), is_mine, |w| self.cant.contains_key(&w)).filter(|_| !front());
        let in_flight = pass.top.load(Ordering::SeqCst);
        // Her UI thread's word on its last put, read once (a stale one never names a later window).
        let reported = if in_flight { 0 } else { pass.still_under.swap(0, Ordering::SeqCst) };
        match self.pace.found(over.is_some(), in_flight, over.is_some_and(|o| reported == id(o))) {
            ZStep::OnTop => {}
            ZStep::Waiting => {
                // A wake-up left unhandled for `REPOST_MS` goes again.
                let _ = pass.want_top(hwnd(b));
            }
            ZStep::Cant => {
                let mut o = over;
                while let Some(w) = o {
                    let n = self.said.entry(id(w)).or_insert(0);
                    *n += 1;
                    if *n == 1 {
                        elog!("synaps-avatar: z: can't get above {} of {} (0x{:x}): left there", class_name(w), exe_of(w), id(w));
                    }
                    self.cant.insert(id(w), (std::time::Instant::now(), cant_for(*n)));
                    o = under_plain_window(w, is_mine, |x| self.cant.contains_key(&x));
                }
            }
            ZStep::Put => {
                if pass.want_top(hwnd(b)) == Some(true) {
                    self.pace.put();
                    self.puts += 1;
                    if let Some(o) = over
                        && (self.puts <= 20 || self.puts.is_multiple_of(100))
                    {
                        elog!("synaps-avatar: z: re-topmost (was below {} of {}, 0x{:x}; #{})", class_name(o), exe_of(o), id(o), self.puts);
                    }
                }
            }
        }
    }
}

/// The poller's word for one surface: its id, its state, take the pointer, where the pointer is
/// (window px), how many input rects.
type Want = (WinId, Arc<Pass>, bool, (i32, i32), usize);

/// The pointer for the input poller: screen px, and whether a button is held (None: unread).
type PointerSource = Box<dyn Fn() -> Option<(POINT, bool)> + Send>;
/// A window and where it goes (screen px: x, y, w, h).
type Placer = std::sync::mpsc::Sender<(WinId, [i32; 4])>;

pub struct Win32 {
    /// Her body's window (0 until adopted).
    body: Arc<AtomicU32>,
    /// The last window of his that had the foreground (the shell's UI in front doesn't count).
    last_app: AtomicU32,
    /// Every surface of hers (left out of `windows`, no hints).
    mine: Arc<Mutex<HashSet<WinId>>>,
    /// Her click-through surfaces (body, intro), by id.
    surfaces: Arc<Mutex<HashMap<WinId, Surface>>>,
    input_thread: Mutex<Option<std::thread::Thread>>,
    clicks: Arc<AtomicBool>,
    escape: Arc<AtomicBool>,
    on_escape: Escapes,
    click_thread: Mutex<Option<std::thread::Thread>>,
    /// When he last gave her his input (`tick` ms; 0: not since the keyboard went back).
    grant_at: Arc<AtomicU64>,
    /// Her effect layers' windows, by layer.
    fx: Mutex<Vec<fx::FxWin>>,
    /// The box (GPUI logical px, root px ÷ her scale: x, y, w, h) in the window options `dress`
    /// saw last, per role. GPUI opens a window whose box isn't on the primary monitor in the
    /// middle of it at a default size, and counts from the primary's top-left, not the virtual
    /// screen's: `adopt` puts it where it was asked for, at its size.
    asked_at: Mutex<Vec<(Role, [f32; 4])>>,
    /// The thread that resizes her GPUI windows (`place_later`).
    later: Mutex<Option<Placer>>,
}

thread_local! {
    /// The watcher's hint sink (WinEvent callbacks run on the thread that set the hooks).
    static HINT: std::cell::RefCell<Option<HintFn>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn win_event(_h: HWINEVENTHOOK, ev: u32, w: HWND, obj: i32, child: i32, _tid: u32, _t: u32) {
    // Windows themselves (not carets, cursors, scroll bars), top-level ones.
    if obj != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || w.0.is_null() || unsafe { GetAncestor(w, GA_ROOT) } != w {
        return;
    }
    if matches!(ev, EVENT_SYSTEM_FOREGROUND | EVENT_OBJECT_SHOW | EVENT_SYSTEM_MINIMIZEEND | EVENT_OBJECT_UNCLOAKED) {
        // Windows came forward: is she still on top of them? (`under_plain_window`, the poller.)
        Z_CHECK.store(true, Ordering::Relaxed);
    }
    let hint = match ev {
        EVENT_OBJECT_LOCATIONCHANGE if showing(w) => Hint::Moved(id(w), frame_rect(w).map(|r| root_rect(&r, id(w)))),
        EVENT_OBJECT_SHOW | EVENT_SYSTEM_MINIMIZEEND | EVENT_OBJECT_UNCLOAKED => Hint::Mapped(id(w)),
        EVENT_OBJECT_HIDE | EVENT_OBJECT_DESTROY | EVENT_SYSTEM_MINIMIZESTART | EVENT_OBJECT_CLOAKED => Hint::Gone(id(w)),
        EVENT_SYSTEM_FOREGROUND => Hint::Focus,
        _ => return,
    };
    HINT.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(hint);
        }
    });
}

type HintFn = Box<dyn Fn(Hint) + Send>;
type SessionFn = Box<dyn Fn(SessionEv) + Send>;

thread_local! {
    /// The session watcher's sink, and whether the machine is asleep (resume comes twice).
    static SESSION: std::cell::RefCell<Option<(SessionFn, bool)>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn session_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // WM_WTSSESSION_CHANGE: WTS_SESSION_LOCK 7, _UNLOCK 8. WM_POWERBROADCAST: PBT_APMSUSPEND 4,
    // PBT_APMRESUMESUSPEND 7, PBT_APMRESUMEAUTOMATIC 0x12.
    let ev = match (msg, wp.0) {
        (0x02B1, 7) => Some(SessionEv::Locked),
        (0x02B1, 8) => Some(SessionEv::Unlocked),
        (0x0218, 4) => Some(SessionEv::Sleeping),
        (0x0218, 7 | 0x12) => Some(SessionEv::Resumed),
        _ => None,
    };
    if let Some(ev) = ev {
        SESSION.with(|s| {
            if let Some((f, asleep)) = s.borrow_mut().as_mut() {
                let dup = match ev {
                    SessionEv::Sleeping => std::mem::replace(asleep, true),
                    SessionEv::Resumed => !std::mem::replace(asleep, false),
                    _ => false,
                };
                if !dup {
                    f(ev);
                }
            }
        });
        if msg == 0x0218 {
            return LRESULT(1);
        }
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

impl Win32 {
    pub fn connect() -> Result<Win32> {
        if unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } <= 0 {
            bail!("windows: no desktop (a service session?)");
        }
        Ok(Win32 {
            body: Arc::new(AtomicU32::new(0)),
            last_app: AtomicU32::new(0),
            mine: Arc::new(Mutex::new(HashSet::new())),
            surfaces: Arc::new(Mutex::new(HashMap::new())),
            input_thread: Mutex::new(None),
            clicks: Arc::new(AtomicBool::new(false)),
            escape: Arc::new(AtomicBool::new(false)),
            on_escape: Arc::new(Mutex::new(None)),
            click_thread: Mutex::new(None),
            grant_at: Arc::new(AtomicU64::new(0)),
            fx: Mutex::new(Vec::new()),
            asked_at: Mutex::new(Vec::new()),
            later: Mutex::new(None),
        })
    }

    fn body_hwnd(&self) -> HWND {
        hwnd(self.body.load(Ordering::Relaxed))
    }

    fn mine(&self, h: HWND) -> bool {
        self.mine.lock().is_ok_and(|m| m.contains(&id(h)))
    }

    /// Make `h` one of her surfaces: a bare topmost tool window. Click-through surfaces (body,
    /// intro, effect hosts) are layered and let the pointer through until their input region
    /// says otherwise; none of them takes the activation from a click (GPUI PW-2: they open
    /// without focus).
    fn adopt_hwnd(&self, role: Role, h: HWND) -> Result<WinId> {
        let through = matches!(role, Role::Body | Role::Intro | Role::Fx);
        // Lets the pointer through (WS_EX_TRANSPARENT below) until the poller says otherwise.
        let pass = through.then(|| Arc::new(Pass::default()));
        let asked = self.asked_at.lock().ok().and_then(|mut a| a.iter().position(|e| e.0 == role).map(|i| a.swap_remove(i).1));
        unsafe {
            // The client area becomes the whole window: where and as big as the core asked for it
            // (else as GPUI put it).
            let (mut c, mut at) = (RECT::default(), POINT::default());
            GetClientRect(h, &mut c)?;
            let _ = ::windows::Win32::Graphics::Gdi::ClientToScreen(h, &mut at);
            if let Some([lx, ly, lw, lh]) = asked {
                let (s, (ox, oy)) = (primary_scale(), origin());
                at = POINT { x: (lx * s).round() as i32 + ox, y: (ly * s).round() as i32 + oy };
                c = RECT { left: 0, top: 0, right: (lw * s).round().max(1.0) as i32, bottom: (lh * s).round().max(1.0) as i32 };
            }
            // GPUI PW-1 creates a pop-up as a bare WS_POPUP; one with a caption (an unpatched
            // GPUI) is made one here.
            // The subclass before any style is touched (the poller's wake-ups are handled from the
            // first one on). A subclass is the window thread's: adopt runs on it.
            debug_assert_eq!(
                GetWindowThreadProcessId(h, None),
                ::windows::Win32::System::Threading::GetCurrentThreadId(),
                "adopt 0x{:x} on its own thread",
                id(h)
            );
            if let Some(pass) = &pass {
                let data = Arc::into_raw(pass.clone()) as usize;
                if !SetWindowSubclass(h, Some(pass_proc), PASS_SUBCLASS, data).as_bool() {
                    drop(Arc::from_raw(data as *const Pass));
                    bail!("windows: cannot subclass 0x{:x} (click-through)", id(h));
                }
            }
            let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
            if style & WS_CAPTION.0 != 0 {
                SetWindowLongPtrW(h, GWL_STYLE, (WS_POPUP.0 | (style & WS_VISIBLE.0)) as isize);
            }
            let mut ex = (ex_style(h) | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) & !WS_EX_APPWINDOW.0;
            if through {
                ex |= WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
            }
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize);
            if through {
                // A layered window shows nothing until it has attributes; DirectComposition's
                // per-pixel alpha still applies under a constant 255.
                SetLayeredWindowAttributes(h, ::windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA)?;
            }
            let set = |attr, v: u32| DwmSetWindowAttribute(h, attr, &v as *const u32 as *const _, 4).ok();
            set(DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED.0 as u32);
            set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
            set(DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
            SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
            let mut wr = RECT::default();
            GetWindowRect(h, &mut wr)?;
            if (wr.right - wr.left, wr.bottom - wr.top) != (c.right, c.bottom) {
                self.place_later(id(h), at.x, at.y, c.right, c.bottom);
            } else if (wr.left, wr.top) != (at.x, at.y) {
                SetWindowPos(h, None, at.x, at.y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)?;
            }
        }
        let w = id(h);
        if let Ok(mut m) = self.mine.lock() {
            m.insert(w);
        }
        if let Some(pass) = pass {
            relock(&self.surfaces, "surfaces").insert(w, Surface { input: Vec::new(), pass });
            self.start_input_poller();
        }
        if role == Role::Body {
            self.body.store(w, Ordering::Relaxed);
        }
        Ok(w)
    }

    /// The pointer poller behind click-through: each surface lets the pointer through
    /// (`WS_EX_TRANSPARENT`) while it is off her input region. A held button freezes it (the
    /// window under the press keeps the pointer, as X's implicit grab does).
    fn start_input_poller(&self) {
        self.start_input_poller_with(Box::new(|| {
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p, buttons_down()))
        }));
    }

    /// The poller, reading the pointer (screen px, a button held) from `pointer` (a test's own).
    /// One per backend: later calls do nothing.
    fn start_input_poller_with(&self, pointer: PointerSource) {
        let mut g = relock(&self.input_thread, "input thread");
        if g.is_some() {
            return;
        }
        let (surfaces, mine, grant, body) = (self.surfaces.clone(), self.mine.clone(), self.grant_at.clone(), self.body.clone());
        let trace = std::env::var_os("SA_WIN_TRACE").is_some();
        let t = std::thread::Builder::new().name("win-input".into()).spawn(move || {
            let mut was_pressed = false;
            let mut z = ZWatch::default();
            loop {
                z.tick(&body, &surfaces, &mine);
                let at = pointer();
                let pressed = at.is_some_and(|(_, down)| down);
                // His press on one of her windows (the Line too): the keyboard is his to give.
                if pressed && !was_pressed && hers_at_pointer(&mine) {
                    grant.store(tick(), Ordering::Relaxed);
                }
                was_pressed = pressed;
                if let Some((p, false)) = at {
                    // Decided under the lock; told after it. Nothing here waits on her UI thread
                    // (it may be waiting for this lock): `Pass::want` only stores and posts.
                    let mut s = relock(&surfaces, "surfaces");
                    s.retain(|_, surf| !surf.pass.gone.load(Ordering::SeqCst));
                    let wants: Vec<Want> = s
                        .iter()
                        .filter_map(|(&w, surf)| {
                            let mut r = RECT::default();
                            unsafe { GetWindowRect(hwnd(w), &mut r) }.ok()?;
                            let (lx, ly) = (p.x - r.left, p.y - r.top);
                            let inside = surf.input.iter().any(|&(x, y, w, h)| lx >= x as i32 && ly >= y as i32 && lx < x as i32 + w as i32 && ly < y as i32 + h as i32);
                            Some((w, surf.pass.clone(), inside, (lx, ly), surf.input.len()))
                        })
                        .collect();
                    drop(s);
                    for (w, pass, inside, (lx, ly), n) in wants {
                        let posted = pass.want(hwnd(w), inside);
                        if trace && let Some(posted) = posted {
                            elog!("synaps-avatar: windows: 0x{w:x} takes the pointer: {inside} at {lx},{ly} ({n} input rects){}", if posted { "" } else { " (not posted)" });
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        match t {
            Ok(t) => *g = Some(t.thread().clone()),
            Err(e) => elog!("synaps-avatar: windows: no input poller (her click-through is stuck): {e}"),
        }
    }

    /// Place her GPUI window `s` (screen px) from another thread, `SWP_ASYNCWINDOWPOS`: her
    /// window's thread takes it from its queue between GPUI's updates, as GPUI's own `resize`
    /// does. A synchronous resize comes back into GPUI while the core's update holds it, and GPUI
    /// would keep laying her out at the old size. In order, per window.
    fn place_later(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) {
        let Ok(mut g) = self.later.lock() else { return };
        let tx = g.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel::<(WinId, [i32; 4])>();
            let spawned = std::thread::Builder::new().name("win-place".into()).spawn(move || {
                for (s, [x, y, w, h]) in rx {
                    unsafe {
                        let _ = SetWindowPos(hwnd(s), None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
                    }
                }
            });
            if let Err(e) = spawned {
                elog!("synaps-avatar: windows: no placing thread: {e}");
            }
            tx
        });
        tx.send((s, [x, y, w.max(1), h.max(1)])).ok();
    }

    fn set_input(&self, s: WinId, input: &[ShapeRect]) {
        let known = relock(&self.surfaces, "surfaces").get_mut(&s).map(|surf| surf.input = input.to_vec()).is_some();
        // Said once the lock is let go (her log may block on a full pipe).
        if !known && std::env::var_os("SA_WIN_TRACE").is_some() {
            elog!("synaps-avatar: windows: input for 0x{s:x}, not a surface of hers");
        }
    }

    fn set_visible(&self, s: WinId, visible: Visible<'_>) {
        let rgn = match visible {
            Visible::All => None,
            Visible::Rects(r) => region(r),
        };
        // The window owns the region from here (also on failure: nothing to free then).
        unsafe { SetWindowRgn(hwnd(s), rgn, true) };
    }

    /// One of her windows has the foreground (her process may then move it among them).
    /// `active_window` with `h` in front. Her tray's window counts as the shell's UI: it takes
    /// the foreground for its menu (`tray::show_menu`), so a pick there that opens the Line
    /// (SETTINGS) would else make it his last window, and Esc would hand his keyboard to a
    /// hidden 0×0 window.
    fn active_of(&self, h: HWND) -> Option<WinId> {
        if h.0.is_null() || is_shell(h) {
            return None;
        }
        if !is_shell_ui(h) && !tray::is_tray(h) {
            self.last_app.store(id(h), Ordering::Relaxed);
            return Some(id(h));
        }
        let last = self.last_app.load(Ordering::Relaxed);
        (last != 0 && showing(hwnd(last))).then_some(last)
    }

    fn hers_in_front(&self) -> bool {
        let fg = unsafe { GetForegroundWindow() };
        !fg.0.is_null() && self.mine(unsafe { GetAncestor(fg, GA_ROOT) })
    }

    fn arm(&self) {
        if let Ok(g) = self.click_thread.lock()
            && let Some(t) = g.as_ref()
        {
            t.unpark();
        }
    }
}

impl Desktop for Win32 {
    /// DWM always composites; GPUI presents every redraw through DirectComposition. Effect
    /// layers reaching out of her window get windows of their own (`fx.rs`). The WinEvent hook
    /// reports moves, hides and shows of every window. Display and work-area changes are hinted,
    /// and the core still re-reads the layout on its timer (a missed broadcast heals). The
    /// keyboard comes only on his input (the foreground lock).
    fn caps(&self) -> Caps {
        Caps {
            shape_mode: false,
            target_events: true,
            fx_layers: true,
            costly_redraw: true,
            implicit_grab: true,
            goal_geometry: false,
            layout_events: false,
            host_per_output: false,
            keyboard_on_input: true,
        }
    }

    /// GPUI's primary display is the primary monitor (not the virtual screen).
    fn primary_display_px(&self) -> i32 {
        unsafe { GetSystemMetrics(SM_CXSCREEN) }
    }

    fn root_size(&self) -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) }
    }

    fn monitors(&self) -> Vec<Monitor> {
        hmonitors()
            .into_iter()
            .filter_map(monitor_info)
            .map(|mi| {
                let r = root_rect(&mi.monitorInfo.rcMonitor, 0);
                Monitor { primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0, x: r.x, y: r.y, w: r.w, h: r.h }
            })
            .collect()
    }

    /// Each monitor's work area (what the taskbar and app bars leave).
    fn work_areas(&self, mons: &[Monitor]) -> Vec<Monitor> {
        let (ox, oy) = origin();
        mons.iter()
            .map(|m| {
                let c = POINT { x: m.x + m.w / 2 + ox, y: m.y + m.h / 2 + oy };
                let work = monitor_info(unsafe { MonitorFromPoint(c, MONITOR_DEFAULTTONEAREST) }).map(|mi| root_rect(&mi.monitorInfo.rcWork, 0));
                match work {
                    Some(r) => Monitor { primary: m.primary, x: r.x, y: r.y, w: r.w, h: r.h },
                    None => m.clone(),
                }
            })
            .collect()
    }

    fn refresh_at(&self, x: i32, y: i32) -> Option<f32> {
        let (ox, oy) = origin();
        let mi = monitor_info(unsafe { MonitorFromPoint(POINT { x: x + ox, y: y + oy }, MONITOR_DEFAULTTONEAREST) })?;
        let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR(mi.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool() }.then_some(())?;
        (dm.dmDisplayFrequency > 1).then_some(dm.dmDisplayFrequency as f32)
    }

    /// Showing top-level windows bottom → top, hers and the desktop's left out. Managed: an
    /// app's window (no tool windows: menus, tooltips, the taskbar, pop-up toasts).
    fn windows(&self) -> Vec<Win> {
        let mut out: Vec<Win> = toplevels()
            .into_iter()
            .filter(|&h| showing(h) && !self.mine(h) && !is_shell(h) && on_current_desktop(h))
            .filter_map(|h| {
                let r = root_rect(&frame_rect(h)?, id(h));
                if r.w <= 0 || r.h <= 0 {
                    return None;
                }
                let ex = ex_style(h);
                let managed = ex & WS_EX_TOOLWINDOW.0 == 0 && ex & WS_EX_NOACTIVATE.0 == 0;
                Some(Win { rect: r, managed })
            })
            .collect();
        out.reverse();
        out
    }

    /// The foreground window; none on the desktop. With the shell's UI in front (Start, Search,
    /// Alt+Tab, a flyout) or her tray's window (`active_of`), his window is still the one he
    /// was in.
    fn active_window(&self) -> Option<WinId> {
        self.active_of(unsafe { GetForegroundWindow() })
    }

    fn focused_window(&self) -> Option<WinId> {
        self.active_window()
    }

    fn toplevel_of(&self, w: WinId) -> Option<WinId> {
        let r = unsafe { GetAncestor(hwnd(w), GA_ROOT) };
        (!r.0.is_null()).then(|| id(r))
    }

    fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        let h = hwnd(w);
        if !showing(h) || !on_current_desktop(h) {
            return None;
        }
        frame_rect(h).map(|r| root_rect(&r, w))
    }

    /// A minimised or cloaked window still exists.
    fn window_exists(&self, w: WinId) -> bool {
        unsafe { IsWindow(Some(hwnd(w))).as_bool() }
    }

    /// It covers its whole monitor (a borderless full screen), or it is in front while the
    /// shell reports a D3D full screen or presentation mode (`fullscreen`).
    fn is_fullscreen(&self, w: WinId) -> bool {
        fullscreen(hwnd(w), true)
    }

    fn pointer(&self) -> Result<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p)? };
        let (ox, oy) = origin();
        Ok((p.x - ox, p.y - oy))
    }

    fn pointer_reader(&self) -> Result<PointerRead> {
        Ok(Box::new(|| {
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p) }.ok()?;
            let (ox, oy) = origin();
            Some((p.x - ox, p.y - oy))
        }))
    }

    fn idle_ms(&self) -> Option<u32> {
        let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        unsafe { GetLastInputInfo(&mut li).as_bool() }.then(|| unsafe { ::windows::Win32::System::SystemInformation::GetTickCount() }.wrapping_sub(li.dwTime))
    }

    fn char_key_down(&self, ch: char) -> Result<bool> {
        let vk = unsafe { VkKeyScanW(ch as u16) };
        if vk == -1 {
            return Ok(false);
        }
        Ok(unsafe { GetAsyncKeyState((vk & 0xff) as i32) } < 0)
    }

    fn compositor_running(&self) -> bool {
        true
    }

    /// The session's lock (WTS session notifications) and the machine's sleep (power
    /// broadcasts), on a hidden window's thread.
    fn watch_session(&self, on: Box<dyn Fn(SessionEv) + Send>) -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        std::thread::Builder::new().name("avatar-session".into()).spawn(move || {
            SESSION.with(|s| *s.borrow_mut() = Some((on, false)));
            let r = (|| -> Result<HWND> {
                let inst = unsafe { ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
                let class = ::windows::core::w!("SynapsAvatarSession");
                let wc = WNDCLASSW { lpfnWndProc: Some(session_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
                unsafe { RegisterClassW(&wc) };
                // A real top-level window, never shown: message-only windows miss broadcasts.
                let h = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, class, class, WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None)? };
                unsafe { ::windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(h, 0)? };
                Ok(h)
            })();
            let ok = r.is_ok();
            tx.send(r.map(|_| ())).ok();
            if !ok {
                return;
            }
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                unsafe { DispatchMessageW(&msg) };
            }
        })?;
        rx.recv()?
    }

    /// RegisterHotKey on a thread of its own (presses only, `MOD_NOREPEAT`): the summon key and
    /// the toggle chord (`chord`). A chord another app holds is skipped with its name logged. A
    /// press is his input to her: the keyboard is hers to take for `GRANT_MS`. With `keys.hold`
    /// the summon key is a press and, polled from here until it is up, a release (`hold.rs`).
    fn watch_keys(&self, keys: Keys, on: Box<dyn Fn(KeyEv) + Send>) -> Result<bool> {
        let wanted = [(KeyEv::Summon, chord(keys.summon, false)), (KeyEv::TalkToggle, chord(keys.toggle, true))];
        let grant = self.grant_at.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        std::thread::Builder::new().name("avatar-keys".into()).spawn(move || {
            let mut held = Vec::new();
            for (i, (ev, c)) in wanted.iter().enumerate() {
                let Some((mods, vk)) = *c else {
                    elog!("synaps-avatar: global keys: {ev:?}: no key types that letter here");
                    continue;
                };
                // A null HWND: WM_HOTKEY comes to this thread's queue.
                match unsafe { RegisterHotKey(None, i as i32 + 1, mods, vk) } {
                    Ok(()) => held.push((i as i32 + 1, *ev)),
                    Err(e) => elog!("synaps-avatar: global keys: {} is taken ({e}); `capri-ctl talk ...` still works", chord_name(mods, vk)),
                }
            }
            let any = !held.is_empty();
            tx.send(if any { Ok(()) } else { Err(anyhow::anyhow!("none registered")) }).ok();
            if !any {
                return;
            }
            // The summon key's letter, polled while a hold is down.
            let hold_vk = wanted[0].1.filter(|_| keys.hold).map(|(_, vk)| vk);
            let mut hold = hold::Hold::default();
            let mut msg = MSG::default();
            let hotkey = |msg: &MSG, hold: &mut hold::Hold| {
                if msg.message == WM_HOTKEY
                    && let Some((_, ev)) = held.iter().find(|(n, _)| *n as usize == msg.wParam.0)
                {
                    grant.store(tick(), Ordering::Relaxed);
                    match ev {
                        KeyEv::Summon if hold_vk.is_some() => {
                            if let Some(e) = hold.pressed() {
                                on(e);
                            }
                        }
                        ev => on(*ev),
                    }
                }
            };
            loop {
                if let (true, Some(vk)) = (hold.held(), hold_vk) {
                    // Held: a look every POLL, the messages in between.
                    std::thread::sleep(hold::POLL);
                    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                        if msg.message == WM_QUIT {
                            return;
                        }
                        hotkey(&msg, &mut hold);
                    }
                    if let Some(e) = hold.poll(unsafe { GetAsyncKeyState(vk as i32) } < 0) {
                        on(e);
                    }
                } else {
                    if !unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                        return;
                    }
                    hotkey(&msg, &mut hold);
                }
            }
        })?;
        rx.recv()??;
        Ok(true)
    }

    /// `WIN+ALT+C` for the summon key (`chord`).
    fn key_label(&self, ev: KeyEv, key: char) -> Option<String> {
        key_label(ev, key)
    }

    /// Her icon in the notification area (`tray.rs`). A click on it or a pick in its menu is his
    /// input to her, as her key is: the keyboard is hers to take for `GRANT_MS`.
    fn tray(&self, icon: super::TrayIcon, instance: &str, on: Box<dyn Fn(super::TrayEv) + Send>) -> Result<Option<Box<dyn super::Tray>>> {
        Ok(Some(Box::new(tray::start(icon, instance, self.grant_at.clone(), on)?)))
    }

    /// One thread: out-of-context WinEvent hooks (moves, shows, hides, minimise, cloak,
    /// foreground) on every window but hers, a hidden top-level window for the display
    /// broadcasts (`layout_proc`), and their message loop. Another blocks on the registry for
    /// virtual desktop switches (`Hint::Desktop`; the windows of the desktop he left are cloaked,
    /// which the hooks report as hidden).
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        let mine = self.mine.clone();
        let not_hers: HintFn = Box::new(move |h: Hint| {
            let w = match h {
                Hint::Moved(w, _) | Hint::Mapped(w) | Hint::Gone(w) => w,
                _ => 0,
            };
            if w == 0 || !mine.lock().is_ok_and(|m| m.contains(&w)) {
                hint(h);
            }
        });
        let hint = Arc::new(Mutex::new(not_hers));
        let call = move |h: Hint| {
            if let Ok(f) = hint.lock() {
                f(h);
            }
        };
        let desks = call.clone();
        std::thread::Builder::new().name("win-desktops".into()).spawn(move || {
            let found = desktops_keys();
            if found.is_empty() {
                elog!("synaps-avatar: windows: no virtual desktop in the registry; a switch shows as his windows hidden");
                return;
            }
            let keys: Vec<HKEY> = found.iter().map(|&(_, k)| k).collect();
            let holds = found.iter().find(|&&(_, k)| current_desktop(k).is_some()).map_or("none yet (one desktop)", |(p, _)| p.as_str());
            elog!("synaps-avatar: windows: virtual desktops: watching {} key(s); the current one is in: {holds}", keys.len());
            // One auto-reset event per key, armed again each time it fires (a notification fires once).
            let mut events = Vec::new();
            for _ in &keys {
                match unsafe { CreateEventW(None, false, false, PCWSTR::null()) } {
                    Ok(e) => events.push(e),
                    Err(e) => {
                        elog!("synaps-avatar: windows: virtual desktops: no event ({e}); a switch shows as his windows hidden");
                        return;
                    }
                }
            }
            let arm = |i: usize| unsafe { RegNotifyChangeKeyValue(keys[i], false, REG_NOTIFY_CHANGE_LAST_SET, Some(events[i]), true) }.is_ok();
            for (i, (path, _)) in found.iter().enumerate() {
                if !arm(i) {
                    elog!("synaps-avatar: windows: virtual desktops: cannot watch {path}; a switch shows as his windows hidden");
                    return;
                }
            }
            let mut now = current_desktop_in(&keys);
            // Blocks until a value under either key changes (his switch writes the current one).
            loop {
                let w = unsafe { WaitForMultipleObjects(&events, false, INFINITE) };
                let i = w.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
                if i >= events.len() || !arm(i) {
                    elog!("synaps-avatar: windows: virtual desktops: the watch ended ({:#x}); a switch shows as his windows hidden", w.0);
                    return;
                }
                let next = current_desktop_in(&keys);
                if next != now {
                    now = next;
                    elog!("synaps-avatar: windows: virtual desktop switch");
                    desks(Hint::Desktop);
                }
            }
        })?;
        std::thread::Builder::new().name("win-watch".into()).spawn(move || {
            HINT.with(|h| *h.borrow_mut() = Some(Box::new(call)));
            // A real top-level window, never shown: broadcasts skip message-only ones.
            unsafe {
                if let Ok(inst) = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None) {
                    let class = ::windows::core::w!("SynapsAvatarLayout");
                    let wc = WNDCLASSW { lpfnWndProc: Some(layout_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
                    RegisterClassW(&wc);
                    if CreateWindowExW(WS_EX_TOOLWINDOW, class, class, WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None).is_err() {
                        elog!("synaps-avatar: windows: no display-change window; the layout is re-read on its timer only");
                    }
                }
            }
            let flags = WINEVENT_OUTOFCONTEXT;
            let ranges = [
                (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
                (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
                (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            ];
            let ok = ranges.iter().all(|&(a, b)| !unsafe { SetWinEventHook(a, b, None, Some(win_event), 0, 0, flags) }.is_invalid());
            tx.send(if ok { Ok(()) } else { Err(anyhow::anyhow!("windows: SetWinEventHook failed")) }).ok();
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        })?;
        rx.recv()?
    }

    /// Button presses anywhere while armed, read off the async key state (nothing is hooked
    /// or taken); the thread sleeps while nothing is armed.
    fn watch_clicks(&self, on_click: Box<dyn Fn(i32, i32) + Send>) -> Result<()> {
        let (clicks, escape, on_esc) = (self.clicks.clone(), self.escape.clone(), self.on_escape.clone());
        let t = std::thread::Builder::new().name("avatar-clicks".into()).spawn(move || {
            let (mut was_down, mut esc_was) = (false, false);
            loop {
                let (c, e) = (clicks.load(Ordering::Relaxed), escape.load(Ordering::Relaxed));
                if !c && !e {
                    (was_down, esc_was) = (false, false);
                    std::thread::park();
                    continue;
                }
                let down = unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) < 0) };
                if c && down && !was_down {
                    let mut p = POINT::default();
                    if unsafe { GetCursorPos(&mut p) }.is_ok() {
                        let (ox, oy) = origin();
                        on_click(p.x - ox, p.y - oy);
                    }
                }
                was_down = down;
                let esc = unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0;
                if e && esc && !esc_was
                    && let Ok(f) = on_esc.lock()
                    && let Some(f) = f.as_ref()
                {
                    f();
                }
                esc_was = esc;
                std::thread::sleep(Duration::from_millis(10));
            }
        })?;
        if let Ok(mut g) = self.click_thread.lock() {
            *g = Some(t.thread().clone());
        }
        Ok(())
    }

    fn arm_clicks(&self, on: bool) {
        if self.clicks.swap(on, Ordering::Relaxed) != on {
            self.arm();
        }
    }

    fn watch_escape(&self, on: Box<dyn Fn() + Send>) {
        if let Ok(mut g) = self.on_escape.lock() {
            *g = Some(on);
        }
    }

    fn arm_escape(&self, on: bool) {
        if self.escape.swap(on, Ordering::Relaxed) != on {
            self.arm();
        }
    }
}

/// The Stage handle shares the state (and the body id) with the Desktop.
impl Stage for Arc<Win32> {
    /// GPUI's pop-up (topmost, a tool window: no taskbar button, not in Alt+Tab), its title bar
    /// hidden; `adopt` finishes the job, at the origin asked for here.
    fn dress(&self, role: Role, o: &mut WindowOptions) {
        o.kind = WindowKind::PopUp;
        let title = o.titlebar.take().and_then(|t| t.title);
        o.titlebar = Some(TitlebarOptions { title, appears_transparent: true, ..Default::default() });
        let at = o.window_bounds.map(|b| b.get_bounds()).map(|b| [f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)]);
        if let Ok(mut a) = self.asked_at.lock() {
            a.retain(|e| e.0 != role);
            a.extend(at.map(|p| (role, p)));
        }
    }

    /// The HWND from GPUI's raw handle, made one of her surfaces (`Win32::adopt_hwnd`).
    fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        let h = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(w)) => HWND(w.hwnd.get() as *mut _),
            other => bail!("not a Win32 window ({other:?})"),
        };
        self.adopt_hwnd(role, h)
    }

    fn body(&self) -> WinId {
        self.body.load(Ordering::Relaxed)
    }

    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        let (ox, oy) = origin();
        unsafe { SetWindowPos(self.body_hwnd(), None, x + ox, y + oy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)? };
        Ok(())
    }

    fn raise(&self) -> Result<()> {
        unsafe { SetWindowPos(self.body_hwnd(), Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)? };
        Ok(())
    }

    /// Through `place_later`: a place often resizes (the Line grows as it fills).
    fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let (ox, oy) = origin();
        self.place_later(s, x + ox, y + oy, w, h);
        Ok(())
    }

    /// Input: the click-through poller's region. Visible: a window region (all: none).
    fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<Visible<'_>>) -> Result<()> {
        let b = self.body();
        if let Some(input) = input {
            self.set_input(b, input);
        }
        if let Some(v) = visible {
            self.set_visible(b, v);
        }
        Ok(())
    }

    fn shape_of(&self, s: WinId, input: &[ShapeRect], visible: Visible<'_>) -> Result<()> {
        self.set_input(s, input);
        self.set_visible(s, visible);
        Ok(())
    }

    /// Tool windows show no icon anywhere.
    fn set_icon(&self, _s: WinId, _data: &[u32]) -> Result<()> {
        Ok(())
    }

    /// A window of its own per layer (`fx.rs`).
    fn fx_show(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()> {
        Win32::fx_show(self, layer, rgb, origin, size, rects, z)
    }

    fn fx_hide(&self, layer: usize) {
        Win32::fx_hide(self, layer);
    }

    fn fx_hide_all(&self) {
        Win32::fx_hide_all(self);
    }

    /// Every call above is done when it returns.
    fn flush(&self) {}

    /// The foreground (Windows hands a process the keyboard only through it), only when it is
    /// his doing (`keyboard_granted`): nothing is ever taken from the window he works in. Err
    /// when refused, by that rule or by Windows (the foreground lock).
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        if !self.keyboard_granted() {
            bail!("windows: no input of his to her just now");
        }
        let h = hwnd(s);
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        let fg = unsafe { GetForegroundWindow() };
        if fg != h {
            bail!("windows: the foreground stayed with 0x{:x}", id(fg));
        }
        Ok(())
    }

    /// One of hers has the foreground, or he gave her his input within `GRANT_MS` (her key,
    /// his press on one of her windows: the poller's, or one that is down now and the poller
    /// hasn't seen yet).
    fn keyboard_granted(&self) -> bool {
        let at = self.grant_at.load(Ordering::Relaxed);
        self.hers_in_front() || (at != 0 && tick().saturating_sub(at) <= GRANT_MS) || (buttons_down() && hers_at_pointer(&self.mine))
    }

    /// Back to `to` only while hers have the foreground: anywhere else he has moved on, and the
    /// keyboard stays where he put it. His input is spent.
    fn release_keyboard(&self, _s: WinId, to: Option<WinId>) -> Result<()> {
        if let Some(to) = to
            && self.hers_in_front()
        {
            unsafe {
                let _ = SetForegroundWindow(hwnd(to));
            }
        }
        self.grant_at.store(0, Ordering::Relaxed);
        Ok(())
    }
}

// ---- the process ----

/// Her chords on Windows: `$mod` is Win+Alt (Win alone is the shell's: Win+C is taken), the
/// toggle adds Ctrl, as i3's `$mod+c` / `$mod+Ctrl+c`. None: no key types `ch` here.
fn chord(ch: char, toggle: bool) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let vk = if ch.is_ascii_alphanumeric() {
        ch.to_ascii_uppercase() as u32
    } else {
        let v = unsafe { VkKeyScanW(ch as u16) };
        if v == -1 {
            return None;
        }
        (v & 0xff) as u32
    };
    let mods = MOD_WIN | MOD_ALT | MOD_NOREPEAT;
    Some((if toggle { mods | MOD_CONTROL } else { mods }, vk))
}

/// "Win+Alt+Ctrl+C", for the log.
fn chord_name(mods: HOT_KEY_MODIFIERS, vk: u32) -> String {
    let mut s = String::new();
    for (m, name) in [(MOD_WIN, "Win+"), (MOD_ALT, "Alt+"), (MOD_CONTROL, "Ctrl+"), (MOD_SHIFT, "Shift+")] {
        if mods.0 & m.0 != 0 {
            s.push_str(name);
        }
    }
    match char::from_u32(vk).filter(|c| c.is_ascii_alphanumeric()) {
        Some(c) => s.push(c),
        None => s.push_str(&format!("VK 0x{vk:02X}")),
    }
    s
}

/// How he presses her key `ev` here: `WIN+ALT+C`, `WIN+ALT+CTRL+C` (`chord`).
fn key_label(ev: KeyEv, key: char) -> Option<String> {
    chord(key, ev == KeyEv::TalkToggle).map(|(m, vk)| chord_name(m, vk).to_uppercase())
}

/// Tests: run `f` with stderr going into a pipe whose reader is gone; stderr is put back after.
#[cfg(test)]
pub fn stderr_into_closed_pipe<R>(f: impl FnOnce() -> R) -> R {
    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, SetStdHandle};
    use std::os::windows::io::AsRawHandle as _;
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    // SAFETY: this process's own std handle, swapped and put back; the pipe outlives the swap.
    let saved = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.expect("stderr");
    unsafe { SetStdHandle(STD_ERROR_HANDLE, HANDLE(writer.as_raw_handle())) }.expect("stderr into the pipe");
    let out = f();
    unsafe { SetStdHandle(STD_ERROR_HANDLE, saved) }.ok();
    drop(writer);
    out
}

/// First thing in main: without a console (the installed, windowed build), her log goes to a
/// file (`dirs::log_without_console`). Then per-monitor DPI awareness v2 before GPUI starts: root
/// px are device px on every monitor, and Windows never bitmap-scales her. Already set (an
/// embedded manifest) is fine if it is the same.
pub fn process_setup() {
    dirs::log_without_console();
    use ::windows::Win32::UI::HiDpi::{AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext};
    let set = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let v2 = unsafe { AreDpiAwarenessContextsEqual(GetThreadDpiAwarenessContext(), DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }.as_bool();
    if let (Err(e), false) = (set, v2) {
        elog!("synaps-avatar: windows: not per-monitor DPI aware (v2): {e}; Windows may scale her on some monitors");
    }
}

/// Another process's environment variable: not read on Windows (it lives in that process's
/// memory, its PEB). His user's variables are hers too, but one set only in the shell that started
/// the daemon isn't: None, can't be read (so a key there is never ruled out).
pub fn process_env(_pid: u32, _key: &str) -> Option<bool> {
    None
}

/// The system heap hands freed pages back on its own.
pub fn trim_heap() {}

/// Her tray icon out before her end is cut short (`platform::exit_now`): a process that ends with
/// its icon in leaves a ghost in the tray until the pointer passes over it. From this thread, not
/// her tray's; 300 ms at most. A quit took it out already (`Os::tray_remove`): nothing to do.
pub fn tray_out_now() {
    if tray::delete_now(Duration::from_millis(300)) {
        elog!("synaps-avatar: tray: her icon taken out before her end");
    }
}

/// QueryPerformanceCounter seconds (Python's `time.perf_counter` on Windows: logs line up).
pub fn monotonic_s() -> f64 {
    use ::windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    if f > 0 { c as f64 / f as f64 } else { 0.0 }
}

/// Unix seconds → (UTC, local) wall-clock times.
fn wall_clocks(unix: f64) -> Option<(::windows::Win32::Foundation::SYSTEMTIME, ::windows::Win32::Foundation::SYSTEMTIME)> {
    use ::windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use ::windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    if !unix.is_finite() || unix < 0.0 {
        return None;
    }
    let t = (unix * 1e7) as u64 + 116_444_736_000_000_000;
    let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    Some((utc, local))
}

/// The UTC offset (seconds east) of local time at unix second `unix` (DST included).
pub fn utc_offset_s(unix: i64) -> i32 {
    use ::windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use ::windows::Win32::System::Time::SystemTimeToFileTime;
    let Some((utc, local)) = wall_clocks(unix as f64) else { return 0 };
    let ticks = |st: &SYSTEMTIME| -> Option<i64> {
        let mut ft = FILETIME::default();
        unsafe { SystemTimeToFileTime(st, &mut ft).ok()? };
        Some(((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64)
    };
    match (ticks(&utc), ticks(&local)) {
        (Some(u), Some(l)) => ((l - u) / 10_000_000) as i32,
        _ => 0,
    }
}

/// The local calendar day of a unix time (years since 1900, day of the year from 0: libc's
/// `tm_year`, `tm_yday`).
pub fn local_day(unix: f64) -> Option<(i32, i32)> {
    let (_, local) = wall_clocks(unix)?;
    let y = local.wYear as i32;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    const DAYS: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = (local.wMonth as usize).clamp(1, 12);
    let yday = DAYS[m - 1] + local.wDay as i32 - 1 + i32::from(leap && m > 2);
    Some((y - 1900, yday))
}

/// GPUI's patches (vendor/gpui-pre-windows/PATCHES.md): their decisions, checked here.
#[cfg(test)]
#[path = "../../../vendor/gpui-pre-windows/src/synaps_patches.rs"]
mod gpui_patches;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpui_pw1_a_popup_is_a_bare_popup() {
        let (ex, style) = gpui_patches::popup_styles();
        assert_eq!(style, WS_POPUP, "no caption, no frame: the window is its client area");
        assert_eq!(style.0 & (WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0), 0);
        assert_eq!(ex, WS_EX_TOOLWINDOW | WS_EX_TOPMOST, "no taskbar button, not in Alt+Tab, on top");
    }

    #[test]
    fn gpui_pw2_a_click_activates_only_what_opened_with_focus() {
        use gpui_patches::{click_activates, mouse_activate_reply};
        assert!(!click_activates(true, false), "her body, intro, effects, the chip");
        assert!(click_activates(true, true), "the Line");
        assert!(click_activates(false, false) && click_activates(false, true), "other kinds as stock");
        assert_eq!(mouse_activate_reply(false), MA_NOACTIVATE as isize);
        assert_eq!(mouse_activate_reply(true), MA_ACTIVATE as isize);
    }

    unsafe extern "system" fn plain_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, msg, wp, lp) }
    }

    /// The hang on Haseeb's box (2026-11-19, four dumps alike): the input poller set
    /// `WS_EX_TRANSPARENT` from its own thread holding `surfaces` (a synchronous send to her UI
    /// thread) while her UI thread, mid-frame, waited for `surfaces` in `set_input`. Here a
    /// thread plays her UI thread: it changes her input region under a still pointer (the
    /// poller's, injected) and works a "frame" (no messages pumped) in between. The poller must
    /// post while it is busy (its posts rise before the pump: it didn't wait), at most one
    /// wake-up in flight; the window's real style ends as last wanted, both ways; and a style
    /// changed behind the poller's back is put back. No pointer of the desktop's is used.
    #[test]
    fn input_flips_never_wait_on_her_ui_thread() {
        use std::sync::mpsc::{RecvTimeoutError, channel};
        use std::time::Instant;
        let (tx, rx) = channel::<bool>();
        std::thread::Builder::new()
            .name("her-ui".into())
            .spawn(move || {
                let x = Win32::connect().unwrap();
                let (ox, oy) = origin();
                let at = POINT { x: ox + 40, y: oy + 40 };
                // The poller's pointer: still, at window px 100,100; no button held.
                let p = POINT { x: at.x + 100, y: at.y + 100 };
                x.start_input_poller_with(Box::new(move || Some((p, false))));
                let h = unsafe {
                    let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
                    let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: ::windows::core::w!("SaHangTest"), ..Default::default() };
                    RegisterClassW(&wc);
                    CreateWindowExW(WINDOW_EX_STYLE(0), ::windows::core::w!("SaHangTest"), ::windows::core::w!("sa-hang-test"), WS_POPUP | WS_VISIBLE, at.x, at.y, 200, 200, None, None, Some(inst.into()), None).unwrap()
                };
                let s = x.adopt_hwnd(Role::Body, h).unwrap();
                let pass = x.surfaces.lock().unwrap().get(&s).map(|surf| surf.pass.clone()).expect("her body is a click-through surface");
                assert!(!takes_pointer(h), "adopted letting the pointer through");
                let (take, through): ([ShapeRect; 1], [ShapeRect; 1]) = ([(50, 50, 100, 100)], [(0, 0, 10, 10)]);
                let pump = || unsafe {
                    let mut m = MSG::default();
                    while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&m);
                        DispatchMessageW(&m);
                    }
                };
                let posts = || pass.posts.load(Ordering::SeqCst);
                let in_flight = || pass.posts.load(Ordering::SeqCst) - pass.wakes.load(Ordering::SeqCst);
                // Busy, not pumping, until the poller posts (at most 2 s).
                let busy_until_posted = |before: usize| {
                    let t = Instant::now();
                    while posts() == before && t.elapsed() < Duration::from_secs(2) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    posts() > before
                };

                // 1. Frames that change her region under the pointer: each one posts before the pump.
                let (t0, mut rounds, mut posted_busy) = (Instant::now(), 0, 0);
                while t0.elapsed() < Duration::from_secs(2) {
                    let before = posts();
                    x.set_input(s, &take);
                    std::thread::sleep(Duration::from_millis(25));
                    x.set_input(s, &through);
                    std::thread::sleep(Duration::from_millis(25));
                    posted_busy += usize::from(posts() > before);
                    assert!(in_flight() <= 1, "{} wake-ups in flight: not coalesced", in_flight());
                    rounds += 1;
                    pump();
                    tx.send(true).ok();
                }
                assert!(rounds >= 10 && posted_busy * 2 >= rounds, "the poller posted while her UI thread was busy in {posted_busy} of {rounds} frames");

                // 2. The real style ends as last wanted, both ways.
                for (want_take, region) in [(true, &take), (false, &through), (true, &take)] {
                    let before = posts();
                    x.set_input(s, region);
                    assert!(busy_until_posted(before) || takes_pointer(h) == want_take, "no wake-up for take={want_take} in 2 s");
                    pump();
                    std::thread::sleep(Duration::from_millis(30));
                    pump();
                    assert_eq!(takes_pointer(h), want_take, "her real style after a wanted take={want_take}");
                    assert_eq!(in_flight(), 0);
                    tx.send(true).ok();
                }

                // 3. Changed behind the poller's back (it wants take): put back within a few ticks.
                unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, (ex_style(h) | WS_EX_TRANSPARENT.0) as isize) };
                assert!(!takes_pointer(h));
                let before = posts();
                assert!(busy_until_posted(before), "a style out of step with the wanted state isn't posted again");
                pump();
                assert!(takes_pointer(h), "put back as wanted");
                tx.send(true).ok();
                unsafe {
                    let _ = DestroyWindow(h);
                }
                tx.send(false).ok();
            })
            .unwrap();
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(true) => {}
                Ok(false) => return,
                Err(RecvTimeoutError::Timeout) => panic!("her UI thread stopped for 5 s: it and the input poller wait on each other"),
                Err(RecvTimeoutError::Disconnected) => panic!("her UI thread ended early (see its panic above)"),
            }
        }
    }

    /// Windows takes her out of the topmost band now and then, her `WS_EX_TOPMOST` bit kept
    /// (Haseeb's box, 2026-11-19): plain windows end up over her. That bit can't be set from
    /// here (Windows ignores `WS_EX_TOPMOST` through `SetWindowLongPtrW`: on his box her style
    /// stayed as it was), and the repair never reads it: she is put right under another thread's
    /// plain window (her bit cleared), then again across a hide and show of hers. The poller must
    /// see it (no pointer, no hook) and her UI thread put her back on top. Another topmost
    /// window over her (his picture-in-picture, an always-on-top app) is left be: no fight.
    #[test]
    fn she_takes_her_place_on_top_back() {
        use std::sync::mpsc::channel;
        use std::time::Instant;
        let (ox, oy) = origin();
        // Another app's plain window, on a thread of its own that pumps (cross-thread calls on it
        // come back).
        let (wtx, wrx) = channel::<isize>();
        let (stop_tx, stop_rx) = channel::<()>();
        let other = std::thread::spawn(move || unsafe {
            let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: ::windows::core::w!("SaZOther"), ..Default::default() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(WS_EX_NOACTIVATE, ::windows::core::w!("SaZOther"), ::windows::core::w!("sa-z-other"), WS_POPUP | WS_VISIBLE, ox + 300, oy + 300, 300, 300, None, None, Some(inst.into()), None).unwrap();
            wtx.send(h.0 as isize).unwrap();
            let mut m = MSG::default();
            while stop_rx.try_recv().is_err() {
                while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = DestroyWindow(h);
        });
        let plain = HWND(wrx.recv().unwrap() as *mut _);
        let x = Win32::connect().unwrap();
        x.start_input_poller_with(Box::new(|| None));
        let body = unsafe {
            let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: ::windows::core::w!("SaZBody"), ..Default::default() };
            RegisterClassW(&wc);
            CreateWindowExW(WINDOW_EX_STYLE(0), ::windows::core::w!("SaZBody"), ::windows::core::w!("sa-z-body"), WS_POPUP | WS_VISIBLE, ox + 350, oy + 350, 200, 200, None, None, Some(inst.into()), None).unwrap()
        };
        let s = x.adopt_hwnd(Role::Body, body).unwrap();
        let pass = x.surfaces.lock().unwrap().get(&s).map(|surf| surf.pass.clone()).expect("her body is a click-through surface");
        let pump = || unsafe {
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        };
        // `plain` over her, read off the z-order itself.
        let plain_over_her = || {
            let mut w = body;
            for _ in 0..4096 {
                match unsafe { GetWindow(w, GW_HWNDPREV) } {
                    Ok(p) if p == plain => return true,
                    Ok(p) => w = p,
                    Err(_) => return false,
                }
            }
            false
        };
        let state = || format!("her ex 0x{:x} showing {}, the plain window's ex 0x{:x} showing {}", ex_style(body), showing(body), ex_style(plain), showing(plain));
        pump();
        assert!(!plain_over_her(), "adopted under the plain window: {}", state());
        for case in ["out of the band", "out of the band across a hide and show"] {
            unsafe {
                if case.contains("hide") {
                    let _ = ShowWindow(body, SW_HIDE);
                }
                // Right under the plain window: out of the topmost band (inserted after a window
                // that isn't topmost).
                SetWindowPos(body, Some(plain), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).unwrap();
                if case.contains("hide") {
                    let _ = ShowWindow(body, SW_SHOWNA);
                }
            }
            assert!(plain_over_her(), "{case}: the plain window isn't over her: the test proves nothing ({})", state());
            let (t0, before) = (Instant::now(), pass.tops.load(Ordering::SeqCst));
            while plain_over_her() {
                assert!(t0.elapsed() < Duration::from_secs(3), "{case}: still under the plain window after 3 s ({})", state());
                std::thread::sleep(Duration::from_millis(10));
                pump();
            }
            assert!(pass.tops.load(Ordering::SeqCst) > before, "{case}: put back, but not by her ({})", state());
            assert_ne!(ex_style(body) & WS_EX_TOPMOST.0, 0, "{case}: topmost again ({})", state());
        }
        // No fight: a topmost window over her stays over her, and she isn't raised once in a second.
        let keep = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe { SetWindowPos(plain, Some(HWND_TOPMOST), 0, 0, 0, 0, keep).unwrap() };
        assert!(plain_over_her(), "a topmost window isn't over her: the case proves nothing ({})", state());
        let (t0, before) = (Instant::now(), pass.tops.load(Ordering::SeqCst));
        while t0.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(10));
            pump();
        }
        assert_eq!(pass.tops.load(Ordering::SeqCst), before, "she was raised over a topmost window ({})", state());
        assert!(plain_over_her(), "a topmost window over her was moved ({})", state());
        unsafe { SetWindowPos(plain, Some(HWND_NOTOPMOST), 0, 0, 0, 0, keep).unwrap() };
        stop_tx.send(()).ok();
        other.join().unwrap();
        unsafe {
            let _ = DestroyWindow(body);
        }
    }

    /// The re-raise's pace (Shady on #85: it stopped backing off at about 1 s, and a window
    /// coming forward skipped the backoff). Each put that hasn't held by the next check doubles
    /// the wait, to 8 s; a check that finds her on top brings back 250 ms; backing off, a window
    /// coming forward waits its turn; her UI thread still on the last put changes nothing; a
    /// window still over her right after her put is one she can't get above.
    #[test]
    fn the_re_raise_backs_off_to_8_s_and_waits_its_turn() {
        let mut z = ZPace::default();
        let mut waits = Vec::new();
        for _ in 0..8 {
            assert_eq!(z.found(true, false, false), ZStep::Put);
            z.put();
            waits.push(z.every.as_millis());
        }
        assert_eq!(waits, [250, 500, 1000, 2000, 4000, 8000, 8000, 8000], "under again after every put");
        assert!(!z.due(Duration::from_millis(300), true), "backing off, a window coming forward waits its turn");
        assert!(!z.due(Duration::from_millis(7999), false) && z.due(Z_MAX, false));
        let backing = z;
        assert_eq!(z.found(true, true, false), ZStep::Waiting);
        assert_eq!(z, backing, "waiting on her UI thread doubles nothing");
        assert_eq!(z.found(false, false, false), ZStep::OnTop);
        assert_eq!(z, ZPace::default(), "on top: the base pace");
        assert!(z.due(Duration::ZERO, true) && !z.due(Duration::from_millis(10), false) && z.due(Z_EVERY, false));
        // A put that holds: the next demotion starts at the base pace.
        assert_eq!(z.found(true, false, false), ZStep::Put);
        z.put();
        assert_eq!(z.found(false, false, false), ZStep::OnTop);
        assert_eq!(z.found(true, false, false), ZStep::Put);
        assert_eq!(z.every, Z_EVERY);
        z.put();
        // Still under that very window right after the put: above the band; settled, no doubling.
        assert_eq!(z.found(true, false, true), ZStep::Cant);
        assert_eq!((z.every, z.unsettled), (Z_EVERY, false));
    }

    /// A window above the topmost band, found over her again and again, is left out longer each
    /// time: 1 min, 2, 4, 8, then 10 at most (Shady on #85: not re-tried every minute all day).
    #[test]
    fn a_window_she_cant_get_above_is_left_out_longer_each_time() {
        let mins: Vec<u64> = (1..=7).map(|n| cant_for(n).as_secs()).collect();
        assert_eq!(mins, [60, 120, 240, 480, 600, 600, 600]);
        assert_eq!(cant_for(0), Z_CANT_FOR, "never less than the first stay");
        assert_eq!(cant_for(u32::MAX), Z_CANT_MAX);
    }

    /// A fullscreen app in front (a game, a video) means no re-raise: she stays where Windows put
    /// her, under another app's window, through a second of checks; with none in front she's put
    /// back on top at the next check (Shady on #85). The poller's own check, by hand: the
    /// fullscreen app is the test's word (`front_fullscreen`), as the foreground can't be taken
    /// on a runner reliably. Her window is adopted as a click-through surface that isn't her body,
    /// so the real poller's watch (it looks at her body only) never raises her behind the test.
    #[test]
    fn a_fullscreen_app_in_front_means_no_re_raise() {
        use std::sync::mpsc::channel;
        use std::time::Instant;
        let (ox, oy) = origin();
        let (wtx, wrx) = channel::<isize>();
        let (stop_tx, stop_rx) = channel::<()>();
        let other = std::thread::spawn(move || unsafe {
            let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: ::windows::core::w!("SaZFsOther"), ..Default::default() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(WS_EX_NOACTIVATE, ::windows::core::w!("SaZFsOther"), ::windows::core::w!("sa-z-fs-other"), WS_POPUP | WS_VISIBLE, ox + 300, oy + 300, 300, 300, None, None, Some(inst.into()), None).unwrap();
            wtx.send(h.0 as isize).unwrap();
            let mut m = MSG::default();
            while stop_rx.try_recv().is_err() {
                while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = DestroyWindow(h);
        });
        let plain = HWND(wrx.recv().unwrap() as *mut _);
        let x = Win32::connect().unwrap();
        let body = unsafe {
            let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: ::windows::core::w!("SaZFsBody"), ..Default::default() };
            RegisterClassW(&wc);
            CreateWindowExW(WINDOW_EX_STYLE(0), ::windows::core::w!("SaZFsBody"), ::windows::core::w!("sa-z-fs-body"), WS_POPUP | WS_VISIBLE, ox + 350, oy + 350, 200, 200, None, None, Some(inst.into()), None).unwrap()
        };
        let s = x.adopt_hwnd(Role::Intro, body).unwrap();
        assert_eq!(x.body.load(Ordering::SeqCst), 0, "adopted as her body: the real poller would raise her");
        let mine_body = AtomicU32::new(s);
        let pass = x.surfaces.lock().unwrap().get(&s).map(|surf| surf.pass.clone()).expect("her body is a click-through surface");
        let pump = || unsafe {
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        };
        let plain_over_her = || {
            let mut w = body;
            for _ in 0..4096 {
                match unsafe { GetWindow(w, GW_HWNDPREV) } {
                    Ok(p) if p == plain => return true,
                    Ok(p) => w = p,
                    Err(_) => return false,
                }
            }
            false
        };
        pump();
        unsafe { SetWindowPos(body, Some(plain), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).unwrap() };
        assert!(plain_over_her(), "the plain window isn't over her: the test proves nothing");
        // A fullscreen app in front: a second of checks, no put.
        let mut z = ZWatch { front_fullscreen: || true, ..ZWatch::default() };
        let (t0, before) = (Instant::now(), pass.tops.load(Ordering::SeqCst));
        while t0.elapsed() < Duration::from_secs(1) {
            z.last = Instant::now() - Duration::from_secs(10);
            z.tick(&mine_body, &x.surfaces, &x.mine);
            std::thread::sleep(Duration::from_millis(20));
            pump();
        }
        assert_eq!((pass.tops.load(Ordering::SeqCst), z.puts), (before, 0), "re-raised over a fullscreen app in front");
        assert!(plain_over_her(), "moved over the window while a fullscreen app was in front");
        // None in front: put back at the next check.
        z.front_fullscreen = || false;
        let t0 = Instant::now();
        while plain_over_her() {
            assert!(t0.elapsed() < Duration::from_secs(3), "no fullscreen app in front, and still under the window after 3 s");
            z.last = Instant::now() - Duration::from_secs(10);
            z.tick(&mine_body, &x.surfaces, &x.mine);
            std::thread::sleep(Duration::from_millis(20));
            pump();
        }
        assert!(z.puts > 0 && pass.tops.load(Ordering::SeqCst) > before, "put back, but not by the check");
        stop_tx.send(()).ok();
        other.join().unwrap();
        unsafe {
            let _ = DestroyWindow(body);
        }
    }

    /// Start's host is a monitor-sized window of the shell's: never a fullscreen app (on
    /// Haseeb's box she hid each time he opened Start). The shell's UI is the shell's processes'
    /// only: of another process, a window with a shell class is an app (every UWP app has a
    /// `Windows.UI.Core.CoreWindow`, top level when it goes full screen: Shady on #85). A plain
    /// monitor-sized window is a fullscreen app; a tool window (an overlay) isn't.
    #[test]
    fn the_shells_ui_is_no_fullscreen_app() {
        assert!(shell_ui("XamlExplorerHostIslandWindow", false, "explorer.exe"), "Start's and Search's host");
        assert!(shell_ui("Windows.UI.Core.CoreWindow", false, "StartMenuExperienceHost.exe"));
        assert!(shell_ui("Windows.UI.Core.CoreWindow", false, "searchhost.exe"), "any case");
        assert!(shell_ui("AFlyoutAnUpdateRenamed", true, "ShellHost.exe"), "a tool window of the shell's");
        assert!(!shell_ui("Windows.UI.Core.CoreWindow", false, "Microsoft.Media.Player.exe"), "a UWP app's own CoreWindow");
        assert!(!shell_ui("XamlExplorerHostIslandWindow", false, "synaps-avatar.exe"), "a shell class outside the shell");
        assert!(!shell_ui("CabinetWClass", false, "explorer.exe"), "his Explorer window is his");
        assert!(!shell_ui("Chrome_WidgetWin_1", true, "brave.exe"), "a tool window of an app's");
        // This desktop's own taskbar, read off the window.
        let tray = unsafe { FindWindowW(::windows::core::w!("Shell_TrayWnd"), PCWSTR::null()) }.expect("a taskbar: the runner has a desktop");
        assert!(is_shell_ui(tray), "the taskbar ({} of {}) isn't the shell's UI", class_name(tray), exe_of(tray));
        let x = Win32::connect().unwrap();
        let m = monitor_info(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }).expect("a monitor").monitorInfo.rcMonitor;
        let make = |class: PCWSTR, ex: WINDOW_EX_STYLE| unsafe {
            let inst = ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            CreateWindowExW(ex | WS_EX_NOACTIVATE, class, ::windows::core::w!("sa-fs-test"), WS_POPUP | WS_VISIBLE, m.left, m.top, m.right - m.left, m.bottom - m.top, None, None, Some(inst.into()), None).unwrap()
        };
        // Monitor-sized windows of this process (no shell's): apps, whatever their class.
        let impostor = make(::windows::core::w!("XamlExplorerHostIslandWindow"), WINDOW_EX_STYLE(0));
        let uwp = make(::windows::core::w!("Windows.UI.Core.CoreWindow"), WINDOW_EX_STYLE(0));
        let app = make(::windows::core::w!("SaFullscreenApp"), WINDOW_EX_STYLE(0));
        let overlay = make(::windows::core::w!("SaOverlay"), WS_EX_TOOLWINDOW);
        for (h, what) in [(impostor, "a shell class outside the shell"), (uwp, "a full-screen UWP app"), (app, "a monitor-sized app window")] {
            assert!(!is_shell_ui(h) && x.is_fullscreen(id(h)), "{what} taken for the shell's UI");
        }
        assert!(!x.is_fullscreen(id(overlay)), "an overlay (tool window) taken for a fullscreen app");
        unsafe {
            for h in [impostor, uwp, app, overlay] {
                let _ = DestroyWindow(h);
            }
        }
    }

    #[test]
    fn gpui_pw3_a_pure_move_skips_the_moved_callback() {
        assert!(!gpui_patches::move_runs_moved_callback());
    }

    #[test]
    fn chords_are_win_alt_and_the_toggle_adds_ctrl() {
        let (m, vk) = chord('c', false).unwrap();
        assert_eq!((m, vk), (MOD_WIN | MOD_ALT | MOD_NOREPEAT, 0x43));
        assert_eq!(chord_name(m, vk), "Win+Alt+C");
        let (m, vk) = chord('c', true).unwrap();
        assert_eq!((m, vk), (MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_NOREPEAT, 0x43));
        assert_eq!(chord_name(m, vk), "Win+Alt+Ctrl+C");
        assert_eq!(chord('7', false).map(|c| c.1), Some(0x37));
        assert_eq!(chord('V', true).map(|c| c.1), Some(0x56), "a capital is the same key");
    }

    #[test]
    fn gpui_pw4_activate_injects_no_input() {
        assert!(!gpui_patches::activate_injects_input());
    }

    #[test]
    fn gpui_pw6_a_display_change_waits_for_the_message_loop() {
        assert!(gpui_patches::display_change_is_posted(), "a move onto another monitor inside an App update re-entered App");
    }

    #[test]
    fn gpui_pw5_only_popups_keep_one_scale() {
        assert!(gpui_patches::keeps_one_scale(true), "her windows: the primary's scale everywhere");
        assert!(!gpui_patches::keeps_one_scale(false), "other kinds follow each monitor, as stock");
    }

    #[test]
    fn her_key_reads_as_he_presses_it() {
        assert_eq!(key_label(KeyEv::Summon, 'c').as_deref(), Some("WIN+ALT+C"));
        assert_eq!(key_label(KeyEv::TalkToggle, 'c').as_deref(), Some("WIN+ALT+CTRL+C"));
    }

    #[test]
    fn calendar_day_counts_from_zero() {
        // 2026-03-01 12:00 UTC: day 59 of a common year, whatever the zone (±12 h stays in it
        // only around noon UTC, so check the year and a one-day window).
        let (y, d) = local_day(1_772_366_400.0).unwrap();
        assert_eq!(y, 126);
        assert!((58..=60).contains(&d), "{d}");
        assert_eq!(local_day(f64::NAN), None);
    }

    #[test]
    fn the_utc_offset_is_whole_minutes() {
        // The VM runs on Pacific time: -8 h or -7 h. Anywhere: whole minutes, within ±14 h.
        let o = utc_offset_s(1_772_953_199);
        assert_eq!(o % 60, 0);
        assert!(o.abs() <= 14 * 3600, "{o}");
    }

    #[test]
    fn hwnd_ids_round_trip() {
        for raw in [0x000100dcisize, 0x7fff_fffe, -2] {
            let h = HWND(raw as *mut _);
            assert_eq!(hwnd(id(h)).0 as isize, raw);
        }
    }
}
