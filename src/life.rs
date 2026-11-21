//! Her day (BRIEF-alive S11, S12, Q1): pure. Sleep follows him (present, away, the screen
//! locked) and the time of day, not the chat: present and quiet she stays up; away past a
//! threshold, or locked, she naps; late at night her energy runs low and she dozes off now and
//! then even while he's there. Coming back (an unlock, a long absence) gets one greeting.
//!
//! Time is local wall seconds (`unix + utc offset`, injected: tests cover a DST change); the
//! brain's own clock maps onto it. Every number here is a guess (BRIEF-alive §7), in one place.

/// Seconds in a day and an hour.
const DAY: f64 = 86_400.0;
const HOUR: f64 = 3600.0;

/// The circadian curve: from each local hour, this level (blended over `BLEND_S` around each
/// change). 08-12 0.8 · 12-18 0.7 · 18-22 0.8 · 22-00:30 0.55 · 00:30-06 0.25 · 06-08 0.5.
const CURVE: [(f64, f64); 6] = [(0.5, 0.25), (6.0, 0.5), (8.0, 0.8), (12.0, 0.7), (18.0, 0.8), (22.0, 0.55)];
const BLEND_S: f64 = 1800.0;

/// Night (the away nap comes sooner): 22:00 to 06:00 local.
const NIGHT: (f64, f64) = (22.0, 6.0);
/// The away nap: this long after he left (by day), at night sooner.
pub const NAP_AWAY_S: f64 = 600.0;
pub const NAP_AWAY_NIGHT_S: f64 = 300.0;

/// Energy bands: high from this up, low under this.
pub const HIGH: f64 = 0.65;
pub const LOW: f64 = 0.35;

/// Fatigue: up this much per second awake with him (0.1 / 30 min), at most `FATIGUE_MAX` (so
/// by day she never runs low: 0.7 × 0.6 = 0.42); down this much per second asleep (0.5 / 20 min).
const FATIGUE_UP: f64 = 0.1 / 1800.0;
const FATIGUE_DOWN: f64 = 0.5 / 1200.0;
const FATIGUE_MAX: f64 = 0.4;
/// Excitement fades with this time constant.
const EXCITE_TAU_S: f64 = 600.0;
/// What a pat adds.
pub const PAT: f64 = 0.15;

/// A doze (low energy, him there): this long, at most once per `DOZE_GAP_S` of being awake.
pub const DOZE_S: (f64, f64) = (240.0, 480.0);
pub const DOZE_GAP_S: f64 = 1500.0;

/// A greeting: back after at least this long away (an unlock or a resume greets whatever the
/// length), at most `GREETS_PER_DAY` a day (days start at 05:00 local).
pub const GREET_AWAY_S: f64 = 1800.0;
pub const GREETS_PER_DAY: usize = 3;
const DAY_STARTS_H: f64 = 5.0;
/// Before noon, the first greeting of the day is the morning one.
const MORNING_H: (f64, f64) = (5.0, 12.0);

/// Local wall seconds of a unix time with its UTC offset then.
pub fn local_s(unix: f64, offset_s: i32) -> f64 {
    unix + offset_s as f64
}

/// The hour of the day (0..24) at local seconds `l`.
pub fn hour(l: f64) -> f64 {
    l.rem_euclid(DAY) / HOUR
}

/// The curve's level at hour `h`, unblended.
fn level(h: f64) -> f64 {
    let h = h.rem_euclid(24.0);
    CURVE.iter().rev().find(|&&(from, _)| h >= from).map_or(CURVE[CURVE.len() - 1].1, |&(_, v)| v)
}

/// The circadian level at local seconds `l`: the curve averaged over the half hour around it
/// (each change a 30 min ramp).
pub fn circadian(l: f64) -> f64 {
    let (a, b) = (l - BLEND_S / 2.0, l + BLEND_S / 2.0);
    // The changes inside [a, b] (each day's), in order.
    let mut cuts: Vec<f64> = Vec::new();
    let d0 = (a / DAY).floor();
    for d in [d0, d0 + 1.0] {
        for &(from, _) in &CURVE {
            let c = d * DAY + from * HOUR;
            if c > a && c < b {
                cuts.push(c);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    let mut sum = 0.0;
    let mut x = a;
    for c in cuts.into_iter().chain([b]) {
        sum += level(hour((x + c) / 2.0)) * (c - x);
        x = c;
    }
    sum / BLEND_S
}

/// Night at local seconds `l`.
pub fn night(l: f64) -> bool {
    let h = hour(l);
    h >= NIGHT.0 || h < NIGHT.1
}

/// When the away nap is due, for an absence from brain time `away` (`local` maps brain time
/// to local seconds): `day_s` later (`NAP_AWAY_S`), or `night_s` (`NAP_AWAY_NIGHT_S`) once it's
/// night (if night falls in between, then).
pub fn nap_due(away: f64, day_s: f64, night_s: f64, local: impl Fn(f64) -> f64) -> f64 {
    let soon = away + night_s.min(day_s);
    if night(local(soon)) {
        return soon;
    }
    let late = away + day_s;
    // Night starts between the two: at its start.
    let (ls, ll) = (local(soon), local(late));
    let start = (ls / DAY).floor() * DAY + NIGHT.0 * HOUR;
    for s in [start, start + DAY] {
        if s > ls && s <= ll {
            return soon + (s - ls);
        }
    }
    late
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Low,
    Mid,
    High,
}

impl Band {
    pub fn parse(s: &str) -> Option<Band> {
        Some(match s {
            "low" => Band::Low,
            "mid" => Band::Mid,
            "high" => Band::High,
            _ => return None,
        })
    }

    /// Her own moves' pace in this band (roams ×1.2 / 1 / 0.5).
    pub fn roam_factor(self) -> f64 {
        match self {
            Band::Low => 0.5,
            Band::Mid => 1.0,
            Band::High => 1.2,
        }
    }
}

pub fn band(e: f64) -> Band {
    if e >= HIGH {
        Band::High
    } else if e < LOW {
        Band::Low
    } else {
        Band::Mid
    }
}

/// What her energy remembers: fatigue (awake with him, slept off) and excitement (a pat).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Energy {
    pub fatigue: f64,
    pub excite: f64,
    /// Brain time of the last `advance`.
    t: Option<f64>,
}

/// What she was doing since the last `advance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spent {
    /// Awake with him there.
    Awake,
    Asleep,
    /// Neither (he's away and she's still up).
    Idle,
}

impl Energy {
    /// Run the clock to brain time `t`, spent as `s`.
    pub fn advance(&mut self, t: f64, s: Spent) {
        let dt = self.t.map_or(0.0, |p| (t - p).max(0.0));
        self.t = Some(self.t.map_or(t, |p| p.max(t)));
        match s {
            Spent::Awake => self.fatigue = (self.fatigue + FATIGUE_UP * dt).min(FATIGUE_MAX),
            Spent::Asleep => self.fatigue = (self.fatigue - FATIGUE_DOWN * dt).max(0.0),
            Spent::Idle => {}
        }
        self.excite *= (-dt / EXCITE_TAU_S).exp();
    }

    pub fn excite(&mut self, by: f64) {
        self.excite = (self.excite + by).min(1.0);
    }

    /// Her energy at local seconds `l` (0..1).
    pub fn value(&self, l: f64) -> f64 {
        (circadian(l) * (1.0 - self.fatigue) + self.excite).clamp(0.0, 1.0)
    }
}

/// Whether she may doze off now, with him there: her energy low, awake long enough, not busy.
pub fn may_doze(energy: f64, awake_for_s: f64) -> bool {
    band(energy) == Band::Low && awake_for_s >= DOZE_GAP_S
}

/// How he came back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Return {
    /// The screen unlocked, or the machine woke (not locked).
    Unlock,
    /// Input again after `away_s` away.
    Back { away_s: f64 },
}

/// The greeting for a return, if any: none under DND or while she's hidden (`quiet`), after a
/// short absence, or past the day's few; the first one of a morning is the morning one.
pub fn greeting(r: Return, quiet: bool, today: usize, local: f64) -> Option<Greet> {
    if quiet || today >= GREETS_PER_DAY {
        return None;
    }
    if let Return::Back { away_s } = r
        && away_s < GREET_AWAY_S
    {
        return None;
    }
    let h = hour(local);
    Some(Greet { morning: today == 0 && h >= MORNING_H.0 && h < MORNING_H.1 })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Greet {
    pub morning: bool,
}

/// The day a local time belongs to (days start at 05:00): greetings count per day.
pub fn day_of(l: f64) -> i64 {
    ((l - DAY_STARTS_H * HOUR) / DAY).floor() as i64
}

/// Her idle beats, spread evenly (r42: random picks bunched the face-only ones into 30-40 s
/// with no change of outline): a gesture (`gesture[i]`: a hop, a hand on her hip, arms crossed,
/// a point) and a face beat take turns, each kind from its own shuffled round (each once
/// before any again), never the same beat twice running. "The same" is the clip itself
/// (`keys[i]`, the view's `beat_key`), not its place in the list: a list that changed (her band,
/// a hang) can't bring the last one straight back, and neither can a glance or music
/// (`mark`).
#[derive(Default)]
pub struct Beats {
    bags: [Vec<usize>; 2],
    /// What played last (its key).
    last: Option<u64>,
    gesture_next: bool,
    /// The list the rounds are of.
    keys: Vec<u64>,
}

impl Beats {
    #[cfg(test)]
    pub fn pick(&mut self, gesture: &[bool], rng: &mut crate::anim::Rng) -> usize {
        let keys: Vec<u64> = (0..gesture.len() as u64).collect();
        self.pick_at(gesture, &keys, 0, rng).unwrap_or(0)
    }

    /// What played last, not one of the list's beats (a glance, music).
    pub fn mark(&mut self, key: u64) {
        self.last = Some(key);
    }

    /// Played last (its key).
    pub fn last(&self) -> Option<u64> {
        self.last
    }

    /// `pick` at her boredom's `stage` (`Restless::stage`): past 0 a gesture may follow a gesture
    /// (`BORED_GESTURE`), so the bigger beats come more often. None: only the one just played
    /// (a turn skipped, then it may come): never the same twice running.
    pub fn pick_at(&mut self, gesture: &[bool], keys: &[u64], stage: usize, rng: &mut crate::anim::Rng) -> Option<usize> {
        let n = gesture.len().min(keys.len());
        let (gesture, keys) = (&gesture[..n], &keys[..n]);
        if n <= 1 {
            let again = n == 0 || self.last == Some(keys[0]);
            self.last = if again { None } else { Some(keys[0]) };
            return (!again).then_some(0);
        }
        if keys != self.keys.as_slice() {
            *self = Beats { keys: keys.to_vec(), gesture_next: true, last: self.last, ..Beats::default() };
        }
        let gestures = gesture.iter().filter(|&&g| g).count();
        if !self.gesture_next && gestures > 1 && rng.range(0.0, 1.0) < BORED_GESTURE[stage.min(BORED_GESTURE.len() - 1)] {
            self.gesture_next = true;
        }
        let has = |g: bool| gesture.contains(&g);
        let g = if has(self.gesture_next) { self.gesture_next } else { !self.gesture_next };
        let k = usize::from(g);
        if self.bags[k].is_empty() {
            let mut bag: Vec<usize> = (0..n).filter(|&i| gesture[i] == g).collect();
            for j in (1..bag.len()).rev() {
                bag.swap(j, rng.pick(j + 1));
            }
            if bag.len() > 1 && bag.last().map(|&i| keys[i]) == self.last {
                let m = bag.len() - 1;
                bag.swap(0, m);
            }
            self.bags[k] = bag;
        }
        let bag = &mut self.bags[k];
        // The one just played (a glance or music marked it, or a changed list): another of
        // the round first, else of the other kind, else a turn skipped.
        if bag.last().map(|&i| keys[i]) == self.last {
            if let Some(j) = bag.iter().position(|&i| Some(keys[i]) != self.last) {
                let m = bag.len() - 1;
                bag.swap(j, m);
            } else if let Some(i) = (0..n).find(|&i| gesture[i] != g && Some(keys[i]) != self.last) {
                self.last = Some(keys[i]);
                return Some(i);
            } else {
                self.last = None;
                return None;
            }
        }
        let i = bag.pop().unwrap_or(0);
        self.last = Some(keys[i]);
        self.gesture_next = !g;
        Some(i)
    }
}

/// The restless idle, "an AI with ADHD": always on, no settings. Her idle beats (`Beats`: a
/// gesture and a face beat in turn, from whatever activities, fidgets, poses or hang fidgets the
/// pack fills) come every few seconds; now and then a tangent, a glance at the pointer, the
/// pointer coming near or a window coming forward (a focus change), dropped quickly for
/// something else. While her session works
/// (thinking, replying, a tool) she's steady: no beats (hyperfocus); they come back soon after.
/// The longer nobody engages her, the bigger the beats (`BORED_S`, `BORED_GESTURE`), topping out
/// before the brain's attention bids (8-15 min) take over. Asleep: still. Held (`brain::HOLDERS`:
/// a bubble, the field, the Line, a prompt, the menu, a drag; the voice, a move, hidden): quiet.
/// A beat plays in place: never a move, so the brain's `max_moves_per_hour` never sees one.
/// Pure: seconds in, beats out.
///
/// The gap between beats: the pack's own for her energy band (`energy.<band>.gapMs`), else this.
pub const MICRO_GAP_MS: [u64; 2] = [2_500, 6_000];
/// The share of beats that are a tangent at the pointer (never two tangents running).
pub const TANGENT_P: f32 = 0.12;
/// A cue (a window coming forward, the pointer near): the glance comes this soon...
pub const CUE_DELAY_MS: [u64; 2] = [300, 900];
/// ...and after any tangent something else comes this soon.
pub const AFTER_TANGENT_MS: [u64; 2] = [900, 1_800];
/// Out of a hold, her session's turn or a sleep: the first beat this soon.
pub const RESUME_MS: [u64; 2] = [1_200, 2_500];
/// Boredom: idle (nobody engaging her) this long, the next stage...
pub const BORED_S: [f64; 3] = [60.0, 180.0, 420.0];
/// ...and per stage the chance a gesture follows a gesture (0: they take turns, r42).
pub const BORED_GESTURE: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// Why she keeps still now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    No,
    /// Her session works (hyperfocus), or a holder: the Line, a prompt, the menu, a drag, the voice.
    /// Engaged: her boredom starts over when it ends.
    Busy,
    /// Nothing to play here (a mood without beats, a perch without fidgets, a move): still, and
    /// her boredom runs on.
    Off,
    Asleep,
}

/// What the view sees that keeps her still (`quiet`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Still {
    /// Sleepy, or the brain's nap.
    pub asleep: bool,
    /// A holder he raised (`brain::HOLDERS`: a reply bubble, the field, the Line, a prompt, the
    /// menu, a drag).
    pub held: bool,
    /// Her session works (thinking, replying, a tool): hyperfocus.
    pub session: bool,
    /// Hands-free, listening or speaking.
    pub voice: bool,
    /// A reaction keeping up with the pointer.
    pub following: bool,
    /// Her own unprompted words in a bubble: quiet, but nobody engaged her.
    pub own_bubble: bool,
    /// A move, a reaction, music, the intro.
    pub moving: bool,
    /// Hidden: the menu's HIDE, the fullscreen hide, parked.
    pub hidden: bool,
    /// Nothing to play here: a mood without autonomy, no beats (a perch without fidgets).
    pub nothing: bool,
}

/// Why she keeps still, from what the view sees.
pub fn quiet(s: Still) -> Quiet {
    if s.asleep {
        Quiet::Asleep
    } else if s.held || s.session || s.voice || s.following {
        Quiet::Busy
    } else if s.own_bubble || s.moving || s.hidden || s.nothing {
        Quiet::Off
    } else {
        Quiet::No
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beat {
    /// One of her idle beats (`Beats::pick_at` at `Restless::stage`).
    Fidget,
    /// A tangent: a glance at the cue (`cued`), else at the pointer; then something else.
    Glance { cued: bool },
}

pub struct Restless {
    rng: crate::anim::Rng,
    /// The next beat due, a cue's glance due (seconds on the view's clock).
    next: Option<f64>,
    cue: Option<f64>,
    /// The last beat was a tangent.
    glanced: bool,
    /// Since when nobody engaged her (boredom's clock).
    engaged: f64,
    /// Keeping still, and whether it engaged her.
    held: Option<bool>,
}

impl Restless {
    pub fn new(seed: u64, t: f64) -> Restless {
        Restless { rng: crate::anim::Rng::new(seed), next: None, cue: None, glanced: false, engaged: t, held: None }
    }

    fn secs(&mut self, r: [u64; 2]) -> f64 {
        f64::from(self.rng.range(r[0] as f32, r[1].max(r[0]) as f32)) / 1000.0
    }

    /// Someone engaged her (a click, a pat, the Line): boredom starts over.
    pub fn engage(&mut self, t: f64) {
        self.engaged = t;
    }

    /// Something to glance at. Dropped while she keeps still, one is coming, or she just
    /// glanced (never two glances running); true if taken.
    pub fn cue(&mut self, t: f64) -> bool {
        let take = self.held.is_none() && self.cue.is_none() && !self.glanced;
        if take {
            self.cue = Some(t + self.secs(CUE_DELAY_MS));
        }
        take
    }

    /// Her boredom's stage at `t` (0..=3).
    pub fn stage(&self, t: f64) -> usize {
        BORED_S.iter().filter(|&&s| t - self.engaged >= s).count()
    }

    /// When she next wants a beat (None: keeping still).
    pub fn due(&self) -> Option<f64> {
        [self.next, self.cue].into_iter().flatten().reduce(f64::min)
    }

    /// Per tick: keeping still (`quiet`) drops what was due; out of it, the first beat soon.
    /// True when a beat is due. `gap`: the gap here, ms [min, max].
    pub fn poll(&mut self, t: f64, quiet: Quiet, gap: [u64; 2]) -> bool {
        if quiet != Quiet::No {
            let engaging = matches!(quiet, Quiet::Busy | Quiet::Asleep);
            self.held = Some(self.held.unwrap_or(false) || engaging);
            self.next = None;
            self.cue = None;
            return false;
        }
        if let Some(engaged) = self.held.take() {
            // Back from her turn, a hold or a sleep: soon, and her boredom starts over.
            if engaged {
                self.engaged = t;
            }
            self.next = Some(t + self.secs(RESUME_MS));
            return false;
        }
        if self.next.is_none() {
            self.next = Some(t + self.secs(gap));
        }
        self.due().is_some_and(|d| d <= t)
    }

    /// The beat due now, if any (the view plays it, then calls `played`).
    pub fn pick(&mut self, t: f64) -> Option<Beat> {
        if self.cue.is_some_and(|c| c <= t) {
            self.cue = None;
            // A tangent came first meanwhile: dropped, never two glances running.
            if !self.glanced {
                self.glanced = true;
                return Some(Beat::Glance { cued: true });
            }
        }
        if self.next.is_none_or(|n| n > t) {
            return None;
        }
        let tangent = !self.glanced && self.rng.range(0.0, 1.0) < TANGENT_P;
        self.glanced = tangent;
        Some(if tangent { Beat::Glance { cued: false } } else { Beat::Fidget })
    }

    /// The glance `pick` gave couldn't play (she's not on her feet): a plain beat instead, so
    /// the next isn't a tangent's quick follow-up and a cue may come.
    pub fn unglance(&mut self) {
        self.glanced = false;
    }

    /// A beat played from `t`, `len_s` long: the next comes a gap after it (soon after a tangent).
    pub fn played(&mut self, t: f64, len_s: f64, gap: [u64; 2]) {
        let after = if self.glanced { AFTER_TANGENT_MS } else { gap };
        self.next = Some(t + len_s + self.secs(after));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_beats_take_turns_and_come_round_evenly() {
        let mut rng = crate::anim::Rng::new(7);
        let gesture = [false, true, true, true, true, true, false, false, false, true, false, false, false, false, true];
        let mut b = Beats::default();
        let mut seen = vec![0i32; gesture.len()];
        let picks: Vec<usize> = (0..600).map(|_| b.pick(&gesture, &mut rng)).collect();
        for w in picks.windows(2) {
            assert_ne!(w[0], w[1], "no beat twice running");
            assert_ne!(gesture[w[0]], gesture[w[1]], "a gesture, a face, a gesture: {w:?}");
        }
        for &i in &picks {
            seen[i] += 1;
        }
        // 300 of each kind: 7 gestures and 8 faces, each once a round.
        for (i, &c) in seen.iter().enumerate() {
            let per: i32 = if gesture[i] { 300 / 7 } else { 300 / 8 };
            assert!((c - per).abs() <= 1, "{i}: {c} {seen:?}");
        }
        // All one kind: a plain shuffled round.
        let mut b = Beats::default();
        let one = [false; 5];
        let p: Vec<usize> = (0..50).map(|_| b.pick(&one, &mut rng)).collect();
        assert!(p.windows(2).all(|w| w[0] != w[1]));
        assert!((0..5).all(|i| p.iter().filter(|&&x| x == i).count() == 10));
    }

    /// Drive a `Restless` and `Beats` from `from` to `to` (s) in 100 ms ticks, the view's side
    /// simulated: a beat plays at once for `len` s. Returns (t, beat index or None for a glance).
    fn drive(r: &mut Restless, b: &mut Beats, from: f64, to: f64, gesture: &[bool], quiet: &dyn Fn(f64) -> Quiet, len: f64) -> Vec<(f64, Option<usize>)> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            let q = quiet(t);
            if r.poll(t, q, MICRO_GAP_MS)
                && let Some(beat) = r.pick(t)
            {
                assert_eq!(q, Quiet::No);
                let stage = r.stage(t);
                let i = match beat {
                    Beat::Fidget => b.pick_at(gesture, &(0..gesture.len() as u64).collect::<Vec<_>>(), stage, &mut r.rng),
                    Beat::Glance { .. } => None,
                };
                if beat == Beat::Fidget && i.is_none() {
                    r.played(t, 0.0, MICRO_GAP_MS);
                } else {
                    out.push((t, i));
                    r.played(t, len, MICRO_GAP_MS);
                }
            }
            t += 0.1;
        }
        out
    }

    const POOL: [bool; 7] = [false, false, false, false, true, true, true];

    fn fidgets(b: &[(f64, Option<usize>)]) -> Vec<(f64, usize)> {
        b.iter().filter_map(|&(t, i)| Some((t, i?))).collect()
    }

    #[test]
    fn restless_never_plays_the_same_beat_twice_running() {
        for seed in 1..8 {
            let mut r = Restless::new(seed, 0.0);
            let beats = drive(&mut r, &mut Beats::default(), 0.0, 3600.0, &POOL, &|_| Quiet::No, 0.8);
            assert!(beats.len() > 500, "{seed}: every few seconds ({})", beats.len());
            let f = fidgets(&beats);
            for w in f.windows(2) {
                assert_ne!(w[0].1, w[1].1, "{seed}: twice running at {:.1}", w[1].0);
            }
            // Never two tangents running.
            for w in beats.windows(2) {
                assert!(w[0].1.is_some() || w[1].1.is_some(), "{seed}: two glances at {:.1}", w[1].0);
            }
            assert!(beats.iter().filter(|b| b.1.is_none()).count() > 30, "{seed}: a tangent now and then");
            assert!((0..POOL.len()).all(|i| f.iter().filter(|x| x.1 == i).count() > 10), "{seed}");
            // A gap of a few seconds between beats, a tangent's follow-up a bit sooner.
            for w in beats.windows(2) {
                let d = w[1].0 - w[0].0;
                assert!((0.8 + 0.9 - 0.11..=0.8 + 6.0 + 0.11).contains(&d), "{seed}: {d}");
            }
        }
        // One beat alone: every other turn, never back to back.
        let mut r = Restless::new(3, 0.0);
        let f = fidgets(&drive(&mut r, &mut Beats::default(), 0.0, 600.0, &[false], &|_| Quiet::No, 0.5));
        assert!(f.len() > 20);
        for w in f.windows(2) {
            assert!(w[1].0 - w[0].0 > 2.5 + 0.5, "skipped a turn between: {w:?}");
        }
    }

    #[test]
    fn restless_is_quiet_while_busy_and_resumes_after() {
        // Her session works from 100 to 400 s (or a holder: the Line, a drag, the voice).
        let busy = |t: f64| if (100.0..400.0).contains(&t) { Quiet::Busy } else { Quiet::No };
        for seed in 1..8 {
            let mut r = Restless::new(seed, 0.0);
            let beats = drive(&mut r, &mut Beats::default(), 0.0, 600.0, &POOL, &busy, 0.8);
            assert!(beats.iter().all(|b| !(100.0..400.0).contains(&b.0)), "{seed}: a beat while busy");
            assert!(beats.iter().any(|b| b.0 < 100.0));
            let first = beats.iter().find(|b| b.0 >= 400.0).unwrap().0;
            let (lo, hi) = (RESUME_MS[0] as f64 / 1000.0, RESUME_MS[1] as f64 / 1000.0);
            assert!(first >= 400.0 + lo - 0.11 && first <= 400.0 + hi + 0.11, "{seed}: resumed at {first}");
            // A cue while busy is dropped, not saved for later.
            let mut r = Restless::new(seed, 0.0);
            r.poll(10.0, Quiet::Busy, MICRO_GAP_MS);
            assert!(!r.cue(10.0));
            assert_eq!(r.due(), None);
        }
    }

    #[test]
    fn restless_is_still_while_asleep() {
        let asleep = |t: f64| if t < 1800.0 { Quiet::Asleep } else { Quiet::No };
        let mut r = Restless::new(5, 0.0);
        let beats = drive(&mut r, &mut Beats::default(), 0.0, 1900.0, &POOL, &asleep, 0.8);
        assert!(beats.iter().all(|b| b.0 >= 1800.0), "nothing asleep");
        assert!(!beats.is_empty(), "awake again: back");
        let mut r = Restless::new(5, 0.0);
        assert!(!r.poll(0.0, Quiet::Asleep, MICRO_GAP_MS));
        assert!(!r.cue(0.0));
        assert!(!r.poll(5.0, Quiet::Asleep, MICRO_GAP_MS) && r.due().is_none() && r.pick(5.0).is_none());
    }

    #[test]
    fn restless_boredom_ramps_into_bigger_beats() {
        let mut share = [0.0f64; 4];
        for seed in 1..41 {
            let mut r = Restless::new(seed, 0.0);
            assert_eq!([r.stage(0.0), r.stage(61.0), r.stage(200.0), r.stage(500.0)], [0, 1, 2, 3]);
            let f = fidgets(&drive(&mut r, &mut Beats::default(), 0.0, 900.0, &POOL, &|_| Quiet::No, 0.8));
            for (k, (lo, hi)) in [(0.0, 60.0), (60.0, 180.0), (180.0, 420.0), (420.0, 900.0)].into_iter().enumerate() {
                let w: Vec<usize> = f.iter().filter(|x| x.0 >= lo && x.0 < hi).map(|x| x.1).collect();
                share[k] += w.iter().filter(|&&i| POOL[i]).count() as f64 / w.len().max(1) as f64 / 40.0;
            }
        }
        // Fresh: a gesture and a face in turn (r42); bored: more gestures each stage.
        assert!((0.4..0.6).contains(&share[0]), "{share:?}");
        assert!(share[0] < share[1] && share[1] < share[2] && share[2] < share[3], "{share:?}");
        assert!(share[3] > 0.7, "{share:?}");
        // Engaged again (a click, or her session's turn): back to the start. Nothing to play for
        // a while (a perch without fidgets) doesn't count.
        let mut r = Restless::new(1, 0.0);
        r.engage(500.0);
        assert_eq!(r.stage(510.0), 0);
        let mut r = Restless::new(1, 0.0);
        r.poll(100.0, Quiet::Off, MICRO_GAP_MS);
        r.poll(200.0, Quiet::No, MICRO_GAP_MS);
        assert_eq!(r.stage(200.0), 2);
        r.poll(300.0, Quiet::Busy, MICRO_GAP_MS);
        r.poll(310.0, Quiet::No, MICRO_GAP_MS);
        assert_eq!(r.stage(310.0), 0);
        // The ramp tops out before the brain's first attention bid (8 min at the earliest).
        const { assert!(BORED_S[2] < 480.0) };
    }

    #[test]
    fn a_cue_is_a_glance_soon_then_something_else() {
        let mut r = Restless::new(9, 0.0);
        assert!(!r.poll(0.0, Quiet::No, MICRO_GAP_MS));
        assert!(r.cue(1.0) && !r.cue(1.1), "one at a time");
        let (lo, hi) = (1.0 + CUE_DELAY_MS[0] as f64 / 1000.0, 1.0 + CUE_DELAY_MS[1] as f64 / 1000.0);
        let due = r.due().unwrap();
        assert!(due >= lo - 1e-6 && due <= hi + 1e-6);
        assert!(r.poll(due, Quiet::No, MICRO_GAP_MS));
        assert_eq!(r.pick(due), Some(Beat::Glance { cued: true }));
        r.played(due, 1.4, MICRO_GAP_MS);
        let next = r.due().unwrap();
        assert!(next - due <= 1.4 + AFTER_TANGENT_MS[1] as f64 / 1000.0 + 1e-6);
        assert!(r.poll(next, Quiet::No, MICRO_GAP_MS));
        assert_eq!(r.pick(next), Some(Beat::Fidget), "dropped for something else");
    }

    #[test]
    fn a_cue_right_after_a_glance_is_dropped() {
        // Cued just after a glance (cued or a tangent): never two glances running.
        let mut r = Restless::new(9, 0.0);
        r.poll(0.0, Quiet::No, MICRO_GAP_MS);
        assert!(r.cue(1.0));
        let due = r.due().unwrap();
        assert!(r.poll(due, Quiet::No, MICRO_GAP_MS));
        assert_eq!(r.pick(due), Some(Beat::Glance { cued: true }));
        r.played(due, 1.0, MICRO_GAP_MS);
        assert!(!r.cue(due + 1.1), "a cue right after a glance");
        let next = r.due().unwrap();
        assert!(r.poll(next, Quiet::No, MICRO_GAP_MS));
        assert_eq!(r.pick(next), Some(Beat::Fidget));
        r.played(next, 0.5, MICRO_GAP_MS);
        assert!(r.cue(next + 1.0), "after something else, a cue again");
        // A cue taken before a tangent that came first: dropped when due.
        for seed in 1..200 {
            let mut r = Restless::new(seed, 0.0);
            r.poll(0.0, Quiet::No, MICRO_GAP_MS);
            let next = r.due().unwrap();
            r.poll(next, Quiet::No, MICRO_GAP_MS);
            if r.pick(next) != Some(Beat::Glance { cued: false }) {
                continue;
            }
            r.played(next, 1.0, MICRO_GAP_MS);
            r.cue = Some(next + 0.1);
            assert_eq!(r.pick(next + 0.1), None, "{seed}: a glance after a glance");
            return;
        }
        panic!("no tangent in 200 seeds");
    }

    #[test]
    fn a_glance_that_cant_play_is_a_plain_beat() {
        let mut r = Restless::new(9, 0.0);
        r.poll(0.0, Quiet::No, MICRO_GAP_MS);
        assert!(r.cue(1.0));
        let due = r.due().unwrap();
        r.poll(due, Quiet::No, MICRO_GAP_MS);
        assert_eq!(r.pick(due), Some(Beat::Glance { cued: true }));
        r.unglance();
        r.played(due, 0.5, [5_000, 5_000]);
        assert!((r.due().unwrap() - (due + 5.5)).abs() < 1e-3, "a plain beat's gap");
        assert!(r.cue(due + 1.0), "a cue may come");
    }

    #[test]
    fn the_same_clip_never_twice_running_across_lists() {
        let mut rng = crate::anim::Rng::new(4);
        let mut b = Beats::default();
        let g = [false, true, false, true];
        // Her band changes the list (other places, other members): never the last clip again.
        let lists: [[u64; 4]; 3] = [[10, 11, 12, 13], [13, 20, 21, 10], [12, 13, 10, 11]];
        let mut last = None;
        for k in 0..300 {
            let keys = lists[k % 7 % 3];
            if let Some(i) = b.pick_at(&g, &keys, k % 4, &mut rng) {
                assert_ne!(Some(keys[i]), last, "{k}");
                last = Some(keys[i]);
            } else {
                last = None;
            }
        }
        // Marked (a glance or music played): not that clip next.
        for _ in 0..50 {
            b.mark(11);
            let i = b.pick_at(&g, &lists[0], 0, &mut rng).unwrap();
            assert_ne!(lists[0][i], 11);
        }
        // One beat, the one just marked: a turn skipped, then it.
        b.mark(7);
        assert_eq!(b.pick_at(&[true], &[7], 0, &mut rng), None);
        assert_eq!(b.pick_at(&[true], &[7], 0, &mut rng), Some(0));
    }

    #[test]
    fn what_keeps_her_still_maps_as_documented() {
        let base = Still::default();
        assert_eq!(quiet(base), Quiet::No);
        let cases: [(&str, Still, Quiet); 10] = [
            ("the Line, the menu, a prompt, his reply", Still { held: true, ..base }, Quiet::Busy),
            ("her session works", Still { session: true, ..base }, Quiet::Busy),
            ("the voice", Still { voice: true, ..base }, Quiet::Busy),
            ("following the pointer", Still { following: true, ..base }, Quiet::Busy),
            ("asleep", Still { asleep: true, ..base }, Quiet::Asleep),
            ("asleep beats everything", Still { asleep: true, held: true, session: true, hidden: true, ..base }, Quiet::Asleep),
            ("hidden or fullscreen", Still { hidden: true, ..base }, Quiet::Off),
            ("her own words", Still { own_bubble: true, ..base }, Quiet::Off),
            ("a move, music, the intro", Still { moving: true, ..base }, Quiet::Off),
            ("nothing to play", Still { nothing: true, ..base }, Quiet::Off),
        ];
        for (what, s, want) in cases {
            assert_eq!(quiet(s), want, "{what}");
        }
        // Her own words don't engage her: boredom runs on; his reply does.
        let mut r = Restless::new(1, 0.0);
        r.poll(200.0, quiet(Still { own_bubble: true, ..base }), MICRO_GAP_MS);
        r.poll(210.0, Quiet::No, MICRO_GAP_MS);
        assert_eq!(r.stage(210.0), 2);
        r.poll(220.0, quiet(Still { held: true, ..base }), MICRO_GAP_MS);
        r.poll(230.0, Quiet::No, MICRO_GAP_MS);
        assert_eq!(r.stage(230.0), 0);
    }

    /// Local seconds at `h:m` on day `d`.
    fn at(d: f64, h: f64, m: f64) -> f64 {
        d * DAY + h * HOUR + m * 60.0
    }

    #[test]
    fn the_curve_at_fixed_local_times() {
        for (h, m, want) in [(10.0, 0.0, 0.8), (15.0, 0.0, 0.7), (20.0, 0.0, 0.8), (23.0, 0.0, 0.55), (3.0, 0.0, 0.25), (7.0, 0.0, 0.5)] {
            assert!((circadian(at(100.0, h, m)) - want).abs() < 1e-9, "{h}:{m} {}", circadian(at(100.0, h, m)));
        }
        // Each change is a 30 min ramp: halfway at the change, whole 15 min either side.
        assert!((circadian(at(100.0, 8.0, 0.0)) - 0.65).abs() < 1e-9);
        assert!((circadian(at(100.0, 7.0, 45.0)) - 0.5).abs() < 1e-9);
        assert!((circadian(at(100.0, 8.0, 15.0)) - 0.8).abs() < 1e-9);
        assert!((circadian(at(100.0, 0.0, 30.0)) - 0.4).abs() < 1e-9, "across midnight's next day");
        // Continuous everywhere (no step over a minute).
        let mut prev = circadian(at(100.0, 0.0, 0.0));
        for k in 1..=1440 {
            let v = circadian(at(100.0, 0.0, k as f64));
            assert!((v - prev).abs() < 0.02, "{k} min: {prev} -> {v}");
            prev = v;
        }
    }

    #[test]
    fn local_time_follows_the_offset_through_a_dst_change() {
        // 2026-03-08 06:59:59 UTC is 01:59:59 EST (-5 h); a second later it's 03:00 EDT (-4 h).
        let u = 1_772_953_199.0;
        assert!((hour(local_s(u, -5 * 3600)) - (2.0 - 1.0 / 3600.0)).abs() < 1e-9);
        assert!((hour(local_s(u + 1.0, -4 * 3600)) - 3.0).abs() < 1e-9);
        // The night threshold reads the local hour, offset included: 2026-03-09 02:58 UTC is
        // 21:58 at -5 h (day) and 22:58 at -4 h (night).
        let u = 1_773_025_080.0;
        assert!(!night(local_s(u, -5 * 3600)));
        assert!(night(local_s(u, -4 * 3600)));
    }

    #[test]
    fn the_away_nap_comes_sooner_at_night() {
        let due = |a: f64| nap_due(a, NAP_AWAY_S, NAP_AWAY_NIGHT_S, |t| t);
        assert_eq!(due(at(10.0, 14.0, 0.0)), at(10.0, 14.0, 10.0));
        assert_eq!(due(at(10.0, 23.0, 0.0)), at(10.0, 23.0, 5.0));
        // Night falls between 5 and 10 min in: at 22:00.
        assert_eq!(due(at(10.0, 21.0, 53.0)), at(10.0, 22.0, 0.0));
        // Morning: back to 10 min from 06:00.
        assert_eq!(due(at(10.0, 6.0, 0.0)), at(10.0, 6.0, 10.0));
    }

    #[test]
    fn energy_bands_by_day_and_night() {
        let mut e = Energy::default();
        e.advance(0.0, Spent::Awake);
        // Fresh: high by day, mid late in the evening, low at 03:00.
        assert_eq!(band(e.value(at(1.0, 10.0, 0.0))), Band::High);
        assert_eq!(band(e.value(at(1.0, 15.0, 0.0))), Band::High);
        assert_eq!(band(e.value(at(1.0, 23.0, 0.0))), Band::Mid);
        assert_eq!(band(e.value(at(1.0, 3.0, 0.0))), Band::Low);
        // A long day with him: tired, but never low by day (fatigue is capped).
        e.advance(10.0 * HOUR, Spent::Awake);
        assert!((e.fatigue - FATIGUE_MAX).abs() < 1e-9);
        assert_eq!(band(e.value(at(1.0, 15.0, 0.0))), Band::Mid);
        assert_eq!(band(e.value(at(1.0, 23.0, 0.0))), Band::Low, "late evening, tired: low");
        // A pat lifts her a while; it fades.
        e.excite(PAT);
        assert_eq!(band(e.value(at(1.0, 23.0, 0.0))), Band::Mid);
        e.advance(10.0 * HOUR + 1800.0, Spent::Awake);
        assert_eq!(band(e.value(at(1.0, 23.0, 0.0))), Band::Low);
        // 20 min asleep takes 0.5 off.
        e.advance(10.0 * HOUR + 3000.0, Spent::Asleep);
        assert!(e.fatigue < 1e-9);
    }

    #[test]
    fn greetings_once_per_return_and_a_few_a_day() {
        let l = at(3.0, 14.0, 0.0);
        assert_eq!(greeting(Return::Unlock, false, 0, l), Some(Greet { morning: false }));
        assert_eq!(greeting(Return::Unlock, true, 0, l), None, "DND or hidden: she just wakes");
        assert_eq!(greeting(Return::Unlock, false, GREETS_PER_DAY, l), None);
        assert_eq!(greeting(Return::Back { away_s: 600.0 }, false, 0, l), None, "a short absence");
        assert_eq!(greeting(Return::Back { away_s: 2400.0 }, false, 0, l), Some(Greet { morning: false }));
        assert_eq!(greeting(Return::Unlock, false, 0, at(3.0, 8.0, 0.0)), Some(Greet { morning: true }));
        assert_eq!(greeting(Return::Unlock, false, 1, at(3.0, 8.0, 0.0)), Some(Greet { morning: false }));
        // The day turns at 05:00.
        assert_eq!(day_of(at(3.0, 4.0, 59.0)), day_of(at(2.0, 23.0, 0.0)));
        assert_eq!(day_of(at(3.0, 5.0, 0.0)), day_of(at(2.0, 23.0, 0.0)) + 1);
    }

    #[test]
    fn dozes_only_low_and_rested() {
        assert!(may_doze(0.25, DOZE_GAP_S));
        assert!(!may_doze(0.25, DOZE_GAP_S - 1.0));
        assert!(!may_doze(0.4, 10.0 * DOZE_GAP_S));
    }
}
