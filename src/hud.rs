//! The HUD's motion: the speech bubble, the status tag and the pill move as one designed
//! system. One curve (`ease`: out, 2 % past the target at most, no wobble) and one set of
//! durations for every size change, open and close; the glitch (bands, an RGB split, a red
//! tear: her teleport's language) only accents the box events, never streaming text. Pure:
//! the view samples it while something moves and asks for frames only then.

use crate::anim::Rng;
use std::time::Instant;

/// A size change: the bubble growing or shrinking to its content, the tag's width, the pill.
pub const SIZE_MS: f32 = 240.0;
/// The bubble opening: from `OPEN_SCALE` at her tail, with the glitch-in.
pub const OPEN_MS: f32 = 260.0;
/// The bubble closing: the glitch-out and a shrink to `CLOSE_SCALE` (or into the pill).
pub const CLOSE_MS: f32 = 200.0;
/// The tag's text swap: the old text slices out, the new slices in.
pub const SWAP_MS: f32 = 220.0;
/// A new reply in an open bubble: a short tremor of two bands.
pub const PULSE_MS: f32 = 160.0;
/// At most one glitch per element this often, and only once its changes have been calm this
/// long: a burst of changes swaps quietly instead of strobing.
pub const GAP_MS: f32 = 600.0;
/// A glitch's pattern holds this long (two frames at 60 Hz: one per frame reads as noise).
pub const STEP_MS: f32 = 33.0;
pub const OPEN_SCALE: f32 = 0.85;
pub const CLOSE_SCALE: f32 = 0.92;
/// The widest sideways shift of a band: the tag's, the bubble's (px).
pub const TAG_SHIFT: f32 = 8.0;
pub const PANEL_SHIFT: f32 = 12.0;
/// The swap's cut from the old text to the new, as a fraction of `SWAP_MS`.
pub const SWAP_CUT: f32 = 0.4;
/// The pill shows once the closing bubble is this far into it.
pub const PILL_AT: f32 = 0.55;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Bands assemble: an appearance.
    In,
    /// Bands slice away: a disappearance.
    Out,
    /// The old content slices out, the new slices in (`SWAP_CUT`).
    Swap,
    /// Two bands tremble: new content in a box that stays.
    Pulse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glitch {
    pub kind: Kind,
    pub t0: Instant,
    pub ms: f32,
    pub seed: u64,
}

/// One band at one moment: drawn or not, its sideways shift and its RGB split (px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub on: bool,
    pub dx: f32,
    pub split: f32,
}

pub const CLEAN: Band = Band { on: true, dx: 0.0, split: 0.0 };

/// A glitch's frame: its bands, whether the old content shows, and the band whose top edge
/// carries the red tear (one step, at the cut).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bands: Vec<Band>,
    pub old: bool,
    pub tear: Option<usize>,
}

impl Glitch {
    pub fn new(kind: Kind, now: Instant, seed: u64) -> Glitch {
        let ms = match kind {
            Kind::In => OPEN_MS * 0.85,
            Kind::Out => CLOSE_MS,
            Kind::Swap => SWAP_MS,
            Kind::Pulse => PULSE_MS,
        };
        Glitch { kind, t0: now, ms, seed: seed | 1 }
    }

    /// Its step (a pattern holds `STEP_MS`) and where that step is, 0..1: a glitch stutters.
    fn step(&self, now: Instant) -> (u64, f32) {
        let step = (ms(now, self.t0) / STEP_MS) as u64;
        (step, (step as f32 * STEP_MS / self.ms).min(1.0))
    }

    /// The first step at or past `u`.
    fn step_at(&self, u: f32) -> u64 {
        (u * self.ms / STEP_MS).ceil() as u64
    }

    pub fn done(&self, now: Instant) -> bool {
        ms(now, self.t0) >= self.ms
    }

    /// Showing what was there before (a swap before its cut, a disappearance).
    pub fn old(&self, now: Instant) -> bool {
        match self.kind {
            Kind::Out => true,
            Kind::Swap => self.step(now).1 < SWAP_CUT,
            _ => false,
        }
    }

    /// `n` bands at `now`; `amp`: the widest shift, px.
    pub fn look(&self, n: usize, amp: f32, now: Instant) -> Look {
        let n = n.max(1);
        let (step, u) = self.step(now);
        // The order bands come and go in: fixed for the glitch. The shifts: new every step.
        let mut order: Vec<usize> = (0..n).collect();
        let mut o = Rng::new(self.seed);
        for i in (1..n).rev() {
            order.swap(i, o.pick(i + 1));
        }
        let mut rank = vec![0usize; n];
        for (r, &i) in order.iter().enumerate() {
            rank[i] = r;
        }
        let mut r = Rng::new(self.seed ^ step.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let jitter: Vec<(f32, bool)> = (0..n)
            .map(|_| {
                let m = r.range(0.35, 1.0) * if r.pick(2) == 0 { -1.0 } else { 1.0 };
                (m, r.pick(3) != 0)
            })
            .collect();
        let frac = |i: usize| rank[i] as f32 / n as f32;
        let assemble = |v: f32| -> Vec<Band> {
            // In at 10..55 % (rank order), shifts decaying to nothing by 85 %.
            (0..n)
                .map(|i| {
                    let on = v >= 0.1 + 0.45 * frac(i);
                    let k = if v >= 0.85 { 0.0 } else { (1.0 - v / 0.85).powi(2) };
                    let (m, split) = jitter[i];
                    Band { on, dx: amp * k * m, split: if split { (4.0 * k).round() } else { 0.0 } }
                })
                .collect()
        };
        let dissolve = |v: f32| -> Vec<Band> {
            // Out at 20..90 % (rank order), shifts growing.
            (0..n)
                .map(|i| {
                    let on = v < 0.9 - 0.7 * frac(i);
                    let k = 0.35 + 0.65 * v;
                    let (m, _) = jitter[i];
                    Band { on, dx: amp * k * m, split: (1.5 + 2.5 * v).round() }
                })
                .collect()
        };
        match self.kind {
            Kind::In => {
                let bands = assemble(u);
                // The tear: the first step anything shows.
                Look { bands, old: false, tear: (step == self.step_at(0.1) && n > 1).then(|| 1 + r.pick(n - 1)) }
            }
            Kind::Out => Look { bands: dissolve(u), old: true, tear: None },
            Kind::Swap => {
                let cut = self.step_at(SWAP_CUT);
                if u < SWAP_CUT {
                    Look { bands: dissolve(u / SWAP_CUT), old: true, tear: None }
                } else {
                    let v = (u - SWAP_CUT) / (1.0 - SWAP_CUT);
                    // The new text comes in fast: its first bands at once.
                    Look { bands: assemble(0.1 + 0.9 * v), old: false, tear: (step == cut && n > 1).then(|| 1 + r.pick(n - 1)) }
                }
            }
            Kind::Pulse => {
                let k = (std::f32::consts::PI * u).sin();
                let a = Rng::new(self.seed).pick(n);
                let b = (a + 1 + Rng::new(self.seed ^ 7).pick(n.max(2) - 1)) % n;
                let bands = (0..n)
                    .map(|i| {
                        if (i == a || i == b) && u < 0.85 {
                            Band { on: true, dx: amp * 0.6 * k * jitter[i].0, split: (3.0 * k).round() }
                        } else {
                            CLEAN
                        }
                    })
                    .collect();
                Look { bands, old: false, tear: None }
            }
        }
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

/// The status tag (and the pill, and the unread chip, which are the tag): what it shows, its
/// glitch, its width.
#[derive(Debug)]
pub struct TagFx<T> {
    shown: Option<T>,
    /// The content leaving (a swap before its cut, a disappearance).
    old: Option<T>,
    glitch: Option<Glitch>,
    last_glitch: Option<Instant>,
    last_change: Option<Instant>,
    width: Tween,
    /// Frames left where the width follows its layout at once (just appeared: the measure
    /// is a frame old).
    snap: u8,
    salt: u64,
}

impl<T: Clone + PartialEq> Default for TagFx<T> {
    fn default() -> Self {
        TagFx { shown: None, old: None, glitch: None, last_glitch: None, last_change: None, width: Tween::still(0.0), snap: 0, salt: 0 }
    }
}

impl<T: Clone + PartialEq> TagFx<T> {
    /// This frame's content (`None`: no tag). `glitch`: one may start (the setting on, alpha
    /// mode, not dragged, not torn by her own glitch). `quiet`: it comes or goes with motion of
    /// its own (the bubble taking over, a teleport, a swap's hold): no glitch for that.
    pub fn observe(&mut self, next: Option<T>, now: Instant, glitch: bool, quiet: bool) {
        if self.glitch.is_some_and(|g| g.done(now)) || (!glitch && self.glitch.is_some()) {
            self.glitch = None;
            self.old = None;
        }
        if next == self.shown {
            return;
        }
        let calm = self.last_change.is_none_or(|t| ms(now, t) >= GAP_MS) && self.last_glitch.is_none_or(|t| ms(now, t) >= GAP_MS);
        let running = self.glitch.map(|g| g.kind);
        self.salt = self.salt.wrapping_add(1);
        let start = |kind: Kind, me: &mut Self| {
            me.glitch = Some(Glitch::new(kind, now, seed(me.salt)));
            me.last_glitch = Some(now);
        };
        match (self.shown.is_some(), next.is_some()) {
            (true, true) => match running {
                // Mid-glitch: its new half shows the newest text.
                Some(Kind::Swap | Kind::In) => {}
                _ if glitch && calm => {
                    self.old = self.shown.clone();
                    start(Kind::Swap, self);
                }
                _ => {
                    self.glitch = None;
                    self.old = None;
                }
            },
            (false, true) => {
                self.snap = 2;
                self.old = None;
                self.glitch = None;
                if glitch && calm && !quiet {
                    start(Kind::In, self);
                }
            }
            (true, false) => {
                self.glitch = None;
                self.old = None;
                if glitch && calm && !quiet {
                    self.old = self.shown.clone();
                    start(Kind::Out, self);
                }
            }
            (false, false) => {}
        }
        self.last_change = Some(now);
        self.shown = next;
    }

    /// What to draw now (the old content during its half of a glitch) and the glitch.
    pub fn frame(&self, now: Instant) -> (Option<&T>, Option<Glitch>) {
        match self.glitch {
            Some(g) if g.old(now) => (self.old.as_ref(), Some(g)),
            g => (self.shown.as_ref(), g),
        }
    }

    /// The width to draw, given the laid-out one (`motion` off: always that).
    pub fn width(&mut self, laid_out: f32, now: Instant, motion: bool) -> f32 {
        if self.snap > 0 || !motion {
            self.snap = self.snap.saturating_sub(1);
            self.width.snap(laid_out);
        } else {
            self.width.go(laid_out, now, SIZE_MS);
        }
        self.width.at(now)
    }

    pub fn width_now(&self, now: Instant) -> f32 {
        self.width.at(now)
    }

    pub fn busy(&self, now: Instant) -> bool {
        self.glitch.is_some_and(|g| !g.done(now)) || self.width.moving(now)
    }
}

/// The bubble while it is open: its drawn size and its glitch.
#[derive(Debug)]
pub struct PanelFx {
    open: bool,
    serial: u64,
    pub w: Tween,
    pub h: Tween,
    glitch: Option<Glitch>,
    last_glitch: Option<Instant>,
    salt: u64,
}

impl Default for PanelFx {
    fn default() -> Self {
        PanelFx { open: false, serial: 0, w: Tween::still(0.0), h: Tween::still(0.0), glitch: None, last_glitch: None, salt: 0 }
    }
}

impl PanelFx {
    /// Open this frame at the laid-out `w` × `h`, showing reply `serial`; `from`: the size it
    /// grows from when it just opened (the pill's), else `OPEN_SCALE` of its own. `motion`
    /// off: drawn as laid out. Returns the size to draw.
    pub fn open(&mut self, (w, h): (f32, f32), serial: u64, from: Option<(f32, f32)>, now: Instant, motion: bool, glitch: bool) -> (f32, f32) {
        if self.glitch.is_some_and(|g| g.done(now)) || !glitch {
            self.glitch = None;
        }
        let throttle_ok = self.last_glitch.is_none_or(|t| ms(now, t) >= GAP_MS);
        self.salt = self.salt.wrapping_add(1);
        if !self.open {
            self.open = true;
            self.serial = serial;
            let (w0, h0) = from.unwrap_or((w * OPEN_SCALE, h * OPEN_SCALE));
            self.w = Tween::start(w0, w, now, OPEN_MS);
            self.h = Tween::start(h0, h, now, OPEN_MS);
            if glitch && throttle_ok {
                self.glitch = Some(Glitch::new(Kind::In, now, seed(self.salt)));
                self.last_glitch = Some(now);
            }
        } else {
            if serial != self.serial {
                self.serial = serial;
                if glitch && throttle_ok && self.glitch.is_none() {
                    self.glitch = Some(Glitch::new(Kind::Pulse, now, seed(self.salt)));
                    self.last_glitch = Some(now);
                }
            }
            self.w.go(w, now, SIZE_MS);
            self.h.go(h, now, SIZE_MS);
        }
        if !motion {
            self.w.snap(w);
            self.h.snap(h);
        }
        (self.w.at(now), self.h.at(now))
    }

    /// Not open this frame. True when it just closed (the caller may show it leaving).
    pub fn closed(&mut self) -> bool {
        self.glitch = None;
        std::mem::replace(&mut self.open, false)
    }

    pub fn glitch(&self, now: Instant) -> Option<Glitch> {
        self.glitch.filter(|g| !g.done(now))
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.w.moving(now) || self.h.moving(now)
    }

    pub fn busy(&self, now: Instant) -> bool {
        self.moving(now) || self.glitch(now).is_some()
    }

    pub fn throttled(&self, now: Instant) -> bool {
        self.last_glitch.is_some_and(|t| ms(now, t) < GAP_MS)
    }
}

/// A bubble that just closed, drawn from what it last showed while it leaves: a glitch-out
/// and a shrink (to `CLOSE_SCALE`, or into the pill's size).
#[derive(Debug)]
pub struct Ghost<S> {
    pub snap: S,
    pub t0: Instant,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub glitch: Option<Glitch>,
    /// It goes into the pill: the tag waits for it.
    pub to_pill: bool,
}

impl<S> Ghost<S> {
    pub fn new(snap: S, from: (f32, f32), pill: Option<(f32, f32)>, now: Instant, glitch: bool, salt: u64) -> Ghost<S> {
        let to = pill.unwrap_or((from.0 * CLOSE_SCALE, from.1 * CLOSE_SCALE));
        Ghost { snap, t0: now, from, to, glitch: glitch.then(|| Glitch::new(Kind::Out, now, seed(salt))), to_pill: pill.is_some() }
    }

    pub fn u(&self, now: Instant) -> f32 {
        (ms(now, self.t0) / CLOSE_MS).clamp(0.0, 1.0)
    }

    pub fn done(&self, now: Instant) -> bool {
        ms(now, self.t0) >= CLOSE_MS
    }

    pub fn size(&self, now: Instant) -> (f32, f32) {
        let k = ease(self.u(now));
        (self.from.0 + (self.to.0 - self.from.0) * k, self.from.1 + (self.to.1 - self.from.1) * k)
    }

    /// The tag (or the pill) may show: the bubble is nearly gone (or nearly the pill).
    pub fn pill_ready(&self, now: Instant) -> bool {
        self.u(now) >= PILL_AT
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
        // One bump, no wobble: rises, then only falls back to 1.
        let v: Vec<f32> = (0..=100).map(|i| ease(i as f32 / 100.0)).collect();
        let top = v.iter().enumerate().fold((0, 0.0f32), |a, (i, &x)| if x > a.1 { (i, x) } else { a }).0;
        assert!(v[..=top].windows(2).all(|w| w[1] >= w[0]) && v[top..].windows(2).all(|w| w[1] <= w[0]));
        // Fast out: more than half way at a fifth of the time.
        assert!(ease(0.2) > 0.5);
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
        assert!(w.moving(at(t0, 300)));
        assert_eq!(w.at(at(t0, 400)), 120.0);
        assert!(!w.moving(at(t0, 400)));
        // A target it already has: nothing restarts.
        w.go(120.1, at(t0, 500), SIZE_MS);
        assert!(!w.moving(at(t0, 500)));
    }

    #[test]
    fn glitches_assemble_dissolve_and_end_clean() {
        let t0 = Instant::now();
        let g = Glitch::new(Kind::In, t0, 42);
        assert!(g.look(4, 10.0, t0).bands.iter().all(|b| !b.on), "nothing on its first frame");
        let end = g.look(4, 10.0, at(t0, g.ms as u64 - 1));
        assert!(end.bands.iter().all(|b| *b == CLEAN), "settled before it ends: {end:?}");
        let o = Glitch::new(Kind::Out, t0, 42);
        assert!(o.look(4, 10.0, t0).bands.iter().all(|b| b.on));
        assert!(o.look(4, 10.0, at(t0, o.ms as u64 - 2)).bands.iter().all(|b| !b.on));
        let s = Glitch::new(Kind::Swap, t0, 9);
        assert!(s.look(4, 10.0, at(t0, 20)).old);
        let cut = (s.step_at(SWAP_CUT) as f32 * STEP_MS) as u64;
        assert!(s.look(4, 10.0, at(t0, cut - 1)).old && s.old(at(t0, cut - 1)));
        assert!(!s.look(4, 10.0, at(t0, cut + 1)).old && !s.old(at(t0, cut + 1)));
        assert!(s.look(4, 10.0, at(t0, cut + 1)).tear.is_some(), "a tear at the cut");
        assert!(s.look(4, 10.0, at(t0, 200)).tear.is_none());
        // Shifts never pass the amplitude; a pattern holds a step.
        for ms in (0..220).step_by(7) {
            let l = s.look(5, 10.0, at(t0, ms));
            assert!(l.bands.iter().all(|b| b.dx.abs() <= 10.0 + 1e-3 && b.split <= 4.0));
        }
        assert_eq!(s.look(5, 10.0, at(t0, 100)), s.look(5, 10.0, at(t0, 101)));
        let p = Glitch::new(Kind::Pulse, t0, 5);
        let l = p.look(6, 10.0, at(t0, 70));
        assert!(l.bands.iter().all(|b| b.on));
        assert_eq!(l.bands.iter().filter(|b| b.dx != 0.0).count(), 2);
    }

    #[test]
    fn cuts_cover_the_box() {
        for seed in 0..20 {
            let c = cuts(34.0, 4, 6.0, seed);
            assert_eq!(c.first(), Some(&0.0));
            assert_eq!(c.last(), Some(&34.0));
            assert!(c.windows(2).all(|w| w[1] > w[0]));
            assert_eq!(c.len(), 5);
        }
        assert_eq!(cuts(10.0, 5, 6.0, 1).len(), 2, "never thinner than min");
    }

    #[test]
    fn the_tag_swaps_coalesces_and_never_strobes() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        t.observe(Some("ONLINE"), t0, true, false);
        assert_eq!(t.frame(t0).1.map(|g| g.kind), Some(Kind::In));
        // Calm, then a change: a swap, the old text first.
        let t1 = at(t0, 2000);
        t.observe(Some("THINKING"), t1, true, false);
        let (c, g) = t.frame(t1);
        assert_eq!((c, g.map(|g| g.kind)), (Some(&"ONLINE"), Some(Kind::Swap)));
        // Another change mid-glitch: no restart, the new half shows the newest.
        t.observe(Some("WORKING · bash"), at(t1, 50), true, false);
        assert_eq!(t.frame(at(t1, 60)).0, Some(&"ONLINE"));
        assert_eq!(t.frame(at(t1, 150)).0, Some(&"WORKING · bash"));
        assert_eq!(t.frame(at(t1, 150)).1.map(|g| g.t0), Some(t1));
        // The burst goes on: quiet swaps (the newest at once), no second glitch.
        t.observe(Some("THINKING"), at(t1, 400), true, false);
        assert_eq!(t.frame(at(t1, 400)), (Some(&"THINKING"), None));
        t.observe(Some("WORKING · read"), at(t1, 800), true, false);
        assert!(t.frame(at(t1, 800)).1.is_none(), "still a burst: changes 400 ms apart");
        // Calm again: the next change glitches.
        t.observe(Some("DONE"), at(t1, 1500), true, false);
        assert_eq!(t.frame(at(t1, 1500)).1.map(|g| g.kind), Some(Kind::Swap));
        // Gone, in place: it slices out showing what it was.
        t.observe(None, at(t1, 3000), true, false);
        assert_eq!(t.frame(at(t1, 3000)), (Some(&"DONE"), t.frame(at(t1, 3000)).1));
        assert_eq!(t.frame(at(t1, 3000)).1.map(|g| g.kind), Some(Kind::Out));
        assert_eq!(t.frame(at(t1, 3300)).0, Some(&"DONE"), "frame() alone doesn't expire it");
        t.observe(None, at(t1, 3300), true, false);
        assert_eq!(t.frame(at(t1, 3300)), (None, None));
        // Quiet (the bubble took over) or the glitch off: no glitch.
        t.observe(Some("ONLINE"), at(t1, 5000), true, true);
        assert!(t.frame(at(t1, 5000)).1.is_none());
        t.observe(Some("THINKING"), at(t1, 7000), false, false);
        assert_eq!(t.frame(at(t1, 7000)), (Some(&"THINKING"), None));
    }

    #[test]
    fn the_tag_width_eases_but_snaps_when_it_appears() {
        let t0 = Instant::now();
        let mut t: TagFx<&str> = TagFx::default();
        t.observe(Some("ONLINE"), t0, false, false);
        assert_eq!(t.width(80.0, t0, true), 80.0);
        assert_eq!(t.width(90.0, at(t0, 16), true), 90.0, "two frames follow the layout");
        let w = t.width(160.0, at(t0, 32), true);
        assert_eq!(w, 90.0);
        assert!(t.busy(at(t0, 100)));
        let mid = t.width(160.0, at(t0, 132), true);
        assert!(mid > 120.0 && mid < 162.0);
        assert_eq!(t.width(160.0, at(t0, 400), true), 160.0);
        assert!(!t.busy(at(t0, 400)));
        assert_eq!(t.width(60.0, at(t0, 500), false), 60.0, "motion off: as laid out");
    }

    #[test]
    fn the_panel_opens_from_her_resizes_and_pulses_on_a_new_reply() {
        let t0 = Instant::now();
        let mut p = PanelFx::default();
        let (w, h) = p.open((400.0, 200.0), 1, None, t0, true, true);
        assert_eq!((w, h), (400.0 * OPEN_SCALE, 200.0 * OPEN_SCALE));
        assert_eq!(p.glitch(t0).map(|g| g.kind), Some(Kind::In));
        assert_eq!(p.open((400.0, 200.0), 1, None, at(t0, 300), true, true), (400.0, 200.0));
        // Taller content: it grows there, no snap.
        let (_, h) = p.open((400.0, 300.0), 1, None, at(t0, 1000), true, true);
        assert_eq!(h, 200.0);
        let (_, h) = p.open((400.0, 300.0), 1, None, at(t0, 1100), true, true);
        assert!(h > 250.0 && h < 303.0);
        // Streaming in the same reply: no glitch. A new reply: a pulse.
        assert!(p.glitch(at(t0, 1100)).is_none());
        p.open((400.0, 300.0), 2, None, at(t0, 1200), true, true);
        assert_eq!(p.glitch(at(t0, 1200)).map(|g| g.kind), Some(Kind::Pulse));
        // Closed: reported once.
        assert!(p.closed());
        assert!(!p.closed());
        // From the pill: it grows out of the pill's size.
        let (w, h) = p.open((400.0, 200.0), 2, Some((120.0, 28.0)), at(t0, 2000), true, false);
        assert_eq!((w, h), (120.0, 28.0));
        assert!(p.glitch(at(t0, 2000)).is_none(), "glitch off");
        // Motion off: as laid out.
        p.closed();
        assert_eq!(p.open((400.0, 200.0), 3, None, at(t0, 3000), false, false), (400.0, 200.0));
    }

    #[test]
    fn a_ghost_shrinks_into_the_pill() {
        let t0 = Instant::now();
        let g = Ghost::new((), (400.0, 200.0), Some((120.0, 28.0)), t0, true, 1);
        assert_eq!(g.size(t0), (400.0, 200.0));
        assert!(!g.pill_ready(t0));
        assert!(g.pill_ready(at(t0, (CLOSE_MS * PILL_AT) as u64 + 1)));
        let (w, h) = g.size(at(t0, CLOSE_MS as u64));
        assert!((w - 120.0).abs() < 0.01 && (h - 28.0).abs() < 0.01);
        assert!(g.done(at(t0, CLOSE_MS as u64)));
        let g = Ghost::new((), (400.0, 200.0), None, t0, false, 1);
        assert!(g.glitch.is_none() && !g.pill_ready(t0));
        let (w, _) = g.size(at(t0, CLOSE_MS as u64));
        assert!((w - 400.0 * CLOSE_SCALE).abs() < 0.01);
    }
}
