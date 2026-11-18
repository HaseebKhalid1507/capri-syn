//! The core over whichever backend: every desktop decision, once. A backend's answers come
//! in as plain data (`Desktop`) and go out as primitives (`Stage`); the rules live here.

use super::settle::Settle;
use super::track::Track;
use super::{Caps, Desktop, FxZ, Hint, Monitor, MoveAnim, PointerCfg, PointerEv, Role, SEAT_MIN, ShapeRect, Stage, Visible, WatchEvent, WinId, WinRect};
use anyhow::Result;
use gpui_kit::{Bounds, Pixels, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions, fill, point, px, rgb, size};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long monitors and work areas are reused (work areas cost a stack walk).
const AREAS_TTL: Duration = Duration::from_secs(3);
/// Backends without target events: how often the perch target is re-read.
/// The target poller's rates: a window that can move without telling (floating), one that
/// moves only with events the backend hints (tiled), and while it moves (a live drag).
const TARGET_POLL_LOOSE: Duration = Duration::from_millis(250);
const TARGET_POLL_TILED: Duration = Duration::from_millis(500);
const TARGET_POLL_BURST: Duration = Duration::from_millis(16);
/// How long the burst rate lasts after the target was last seen moving.
const TARGET_BURST_FOR: Duration = Duration::from_millis(600);

/// What the watcher threads share with the core.
struct Shared {
    /// The window the watcher follows (0 = none).
    target: AtomicU32,
    /// A compositor runs (kept current by the watcher).
    composited: AtomicBool,
    /// It takes a change of her window's shape for a resize (`Desktop::compositor_reshapes`).
    reshapes: AtomicBool,
    /// The target poller (backends without target events), woken by a new target.
    poller: OnceLock<std::thread::Thread>,
    /// The space the target was first seen on (`Place::space`; `NO_SPACE`: not yet).
    space: AtomicI64,
    /// Bumped by every `Hint::Layout` (the cached layout is stale).
    layout_gen: AtomicU64,
    /// When the target's last move (as the poller found it) likely began: halfway through the
    /// sleep it slept out, or when a hint woke it (the move had just happened).
    moved_at: Mutex<Option<Instant>>,
}

const NO_SPACE: i64 = i64::MIN;

impl Shared {
    /// Wake the target poller (if any, and following something): re-read at once.
    fn poll_now(&self) {
        if self.target.load(Ordering::Relaxed) != 0
            && let Some(p) = self.poller.get()
        {
            p.unpark();
        }
    }

    /// The one change-detection path: a backend's hint (or the poller's diff) → what the
    /// view hears.
    fn hint(&self, desk: &dyn Desktop, h: Hint, on: &dyn Fn(WatchEvent)) {
        let t = self.target.load(Ordering::Relaxed);
        match h {
            Hint::Moved(w, r) if t != 0 && w == t => {
                if let Some(r) = r.or_else(|| desk.toplevel_rect(w)) {
                    on(WatchEvent::TargetMoved(r));
                }
            }
            Hint::Mapped(w) if t != 0 && w == t => on(WatchEvent::TargetBack(t)),
            Hint::Gone(w) if t != 0 && w == t => on(WatchEvent::TargetGone(t)),
            Hint::Focus => {
                self.poll_now();
                on(WatchEvent::Focus);
            }
            Hint::Desktop => {
                self.poll_now();
                on(WatchEvent::Desktop);
            }
            Hint::Changed => self.poll_now(),
            Hint::Layout => {
                self.layout_gen.fetch_add(1, Ordering::Relaxed);
                self.poll_now();
                on(WatchEvent::Layout);
            }
            Hint::Compositor { runs, reshapes } => {
                self.reshapes.store(reshapes, Ordering::Relaxed);
                if self.composited.swap(runs, Ordering::Relaxed) != runs {
                    on(WatchEvent::Compositor(runs));
                }
            }
            _ => {}
        }
    }
}

/// The pointer watcher's view of her: her body box (root px, w = 0: none) and whether it
/// reports at all.
#[derive(Default)]
struct PointerWatch {
    her: Mutex<WinRect>,
    armed: AtomicBool,
    /// Milliseconds (since the watcher started) of the last pointer motion, u64::MAX: none yet.
    moved_ms: AtomicU64,
    started: Mutex<Option<Instant>>,
}

/// A cached layout: when it was read, the layout generation then, the monitors (or areas).
type Cached = RefCell<Option<(Instant, u64, Vec<Monitor>)>>;

/// The target poller's memory: the window, the space it was first seen on (hers), its rect
/// there at the last read (None: not visible there), when it last moved.
#[derive(Default)]
struct TargetTrack {
    t: WinId,
    read: bool,
    space: Option<i64>,
    rect: Option<WinRect>,
    moved: Option<Instant>,
}

impl TargetTrack {
    /// One read of target `t`: the hint it makes, and how long until the next read.
    fn step(&mut self, t: WinId, place: Option<super::Place>, now: Instant) -> (Option<Hint>, Duration) {
        if self.t != t {
            *self = TargetTrack { t, ..Default::default() };
        }
        if self.space.is_none() {
            self.space = place.map(|p| p.space);
        }
        let here = place.filter(|p| Some(p.space) == self.space).map(|p| p.rect);
        let hint = match (self.read, self.rect, here) {
            (false, ..) => None,
            (_, Some(a), Some(b)) if a != b => {
                self.moved = Some(now);
                Some(Hint::Moved(t, Some(b)))
            }
            (_, Some(_), None) => Some(Hint::Gone(t)),
            (_, None, Some(_)) => Some(Hint::Mapped(t)),
            _ => None,
        };
        (self.read, self.rect) = (true, here);
        let wait = if self.moved.is_some_and(|m| now.duration_since(m) < TARGET_BURST_FOR) {
            TARGET_POLL_BURST
        } else if place.is_none_or(|p| p.loose) {
            TARGET_POLL_LOOSE
        } else {
            TARGET_POLL_TILED
        };
        (hint, wait)
    }
}

/// The drag poller: its pointer track and its stop flag.
type Poll = (Arc<Mutex<Track>>, Arc<AtomicBool>);

/// One effect layer as last set: solid `rgb` over `rects` (relative to `origin`, root px) in a
/// `size` px box, and where it shows: on a surface of its own (`staged`: the backend paints
/// it) or in her window (her view paints it, `Os::paint_fx`).
#[derive(Clone, Debug, PartialEq)]
struct FxLayer {
    rgb: u32,
    origin: (i32, i32),
    size: (i32, i32),
    rects: Vec<ShapeRect>,
    staged: bool,
}

/// Every effect layer that is up, in the stacking order a server keeps, wherever each one
/// shows (`Os::fx_staged` decides). Her view paints the ones in her window from it.
#[derive(Default)]
struct FxList {
    layers: std::collections::HashMap<usize, FxLayer>,
    /// Above her body, bottom → top.
    over: Vec<usize>,
    /// Below her body, bottom → top.
    under: Vec<usize>,
    /// What her window shows changed since her view last took it.
    dirty: bool,
    /// What shows outside her window changed (the core's effect hosts repaint).
    staged_dirty: bool,
}

impl FxList {
    /// Set layer `layer` (`rects: None` keeps its rects). Returns where it showed before:
    /// `Some(staged)`, or `None` if it wasn't up.
    #[allow(clippy::too_many_arguments)]
    fn show(&mut self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ, staged: bool) -> Option<bool> {
        let prev = self.layers.get(&layer).cloned();
        let rects = rects.map(<[ShapeRect]>::to_vec).or_else(|| prev.as_ref().map(|l| l.rects.clone())).unwrap_or_default();
        let before = (self.over.clone(), self.under.clone());
        let now = FxLayer { rgb, origin, size, rects, staged };
        // The same layer set again (another tick, nothing new): no frame for it.
        let same = prev.as_ref() == Some(&now);
        self.layers.insert(layer, now);
        self.over.retain(|&l| l != layer);
        self.under.retain(|&l| l != layer);
        match z {
            FxZ::Top => self.over.push(layer),
            FxZ::Below(j) => match self.over.iter().position(|&l| l == j) {
                Some(i) => self.over.insert(i, layer),
                None => self.over.push(layer),
            },
            FxZ::UnderBody => self.under.push(layer),
        }
        // Her window repaints only for what it shows: this layer is (or was) in it.
        let in_window = !staged || prev.as_ref().is_some_and(|p| !p.staged);
        let changed = !same || before != (self.over.clone(), self.under.clone());
        self.dirty |= in_window && changed;
        self.staged_dirty |= (staged || prev.as_ref().is_some_and(|p| p.staged)) && changed;
        prev.map(|p| p.staged)
    }

    /// Take layer `layer` down. Returns where it showed (`Some(staged)`), `None` if it wasn't up.
    fn hide(&mut self, layer: usize) -> Option<bool> {
        let l = self.layers.remove(&layer)?;
        self.over.retain(|&k| k != layer);
        self.under.retain(|&k| k != layer);
        self.dirty |= !l.staged;
        self.staged_dirty |= l.staged;
        Some(l.staged)
    }

    fn clear(&mut self) {
        let in_window = self.layers.values().any(|l| !l.staged);
        let staged = self.layers.values().any(|l| l.staged);
        *self = FxList { dirty: self.dirty || in_window, staged_dirty: self.staged_dirty || staged, ..FxList::default() };
    }

    /// The staged layers' rects, root px, bottom → top, with colours.
    fn staged(&self) -> Vec<((i32, i32, i32, i32), u32)> {
        self.under
            .iter()
            .chain(&self.over)
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.staged)
            .flat_map(|l| l.rects.iter().map(move |r| ((l.origin.0 + r.0 as i32, l.origin.1 + r.1 as i32, r.2 as i32, r.3 as i32), l.rgb)))
            .collect()
    }

    /// Layers in the stacking order, bottom → top, with where each stacks when re-shown in
    /// that order (`Top` / `UnderBody` each, so the order comes out the same).
    fn order(&self) -> Vec<(usize, FxZ)> {
        self.under.iter().map(|&l| (l, FxZ::UnderBody)).chain(self.over.iter().map(|&l| (l, FxZ::Top))).collect()
    }

    /// The rects her window paints (the layers not staged), window-local px (her window at
    /// `win`), bottom → top, with colours.
    fn scene(&self, under: bool, win: (i32, i32)) -> Vec<((i32, i32, i32, i32), u32)> {
        let order = if under { &self.under } else { &self.over };
        order
            .iter()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| !l.staged)
            .flat_map(|l| l.rects.iter().map(move |r| ((l.origin.0 - win.0 + r.0 as i32, l.origin.1 - win.1 + r.1 as i32, r.2 as i32, r.3 as i32), l.rgb)))
            .collect()
    }
}

pub struct Os {
    desk: Arc<dyn Desktop>,
    stage: Box<dyn Stage>,
    caps: Caps,
    shared: Arc<Shared>,
    /// The mode forced by `SA_COMPOSITED` or settings.toml `[display] composited`: 0 = follow
    /// the compositor, 1 = shape (no alpha), 2 = alpha.
    force: Cell<u8>,
    mons: Cached,
    areas: Cached,
    /// While she's carried or flying: both caches stay as snapshotted at the grab (no round
    /// trip inside a 60 Hz tick).
    frozen: Cell<bool>,
    /// The pointer polled on its own connection while she's held by the real mouse: its
    /// changes (`track.rs`), and the stop flag.
    poll: RefCell<Option<Poll>>,
    /// Bounding shapes sent, and their rects (for the rag's meter).
    shape_sends: Cell<u64>,
    shape_rects: Cell<u64>,
    pointer_watch: Arc<PointerWatch>,
    /// Every effect layer that is up, and where each shows.
    fx_list: RefCell<FxList>,
    /// Where her body was last put (root px).
    body_at: Cell<(i32, i32)>,
    /// Her body window's size, px ((0, 0) before `adopt`).
    body_size: Cell<(i32, i32)>,
    /// Her body hosts, one per output (`Caps::host_per_output`): id and its monitor.
    hosts: RefCell<Vec<(WinId, Monitor)>>,
    /// She moved to another host: the view shows her in it (`take_host_change`).
    host_changed: Cell<Option<WinId>>,
    /// Her input region and her moves kept apart (`Os::settling`).
    settle: RefCell<Settle>,
}

impl Os {
    pub fn new(desk: Arc<dyn Desktop>, stage: Box<dyn Stage>) -> Os {
        let caps = desk.caps();
        let shared = Shared { target: AtomicU32::new(0), composited: AtomicBool::new(desk.compositor_running()), reshapes: AtomicBool::new(desk.compositor_reshapes()), poller: OnceLock::new(), space: AtomicI64::new(NO_SPACE), layout_gen: AtomicU64::new(0), moved_at: Mutex::new(None) };
        Os {
            desk,
            stage,
            caps,
            shared: Arc::new(shared),
            force: Cell::new(0),
            mons: RefCell::new(None),
            areas: RefCell::new(None),
            frozen: Cell::new(false),
            poll: RefCell::new(None),
            shape_sends: Cell::new(0),
            shape_rects: Cell::new(0),
            pointer_watch: Arc::new(PointerWatch { moved_ms: AtomicU64::new(u64::MAX), ..PointerWatch::default() }),
            fx_list: RefCell::new(FxList::default()),
            body_at: Cell::new((0, 0)),
            body_size: Cell::new((0, 0)),
            hosts: RefCell::new(Vec::new()),
            host_changed: Cell::new(None),
            settle: RefCell::new(Settle::default()),
        }
    }

    // ---- her windows -------------------------------------------------------------------

    /// Options for one of her windows: hers to decide, the backend only dresses them.
    pub fn window_options(&self, role: Role, bounds: Bounds<Pixels>, focus: bool) -> WindowOptions {
        let (title, background) = match role {
            Role::Body => ("synaps-avatar", WindowBackgroundAppearance::Transparent),
            Role::Reader => ("synaps-avatar reader", WindowBackgroundAppearance::Opaque),
            Role::Chip => ("synaps-avatar chip", WindowBackgroundAppearance::Opaque),
            Role::Line => ("synaps-avatar line", WindowBackgroundAppearance::Opaque),
            Role::Fx => ("synaps-avatar fx", WindowBackgroundAppearance::Transparent),
            Role::Intro => ("synaps-avatar intro", WindowBackgroundAppearance::Transparent),
        };
        let mut o = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
            focus,
            show: true,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: background,
            app_id: Some("synaps-avatar".into()),
            ..Default::default()
        };
        if matches!(role, Role::Body | Role::Intro) {
            // She is never the active window: GPUI's default caps inactive windows at 30 fps
            // (a drag then ran ~40 ticks/s). Our own deadlines decide when she draws.
            o.inactive_frame_interval = None;
        }
        self.stage.dress(role, &mut o);
        o
    }

    pub fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        if role == Role::Body {
            let (v, sf) = (window.viewport_size(), window.scale_factor());
            self.set_body_size((f32::from(v.width) * sf).round() as i32, (f32::from(v.height) * sf).round() as i32);
        }
        let id = self.stage.adopt(role, window)?;
        if role == Role::Body && self.caps.host_per_output {
            let (b, k) = (window.bounds(), window.scale_factor());
            let d = |v: Pixels| (f32::from(v) * k).round() as i32;
            let (w, h) = self.body_size.get();
            let m = self.monitor_at(d(b.origin.x) + w / 2, d(b.origin.y) + h / 2);
            self.hosts.borrow_mut().push((id, m));
        }
        Ok(id)
    }

    /// Her body needs a host per output (`Caps::host_per_output`).
    pub fn host_per_output(&self) -> bool {
        self.caps.host_per_output
    }

    /// One body host per output (`Caps::host_per_output`): the monitors still without one.
    pub fn hosts_wanted(&self) -> Vec<Monitor> {
        if !self.caps.host_per_output {
            return Vec::new();
        }
        let h = self.hosts.borrow();
        self.monitors().into_iter().filter(|m| !h.iter().any(|(_, hm)| hm.rect() == m.rect())).collect()
    }

    /// Body hosts whose monitor is gone (dropped from the list: the view closes them).
    pub fn hosts_gone(&self) -> Vec<WinId> {
        let ms = self.monitors();
        let mut h = self.hosts.borrow_mut();
        let gone: Vec<WinId> = h.iter().filter(|(_, m)| !ms.iter().any(|x| x.rect() == m.rect())).map(|(id, _)| *id).collect();
        h.retain(|(id, _)| !gone.contains(id));
        gone
    }

    /// The body host she moved to since the last call, if any.
    pub fn take_host_change(&self) -> Option<WinId> {
        self.host_changed.take()
    }

    /// The host whose monitor holds the centre of her window at (x, y) (the nearest if none).
    fn host_for(&self, x: i32, y: i32) -> Option<WinId> {
        let h = self.hosts.borrow();
        let (w, hh) = self.body_size.get();
        let (cx, cy) = (x + w / 2, y + hh / 2);
        h.iter().find(|(_, m)| m.contains(cx, cy)).or_else(|| h.iter().min_by_key(|(_, m)| m.distance2(cx, cy))).map(|(id, _)| *id)
    }

    /// Her body window's size, px (the effect layers inside it can show in it).
    pub(super) fn set_body_size(&self, w: i32, h: i32) {
        self.body_size.set((w, h));
    }

    /// Her body's window id.
    pub fn body(&self) -> WinId {
        self.stage.body()
    }

    /// Body origin, root px; on another output's host when she crossed to it. Under picom
    /// (`Os::settling`) a move right after a region change waits (`Os::settle`).
    pub fn move_to(&self, x: i32, y: i32) -> Result<()> {
        if !self.settle.borrow_mut().wants_move(self.settling(), (x, y), Instant::now()) {
            return Ok(());
        }
        self.send_move(x, y)
    }

    /// A landing (a teleport's, a placed move): out at once, never held (`Os::move_to` may
    /// be): one cross-fade of her by picom costs less than her standing where she left.
    pub fn move_now(&self, x: i32, y: i32) -> Result<()> {
        self.settle.borrow_mut().landed(Instant::now());
        self.send_move(x, y)
    }

    /// Where her body is on the desktop, root px: the last move out (a held one is not yet).
    pub fn shown_at(&self) -> (i32, i32) {
        self.body_at.get()
    }

    fn send_move(&self, x: i32, y: i32) -> Result<()> {
        self.body_at.set((x, y));
        if let Some(h) = self.host_for(x, y)
            && h != self.stage.body()
        {
            self.stage.set_body(h);
            self.host_changed.set(Some(h));
        }
        self.stage.move_to(x, y)
    }


    pub fn raise(&self) -> Result<()> {
        self.stage.raise()
    }

    /// Place and size one of her other windows (the Line).
    pub fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.stage.place(s, x, y, w, h)
    }

    pub fn set_icon(&self, s: WinId, data: &[u32]) -> Result<()> {
        self.stage.set_icon(s, data)
    }

    /// Input region = `input`; everything else clicks through. The visible region is the
    /// mode's: all of the window when a compositor blends her alpha, else `visible`, so no
    /// black box shows. Each `None` stays as it is.
    ///
    /// Under picom (`Os::settling`) the input region waits while she moves or the button is
    /// down on her (`settle.rs` has the rules; `Os::settle` sends it when due).
    pub fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<&[ShapeRect]>) -> Result<()> {
        let input = input.filter(|r| self.settle.borrow_mut().wants_region(self.settling(), r, Instant::now()));
        if input.is_none() && visible.is_none() {
            return Ok(());
        }
        let visible = visible.map(|v| {
            if self.composited() {
                Visible::All
            } else {
                self.shape_sends.set(self.shape_sends.get() + 1);
                self.shape_rects.set(self.shape_rects.get() + v.len() as u64);
                Visible::Rects(v)
            }
        });
        self.stage.set_shapes(input, visible)
    }

    /// Her input region and her moves are kept apart (`settle.rs`): under a compositor that
    /// takes a change of her window's shape for a resize (picom: it then cross-fades her from
    /// a stale snapshot of herself until she holds still), in alpha mode (in shape mode her
    /// shape is what shows: it follows every frame). Elsewhere both go out at once.
    pub fn settling(&self) -> bool {
        self.shared.reshapes.load(Ordering::Relaxed) && self.composited()
    }

    /// The button went down on her (`down`) or up. With an implicit grab
    /// (`Caps::implicit_grab`) every pointer event is hers while it is down, whatever her input
    /// region: the region then waits for the release, so a drag never waits for one.
    pub fn set_pressed(&self, down: bool) {
        if self.caps.implicit_grab {
            self.settle.borrow_mut().press(down, Instant::now());
        }
    }

    /// When what `settle` holds back is due, if anything is.
    pub fn settle_due(&self) -> Option<Instant> {
        self.settle.borrow().due(self.settling(), Instant::now())
    }

    /// Send what is held back and due now: a move or her input region (both at once out of
    /// that mode). A failed move never drops the region.
    pub fn settle(&self) -> Result<()> {
        let (pos, input) = self.settle.borrow_mut().take_due(self.settling(), Instant::now());
        let moved = pos.map_or(Ok(()), |(x, y)| self.send_move(x, y));
        let shaped = input.map_or(Ok(()), |r| self.stage.set_shapes(Some(&r), None));
        moved.and(shaped)
    }

    /// Another surface's input region = `input`; visible: all of it under a compositor, else
    /// `visible` (as `set_shapes`).
    pub fn shape_of(&self, s: WinId, input: &[ShapeRect], visible: &[ShapeRect]) -> Result<()> {
        let v = if self.composited() { Visible::All } else { Visible::Rects(visible) };
        self.stage.shape_of(s, input, v)
    }

    /// Bounding shapes sent and their total rects since the last call (the rag's meter).
    pub fn take_shape_stats(&self) -> (u64, u64) {
        (self.shape_sends.replace(0), self.shape_rects.replace(0))
    }

    // ---- effect layers -------------------------------------------------------------------

    /// Show effect layer `layer` (solid `rgb`) on top of everything at root `origin` with the
    /// rects `rects` (relative to `origin`; none: the layer hides). Input passes through. Not
    /// flushed, so several layers go out in one write.
    pub fn fx_set(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: &[ShapeRect]) -> Result<()> {
        self.fx(layer, rgb, origin, size, Some(rects), FxZ::Top)
    }

    /// Like `fx_set`, stacked directly below layer `over`. Stacking a group top-down this way
    /// never puts a lower layer above a higher one, even for a moment (raising each to the
    /// top in turn does, and a screen grab catches it).
    pub fn fx_set_below(&self, layer: usize, over: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: &[ShapeRect]) -> Result<()> {
        self.fx(layer, rgb, origin, size, Some(rects), FxZ::Below(over))
    }

    /// An effect layer stacked directly below her body (e.g. a contact shadow). `rects:
    /// None` keeps the current rects and only moves it. Flushed.
    pub fn fx_under(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>) -> Result<()> {
        self.fx(layer, rgb, origin, size, rects, FxZ::UnderBody)?;
        self.stage.flush();
        Ok(())
    }

    fn fx(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()> {
        if rects.is_some_and(|r| r.is_empty()) {
            self.fx_unmap(layer);
            return Ok(());
        }
        let staged = self.fx_staged(origin, size);
        let mut l = self.fx_list.borrow_mut();
        match (l.show(layer, rgb, origin, size, rects, z, staged), staged) {
            // Into her window: its surface goes.
            (Some(true), false) => {
                self.stage.fx_hide(layer);
                Ok(())
            }
            (_, false) => Ok(()),
            // Out of her window onto a surface: that surface needs the rects.
            (Some(false), true) => {
                let rects = l.layers.get(&layer).map(|f| f.rects.clone()).unwrap_or_default();
                self.stage.fx_show(layer, rgb, origin, size, Some(&rects), z)
            }
            (_, true) => self.stage.fx_show(layer, rgb, origin, size, rects, z),
        }
    }

    /// Where an effect layer shows: on a surface of its own (the backend paints it), or in her
    /// window (her view paints it). Without a compositor her window shows only her figure's
    /// shape, so every layer needs its own surface. When a compositor blends her, a layer
    /// that fits in her window is part of her picture: it composites with her in one surface.
    /// Small opaque surfaces stacked over her translucent window are composited apart from
    /// her, and a compositor can then paint the desktop over their boxes when she redraws
    /// under them (dark boxes and bars around her glints). Only the layers reaching outside
    /// her window (the bolt) keep surfaces of their own.
    /// Without surfaces from the backend, the core's effect hosts show what reaches outside
    /// her window (`Caps::host_per_output`; else it's clipped to her window).
    fn fx_staged(&self, origin: (i32, i32), size: (i32, i32)) -> bool {
        if !self.caps.fx_layers && !self.caps.host_per_output {
            return false;
        }
        if !self.composited() {
            return true;
        }
        let ((bx, by), (bw, bh)) = (self.body_at.get(), self.body_size.get());
        let inside = origin.0 >= bx && origin.1 >= by && origin.0 + size.0 <= bx + bw && origin.1 + size.1 <= by + bh;
        !inside
    }

    /// Hide one effect layer if it is up.
    pub fn fx_unmap(&self, layer: usize) {
        self.fx_list.borrow_mut().hide(layer);
        if self.caps.fx_layers {
            self.stage.fx_hide(layer);
        }
    }

    pub fn fx_hide(&self) {
        self.fx_list.borrow_mut().clear();
        if self.caps.fx_layers {
            self.stage.fx_hide_all();
        }
    }

    /// The compositor mode changed: every layer that is up shows where the new mode puts it
    /// (in her window or on a surface of its own), in the same stacking order.
    pub fn fx_reroute(&self) {
        let mut l = self.fx_list.borrow_mut();
        let mut moved = false;
        for (&layer, f) in l.layers.iter_mut() {
            let staged = self.fx_staged(f.origin, f.size);
            if staged != f.staged {
                if f.staged {
                    self.stage.fx_hide(layer);
                }
                f.staged = staged;
                moved = true;
            }
        }
        if !moved {
            return;
        }
        l.dirty = true;
        // Show every surface again bottom → top: those that moved out of her window land in
        // their place among the ones already up (a mode switch is rare).
        for (layer, z) in l.order() {
            if let Some(f) = l.layers.get(&layer).filter(|f| f.staged) {
                self.stage.fx_show(layer, f.rgb, f.origin, f.size, Some(&f.rects), z).ok();
            }
        }
        self.stage.flush();
    }

    /// While a press lasts her input region must cover her window (`Caps::implicit_grab` off).
    pub fn press_takes_window(&self) -> bool {
        !self.caps.implicit_grab
    }

    /// A perch that jumps glides after its window (`Caps::goal_geometry`).
    /// When the perch target's last move likely began (the poller's estimate; None where the
    /// backend tells of every move: then, when it's handled).
    pub fn target_moved_at(&self) -> Option<Instant> {
        self.shared.moved_at.lock().ok().and_then(|m| *m)
    }

    /// How a perch that jumps follows its window (`Caps::goal_geometry`): on the compositor's
    /// own animation for that move, so she rides the window instead of trailing it; a plain
    /// ease-out where the backend can't say. None: she snaps (the window jumps too).
    pub fn perch_glide(&self) -> Option<MoveAnim> {
        if !self.caps.goal_geometry {
            return None;
        }
        match self.desk.move_anim() {
            Some(a) if a.ms <= 0.0 => None,
            Some(a) => Some(a),
            None => Some(MoveAnim::EASE_OUT),
        }
    }

    /// Effect steps that can wait ride redraws that happen anyway: a redraw is a full present
    /// (`Caps::costly_redraw`), or the effects are painted in her window (`fx_in_scene`), so
    /// each step on its own would be a redraw of her.
    pub fn redraw_costly(&self) -> bool {
        self.caps.costly_redraw || self.fx_in_scene()
    }

    /// Effect layers may show in her window (her view paints them, `paint_fx`): the backend
    /// has no surfaces for them, or a compositor blends her (`fx_staged`).
    pub fn fx_in_scene(&self) -> bool {
        !self.caps.fx_layers || self.composited()
    }

    /// Paint the in-scene effect layers below her body (`under`) or above it, bottom → top,
    /// in her window (where `move_to` put it; `sf` device px per logical px). Nothing when the
    /// server paints them. Painting the ones above her marks the list clean.
    pub fn paint_fx(&self, window: &mut Window, under: bool, sf: f32) {
        if !self.fx_in_scene() {
            return;
        }
        let mut l = self.fx_list.borrow_mut();
        for ((x, y, w, h), c) in l.scene(under, self.body_at.get()) {
            let b = Bounds::new(point(px(x as f32 / sf), px(y as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf)));
            window.paint_quad(fill(b, rgb(c)));
        }
        if !under {
            l.dirty = false;
        }
    }

    /// What her window paints of the effect layers (window-local px, bottom → top).
    #[cfg(test)]
    pub(super) fn fx_scene(&self, under: bool) -> Vec<((i32, i32, i32, i32), u32)> {
        self.fx_list.borrow().scene(under, self.body_at.get())
    }

    /// In-scene effect layers changed since the last call: her window needs a frame.
    /// What the effect hosts show changed since the last call (`Caps::host_per_output`).
    pub fn fx_take_staged(&self) -> bool {
        !self.caps.fx_layers && std::mem::take(&mut self.fx_list.borrow_mut().staged_dirty)
    }

    /// The box of what the effect hosts show (root px), if anything.
    pub fn fx_staged_box(&self) -> Option<WinRect> {
        self.fx_list.borrow().staged().iter().map(|&((x, y, w, h), _)| WinRect::new(x, y, w, h)).reduce(|a, b| {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            WinRect::new(x0, y0, a.right().max(b.right()) - x0, a.bottom().max(b.bottom()) - y0)
        })
    }

    /// Paint what the effect hosts show, in a host whose top-left is at `at` (root px).
    pub fn paint_fx_staged(&self, window: &mut Window, at: (i32, i32), sf: f32) {
        for ((x, y, w, h), c) in self.fx_list.borrow().staged() {
            let b = Bounds::new(point(px((x - at.0) as f32 / sf), px((y - at.1) as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf)));
            window.paint_quad(fill(b, rgb(c)));
        }
    }

    pub fn fx_take_dirty(&self) -> bool {
        std::mem::take(&mut self.fx_list.borrow_mut().dirty)
    }

    pub fn flush(&self) {
        self.stage.flush();
    }

    // ---- keyboard ------------------------------------------------------------------------

    /// The keyboard to her surface `s` (her body, the reader, the Line).
    pub fn grab_keyboard(&self, s: WinId) -> Result<()> {
        self.stage.grab_keyboard(s)
    }

    /// Her surface `s` lets the keyboard go, back to `to` (the window that had it).
    pub fn release_keyboard(&self, s: WinId, to: Option<WinId>) -> Result<()> {
        self.stage.release_keyboard(s, to)
    }

    /// The desktop hands her the keyboard only right after his own input to her
    /// (`Caps::keyboard_on_input`).
    pub fn keyboard_on_input(&self) -> bool {
        self.caps.keyboard_on_input
    }

    /// Taking the keyboard now is his doing (his key, his click on her, or hers have it). Always
    /// where the desktop hands it over freely.
    pub fn may_take_keyboard(&self) -> bool {
        !self.caps.keyboard_on_input || self.stage.keyboard_granted()
    }

    /// How he presses her key `ev` (letter `key`): the backend's chord where it grabs the keys,
    /// else the binding the docs give his desktop (`$MOD+C`).
    pub fn key_label(&self, ev: super::KeyEv, key: char) -> String {
        let k = key.to_ascii_uppercase();
        self.desk.key_label(ev, key).unwrap_or_else(|| match ev {
            super::KeyEv::Summon | super::KeyEv::TalkPress | super::KeyEv::TalkRelease => format!("$MOD+{k}"),
            super::KeyEv::TalkToggle => format!("$MOD+CTRL+{k}"),
        })
    }

    /// Whatever holds the keyboard.
    pub fn focused_window(&self) -> Option<WinId> {
        self.desk.focused_window()
    }

    /// The window that has his keyboard now, not hers: where it goes back to.
    pub fn prev_focus(&self) -> Option<WinId> {
        let ours = self.body();
        self.desk.active_window().or_else(|| self.desk.focused_window()).filter(|&w| w != ours)
    }

    pub fn char_key_down(&self, ch: char) -> Result<bool> {
        self.desk.char_key_down(ch)
    }

    // ---- compositor mode -----------------------------------------------------------------

    /// Alpha mode: the window's own alpha is what shows (a compositor blends it), so no
    /// bounding shape and nothing painted that only a shape would hide. Else shape mode.
    pub fn composited(&self) -> bool {
        if !self.caps.shape_mode {
            return true;
        }
        match self.force.get() {
            1 => false,
            2 => true,
            _ => self.shared.composited.load(Ordering::Relaxed),
        }
    }

    /// Whether a compositor is running, whatever the mode.
    pub fn compositor_running(&self) -> bool {
        self.shared.composited.load(Ordering::Relaxed)
    }

    /// Force a mode (`None`: follow the compositor). True if the effective mode changed.
    pub fn force_composited(&self, mode: Option<bool>) -> bool {
        if !self.caps.shape_mode && mode == Some(false) && self.force.get() != 1 {
            elog!("synaps-avatar: [display] composited = false ignored: this desktop always blends her alpha");
        }
        let before = self.composited();
        self.force.set(mode.map_or(0, |m| if m { 2 } else { 1 }));
        before != self.composited()
    }

    /// What forces the mode: `SA_COMPOSITED=0|1` (env) wins over the settings key.
    pub fn forced_mode(&self) -> Option<bool> {
        match self.force.get() {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }

    // ---- monitors and work areas ---------------------------------------------------------

    /// The cache, while it holds: for `AREAS_TTL`, until a `Hint::Layout` where they come
    /// (`Caps::layout_events`), or while she's carried.
    fn cached(&self, c: &Cached) -> Option<Vec<Monitor>> {
        let now_gen = self.shared.layout_gen.load(Ordering::Relaxed);
        let holds = |t: &Instant, g: u64| if self.caps.layout_events { g == now_gen } else { t.elapsed() < AREAS_TTL };
        c.borrow().as_ref().filter(|(t, g, _)| holds(t, *g) || self.frozen.get()).map(|(_, _, m)| m.clone())
    }

    fn layout_gen(&self) -> u64 {
        self.shared.layout_gen.load(Ordering::Relaxed)
    }

    /// The monitors (cached for `AREAS_TTL`, or frozen while she's carried); at least one.
    pub fn monitors(&self) -> Vec<Monitor> {
        if let Some(m) = self.cached(&self.mons) {
            return m;
        }
        let mut out = self.desk.monitors();
        if out.is_empty() {
            let (w, h) = self.desk.root_size();
            out.push(Monitor { primary: true, x: 0, y: 0, w, h });
        }
        *self.mons.borrow_mut() = Some((Instant::now(), self.layout_gen(), out.clone()));
        out
    }

    /// Each monitor minus the docks, same order (cached like the monitors).
    pub fn work_areas(&self) -> Vec<Monitor> {
        if let Some(a) = self.cached(&self.areas) {
            return a;
        }
        let areas = self.desk.work_areas(&self.monitors());
        *self.areas.borrow_mut() = Some((Instant::now(), self.layout_gen(), areas.clone()));
        areas
    }

    fn at(ms: Vec<Monitor>, x: i32, y: i32) -> Monitor {
        ms.iter()
            .find(|m| m.contains(x, y))
            .cloned()
            .unwrap_or_else(|| ms.iter().min_by_key(|m| m.distance2(x, y)).cloned().expect("a monitor"))
    }

    /// The monitor containing (x, y), else the nearest one.
    pub fn monitor_at(&self, x: i32, y: i32) -> Monitor {
        Self::at(self.monitors(), x, y)
    }

    /// The work area containing (x, y), else the nearest one.
    pub fn area_at(&self, x: i32, y: i32) -> Monitor {
        Self::at(self.work_areas(), x, y)
    }

    pub fn primary_area(&self) -> Monitor {
        let ms = self.work_areas();
        ms.iter().find(|m| m.primary).cloned().unwrap_or_else(|| ms[0].clone())
    }

    /// Drop the cached layout (monitors or panels changed; tests change them under it).
    pub fn forget_layout(&self) {
        *self.mons.borrow_mut() = None;
        *self.areas.borrow_mut() = None;
    }

    /// Carried (on) or back to normal (off): on takes a fresh snapshot of the monitors and
    /// work areas and keeps it until off.
    pub fn freeze_layout(&self, on: bool) {
        // A snapshot younger than ~1 s is fresh enough (the refresh walks the tree: a few ms
        // on the first dragged frame).
        let fresh = |c: &Cached| c.borrow().as_ref().is_some_and(|(t, g, _)| if self.caps.layout_events { *g == self.layout_gen() } else { t.elapsed() < Duration::from_secs(1) });
        if on && !(fresh(&self.mons) && fresh(&self.areas)) {
            self.frozen.set(false);
            *self.mons.borrow_mut() = None;
            *self.areas.borrow_mut() = None;
            self.work_areas();
        }
        self.frozen.set(on);
    }

    /// Device px across GPUI's primary display (`Desktop::primary_display_px`): her scale is
    /// this ÷ its GPUI width.
    pub fn primary_display_px(&self) -> i32 {
        self.desk.primary_display_px()
    }

    /// Refresh rate (Hz) of the output showing (x, y); 60 when unknown or implausible.
    pub fn refresh_at(&self, x: i32, y: i32) -> f32 {
        self.desk.refresh_at(x, y).filter(|r| (20.0..=500.0).contains(r)).unwrap_or(60.0)
    }

    // ---- other windows ---------------------------------------------------------------------

    /// Every visible window, bottom → top, and whether she may sit on it: big enough and
    /// managed (a popup or menu only occludes).
    pub fn stack(&self) -> Vec<(WinRect, bool)> {
        self.desk
            .windows()
            .into_iter()
            .map(|w| {
                let big = w.rect.w >= SEAT_MIN.0 && w.rect.h >= SEAT_MIN.1;
                (w.rect, big && w.managed)
            })
            .collect()
    }

    pub fn active_window(&self) -> Option<WinId> {
        self.desk.active_window()
    }

    pub fn toplevel_of(&self, w: WinId) -> Option<WinId> {
        self.desk.toplevel_of(w)
    }

    /// The perch target counts only on the space it was first seen on: elsewhere it has left
    /// hers (`Place::space`).
    pub fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        let space = self.shared.space.load(Ordering::Relaxed);
        if w != 0 && w == self.shared.target.load(Ordering::Relaxed) && space != NO_SPACE {
            return self.desk.toplevel_place(w).filter(|p| p.space == space).map(|p| p.rect);
        }
        self.desk.toplevel_rect(w)
    }

    pub fn window_exists(&self, w: WinId) -> bool {
        self.desk.window_exists(w)
    }

    pub fn is_fullscreen(&self, w: WinId) -> bool {
        self.desk.is_fullscreen(w)
    }

    /// Milliseconds since his last input; a backend that can't tell: since the pointer last
    /// moved (presence while the pointer moves; typing alone then reads as idle).
    pub fn idle_ms(&self) -> Option<u32> {
        self.desk.idle_ms().or_else(|| self.pointer_still_ms().map(|ms| ms.min(u32::MAX as u64) as u32))
    }

    /// Grab her global keys, if this backend does (`Desktop::watch_keys`): Ok(false) when the
    /// desktop's bindings drive her instead.
    pub fn watch_keys(&self, keys: super::Keys, on: impl Fn(super::KeyEv) + Send + 'static) -> Result<bool> {
        self.desk.watch_keys(keys, Box::new(on))
    }

    /// Report the session's lock and sleep (`WatchEvent::Session`).
    pub fn watch_session(&self, on: impl Fn(WatchEvent) + Send + 'static) -> Result<()> {
        self.desk.watch_session(Box::new(move |e| on(WatchEvent::Session(e))))
    }

    /// Follow `w` (0 = nothing).
    pub fn set_target(&self, w: WinId) {
        if self.shared.target.swap(w, Ordering::Relaxed) != w {
            self.shared.space.store(NO_SPACE, Ordering::Relaxed);
        }
        if let Some(p) = self.shared.poller.get() {
            p.unpark();
        }
    }

    /// The watchers: the backend's hints (and, without target events, the target poller)
    /// through the one change-detection path. `on_event` runs on their threads.
    pub fn spawn_watcher(&self, on_event: impl Fn(WatchEvent) + Send + Sync + 'static) -> Result<()> {
        let on: Arc<dyn Fn(WatchEvent) + Send + Sync> = Arc::new(on_event);
        let (s, d, o) = (self.shared.clone(), self.desk.clone(), on.clone());
        self.desk.watch(Box::new(move |h| s.hint(&*d, h, &*o)))?;
        if !self.caps.target_events {
            self.spawn_target_poll(on)?;
        }
        Ok(())
    }

    /// Re-read the target while there is one (parked otherwise; a hint wakes it), diff, and
    /// feed the differences through the same path as a backend's hints. Every
    /// `TARGET_POLL_TILED` (`_LOOSE` for a floating one), `_BURST` while it moves.
    fn spawn_target_poll(&self, on: Arc<dyn Fn(WatchEvent) + Send + Sync>) -> Result<()> {
        let (s, d) = (self.shared.clone(), self.desk.clone());
        let h = std::thread::Builder::new().name("avatar-target".into()).spawn(move || {
            let mut track = TargetTrack::default();
            // The last sleep: (since, for how long).
            let mut slept: Option<(Instant, Duration)> = None;
            loop {
                let t = s.target.load(Ordering::Relaxed);
                if t == 0 {
                    track = TargetTrack::default();
                    slept = None;
                    std::thread::park();
                    continue;
                }
                let now = Instant::now();
                let (hint, wait) = track.step(t, d.toplevel_place(t), now);
                if s.target.load(Ordering::Relaxed) == t {
                    s.space.store(track.space.unwrap_or(NO_SPACE), Ordering::Relaxed);
                }
                if let Some(h) = hint {
                    // Slept it out: the move began somewhere in that sleep. Woken early (a
                    // hint): just now.
                    let age = slept.filter(|(at, w)| now.duration_since(*at) + Duration::from_millis(2) >= *w).map_or(Duration::ZERO, |(_, w)| w / 2);
                    if let Ok(mut m) = s.moved_at.lock() {
                        *m = Some(now.checked_sub(age).unwrap_or(now));
                    }
                    s.hint(&*d, h, &*on);
                }
                slept = Some((Instant::now(), wait));
                std::thread::park_timeout(wait);
            }
        })?;
        self.shared.poller.set(h.thread().clone()).ok();
        Ok(())
    }

    /// Clicks anywhere, at the pointer, while armed (`arm_clicks`). `on_click` runs on the
    /// watcher's thread.
    pub fn watch_clicks(&self, on_click: impl Fn(i32, i32) + Send + 'static) -> Result<()> {
        self.desk.watch_clicks(Box::new(on_click))
    }

    /// Report clicks (the Line holds the keyboard) or not.
    /// Escape presses anywhere while armed (`arm_escape`): `on` runs on the click thread.
    pub fn watch_escape(&self, on: impl Fn() + Send + 'static) {
        self.desk.watch_escape(Box::new(on));
    }

    pub fn arm_escape(&self, on: bool) {
        self.desk.arm_escape(on);
    }

    /// The pointer's image over her body (None: the desktop's).
    pub fn pointer_image(&self, img: Option<&super::PointerImage>) {
        self.desk.pointer_image(self.body(), img);
    }

    pub fn arm_clicks(&self, on: bool) {
        self.desk.arm_clicks(on);
    }

    // ---- pointer ---------------------------------------------------------------------------

    pub fn pointer(&self) -> Result<(i32, i32)> {
        self.desk.pointer()
    }

    /// Where the pointer was for a frame drawn at `at` (`track.rs`: resampled from the
    /// poller's changes), else a fresh read.
    pub fn pointer_at(&self, at: Instant) -> Result<(f32, f32)> {
        if let Some(p) = self.track(|t| t.at(at)).flatten() {
            return Ok(p);
        }
        self.pointer().map(|p| (p.0 as f32, p.1 as f32))
    }

    /// Bumped by every pointer change the poller sees (0 when not polling).
    pub fn pointer_seq(&self) -> u64 {
        self.track(|t| t.seq()).unwrap_or(0)
    }

    /// The poller saw the pointer move within `d` before `now`.
    pub fn pointer_moved_within(&self, now: Instant, d: Duration) -> bool {
        self.track(|t| t.newest()).flatten().is_some_and(|(at, _)| now.saturating_duration_since(at) <= d)
    }

    fn track<T>(&self, f: impl FnOnce(&Track) -> T) -> Option<T> {
        let g = self.poll.borrow();
        let (t, _) = g.as_ref()?;
        t.lock().ok().map(|t| f(&t))
    }

    /// Poll the pointer on a thread with its own connection (250 Hz) while on, so a held tick
    /// reads an atomic instead of a round trip.
    pub fn poll_pointer(&self, on: bool) {
        let mut g = self.poll.borrow_mut();
        if !on {
            if let Some((_, stop)) = g.take() {
                stop.store(true, Ordering::Relaxed);
            }
            return;
        }
        if g.is_some() {
            return;
        }
        let track = Arc::new(Mutex::new(Track::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (tr, s, d) = (track.clone(), stop.clone(), self.desk.clone());
        let spawned = std::thread::Builder::new().name("avatar-pointer".into()).spawn(move || {
            let Ok(mut read) = d.pointer_reader() else { return };
            while !s.load(Ordering::Relaxed) {
                if let Some(p) = read()
                    && let Ok(mut t) = tr.lock()
                {
                    t.push(Instant::now(), p);
                }
                // 2 ms: each change timed to within a read (the frame resamples them).
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        if spawned.is_ok() {
            *g = Some((track, stop));
        }
    }

    /// Watch the pointer on its own connection: near her (`near_px` of her body box), a fast
    /// swipe past her (a segment between two samples passing within `swipe_px`), resting on
    /// her for `hover`. Polled, 16 Hz within ~700 px of her, 5 Hz farther, 2 Hz disarmed (her
    /// box and the arming come from the view, `set_pointer_watch`). One read a sample.
    pub fn watch_pointer(&self, cfg: PointerCfg, on: impl Fn(PointerEv) + Send + 'static) -> Result<()> {
        let mut read = self.desk.pointer_reader()?;
        let pw = self.pointer_watch.clone();
        let t0 = Instant::now();
        if let Ok(mut g) = pw.started.lock() {
            *g = Some(t0);
        }
        std::thread::Builder::new().name("avatar-pointer-watch".into()).spawn(move || {
            let mut last: Option<(i32, i32)> = None;
            let mut sense = super::pointer::Sense::default();
            loop {
                let armed = pw.armed.load(Ordering::Relaxed);
                let Some((x, y)) = read() else { return };
                let now = Instant::now();
                if last != Some((x, y)) {
                    pw.moved_ms.store(now.duration_since(t0).as_millis() as u64, Ordering::Relaxed);
                }
                last = Some((x, y));
                let her = pw.her.lock().map(|g| *g).unwrap_or_default();
                let (evs, far) = sense.sample(&cfg, now, x, y, Some(her), armed);
                for e in evs {
                    on(e);
                }
                let wait = if !armed { 500 } else if far { 200 } else { 60 };
                std::thread::sleep(Duration::from_millis(wait));
            }
        })?;
        Ok(())
    }

    /// Her body box for the pointer watcher, and whether it reports.
    pub fn set_pointer_watch(&self, her: Option<WinRect>, armed: bool) {
        if let Ok(mut g) = self.pointer_watch.her.lock() {
            *g = her.unwrap_or_default();
        }
        self.pointer_watch.armed.store(armed && her.is_some(), Ordering::Relaxed);
    }

    /// Milliseconds since the pointer last moved (None: no watcher, or no motion seen yet).
    pub fn pointer_still_ms(&self) -> Option<u64> {
        let t0 = (*self.pointer_watch.started.lock().ok()?)?;
        let m = self.pointer_watch.moved_ms.load(Ordering::Relaxed);
        (m != u64::MAX).then(|| (t0.elapsed().as_millis() as u64).saturating_sub(m))
    }
}

/// Distance from (x, y) to the rect (0 inside).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx_list_stacks_like_a_server() {
        let mut l = FxList::default();
        let show = |l: &mut FxList, layer, rgb, o, r: Option<&[ShapeRect]>, z| l.show(layer, rgb, o, (60, 60), r, z, false);
        show(&mut l, 1, 0xff0000, (100, 100), Some(&[(0, 0, 4, 10)]), FxZ::Top);
        show(&mut l, 2, 0x660000, (100, 100), Some(&[(0, 0, 8, 10)]), FxZ::Below(1));
        show(&mut l, 3, 0x111111, (90, 120), Some(&[(0, 0, 50, 3)]), FxZ::UnderBody);
        show(&mut l, 4, 0x00ff00, (0, 0), Some(&[(1, 1, 1, 1)]), FxZ::Below(9));
        let over: Vec<u32> = l.scene(false, (100, 100)).iter().map(|q| q.1).collect();
        assert_eq!(over, [0x660000, 0xff0000, 0x00ff00], "glow below the line; below a missing layer = on top");
        assert_eq!(l.scene(true, (100, 100)), [((-10, 20, 50, 3), 0x111111)], "window-local, under her body");
        show(&mut l, 1, 0xff0000, (110, 100), None, FxZ::Top);
        assert_eq!(l.scene(false, (100, 100)).last(), Some(&((10, 0, 4, 10), 0xff0000)), "None keeps the rects, moves the layer");
        // A tick sets the same pair again (the core on top, its glow below it): no frame.
        show(&mut l, 2, 0x660000, (100, 100), Some(&[(0, 0, 8, 10)]), FxZ::Below(1));
        l.dirty = false;
        show(&mut l, 1, 0xff0000, (110, 100), None, FxZ::Top);
        show(&mut l, 2, 0x660000, (100, 100), Some(&[(0, 0, 8, 10)]), FxZ::Below(1));
        assert!(!l.dirty, "the same layers set again need no frame");
        show(&mut l, 2, 0x660000, (101, 100), None, FxZ::Below(1));
        assert!(l.dirty, "moved");
        l.dirty = false;
        assert_eq!(l.hide(7), None);
        assert!(!l.dirty, "hiding a layer that isn't up changes nothing");
        assert_eq!(l.hide(2), Some(false));
        assert!(l.dirty && l.scene(false, (0, 0)).len() == 2);
        l.clear();
        assert!(l.scene(false, (0, 0)).is_empty() && l.scene(true, (0, 0)).is_empty());
    }

    /// Layers on surfaces of their own are kept in the stacking order but never paint in her
    /// window, and never cost her a frame; one moving into or out of her window does.
    #[test]
    fn fx_list_staged_layers_stay_out_of_her_window() {
        let mut l = FxList::default();
        assert_eq!(l.show(0, 0xff0000, (0, 0), (900, 40), Some(&[(0, 0, 900, 4)]), FxZ::Top, true), None);
        assert_eq!(l.show(1, 0xffc2e2, (120, 130), (10, 10), Some(&[(4, 0, 2, 10), (0, 4, 10, 2)]), FxZ::Top, true), None);
        assert!(!l.dirty && l.scene(false, (100, 100)).is_empty(), "staged: nothing for her window");
        assert_eq!(l.show(1, 0xffc2e2, (121, 130), (10, 10), None, FxZ::Top, true), Some(true));
        assert!(!l.dirty, "a staged layer moving costs her no frame");
        // Into her window (a compositor came): its rects come along, and she needs a frame.
        assert_eq!(l.show(1, 0xffc2e2, (121, 130), (10, 10), None, FxZ::Top, false), Some(true));
        assert!(l.dirty);
        assert_eq!(l.scene(false, (100, 100)), [((25, 30, 2, 10), 0xffc2e2), ((21, 34, 10, 2), 0xffc2e2)]);
        l.dirty = false;
        // Out again: gone from her window, and she needs a frame to drop it.
        assert_eq!(l.show(1, 0xffc2e2, (121, 130), (10, 10), None, FxZ::Top, true), Some(false));
        assert!(l.dirty && l.scene(false, (100, 100)).is_empty());
        assert_eq!(l.order(), [(0, FxZ::Top), (1, FxZ::Top)], "the order is kept across where they show");
        l.dirty = false;
        assert_eq!(l.hide(0), Some(true));
        l.clear();
        assert!(!l.dirty, "dropping staged layers costs her no frame");
    }
}
