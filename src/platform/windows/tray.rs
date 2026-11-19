//! Her icon in the notification area: `Shell_NotifyIconW` from a hidden top-level window on a
//! thread of its own, pumping its own messages as the session and layout watchers do (a
//! message-only window would miss `TaskbarCreated`: it is a broadcast). The core talks to it
//! through `WinTray` (`platform::Tray`): a new tooltip or icon is posted to it, the menu is kept
//! for the next right click, the removal is posted and waited for. Nothing of hers ever waits on
//! Explorer: `start` returns once the window is up, and the icon goes in on the tray's thread.
//!
//! - Her icon is known by a GUID made from her exe's path (and her instance, `guid`; the shell
//!   ties a GUID to one path): each install keeps its own, and an add first deletes whatever a
//!   killed run left under it (a ghost).
//! - An add that fails is tried again every `RETRY_MS` for `RETRY_FOR_MS` (the taskbar is up
//!   before its tray takes icons, at sign-in), and again when Explorer starts again. A shell
//!   that refuses her GUID outright (not a timeout: it may still take that one) gets her window's
//!   id, and keeps it only once that goes in (`add_by`: at sign-in a tray not taking icons yet
//!   refuses both, and the next try is by her GUID again). Once her window's id was tried, her
//!   adds and her removal delete by both (`fell_back`).
//! - Her icon is her exe's own (build.rs: assets/icon/capri-syn.ico), or his .ico: Windows picks
//!   the image made for the tray's size from it. A picture is drawn only where neither loads.
//! - Explorer restarted (`TaskbarCreated`): added again, at the size the tray wants now.
//! - A click (`NIN_SELECT`; Enter: `NIN_KEYSELECT`) is `TrayEv::Open`, once per double-click
//!   time. `WM_CONTEXTMENU` opens the menu her model laid out (`TrackPopupMenu` at the click;
//!   what can't act now greyed), and its pick comes back as `WM_COMMAND`: `TrayEv::Pick`. Either
//!   is his input to her: the keyboard is hers to take for `GRANT_MS` (the Line on a click).
//! - Gone on `remove` (QUIT), on its window's end and when the session ends: no ghost.

use super::super::{Tray, TrayEv, TrayIcon, TrayItem};
use super::tick;
use ::windows::Win32::Foundation::{ERROR_TIMEOUT, GetLastError, HWND, LPARAM, LRESULT, POINT, WIN32_ERROR, WPARAM};
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
/// Into the tray, the first time (posted by `start` to its own window).
const WM_TRAY_ADD: u32 = WM_APP + 5;
/// The timer that tries an add again while she isn't in.
const ADD_TIMER: usize = 1;
const RETRY_MS: u32 = 2000;
const RETRY_FOR_MS: u64 = 60_000;
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

/// Her icon's ids while it may be in the tray (her window, her GUID; 0: none), for an end that
/// can't go through her tray's thread (`delete_now`). Cleared when that thread takes it out.
static LIVE: Mutex<(isize, u128)> = Mutex::new((0, 0));

fn forget_live(h: HWND) {
    let mut l = LIVE.lock().unwrap_or_else(|p| p.into_inner());
    if l.0 == h.0 as isize {
        *l = (0, 0);
    }
}

/// Her icon out of the tray now, from whichever thread ends her (`platform::exit_now`: the quit
/// watchdog, a fatal exit), never through her tray's thread (it may be the one that's stuck): by
/// her GUID and by her window's id, as `delete_any`. On a thread of its own, waited for `within`
/// at most, so a hung Explorer can't hold her end. False: nothing to do (her tray took it out
/// already), or it didn't answer in time.
pub(super) fn delete_now(within: Duration) -> bool {
    let (h, g) = std::mem::take(&mut *LIVE.lock().unwrap_or_else(|p| p.into_inner()));
    if h == 0 {
        return false;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("tray-out".into()).spawn(move || {
        let h = HWND(h as *mut _);
        let none = NOTIFY_ICON_DATA_FLAGS(0);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, Some(GUID::from_u128(g)), none));
            let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, None, none));
        }
        tx.send(()).ok();
    });
    spawned.is_ok() && rx.recv_timeout(within).is_ok()
}

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
    /// Her window's id was tried (`add_by`): it may be in, so her adds and her removal delete
    /// by both ids from now on.
    fell_back: bool,
    /// An add timed out: the shell may have taken it after all.
    timed_out: bool,
    /// Adds are tried again until then (`tick` ms); None: not trying.
    retry_until: Option<u64>,
    /// Out for good (`delete`): no add after it.
    left: bool,
    /// The shell speaks version 4 (`NIN_SELECT`, `WM_CONTEXTMENU`); else plain mouse messages.
    v4: bool,
    /// When the last `Open` went (`tick` ms): the second click of a double click is the same one.
    opened_at: u64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// Her GUID, for a removal that can't wait for her state (`delete_any`).
    static GUID_NOW: std::cell::Cell<u128> = const { std::cell::Cell::new(0) };
}

/// Runs `f` on the tray's state. None: there is none, or a call of hers is under way below
/// (a message sent to her window while she waits on the shell): `busy`.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

fn busy() -> bool {
    STATE.with(|s| s.try_borrow_mut().is_err())
}

/// Her icon's GUID, from her exe's path and her `instance` (`Desktop::tray`): one per install,
/// kept across runs, and another for another of her.
fn guid(instance: &str) -> GUID {
    use sha2::{Digest, Sha256};
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let key = if instance.is_empty() { format!("capri-syn tray icon\0{exe}") } else { format!("capri-syn tray icon\0{exe}\0{instance}") };
    let d = Sha256::digest(key.as_bytes());
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

/// An icon of her window's, by `guid`, else by her window and `UID`; with `flags`.
fn data_for(h: HWND, guid: Option<GUID>, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: h,
        uID: UID,
        uFlags: if guid.is_some() { flags | NIF_GUID } else { flags },
        guidItem: guid.unwrap_or_default(),
        ..Default::default()
    }
}

/// Her icon's identity as the shell knows it now, with `flags`.
fn nid(h: HWND, s: &State, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
    data_for(h, s.by_guid.then_some(s.guid), flags)
}

/// Explorer's taskbar is up (the tray is in it).
fn taskbar_up() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok_and(|h| !h.is_invalid())
}

/// An add by the rule, apart from the shell (`shell`: an add by her GUID, true, or by her
/// window's id; Err: the shell's error).
#[derive(Debug, PartialEq)]
struct Added {
    /// In; else the error to tell (her GUID's, where both were refused).
    r: Result<(), WIN32_ERROR>,
    /// Her GUID's error where it was refused and her window's id tried for it (her log).
    guid_refused: Option<WIN32_ERROR>,
    /// By her GUID from now on; else by her window's id.
    by_guid: bool,
    /// Her window's id was tried: it may be in (a late add).
    window_tried: bool,
}

/// Her GUID while the shell takes it. Refused outright (not a timeout: it may still go in) with
/// the taskbar up, her window's id; given up for it only if that goes in: at sign-in a tray not
/// taking icons yet refuses both, and the GUID is tried again (`ADD_TIMER`).
fn add_by(by_guid: bool, up: impl FnOnce() -> bool, mut shell: impl FnMut(bool) -> Result<(), WIN32_ERROR>) -> Added {
    match shell(by_guid) {
        Err(e) if by_guid && e != ERROR_TIMEOUT && up() => match shell(false) {
            Ok(()) => Added { r: Ok(()), guid_refused: Some(e), by_guid: false, window_tried: true },
            Err(_) => Added { r: Err(e), guid_refused: Some(e), by_guid: true, window_tried: true },
        },
        r => Added { r, guid_refused: None, by_guid, window_tried: !by_guid },
    }
}

/// Into the tray (again): what a run before left under her GUID goes first (and under her
/// window's id, once that was tried). Ok: it is in; else the shell's error (her GUID's where
/// both were refused).
fn add(h: HWND, s: &mut State) -> Result<(), WIN32_ERROR> {
    let tip = s.shared.lock().map(|g| g.tip.clone()).unwrap_or_default();
    let full = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    let data = |s: &State| NOTIFYICONDATAW { uCallbackMessage: WM_TRAY, hIcon: s.hicon, szTip: tip_w(&tip), ..nid(h, s, full) };
    let none = NOTIFY_ICON_DATA_FLAGS(0);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, Some(s.guid), none));
        if s.fell_back {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, None, none));
        }
        let (guid, hicon) = (s.guid, s.hicon);
        let a = add_by(s.by_guid, taskbar_up, |g| {
            let d = NOTIFYICONDATAW { uCallbackMessage: WM_TRAY, hIcon: hicon, szTip: tip_w(&tip), ..data_for(h, g.then_some(guid), full) };
            // Read at once: the next call may set its own.
            if Shell_NotifyIconW(NIM_ADD, &d).as_bool() { Ok(()) } else { Err(GetLastError()) }
        });
        if s.by_guid && !a.by_guid {
            elog!("synaps-avatar: tray: the shell refused her icon's GUID ({}); by her window from now on", err(a.guid_refused));
        }
        s.by_guid = a.by_guid;
        s.fell_back |= a.window_tried;
        s.timed_out = a.r == Err(ERROR_TIMEOUT);
        if a.r.is_ok() {
            let mut v = data(s);
            v.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            s.v4 = Shell_NotifyIconW(NIM_SETVERSION, &v).as_bool();
        }
        a.r
    }
}

/// A shell error for her log: `GetLastError`'s code (`error 1460`: ERROR_TIMEOUT).
fn err(e: Option<WIN32_ERROR>) -> String {
    e.map_or_else(|| "no error".into(), |e| format!("error {}", e.0))
}

/// Why an add is tried, for her log.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    Start,
    Retry,
    Restart,
}

/// An add, and what follows from it: shown, logged, and while she isn't in, tried again every
/// `RETRY_MS` for `RETRY_FOR_MS`.
fn try_add(h: HWND, s: &mut State, why: Why) {
    if s.left {
        return;
    }
    let r = add(h, s);
    let ok = r.is_ok();
    if let Ok(mut g) = s.shared.lock() {
        g.shown = ok;
        if why == Why::Restart {
            g.readded += 1;
        }
    }
    let from = s.shared.lock().map(|g| g.from).unwrap_or_default();
    let by = if s.by_guid { "by its GUID" } else { "by her window" };
    if ok {
        let again = s.retry_until.take().is_some();
        let _ = unsafe { KillTimer(Some(h), ADD_TIMER) };
        match why {
            Why::Restart => elog!("synaps-avatar: tray: Explorer started again; her icon is back"),
            _ => elog!("synaps-avatar: tray: her icon is in ({} px from {from}, {by}){}", s.px, if again { ", tried again" } else { "" }),
        }
        return;
    }
    let now = tick();
    match s.retry_until {
        None => {
            s.retry_until = Some(now + RETRY_FOR_MS);
            let _ = unsafe { SetTimer(Some(h), ADD_TIMER, RETRY_MS, None) };
            elog!(
                "synaps-avatar: tray: her icon didn't go in ({}; taskbar up: {}{}); again every {} s for {} s, and when Explorer starts again",
                err(r.err()),
                taskbar_up(),
                if s.timed_out { ", timed out" } else { "" },
                RETRY_MS / 1000,
                RETRY_FOR_MS / 1000
            );
        }
        Some(until) if now >= until => {
            let _ = unsafe { KillTimer(Some(h), ADD_TIMER) };
            elog!("synaps-avatar: tray: her icon still isn't in ({}); again when Explorer starts again", err(r.err()));
        }
        Some(_) => {}
    }
}

/// Out of the tray, once: by the id she has now where she is in (or an add timed out: she may
/// be), and by the other too once her window's id was tried. No add after it.
fn delete(h: HWND, s: &mut State) {
    if std::mem::replace(&mut s.left, true) {
        return;
    }
    forget_live(h);
    let _ = unsafe { KillTimer(Some(h), ADD_TIMER) };
    let shown = s.shared.lock().map(|mut g| std::mem::replace(&mut g.shown, false)).unwrap_or(true);
    unsafe {
        if shown || s.timed_out {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid(h, s, NOTIFY_ICON_DATA_FLAGS(0)));
        }
        if s.fell_back {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, (!s.by_guid).then_some(s.guid), NOTIFY_ICON_DATA_FLAGS(0)));
        }
    }
}

/// Out of the tray by every id she may have, without her state (it is in use below: the
/// session ends while a call of hers waits on the shell).
fn delete_any(h: HWND) {
    forget_live(h);
    let none = NOTIFY_ICON_DATA_FLAGS(0);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, Some(GUID::from_u128(GUID_NOW.get())), none));
        let _ = Shell_NotifyIconW(NIM_DELETE, &data_for(h, None, none));
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

/// The menu as her model laid it out: a row with choices opens them, the lead row is bold, what
/// can't act now is greyed (`TrackPopupMenu` never picks it).
fn build_menu(items: &[TrayItem]) -> ::windows::core::Result<HMENU> {
    // `&` marks a mnemonic in a menu: hers are written as they read.
    let text = |s: &str| HSTRING::from(s.replace('&', "&&"));
    let greyed = |f: MENU_ITEM_FLAGS, enabled: bool| if enabled { f } else { f | MF_GRAYED };
    unsafe {
        let m = CreatePopupMenu()?;
        for it in items {
            let r = match it {
                TrayItem::Item { id, label, checked, lead, enabled } => {
                    let f = greyed(if *checked { MF_STRING | MF_CHECKED } else { MF_STRING }, *enabled);
                    AppendMenuW(m, f, *id as usize, &text(label)).and_then(|()| if *lead { SetMenuDefaultItem(m, *id, 0) } else { Ok(()) })
                }
                // A submenu not taken in is ours to destroy.
                TrayItem::Sub { label, items, enabled } => build_menu(items).and_then(|sub| {
                    AppendMenuW(m, greyed(MF_STRING | MF_POPUP, *enabled), sub.0 as usize, &text(label)).inspect_err(|_| {
                        let _ = DestroyMenu(sub);
                    })
                }),
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
    let ours = matches!(msg, WM_TRAY | WM_TRAY_ADD | WM_TRAY_TIP | WM_TRAY_ICON | WM_TRAY_REMOVE | WM_COMMAND) || (msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed));
    if ours && busy() {
        // Sent while a call of hers waits on the shell: after it.
        let _ = unsafe { PostMessageW(Some(h), msg, wp, lp) };
        return LRESULT(0);
    }
    match msg {
        WM_TRAY_ADD => {
            with(|s| try_add(h, s, Why::Start));
        }
        // Not yet in: again (a tick while a call of hers waits is skipped; the next one comes).
        WM_TIMER if wp.0 == ADD_TIMER => {
            with(|s| {
                if s.shared.lock().is_ok_and(|g| g.shown) {
                    s.retry_until = None;
                    let _ = unsafe { KillTimer(Some(h), ADD_TIMER) };
                } else {
                    try_add(h, s, Why::Retry);
                }
            });
        }
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
        // The session ends: out before the process is stopped, now (a repost would never run),
        // by every id if her state is in use below.
        WM_ENDSESSION if wp.0 != 0 => {
            if with(|s| delete(h, s)).is_none() {
                delete_any(h);
            }
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
        // Explorer started again: its tray is new and empty. In again (and tried again for a
        // while if it isn't ready).
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with(|s| {
                refresh_icon(h, s);
                s.retry_until = None;
                let _ = unsafe { KillTimer(Some(h), ADD_TIMER) };
                try_add(h, s, Why::Restart);
            });
        }
        _ => {}
    }
    if ours || msg == WM_DESTROY {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Her tray's window class (`is_tray`).
const CLASS: &str = "SynapsAvatarTray";

/// `h` is a tray window of hers (this run's, or another instance's): hidden and 0×0, in front
/// only while its menu is up (`show_menu`). Never his window (`Win32::active_window`).
pub(super) fn is_tray(h: HWND) -> bool {
    super::class_name(h) == CLASS
}

/// Her tray's window, on the calling thread: a real top-level window, never shown
/// (`TaskbarCreated` skips message-only ones).
fn window() -> Result<HWND> {
    let inst = unsafe { GetModuleHandleW(None)? };
    let class = HSTRING::from(CLASS);
    let wc = WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: inst.into(), lpszClassName: PCWSTR(class.as_ptr()), ..Default::default() };
    unsafe { RegisterClassW(&wc) };
    Ok(unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, &class, w!("capri-syn tray"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None)? })
}

/// Her icon in the tray, on a thread of its own (`instance`: `Desktop::tray`'s). Ok once its
/// window is up, before any call to the shell: the icon goes in on that thread right after
/// (`WM_TRAY_ADD`), and is tried again if it doesn't.
pub(super) fn start(icon: TrayIcon, instance: &str, grant: Arc<AtomicU64>, on: Box<dyn Fn(TrayEv) + Send>) -> Result<WinTray> {
    let guid = guid(instance);
    let shared = Arc::new(Mutex::new(Shared::default()));
    let sh = shared.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<isize>>();
    std::thread::Builder::new().name("avatar-tray".into()).spawn(move || {
        let h = match window() {
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
        if let Ok(mut g) = sh.lock() {
            g.from = from;
        }
        let s = State {
            on,
            shared: sh,
            grant,
            own: made.is_some(),
            hicon: made.map_or_else(stand_in, |m| m.0),
            icon,
            px,
            guid,
            by_guid: true,
            fell_back: false,
            timed_out: false,
            retry_until: None,
            left: false,
            v4: false,
            opened_at: 0,
        };
        STATE.with(|st| *st.borrow_mut() = Some(s));
        GUID_NOW.set(guid.to_u128());
        *LIVE.lock().unwrap_or_else(|p| p.into_inner()) = (h.0 as isize, guid.to_u128());
        // Her side goes on now; the shell is called from the loop.
        let _ = unsafe { PostMessageW(Some(h), WM_TRAY_ADD, WPARAM(0), LPARAM(0)) };
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
        /// Waits up to 5 s for `f` to hold.
        fn until(&self, f: impl Fn(&Self) -> bool) -> bool {
            let end = Instant::now() + Duration::from_secs(5);
            while !f(self) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            f(self)
        }
    }

    /// The test's own instance: never the GUID of a real run of hers on this machine.
    const TEST: &str = "tray test";
    /// The cut-short end's test: an instance of its own.
    const CUT: &str = "tray test, cut short";
    /// The live tray tests one at a time: `LIVE` is one per process, as her tray is.
    static LIVE_TEST: Mutex<()> = Mutex::new(());

    /// Her icon as the shell knows it (by the GUID, which a fresh start always gets here).
    fn found_by(h: HWND) -> NOTIFYICONIDENTIFIER {
        found_as(h, TEST)
    }

    fn found_as(h: HWND, instance: &str) -> NOTIFYICONIDENTIFIER {
        NOTIFYICONIDENTIFIER { cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: h, uID: UID, guidItem: guid(instance) }
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
    /// Windows picks the 16 px one for 16 px and the 256 one for 256. (MSVC's linker takes the
    /// .res; a -gnu build has none and draws her picture.)
    #[cfg(target_env = "msvc")]
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

    /// The add's rule against a shell that answers from a script: the ids it was asked by (true:
    /// her GUID) and what came of it.
    fn add_with(by_guid: bool, up: bool, answers: &[Result<(), WIN32_ERROR>]) -> (Added, Vec<bool>) {
        let mut asked = Vec::new();
        let mut it = answers.iter().copied();
        let a = add_by(by_guid, || up, |g| {
            asked.push(g);
            it.next().expect("asked more than the script says")
        });
        (a, asked)
    }

    #[test]
    fn her_guid_is_given_up_only_once_her_window_goes_in() {
        let no = WIN32_ERROR(5);
        let added = |r, by_guid, window_tried| Added { r, guid_refused: None, by_guid, window_tried };
        let refused = |r, by_guid| Added { r, guid_refused: Some(no), by_guid, window_tried: true };
        // At sign-in the tray refuses both: by her GUID still (the timer tries it again), and her
        // window's id was tried (deletes go by both). The GUID's refusal is what her log tells.
        let other = WIN32_ERROR(1008);
        assert_eq!(add_with(true, true, &[Err(no), Err(other)]), (refused(Err(no), true), vec![true, false]));
        // Her GUID held for another path: by her window, for good.
        assert_eq!(add_with(true, true, &[Err(no), Ok(())]), (refused(Ok(()), false), vec![true, false]));
        // Late, not refused (it may still go in), or no taskbar yet: her GUID only.
        assert_eq!(add_with(true, true, &[Err(ERROR_TIMEOUT)]), (added(Err(ERROR_TIMEOUT), true, false), vec![true]));
        assert_eq!(add_with(true, false, &[Err(no)]), (added(Err(no), true, false), vec![true]));
        assert_eq!(add_with(true, true, &[Ok(())]), (added(Ok(()), true, false), vec![true]));
        // Fallen back: by her window, refused or not.
        assert_eq!(add_with(false, true, &[Err(no)]), (added(Err(no), false, true), vec![false]));
        assert_eq!(add_with(false, true, &[Ok(())]), (added(Ok(()), false, true), vec![false]));
    }

    #[test]
    fn a_guid_per_install_and_instance() {
        assert_eq!(guid(""), guid(""), "kept across runs");
        assert_ne!(guid(""), guid(TEST));
        assert_ne!(guid(TEST), guid("C:\\other\\synaps-avatar.sock"));
        let b = guid("").to_u128().to_be_bytes();
        assert_eq!((b[6] >> 4, b[8] >> 6), (5, 2), "a name-based GUID's shape");
    }

    #[test]
    fn the_menu_is_drawn_as_her_model_lays_it_out() {
        let why = crate::line::MenuSave::Refused.says();
        let s = Menu { hidden: true, dnd: Some(false), attention: Some(Attention::HangOnly), handsfree: Some(false), unsaved: why, ..Menu::default() };
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
                    TrayItem::Sub { label, items, .. } => {
                        assert_eq!(text(m, i as u32), *label);
                        let sub = GetSubMenu(m, i as i32);
                        assert!(!sub.is_invalid());
                        assert_eq!(GetMenuItemCount(Some(sub)), items.len() as i32);
                        let checked: Vec<bool> = (0..3).map(|j| GetMenuState(sub, j, MF_BYPOSITION) & MF_CHECKED.0 != 0).collect();
                        assert_eq!(checked, [false, true, false], "HANG ONLY in force");
                        // Its last pick didn't save: why, under a rule, greyed (no pick).
                        assert!(GetMenuState(sub, 3, MF_BYPOSITION) & MF_SEPARATOR.0 != 0);
                        assert!(GetMenuState(sub, 4, MF_BYPOSITION) & MF_GRAYED.0 != 0);
                        assert_eq!((GetMenuItemID(sub, 4), text(sub, 4)), (0, why.unwrap().to_string()));
                    }
                    TrayItem::Rule => assert!(state & MF_SEPARATOR.0 != 0),
                }
            }
            assert_eq!(GetMenuDefaultItem(m, 0, GET_MENU_DEFAULT_ITEM_FLAGS(0)), menu::Item::ShowHide.id(), "SHOW leads while she is hidden");
            DestroyMenu(m).unwrap();
        }
    }

    /// While she quits, every row but QUIT is greyed (ATTENTION's choices with it).
    #[test]
    fn what_cant_act_now_is_greyed_in_the_tray() {
        let s = Menu { quitting: true, dnd: Some(false), attention: Some(Attention::Mix), handsfree: Some(false), ..Menu::default() };
        let items = menu::tray_items(&menu::rows(&s));
        let m = build_menu(&items).unwrap();
        unsafe {
            let mut greyed = Vec::new();
            for (i, it) in items.iter().enumerate() {
                let g = GetMenuState(m, i as u32, MF_BYPOSITION) & MF_GRAYED.0 != 0;
                match it {
                    TrayItem::Item { label, .. } => greyed.push((label.clone(), g)),
                    TrayItem::Sub { label, .. } => {
                        greyed.push((label.clone(), g));
                        let sub = GetSubMenu(m, i as i32);
                        assert!((0..3).all(|j| GetMenuState(sub, j, MF_BYPOSITION) & MF_GRAYED.0 != 0));
                    }
                    TrayItem::Rule => {}
                }
            }
            let live: Vec<&str> = greyed.iter().filter(|(_, g)| !g).map(|(l, _)| l.as_str()).collect();
            assert_eq!(live, ["QUIT"], "{greyed:?}");
            DestroyMenu(m).unwrap();
        }
    }

    unsafe extern "system" fn his_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, msg, wp, lp) }
    }

    /// Her tray's window takes the foreground for its menu (`show_menu`), and a pick that opens
    /// the Line (SETTINGS) asks then which window of his was in front, to give his keyboard back
    /// to on Esc (Shady on #79: it was her hidden 0×0 window). Never hers: his last window, as
    /// with the shell's UI in front; none before he had one. Read off real windows; then, where
    /// this desktop lets a test take the foreground, as `show_menu` takes it.
    #[test]
    fn her_tray_window_is_never_his_last_window() {
        use crate::platform::Desktop;
        use super::super::{Win32, id};
        let tray = window().unwrap();
        assert!(is_tray(tray), "her tray's window not known by its class ({})", super::super::class_name(tray));
        let his = unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            let class = w!("SaHisApp");
            let wc = WNDCLASSW { lpfnWndProc: Some(his_proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            // Small, in the primary monitor's far corner: clear of the other tests' windows.
            let (x, y) = (GetSystemMetrics(SM_CXSCREEN) - 110, GetSystemMetrics(SM_CYSCREEN) - 170);
            CreateWindowExW(WS_EX_NOACTIVATE, class, w!("sa-his-app"), WS_POPUP | WS_VISIBLE, x, y, 100, 60, None, None, Some(inst.into()), None).unwrap()
        };
        assert!(!is_tray(his));
        let x = Win32::connect().unwrap();
        assert_eq!(x.active_of(tray), None, "her tray's window taken for his (none of his yet)");
        assert_eq!(x.active_of(his), Some(id(his)));
        assert_eq!(x.active_of(tray), Some(id(his)), "his window not kept with her tray's in front");
        assert_eq!(x.last_app.load(Ordering::Relaxed), id(his), "her tray's window stored as his");
        unsafe {
            let _ = SetForegroundWindow(tray);
            if GetForegroundWindow() == tray {
                assert_eq!(x.active_window(), Some(id(his)), "her tray's menu up: his window is the one he was in");
                elog!("tray test: her tray's window in front (as for its menu): his window kept");
            } else {
                elog!("tray test: this desktop didn't give the test the foreground; the real windows above stand");
            }
            let _ = DestroyWindow(tray);
            let _ = DestroyWindow(his);
        }
    }

    /// The tray icon on the runner's own taskbar: in (her exe's icon) after `start` has
    /// returned, found, every menu pick and a click back through her window's own path to the
    /// commands the view dispatches (`menu_do`, the Line), his own .ico taken, in again after
    /// Explorer says it started, then out with nothing left. CI only (`CI`; `SA_TRAY_TEST=1`
    /// elsewhere): it puts a real icon in the tray of whoever runs it.
    #[test]
    fn her_tray_icon_comes_answers_and_goes() {
        if std::env::var_os("CI").is_none() && std::env::var_os("SA_TRAY_TEST").is_none() {
            elog!("tray test: skipped (not CI; SA_TRAY_TEST=1 runs it here)");
            return;
        }
        let _one = LIVE_TEST.lock().unwrap_or_else(|p| p.into_inner());
        let (tx, rx) = mpsc::channel();
        let grant = Arc::new(AtomicU64::new(0));
        let own = crate::brand::own();
        let icon = TrayIcon { file: None, stamp: 0, w: own.width(), h: own.height(), rgba: own.into_raw() };
        let on: Box<dyn Fn(TrayEv) + Send> = Box::new(move |e| {
            tx.send(e).ok();
        });
        let t0 = Instant::now();
        let t = start(icon, TEST, grant.clone(), on).unwrap();
        let returned = t0.elapsed();
        let h = t.h();
        let shown = t.until(|t| t.shown());
        // On the runner's log (elog: not captured).
        elog!(
            "tray test: start returned in {} ms, icon in after {} ms: {shown}; taskbar up: {}, found at: {:?}",
            returned.as_millis(),
            t0.elapsed().as_millis(),
            taskbar_up(),
            unsafe { Shell_NotifyIconGetRect(&found_by(h)) }
        );
        assert!(shown, "Shell_NotifyIconW(NIM_ADD) returned FALSE");
        if cfg!(target_env = "msvc") {
            assert_eq!(t.drawn_from(), "her exe");
        }
        let found = unsafe { Shell_NotifyIconGetRect(&found_by(h)) }.is_ok();

        // What the core sends.
        t.set_tip(&crate::brand::tooltip("ONLINE", false));
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
        assert!(matches!(menu::command_for(ev), Some(Command::TrayOpen)));
        click();
        assert!(rx.recv_timeout(Duration::from_millis(250)).is_err(), "a double click opens once");

        // His own .ico (settings reload): loaded from his file (its picture here is no picture,
        // so nothing else could have made it), and the shell takes it.
        let dir = std::env::temp_dir().join(format!("capri-tray-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("icon.ico");
        image::RgbaImage::from_pixel(32, 32, image::Rgba([0x20, 0x80, 0xff, 0xff])).save(&file).unwrap();
        t.set_icon(TrayIcon { file: Some(file), stamp: 1, w: 1, h: 1, rgba: Vec::new() });
        t.until(|t| t.repainted() > 0);
        assert_eq!((t.repainted(), t.drawn_from(), t.shown()), (1, "his file", true), "NIM_MODIFY with his icon returned FALSE");
        std::fs::remove_dir_all(dir).ok();

        // Explorer restarted: in again.
        let _ = unsafe { SendMessageW(h, TASKBAR_CREATED.load(Ordering::Relaxed), None, None) };
        assert_eq!((t.readded(), t.shown()), (1, true));

        // QUIT: out, her window gone, nothing left in the tray.
        t.remove();
        assert!(!t.shown());
        assert!(!unsafe { IsWindow(Some(h)) }.as_bool());
        let after = unsafe { Shell_NotifyIconGetRect(&found_by(h)) };
        elog!("tray test: removed; found at: {after:?}");
        if found {
            assert!(after.is_err(), "no ghost");
        }
        drop(t);
    }

    /// Her end cut short (the quit watchdog, a fatal exit: `platform::exit_now`): her icon out
    /// from that thread, not her tray's, so no ghost is left in the tray; once. And after a quit
    /// that took it out through her tray's thread, nothing left for it to do. CI only, as above.
    #[test]
    fn a_cut_short_end_takes_her_icon_out() {
        if std::env::var_os("CI").is_none() && std::env::var_os("SA_TRAY_TEST").is_none() {
            elog!("tray test: skipped (not CI; SA_TRAY_TEST=1 runs it here)");
            return;
        }
        let _one = LIVE_TEST.lock().unwrap_or_else(|p| p.into_inner());
        let icon = || {
            let own = crate::brand::own();
            TrayIcon { file: None, stamp: 0, w: own.width(), h: own.height(), rgba: own.into_raw() }
        };
        let t = start(icon(), CUT, Arc::new(AtomicU64::new(0)), Box::new(|_: TrayEv| {})).unwrap();
        let h = t.h();
        assert!(t.until(|t| t.shown()), "Shell_NotifyIconW(NIM_ADD) returned FALSE");
        let found = unsafe { Shell_NotifyIconGetRect(&found_as(h, CUT)) }.is_ok();
        assert!(delete_now(Duration::from_secs(2)), "taken out, from this thread");
        let after = unsafe { Shell_NotifyIconGetRect(&found_as(h, CUT)) };
        elog!("tray test: cut short; found before: {found}, after: {after:?}");
        if found {
            assert!(after.is_err(), "no ghost");
        }
        assert!(!delete_now(Duration::from_secs(2)), "once");
        t.remove();
        assert!(!unsafe { IsWindow(Some(h)) }.as_bool());
        drop(t);

        // A quit that took it out first (`tray_remove`): nothing for her end to do.
        let t = start(icon(), CUT, Arc::new(AtomicU64::new(0)), Box::new(|_: TrayEv| {})).unwrap();
        assert!(t.until(|t| t.shown()));
        t.remove();
        assert!(!delete_now(Duration::from_secs(2)), "her tray took it out already");
        drop(t);
    }
}
