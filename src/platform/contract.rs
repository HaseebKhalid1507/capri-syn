//! The contract every backend passes, written once: the scenarios the core relies on (perch on
//! a window, the window moves / hides / comes back / closes, monitor switch and work areas,
//! fullscreen, the drag's pointer, her body's moves and the teleport's effect layers, shapes,
//! the keyboard, the pointer watcher). `fake.rs` runs it in every `cargo test` (with and
//! without target events); a real backend runs it on a test display (X11: `x11/testkit.rs`
//! on Xvfb; Hyprland: `hypr/testkit.rs` in the `tools/wlrig` sandbox). A backend is done when it
//! passes this, not its own tests.
//!
//! What a scenario checks is what shows: where her body is, which pixels click through, the
//! extent of each effect layer as painted. `settle` is async: a backend that paints effects
//! in her own window (Wayland) needs its event loop to turn between steps.

use super::{Os, PointerCfg, PointerEv, ShapeRect, WatchEvent, WinId, WinRect};
use anyhow::{Result, anyhow, ensure};
use std::future::Future;
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
    /// Each visible effect layer's painted extent (the bounding box of its rects), root px.
    fn fx_rects(&mut self) -> Vec<WinRect>;
    /// A click at root (x, y) lands on her body (else it passes through).
    fn hits(&mut self, x: i32, y: i32) -> bool;
    /// Her body's visible region (None: all of the window).
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>>;
    /// Let the server catch up (its events go out, her pending surface state is committed,
    /// a frame is drawn).
    fn settle(&mut self) -> impl Future<Output = ()>;
}

/// Run a contract future to the end on a backend whose `settle` never waits on an event loop
/// (the fake, X11).
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Every scenario; the first failure stops the run. Returns the names that passed.
pub async fn run<D: Driver>(os: &Os, d: &mut D) -> Result<Vec<&'static str>> {
    let (tx, rx) = channel();
    let tx2 = tx.clone();
    os.spawn_watcher(move |e| {
        tx.send(e).ok();
    })?;
    let cfg = PointerCfg { near_px: 150.0, swipe_px: 120.0, swipe_speed: 1500.0, hover: Duration::from_millis(1500) };
    os.watch_pointer(cfg, move |p| {
        tx2.send(WatchEvent::Pointer(p)).ok();
    })?;
    let mut passed = Vec::new();
    macro_rules! scenario {
        ($name:literal, $f:ident) => {
            drain(&rx);
            $f(os, d, &rx).await.map_err(|e| anyhow!("{}: {e:#}", $name))?;
            passed.push($name);
        };
    }
    scenario!("stack-and-seats", stack_and_seats);
    scenario!("perch-follow", perch_follow);
    scenario!("monitors-and-areas", monitors_and_areas);
    scenario!("fullscreen", fullscreen);
    scenario!("drag-pointer", drag_pointer);
    scenario!("body-and-teleport-fx", body_and_fx);
    scenario!("shapes", shapes);
    scenario!("keyboard", keyboard);
    scenario!("pointer-near", pointer_near);
    Ok(passed)
}

/// The extent `(x, y, w, h)` of a painted layer, with the id dropped.
fn extent(r: &WinRect) -> (i32, i32, i32, i32) {
    (r.x, r.y, r.w, r.h)
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
async fn stack_and_seats<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    // An effect layer up (inside her window: some backends paint effects there).
    let b = d.body_rect().ok_or(anyhow!("no body"))?;
    os.fx_set(0, 0xff0000, (b.x + 10, b.y + 10), (20, 20), &[(0, 0, 20, 20)])?;
    os.flush();
    let a = d.open(rect(100, 300, 500, 300), true);
    let b = d.open(rect(700, 300, 100, 40), true);
    let c = d.open(rect(200, 200, 300, 200), false);
    d.settle().await;
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
    d.settle().await;
    let fx = d.fx_rects();
    ensure!(fx.is_empty(), "the effect layer stayed up: {fx:?}");
    ensure!(os.stack().iter().all(|(r, _)| ![a, b, c].contains(&r.id)), "closed windows still listed");
    Ok(())
}

/// The perch target: moves, hides (the workspace left), comes back, closes. Other windows'
/// moves are not hers.
async fn perch_follow<D: Driver>(os: &Os, d: &mut D, rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    let b = d.open(rect(800, 300, 400, 300), true);
    d.settle().await;
    os.set_target(a);
    d.settle().await;
    drain(rx);
    d.configure(b, rect(820, 320, 400, 300));
    d.settle().await;
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
    d.settle().await;
    ensure!(!os.window_exists(a), "closed but exists");
    os.set_target(0);
    d.close(b);
    d.settle().await;
    Ok(())
}

/// Two monitors: which one a point is on (or nearest), the primary, a panel's reserved strip.
async fn monitors_and_areas<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    ensure!(d.split_monitors(true), "the driver can't make two monitors");
    d.settle().await;
    {
        os.forget_layout();
        let mut ms = os.monitors();
        ensure!(ms.len() == 2, "two monitors, got {ms:?}");
        ms.sort_by_key(|m| m.x);
        let (left, right) = (&ms[0], &ms[1]);
        ensure!(right.x >= left.x + left.w, "side by side: {ms:?}");
        let r = os.monitor_at(right.x + 10, right.y + 10);
        ensure!(r == *right, "monitor_at on the right one: {r:?}");
        let off = os.monitor_at(left.x - 500, left.y + left.h / 2);
        ensure!(off == *left, "nearest monitor to a point off the left: {off:?}");
        ensure!(ms.iter().filter(|m| m.primary).count() <= 1, "more than one primary");
        d.split_monitors(false);
        d.settle().await;
    }
    let dock = d.dock_top(30);
    d.settle().await;
    os.forget_layout();
    let ms = os.monitors();
    let first = &ms[0];
    let area = os.area_at(first.x + 10, first.y + 100);
    ensure!(area.y == first.y + 30 && area.h == first.h - 30, "work area under a 30 px panel: {area:?} of {first:?}");
    ensure!(os.primary_area().h <= first.h, "primary area");
    d.close(dock);
    d.settle().await;
    os.forget_layout();
    ensure!(os.area_at(first.x + 10, first.y + 100).y == first.y, "the panel's strip stayed reserved");
    Ok(())
}

async fn fullscreen<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    d.settle().await;
    ensure!(!os.is_fullscreen(a), "fullscreen before asking");
    d.fullscreen(a, true);
    d.settle().await;
    ensure!(os.is_fullscreen(a), "not fullscreen after asking");
    d.fullscreen(a, false);
    d.settle().await;
    ensure!(!os.is_fullscreen(a), "still fullscreen");
    d.close(a);
    d.settle().await;
    Ok(())
}

/// A held drag reads the pointer from the poller; the layout stays frozen meanwhile.
async fn drag_pointer<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    d.warp(300, 400);
    d.settle().await;
    ensure!(os.pointer()? == (300, 400), "pointer {:?}", os.pointer()?);
    os.freeze_layout(true);
    os.poll_pointer(true);
    let seq = os.pointer_seq();
    d.warp(420, 380);
    d.settle().await;
    // The poller saw the move, and a frame after it (at rest past the resampler's reach)
    // puts her where the pointer is.
    let at = |os: &Os| os.pointer_at(Instant::now() + Duration::from_millis(40)).ok();
    let end = Instant::now() + Duration::from_millis(500);
    while (os.pointer_seq() == seq || at(os) != Some((420.0, 380.0))) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(5));
    }
    ensure!(at(os) == Some((420.0, 380.0)), "polled pointer {:?}", at(os));
    ensure!(os.pointer_seq() > seq, "the poller saw no change");
    os.poll_pointer(false);
    os.freeze_layout(false);
    ensure!(os.refresh_at(300, 400) >= 20.0, "refresh rate");
    Ok(())
}

/// Her body lands where she's put (drag release, teleport arrival); the bolt's layers show
/// and go, stacked as asked.
async fn body_and_fx<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    os.move_to(250, 260)?;
    d.settle().await;
    let b = d.body_rect().ok_or(anyhow!("no body"))?;
    ensure!((b.x, b.y) == (250, 260), "body at {:?}", (b.x, b.y));
    // The bolt's two layers (the glow stacked below the line) and a contact shadow under her,
    // where her window is (some backends paint effects inside it).
    let (ox, oy) = (b.x + 20, b.y + 20);
    os.fx_set(1, 0xff0000, (ox, oy), (60, 120), &[(0, 0, 4, 120)])?;
    os.fx_set_below(2, 1, 0x660000, (ox + 30, oy), (60, 120), &[(0, 0, 8, 120)])?;
    os.fx_under(3, 0x2266aa, (b.x + 10, b.y + b.h - 20), (100, 6), Some(&[(0, 0, 100, 3)]))?;
    os.flush();
    d.settle().await;
    let mut fx: Vec<_> = d.fx_rects().iter().map(extent).collect();
    fx.sort();
    let mut want = vec![(ox, oy, 4, 120), (ox + 30, oy, 8, 120), (b.x + 10, b.y + b.h - 20, 100, 3)];
    want.sort();
    ensure!(fx == want, "three layers painted at {fx:?}, want {want:?}");
    os.fx_set(2, 0x660000, (0, 0), (1, 1), &[])?;
    os.flush();
    d.settle().await;
    let fx = d.fx_rects();
    ensure!(fx.len() == 2, "an empty fx_set left its layer up: {fx:?}");
    // The shadow follows her with its rects kept (`None`).
    os.fx_under(3, 0x2266aa, (b.x + 30, b.y + b.h - 20), (100, 6), None)?;
    d.settle().await;
    ensure!(d.fx_rects().iter().any(|r| extent(r) == (b.x + 30, b.y + b.h - 20, 100, 3)), "the moved shadow: {:?}", d.fx_rects());
    os.fx_hide();
    d.settle().await;
    let fx = d.fx_rects();
    ensure!(fx.is_empty(), "fx_hide left layers up: {fx:?}");
    Ok(())
}

/// Click-through: a click lands on her where her input region says, and passes through
/// elsewhere, in either compositor mode.
async fn shapes<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let b = d.body_rect().ok_or(anyhow!("no body"))?;
    let input: Vec<ShapeRect> = vec![(10, 20, 30, 40), (50, 60, 10, 10)];
    let on = [(25, 40), (12, 22), (38, 58), (55, 65)];
    let off = [(5, 5), (45, 40), (25, 70), (65, 65), (100, 100)];
    for mode in [Some(false), Some(true), None] {
        os.force_composited(mode);
        os.take_shape_stats();
        os.set_shapes(Some(&input[..]), Some(&[(0, 0, 5, 5)]))?;
        d.settle().await;
        for (x, y) in on {
            ensure!(d.hits(b.x + x, b.y + y), "({x}, {y}) inside her input region passed through (mode {mode:?})");
        }
        for (x, y) in off {
            ensure!(!d.hits(b.x + x, b.y + y), "({x}, {y}) outside her input region landed on her (mode {mode:?})");
        }
        let (sends, rects) = os.take_shape_stats();
        let shaped = !os.composited();
        ensure!((sends, rects) == if shaped { (1, 1) } else { (0, 0) }, "shape stats {sends}/{rects} (shaped {shaped})");
        let vis = d.visible_region();
        ensure!(vis == if shaped { Some(vec![(0, 0, 5, 5)]) } else { None }, "visible region {vis:?} (shaped {shaped})");
    }
    // Nothing clickable: every click passes through.
    os.set_shapes(Some(&[]), None)?;
    d.settle().await;
    ensure!(!d.hits(b.x + 25, b.y + 40), "an empty input region still takes clicks");
    Ok(())
}

/// The keyboard to her, and back to the window that had it.
async fn keyboard<D: Driver>(os: &Os, d: &mut D, _rx: &Receiver<WatchEvent>) -> Result<()> {
    let a = d.open(rect(100, 300, 500, 300), true);
    d.settle().await;
    os.grab_keyboard(os.body())?;
    d.settle().await;
    ensure!(os.focused_window() == Some(os.body()), "focus {:?} after the grab", os.focused_window());
    os.release_keyboard(os.body(), Some(a))?;
    d.settle().await;
    ensure!(os.focused_window() == Some(a), "focus {:?} after the release", os.focused_window());
    ensure!(os.prev_focus() == Some(a), "prev_focus {:?}", os.prev_focus());
    d.close(a);
    d.settle().await;
    Ok(())
}

/// The pointer coming near her body reports once.
async fn pointer_near<D: Driver>(os: &Os, d: &mut D, rx: &Receiver<WatchEvent>) -> Result<()> {
    let her = rect(600, 300, 100, 200);
    d.warp(1200, 900);
    d.settle().await;
    os.set_pointer_watch(Some(her), true);
    std::thread::sleep(Duration::from_millis(800));
    drain(rx);
    d.warp(560, 400);
    d.settle().await;
    let e = wait(rx, 2000, |e| matches!(e, WatchEvent::Pointer(PointerEv::Near)));
    os.set_pointer_watch(None, false);
    ensure!(e.is_some(), "no Near");
    ensure!(os.pointer_still_ms().is_some(), "no motion seen");
    Ok(())
}
