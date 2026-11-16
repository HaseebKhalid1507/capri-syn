//! The contract suite on a real X server. Test displays only: it runs when
//! `SA_CONTRACT_DISPLAY` names the same display as `DISPLAY` (never `:0`), e.g. on bella
//!   Xvfb :77 …; DISPLAY=:77 SA_CONTRACT_DISPLAY=:77 <test binary> --ignored contract_x11
//! Its own connection plays the world (windows, a panel, two RandR monitors, the pointer);
//! a plain override-redirect window stands in for her body.

use super::super::contract::{self, Driver};
use super::super::{Os, ShapeRect, WinId, WinRect};
use super::X11;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

struct XDriver {
    c: RustConnection,
    root: u32,
    body: u32,
    root_size: (u16, u16),
    atom: Box<dyn Fn(&str) -> u32>,
}

impl XDriver {
    fn new(x: &X11) -> XDriver {
        let (c, screen) = x11rb::connect(None).expect("test display");
        let s = &c.setup().roots[screen];
        let (root, root_size) = (s.root, (s.width_in_pixels, s.height_in_pixels));
        let body = c.generate_id().unwrap();
        let aux = xproto::CreateWindowAux::new().background_pixel(0x204060).override_redirect(1);
        c.create_window(x11rb::COPY_DEPTH_FROM_PARENT, body, root, 900, 100, 200, 300, 0, xproto::WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &aux).unwrap();
        c.map_window(body).unwrap();
        c.sync().unwrap();
        x.win.store(body, Ordering::Relaxed);
        let c2 = x11rb::connect(None).expect("test display").0;
        let atom = Box::new(move |n: &str| c2.intern_atom(false, n.as_bytes()).unwrap().reply().unwrap().atom);
        XDriver { c, root, body, root_size, atom }
    }

    fn sync(&self) {
        self.c.sync().unwrap();
    }
}

impl Driver for XDriver {
    fn open(&mut self, r: WinRect, managed: bool) -> WinId {
        let id = self.c.generate_id().unwrap();
        let aux = xproto::CreateWindowAux::new().background_pixel(0x808080).override_redirect(u32::from(!managed));
        self.c
            .create_window(x11rb::COPY_DEPTH_FROM_PARENT, id, self.root, r.x as i16, r.y as i16, r.w as u16, r.h as u16, 0, xproto::WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &aux)
            .unwrap();
        self.c.map_window(id).unwrap();
        self.sync();
        id
    }
    fn configure(&mut self, w: WinId, r: WinRect) {
        let aux = xproto::ConfigureWindowAux::new().x(r.x).y(r.y).width(r.w as u32).height(r.h as u32);
        self.c.configure_window(w, &aux).unwrap();
        self.sync();
    }
    fn hide(&mut self, w: WinId) {
        self.c.unmap_window(w).unwrap();
        self.sync();
    }
    fn show(&mut self, w: WinId) {
        self.c.map_window(w).unwrap();
        self.sync();
    }
    fn close(&mut self, w: WinId) {
        self.c.destroy_window(w).unwrap();
        self.sync();
    }
    fn fullscreen(&mut self, w: WinId, on: bool) {
        let state = (self.atom)("_NET_WM_STATE");
        if on {
            let fs = (self.atom)("_NET_WM_STATE_FULLSCREEN");
            self.c.change_property32(xproto::PropMode::REPLACE, w, state, xproto::AtomEnum::ATOM, &[fs]).unwrap();
        } else {
            self.c.delete_property(w, state).unwrap();
        }
        self.sync();
    }
    /// RandR 1.5 monitors: the left one takes the screen's output, the right one is virtual.
    fn split_monitors(&mut self, on: bool) -> bool {
        let (l, r) = ((self.atom)("SA-CONTRACT-L"), (self.atom)("SA-CONTRACT-R"));
        if !on {
            self.c.randr_delete_monitor(self.root, l).ok();
            self.c.randr_delete_monitor(self.root, r).ok();
            self.c.sync().ok();
            return true;
        }
        let Ok(Ok(res)) = self.c.randr_get_screen_resources_current(self.root).map(|c| c.reply()) else { return false };
        let (w, h) = (self.root_size.0, self.root_size.1);
        let mon = |name: u32, x: i16, outputs: Vec<u32>| randr::MonitorInfo {
            name,
            primary: x == 0,
            automatic: false,
            x,
            y: 0,
            width: w / 2,
            height: h,
            width_in_millimeters: 300,
            height_in_millimeters: 300,
            outputs,
        };
        let ok = self.c.randr_set_monitor(self.root, mon(l, 0, res.outputs.first().copied().into_iter().collect())).is_ok_and(|c| c.check().is_ok())
            && self.c.randr_set_monitor(self.root, mon(r, (w / 2) as i16, Vec::new())).is_ok_and(|c| c.check().is_ok());
        self.sync();
        ok
    }
    fn dock_top(&mut self, h: i32) -> WinId {
        let w = self.root_size.0 as i32;
        let id = self.open(WinRect::new(0, 0, w, h), true);
        let strut = (self.atom)("_NET_WM_STRUT_PARTIAL");
        self.c.change_property32(xproto::PropMode::REPLACE, id, strut, xproto::AtomEnum::CARDINAL, &[0, 0, h as u32, 0, 0, 0, 0, 0, 0, (w - 1) as u32, 0, 0]).unwrap();
        self.sync();
        id
    }
    fn warp(&mut self, x: i32, y: i32) {
        self.c.warp_pointer(x11rb::NONE, self.root, 0, 0, 0, 0, x as i16, y as i16).unwrap();
        self.sync();
    }
    fn body_rect(&mut self) -> Option<WinRect> {
        let g = self.c.get_geometry(self.body).ok()?.reply().ok()?;
        Some(WinRect { id: self.body, x: g.x as i32, y: g.y as i32, w: g.width as i32, h: g.height as i32 })
    }
    fn fx_rects(&mut self) -> Vec<WinRect> {
        let tree = self.c.query_tree(self.root).unwrap().reply().unwrap();
        tree.children
            .into_iter()
            .filter(|&w| {
                let class = self.c.get_property(false, w, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::STRING, 0, 16).unwrap().reply().map(|r| r.value).unwrap_or_default();
                let up = self.c.get_window_attributes(w).unwrap().reply().is_ok_and(|a| a.map_state == xproto::MapState::VIEWABLE);
                up && class.starts_with(b"synaps-avatar-fx\0")
            })
            .filter_map(|w| {
                // What the server paints: the bounding shape, at the window's place.
                let g = self.c.get_geometry(w).ok()?.reply().ok()?;
                let r = self.c.shape_get_rectangles(w, shape::SK::BOUNDING).ok()?.reply().ok()?;
                let rects: Vec<ShapeRect> = r.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect();
                let b = crate::shape::bbox(&rects)?;
                Some(WinRect { id: w, x: g.x as i32 + b.0 as i32, y: g.y as i32 + b.1 as i32, w: b.2 as i32, h: b.3 as i32 })
            })
            .collect()
    }
    /// The server's input shape, at the window's place.
    fn hits(&mut self, x: i32, y: i32) -> bool {
        let Some(b) = self.body_rect() else { return false };
        let r = self.c.shape_get_rectangles(self.body, shape::SK::INPUT).unwrap().reply().unwrap();
        let rects: Vec<ShapeRect> = r.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect();
        crate::shape::hit(&rects, x - b.x, y - b.y)
    }
    /// The bounding shape; the window's own rect (no shape) is None.
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>> {
        let shaped = self.c.shape_query_extents(self.body).unwrap().reply().unwrap().bounding_shaped;
        let r = self.c.shape_get_rectangles(self.body, shape::SK::BOUNDING).unwrap().reply().unwrap();
        shaped.then(|| r.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect())
    }
    async fn settle(&mut self) {
        self.sync();
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

#[test]
#[ignore = "needs a test X display: DISPLAY=:N SA_CONTRACT_DISPLAY=:N (never :0)"]
fn contract_x11() {
    let disp = std::env::var("DISPLAY").unwrap_or_default();
    assert!(!disp.is_empty() && disp != ":0" && std::env::var("SA_CONTRACT_DISPLAY").ok() == Some(disp.clone()), "refusing DISPLAY={disp:?}: set SA_CONTRACT_DISPLAY to the same test display");
    let x = Arc::new(X11::connect().unwrap());
    let mut d = XDriver::new(&x);
    let os = Os::new(x.clone(), Box::new(x));
    let passed = contract::block_on(contract::run(&os, &mut d)).unwrap();
    eprintln!("contract_x11 on {disp}: {} scenarios passed: {}", passed.len(), passed.join(", "));
    assert_eq!(passed.len(), 11);
}
