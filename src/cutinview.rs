//! Cut-ins on screen (`crate::cutin` decides where and when): the beat's still (`anim/mochi`,
//! local only) decoded at her scale off the main thread, cut to its plate, in a window of its
//! own beside her for `cutins.ms`; input passes through it. Then the window goes, and its pixels
//! with it.

use super::Avatar;
use crate::cutin::{self, Beat, Phase};
use crate::frames::{HIT_ALPHA, alpha_mask};
use crate::fx::{self, BodyPlan, Glitch, GlitchKind, GlitchStep};
use crate::platform::{Os, Role, WinId};
use crate::shape::ShapeRect;
use gpui_kit::*;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct CutInState {
    /// Asked for (from code without a context): decoded and opened at the next schedule from
    /// `due` on, once she's settled; given up `WAIT_MAX` after it was asked for.
    want: Option<(Beat, Instant, Instant)>,
    busy: bool,
    last: Option<Instant>,
    window: Option<(AnyWindowHandle, WeakEntity<CutInView>)>,
    /// A turn started then (thinking or working, from rest): `done` marks a long one.
    pub turn_since: Option<Instant>,
    /// The mood the open Line's face shows (pushed again when hers changes).
    pub line_mood: Option<crate::anim::Mood>,
    pub shown: u32,
}

/// The still, ready to show: its pixels (BGRA), its opaque rows, its size (device px).
struct Still {
    image: Arc<RenderImage>,
    rects: Arc<Vec<ShapeRect>>,
    size: (i32, i32),
}

/// The rim's width (logical px) and the gap to her (logical px).
const RIM: f32 = 2.0;
const GAP: f32 = 14.0;
/// NEEDS YOU waits this long (a summon to him may be on its way: its landing has its own), and
/// any beat at most this long for her to be still.
const NEEDS_YOU_WAIT: Duration = Duration::from_millis(600);
const WAIT_MAX: Duration = Duration::from_millis(3000);

impl CutInState {
    /// When the one asked for may open.
    pub fn due(&self) -> Option<Instant> {
        // Past due (she's moving): a look every 40 ms until she's still.
        self.want.map(|w| w.1.max(Instant::now() + Duration::from_millis(40)))
    }
}

impl Avatar {
    /// A cut-in for `beat`, if the pack has its still, she's on screen, and the last one was
    /// long enough ago.
    pub(super) fn cutin(&mut self, beat: Beat, now: Instant) {
        let Some(spec) = &self.pack.behaviour.cutins else { return };
        let Some(stem) = spec.beats.get(beat.key()) else { return };
        if !self.pack.anim.as_ref().is_some_and(|a| a.mochi.contains_key(stem)) {
            return;
        }
        let gap = Duration::from_millis(spec.gap_ms);
        let c = &self.cutins;
        // A summon's landing replaces a NEEDS YOU still waiting.
        let replaces = beat == Beat::Summon && c.want.is_some_and(|w| w.0 == Beat::NeedsYou);
        if c.busy || c.window.is_some() || (c.want.is_some() && !replaces) || c.last.is_some_and(|t| now.saturating_duration_since(t) < gap) {
            return;
        }
        let fullscreen = self.brain.as_ref().is_some_and(|h| h.fs != super::brainview::FsHide::Shown);
        if !self.loaded || self.hidden || self.quitting || self.parked.is_some() || self.intro_playing() || fullscreen || self.dnd_on() || self.reading() {
            return;
        }
        let wait = if beat == Beat::NeedsYou { NEEDS_YOU_WAIT } else { Duration::ZERO };
        self.cutins.want = Some((beat, now + wait, now));
    }

    /// The Line or the reader is open: he's reading (the Line's face shows her mood there).
    fn reading(&self) -> bool {
        self.line.is_some() || self.reader.is_some()
    }

    /// She is somewhere to stay a moment (not in flight, carried or walking).
    fn cutin_settled(&self) -> bool {
        self.teleport.is_none() && self.rag.is_none() && !self.hidden && !self.brain_walking() && !self.brain_in_flight()
    }

    /// From `schedule`: the one on screen goes when she moves off; the one asked for is decoded
    /// and opened once due and she's settled.
    pub(super) fn cutin_fetch(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        // The open Line's face follows her mood.
        let shown = self.line.as_ref().map(|_| self.mood);
        if shown != self.cutins.line_mood {
            self.cutins.line_mood = shown;
            if shown.is_some() && self.stills.as_ref().is_some_and(|s| !s.moods.is_empty()) {
                self.line_push(cx);
            }
        }
        if (!self.cutin_settled() || self.reading())
            && let Some((_, v)) = &self.cutins.window
        {
            v.update(cx, |c, cx| c.leave(cx)).ok();
        }
        let Some((beat, due, asked)) = self.cutins.want else { return };
        if now.saturating_duration_since(asked) > WAIT_MAX {
            self.cutins.want = None;
            return;
        }
        if self.reading() {
            self.cutins.want = None;
            return;
        }
        if now < due || !self.cutin_settled() {
            return;
        }
        self.cutins.want = None;
        let Some(spec) = self.pack.behaviour.cutins.clone() else { return };
        let Some((path, sha)) = spec.beats.get(beat.key()).and_then(|s| self.pack.anim.as_ref()?.mochi.get(s).cloned()) else { return };
        let sf = self.layout.sf;
        let h = (spec.height * sf).round().max(16.0) as u32;
        let rim = (RIM * sf).round().max(1.0) as u32;
        let rim_rgb = self.pack.palette().accent;
        self.cutins.busy = true;
        self.cutins.last = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            let r = crate::off_thread(move || {
                let mut img = crate::frames::still(&path, sha.as_deref(), h)?;
                cutin::frame(&mut img, rim, rim_rgb);
                let (w, h) = img.dimensions();
                let mut px = img.into_raw();
                let rects = alpha_mask(&px, w, h).rects();
                for p in px.as_chunks_mut::<4>().0.iter_mut() {
                    if p[3] < HIT_ALPHA {
                        *p = [0; 4];
                    }
                    p.swap(0, 2);
                }
                let rgba = image::RgbaImage::from_raw(w, h, px).ok_or_else(|| anyhow::anyhow!("cut-in size"))?;
                Ok(Still { image: Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])), rects: Arc::new(rects), size: (w as i32, h as i32) })
            })
            .await;
            this.update(cx, |v, cx| v.cutin_open(beat, spec.ms, r, cx)).ok();
        })
        .detach();
    }

    fn cutin_open(&mut self, beat: Beat, ms: u64, r: anyhow::Result<Still>, cx: &mut Context<Self>) {
        self.cutins.busy = false;
        let still = match r {
            Ok(s) => s,
            Err(e) => {
                elog!("synaps-avatar: cut-in {}: {e:#}", beat.key());
                return;
            }
        };
        // Gone off screen meanwhile, or he opened the Line: not now.
        if !self.loaded || self.hidden || self.quitting || self.parked.is_some() || self.reading() {
            return;
        }
        let sf = self.layout.sf;
        let body = self.her_body();
        let (gx, gy) = self.ground();
        let a = self.os.area_at(gx, gy - 1);
        let Some((x, y)) = cutin::place((body.x, body.y, body.w, body.h), (a.x, a.y, a.w, a.h), still.size, (GAP * sf).round() as i32, !self.panel_up()) else {
            return;
        };
        let (w, h) = still.size;
        let bounds = Bounds::new(point(px(x as f32 / sf), px(y as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf)));
        let options = self.os.window_options(Role::Intro, bounds, false);
        let me = cx.entity().downgrade();
        let os = self.os.clone();
        let seed = self.rng.next();
        let first = still.rects.clone();
        let mut view = None;
        let opened = cx.open_window(options, |window, cx| {
            let id = os.adopt(Role::Intro, window).unwrap_or(0);
            // Its shape before its first frame (mapped without one, it showed as a black box).
            if id != 0 {
                os.shape_of(id, &[], &first).ok();
            }
            let on_done: Done = Box::new(move |cx: &mut App| {
                me.update(cx, |a, cx| a.cutin_done(cx)).ok();
            });
            let v = cx.new(|_| CutInView { os: os.clone(), id, still, ms, seed, sf, t0: Instant::now(), renders: 0, sent: 0, step: 0, timer_for: None, on_done: Some(on_done) });
            view = Some(v.downgrade());
            v
        });
        match (opened, view) {
            (Ok(handle), Some(v)) => {
                self.cutins.window = Some((handle.into(), v));
                self.cutins.shown += 1;
                elog!("synaps-avatar: cut-in {} at {x},{y} ({w}x{h})", beat.key());
            }
            (Err(e), _) => elog!("synaps-avatar: cut-in {}: cannot open its window: {e:#}", beat.key()),
            _ => {}
        }
    }

    /// Its time is up: the window goes (its image with it), and the heap it was in.
    fn cutin_done(&mut self, cx: &mut Context<Self>) {
        if let Some((h, _)) = self.cutins.window.take() {
            h.update(cx, |_, window, _| window.remove_window()).ok();
        }
        crate::platform::trim_heap();
    }

    /// The mood changed: the beats that mark a change of mood (and the turn's clock).
    pub(super) fn cutin_mood(&mut self, prev: crate::anim::Mood, m: crate::anim::Mood, now: Instant) {
        use crate::anim::Mood;
        if m == prev {
            return;
        }
        let busy = |m: Mood| matches!(m, Mood::Thinking | Mood::Working | Mood::Talking);
        if busy(m) && !busy(prev) && prev != Mood::NeedsYou {
            self.cutins.turn_since = Some(now);
        }
        match m {
            Mood::NeedsYou => self.cutin(Beat::NeedsYou, now),
            Mood::Annoyed => self.cutin(Beat::Annoyed, now),
            Mood::Done => {
                let after = self.pack.behaviour.cutins.as_ref().map_or(u64::MAX, |c| c.done_after_ms);
                if self.cutins.turn_since.take().is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_millis(after)) {
                    self.cutin(Beat::Done, now);
                }
            }
            Mood::Idle | Mood::Sleepy | Mood::Away => self.cutins.turn_since = None,
            _ => {}
        }
    }

    /// A click got her laugh: now and then the laugh's cut-in too.
    pub(super) fn cutin_laugh(&mut self, now: Instant) {
        let p = self.pack.behaviour.cutins.as_ref().map_or(0.0, |c| c.laugh_chance);
        if self.rng.range(0.0, 1.0) < p {
            self.cutin(Beat::Laugh, now);
        }
    }
}

type Done = Box<dyn FnOnce(&mut App)>;

/// The cut-in's window: its still, from its own clock (`cutin::phase`).
struct CutInView {
    os: Rc<Os>,
    id: WinId,
    still: Still,
    ms: u64,
    seed: u64,
    sf: f32,
    t0: Instant,
    /// The clock starts again at the second render (the first uploads the still).
    renders: u32,
    sent: u64,
    step: u64,
    timer_for: Option<u64>,
    on_done: Option<Done>,
}

impl CutInView {
    /// She moved off: straight to its exit (from where it is).
    fn leave(&mut self, cx: &mut Context<Self>) {
        let t = Instant::now().saturating_duration_since(self.t0).as_millis() as u64;
        let out = self.ms.saturating_sub(cutin::OUT_MS);
        if t < out {
            self.ms = t + cutin::OUT_MS;
            self.timer_for = None;
            cx.notify();
        }
    }
}

fn hash_rects(r: &[ShapeRect]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    r.hash(&mut h);
    h.finish()
}

impl Render for CutInView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.renders < 2 {
            self.renders += 1;
            self.t0 = Instant::now();
            window.request_animation_frame();
        }
        let now = Instant::now();
        let t = now.saturating_duration_since(self.t0).as_millis() as u64;
        let ph = cutin::phase(t, self.ms);
        if ph == Phase::Gone {
            if let Some(f) = self.on_done.take() {
                cx.defer(f);
            }
            return div().size_full();
        }
        let (w, h) = self.still.size;
        let sf = self.sf;
        let mut plan = BodyPlan::plain();
        match ph {
            Phase::Red => plan.tint = Some(fx::BOLT_RED),
            Phase::Tear(_) => {
                let at = self.t0 + Duration::from_millis(cutin::RED_MS);
                let mut g = Glitch::new(GlitchKind::Burst, at, self.seed);
                g.ms = cutin::TEAR_MS as f32;
                self.step += 1;
                plan.glitch(GlitchStep {
                    strength: g.strength(now),
                    coverage: 1.0,
                    seed: g.step_seed(self.step),
                    order: g.seed(),
                    y0: 0,
                    h,
                    body: (0, 0, w, h),
                    face: None,
                    solid: 0.0,
                    tag: None,
                    sf,
                    calm: false,
                    light: false,
                    still_from: None,
                });
            }
            Phase::Out(u) => plan.slices = Some(crate::intro::out_bands(h, u, self.seed, (8.0 * sf).round() as i32)),
            Phase::Hold | Phase::Gone => {}
        }
        // Moving: every frame. Held: a wake-up when it starts to go.
        if ph == Phase::Hold {
            let next = self.ms.saturating_sub(cutin::OUT_MS);
            if self.timer_for != Some(next) {
                self.timer_for = Some(next);
                let wait = Duration::from_millis(next.saturating_sub(t) + 2);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }
        } else {
            window.request_animation_frame();
        }
        let rects = self.still.rects.clone();
        let alpha = self.os.composited();
        let shape = if plan.is_plain() && plan.tint.is_none() { rects.to_vec() } else { plan.shape(&rects) };
        let hsh = hash_rects(&shape);
        // Never an empty shape (GPUI stops drawing a fully obscured X window).
        if hsh != self.sent && !shape.is_empty() {
            self.sent = hsh;
            self.os.shape_of(self.id, &[], &shape).ok();
        }
        let ghost = plan.ghost_rects(&rects);
        let tint = plan.tint.map(|c| (c, plan.body_rects(&rects)));
        let bars = if plan.tint.is_some() { Vec::new() } else { plan.bar_rects(&rects) };
        let img = self.still.image.clone();
        let plan = Rc::new(plan);
        div().size_full().child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let (bx, by) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(bx + x / sf), px(by + y / sf)), size(px(w / sf), px(h / sf)));
                    let quad = |window: &mut Window, r: &ShapeRect, c: u32| {
                        window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(c)));
                    };
                    let full = rect(0.0, 0.0, w as f32, h as f32);
                    // The shape lands a frame late without a compositor: the still underneath,
                    // so where it grows there is the still, not black.
                    if !alpha && plan.slices.is_some() {
                        window.paint_image(full, full, Corners::default(), img.clone(), 0, false).ok();
                    }
                    for (r, c) in &ghost {
                        quad(window, r, *c);
                    }
                    let (ix, iy, iw, ih) = plan.image_rect(w, h);
                    match (&tint, &plan.slices) {
                        (Some((c, rs)), _) => {
                            for r in rs {
                                quad(window, r, *c);
                            }
                        }
                        (None, None) => {
                            let r = rect(ix, iy, iw, ih);
                            window.paint_image(r, r, Corners::default(), img.clone(), 0, false).ok();
                        }
                        (None, Some(bands)) => {
                            for &(y0, y1, dx, dy) in bands {
                                let shifted = rect(ix + dx as f32, iy + dy as f32, iw, ih);
                                let clip = rect(ix + dx as f32, (y0 + dy) as f32, iw, (y1 - y0) as f32);
                                window.paint_image(clip, shifted, Corners::default(), img.clone(), 0, false).ok();
                            }
                        }
                    }
                    for (r, c) in &bars {
                        quad(window, r, *c);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}
