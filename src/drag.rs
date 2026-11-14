//! The ragdoll's sprite state machine (BRIEF-ragdoll §2): which frame she shows while held,
//! flung, bouncing and landing. Pure: seconds in, frame ids out. The view (round B) feeds the
//! pointer estimators and the flight contacts from ragdoll.rs.

use crate::ragdoll::{Contact, HARD_VY, HARD_WALL, V_FLING};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Head,
    Torso,
    Legs,
}

/// A timed frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub frame: String,
    pub ms: u32,
}

#[cfg(test)]
fn steps(v: &[(&str, u32)]) -> Vec<Step> {
    v.iter().map(|&(f, ms)| Step { frame: f.into(), ms }).collect()
}

/// The pack's drag / fling / land sections (Capri's values by default).
#[derive(Clone, Debug, PartialEq)]
pub struct RagSpec {
    pub grab: Vec<Step>,
    pub hang: String,
    /// (after s, frame)
    pub still: Vec<(f32, String)>,
    pub swing: Vec<Step>,
    pub shake: String,
    pub dizzy: Vec<Step>,
    pub ankle: Option<String>,
    pub zones: (f32, f32),
    pub launch: Vec<Step>,
    pub fly: String,
    pub fall: String,
    pub bounce: Vec<Step>,
    /// The tuck at the apex of a fling (a beat; `fly` the rest of the flight).
    pub tuck: Vec<Step>,
    pub annoyed_after: u32,
    pub soft: Vec<Step>,
    pub hard: Vec<Step>,
    pub perch: Vec<Step>,
    pub perch_base: String,
}

impl RagSpec {
    #[cfg(test)]
    pub fn capri() -> RagSpec {
        RagSpec {
            grab: steps(&[("surprised", 260)]),
            hang: "surprised".into(),
            still: vec![(2.5, "embarrassed".into()), (6.5, "arms_crossed-annoyed".into())],
            swing: steps(&[("idle-laughing", 900), ("happy", 600)]),
            shake: "idle-surprised_closed".into(),
            dizzy: steps(&[("idle-surprised_closed", 1100), ("concerned", 1000)]),
            ankle: Some("pose-ceiling-upside-down".into()),
            zones: (0.30, 0.62),
            launch: steps(&[("idle-surprised_closed", 90)]),
            fly: "surprised".into(),
            fall: "surprised".into(),
            bounce: steps(&[("crouch-surprised", 70)]),
            tuck: steps(&[("crouch-surprised", 70)]),
            annoyed_after: 3,
            soft: steps(&[("crouch-neutral", 140), ("transition-stand_crouch", 160)]),
            hard: steps(&[("crouch-surprised", 260), ("transition-stand_crouch", 140), ("annoyed", 900), ("arms_crossed-annoyed", 1400)]),
            perch: steps(&[("transition-edge_perch", 160)]),
            perch_base: "pose-sit-edge".into(),
        }
    }

    pub fn zone(&self, u: f32) -> Zone {
        if u < self.zones.0 {
            Zone::Head
        } else if u < self.zones.1 {
            Zone::Torso
        } else if self.ankle.is_some() {
            Zone::Legs
        } else {
            Zone::Torso
        }
    }
}

/// One scripted-drag operation (`avatarctl drag …`, BRIEF-ragdoll §6). Window-local device px
/// for grab points; ms since the grab and root px offsets from the grab point for paths.
#[derive(Clone, Debug, PartialEq)]
pub enum DragOp {
    Grab(Target),
    Path(Vec<(u32, i32, i32)>, bool),
    Release,
    Cancel,
    Fling { at: Target, v: (f32, f32), lead_ms: u32 },
    Shake { at: Target, amp: (f32, f32), hz: f32, ms: u32, release: bool },
    /// While held: a shake from where the path ends now.
    Wiggle { amp: (f32, f32), hz: f32, ms: u32 },
    Seed(u64),
}

/// Where a scripted grab lands: window-local device px, or the middle of a grip zone (an
/// opaque pixel nearest the head's column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    At(i32, i32),
    Zone(Zone),
}

/// A scripted pointer: keyframes (seconds since the grab, root offset), linear or Catmull-Rom,
/// plus an optional sinusoidal shake on top; the release time if one was asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    keys: Vec<(f32, f32, f32)>,
    smooth: bool,
    shake: Option<(f32, f32, f32, f32, f32)>,
    pub release_at: Option<f32>,
}

impl Script {
    /// Keyframes at `ms` (appended; an earlier time than the last is moved after it).
    pub fn path(&mut self, keys: &[(u32, i32, i32)], smooth: bool) {
        for &(ms, x, y) in keys {
            let t = (ms as f32 / 1000.0).max(self.keys.last().map_or(0.0, |k| k.0));
            self.keys.push((t, x as f32, y as f32));
        }
        self.smooth |= smooth;
    }

    /// Speed up from rest to `v` (px/s) over `lead` s, keep it 80 ms (the release estimate
    /// sees a steady speed), then release.
    pub fn fling(v: (f32, f32), lead: f32) -> Script {
        let lead = lead.max(0.02);
        let mut s = Script::default();
        let n = 12;
        for i in 0..=n {
            let t = lead * i as f32 / n as f32;
            let k = 0.5 * t * t / lead;
            s.keys.push((t, v.0 * k, v.1 * k));
        }
        let (x, y) = (v.0 * lead * 0.5, v.1 * lead * 0.5);
        s.keys.push((lead + 0.08, x + v.0 * 0.08, y + v.1 * 0.08));
        s.release_at = Some(lead + 0.08);
        s
    }

    pub fn shake(amp: (f32, f32), hz: f32, ms: u32, release: bool) -> Script {
        let dur = ms as f32 / 1000.0;
        Script { keys: vec![(0.0, 0.0, 0.0)], smooth: false, shake: Some((amp.0, amp.1, hz.max(0.1), 0.0, dur)), release_at: release.then_some(dur) }
    }

    /// A shake from the end of the path so far (held there meanwhile).
    pub fn wiggle(&mut self, amp: (f32, f32), hz: f32, ms: u32) {
        let t0 = self.keys.last().map_or(0.0, |k| k.0);
        let dur = ms as f32 / 1000.0;
        if let Some(&(_, x, y)) = self.keys.last() {
            self.keys.push((t0 + dur, x, y));
        }
        self.shake = Some((amp.0, amp.1, hz.max(0.1), t0, dur));
    }

    /// When the scripted motion is over (s since the grab).
    pub fn ends_at(&self) -> f32 {
        let k = self.keys.last().map_or(0.0, |k| k.0);
        let s = self.shake.map_or(0.0, |s| s.3 + s.4);
        k.max(s).max(self.release_at.unwrap_or(0.0))
    }

    /// The root offset from the grab point at `t` s.
    pub fn at(&self, t: f32) -> (f32, f32) {
        let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
        let mut p = match self.keys.len() {
            0 => (0.0, 0.0),
            _ if t <= self.keys[0].0 => (self.keys[0].1, self.keys[0].2),
            n if t >= self.keys[n - 1].0 => (self.keys[n - 1].1, self.keys[n - 1].2),
            n => {
                let i = (1..n).find(|&i| self.keys[i].0 >= t).unwrap_or(n - 1);
                let (a, b) = (self.keys[i - 1], self.keys[i]);
                let u = if b.0 > a.0 { (t - a.0) / (b.0 - a.0) } else { 1.0 };
                if self.smooth {
                    let p0 = if i >= 2 { self.keys[i - 2] } else { a };
                    let p3 = if i + 1 < n { self.keys[i + 1] } else { b };
                    let cr = |p0: f32, p1: f32, p2: f32, p3: f32| {
                        let (u2, u3) = (u * u, u * u * u);
                        0.5 * (2.0 * p1 + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3)
                    };
                    (cr(p0.1, a.1, b.1, p3.1), cr(p0.2, a.2, b.2, p3.2))
                } else {
                    (a.1 + (b.1 - a.1) * u, a.2 + (b.2 - a.2) * u)
                }
            }
        };
        if let Some((ax, ay, hz, t0, dur)) = self.shake
            && t >= t0
            && t <= t0 + dur
        {
            let w = (std::f32::consts::TAU * hz * (t - t0)).sin();
            p.0 += ax * w;
            p.1 += ay * w;
        }
        p
    }
}

pub const STILL_CURSOR: f32 = 40.0;
pub const STILL_FEET: f32 = 30.0;
pub const MOTION_RESET: f32 = 300.0;
pub const SWING_AMP: (f32, f32) = (25.0, 110.0);
pub const SWING_HOLD: f32 = 1.2;
pub const SWING_CURSOR_MAX: f32 = 650.0;
pub const SHAKE_REVERSALS: usize = 3;
pub const DIZZY_AFTER: f32 = 0.45;
pub const FLING_STREAK_S: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RagEvent {
    /// The 5 px drag threshold passed; `u` = grip row in the frame's opaque span (0 top, 1 bottom).
    Grab { u: f32 },
    /// Released with this velocity.
    Release { vel: (f32, f32) },
    Contact(Contact),
    /// The landing squash settled.
    Settled,
    /// A press on her mid-flight.
    Catch { u: f32 },
    /// A fling passed the top of its arc.
    Apex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RagInput {
    pub event: Option<RagEvent>,
    pub cursor_speed: f32,
    /// Reversals in the last 800 ms and seconds since the last one.
    pub reversals: usize,
    pub since_reversal: Option<f32>,
    /// Feet sway amplitude (px) and speed (px/s) from the chain.
    pub feet_amp: f32,
    pub feet_speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldState {
    Grab,
    Hang,
    Still(usize),
    Swing,
    Shake,
    Dizzy,
    Ankle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandKind {
    Soft,
    Hard,
    Perch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Held { zone: Zone, state: HeldState },
    Flight { fling: bool },
    Land(LandKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RagOut {
    pub phase: Phase,
    /// The frame to show now (None: the mood base, i.e. not ragdolling).
    pub frame: Option<String>,
    /// Timed steps playing, and when they end (machine time).
    pub steps: Option<(Vec<Step>, f32)>,
    /// One-shot flags for the view.
    pub flip: bool,
    pub glitch: bool,
    pub squash: Option<f32>,
}

pub struct RagMachine {
    pub spec: RagSpec,
    phase: Phase,
    /// The step list playing and its start.
    playing: Option<(Vec<Step>, f32)>,
    /// Then this frame.
    after: Option<String>,
    still_since: Option<f32>,
    swing_since: Option<f32>,
    flings: Vec<f32>,
    shook: bool,
    settled: bool,
}

impl RagMachine {
    pub fn new(spec: RagSpec) -> RagMachine {
        RagMachine { spec, phase: Phase::Idle, playing: None, after: None, still_since: None, swing_since: None, flings: Vec::new(), shook: false, settled: false }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The next landing is hard whatever its speed (the brain's fling offence).
    pub fn force_hard(&mut self, t: f32) {
        let n = self.spec.annoyed_after.max(1) as usize;
        self.flings.extend(std::iter::repeat_n(t, n));
    }

    /// When the playing clip or the next still stage changes the frame (machine time), so a
    /// held, still, asleep chain needs no ticks until then.
    pub fn next_change(&self, t: f32) -> Option<f32> {
        let clip = self.playing_end().filter(|&e| e > t);
        let still = match (self.phase, self.still_since) {
            (Phase::Held { state: HeldState::Hang | HeldState::Still(_), .. }, Some(s)) => {
                self.spec.still.iter().map(|(a, _)| s + a).find(|&x| x > t)
            }
            _ => None,
        };
        match (clip, still) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn play(&mut self, t: f32, s: Vec<Step>, then: Option<String>) {
        self.playing = if s.is_empty() { None } else { Some((s, t)) };
        self.after = then;
    }

    fn playing_frame(&self, t: f32) -> Option<(String, f32)> {
        let (s, t0) = self.playing.as_ref()?;
        let mut acc = *t0;
        for st in s {
            acc += st.ms as f32 / 1000.0;
            if t < acc {
                return Some((st.frame.clone(), acc));
            }
        }
        None
    }

    fn playing_end(&self) -> Option<f32> {
        self.playing.as_ref().map(|(s, t0)| t0 + s.iter().map(|x| x.ms as f32 / 1000.0).sum::<f32>())
    }

    fn held(&mut self, t: f32, zone: Zone, state: HeldState) {
        self.phase = Phase::Held { zone, state };
        let _ = t;
    }

    pub fn update(&mut self, t: f32, i: &RagInput) -> RagOut {
        let mut out_flip = false;
        let mut glitch = false;
        let mut squash: Option<f32> = None;
        if let Some(ev) = i.event {
            match (self.phase, ev) {
                (Phase::Idle | Phase::Land(_), RagEvent::Grab { u }) | (Phase::Flight { .. }, RagEvent::Catch { u }) => {
                    let zone = self.spec.zone(u);
                    self.still_since = None;
                    self.swing_since = None;
                    self.shook = false;
                    if zone == Zone::Legs {
                        out_flip = true;
                        self.play(t, Vec::new(), None);
                        self.held(t, zone, HeldState::Ankle);
                    } else {
                        let g = self.spec.grab.clone();
                        self.play(t, g, None);
                        self.held(t, zone, HeldState::Grab);
                    }
                }
                (Phase::Held { .. }, RagEvent::Release { vel }) => {
                    let sp = (vel.0 * vel.0 + vel.1 * vel.1).sqrt();
                    let fling = sp >= V_FLING;
                    if fling {
                        self.flings.push(t);
                        let l = self.spec.launch.clone();
                        let fly = self.spec.fly.clone();
                        self.play(t, l, Some(fly));
                    } else {
                        let fall = self.spec.fall.clone();
                        self.play(t, Vec::new(), Some(fall));
                    }
                    self.phase = Phase::Flight { fling };
                }
                (Phase::Flight { fling: true }, RagEvent::Apex) => {
                    let k = self.spec.tuck.clone();
                    let fly = self.spec.fly.clone();
                    self.play(t, k, Some(fly));
                }
                (Phase::Flight { fling }, RagEvent::Contact(c)) => match c {
                    Contact::Bounce { speed, normal } => {
                        let b = self.spec.bounce.clone();
                        let base = if fling { self.spec.fly.clone() } else { self.spec.fall.clone() };
                        self.play(t, b, Some(base));
                        squash = Some(speed);
                        if normal.0 != 0.0 && speed >= HARD_WALL {
                            // A wall slam this hard makes the landing hard.
                            self.flings.extend([t, t, t]);
                        }
                    }
                    Contact::Floor { speed } => {
                        self.flings.retain(|&f| f > t - FLING_STREAK_S);
                        let streak = self.flings.len() as u32 >= self.spec.annoyed_after;
                        let hard = speed >= HARD_VY || streak;
                        if hard {
                            self.flings.clear();
                        }
                        let s = if hard { self.spec.hard.clone() } else { self.spec.soft.clone() };
                        self.play(t, s, None);
                        glitch = hard;
                        squash = Some(speed);
                        self.settled = false;
                        self.phase = Phase::Land(if hard { LandKind::Hard } else { LandKind::Soft });
                    }
                    Contact::Perch { speed, .. } => {
                        let s = self.spec.perch.clone();
                        let base = self.spec.perch_base.clone();
                        self.play(t, s, Some(base));
                        squash = Some(speed);
                        self.settled = false;
                        self.phase = Phase::Land(LandKind::Perch);
                    }
                    Contact::None => {}
                },
                (Phase::Land(_), RagEvent::Settled) => {}
                _ => {}
            }
        }
        // Timers.
        match self.phase {
            Phase::Held { zone, state } => {
                let next = self.held_next(t, zone, state, i);
                if next != state {
                    self.enter_held(t, zone, next);
                }
            }
            Phase::Land(kind) => {
                // Idle once the squash settled and the clip is done (a perch stays on its base).
                if matches!(i.event, Some(RagEvent::Settled)) {
                    self.settled = true;
                }
                let clip_done = self.playing_end().is_none_or(|e| t >= e);
                if clip_done && self.settled {
                    self.phase = Phase::Idle;
                    self.playing = None;
                    self.after = if kind == LandKind::Perch { Some(self.spec.perch_base.clone()) } else { None };
                }
            }
            _ => {}
        }
        let (frame, steps) = self.frame(t);
        RagOut { phase: self.phase, frame, steps, flip: out_flip, glitch, squash }
    }

    fn held_next(&mut self, t: f32, zone: Zone, state: HeldState, i: &RagInput) -> HeldState {
        if state == HeldState::Ankle {
            return HeldState::Ankle;
        }
        if state == HeldState::Grab && self.playing_end().is_some_and(|e| t < e) {
            return HeldState::Grab;
        }
        let state = if state == HeldState::Grab { HeldState::Hang } else { state };
        // Shake beats everything; dizzy after it calms.
        if i.reversals >= SHAKE_REVERSALS {
            self.shook = true;
            return HeldState::Shake;
        }
        if state == HeldState::Shake {
            return if i.since_reversal.is_some_and(|s| s >= DIZZY_AFTER) { HeldState::Dizzy } else { HeldState::Shake };
        }
        if state == HeldState::Dizzy {
            return if self.playing_end().is_some_and(|e| t < e) { HeldState::Dizzy } else { HeldState::Hang };
        }
        let _ = zone;
        if i.cursor_speed > MOTION_RESET {
            self.still_since = None;
        }
        // Swing: feet amplitude in range for SWING_HOLD, slow cursor, no recent shake.
        let swinging = (SWING_AMP.0..=SWING_AMP.1).contains(&i.feet_amp)
            && i.cursor_speed < SWING_CURSOR_MAX
            && i.since_reversal.is_none_or(|s| s >= SWING_HOLD);
        if swinging {
            let since = *self.swing_since.get_or_insert(t);
            if t - since >= SWING_HOLD {
                return HeldState::Swing;
            }
        } else {
            self.swing_since = None;
            if state == HeldState::Swing {
                return HeldState::Hang;
            }
        }
        if state == HeldState::Swing {
            return HeldState::Swing;
        }
        let still = i.cursor_speed < STILL_CURSOR && i.feet_speed < STILL_FEET;
        if still {
            let since = *self.still_since.get_or_insert(t);
            let k = self.spec.still.iter().rposition(|(after, _)| t - since >= *after);
            return k.map_or(HeldState::Hang, HeldState::Still);
        }
        if i.cursor_speed > MOTION_RESET || matches!(state, HeldState::Hang) {
            self.still_since = None;
            return HeldState::Hang;
        }
        // In between (slow but not still): keep the stage.
        state
    }

    fn enter_held(&mut self, t: f32, zone: Zone, s: HeldState) {
        match s {
            HeldState::Swing => {
                let sw = self.spec.swing.clone();
                self.play(t, sw, None);
            }
            HeldState::Dizzy => {
                let d = self.spec.dizzy.clone();
                self.play(t, d, None);
            }
            _ => self.play(t, Vec::new(), None),
        }
        self.held(t, zone, s);
    }

    fn frame(&self, t: f32) -> (Option<String>, Option<(Vec<Step>, f32)>) {
        let steps = self.playing.as_ref().map(|(s, _)| (s.clone(), self.playing_end().unwrap_or(t)));
        match self.phase {
            Phase::Idle => (None, None),
            Phase::Held { state, .. } => {
                let f = match state {
                    HeldState::Ankle => self.spec.ankle.clone().unwrap_or_else(|| self.spec.hang.clone()),
                    HeldState::Grab | HeldState::Dizzy => self.playing_frame(t).map(|x| x.0).unwrap_or_else(|| self.spec.hang.clone()),
                    HeldState::Swing => {
                        // Loops.
                        let (s, t0) = self.playing.as_ref().map(|(s, t0)| (s.clone(), *t0)).unwrap_or_default();
                        let total: f32 = s.iter().map(|x| x.ms as f32 / 1000.0).sum();
                        if total > 0.0 {
                            let mut u = (t - t0).rem_euclid(total);
                            let mut f = s[0].frame.clone();
                            for st in &s {
                                let d = st.ms as f32 / 1000.0;
                                if u < d {
                                    f = st.frame.clone();
                                    break;
                                }
                                u -= d;
                            }
                            f
                        } else {
                            self.spec.hang.clone()
                        }
                    }
                    HeldState::Still(k) => self.spec.still[k].1.clone(),
                    HeldState::Shake => self.spec.shake.clone(),
                    HeldState::Hang => self.spec.hang.clone(),
                };
                (Some(f), steps)
            }
            Phase::Flight { .. } | Phase::Land(_) => {
                let f = self.playing_frame(t).map(|x| x.0).or_else(|| self.after.clone());
                (f, steps)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_paths() {
        let mut s = Script::default();
        s.path(&[(0, 0, 0), (400, 0, -140), (900, 240, -140)], false);
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        assert!(near(s.at(0.2), (0.0, -70.0)));
        assert!(near(s.at(0.65), (120.0, -140.0)), "{:?}", s.at(0.65));
        assert_eq!(s.at(5.0), (240.0, -140.0));
        assert!((s.ends_at() - 0.9).abs() < 1e-6);
        // Fling: the release estimate sees the asked speed.
        let f = Script::fling((2600.0, -1400.0), 0.12);
        let r = f.release_at.unwrap();
        let mut p = crate::ragdoll::Pointer::default();
        let mut t = 0.0;
        while t <= r + 1e-6 {
            let (x, y) = f.at(t);
            p.push(t, x, y);
            t += 1.0 / 60.0;
        }
        let v = p.velocity(r);
        assert!((v.0 - 2600.0).abs() < 2600.0 * 0.08 && (v.1 + 1400.0).abs() < 1400.0 * 0.08, "{v:?}");
        // Shake: back at the rest point after whole cycles; reversals counted.
        let sh = Script::shake((150.0, 0.0), 5.0, 1200, false);
        assert!(sh.at(1.2).0.abs() < 1e-2);
        let mut p = crate::ragdoll::Pointer::default();
        let mut t = 0.0;
        while t < 1.2 {
            let (x, y) = sh.at(t);
            p.push(t, x, y);
            t += 1.0 / 60.0;
        }
        assert!(p.reversals(1.2).0 >= SHAKE_REVERSALS);
    }

    fn ev(e: RagEvent) -> RagInput {
        RagInput { event: Some(e), ..RagInput::default() }
    }

    fn moving(speed: f32) -> RagInput {
        RagInput { cursor_speed: speed, feet_speed: speed, ..RagInput::default() }
    }

    #[test]
    fn grab_then_hang_then_still_stages() {
        let mut m = RagMachine::new(RagSpec::capri());
        let o = m.update(0.0, &ev(RagEvent::Grab { u: 0.1 }));
        assert_eq!((o.frame.as_deref(), o.phase), (Some("surprised"), Phase::Held { zone: Zone::Head, state: HeldState::Grab }));
        let o = m.update(0.3, &moving(0.0));
        assert_eq!(o.phase, Phase::Held { zone: Zone::Head, state: HeldState::Hang });
        assert_eq!(m.update(2.7, &moving(0.0)).frame.as_deref(), Some("surprised"));
        assert_eq!(m.update(2.81, &moving(0.0)).frame.as_deref(), Some("embarrassed"));
        assert_eq!(m.update(6.81, &moving(0.0)).frame.as_deref(), Some("arms_crossed-annoyed"));
        // Motion over 300 px/s: back to hang, the timer restarts.
        assert_eq!(m.update(7.0, &moving(400.0)).frame.as_deref(), Some("surprised"));
        assert_eq!(m.update(8.0, &moving(0.0)).frame.as_deref(), Some("surprised"));
        assert_eq!(m.update(10.6, &moving(0.0)).frame.as_deref(), Some("embarrassed"));
    }

    #[test]
    fn swing_loops_and_ends() {
        let mut m = RagMachine::new(RagSpec::capri());
        m.update(0.0, &ev(RagEvent::Grab { u: 0.5 }));
        let sw = RagInput { cursor_speed: 100.0, feet_amp: 60.0, feet_speed: 200.0, ..RagInput::default() };
        m.update(0.3, &sw);
        assert_eq!(m.update(1.4, &sw).phase, Phase::Held { zone: Zone::Torso, state: HeldState::Hang });
        assert_eq!(m.update(1.6, &sw).frame.as_deref(), Some("idle-laughing"));
        assert_eq!(m.update(2.6, &sw).frame.as_deref(), Some("happy"));
        assert_eq!(m.update(3.2, &sw).frame.as_deref(), Some("idle-laughing"), "loops");
        assert_eq!(m.update(3.3, &moving(0.0)).phase, Phase::Held { zone: Zone::Torso, state: HeldState::Hang });
    }

    #[test]
    fn shake_then_dizzy_then_hang() {
        let mut m = RagMachine::new(RagSpec::capri());
        m.update(0.0, &ev(RagEvent::Grab { u: 0.2 }));
        let shake = RagInput { cursor_speed: 1500.0, reversals: 3, since_reversal: Some(0.05), ..RagInput::default() };
        assert_eq!(m.update(0.5, &shake).frame.as_deref(), Some("idle-surprised_closed"));
        let calm = |s| RagInput { cursor_speed: 10.0, reversals: 0, since_reversal: Some(s), ..RagInput::default() };
        assert_eq!(m.update(0.7, &calm(0.25)).phase, Phase::Held { zone: Zone::Head, state: HeldState::Shake });
        let o = m.update(0.95, &calm(0.5));
        assert_eq!(o.phase, Phase::Held { zone: Zone::Head, state: HeldState::Dizzy });
        // Eyes shut long enough to read (1.1 s), then concerned, then the hang.
        assert_eq!(m.update(1.5, &calm(1.0)).frame.as_deref(), Some("idle-surprised_closed"));
        assert_eq!(m.update(2.4, &calm(2.0)).frame.as_deref(), Some("concerned"));
        assert_eq!(m.update(3.4, &calm(3.0)).phase, Phase::Held { zone: Zone::Head, state: HeldState::Hang });
    }

    #[test]
    fn ankle_flips_and_locks() {
        let mut m = RagMachine::new(RagSpec::capri());
        let o = m.update(0.0, &ev(RagEvent::Grab { u: 0.8 }));
        assert!(o.flip);
        assert_eq!(o.frame.as_deref(), Some("pose-ceiling-upside-down"));
        let shake = RagInput { cursor_speed: 1500.0, reversals: 4, since_reversal: Some(0.0), ..RagInput::default() };
        assert_eq!(m.update(1.0, &shake).frame.as_deref(), Some("pose-ceiling-upside-down"), "one frame through every state");
        // No ankle art: Legs act as Torso.
        let mut m = RagMachine::new(RagSpec { ankle: None, ..RagSpec::capri() });
        assert_eq!(m.update(0.0, &ev(RagEvent::Grab { u: 0.8 })).phase, Phase::Held { zone: Zone::Torso, state: HeldState::Grab });
    }

    #[test]
    fn fling_bounce_land() {
        let mut m = RagMachine::new(RagSpec::capri());
        m.update(0.0, &ev(RagEvent::Grab { u: 0.4 }));
        let o = m.update(1.0, &ev(RagEvent::Release { vel: (1500.0, -200.0) }));
        assert_eq!((o.phase, o.frame.as_deref()), (Phase::Flight { fling: true }, Some("idle-surprised_closed")));
        // Open (head, arms, legs readable) most of the flight; tucked only a beat at the apex
        // and at a bounce.
        assert_eq!(m.update(1.1, &RagInput::default()).frame.as_deref(), Some("surprised"));
        assert_eq!(m.update(1.15, &ev(RagEvent::Apex)).frame.as_deref(), Some("crouch-surprised"));
        assert_eq!(m.update(1.25, &RagInput::default()).frame.as_deref(), Some("surprised"));
        let o = m.update(1.3, &ev(RagEvent::Contact(Contact::Bounce { normal: (-1.0, 0.0), speed: 1200.0 })));
        assert_eq!((o.frame.as_deref(), o.squash), (Some("crouch-surprised"), Some(1200.0)));
        assert_eq!(m.update(1.5, &RagInput::default()).frame.as_deref(), Some("surprised"));
        let o = m.update(1.8, &ev(RagEvent::Contact(Contact::Floor { speed: 800.0 })));
        assert_eq!((o.phase, o.frame.as_deref(), o.glitch), (Phase::Land(LandKind::Soft), Some("crouch-neutral"), false));
        assert_eq!(m.update(2.0, &RagInput::default()).frame.as_deref(), Some("transition-stand_crouch"));
        assert_eq!(m.update(2.2, &ev(RagEvent::Settled)).phase, Phase::Idle);
    }

    #[test]
    fn hard_landing_and_streak() {
        let mut m = RagMachine::new(RagSpec::capri());
        m.update(0.0, &ev(RagEvent::Grab { u: 0.4 }));
        m.update(0.5, &ev(RagEvent::Release { vel: (0.0, 100.0) }));
        assert_eq!(m.update(0.6, &RagInput::default()).frame.as_deref(), Some("surprised"), "a drop falls");
        let o = m.update(1.0, &ev(RagEvent::Contact(Contact::Floor { speed: 2000.0 })));
        assert_eq!((o.phase, o.glitch), (Phase::Land(LandKind::Hard), true));
        assert_eq!(m.update(1.5, &RagInput::default()).frame.as_deref(), Some("annoyed"));
        m.update(4.0, &ev(RagEvent::Settled));
        // Three flings within 12 s: the third landing is hard even when soft.
        let mut hard = Vec::new();
        for k in 0..3 {
            let t = 10.0 + k as f32 * 3.0;
            m.update(t, &ev(RagEvent::Grab { u: 0.4 }));
            m.update(t + 0.5, &ev(RagEvent::Release { vel: (1200.0, 0.0) }));
            hard.push(m.update(t + 1.0, &ev(RagEvent::Contact(Contact::Floor { speed: 500.0 }))).glitch);
            m.update(t + 2.9, &ev(RagEvent::Settled));
        }
        assert_eq!(hard, vec![false, false, true]);
    }

    #[test]
    fn perch_and_catch() {
        let mut m = RagMachine::new(RagSpec::capri());
        m.update(0.0, &ev(RagEvent::Grab { u: 0.4 }));
        m.update(0.5, &ev(RagEvent::Release { vel: (1000.0, 0.0) }));
        // Caught mid-flight: held again.
        let o = m.update(0.8, &ev(RagEvent::Catch { u: 0.1 }));
        assert_eq!(o.phase, Phase::Held { zone: Zone::Head, state: HeldState::Grab });
        m.update(1.2, &ev(RagEvent::Release { vel: (100.0, 0.0) }));
        let o = m.update(1.5, &ev(RagEvent::Contact(Contact::Perch { win: 3, x: 10.0, speed: 600.0 })));
        assert_eq!((o.phase, o.frame.as_deref()), (Phase::Land(LandKind::Perch), Some("transition-edge_perch")));
        assert_eq!(m.update(1.8, &RagInput::default()).frame.as_deref(), Some("pose-sit-edge"));
        // A catch during the landing is ignored (a normal press instead).
        assert_eq!(m.update(1.9, &ev(RagEvent::Catch { u: 0.1 })).phase, Phase::Land(LandKind::Perch));
    }
}
