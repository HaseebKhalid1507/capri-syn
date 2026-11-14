//! X11 shim for what gpui-pre 0.3.12 doesn't expose on X11: input/bounding shape
//! (click-through), absolute moves, stacking, monitors, compositor detection and a
//! focus request. Uses its own x11rb connection; the window id comes from GPUI's
//! `HasWindowHandle`.

use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xfixes::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub use crate::shape::ShapeRect;

#[derive(Clone, Debug)]
pub struct Monitor {
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Monitor {
    pub fn rect(&self) -> WinRect {
        WinRect::new(self.x, self.y, self.w, self.h)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    fn distance2(&self, x: i32, y: i32) -> i64 {
        let dx = (self.x - x).max(0).max(x - (self.x + self.w - 1)) as i64;
        let dy = (self.y - y).max(0).max(y - (self.y + self.h - 1)) as i64;
        dx * dx + dy * dy
    }
}

pub use crate::edges::WinRect;
use crate::edges::{self, Strut};

/// What the watcher thread reports.
#[derive(Clone, Copy, Debug)]
pub enum WatchEvent {
    Compositor(bool),
    /// The perch target moved or resized.
    TargetMoved(WinRect),
    /// The perch target was unmapped or destroyed.
    TargetGone(u32),
    /// The target window is mapped again (a workspace flicked back).
    TargetBack(u32),
}

pub struct X11 {
    conn: Arc<RustConnection>,
    pub win: u32,
    root: u32,
    pub screen_w: u16,
    pub screen_h: u16,
    /// A compositor owns `_NET_WM_CM_S{screen}` (kept current by the watcher).
    composited: Arc<AtomicBool>,
    /// The mode forced by `SA_COMPOSITED` or settings.toml `[display] composited`: 0 = follow
    /// the compositor, 1 = shape (no alpha), 2 = alpha.
    force: AtomicU8,
    atoms: Atoms,
    /// Top-level window the watcher follows (0 = none).
    target: Arc<AtomicU32>,
    /// Effect windows (teleport bolt layers), created on first use: (window, mapped).
    /// Effect layers: (window, mapped, background colour).
    fx: Mutex<Vec<(u32, bool, u32)>>,
    /// Work areas as last computed, and when (they cost a stack walk: cached briefly).
    areas: Mutex<Option<(std::time::Instant, Vec<Monitor>)>>,
    /// Monitors as last read (a RandR round trip: cached like the work areas).
    mons: Mutex<Option<(std::time::Instant, Vec<Monitor>)>>,
    /// While she's carried or flying: both caches stay as snapshotted at the grab (no round
    /// trip inside a 60 Hz tick).
    frozen: AtomicBool,
    /// The pointer polled on its own connection while she's held by the real mouse:
    /// (x << 32 | y as u32, or u64::MAX before the first read; stop flag).
    poll: Mutex<Option<(Arc<std::sync::atomic::AtomicU64>, Arc<AtomicBool>)>>,
    /// Bounding shapes sent, and their rects (for the rag's meter).
    shape_sends: std::sync::atomic::AtomicU64,
    shape_rects: std::sync::atomic::AtomicU64,
}

/// One viewable root child, as `stack()` sees it.
#[derive(Clone, Debug)]
struct Child {
    rect: WinRect,
    override_redirect: bool,
    i3_frame: bool,
}

/// How long `work_areas()` reuses its last answer.
const AREAS_TTL: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Clone, Copy)]
struct Atoms {
    cm: u32,
    wm_class: u32,
    gtk_frame_extents: u32,
    compton_shadow: u32,
    active_window: u32,
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

#[derive(Clone, Copy)]
enum Stack {
    Top,
    BelowAvatar,
    BelowLayer(usize),
}

impl X11 {
    /// Connect; `win` is set once GPUI has created the window.
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
            win: 0,
            root,
            screen_w,
            screen_h,
            composited: Arc::new(AtomicBool::new(composited)),
            force: AtomicU8::new(0),
            atoms,
            target: Arc::new(AtomicU32::new(0)),
            fx: Mutex::new(Vec::new()),
            areas: Mutex::new(None),
            mons: Mutex::new(None),
            frozen: AtomicBool::new(false),
            poll: Mutex::new(None),
            shape_sends: std::sync::atomic::AtomicU64::new(0),
            shape_rects: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// Alpha mode: the window's own alpha is what shows (a compositor blends it), so no
    /// bounding shape and nothing painted that only a shape would hide. Else shape mode.
    pub fn composited(&self) -> bool {
        match self.force.load(Ordering::Relaxed) {
            1 => false,
            2 => true,
            _ => self.composited.load(Ordering::Relaxed),
        }
    }

    /// Whether a compositor is running, whatever the mode.
    pub fn compositor_running(&self) -> bool {
        self.composited.load(Ordering::Relaxed)
    }

    /// Force a mode (`None`: follow the compositor). True if the effective mode changed.
    pub fn force_composited(&self, mode: Option<bool>) -> bool {
        let before = self.composited();
        self.force.store(mode.map_or(0, |m| if m { 2 } else { 1 }), Ordering::Relaxed);
        before != self.composited()
    }

    /// What forces the mode: `SA_COMPOSITED=0|1` (env) wins over the settings key.
    pub fn forced_mode(&self) -> Option<bool> {
        match self.force.load(Ordering::Relaxed) {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }

    /// Properties for the compositor: a `_GTK_FRAME_EXTENTS` of zero keeps picom's stock
    /// `blur-background-exclude = [ "_GTK_FRAME_EXTENTS@" ]` from frosting our bounding box.
    pub fn set_compositor_hints(&self) -> Result<()> {
        self.conn.change_property32(
            xproto::PropMode::REPLACE,
            self.win,
            self.atoms.gtk_frame_extents,
            xproto::AtomEnum::CARDINAL,
            &[0, 0, 0, 0],
        )?;
        // picom/compton: no drop shadow around the transparent window.
        self.conn.change_property32(
            xproto::PropMode::REPLACE,
            self.win,
            self.atoms.compton_shadow,
            xproto::AtomEnum::CARDINAL,
            &[0],
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Input region = `input`; everything else clicks through to the windows below.
    /// Without a compositor the bounding shape is set to `visible`, so no black box shows.
    pub fn set_shape(&self, input: &[ShapeRect], visible: &[ShapeRect]) -> Result<()> {
        self.set_shapes(Some(input), Some(visible))
    }

    /// Send only the shapes given (each `None` stays as it is): a bounding change no longer
    /// resends the input shape, nor the other way round.
    pub fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<&[ShapeRect]>) -> Result<()> {
        let conv = |r: &[ShapeRect]| -> Vec<xproto::Rectangle> {
            r.iter().map(|&(x, y, width, height)| xproto::Rectangle { x, y, width, height }).collect()
        };
        if let Some(input) = input {
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, xproto::ClipOrdering::UNSORTED, self.win, 0, 0, &conv(input))?;
        }
        if let Some(visible) = visible {
            if self.composited() {
                self.conn.shape_mask(shape::SO::SET, shape::SK::BOUNDING, self.win, 0, 0, x11rb::NONE)?;
            } else {
                self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, xproto::ClipOrdering::UNSORTED, self.win, 0, 0, &conv(visible))?;
                self.shape_sends.fetch_add(1, Ordering::Relaxed);
                self.shape_rects.fetch_add(visible.len() as u64, Ordering::Relaxed);
            }
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Bounding shapes sent and their total rects since the last call (the rag's meter).
    pub fn take_shape_stats(&self) -> (u64, u64) {
        (self.shape_sends.swap(0, Ordering::Relaxed), self.shape_rects.swap(0, Ordering::Relaxed))
    }

    pub fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.conn.configure_window(self.win, &xproto::ConfigureWindowAux::new().x(x).y(y))?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn raise(&self) -> Result<()> {
        self.conn
            .configure_window(self.win, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE))?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn pointer(&self) -> Result<(i32, i32)> {
        let r = self.conn.query_pointer(self.root)?.reply()?;
        Ok((r.root_x as i32, r.root_y as i32))
    }

    /// Ask the server for keyboard focus (override-redirect windows never get it from the WM).
    pub fn take_focus(&self) -> Result<()> {
        self.conn.set_input_focus(xproto::InputFocus::PARENT, self.win, x11rb::CURRENT_TIME)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Hand the keyboard to `w` (back to the window that had it).
    pub fn give_focus(&self, w: u32) -> Result<()> {
        self.conn.set_input_focus(xproto::InputFocus::PARENT, w, x11rb::CURRENT_TIME)?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn focused_window(&self) -> Result<u32> {
        Ok(self.conn.get_input_focus()?.reply()?.focus)
    }

    // ---- other windows (perching) -------------------------------------------------

    /// The focused client (`_NET_ACTIVE_WINDOW`), if the WM publishes one.
    pub fn active_window(&self) -> Option<u32> {
        let r = self.conn.get_property(false, self.root, self.atoms.active_window, xproto::AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
        r.value32()?.next().filter(|&w| w != 0)
    }

    /// Milliseconds since the last keyboard or mouse input anywhere (MIT-SCREEN-SAVER).
    pub fn idle_ms(&self) -> Option<u32> {
        use x11rb::protocol::screensaver::ConnectionExt as _;
        Some(self.conn.screensaver_query_info(self.root).ok()?.reply().ok()?.ms_since_user_input)
    }

    /// `w` (a client) asks for fullscreen (`_NET_WM_STATE_FULLSCREEN`).
    pub fn is_fullscreen(&self, w: u32) -> bool {
        self.conn
            .get_property(false, w, self.atoms.wm_state, xproto::AtomEnum::ATOM, 0, 16)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|mut v| v.any(|a| a == self.atoms.wm_state_fullscreen)))
            .unwrap_or(false)
    }

    /// The child of the root that contains `w` (the WM frame for a managed client).
    pub fn toplevel_of(&self, mut w: u32) -> Option<u32> {
        for _ in 0..32 {
            let t = self.conn.query_tree(w).ok()?.reply().ok()?;
            if t.parent == self.root || t.parent == x11rb::NONE {
                return Some(w);
            }
            w = t.parent;
        }
        None
    }

    /// Outer rect of a top-level window, if it is mapped and viewable.
    pub fn toplevel_rect(&self, w: u32) -> Option<WinRect> {
        let a = self.conn.get_window_attributes(w).ok()?.reply().ok()?;
        if a.map_state != xproto::MapState::VIEWABLE {
            return None;
        }
        let g = self.conn.get_geometry(w).ok()?.reply().ok()?;
        let b = g.border_width as i32;
        Some(WinRect { id: w, x: g.x as i32, y: g.y as i32, w: g.width as i32 + 2 * b, h: g.height as i32 + 2 * b })
    }

    /// Viewable root children, bottom → top, except ours, our effect layers and InputOnly
    /// windows. Small and override-redirect ones stay (they occlude).
    fn children(&self) -> Vec<Child> {
        let Ok(Ok(tree)) = self.conn.query_tree(self.root).map(|c| c.reply()) else { return Vec::new() };
        let fx: Vec<u32> = self.fx.lock().map(|v| v.iter().map(|f| f.0).collect()).unwrap_or_default();
        let cookies: Vec<_> = tree
            .children
            .iter()
            .filter(|&&w| w != self.win && !fx.contains(&w))
            .filter_map(|&w| Some((w, self.conn.get_window_attributes(w).ok()?, self.conn.get_geometry(w).ok()?)))
            .collect();
        let viewable: Vec<(WinRect, bool)> = cookies
            .into_iter()
            .filter_map(|(w, a, g)| {
                let (a, g) = (a.reply().ok()?, g.reply().ok()?);
                if a.map_state != xproto::MapState::VIEWABLE || a.class == xproto::WindowClass::INPUT_ONLY {
                    return None;
                }
                let b = g.border_width as i32;
                Some((WinRect { id: w, x: g.x as i32, y: g.y as i32, w: g.width as i32 + 2 * b, h: g.height as i32 + 2 * b }, a.override_redirect))
            })
            .collect();
        // i3 frames are override-redirect too: tell them from popups by class.
        let class: Vec<_> = viewable
            .iter()
            .map(|(r, or)| {
                (*or && r.w >= 120 && r.h >= 60)
                    .then(|| self.conn.get_property(false, r.id, self.atoms.wm_class, xproto::AtomEnum::STRING, 0, 64).ok())
                    .flatten()
            })
            .collect();
        viewable
            .into_iter()
            .zip(class)
            .map(|((rect, or), c)| {
                let i3_frame = c.and_then(|c| c.reply().ok()).is_some_and(|r| r.value.windows(8).any(|s| s == b"i3-frame"));
                Child { rect, override_redirect: or, i3_frame }
            })
            .collect()
    }

    /// Every viewable root child, bottom → top (see `children`), and whether it is a
    /// window she may sit on: at least 120×60 and managed (an override-redirect popup or
    /// menu only occludes; i3's own frames count as managed).
    pub fn stack(&self) -> Vec<(WinRect, bool)> {
        self.children()
            .into_iter()
            .map(|c| {
                let big = c.rect.w >= 120 && c.rect.h >= 60;
                (c.rect, big && (!c.override_redirect || c.i3_frame))
            })
            .collect()
    }

    /// Each monitor minus the docks' struts (`_NET_WM_STRUT_PARTIAL`, `_NET_WM_STRUT`, or a
    /// `_NET_WM_WINDOW_TYPE_DOCK` window's own geometry). i3 sets no `_NET_WORKAREA`, and its
    /// docks sit inside i3 frames, so the frames' children are read too. Cached for 3 s;
    /// on any failure, the monitors.
    pub fn work_areas(&self) -> Vec<Monitor> {
        if let Ok(c) = self.areas.lock()
            && let Some((t, a)) = c.as_ref()
            && (t.elapsed() < AREAS_TTL || self.frozen.load(Ordering::Relaxed))
        {
            return a.clone();
        }
        let mons = self.monitors();
        let rects: Vec<WinRect> = mons.iter().map(Monitor::rect).collect();
        let root = (self.screen_w as i32, self.screen_h as i32);
        let struts = self.struts(&rects, root);
        let areas: Vec<Monitor> = edges::work_areas(&rects, root, &struts)
            .into_iter()
            .zip(&mons)
            .map(|(a, m)| Monitor { primary: m.primary, x: a.x, y: a.y, w: a.w, h: a.h })
            .collect();
        if let Ok(mut c) = self.areas.lock() {
            *c = Some((std::time::Instant::now(), areas.clone()));
        }
        areas
    }

    /// The work area containing (x, y), else the nearest one.
    pub fn area_at(&self, x: i32, y: i32) -> Monitor {
        let ms = self.work_areas();
        ms.iter()
            .find(|m| m.contains(x, y))
            .cloned()
            .unwrap_or_else(|| ms.iter().min_by_key(|m| m.distance2(x, y)).cloned().expect("a monitor"))
    }

    pub fn primary_area(&self) -> Monitor {
        let ms = self.work_areas();
        ms.iter().find(|m| m.primary).cloned().unwrap_or_else(|| ms[0].clone())
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
            if let Some(st) = Strut::partial(&p).or_else(|| Strut::plain(&s, root)) {
                out.push(st);
            } else if t.contains(&a.wm_type_dock)
                && let Some(st) = Strut::from_dock(&r, mons, root)
            {
                out.push(st);
            }
        }
        out
    }

    /// Follow `w` (0 = nothing) in the watcher thread.
    pub fn set_target(&self, w: u32) {
        self.target.store(w, Ordering::Relaxed);
    }

    // ---- effect windows -----------------------------------------------------------

    /// Show effect layer `layer` (solid `rgb`) on top of everything at root `origin` with
    /// the bounding shape `rects` (relative to `origin`). Input passes through. Created on
    /// first use. Not flushed, so several layers go out in one write.
    pub fn fx_set(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: &[ShapeRect]) -> Result<()> {
        self.fx_place(layer, rgb, origin, size, Some(rects), Stack::Top)
    }

    /// Like `fx_set`, stacked directly below layer `over`. Stacking a group top-down this
    /// way never puts a lower layer above a higher one, even for a moment (raising each
    /// to the top in turn does, and a screen grab catches it).
    pub fn fx_set_below(&self, layer: usize, over: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: &[ShapeRect]) -> Result<()> {
        self.fx_place(layer, rgb, origin, size, Some(rects), Stack::BelowLayer(over))
    }

    /// An effect layer stacked directly below the avatar window (e.g. a contact shadow).
    /// `rects: None` keeps the current shape and only moves the window.
    pub fn fx_under(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>) -> Result<()> {
        self.fx_place(layer, rgb, origin, size, rects, Stack::BelowAvatar)?;
        self.conn.flush()?;
        Ok(())
    }

    fn fx_place(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, stack: Stack) -> Result<()> {
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
            self.conn.change_property8(
                xproto::PropMode::REPLACE,
                id,
                xproto::AtomEnum::WM_CLASS,
                xproto::AtomEnum::STRING,
                b"synaps-avatar-fx\0synaps-avatar-fx\0",
            )?;
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
        if rects.is_some_and(|r| r.is_empty()) {
            if mapped {
                self.conn.unmap_window(id)?;
                fx[layer].1 = false;
            }
            return Ok(());
        }
        if let Some(rects) = rects {
            let r: Vec<xproto::Rectangle> = rects.iter().map(|&(x, y, width, height)| xproto::Rectangle { x, y, width, height }).collect();
            self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, xproto::ClipOrdering::UNSORTED, id, 0, 0, &r)?;
        }
        let aux = xproto::ConfigureWindowAux::new().x(origin.0).y(origin.1).width(size.0.max(1) as u32).height(size.1.max(1) as u32);
        let aux = match stack {
            Stack::Top => aux.stack_mode(xproto::StackMode::ABOVE),
            Stack::BelowAvatar => aux.sibling(self.win).stack_mode(xproto::StackMode::BELOW),
            Stack::BelowLayer(j) => match fx.get(j) {
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

    pub fn flush(&self) {
        self.conn.flush().ok();
    }

    /// Unmap one effect layer if it exists and is up.
    pub fn fx_unmap(&self, layer: usize) {
        if let Ok(mut fx) = self.fx.lock()
            && let Some(f) = fx.get_mut(layer)
            && f.1
        {
            self.conn.unmap_window(f.0).ok();
            f.1 = false;
        }
    }

    pub fn fx_hide(&self) {
        if let Ok(mut fx) = self.fx.lock() {
            for f in fx.iter_mut().filter(|f| f.1) {
                self.conn.unmap_window(f.0).ok();
                f.1 = false;
            }
            self.conn.flush().ok();
        }
    }

    /// The monitors (cached for `AREAS_TTL`, or frozen while she's carried).
    pub fn monitors(&self) -> Vec<Monitor> {
        if let Ok(c) = self.mons.lock()
            && let Some((t, m)) = c.as_ref()
            && (t.elapsed() < AREAS_TTL || self.frozen.load(Ordering::Relaxed))
        {
            return m.clone();
        }
        let out = self.read_monitors();
        if let Ok(mut c) = self.mons.lock() {
            *c = Some((std::time::Instant::now(), out.clone()));
        }
        out
    }

    /// Carried (on) or back to normal (off): on takes a fresh snapshot of the monitors and
    /// work areas and keeps it until off.
    pub fn freeze_layout(&self, on: bool) {
        // A snapshot younger than ~1 s is fresh enough (the refresh walks the tree: a few ms
        // on the first dragged frame).
        let fresh = |m: &Mutex<Option<(std::time::Instant, Vec<Monitor>)>>| {
            m.lock().ok().is_some_and(|c| c.as_ref().is_some_and(|(t, _)| t.elapsed() < std::time::Duration::from_secs(1)))
        };
        if on && !(fresh(&self.mons) && fresh(&self.areas)) {
            self.frozen.store(false, Ordering::Relaxed);
            if let Ok(mut c) = self.mons.lock() {
                *c = None;
            }
            if let Ok(mut c) = self.areas.lock() {
                *c = None;
            }
            self.work_areas();
        }
        self.frozen.store(on, Ordering::Relaxed);
    }

    /// Poll the pointer on a thread with its own connection (250 Hz) while on, so a held
    /// tick reads an atomic instead of a QueryPointer round trip.
    pub fn poll_pointer(&self, on: bool) {
        let Ok(mut g) = self.poll.lock() else { return };
        if !on {
            if let Some((_, stop)) = g.take() {
                stop.store(true, Ordering::Relaxed);
            }
            return;
        }
        if g.is_some() {
            return;
        }
        let pos = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (pos.clone(), stop.clone());
        let spawned = std::thread::Builder::new().name("avatar-pointer".into()).spawn(move || {
            let Ok((c, screen)) = x11rb::connect(None) else { return };
            let root = c.setup().roots[screen].root;
            while !s.load(Ordering::Relaxed) {
                if let Ok(Ok(r)) = c.query_pointer(root).map(|k| k.reply()) {
                    p.store(((r.root_x as i32 as u32 as u64) << 32) | r.root_y as i32 as u32 as u64, Ordering::Relaxed);
                }
                // 2 ms: the window follows it at up to the monitor's refresh (see rag_follow).
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        if spawned.is_ok() {
            *g = Some((pos, stop));
        }
    }

    /// The polled pointer while polling (and read once), else a QueryPointer.
    pub fn pointer_fast(&self) -> Result<(i32, i32)> {
        if let Ok(g) = self.poll.lock()
            && let Some((pos, _)) = g.as_ref()
        {
            let v = pos.load(Ordering::Relaxed);
            if v != u64::MAX {
                return Ok(((v >> 32) as u32 as i32, v as u32 as i32));
            }
        }
        self.pointer()
    }

    /// Refresh rate (Hz) of the CRTC showing (x, y), from its RandR mode timings; 60 when
    /// unknown. A few round trips: called once per drag, at the grab.
    pub fn refresh_at(&self, x: i32, y: i32) -> f32 {
        let get = || -> Option<f32> {
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
        };
        get().filter(|r| (20.0..=500.0).contains(r)).unwrap_or(60.0)
    }

    fn read_monitors(&self) -> Vec<Monitor> {
        let mut out = Vec::new();
        if let Ok(Ok(r)) = self.conn.randr_get_monitors(self.root, true).map(|c| c.reply()) {
            for m in r.monitors {
                out.push(Monitor {
                    primary: m.primary,
                    x: m.x as i32,
                    y: m.y as i32,
                    w: m.width as i32,
                    h: m.height as i32,
                });
            }
        }
        if out.is_empty() {
            out.push(Monitor {
                primary: true,
                x: 0,
                y: 0,
                w: self.screen_w as i32,
                h: self.screen_h as i32,
            });
        }
        out
    }

    /// The monitor containing (x, y), else the nearest one.
    pub fn monitor_at(&self, x: i32, y: i32) -> Monitor {
        let ms = self.monitors();
        ms.iter()
            .find(|m| m.contains(x, y))
            .cloned()
            .unwrap_or_else(|| ms.iter().min_by_key(|m| m.distance2(x, y)).cloned().expect("a monitor"))
    }

    /// Background thread on the same connection: keeps the avatar above managed windows,
    /// tracks the compositor appearing or going away, and follows the perch target.
    /// `on_event` runs on that thread.
    pub fn spawn_watcher(&self, on_event: impl Fn(WatchEvent) + Send + 'static) -> Result<()> {
        let conn = self.conn.clone();
        let (win, root, atoms, composited) = (self.win, self.root, self.atoms, self.composited.clone());
        let target = self.target.clone();
        conn.change_window_attributes(
            root,
            &xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::SUBSTRUCTURE_NOTIFY),
        )?;
        let have_xfixes = conn.extension_information(xfixes::X11_EXTENSION_NAME)?.is_some();
        if have_xfixes {
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
                let handle = |ev: Event, restack: &mut bool, may_cover: &mut HashMap<u32, bool>| {
                    let t = target.load(Ordering::Relaxed);
                    match ev {
                        Event::ConfigureNotify(e) if e.window != win => {
                            *restack = true;
                            if t != 0 && e.window == t {
                                let b = e.border_width as i32;
                                on_event(WatchEvent::TargetMoved(WinRect {
                                    id: t,
                                    x: e.x as i32,
                                    y: e.y as i32,
                                    w: e.width as i32 + 2 * b,
                                    h: e.height as i32 + 2 * b,
                                }));
                            }
                        }
                        Event::MapNotify(e) if e.window != win => {
                            *restack = true;
                            if t != 0 && e.window == t {
                                on_event(WatchEvent::TargetBack(t));
                            }
                        }
                        Event::UnmapNotify(e) if t != 0 && e.window == t => on_event(WatchEvent::TargetGone(t)),
                        Event::DestroyNotify(e) => {
                            may_cover.remove(&e.window);
                            if t != 0 && e.window == t {
                                on_event(WatchEvent::TargetGone(t));
                            }
                        }
                        Event::XfixesSelectionNotify(e) if e.selection == atoms.cm => {
                            let now = e.owner != x11rb::NONE;
                            if composited.swap(now, Ordering::Relaxed) != now {
                                on_event(WatchEvent::Compositor(now));
                            }
                        }
                        _ => {}
                    }
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
}

fn keep_above(conn: &RustConnection, win: u32, root: u32, atoms: Atoms, may_cover: &mut HashMap<u32, bool>) -> Result<()> {
    let tree = conn.query_tree(root)?.reply()?;
    let Some(pos) = tree.children.iter().position(|&w| w == win) else { return Ok(()) };
    for &w in &tree.children[pos + 1..] {
        let allowed = match may_cover.get(&w) {
            Some(&a) => a,
            None => {
                let attrs = conn.get_window_attributes(w)?.reply()?;
                let class = conn
                    .get_property(false, w, atoms.wm_class, xproto::AtomEnum::STRING, 0, 64)?
                    .reply()
                    .map(|r| r.value)
                    .unwrap_or_default();
                let i3_frame = class.windows(8).any(|s| s == b"i3-frame");
                let a = attrs.override_redirect && !i3_frame;
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
