//! The Windows backend: plain translation between Win32/DWM and the core's types (her body, her
//! idle loop, perching, her global keys; voice, the Line's keyboard and server-painted effect
//! layers come later). Every rule built on these answers is the core's (`os.rs`).
//!
//! GPUI's Windows crate is vendored and patched (vendor/gpui-pre-windows/PATCHES.md): a `PopUp`
//! is a bare `WS_POPUP` (PW-1), a click on a window opened without focus doesn't activate it
//! (PW-2), a pure move doesn't re-render (PW-3). `adopt` adds the rest: topmost tool window, DWM's
//! frame, border and corner rounding off, and click-through. GPUI composites through DirectComposition (`WS_EX_NOREDIRECTIONBITMAP`, a
//! premultiplied swap chain): per-pixel alpha with no layered-window bitmap. Click-through:
//! `WS_EX_LAYERED` + `WS_EX_TRANSPARENT`, the latter toggled by a pointer poller from her input
//! region (`WM_NCHITTEST` → `HTTRANSPARENT` only passes clicks to windows of the same thread,
//! measured in the P0 probe). Coordinates: root px are the virtual screen's, from its top-left.

use super::{Caps, Desktop, FxZ, Hint, KeyEv, Keys, Monitor, PointerRead, Role, SessionEv, ShapeRect, Stage, Visible, Win, WinId, WinRect};
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use ::windows::Win32::Graphics::Dwm::{
    DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use ::windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DEVMODEW, DeleteObject, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC,
    HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromPoint, MonitorFromWindow, RGN_OR, SetWindowRgn,
};
use ::windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, HOT_KEY_MODIFIERS, LASTINPUTINFO, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, VK_ESCAPE,
    VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VkKeyScanW,
};
use ::windows::Win32::UI::Shell::{QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::{BOOL, PCWSTR};
use anyhow::{Result, bail};
use gpui_kit::{TitlebarOptions, Window, WindowKind, WindowOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(test)]
mod testkit;

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

/// One of her click-through surfaces: its input region (window px) and whether it lets the
/// pointer through now.
struct Surface {
    input: Vec<ShapeRect>,
    transparent: bool,
}

type Escapes = Arc<Mutex<Option<Box<dyn Fn() + Send>>>>;

pub struct Win32 {
    /// Her body's window (0 until adopted).
    body: AtomicU32,
    /// Every surface of hers (left out of `windows`, no hints).
    mine: Arc<Mutex<HashSet<WinId>>>,
    /// Her click-through surfaces (body, intro), by id.
    surfaces: Arc<Mutex<HashMap<WinId, Surface>>>,
    input_thread: Mutex<Option<std::thread::Thread>>,
    clicks: Arc<AtomicBool>,
    escape: Arc<AtomicBool>,
    on_escape: Escapes,
    click_thread: Mutex<Option<std::thread::Thread>>,
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
            body: AtomicU32::new(0),
            mine: Arc::new(Mutex::new(HashSet::new())),
            surfaces: Arc::new(Mutex::new(HashMap::new())),
            input_thread: Mutex::new(None),
            clicks: Arc::new(AtomicBool::new(false)),
            escape: Arc::new(AtomicBool::new(false)),
            on_escape: Arc::new(Mutex::new(None)),
            click_thread: Mutex::new(None),
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
        unsafe {
            // Keep the client area where GPUI put it, at its size: it becomes the whole window.
            let (mut c, mut at) = (RECT::default(), POINT::default());
            GetClientRect(h, &mut c)?;
            let _ = ::windows::Win32::Graphics::Gdi::ClientToScreen(h, &mut at);
            // GPUI PW-1 creates a pop-up as a bare WS_POPUP; one with a caption (an unpatched
            // GPUI) is made one here.
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
            SetWindowPos(h, Some(HWND_TOPMOST), at.x, at.y, c.right, c.bottom, SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
        }
        let w = id(h);
        if let Ok(mut m) = self.mine.lock() {
            m.insert(w);
        }
        if through {
            if let Ok(mut g) = self.surfaces.lock() {
                g.insert(w, Surface { input: Vec::new(), transparent: true });
            }
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
        let Ok(mut g) = self.input_thread.lock() else { return };
        if g.is_some() {
            return;
        }
        let surfaces = self.surfaces.clone();
        let trace = std::env::var_os("SA_WIN_TRACE").is_some();
        let t = std::thread::Builder::new().name("win-input".into()).spawn(move || {
            loop {
                let mut p = POINT::default();
                let pressed = unsafe { [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| GetAsyncKeyState(k.0 as i32) < 0) };
                if unsafe { GetCursorPos(&mut p) }.is_ok() && !pressed {
                    let Ok(mut s) = surfaces.lock() else { return };
                    for (&w, surf) in s.iter_mut() {
                        let h = hwnd(w);
                        let mut r = RECT::default();
                        if unsafe { GetWindowRect(h, &mut r) }.is_err() {
                            continue;
                        }
                        let (lx, ly) = (p.x - r.left, p.y - r.top);
                        let inside = surf.input.iter().any(|&(x, y, w, h)| lx >= x as i32 && ly >= y as i32 && lx < x as i32 + w as i32 && ly < y as i32 + h as i32);
                        if inside == surf.transparent {
                            let ex = ex_style(h);
                            let ex = if inside { ex & !WS_EX_TRANSPARENT.0 } else { ex | WS_EX_TRANSPARENT.0 };
                            unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, ex as isize) };
                            surf.transparent = !inside;
                            if trace {
                                elog!("synaps-avatar: windows: 0x{w:x} takes the pointer: {inside} at {lx},{ly} ({} input rects)", surf.input.len());
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        if let Ok(t) = t {
            *g = Some(t.thread().clone());
        }
    }

    fn set_input(&self, s: WinId, input: &[ShapeRect]) {
        if let Ok(mut g) = self.surfaces.lock() {
            match g.get_mut(&s) {
                Some(surf) => surf.input = input.to_vec(),
                None if std::env::var_os("SA_WIN_TRACE").is_some() => elog!("synaps-avatar: windows: input for 0x{s:x}, not a surface of hers"),
                None => {}
            }
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
    /// layers are painted in her window for now (server-painted ones: layered tool windows,
    /// later). The WinEvent hook reports moves, hides and shows of every window.
    fn caps(&self) -> Caps {
        Caps { shape_mode: false, target_events: true, fx_layers: false, costly_redraw: true, implicit_grab: true, goal_geometry: false, layout_events: false, host_per_output: false }
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
            .filter(|&h| showing(h) && !self.mine(h) && !is_shell(h))
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

    /// The foreground window; the desktop in front is nobody (as a focused X root is).
    fn active_window(&self) -> Option<WinId> {
        let h = unsafe { GetForegroundWindow() };
        (!h.0.is_null() && !is_shell(h)).then(|| id(h))
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
        if !showing(h) {
            return None;
        }
        frame_rect(h).map(|r| root_rect(&r, w))
    }

    /// A minimised or cloaked window still exists.
    fn window_exists(&self, w: WinId) -> bool {
        unsafe { IsWindow(Some(hwnd(w))).as_bool() }
    }

    /// It covers its whole monitor (a borderless full screen), or it is in front while the
    /// shell reports a D3D full screen or presentation mode.
    fn is_fullscreen(&self, w: WinId) -> bool {
        let h = hwnd(w);
        if is_shell(h) || !showing(h) {
            return false;
        }
        let covers = frame_rect(h).zip(monitor_info(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) })).is_some_and(|(r, mi)| {
            let m = mi.monitorInfo.rcMonitor;
            r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
        });
        if covers {
            return true;
        }
        let state = unsafe { SHQueryUserNotificationState() };
        matches!(state, Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE) && unsafe { GetForegroundWindow() } == h
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

    /// One thread: out-of-context WinEvent hooks (moves, shows, hides, minimise, cloak,
    /// foreground) on every window but hers, and its message loop.
    /// RegisterHotKey on a thread of its own (presses only, `MOD_NOREPEAT`): the summon key and
    /// the toggle chord (`chord`). A chord another app holds is skipped with its name logged.
    fn watch_keys(&self, keys: Keys, on: Box<dyn Fn(KeyEv) + Send>) -> Result<bool> {
        let wanted = [(KeyEv::Summon, chord(keys.summon, false)), (KeyEv::TalkToggle, chord(keys.toggle, true))];
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
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                if msg.message == WM_HOTKEY
                    && let Some((_, ev)) = held.iter().find(|(n, _)| *n as usize == msg.wParam.0)
                {
                    on(*ev);
                }
            }
        })?;
        rx.recv()??;
        Ok(true)
    }

    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
        let mine = self.mine.clone();
        let hint: Box<dyn Fn(Hint) + Send> = Box::new(move |h: Hint| {
            let w = match h {
                Hint::Moved(w, _) | Hint::Mapped(w) | Hint::Gone(w) => w,
                _ => 0,
            };
            if w == 0 || !mine.lock().is_ok_and(|m| m.contains(&w)) {
                hint(h);
            }
        });
        std::thread::Builder::new().name("win-watch".into()).spawn(move || {
            HINT.with(|h| *h.borrow_mut() = Some(hint));
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
    /// hidden; `adopt` finishes the job.
    fn dress(&self, _role: Role, o: &mut WindowOptions) {
        o.kind = WindowKind::PopUp;
        let title = o.titlebar.take().and_then(|t| t.title);
        o.titlebar = Some(TitlebarOptions { title, appears_transparent: true, ..Default::default() });
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

    fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let (ox, oy) = origin();
        unsafe { SetWindowPos(hwnd(s), None, x + ox, y + oy, w.max(1), h.max(1), SWP_NOZORDER | SWP_NOACTIVATE)? };
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

    /// Not called: `Caps::fx_layers` is off (the core paints the layers in her window).
    fn fx_show(&self, _layer: usize, _rgb: u32, _origin: (i32, i32), _size: (i32, i32), _rects: Option<&[ShapeRect]>, _z: FxZ) -> Result<()> {
        Ok(())
    }

    fn fx_hide(&self, _layer: usize) {}

    fn fx_hide_all(&self) {}

    fn flush(&self) {}

    /// The foreground (Windows hands a process the keyboard only through it).
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        unsafe {
            let _ = SetForegroundWindow(hwnd(s));
        }
        Ok(())
    }

    fn release_keyboard(&self, _s: WinId, to: Option<WinId>) -> Result<()> {
        if let Some(to) = to {
            self.grab_keyboard(to)?;
        }
        Ok(())
    }
}

// ---- the process ----

/// Per-monitor DPI awareness before GPUI starts (its manifest may already have set it): root px
/// are device px on every monitor.
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

/// His media players: not read on Windows yet (SMTC, the system media transport controls, comes
/// later). A watcher's first report is the sum now; here that's the sum of none (quiet), and Err:
/// no change will follow (the core then has no music detection).
pub fn watch_media(filter: crate::media::Filter, on_change: impl Fn(crate::media::Playback) + Send + Sync + 'static) -> Result<()> {
    on_change(crate::media::aggregate(std::iter::empty(), &filter));
    bail!("no media watch on Windows yet (SMTC later)")
}

/// Notifications: none on Windows yet (a toast needs an AppUserModelID, which comes with
/// packaging).
pub fn notify(_summary: &str, _body: &str, _image: Option<std::path::PathBuf>) -> Result<u32> {
    bail!("no notifications on Windows yet (toasts come with packaging)")
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

pub fn process_setup() {
    unsafe {
        let _ = ::windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(::windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// The system heap hands freed pages back on its own.
pub fn trim_heap() {}

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
    fn media_and_notifications_say_they_are_missing() {
        let quiet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let q = quiet.clone();
        assert!(watch_media(crate::media::Filter::default(), move |p| q.lock().unwrap().push(p)).is_err());
        assert_eq!(*quiet.lock().unwrap(), vec![crate::media::Playback::default()], "one report: quiet");
        assert!(notify("s", "b", None).is_err());
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
