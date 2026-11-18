//! The HUD's motion: the speech bubble, the status tag and the pill move as one designed
//! system. One curve (`ease`: out, 2 % past the target at most, no wobble), one crossfade
//! (`blend`) and one set of durations, for every size change, swap, open and close.
//!
//! Nothing is ever an empty shell, and a shell is never faded (its layers would show through
//! each other). A plate or bubble stays opaque while it reshapes: its outline morphs into the
//! new one (`theme::morph`) as its colours mix, the box easing from wherever it is: a growing
//! box leads its new words, a shrinking one waits for the old words to go.
//!
//! Words never fade through each other: they cut. On one frame the old go and the new are
//! there, readable (`CUT_FROM` or more, whole a frame later), the border's RGB split marking
//! the cut (`Glitch`, two frames at most). A tag's label and its block cut at `SWAP`; a new
//! reply cuts at once; the bubble grows out of the tag (the tag's label until its box covers
//! the reply's words, then the words) and goes back into it (the words on its first frame,
//! the tag's label from the next, the box easing in around it). A tag label holds at least
//! `DWELL_MS`; a burst shows only the latest. Pure: the view samples it while something moves
//! and asks for frames only then.

use std::time::{Duration, Instant};

/// One glitch step: one present at 60 Hz.
pub const STEP_MS: f32 = 1000.0 / 60.0;
/// A glitch: two frames, on the border only (its steps end between frames: a paint a hair
/// early never makes a third).
pub const GLITCH_MS: f32 = 1.5 * STEP_MS;
/// The border's RGB split on a glitch's first step, px (half on its second).
pub const SPLIT: f32 = 2.0;
/// A size change: the bubble growing or shrinking to its content, the tag's width.
pub const SIZE_MS: f32 = 200.0;
/// The bubble opening: from just inside its own box (`INSET`), or out of the pill.
pub const OPEN_MS: f32 = 240.0;
/// Any crossfade: old words (and plate) out, new in, overlapping.
pub const XFADE_MS: f32 = 120.0;
/// A growing plate leads its new words by this much (it is ~90 % there: they never overhang it).
pub const GROW_LEAD_MS: f32 = 90.0;
/// A shrinking box waits this long: the old words are nearly gone before it moves.
pub const SHRINK_LAG_MS: f32 = 90.0;
/// The tag coming (its words from `FLOOR` up, its plate opening out from just inside its
/// width) and going (its words down to `FLOOR`, its plate settling in, then gone).
pub const APPEAR_MS: f32 = 90.0;
pub const EXIT_MS: f32 = 90.0;
/// The lowest level a tag's words show at (coming, going): never fainter.
pub const FLOOR: f32 = 0.6;
/// New words on the frame they cut in: readable at once, whole on the next.
pub const CUT_FROM: f32 = 0.6;
/// The bubble closing with no tag to go into: its border glitches, it settles just inside its
/// box, its words dim to `FLOOR` over its last `CLOSE_DIM_MS`, and it is gone.
pub const CLOSE_MS: f32 = 120.0;
pub const CLOSE_DIM_MS: f32 = 34.0;
/// The bubble going into the tag (closing, or collapsing into the pill): its words on its
/// first frame, then (from `INTO_CUT_MS`) the tag's label, whole, at its centre as its box and
/// outline ease into the tag's over `INTO_MS`.
pub const INTO_CUT_MS: f32 = 0.5 * STEP_MS;
pub const INTO_MS: f32 = 150.0;
/// The bubble growing out of the tag: quicker than from her (`OPEN_MS`); the tag's label
/// stays at its centre until the shell covers the reply's words, which cut in then.
pub const OUT_OF_TAG_MS: f32 = 180.0;
/// A tag label shows at least this long; changes meanwhile wait, and only the latest shows.
pub const DWELL_MS: f32 = 350.0;
/// At most one glitch per element this often.
pub const GAP_MS: f32 = 600.0;
/// How far inside its own box the bubble opens and closes (x per side, y at the top): inside
/// its padding, so its words are never cut.
pub const INSET: (f32, f32) = (7.0, 8.0);
/// The curve's overshoot constant: 2.0 % past the target at 71 % of the way, then settled.
const BACK: f32 = 0.75;

fn ms(now: Instant, t0: Instant) -> f32 {
    now.saturating_duration_since(t0).as_secs_f32() * 1000.0
}

fn after(t: Instant, ms_: f32) -> Instant {
    t + Duration::from_secs_f32(ms_.max(0.0) / 1000.0)
}

/// 0 → 1 over `ms_` from `t0` (0 before it).
fn lin(now: Instant, t0: Instant, ms_: f32) -> f32 {
    (ms(now, t0) / ms_).clamp(0.0, 1.0)
}

/// The one curve: ease-out with a slight overshoot (`BACK`), 0 at 0 and 1 at 1.
pub fn ease(u: f32) -> f32 {
    let s = u.clamp(0.0, 1.0) - 1.0;
    1.0 + (BACK + 1.0) * s * s * s + BACK * s * s
}

/// Smooth in and out, 0 → 1 (a shape's morph, its colours).
pub fn blend(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Where a tag's label (and its block) cuts, a part of `XFADE_MS`: the old whole before it,
/// the new from it.
pub const SWAP: f32 = 0.5;

/// New words `t` ms after they cut in (0 before): `CUT_FROM` on that frame, whole a frame
/// later.
pub fn cut_in(t: f32) -> f32 {
    if t < 0.0 { 0.0 } else { CUT_FROM + (1.0 - CUT_FROM) * blend(t / STEP_MS) }
}

/// The one label cut at `u` (0..1 of `XFADE_MS`): the old words' level, the new words'. Never
/// both, never neither: the old whole until `SWAP`, the new at `CUT_FROM` or more from it.
pub fn xfade(u: f32) -> (f32, f32) {
    let u = u.clamp(0.0, 1.0);
    if u < SWAP { (1.0, 0.0) } else { (0.0, cut_in((u - SWAP) * XFADE_MS)) }
}

/// A plate's own content (WORKING's red block) through a cut at `u`: it goes and comes with
/// its label, so a plate is never a dim outline with a faint label in it.
pub fn plate_levels(u: f32) -> (f32, f32) {
    xfade(u)
}

/// A value easing toward its target along `ease` (it may start later than now).
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

    pub fn start(from: f32, to: f32, t0: Instant, ms: f32) -> Tween {
        Tween { from, to, t0: Some(t0), ms }
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

/// A border glitch: the outline's RGB split, `SPLIT` px then half, while the content shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glitch {
    pub t0: Instant,
}

impl Glitch {
    /// The split now, px (0: over, or not yet): whole on the frame it lands on, half on the
    /// next if that comes within `GLITCH_MS`.
    pub fn split(&self, now: Instant) -> f32 {
        if now < self.t0 {
            return 0.0;
        }
        match ms(now, self.t0) {
            t if t < STEP_MS => SPLIT,
            t if t < GLITCH_MS => SPLIT * 0.5,
            _ => 0.0,
        }
    }

    pub fn done(&self, now: Instant) -> bool {
        now >= self.t0 && ms(now, self.t0) >= GLITCH_MS
    }
}

/// At most one glitch per element per `GAP_MS`.
#[derive(Clone, Copy, Debug, Default)]
struct Throttle(Option<Instant>);

impl Throttle {
    fn take(&mut self, now: Instant, ok: bool) -> Option<Glitch> {
        if !ok || self.blocked(now) {
            return None;
        }
        self.0 = Some(now);
        Some(Glitch { t0: now })
    }

    fn blocked(&self, now: Instant) -> bool {
        self.0.is_some_and(|t| ms(now, t) < GAP_MS)
    }
}

/// The tag coming, there, or going.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Vis {
    Steady,
    /// Coming: since (None: its width isn't measured yet, nothing drawn).
    Appear(Option<Instant>),
    /// Going: since.
    Exit(Instant),
}

/// What a tag draws this frame.
#[derive(Debug, PartialEq)]
pub struct TagFrame<'a, T> {
    /// The newest content (None: no tag).
    pub cur: Option<&'a T>,
    /// Its words' level (0..1; 1 unless it is crossfading in).
    pub words: f32,
    /// How far the plate's outline and colours are from the old content's to its (0..1).
    pub shape: f32,
    /// Its laid-out width (the plate is drawn at `width`, its content centred in it).
    pub nat: f32,
    /// The content crossfading out: it, its laid-out width, its words' level.
    pub prev: Option<(&'a T, f32, f32)>,
    /// Its words' level as it comes or goes (the plate stays opaque).
    pub alpha: f32,
    /// The plates' own content (a block): the old's level, the new's.
    pub blocks: (f32, f32),
    /// The border's RGB split, px.
    pub split: f32,
}

/// The status tag (and the pill, and the unread chip, which are the tag): what it shows, its
/// crossfades, its width.
#[derive(Debug)]
pub struct TagFx<T> {
    /// The newest shown (what the view lays out).
    cur: Option<T>,
    /// The latest asked for.
    want: Option<T>,
    /// Crossfading out: the old content and its laid-out width.
    prev: Option<(T, f32)>,
    /// The new words' crossfade start (None: not measured yet).
    x0: Option<Instant>,
    /// The plate's morph start (with its width).
    m0: Option<Instant>,
    vis: Vis,
    /// The newest content just changed: this frame's layout is the old one's.
    measuring: bool,
    /// Measured next frame: the change starts then.
    pending: bool,
    /// The newest content's laid-out width.
    nat: f32,
    /// When the newest content came in (the dwell).
    shown_at: Option<Instant>,
    glitch: Option<Glitch>,
    glitch_ok: bool,
    throttle: Throttle,
    width: Tween,
    /// Frames where the width follows its layout at once (a quiet change).
    snap: u8,
}

impl<T: Clone + PartialEq> Default for TagFx<T> {
    fn default() -> Self {
        TagFx {
            cur: None,
            want: None,
            prev: None,
            x0: None,
            m0: None,
            vis: Vis::Steady,
            measuring: false,
            pending: false,
            nat: 0.0,
            shown_at: None,
            glitch: None,
            glitch_ok: false,
            throttle: Throttle::default(),
            width: Tween::still(0.0),
            snap: 0,
        }
    }
}

impl<T: Clone + PartialEq> TagFx<T> {
    /// What is due: a crossfade over, a coming or going done.
    fn settle(&mut self, now: Instant) {
        if let Vis::Exit(t0) = self.vis
            && ms(now, t0) >= EXIT_MS
        {
            self.cur = None;
            self.prev = None;
            self.vis = Vis::Steady;
        }
        if let Vis::Appear(Some(t0)) = self.vis
            && ms(now, t0) >= APPEAR_MS
        {
            self.vis = Vis::Steady;
        }
        let words_done = self.x0.is_some_and(|x| now >= x && ms(now, x) >= XFADE_MS);
        let shape_done = self.m0.is_none_or(|m| now >= m && ms(now, m) >= SIZE_MS);
        if self.prev.is_some() && words_done && shape_done {
            self.prev = None;
        }
        if self.glitch.is_some_and(|g| g.done(now)) {
            self.glitch = None;
        }
    }

    /// This frame's wanted content (`None`: no tag). `glitch`: its border may glitch (the
    /// setting on, alpha mode, not dragged, not torn by her own glitch). `quiet`: it comes or
    /// goes with motion of its own (the bubble taking over, a teleport, a swap's hold, his
    /// hand, motion off): at once.
    pub fn observe(&mut self, next: Option<T>, now: Instant, glitch: bool, quiet: bool) {
        self.want = next;
        self.settle(now);
        if quiet {
            if self.want != self.cur {
                self.shown_at = Some(now);
                self.snap = 2;
            } else if self.vis == Vis::Steady && self.prev.is_none() && !self.measuring && !self.pending {
                self.cur = self.want.clone();
                return;
            }
            self.cur = self.want.clone();
            self.prev = None;
            self.x0 = None;
            self.m0 = None;
            self.vis = Vis::Steady;
            self.measuring = false;
            self.pending = false;
            self.glitch = None;
            return;
        }
        if self.want == self.cur {
            // The same to the eye (a level meter moved): drawn as it is now.
            self.cur = self.want.clone();
            if matches!(self.vis, Vis::Exit(_)) {
                // Called back while going: it stays.
                self.vis = Vis::Steady;
            }
            return;
        }
        if matches!(self.vis, Vis::Exit(_)) {
            return;
        }
        // A change of its own: a quiet change's snap still owed (it was never laid out: the
        // bubble had the HUD) would cut it short and leave its words unshown.
        self.snap = 0;
        if self.cur.is_none() {
            self.cur = self.want.clone();
            self.vis = Vis::Appear(None);
            self.measuring = true;
            self.glitch_ok = glitch;
            self.shown_at = Some(now);
            return;
        }
        // Mid-change, or the label hasn't had its dwell: it waits (only the latest shows).
        let busy = self.prev.is_some() || self.measuring || self.pending || matches!(self.vis, Vis::Appear(_));
        if busy || self.shown_at.is_some_and(|t| ms(now, t) < DWELL_MS) {
            return;
        }
        match self.want.clone() {
            None => {
                self.vis = Vis::Exit(now);
                self.glitch = self.throttle.take(now, glitch);
                let w = self.width.at(now);
                self.width = Tween::start(w, (w - 2.0 * INSET.0).max(w * 0.9), now, EXIT_MS);
            }
            Some(w) => {
                let old = self.cur.replace(w).expect("a tag to change from");
                self.prev = Some((old, self.nat));
                self.x0 = None;
                self.m0 = None;
                self.measuring = true;
                self.glitch_ok = glitch;
                self.shown_at = Some(now);
            }
        }
    }

    /// The plate's width to draw, given the laid-out one (`motion` off: always that).
    pub fn width(&mut self, laid_out: f32, now: Instant, motion: bool) -> f32 {
        if !motion || self.snap > 0 {
            self.snap = self.snap.saturating_sub(1);
            self.width.snap(laid_out);
            self.nat = laid_out;
            self.measuring = false;
            self.pending = false;
            return laid_out;
        }
        if self.measuring {
            // This frame's layout is still the old content's: hold; next frame it's measured.
            self.measuring = false;
            self.pending = true;
            return self.width.at(now);
        }
        if !std::mem::take(&mut self.pending) {
            // Steady (or mid-change): a label's width that changed (its meter) eases there.
            if self.prev.is_none() && self.vis == Vis::Steady {
                self.nat = laid_out;
                self.width.go(laid_out, now, SIZE_MS);
            }
            return self.width.at(now);
        }
        // The newest content is measured: its change starts now.
        self.nat = laid_out;
        let g = self.throttle.take(now, self.glitch_ok);
        if self.vis == Vis::Appear(None) {
            // Coming: from just inside its width (inside its padding), at `FLOOR`.
            let from = (laid_out - 2.0 * INSET.0).max(laid_out * 0.9);
            self.width = Tween::start(from, laid_out, now, SIZE_MS);
            self.vis = Vis::Appear(Some(now));
        } else {
            let w = self.width.at(now);
            if laid_out > w + 0.5 {
                // Growing: the plate leads, the new words follow.
                self.width = Tween::start(w, laid_out, now, SIZE_MS);
                self.x0 = Some(after(now, GROW_LEAD_MS));
                self.m0 = Some(now);
            } else if laid_out < w - 0.5 {
                // Shrinking: the old words go first, then the plate.
                let m = after(now, SHRINK_LAG_MS);
                self.width = Tween::start(w, laid_out, m, SIZE_MS);
                self.x0 = Some(now);
                self.m0 = Some(m);
            } else {
                self.x0 = Some(now);
                self.m0 = Some(now);
            }
        }
        // The border's split lands on the label's cut (not ahead of it, on the old one).
        let at = match (self.vis, self.x0) {
            (Vis::Appear(_), _) | (_, None) => now,
            (_, Some(x0)) => after(x0, XFADE_MS * SWAP),
        };
        self.glitch = g.map(|_| Glitch { t0: at });
        self.width.at(now)
    }

    /// The width as it is (the tag not laid out this frame: going).
    pub fn width_now(&self, now: Instant) -> f32 {
        self.width.at(now)
    }

    /// What to draw now.
    pub fn frame(&self, now: Instant) -> TagFrame<'_, T> {
        let u = match (&self.prev, self.x0) {
            (None, _) => 1.0,
            (Some(_), Some(x0)) if now >= x0 => ms(now, x0) / XFADE_MS,
            (Some(_), _) => 0.0,
        };
        let (old, new) = if self.prev.is_some() { xfade(u) } else { (0.0, 1.0) };
        let blocks = if self.prev.is_some() { plate_levels(u) } else { (0.0, 1.0) };
        let alpha = match self.vis {
            Vis::Steady => 1.0,
            Vis::Appear(None) => 0.0,
            Vis::Appear(Some(t0)) => FLOOR + (1.0 - FLOOR) * lin(now, t0, APPEAR_MS),
            Vis::Exit(t0) => 1.0 - (1.0 - FLOOR) * lin(now, t0, EXIT_MS),
        };
        let shape = match (&self.prev, self.m0) {
            (None, _) => 1.0,
            (Some(_), None) => 0.0,
            (Some(_), Some(m)) => blend(lin(now, m, SIZE_MS)),
        };
        TagFrame {
            cur: self.cur.as_ref(),
            words: new,
            shape,
            nat: self.nat,
            prev: self.prev.as_ref().map(|(p, w)| (p, *w, old)),
            alpha,
            blocks,
            split: self.glitch.map_or(0.0, |g| g.split(now)),
        }
    }

    /// Something moves: a frame per present.
    pub fn busy(&self, now: Instant) -> bool {
        self.vis != Vis::Steady || self.prev.is_some() || self.measuring || self.pending || self.width.moving(now) || self.glitch.is_some_and(|g| !g.done(now))
    }

    /// When a waiting change is due (its dwell over).
    pub fn wake_at(&self) -> Option<Instant> {
        let idle = self.vis == Vis::Steady && self.prev.is_none() && !self.measuring && !self.pending;
        (idle && self.want != self.cur).then(|| self.shown_at.map(|t| after(t, DWELL_MS))).flatten()
    }
}

/// The bubble this frame: its shell's size, its content's (the box its words are laid out
/// in: the one it is going to while it opens, its words where they will be, shown only inside
/// the shell), their levels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelFrame {
    pub shell: (f32, f32),
    pub content: (f32, f32),
    /// Everything inside it (out of the tag: 0 until its shell covers its words, then cut in).
    pub inner: f32,
    /// The live reply's words.
    pub words: f32,
    /// The reply it replaced: its words' level (0: none; a new reply cuts at once).
    pub old: f32,
    /// Just replaced (this frame).
    pub replaced: bool,
    /// Out of the tag: the tag's words' level, at the shell's centre.
    pub pill: Option<f32>,
    /// The border's RGB split, px.
    pub split: f32,
}

/// The bubble while it is open: its drawn size, its crossfades, its border's glitch.
#[derive(Debug)]
pub struct PanelFx {
    open: bool,
    serial: u64,
    pub w: Tween,
    pub h: Tween,
    /// Opening: since when, and whether out of the pill.
    opened: Option<(Instant, bool)>,
    /// Out of the tag: since when, and when (ms) its shell covers its words (the tag's label
    /// until then, the bubble's words from then).
    pill_x0: Option<(Instant, f32)>,
    glitch: Option<Glitch>,
    throttle: Throttle,
}

impl Default for PanelFx {
    fn default() -> Self {
        PanelFx { open: false, serial: 0, w: Tween::still(0.0), h: Tween::still(0.0), opened: None, pill_x0: None, glitch: None, throttle: Throttle::default() }
    }
}

impl PanelFx {
    /// Open this frame at the laid-out `w` × `h`, showing reply `serial`; `pill`: the pill's
    /// size when it just opened out of it. A new reply crossfades its words in the box as it
    /// is. `motion` off: drawn as laid out, words at once.
    pub fn open(&mut self, (w, h): (f32, f32), serial: u64, pill: Option<(f32, f32)>, now: Instant, motion: bool, glitch: bool) -> PanelFrame {
        let mut replaced = false;
        if self.glitch.is_some_and(|g| g.done(now)) {
            self.glitch = None;
        }
        if !self.open {
            self.open = true;
            self.serial = serial;
            self.pill_x0 = None;
            let (w0, h0) = pill.unwrap_or(((w - 2.0 * INSET.0).max(w * 0.9), (h - INSET.1).max(h * 0.9)));
            let d = if pill.is_some() { OUT_OF_TAG_MS } else { OPEN_MS };
            self.w = Tween::start(w0, w, now, d);
            self.h = Tween::start(h0, h, now, d);
            self.opened = Some((now, pill.is_some()));
            // When the shell covers its words (inside its padding): the first such step of
            // the curve.
            let cover = (1..=100)
                .map(|i| i as f32 / 100.0)
                .find(|&u| {
                    let k = ease(u);
                    w - (w0 + (w - w0) * k) <= 2.0 * INSET.0 && h - (h0 + (h - h0) * k) <= INSET.1
                })
                .unwrap_or(1.0)
                * d;
            self.pill_x0 = pill.map(|_| (now, cover));
            // Out of the tag, the border's split marks the words cutting in.
            self.glitch = self.throttle.take(now, glitch).map(|g| Glitch { t0: after(g.t0, if pill.is_some() { cover } else { 0.0 }) });
        } else {
            if serial != self.serial {
                // A new reply: its words cut in over the old at once, the border split.
                self.serial = serial;
                replaced = true;
                self.glitch = self.throttle.take(now, glitch);
            }
            self.w.go(w, now, SIZE_MS);
            self.h.go(h, now, SIZE_MS);
        }
        if !motion {
            self.w.snap(w);
            self.h.snap(h);
            self.opened = None;
            self.pill_x0 = None;
            self.glitch = None;
        }
        if self.opened.is_some_and(|(t0, _)| ms(now, t0) >= OPEN_MS && !self.w.moving(now) && !self.h.moving(now)) {
            self.opened = None;
        }
        let shell = (self.w.at(now), self.h.at(now));
        // Opening: its words where they will be (the view shows them only inside the shell).
        let content = if self.opened.is_some() { (w, h) } else { shell };
        // Out of the tag: its label at the centre until the shell covers the words, which cut
        // in then (never both, never neither).
        let (pill_words, inner) = match self.pill_x0 {
            Some((x, cover)) => {
                let t = ms(now, x) - cover;
                if t < 0.0 { (1.0, 0.0) } else { (0.0, cut_in(t)) }
            }
            None => (0.0, 1.0),
        };
        if self.pill_x0.is_some_and(|(x, cover)| ms(now, x) >= cover + STEP_MS) {
            self.pill_x0 = None;
        }
        PanelFrame {
            shell,
            content,
            inner,
            words: 1.0,
            old: 0.0,
            replaced,
            pill: (inner < 1.0).then_some(pill_words),
            split: self.glitch.map_or(0.0, |g| g.split(now)),
        }
    }

    /// Not open this frame. True when it just closed (the caller may show it leaving).
    pub fn closed(&mut self) -> bool {
        self.glitch = None;
        self.opened = None;
        self.pill_x0 = None;
        std::mem::replace(&mut self.open, false)
    }

    pub fn busy(&self, now: Instant) -> bool {
        self.w.moving(now) || self.h.moving(now) || self.glitch.is_some_and(|g| !g.done(now)) || self.opened.is_some() || self.pill_x0.is_some()
    }

    /// A glitch now would be over the limit.
    pub fn throttled(&self, now: Instant) -> bool {
        self.throttle.blocked(now)
    }
}

/// A box: x, y, w, h.
pub type Rect = (f32, f32, f32, f32);

/// A bubble that just closed, drawn from what it last showed while it leaves (its shell opaque
/// throughout). Into the tag (or the pill): its words on its first frame, then the tag's label,
/// whole, at its centre (the border split on that cut) as its box eases into the tag's and its
/// outline morphs into the tag's. With no tag to go into: its border glitches, it settles just
/// inside its own box, its words dim to `FLOOR`, and it is gone.
#[derive(Debug)]
pub struct Ghost<S> {
    pub snap: S,
    pub t0: Instant,
    pub to_pill: bool,
    to: Rect,
    /// The move: from, its start, its length (a retarget rebases it).
    m_from: Rect,
    m0: Instant,
    m_ms: f32,
    glitch: Option<Glitch>,
}

impl<S> Ghost<S> {
    /// From `from`; into `pill` if given, else closing in place.
    pub fn new(snap: S, from: Rect, pill: Option<Rect>, now: Instant, glitch: bool) -> Ghost<S> {
        let to = pill.unwrap_or((from.0 + INSET.0, from.1 + INSET.1, from.2 - 2.0 * INSET.0, from.3 - INSET.1));
        let (m0, m_ms) = if pill.is_some() { (after(now, INTO_CUT_MS), INTO_MS) } else { (now, CLOSE_MS) };
        let g0 = if pill.is_some() { after(now, INTO_CUT_MS) } else { now };
        Ghost { snap, t0: now, to_pill: pill.is_some(), to, m_from: from, m0, m_ms, glitch: glitch.then_some(Glitch { t0: g0 }) }
    }

    /// The pill's box as measured now: the move goes there, from where it is.
    pub fn retarget(&mut self, to: Rect, now: Instant) {
        let same = (to.0 - self.to.0).abs() < 0.5 && (to.1 - self.to.1).abs() < 0.5 && (to.2 - self.to.2).abs() < 0.5 && (to.3 - self.to.3).abs() < 0.5;
        if !self.to_pill || same {
            return;
        }
        if now > self.m0 {
            let left = (self.m_ms - ms(now, self.m0)).max(100.0);
            self.m_from = self.rect(now);
            self.m0 = now;
            self.m_ms = left;
        }
        self.to = to;
    }

    fn u(&self, now: Instant) -> f32 {
        lin(now, self.m0, self.m_ms)
    }

    /// How far the box has moved, 0..1 (eased).
    pub fn k(&self, now: Instant) -> f32 {
        ease(self.u(now))
    }

    /// Where it goes.
    pub fn target(&self) -> Rect {
        self.to
    }

    pub fn rect(&self, now: Instant) -> Rect {
        let k = self.k(now);
        let l = |a: f32, b: f32| a + (b - a) * k;
        (l(self.m_from.0, self.to.0), l(self.m_from.1, self.to.1), l(self.m_from.2, self.to.2), l(self.m_from.3, self.to.3))
    }

    /// Its words' level: closing, they dim to `FLOOR` over its last `CLOSE_DIM_MS`.
    pub fn alpha(&self, now: Instant) -> f32 {
        if self.to_pill {
            return 1.0;
        }
        let t = ms(now, self.t0) - (CLOSE_MS - CLOSE_DIM_MS);
        1.0 - (1.0 - FLOOR) * (t / CLOSE_DIM_MS).clamp(0.0, 1.0)
    }

    /// Into the tag: its own words' level, the tag's words', and how far its outline and
    /// colours are into the tag's (0..1). The words cut: its own, then the tag's.
    pub fn pill(&self, now: Instant) -> (f32, f32, f32) {
        let t = ms(now, self.t0) - INTO_CUT_MS;
        let (own, tag) = if t < 0.0 { (1.0, 0.0) } else { (0.0, cut_in(t)) };
        (own, tag, ease(self.u(now)).min(1.0))
    }

    /// The border's RGB split, px.
    pub fn split(&self, now: Instant) -> f32 {
        self.glitch.map_or(0.0, |g| g.split(now))
    }

    pub fn done(&self, now: Instant) -> bool {
        now >= self.m0 && ms(now, self.m0) >= self.m_ms
    }

    /// The tag (the pill) takes over: the box is in the pill's, or gone.
    pub fn tag_ready(&self, now: Instant) -> bool {
        self.done(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // A cut: never both, never neither, the new readable on its first frame, whole a frame on.
        for i in 0..=1000 {
            let u = i as f32 / 1000.0;
            let (o, n) = xfade(u);
            assert!(o.min(n) == 0.0 && o.max(n) >= CUT_FROM, "at {u}: {o} {n}");
            assert!(u >= SWAP || o == 1.0, "the old whole until the cut");
            assert_eq!(plate_levels(u), (o, n), "the block goes and comes with its label");
        }
        assert_eq!(xfade(SWAP + STEP_MS / XFADE_MS), (0.0, 1.0), "whole a frame after the cut");
        assert_eq!(xfade(0.0), (1.0, 0.0));
        assert_eq!(xfade(1.0), (0.0, 1.0));
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
        let later = Tween::start(100.0, 50.0, at(t0, 90), SIZE_MS);
        assert_eq!(later.at(at(t0, 40)), 100.0, "a later start holds until then");
        assert!(later.moving(at(t0, 40)));
    }

    /// Steps a tag through frames 16 ms apart, `wants` changing at given times; each frame
    /// lays out the newest content at `widths(content)`. Returns per frame: the drawn width,
    /// the frame (owned).
    /// One frame: ms, drawn width, newest content, its words, the old content and its words,
    /// the tag's level, the border's split.
    type Step = (u64, f32, Option<&'static str>, f32, Option<(&'static str, f32)>, f32, f32);

    fn run(fx: &mut TagFx<&'static str>, t0: Instant, wants: &[(u64, Option<&'static str>)], until: u64, widths: fn(&str) -> f32) -> Vec<Step> {
        let mut out = Vec::new();
        let mut laid: Option<f32> = None;
        let mut t = 0;
        while t <= until {
            let now = at(t0, t);
            let want = wants.iter().rev().find(|w| w.0 <= t).and_then(|w| w.1);
            fx.observe(want, now, true, false);
            let w = match laid {
                Some(l) => fx.width(l, now, true),
                None => fx.width_now(now),
            };
            let f = fx.frame(now);
            out.push((t, w, f.cur.copied(), f.words, f.prev.map(|p| (*p.0, p.2)), f.alpha, f.split));
            assert!((0.0..=1.0).contains(&f.shape));
            laid = f.cur.map(|c| widths(c));
            t += 16;
        }
        out
    }

    fn widths(s: &str) -> f32 {
        match s {
            "ONLINE" => 100.0,
            "THINKING" => 130.0,
            "WORKING" => 190.0,
            "DONE" => 90.0,
            _ => 120.0,
        }
    }

    #[test]
    fn a_swap_crossfades_and_never_shows_an_empty_plate() {
        let t0 = Instant::now();
        let mut fx = TagFx::default();
        let f = run(&mut fx, t0, &[(0, Some("ONLINE")), (1000, Some("WORKING")), (2000, Some("DONE"))], 2600, widths);
        let mut low = 0;
        for &(t, _, cur, words, prev, alpha, _) in f.iter().filter(|f| f.0 >= 200) {
            let shown = words.max(prev.map_or(0.0, |p| p.1));
            assert!(cur.is_some() && alpha >= 0.99 && words.min(prev.map_or(0.0, |p| p.1)) == 0.0, "one label at a time at {t} ms");
            low = if shown < FLOOR - 1e-3 { low + 1 } else { 0 };
            assert!(low <= 1, "words under the floor for more than a frame at {t} ms: {words} {prev:?}");
        }
        // WORKING → DONE: the old words go first, then the plate shrinks (no overhang).
        let s = f.iter().find(|f| f.0 >= 2000 && f.2 == Some("DONE")).unwrap().0;
        let x = |ms: u64| f.iter().find(|f| f.0 >= s + ms).unwrap();
        assert!(x(64).1 > 189.0, "the plate holds while WORKING fades");
        assert!(x(250).3 >= 0.99, "DONE in full by ~250 ms");
        assert!((x(400).1 - 90.0).abs() < 0.5 && x(400).4.is_none(), "settled at DONE's width");
        let widths_seen: Vec<f32> = f.iter().filter(|f| f.0 >= s && f.0 < s + 400).map(|f| f.1).collect();
        assert!(widths_seen.windows(2).all(|w| (w[1] - w[0]).abs() < 40.0), "no one-frame shape swap");
        // ONLINE → WORKING: the plate grows first, the new words follow.
        let g = f.iter().find(|f| f.0 >= 1000 && f.2 == Some("WORKING")).unwrap().0;
        let y = |ms: u64| f.iter().find(|f| f.0 >= g + ms).unwrap();
        assert!(y(48).3 < 0.05 && y(48).1 > 140.0, "the plate leads");
        // Its two border frames land on the swap: the frame the new words show, or the one before.
        let sw = f.iter().position(|f| f.0 >= g && f.3 > 0.0).unwrap();
        assert!(f[sw - 1].6 > 0.0 || f[sw].6 > 0.0, "the border glitches at the swap");
        let swap_t = f[sw].0;
        assert!(f.iter().filter(|s| s.0 >= g && s.0 + 20 < swap_t).all(|s| s.6 == 0.0), "not ahead of it");
        let glitched = f.iter().filter(|f| f.6 > 0.0).count();
        assert!(glitched <= 3 * 3, "a glitch is two frames per change: {glitched} frames for 3");
    }

    #[test]
    fn a_burst_dwells_and_shows_only_the_latest_with_no_faint_frames() {
        let t0 = Instant::now();
        let mut fx = TagFx::default();
        let wants = [(0, Some("ONLINE")), (1000, Some("THINKING")), (1250, Some("WORKING")), (1500, Some("THINKING")), (1800, Some("WORKING")), (2050, Some("THINKING")), (2350, Some("WORKING")), (2650, Some("DONE"))];
        let f = run(&mut fx, t0, &wants, 3600, widths);
        let mut seen: Vec<(u64, &str)> = Vec::new();
        let mut low = 0;
        for &(t, _, cur, words, prev, alpha, _) in f.iter().filter(|f| f.0 >= 200) {
            low = if words.max(prev.map_or(0.0, |p| p.1)) < FLOOR - 1e-3 { low + 1 } else { 0 };
            assert!(alpha >= 0.99 && low <= 1, "no faint frames at {t}");
            if let Some(c) = cur
                && seen.last().is_none_or(|s| s.1 != c)
            {
                seen.push((t, c));
            }
        }
        assert!(seen[1..].windows(2).all(|w| w[1].0 - w[0].0 >= DWELL_MS as u64), "each held its dwell: {seen:?}");
        assert_eq!(seen.last().unwrap().1, "DONE", "the latest wins");
        assert!(seen.len() <= 6, "skipped what was in between: {seen:?}");
    }

    #[test]
    fn it_comes_and_goes_from_the_floor_and_quietly_when_told() {
        let t0 = Instant::now();
        let mut fx = TagFx::default();
        let f = run(&mut fx, t0, &[(0, Some("ONLINE")), (1000, None)], 1300, widths);
        let first = f.iter().find(|f| f.5 > 0.0).unwrap();
        assert!(first.5 >= FLOOR - 0.01, "it comes in at the floor, not from nothing");
        assert!(f.iter().filter(|f| f.0 < 1000).all(|f| f.5 == 0.0 || f.5 >= FLOOR - 0.01));
        let going: Vec<_> = f.iter().filter(|f| f.0 >= 1000 && f.2.is_some()).collect();
        assert!(going.iter().all(|f| f.5 >= FLOOR - 0.01), "it goes down to the floor, then it's gone");
        assert!(f.last().unwrap().2.is_none());
        let mut q = TagFx::default();
        q.observe(Some("ONLINE"), t0, true, true);
        assert_eq!(q.frame(t0).alpha, 1.0);
        q.observe(Some("DONE"), at(t0, 16), true, true);
        assert!(q.frame(at(t0, 16)).prev.is_none() && q.frame(at(t0, 16)).cur == Some(&"DONE"));
        // Put away quietly (the bubble took over) and never laid out since: it comes back with
        // its words, and settles.
        let mut b = TagFx::default();
        let f = run(&mut b, t0, &[(0, Some("ONLINE"))], 600, widths);
        assert!(f.last().unwrap().3 == 1.0);
        b.observe(None, at(t0, 616), true, true);
        let f = run(&mut b, at(t0, 632), &[(0, Some("ONLINE"))], 800, widths);
        let last = f.last().unwrap();
        assert!(last.3 == 1.0 && last.5 == 1.0, "back with its words: {last:?}");
        assert!(!b.busy(at(t0, 632 + 800)), "and settled");
    }

    #[test]
    fn the_bubble_opens_full_of_its_words_and_crosses_a_new_reply() {
        let t0 = Instant::now();
        let mut p = PanelFx::default();
        let f = p.open((400.0, 120.0), 1, None, t0, true, true);
        assert_eq!(f.words, 1.0, "its words from the first frame");
        assert!(f.shell.0 >= 400.0 - 2.0 * INSET.0 - 0.01 && f.shell.1 >= 120.0 - INSET.1 - 0.01);
        assert_eq!(f.content, (400.0, 120.0), "laid out in the box it's going to");
        assert!(f.split > 0.0);
        let f = p.open((400.0, 120.0), 1, None, at(t0, 300), true, true);
        assert_eq!((f.shell, f.content, f.split), ((400.0, 120.0), (400.0, 120.0), 0.0));
        // A new, shorter reply: its words cut in at once, whole (the old never drawn again),
        // the border split on that frame; the box eases to it.
        let f = p.open((300.0, 80.0), 2, None, at(t0, 700), true, true);
        assert!(f.replaced && f.old == 0.0 && f.words == 1.0 && f.split > 0.0);
        let mut t = 708;
        while t <= 1000 {
            let f = p.open((300.0, 80.0), 2, None, at(t0, t), true, true);
            assert!(!f.replaced && f.old == 0.0 && f.words == 1.0, "at {t}");
            assert!(t < 700 + 2 * STEP_MS as u64 || f.split == 0.0, "two frames of split at most");
            t += 8;
        }
        let f = p.open((300.0, 80.0), 2, None, at(t0, 1000), true, true);
        assert_eq!(f.shell, (300.0, 80.0));
    }

    #[test]
    fn out_of_the_tag_its_label_holds_until_the_words_cut_in() {
        let t0 = Instant::now();
        let mut p = PanelFx::default();
        let mut t = 0;
        let mut landed = None;
        while t <= 500 {
            let f = p.open((400.0, 150.0), 1, Some((110.0, 25.0)), at(t0, t), true, true);
            let pw = f.pill.unwrap_or(0.0);
            assert!(pw.min(f.inner) == 0.0 && pw.max(f.inner) >= CUT_FROM, "never both, never neither at {t}: tag {pw} live {}", f.inner);
            if f.split > 0.0 {
                assert!(f.inner > 0.0, "the split marks the cut, at {t}");
            }
            if f.inner > 0.0 {
                assert!(400.0 - f.shell.0 <= 2.0 * INSET.0 + 0.5 && 150.0 - f.shell.1 <= INSET.1 + 0.5, "its words only once the shell covers them, at {t}");
            }
            if f.inner > 0.0 && landed.is_none() {
                landed = Some((t, f.shell));
            }
            t += 8;
        }
        let (t, shell) = landed.expect("its words");
        assert!(t <= 150 && 400.0 - shell.0 < 0.05 * 290.0, "in as the box lands: {t} ms, {shell:?}");
        assert!(p.open((400.0, 150.0), 1, None, at(t0, 600), true, true).pill.is_none());
    }

    #[test]
    fn a_ghost_goes_into_the_tag_or_closes_in_place() {
        let t0 = Instant::now();
        let c = Ghost::new((), (0.0, 0.0, 400.0, 200.0), None, t0, true);
        assert!(c.split(t0) > 0.0 && c.alpha(t0) == 1.0, "the border glitches with everything there");
        let r = c.rect(at(t0, 119));
        assert!((r.2 - (400.0 - 2.0 * INSET.0)).abs() < 1.0 && (r.1 + r.3 - 200.0).abs() < 0.5, "settles inside its box, about its bottom");
        assert!((0..=CLOSE_MS as u64).all(|t| c.alpha(at(t0, t)) >= FLOOR - 0.01));
        assert!(!c.done(at(t0, 100)) && c.done(at(t0, 121)));
        let mut g = Ghost::new((), (0.0, 0.0, 400.0, 200.0), Some((140.0, 230.0, 120.0, 25.0)), t0, true);
        // Its first frame: as it was (its words, its box, no split).
        assert_eq!((g.split(t0), g.pill(t0).0, g.pill(t0).1, g.rect(t0)), (0.0, 1.0, 0.0, (0.0, 0.0, 400.0, 200.0)));
        // The next: the tag's label, readable, the split on the cut, the box on its way.
        let n = at(t0, 17);
        assert!(g.pill(n).0 == 0.0 && g.pill(n).1 >= CUT_FROM && g.split(n) > 0.0 && g.rect(n).2 < 380.0);
        for t in (0..=300).step_by(4) {
            let (own, tag, q) = g.pill(at(t0, t));
            assert!(own.min(tag) == 0.0 && own.max(tag) >= CUT_FROM && (0.0..=1.0).contains(&q), "one at a time, never neither, at {t}");
        }
        assert!(g.rect(at(t0, 60)).2 < 200.0, "it never sits big for long");
        g.retarget((150.0, 230.0, 100.0, 25.0), at(t0, 50));
        // Retargeted at 50 ms (the tag measured): the rest of its move from there.
        let end = 50 + (INTO_MS - (50.0 - INTO_CUT_MS)).max(100.0) as u64;
        assert!(!g.tag_ready(at(t0, end - 10)) && g.tag_ready(at(t0, end + 1)));
        assert_eq!(g.rect(at(t0, end + 1)), (150.0, 230.0, 100.0, 25.0), "it ends at exactly the pill's box");
        assert_eq!(g.pill(at(t0, end + 1)).2, 1.0, "and exactly the pill's outline");
    }
}
