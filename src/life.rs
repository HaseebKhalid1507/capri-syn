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
/// before any again), never the same beat twice running.
#[derive(Default)]
pub struct Beats {
    bags: [Vec<usize>; 2],
    last: Option<usize>,
    gesture_next: bool,
    n: usize,
}

impl Beats {
    pub fn pick(&mut self, gesture: &[bool], rng: &mut crate::anim::Rng) -> usize {
        let n = gesture.len();
        if n <= 1 {
            return 0;
        }
        if n != self.n {
            *self = Beats { n, gesture_next: true, ..Beats::default() };
        }
        let has = |g: bool| gesture.contains(&g);
        let g = if has(self.gesture_next) { self.gesture_next } else { !self.gesture_next };
        let k = usize::from(g);
        if self.bags[k].is_empty() {
            let mut bag: Vec<usize> = (0..n).filter(|&i| gesture[i] == g).collect();
            for j in (1..bag.len()).rev() {
                bag.swap(j, rng.pick(j + 1));
            }
            if bag.len() > 1 && bag.last() == self.last.as_ref() {
                let m = bag.len() - 1;
                bag.swap(0, m);
            }
            self.bags[k] = bag;
        }
        let i = self.bags[k].pop().unwrap_or(0);
        self.last = Some(i);
        self.gesture_next = !g;
        i
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
