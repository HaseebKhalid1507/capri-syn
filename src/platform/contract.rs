//! The contract every backend passes, written once: the scenarios the core relies on (perch on
//! a window, the window moves / hides / comes back / closes, monitor switch and work areas,
//! fullscreen, the drag's pointer, her body's moves and the teleport's effect layers, shapes,
//! the keyboard, the pointer watcher). `fake.rs` runs it in every `cargo test` (with and
//! without target events); a real backend runs it on a test display (X11: `x11/testkit.rs`
//! on Xvfb). A backend is done when it passes this, not its own tests.

use super::{Os, PointerCfg, PointerEv, ShapeRect, WatchEvent, WinId, WinRect};
use anyhow::{Result, anyhow, ensure};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// What a scenario does to the world, and what it reads back from the backend's side.
pub trait Driver {
    /// A visible window at `r`: managed (a WM-placed client) or a popup.
    fn open(&mut self, r: WinRect, managed: bool) -> WinId;
    fn configure(&mut self, w: WinId, r: WinRect);
    fn hide(&mut self, w: WinId);
    fn show(&mut self, w: WinId);
    fn close(&mut self, w: WinId);
    fn fullscreen(&mut self, w: WinId, on: bool);
    /// The root as two monitors side by side (on), or back (off). False: can't.
    fn split_monitors(&mut self, on: bool) -> bool;
    /// A panel along the top of the first monitor, reserving `h` px.
    fn dock_top(&mut self, h: i32) -> WinId;
    fn warp(&mut self, x: i32, y: i32);
    /// Where her body is, as the display server has it.
    fn body_rect(&mut self) -> Option<WinRect>;
    /// Her visible effect layers.
    fn fx_rects(&mut self) -> Vec<WinRect>;
    /// Her body's input region.
    fn input_region(&mut self) -> Vec<ShapeRect>;
    /// Her body's visible region (None: all of the window).
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>>;
    /// Let the server catch up (and its events go out).
    fn settle(&mut self);
}

/// Every scenario; the first failure stops the run. Returns the names that passed.
pub fn run(os: &Os, d: &mut dyn Driver) -> Result<Vec<&'static str>> {
    let (tx, rx) = channel();
    let tx2 = tx.clone();
    os.spawn_watcher(move |e| {
        tx.send(e).ok();
    })?;
    let cfg = PointerCfg { near_px: 150.0, swipe_px: 120.0, swipe_speed: 1500.0, hover: Duration::from_millis(1500) };
    os.watch_pointer(cfg, move |p| {
        tx2.send(WatchEvent::Pointer(p)).ok();
    })?;
    type Scenario = fn(&Os, &mut dyn Driver, &Receiver<WatchEvent>) -> Result<()>;
    let all: [(&'static str, Scenario); 9] = [
        ("stack-and-seats", stack_and_seats),
        ("perch-follow", perch_follow),
        ("monitors-and-areas", monitors_and_areas),
        ("fullscreen", fullscreen),
        ("drag-pointer", drag_pointer),
        ("body-and-teleport-fx", body_and_fx),
        ("shapes", shapes),
        ("keyboard", keyboard),
        ("pointer-near", pointer_near),
    ];
    let mut passed = Vec::new();
    for (name, f) in all {
        drain(&rx);
        f(os, d, &rx).map_err(|e| anyhow!("{name}: {e:#}"))?;
        passed.push(name);
    }
    Ok(passed)
}

fn drain(rx: &Receiver<WatchEvent>) {
    while rx.try_recv().is_ok() {}
}

/// The first event matching `want` within `ms`; others are dropped.
fn wait(rx: &Receiver<WatchEvent>, ms: u64, want: impl Fn(&WatchEvent) -> bool) -> Option<WatchEvent> {
    let end = Instant::now() + Duration::from_millis(ms);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(e) if want(&e) => return Some(e),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

fn target_event(e: &WatchEvent) -> bool {
    matches!(e, WatchEvent::TargetMoved(_) | WatchEvent::TargetGone(_) | WatchEvent::TargetBack(_))
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> WinRect {
    WinRect::new(x, y, w, h)
}

fn same(a: &WinRect, b: &WinRect) -> bool {
    (a.x, a.y, a.w, a.h) == (b.x, b.y, b.w, b.h)
}

/// Bottom → top order, her own surfaces left out, the seat rule (big and managed).
fn stack_and_seats(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    os.fx_set(0, 0xff0000, (40, 40), (20, 20), &[(0, 0, 20, 20)])?;
    os.flush();
    let a = d.open(rect(100, 300, 500, 300), true);
    let b = d.open(rect(700, 300, 100, 40), true);
    let c = d.open(rect(200, 200, 300, 200), false);
    d.settle();
    let st = os.stack();
    let pos = |w: WinId| st.iter().position(|(r, _)| r.id == w);
    let (pa, pb, pc) = (pos(a).ok_or(anyhow!("A not listed"))?, pos(b).ok_or(anyhow!("B not listed"))?, pos(c).ok_or(anyhow!("popup not listed"))?);
    ensure!(pa < pb && pb < pc, "order bottom → top: A {pa} B {pb} popup {pc}");
    ensure!(same(&st[pa].0, &rect(100, 300, 500, 300)), "A's rect {:?}", st[pa].0);
    ensure!(st[pa].1 && !st[pb].1 && !st[pc].1, "seats: big managed yes, small no, popup no: {} {} {}", st[pa].1, st[pb].1, st[pc].1);
    ensure!(pos(os.body()).is_none(), "her body is in the stack");
    let fx = d.fx_rects();
    ensure!(!fx.is_empty() && st.iter().all(|(r, _)| fx.iter().all(|f| f.id != r.id)), "an effect layer is in the stack (or none shows)");
    os.fx_unmap(0);
    os.flush();
    for w in [a, b, c] {
        d.close(w);
    }
    d.settle();
    ensure!(d.fx_rects().is_empty(), "the effect layer stayed up");
    ensure!(os.stack().iter().all(|(r, _)| ![a, b, c].contains(&r.id)), "closed windows still listed");
    Ok(())
}

/// The perch target: moves, hides (the workspace left), comes back, closes. Other windows'
/// moves are not hers.
fn perch_follow(os: &Os, d: &mut dyn Driver, rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    let b = d.open(rect(800, 300, 400, 300), true);
    d.settle();
    os.set_target(a);
    d.settle();
    drain(rx);
    d.configure(b, rect(820, 320, 400, 300));
    d.settle();
    ensure!(wait(rx, 600, target_event).is_none(), "another window's move reached her");
    d.configure(a, rect(150, 350, 520, 310));
    let e = wait(rx, 1500, |e| matches!(e, WatchEvent::TargetMoved(r) if r.id == a && same(r, &rect(150, 350, 520, 310))));
    ensure!(e.is_some(), "no TargetMoved to the new rect");
    ensure!(os.toplevel_rect(a).is_some_and(|r| same(&r, &rect(150, 350, 520, 310))), "toplevel_rect after the move");
    d.hide(a);
    ensure!(wait(rx, 1500, |e| matches!(e, WatchEvent::TargetGone(w) if *w == a)).is_some(), "no TargetGone on hide");
    ensure!(os.toplevel_rect(a).is_none() && os.window_exists(a), "hidden: no rect, still exists");
    d.show(a);
    ensure!(wait(rx, 1500, |e| matches!(e, WatchEvent::TargetBack(w) if *w == a)).is_some(), "no TargetBack on show");
    d.close(a);
    ensure!(wait(rx, 1500, |e| matches!(e, WatchEvent::TargetGone(w) if *w == a)).is_some(), "no TargetGone on close");
    d.settle();
    ensure!(!os.window_exists(a), "closed but exists");
    os.set_target(0);
    d.close(b);
    d.settle();
    Ok(())
}

/// Two monitors: which one a point is on (or nearest), the primary, a panel's reserved strip.
fn monitors_and_areas(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let (rw, rh) = os.root_size();
    ensure!(d.split_monitors(true), "the driver can't make two monitors");
    {
        os.forget_layout();
        let ms = os.monitors();
        ensure!(ms.len() == 2, "two monitors, got {ms:?}");
        let half = rw / 2;
        let right = os.monitor_at(half + 10, 10);
        ensure!((right.x, right.w) == (half, rw - half), "monitor_at on the right half: {right:?}");
        let off = os.monitor_at(-500, rh / 2);
        ensure!(off.x == 0, "nearest monitor to a point off the left: {off:?}");
        ensure!(ms.iter().filter(|m| m.primary).count() <= 1, "more than one primary");
        d.split_monitors(false);
    }
    let dock = d.dock_top(30);
    d.settle();
    os.forget_layout();
    let ms = os.monitors();
    let first = &ms[0];
    let area = os.area_at(first.x + 10, first.y + 100);
    ensure!(area.y == first.y + 30 && area.h == first.h - 30, "work area under a 30 px panel: {area:?} of {first:?}");
    ensure!(os.primary_area().h <= first.h, "primary area");
    d.close(dock);
    d.settle();
    os.forget_layout();
    ensure!(os.area_at(first.x + 10, first.y + 100).y == first.y, "the panel's strip stayed reserved");
    Ok(())
}

fn fullscreen(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    d.settle();
    ensure!(!os.is_fullscreen(a), "fullscreen before asking");
    d.fullscreen(a, true);
    d.settle();
    ensure!(os.is_fullscreen(a), "not fullscreen after asking");
    d.fullscreen(a, false);
    d.settle();
    ensure!(!os.is_fullscreen(a), "still fullscreen");
    d.close(a);
    d.settle();
    Ok(())
}

/// A held drag reads the pointer from the poller; the layout stays frozen meanwhile.
fn drag_pointer(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    d.warp(300, 400);
    d.settle();
    ensure!(os.pointer()? == (300, 400), "pointer {:?}", os.pointer()?);
    os.freeze_layout(true);
    os.poll_pointer(true);
    d.warp(420, 380);
    d.settle();
    let end = Instant::now() + Duration::from_millis(500);
    while os.pointer_fast()? != (420, 380) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    ensure!(os.pointer_fast()? == (420, 380), "polled pointer {:?}", os.pointer_fast()?);
    os.poll_pointer(false);
    os.freeze_layout(false);
    ensure!(os.refresh_at(300, 400) >= 20.0, "refresh rate");
    Ok(())
}

/// Her body lands where she's put (drag release, teleport arrival); the bolt's layers show
/// and go, stacked as asked.
fn body_and_fx(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    os.move_to(250, 260)?;
    d.settle();
    let b = d.body_rect().ok_or(anyhow!("no body"))?;
    ensure!((b.x, b.y) == (250, 260), "body at {:?}", (b.x, b.y));
    os.fx_set(1, 0xff0000, (400, 100), (60, 300), &[(0, 0, 4, 300)])?;
    os.fx_set_below(2, 1, 0x660000, (400, 100), (60, 300), &[(0, 0, 8, 300)])?;
    os.fx_under(3, 0x111111, (250, 560), (100, 6), Some(&[(0, 0, 100, 3)]))?;
    os.flush();
    d.settle();
    let fx = d.fx_rects();
    ensure!(fx.len() == 3, "three layers up, got {fx:?}");
    ensure!(fx.iter().any(|r| (r.x, r.y, r.w, r.h) == (400, 100, 60, 300)), "the bolt layer's place: {fx:?}");
    os.fx_set(2, 0x660000, (0, 0), (1, 1), &[])?;
    os.flush();
    d.settle();
    ensure!(d.fx_rects().len() == 2, "an empty fx_set left its layer up");
    os.fx_hide();
    d.settle();
    ensure!(d.fx_rects().is_empty(), "fx_hide left layers up");
    Ok(())
}

/// Click-through: the input region is what she says, in either compositor mode.
fn shapes(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let input: Vec<ShapeRect> = vec![(10, 20, 30, 40), (50, 60, 10, 10)];
    for mode in [Some(false), Some(true), None] {
        os.force_composited(mode);
        os.take_shape_stats();
        os.set_shapes(Some(&input[..]), Some(&[(0, 0, 5, 5)]))?;
        d.settle();
        let mut got = d.input_region();
        got.sort();
        ensure!(got == input, "input region {got:?} (mode {mode:?})");
        let (sends, rects) = os.take_shape_stats();
        let shaped = !os.composited();
        ensure!((sends, rects) == if shaped { (1, 1) } else { (0, 0) }, "shape stats {sends}/{rects} (shaped {shaped})");
        let vis = d.visible_region();
        ensure!(vis == if shaped { Some(vec![(0, 0, 5, 5)]) } else { None }, "visible region {vis:?} (shaped {shaped})");
    }
    Ok(())
}

/// The keyboard to her, and back to the window that had it.
fn keyboard(os: &Os, d: &mut dyn Driver, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    d.settle();
    os.grab_keyboard(os.body())?;
    d.settle();
    ensure!(os.focused_window() == Some(os.body()), "focus {:?} after the grab", os.focused_window());
    os.release_keyboard(os.body(), Some(a))?;
    d.settle();
    ensure!(os.focused_window() == Some(a), "focus {:?} after the release", os.focused_window());
    ensure!(os.prev_focus() == Some(a), "prev_focus {:?}", os.prev_focus());
    d.close(a);
    d.settle();
    Ok(())
}

/// The pointer coming near her body reports once.
fn pointer_near(os: &Os, d: &mut dyn Driver, rx: &Receiver<WatchEvent>) -> Result<()> {
    let her = rect(600, 300, 100, 200);
    d.warp(1200, 900);
    d.settle();
    os.set_pointer_watch(Some(her), true);
    std::thread::sleep(Duration::from_millis(800));
    drain(rx);
    d.warp(560, 400);
    d.settle();
    let e = wait(rx, 2000, |e| matches!(e, WatchEvent::Pointer(PointerEv::Near)));
    os.set_pointer_watch(None, false);
    ensure!(e.is_some(), "no Near");
    ensure!(os.pointer_still_ms().is_some(), "no motion seen");
    Ok(())
}
