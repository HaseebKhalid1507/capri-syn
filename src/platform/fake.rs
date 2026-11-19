//! An in-memory backend: drives the core in unit tests, and runs the contract suite in every
//! `cargo test` (with and without target events, so both change-detection paths are covered).

use super::contract::{self, Driver};
use super::{Caps, Desktop, FxZ, Hint, Monitor, Os, Place, PointerRead, Role, SessionEv, ShapeRect, Stage, Visible, Win, WinId, WinRect};
use anyhow::{Result, anyhow};
use gpui_kit::{Window, WindowKind, WindowOptions};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const BODY: WinId = 1;
/// Her body's size here (the contract places effects inside it).
const BODY_SIZE: (i32, i32) = (300, 400);

struct FWin {
    rect: WinRect,
    managed: bool,
    visible: bool,
    fullscreen: bool,
    /// Reserves this many px at the top of its monitor (a panel).
    reserve_top: i32,
    /// Its workspace (1, or 2: the one the second monitor shows).
    space: i64,
}

#[derive(Default)]
struct World {
    root: (i32, i32),
    monitors: Vec<Monitor>,
    /// Bottom → top.
    wins: Vec<(WinId, FWin)>,
    next: WinId,
    pointer: (i32, i32),
    focus: Option<WinId>,
    body_at: (i32, i32),
    input: Vec<ShapeRect>,
    /// None: all of the window.
    visible: Option<Vec<ShapeRect>>,
    /// Effect layers: origin and rects.
    fx: HashMap<usize, ((i32, i32), Vec<ShapeRect>)>,
    /// Like Hyprland today: no idle time of its own (the core counts from the pointer).
    no_idle: bool,
    /// He clicked on her since the keyboard last went back (`Caps::keyboard_on_input`).
    his_input: bool,
    /// The primary button as the server has it (None: can't tell).
    primary: Option<bool>,
    /// What her tray icon was told, in order.
    tray: Vec<String>,
}

type HintFn = Box<dyn Fn(Hint) + Send>;

type SessionFn = Box<dyn Fn(SessionEv) + Send>;

struct Fake {
    world: Mutex<World>,
    hint: Mutex<Option<HintFn>>,
    session: Mutex<Option<SessionFn>>,
    caps: Caps,
}

impl Fake {
    fn w(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().expect("fake world")
    }

    /// A hint, if this fake has target events (never under the world lock).
    fn hint(&self, h: Hint) {
        if self.caps.target_events
            && let Some(f) = self.hint.lock().expect("fake hint").as_ref()
        {
            f(h);
        }
    }

    fn win<T>(&self, w: WinId, f: impl FnOnce(&mut FWin) -> T) -> Option<T> {
        self.w().wins.iter_mut().find(|(id, _)| *id == w).map(|(_, x)| f(x))
    }
}

struct FakeDesktop(Arc<Fake>);

impl Desktop for FakeDesktop {
    fn caps(&self) -> Caps {
        self.0.caps
    }
    fn root_size(&self) -> (i32, i32) {
        self.0.w().root
    }
    fn monitors(&self) -> Vec<Monitor> {
        self.0.w().monitors.clone()
    }
    fn work_areas(&self, mons: &[Monitor]) -> Vec<Monitor> {
        let w = self.0.w();
        mons.iter()
            .map(|m| {
                let top = w.wins.iter().filter(|(_, f)| f.visible && f.reserve_top > 0 && m.rect().intersect(&f.rect).is_some()).map(|(_, f)| f.reserve_top).max().unwrap_or(0);
                Monitor { primary: m.primary, x: m.x, y: m.y + top, w: m.w, h: m.h - top }
            })
            .collect()
    }
    fn refresh_at(&self, _x: i32, _y: i32) -> Option<f32> {
        Some(144.0)
    }
    fn windows(&self) -> Vec<Win> {
        self.0.w().wins.iter().filter(|(_, f)| f.visible).map(|(_, f)| Win { rect: f.rect, managed: f.managed }).collect()
    }
    fn active_window(&self) -> Option<WinId> {
        None
    }
    fn focused_window(&self) -> Option<WinId> {
        self.0.w().focus
    }
    fn toplevel_of(&self, w: WinId) -> Option<WinId> {
        self.window_exists(w).then_some(w)
    }
    fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        self.toplevel_place(w).map(|p| p.rect)
    }
    fn toplevel_place(&self, w: WinId) -> Option<Place> {
        self.0.win(w, |f| f.visible.then_some(Place { rect: f.rect, space: f.space, loose: true })).flatten()
    }
    fn window_exists(&self, w: WinId) -> bool {
        self.0.win(w, |_| ()).is_some()
    }
    fn is_fullscreen(&self, w: WinId) -> bool {
        self.0.win(w, |f| f.fullscreen).unwrap_or(false)
    }
    fn pointer(&self) -> Result<(i32, i32)> {
        Ok(self.0.w().pointer)
    }
    fn primary_down(&self) -> Option<bool> {
        self.0.w().primary
    }
    fn pointer_reader(&self) -> Result<PointerRead> {
        let f = self.0.clone();
        Ok(Box::new(move || Some(f.w().pointer)))
    }
    fn idle_ms(&self) -> Option<u32> {
        (!self.0.w().no_idle).then_some(0)
    }
    fn watch_session(&self, on: Box<dyn Fn(SessionEv) + Send>) -> Result<()> {
        *self.0.session.lock().map_err(|_| anyhow!("fake session"))? = Some(on);
        Ok(())
    }
    fn char_key_down(&self, _ch: char) -> Result<bool> {
        Ok(false)
    }
    fn compositor_running(&self) -> bool {
        false
    }
    /// A picom desktop (the core then keeps her region and her moves apart).
    fn compositor_reshapes(&self) -> bool {
        true
    }
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        *self.0.hint.lock().map_err(|_| anyhow!("fake hint"))? = Some(hint);
        Ok(())
    }
    fn watch_clicks(&self, _on: Box<dyn Fn(i32, i32) + Send>) -> Result<()> {
        Ok(())
    }
    fn arm_clicks(&self, _on: bool) {}
    fn tray(&self, _icon: super::TrayIcon, _instance: &str, _on: Box<dyn Fn(super::TrayEv) + Send>) -> Result<Option<Box<dyn super::Tray>>> {
        Ok(Some(Box::new(FakeTray(self.0.clone()))))
    }
}

/// A tray that writes down what it is told.
struct FakeTray(Arc<Fake>);

impl super::Tray for FakeTray {
    fn set_tip(&self, tip: &str) {
        self.0.w().tray.push(format!("tip {tip}"));
    }
    fn set_icon(&self, icon: super::TrayIcon) {
        self.0.w().tray.push(format!("icon {}x{}", icon.w, icon.h));
    }
    fn set_menu(&self, items: Vec<super::TrayItem>) {
        let lead = items.iter().find_map(|i| match i {
            super::TrayItem::Item { label, lead: true, .. } => Some(label.clone()),
            _ => None,
        });
        self.0.w().tray.push(format!("menu {} lead {}", items.len(), lead.unwrap_or_default()));
    }
    fn remove(&self) {
        self.0.w().tray.push("remove".into());
    }
}

struct FakeStage(Arc<Fake>);

impl Stage for FakeStage {
    fn dress(&self, _role: Role, o: &mut WindowOptions) {
        o.kind = WindowKind::PopUp;
    }
    fn adopt(&self, _role: Role, _window: &Window) -> Result<WinId> {
        Ok(BODY)
    }
    fn body(&self) -> WinId {
        BODY
    }
    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.0.w().body_at = (x, y);
        Ok(())
    }
    fn raise(&self) -> Result<()> {
        Ok(())
    }
    fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.0.win(s, |f| f.rect = WinRect { id: s, x, y, w, h });
        Ok(())
    }
    fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<Visible<'_>>) -> Result<()> {
        let mut w = self.0.w();
        if let Some(i) = input {
            w.input = i.to_vec();
        }
        match visible {
            Some(Visible::All) => w.visible = None,
            Some(Visible::Rects(r)) => w.visible = Some(r.to_vec()),
            None => {}
        }
        Ok(())
    }
    fn shape_of(&self, _s: WinId, _input: &[ShapeRect], _visible: Visible<'_>) -> Result<()> {
        Ok(())
    }
    fn set_icon(&self, _s: WinId, _data: &[u32]) -> Result<()> {
        Ok(())
    }
    fn fx_show(&self, layer: usize, _rgb: u32, origin: (i32, i32), _size: (i32, i32), rects: Option<&[ShapeRect]>, _z: FxZ) -> Result<()> {
        let mut w = self.0.w();
        let rects = rects.map(<[ShapeRect]>::to_vec).or_else(|| w.fx.get(&layer).map(|l| l.1.clone())).unwrap_or_default();
        w.fx.insert(layer, (origin, rects));
        Ok(())
    }
    fn fx_hide(&self, layer: usize) {
        self.0.w().fx.remove(&layer);
    }
    fn fx_hide_all(&self) {
        self.0.w().fx.clear();
    }
    fn flush(&self) {}
    /// Where the keyboard comes only on his input (as on Windows): refused without it.
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        if self.0.caps.keyboard_on_input && !self.keyboard_granted() {
            anyhow::bail!("fake: no input of his to her");
        }
        self.0.w().focus = Some(s);
        Ok(())
    }
    fn keyboard_granted(&self) -> bool {
        let w = self.0.w();
        w.his_input || w.focus == Some(BODY)
    }
    /// Back to `to` only from her (where the keyboard comes on his input, his input is spent).
    fn release_keyboard(&self, _s: WinId, to: Option<WinId>) -> Result<()> {
        let mut w = self.0.w();
        if self.0.caps.keyboard_on_input {
            if w.focus != Some(BODY) {
                return Ok(());
            }
            w.his_input = false;
        }
        if to.is_some() {
            w.focus = to;
        }
        Ok(())
    }
}

struct FakeDriver(Arc<Fake>);

impl FakeDriver {
    fn add(&mut self, rect: WinRect, managed: bool, reserve_top: i32) -> WinId {
        let id = {
            let mut w = self.0.w();
            w.next += 1;
            let id = w.next;
            w.wins.push((id, FWin { rect: WinRect { id, ..rect }, managed, visible: true, fullscreen: false, reserve_top, space: 1 }));
            id
        };
        self.0.hint(Hint::Mapped(id));
        id
    }
}

impl Driver for FakeDriver {
    fn open(&mut self, r: WinRect, managed: bool) -> WinId {
        self.add(r, managed, 0)
    }
    fn configure(&mut self, w: WinId, r: WinRect) {
        let r = WinRect { id: w, ..r };
        self.0.win(w, |f| f.rect = r);
        self.0.hint(Hint::Moved(w, None));
    }
    fn hide(&mut self, w: WinId) {
        self.0.win(w, |f| f.visible = false);
        self.0.hint(Hint::Gone(w));
    }
    fn show(&mut self, w: WinId) {
        self.0.win(w, |f| (f.visible, f.space) = (true, 1));
        self.0.hint(Hint::Mapped(w));
    }
    fn close(&mut self, w: WinId) {
        self.0.w().wins.retain(|(id, _)| *id != w);
        self.0.hint(Hint::Gone(w));
    }
    fn fullscreen(&mut self, w: WinId, on: bool) {
        self.0.win(w, |f| f.fullscreen = on);
    }
    fn split_monitors(&mut self, on: bool) -> bool {
        let mut w = self.0.w();
        let (rw, rh) = w.root;
        w.monitors = if on {
            vec![Monitor { primary: true, x: 0, y: 0, w: rw / 2, h: rh }, Monitor { primary: false, x: rw / 2, y: 0, w: rw - rw / 2, h: rh }]
        } else {
            vec![Monitor { primary: true, x: 0, y: 0, w: rw, h: rh }]
        };
        true
    }
    fn dock_top(&mut self, h: i32) -> WinId {
        let rw = self.0.w().root.0;
        self.add(WinRect::new(0, 0, rw, h), true, h)
    }
    fn warp(&mut self, x: i32, y: i32) {
        self.0.w().pointer = (x, y);
    }
    fn body_rect(&mut self) -> Option<WinRect> {
        let (x, y) = self.0.w().body_at;
        Some(WinRect { id: BODY, x, y, w: BODY_SIZE.0, h: BODY_SIZE.1 })
    }
    /// What a server would paint: each layer's rects, at its origin.
    fn fx_rects(&mut self) -> Vec<WinRect> {
        let w = self.0.w();
        w.fx.values().filter_map(|((x, y), r)| crate::shape::bbox(r).map(|b| WinRect::new(x + b.0 as i32, y + b.1 as i32, b.2 as i32, b.3 as i32))).collect()
    }
    fn hits(&mut self, x: i32, y: i32) -> bool {
        let w = self.0.w();
        crate::shape::hit(&w.input, x - w.body_at.0, y - w.body_at.1)
    }
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>> {
        self.0.w().visible.clone()
    }
    async fn settle(&mut self) {}
    fn his_click(&mut self, _s: WinId) -> bool {
        self.0.w().his_input = true;
        true
    }
    fn session(&mut self, ev: SessionEv) -> bool {
        if let Some(f) = self.0.session.lock().expect("fake session").as_ref() {
            f(ev);
        }
        true
    }
    /// Spaces only where target events are polled (a backend with events has none here).
    fn to_other_space(&mut self, w: WinId) -> bool {
        if self.0.caps.target_events {
            return false;
        }
        self.0.win(w, |f| f.space = 2);
        true
    }
    fn flip_other_space(&mut self) -> bool {
        !self.0.caps.target_events
    }
}

fn fake(target_events: bool) -> (Os, FakeDriver) {
    fake_with(target_events, false)
}

/// `keyboard_on_input`: the keyboard comes only on his input (Windows' rule).
fn fake_with(target_events: bool, keyboard_on_input: bool) -> (Os, FakeDriver) {
    let world = World { root: (1600, 1000), monitors: vec![Monitor { primary: true, x: 0, y: 0, w: 1600, h: 1000 }], next: 100, ..World::default() };
    let caps = Caps { shape_mode: true, target_events, fx_layers: true, costly_redraw: false, implicit_grab: true, goal_geometry: false, layout_events: false, host_per_output: false, keyboard_on_input };
    let f = Arc::new(Fake { world: Mutex::new(world), hint: Mutex::new(None), session: Mutex::new(None), caps });
    (Os::new(Arc::new(FakeDesktop(f.clone())), Box::new(FakeStage(f.clone()))), FakeDriver(f))
}

#[test]
fn contract_fake_with_target_events() {
    let (os, mut d) = fake(true);
    let passed = contract::block_on(contract::run(&os, &mut d)).unwrap();
    assert_eq!(passed.len(), 14);
}

#[test]
fn without_idle_time_presence_is_the_pointer() {
    let (os, mut d) = fake(true);
    d.0.w().no_idle = true;
    assert_eq!(os.idle_ms(), None, "nothing seen yet");
    let cfg = super::pointer::cfg(1.0);
    os.watch_pointer(cfg, |_| {}).unwrap();
    os.set_pointer_watch(Some(WinRect::new(600, 300, 100, 200)), true);
    std::thread::sleep(std::time::Duration::from_millis(300));
    d.warp(560, 400);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let ms = os.idle_ms().expect("the pointer's");
    assert!(ms < 1000, "{ms} ms since it moved");
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(os.idle_ms().unwrap() >= ms + 500, "and growing while it's still");
    os.set_pointer_watch(None, false);
}

#[test]
fn contract_fake_polled_target() {
    let (os, mut d) = fake(false);
    contract::block_on(contract::run(&os, &mut d)).unwrap();
}

/// The keyboard only on his input (Windows' foreground lock): every scenario still passes, and
/// `keyboard` checks the refusal and the grant.
#[test]
fn contract_fake_keyboard_on_his_input() {
    let (os, mut d) = fake_with(true, true);
    let passed = contract::block_on(contract::run(&os, &mut d)).unwrap();
    assert_eq!(passed.len(), 14);
    // Nothing of his since the keyboard went back: none to take.
    assert!(!os.may_take_keyboard());
    assert!(os.grab_keyboard(os.body()).is_err());
}

/// Her tray hears only what changed: the same tooltip and menu every schedule cost the desktop
/// nothing, and a quit takes the icon out once.
#[test]
fn the_tray_hears_only_changes() {
    use crate::menu::{State, rows, tray_items};
    use std::cell::Cell;
    let (os, d) = fake(true);
    let made = Cell::new(0);
    let show = |key: u64, tip: &str, s: State| {
        os.tray_show(key, || {
            made.set(made.get() + 1);
            (tip.to_string(), tray_items(&rows(&s)))
        })
    };
    show(1, "capri-syn · ONLINE", State::default());
    assert!(!os.has_tray() && d.0.w().tray.is_empty() && made.get() == 0, "no tray yet: nothing made, nothing told");
    let icon = |stamp: u64, n: u32| super::TrayIcon { file: None, stamp, w: n, h: n, rgba: vec![0; (n * n * 4) as usize] };
    assert!(os.start_tray(icon(7, 256), "", |_| {}).unwrap() && os.has_tray());
    for _ in 0..3 {
        show(1, "capri-syn · ONLINE", State::default());
    }
    assert_eq!(made.get(), 1, "the same key: made once");
    // A new key: made again, and only what changed is told.
    show(2, "capri-syn · ONLINE", State::default());
    show(3, "capri-syn · SLEEPING", State::default());
    show(4, "capri-syn · HIDDEN", State { hidden: true, ..State::default() });
    assert_eq!(made.get(), 4);
    // His own icon put in (`settings reload`); the same stamp again is no news.
    assert!(!os.tray_icon(icon(7, 256)));
    assert!(os.tray_icon(icon(8, 48)) && !os.tray_icon(icon(8, 48)));
    os.tray_remove();
    os.tray_remove();
    show(5, "capri-syn · ONLINE", State::default());
    assert!(!os.tray_icon(icon(9, 32)));
    assert!(!os.has_tray());
    assert_eq!(d.0.w().tray, ["tip capri-syn · ONLINE", "menu 6 lead ", "tip capri-syn · SLEEPING", "tip capri-syn · HIDDEN", "menu 6 lead SHOW", "icon 48x48", "remove"]);
}

#[test]
fn window_options_are_the_cores() {
    let (os, _) = fake(true);
    let b = gpui_kit::Bounds::new(gpui_kit::point(gpui_kit::px(1.0), gpui_kit::px(2.0)), gpui_kit::size(gpui_kit::px(3.0), gpui_kit::px(4.0)));
    let body = os.window_options(Role::Body, b, false);
    assert!(body.inactive_frame_interval.is_none() && !body.focus && matches!(body.kind, WindowKind::PopUp));
    let line = os.window_options(Role::Line, b, true);
    assert!(line.inactive_frame_interval.is_some() && line.focus);
}

#[test]
fn caches_hold_and_freeze() {
    let (os, mut d) = fake(true);
    assert_eq!(os.monitors().len(), 1);
    assert_eq!(os.work_areas().len(), 1);
    d.split_monitors(true);
    assert_eq!(os.monitors().len(), 1, "cached for AREAS_TTL");
    os.freeze_layout(true);
    assert_eq!(os.monitors().len(), 1, "a fresh snapshot is kept while carried");
    os.freeze_layout(false);
    os.forget_layout();
    assert_eq!(os.monitors().len(), 2);
    os.freeze_layout(true);
    assert_eq!(os.work_areas().len(), 2, "stale areas: freezing takes a fresh snapshot");
    os.freeze_layout(false);
}

/// Under picom (a compositor that takes a region change for a resize: the fake desktop says
/// so at connect, `Desktop::compositor_reshapes`): her input region waits while she moves or
/// the button is down on her, a move right after a region change waits, a landing never does,
/// so picom never takes a region change and a move in one frame (it then cross-faded her from
/// a snapshot of herself facing the old way until she held still). Another compositor, or none
/// (her shape is what shows), gets both at once. The timings: `settle.rs`.
#[test]
fn input_region_and_moves_never_meet_under_picom() {
    use super::settle::{GAP, SETTLE};
    use std::time::Duration;
    let (os, d) = fake(true);
    os.spawn_watcher(|_| ()).unwrap();
    let settled = || {
        while let Some(t) = os.settle_due() {
            std::thread::sleep(t.saturating_duration_since(std::time::Instant::now()) + Duration::from_millis(1));
            os.settle().unwrap();
        }
    };
    // One lock per look (the world's mutex isn't reentrant).
    let got = || {
        let w = d.0.w();
        (w.body_at, w.input.clone())
    };
    os.force_composited(Some(true));
    assert!(os.settling(), "picom, in alpha mode");
    os.move_to(100, 100).unwrap();
    os.set_shapes(Some(&[(1, 2, 3, 4)]), None).unwrap();
    assert_eq!(got(), ((100, 100), vec![]), "the move out, the region held");
    assert!(os.settle_due().is_some());
    os.set_shapes(Some(&[(5, 6, 7, 8)]), None).unwrap();
    os.move_to(104, 100).unwrap();
    assert_eq!(got().1, vec![], "still moving: still held");
    settled();
    assert_eq!(got().1, vec![(5, 6, 7, 8)], "the newest region, once she held still");
    os.move_to(110, 100).unwrap();
    assert_eq!((got().0, os.shown_at()), ((104, 100), (104, 100)), "a move right after a region change waits");
    os.move_to(112, 100).unwrap();
    std::thread::sleep(GAP);
    os.settle().unwrap();
    assert_eq!(got().0, (112, 100), "the newest position, once due");
    assert!(os.settle_due().is_none());
    // A landing never waits, even right after a region change.
    std::thread::sleep(SETTLE);
    os.set_shapes(Some(&[(1, 1, 1, 1)]), None).unwrap();
    os.move_now(500, 100).unwrap();
    assert_eq!(got(), ((500, 100), vec![(1, 1, 1, 1)]), "a landing goes out at once");
    // The button down on her: her region waits for the release, a drag move never waits for
    // one (the last region out long before).
    std::thread::sleep(SETTLE);
    os.set_held(true, false);
    os.set_shapes(Some(&[(2, 2, 2, 2)]), None).unwrap();
    std::thread::sleep(SETTLE + Duration::from_millis(5));
    os.settle().unwrap();
    assert_eq!(got().1, vec![(1, 1, 1, 1)], "pressed: held, however still she is");
    assert!(os.settle_due().is_none(), "nothing due while pressed");
    os.move_to(506, 100).unwrap();
    assert_eq!(got().0, (506, 100), "a drag move never waits");
    os.set_held(false, false);
    settled();
    assert_eq!(got().1, vec![(2, 2, 2, 2)], "released and still: out");
    // A scripted drag stands in for a real one: in hand the same way, from its first frame
    // drawn (`rag_script_held`): that frame's region goes out as she stands, then her first
    // move waits out `GAP` (picom shows her picked up before she moves), and no more. A catch
    // right after a region went out mid-flight waits the same.
    os.set_shapes(Some(&[(4, 4, 4, 4)]), None).unwrap();
    assert_eq!(got().1, vec![(4, 4, 4, 4)], "still: out at once");
    os.set_held(false, true);
    os.move_to(508, 100).unwrap();
    assert_eq!(got().0, (506, 100), "in hand, a move right after a region still waits out GAP");
    assert!(os.settle_due().is_some_and(|t| t <= std::time::Instant::now() + GAP), "due within GAP");
    std::thread::sleep(GAP);
    os.settle().unwrap();
    assert_eq!(got().0, (508, 100), "out once GAP passed");
    os.move_to(509, 100).unwrap();
    assert_eq!(got().0, (509, 100), "then never waits");
    os.set_shapes(Some(&[(5, 5, 5, 5)]), None).unwrap();
    std::thread::sleep(SETTLE + Duration::from_millis(5));
    os.settle().unwrap();
    assert_eq!(got().1, vec![(4, 4, 4, 4)], "held by a script: her region waits for the release");
    os.set_held(false, false);
    settled();
    assert_eq!(got().1, vec![(5, 5, 5, 5)], "let go and still: out");
    // Another compositor (KWin, Mutter, ...): not held to it.
    d.0.hint(Hint::Compositor { runs: true, reshapes: false });
    assert!(!os.settling(), "only picom");
    os.move_to(510, 100).unwrap();
    os.set_shapes(Some(&[(3, 3, 3, 3)]), None).unwrap();
    os.move_to(512, 100).unwrap();
    assert_eq!(got(), ((512, 100), vec![(3, 3, 3, 3)]), "both at once");
    d.0.hint(Hint::Compositor { runs: true, reshapes: true });
    assert!(os.settling(), "picom again");
    // Shape mode: her shape must follow every frame.
    os.force_composited(Some(false));
    assert!(!os.settling());
    os.move_to(520, 100).unwrap();
    os.set_shapes(Some(&[(9, 9, 9, 9)]), None).unwrap();
    os.move_to(524, 100).unwrap();
    assert_eq!(got(), ((524, 100), vec![(9, 9, 9, 9)]), "both at once");
    assert!(os.settle_due().is_none());
}

/// A press on her whose release never came (a grab elsewhere took it): lost once the server
/// has had the button up for `PRESS_LOST`, never while it is down or where the backend can't
/// tell; a quick tap (the release on its way) is never taken for one.
#[test]
fn a_press_without_its_release_is_lost_once_the_button_stays_up() {
    use std::time::{Duration, Instant};
    let (os, d) = fake(true);
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let set = |b: Option<bool>| d.0.w().primary = b;
    // Can't tell: never lost, and no wake for it.
    assert_eq!(os.press_due(true, at(0)), Some(at(0)), "a press not asked about yet: a look now");
    assert!(!os.press_lost(true, at(0)) && !os.press_lost(true, at(5_000)));
    assert_eq!(os.press_due(true, at(5_000)), None);
    os.press_lost(false, at(5_000));
    assert_eq!(os.press_due(false, at(5_000)), None, "no press, no wake");
    // Down: never lost.
    set(Some(true));
    for ms in (0..2_000).step_by(60) {
        assert!(!os.press_lost(true, at(10_000 + ms)), "lost with the button down");
        assert_eq!(os.press_due(true, at(10_000 + ms)), Some(at(10_000 + ms + 50)), "down: a look every PRESS_ASK");
    }
    os.press_lost(false, at(20_000));
    // Up (its release still on the way, or never coming): lost only after PRESS_LOST.
    set(Some(false));
    assert!(!os.press_lost(true, at(30_000)), "a quick tap's release may still be on its way");
    assert_eq!(os.press_due(true, at(30_000)), Some(at(30_200)), "up: a wake when it is lost");
    assert!(!os.press_lost(true, at(30_100)));
    assert!(!os.press_lost(true, at(30_190)));
    assert!(os.press_lost(true, at(30_260)), "up for 200 ms: lost");
    // The release came after all (the view's press ended): nothing to lose; a new press
    // starts over.
    assert!(!os.press_lost(false, at(30_300)));
    assert!(!os.press_lost(true, at(30_400)), "a new press: up since now, not since before");
    // Down again in between: the up time starts over.
    set(Some(true));
    assert!(!os.press_lost(true, at(30_500)));
    set(Some(false));
    assert!(!os.press_lost(true, at(30_560)));
    assert!(!os.press_lost(true, at(30_700)));
    assert!(os.press_lost(true, at(30_780)));
}

/// Where effect layers show, on a backend with surfaces for them: on those surfaces without a
/// compositor (her window shows only her figure's shape); in her window when a compositor
/// blends her and the layer fits in it (separate surfaces over her translucent window got
/// dark boxes from the compositor around every glint); on surfaces again for what leaves her window
/// (the bolt). A compositor coming or going moves the layers that are up.
#[test]
fn effects_fit_in_her_window_when_composited() {
    let (os, mut d) = fake(true);
    let staged = |d: &mut FakeDriver| {
        let mut v: Vec<_> = d.fx_rects().iter().map(|r| (r.x, r.y, r.w, r.h)).collect();
        v.sort();
        v
    };
    let star: Vec<ShapeRect> = vec![(4, 0, 2, 10), (0, 4, 10, 2)];
    os.set_body_size(BODY_SIZE.0, BODY_SIZE.1);
    os.move_to(100, 100).unwrap();
    // Shape mode: every layer on a surface of its own, her window paints none.
    os.force_composited(Some(false));
    os.fx_set(30, 0xffc2e2, (150, 160), (10, 10), &star).unwrap();
    os.fx_set_below(29, 30, 0xffc2e2, (149, 159), (12, 12), &[(5, 0, 2, 12), (0, 5, 12, 2)]).unwrap();
    os.fx_set(0, 0xff0000, (100, 300), (900, 40), &[(0, 0, 900, 4)]).unwrap();
    os.fx_under(14, 0x05060a, (110, 480), (200, 6), Some(&[(0, 0, 200, 3)])).unwrap();
    assert!(!os.fx_in_scene() && !os.fx_take_dirty());
    assert!(!os.redraw_costly(), "server-painted steps are free: no need to ride other redraws");
    assert_eq!(staged(&mut d), [(100, 300, 900, 4), (110, 480, 200, 3), (149, 159, 12, 12), (150, 160, 10, 10)]);
    // The shadow follows her with its rects kept (`None`), still on its surface.
    os.fx_under(14, 0x05060a, (112, 480), (200, 6), None).unwrap();
    assert!(staged(&mut d).contains(&(112, 480, 200, 3)) && os.fx_scene(true).is_empty());
    // A compositor comes: what fits in her window moves into it (no surface left over), the bolt
    // reaching past her stays on its own.
    os.force_composited(Some(true));
    os.fx_reroute();
    assert!(os.fx_in_scene() && os.fx_take_dirty(), "her window needs a frame for them");
    assert!(os.redraw_costly(), "each step in her window is a redraw of her: steps ride redraws");
    assert_eq!(staged(&mut d), [(100, 300, 900, 4)], "only the bolt keeps a surface");
    assert_eq!(os.fx_scene(false), [((54, 59, 2, 12), 0xffc2e2), ((49, 64, 12, 2), 0xffc2e2), ((54, 60, 2, 10), 0xffc2e2), ((50, 64, 10, 2), 0xffc2e2)], "glow below its core, window-local");
    assert_eq!(os.fx_scene(true), [((12, 380, 200, 3), 0x05060a)], "the shadow under her body");
    // The next glint step lands in her window too; a layer leaving it gets a surface, rects and all.
    os.fx_set(30, 0xffffff, (151, 160), (10, 10), &star).unwrap();
    assert!(os.fx_take_dirty() && os.fx_scene(false).last() == Some(&((51, 64, 10, 2), 0xffffff)));
    assert_eq!(staged(&mut d).len(), 1, "a glint step never maps a surface over her");
    os.fx_under(14, 0x05060a, (350, 480), (200, 6), None).unwrap();
    assert!(staged(&mut d).contains(&(350, 480, 200, 3)) && os.fx_scene(true).is_empty() && os.fx_take_dirty());
    // Without her size (before `adopt`) nothing is known to fit: surfaces, as before.
    os.set_body_size(0, 0);
    os.fx_set(31, 0xffc2e2, (150, 160), (10, 10), &star).unwrap();
    assert!(staged(&mut d).contains(&(150, 160, 10, 10)));
    os.fx_unmap(31);
    os.set_body_size(BODY_SIZE.0, BODY_SIZE.1);
    // The compositor goes: everything back on surfaces, her window paints nothing.
    os.force_composited(Some(false));
    os.fx_reroute();
    assert!(!os.fx_in_scene() && os.fx_scene(false).is_empty() && os.fx_scene(true).is_empty());
    assert_eq!(staged(&mut d), [(100, 300, 900, 4), (149, 159, 12, 12), (151, 160, 10, 10), (350, 480, 200, 3)]);
    os.fx_unmap(30);
    assert_eq!(staged(&mut d).len(), 3);
    os.force_composited(Some(true));
    os.fx_reroute();
    os.fx_hide();
    assert!(staged(&mut d).is_empty() && os.fx_scene(false).is_empty() && os.fx_scene(true).is_empty());
}
