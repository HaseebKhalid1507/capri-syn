//! The speech bubble's size (logical px): as wide as its widest line and as tall as its lines,
//! between a floor (what its header and every key in its footer need, whole) and the most it may
//! take where she is. A streaming reply grows in steps, at most once per `STEP`, and never
//! shrinks while more may come; when its turn ends it settles once to its fit. What it shows
//! now never waits for a step: text the box doesn't hold (all of a long reply at once, a spoken
//! caption) grows it at once. In a shaped window (no compositor) a width change drops the bubble
//! out of the shape for a frame: there it streams at the most and never narrows within a reply.
//! Pure: the view lays out what this decides, and the tests run the same code.

use std::time::{Duration, Instant};

/// A streaming bubble's width grows in steps of this over its floor...
pub const STEP_W: f32 = 48.0;
/// ...and its size changes at most this often (`hud::SIZE_MS` and a little: one ease ends before
/// the next starts).
pub const STEP: Duration = Duration::from_millis(250);
/// The longest words wait for the box to grow around them, from when it fell short of what it is
/// growing to (its last change; its next is due within `STEP` and eases in `hud::SIZE_MS`), or
/// from when that last grew. Its stale wrap holds its words no longer either, from its last frame
/// (not drawn as a reply: none).
pub const WAIT: Duration = Duration::from_millis(750);
/// ...and never longer than this from when the box fell short, however often what it grows to
/// grows (a stream that keeps it a step behind).
pub const WAIT_CAP: Duration = Duration::from_millis(2 * 750);
/// The text field narrows a step only once its words are this far under the step below: typing
/// and deleting at a step's edge never flips its width.
pub const HYST: f32 = 16.0;

/// The footer's keys, as the view draws them: `KEY_GAP` apart; a text key `KEY_PX` of plate either
/// side of its words; the chevron key `CHEVRON_W`; a spacer `SPACER_W`; READ `READ_PX` either side
/// of its glyph (`READ_GLYPH_W`), `READ_GAP`, its words; the pager two `PAGER_ARROW_W` arrows
/// `PAGER_GAP` from its count, `PAGER_PX` either side of that.
pub const KEY_GAP: f32 = 4.0;
pub const KEY_PX: f32 = 12.0;
pub const CHEVRON_W: f32 = 24.0;
pub const SPACER_W: f32 = 4.0;
pub const READ_PX: f32 = 14.0;
pub const READ_GAP: f32 = 7.0;
pub const READ_GLYPH_W: f32 = 11.0;
pub const PAGER_ARROW_W: f32 = 19.0;
pub const PAGER_GAP: f32 = 2.0;
pub const PAGER_PX: f32 = 3.0;
/// Clear air between the footer's left part (the pager, a note, a chip) and its keys.
pub const FOOT_AIR: f32 = 12.0;
/// On every need: GPUI rounds as it lays out, and a key a pixel short is a key cut.
pub const SLACK: f32 = 2.0;

/// A text key (ABORT, DENY, ALLOW…) whose words are `words` wide.
pub fn text_key(words: f32) -> f32 {
    2.0 * KEY_PX + words
}

/// READ (or CLOSE READER) whose words are `words` wide.
pub fn read_key(words: f32) -> f32 {
    2.0 * READ_PX + READ_GLYPH_W + READ_GAP + words
}

/// The pager (‹ 2/3 ›) whose count is `count` wide.
pub fn pager(count: f32) -> f32 {
    2.0 * PAGER_ARROW_W + 2.0 * PAGER_GAP + 2.0 * PAGER_PX + count
}

/// A footer as the view lays it out: a part at its left (`left` wide, `inset` from the left edge;
/// 0: none) and keys at its right (`keys`, `KEY_GAP` apart, `edge` from the right edge).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foot {
    pub inset: f32,
    pub left: f32,
    pub keys: Vec<f32>,
    pub edge: f32,
}

impl Foot {
    /// The least width that shows all of it, whole and apart.
    pub fn need(&self) -> f32 {
        let keys = self.keys.iter().sum::<f32>() + KEY_GAP * self.keys.len().saturating_sub(1) as f32;
        let air = if self.left > 0.0 && !self.keys.is_empty() { FOOT_AIR } else { 0.0 };
        self.inset + self.left + air + keys + self.edge + SLACK
    }
}

/// The bubble's width: what its text needs (its widest line and padding), never under `floor`
/// (its header and footer, whole), never over `max_w` (where she is).
pub fn width(text: f32, floor: f32, max_w: f32) -> f32 {
    text.max(floor).min(max_w)
}

/// `w` a step up: its floor, or the floor and whole `STEP_W` steps over it, at most `max_w`.
pub fn step_up(w: f32, floor: f32, max_w: f32) -> f32 {
    let floor = floor.min(max_w);
    if w <= floor + 0.01 {
        return floor;
    }
    (floor + ((w - floor) / STEP_W - 1e-3).ceil() * STEP_W).clamp(w.min(max_w), max_w)
}

/// What a frame asks of a reply's size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ask {
    /// Its exact fit, (width, rows): all its text, typed out.
    pub fit: (f32, usize),
    /// The least width: its header and footer, whole.
    pub floor: f32,
    /// What the text it shows now needs, (width, rows): never kept waiting for a step.
    pub hold: (f32, usize),
    /// Gone long: its opening lines and READ.
    pub long: bool,
    /// Streaming: more may come.
    pub more: bool,
    /// A shaped window (no compositor): it streams at `max_w` and never narrows within a reply.
    pub fixed: bool,
    /// The most it may take where she is.
    pub max_w: f32,
}

/// A reply bubble's size as laid out: its width and its rows. A streaming reply's grows in steps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grow {
    pub w: f32,
    pub rows: usize,
    /// When it last changed (None: never laid out).
    at: Option<Instant>,
    /// The widest its header has needed in this reply: a status changing under a settled reply
    /// (SPEAKING, DONE, ONLINE) doesn't resize it.
    head: f32,
    /// What its last layout asked for, (width, rows): laid out, or its next step (due within
    /// `STEP`); and the most it may take where she is, then.
    want: (f32, usize),
    most: f32,
    /// The box it was drawn in last frame, (width, rows), and when: its words wait for it, not
    /// for a guess.
    drawn: (f32, usize),
    drawn_at: Option<Instant>,
    /// Since when the box as drawn has been short of what it is growing to (None: it holds it):
    /// from when it fell short (`WAIT_CAP`), and from then or when that last grew (`WAIT`).
    short_from: Option<Instant>,
    short_since: Option<Instant>,
    /// What it was growing to when last drawn: its wait starts afresh when that grows.
    aim: (f32, usize),
    /// Not drawn as a reply since its last frame (the reader's stub, a prompt, the text field took
    /// the panel, or it closed): drawn again, its wait starts afresh.
    away: bool,
    /// The reader's stub: the widest it has been since it opened.
    stub: f32,
}

impl Grow {
    /// The size to lay out now. Streaming (`more`): the fit's width a step up, never less than it
    /// was, at most `max_w`; its rows never fewer unless it went long (its opening lines and
    /// READ); a change at most once per `STEP`, but at once on its first layout, back inside
    /// `max_w` when her room shrinks, out to a floor that rose (a key appearing, whole), around
    /// text it shows that the box doesn't hold, and to its opening lines when it goes long. Not
    /// streaming: the fit (an ended stream settles once). `fixed`: `max_w` while streaming, never
    /// narrower within the reply, no settling.
    pub fn next(&mut self, a: Ask, now: Instant) -> (f32, usize) {
        self.stub = 0.0;
        let first = self.at.is_none();
        let w = match (a.more, a.fixed) {
            (true, true) => a.max_w,
            (true, false) => step_up(a.fit.0, a.floor, a.max_w),
            (false, _) => a.fit.0,
        };
        let w = if !first && (a.more || a.fixed) { w.max(self.w) } else { w }.min(a.max_w);
        // A change under what the box eases by (`hud::Tween::go`) is none: nothing waits for it.
        let w = if (w - self.w).abs() <= 0.25 { self.w } else { w };
        let rows = if !first && a.more && !a.long { a.fit.1.max(self.rows) } else { a.fit.1 };
        (self.want, self.most) = ((w, rows), a.max_w);
        let due = !a.more
            || a.fixed
            || first
            || self.w > a.max_w + 0.01
            || self.w + 0.01 < a.floor.min(a.max_w)
            || self.w + 0.01 < a.hold.0.min(a.max_w)
            || self.rows < a.hold.1
            || (a.long && rows != self.rows)
            || self.at.is_some_and(|t| now >= t + STEP);
        if due && (w != self.w || rows != self.rows) {
            (self.w, self.rows) = (w, rows);
            self.at = Some(now);
        }
        (self.w, self.rows)
    }

    /// Its header's need now, and the widest it has needed in this reply: its floor's part.
    pub fn head(&mut self, need: f32) -> f32 {
        self.head = self.head.max(need);
        self.head
    }

    /// The reader's stub at `floor` (its header and keys): never narrower while it is open (a
    /// status, its line count changing under it don't resize it), at most `max_w`. Not drawn as
    /// a reply meanwhile (`away`).
    pub fn stub(&mut self, floor: f32, max_w: f32) -> f32 {
        self.away();
        self.stub = self.stub.max(floor);
        self.stub.min(max_w)
    }

    /// Not drawn as a reply this frame (the stub, a prompt or the text field has the panel, or it
    /// is closed): drawn again, its words' wait starts afresh, however long it was away.
    pub fn away(&mut self) {
        self.away = true;
    }

    /// What it is growing to: its size laid out, or its next step.
    fn target(&self) -> (f32, usize) {
        (self.w.max(self.want.0), self.rows.max(self.want.1))
    }

    /// The box it was drawn in this frame, (width, rows it holds): its words' box, never under
    /// the size it eases to as it narrows (`hud::PanelFx::open`), so only growth leaves it short.
    /// Short of what it is growing to, its wait starts: when it falls short, afresh when what it
    /// grows to grows (a fast stream's next step) or it is drawn again after it was `away`.
    pub fn draw(&mut self, w: f32, rows: usize, now: Instant) {
        let (tw, tr) = self.target();
        let (aw, ar) = self.aim;
        let short = w + 0.01 < tw || rows < tr;
        let back = std::mem::take(&mut self.away) || self.drawn_at.is_none();
        let fresh = back || tw > aw + 0.01 || tr > ar;
        self.short_from = match self.short_from {
            _ if !short => None,
            Some(t) if !back => Some(t),
            _ => Some(now),
        };
        self.short_since = match self.short_since {
            _ if !short => None,
            Some(t) if !fresh => Some(t),
            _ => Some(now),
        };
        (self.aim, self.drawn, self.drawn_at) = ((tw, tr), (w, rows), Some(now));
    }

    /// The rows the box as drawn last frame holds.
    pub fn drawn_rows(&self) -> usize {
        self.drawn.1
    }

    /// Drawn as a reply within `WAIT` (not `away` since).
    pub fn drawing(&self, now: Instant) -> bool {
        !self.away && self.drawn_at.is_some_and(|t| now < t + WAIT)
    }

    /// What needs `need` px (a line with its padding, the footer's keys), at row `row` of the
    /// page, waits: the box as drawn doesn't hold it and is growing to (laid out, or its next
    /// step), short of that for under `WAIT` (and under `WAIT_CAP` since it fell short). A need
    /// wider than the most it may take waits for that much only (then shows, clipped by what it
    /// is over); growth that isn't coming never keeps it waiting.
    pub fn waits(&self, need: f32, row: usize, now: Instant) -> bool {
        let need = need.min(self.most);
        let (w, rows) = self.target();
        let short = need > self.drawn.0 + 0.01 || row >= self.drawn.1;
        short && need <= w + 0.01 && row < rows && self.short_since.is_some_and(|t| now < t + WAIT) && self.short_from.is_some_and(|t| now < t + WAIT_CAP)
    }
}

/// The text field's width: its words a step up (`step_up`), never narrower until they are `HYST`
/// under the step below; its floor never lower while it is open (a hint, ESC AGAIN, the busy note
/// coming and going don't move it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Field {
    w: f32,
    floor: f32,
}

impl Field {
    /// Its width now. `fixed` (a shaped window, no compositor): `max_w` while it is open, never
    /// a step (a width change drops it from the shape for a frame).
    pub fn next(&mut self, fit: f32, floor: f32, max_w: f32, fixed: bool) -> f32 {
        if fixed {
            self.w = max_w;
            return max_w;
        }
        self.floor = self.floor.max(floor);
        let floor = self.floor.min(max_w);
        let up = step_up(fit, floor, max_w);
        let keep = self.w > 0.0 && up < self.w && fit > self.w - STEP_W - HYST;
        self.w = if keep { self.w.clamp(floor, max_w) } else { up };
        self.w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// A frame's ask: nothing shown it doesn't hold, a composited window.
    fn ask(fit: (f32, usize), floor: f32, long: bool, more: bool, max_w: f32) -> Ask {
        Ask { fit, floor, hold: (0.0, 0), long, more, fixed: false, max_w }
    }

    #[test]
    fn the_width_is_the_text_between_its_floor_and_the_most() {
        assert_eq!(width(80.0, 140.0, 470.0), 140.0);
        assert_eq!(width(300.0, 140.0, 470.0), 300.0);
        assert_eq!(width(600.0, 140.0, 470.0), 470.0);
        // A floor wider than her room: the room.
        assert_eq!(width(80.0, 400.0, 360.0), 360.0);
    }

    #[test]
    fn a_step_up_starts_at_the_floor() {
        assert_eq!(step_up(100.0, 140.0, 470.0), 140.0);
        assert_eq!(step_up(140.0, 140.0, 470.0), 140.0);
        assert_eq!(step_up(141.0, 140.0, 470.0), 140.0 + STEP_W);
        assert_eq!(step_up(140.0 + STEP_W, 140.0, 470.0), 140.0 + STEP_W);
        assert_eq!(step_up(460.0, 140.0, 470.0), 470.0);
        assert_eq!(step_up(500.0, 140.0, 470.0), 470.0);
    }

    #[test]
    fn a_footer_needs_its_keys_apart_and_its_left_part_clear_of_them() {
        let f = Foot { inset: 16.0, left: 70.0, keys: vec![80.0, SPACER_W, CHEVRON_W], edge: 11.0 };
        assert_eq!(f.need(), 16.0 + 70.0 + FOOT_AIR + 80.0 + 4.0 + 24.0 + 2.0 * KEY_GAP + 11.0 + SLACK);
        let keys = Foot { inset: 20.0, keys: vec![CHEVRON_W], edge: 11.0, ..Default::default() };
        assert_eq!(keys.need(), 20.0 + 24.0 + 11.0 + SLACK);
        assert_eq!(text_key(40.0), 64.0);
        assert_eq!(read_key(100.0), 100.0 + 28.0 + 11.0 + 7.0);
    }

    /// A reply streams a word every 30 ms, its fit growing; a frame every 16 ms.
    #[test]
    fn a_stream_grows_in_steps_at_most_four_times_a_second_and_never_shrinks() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        let (floor, max_w) = (140.0, 470.0);
        let mut last = (0.0, 0);
        let mut changes: Vec<u64> = Vec::new();
        for ms in (0..3_000u64).step_by(16) {
            let words = (ms / 30) as f32;
            // Its widest line: 9 px a word up to the text box, a row per 40 words.
            let fit = ((floor - 40.0 + 9.0 * words).clamp(floor, max_w), 1 + (words as usize / 40).min(7));
            let now = at(t0, ms);
            let size = g.next(ask(fit, floor, false, true, max_w), now);
            assert!(size.0 >= last.0 && size.1 >= last.1, "{ms}: shrank {last:?} → {size:?}");
            assert!(size.0 <= max_w);
            if size != last {
                changes.push(ms);
            }
            last = size;
        }
        for w in changes.windows(2) {
            assert!(w[1] - w[0] >= STEP.as_millis() as u64 - 16, "{changes:?}");
        }
        assert!(changes.len() >= 3 && changes.len() <= 3 * 4 + 1, "{changes:?}");
        // Caught up a step after its last word, in the floor and whole steps over it (or the most).
        let fit = (max_w - 30.0, 3);
        let size = g.next(ask(fit, floor, false, true, max_w), at(t0, 3_000 + STEP.as_millis() as u64));
        assert!(size.0 >= fit.0 && size.1 == fit.1, "{size:?}");
        assert!(((g.w - floor) / STEP_W).fract().abs() < 1e-3 || g.w == max_w, "{}", g.w);
    }

    #[test]
    fn its_end_settles_once_to_the_fit() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert_eq!(g.next(ask((150.0, 1), 140.0, false, true, 470.0), t0), (140.0 + STEP_W, 1));
        // Ended: its fit at once, even within a step.
        assert_eq!(g.next(ask((152.0, 1), 140.0, false, false, 470.0), at(t0, 20)), (152.0, 1));
        assert_eq!(g.next(ask((152.0, 1), 140.0, false, false, 470.0), at(t0, 40)), (152.0, 1));
    }

    #[test]
    fn a_short_reply_streams_at_its_floor() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        for (ms, w) in [(0u64, 60.0f32), (30, 90.0), (300, 120.0)] {
            assert_eq!(g.next(ask((w.max(140.0), 1), 140.0, false, true, 470.0), at(t0, ms)), (140.0, 1));
        }
        assert_eq!(g.next(ask((140.0, 1), 140.0, false, false, 470.0), at(t0, 330)), (140.0, 1), "nothing to settle");
    }

    #[test]
    fn going_long_takes_its_opening_lines_and_her_room_shrinking_clamps_at_once() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((470.0, 8), 140.0, false, true, 470.0), t0);
        // Long within a step of its last change: its opening lines at once (no empty band).
        assert_eq!(g.next(ask((470.0, 3), 140.0, true, true, 470.0), at(t0, 20)), (470.0, 3));
        // She moved to the screen's edge mid-stream: inside her room now, not a step later.
        assert_eq!(g.next(ask((420.0, 3), 140.0, true, true, 420.0), at(t0, 40)).0, 420.0);
    }

    /// Shady (#143): READ came mid-step, its floor over the box, and showed cut for 250 ms.
    #[test]
    fn a_floor_that_rises_mid_step_applies_at_once() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert_eq!(g.next(ask((200.0, 2), 140.0, false, true, 470.0), t0).0, 140.0 + 2.0 * STEP_W);
        // 20 ms on, READ and the pager: a floor of 300.
        let w = g.next(ask((200.0, 3), 300.0, true, true, 470.0), at(t0, 20)).0;
        assert!(w >= 300.0, "{w}");
    }

    /// All of a long reply at once (`reveal_all_if_long`), or a spoken caption: the text shown
    /// grows the box at once, not a step later.
    #[test]
    fn text_shown_that_the_box_doesnt_hold_grows_it_at_once() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((150.0, 1), 140.0, false, true, 470.0), t0);
        let a = Ask { hold: (460.0, 3), ..ask((470.0, 3), 140.0, false, true, 470.0) };
        assert_eq!(g.next(a, at(t0, 16)), (470.0, 3));
        // Within its box: it waits for the step as before.
        let mut g = Grow::default();
        g.next(ask((150.0, 1), 140.0, false, true, 470.0), t0);
        let a = Ask { hold: (180.0, 1), ..ask((300.0, 2), 140.0, false, true, 470.0) };
        assert_eq!(g.next(a, at(t0, 16)), (140.0 + STEP_W, 1));
    }

    /// No compositor: a shaped window drops the bubble from its shape for a frame on a width
    /// change, so it streams at the most and never narrows within the reply (790ed21).
    #[test]
    fn a_shaped_window_streams_at_the_most_and_never_narrows() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        let fixed = |fit, more| Ask { fixed: true, ..ask(fit, 140.0, false, more, 470.0) };
        assert_eq!(g.next(fixed((150.0, 1), true), t0), (470.0, 1));
        // Its rows grow (upward, in the same column: no drop) at once.
        assert_eq!(g.next(fixed((200.0, 2), true), at(t0, 16)), (470.0, 2));
        // Ended: no settling narrower.
        assert_eq!(g.next(fixed((200.0, 2), false), at(t0, 400)), (470.0, 2));
        // A reply that never streamed: its fit, and it stays.
        let mut g = Grow::default();
        assert_eq!(g.next(fixed((180.0, 1), false), t0), (180.0, 1));
        assert_eq!(g.next(fixed((160.0, 1), false), at(t0, 16)), (180.0, 1));
    }

    /// Shady (#143): "ok" went 117 → 129 px as DONE turned ONLINE under it.
    #[test]
    fn a_settled_reply_keeps_its_widest_header() {
        let mut g = Grow::default();
        assert_eq!(g.head(129.0), 129.0, "SPEAKING");
        assert_eq!(g.head(117.0), 129.0, "DONE");
        assert_eq!(g.head(125.0), 129.0, "ONLINE");
        assert_eq!(g.head(140.0), 140.0, "wider: it grows");
    }

    #[test]
    fn words_wait_for_the_box_as_drawn_only_while_it_grows_to_them() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert!(!g.waits(999.0, 9, t0), "never drawn: nothing waits");
        g.next(ask((200.0, 1), 140.0, false, true, 470.0), t0);
        g.draw(g.w, 1, t0);
        let w = g.w;
        assert!(!g.waits(w, 0, at(t0, 10)), "it holds it");
        // Its next step (due within STEP) will hold a line a little wider: it waits for it.
        g.next(ask((w + 30.0, 2), 140.0, false, true, 470.0), at(t0, 16));
        assert_eq!(g.w, w, "not due yet");
        g.draw(g.w, 1, at(t0, 16));
        assert!(g.waits(w + 20.0, 0, at(t0, 20)), "wider than the box, its next step holds it");
        assert!(g.waits(w, 1, at(t0, 20)), "a row it hasn't, its next step has");
        assert!(!g.waits(w + 200.0, 0, at(t0, 20)), "no step coming holds it: no wait");
        assert!(!g.waits(w, 2, at(t0, 20)), "nor a row none coming has");
        // Grown: its words wait until the box is drawn around them...
        g.next(ask((w + 30.0, 2), 140.0, false, true, 470.0), at(t0, 260));
        assert!(g.w > w);
        g.draw(w + 0.5, 1, at(t0, 280));
        assert!(g.waits(w + 20.0, 0, at(t0, 300)), "drawn short of it");
        g.draw(g.w, 2, at(t0, 460));
        assert!(!g.waits(w + 20.0, 1, at(t0, 460)), "drawn around it");
        // ...and never past WAIT from when it fell short (a box that never gets there: no
        // redraw keeps them waiting).
        g.next(ask((g.w + 60.0, 2), 140.0, false, true, 470.0), at(t0, 720));
        for ms in (720..2_000).step_by(35) {
            g.draw(w, 2, at(t0, ms));
            let waits = g.waits(g.w, 0, at(t0, ms));
            assert_eq!(waits, ms < 720 + WAIT.as_millis() as u64, "{ms}");
        }
    }

    /// Shady (#143): GPUI's shaped width over the wrapper's (kerning, an emoji with VS16): a line
    /// wider than the most the box may take. It waits for the box to reach that, then shows.
    #[test]
    fn a_line_wider_than_the_most_waits_only_for_the_most() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((470.0, 2), 140.0, false, true, 470.0), t0);
        g.draw(300.0, 2, at(t0, 16));
        assert!(g.waits(475.0, 0, at(t0, 20)), "easing out to the most: it waits");
        g.draw(470.0, 2, at(t0, 220));
        assert!(!g.waits(475.0, 0, at(t0, 230)), "at the most: it shows");
        for ms in (230..3_000).step_by(35) {
            g.next(ask((470.0, 2), 140.0, false, true, 470.0), at(t0, ms));
            g.draw(470.0, 2, at(t0, ms));
            assert!(!g.waits(475.0, 1, at(t0, ms)), "{ms}");
        }
    }

    /// Drawn at or over the size it goes to (narrowing: `hud::PanelFx::open` keeps its words' box
    /// there through the ease's overshoot), it is never short; under what it grows to, it is.
    #[test]
    fn only_growth_leaves_the_box_short() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((459.0, 3), 140.0, false, true, 470.0), t0);
        assert_eq!(g.w, 470.0);
        g.draw(470.0, 3, at(t0, 16));
        // Ended: it settles to 459, its words' box 470 → 459 and held there (the shell dips).
        assert_eq!(g.next(ask((459.0, 3), 140.0, false, false, 470.0), at(t0, 400)), (459.0, 3));
        for (ms, w) in [(416, 470.0), (450, 464.0), (500, 459.005), (540, 459.0), (620, 459.0)] {
            g.draw(w, 3, at(t0, ms));
            assert!(!g.waits(458.94, 1, at(t0, ms)) && !g.waits(459.0, 2, at(t0, ms)), "{ms}: drawn {w}");
        }
        // Going long: its rows 8 → 3, held at 3.
        let mut g = Grow::default();
        g.next(ask((470.0, 8), 140.0, false, true, 470.0), t0);
        g.draw(470.0, 8, at(t0, 16));
        g.next(ask((470.0, 3), 140.0, true, true, 470.0), at(t0, 30));
        for (ms, rows) in [(46, 8), (100, 4), (120, 3), (200, 3)] {
            g.draw(470.0, rows, at(t0, ms));
            assert!(!g.waits(400.0, 1, at(t0, ms)) && !g.waits(400.0, 2, at(t0, ms)), "{ms}: {rows} rows");
        }
        // Growing it waits for the box.
        let mut g = Grow::default();
        g.next(ask((300.0, 2), 140.0, false, false, 470.0), t0);
        g.draw(200.0, 2, at(t0, 16));
        assert!(g.waits(280.0, 0, at(t0, 20)));
        // A narrowing that turns into growth: short again.
        g.draw(300.0, 2, at(t0, 220));
        g.next(ask((250.0, 2), 140.0, false, false, 470.0), at(t0, 240));
        g.draw(290.0, 2, at(t0, 256));
        assert!(!g.waits(280.0, 0, at(t0, 260)), "narrowing: it holds it");
        g.next(ask((400.0, 2), 140.0, false, false, 470.0), at(t0, 270));
        g.draw(285.0, 2, at(t0, 286));
        assert!(g.waits(380.0, 0, at(t0, 290)), "grows past where it narrowed from: it waits");
    }

    /// Shady (#143 follow-up): a change under what the box eases by never comes, and kept as what
    /// it grows to, it held a line that needed it for `WAIT`.
    #[test]
    fn a_change_too_small_to_draw_keeps_nothing_waiting() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((300.0, 2), 140.0, false, false, 470.0), t0);
        g.draw(300.0, 2, at(t0, 16));
        assert_eq!(g.next(ask((300.2, 2), 140.0, false, false, 470.0), at(t0, 20)), (300.0, 2));
        assert_eq!(g.target(), (300.0, 2));
        g.draw(300.0, 2, at(t0, 32));
        assert!(!g.waits(300.1, 0, at(t0, 36)));
        // Streaming, the same.
        let mut g = Grow::default();
        g.next(ask((470.0, 2), 140.0, false, true, 470.0), t0);
        g.draw(470.0, 2, at(t0, 16));
        g.next(ask((470.0, 2), 140.0, false, true, 470.2), at(t0, 20));
        g.draw(470.0, 2, at(t0, 32));
        assert!(!g.waits(470.1, 0, at(t0, 36)));
    }

    /// Shady (#143, G1): a fast stream's box is always a step behind what it grows to; its wait
    /// ran out 750 ms in, and lines showed past the box. It starts afresh as that grows.
    #[test]
    fn a_fast_stream_waits_afresh_each_time_what_it_grows_to_grows() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        let mut waited = 0;
        // The box as drawn: easing (linearly, over `STEP` less a frame) to each size laid out.
        let (mut from, mut to, mut since) = (0.0f32, 0.0f32, 0u64);
        for ms in (0..1_500u64).step_by(16) {
            let now = at(t0, ms);
            let fit = 140.0 + 0.3 * ms as f32;
            g.next(ask((fit.min(470.0), 2), 140.0, false, true, 470.0), now);
            if g.w != to {
                (from, to, since) = (from + (to - from) * ((ms - since) as f32 / 234.0).min(1.0), g.w, ms);
            }
            let drawn = from + (to - from) * ((ms - since) as f32 / 234.0).min(1.0);
            g.draw(drawn, 2, now);
            // A line its next step holds, wider than the box as drawn: it waits, however long the
            // stream has kept the box short.
            let (tw, _) = g.target();
            if tw > drawn + 1.0 && tw < 470.0 {
                assert!(g.waits(tw, 0, now), "{ms}: drawn {drawn}, growing to {tw}: shown past the box");
                waited = ms;
            }
        }
        assert!(waited > WAIT.as_millis() as u64, "{waited}");
    }

    /// Shady (#143 follow-up): its words never wait past `WAIT_CAP` from when the box fell short,
    /// however often what it grows to grows.
    #[test]
    fn a_box_kept_short_lets_its_words_go_after_the_cap() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((150.0, 2), 140.0, false, false, 470.0), t0);
        g.draw(150.0, 2, at(t0, 0));
        for ms in (16..3_000u64).step_by(16) {
            // What it grows to grows every frame; the box is drawn where it was.
            g.next(ask((160.0 + 0.1 * ms as f32, 2), 140.0, false, false, 470.0), at(t0, ms));
            g.draw(150.0, 2, at(t0, ms));
            assert_eq!(g.waits(155.0, 0, at(t0, ms)), ms < 16 + WAIT_CAP.as_millis() as u64, "{ms}");
        }
    }

    /// Shady (#143, G2 and its follow-up): behind the reader's stub its wait went stale, and the
    /// lines showed past the box as it grew back. Away (the stub, a prompt, the field), drawn
    /// again its wait starts afresh. Marked, not guessed from time: at 10 fps a frame is no gap.
    #[test]
    fn drawn_again_after_it_was_away_its_wait_starts_afresh() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next(ask((400.0, 3), 140.0, false, false, 470.0), t0);
        g.draw(300.0, 3, at(t0, 16));
        assert!(g.waits(390.0, 0, at(t0, 20)));
        // The reader opened: the stub, no draw for 2 s; it closed, the box easing back.
        assert_eq!(g.stub(180.0, 470.0), 180.0);
        assert!(!g.drawing(at(t0, 30)), "away: its words don't wait for a box that isn't drawn");
        g.next(ask((400.0, 3), 140.0, false, false, 470.0), at(t0, 2_000));
        g.draw(200.0, 3, at(t0, 2_016));
        assert!(g.drawing(at(t0, 2_020)));
        assert!(g.waits(390.0, 0, at(t0, 2_020)), "a stale wait: shown past the box");
        assert!(!g.waits(390.0, 0, at(t0, 2_016 + WAIT.as_millis() as u64)), "and bounded");
        // A prompt or the field took the panel (`away`): the same, however soon it is back.
        g.draw(200.0, 3, at(t0, 3_000));
        g.away();
        g.draw(200.0, 3, at(t0, 3_016));
        assert!(g.waits(390.0, 0, at(t0, 3_020)));
        // At 10 fps, every frame 120 ms apart: one wait, not one a frame.
        let mut g = Grow::default();
        g.next(ask((400.0, 3), 140.0, false, false, 470.0), t0);
        for ms in (0..2_000u64).step_by(120) {
            g.draw(300.0, 3, at(t0, ms));
            assert_eq!(g.waits(390.0, 0, at(t0, ms)), ms < WAIT.as_millis() as u64, "{ms}");
        }
    }

    /// Shady (#143, G3): the stub resized as CLOSE READER's count changed. Never narrower while it
    /// is open; afresh when it opens again.
    #[test]
    fn the_stub_never_narrows_while_it_is_open() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert_eq!(g.stub(240.0, 470.0), 240.0);
        assert_eq!(g.stub(236.0, 470.0), 240.0, "a narrower count");
        assert_eq!(g.stub(250.0, 470.0), 250.0, "a digit more");
        assert_eq!(g.stub(250.0, 230.0), 230.0, "her room: inside it");
        g.next(ask((300.0, 2), 140.0, false, false, 470.0), t0);
        assert_eq!(g.stub(236.0, 470.0), 236.0, "opened again");
    }

    #[test]
    fn the_field_grows_in_steps_and_doesnt_flip_at_a_steps_edge() {
        let mut f = Field::default();
        assert_eq!(f.next(100.0, 140.0, 470.0, false), 140.0);
        let edge = 140.0 + STEP_W;
        assert_eq!(f.next(edge + 1.0, 140.0, 470.0, false), edge + STEP_W, "a letter over: a step up");
        assert_eq!(f.next(edge - 1.0, 140.0, 470.0, false), edge + STEP_W, "deleted back: it stays");
        assert_eq!(f.next(edge + 1.0, 140.0, 470.0, false), edge + STEP_W);
        assert_eq!(f.next(edge - HYST - 1.0, 140.0, 470.0, false), edge, "well under: a step down");
        // A hint widened its floor; it went: the floor stays while the field is open.
        assert_eq!(f.next(100.0, 300.0, 470.0, false), 300.0);
        assert_eq!(f.next(100.0, 140.0, 470.0, false), 300.0);
        // Her room shrank: inside it.
        assert_eq!(f.next(100.0, 140.0, 260.0, false), 260.0);
    }

    /// No compositor: the field at the most while it is open, as a stream (no step drops it from
    /// the window's shape).
    #[test]
    fn a_shaped_window_holds_the_field_at_the_most() {
        let mut f = Field::default();
        for fit in [100.0, 200.0, 300.0, 120.0] {
            assert_eq!(f.next(fit, 140.0, 470.0, true), 470.0, "{fit}");
        }
        assert_eq!(f.next(100.0, 140.0, 420.0, true), 420.0, "her room: inside it");
    }
}
