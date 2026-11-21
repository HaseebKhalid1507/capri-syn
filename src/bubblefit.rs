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
/// New words wait for the box only while it is drawn: within this of its last frame (the view
/// draws it every typing tick meanwhile, and its next change is due within `STEP`). A bubble not
/// drawn (behind the chip) holds its words no longer.
pub const WAIT: Duration = Duration::from_millis(750);
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
    /// The box it was drawn in last frame, (width, rows), and when: its words wait for it, not
    /// for a guess.
    drawn: (f32, usize),
    drawn_at: Option<Instant>,
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
        let first = self.at.is_none();
        let w = match (a.more, a.fixed) {
            (true, true) => a.max_w,
            (true, false) => step_up(a.fit.0, a.floor, a.max_w),
            (false, _) => a.fit.0,
        };
        let w = if !first && (a.more || a.fixed) { w.max(self.w) } else { w }.min(a.max_w);
        let rows = if !first && a.more && !a.long { a.fit.1.max(self.rows) } else { a.fit.1 };
        let due = !a.more
            || a.fixed
            || first
            || self.w > a.max_w + 0.01
            || self.w + 0.01 < a.floor.min(a.max_w)
            || self.w + 0.01 < a.hold.0.min(a.max_w)
            || self.rows < a.hold.1
            || (a.long && rows != self.rows)
            || self.at.is_some_and(|t| now >= t + STEP);
        if due && ((w - self.w).abs() > 0.25 || rows != self.rows) {
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

    /// The box it was drawn in this frame, (width, rows it holds).
    pub fn draw(&mut self, w: f32, rows: usize, now: Instant) {
        self.drawn = (w, rows);
        self.drawn_at = Some(now);
    }

    /// Drawn within `WAIT`: its words may wait for it to grow.
    pub fn drawing(&self, now: Instant) -> bool {
        self.drawn_at.is_some_and(|t| now < t + WAIT)
    }

    /// Words of a line that needs `need` px (its text and padding), at row `row` of the page,
    /// wait: the box it was drawn in doesn't hold it (it is growing to, or its next step will:
    /// its fit holds every line). A bubble not drawn (behind the chip) holds them back no longer
    /// than `WAIT`.
    pub fn holds_back(&self, need: f32, row: usize, now: Instant) -> bool {
        self.drawing(now) && (need > self.drawn.0 + 0.01 || row >= self.drawn.1)
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
    pub fn next(&mut self, fit: f32, floor: f32, max_w: f32) -> f32 {
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
    fn words_wait_for_the_box_as_drawn_only_while_it_is_drawn() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert!(!g.holds_back(999.0, 9, t0), "never drawn: nothing waits");
        g.next(ask((200.0, 1), 140.0, false, true, 470.0), t0);
        g.draw(g.w, 1, t0);
        let w = g.w;
        assert!(!g.holds_back(w, 0, at(t0, 10)));
        assert!(g.holds_back(w + 1.0, 0, at(t0, 10)), "wider than the box");
        assert!(g.holds_back(w, 1, at(t0, 10)), "a row it hasn't");
        assert!(g.holds_back(w + 1.0, 1, at(t0, 260)), "its next step is due: still waiting");
        assert!(!g.holds_back(w + 1.0, 1, at(t0, WAIT.as_millis() as u64)), "not drawn since (behind the chip): it goes on");
        // Grown: its words wait until the box is drawn around them, however long its ease.
        g.next(ask((330.0, 2), 140.0, false, true, 470.0), at(t0, 260));
        g.draw(w + 0.5, 1, at(t0, 280));
        assert!(g.holds_back(w + 1.0, 0, at(t0, 300)), "drawn short of it");
        g.draw(g.w, 2, at(t0, 300));
        assert!(!g.holds_back(w + 1.0, 1, at(t0, 300)), "drawn around it");
    }

    #[test]
    fn the_field_grows_in_steps_and_doesnt_flip_at_a_steps_edge() {
        let mut f = Field::default();
        assert_eq!(f.next(100.0, 140.0, 470.0), 140.0);
        let edge = 140.0 + STEP_W;
        assert_eq!(f.next(edge + 1.0, 140.0, 470.0), edge + STEP_W, "a letter over: a step up");
        assert_eq!(f.next(edge - 1.0, 140.0, 470.0), edge + STEP_W, "deleted back: it stays");
        assert_eq!(f.next(edge + 1.0, 140.0, 470.0), edge + STEP_W);
        assert_eq!(f.next(edge - HYST - 1.0, 140.0, 470.0), edge, "well under: a step down");
        // A hint widened its floor; it went: the floor stays while the field is open.
        assert_eq!(f.next(100.0, 300.0, 470.0), 300.0);
        assert_eq!(f.next(100.0, 140.0, 470.0), 300.0);
        // Her room shrank: inside it.
        assert_eq!(f.next(100.0, 140.0, 260.0), 260.0);
    }
}
