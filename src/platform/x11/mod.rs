//! The X11 backend: plain translation between X and the core's types. Its own x11rb
//! connection; the window ids come from GPUI's `HasWindowHandle`. What gpui-pre 0.3.12 doesn't
//! expose on X11 lives here: input/bounding shape (click-through), absolute moves, stacking,
//! monitors, struts, compositor detection, focus. Every rule built on it is the core's
//! (`platform/os.rs`).

mod struts;

use super::{Caps, Desktop, FxZ, Hint, Monitor, PointerImage, PointerRead, Role, SEAT_MIN, ShapeRect, Stage, Visible, Win, WinId, WinRect};
use struts::{Strut, work_areas};
use anyhow::{Context as _, Result, bail};
use gpui_kit::{Window, WindowKind, WindowOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::render::{self, ConnectionExt as _};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xfixes::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

#[cfg(test)]
mod testkit;

pub struct X11 {
    conn: Arc<RustConnection>,
    /// Her body's window (0 until adopted).
    win: AtomicU32,
    root: u32,
    screen_w: u16,
    screen_h: u16,
    /// A compositor owned `_NET_WM_CM_S{screen}` at connect.
    composited: bool,
    atoms: Atoms,
    /// Effect layers: (window, mapped, background colour), created on first use.
    fx: Mutex<Vec<(u32, bool, u32)>>,
    /// Clicks are reported (`arm_clicks`).
    clicks: Arc<AtomicBool>,
    click_conn: Mutex<Option<(Arc<RustConnection>, u32)>>,
    /// Escape presses are reported (`arm_escape`), to this.
    escape: Arc<AtomicBool>,
    on_escape: Escapes,
    /// The pointer image made last (its key, the X cursor).
    cursor: Mutex<Option<(u64, u32)>>,
}

type Escapes = Arc<Mutex<Option<Box<dyn Fn() + Send>>>>;

/// The keycodes that type Escape (keysym 0xff1b).
fn escape_codes(c: &RustConnection) -> Vec<u8> {
    let setup = c.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let Ok(Ok(map)) = c.get_keyboard_mapping(min, max - min + 1).map(|k| k.reply()) else { return vec![9] };
    let per = map.keysyms_per_keycode.max(1) as usize;
    let codes: Vec<u8> = map.keysyms.chunks(per).enumerate().filter(|(_, s)| s.first() == Some(&0xff1b)).map(|(i, _)| min + i as u8).collect();
    if codes.is_empty() { vec![9] } else { codes }
}

/// One viewable root child, as `children()` sees it.
#[derive(Clone, Debug)]
struct Child {
    rect: WinRect,
    override_redirect: bool,
    i3_frame: bool,
}

#[derive(Clone, Copy)]
struct Atoms {
    cm: u32,
    wm_class: u32,
    gtk_frame_extents: u32,
    compton_shadow: u32,
    active_window: u32,
    current_desktop: u32,
    strut_partial: u32,
    strut: u32,
    wm_type: u32,
    wm_type_dock: u32,
    wm_state: u32,
    wm_state_fullscreen: u32,
}

fn intern(conn: &RustConnection, name: &str) -> Result<u32> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

/// i3 frames are override-redirect too: tell them from popups by class.
fn is_i3_frame(class: &[u8]) -> bool {
    class.windows(8).any(|s| s == b"i3-frame")
}

fn xrect(w: u32, x: i16, y: i16, width: u16, height: u16, border: u16) -> WinRect {
    let b = border as i32;
    WinRect { id: w, x: x as i32, y: y as i32, w: width as i32 + 2 * b, h: height as i32 + 2 * b }
}

fn xrects(r: &[ShapeRect]) -> Vec<xproto::Rectangle> {
    r.iter().map(|&(x, y, width, height)| xproto::Rectangle { x, y, width, height }).collect()
}

/// `_NET_WM_STRUT_PARTIAL` (12 values).
fn strut_partial(v: &[u32]) -> Option<Strut> {
    if v.len() < 12 {
        return None;
    }
    let g = |i: usize| v[i].min(i32::MAX as u32) as i32;
    Some(Strut {
        left: g(0),
        right: g(1),
        top: g(2),
        bottom: g(3),
        left_y: (g(4), g(5)),
        right_y: (g(6), g(7)),
        top_x: (g(8), g(9)),
        bottom_x: (g(10), g(11)),
    })
}

/// The older `_NET_WM_STRUT` (4 values): the whole root edge.
fn strut_plain(v: &[u32], root: (i32, i32)) -> Option<Strut> {
    if v.len() < 4 {
        return None;
    }
    let g = |i: usize| v[i].min(i32::MAX as u32) as i32;
    let (w, h) = (root.0 - 1, root.1 - 1);
    Some(Strut { left: g(0), right: g(1), top: g(2), bottom: g(3), left_y: (0, h), right_y: (0, h), top_x: (0, w), bottom_x: (0, w) })
}

/// A `_NET_WM_WINDOW_TYPE_DOCK` window without strut properties: its geometry, if it hugs one
/// edge of a monitor.
fn strut_from_dock(d: &WinRect, monitors: &[WinRect], root: (i32, i32)) -> Option<Strut> {
    let m = monitors.iter().find(|m| m.contains(d.center().0, d.center().1))?;
    let (x1, y1) = (d.right() - 1, d.bottom() - 1);
    let wide = d.w >= d.h;
    if wide && d.y <= m.y + 2 {
        Some(Strut { top: d.bottom(), top_x: (d.x, x1), ..Strut::default() })
    } else if wide && d.bottom() >= m.bottom() - 2 {
        Some(Strut { bottom: root.1 - d.y, bottom_x: (d.x, x1), ..Strut::default() })
    } else if !wide && d.x <= m.x + 2 {
        Some(Strut { left: d.right(), left_y: (d.y, y1), ..Strut::default() })
    } else if !wide && d.right() >= m.right() - 2 {
        Some(Strut { right: root.0 - d.x, right_y: (d.y, y1), ..Strut::default() })
    } else {
        None
    }
}

impl X11 {
    /// Input and bounding shape of one of her windows; each `None` stays as it is.
    fn shape_win(&self, win: u32, input: Option<&[ShapeRect]>, visible: Option<Visible<'_>>) -> Result<()> {
        if let Some(input) = input {
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, win, 0, 0, &xrects(input))?;
        }
        match visible {
            Some(Visible::All) => {
                self.conn.shape_mask(shape::SO::SET, shape::SK::BOUNDING, win, 0, 0, x11rb::NONE)?;
            }
            Some(Visible::Rects(r)) => {
                self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, xproto::ClipOrdering::UNSORTED, win, 0, 0, &xrects(r))?;
            }
            None => {}
        }
        self.conn.flush()?;
        Ok(())
    }

    /// The raw events armed now: button presses (clicks) and key presses (Escape).
    fn select_raw(&self) {
        use x11rb::protocol::xinput::{self, ConnectionExt as _};
        let Ok(g) = self.click_conn.lock() else { return };
        let Some((c, root)) = g.as_ref() else { return };
        let mut bits = 0u32;
        if self.clicks.load(Ordering::Relaxed) {
            bits |= u32::from(xinput::XIEventMask::RAW_BUTTON_PRESS);
        }
        if self.escape.load(Ordering::Relaxed) {
            bits |= u32::from(xinput::XIEventMask::RAW_KEY_PRESS);
        }
        let m = xinput::EventMask { deviceid: xinput::Device::ALL_MASTER.into(), mask: vec![xinput::XIEventMask::from(bits)] };
        c.xinput_xi_select_events(*root, &[m]).ok();
        c.flush().ok();
    }

    pub fn connect() -> Result<X11> {
        let (conn, screen) = x11rb::connect(None).context("x11: cannot connect to the X server")?;
        if conn.extension_information(shape::X11_EXTENSION_NAME)?.is_none() {
            bail!("x11: the SHAPE extension is missing (needed for click-through)");
        }
        conn.shape_query_version()?.reply()?;
        let (root, screen_w, screen_h) = {
            let s = &conn.setup().roots[screen];
            (s.root, s.width_in_pixels, s.height_in_pixels)
        };
        let atoms = Atoms {
            cm: intern(&conn, &format!("_NET_WM_CM_S{screen}"))?,
            wm_class: xproto::AtomEnum::WM_CLASS.into(),
            gtk_frame_extents: intern(&conn, "_GTK_FRAME_EXTENTS")?,
            compton_shadow: intern(&conn, "_COMPTON_SHADOW")?,
            active_window: intern(&conn, "_NET_ACTIVE_WINDOW")?,
            current_desktop: intern(&conn, "_NET_CURRENT_DESKTOP")?,
            strut_partial: intern(&conn, "_NET_WM_STRUT_PARTIAL")?,
            strut: intern(&conn, "_NET_WM_STRUT")?,
            wm_type: intern(&conn, "_NET_WM_WINDOW_TYPE")?,
            wm_type_dock: intern(&conn, "_NET_WM_WINDOW_TYPE_DOCK")?,
            wm_state: intern(&conn, "_NET_WM_STATE")?,
            wm_state_fullscreen: intern(&conn, "_NET_WM_STATE_FULLSCREEN")?,
        };
        let composited = conn.get_selection_owner(atoms.cm)?.reply()?.owner != x11rb::NONE;
        Ok(X11 {
            conn: Arc::new(conn),
            win: AtomicU32::new(0),
            root,
            screen_w,
            screen_h,
            composited,
            atoms,
            fx: Mutex::new(Vec::new()),
            clicks: Arc::new(AtomicBool::new(false)),
            click_conn: Mutex::new(None),
            escape: Arc::new(AtomicBool::new(false)),
            on_escape: Arc::new(Mutex::new(None)),
            cursor: Mutex::new(None),
        })
    }

    fn win(&self) -> u32 {
        self.win.load(Ordering::Relaxed)
    }

    /// Properties for the compositor: a `_GTK_FRAME_EXTENTS` of zero keeps picom's stock
    /// `blur-background-exclude = [ "_GTK_FRAME_EXTENTS@" ]` from frosting our bounding box.
    fn set_compositor_hints(&self) -> Result<()> {
        let win = self.win();
        self.conn.change_property32(xproto::PropMode::REPLACE, win, self.atoms.gtk_frame_extents, xproto::AtomEnum::CARDINAL, &[0, 0, 0, 0])?;
        // picom/compton: no drop shadow around the transparent window.
        self.conn.change_property32(xproto::PropMode::REPLACE, win, self.atoms.compton_shadow, xproto::AtomEnum::CARDINAL, &[0])?;
        self.conn.flush()?;
        Ok(())
    }

    /// Viewable root children, bottom → top, except ours, our effect layers and InputOnly
    /// windows. Small and override-redirect ones stay (they occlude).
    fn children(&self) -> Vec<Child> {
        let Ok(Ok(tree)) = self.conn.query_tree(self.root).map(|c| c.reply()) else { return Vec::new() };
        let fx: Vec<u32> = self.fx.lock().map(|v| v.iter().map(|f| f.0).collect()).unwrap_or_default();
        let win = self.win();
        let cookies: Vec<_> = tree
            .children
            .iter()
            .filter(|&&w| w != win && !fx.contains(&w))
            .filter_map(|&w| Some((w, self.conn.get_window_attributes(w).ok()?, self.conn.get_geometry(w).ok()?)))
            .collect();
        let viewable: Vec<(WinRect, bool)> = cookies
            .into_iter()
            .filter_map(|(w, a, g)| {
                let (a, g) = (a.reply().ok()?, g.reply().ok()?);
                if a.map_state != xproto::MapState::VIEWABLE || a.class == xproto::WindowClass::INPUT_ONLY {
                    return None;
                }
                Some((xrect(w, g.x, g.y, g.width, g.height, g.border_width), a.override_redirect))
            })
            .collect();
        // Only override-redirect windows big enough to sit on are worth telling apart.
        let class: Vec<_> = viewable
            .iter()
            .map(|(r, or)| {
                (*or && r.w >= SEAT_MIN.0 && r.h >= SEAT_MIN.1)
                    .then(|| self.conn.get_property(false, r.id, self.atoms.wm_class, xproto::AtomEnum::STRING, 0, 64).ok())
                    .flatten()
            })
            .collect();
        viewable
            .into_iter()
            .zip(class)
            .map(|((rect, or), c)| {
                let i3_frame = c.and_then(|c| c.reply().ok()).is_some_and(|r| is_i3_frame(&r.value));
                Child { rect, override_redirect: or, i3_frame }
            })
            .collect()
    }

    /// Struts of the docks: thin viewable windows hugging a monitor edge, and their
    /// children (a reparenting WM puts the dock client inside a frame).
    fn struts(&self, mons: &[WinRect], root: (i32, i32)) -> Vec<Strut> {
        let hugs = |r: &WinRect| {
            mons.iter().any(|m| {
                m.intersect(r).is_some()
                    && ((r.h <= 160 && (r.y <= m.y + 2 || r.bottom() >= m.bottom() - 2))
                        || (r.w <= 160 && (r.x <= m.x + 2 || r.right() >= m.right() - 2)))
            })
        };
        let docks: Vec<WinRect> = self.children().into_iter().map(|c| c.rect).filter(|r| hugs(r)).collect();
        let mut wins: Vec<(u32, WinRect)> = docks.iter().map(|r| (r.id, *r)).collect();
        let kids: Vec<_> = docks.iter().filter_map(|r| Some((*r, self.conn.query_tree(r.id).ok()?))).collect();
        for (r, k) in kids {
            if let Ok(t) = k.reply() {
                wins.extend(t.children.into_iter().map(|c| (c, r)));
            }
        }
        let a = self.atoms;
        let cookies: Vec<_> = wins
            .iter()
            .map(|&(w, r)| {
                let get = |atom: u32, ty: xproto::AtomEnum, n: u32| self.conn.get_property(false, w, atom, ty, 0, n).ok();
                (r, get(a.strut_partial, xproto::AtomEnum::CARDINAL, 12), get(a.strut, xproto::AtomEnum::CARDINAL, 4), get(a.wm_type, xproto::AtomEnum::ATOM, 8))
            })
            .collect();
        let mut out = Vec::new();
        for (r, p, s, t) in cookies {
            let vals = |c: Option<x11rb::cookie::Cookie<'_, Arc<RustConnection>, xproto::GetPropertyReply>>| {
                c.and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect::<Vec<u32>>())).unwrap_or_default()
            };
            let (p, s, t) = (vals(p), vals(s), vals(t));
            if let Some(st) = strut_partial(&p).or_else(|| strut_plain(&s, root)) {
                out.push(st);
            } else if t.contains(&a.wm_type_dock)
                && let Some(st) = strut_from_dock(&r, mons, root)
            {
                out.push(st);
            }
        }
        out
    }
}

impl Desktop for X11 {
    fn caps(&self) -> Caps {
        Caps { shape_mode: true, target_events: true, fx_layers: true, costly_redraw: false, implicit_grab: true, goal_geometry: false, layout_events: false, host_per_output: false }
    }

    fn root_size(&self) -> (i32, i32) {
        (self.screen_w as i32, self.screen_h as i32)
    }

    /// RandR monitors.
    fn monitors(&self) -> Vec<Monitor> {
        let Ok(Ok(r)) = self.conn.randr_get_monitors(self.root, true).map(|c| c.reply()) else { return Vec::new() };
        r.monitors.into_iter().map(|m| Monitor { primary: m.primary, x: m.x as i32, y: m.y as i32, w: m.width as i32, h: m.height as i32 }).collect()
    }

    /// Minus the docks' struts (`_NET_WM_STRUT_PARTIAL`, `_NET_WM_STRUT`, or a
    /// `_NET_WM_WINDOW_TYPE_DOCK` window's own geometry). i3 sets no usable `_NET_WORKAREA`,
    /// and its docks sit inside i3 frames, so the frames' children are read too.
    fn work_areas(&self, mons: &[Monitor]) -> Vec<Monitor> {
        let rects: Vec<WinRect> = mons.iter().map(Monitor::rect).collect();
        let root = self.root_size();
        let struts = self.struts(&rects, root);
        work_areas(&rects, root, &struts)
            .into_iter()
            .zip(mons)
            .map(|(a, m)| Monitor { primary: m.primary, x: a.x, y: a.y, w: a.w, h: a.h })
            .collect()
    }

    /// From the RandR mode timings of the CRTC showing (x, y).
    fn refresh_at(&self, x: i32, y: i32) -> Option<f32> {
        let res = self.conn.randr_get_screen_resources_current(self.root).ok()?.reply().ok()?;
        for &c in &res.crtcs {
            let Ok(Ok(ci)) = self.conn.randr_get_crtc_info(c, res.config_timestamp).map(|r| r.reply()) else { continue };
            let (cx, cy, cw, ch) = (ci.x as i32, ci.y as i32, ci.width as i32, ci.height as i32);
            if ci.mode == 0 || x < cx || y < cy || x >= cx + cw || y >= cy + ch {
                continue;
            }
            let m = res.modes.iter().find(|m| m.id == ci.mode)?;
            let total = m.htotal as f64 * m.vtotal as f64;
            return (total > 0.0).then(|| (m.dot_clock as f64 / total) as f32);
        }
        None
    }

    /// Managed: not override-redirect, or one of i3's own frames.
    fn windows(&self) -> Vec<Win> {
        self.children().into_iter().map(|c| Win { rect: c.rect, managed: !c.override_redirect || c.i3_frame }).collect()
    }

    /// `_NET_ACTIVE_WINDOW`, if the WM publishes one.
    fn active_window(&self) -> Option<WinId> {
        let r = self.conn.get_property(false, self.root, self.atoms.active_window, xproto::AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        r.value32()?.next().filter(|&w| w != 0)
    }

    /// The X input focus (None and PointerRoot are nobody).
    fn focused_window(&self) -> Option<WinId> {
        Some(self.conn.get_input_focus().ok()?.reply().ok()?.focus).filter(|&w| w > 1)
    }

    /// The child of the root that contains `w`.
    fn toplevel_of(&self, mut w: WinId) -> Option<WinId> {
        for _ in 0..32 {
            let t = self.conn.query_tree(w).ok()?.reply().ok()?;
            if t.parent == self.root || t.parent == x11rb::NONE {
                return Some(w);
            }
            w = t.parent;
        }
        None
    }

    fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        let a = self.conn.get_window_attributes(w).ok()?.reply().ok()?;
        if a.map_state != xproto::MapState::VIEWABLE {
            return None;
        }
        let g = self.conn.get_geometry(w).ok()?.reply().ok()?;
        Some(xrect(w, g.x, g.y, g.width, g.height, g.border_width))
    }

    /// An unmap without a destroy is a workspace switch or a minimise, not a close.
    fn window_exists(&self, w: WinId) -> bool {
        self.conn.get_window_attributes(w).ok().and_then(|c| c.reply().ok()).is_some()
    }

    /// `_NET_WM_STATE_FULLSCREEN`.
    fn is_fullscreen(&self, w: WinId) -> bool {
        self.conn
            .get_property(false, w, self.atoms.wm_state, xproto::AtomEnum::ATOM, 0, 16)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|mut v| v.any(|a| a == self.atoms.wm_state_fullscreen)))
            .unwrap_or(false)
    }

    fn pointer(&self) -> Result<(i32, i32)> {
        let r = self.conn.query_pointer(self.root)?.reply()?;
        Ok((r.root_x as i32, r.root_y as i32))
    }

    fn pointer_reader(&self) -> Result<PointerRead> {
        let (c, screen) = x11rb::connect(None)?;
        let root = c.setup().roots[screen].root;
        Ok(Box::new(move || {
            let r = c.query_pointer(root).ok()?.reply().ok()?;
            Some((r.root_x as i32, r.root_y as i32))
        }))
    }

    fn watch_session(&self, on: Box<dyn Fn(super::SessionEv) + Send>) -> Result<()> {
        super::linux::watch_logind(on)
    }

    /// MIT-SCREEN-SAVER.
    fn idle_ms(&self) -> Option<u32> {
        use x11rb::protocol::screensaver::ConnectionExt as _;
        Some(self.conn.screensaver_query_info(self.root).ok()?.reply().ok()?.ms_since_user_input)
    }

    /// An X auto-repeat sends a release and a press back to back; the key stays down
    /// through it, a real release does not.
    fn char_key_down(&self, ch: char) -> Result<bool> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self.conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
        let per = map.keysyms_per_keycode.max(1) as usize;
        let want = [ch.to_ascii_lowercase() as u32, ch.to_ascii_uppercase() as u32];
        let codes: Vec<u8> = map
            .keysyms
            .chunks(per)
            .enumerate()
            .filter(|(_, syms)| syms.iter().take(2).any(|s| want.contains(s)))
            .map(|(i, _)| min + i as u8)
            .collect();
        let keys = self.conn.query_keymap()?.reply()?.keys;
        Ok(codes.iter().any(|&c| keys[c as usize / 8] & (1 << (c % 8)) != 0))
    }

    fn compositor_running(&self) -> bool {
        self.composited
    }

    /// One thread on the main connection: SubstructureNotify and property changes on the
    /// root, XFixes on the compositor selection, as hints. It also keeps her body above
    /// managed windows (an override-redirect window isn't kept up by the WM).
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        let conn = self.conn.clone();
        let (win, root, atoms) = (self.win(), self.root, self.atoms);
        conn.change_window_attributes(
            root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::SUBSTRUCTURE_NOTIFY | xproto::EventMask::PROPERTY_CHANGE),
        )?;
        if conn.extension_information(xfixes::X11_EXTENSION_NAME)?.is_some() {
            conn.xfixes_query_version(5, 0)?.reply()?;
            conn.xfixes_select_selection_input(
                root,
                atoms.cm,
                xfixes::SelectionEventMask::SET_SELECTION_OWNER
                    | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )?;
        }
        conn.flush()?;
        std::thread::Builder::new().name("x11-watch".into()).spawn(move || {
            // Whether a top-level window may sit above us: menus, launchers and other
            // override-redirect popups may; i3 frames and managed windows may not.
            let mut may_cover: HashMap<u32, bool> = HashMap::new();
            loop {
                let Ok(ev) = conn.wait_for_event() else { return };
                let mut restack = false;
                let handle = |ev: Event, restack: &mut bool, may_cover: &mut HashMap<u32, bool>| match ev {
                    Event::ConfigureNotify(e) if e.window != win => {
                        *restack = true;
                        hint(Hint::Moved(e.window, Some(xrect(e.window, e.x, e.y, e.width, e.height, e.border_width))));
                    }
                    Event::MapNotify(e) if e.window != win => {
                        *restack = true;
                        hint(Hint::Mapped(e.window));
                    }
                    Event::UnmapNotify(e) => hint(Hint::Gone(e.window)),
                    Event::DestroyNotify(e) => {
                        may_cover.remove(&e.window);
                        hint(Hint::Gone(e.window));
                    }
                    Event::PropertyNotify(e) if e.window == root && e.atom == atoms.active_window => hint(Hint::Focus),
                    Event::PropertyNotify(e) if e.window == root && e.atom == atoms.current_desktop => hint(Hint::Desktop),
                    Event::XfixesSelectionNotify(e) if e.selection == atoms.cm => hint(Hint::Compositor(e.owner != x11rb::NONE)),
                    _ => {}
                };
                handle(ev, &mut restack, &mut may_cover);
                while let Ok(Some(ev)) = conn.poll_for_event() {
                    handle(ev, &mut restack, &mut may_cover);
                }
                if restack {
                    keep_above(&conn, win, root, atoms, &mut may_cover).ok();
                }
            }
        })?;
        Ok(())
    }

    /// XInput2 raw button presses on the root (every client may listen, nothing is grabbed
    /// or taken), at the pointer, while armed: only then are they selected, so a closed Line
    /// costs nothing per click. One thread on its own connection, asleep in the X read.
    fn watch_clicks(&self, on_click: Box<dyn Fn(i32, i32) + Send>) -> Result<()> {
        use x11rb::protocol::xinput::ConnectionExt as _;
        let (c, screen) = x11rb::connect(None)?;
        let root = c.setup().roots[screen].root;
        c.xinput_xi_query_version(2, 2)?.reply()?;
        let c = Arc::new(c);
        let esc_codes = escape_codes(&c);
        let (c2, armed2, esc, on_esc): (_, _, _, Escapes) = (c.clone(), self.clicks.clone(), self.escape.clone(), self.on_escape.clone());
        std::thread::Builder::new().name("avatar-clicks".into()).spawn(move || {
            while let Ok(ev) = c2.wait_for_event() {
                match ev {
                    x11rb::protocol::Event::XinputRawButtonPress(_) if armed2.load(Ordering::Relaxed) => {
                        if let Ok(Ok(r)) = c2.query_pointer(root).map(|k| k.reply()) {
                            on_click(r.root_x as i32, r.root_y as i32);
                        }
                    }
                    x11rb::protocol::Event::XinputRawKeyPress(e) if esc.load(Ordering::Relaxed) && esc_codes.contains(&(e.detail as u8)) => {
                        if let Ok(f) = on_esc.lock()
                            && let Some(f) = f.as_ref()
                        {
                            f();
                        }
                    }
                    _ => {}
                }
            }
        })?;
        if let Ok(mut g) = self.click_conn.lock() {
            *g = Some((c, root));
        }
        Ok(())
    }

    fn arm_clicks(&self, on: bool) {
        if self.clicks.swap(on, Ordering::Relaxed) != on {
            self.select_raw();
        }
    }

    fn watch_escape(&self, on: Box<dyn Fn() + Send>) {
        if let Ok(mut g) = self.on_escape.lock() {
            *g = Some(on);
        }
    }

    fn arm_escape(&self, on: bool) {
        if self.escape.swap(on, Ordering::Relaxed) != on {
            self.select_raw();
        }
    }

    fn pointer_image(&self, s: WinId, img: Option<&PointerImage>) {
        if s == 0 {
            return;
        }
        let cursor = match img {
            None => x11rb::NONE,
            Some(i) => match self.make_cursor(i) {
                Ok(c) => c,
                Err(e) => {
                    elog!("synaps-avatar: x11: pointer image: {e:#}");
                    return;
                }
            },
        };
        self.conn.change_window_attributes(s, &xproto::ChangeWindowAttributesAux::new().cursor(cursor)).ok();
        self.conn.flush().ok();
    }
}

impl X11 {
    /// An ARGB cursor from `i` (RENDER), made once per image.
    fn make_cursor(&self, i: &PointerImage) -> Result<u32> {
        let mut g = self.cursor.lock().map_err(|_| anyhow::anyhow!("cursor lock"))?;
        if let Some((k, c)) = *g
            && k == i.key
        {
            return Ok(c);
        }
        let c = &self.conn;
        let formats = c.render_query_pict_formats()?.reply()?;
        let fmt = formats
            .formats
            .iter()
            .find(|f| f.depth == 32 && f.type_ == render::PictType::DIRECT && f.direct.alpha_mask == 0xff && f.direct.alpha_shift == 24 && f.direct.red_shift == 16)
            .context("no ARGB32 picture format")?
            .id;
        let pix = c.generate_id()?;
        c.create_pixmap(32, pix, self.root, i.w, i.h)?;
        let gc = c.generate_id()?;
        c.create_gc(gc, pix, &xproto::CreateGCAux::new())?;
        let bytes: Vec<u8> = i.argb.iter().flat_map(|p| p.to_le_bytes()).collect();
        c.put_image(xproto::ImageFormat::Z_PIXMAP, pix, gc, i.w, i.h, 0, 0, 0, 32, &bytes)?;
        let pic = c.generate_id()?;
        c.render_create_picture(pic, pix, fmt, &render::CreatePictureAux::new())?;
        let cur = c.generate_id()?;
        c.render_create_cursor(cur, pic, i.hot.0, i.hot.1)?;
        c.render_free_picture(pic)?;
        c.free_gc(gc)?;
        c.free_pixmap(pix)?;
        if let Some((_, old)) = g.replace((i.key, cur)) {
            c.free_cursor(old).ok();
        }
        Ok(cur)
    }
}

/// The Stage handle shares the connection (and the body id) with the Desktop.
impl Stage for Arc<X11> {
    /// Override-redirect pop-ups: no WM placement, no decorations, no taskbar entry.
    fn dress(&self, _role: Role, o: &mut WindowOptions) {
        o.kind = WindowKind::PopUp;
    }

    /// The XID from GPUI's raw handle; the body also gets its compositor hints.
    fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        let xid = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
            Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
            other => bail!("not an X11 window ({other:?}); phase 1 supports X11 only"),
        };
        if role == Role::Body {
            self.win.store(xid, Ordering::Relaxed);
            self.set_compositor_hints().ok();
        }
        Ok(xid)
    }

    fn body(&self) -> WinId {
        self.win()
    }

    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.conn.configure_window(self.win(), &xproto::ConfigureWindowAux::new().x(x).y(y))?;
        self.conn.flush()?;
        Ok(())
    }

    fn raise(&self) -> Result<()> {
        self.conn.configure_window(self.win(), &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE))?;
        self.conn.flush()?;
        Ok(())
    }

    fn place(&self, s: WinId, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        let aux = xproto::ConfigureWindowAux::new().x(x).y(y).width(width.max(1) as u32).height(height.max(1) as u32);
        self.conn.configure_window(s, &aux)?;
        self.conn.flush()?;
        Ok(())
    }

    /// SHAPE input; SHAPE bounding for the visible region (none: the whole window).
    fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<Visible<'_>>) -> Result<()> {
        self.shape_win(self.win(), input, visible)
    }

    fn shape_of(&self, s: WinId, input: &[ShapeRect], visible: Visible<'_>) -> Result<()> {
        self.shape_win(s, Some(input), Some(visible))
    }

    /// `_NET_WM_ICON`: (w, h, ARGB pixels…) per size.
    fn set_icon(&self, s: WinId, data: &[u32]) -> Result<()> {
        let atom = intern(&self.conn, "_NET_WM_ICON")?;
        self.conn.change_property32(xproto::PropMode::REPLACE, s, atom, xproto::AtomEnum::CARDINAL, data)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Server-painted override-redirect windows: `background_pixel` and a bounding shape, an
    /// empty input shape.
    fn fx_show(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()> {
        let mut fx = self.fx.lock().map_err(|_| anyhow::anyhow!("fx lock"))?;
        while fx.len() <= layer {
            let id = self.conn.generate_id()?;
            self.conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                id,
                self.root,
                0,
                0,
                1,
                1,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &xproto::CreateWindowAux::new().background_pixel(rgb).override_redirect(1),
            )?;
            self.conn.change_property8(xproto::PropMode::REPLACE, id, xproto::AtomEnum::WM_CLASS, xproto::AtomEnum::STRING, b"synaps-avatar-fx\0synaps-avatar-fx\0")?;
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, id, 0, 0, &[])?;
            fx.push((id, false, rgb));
        }
        let (id, mapped, colour) = fx[layer];
        if colour != rgb {
            // Layers are created lazily in a batch, and some are reused: keep the colour current.
            self.conn.change_window_attributes(id, &xproto::ChangeWindowAttributesAux::new().background_pixel(rgb))?;
            if mapped {
                self.conn.clear_area(false, id, 0, 0, 0, 0)?;
            }
            fx[layer].2 = rgb;
        }
        if let Some(rects) = rects {
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, xproto::ClipOrdering::UNSORTED, id, 0, 0, &xrects(rects))?;
        }
        let aux = xproto::ConfigureWindowAux::new().x(origin.0).y(origin.1).width(size.0.max(1) as u32).height(size.1.max(1) as u32);
        let aux = match z {
            FxZ::Top => aux.stack_mode(xproto::StackMode::ABOVE),
            FxZ::UnderBody => aux.sibling(self.win()).stack_mode(xproto::StackMode::BELOW),
            FxZ::Below(j) => match fx.get(j) {
                Some(&(sib, true, _)) => aux.sibling(sib).stack_mode(xproto::StackMode::BELOW),
                _ => aux.stack_mode(xproto::StackMode::ABOVE),
            },
        };
        self.conn.configure_window(id, &aux)?;
        if !mapped {
            self.conn.map_window(id)?;
            fx[layer].1 = true;
        }
        Ok(())
    }

    fn fx_hide(&self, layer: usize) {
        if let Ok(mut fx) = self.fx.lock()
            && let Some(f) = fx.get_mut(layer)
            && f.1
        {
            self.conn.unmap_window(f.0).ok();
            f.1 = false;
        }
    }

    fn fx_hide_all(&self) {
        if let Ok(mut fx) = self.fx.lock() {
            for f in fx.iter_mut().filter(|f| f.1) {
                self.conn.unmap_window(f.0).ok();
                f.1 = false;
            }
            self.conn.flush().ok();
        }
    }

    fn flush(&self) {
        self.conn.flush().ok();
    }

    /// SetInputFocus (override-redirect windows never get it from the WM).
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        self.conn.set_input_focus(xproto::InputFocus::PARENT, s, x11rb::CURRENT_TIME)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Hand the keyboard to `to` (closing or unmapping `s` lets the WM pick otherwise).
    fn release_keyboard(&self, _s: WinId, to: Option<WinId>) -> Result<()> {
        if let Some(to) = to {
            self.grab_keyboard(to)?;
        }
        Ok(())
    }
}

/// Raise her body over the first viewable window above it that may not cover her.
fn keep_above(conn: &RustConnection, win: u32, root: u32, atoms: Atoms, may_cover: &mut HashMap<u32, bool>) -> Result<()> {
    let tree = conn.query_tree(root)?.reply()?;
    let Some(pos) = tree.children.iter().position(|&w| w == win) else { return Ok(()) };
    for &w in &tree.children[pos + 1..] {
        let allowed = match may_cover.get(&w) {
            Some(&a) => a,
            None => {
                let attrs = conn.get_window_attributes(w)?.reply()?;
                let class = conn.get_property(false, w, atoms.wm_class, xproto::AtomEnum::STRING, 0, 64)?.reply().map(|r| r.value).unwrap_or_default();
                let a = attrs.override_redirect && !is_i3_frame(&class);
                may_cover.insert(w, a);
                a
            }
        };
        if allowed {
            continue;
        }
        let mapped = conn.get_window_attributes(w)?.reply()?.map_state == xproto::MapState::VIEWABLE;
        if mapped {
            conn.configure_window(win, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE))?;
            conn.flush()?;
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u32, x: i32, y: i32, ww: i32, h: i32) -> WinRect {
        WinRect { id, x, y, w: ww, h }
    }

    #[test]
    fn work_area_from_top_struts() {
        // bella: two monitors side by side, polybar (29 px) at the top of each.
        let mons = [w(0, 0, 0, 2560, 1600), w(1, 2560, 0, 2560, 1440)];
        let root = (5120, 1600);
        let s1 = strut_partial(&[0, 0, 29, 0, 0, 0, 0, 0, 2560, 5119, 0, 0]).unwrap();
        let s2 = strut_partial(&[0, 0, 29, 0, 0, 0, 0, 0, 0, 2559, 0, 0]).unwrap();
        let a = work_areas(&mons, root, &[s1, s2]);
        assert_eq!(a[0], w(0, 0, 29, 2560, 1571));
        assert_eq!(a[1], w(1, 2560, 29, 2560, 1411));
        // Only one bar: the other monitor keeps its top.
        let a = work_areas(&mons, root, &[s1]);
        assert_eq!(a[0].y, 0);
        assert_eq!(a[1].y, 29);
        // A dock with geometry only.
        let d = strut_from_dock(&w(9, 2560, 0, 2560, 29), &mons, root).unwrap();
        assert_eq!(work_areas(&mons, root, &[d])[1].y, 29);
        // A bottom bar on the shorter monitor (struts count from the root's bottom).
        let b = strut_partial(&[0, 0, 0, 160 + 30, 0, 0, 0, 0, 0, 0, 2560, 5119]).unwrap();
        assert_eq!(work_areas(&mons, root, &[b])[1].bottom(), 1410);
        assert_eq!(work_areas(&mons, root, &[b])[0].bottom(), 1600);
        assert_eq!(strut_plain(&[0, 0, 29, 0], root).unwrap().top_x, (0, 5119));
    }
}
