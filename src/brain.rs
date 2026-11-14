//! Her brain: a cat on the desktop. Two drives decide whether she roams on her own (roam R) or
//! comes over for attention (attention A); a session that needs him summons her (urgency).
//!
//! Pure: time is `f64` brain seconds (wall clock × scale), randomness a seeded `Rng`; no X, no
//! `Instant`. The view builds `Senses` at wake-ups, feeds events, and applies the intents.
//!
//! Exact wake-ups: every timer (A crossing θ, R reaching 1, bid stages, linger, sulk, DND end,
//! the presence poll) is computed in closed form, and `Decision::wake_at` is the earliest. Between
//! two of them nothing runs. Senses are only *read* at poll steps, so ticking at every brain
//! second or only at the returned wake-ups gives the same decisions (test `exact_wakes`).

use crate::anim::Rng;
use crate::settings::{Attention, BrainSettings, Fullscreen};
use std::collections::{HashMap, VecDeque};

// ---- vocabulary ------------------------------------------------------------------------

/// How she shows up next to his work for a bid or a summon (BRIEF-brain §14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Approach {
    /// Gripping the bar's bottom edge over the focused window's top corner farther from the cursor.
    Hang,
    HangUpsideDown,
    /// On the focused window's side edge (upper third), the side farther from the cursor.
    Cling,
    /// Sitting on the focused window's top edge (when there is head room).
    SitEdge,
    /// Crouched at the focused window's bottom corner farther from the cursor.
    Peek,
    /// The floor beside the focused window, the adjacent monitor, floating beside it, in place.
    Beside,
}

impl Approach {
    pub const ALL: [Approach; 6] = [Approach::Hang, Approach::HangUpsideDown, Approach::Cling, Approach::SitEdge, Approach::Peek, Approach::Beside];
    pub fn key(self) -> &'static str {
        match self {
            Approach::Hang => "hang",
            Approach::HangUpsideDown => "hang_upside_down",
            Approach::Cling => "cling",
            Approach::SitEdge => "sit_edge",
            Approach::Peek => "peek",
            Approach::Beside => "beside",
        }
    }
    pub fn parse(s: &str) -> Option<Approach> {
        Approach::ALL.into_iter().find(|a| a.key() == s.trim())
    }
    /// May overlap the focused window's corner (the cover exception).
    pub fn covers(self) -> bool {
        matches!(self, Approach::Hang | Approach::HangUpsideDown | Approach::Cling | Approach::Peek)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arrival {
    /// Vanish → bolt → land (the RESET POSITION move).
    Teleport,
    /// Vanish, then appear there: no bolt (short hops).
    Glitch,
    /// Falls from above the line and catches it (needs the ragdoll: weight 0 for now).
    DropIn,
    /// A hop along a short arc (within ~300 px only).
    Hop,
    /// Float to float: an eased drift (a transport the view picks by distance, no weight).
    Drift,
}

impl Arrival {
    pub const ALL: [Arrival; 5] = [Arrival::Teleport, Arrival::Glitch, Arrival::DropIn, Arrival::Hop, Arrival::Drift];
    pub fn key(self) -> &'static str {
        match self {
            Arrival::Teleport => "teleport",
            Arrival::Glitch => "glitch",
            Arrival::DropIn => "drop_in",
            Arrival::Hop => "hop",
            Arrival::Drift => "drift",
        }
    }
    pub fn parse(s: &str) -> Option<Arrival> {
        Arrival::ALL.into_iter().find(|a| a.key() == s.trim())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    Headpat,
    Click,
    DragStart,
    Chip,
    Typing,
    Submit,
    PromptAnswer,
    Menu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummonKind {
    Prompt(u64),
    Done,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSignal {
    /// A new pending prompt (NeedsYou).
    Prompt(u64),
    /// The turn finished.
    Done,
    /// Aborted or failed; `own_abort`: his own abort < 3 s ago (that's him, not news).
    Error { own_abort: bool },
    /// The prompt is answered, or he typed to her.
    Cleared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    Roam,
    Seek,
    Summon,
    Sulk,
    Nap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ctl {
    On,
    Off,
    /// DND for this many brain seconds.
    Dnd(f64),
    Undnd,
    Force(Force),
    Seed(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrived {
    /// The approach used (None for a passive move).
    pub approach: Option<Approach>,
    pub arrival: Arrival,
    /// Her body overlaps the focused window (cover exception): she must leave after.
    pub covering: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrainEvent {
    Interaction(Touch),
    /// Released as a fling (ragdoll, R-P3: the view sends these in round B).
    #[cfg_attr(not(test), allow(dead_code))]
    Flung { speed: f32 },
    #[allow(dead_code)]
    Landed { hard: bool },
    Arrived(Arrived),
    /// The view couldn't do the move: nothing fits, or it is busy/gated right now.
    MoveFailed { busy: bool },
    Session(SessionSignal),
    Control(Ctl),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Attention,
    Summon(SummonKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// Roam: a kind from places.rs (edge, floor, floor-sit, float). `far`: ≥ 900 px from the
    /// cursor, another window than now (after a sulk or a fling).
    Passive { far: bool },
    /// Come over: try the approaches in this order, then the `beside` chain.
    Active { why: Why, approaches: Vec<Approach> },
    /// Leave his window after a bid that covered it (a quiet passive spot).
    Leave,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MovePlan {
    pub goal: Goal,
    /// The rule that asked for it (the decision log, `brain state`): roam, sulk_far, bid,
    /// summon, covering.
    pub why: &'static str,
    /// Arrival styles in order; the view takes the first that fits the distance.
    pub arrivals: Vec<Arrival>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmoteKey {
    /// Entry `i` of the pack's `brain.bid` pool.
    Bid(usize),
    /// Insist stage 1..=ESCALATIONS.
    Insist(u8),
    Satisfied { pat: bool },
    Sulk,
    /// A summon for a finished turn or an error landed: one wave.
    Wave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestPose {
    FloorSit,
    Nap,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Move(MovePlan),
    Emote(EmoteKey),
    Pose(Option<RestPose>),
    /// Fullscreen on her monitor (hide mode): glitch out (true) / back in (false).
    Hide(bool),
    /// Open the reply bubble (a Done/Error summon landed next to his work).
    Unfold,
    /// Persist the drives.
    Save,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub intents: Vec<(f64, Intent)>,
    pub wake_at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlaceKind {
    #[default]
    Ground,
    Perch,
    Float,
    /// Hanging or clinging (an approach pose).
    Hang,
}

/// What the view sees at a wake-up (cheap reads only).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Senses {
    pub now: f64,
    /// Brain seconds since the last keyboard/mouse input anywhere.
    pub idle_s: f64,
    /// A fullscreen window on her monitor, or the focused window is fullscreen.
    pub fullscreen: bool,
    /// A fullscreen window on her own monitor (fullscreen = "hide").
    pub fullscreen_here: bool,
    /// Why she must not move now (bubble, field, menu, reaction, teleport, music, session busy).
    pub busy: Vec<&'static str>,
    /// He is near her: her monitor has the focus and she is within reach of the focused window.
    pub near: bool,
    pub placement: PlaceKind,
}

// ---- tuning ------------------------------------------------------------------------------

/// Every constant in one place (brain seconds). From the settings, the rest are the brief's.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub attn_fill_s: f64,
    pub theta: (f64, f64),
    pub theta_raise: f64,
    pub active_gap_min_s: f64,
    pub roam_s: (f64, f64),
    pub move_cap_per_hour: u32,
    pub launch_grace_s: f64,
    pub first_roam_min_s: f64,
    pub away_after_s: f64,
    pub back_idle_s: f64,
    pub nap_after_s: f64,
    pub bid_stage_s: f64,
    pub bid_emote_s: (f64, f64),
    pub escalations: u8,
    pub sulk_s: f64,
    pub linger_s: (f64, f64),
    pub offense_window_s: f64,
    pub summon_gap_s: f64,
    pub resummon_s: f64,
    pub summons_per_prompt: u8,
    /// A prompt summon stays this long, a done/error one `summon_note_s`; then she goes back.
    pub summon_hold_s: f64,
    pub summon_note_s: f64,
    pub poll_s: (f64, f64),
    /// A bid that found no spot (and she isn't on his monitor) retries after this.
    pub retry_s: f64,
    /// No Arrived/MoveFailed by then: the move counts as failed.
    pub seek_timeout_s: f64,
    /// After the satisfied emote, she leaves a window she covers.
    pub leave_after_s: f64,
    /// After he comes back, no bid for this long.
    pub back_grace_s: f64,
    /// Number of entries in the pack's bid pool.
    pub bid_pool: usize,
}

impl Tuning {
    pub fn new(s: &BrainSettings) -> Tuning {
        let fill = 1500.0;
        Tuning {
            attn_fill_s: fill,
            theta: (s.bid_every_min.0 * 60.0 / fill, s.bid_every_min.1 * 60.0 / fill),
            theta_raise: 0.3,
            active_gap_min_s: s.bid_every_min.0 * 60.0,
            roam_s: (s.passive_every_min.0 * 60.0, s.passive_every_min.1 * 60.0),
            move_cap_per_hour: s.max_moves_per_hour,
            launch_grace_s: 600.0,
            first_roam_min_s: 90.0,
            away_after_s: 300.0,
            back_idle_s: 3.0,
            nap_after_s: 600.0,
            bid_stage_s: 20.0,
            bid_emote_s: (6.0, 10.0),
            escalations: 2,
            sulk_s: 6.0,
            linger_s: (60.0, 150.0),
            offense_window_s: 90.0,
            summon_gap_s: 120.0,
            resummon_s: 300.0,
            summons_per_prompt: 3,
            summon_hold_s: 180.0,
            summon_note_s: 45.0,
            poll_s: (15.0, 3.0),
            retry_s: 300.0,
            seek_timeout_s: 30.0,
            leave_after_s: 3.0,
            back_grace_s: 120.0,
            bid_pool: 4,
        }
    }
}

// ---- shuffle bag ------------------------------------------------------------------------

/// Weighted picks that never reuse the last `no_repeat` choices (when anything else is left).
#[derive(Clone, Debug, PartialEq)]
pub struct Bag<T> {
    pub history: VecDeque<T>,
}

impl<T> Default for Bag<T> {
    fn default() -> Self {
        Bag { history: VecDeque::new() }
    }
}

impl<T: Copy + PartialEq> Bag<T> {
    /// Every item with weight > 0, in pick order: the ones not used in the last `no_repeat`
    /// picks first (weighted, without replacement), then the recent ones, oldest use first.
    pub fn order(&self, weights: &[(T, f64)], no_repeat: usize, rng: &mut Rng) -> Vec<T> {
        let recent: Vec<T> = self.history.iter().rev().take(no_repeat).copied().collect();
        let mut fresh: Vec<(T, f64)> = weights.iter().copied().filter(|(t, w)| *w > 0.0 && !recent.contains(t)).collect();
        let mut out = Vec::new();
        while !fresh.is_empty() {
            let total: f64 = fresh.iter().map(|p| p.1).sum();
            let mut r = rng.range(0.0, 1.0) as f64 * total;
            let mut k = fresh.len() - 1;
            for (i, p) in fresh.iter().enumerate() {
                if r < p.1 {
                    k = i;
                    break;
                }
                r -= p.1;
            }
            out.push(fresh.remove(k).0);
        }
        // Recent ones last, the oldest first.
        for t in recent.iter().rev() {
            if weights.iter().any(|(x, w)| x == t && *w > 0.0) && !out.contains(t) {
                out.push(*t);
            }
        }
        out
    }

    pub fn used(&mut self, t: T) {
        self.history.push_back(t);
        while self.history.len() > 8 {
            self.history.pop_front();
        }
    }
}

// ---- state --------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Roam via R.
    Passive,
    /// A Move is out; waiting for Arrived / MoveFailed.
    Seeking { why: Why, since: f64 },
    /// Stage 0 = bid, 1..=ESCALATIONS = insist.
    Bidding { stage: u8, since: f64, next_emote: f64, emote: usize },
    /// Arrived for a summon: the session's mood carries it.
    Summoned { kind: SummonKind, until: f64 },
    /// Linger; `leave_at`: move off his window first.
    Satisfied { until: f64, leave_at: Option<f64> },
    /// `far`: still on his window (it sulked in place), so the far move ends the sulk.
    /// Otherwise she stays down where she sulked and her next roam goes far.
    Sulking { until: f64, far: bool },
    /// He is away: pose overlay on the ground.
    Napping,
}

impl Mode {
    fn key(&self) -> &'static str {
        match self {
            Mode::Passive => "passive",
            Mode::Seeking { .. } => "seeking",
            Mode::Bidding { .. } => "bidding",
            Mode::Summoned { .. } => "summoned",
            Mode::Satisfied { .. } => "satisfied",
            Mode::Sulking { .. } => "sulking",
            Mode::Napping => "napping",
        }
    }
    fn active(&self) -> bool {
        matches!(self, Mode::Seeking { .. } | Mode::Bidding { .. } | Mode::Summoned { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    kind: SummonKind,
    since: f64,
    /// `brain force summon`: ignores "he is near" and the gap.
    forced: bool,
}

/// Persisted drives (state.json "brain"), unix seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub saved_unix: f64,
    pub attention: f64,
    pub theta: f64,
    pub roam_frac: f64,
    pub last_bid_unix: Option<f64>,
    pub dnd_until_unix: Option<f64>,
    pub enabled: bool,
    pub moves_unix: Vec<f64>,
}

impl Saved {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "v": 1, "savedUnix": self.saved_unix, "attention": self.attention, "theta": self.theta,
            "roamFrac": self.roam_frac, "lastBidUnix": self.last_bid_unix, "dndUntilUnix": self.dnd_until_unix,
            "enabled": self.enabled, "movesUnix": self.moves_unix })
    }
    pub fn from_json(v: &serde_json::Value) -> Option<Saved> {
        if v.get("v")?.as_i64()? != 1 {
            return None;
        }
        let f = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
        Some(Saved {
            saved_unix: f("savedUnix")?,
            attention: f("attention")?.clamp(-0.8, 1.2),
            theta: f("theta")?.clamp(0.1, 3.0),
            roam_frac: f("roamFrac")?.clamp(0.0, 1.0),
            last_bid_unix: f("lastBidUnix"),
            dnd_until_unix: f("dndUntilUnix"),
            enabled: v.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true),
            moves_unix: v.get("movesUnix").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub t: f64,
    pub what: String,
}

const LOG_MAX: usize = 6000;
const EPS: f64 = 1e-9;

pub struct Brain {
    pub tuning: Tuning,
    pub settings: BrainSettings,
    rng: Rng,
    start: f64,
    /// Drives are integrated up to here.
    t: f64,
    mode: Mode,
    attn: f64,
    theta: f64,
    roam: f64,
    t_roam: f64,
    present: bool,
    away_since: Option<f64>,
    sensed: Senses,
    next_poll: f64,
    enabled: bool,
    dnd_until: Option<f64>,
    last_bid: Option<f64>,
    last_active_arrival: Option<f64>,
    last_interaction: Option<f64>,
    moves: VecDeque<f64>,
    pending: Option<Pending>,
    /// The unanswered prompt, and per prompt id: (summons so far, last summon).
    prompt_open: Option<u64>,
    prompt_summons: HashMap<u64, (u8, f64)>,
    last_summon: Option<f64>,
    covering: bool,
    /// The last moves asked for and why (`brain state`).
    recent_moves: std::collections::VecDeque<(f64, &'static str)>,
    far_next: bool,
    no_bid_before: f64,
    /// The bid before the one out now (a failed bid didn't happen).
    prev_bid: Option<f64>,
    force_seek: bool,
    hidden: bool,
    napping_pose: bool,
    pub approaches: Bag<Approach>,
    pub arrivals: Bag<Arrival>,
    pub emotes: Bag<usize>,
    /// An ignored bid that covered his window leaves it before the sulk (false: sulks in place,
    /// then the far move; the gauntlet's B variant).
    pub sulk_leaves: bool,
    log: VecDeque<LogLine>,
    logged: u64,
    out: Vec<(f64, Intent)>,
}

impl Brain {
    /// `saved` comes with how many brain seconds ago it was written (the view converts).
    pub fn new(seed: u64, settings: BrainSettings, tuning: Tuning, saved: Option<(&Saved, f64)>, now: f64) -> Brain {
        let mut rng = Rng::new(seed);
        let theta = rng.range(tuning.theta.0 as f32, tuning.theta.1 as f32) as f64;
        let t_roam = rng.range(tuning.roam_s.0 as f32, tuning.roam_s.1 as f32) as f64;
        // A fresh start roams first after FIRST_ROAM_MIN..ROAM_MAX.
        let first = rng.range(tuning.first_roam_min_s as f32, tuning.roam_s.1.max(tuning.first_roam_min_s) as f32) as f64;
        let mut b = Brain {
            rng,
            start: now,
            t: now,
            mode: Mode::Passive,
            attn: 0.0,
            theta,
            roam: (1.0 - first / t_roam).max(0.0),
            t_roam,
            present: true,
            away_since: None,
            sensed: Senses { now, ..Senses::default() },
            next_poll: now,
            enabled: settings.enabled,
            dnd_until: None,
            last_bid: None,
            last_active_arrival: None,
            last_interaction: None,
            moves: VecDeque::new(),
            pending: None,
            prompt_open: None,
            prompt_summons: HashMap::new(),
            last_summon: None,
            covering: false,
            recent_moves: std::collections::VecDeque::new(),
            far_next: false,
            no_bid_before: now + tuning.launch_grace_s,
            prev_bid: None,
            force_seek: false,
            hidden: false,
            napping_pose: false,
            approaches: Bag::default(),
            arrivals: Bag::default(),
            emotes: Bag::default(),
            sulk_leaves: true,
            log: VecDeque::new(),
            logged: 0,
            out: Vec::new(),
            tuning,
            settings,
        };
        if let Some((s, ago)) = saved {
            // Attention never grows while she was off, and stays well under θ at launch.
            b.attn = s.attention.min(0.6);
            b.theta = s.theta;
            // Keep the roam fraction, but the first move no sooner than FIRST_ROAM_MIN.
            b.roam = s.roam_frac.min(1.0 - b.tuning.first_roam_min_s / b.t_roam).max(0.0);
            b.enabled = s.enabled;
            b.last_bid = s.last_bid_unix.map(|u| now - ago - (s.saved_unix - u));
            b.dnd_until = s.dnd_until_unix.map(|u| now - ago + (u - s.saved_unix)).filter(|&d| d > now);
            b.moves = s.moves_unix.iter().map(|&u| now - ago - (s.saved_unix - u)).filter(|&m| m > now - 3600.0).collect();
        }
        b.note(now, format!("start attention={:.2} theta={:.2} roam={:.2} t_roam={:.0}", b.attn, b.theta, b.roam, b.t_roam));
        b
    }

    pub fn saved(&self, now: f64, now_unix: f64) -> Saved {
        let unix = |t: f64| now_unix - (now - t);
        let attn = self.attn_at(now);
        Saved {
            saved_unix: now_unix,
            attention: attn,
            theta: self.theta,
            roam_frac: self.roam_at(now),
            last_bid_unix: self.last_bid.map(unix),
            dnd_until_unix: self.dnd_until.filter(|&d| d > now).map(unix),
            enabled: self.enabled,
            moves_unix: self.moves.iter().map(|&m| unix(m)).collect(),
        }
    }

    // ---- drives ----------------------------------------------------------------------

    fn attn_rate(&self) -> f64 {
        let grows = self.present && !self.mode.active() && self.mode != Mode::Napping;
        if grows { 1.0 / self.tuning.attn_fill_s } else { 0.0 }
    }

    fn roam_rate(&self) -> f64 {
        if self.present && self.mode == Mode::Passive { 1.0 / self.t_roam } else { 0.0 }
    }

    fn attn_at(&self, t: f64) -> f64 {
        (self.attn + self.attn_rate() * (t - self.t).max(0.0)).min(1.2)
    }

    fn roam_at(&self, t: f64) -> f64 {
        (self.roam + self.roam_rate() * (t - self.t).max(0.0)).min(1.0)
    }

    /// Integrate the drives to `t` (rates are constant between steps).
    fn advance(&mut self, t: f64) {
        if t > self.t {
            self.attn = self.attn_at(t);
            self.roam = self.roam_at(t);
            self.t = t;
        }
    }

    fn redraw_theta(&mut self, raise: f64) {
        let (lo, hi) = self.tuning.theta;
        self.theta = self.rng.range(lo as f32, hi as f32) as f64 + raise;
    }

    fn redraw_roam(&mut self) {
        let (lo, hi) = self.tuning.roam_s;
        self.t_roam = self.rng.range(lo as f32, hi as f32) as f64;
        self.roam = 0.0;
    }

    // ---- gates -------------------------------------------------------------------------

    fn dnd(&self, t: f64) -> bool {
        self.dnd_until.is_some_and(|d| d > t)
    }

    /// Moves may happen (rules 1, 6, 7 and on/off; Busy separately).
    fn gate_open(&self, t: f64) -> bool {
        self.enabled && !self.dnd(t) && !self.sensed.fullscreen && !self.hidden && self.present
    }

    fn moves_in_hour(&self, t: f64) -> usize {
        self.moves.iter().filter(|&&m| m > t - 3600.0).count()
    }

    fn cap_ok(&self, t: f64) -> bool {
        self.moves_in_hour(t) < self.tuning.move_cap_per_hour as usize
    }

    /// When the rolling hour frees a slot again.
    fn cap_free_at(&self, t: f64) -> f64 {
        let recent: Vec<f64> = self.moves.iter().copied().filter(|&m| m > t - 3600.0).collect();
        let cap = self.tuning.move_cap_per_hour as usize;
        if recent.len() < cap { t } else { recent[recent.len() - cap] + 3600.0 }
    }

    fn bid_allowed_at(&self) -> f64 {
        let gap = self.last_bid.map_or(f64::MIN, |b| b + self.tuning.active_gap_min_s);
        self.no_bid_before.max(gap)
    }

    // ---- the clock ------------------------------------------------------------------------

    /// The next time anything can happen (strictly after the last step).
    pub fn due(&self) -> f64 {
        self.due_from(Some(self.next_poll))
    }

    fn due_from(&self, poll: Option<f64>) -> f64 {
        let t = self.t;
        let mut d = poll.unwrap_or(f64::INFINITY);
        let mut take = |x: f64| {
            if x > t + EPS && x < d {
                d = x;
            }
        };
        if let Some(u) = self.dnd_until {
            take(u);
        }
        if let (false, Some(a)) = (self.present, self.away_since)
            && self.mode != Mode::Napping
        {
            take(a + self.tuning.nap_after_s);
        }
        match &self.mode {
            Mode::Seeking { since, .. } => take(since + self.tuning.seek_timeout_s),
            Mode::Bidding { stage, since, next_emote, .. } => {
                take(*next_emote);
                take(since + self.tuning.bid_stage_s * (*stage as f64 + 1.0));
            }
            Mode::Summoned { until, .. } => take(*until),
            Mode::Satisfied { until, leave_at } => {
                take(*until);
                if let Some(l) = leave_at {
                    take(*l);
                }
            }
            Mode::Sulking { until, .. } => take(*until),
            Mode::Passive => {
                let r = self.attn_rate();
                if self.settings.attention != Attention::Off && r > 0.0 && self.attn < self.theta {
                    take(t + (self.theta - self.attn) / r);
                }
                if self.attn >= self.theta - EPS || self.force_seek {
                    take(self.bid_allowed_at());
                    take(self.cap_free_at(t));
                }
                let rr = self.roam_rate();
                if rr > 0.0 && self.roam < 1.0 {
                    take(t + (1.0 - self.roam) / rr);
                }
                if self.roam >= 1.0 - EPS {
                    take(self.cap_free_at(t));
                }
            }
            Mode::Napping => {}
        }
        d
    }

    fn poll_period(&self) -> f64 {
        let waiting = self.pending.is_some() && (!self.present || !self.gate_open(self.t));
        if waiting { self.tuning.poll_s.1 } else { self.tuning.poll_s.0 }
    }

    /// Run every step due by `s.now`, reading `s` at the polls; the intents since last time.
    pub fn tick(&mut self, s: &Senses) -> Decision {
        for _ in 0..10_000 {
            let d = self.due();
            if d > s.now + EPS {
                break;
            }
            self.step(d.max(self.t), Some(s));
        }
        Decision { intents: std::mem::take(&mut self.out), wake_at: self.due() }
    }

    /// Steps due before `now`, without new senses (an event arrives between wake-ups): a
    /// poll waits for the tick that comes with the senses.
    fn catch_up(&mut self, now: f64) {
        for _ in 0..10_000 {
            let d = self.due_from(None);
            if d > now + EPS {
                break;
            }
            self.step(d.max(self.t), None);
        }
        self.advance(now);
    }

    /// Read the senses at the next tick (a test changed one: `brain sense …`).
    pub fn poll_now(&mut self) {
        self.next_poll = self.next_poll.min(self.t);
    }

    fn emit(&mut self, t: f64, i: Intent) {
        let what = match &i {
            Intent::Move(p) => {
                self.recent_moves.push_back((t, p.why));
                while self.recent_moves.len() > 12 {
                    self.recent_moves.pop_front();
                }
                format!("move {} (why {})", plan_text(p), p.why)
            }
            Intent::Emote(e) => format!("emote {e:?}"),
            Intent::Pose(p) => format!("pose {p:?}"),
            Intent::Hide(h) => format!("hide {h}"),
            Intent::Unfold => "unfold".into(),
            Intent::Save => return self.out.push((t, i)),
        };
        self.note(t, what);
        self.out.push((t, i));
    }

    /// A line in the decision log (the view adds spots and blocks).
    pub fn note(&mut self, t: f64, what: String) {
        self.log.push_back(LogLine { t, what });
        self.logged += 1;
        while self.log.len() > LOG_MAX {
            self.log.pop_front();
        }
    }

    fn step(&mut self, t: f64, senses: Option<&Senses>) {
        self.advance(t);
        // The fullscreen gate is read at every tick that brings senses (a timer between two
        // polls must not move her onto a fullscreen app that came up since the last poll).
        if let Some(s) = senses {
            self.sensed.fullscreen = s.fullscreen;
            self.sensed.fullscreen_here = s.fullscreen_here;
        }
        // Poll: the only place the other senses are read.
        if self.next_poll <= t + EPS
            && let Some(s) = senses
        {
            self.sensed = Senses { now: t, ..s.clone() };
            self.poll(t);
            self.next_poll = t + self.poll_period();
        }
        if self.dnd_until.is_some_and(|d| d <= t + EPS) {
            self.dnd_until = None;
            self.note(t, "dnd ends".into());
            self.emit(t, Intent::Save);
        }
        if let (false, Some(a)) = (self.present, self.away_since)
            && self.mode != Mode::Napping
            && t + EPS >= a + self.tuning.nap_after_s
        {
            self.end_episode(t, false);
            self.mode = Mode::Napping;
            if self.sensed.placement == PlaceKind::Ground {
                self.napping_pose = true;
                self.emit(t, Intent::Pose(Some(RestPose::Nap)));
            } else {
                self.note(t, "nap (in place)".into());
            }
        }
        self.timers(t);
        self.triggers(t);
    }

    fn poll(&mut self, t: f64) {
        let idle = self.sensed.idle_s;
        if self.present && idle >= self.tuning.away_after_s {
            self.advance(t);
            self.present = false;
            self.away_since = Some(t);
            self.note(t, format!("away (idle {idle:.0} s)"));
            if self.mode.active() {
                self.end_episode(t, true);
            }
        } else if !self.present && idle < self.tuning.back_idle_s {
            self.advance(t);
            self.present = true;
            self.away_since = None;
            self.no_bid_before = self.no_bid_before.max(t + self.tuning.back_grace_s);
            self.note(t, "back".into());
            if self.mode == Mode::Napping {
                self.mode = Mode::Passive;
                if std::mem::take(&mut self.napping_pose) {
                    self.emit(t, Intent::Pose(None));
                }
            }
        }
        // Fullscreen on her monitor, hide mode.
        let want = self.settings.fullscreen == Fullscreen::Hide && self.sensed.fullscreen_here;
        if want != self.hidden {
            self.hidden = want;
            if want && self.mode.active() {
                self.end_episode(t, false);
            }
            self.emit(t, Intent::Hide(want));
        }
        // An unanswered prompt, him elsewhere again: one more summon (at most 3 per prompt).
        if let Some(id) = self.prompt_open
            && self.pending.is_none()
            && !self.mode.active()
            && !self.sensed.near
            && let Some(&(n, last)) = self.prompt_summons.get(&id)
            && n < self.tuning.summons_per_prompt
            && t - last >= self.tuning.resummon_s
        {
            self.pending = Some(Pending { kind: SummonKind::Prompt(id), since: t, forced: false });
            self.note(t, format!("resummon prompt {id}"));
        }
    }

    fn timers(&mut self, t: f64) {
        match self.mode.clone() {
            Mode::Seeking { since, .. } if t + EPS >= since + self.tuning.seek_timeout_s => {
                self.note(t, "seek timed out".into());
                self.move_failed(t, false);
            }
            Mode::Bidding { stage, since, next_emote, emote } => {
                if t + EPS >= since + self.tuning.bid_stage_s * (stage as f64 + 1.0) {
                    let stage = stage + 1;
                    if stage > self.tuning.escalations {
                        self.sulk(t, true);
                    } else {
                        self.emit(t, Intent::Emote(EmoteKey::Insist(stage)));
                        let next = t + self.emote_gap();
                        self.mode = Mode::Bidding { stage, since, next_emote: next, emote };
                    }
                } else if t + EPS >= next_emote {
                    // Later emotes of the bid: the next in the pool (never the one just shown).
                    let k = self.tuning.bid_pool.max(1);
                    let e = if stage == 0 { (emote + 1 + self.rng.pick(k.saturating_sub(1).max(1))) % k } else { emote };
                    let key = if stage == 0 { EmoteKey::Bid(e) } else { EmoteKey::Insist(stage) };
                    self.emit(t, Intent::Emote(key));
                    let next = t + self.emote_gap();
                    self.mode = Mode::Bidding { stage, since, next_emote: next, emote: e };
                }
            }
            Mode::Summoned { until, .. } if t + EPS >= until => {
                self.note(t, "summon over (no answer here)".into());
                self.end_episode(t, true);
            }
            Mode::Satisfied { until, leave_at } => {
                if leave_at.is_some_and(|l| t + EPS >= l) {
                    if self.leave(t) {
                        self.mode = Mode::Satisfied { until, leave_at: None };
                    } else {
                        self.mode = Mode::Satisfied { until, leave_at: Some(self.next_poll.max(t + 1.0)) };
                    }
                } else if t + EPS >= until {
                    self.mode = Mode::Passive;
                    self.roam = 0.0;
                    self.note(t, "linger over".into());
                }
            }
            Mode::Sulking { until, far } if t + EPS >= until => {
                self.mode = Mode::Passive;
                self.redraw_roam();
                self.covering = false;
                if !far {
                    // She left his window to sulk and sits it out: no move now.
                    self.far_next = true;
                    self.note(t, "sulk over: stays low, the next roam goes far".into());
                } else if self.gate_open(t) {
                    self.record_move(t);
                    let arrivals = self.passive_arrivals();
                    self.emit(t, Intent::Move(MovePlan { goal: Goal::Passive { far: true }, why: "sulk_far", arrivals }));
                } else {
                    self.far_next = true;
                }
                self.emit(t, Intent::Save);
            }
            _ => {}
        }
    }

    fn triggers(&mut self, t: f64) {
        // Summons first: the real job.
        if let Some(p) = self.pending
            && !matches!(self.mode, Mode::Seeking { why: Why::Summon(_), .. } | Mode::Summoned { .. })
        {
            let gap_ok = p.forced || self.last_summon.is_none_or(|l| t - l >= self.tuning.summon_gap_s);
            if self.enabled && self.gate_open(t) && gap_ok {
                if self.sensed.near && !p.forced {
                    // He is right there: the mood shows it in place.
                    self.pending = None;
                    self.note(t, format!("summon {:?} not needed (he is near)", p.kind));
                } else {
                    self.pending = None;
                    self.last_summon = Some(t);
                    if let SummonKind::Prompt(id) = p.kind {
                        let e = self.prompt_summons.entry(id).or_insert((0, t));
                        *e = (e.0 + 1, t);
                    }
                    // A summon preempts whatever she was doing (a bid included).
                    self.mode = Mode::Passive;
                    self.seek(t, Why::Summon(p.kind));
                    return;
                }
            }
        }
        if self.mode != Mode::Passive || !self.gate_open(t) {
            return;
        }
        let busy = !self.sensed.busy.is_empty();
        // Attention bid.
        // A bid keeps one slot of the hour free for leaving his window after it.
        let room = self.moves_in_hour(t) + 1 < self.tuning.move_cap_per_hour as usize;
        let rules = self.force_seek || (t + EPS >= self.bid_allowed_at() && room);
        if self.settings.attention != Attention::Off && self.attn >= self.theta - EPS && rules && !busy {
            self.force_seek = false;
            self.prev_bid = self.last_bid;
            self.last_bid = Some(t);
            self.record_move(t);
            self.seek(t, Why::Attention);
            return;
        }
        // Roam.
        if self.roam >= 1.0 - EPS && self.cap_ok(t) && !busy {
            let far = std::mem::take(&mut self.far_next);
            self.redraw_roam();
            self.record_move(t);
            let arrivals = self.passive_arrivals();
            self.emit(t, Intent::Move(MovePlan { goal: Goal::Passive { far }, why: "roam", arrivals }));
            self.emit(t, Intent::Save);
        }
    }

    fn record_move(&mut self, t: f64) {
        self.moves.push_back(t);
        while self.moves.front().is_some_and(|&m| m <= t - 3600.0) {
            self.moves.pop_front();
        }
    }

    fn emote_gap(&mut self) -> f64 {
        let (lo, hi) = self.tuning.bid_emote_s;
        self.rng.range(lo as f32, hi as f32) as f64
    }

    fn passive_arrivals(&mut self) -> Vec<Arrival> {
        let w: Vec<(Arrival, f64)> = self.settings.arrivals.iter().copied().filter(|a| matches!(a.0, Arrival::Teleport | Arrival::Glitch)).collect();
        let mut v = Bag::<Arrival>::default().order(&w, 0, &mut self.rng);
        if !v.contains(&Arrival::Teleport) {
            v.push(Arrival::Teleport);
        }
        v
    }

    fn approach_weights(&self) -> Vec<(Approach, f64)> {
        match self.settings.attention {
            Attention::HangOnly => vec![(Approach::Hang, 1.0)],
            _ => self.settings.approaches.clone(),
        }
    }

    fn seek(&mut self, t: f64, why: Why) {
        let n = self.settings.no_repeat;
        let mut approaches = self.approaches.order(&self.approach_weights(), n, &mut self.rng);
        approaches.retain(|a| *a != Approach::Beside);
        approaches.push(Approach::Beside);
        let mut arrivals = self.arrivals.order(&self.settings.arrivals, n, &mut self.rng);
        if !arrivals.contains(&Arrival::Teleport) {
            arrivals.push(Arrival::Teleport);
        }
        self.mode = Mode::Seeking { why, since: t };
        let w = if matches!(why, Why::Summon(_)) { "summon" } else { "bid" };
        self.emit(t, Intent::Move(MovePlan { goal: Goal::Active { why, approaches }, why: w, arrivals }));
    }

    /// The bid or summon is over (no response needed): back to passive, off his window.
    fn end_episode(&mut self, t: f64, may_leave: bool) {
        if self.mode.active() || matches!(self.mode, Mode::Satisfied { .. } | Mode::Sulking { .. }) {
            self.mode = Mode::Passive;
            self.roam = 0.0;
        }
        if may_leave {
            self.leave(t);
        }
        self.emit(t, Intent::Save);
    }

    /// Move off his window if she covers it. False: couldn't now (busy or fullscreen).
    fn leave(&mut self, t: f64) -> bool {
        if !self.covering {
            return true;
        }
        // Busy (a reaction, a bubble) doesn't hold it: the view runs it right after.
        if self.sensed.fullscreen || self.hidden {
            return false;
        }
        self.covering = false;
        self.record_move(t);
        let arrivals = self.passive_arrivals();
        self.emit(t, Intent::Move(MovePlan { goal: Goal::Leave, why: "covering", arrivals }));
        true
    }

    fn satisfied(&mut self, t: f64, pat: bool) {
        self.attn = 0.0;
        self.redraw_theta(0.0);
        let (lo, hi) = self.tuning.linger_s;
        let until = t + self.rng.range(lo as f32, hi as f32) as f64;
        let leave_at = self.covering.then_some(t + self.tuning.leave_after_s);
        self.mode = Mode::Satisfied { until, leave_at };
        self.emit(t, Intent::Emote(EmoteKey::Satisfied { pat }));
        self.emit(t, Intent::Save);
    }

    /// `ignored`: the bid ran out (θ raised, then a far move); else offended (fling).
    fn sulk(&mut self, t: f64, ignored: bool) {
        self.attn = if ignored { 0.0 } else { -0.8 };
        self.redraw_theta(if ignored { self.tuning.theta_raise } else { 0.0 });
        // Offended (a fling) or sulking on his window: the far move at the end.
        let far = !ignored || (self.covering && !self.sulk_leaves);
        self.mode = Mode::Sulking { until: t + self.tuning.sulk_s, far };
        if ignored {
            // Off his window first (a hang can't sulk with its arms crossed): the sulk plays
            // where she lands, and she stays down there.
            if self.covering && self.sulk_leaves {
                self.leave(t);
            }
            self.emit(t, Intent::Emote(EmoteKey::Sulk));
        } else {
            self.note(t, "offended (the ragdoll's hard landing plays the sulk)".into());
        }
        // A covered window is left with the far move at the end of the sulk.
        self.covering = false;
    }

    fn move_failed(&mut self, t: f64, busy: bool) {
        match self.mode.clone() {
            Mode::Seeking { why: Why::Attention, .. } => {
                self.mode = Mode::Passive;
                if !busy {
                    // Nowhere to go and not on his monitor: A stays, try again later.
                    self.no_bid_before = self.no_bid_before.max(t + self.tuning.retry_s);
                }
                self.last_bid = self.prev_bid;
                self.moves.pop_back();
            }
            Mode::Seeking { why: Why::Summon(kind), .. } => {
                self.mode = Mode::Passive;
                self.pending = Some(Pending { kind, since: t, forced: false });
                self.last_summon = Some(t - self.tuning.summon_gap_s + if busy { 3.0 } else { 60.0 });
            }
            _ => {
                // A passive move found no spot: try a while later.
                self.roam = self.roam.min(0.5);
            }
        }
    }

    // ---- events --------------------------------------------------------------------------

    pub fn on_event(&mut self, ev: BrainEvent, now: f64) {
        self.catch_up(now);
        let t = now;
        match ev {
            BrainEvent::Interaction(touch) => {
                self.note(t, format!("touch {touch:?}"));
                self.last_interaction = Some(t);
                if !self.present || self.mode == Mode::Napping {
                    // He is obviously here.
                    self.present = true;
                    self.away_since = None;
                    if self.mode == Mode::Napping {
                        self.mode = Mode::Passive;
                        if std::mem::take(&mut self.napping_pose) {
                            self.emit(t, Intent::Pose(None));
                        }
                    }
                }
                let answered = matches!(touch, Touch::Typing | Touch::Submit | Touch::PromptAnswer);
                if self.mode.active() || matches!(self.mode, Mode::Sulking { .. }) {
                    if answered {
                        self.prompt_open = None;
                        self.pending = None;
                    }
                    self.satisfied(t, touch == Touch::Headpat);
                    return;
                }
                let drain = match touch {
                    Touch::Headpat => 1.0,
                    Touch::Click => 0.5,
                    Touch::Chip => 0.3,
                    Touch::DragStart => 0.6,
                    Touch::Typing | Touch::Submit | Touch::PromptAnswer => f64::INFINITY,
                    Touch::Menu => 0.0,
                };
                if drain > 0.0 {
                    self.attn = (self.attn - drain).max(0.0).min(self.attn.max(0.0));
                }
                if answered {
                    self.prompt_open = None;
                }
                self.roam *= 0.5;
            }
            BrainEvent::Flung { speed } => {
                let offended = self.last_active_arrival.is_some_and(|a| t - a <= self.tuning.offense_window_s);
                self.note(t, format!("flung {speed:.0} px/s{}", if offended { " (offended)" } else { "" }));
                self.far_next = true;
                if offended {
                    self.last_active_arrival = None;
                    self.sulk(t, false);
                } else {
                    self.attn = (self.attn - 0.3).max(-0.8);
                }
                self.emit(t, Intent::Save);
            }
            BrainEvent::Landed { hard } => self.note(t, format!("landed{}", if hard { " hard" } else { "" })),
            BrainEvent::Arrived(a) => {
                self.note(
                    t,
                    format!("arrived {} by {}{}", a.approach.map_or("passive", Approach::key), a.arrival.key(), if a.covering { " (covering)" } else { "" }),
                );
                self.covering = a.covering;
                if let Mode::Seeking { why, .. } = self.mode {
                    self.last_active_arrival = Some(t);
                    if let Some(ap) = a.approach {
                        self.approaches.used(ap);
                    }
                    self.arrivals.used(a.arrival);
                    match why {
                        Why::Attention => {
                            let pool: Vec<(usize, f64)> = (0..self.tuning.bid_pool.max(1)).map(|i| (i, 1.0)).collect();
                            let e = self.emotes.order(&pool, self.settings.no_repeat, &mut self.rng).first().copied().unwrap_or(0);
                            self.emotes.used(e);
                            self.emit(t, Intent::Emote(EmoteKey::Bid(e)));
                            let next = t + self.emote_gap();
                            self.mode = Mode::Bidding { stage: 0, since: t, next_emote: next, emote: e };
                        }
                        Why::Summon(kind) => {
                            let hold = if matches!(kind, SummonKind::Prompt(_)) { self.tuning.summon_hold_s } else { self.tuning.summon_note_s };
                            if !matches!(kind, SummonKind::Prompt(_)) {
                                self.emit(t, Intent::Emote(EmoteKey::Wave));
                                self.emit(t, Intent::Unfold);
                            }
                            self.mode = Mode::Summoned { kind, until: t + hold };
                        }
                    }
                } else if a.approach.is_none() && self.mode == Mode::Passive && self.moves.back().is_some_and(|&m| t - m > 5.0) {
                    // The view held the move back (behind a clip, a bubble): the next roam
                    // counts from when she got there.
                    self.roam = 0.0;
                }
                self.emit(t, Intent::Save);
            }
            BrainEvent::MoveFailed { busy } => {
                self.note(t, format!("move failed{}", if busy { " (busy)" } else { "" }));
                self.move_failed(t, busy);
            }
            BrainEvent::Session(sig) => {
                self.note(t, format!("session {sig:?}"));
                match sig {
                    SessionSignal::Prompt(id) => {
                        self.prompt_open = Some(id);
                        // Once per prompt id; RESUMMON_S repeats come from the poll.
                        if !self.prompt_summons.contains_key(&id) && self.pending.is_none_or(|p| p.kind != SummonKind::Prompt(id)) {
                            self.pending = Some(Pending { kind: SummonKind::Prompt(id), since: t, forced: false });
                        }
                    }
                    SessionSignal::Done => self.pending = Some(Pending { kind: SummonKind::Done, since: t, forced: false }),
                    SessionSignal::Error { own_abort: true } => {
                        // His own abort: that's him interacting, not news.
                        self.last_interaction = Some(t);
                        self.roam *= 0.5;
                    }
                    SessionSignal::Error { own_abort: false } => {
                        self.pending = Some(Pending { kind: SummonKind::Error, since: t, forced: false })
                    }
                    SessionSignal::Cleared => {
                        self.prompt_open = None;
                        self.pending = None;
                        if matches!(self.mode, Mode::Summoned { .. } | Mode::Seeking { why: Why::Summon(_), .. }) {
                            self.satisfied(t, false);
                        }
                    }
                }
            }
            BrainEvent::Control(c) => self.control(c, t),
        }
        // React now (a forced roam, a summon), not at the next wake-up.
        self.timers(t);
        self.triggers(t);
    }

    fn control(&mut self, c: Ctl, t: f64) {
        self.note(t, format!("control {c:?}"));
        match c {
            Ctl::On => self.enabled = true,
            Ctl::Off => {
                self.enabled = false;
                if self.mode.active() {
                    self.end_episode(t, true);
                }
            }
            Ctl::Dnd(secs) => {
                self.dnd_until = Some(t + secs.max(1.0));
                if self.mode.active() {
                    self.end_episode(t, true);
                }
            }
            Ctl::Undnd => self.dnd_until = None,
            Ctl::Force(f) => match f {
                Force::Roam => self.roam = 1.0,
                Force::Seek => {
                    self.attn = self.attn.max(self.theta);
                    self.force_seek = true;
                    // A test hook: a linger or a sulk doesn't hold it back.
                    if matches!(self.mode, Mode::Satisfied { .. } | Mode::Sulking { .. }) {
                        self.mode = Mode::Passive;
                    }
                }
                Force::Summon => self.pending = Some(Pending { kind: SummonKind::Done, since: t, forced: true }),
                Force::Sulk => self.sulk(t, true),
                Force::Nap => {
                    self.present = false;
                    self.away_since = Some(t - self.tuning.nap_after_s);
                }
            },
            Ctl::Seed(s) => self.rng = Rng::new(s),
        }
        self.emit(t, Intent::Save);
    }

    // ---- read-outs ----------------------------------------------------------------------

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mode_key(&self) -> &'static str {
        self.mode.key()
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn dnd_left(&self, now: f64) -> Option<f64> {
        self.dnd_until.filter(|&d| d > now).map(|d| d - now)
    }
    /// A move the view made on its own (her perch went away): logged with its rule.
    pub fn note_external(&mut self, t: f64, why: &'static str) {
        self.recent_moves.push_back((t, why));
        while self.recent_moves.len() > 12 {
            self.recent_moves.pop_front();
        }
        self.note(t, format!("view move (why {why})"));
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn covering(&self) -> bool {
        self.covering
    }
    /// Lines logged so far (ever), and the kept tail.
    pub fn log(&self) -> (u64, &VecDeque<LogLine>) {
        (self.logged, &self.log)
    }

    pub fn snapshot(&self, now: f64) -> serde_json::Value {
        let ago = |t: Option<f64>| t.map(|t| ((now - t) * 10.0).round() / 10.0);
        let r = |v: f64| (v * 1000.0).round() / 1000.0;
        let mut suppressed = Vec::new();
        if self.sensed.fullscreen {
            suppressed.push("fullscreen");
        }
        if self.dnd(now) {
            suppressed.push("dnd");
        }
        if !self.enabled {
            suppressed.push("off");
        }
        if !self.present {
            suppressed.push("away");
        }
        let s = &self.settings;
        serde_json::json!({
            "mode": self.mode.key(),
            "drives": { "attention": r(self.attn_at(now)), "theta": r(self.theta), "roam": r(self.roam_at(now)),
                        "urgency": self.pending.map(|p| format!("{:?}", p.kind)) },
            "gate": { "enabled": self.enabled, "dnd_left_s": self.dnd_left(now).map(|d| d.round()), "fullscreen": self.sensed.fullscreen,
                      "hidden": self.hidden },
            "suppressed_by": suppressed,
            "busy": self.sensed.busy,
            "present": self.present,
            "idle_s": r(self.sensed.idle_s),
            "near": self.sensed.near,
            "covering": self.covering,
            "last_interaction_s": ago(self.last_interaction),
            "next_wake_in": r(self.due() - now),
            "moves_last_hour": self.moves_in_hour(now),
            "recent_moves": self.recent_moves.iter().map(|(t, w)| serde_json::json!({ "ago_s": r(now - t), "why": w })).collect::<Vec<_>>(),
            "last_bid_ago": ago(self.last_bid),
            "pending_summon": self.pending.map(|p| format!("{:?}", p.kind)),
            "prompt_open": self.prompt_open,
            "settings": { "enabled": s.enabled, "attention": s.attention.key(), "no_repeat": s.no_repeat,
                          "cover_max_pct": s.cover_max_pct, "passive_every_min": [s.passive_every_min.0, s.passive_every_min.1],
                          "bid_every_min": [s.bid_every_min.0, s.bid_every_min.1], "max_moves_per_hour": s.max_moves_per_hour,
                          "fullscreen": format!("{:?}", s.fullscreen).to_lowercase(), "monitor": format!("{:?}", s.monitor).to_lowercase(),
                          "approaches": s.approaches.iter().map(|(a, w)| (a.key().to_string(), serde_json::json!(w))).collect::<serde_json::Map<_, _>>(),
                          "arrivals": s.arrivals.iter().map(|(a, w)| (a.key().to_string(), serde_json::json!(w))).collect::<serde_json::Map<_, _>>() },
            "bags": { "approach": self.approaches.history.iter().map(|a| a.key()).collect::<Vec<_>>(),
                      "arrival": self.arrivals.history.iter().map(|a| a.key()).collect::<Vec<_>>(),
                      "emote": self.emotes.history },
            "decisions": self.log.iter().rev().take(10).rev().map(|l| serde_json::json!([r(l.t - self.start), l.what])).collect::<Vec<_>>(),
        })
    }

    pub fn start(&self) -> f64 {
        self.start
    }
}

fn plan_text(p: &MovePlan) -> String {
    let arr: Vec<_> = p.arrivals.iter().map(|a| a.key()).collect();
    match &p.goal {
        Goal::Passive { far } => format!("passive{} [{}]", if *far { " far" } else { "" }, arr.join(",")),
        Goal::Leave => format!("leave [{}]", arr.join(",")),
        Goal::Active { why, approaches } => {
            let ap: Vec<_> = approaches.iter().map(|a| a.key()).collect();
            format!("{} [{}] [{}]", match why {
                Why::Attention => "bid".to_string(),
                Why::Summon(k) => format!("summon {k:?}"),
            }, ap.join(","), arr.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brain(seed: u64) -> Brain {
        let s = BrainSettings::default();
        Brain::new(seed, s.clone(), Tuning::new(&s), None, 0.0)
    }

    fn here(now: f64) -> Senses {
        Senses { now, ..Senses::default() }
    }

    /// Drive a brain over [from, to) with a senses script and events; the view's side is
    /// simulated: every Move arrives at once (the first approach and arrival).
    struct Run {
        log: Vec<(f64, Intent)>,
    }

    fn run(
        b: &mut Brain,
        from: f64,
        to: f64,
        every_second: bool,
        senses: &dyn Fn(f64) -> Senses,
        events: &[(f64, BrainEvent)],
    ) -> Run {
        let mut log = Vec::new();
        let mut ev = events.to_vec();
        ev.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut ei = 0;
        let mut t = from;
        let apply = |b: &mut Brain, d: Decision, log: &mut Vec<(f64, Intent)>| {
            let mut pending: VecDeque<(f64, Intent)> = d.intents.into();
            while let Some((ti, i)) = pending.pop_front() {
                if let Intent::Move(p) = &i {
                    let (approach, covering) = match &p.goal {
                        Goal::Active { approaches, .. } => (approaches.first().copied(), approaches.first().is_some_and(|a| a.covers())),
                        _ => (None, false),
                    };
                    let a = Arrived { approach, arrival: p.arrivals[0], covering };
                    log.push((ti, i.clone()));
                    b.on_event(BrainEvent::Arrived(a), ti);
                    pending.extend(std::mem::take(&mut b.out));
                    continue;
                }
                if i != Intent::Save {
                    log.push((ti, i));
                }
            }
        };
        while t < to {
            let next_tick = if every_second { (t + 1.0).floor() } else { b.due() };
            let next_ev = ev.get(ei).map_or(f64::INFINITY, |e| e.0);
            if next_ev <= next_tick && next_ev < to {
                // The view ticks right after an event.
                b.on_event(ev[ei].1, next_ev);
                let d = b.tick(&senses(next_ev).clone_at(next_ev));
                apply(b, d, &mut log);
                ei += 1;
                t = next_ev;
                continue;
            }
            if next_tick >= to {
                break;
            }
            t = next_tick.max(t);
            let d = b.tick(&senses(t).clone_at(t));
            apply(b, d, &mut log);
        }
        Run { log }
    }

    impl Senses {
        fn clone_at(&self, now: f64) -> Senses {
            Senses { now, ..self.clone() }
        }
    }

    fn moves(r: &Run) -> Vec<(f64, MovePlan)> {
        r.log.iter().filter_map(|(t, i)| if let Intent::Move(p) = i { Some((*t, p.clone())) } else { None }).collect()
    }
    fn bids(r: &Run) -> Vec<f64> {
        moves(r).into_iter().filter(|(_, p)| matches!(p.goal, Goal::Active { why: Why::Attention, .. })).map(|m| m.0).collect()
    }

    #[test]
    fn attention_builds_and_seeks() {
        for seed in 1..20 {
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 4000.0, false, &here, &[]);
            let first = bids(&r)[0];
            assert!((1200.0..=1800.0 + 1e-6).contains(&first), "seed {seed}: first bid at {first}");
            assert!(first >= b.tuning.launch_grace_s);
        }
    }

    #[test]
    fn focus_changes_never_evict_her() {
        // Sitting on the floor; his focus flips every 7 s between a window under her and
        // another one (near on/off), as workspace switches do. No move until the roam clock,
        // the same as with no focus change at all, and every move is a roam.
        let s = BrainSettings { attention: Attention::Off, ..Default::default() };
        let mk = || Brain::new(11, s.clone(), Tuning::new(&s), None, 0.0);
        let flicker = |t: f64| Senses { near: (t / 7.0) as i64 % 2 == 0, placement: PlaceKind::Ground, ..here(t) };
        let calm = |t: f64| Senses { placement: PlaceKind::Ground, ..here(t) };
        let mut a = mk();
        let ra = run(&mut a, 0.0, 1500.0, true, &flicker, &[]);
        let mut b = mk();
        let rb = run(&mut b, 0.0, 1500.0, true, &calm, &[]);
        let (ma, mb) = (moves(&ra), moves(&rb));
        assert!(!ma.is_empty(), "a roam happens eventually");
        assert_eq!(ma.iter().map(|m| m.0).collect::<Vec<_>>(), mb.iter().map(|m| m.0).collect::<Vec<_>>(), "focus changes move nothing");
        assert!(ma[0].0 >= s.passive_every_min.0 * 60.0 - 1e-6, "first move {} is on the roam clock", ma[0].0);
        assert!(ma.iter().all(|(_, p)| p.why == "roam" && matches!(p.goal, Goal::Passive { .. })));
        assert!(a.snapshot(1500.0)["recent_moves"].as_array().is_some_and(|v| v.iter().all(|m| m["why"] == "roam")));
    }

    #[test]
    fn away_freezes_and_naps() {
        let mut b = brain(3);
        let script = |t: f64| Senses { idle_s: if t < 100.0 || t >= 2000.0 { 0.0 } else { t - 100.0 }, ..here(t) };
        let r = run(&mut b, 0.0, 2200.0, false, &script, &[]);
        // Away from the first poll with idle >= 300 (t = 405); nap 600 s later.
        let nap = r.log.iter().find(|(_, i)| *i == Intent::Pose(Some(RestPose::Nap))).expect("nap").0;
        assert!((1004.0..=1006.0).contains(&nap), "nap at {nap}");
        assert!(bids(&r).is_empty(), "no bid while away or right after");
        let wake = r.log.iter().find(|(_, i)| *i == Intent::Pose(None)).expect("back").0;
        assert_eq!(wake, 2010.0);
        assert!(moves(&r).iter().all(|(t, _)| *t < 400.0 || *t >= 2010.0), "no moves while away");
    }

    #[test]
    fn response_satisfies() {
        let mut b = brain(4);
        let ev = [(700.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (705.0, BrainEvent::Interaction(Touch::Headpat))];
        let r = run(&mut b, 0.0, 3000.0, false, &here, &ev);
        let bids = bids(&r);
        assert_eq!(bids[0], 700.0);
        assert!(r.log.iter().any(|(t, i)| *t == 705.0 && *i == Intent::Emote(EmoteKey::Satisfied { pat: true })));
        assert!(bids.get(1).is_none_or(|&t| t >= 700.0 + 1200.0), "next bid {bids:?}");
        // If the approach covered his window, she leaves 3 s after the emote; never later.
        let covered = moves(&r).iter().any(|(_, p)| matches!(&p.goal, Goal::Active { approaches, .. } if approaches[0].covers()));
        let left = moves(&r).iter().any(|(t, p)| *t == 708.0 && p.goal == Goal::Leave);
        assert_eq!(covered, left);
        assert!(!b.covering());
    }

    #[test]
    fn ignored_escalates_then_sulks() {
        let mut b = brain(5);
        let theta0 = b.theta;
        let r = run(&mut b, 0.0, 800.0, false, &here, &[(700.0, BrainEvent::Control(Ctl::Force(Force::Seek)))]);
        let insists: Vec<_> = r.log.iter().filter(|(_, i)| matches!(i, Intent::Emote(EmoteKey::Insist(_)))).map(|x| x.0).collect();
        let distinct: Vec<u8> = r.log.iter().filter_map(|(_, i)| if let Intent::Emote(EmoteKey::Insist(s)) = i { Some(*s) } else { None }).collect();
        assert!(insists[0] == 720.0 && distinct.contains(&1) && distinct.contains(&2) && !distinct.contains(&3), "{insists:?} {distinct:?}");
        let sulk = r.log.iter().find(|(_, i)| *i == Intent::Emote(EmoteKey::Sulk)).expect("sulk").0;
        assert_eq!(sulk, 760.0);
        // She stays low after the sulk (r6 critics): no move then; her next roam goes far.
        assert!(moves(&r).into_iter().all(|(t, _)| t <= sulk), "no move right after the sulk");
        assert!(b.theta >= b.tuning.theta.0 + 0.3 - 1e-6 && b.theta != theta0);
        let r = run(&mut b, 800.0, 2000.0, false, &here, &[]);
        let next = moves(&r).into_iter().next().expect("a roam later");
        assert_eq!(next.1.goal, Goal::Passive { far: true });
        assert!(next.0 >= 766.0 + 180.0 - 1e-6, "on the roam clock: {}", next.0);
    }

    #[test]
    fn fling_offends() {
        let mut b = brain(6);
        let ev = [(700.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (730.0, BrainEvent::Flung { speed: 1500.0 })];
        let r = run(&mut b, 0.0, 6000.0, false, &here, &ev);
        let bids = bids(&r);
        assert!(bids[1] >= 730.0 + 2400.0 - 1.0, "{bids:?}");
        let mut b = brain(6);
        let ev = [(700.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (705.0, BrainEvent::Interaction(Touch::Click))];
        run(&mut b, 0.0, 900.0, false, &here, &ev);
        let a0 = b.attn_at(900.0);
        b.on_event(BrainEvent::Flung { speed: 1500.0 }, 900.0);
        assert!((b.attn_at(900.0) - (a0 - 0.3).max(-0.8)).abs() < 1e-9, "a plain fling: -0.3");
        assert_ne!(b.mode_key(), "sulking");
    }

    #[test]
    fn summon_bypasses_rate_but_not_gate() {
        let mut b = brain(7);
        // A bid at 700, then a prompt at 710: the summon goes at once (no gap, no grace).
        let ev = [
            (700.0, BrainEvent::Control(Ctl::Force(Force::Seek))),
            (710.0, BrainEvent::Session(SessionSignal::Prompt(1))),
            (715.0, BrainEvent::Session(SessionSignal::Prompt(1))),
        ];
        let r = run(&mut b, 0.0, 760.0, false, &here, &ev);
        let s: Vec<_> = moves(&r).into_iter().filter(|(_, p)| matches!(p.goal, Goal::Active { why: Why::Summon(_), .. })).collect();
        assert_eq!(s.len(), 1, "one summon per prompt id: {s:?}");
        assert_eq!(s[0].0, 710.0);
        // Under fullscreen: deferred until the first poll after it ends.
        let mut b = brain(8);
        let fs = |t: f64| Senses { fullscreen: (300.0..600.0).contains(&t), ..here(t) };
        let r = run(&mut b, 0.0, 700.0, false, &fs, &[(320.0, BrainEvent::Session(SessionSignal::Done))]);
        let s: Vec<_> = moves(&r).into_iter().filter(|(_, p)| matches!(p.goal, Goal::Active { .. })).collect();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].0, 600.0, "first poll after fullscreen");
        // He is near her: no summon.
        let mut b = brain(9);
        let near = |t: f64| Senses { near: true, ..here(t) };
        let r = run(&mut b, 0.0, 400.0, false, &near, &[(320.0, BrainEvent::Session(SessionSignal::Done))]);
        assert!(moves(&r).iter().all(|(_, p)| !matches!(p.goal, Goal::Active { .. })));
    }

    #[test]
    fn own_abort_is_not_news() {
        let mut b = brain(10);
        let r = run(&mut b, 0.0, 400.0, false, &here, &[(300.0, BrainEvent::Session(SessionSignal::Error { own_abort: true }))]);
        assert!(moves(&r).iter().all(|(_, p)| !matches!(p.goal, Goal::Active { .. })));
        assert_eq!(b.last_interaction, Some(300.0));
    }

    #[test]
    fn dnd_blocks_moves() {
        let mut b = brain(11);
        let ev = [(100.0, BrainEvent::Control(Ctl::Dnd(3600.0))), (200.0, BrainEvent::Session(SessionSignal::Done))];
        let r = run(&mut b, 0.0, 5000.0, false, &here, &ev);
        let m = moves(&r);
        assert!(m.iter().all(|(t, _)| *t < 100.0 || *t >= 3700.0), "{m:?}");
        assert!(m.iter().any(|(t, _)| *t >= 3700.0), "resumes");
        assert_eq!(m.iter().find(|(t, _)| *t >= 3700.0).map(|x| x.0), Some(3700.0), "the deferred summon fires when DND ends");
    }

    fn day_script(seed: u64) -> (impl Fn(f64) -> Senses, Vec<(f64, BrainEvent)>) {
        // Random presence and fullscreen in 15 s blocks, random touches and session signals.
        let mut rng = Rng::new(seed);
        let n = 24 * 3600 / 15;
        let mut idle_at = vec![0.0f64; n + 1];
        let mut fs = vec![false; n + 1];
        let mut busy = vec![false; n + 1];
        let mut away_left = 0;
        let mut fs_left = 0;
        let mut since_input = 0.0;
        for i in 0..=n {
            if away_left == 0 && rng.range(0.0, 1.0) < 0.004 {
                away_left = 20 + rng.pick(200);
            }
            if fs_left == 0 && rng.range(0.0, 1.0) < 0.003 {
                fs_left = 20 + rng.pick(300);
            }
            since_input = if away_left > 0 { since_input + 15.0 } else { 0.0 };
            idle_at[i] = since_input;
            fs[i] = fs_left > 0;
            busy[i] = rng.range(0.0, 1.0) < 0.05;
            away_left = away_left.saturating_sub(1);
            fs_left = fs_left.saturating_sub(1);
        }
        let mut ev = Vec::new();
        let mut t = 0.0;
        while t < 24.0 * 3600.0 {
            t += 200.0 + rng.range(0.0, 2400.0) as f64;
            let e = match rng.pick(6) {
                0 => BrainEvent::Interaction(Touch::Headpat),
                1 => BrainEvent::Interaction(Touch::Click),
                2 => BrainEvent::Session(SessionSignal::Done),
                3 => BrainEvent::Session(SessionSignal::Prompt(t as u64)),
                4 => BrainEvent::Session(SessionSignal::Cleared),
                _ => BrainEvent::Interaction(Touch::Typing),
            };
            ev.push(((t / 15.0).floor() * 15.0 + 7.0, e));
        }
        let f = move |t: f64| {
            let i = ((t / 15.0).floor() as usize).min(n);
            Senses { idle_s: idle_at[i], fullscreen: fs[i], fullscreen_here: fs[i], busy: if busy[i] { vec!["bubble"] } else { vec![] }, ..here(t) }
        };
        (f, ev)
    }

    #[test]
    fn day_rate_limits() {
        for seed in [21, 22, 23] {
            let mut b = brain(seed);
            let (s, ev) = day_script(seed);
            let r = run(&mut b, 0.0, 24.0 * 3600.0, false, &s, &ev);
            let m = moves(&r);
            assert!(m.len() > 40, "seed {seed}: only {} moves", m.len());
            // Zero moves while fullscreen (as polled), none while hidden.
            for (t, _) in &m {
                assert!(!s(*t).fullscreen, "seed {seed}: move at {t} under fullscreen");
            }
            // Autonomous moves (not summons, not leaving): <= 15 in any rolling hour.
            let auto: Vec<f64> =
                m.iter().filter(|(_, p)| !matches!(p.goal, Goal::Active { why: Why::Summon(_), .. } | Goal::Leave)).map(|x| x.0).collect();
            for (i, &t) in auto.iter().enumerate() {
                let n = auto[..=i].iter().filter(|&&x| x > t - 3600.0).count();
                assert!(n <= 15, "seed {seed}: {n} moves in the hour before {t}");
            }
            // Passive moves at least ROAM_MIN apart from the move before.
            let passive: Vec<f64> = m.iter().filter(|(_, p)| p.goal == Goal::Passive { far: false }).map(|x| x.0).collect();
            for (i, &t) in passive.iter().enumerate() {
                let prev = m.iter().map(|x| x.0).filter(|&x| x < t).fold(f64::MIN, f64::max);
                if i > 0 {
                    assert!(t - prev >= 180.0 - 1e-6 || prev == f64::MIN, "seed {seed}: passive move {t} only {} s after {prev}", t - prev);
                }
            }
            let bids = bids(&r);
            for w in bids.windows(2) {
                assert!(w[1] - w[0] >= 1200.0 - 1e-6, "seed {seed}: bids {w:?}");
            }
        }
    }

    #[test]
    fn calm_day_passive_gaps_within_range() {
        let mut b = brain(31);
        let s = BrainSettings { attention: Attention::Off, ..BrainSettings::default() };
        b.settings = s;
        let r = run(&mut b, 0.0, 6.0 * 3600.0, false, &here, &[]);
        let p: Vec<f64> = moves(&r).iter().map(|x| x.0).collect();
        assert!(p.len() > 30);
        for w in p.windows(2) {
            let g = w[1] - w[0];
            // Longer only when the hourly cap held it back.
            let capped = p.iter().filter(|&&m| m > w[1] - 3600.0 - 1e-6 && m < w[1]).count() >= 15;
            assert!((180.0 - 1e-6..=600.0 + 1e-6).contains(&g) || capped, "gap {g} at {}", w[1]);
        }
    }

    #[test]
    fn deterministic_and_exact_wakes() {
        let (s, ev) = day_script(41);
        let mut a = brain(41);
        let ra = run(&mut a, 0.0, 6.0 * 3600.0, false, &s, &ev);
        let mut b = brain(41);
        let rb = run(&mut b, 0.0, 6.0 * 3600.0, false, &s, &ev);
        assert_eq!(ra.log, rb.log, "same seed + script: same log");
        let mut c = brain(41);
        let rc = run(&mut c, 0.0, 6.0 * 3600.0, true, &s, &ev);
        let fmt = |r: &Run| r.log.iter().map(|(t, i)| format!("{:.6} {i:?}", t)).collect::<Vec<_>>();
        assert_eq!(fmt(&ra), fmt(&rc), "ticking every second = ticking at wake_at");
        assert!(ra.log.len() > 20);
    }

    #[test]
    fn wakes_are_sparse_at_rest() {
        // Nothing pending: one wake per presence poll (15 s), plus the drive crossings.
        let mut b = brain(42);
        b.settings.attention = Attention::Off;
        let mut wakes = 0;
        let mut t = 0.0;
        while t < 3600.0 {
            let d = b.tick(&here(t));
            t = d.wake_at;
            wakes += 1;
        }
        assert!(wakes <= 3600 / 15 + 20, "{wakes} wakes in an hour");
    }

    #[test]
    fn persist_roundtrip_and_launch_grace() {
        let mut b = brain(51);
        run(&mut b, 0.0, 1000.0, false, &here, &[(100.0, BrainEvent::Control(Ctl::Dnd(7200.0)))]);
        let s = b.saved(1000.0, 1_000_000.0);
        let back = Saved::from_json(&s.to_json()).unwrap();
        assert_eq!(back, s);
        // Restarted 100 s later (brain time = wall): attention capped, DND still on.
        let st = BrainSettings::default();
        let mut c = Brain::new(52, st.clone(), Tuning::new(&st), Some((&back, 100.0)), 50.0);
        assert!(c.attn <= 0.6 && (c.theta - b.theta).abs() < 1e-9);
        assert!(c.dnd_left(50.0).is_some_and(|d| (d - (7200.0 - 900.0 - 100.0)).abs() < 1.0));
        c.on_event(BrainEvent::Control(Ctl::Undnd), 50.0);
        c.attn = 2.0;
        let r = run(&mut c, 50.0, 50.0 + 590.0, false, &here, &[]);
        assert!(bids(&r).is_empty(), "no bid within the launch grace");
        assert!(moves(&r).iter().all(|(t, _)| *t >= 50.0 + 90.0 - 1e-6), "first roam no sooner than FIRST_ROAM_MIN");
    }

    #[test]
    fn bag_never_repeats_recent_and_skips_zero_weights() {
        let mut rng = Rng::new(61);
        let mut bag = Bag::default();
        let w = [(Approach::Hang, 3.0), (Approach::Cling, 2.0), (Approach::Peek, 0.0), (Approach::SitEdge, 1.0), (Approach::Beside, 1.0)];
        let mut picks = Vec::new();
        for _ in 0..300 {
            let o = bag.order(&w, 2, &mut rng);
            assert!(!o.contains(&Approach::Peek), "weight 0 never comes up");
            let p = o[0];
            picks.push(p);
            bag.used(p);
        }
        for win in picks.windows(3) {
            assert!(win[2] != win[1] && win[2] != win[0], "{win:?}");
        }
        // Weights still matter: hang is the most common.
        let count = |a| picks.iter().filter(|&&p| p == a).count();
        assert!(count(Approach::Hang) > count(Approach::SitEdge));
        // Fewer options than no_repeat + 1: the oldest recent one comes back.
        let mut bag = Bag::default();
        let w2 = [(Arrival::Teleport, 1.0), (Arrival::Glitch, 1.0)];
        bag.used(Arrival::Teleport);
        bag.used(Arrival::Glitch);
        assert_eq!(bag.order(&w2, 2, &mut rng), vec![Arrival::Teleport, Arrival::Glitch]);
    }

    #[test]
    fn six_bids_mix_combos() {
        let mut b = brain(71);
        let ev: Vec<_> = (0..6).map(|k| (700.0 + 1300.0 * k as f64, BrainEvent::Control(Ctl::Force(Force::Seek)))).collect();
        let pats: Vec<_> = (0..6).map(|k| (705.0 + 1300.0 * k as f64, BrainEvent::Interaction(Touch::Headpat))).collect();
        let r = run(&mut b, 0.0, 9000.0, false, &here, &[ev, pats].concat());
        let combos: Vec<_> = moves(&r)
            .into_iter()
            .filter_map(|(_, p)| match p.goal {
                Goal::Active { why: Why::Attention, approaches } => Some((approaches[0], p.arrivals[0])),
                _ => None,
            })
            .take(6)
            .collect();
        assert_eq!(combos.len(), 6);
        let distinct: std::collections::HashSet<_> = combos.iter().collect();
        assert!(distinct.len() >= 4, "{combos:?}");
        for w in combos.windows(3) {
            assert!(w[2].0 != w[1].0 && w[2].0 != w[0].0, "approach repeated within 2: {combos:?}");
        }
    }

    #[test]
    fn fullscreen_hides_and_returns() {
        let mut b = brain(81);
        let fs = |t: f64| Senses { fullscreen: (300.0..600.0).contains(&t), fullscreen_here: (300.0..600.0).contains(&t), ..here(t) };
        let r = run(&mut b, 0.0, 700.0, false, &fs, &[]);
        let hides: Vec<_> = r.log.iter().filter(|(_, i)| matches!(i, Intent::Hide(_))).cloned().collect();
        assert_eq!(hides, vec![(300.0, Intent::Hide(true)), (600.0, Intent::Hide(false))]);
        assert!(moves(&r).iter().all(|(t, _)| !(300.0..600.0).contains(t)));
        // "stay": no hide, still no moves.
        let mut b = brain(81);
        b.settings.fullscreen = Fullscreen::Stay;
        let r = run(&mut b, 0.0, 700.0, false, &fs, &[(310.0, BrainEvent::Control(Ctl::Force(Force::Roam)))]);
        assert!(r.log.iter().all(|(_, i)| !matches!(i, Intent::Hide(_))));
        assert!(moves(&r).iter().all(|(t, _)| !(300.0..600.0).contains(t)));
        assert!(moves(&r).iter().any(|(t, _)| *t == 600.0), "the forced roam waits for the gate");
    }
}
