//! The HUD's motion: the speech bubble, the status tag and the pill move as one designed
//! system. One curve (`ease`: out, 2 % past the target at most, no wobble) and one set of
//! durations for every size change, open and close.
//!
//! Text is never glitched. Words fade out before their box moves, and come back clean (a
//! short fade) once it has settled. The glitch (bands shifted inside the box, an RGB split
//! on its edges, a red tear: her teleport's language) touches only the empty shell, for two
//! frames at most, and never leaves the box's bounds. A tag label holds at least `DWELL_MS`;
//! a burst of changes shows only the latest. Pure: the view samples it while something moves
//! and asks for frames only then.

use crate::anim::Rng;
use std::time::Instant;

/// One glitch frame (two presents at 60 Hz: one per present reads as noise).
pub const STEP_MS: f32 = 1000.0 / 30.0;
/// A glitch: two frames at most.
pub const GLITCH_MS: f32 = 2.0 * STEP_MS;
/// A size change: the bubble growing or shrinking to its content, the tag's width.
pub const SIZE_MS: f32 = 220.0;
/// The bubble's shell opening: from `OPEN_SCALE` at her tail.
pub const OPEN_MS: f32 = 260.0;
/// Words fading out before their box moves (a tag's swap, the bubble into the pill).
pub const TEXT_OUT_MS: f32 = 90.0;
/// The bubble closing: its words are gone by then.
pub const CLOSE_TEXT_MS: f32 = 50.0;
/// Words coming back once their box has settled.
pub const TEXT_IN_MS: f32 = 80.0;
/// The emptied bubble shrinking to `CLOSE_SCALE` before its exit glitch.
pub const SHRINK_MS: f32 = 140.0;
/// The emptied bubble easing into the pill.
pub const PILL_MS: f32 = 180.0;
/// Words come in once a size ease is this far (past its overshoot's rise).
pub const SETTLED: f32 = 0.7;
/// A tag label shows at least this long; changes meanwhile wait, and only the latest shows.
pub const DWELL_MS: f32 = 350.0;
/// At most one glitch per element this often.
pub const GAP_MS: f32 = 600.0;
pub const OPEN_SCALE: f32 = 0.85;
pub const CLOSE_SCALE: f32 = 0.9;
/// The widest sideways shift of a shell band: the tag's, the bubble's (px).
pub const TAG_SHIFT: f32 = 6.0;
pub const PANEL_SHIFT: f32 = 8.0;
/// The curve's overshoot constant: 2.0 % past the target at 71 % of the way, then settled.
const BACK: f32 = 0.75;

fn ms(now: Instant, t0: Instant) -> f32 {
    now.saturating_duration_since(t0).as_secs_f32() * 1000.0
}

/// The one curve: ease-out with a slight overshoot (`BACK`), 0 at 0 and 1 at 1.
pub fn ease(u: f32) -> f32 {
    let s = u.clamp(0.0, 1.0) - 1.0;
    1.0 + (BACK + 1.0) * s * s * s + BACK * s * s
}

/// A fade's level `ms_` long from `t0`: 0 → 1.
fn fade_in(now: Instant, t0: Instant, ms_: f32) -> f32 {
    (ms(now, t0) / ms_).clamp(0.0, 1.0)
}

/// A value easing toward its target along `ease`.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    t0: Option<Instant>,
    ms: f32,
}

impl Tween {
    pub const fn still(v: f32) -> Tween {
        Tween { from: v, to: v, t0: None, ms: SIZE_MS }
    }

    pub fn start(from: f32, to: f32, now: Instant, ms: f32) -> Tween {
        Tween { from, to, t0: Some(now), ms }
    }

    pub fn at(&self, now: Instant) -> f32 {
        match self.t0 {
            Some(t0) if ms(now, t0) < self.ms => self.from + (self.to - self.from) * ease(ms(now, t0) / self.ms),
            _ => self.to,
        }
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.t0.is_some_and(|t0| ms(now, t0) < self.ms)
    }

    /// How far along, 0..1 (1: still).
    pub fn progress(&self, now: Instant) -> f32 {
        self.t0.map_or(1.0, |t0| (ms(now, t0) / self.ms).min(1.0))
    }

    /// Toward `to` from wherever it is now; no change when that is already where it goes.
    pub fn go(&mut self, to: f32, now: Instant, ms: f32) {
        if (to - self.to).abs() < 0.25 {
            return;
        }
        *self = Tween::start(self.at(now), to, now, ms);
    }

    pub fn snap(&mut self, v: f32) {
        *self = Tween::still(v);
    }
}

/// One shell band at one moment: its sideways shift and its RGB split (px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub dx: f32,
    pub split: f32,
}

/// A shell glitch frame: its bands and the band whose top edge carries the red tear.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bands: Vec<Band>,
    pub tear: Option<usize>,
}

/// The empty shell's glitch: two frames. Every band stays (the shell never thins); the first
/// frame shifts them fully with a 3 px split and a red tear, the second at 40 % with 2 px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glitch {
    pub t0: Instant,
    pub seed: u64,
}

impl Glitch {
    pub fn new(now: Instant, seed: u64) -> Glitch {
        Glitch { t0: now, seed: seed | 1 }
    }

    pub fn done(&self, now: Instant) -> bool {
        ms(now, self.t0) >= GLITCH_MS
    }

    /// `n` bands at `now` (None: over); `amp`: the widest shift, px.
    pub fn look(&self, n: usize, amp: f32, now: Instant) -> Option<Look> {
        if self.done(now) {
            return None;
        }
        let n = n.max(1);
        let step = (ms(now, self.t0) / STEP_MS) as u64;
        let (k, split) = if step == 0 { (1.0, 3.0) } else { (0.4, 2.0) };
        let mut r = Rng::new(self.seed ^ step.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let bands = (0..n)
            .map(|_| {
                let m = r.range(0.35, 1.0) * if r.pick(2) == 0 { -1.0 } else { 1.0 };
                // Some bands hold still: a tear, not a slide.
                let moved = r.pick(3) != 0;
                Band { dx: if moved { (amp * k * m).round() } else { 0.0 }, split: if moved { split } else { 0.0 } }
            })
            .collect();
        Some(Look { bands, tear: (step == 0 && n > 1).then(|| 1 + r.pick(n - 1)) })
    }
}

/// Band edges over `h` px: `n` bands, none thinner than `min` (fixed per glitch).
pub fn cuts(h: f32, n: usize, min: f32, seed: u64) -> Vec<f32> {
    let n = n.clamp(1, ((h / min.max(1.0)) as usize).max(1));
    let mut r = Rng::new(seed | 1);
    let mut w: Vec<f32> = (0..n).map(|_| r.range(0.6, 1.4)).collect();
    let sum: f32 = w.iter().sum();
    w.iter_mut().for_each(|x| *x *= h / sum);
    let mut out = vec![0.0];
    let mut y = 0.0;
    for x in &w[..n - 1] {
        y += x;
        out.push(y.round());
    }
    out.push(h);
    out
}

/// A fresh seed per glitch.
pub fn seed(salt: u64) -> u64 {
    (Rng::seeded().next() ^ salt.wrapping_mul(0x2545_F491_4F6C_DD1D)) | 1
}

/// Where a tag change is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Steady,
    /// The old words fading out (since `t0`); `glitch`: the shell glitches after.
    TextOut { t0: Instant, glitch: bool },
    /// The new content's shell: its glitch, its width settling; no words yet.
    Shell { t0: Instant, glitch: Option<Glitch> },
    /// Going: the empty shell's exit glitch, then nothing.
    Exit { glitch: Glitch },
    /// The new words fading in.
    TextIn { t0: Instant },
}

/// What a tag draws this frame: its content (None: nothing), its words' level (0..1), its
/// shell's glitch.
#[derive(Debug, PartialEq)]
pub struct TagFrame<'a, T> {
    pub content: Option<&'a T>,
    pub text: f32,
    pub glitch: Option<Glitch>,
}

/// The status tag (and the pill, and the unread chip, which are the tag): what it shows, its
/// changes, its width.
#[derive(Debug)]
pub struct TagFx<T> {
    /// What it draws.
    shown: Option<T>,
    /// The latest asked for.
    want: Option<T>,
    phase: Phase,
    /// When the shown words came in (the dwell).
    shown_at: Option<Instant>,
    last_glitch: Option<Instant>,
    width: Tween,
    /// Frames left where the width follows its layout at once (just appeared: the measure
    /// is a frame old).
    snap: u8,
    /// Its content just changed: the layout's width is last frame's.
    measuring: bool,
    salt: u64,
}

impl<T: Clone + PartialEq> Default for TagFx<T> {
    fn default() -> Self {
        TagFx { shown: None, want: None, phase: Phase::Steady, shown_at: None, last_glitch: None, width: Tween::still(0.0), snap: 0, measuring: false, salt: 0 }
    }
}

impl<T: Clone + PartialEq> TagFx<T> {
    fn glitch_now(&mut self, now: Instant, ok: bool) -> Option<Glitch> {
        if !ok || self.last_glitch.is_some_and(|t| ms(now, t) < GAP_MS) {
            return None;
        }
        self.last_glitch = Some(now);
        self.salt = self.salt.wrapping_add(1);
        Some(Glitch::new(now, seed(self.salt)))
    }

    fn switch(&mut self, now: Instant, glitch: bool) {
        let appear = self.shown.is_none();
        self.measuring = !appear;
        if appear {
            self.snap = 2;
        }
        self.shown = self.want.clone();
        let glitch = self.glitch_now(now, glitch);
        self.phase = Phase::Shell { t0: now, glitch };
    }

    /// This frame's wanted content (`None`: no tag). `glitch`: its shell may glitch (the
    /// setting on, alpha mode, not dragged, not torn by her own glitch). `quiet`: it comes or
    /// goes with motion of its own (the bubble taking over, a teleport, a swap's hold, his
    /// hand): at once.
    pub fn observe(&mut self, next: Option<T>, now: Instant, glitch: bool, quiet: bool) {
        if next == self.want && next == self.shown && self.phase == Phase::Steady {
            // The same to the eye (a level meter moved): drawn as it is now.
            self.shown = next.clone();
            self.want = next;
            return;
        }
        self.want = next;
        if quiet && self.want != self.shown {
            self.measuring = self.shown.is_some() && self.want.is_some();
            if self.shown.is_none() {
                self.snap = 2;
            }
            self.shown = self.want.clone();
            self.phase = Phase::Steady;
            self.shown_at = Some(now);
            return;
        }
        // Step through what is due (a frame can cross more than one edge).
        for _ in 0..4 {
            match self.phase {
                Phase::Steady => {
                    if self.want == self.shown {
                        if let (Some(w), Some(s)) = (&self.want, &mut self.shown) {
                            // Equal to the eye: the newest look (a meter level) still shows.
                            *s = w.clone();
                        }
                        return;
                    }
                    if self.shown.is_none() {
                        self.switch(now, glitch);
                    } else if self.shown_at.is_some_and(|t| ms(now, t) < DWELL_MS) {
                        return;
                    } else {
                        self.phase = Phase::TextOut { t0: now, glitch };
                    }
                }
                Phase::TextOut { t0, glitch: g } => {
                    if ms(now, t0) < TEXT_OUT_MS {
                        return;
                    }
                    if self.want.is_some() {
                        self.switch(now, g && glitch);
                    } else {
                        match self.glitch_now(now, g && glitch) {
                            Some(x) => self.phase = Phase::Exit { glitch: x },
                            None => {
                                self.shown = None;
                                self.phase = Phase::Steady;
                            }
                        }
                    }
                }
                Phase::Exit { glitch: g } => {
                    if !g.done(now) {
                        return;
                    }
                    self.shown = None;
                    self.phase = Phase::Steady;
                }
                Phase::Shell { t0, glitch: g } => {
                    let settled = !self.measuring && self.snap == 0 && (!self.width.moving(now) || self.width.progress(now) >= SETTLED);
                    if g.is_some_and(|g| !g.done(now)) || ms(now, t0) < STEP_MS || !settled {
                        return;
                    }
                    self.phase = Phase::TextIn { t0: now };
                }
                Phase::TextIn { t0 } => {
                    if ms(now, t0) < TEXT_IN_MS {
                        return;
                    }
                    self.phase = Phase::Steady;
                    self.shown_at = Some(t0);
                }
            }
        }
    }

    /// What to draw now.
    pub fn frame(&self, now: Instant) -> TagFrame<'_, T> {
        let (text, glitch) = match self.phase {
            Phase::Steady => (1.0, None),
            Phase::TextOut { t0, .. } => (1.0 - fade_in(now, t0, TEXT_OUT_MS), None),
            Phase::Shell { glitch, .. } => (0.0, glitch.filter(|g| !g.done(now))),
            Phase::Exit { glitch } => (0.0, Some(glitch).filter(|g| !g.done(now))),
            Phase::TextIn { t0 } => (fade_in(now, t0, TEXT_IN_MS), None),
        };
        TagFrame { content: self.shown.as_ref(), text, glitch }
    }

    /// The width to draw, given the laid-out one (`motion` off: always that).
    pub fn width(&mut self, laid_out: f32, now: Instant, motion: bool) -> f32 {
        if std::mem::take(&mut self.measuring) && motion {
            // Last frame's layout: the old content's width; the new one is measured this frame.
            return self.width.at(now);
        }
        if self.snap > 0 || !motion {
            self.snap = self.snap.saturating_sub(1);
            self.width.snap(laid_out);
        } else {
            self.width.go(laid_out, now, SIZE_MS);
        }
        self.width.at(now)
    }

    /// Its content just changed and its new width isn't measured yet: draw it at the width
    /// it has (`width`), not as laid out.
    pub fn measuring(&self) -> bool {
        self.measuring
    }

    pub fn width_now(&self, now: Instant) -> f32 {
        self.width.at(now)
    }

    /// Something moves: a frame per present.
    pub fn busy(&self, now: Instant) -> bool {
        self.phase != Phase::Steady || self.width.moving(now)
    }

    /// When a waiting change is due (its dwell over).
    pub fn wake_at(&self) -> Option<Instant> {
        (self.phase == Phase::Steady && self.want != self.shown).then(|| self.shown_at.map(|t| t + std::time::Duration::from_secs_f32(DWELL_MS / 1000.0))).flatten()
    }
}

/// When the live words show.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Words {
    Shown,
    /// Hidden until the shell settles.
    Wait { t0: Instant },
    In { t0: Instant },
}

/// The bubble while it is open: its drawn size, its shell's glitch, its words' level.
#[derive(Debug)]
pub struct PanelFx {
    open: bool,
    serial: u64,
    pub w: Tween,
    pub h: Tween,
    glitch: Option<Glitch>,
    words: Words,
    last_glitch: Option<Instant>,
    salt: u64,
}

impl Default for PanelFx {
    fn default() -> Self {
        PanelFx { open: false, serial: 0, w: Tween::still(0.0), h: Tween::still(0.0), glitch: None, words: Words::Shown, last_glitch: None, salt: 0 }
    }
}

impl PanelFx {
    fn glitch_now(&mut self, now: Instant, ok: bool) {
        if ok && self.last_glitch.is_none_or(|t| ms(now, t) >= GAP_MS) {
            self.last_glitch = Some(now);
            self.salt = self.salt.wrapping_add(1);
            self.glitch = Some(Glitch::new(now, seed(self.salt)));
        }
    }

    /// Open this frame at the laid-out `w` × `h`, showing reply `serial`; `from`: the size it
    /// grows from when it just opened (the pill's), else `OPEN_SCALE` of its own. A new
    /// reply blanks the words, glitches the shell and lets the words back once it settles.
    /// `motion` off: drawn as laid out, words at once. Returns the size to draw.
    pub fn open(&mut self, (w, h): (f32, f32), serial: u64, from: Option<(f32, f32)>, now: Instant, motion: bool, glitch: bool) -> (f32, f32) {
        if !glitch || self.glitch.is_some_and(|g| g.done(now)) {
            self.glitch = None;
        }
        if !self.open {
            self.open = true;
            self.serial = serial;
            let (w0, h0) = from.unwrap_or((w * OPEN_SCALE, h * OPEN_SCALE));
            self.w = Tween::start(w0, w, now, OPEN_MS);
            self.h = Tween::start(h0, h, now, OPEN_MS);
            self.words = Words::Wait { t0: now };
            self.glitch_now(now, glitch);
        } else {
            if serial != self.serial {
                self.serial = serial;
                self.words = Words::Wait { t0: now };
                self.glitch_now(now, glitch);
            }
            self.w.go(w, now, SIZE_MS);
            self.h.go(h, now, SIZE_MS);
        }
        if !motion {
            self.w.snap(w);
            self.h.snap(h);
            self.words = Words::Shown;
        }
        self.words = match self.words {
            Words::Wait { t0 } => {
                let settled = self.w.progress(now).min(self.h.progress(now)) >= SETTLED;
                if settled && self.glitch.is_none_or(|g| g.done(now)) && ms(now, t0) >= STEP_MS { Words::In { t0: now } } else { Words::Wait { t0 } }
            }
            Words::In { t0 } if ms(now, t0) >= TEXT_IN_MS => Words::Shown,
            x => x,
        };
        (self.w.at(now), self.h.at(now))
    }

    /// The live words' level now (0: hidden while the shell moves).
    pub fn words(&self, now: Instant) -> f32 {
        match self.words {
            Words::Shown => 1.0,
            Words::Wait { .. } => 0.0,
            Words::In { t0 } => fade_in(now, t0, TEXT_IN_MS),
        }
    }

    /// Not open this frame. True when it just closed (the caller may show it leaving).
    pub fn closed(&mut self) -> bool {
        self.glitch = None;
        self.words = Words::Shown;
        std::mem::replace(&mut self.open, false)
    }

    pub fn glitch(&self, now: Instant) -> Option<Glitch> {
        self.glitch.filter(|g| !g.done(now))
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.w.moving(now) || self.h.moving(now)
    }

    pub fn busy(&self, now: Instant) -> bool {
        self.moving(now) || self.glitch(now).is_some() || self.words != Words::Shown
    }

    pub fn throttled(&self, now: Instant) -> bool {
        self.last_glitch.is_some_and(|t| ms(now, t) < GAP_MS)
    }
}

/// A box: x, y, w, h.
pub type Rect = (f32, f32, f32, f32);

/// A bubble that just closed, drawn from what it last showed while it leaves: its words fade
/// first, then the empty shell shrinks (to `CLOSE_SCALE` about its tail, then a two-frame exit
/// glitch) or eases into the pill's box.
#[derive(Debug)]
pub struct Ghost<S> {
    pub snap: S,
    pub t0: Instant,
    pub from: Rect,
    pub to: Rect,
    /// The pill's box: it eases there (else it shrinks about `pivot`).
    pub to_pill: bool,
    exit: Option<Glitch>,
}

impl<S> Ghost<S> {
    /// From `from`; into `pill` if given, else shrinking about `pivot` (the tail's tip).
    pub fn new(snap: S, from: Rect, pill: Option<Rect>, pivot: (f32, f32), now: Instant, glitch: bool, salt: u64) -> Ghost<S> {
        let s = CLOSE_SCALE;
        let to = pill.unwrap_or((pivot.0 - (pivot.0 - from.0) * s, pivot.1 - (pivot.1 - from.1) * s, from.2 * s, from.3 * s));
        let words = if pill.is_some() { TEXT_OUT_MS } else { CLOSE_TEXT_MS };
        let move_ms = if pill.is_some() { PILL_MS } else { SHRINK_MS };
        let at = now + std::time::Duration::from_secs_f32((words + move_ms) / 1000.0);
        let exit = (glitch && pill.is_none()).then(|| Glitch::new(at, seed(salt)));
        Ghost { snap, t0: now, from, to, to_pill: pill.is_some(), exit }
    }

    fn words_ms(&self) -> f32 {
        if self.to_pill { TEXT_OUT_MS } else { CLOSE_TEXT_MS }
    }

    fn move_ms(&self) -> f32 {
        if self.to_pill { PILL_MS } else { SHRINK_MS }
    }

    /// Its words' level (they fade first).
    pub fn words(&self, now: Instant) -> f32 {
        1.0 - fade_in(now, self.t0, self.words_ms())
    }

    /// How far the shell has moved, 0..1 (eased).
    pub fn k(&self, now: Instant) -> f32 {
        let t = ms(now, self.t0) - self.words_ms();
        if t <= 0.0 { 0.0 } else { ease(t / self.move_ms()) }
    }

    pub fn rect(&self, now: Instant) -> Rect {
        let k = self.k(now);
        let l = |a: f32, b: f32| a + (b - a) * k;
        (l(self.from.0, self.to.0), l(self.from.1, self.to.1), l(self.from.2, self.to.2), l(self.from.3, self.to.3))
    }

    /// The shell's exit glitch, while it runs.
    pub fn glitch(&self, now: Instant) -> Option<Glitch> {
        self.exit.filter(|g| now >= g.t0 && !g.done(now))
    }

    fn moved(&self, now: Instant) -> bool {
        ms(now, self.t0) >= self.words_ms() + self.move_ms()
    }

    pub fn done(&self, now: Instant) -> bool {
        self.moved(now) && self.exit.is_none_or(|g| g.done(now))
    }

    /// The tag (or the pill) may show: the shell is in the pill's box, or gone.
    pub fn tag_ready(&self, now: Instant) -> bool {
        if self.to_pill { self.moved(now) } else { self.done(now) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn the_curve_lands_and_overshoots_two_percent_at_most() {
        assert_eq!(ease(0.0), 0.0);
        assert!((ease(1.0) - 1.0).abs() < 1e-6);
        let peak = (0..=1000).map(|i| ease(i as f32 / 1000.0)).fold(0.0f32, f32::max);
        assert!(peak > 1.015 && peak < 1.021, "peak {peak}");
        let v: Vec<f32> = (0..=100).map(|i| ease(i as f32 / 100.0)).collect();
        let top = v.iter().enumerate().fold((0, 0.0f32), |a, (i, &x)| if x > a.1 { (i, x) } else { a }).0;
        assert!(v[..=top].windows(2).all(|w| w[1] >= w[0]) && v[top..].windows(2).all(|w| w[1] <= w[0]), "one bump, no wobble");
        assert!(ease(0.2) > 0.5, "fast out");
    }

    #[test]
    fn a_tween_retargets_from_where_it_is() {
        let t0 = Instant::now();
        let mut w = Tween::still(100.0);
        w.go(200.0, t0, SIZE_MS);
        assert_eq!(w.at(t0), 100.0);
        let mid = w.at(at(t0, 100));
        assert!(mid > 150.0 && mid < 200.0);
        w.go(120.0, at(t0, 100), SIZE_MS);
        assert!((w.at(at(t0, 100)) - mid).abs() < 0.01, "no jump on a retarget");
        assert_eq!(w.at(at(t0, 400)), 120.0);
        assert!(!w.moving(at(t0, 400)) && w.progress(at(t0, 400)) == 1.0);
        w.go(120.1, at(t0, 500), SIZE_MS);
        assert!(!w.moving(at(t0, 500)));
    }

    #[test]
    fn a_glitch_is_two_frames_of_whole_bands() {
        let t0 = Instant::now();
        let g = Glitch::new(t0, 42);
        let a = g.look(5, 10.0, t0).unwrap();
        assert_eq!(a.bands.len(), 5, "every band stays: the shell never thins");
        assert!(a.tear.is_some() && a.bands.iter().all(|b| b.dx.abs() <= 10.0 && b.split <= 3.0));
        let b = g.look(5, 10.0, at(t0, 40)).unwrap();
        assert!(b.tear.is_none() && b.bands.iter().all(|b| b.dx.abs() <= 4.0));
        assert_eq!(g.look(5, 10.0, at(t0, 30)), Some(a), "a frame holds a step");
        assert!(g.look(5, 10.0, at(t0, 67)).is_none() && g.done(at(t0, 67)), "two frames at most");
    }

    #[test]
    fn cuts_cover_the_box() {
        for seed in 0..20 {
            let c = cuts(25.0, 4, 6.0, seed);
            assert_eq!((c.first(), c.last(), c.len()), (Some(&0.0), Some(&25.0), 5));
            assert!(c.windows(2).all(|w| w[1] > w[0]));
        }
        assert_eq!(cuts(10.0, 5, 6.0, 1).len(), 2, "never thinner than min");
    }

    /// Steps a tag through `ms` at 60 Hz (its width as laid out: `w` px per char).
    fn run<'a>(t: &mut TagFx<&'a str>, from: Instant, to_ms: u64, want: Option<&'a str>, glitch: bool) -> Vec<(u64, Option<&'a str>, f32, bool)> {
        let mut out = Vec::new();
        let mut ms_ = 0;
        while ms_ <= to_ms {
            let now = at(from, ms_);
            t.observe(want, now, glitch, false);
            let laid = t.frame(now).content.map_or(0.0, |c| c.len() as f32 * 10.0);
            t.width(laid, now, true);
            let f = t.frame(now);
            out.push((ms_, f.content.copied(), f.text, f.glitch.is_some()));
            ms_ += 16;
        }
        out
    }

    #[test]
    fn a_swap_fades_the_words_glitches_the_empty_shell_then_brings_the_words_back() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        let a = run(&mut t, t0, 400, Some("ONLINE"), true);
        assert!(a.iter().filter(|f| f.3).all(|f| f.2 == 0.0), "no words while the shell glitches");
        assert!(a.iter().filter(|f| f.3).count() <= 5, "two glitch frames (~67 ms)");
        assert_eq!(a.last().unwrap().2, 1.0);
        let t1 = at(t0, 2000);
        let s = run(&mut t, t1, 600, Some("WORKING · bash"), true);
        // Out: the old words fade on a still shell; then the shell glitches empty; then the new
        // words come back once its width has (nearly) settled.
        let first_new = s.iter().position(|f| f.1 == Some("WORKING · bash")).unwrap();
        assert!(s[..first_new].iter().all(|f| f.1 == Some("ONLINE") && !f.3));
        assert!(s[first_new - 1].2 < 0.15, "the old words are (nearly) gone before the switch");
        assert!(s[first_new].2 == 0.0 && s[first_new].3, "the new shell glitches empty");
        let back = s.iter().position(|f| f.1 == Some("WORKING · bash") && f.2 > 0.0).unwrap();
        assert!(back * 16 >= 90 + 67, "words back after the shell: {} ms", back * 16);
        assert!(s.iter().filter(|f| f.3).all(|f| f.2 == 0.0));
        assert_eq!(s.last().unwrap().2, 1.0);
    }

    #[test]
    fn a_burst_dwells_and_shows_only_the_latest() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        run(&mut t, t0, 400, Some("ONLINE"), true);
        // Seven changes 250 ms apart: each label holds >= DWELL_MS once in, the ones in between
        // are skipped, the last one lands.
        let labels = ["THINKING", "WORKING", "THINKING", "WORKING", "THINKING", "WORKING", "DONE"];
        let t1 = at(t0, 2000);
        let mut seen: Vec<(u64, &str)> = Vec::new();
        for ms_ in (0..3000).step_by(16) {
            let want = labels[((ms_ / 250) as usize).min(labels.len() - 1)];
            let now = at(t1, ms_);
            t.observe(Some(want), now, true, false);
            let laid = t.frame(now).content.map_or(0.0, |c| c.len() as f32 * 10.0);
            t.width(laid, now, true);
            let f = t.frame(now);
            if f.text >= 1.0 && seen.last().is_none_or(|s| s.1 != *f.content.unwrap()) {
                seen.push((ms_, f.content.unwrap()));
            }
        }
        assert_eq!(seen.last().unwrap().1, "DONE");
        assert!(seen.len() <= 4, "skipped the ones in between: {seen:?}");
        assert!(seen[1..].windows(2).all(|w| w[1].0 - w[0].0 >= DWELL_MS as u64), "each held its dwell: {seen:?}");
    }

    #[test]
    fn it_goes_in_place_or_quietly() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        run(&mut t, t0, 400, Some("DONE"), true);
        let g = run(&mut t, at(t0, 2000), 400, None, true);
        assert!(g.iter().take_while(|f| f.1.is_some()).filter(|f| f.3).all(|f| f.2 == 0.0));
        assert_eq!(g.last().unwrap().1, None);
        assert!(g.iter().position(|f| f.1.is_none()).unwrap() * 16 <= 90 + 67 + 32);
        // Quiet (the bubble took over): at once, both ways.
        t.observe(Some("ONLINE"), at(t0, 5000), true, true);
        assert_eq!(t.frame(at(t0, 5000)), TagFrame { content: Some(&"ONLINE"), text: 1.0, glitch: None });
        t.observe(None, at(t0, 5016), true, true);
        assert_eq!(t.frame(at(t0, 5016)).content, None);
        // Glitch off: the words still fade, the shell never glitches.
        let n = run(&mut t, at(t0, 7000), 500, Some("THINKING"), false);
        assert!(n.iter().all(|f| !f.3) && n.last().unwrap().2 == 1.0);
        // A dwell wait asks to be woken.
        let t2 = at(t0, 7600);
        t.observe(Some("WORKING"), t2, false, false);
        assert!(t.wake_at().is_none() || t.wake_at().unwrap() > t2 || t.busy(t2));
    }

    #[test]
    fn the_tag_width_eases_but_snaps_when_it_appears() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        t.observe(Some("ONLINE"), t0, false, false);
        assert_eq!(t.width(80.0, t0, true), 80.0);
        assert_eq!(t.width(90.0, at(t0, 16), true), 90.0, "two frames follow the layout");
        assert_eq!(t.width(60.0, at(t0, 500), false), 60.0, "motion off: as laid out");
    }

    #[test]
    fn the_panel_opens_settles_then_shows_its_words() {
        let t0 = Instant::now();
        let mut p = PanelFx::default();
        let (w, h) = p.open((400.0, 200.0), 1, None, t0, true, true);
        assert_eq!((w, h), (400.0 * OPEN_SCALE, 200.0 * OPEN_SCALE));
        assert!(p.glitch(t0).is_some() && p.words(t0) == 0.0, "the empty shell glitches");
        assert!(p.glitch(at(t0, 70)).is_none(), "two frames");
        let mut first_words = None;
        for ms_ in (16..400).step_by(16) {
            p.open((400.0, 200.0), 1, None, at(t0, ms_), true, true);
            if first_words.is_none() && p.words(at(t0, ms_)) > 0.0 {
                first_words = Some(ms_);
            }
        }
        let fw = first_words.unwrap();
        assert!(fw as f32 >= OPEN_MS * SETTLED, "words after the shell settles: {fw}");
        assert_eq!(p.words(at(t0, 400)), 1.0);
        // Taller content: it grows, the words stay.
        let (_, h) = p.open((400.0, 300.0), 1, None, at(t0, 1000), true, true);
        assert_eq!(h, 200.0);
        assert_eq!(p.words(at(t0, 1000)), 1.0);
        // A new reply: the words blank, the shell glitches, they come back.
        p.open((400.0, 300.0), 2, None, at(t0, 2000), true, true);
        assert!(p.words(at(t0, 2000)) == 0.0 && p.glitch(at(t0, 2000)).is_some());
        for ms_ in (2016..2400).step_by(16) {
            p.open((400.0, 300.0), 2, None, at(t0, ms_), true, true);
        }
        assert_eq!(p.words(at(t0, 2400)), 1.0);
        assert!(p.closed() && !p.closed());
        // From the pill; motion off: as laid out, words at once.
        assert_eq!(p.open((400.0, 200.0), 2, Some((120.0, 28.0)), at(t0, 3000), true, false), (120.0, 28.0));
        p.closed();
        assert_eq!(p.open((400.0, 200.0), 3, None, at(t0, 4000), false, false), (400.0, 200.0));
        assert_eq!(p.words(at(t0, 4000)), 1.0);
    }

    #[test]
    fn a_ghost_fades_its_words_then_moves_its_shell() {
        let t0 = Instant::now();
        let g = Ghost::new((), (0.0, 0.0, 400.0, 200.0), Some((140.0, 230.0, 120.0, 25.0)), (200.0, 216.0), t0, true, 1);
        assert_eq!(g.rect(at(t0, 80)), (0.0, 0.0, 400.0, 200.0), "the shell holds while the words fade");
        assert!(g.words(at(t0, 90)) <= 0.01);
        assert!(!g.tag_ready(at(t0, 200)) && g.tag_ready(at(t0, 271)));
        let r = g.rect(at(t0, 271));
        assert!((r.2 - 120.0).abs() < 0.01 && (r.1 - 230.0).abs() < 0.01);
        assert!(g.glitch(at(t0, 100)).is_none(), "into the pill: no exit glitch (the pill's own takes over)");
        let c = Ghost::new((), (0.0, 0.0, 400.0, 200.0), None, (200.0, 216.0), t0, true, 1);
        assert!(c.words(at(t0, 50)) <= 0.01, "closing: the words are gone by +50 ms");
        assert!(c.glitch(at(t0, 100)).is_none() && c.glitch(at(t0, 195)).is_some(), "the exit glitch after the shrink");
        assert!(!c.done(at(t0, 200)) && c.done(at(t0, 260)));
        let r = c.rect(at(t0, 191));
        assert!((r.2 - 400.0 * CLOSE_SCALE).abs() < 0.5 && (r.1 + r.3 - (216.0 - (216.0 - 200.0) * CLOSE_SCALE)).abs() < 0.5);
    }
}
