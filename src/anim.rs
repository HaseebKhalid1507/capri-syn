//! Animation: moods (the phase-2 interface), clip playback and timing. Pure logic over
//! frame indices and Instants; the view drives it with one timer at the next deadline.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Idle,
    Thinking,
    /// A tool is running.
    Working,
    /// Assistant text is streaming.
    Talking,
    /// A permission prompt is waiting on the user.
    NeedsYou,
    /// The turn finished.
    Done,
    /// Aborted or errored.
    Annoyed,
    /// Long idle or compaction.
    Sleepy,
    /// Session ended or detached.
    Away,
}

impl Mood {
    pub const ALL: [Mood; 9] = [
        Mood::Idle,
        Mood::Thinking,
        Mood::Working,
        Mood::Talking,
        Mood::NeedsYou,
        Mood::Done,
        Mood::Annoyed,
        Mood::Sleepy,
        Mood::Away,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Mood::Idle => "idle",
            Mood::Thinking => "thinking",
            Mood::Working => "working",
            Mood::Talking => "talking",
            Mood::NeedsYou => "needs_you",
            Mood::Done => "done",
            Mood::Annoyed => "annoyed",
            Mood::Sleepy => "sleepy",
            Mood::Away => "away",
        }
    }

    pub fn parse(s: &str) -> Option<Mood> {
        let k: String = s.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        Mood::ALL.into_iter().find(|m| m.key().replace('_', "") == k)
    }

    /// Fallback HUD label when the pack's mood table has none.
    pub fn default_status(self) -> &'static str {
        match self {
            Mood::Idle => "ONLINE",
            Mood::Thinking => "THINKING",
            Mood::Working => "WORKING",
            Mood::Talking => "SPEAKING",
            Mood::NeedsYou => "NEEDS YOU",
            Mood::Done => "DONE",
            Mood::Annoyed => "ERROR",
            Mood::Sleepy => "SLEEPING",
            Mood::Away => "OFFLINE",
        }
    }
}

/// (frame index, duration ms)
pub type Seq = Vec<(usize, u32)>;


/// Small xorshift; jitter and random picks don't need more.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn seeded() -> Rng {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Rng(t.as_nanos() as u64 | 1)
    }
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.next() >> 40) as f32 / (1u64 << 24) as f32 * (hi - lo)
    }
    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A loop with its way in and out (`moods.json` `{"entry", "loop", "exit"}`, a strip's
/// `entry`/`exit` in anim.json): `entry` plays once on entry, `body` round and round while the
/// loop holds, `exit` once when it ends in place. A plain loop is all body.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ranged<T> {
    pub entry: Vec<T>,
    pub body: Vec<T>,
    pub exit: Vec<T>,
}

/// A looped clip of frames (`Ranged` of `Seq` steps).
pub type Looped = Ranged<(usize, u32)>;

impl<T: Clone> Ranged<T> {
    /// All of it loops (a plain list: today's behaviour).
    pub fn whole(body: Vec<T>) -> Ranged<T> {
        Ranged { entry: Vec::new(), body, exit: Vec::new() }
    }

    /// It has a way in or out (not a plain loop).
    pub fn ranged(&self) -> bool {
        !self.entry.is_empty() || !self.exit.is_empty()
    }

    pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Ranged<U> {
        Ranged { entry: self.entry.iter().map(&f).collect(), body: self.body.iter().map(&f).collect(), exit: self.exit.iter().map(&f).collect() }
    }

    /// The first step shown on entry.
    pub fn first(&self) -> Option<&T> {
        self.entry.first().or(self.body.first())
    }
}

impl Ranged<(u32, u32)> {
    /// Positions `(pos, ms)` split by where they sit in a run of `len`: the first `entry`
    /// positions once, the last `exit` once, the rest loop (a strip's range, thinned or with
    /// cells skipped: what's left of each part stays in it).
    pub fn by_pos(items: Vec<(u32, u32)>, len: u32, entry: u32, exit: u32) -> Self {
        let mut r = Ranged::whole(Vec::new());
        for it in items {
            if it.0 < entry {
                r.entry.push(it);
            } else if it.0 + exit >= len {
                r.exit.push(it);
            } else {
                r.body.push(it);
            }
        }
        r
    }

    /// The position `ms` in: the entry once, then the body round and round (a loop driven by
    /// its own clock, the dangle). Its exit never shows here: that loop only ends in a cut.
    pub fn cell_at(&self, ms: u64) -> Option<u32> {
        let mut u = ms;
        for &(c, d) in &self.entry {
            let d = u64::from(d.max(1));
            if u < d {
                return Some(c);
            }
            u -= d;
        }
        crate::motion::cell_at(&self.body, u).or(self.entry.last().map(|c| c.0))
    }
}

#[derive(Clone)]
pub struct Animator {
    base: usize,
    looping: Option<Looped>,
    /// On the loop's entry (else its body).
    in_entry: bool,
    loop_pos: usize,
    /// The loop has started (its entry played or playing): a one-shot over it resumes the body.
    entered: bool,
    /// Its exit plays when the loop ends (`cut` disarms it: a drag, a teleport, sleep).
    armed: bool,
    oneshot: Option<Seq>,
    one_pos: usize,
    /// The first `exit_n` steps of the one-shot are the exit of `exit_of` (the loop that ended).
    exit_n: usize,
    exit_of: Option<Looped>,
    due: Option<Instant>,
    rng: Rng,
    /// The loop ends on a rest: a one-shot over it (a blink) returns to that rest, not to
    /// the start of the loop's motion.
    rest_last: bool,
    /// The next hold the loop starts or resumes on ends within this (ms), once.
    soon: Option<u32>,
}

impl Animator {
    pub fn new(base: usize) -> Animator {
        Animator {
            base,
            looping: None,
            in_entry: false,
            loop_pos: 0,
            entered: false,
            armed: false,
            oneshot: None,
            one_pos: 0,
            exit_n: 0,
            exit_of: None,
            due: None,
            rng: Rng::seeded(),
            rest_last: false,
            soon: None,
        }
    }

    /// The part of the loop playing: its entry or its body.
    fn part(&self) -> Option<&Seq> {
        self.looping.as_ref().map(|l| if self.in_entry { &l.entry } else { &l.body })
    }

    pub fn frame(&self) -> usize {
        if let Some(s) = &self.oneshot {
            return s[self.one_pos].0;
        }
        if let Some(s) = self.part() {
            return s[self.loop_pos].0;
        }
        self.base
    }

    /// The mood layer's resting frame (stable while loops and one-shots play).
    /// The frame that shows at `t` (a copy steps there; this one doesn't move).
    pub fn frame_at(&self, t: Instant) -> usize {
        if self.due.is_none_or(|d| d > t) {
            return self.frame();
        }
        let mut a = self.clone();
        a.tick(t);
        a.frame()
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn busy(&self) -> bool {
        self.oneshot.is_some()
    }

    /// A loop's exit is playing.
    pub fn in_exit(&self) -> bool {
        self.oneshot.is_some() && self.one_pos < self.exit_n
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Looped clips breathe: long holds vary so a blink loop doesn't tick like a metronome.
    /// The entry plays once, at its own ms.
    fn loop_ms(&mut self, ms: u32) -> u64 {
        if ms >= 400 && !self.in_entry { (ms as f32 * self.rng.range(0.6, 1.7)) as u64 } else { ms as u64 }
    }

    /// Into the loop: its entry (if any), then the body.
    fn start_loop(&mut self, now: Instant) {
        self.entered = self.looping.is_some();
        self.armed = self.entered;
        self.in_entry = self.looping.as_ref().is_some_and(|l| !l.entry.is_empty());
        self.loop_pos = 0;
        self.run_loop(now);
    }

    /// After a one-shot: the loop from its start (its entry, if it hadn't started), or the body
    /// from its start, or back on its rest.
    fn resume_loop(&mut self, now: Instant) {
        if !self.entered {
            self.start_loop(now);
            return;
        }
        self.in_entry = false;
        self.loop_pos = match &self.looping {
            Some(l) if self.rest_last => l.body.len() - 1,
            _ => 0,
        };
        self.run_loop(now);
    }

    fn run_loop(&mut self, now: Instant) {
        self.due = match self.part() {
            Some(s) => {
                let ms = s[self.loop_pos].1;
                let d = self.loop_ms(ms);
                let d = if ms >= 400 { self.soon.take().map_or(d, |c| d.min(c as u64)) } else { d };
                Some(now + Duration::from_millis(d))
            }
            None => None,
        };
    }

    /// The hold she is on (or the next one the loop starts or resumes on, after a one-shot) ends
    /// within `ms`: life right after a landing (a blink), not a still hold first.
    pub fn rest_soon(&mut self, ms: u32, now: Instant) {
        let held = self.part().is_some_and(|s| s[self.loop_pos].1 >= 400);
        if self.oneshot.is_none() && held {
            let t = now + Duration::from_millis(ms as u64);
            self.due = Some(self.due.map_or(t, |d| d.min(t)));
        } else {
            self.soon = Some(ms);
        }
    }

    /// Mood layer: base frame + optional loop. A playing one-shot finishes first.
    pub fn set_layer(&mut self, base: usize, looping: Option<Seq>, now: Instant) {
        self.set_layer_resting(base, looping, false, now);
    }

    /// `set_layer`, for a loop whose last step is a rest (`rest_last`): one-shots return to it.
    pub fn set_layer_resting(&mut self, base: usize, looping: Option<Seq>, rest_last: bool, now: Instant) {
        self.set_layer_looped(base, looping.map(Looped::whole), rest_last, now);
    }

    /// `set_layer_resting` with a loop range. The loop that was playing ends: its exit plays
    /// first (unless `cut`), then the new loop's entry and body. The same ranged loop again
    /// carries on where it is; the one whose exit is playing goes straight back into its body.
    pub fn set_layer_looped(&mut self, base: usize, looping: Option<Looped>, rest_last: bool, now: Instant) {
        let looping = looping.filter(|l| !l.body.is_empty());
        self.base = base;
        self.rest_last = rest_last;
        // Back on during its exit: into the loop, not its entry.
        if looping.is_some() && self.in_exit() && looping == self.exit_of {
            let rest = self.oneshot.take().map(|s| s[self.exit_n..].to_vec()).unwrap_or_default();
            self.exit_n = 0;
            self.exit_of = None;
            self.looping = looping;
            self.entered = true;
            self.armed = true;
            self.in_entry = false;
            self.loop_pos = 0;
            if rest.is_empty() {
                self.run_loop(now);
            } else {
                self.play(rest, now);
            }
            return;
        }
        // The same ranged loop: no seam, no entry again.
        if looping.as_ref().is_some_and(Looped::ranged) && looping == self.looping {
            self.armed |= self.entered;
            return;
        }
        // It ends in place: its exit, over any one-shot that was over it.
        let exit = match &self.looping {
            Some(l) if self.armed && self.entered && !l.exit.is_empty() && !self.in_exit() && looping != self.looping => Some(l.clone()),
            _ => None,
        };
        self.looping = looping;
        self.entered = false;
        self.armed = false;
        self.in_entry = false;
        self.loop_pos = 0;
        if let Some(old) = exit {
            self.due = Some(now + Duration::from_millis(old.exit[0].1 as u64));
            self.oneshot = Some(old.exit.clone());
            self.one_pos = 0;
            self.exit_n = old.exit.len();
            self.exit_of = Some(old);
        } else if self.oneshot.is_none() {
            self.start_loop(now);
        }
    }

    /// An interruption (a drag, a teleport, sleep): the loop's exit won't play, and one
    /// playing stops here.
    pub fn cut(&mut self, now: Instant) {
        self.armed = false;
        if self.in_exit() {
            let rest = self.oneshot.take().map(|s| s[self.exit_n..].to_vec()).unwrap_or_default();
            self.exit_n = 0;
            self.exit_of = None;
            if rest.is_empty() {
                self.resume_loop(now);
            } else {
                self.play(rest, now);
            }
        }
    }

    /// Play a sequence once, replacing any one-shot in progress (an exit playing finishes
    /// first: the sequence follows it).
    pub fn play(&mut self, seq: Seq, now: Instant) {
        if seq.is_empty() {
            return;
        }
        if self.in_exit()
            && let Some(s) = &mut self.oneshot
        {
            s.truncate(self.exit_n);
            s.extend(seq);
            return;
        }
        self.exit_n = 0;
        self.exit_of = None;
        self.due = Some(now + Duration::from_millis(seq[0].1 as u64));
        self.oneshot = Some(seq);
        self.one_pos = 0;
    }

    /// Hold `frame` for `ms` first, then whatever was showing goes on (a cut a frame late: the
    /// new drawing goes underneath while the old one still shows).
    pub fn hold_first(&mut self, frame: usize, ms: u32, now: Instant) {
        let mut seq = vec![(frame, ms)];
        if let Some(s) = self.oneshot.take() {
            seq.extend(s[self.one_pos..].iter().copied());
        }
        self.exit_n = 0;
        self.exit_of = None;
        self.play(seq, now);
    }

    /// Play `seq` after the one-shot in progress (or now, if none).
    pub fn then(&mut self, seq: Seq, now: Instant) {
        match &mut self.oneshot {
            Some(s) => s.extend(seq),
            None => self.play(seq, now),
        }
    }

    /// The one-shot stops (an exit playing finishes: only what follows it goes).
    pub fn stop_oneshot(&mut self, now: Instant) {
        if self.in_exit() {
            if let Some(s) = &mut self.oneshot {
                s.truncate(self.exit_n);
            }
            return;
        }
        if self.oneshot.take().is_some() {
            self.resume_loop(now);
        }
    }

    /// Showing the loop's rest (its last step) or the start of its motion.
    pub fn at_rest(&self) -> bool {
        self.oneshot.is_none() && !self.in_entry && self.looping.as_ref().is_some_and(|l| self.loop_pos == 0 || self.loop_pos + 1 == l.body.len())
    }

    /// Advance past every step that ended by `now`. True if the shown frame changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.frame();
        let mut guard = 0;
        while let Some(due) = self.due
            && due <= now
            && guard < 64
        {
            guard += 1;
            // After a stall (suspend, debugger) restart timing from now instead of replaying.
            let from = if now.duration_since(due) > Duration::from_secs(2) { now } else { due };
            if let Some(s) = &self.oneshot {
                self.one_pos += 1;
                let len = s.len();
                if self.exit_n > 0 && self.one_pos >= self.exit_n {
                    self.exit_n = 0;
                    self.exit_of = None;
                }
                if self.one_pos < len {
                    let ms = self.oneshot.as_ref().map_or(0, |s| s[self.one_pos].1);
                    self.due = Some(from + Duration::from_millis(ms as u64));
                } else {
                    self.oneshot = None;
                    self.resume_loop(from);
                }
            } else if let Some(l) = &self.looping {
                if self.in_entry && self.loop_pos + 1 >= l.entry.len() {
                    self.in_entry = false;
                    self.loop_pos = 0;
                } else {
                    let n = if self.in_entry { l.entry.len() } else { l.body.len() };
                    self.loop_pos = (self.loop_pos + 1) % n;
                }
                let ms = self.part().map_or(0, |s| s[self.loop_pos].1);
                let ms = self.loop_ms(ms);
                self.due = Some(from + Duration::from_millis(ms));
            } else {
                self.due = None;
            }
        }
        self.frame() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entry 1 (frame 1), loop 2-3, exit 9; 100 ms each.
    fn ranged() -> Looped {
        Ranged { entry: vec![(1, 100)], body: vec![(2, 100), (3, 100)], exit: vec![(9, 100)] }
    }

    fn ms(t: Instant, n: u64) -> Instant {
        t + Duration::from_millis(n)
    }

    #[test]
    fn the_entry_plays_once_then_only_the_loop() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer_looped(0, Some(ranged()), false, t);
        let shown: Vec<usize> = (0..8).map(|i| a.frame_at(ms(t, i * 100))).collect();
        assert_eq!(shown, vec![1, 2, 3, 2, 3, 2, 3, 2], "in once, then round and round the middle");
        // The seam: the last loop step hands straight to the first, on its own deadline.
        for i in 1..=6 {
            a.tick(ms(t, i * 100));
        }
        assert_eq!(a.frame(), 3);
        assert_eq!(a.deadline(), Some(ms(t, 700)), "no gap at the seam");
        a.tick(ms(t, 700));
        assert_eq!(a.frame(), 2);
        a.tick(ms(t, 800));
        // The same ranged loop again (a relayer): no seam, no entry again.
        a.set_layer_looped(0, Some(ranged()), false, ms(t, 850));
        assert_eq!(a.frame(), 3);
        assert_eq!(a.deadline(), Some(ms(t, 900)));
    }

    #[test]
    fn the_exit_plays_once_when_the_loop_ends() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer_looped(0, Some(ranged()), false, t);
        a.tick(ms(t, 100));
        assert_eq!(a.frame(), 2);
        // The music stops: the exit, then the next layer (a plain loop of 5).
        a.set_layer(4, Some(vec![(5, 100)]), ms(t, 150));
        assert_eq!(a.frame(), 9, "out once");
        assert!(a.in_exit() && a.busy());
        a.tick(ms(t, 250));
        assert_eq!(a.frame(), 5, "then the new layer");
        assert!(!a.in_exit());
        // No loop after: the exit, then the base.
        let mut b = Animator::new(0);
        b.set_layer_looped(0, Some(ranged()), false, t);
        b.tick(ms(t, 100));
        b.set_layer(7, None, ms(t, 120));
        assert_eq!(b.frame(), 9);
        b.tick(ms(t, 220));
        assert_eq!(b.frame(), 7);
        // Still in its entry: it hadn't started looping, but it ends the same way.
        let mut c = Animator::new(0);
        c.set_layer_looped(0, Some(ranged()), false, t);
        c.set_layer(7, None, ms(t, 50));
        assert_eq!(c.frame(), 9);
    }

    #[test]
    fn back_on_during_the_exit_goes_into_the_loop_not_the_entry() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer_looped(0, Some(ranged()), false, t);
        a.tick(ms(t, 100));
        a.set_layer(7, None, ms(t, 150));
        assert!(a.in_exit());
        a.set_layer(7, None, ms(t, 160));
        assert!(a.in_exit(), "a relayer to the same doesn't stop it");
        a.set_layer_looped(0, Some(ranged()), false, ms(t, 180));
        assert!(!a.in_exit());
        assert_eq!(a.frame(), 2, "the loop's first step, not the entry");
        a.tick(ms(t, 280));
        assert_eq!(a.frame(), 3);
    }

    #[test]
    fn a_cut_ends_the_loop_without_its_exit() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer_looped(0, Some(ranged()), false, t);
        a.tick(ms(t, 100));
        a.cut(ms(t, 120));
        a.set_layer(7, Some(vec![(8, 100)]), ms(t, 120));
        assert_eq!(a.frame(), 8, "a drag, a teleport, sleep: straight to the next");
        // One playing stops at the cut.
        let mut b = Animator::new(0);
        b.set_layer_looped(0, Some(ranged()), false, t);
        b.tick(ms(t, 100));
        b.set_layer(7, None, ms(t, 120));
        assert!(b.in_exit());
        b.cut(ms(t, 130));
        assert!(!b.busy());
        assert_eq!(b.frame(), 7);
    }

    #[test]
    fn one_shots_wait_for_the_exit_and_a_relayer_keeps_it() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer_looped(0, Some(ranged()), false, t);
        a.tick(ms(t, 100));
        a.set_layer(7, None, ms(t, 120));
        a.play(vec![(6, 100)], ms(t, 130));
        assert_eq!(a.frame(), 9, "the exit finishes first");
        a.stop_oneshot(ms(t, 140));
        assert!(a.in_exit(), "only what follows it goes");
        a.play(vec![(6, 100)], ms(t, 150));
        a.tick(ms(t, 220));
        assert_eq!(a.frame(), 6, "then the one-shot");
        a.tick(ms(t, 320));
        assert_eq!(a.frame(), 7);
        // A one-shot over the loop (a blink) comes back to the loop, not the entry.
        let mut b = Animator::new(0);
        b.set_layer_looped(0, Some(ranged()), false, t);
        b.tick(ms(t, 100));
        b.play(vec![(6, 50)], ms(t, 100));
        b.tick(ms(t, 150));
        assert_eq!(b.frame(), 2);
        // Set under a one-shot: the entry plays after it.
        let mut c = Animator::new(0);
        c.play(vec![(6, 50)], t);
        c.set_layer_looped(0, Some(ranged()), false, t);
        c.tick(ms(t, 50));
        assert_eq!(c.frame(), 1, "in once, after the one-shot");
    }

    #[test]
    fn a_plain_list_loops_whole_as_before() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer(0, Some(vec![(1, 100), (2, 100), (3, 100)]), t);
        let shown: Vec<usize> = (0..7).map(|i| a.frame_at(ms(t, i * 100))).collect();
        assert_eq!(shown, vec![1, 2, 3, 1, 2, 3, 1]);
        a.tick(ms(t, 150));
        // The same plain loop again restarts it, as it always did; another one cuts straight in.
        a.set_layer(0, Some(vec![(1, 100), (2, 100), (3, 100)]), ms(t, 150));
        assert_eq!(a.frame(), 1);
        a.set_layer(0, Some(vec![(5, 100)]), ms(t, 160));
        assert_eq!(a.frame(), 5);
        assert!(!a.busy());
        assert_eq!(Looped::whole(vec![(1, 1)]), Ranged { entry: vec![], body: vec![(1, 1)], exit: vec![] });
        assert!(!Looped::whole(vec![(1, 1)]).ranged() && ranged().ranged());
    }

    #[test]
    fn a_strip_range_splits_by_position_and_its_clock_runs_in_once() {
        // Six positions, entry 2, exit 1; position 3 thinned out.
        let r = Ranged::by_pos(vec![(0, 10), (1, 10), (2, 10), (4, 10), (5, 10)], 6, 2, 1);
        assert_eq!((r.entry, r.body.clone(), r.exit), (vec![(0, 10), (1, 10)], vec![(2, 10), (4, 10)], vec![(5, 10)]));
        let r = Ranged::by_pos(vec![(0, 10), (1, 10), (2, 10), (3, 10)], 4, 1, 1);
        assert_eq!([0, 9, 10, 19, 20, 29, 30, 40].map(|t| r.cell_at(t)), [0, 0, 1, 1, 2, 2, 1, 2].map(Some));
        let whole = Ranged::by_pos(vec![(0, 10), (1, 10)], 2, 0, 0);
        assert_eq!([0, 10, 20].map(|t| whole.cell_at(t)), [0, 1, 0].map(Some), "no range: as before");
    }

    #[test]
    fn mood_parse() {
        assert_eq!(Mood::parse("needs_you"), Some(Mood::NeedsYou));
        assert_eq!(Mood::parse("NeedsYou"), Some(Mood::NeedsYou));
        assert_eq!(Mood::parse("idle"), Some(Mood::Idle));
        assert_eq!(Mood::parse("nope"), None);
    }

    #[test]
    fn rest_soon_shortens_the_next_hold_once() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer(0, Some(vec![(0, 3000), (1, 80), (0, 700)]), t);
        a.rest_soon(500, t);
        assert!(a.deadline().is_some_and(|d| d <= t + Duration::from_millis(500)), "on the hold now");
        // Over a one-shot: the hold it resumes on.
        let mut b = Animator::new(0);
        b.set_layer(0, Some(vec![(0, 3000), (1, 80), (0, 700)]), t);
        b.play(vec![(5, 300)], t);
        b.rest_soon(450, t);
        b.tick(t + Duration::from_millis(300));
        assert_eq!(b.frame(), 0);
        assert!(b.deadline().is_some_and(|d| d <= t + Duration::from_millis(750)));
        // Once: used up, the next pass holds as long as ever.
        assert!(b.soon.is_none());
    }

    #[test]
    fn oneshot_returns_to_loop() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.set_layer(0, Some(vec![(0, 100), (1, 50)]), t);
        a.play(vec![(5, 100), (6, 100)], t);
        assert_eq!(a.frame(), 5);
        a.tick(t + Duration::from_millis(100));
        assert_eq!(a.frame(), 6);
        a.tick(t + Duration::from_millis(200));
        assert_eq!(a.frame(), 0);
        assert!(!a.busy());
    }

    #[test]
    fn a_blink_over_a_resting_loop_comes_back_to_the_rest() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        // A pass (1, 2), then the rest on 0 (300 ms; under the 400 ms that breathe).
        a.set_layer_resting(0, Some(vec![(0, 100), (1, 100), (2, 100), (0, 300)]), true, t);
        assert!(a.at_rest(), "it starts with the motion's first cell");
        a.tick(t + Duration::from_millis(100));
        assert_eq!(a.frame(), 1);
        assert!(!a.at_rest(), "mid-sway");
        a.tick(t + Duration::from_millis(300));
        assert!(a.at_rest());
        a.play(vec![(9, 50)], t + Duration::from_millis(350));
        a.tick(t + Duration::from_millis(400));
        assert_eq!(a.frame(), 0, "back on the rest");
        a.tick(t + Duration::from_millis(650));
        assert_eq!(a.frame(), 0, "the rest held out");
        a.tick(t + Duration::from_millis(700));
        assert_eq!(a.frame(), 0, "the next pass starts");
        a.tick(t + Duration::from_millis(800));
        assert_eq!(a.frame(), 1);
        // A plain loop still restarts after a one-shot.
        a.set_layer(0, Some(vec![(0, 100), (1, 100), (2, 100)]), t);
        a.tick(t + Duration::from_millis(900));
        a.play(vec![(9, 50)], t + Duration::from_millis(900));
        a.tick(t + Duration::from_millis(950));
        assert_eq!(a.frame(), 0);
    }

    #[test]
    fn then_follows_the_oneshot_in_progress() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.play(vec![(5, 100)], t);
        a.then(vec![(7, 100)], t + Duration::from_millis(50));
        assert_eq!(a.frame(), 5, "the arrival keeps playing");
        a.tick(t + Duration::from_millis(100));
        assert_eq!(a.frame(), 7, "then the entry");
        a.tick(t + Duration::from_millis(200));
        assert!(!a.busy());
        a.then(vec![(3, 100)], t + Duration::from_millis(300));
        assert_eq!(a.frame(), 3, "nothing playing: now");
    }

    #[test]
    fn hold_first_shows_the_old_frame_then_the_rest() {
        let t = Instant::now();
        let mut a = Animator::new(0);
        a.play(vec![(5, 100), (6, 100)], t);
        a.hold_first(9, 66, t);
        assert_eq!(a.frame(), 9, "the old drawing holds");
        a.tick(t + Duration::from_millis(66));
        assert_eq!(a.frame(), 5, "then the sequence from its start");
        a.tick(t + Duration::from_millis(166));
        assert_eq!(a.frame(), 6);
        a.tick(t + Duration::from_millis(266));
        assert!(!a.busy());
        a.hold_first(4, 33, t + Duration::from_millis(300));
        assert_eq!(a.frame(), 4, "nothing playing: the hold, then the loop");
        a.tick(t + Duration::from_millis(333));
        assert_eq!(a.frame(), 0);
    }
}
