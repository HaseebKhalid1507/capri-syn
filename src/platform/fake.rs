//! An in-memory backend: drives the core in unit tests, and runs the contract suite in every
//! `cargo test` (with and without target events, so both change-detection paths are covered).

use super::contract::{self, Driver};
use super::{Caps, Desktop, FxZ, Hint, Monitor, Os, Place, PointerRead, Role, ShapeRect, Stage, Visible, Win, WinId, WinRect};
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
}

type HintFn = Box<dyn Fn(Hint) + Send>;

struct Fake {
    world: Mutex<World>,
    hint: Mutex<Option<HintFn>>,
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
    fn pointer_reader(&self) -> Result<PointerRead> {
        let f = self.0.clone();
        Ok(Box::new(move || Some(f.w().pointer)))
    }
    fn idle_ms(&self) -> Option<u32> {
        Some(0)
    }
    fn char_key_down(&self, _ch: char) -> Result<bool> {
        Ok(false)
    }
    fn compositor_running(&self) -> bool {
        false
    }
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        *self.0.hint.lock().map_err(|_| anyhow!("fake hint"))? = Some(hint);
        Ok(())
    }
    fn watch_clicks(&self, _on: Box<dyn Fn(i32, i32) + Send>) -> Result<()> {
        Ok(())
    }
    fn arm_clicks(&self, _on: bool) {}
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
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        self.0.w().focus = Some(s);
        Ok(())
    }
    fn release_keyboard(&self, _s: WinId, to: Option<WinId>) -> Result<()> {
        if to.is_some() {
            self.0.w().focus = to;
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
    let world = World { root: (1600, 1000), monitors: vec![Monitor { primary: true, x: 0, y: 0, w: 1600, h: 1000 }], next: 100, ..World::default() };
    let f = Arc::new(Fake { world: Mutex::new(world), hint: Mutex::new(None), caps: Caps { shape_mode: true, target_events, fx_layers: true, costly_redraw: false, implicit_grab: true, goal_geometry: false, layout_events: false } });
    (Os::new(Arc::new(FakeDesktop(f.clone())), Box::new(FakeStage(f.clone()))), FakeDriver(f))
}

#[test]
fn contract_fake_with_target_events() {
    let (os, mut d) = fake(true);
    let passed = contract::block_on(contract::run(&os, &mut d)).unwrap();
    assert_eq!(passed.len(), 11);
}

#[test]
fn contract_fake_polled_target() {
    let (os, mut d) = fake(false);
    contract::block_on(contract::run(&os, &mut d)).unwrap();
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
