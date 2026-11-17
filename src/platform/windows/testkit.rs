//! The contract suite on a real Windows desktop (the capri-win VM). It runs when
//! `SA_CONTRACT_WINDOWS=1`, in an interactive session (not over SSH: a service session has no
//! desktop), e.g. `SA_CONTRACT_WINDOWS=1 <test binary> --ignored contract_windows --nocapture`.
//! The test thread plays the world (framed windows, pop-ups, the pointer) and pumps their
//! messages in `settle`; a plain layered window stands in for her body.

use super::super::contract::{self, Driver};
use super::super::{Os, Role, ShapeRect, WinId, WinRect};
use super::{Win32, frame_rect, hwnd, id, origin, root_rect};
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use ::windows::Win32::Graphics::Gdi::{CreateSolidBrush, GetWindowRgn, CreateRectRgn, DeleteObject, GetRegionData, RGNDATA, RGNDATAHEADER, HBRUSH};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::w;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

unsafe extern "system" fn world_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

struct WinDriver {
    os: Arc<Win32>,
    /// The core under test (its display list is what her window paints).
    core: Rc<Os>,
    body: HWND,
    /// Framed windows' styles and rects before `fullscreen(on)`.
    saved: HashMap<WinId, (isize, RECT)>,
}

impl WinDriver {
    fn new(x: &Arc<Win32>, core: Rc<Os>) -> WinDriver {
        unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(world_proc),
                hInstance: inst.into(),
                lpszClassName: w!("SaContractWorld"),
                hbrBackground: HBRUSH(CreateSolidBrush(::windows::Win32::Foundation::COLORREF(0x808080)).0),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let (ox, oy) = origin();
            let body = CreateWindowExW(WINDOW_EX_STYLE(0), w!("SaContractWorld"), w!("sa-contract-body"), WS_POPUP | WS_VISIBLE, 900 + ox, 100 + oy, 200, 300, None, None, Some(inst.into()), None).unwrap();
            x.adopt_hwnd(Role::Body, body).unwrap();
            let mut d = WinDriver { os: x.clone(), core, body, saved: HashMap::new() };
            d.pump(50);
            d
        }
    }

    /// Run the test thread's message loop for `ms` (its windows answer, the hook's events flow).
    fn pump(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut msg = MSG::default();
        loop {
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if Instant::now() >= end {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Put `h` so that what shows of it (DWM's frame) is `r` (root px).
    fn place_frame(&mut self, h: HWND, r: WinRect) {
        let (ox, oy) = origin();
        unsafe {
            let mut wr = RECT::default();
            GetWindowRect(h, &mut wr).ok();
            let fr = frame_rect(h).unwrap_or(wr);
            // The invisible borders: window rect minus frame, on each side.
            let (l, t, rr, b) = (fr.left - wr.left, fr.top - wr.top, wr.right - fr.right, wr.bottom - fr.bottom);
            SetWindowPos(h, None, r.x + ox - l, r.y + oy - t, r.w + l + rr, r.h + t + b, SWP_NOZORDER | SWP_NOACTIVATE).ok();
        }
    }
}

impl Driver for WinDriver {
    /// Managed: a framed app window (WS_OVERLAPPEDWINDOW). Else a tool-window pop-up (a menu).
    fn open(&mut self, r: WinRect, managed: bool) -> WinId {
        let (ox, oy) = origin();
        let (ex, style) = if managed { (WINDOW_EX_STYLE(0), WS_OVERLAPPEDWINDOW) } else { (WS_EX_TOOLWINDOW | WS_EX_TOPMOST, WS_POPUP) };
        let h = unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            CreateWindowExW(ex, w!("SaContractWorld"), w!("sa-contract"), style, r.x + ox, r.y + oy, r.w, r.h, None, None, Some(inst.into()), None).unwrap()
        };
        self.place_frame(h, r);
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
        // Her body stays above everything the world opens (it is topmost too).
        self.os_raise();
        self.pump(30);
        id(h)
    }
    fn configure(&mut self, w: WinId, r: WinRect) {
        self.place_frame(hwnd(w), r);
        self.pump(5);
    }
    fn hide(&mut self, w: WinId) {
        unsafe {
            let _ = ShowWindow(hwnd(w), SW_HIDE);
        }
        self.pump(5);
    }
    fn show(&mut self, w: WinId) {
        unsafe {
            let _ = ShowWindow(hwnd(w), SW_SHOWNOACTIVATE);
        }
        self.pump(5);
    }
    fn close(&mut self, w: WinId) {
        unsafe { DestroyWindow(hwnd(w)).ok() };
        self.pump(5);
    }
    /// Borderless full screen, the way apps do it: no frame, the monitor's rect.
    fn fullscreen(&mut self, w: WinId, on: bool) {
        let h = hwnd(w);
        unsafe {
            if on {
                let mut wr = RECT::default();
                GetWindowRect(h, &mut wr).ok();
                self.saved.insert(w, (GetWindowLongPtrW(h, GWL_STYLE), wr));
                SetWindowLongPtrW(h, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let m = ::windows::Win32::Graphics::Gdi::MonitorFromWindow(h, ::windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST);
                let mut mi = ::windows::Win32::Graphics::Gdi::MONITORINFO { cbSize: std::mem::size_of::<::windows::Win32::Graphics::Gdi::MONITORINFO>() as u32, ..Default::default() };
                let _ = ::windows::Win32::Graphics::Gdi::GetMonitorInfoW(m, &mut mi);
                let r = mi.rcMonitor;
                SetWindowPos(h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED).ok();
            } else if let Some((style, wr)) = self.saved.remove(&w) {
                SetWindowLongPtrW(h, GWL_STYLE, style);
                SetWindowPos(h, None, wr.left, wr.top, wr.right - wr.left, wr.bottom - wr.top, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED).ok();
            }
        }
        self.pump(20);
    }
    /// One monitor in the VM (QEMU could give it a second head; not in P0).
    fn split_monitors(&mut self, on: bool) -> bool {
        !on
    }
    /// A strip along the top (no AppBar registration in P0: it reserves nothing).
    fn dock_top(&mut self, h: i32) -> WinId {
        let (rw, _) = super::super::Desktop::root_size(&*self.os);
        self.open(WinRect::new(0, 0, rw, h), false)
    }
    fn warp(&mut self, x: i32, y: i32) {
        let (ox, oy) = origin();
        unsafe { SetCursorPos(x + ox, y + oy).ok() };
    }
    fn body_rect(&mut self) -> Option<WinRect> {
        let mut r = RECT::default();
        unsafe { GetWindowRect(self.body, &mut r).ok()? };
        Some(root_rect(&r, id(self.body)))
    }
    /// What her window paints of the effect layers (`Caps::fx_layers` is off): the core's
    /// display list, clipped to her window, one extent per colour.
    fn fx_rects(&mut self) -> Vec<WinRect> {
        let Some(b) = self.body_rect() else { return Vec::new() };
        let os = self.core.clone();
        let mut ext: HashMap<u32, WinRect> = HashMap::new();
        for under in [true, false] {
            for ((x, y, w, h), c) in os.fx_scene(under) {
                let r = WinRect::new(b.x + x, b.y + y, w, h);
                let Some(r) = r.intersect(&b) else { continue };
                ext.entry(c)
                    .and_modify(|e| {
                        let (x0, y0) = (e.x.min(r.x), e.y.min(r.y));
                        *e = WinRect::new(x0, y0, e.right().max(r.right()) - x0, e.bottom().max(r.bottom()) - y0);
                    })
                    .or_insert(r);
            }
        }
        ext.into_values().collect()
    }
    /// The real thing: the pointer goes there, the click-through poller takes its turn, and the
    /// window the system would send a click to is asked for.
    fn hits(&mut self, x: i32, y: i32) -> bool {
        let (ox, oy) = origin();
        unsafe { SetCursorPos(x + ox, y + oy).ok() };
        self.pump(40);
        let h = unsafe { WindowFromPoint(POINT { x: x + ox, y: y + oy }) };
        !h.0.is_null() && unsafe { GetAncestor(h, GA_ROOT) } == self.body
    }
    /// Her window region (none: all of the window).
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>> {
        unsafe {
            let rgn = CreateRectRgn(0, 0, 0, 0);
            let kind = GetWindowRgn(self.body, rgn);
            // ERROR (0): no region.
            let out = if kind.0 == 0 {
                None
            } else {
                let n = GetRegionData(rgn, 0, None);
                let mut buf = vec![0u8; n as usize];
                GetRegionData(rgn, n, Some(buf.as_mut_ptr() as *mut RGNDATA));
                let hdr = &*(buf.as_ptr() as *const RGNDATAHEADER);
                let rects = std::slice::from_raw_parts(buf.as_ptr().add(hdr.dwSize as usize) as *const RECT, hdr.nCount as usize);
                Some(rects.iter().map(|r| (r.left as i16, r.top as i16, (r.right - r.left) as u16, (r.bottom - r.top) as u16)).collect())
            };
            let _ = DeleteObject(rgn.into());
            out
        }
    }
    async fn settle(&mut self) {
        self.pump(120);
    }
}

impl WinDriver {
    fn os_raise(&mut self) {
        unsafe { SetWindowPos(self.body, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).ok() };
    }
}

#[test]
#[ignore = "needs an interactive Windows desktop: SA_CONTRACT_WINDOWS=1 <test binary> --ignored contract_windows"]
fn contract_windows() {
    assert!(std::env::var("SA_CONTRACT_WINDOWS").is_ok_and(|v| v == "1"), "refusing: set SA_CONTRACT_WINDOWS=1 on a test desktop");
    let x = Arc::new(Win32::connect().unwrap());
    let os = Rc::new(Os::new(x.clone(), Box::new(x.clone())));
    let mut d = WinDriver::new(&x, os.clone());
    // Where the core thinks her body is (it paints effect layers relative to it), as the app's
    // start does right after `adopt`.
    os.move_to(900, 100).unwrap();
    let results = contract::block_on(contract::run_each(&os, &mut d)).unwrap();
    let passed: Vec<&str> = results.iter().filter(|r| r.1.is_ok()).map(|r| r.0).collect();
    for (name, r) in &results {
        match r {
            Ok(()) => eprintln!("contract_windows: PASS {name}"),
            Err(e) => eprintln!("contract_windows: FAIL {name}: {e:#}"),
        }
    }
    eprintln!("contract_windows: {} of {} scenarios passed: {}", passed.len(), results.len(), passed.join(", "));
}
