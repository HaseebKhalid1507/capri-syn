//! Effect layers on Windows (`Caps::fx_layers`), like X11's server-painted ones: a plain Win32
//! window per layer (no GPUI), filled with one colour by the system, shaped by a window region,
//! layered and click-through (`WS_EX_TRANSPARENT`, `HTTRANSPARENT`), never activated, topmost
//! and out of the taskbar. Created on the main thread: GPUI's message loop dispatches theirs.
//! Only the layers reaching outside her window come here (`Os::fx_staged`: DWM always
//! composites, so the others are painted in her window).

use super::{FxZ, ShapeRect, Win32, WinId, hwnd, id, origin, region};
use ::windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use ::windows::Win32::Graphics::Dwm::{DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute};
use ::windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC, InvalidateRect, RDW_ERASE, RDW_ERASENOW, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow, SetWindowRgn};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::UI::WindowsAndMessaging::*;
use ::windows::core::{PCWSTR, w};
use anyhow::{Result, anyhow};

const CLASS: PCWSTR = w!("SynapsAvatarFx");

/// One layer's window: shown or not, its colour (0xRRGGBB).
pub(super) struct FxWin {
    pub id: WinId,
    pub up: bool,
    pub rgb: u32,
}

/// 0xRRGGBB → a COLORREF (0x00BBGGRR).
pub(super) fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

/// Fills itself with the COLORREF in its user data; lets every click through; never activates.
unsafe extern "system" fn fx_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => {
                #[cfg(test)]
                tests::erasing(h);
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let b = CreateSolidBrush(COLORREF(GetWindowLongPtrW(h, GWLP_USERDATA) as u32));
                FillRect(HDC(wp.0 as *mut _), &r, b);
                let _ = DeleteObject(b.into());
                LRESULT(1)
            }
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }
}

/// Shows the hidden window `h` at root `x, y` (`w` × `ht` px), above `after`, already filled.
/// Shown as it is, its fresh surface stays black until its WM_ERASEBKGND comes round the message
/// loop, and DWM can composite it before then: a dark first frame (r65: the bolt's first, a dark
/// speck where it set off). So it goes up transparent, is filled before this returns
/// (`RDW_ERASENOW`), and only then turns opaque.
fn show_filled(h: HWND, after: HWND, x: i32, y: i32, w: i32, ht: i32) -> ::windows::core::Result<()> {
    unsafe {
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA);
        let shown = SetWindowPos(h, Some(after), x, y, w, ht, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = RedrawWindow(Some(h), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ERASENOW | RDW_UPDATENOW);
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
        shown
    }
}

fn create(rgb: u32) -> Option<HWND> {
    static CLASS_ONCE: std::sync::Once = std::sync::Once::new();
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        CLASS_ONCE.call_once(|| {
            let wc = WNDCLASSW { lpfnWndProc: Some(fx_proc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() };
            RegisterClassW(&wc);
        });
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST;
        let h = CreateWindowExW(ex, CLASS, w!(""), WS_POPUP, 0, 0, 1, 1, None, None, Some(inst.into()), None).ok()?;
        // A layered window shows nothing until it has attributes: opaque, painted by the system.
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
        SetWindowLongPtrW(h, GWLP_USERDATA, colorref(rgb) as isize);
        let set = |attr, v: u32| DwmSetWindowAttribute(h, attr, &v as *const u32 as *const _, 4).ok();
        set(DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED.0 as u32);
        set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
        Some(h)
    }
}

impl Win32 {
    /// `Stage::fx_show`: layer `layer` up, solid `rgb` over `rects` (window px; None keeps its
    /// region), its window at root `at` sized `size`, stacked `z`.
    /// The `fx` lock is never held across a user32 call (one may send messages and wait).
    pub(super) fn fx_show(&self, layer: usize, rgb: u32, at: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()> {
        loop {
            let n = self.fx.lock().map_err(|_| anyhow!("fx lock"))?.len();
            if n > layer {
                break;
            }
            let h = create(rgb).ok_or_else(|| anyhow!("windows: no window for effect layer {n}"))?;
            if let Ok(mut m) = self.mine.lock() {
                m.insert(id(h));
            }
            self.fx.lock().map_err(|_| anyhow!("fx lock"))?.push(FxWin { id: id(h), up: false, rgb });
        }
        let (h, after, recolour, up) = {
            let mut fx = self.fx.lock().map_err(|_| anyhow!("fx lock"))?;
            let after = match z {
                FxZ::Top => HWND_TOPMOST,
                FxZ::UnderBody => self.body_hwnd(),
                FxZ::Below(j) => match fx.get(j) {
                    Some(s) if s.up => hwnd(s.id),
                    _ => HWND_TOPMOST,
                },
            };
            let f = &mut fx[layer];
            (hwnd(f.id), after, std::mem::replace(&mut f.rgb, rgb) != rgb, f.up)
        };
        unsafe {
            if recolour {
                SetWindowLongPtrW(h, GWLP_USERDATA, colorref(rgb) as isize);
                if up {
                    let _ = InvalidateRect(Some(h), None, true);
                }
            }
            if let Some(r) = rects {
                // The window owns the region from here.
                SetWindowRgn(h, region(r), up);
            }
            let (ox, oy) = origin();
            let (x, y, w, ht) = (at.0 + ox, at.1 + oy, size.0.max(1), size.1.max(1));
            if up {
                SetWindowPos(h, Some(after), x, y, w, ht, SWP_NOACTIVATE)?;
            } else {
                show_filled(h, after, x, y, w, ht)?;
            }
        }
        if let Ok(mut fx) = self.fx.lock()
            && let Some(f) = fx.get_mut(layer)
        {
            f.up = true;
        }
        Ok(())
    }

    pub(super) fn fx_hide(&self, layer: usize) {
        let h = self.fx.lock().ok().and_then(|mut fx| fx.get_mut(layer).filter(|f| f.up).map(|f| {
            f.up = false;
            hwnd(f.id)
        }));
        if let Some(h) = h {
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }

    pub(super) fn fx_hide_all(&self) {
        let up: Vec<HWND> = self.fx.lock().map(|mut fx| fx.iter_mut().filter(|f| f.up).map(|f| {
            f.up = false;
            hwnd(f.id)
        }).collect()).unwrap_or_default();
        for h in up {
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// An effect window's layered alpha at each WM_ERASEBKGND on this thread.
        static ERASES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn erasing(h: HWND) {
        let mut alpha = 0u8;
        unsafe {
            let _ = GetLayeredWindowAttributes(h, None, Some(&raw mut alpha), None);
        }
        ERASES.with(|e| e.borrow_mut().push(alpha));
    }

    #[test]
    fn a_layer_goes_up_already_filled() {
        // Shown for a strike, the layer is filled while it's still transparent, and opaque only
        // once that's done: the screen never gets its unpainted surface (a dark first frame).
        let h = create(0xff1738).expect("an effect window");
        let (ox, oy) = origin();
        ERASES.with(|e| e.borrow_mut().clear());
        show_filled(h, HWND_TOPMOST, ox + 40, oy + 40, 160, 90).expect("shown");
        let seen = ERASES.with(|e| e.borrow().clone());
        let mut alpha = 0u8;
        let visible = unsafe {
            let _ = GetLayeredWindowAttributes(h, None, Some(&raw mut alpha), None);
            let v = IsWindowVisible(h).as_bool();
            let _ = DestroyWindow(h);
            v
        };
        assert!(visible);
        assert_eq!(seen.first(), Some(&0), "filled while transparent, before this returned: {seen:?}");
        assert_eq!(alpha, 255, "opaque once up");
    }

    #[test]
    fn colours_swap_to_bgr() {
        assert_eq!(colorref(0xff0000), 0x0000ff);
        assert_eq!(colorref(0x2266aa), 0xaa6622);
        assert_eq!(colorref(0x00ff00), 0x00ff00);
    }
}
