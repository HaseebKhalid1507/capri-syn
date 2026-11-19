//! Her icon in the notification area: `Shell_NotifyIconW` from a hidden top-level window on a
//! thread of its own, pumping its own messages as the session and layout watchers do (a
//! message-only window would miss `TaskbarCreated`: it is a broadcast). The core talks to it
//! through `WinTray` (`platform::Tray`): a new tooltip or icon is posted to it, the menu is kept
//! for the next right click, the removal is posted and waited for.
//!
//! - Her icon is known by a GUID made from her exe's path (`guid`; the shell ties a GUID to one
//!   path): each install keeps its own, and a start first deletes whatever a killed run left
//!   under it (a ghost) before adding hers.
//! - Her icon is her exe's own (build.rs: assets/icon/capri-syn.ico), or his .ico: Windows picks
//!   the image made for the tray's size from it. A picture is drawn only where neither loads.
//! - Explorer restarted (`TaskbarCreated`): added again, at the size the tray wants now.
//! - A click (`NIN_SELECT`; Enter: `NIN_KEYSELECT`) is `TrayEv::Open`, once per double-click
//!   time. `WM_CONTEXTMENU` opens the menu her model laid out (`TrackPopupMenu` at the click),
//!   and its pick comes back as `WM_COMMAND`: `TrayEv::Pick`. Either is his input to her: the
//!   keyboard is hers to take for `GRANT_MS` (the Line on a click).
//! - Gone on `remove` (QUIT), on its window's end and when the session ends: no ghost.

use super::super::{Tray, TrayEv, TrayIcon, TrayItem};
use super::tick;
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use ::windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, RGBQUAD};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use ::windows::Win32::UI::Shell::{
    NIF_GUID, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NOTIFY_ICON_DATA_FLAGS, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::{GUID, HSTRING, PCWSTR, w};
use anyhow::Result;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The icon's callback message, to her tray window.
const WM_TRAY: u32 = WM_APP + 1;
/// The tooltip changed (`WinTray::set_tip`).
const WM_TRAY_TIP: u32 = WM_APP + 2;
/// Out of the tray, and the thread ends (`WinTray::remove`).
const WM_TRAY_REMOVE: u32 = WM_APP + 3;
/// A new icon (`WinTray::set_icon`: his own put in or taken out).
const WM_TRAY_ICON: u32 = WM_APP + 4;
/// Her icon's id (with the GUID; alone where the shell refuses the GUID).
const UID: u32 = 1;
/// Enter or Space on her icon: `NIN_SELECT | NINF_KEY`.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
/// Her icon group in her exe (build.rs: assets/icon/capri-syn.ico).
const ICON_ID: usize = 1;
/// How long `remove` waits for her icon to be out.
const REMOVE_WAIT: Duration = Duration::from_secs(1);

/// `TaskbarCreated` (one value for the whole session, from `RegisterWindowMessageW`).
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// What the core and the tray's thread share.
#[derive(Default)]
struct Shared {
    tip: String,
    menu: Vec<TrayItem>,
    /// The next icon, till the tray's thread takes it.
    icon: Option<TrayIcon>,
    /// New pictures the shell took (a DPI change, `set_icon`).
    repainted: u32,
    /// Where the icon showing came from (`make_icon`).
    from: &'static str,
    /// In the tray now: its last add went in, and no removal since.
    shown: bool,
    /// Adds since the first (Explorer restarted).
    readded: u32,
}

/// The tray thread's own.
struct State {
    on: Box<dyn Fn(TrayEv) + Send>,
    shared: Arc<Mutex<Shared>>,
    grant: Arc<AtomicU64>,
    icon: TrayIcon,
    /// `icon` at `px` (else the stand-in, `own` false: shared, never destroyed).
    hicon: HICON,
    own: bool,
    px: u32,
    guid: GUID,
    /// The shell took the GUID (else her window and `UID` name the icon).
    by_guid: bool,
    /// The shell speaks version 4 (`NIN_SELECT`, `WM_CONTEXTMENU`); else plain mouse messages.
    v4: bool,
    /// When the last `Open` went (`tick` ms): the second click of a double click is the same one.
    opened_at: u64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs `f` on the tray's state. None: there is none, or a call of hers is under way below
/// (a message sent to her window while she waits on the shell): `busy`.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

fn busy() -> bool {
    STATE.with(|s| s.try_borrow_mut().is_err())
}

/// Her icon's GUID, from her exe's path: one per install, kept across runs.
fn guid() -> GUID {
    use sha2::{Digest, Sha256};
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let d = Sha256::digest(format!("capri-syn tray icon\0{exe}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    // A name-based GUID's shape (version 5, variant 1).
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    GUID::from_u128(u128::from_be_bytes(b))
}

/// `tip` as the shell takes it: UTF-16, cut at a whole character to 127 units, NUL-ended.
fn tip_w(tip: &str) -> [u16; 128] {
    let mut out = [0u16; 128];
    let mut n = 0;
    for c in tip.chars() {
        let mut b = [0u16; 2];
        let e = c.encode_utf16(&mut b);
        if n + e.len() > 127 {
            break;
        }
        out[n..n + e.len()].copy_from_slice(e);
        n += e.len();
    }
    out
}

/// Her icon's identity, with `flags`.
fn nid(h: HWND, s: &State, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: h,
        uID: UID,
        uFlags: if s.by_guid { flags | NIF_GUID } else { flags },
        guidItem: s.guid,
        ..Default::default()
    }
}

/// Explorer's taskbar is up (the tray is in it).
fn taskbar_up() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok_and(|h| !h.is_invalid())
}

/// Into the tray (again): what a run before left under her GUID goes first. True: it is in.
fn add(h: HWND, s: &mut State) -> bool {
    let tip = s.shared.lock().map(|g| g.tip.clone()).unwrap_or_default();
    let full = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    let data = |s: &State| NOTIFYICONDATAW { uCallbackMessage: WM_TRAY, hIcon: s.hicon, szTip: tip_w(&tip), ..nid(h, s, full) };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid(h, s, NOTIFY_ICON_DATA_FLAGS(0)));
        let mut ok = Shell_NotifyIconW(NIM_ADD, &data(s)).as_bool();
        if !ok && s.by_guid && taskbar_up() {
            // The shell refused her GUID (it holds it for another path): by her window instead.
            elog!("synaps-avatar: tray: the shell refused her icon's GUID; by her window instead");
            s.by_guid = false;
            ok = Shell_NotifyIconW(NIM_ADD, &data(s)).as_bool();
        }
        if ok {
            let mut v = data(s);
            v.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            s.v4 = Shell_NotifyIconW(NIM_SETVERSION, &v).as_bool();
        }
        ok
    }
}

/// Out of the tray.
fn delete(h: HWND, s: &State) {
    let shown = s.shared.lock().map(|mut g| std::mem::replace(&mut g.shown, false)).unwrap_or(true);
    if shown {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid(h, s, NOTIFY_ICON_DATA_FLAGS(0)));
        }
    }
}

/// The small icon's size where the tray is (the primary monitor), px.
fn small_px() -> u32 {
    let dpi = (96.0 * super::primary_scale()).round() as u32;
    let n = unsafe { ::windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXSMICON, dpi) };
    if n > 0 { n as u32 } else { 16 }
}

/// Her icon at `px`: an icon's own image for that size where there is one (his .ico, else her
/// exe's, `ICON_ID`), else drawn from its picture. With where it came from.
fn make_icon(icon: &TrayIcon, px: u32) -> Option<(HICON, &'static str)> {
    let n = px as i32;
    let loaded = unsafe {
        match &icon.file {
            Some(f) => LoadImageW(None, &HSTRING::from(f.as_path()), IMAGE_ICON, n, n, LR_LOADFROMFILE),
            None => LoadImageW(GetModuleHandleW(None).ok().map(Into::into), PCWSTR(std::ptr::without_provenance(ICON_ID)), IMAGE_ICON, n, n, LR_DEFAULTCOLOR),
        }
    };
    match loaded {
        Ok(h) => Some((HICON(h.0), if icon.file.is_some() { "his file" } else { "her exe" })),
        Err(_) => draw_icon(icon, px).map(|h| (h, "her picture")),
    }
}

/// `icon`'s picture at `px`×`px` with its alpha. None: it doesn't read, or GDI refused.
fn draw_icon(icon: &TrayIcon, px: u32) -> Option<HICON> {
    let img = image::RgbaImage::from_raw(icon.w, icon.h, icon.rgba.clone())?;
    let img = image::imageops::resize(&img, px, px, image::imageops::FilterType::Lanczos3);
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: px as i32,
        // Top-down rows.
        biHeight: -(px as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let bmi = BITMAPINFO { bmiHeader: header, bmiColors: [RGBQUAD::default()] };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    unsafe {
        let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(color.into());
            return None;
        }
        // BGRA, straight alpha (an icon's own).
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (px * px * 4) as usize);
        for (d, p) in dst.as_chunks_mut::<4>().0.iter_mut().zip(img.pixels()) {
            *d = [p[2], p[1], p[0], p[3]];
        }
        // The AND mask, all clear: her alpha says what shows. Rows are word-aligned.
        let mask_bits = vec![0u8; (px.div_ceil(16) * 2 * px) as usize];
        let mask = CreateBitmap(px as i32, px as i32, 1, 1, Some(mask_bits.as_ptr() as *const _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let h = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        h
    }
}

/// Windows' own application icon: hers when hers doesn't make an icon.
fn stand_in() -> HICON {
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

/// Her icon at the size the tray wants now (a DPI changed): told to the shell if it changed.
fn refresh_icon(h: HWND, s: &mut State) {
    let px = small_px();
    if px != s.px {
        paint_icon(h, s, px);
    }
}

/// `s.icon` made at `px` and told to the shell (kept as it was if it doesn't make an icon).
fn paint_icon(h: HWND, s: &mut State, px: u32) {
    let Some((new, from)) = make_icon(&s.icon, px) else { return };
    let old = std::mem::replace(&mut s.hicon, new);
    if std::mem::replace(&mut s.own, true) {
        unsafe {
            let _ = DestroyIcon(old);
        }
    }
    s.px = px;
    if let Ok(mut g) = s.shared.lock() {
        g.from = from;
    }
    if s.shared.lock().is_ok_and(|g| g.shown) {
        let d = NOTIFYICONDATAW { hIcon: s.hicon, ..nid(h, s, NIF_ICON) };
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) }.as_bool()
            && let Ok(mut g) = s.shared.lock()
        {
            g.repainted += 1;
        }
    }
}

/// The menu as her model laid it out: a row with choices opens them, the lead row is bold.
fn build_menu(items: &[TrayItem]) -> ::windows::core::Result<HMENU> {
    // `&` marks a mnemonic in a menu: hers are written as they read.
    let text = |s: &str| HSTRING::from(s.replace('&', "&&"));
    unsafe {
        let m = CreatePopupMenu()?;
        for it in items {
            let r = match it {
                TrayItem::Item { id, label, checked, lead } => {
                    let f = if *checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                    AppendMenuW(m, f, *id as usize, &text(label)).and_then(|()| if *lead { SetMenuDefaultItem(m, *id, 0) } else { Ok(()) })
                }
                TrayItem::Sub { label, items } => build_menu(items).and_then(|sub| AppendMenuW(m, MF_STRING | MF_POPUP, sub.0 as usize, &text(label))),
                TrayItem::Rule => AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null()),
            };
            if let Err(e) = r {
                let _ = DestroyMenu(m);
                return Err(e);
            }
        }
        Ok(m)
    }
}

/// A click on her icon: `Open`, once per double click.
fn open() {
    with(|s| {
        let now = tick();
        if s.opened_at != 0 && now.saturating_sub(s.opened_at) < u64::from(unsafe { GetDoubleClickTime() }) {
            return;
        }
        s.opened_at = now;
        s.grant.store(now, Ordering::Relaxed);
        (s.on)(TrayEv::Open);
    });
}

/// A pick in her menu (`WM_COMMAND`, as `TrackPopupMenu` sends it).
fn pick(id: u32) {
    with(|s| {
        s.grant.store(tick(), Ordering::Relaxed);
        (s.on)(TrayEv::Pick(id));
    });
}

/// Her menu at `at` (screen px), until he picks or clicks away.
fn show_menu(h: HWND, at: POINT) {
    let items = with(|s| s.shared.lock().map(|g| g.menu.clone()).unwrap_or_default()).unwrap_or_default();
    if items.is_empty() {
        return;
    }
    let m = match build_menu(&items) {
        Ok(m) => m,
        Err(e) => {
            elog!("synaps-avatar: tray: no menu ({e})");
            return;
        }
    };
    unsafe {
        // In front, or a click elsewhere won't close it (the shell's own sample does the same).
        let _ = SetForegroundWindow(h);
        let align = if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
        let _ = TrackPopupMenu(m, TPM_RIGHTBUTTON | align, at.x, at.y, None, h, None);
        let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
    }
}

/// A message about her icon (`WM_TRAY`): version 4's, else the plain mouse messages.
fn on_icon(h: HWND, ev: u32, wp: WPARAM) {
    let v4 = with(|s| s.v4).unwrap_or(true);
    let cursor = || {
        let mut p = POINT::default();
        let _ = unsafe { GetCursorPos(&mut p) };
        p
    };
    match ev {
        NIN_SELECT | NIN_KEYSELECT if v4 => open(),
        WM_LBUTTONUP if !v4 => open(),
        // Where it was asked for (the icon, from the keyboard).
        WM_CONTEXTMENU if v4 => show_menu(h, POINT { x: (wp.0 & 0xffff) as u16 as i16 as i32, y: ((wp.0 >> 16) & 0xffff) as u16 as i16 as i32 }),
        WM_RBUTTONUP if !v4 => show_menu(h, cursor()),
        _ => {}
    }
}

unsafe extern "system" fn tray_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ours = matches!(msg, WM_TRAY | WM_TRAY_TIP | WM_TRAY_ICON | WM_TRAY_REMOVE | WM_COMMAND) || (msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed));
    if ours && busy() {
        // Sent while a call of hers waits on the shell: after it.
        let _ = unsafe { PostMessageW(Some(h), msg, wp, lp) };
        return LRESULT(0);
    }
    match msg {
        WM_TRAY => on_icon(h, (lp.0 as u32) & 0xffff, wp),
        WM_COMMAND => {
            let id = (wp.0 & 0xffff) as u32;
            if id != 0 {
                pick(id);
            }
        }
        WM_TRAY_TIP => {
            with(|s| {
                let tip = s.shared.lock().map(|g| g.tip.clone()).unwrap_or_default();
                if s.shared.lock().is_ok_and(|g| g.shown) {
                    let d = NOTIFYICONDATAW { szTip: tip_w(&tip), ..nid(h, s, NIF_TIP | NIF_SHOWTIP) };
                    unsafe {
                        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
                    }
                }
            });
        }
        WM_TRAY_REMOVE => {
            with(|s| delete(h, s));
            let _ = unsafe { DestroyWindow(h) };
        }
        WM_DESTROY => {
            with(|s| delete(h, s));
            unsafe { PostQuitMessage(0) };
        }
        // The session ends: out before the process is stopped.
        WM_ENDSESSION if wp.0 != 0 => {
            with(|s| delete(h, s));
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            with(|s| refresh_icon(h, s));
        }
        WM_TRAY_ICON => {
            with(|s| {
                if let Some(i) = s.shared.lock().ok().and_then(|mut g| g.icon.take()) {
                    s.icon = i;
                    paint_icon(h, s, small_px());
                }
            });
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with(|s| {
                refresh_icon(h, s);
                let ok = add(h, s);
                if let Ok(mut g) = s.shared.lock() {
                    g.shown = ok;
                    g.readded += 1;
                }
                elog!("synaps-avatar: tray: Explorer started again; her icon {}", if ok { "is back" } else { "didn't go in" });
            });
        }
        _ => {}
    }
    if ours || msg == WM_DESTROY {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Her icon in the tray, on a thread of its own. Ok once its window is up; the icon itself
/// goes in now if the taskbar is there, else when Explorer says it started.
pub(super) fn start(icon: TrayIcon, grant: Arc<AtomicU64>, on: Box<dyn Fn(TrayEv) + Send>) -> Result<WinTray> {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let sh = shared.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<isize>>();
    std::thread::Builder::new().name("avatar-tray".into()).spawn(move || {
        let made = (|| -> Result<HWND> {
            let inst = unsafe { ::windows::Win32::System::LibraryLoader::GetModuleHandleW(None)? };
            let class = w!("SynapsAvatarTray");
            let wc = WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            unsafe { RegisterClassW(&wc) };
            // A real top-level window, never shown: `TaskbarCreated` skips message-only ones.
            Ok(unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, class, w!("capri-syn tray"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None)? })
        })();
        let h = match made {
            Ok(h) => h,
            Err(e) => {
                tx.send(Err(e)).ok();
                return;
            }
        };
        let created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        TASKBAR_CREATED.store(created, Ordering::Relaxed);
        // An Explorer at another integrity level may tell her it restarted too.
        let _ = unsafe { ChangeWindowMessageFilterEx(h, created, MSGFLT_ALLOW, None) };
        let px = small_px();
        let made = make_icon(&icon, px);
        let from = made.map_or("Windows' stand-in", |m| m.1);
        let mut s = State { on, shared: sh, grant, own: made.is_some(), hicon: made.map_or_else(stand_in, |m| m.0), icon, px, guid: guid(), by_guid: true, v4: false, opened_at: 0 };
        let ok = add(h, &mut s);
        if let Ok(mut g) = s.shared.lock() {
            g.shown = ok;
            g.from = from;
        }
        if ok {
            elog!("synaps-avatar: tray: her icon is in ({px} px from {from}, {})", if s.by_guid { "by its GUID" } else { "by her window" });
        } else {
            elog!("synaps-avatar: tray: her icon didn't go in (taskbar up: {}); again when Explorer says it started", taskbar_up());
        }
        STATE.with(|st| *st.borrow_mut() = Some(s));
        tx.send(Ok(h.0 as isize)).ok();
        let mut msg = MSG::default();
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            unsafe { DispatchMessageW(&msg) };
        }
        if let Some(s) = STATE.with(|st| st.borrow_mut().take())
            && s.own
        {
            unsafe {
                let _ = DestroyIcon(s.hicon);
            }
        }
    })?;
    let h = rx.recv()??;
    Ok(WinTray { hwnd: h, shared, removed: std::sync::atomic::AtomicBool::new(false) })
}

/// The core's handle on her tray icon.
pub(super) struct WinTray {
    hwnd: isize,
    shared: Arc<Mutex<Shared>>,
    removed: std::sync::atomic::AtomicBool,
}

impl WinTray {
    fn h(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
}

impl Tray for WinTray {
    fn set_tip(&self, tip: &str) {
        if let Ok(mut g) = self.shared.lock() {
            g.tip = tip.to_string();
        }
        let _ = unsafe { PostMessageW(Some(self.h()), WM_TRAY_TIP, WPARAM(0), LPARAM(0)) };
    }

    fn set_menu(&self, items: Vec<TrayItem>) {
        if let Ok(mut g) = self.shared.lock() {
            g.menu = items;
        }
    }

    fn set_icon(&self, icon: TrayIcon) {
        if let Ok(mut g) = self.shared.lock() {
            g.icon = Some(icon);
        }
        let _ = unsafe { PostMessageW(Some(self.h()), WM_TRAY_ICON, WPARAM(0), LPARAM(0)) };
    }

    /// Posted to its thread and waited for (`REMOVE_WAIT` at most): out of the tray before the
    /// process ends.
    fn remove(&self) {
        if self.removed.swap(true, Ordering::Relaxed) {
            return;
        }
        let h = self.h();
        if unsafe { PostMessageW(Some(h), WM_TRAY_REMOVE, WPARAM(0), LPARAM(0)) }.is_err() {
            return;
        }
        let end = Instant::now() + REMOVE_WAIT;
        while unsafe { IsWindow(Some(h)) }.as_bool() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Drop for WinTray {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Command;
    use crate::menu::{self, State as Menu};
    use crate::settings::Attention;
    use ::windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
    use std::sync::mpsc;

    impl WinTray {
        fn shown(&self) -> bool {
            self.shared.lock().unwrap().shown
        }
        fn readded(&self) -> u32 {
            self.shared.lock().unwrap().readded
        }
        fn repainted(&self) -> u32 {
            self.shared.lock().unwrap().repainted
        }
        fn drawn_from(&self) -> &'static str {
            self.shared.lock().unwrap().from
        }
    }

    /// Her icon as the shell knows it (by the GUID, which a fresh start always gets here).
    fn ident(h: HWND) -> NOTIFYICONIDENTIFIER {
        NOTIFYICONIDENTIFIER { cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: h, uID: UID, guidItem: guid() }
    }

    /// Every pick in a menu, top to bottom, choices in place.
    fn picks(items: &[TrayItem], out: &mut Vec<u32>) {
        for it in items {
            match it {
                TrayItem::Item { id, .. } => out.push(*id),
                TrayItem::Sub { items, .. } => picks(items, out),
                TrayItem::Rule => {}
            }
        }
    }

    fn text(m: HMENU, pos: u32) -> String {
        let mut b = [0u16; 128];
        let n = unsafe { GetMenuStringW(m, pos, Some(&mut b), MF_BYPOSITION) };
        String::from_utf16_lossy(&b[..n.max(0) as usize])
    }

    /// build.rs put assets/icon/capri-syn.ico in this exe as icon group 1, every image of it:
    /// Windows picks the 16 px one for 16 px and the 256 one for 256.
    #[test]
    fn her_exe_carries_her_icon() {
        use ::windows::Win32::System::LibraryLoader::{FindResourceW, LoadResource, LockResource, SizeofResource};
        unsafe {
            let m = GetModuleHandleW(None).unwrap();
            let r = FindResourceW(Some(m), PCWSTR(std::ptr::without_provenance(ICON_ID)), RT_GROUP_ICON);
            assert!(!r.is_invalid(), "no icon group 1 in the exe");
            assert_eq!(SizeofResource(Some(m), r), 6 + 14 * 9, "nine images");
            let dir = LockResource(LoadResource(Some(m), r).unwrap()) as *const u8;
            assert_eq!(LookupIconIdFromDirectoryEx(dir, true, 16, 16, LR_DEFAULTCOLOR), 1);
            assert_eq!(LookupIconIdFromDirectoryEx(dir, true, 256, 256, LR_DEFAULTCOLOR), 9);
            for px in [16, 20, 24, 32] {
                let h = LoadImageW(Some(m.into()), PCWSTR(std::ptr::without_provenance(ICON_ID)), IMAGE_ICON, px, px, LR_DEFAULTCOLOR).unwrap();
                DestroyIcon(HICON(h.0)).unwrap();
            }
        }
    }

    #[test]
    fn the_tip_is_cut_whole_to_what_the_shell_takes() {
        let t = tip_w("capri-syn · ONLINE");
        assert_eq!(String::from_utf16_lossy(&t[..18]), "capri-syn · ONLINE");
        assert_eq!(t[18], 0);
        // 126 units, then a character of two: it doesn't fit whole, so it isn't cut in half.
        let long = format!("{}😀tail", "a".repeat(126));
        let t = tip_w(&long);
        assert_eq!(t[125], 'a' as u16);
        assert_eq!((t[126], t[127]), (0, 0));
    }

    #[test]
    fn the_menu_is_drawn_as_her_model_lays_it_out() {
        let s = Menu { hidden: true, dnd: Some(false), attention: Some(Attention::HangOnly), handsfree: Some(false), quit_armed: false };
        let items = menu::tray_items(&menu::rows(&s));
        let m = build_menu(&items).unwrap();
        unsafe {
            assert_eq!(GetMenuItemCount(Some(m)), items.len() as i32);
            for (i, it) in items.iter().enumerate() {
                let state = GetMenuState(m, i as u32, MF_BYPOSITION);
                match it {
                    TrayItem::Item { id, label, .. } => {
                        assert_eq!((GetMenuItemID(m, i as i32), text(m, i as u32)), (*id, label.clone()));
                    }
                    TrayItem::Sub { label, items } => {
                        assert_eq!(text(m, i as u32), *label);
                        let sub = GetSubMenu(m, i as i32);
                        assert!(!sub.is_invalid());
                        assert_eq!(GetMenuItemCount(Some(sub)), items.len() as i32);
                        let checked: Vec<bool> = (0..items.len()).map(|j| GetMenuState(sub, j as u32, MF_BYPOSITION) & MF_CHECKED.0 != 0).collect();
                        assert_eq!(checked, [false, true, false], "HANG ONLY in force");
                    }
                    TrayItem::Rule => assert!(state & MF_SEPARATOR.0 != 0),
                }
            }
            assert_eq!(GetMenuDefaultItem(m, 0, GET_MENU_DEFAULT_ITEM_FLAGS(0)), menu::Item::ShowHide.id(), "SHOW leads while she is hidden");
            DestroyMenu(m).unwrap();
        }
    }

    /// The tray icon on the runner's own taskbar: in (her exe's icon), found, every menu pick and
    /// a click back through her window's own path to the commands the view dispatches
    /// (`menu_do`, the Line), his own .ico taken, in again after Explorer says it started, then
    /// out with nothing left.
    #[test]
    fn her_tray_icon_comes_answers_and_goes() {
        let (tx, rx) = mpsc::channel();
        let grant = Arc::new(AtomicU64::new(0));
        let icon = crate::brand::icon().tray();
        assert!(icon.file.is_none(), "the runner has an icon of his");
        let on: Box<dyn Fn(TrayEv) + Send> = Box::new(move |e| {
            tx.send(e).ok();
        });
        let t = start(icon, grant.clone(), on).unwrap();
        let h = t.h();
        // On the runner's log (elog: not captured).
        elog!("tray test: taskbar up: {}, icon in: {}, found at: {:?}", taskbar_up(), t.shown(), unsafe { Shell_NotifyIconGetRect(&ident(h)) });
        assert!(t.shown(), "Shell_NotifyIconW(NIM_ADD) returned FALSE");
        assert_eq!(t.drawn_from(), "her exe");
        let found = unsafe { Shell_NotifyIconGetRect(&ident(h)) }.is_ok();

        // What the core sends.
        t.set_tip(&crate::brand::tooltip("ONLINE"));
        let s = Menu { dnd: Some(false), attention: Some(Attention::Mix), handsfree: Some(false), ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        t.set_menu(items.clone());
        let mut ids = Vec::new();
        picks(&items, &mut ids);
        assert_eq!(ids.len(), 10, "{ids:?}");

        // Each pick as TrackPopupMenu hands it to her window, back as the view's command.
        for id in ids {
            unsafe { PostMessageW(Some(h), WM_COMMAND, WPARAM(id as usize), LPARAM(0)) }.unwrap();
            let ev = rx.recv_timeout(Duration::from_secs(5)).expect("the pick came back");
            assert_eq!(ev, TrayEv::Pick(id));
            assert!(matches!(menu::command_for(ev), Some(Command::MenuPick(i)) if i.id() == id), "{ev:?}");
        }
        assert!(grant.load(Ordering::Relaxed) != 0, "a pick is his input to her");

        // A click: her summon key's tap. A double click's second half is the same click.
        let click = || unsafe { PostMessageW(Some(h), WM_TRAY, WPARAM(0), LPARAM((NIN_SELECT | (UID << 16)) as isize)) }.unwrap();
        click();
        let ev = rx.recv_timeout(Duration::from_secs(5)).expect("the click came back");
        assert_eq!(ev, TrayEv::Open);
        assert!(matches!(menu::command_for(ev), Some(Command::TalkTap)));
        click();
        assert!(rx.recv_timeout(Duration::from_millis(250)).is_err(), "a double click opens once");

        // His own .ico (settings reload): loaded from his file (its picture here is no picture,
        // so nothing else could have made it), and the shell takes it.
        let dir = std::env::temp_dir().join(format!("capri-tray-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("icon.ico");
        image::RgbaImage::from_pixel(32, 32, image::Rgba([0x20, 0x80, 0xff, 0xff])).save(&file).unwrap();
        t.set_icon(TrayIcon { file: Some(file), w: 1, h: 1, rgba: Vec::new() });
        let end = Instant::now() + Duration::from_secs(5);
        while t.repainted() == 0 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!((t.repainted(), t.drawn_from(), t.shown()), (1, "his file", true), "NIM_MODIFY with his icon returned FALSE");
        std::fs::remove_dir_all(dir).ok();

        // Explorer restarted: in again.
        let _ = unsafe { SendMessageW(h, TASKBAR_CREATED.load(Ordering::Relaxed), None, None) };
        assert_eq!((t.readded(), t.shown()), (1, true));

        // QUIT: out, her window gone, nothing left in the tray.
        t.remove();
        assert!(!t.shown());
        assert!(!unsafe { IsWindow(Some(h)) }.as_bool());
        let after = unsafe { Shell_NotifyIconGetRect(&ident(h)) };
        elog!("tray test: removed; found at: {after:?}");
        if found {
            assert!(after.is_err(), "no ghost");
        }
        drop(t);
    }
}
