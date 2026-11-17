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

#[derive(Clone)]
pub struct Animator {
    base: usize,
    looping: Option<Seq>,
    loop_pos: usize,
    oneshot: Option<Seq>,
    one_pos: usize,
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
        Animator { base, looping: None, loop_pos: 0, oneshot: None, one_pos: 0, due: None, rng: Rng::seeded(), rest_last: false, soon: None }
    }

    pub fn frame(&self) -> usize {
        if let Some(s) = &self.oneshot {
            return s[self.one_pos].0;
        }
        if let Some(s) = &self.looping {
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

    pub fn deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Looped clips breathe: long holds vary so a blink loop doesn't tick like a metronome.
    fn loop_ms(&mut self, ms: u32) -> u64 {
        if ms >= 400 { (ms as f32 * self.rng.range(0.6, 1.7)) as u64 } else { ms as u64 }
    }

    fn start_loop(&mut self, now: Instant) {
        self.loop_pos = 0;
        self.run_loop(now);
    }

    /// After a one-shot: the loop from its start, or back on its rest.
    fn resume_loop(&mut self, now: Instant) {
        self.loop_pos = match &self.looping {
            Some(s) if self.rest_last => s.len() - 1,
            _ => 0,
        };
        self.run_loop(now);
    }

    fn run_loop(&mut self, now: Instant) {
        self.due = match &self.looping {
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
        let held = self.looping.as_ref().is_some_and(|s| s[self.loop_pos].1 >= 400);
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
        self.rest_last = rest_last;
        self.base = base;
        self.looping = looping.filter(|s| !s.is_empty());
        if self.oneshot.is_none() {
            self.start_loop(now);
        }
    }

    /// Play a sequence once, replacing any one-shot in progress.
    pub fn play(&mut self, seq: Seq, now: Instant) {
        if seq.is_empty() {
            return;
        }
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
        self.play(seq, now);
    }

    /// Play `seq` after the one-shot in progress (or now, if none).
    pub fn then(&mut self, seq: Seq, now: Instant) {
        match &mut self.oneshot {
            Some(s) => s.extend(seq),
            None => self.play(seq, now),
        }
    }

    pub fn stop_oneshot(&mut self, now: Instant) {
        if self.oneshot.take().is_some() {
            self.resume_loop(now);
        }
    }

    /// Showing the loop's rest (its last step) or the start of its motion.
    pub fn at_rest(&self) -> bool {
        self.oneshot.is_none() && self.looping.as_ref().is_some_and(|s| self.loop_pos == 0 || self.loop_pos + 1 == s.len())
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
                if self.one_pos < s.len() {
                    self.due = Some(from + Duration::from_millis(s[self.one_pos].1 as u64));
                } else {
                    self.oneshot = None;
                    self.resume_loop(from);
                }
            } else if let Some(s) = &self.looping {
                self.loop_pos = (self.loop_pos + 1) % s.len();
                let ms = s[self.loop_pos].1;
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
