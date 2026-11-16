//! The core over whichever backend: every desktop decision, once. A backend's answers come
//! in as plain data (`Desktop`) and go out as primitives (`Stage`); the rules live here.

use super::{Caps, Desktop, FxZ, Hint, Monitor, PointerCfg, PointerEv, Role, SEAT_MIN, ShapeRect, Stage, Visible, WatchEvent, WinId, WinRect};
use anyhow::Result;
use gpui_kit::{Bounds, Pixels, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long monitors and work areas are reused (work areas cost a stack walk).
const AREAS_TTL: Duration = Duration::from_secs(3);
/// Backends without target events: how often the perch target is re-read.
const TARGET_POLL: Duration = Duration::from_millis(250);

/// What the watcher threads share with the core.
struct Shared {
    /// The window the watcher follows (0 = none).
    target: AtomicU32,
    /// A compositor runs (kept current by the watcher).
    composited: AtomicBool,
    /// The target poller (backends without target events), woken by a new target.
    poller: OnceLock<std::thread::Thread>,
}

impl Shared {
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
            Hint::Focus => on(WatchEvent::Focus),
            Hint::Desktop => on(WatchEvent::Desktop),
            Hint::Compositor(now) if self.composited.swap(now, Ordering::Relaxed) != now => on(WatchEvent::Compositor(now)),
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

type Cached = RefCell<Option<(Instant, Vec<Monitor>)>>;

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
    /// The pointer polled on its own connection while she's held by the real mouse:
    /// (x << 32 | y as u32, or u64::MAX before the first read; stop flag).
    poll: RefCell<Option<(Arc<AtomicU64>, Arc<AtomicBool>)>>,
    /// Bounding shapes sent, and their rects (for the rag's meter).
    shape_sends: Cell<u64>,
    shape_rects: Cell<u64>,
    pointer_watch: Arc<PointerWatch>,
}

impl Os {
    pub fn new(desk: Arc<dyn Desktop>, stage: Box<dyn Stage>) -> Os {
        let caps = desk.caps();
        let shared = Shared { target: AtomicU32::new(0), composited: AtomicBool::new(desk.compositor_running()), poller: OnceLock::new() };
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
        if role == Role::Body {
            // She is never the active window: GPUI's default caps inactive windows at 30 fps
            // (a drag then ran ~40 ticks/s). Our own deadlines decide when she draws.
            o.inactive_frame_interval = None;
        }
        self.stage.dress(role, &mut o);
        o
    }

    pub fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        self.stage.adopt(role, window)
    }

    /// Her body's window id.
    pub fn body(&self) -> WinId {
        self.stage.body()
    }

    pub fn move_to(&self, x: i32, y: i32) -> Result<()> {
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
    pub fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<&[ShapeRect]>) -> Result<()> {
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
            self.stage.fx_hide(layer);
            return Ok(());
        }
        self.stage.fx_show(layer, rgb, origin, size, rects, z)
    }

    /// Hide one effect layer if it is up.
    pub fn fx_unmap(&self, layer: usize) {
        self.stage.fx_hide(layer);
    }

    pub fn fx_hide(&self) {
        self.stage.fx_hide_all();
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

    /// The root's size, px.
    pub fn root_size(&self) -> (i32, i32) {
        self.desk.root_size()
    }

    fn cached(&self, c: &Cached) -> Option<Vec<Monitor>> {
        c.borrow().as_ref().filter(|(t, _)| t.elapsed() < AREAS_TTL || self.frozen.get()).map(|(_, m)| m.clone())
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
        *self.mons.borrow_mut() = Some((Instant::now(), out.clone()));
        out
    }

    /// Each monitor minus the docks, same order (cached like the monitors).
    pub fn work_areas(&self) -> Vec<Monitor> {
        if let Some(a) = self.cached(&self.areas) {
            return a;
        }
        let areas = self.desk.work_areas(&self.monitors());
        *self.areas.borrow_mut() = Some((Instant::now(), areas.clone()));
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

    /// Drop the cached layout (tests change monitors and panels under it).
    #[cfg(test)]
    pub fn forget_layout(&self) {
        *self.mons.borrow_mut() = None;
        *self.areas.borrow_mut() = None;
    }

    /// Carried (on) or back to normal (off): on takes a fresh snapshot of the monitors and
    /// work areas and keeps it until off.
    pub fn freeze_layout(&self, on: bool) {
        // A snapshot younger than ~1 s is fresh enough (the refresh walks the tree: a few ms
        // on the first dragged frame).
        let fresh = |c: &Cached| c.borrow().as_ref().is_some_and(|(t, _)| t.elapsed() < Duration::from_secs(1));
        if on && !(fresh(&self.mons) && fresh(&self.areas)) {
            self.frozen.set(false);
            *self.mons.borrow_mut() = None;
            *self.areas.borrow_mut() = None;
            self.work_areas();
        }
        self.frozen.set(on);
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

    pub fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        self.desk.toplevel_rect(w)
    }

    pub fn window_exists(&self, w: WinId) -> bool {
        self.desk.window_exists(w)
    }

    pub fn is_fullscreen(&self, w: WinId) -> bool {
        self.desk.is_fullscreen(w)
    }

    pub fn idle_ms(&self) -> Option<u32> {
        self.desk.idle_ms()
    }

    /// Follow `w` (0 = nothing).
    pub fn set_target(&self, w: WinId) {
        self.shared.target.store(w, Ordering::Relaxed);
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

    /// Re-read the target every `TARGET_POLL` while there is one (parked otherwise), diff,
    /// and feed the differences through the same path as a backend's hints.
    fn spawn_target_poll(&self, on: Arc<dyn Fn(WatchEvent) + Send + Sync>) -> Result<()> {
        let (s, d) = (self.shared.clone(), self.desk.clone());
        let h = std::thread::Builder::new().name("avatar-target".into()).spawn(move || {
            let mut last: (WinId, Option<WinRect>) = (0, None);
            loop {
                let t = s.target.load(Ordering::Relaxed);
                if t == 0 {
                    last = (0, None);
                    std::thread::park();
                    continue;
                }
                let now = d.toplevel_rect(t);
                if last.0 == t {
                    let h = match (last.1, now) {
                        (Some(a), Some(b)) if a != b => Some(Hint::Moved(t, Some(b))),
                        (Some(_), None) => Some(Hint::Gone(t)),
                        (None, Some(_)) => Some(Hint::Mapped(t)),
                        _ => None,
                    };
                    if let Some(h) = h {
                        s.hint(&*d, h, &*on);
                    }
                }
                last = (t, now);
                std::thread::park_timeout(TARGET_POLL);
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
    pub fn arm_clicks(&self, on: bool) {
        self.desk.arm_clicks(on);
    }

    // ---- pointer ---------------------------------------------------------------------------

    pub fn pointer(&self) -> Result<(i32, i32)> {
        self.desk.pointer()
    }

    /// The polled pointer while polling (and read once), else a fresh read.
    pub fn pointer_fast(&self) -> Result<(i32, i32)> {
        if let Some((pos, _)) = self.poll.borrow().as_ref() {
            let v = pos.load(Ordering::Relaxed);
            if v != u64::MAX {
                return Ok(((v >> 32) as u32 as i32, v as u32 as i32));
            }
        }
        self.pointer()
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
        let pos = Arc::new(AtomicU64::new(u64::MAX));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s, d) = (pos.clone(), stop.clone(), self.desk.clone());
        let spawned = std::thread::Builder::new().name("avatar-pointer".into()).spawn(move || {
            let Ok(mut read) = d.pointer_reader() else { return };
            while !s.load(Ordering::Relaxed) {
                if let Some((x, y)) = read() {
                    p.store(((x as u32 as u64) << 32) | y as u32 as u64, Ordering::Relaxed);
                }
                // 2 ms: the window follows it at up to the monitor's refresh (see rag_follow).
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        if spawned.is_ok() {
            *g = Some((pos, stop));
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
            let mut last: Option<(Instant, i32, i32)> = None;
            let (mut was_near, mut hover_since): (bool, Option<Instant>) = (false, None);
            let mut hovered = false;
            loop {
                let armed = pw.armed.load(Ordering::Relaxed);
                let Some((x, y)) = read() else { return };
                let now = Instant::now();
                if last.is_none_or(|l| (l.1, l.2) != (x, y)) {
                    pw.moved_ms.store(now.duration_since(t0).as_millis() as u64, Ordering::Relaxed);
                }
                let her = pw.her.lock().map(|g| *g).unwrap_or_default();
                let mut far = true;
                if armed && her.w > 0 {
                    let d = rect_dist(&her, x as f32, y as f32);
                    far = d > 700.0;
                    let near = d <= cfg.near_px;
                    if near && !was_near && d > 0.0 {
                        on(PointerEv::Near);
                    }
                    was_near = near;
                    // Resting on her a while (once per visit of the pointer).
                    if d <= 0.0 {
                        let since = *hover_since.get_or_insert(now);
                        if !hovered && now.duration_since(since) >= cfg.hover {
                            hovered = true;
                            on(PointerEv::Hover);
                        }
                    } else {
                        hover_since = None;
                        hovered = false;
                    }
                    // A fast swipe past her: the segment since the last sample comes close,
                    // both ends outside her.
                    if let Some((lt, lx, ly)) = last {
                        let dt = now.duration_since(lt).as_secs_f32();
                        let len = (((x - lx).pow(2) + (y - ly).pow(2)) as f32).sqrt();
                        if dt > 0.0 && dt < 0.4 && len / dt >= cfg.swipe_speed && d > 0.0 && rect_dist(&her, lx as f32, ly as f32) > 0.0 {
                            let seg = seg_rect_dist(&her, (lx as f32, ly as f32), (x as f32, y as f32));
                            if seg <= cfg.swipe_px {
                                on(PointerEv::Swipe(len / dt));
                            }
                        }
                    }
                } else {
                    was_near = false;
                    hover_since = None;
                }
                last = Some((now, x, y));
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
fn rect_dist(r: &WinRect, x: f32, y: f32) -> f32 {
    let dx = (r.x as f32 - x).max(x - r.right() as f32).max(0.0);
    let dy = (r.y as f32 - y).max(y - r.bottom() as f32).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Closest distance from the segment a→b to the rect (sampled: 24 points, enough at 60 ms).
fn seg_rect_dist(r: &WinRect, a: (f32, f32), b: (f32, f32)) -> f32 {
    (0..=24).map(|k| k as f32 / 24.0).map(|u| rect_dist(r, a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)).fold(f32::MAX, f32::min)
}
