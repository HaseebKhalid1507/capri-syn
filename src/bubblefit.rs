//! The speech bubble's size (logical px): as wide as its widest line and as tall as its lines,
//! between a floor (what its header and every key in its footer need, whole) and the most it may
//! take where she is. A streaming reply grows in steps, at most once per `STEP`, and never
//! shrinks while more may come; when its turn ends it settles once to its fit. Pure: the view
//! lays out what this decides, and the tests run the same code.

use std::time::{Duration, Instant};

/// A streaming bubble's width grows in steps of this over its floor...
pub const STEP_W: f32 = 48.0;
/// ...and its size changes at most this often (`hud::SIZE_MS` and a little: one ease ends before
/// the next starts).
pub const STEP: Duration = Duration::from_millis(250);
/// After a change, new words wait this long before going where only the new size holds them: a
/// growing box leads its new words (`hud::GROW_LEAD_MS`).
pub const LEAD: Duration = Duration::from_millis(90);
/// The longest new words wait for the box after its last change: its next is due after `STEP`
/// (the view lays it out every frame meanwhile); a bubble not laid out (behind the chip) holds
/// its words no longer than this.
pub const WAIT: Duration = Duration::from_millis(750);

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

/// A bubble's size as laid out: its width and its rows. A streaming reply's grows in steps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grow {
    pub w: f32,
    pub rows: usize,
    /// The size before the last change, and when that was (None: never laid out).
    was: (f32, usize),
    at: Option<Instant>,
}

impl Grow {
    /// The size to lay out now, `fit` (width, rows) its exact fit this frame and `floor` the
    /// width's. Streaming (`more`): the fit's width a step up, never less than it was, at most
    /// `max_w`; its rows never fewer unless it went long (`long`: its opening lines and READ);
    /// a change at most once per `STEP` (the first at once, and at once back inside `max_w`
    /// when her room shrinks). Not streaming: the fit (an ended stream settles once).
    pub fn next(&mut self, fit: (f32, usize), floor: f32, long: bool, more: bool, max_w: f32, now: Instant) -> (f32, usize) {
        let first = self.at.is_none();
        let want = if !more {
            fit
        } else {
            let w = step_up(fit.0, floor, max_w);
            if first { (w, fit.1) } else { (w.max(self.w).min(max_w), if long { fit.1 } else { fit.1.max(self.rows) }) }
        };
        let due = !more || first || self.w > max_w + 0.01 || self.at.is_some_and(|t| now >= t + STEP);
        if due && ((want.0 - self.w).abs() > 0.25 || want.1 != self.rows) {
            // Its first size has nothing to lead: the bubble opens around its words (`hud`).
            self.was = if first { want } else { (self.w, self.rows) };
            (self.w, self.rows) = want;
            self.at = Some(now);
        }
        (self.w, self.rows)
    }

    /// The box new words may go into now: the new size once it has led them by `LEAD`, until then
    /// what both sizes hold.
    pub fn ready(&self, now: Instant) -> (f32, usize) {
        match self.at {
            Some(t) if now < t + LEAD => (self.w.min(self.was.0), self.rows.min(self.was.1)),
            _ => (self.w, self.rows),
        }
    }

    /// Within `WAIT` of its last change: words may wait for the next.
    pub fn changing(&self, now: Instant) -> bool {
        self.at.is_some_and(|t| now < t + WAIT)
    }

    /// Words of a line that needs `need` px (its text and padding), at row `row` of the page,
    /// wait: the box ready now doesn't hold it, and it changed within `WAIT` (its next change is
    /// due; a bubble not laid out, behind the chip, holds its words back no longer).
    pub fn holds_back(&self, need: f32, row: usize, now: Instant) -> bool {
        if !self.changing(now) {
            return false;
        }
        let (w, rows) = self.ready(now);
        need > w + 0.01 || row >= rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
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
            let size = g.next(fit, floor, false, true, max_w, now);
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
        let size = g.next(fit, floor, false, true, max_w, at(t0, 3_000 + STEP.as_millis() as u64));
        assert!(size.0 >= fit.0 && size.1 == fit.1, "{size:?}");
        assert!(((g.w - floor) / STEP_W).fract().abs() < 1e-3 || g.w == max_w, "{}", g.w);
    }

    #[test]
    fn its_end_settles_once_to_the_fit() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert_eq!(g.next((150.0, 1), 140.0, false, true, 470.0, t0), (140.0 + STEP_W, 1));
        // Ended: its fit at once, even within a step.
        assert_eq!(g.next((152.0, 1), 140.0, false, false, 470.0, at(t0, 20)), (152.0, 1));
        assert_eq!(g.next((152.0, 1), 140.0, false, false, 470.0, at(t0, 40)), (152.0, 1));
    }

    #[test]
    fn a_short_reply_streams_at_its_floor() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        for (ms, w) in [(0u64, 60.0f32), (30, 90.0), (300, 120.0)] {
            assert_eq!(g.next((w.max(140.0), 1), 140.0, false, true, 470.0, at(t0, ms)), (140.0, 1));
        }
        assert_eq!(g.next((140.0, 1), 140.0, false, false, 470.0, at(t0, 330)), (140.0, 1), "nothing to settle");
    }

    #[test]
    fn going_long_takes_its_opening_lines_and_her_room_shrinking_clamps_at_once() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        g.next((470.0, 8), 140.0, false, true, 470.0, t0);
        assert_eq!(g.next((470.0, 3), 140.0, true, true, 470.0, at(t0, 300)), (470.0, 3));
        // She moved to the screen's edge mid-stream: inside her room now, not a step later.
        assert_eq!(g.next((420.0, 3), 140.0, true, true, 420.0, at(t0, 310)).0, 420.0);
    }

    #[test]
    fn words_wait_for_the_box_only_while_a_change_is_due() {
        let t0 = Instant::now();
        let mut g = Grow::default();
        assert!(!g.holds_back(999.0, 9, t0), "never laid out: nothing waits");
        g.next((200.0, 1), 140.0, false, true, 470.0, t0);
        let w = g.w;
        assert!(!g.holds_back(w, 0, at(t0, 10)));
        assert!(g.holds_back(w + 1.0, 0, at(t0, 10)), "wider than the box");
        assert!(g.holds_back(w, 1, at(t0, 10)), "a row it hasn't");
        assert!(g.holds_back(w + 1.0, 1, at(t0, 260)), "its change is due: still waiting");
        assert!(!g.holds_back(w + 1.0, 1, at(t0, WAIT.as_millis() as u64)), "no change came: it goes on");
        // Grown: the new size leads its words by LEAD.
        g.next((330.0, 2), 140.0, false, true, 470.0, at(t0, 260));
        assert!(g.holds_back(w + 1.0, 0, at(t0, 300)));
        assert!(!g.holds_back(w + 1.0, 1, at(t0, 260 + LEAD.as_millis() as u64)));
    }
}
