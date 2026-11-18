//! Her input region and her moves kept apart where the compositor takes a change of her
//! window's shape for a resize (`Os::settling`: picom). picom takes the X events of a frame
//! together: a ShapeNotify (of any kind, her input region's too) marks her window's pixmap
//! stale, and a move in the same frame starts its geometry animation from a snapshot of her
//! pixels from before the change (her facing the old way after a turn), blended in until she
//! holds still. A region change and a move a frame or more apart never meet.
//!
//! Under picom (`on`; elsewhere everything goes out at once):
//! - a region set while she moves waits until she has held still for `SETTLE`, a newer one
//!   replacing it; but the region out may be wrong for her (`mismatch` over `MISS`: clicks go
//!   through her or are eaten next to her: a fling's flight, a turn mid-walk, a bubble closed)
//!   for no longer than `MAX_HOLD` in all: then her moves stop until the newest is out, `GAP`
//!   after the last of them (~70 ms). Her walk cycle alone stays under `MISS`, so a walk
//!   doesn't stop for it;
//! - a move waits out `GAP` after a region went out;
//! - while her drag is in hand (`press`: the button down on her, an implicit grab: every
//!   pointer event is hers whatever her region; or a scripted drag, standing in for one),
//!   regions wait for the release and moves never wait: a drag never stops for her region;
//! - a landing (`landed`: a teleport's, a placed move) never waits.
//!
//! So, outside landings and drags in hand, a region and a move never go out within `GAP` of
//! each other. Time comes in as `now`: the rules are tested on a clock of their own.

use super::ShapeRect;
use std::time::{Duration, Instant};

/// A region set while she moves goes out once she has held still this long.
pub const SETTLE: Duration = Duration::from_millis(60);
/// ... or once the region out has been wrong for her this long in all (`MISS`): her moves
/// then stop for the newest.
pub const MAX_HOLD: Duration = Duration::from_millis(250);
/// How wrong the region out may be for her (`mismatch`) before it counts towards `MAX_HOLD`.
/// Her regions with nothing held (no compositor), against the region 300 ms before: her walk
/// cycle p50 21 %, p90 25 % (one-way, through her only: 20 %, 25 %); a walk against the
/// stand it started from 44 %; a fling's flight up to 66 %. Replayed through this gate with a
/// move every frame: one stop as a walk sets off facing the other way, none in a walk after,
/// one a fling (the one-way miss starting over: none, its flight region 330 ms late).
pub const MISS: f32 = 0.3;
/// A region and a move never go out closer than this: two frames at 60 Hz, so picom takes
/// them in different frames. picom's own trace log under his config (xrender, NVIDIA path,
/// 20 clips): of 4,675 back-to-back frames that took a move or a region of hers, the longest
/// gap was 31 ms, and no frame took both.
pub const GAP: Duration = Duration::from_millis(34);

/// What is held back, and when each kind last went out.
#[derive(Default)]
pub(super) struct Settle {
    moved: Option<Instant>,
    shaped: Option<Instant>,
    /// The newest region held.
    input: Option<Vec<ShapeRect>>,
    /// The region out (the last sent).
    out: Vec<ShapeRect>,
    /// How long the region out has been wrong for her since it went out (`MISS`; while her
    /// drag is in hand it doesn't count) ...
    wrong: Duration,
    /// ... and since when it is wrong now (the newest held misses), not yet in `wrong`.
    wrong_from: Option<Instant>,
    /// The newest region held is wrong for her against the region out.
    held_wrong: bool,
    /// The newest position held.
    pos: Option<(i32, i32)>,
    /// The held region is overdue (`MAX_HOLD`): moves wait until it is out.
    flush: bool,
    /// Her drag is in hand (`press`).
    pressed: bool,
}

impl Settle {
    /// How long the region out has been wrong for her by `now`.
    fn wrong_for(&self, now: Instant) -> Duration {
        self.wrong + self.wrong_from.map_or(Duration::ZERO, |t| now.saturating_duration_since(t))
    }

    /// The time it has been wrong so far into `wrong`; from `now` on it counts if `counts`.
    fn recount(&mut self, counts: bool, now: Instant) {
        self.wrong = self.wrong_for(now);
        self.wrong_from = counts.then_some(now);
    }

    fn overdue(&mut self, now: Instant) {
        if !self.pressed && self.input.is_some() && self.wrong_for(now) >= MAX_HOLD {
            self.flush = true;
        }
    }

    fn sent(&mut self, r: Vec<ShapeRect>, now: Instant) {
        self.out = r;
        self.right();
        self.shaped = Some(now);
    }

    /// The region out is what she wants: nothing is held, nothing counts.
    fn right(&mut self) {
        self.input = None;
        self.wrong = Duration::ZERO;
        self.wrong_from = None;
        self.held_wrong = false;
        self.flush = false;
    }

    fn move_waits(&self, now: Instant) -> bool {
        !self.pressed && (self.flush || self.shaped.is_some_and(|t| now < t + GAP))
    }

    fn region_may_go(&self, now: Instant) -> bool {
        if self.pressed {
            false
        } else if self.flush {
            self.moved.is_none_or(|t| now >= t + GAP)
        } else {
            self.pos.is_none() && self.moved.is_none_or(|t| now >= t + SETTLE)
        }
    }

    /// A move to `p`: true if it goes out now, else it is held (`take_due`).
    pub fn wants_move(&mut self, on: bool, p: (i32, i32), now: Instant) -> bool {
        if on {
            self.overdue(now);
            if self.move_waits(now) {
                self.pos = Some(p);
                return false;
            }
        }
        self.landed(now);
        true
    }

    /// A move went out, held or not (a landing never waits).
    pub fn landed(&mut self, now: Instant) {
        self.pos = None;
        self.moved = Some(now);
    }

    /// Her input region set to `r`: true if it goes out now, else it is held (`take_due`) or
    /// already out.
    pub fn wants_region(&mut self, on: bool, r: &[ShapeRect], now: Instant) -> bool {
        if on {
            if r == self.out {
                // Back to the region out: nothing to send.
                self.right();
                return false;
            }
            // Wrong time adds up from region to region: a frame that happens to fit doesn't
            // start it over (a tumble, a mirrored walk cycle); only a region out ends it.
            self.held_wrong = mismatch(&self.out, r) > MISS;
            self.recount(self.held_wrong && !self.pressed, now);
            self.input = Some(r.to_vec());
            self.overdue(now);
            if !self.region_may_go(now) {
                return false;
            }
        }
        self.sent(r.to_vec(), now);
        true
    }

    /// Her drag is in hand (`down`) or no longer: the button down on her or up, a scripted
    /// drag held or let go. In hand, regions wait for the release and moves never wait; the
    /// time her region out is wrong doesn't count (no click goes by it).
    pub fn press(&mut self, down: bool, now: Instant) {
        if down == self.pressed {
            return;
        }
        self.pressed = down;
        self.flush = false;
        self.recount(!down && self.held_wrong, now);
    }

    /// When what is held is due out (`take_due`), if anything is.
    pub fn due(&self, on: bool, now: Instant) -> Option<Instant> {
        if !on {
            return (self.pos.is_some() || self.input.is_some()).then_some(now);
        }
        let mut due: Option<Instant> = None;
        let mut at = |t: Instant| due = Some(due.map_or(t, |d| d.min(t)));
        if self.pos.is_some() {
            if self.pressed {
                at(now);
            } else if !self.flush {
                at(self.shaped.map_or(now, |t| t + GAP));
            }
        }
        if self.input.is_some() && !self.pressed {
            if self.flush {
                at(self.moved.map_or(now, |t| t + GAP));
            } else {
                if self.pos.is_none() {
                    at(self.moved.map_or(now, |t| t + SETTLE));
                }
                if let Some(t) = self.wrong_from {
                    at(t + MAX_HOLD.saturating_sub(self.wrong));
                }
            }
        }
        due
    }

    /// What is held and due out now: a move or a region, never both under picom (the other
    /// then waits for the one that went).
    pub fn take_due(&mut self, on: bool, now: Instant) -> (Option<(i32, i32)>, Option<Vec<ShapeRect>>) {
        if !on {
            let (p, r) = (self.pos.take(), self.input.clone());
            if p.is_some() {
                self.moved = Some(now);
            }
            if let Some(r) = &r {
                self.sent(r.clone(), now);
            }
            self.flush = false;
            return (p, r);
        }
        self.overdue(now);
        if let Some(r) = self.input.clone().filter(|_| self.region_may_go(now)) {
            self.sent(r.clone(), now);
            return (None, Some(r));
        }
        if self.pos.is_some() && !self.move_waits(now) {
            self.moved = Some(now);
            return (self.pos.take(), None);
        }
        (None, None)
    }
}

/// How wrong region `out` is for region `new` (px), 0 to 1: the larger of the share of `new`
/// it misses (clicks on her go through) and the share of it outside `new` (clicks next to her
/// are eaten: a bubble or a menu closed, her flight over). An empty `new` against a non-empty
/// `out` (hidden) is 1; two empty ones 0. Exact, on bitsets over both regions' box (rects may
/// overlap).
fn mismatch(out: &[ShapeRect], new: &[ShapeRect]) -> f32 {
    let span = |r: &ShapeRect| (i32::from(r.0), i32::from(r.1), i32::from(r.0) + i32::from(r.2), i32::from(r.1) + i32::from(r.3));
    let Some((x0, y0, x1, y1)) = out.iter().chain(new).map(span).filter(|s| s.2 > s.0 && s.3 > s.1).reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))) else {
        return 0.0;
    };
    let words = ((x1 - x0) as usize).div_ceil(64);
    let rows = (y1 - y0) as usize;
    let paint = |rects: &[ShapeRect]| {
        let mut m = vec![0u64; words * rows];
        for (a, b, c, d) in rects.iter().map(span) {
            let (a, c) = (a.max(x0) - x0, c.min(x1) - x0);
            if a >= c {
                continue;
            }
            for y in b.max(y0)..d.min(y1) {
                let row = &mut m[(y - y0) as usize * words..][..words];
                for (i, w) in row.iter_mut().enumerate() {
                    let (lo, hi) = (a.max(64 * i as i32), c.min(64 * i as i32 + 64));
                    if lo < hi {
                        let n = (hi - lo) as u32;
                        *w |= (if n == 64 { u64::MAX } else { (1u64 << n) - 1 }) << (lo - 64 * i as i32);
                    }
                }
            }
        }
        m
    };
    let (o, n) = (paint(out), paint(new));
    let count = |m: &[u64]| m.iter().map(|w| w.count_ones()).sum::<u32>();
    let (all_o, all_n) = (count(&o), count(&n));
    let hit: u32 = n.iter().zip(&o).map(|(a, b)| (a & b).count_ones()).sum();
    let share = |all: u32| if all == 0 { 0.0 } else { 1.0 - hit as f32 / all as f32 };
    if all_n == 0 && all_o > 0 { 1.0 } else { share(all_n).max(share(all_o)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    /// One run of her on a clock of its own: what she asks for each frame, what went out when,
    /// the held work sent at each frame's start and at each wake it asked for (as the view does).
    struct Run {
        s: Settle,
        t0: Instant,
        now: Instant,
        /// (when, x) of each move out; (when, region) of each region out.
        moves: Vec<(Duration, i32)>,
        regions: Vec<(Duration, Vec<ShapeRect>)>,
        landings: Vec<Duration>,
        /// Where her window is, and the region that is out.
        at: i32,
        out: Vec<ShapeRect>,
        /// (when, region) of each region she asked for.
        wanted: Vec<(Duration, Vec<ShapeRect>)>,
    }

    impl Run {
        fn new() -> Run {
            let t0 = Instant::now();
            Run { s: Settle::default(), t0, now: t0, moves: vec![], regions: vec![], landings: vec![], at: 0, out: vec![], wanted: vec![] }
        }
        fn t(&self) -> Duration {
            self.now - self.t0
        }
        fn sent(&mut self, (p, r): (Option<(i32, i32)>, Option<Vec<ShapeRect>>)) {
            if let Some((x, _)) = p {
                self.at = x;
                self.moves.push((self.t(), x));
            }
            if let Some(r) = r {
                self.out = r.clone();
                self.regions.push((self.t(), r));
            }
        }
        /// Let `d` pass, waking whenever the held work asks to.
        fn wait(&mut self, d: Duration) {
            let end = self.now + d;
            while let Some(t) = self.s.due(true, self.now).filter(|&t| t <= end) {
                self.now = t.max(self.now);
                let got = self.s.take_due(true, self.now);
                // A wake that only made the region overdue: the next one is later.
                if got == (None, None) && self.s.due(true, self.now).is_none_or(|n| n <= self.now) {
                    break;
                }
                self.sent(got);
            }
            self.now = end;
        }
        /// One frame: held work first (the view's tick), then a move to `x` and region `r`.
        fn frame(&mut self, x: Option<i32>, r: Option<ShapeRect>) {
            let got = self.s.take_due(true, self.now);
            self.sent(got);
            if let Some(x) = x
                && self.s.wants_move(true, (x, 0), self.now)
            {
                self.sent((Some((x, 0)), None));
            }
            if let Some(r) = r {
                self.wanted.push((self.t(), vec![r]));
                if self.s.wants_region(true, &[r], self.now) {
                    self.sent((None, Some(vec![r])));
                }
            }
            self.wait(FRAME);
        }
        fn land(&mut self, x: i32) {
            self.s.landed(self.now);
            self.at = x;
            self.landings.push(self.t());
            self.moves.push((self.t(), x));
        }
        /// No region and move went out within `GAP` of each other, landings aside.
        fn never_met(&self) {
            for (tr, _) in &self.regions {
                for (tm, _) in self.moves.iter().filter(|(t, _)| !self.landings.contains(t)) {
                    let d = if tr > tm { *tr - *tm } else { *tm - *tr };
                    assert!(d >= GAP, "a region at {tr:?} and a move at {tm:?}");
                }
            }
        }
        /// The longest a region set in [from, to) waited, and the longest she stood still
        /// (between two moves out) in it.
        fn longest_still(&self, from: Duration, to: Duration) -> Duration {
            self.moves.windows(2).filter(|w| w[0].0 >= from && w[1].0 < to).map(|w| w[1].0 - w[0].0).max().unwrap_or_default()
        }
        /// From `from` on, the most time any one region out was wrong for her (`mismatch` of
        /// it against the newest she asked for over `MISS`: a click there went through her or
        /// was eaten), ms by ms.
        fn worst_wrong(&self, from: Duration) -> Duration {
            let last = |v: &[(Duration, Vec<ShapeRect>)], t: Duration| v.iter().rposition(|(at, _)| *at <= t);
            let (mut worst, mut cur, mut epoch, mut memo) = (Duration::ZERO, Duration::ZERO, None, None);
            let mut t = from;
            while t < self.t() {
                let (o, w) = (last(&self.regions, t), last(&self.wanted, t));
                if o != epoch {
                    (epoch, cur) = (o, Duration::ZERO);
                }
                if memo.is_none_or(|(k, _)| k != (o, w)) {
                    let out = o.map_or(&[][..], |i| &self.regions[i].1[..]);
                    let bad = w.is_some_and(|i| mismatch(out, &self.wanted[i].1) > MISS);
                    memo = Some(((o, w), bad));
                }
                if memo.is_some_and(|(_, bad)| bad) {
                    cur += Duration::from_millis(1);
                    worst = worst.max(cur);
                }
                t += Duration::from_millis(1);
            }
            worst
        }
    }

    /// No region out was wrong for her longer than `MAX_HOLD` and the stop it takes.
    const WRONG_MAX: Duration = Duration::from_millis(250 + 2 * 34 + 2 * 16);

    /// Her region for frame `i` of a walk facing right (`dir` 1) or left: her body bobbing a
    /// px, and a leg through four places (her walk cycle: never quite the region out, never
    /// more than ~15 % off it).
    fn walk_region(i: i32, dir: i32) -> Vec<ShapeRect> {
        let x = if dir > 0 { 60 } else { 10 };
        let bob = ((i / 5) % 2) as i16;
        vec![(x, 20 + bob, 30, 40), (x + 7 * ((i / 6) % 4) as i16, 60 + bob, 10, 20)]
    }

    /// One frame of her walk: a move and her region.
    fn step(r: &mut Run, x: i32, region: &[ShapeRect]) {
        let got = r.s.take_due(true, r.now);
        r.sent(got);
        if r.s.wants_move(true, (x, 0), r.now) {
            r.sent((Some((x, 0)), None));
        }
        r.wanted.push((r.t(), region.to_vec()));
        if r.s.wants_region(true, region, r.now) {
            r.sent((None, Some(region.to_vec())));
        }
        r.wait(FRAME);
    }

    /// She stands (her region out), then walks.
    fn standing(region: &[ShapeRect]) -> Run {
        let mut r = Run::new();
        r.wanted.push((r.t(), region.to_vec()));
        assert!(r.s.wants_region(true, region, r.now));
        r.sent((None, Some(region.to_vec())));
        r.wait(SETTLE);
        r
    }

    #[test]
    fn mismatch_counts_clicks_through_her_and_clicks_eaten_next_to_her() {
        assert_eq!(mismatch(&[(0, 0, 10, 10)], &[(0, 0, 10, 10)]), 0.0);
        assert_eq!(mismatch(&[(0, 0, 10, 10)], &[(5, 0, 10, 10)]), 0.5);
        // Nothing out where she now is: every click goes through her.
        assert_eq!(mismatch(&[], &[(5, 0, 10, 10)]), 1.0);
        // Hidden (no region), the old one still out: every click on it is eaten.
        assert_eq!(mismatch(&[(0, 0, 10, 10)], &[]), 1.0);
        assert_eq!(mismatch(&[], &[]), 0.0);
        // A bubble or a menu closed, her flight over: the new region is all inside the old
        // (nothing goes through her), the rest of the old one eats clicks.
        assert_eq!(mismatch(&[(0, 0, 10, 10), (10, 0, 30, 10)], &[(0, 0, 10, 10)]), 0.75);
        assert_eq!(mismatch(&[(0, 0, 40, 40)], &[(10, 10, 20, 20)]), 0.75);
        // ... and the other way round: grown.
        assert_eq!(mismatch(&[(10, 10, 20, 20)], &[(0, 0, 40, 40)]), 0.75);
        // Across 64-px words, overlapping rects counted once, negative coords.
        assert!((mismatch(&[(0, 0, 100, 1)], &[(30, 0, 100, 1)]) - 0.3).abs() < 1e-6);
        assert_eq!(mismatch(&[(0, 0, 200, 2)], &[(10, 0, 50, 2), (20, 0, 50, 2)]), 0.7);
        assert_eq!(mismatch(&[(10, 0, 50, 2), (20, 0, 50, 2)], &[(10, 0, 60, 2)]), 0.0);
        assert_eq!(mismatch(&[(-20, -5, 10, 10)], &[(-20, -5, 20, 10)]), 0.5);
    }

    /// A long walk (3 s, a move every frame, her walk cycle reshaping her each frame): her
    /// region out stays within `MISS` of her, so she never stops for it.
    #[test]
    fn a_long_walk_never_stops_for_her_walk_cycle() {
        let mut r = standing(&walk_region(0, 1));
        let from = r.t();
        for i in 0..190 {
            step(&mut r, i * 2, &walk_region(i, 1));
        }
        r.never_met();
        assert_eq!(r.regions.len(), 1, "no region forced out");
        let still = r.longest_still(from, r.t());
        assert!(still <= FRAME + Duration::from_millis(1), "she stopped {still:?}");
    }

    /// A walk that changes her pose (a region missing more than `MISS` of her): out within
    /// `MAX_HOLD` and one short stop, no region meeting a move.
    #[test]
    fn a_pose_change_mid_walk_goes_out_with_one_short_stop() {
        let mut r = standing(&walk_region(0, 1));
        for i in 0..60 {
            step(&mut r, i * 2, &walk_region(i, 1));
        }
        let change = r.t();
        let crouch = vec![(50, 50, 50, 30)];
        let mut out_at = None;
        for i in 60..150 {
            step(&mut r, i * 2, &crouch);
            if out_at.is_none() && r.out == crouch {
                out_at = Some(r.t() - change);
            }
        }
        r.never_met();
        let o = out_at.expect("her new pose never went out");
        assert!(o <= MAX_HOLD + 2 * GAP + 2 * FRAME, "out {o:?} after the change");
        let still = r.longest_still(change, r.t());
        assert!(still <= 2 * GAP + 2 * FRAME, "she stopped {still:?} for it");
        assert_eq!(r.regions.len(), 2, "one forced region");
    }

    /// He flings her: the flight region (set as she leaves his hand, then each frame of the
    /// tumble) is out within `MAX_HOLD` and a stop, so his press mid-air finds her.
    #[test]
    fn a_fling_gets_its_flight_region_out_for_the_catch() {
        let mut r = standing(&[(0, 0, 50, 50)]);
        r.s.press(true, r.now);
        for i in 0..30 {
            // The drag: moves never wait, regions wait for the release.
            let n = r.moves.len();
            step(&mut r, i * 3, &[(0, 0, 50, 50 + (i % 2) as u16)]);
            assert_eq!(r.moves.len(), n + 1, "a drag move waited");
        }
        assert_eq!(r.regions.len(), 1, "no region out while pressed");
        r.s.press(false, r.now);
        let release = r.t();
        let mut caught_from = None;
        for i in 0..60 {
            step(&mut r, 90 + i * 8, &[((i % 3) as i16, 0, 80, 120)]);
            if caught_from.is_none() && r.out.first().is_some_and(|o| o.2 == 80) {
                caught_from = Some(r.t() - release);
            }
        }
        r.never_met();
        let c = caught_from.expect("the flight region never went out");
        assert!(c <= MAX_HOLD + 2 * GAP + 2 * FRAME, "his catch could land only {c:?} into the flight");
    }

    /// A turn mid-walk: the region facing the new way is out within `MAX_HOLD` and a stop,
    /// while she keeps walking.
    #[test]
    fn a_turn_mid_walk_gets_her_new_region_out() {
        let mut r = standing(&walk_region(0, 1));
        for i in 0..40 {
            step(&mut r, i * 2, &walk_region(i, 1));
        }
        assert_eq!(r.regions.len(), 1);
        let turn = r.t();
        let mut faced = None;
        for i in 40..100 {
            step(&mut r, 80 - (i - 40) * 2, &walk_region(i, -1));
            if faced.is_none() && r.out.first().is_some_and(|o| o.0 == 10) {
                faced = Some(r.t() - turn);
            }
        }
        r.never_met();
        let f = faced.expect("her region never turned");
        assert!(f <= MAX_HOLD + 2 * GAP + 2 * FRAME, "her region turned {f:?} after her");
    }

    /// Shady's review of #69: one frame that happened to fit started `MAX_HOLD` over. A
    /// tumble passes near upright now and then (here every fourth frame fits the region out),
    /// so the flight region never went out and his press mid-air went through her: now the
    /// time it is wrong adds up, and the region out is never wrong for longer than the cap.
    #[test]
    fn a_tumbling_fling_that_fits_now_and_then_still_gets_its_region_out() {
        let stand = [(0, 0, 50, 100)];
        let mut r = standing(&stand);
        let from = r.t();
        for i in 0..90 {
            let region = if i % 4 == 0 { vec![(2 + (i % 3) as i16, 0, 50, 100)] } else { vec![(-20, 25 + (i % 7) as i16, 100, 50)] };
            step(&mut r, i * 9, &region);
        }
        r.never_met();
        assert!(r.regions.len() >= 2, "the flight region never went out");
        let w = r.worst_wrong(from);
        assert!(w <= WRONG_MAX, "a region out was wrong for her {w:?}");
    }

    /// A turn mid-walk whose mirrored walk cycle fits the region out now and then (her hair
    /// swings in front of her for 12 frames of 16): her region still turns within the cap.
    #[test]
    fn a_mirrored_walk_cycle_that_fits_now_and_then_still_turns_her_region() {
        // Torso, a leg on the side she faces, her hair behind her for three steps of four.
        let walk = |i: i32, dir: i32| -> Vec<ShapeRect> {
            let leg = 5 * ((i / 4) % 4) as i16;
            let mut v = vec![(40, 20, 30, 40), if dir > 0 { (60 + leg, 60, 10, 20) } else { (30 - leg, 60, 10, 20) }];
            if dir < 0 && (i / 4) % 4 != 3 {
                v.push((0, 10, 40, 20));
            }
            v
        };
        assert!(mismatch(&walk(0, 1), &walk(12, -1)) < MISS && mismatch(&walk(0, 1), &walk(0, -1)) > MISS);
        let mut r = standing(&walk(0, 1));
        for i in 0..30 {
            step(&mut r, i * 2, &walk(i, 1));
        }
        let turn = r.t();
        let mut faced = None;
        for i in 0..80 {
            step(&mut r, 60 - i * 2, &walk(i, -1));
            if faced.is_none() && r.out.len() == 3 {
                faced = Some(r.t() - turn);
            }
        }
        r.never_met();
        assert!(faced.is_some(), "her region never turned");
        let w = r.worst_wrong(turn);
        assert!(w <= WRONG_MAX, "a region out was wrong for her {w:?}");
    }

    /// Shady's review: the gate saw only clicks going through her, not clicks eaten. Each of
    /// these leaves her new region inside the old one: hidden mid-move (no region), a bubble
    /// or her menu closed while she walks, her flight over as she slides on landing. The old
    /// region goes within the cap.
    #[test]
    fn a_region_that_shrinks_while_she_moves_goes_out() {
        let body = |i: i32| walk_region(i, 1);
        let with = move |i: i32, extra: ShapeRect| {
            let mut v = body(i);
            v.push(extra);
            v
        };
        type Pose = Box<dyn Fn(i32) -> Vec<ShapeRect>>;
        let cases: [(&str, Pose, Pose); 3] = [
            ("hidden", Box::new(body), Box::new(|_| vec![])),
            ("bubble closed", Box::new(move |i| with(i, (60, -40, 80, 50))), Box::new(body)),
            ("flight over", Box::new(|i| vec![(0, (i % 3) as i16, 200, 200)]), Box::new(|_| vec![(50, 100, 50, 100)])),
        ];
        for (what, before, after) in cases {
            let mut r = standing(&before(0));
            for i in 0..20 {
                step(&mut r, i * 2, &before(i));
            }
            let change = r.t();
            let n = r.regions.len();
            for i in 20..80 {
                step(&mut r, i * 2, &after(i));
            }
            r.never_met();
            assert!(r.regions.len() > n, "{what}: the smaller region never went out");
            let w = r.worst_wrong(change);
            assert!(w <= WRONG_MAX, "{what}: a region out was wrong for her {w:?}");
        }
    }

    /// A drag that pauses before it starts and mid-way (the press squash and her dangle change
    /// her region while she is still): no drag move waits.
    #[test]
    fn a_drag_never_waits_for_a_region() {
        let mut r = Run::new();
        // Still before the press: her idle region is out, the last one just before it.
        r.frame(None, Some((1, 1, 1, 1)));
        r.wait(SETTLE * 2);
        assert!(r.s.wants_region(true, &[(1, 1, 1, 2)], r.now));
        r.sent((None, Some(vec![(1, 1, 1, 2)])));
        r.now += Duration::from_millis(2);
        r.s.press(true, r.now);
        assert!(r.s.wants_move(true, (-1, 0), r.now), "in hand, a move never waits, even right after a region");
        r.sent((Some((-1, 0)), None));
        // The squash, then a pause before the drag.
        for _ in 0..10 {
            r.frame(None, Some((2, 2, 2, 2)));
        }
        let mut asked = 0;
        for i in 0..60 {
            // A pause mid-drag: still frames with a new region each (her dangle).
            let x = if (20..35).contains(&i) { None } else { Some(i) };
            let n = r.moves.len();
            r.frame(x, Some((3, i as i16, 3, 3)));
            if x.is_some() {
                asked += 1;
                assert_eq!(r.moves.len(), n + 1, "a drag move waited at frame {i}");
            }
        }
        assert_eq!(asked, 45);
        assert_eq!(r.regions.len(), 2, "no region out while pressed");
        // Released and still: the newest region goes out once she has held still.
        r.s.press(false, r.now);
        r.wait(SETTLE + FRAME);
        assert_eq!(r.out, vec![(3, 59, 3, 3)]);
    }

    /// Still: a new region goes out at once; a move right after it waits out `GAP`; a landing
    /// never waits.
    #[test]
    fn still_she_reshapes_at_once_and_lands_at_once() {
        let mut r = Run::new();
        assert!(r.s.wants_region(true, &[(1, 1, 1, 1)], r.now), "still: out at once");
        r.now += Duration::from_millis(5);
        assert!(!r.s.wants_move(true, (7, 0), r.now), "a move right after a region waits");
        r.wait(GAP);
        assert_eq!(r.moves, vec![(GAP, 7)], "out once GAP passed");
        assert!(!r.s.wants_region(true, &[(2, 2, 2, 2)], r.now), "a region right after a move waits");
        r.land(500);
        assert_eq!(r.s.due(true, r.now), Some(r.now + SETTLE), "the region waits for her to hold still after the landing");
    }

    /// No compositor to fool (`on` false): everything goes out at once, and what was held when
    /// picom went away goes out at the next look.
    #[test]
    fn without_picom_nothing_waits() {
        let mut s = Settle::default();
        let now = Instant::now();
        assert!(s.wants_move(false, (1, 0), now));
        assert!(s.wants_region(false, &[(1, 1, 1, 1)], now));
        assert!(s.wants_move(false, (2, 0), now));
        assert!(!s.wants_move(true, (3, 0), now), "under picom a move right after a region waits");
        assert!(!s.wants_region(true, &[(2, 2, 2, 2)], now), "and a region right after a move");
        assert_eq!(s.due(false, now), Some(now));
        assert_eq!(s.take_due(false, now), (Some((3, 0)), Some(vec![(2, 2, 2, 2)])));
        assert_eq!(s.due(false, now), None);
    }
}
