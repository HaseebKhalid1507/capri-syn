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

impl Touch {
    /// Him handling her (a pat, a drag, the menu) doesn't wake her; talking to her does (the
    /// field, the Line, the chip, $mod+c, an answer).
    pub fn wakes(self) -> bool {
        !matches!(self, Touch::Headpat | Touch::DragStart | Touch::Menu)
    }
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
    /// The rule that asked for the move (`MovePlan::why`).
    pub why: &'static str,
}

/// What a small hop is for (all ≤ 300 px, `places::HOP_MAX`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HopKind {
    /// A micro-hop: along the edge she's on, to the next window over, down to the floor and back.
    Micro,
    /// Her perch is being dragged fast: off it, landing nearby.
    Startle,
    /// The pointer hovered on her a while: a playful hop away from it.
    Dodge,
}

impl HopKind {
    pub fn key(self) -> &'static str {
        match self {
            HopKind::Micro => "micro",
            HopKind::Startle => "startle",
            HopKind::Dodge => "dodge",
        }
    }
}

/// Things on screen she reacts to (the view sees them; she decides).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    /// He focused another window.
    Focus,
    /// The window she sits on is being dragged at this speed (device px/s).
    PerchDragged { speed: f32 },
    /// The window she sat on closed; `target`: another visible window has room on top.
    PerchClosed { target: bool },
    /// The pointer came within reach (~150 px).
    CursorNear,
    /// The pointer swept past her at this speed (device px/s).
    CursorSwipe { speed: f32 },
    /// The pointer has rested on her a while.
    CursorHover,
    /// He switched workspace. `away`: she isn't on the monitor he works on; `stranded`: her
    /// perch went with the old workspace (she'd fall).
    Workspace { away: bool, stranded: bool },
}

/// A reaction in place (no move): the view plays it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum React {
    /// Look at the pointer.
    Look,
    /// A startled jump in place.
    Startle,
    /// Lean in for a headpat.
    Lean,
    /// Her perch closed and she doesn't teleport: she falls (as before).
    Fall,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BrainEvent {
    Interaction(Touch),
    /// Released as a fling (ragdoll, R-P3: the view sends these in round B).
    #[cfg_attr(not(test), allow(dead_code))]
    Flung { speed: f32 },
    /// A drop or a throw settled; `ground`: on the floor (else a window top).
    Landed { hard: bool, ground: bool },
    Arrived(Arrived),
    /// The view couldn't do the move: nothing fits, or it is busy/gated right now. `why`:
    /// the plan's rule.
    MoveFailed { busy: bool, why: &'static str },
    Session(SessionSignal),
    Control(Ctl),
    /// Something on screen (focus, a window, the pointer, a workspace): decided at the next
    /// tick, on fresh senses (busy, typing, fullscreen, where she is).
    Screen(Screen),
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
    /// A small hop (≤ 300 px). `away`: he is typing, keep away from his window.
    Hop { kind: HopKind, away: bool },
    /// A roam while he has been idle a while: any monitor, more of the others.
    Explore,
    /// The newly focused window's top edge (else the floor beside it), never his work area.
    FocusEdge,
    /// Her perch closed: the top of another visible window (a teleport).
    Rescue,
    /// He switched workspace: a spot (a window top first) on the monitor he works on.
    Follow,
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
    /// A reaction in place.
    React(React),
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
    /// He is typing fast (input every second or so, the pointer still): only micro-hops.
    pub typing: bool,
    /// She is asleep (the session's Sleepy): nothing moves her on her own until she wakes.
    pub asleep: bool,
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
    /// A fresh start roams first this long after launch; a saved roam clock no sooner.
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
    // ---- lively (LOCKED #17) ----
    /// A micro-hop every this often while she is out of the way.
    pub micro_s: (f64, f64),
    /// What a small hop (micro, startle, dodge) counts against `move_cap_per_hour`.
    pub hop_weight: f64,
    /// A focus change makes her hop to its edge this often, this long after it, at most once
    /// in `focus_gap_s`.
    pub focus_p: f64,
    pub focus_delay_s: (f64, f64),
    pub focus_gap_s: f64,
    /// The pointer near her: a look at most this often.
    pub look_gap_s: f64,
    /// A fast swipe past her (this fast, device px/s): a startle, at most this often.
    pub swipe_speed: f32,
    pub startle_gap_s: f64,
    /// The pointer resting on her: a dodge (`dodge_p`) or a lean, at most this often.
    pub hover_gap_s: f64,
    pub dodge_p: f64,
    /// Her perch dragged this fast (device px/s): a startled hop off it, at most this often.
    pub drag_speed: f32,
    pub drag_gap_s: f64,
    /// Her perch closed and another window has room on top: she teleports there this often
    /// (Haseeb 22:55: 50–60 %), else she falls.
    pub close_teleport_p: f64,
    /// He switched workspace and she isn't on his monitor: she follows this long after.
    pub follow_delay_s: (f64, f64),
    /// Idle this long: her roams explore (any monitor).
    pub explore_idle_s: f64,
    /// Woken (Sleepy over, the nap over): no move of her own for this long (she just stood up).
    pub wake_grace_s: f64,
    /// Input this soon after he handled her while she naps (a drag, a pat) is that handling,
    /// not him being back.
    pub handled_s: f64,
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
            first_roam_min_s: 30.0,
            away_after_s: 300.0,
            back_idle_s: 3.0,
            nap_after_s: 600.0,
            bid_stage_s: 20.0,
            bid_emote_s: (6.0, 10.0),
            escalations: 2,
            sulk_s: 6.0,
            linger_s: (20.0, 60.0),
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
            micro_s: (20.0, 60.0),
            hop_weight: 0.25,
            focus_p: 1.0 / 3.0,
            focus_delay_s: (2.0, 5.0),
            focus_gap_s: 15.0,
            look_gap_s: 15.0,
            swipe_speed: 1500.0,
            startle_gap_s: 30.0,
            hover_gap_s: 45.0,
            dodge_p: 0.5,
            drag_speed: 1800.0,
            drag_gap_s: 5.0,
            close_teleport_p: 0.55,
            follow_delay_s: (0.8, 2.0),
            explore_idle_s: 120.0,
            wake_grace_s: 20.0,
            handled_s: 10.0,
        }
    }
}

/// Moves that count fully against the hourly cap and those that count `hop_weight`.
fn weight_of(why: &str, hop: f64) -> f64 {
    if matches!(why, "micro" | "dodge" | "startle_drag") { hop } else { 1.0 }
}

/// Rules whose arrival restarts the roam clock when the view held the move back.
fn roam_kind(why: &str) -> bool {
    matches!(why, "roam" | "explore" | "sulk_far" | "covering")
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
    /// Small hops (they count `hop_weight` against the cap).
    pub hops_unix: Vec<f64>,
}

impl Saved {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "v": 1, "savedUnix": self.saved_unix, "attention": self.attention, "theta": self.theta,
            "roamFrac": self.roam_frac, "lastBidUnix": self.last_bid_unix, "dndUntilUnix": self.dnd_until_unix,
            "enabled": self.enabled, "movesUnix": self.moves_unix, "hopsUnix": self.hops_unix })
    }
    pub fn from_json(v: &serde_json::Value) -> Option<Saved> {
        if v.get("v")?.as_i64()? != 1 {
            return None;
        }
        let f = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
        let list = |k: &str| v.get(k).and_then(|m| m.as_array()).map(|a| a.iter().filter_map(serde_json::Value::as_f64).collect()).unwrap_or_default();
        Some(Saved {
            saved_unix: f("savedUnix")?,
            attention: f("attention")?.clamp(-0.8, 1.2),
            theta: f("theta")?.clamp(0.1, 3.0),
            roam_frac: f("roamFrac")?.clamp(0.0, 1.0),
            last_bid_unix: f("lastBidUnix"),
            dnd_until_unix: f("dndUntilUnix"),
            enabled: v.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true),
            moves_unix: list("movesUnix"),
            hops_unix: list("hopsUnix"),
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
    /// Autonomous moves of the last hour: (time, weight against the cap, rule).
    moves: VecDeque<(f64, f64, &'static str)>,
    /// The next micro-hop.
    next_micro: f64,
    /// A focus hop and a workspace follow waiting for their beat.
    focus_at: Option<f64>,
    follow_at: Option<f64>,
    /// Rate limits of the reactions (when each last went).
    last_focus: Option<f64>,
    last_look: Option<f64>,
    last_startle: Option<f64>,
    last_hover: Option<f64>,
    last_drag: Option<f64>,
    /// Screen things waiting for the next tick's senses.
    screen_q: Vec<Screen>,
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
    /// The last wake-up (Sleepy or the nap over): the grace runs from here.
    woke_at: Option<f64>,
    /// The last time he handled her while she slept (a drag, a pat, a landing).
    handled_at: Option<f64>,
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
        // A fresh start roams first 30 s after launch (LOCKED #17).
        let first = tuning.first_roam_min_s;
        // The first micro-hop no sooner than the first roam.
        let micro = rng.range(tuning.micro_s.0.max(tuning.first_roam_min_s) as f32, tuning.micro_s.1.max(tuning.first_roam_min_s + 1.0) as f32) as f64;
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
            next_micro: now + micro,
            focus_at: None,
            follow_at: None,
            last_focus: None,
            last_look: None,
            last_startle: None,
            last_hover: None,
            last_drag: None,
            screen_q: Vec::new(),
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
            woke_at: None,
            handled_at: None,
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
            let at = |u: f64| now - ago - (s.saved_unix - u);
            let hop = b.tuning.hop_weight;
            let mut m: Vec<(f64, f64, &'static str)> = s.moves_unix.iter().map(|&u| (at(u), 1.0, "roam")).chain(s.hops_unix.iter().map(|&u| (at(u), hop, "micro"))).filter(|m| m.0 > now - 3600.0).collect();
            m.sort_by(|a, b| a.0.total_cmp(&b.0));
            b.moves = m.into();
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
            moves_unix: self.moves.iter().filter(|m| m.1 >= 1.0).map(|m| unix(m.0)).collect(),
            hops_unix: self.moves.iter().filter(|m| m.1 < 1.0).map(|m| unix(m.0)).collect(),
        }
    }

    // ---- drives ----------------------------------------------------------------------

    fn attn_rate(&self) -> f64 {
        let grows = self.present && !self.mode.active() && self.mode != Mode::Napping && !self.sensed.asleep;
        if grows { 1.0 / self.tuning.attn_fill_s } else { 0.0 }
    }

    fn roam_rate(&self) -> f64 {
        if self.present && self.mode == Mode::Passive && !self.sensed.asleep { 1.0 / self.t_roam } else { 0.0 }
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

    /// Asleep: the session's Sleepy or the away nap. Nothing moves her on her own.
    fn asleep(&self) -> bool {
        self.sensed.asleep || self.mode == Mode::Napping
    }

    /// Asleep, or just woken (the grace): her own moves wait.
    fn resting(&self, t: f64) -> bool {
        self.asleep() || self.woke_at.is_some_and(|w| t + EPS < w + self.tuning.wake_grace_s)
    }

    /// Just woken: the grace from now (the stand-up plays first, then she stays a moment).
    fn woke(&mut self, t: f64, why: &str) {
        self.woke_at = Some(t);
        self.next_micro = self.next_micro.max(t + self.tuning.wake_grace_s);
        self.note(t, format!("awake ({why})"));
    }

    /// The nap is over (he is back, he talked to her): up, and the grace.
    fn end_nap(&mut self, t: f64, why: &str) {
        if self.mode != Mode::Napping {
            return;
        }
        self.mode = Mode::Passive;
        self.handled_at = None;
        if std::mem::take(&mut self.napping_pose) {
            self.emit(t, Intent::Pose(None));
        }
        if !self.sensed.asleep {
            self.woke(t, why);
        }
    }

    /// The moves of the rolling hour, weighted (a small hop counts `hop_weight`).
    fn load(&self, t: f64) -> f64 {
        self.moves.iter().filter(|m| m.0 > t - 3600.0).map(|m| m.1).sum()
    }

    /// A move weighing `w` fits in the hour's cap.
    fn cap_ok(&self, t: f64, w: f64) -> bool {
        self.load(t) + w <= self.tuning.move_cap_per_hour as f64 + EPS
    }

    /// When the rolling hour has room for `w` again.
    fn cap_free_at(&self, t: f64, w: f64) -> f64 {
        let cap = self.tuning.move_cap_per_hour as f64;
        let mut load = self.load(t);
        if load + w <= cap + EPS {
            return t;
        }
        for m in self.moves.iter().filter(|m| m.0 > t - 3600.0) {
            load -= m.1;
            if load + w <= cap + EPS {
                return m.0 + 3600.0;
            }
        }
        t
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
                    take(self.cap_free_at(t, 2.0));
                }
                let rr = self.roam_rate();
                if rr > 0.0 && self.roam < 1.0 {
                    take(t + (1.0 - self.roam) / rr);
                }
                if self.roam >= 1.0 - EPS {
                    take(self.cap_free_at(t, 1.0));
                }
                if let Some(w) = self.woke_at {
                    take(w + self.tuning.wake_grace_s);
                }
                if self.present {
                    take(self.next_micro);
                    if let Some(f) = self.focus_at {
                        take(f);
                    }
                    if let Some(f) = self.follow_at {
                        take(f);
                    }
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
            Intent::React(r) => format!("react {r:?}"),
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
            // Asleep too: a roam due between two polls must not walk her off in her sleep.
            if self.sensed.asleep != s.asleep {
                self.sensed.asleep = s.asleep;
                if s.asleep {
                    self.note(t, "asleep".into());
                    self.focus_at = None;
                    self.follow_at = None;
                    // A visit of her own ends (a summon still goes: the session wakes her).
                    if matches!(self.mode, Mode::Bidding { .. } | Mode::Seeking { why: Why::Attention, .. }) {
                        self.end_episode(t, false);
                    }
                } else if self.mode != Mode::Napping {
                    self.woke(t, "sleepy over");
                }
            }
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
        if senses.is_some() {
            for s in std::mem::take(&mut self.screen_q) {
                self.screen(s, t);
            }
        }
        if let (false, Some(a)) = (self.present, self.away_since)
            && self.mode != Mode::Napping
            && t + EPS >= a + self.tuning.nap_after_s
        {
            self.end_episode(t, false);
            self.mode = Mode::Napping;
            self.focus_at = None;
            self.follow_at = None;
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
        // Carrying her while she naps is handling her, not him being back.
        if self.asleep() && self.sensed.busy.contains(&"drag") {
            self.handled_at = Some(t);
        }
        let handled = self.mode == Mode::Napping && self.handled_at.is_some_and(|h| t - idle <= h + self.tuning.handled_s);
        if self.present && idle >= self.tuning.away_after_s {
            self.advance(t);
            self.present = false;
            self.away_since = Some(t);
            self.note(t, format!("away (idle {idle:.0} s)"));
            if self.mode.active() {
                self.end_episode(t, true);
            }
        } else if !self.present && idle < self.tuning.back_idle_s && !handled {
            self.advance(t);
            self.present = true;
            self.away_since = None;
            self.no_bid_before = self.no_bid_before.max(t + self.tuning.back_grace_s);
            self.next_micro = t + self.micro_gap();
            self.note(t, "back".into());
            self.end_nap(t, "he is back");
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
                self.move_failed(t, false, "seek");
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
                } else if self.gate_open(t) && !self.asleep() {
                    self.record_move(t, "sulk_far");
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
        // A due micro-hop that can't go now is skipped, not held (the next one comes later).
        let micro_due = self.present && t + EPS >= self.next_micro;
        let micro_fresh = micro_due && t - self.next_micro < 5.0;
        if micro_due {
            self.next_micro = t + self.micro_gap();
        }
        // Stale reaction beats (she was busy elsewhere meanwhile) are dropped.
        let focus_due = self.focus_at.filter(|&f| t + EPS >= f).inspect(|_| self.focus_at = None);
        let follow_due = self.follow_at.filter(|&f| t + EPS >= f).inspect(|_| self.follow_at = None);
        // Asleep (or just up): none of her own moves (summons above still go).
        if self.mode != Mode::Passive || !self.gate_open(t) || self.resting(t) {
            return;
        }
        let busy = !self.sensed.busy.is_empty();
        // He is typing fast: only micro-hops (roams and visits wait for a pause).
        let typing = self.sensed.typing;
        // Reactions first: they are about what he just did.
        if let Some(f) = follow_due
            && t - f < 10.0
            && !busy
            && self.cap_ok(t, 1.0)
        {
            self.relocated(t, "workspace", Goal::Follow, vec![Arrival::Glitch, Arrival::Teleport]);
            return;
        }
        if let Some(f) = focus_due
            && t - f < 10.0
            && !busy
            && !typing
            && self.cap_ok(t, 1.0)
        {
            self.last_focus = Some(t);
            let arrivals = self.reaction_arrivals();
            self.relocated(t, "focus", Goal::FocusEdge, arrivals);
            return;
        }
        // Attention bid.
        // A bid keeps one slot of the hour free for leaving his window after it.
        let room = self.cap_ok(t, 2.0);
        let rules = self.force_seek || (t + EPS >= self.bid_allowed_at() && room && !typing);
        if self.settings.attention != Attention::Off && self.attn >= self.theta - EPS && rules && !busy {
            self.force_seek = false;
            self.prev_bid = self.last_bid;
            self.last_bid = Some(t);
            self.record_move(t, "bid");
            self.focus_at = None;
            self.follow_at = None;
            self.seek(t, Why::Attention);
            return;
        }
        // Roam: the bigger hops, in his pauses.
        if self.roam >= 1.0 - EPS && self.cap_ok(t, 1.0) && !busy && !typing {
            let far = std::mem::take(&mut self.far_next);
            let explore = !far && self.sensed.idle_s >= self.tuning.explore_idle_s;
            self.redraw_roam();
            let (goal, why) = if explore { (Goal::Explore, "explore") } else { (Goal::Passive { far }, "roam") };
            self.record_move(t, why);
            self.next_micro = self.next_micro.max(t + self.micro_gap());
            let arrivals = self.passive_arrivals();
            self.emit(t, Intent::Move(MovePlan { goal, why, arrivals }));
            self.emit(t, Intent::Save);
            return;
        }
        // Micro-hop: a little burst of life while she is out of the way.
        // A micro-hop keeps a full slot of the hour free for the next roam.
        if micro_fresh && !busy && self.hops_on() && self.out_of_the_way() && self.cap_ok(t, self.tuning.hop_weight + 1.0) {
            self.record_move(t, "micro");
            self.emit(t, Intent::Move(MovePlan { goal: Goal::Hop { kind: HopKind::Micro, away: typing }, why: "micro", arrivals: vec![Arrival::Hop] }));
        }
    }

    /// Out of the way: on the floor or a window top (not floating, hanging or on his window),
    /// not sitting low after a sulk.
    fn out_of_the_way(&self) -> bool {
        !self.covering && !self.far_next && matches!(self.sensed.placement, PlaceKind::Ground | PlaceKind::Perch)
    }

    fn micro_gap(&mut self) -> f64 {
        let (lo, hi) = self.tuning.micro_s;
        self.rng.range(lo as f32, hi as f32) as f64
    }

    /// A reaction that moves her somewhere new (focus, workspace, a closed perch): a full move,
    /// and the roam clock halves (she just went somewhere).
    fn relocated(&mut self, t: f64, why: &'static str, goal: Goal, arrivals: Vec<Arrival>) {
        self.record_move(t, why);
        self.roam *= 0.5;
        self.next_micro = self.next_micro.max(t + self.micro_gap());
        self.emit(t, Intent::Move(MovePlan { goal, why, arrivals }));
        self.emit(t, Intent::Save);
    }

    /// A hop if it's close, else the arrival bag's order (a teleport last).
    fn reaction_arrivals(&mut self) -> Vec<Arrival> {
        let mut v = if self.hops_on() { vec![Arrival::Hop] } else { Vec::new() };
        for a in self.passive_arrivals() {
            if !v.contains(&a) {
                v.push(a);
            }
        }
        v
    }

    /// `arrivals.hop = 0` in settings: she never hops (no micro-hops, dodges or startled hops).
    fn hops_on(&self) -> bool {
        self.settings.arrivals.iter().any(|a| a.0 == Arrival::Hop && a.1 > 0.0)
    }

    fn record_move(&mut self, t: f64, why: &'static str) {
        self.moves.push_back((t, weight_of(why, self.tuning.hop_weight), why));
        while self.moves.front().is_some_and(|m| m.0 <= t - 3600.0) {
            self.moves.pop_front();
        }
    }

    /// A move that didn't happen: off the books (the last one with that rule).
    fn unrecord(&mut self, why: &str) {
        if let Some(i) = self.moves.iter().rposition(|m| m.2 == why) {
            self.moves.remove(i);
        }
    }

    fn emote_gap(&mut self) -> f64 {
        let (lo, hi) = self.tuning.bid_emote_s;
        self.rng.range(lo as f32, hi as f32) as f64
    }

    /// Her own moves draw from the arrival weights too (LOCKED #17: hop 4, teleport 2, drop_in
    /// 2, glitch 1); the view takes the first that fits the distance, a teleport last.
    fn passive_arrivals(&mut self) -> Vec<Arrival> {
        let w: Vec<(Arrival, f64)> = self.settings.arrivals.iter().copied().filter(|a| a.0 != Arrival::Drift).collect();
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
        self.focus_at = None;
        self.follow_at = None;
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
        // Busy (a reaction, a bubble) doesn't hold it: the view runs it right after. Asleep
        // does: she isn't walked off in her sleep.
        if self.sensed.fullscreen || self.hidden || self.asleep() {
            return false;
        }
        self.covering = false;
        self.record_move(t, "covering");
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

    fn move_failed(&mut self, t: f64, busy: bool, why: &'static str) {
        // Hops and reactions that couldn't go simply didn't happen (the view falls for a
        // closed perch).
        if matches!(why, "micro" | "dodge" | "startle_drag" | "focus" | "workspace" | "perch_closed") {
            self.unrecord(why);
            return;
        }
        match self.mode.clone() {
            Mode::Seeking { why: Why::Attention, .. } => {
                self.mode = Mode::Passive;
                if !busy {
                    // Nowhere to go and not on his monitor: A stays, try again later.
                    self.no_bid_before = self.no_bid_before.max(t + self.tuning.retry_s);
                }
                self.last_bid = self.prev_bid;
                self.unrecord("bid");
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
                if self.asleep() && !touch.wakes() {
                    // A pat, a drag, the menu: she sleeps on (wherever he puts her).
                    self.handled_at = Some(t);
                } else if !self.present || self.mode == Mode::Napping {
                    // He is obviously here.
                    self.present = true;
                    self.away_since = None;
                    self.end_nap(t, "touch");
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
            BrainEvent::Flung { speed } if self.asleep() => {
                // Thrown in her sleep: she sleeps where she lands (no sulk, no far move).
                self.note(t, format!("flung {speed:.0} px/s (asleep)"));
                self.handled_at = Some(t);
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
            BrainEvent::Landed { hard, ground } => {
                self.note(t, format!("landed{}{}", if hard { " hard" } else { "" }, if ground { "" } else { " on a window" }));
                if self.asleep() {
                    // He put her there: she sleeps there (never moved off his window for it).
                    self.handled_at = Some(t);
                    self.covering = false;
                    if self.mode == Mode::Napping {
                        // Carrying her stood her up: the nap's pose again on the floor.
                        self.napping_pose = ground;
                        if ground {
                            self.emit(t, Intent::Pose(Some(RestPose::Nap)));
                        }
                    }
                }
            }
            BrainEvent::Arrived(a) => {
                self.note(
                    t,
                    format!("arrived {} by {}{}", a.approach.map_or("passive", Approach::key), a.arrival.key(), if a.covering { " (covering)" } else { "" }),
                );
                self.covering = a.covering;
                // Only the visit's or the summon's own landing ends the seek (a hop decided
                // just before it may land first).
                if let Mode::Seeking { why, .. } = self.mode
                    && matches!(a.why, "bid" | "summon")
                {
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
                } else if a.approach.is_none() && self.mode == Mode::Passive && roam_kind(a.why) && self.moves.iter().rev().find(|m| m.2 == a.why).is_some_and(|m| t - m.0 > 5.0) {
                    // The view held the move back (behind a clip, a bubble): the next roam
                    // counts from when she got there.
                    self.roam = 0.0;
                }
                self.emit(t, Intent::Save);
            }
            BrainEvent::MoveFailed { busy, why } => {
                self.note(t, format!("move failed{} (why {why})", if busy { " (busy)" } else { "" }));
                self.move_failed(t, busy, why);
            }
            BrainEvent::Screen(s) => {
                // Read the senses now: the reaction is decided on what is up this moment.
                self.screen_q.push(s);
                self.next_poll = self.next_poll.min(t);
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

    /// Something on screen, on this tick's senses. Every reaction needs the gate open (on, no
    /// DND, no fullscreen, he is here, not hidden) and nothing up: a bubble, a field, the Line,
    /// a prompt, the menu, a drag (`HOLDERS`), a move under way or her hidden; the moves also
    /// fit the hourly cap.
    fn screen(&mut self, s: Screen, t: f64) {
        let busy = self.sensed.busy.iter().any(|b| HOLDERS.contains(b) || matches!(*b, "teleport" | "hidden"));
        let typing = self.sensed.typing;
        self.note(t, format!("screen {s:?}{}{}", if busy { " busy" } else { "" }, if typing { " typing" } else { "" }));
        let calm = self.gate_open(t) && !busy;
        // Asleep (or just up): no reaction to the screen; only her perch closing (or leaving
        // with the workspace) still moves her, as when awake.
        let awake = !self.resting(t);
        let passive = self.mode == Mode::Passive;
        let since = |l: Option<f64>, gap: f64| l.is_none_or(|l| t - l >= gap - EPS);
        match s {
            Screen::Focus => {
                // A newer focus change replaces the hop waiting for the last one.
                self.focus_at = None;
                if calm && awake && passive && !typing && since(self.last_focus, self.tuning.focus_gap_s) && (self.rng.range(0.0, 1.0) as f64) < self.tuning.focus_p {
                    let (lo, hi) = self.tuning.focus_delay_s;
                    let at = t + self.rng.range(lo as f32, hi as f32) as f64;
                    self.focus_at = Some(at);
                    self.note(t, format!("focus hop in {:.1} s", at - t));
                }
            }
            Screen::PerchDragged { speed } => {
                let w = self.tuning.hop_weight;
                if speed >= self.tuning.drag_speed && calm && awake && passive && self.hops_on() && since(self.last_drag, self.tuning.drag_gap_s) && self.cap_ok(t, w) {
                    self.last_drag = Some(t);
                    self.record_move(t, "startle_drag");
                    self.emit(t, Intent::Move(MovePlan { goal: Goal::Hop { kind: HopKind::Startle, away: false }, why: "startle_drag", arrivals: vec![Arrival::Hop] }));
                }
            }
            Screen::PerchClosed { target } => {
                // No target, a gate shut, something up, a visit under way: she falls as before.
                let go = target && calm && !self.mode.active() && self.cap_ok(t, 1.0) && (self.rng.range(0.0, 1.0) as f64) < self.tuning.close_teleport_p;
                if go {
                    if !passive {
                        // A linger or a sulk is over: she went somewhere else.
                        self.mode = Mode::Passive;
                    }
                    self.relocated(t, "perch_closed", Goal::Rescue, vec![Arrival::Teleport]);
                } else {
                    self.emit(t, Intent::React(React::Fall));
                }
            }
            Screen::CursorNear => {
                if calm && awake && since(self.last_look, self.tuning.look_gap_s) {
                    self.last_look = Some(t);
                    self.emit(t, Intent::React(React::Look));
                }
            }
            Screen::CursorSwipe { speed } => {
                if speed >= self.tuning.swipe_speed && calm && awake && !self.mode.active() && since(self.last_startle, self.tuning.startle_gap_s) {
                    self.last_startle = Some(t);
                    // A startle counts as a look too (no look right after).
                    self.last_look = Some(t);
                    self.emit(t, Intent::React(React::Startle));
                }
            }
            Screen::CursorHover => {
                if calm && awake && since(self.last_hover, self.tuning.hover_gap_s) {
                    self.last_hover = Some(t);
                    let w = self.tuning.hop_weight;
                    let dodge = passive && self.hops_on() && self.out_of_the_way() && self.cap_ok(t, w) && (self.rng.range(0.0, 1.0) as f64) < self.tuning.dodge_p;
                    if dodge {
                        self.record_move(t, "dodge");
                        self.emit(t, Intent::Move(MovePlan { goal: Goal::Hop { kind: HopKind::Dodge, away: false }, why: "dodge", arrivals: vec![Arrival::Hop] }));
                    } else {
                        self.emit(t, Intent::React(React::Lean));
                    }
                }
            }
            Screen::Workspace { away, stranded } => {
                self.follow_at = None;
                if stranded {
                    // Her perch went with the old workspace: follow him now, else fall.
                    if calm && !self.mode.active() && self.cap_ok(t, 1.0) {
                        self.mode = Mode::Passive;
                        self.relocated(t, "workspace", Goal::Follow, vec![Arrival::Glitch, Arrival::Teleport]);
                    } else {
                        self.emit(t, Intent::React(React::Fall));
                    }
                } else if away && calm && awake && passive {
                    let (lo, hi) = self.tuning.follow_delay_s;
                    self.follow_at = Some(t + self.rng.range(lo as f32, hi as f32) as f64);
                }
            }
        }
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
            "asleep": self.asleep(),
            "wake_grace_left_s": self.woke_at.map(|w| r(w + self.tuning.wake_grace_s - now)).filter(|&g| g > 0.0),
            "idle_s": r(self.sensed.idle_s),
            "near": self.sensed.near,
            "covering": self.covering,
            "last_interaction_s": ago(self.last_interaction),
            "next_wake_in": r(self.due() - now),
            "moves_last_hour": self.moves.iter().filter(|m| m.0 > now - 3600.0).count(),
            "load_last_hour": r(self.load(now)),
            "next_micro_in": self.present.then(|| r(self.next_micro - now)),
            "focus_hop_in": self.focus_at.map(|f| r(f - now)),
            "follow_in": self.follow_at.map(|f| r(f - now)),
            "typing": self.sensed.typing,
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

/// What holds a move back at the very last moment (the view checks right before it runs one;
/// the brain only reads `busy` at its polls): a bubble, a field, the Line, a prompt, the menu
/// or a drag. Summons win, and leaving his window after a visit isn't held either (as before).
pub const HOLDERS: [&str; 6] = ["bubble", "field", "line", "prompt", "menu", "drag"];

pub fn held_by(busy: &[&str], plan: &MovePlan) -> Option<&'static str> {
    if matches!(plan.goal, Goal::Active { why: Why::Summon(_), .. } | Goal::Leave) {
        return None;
    }
    HOLDERS.iter().copied().find(|h| busy.contains(h))
}

fn plan_text(p: &MovePlan) -> String {
    let arr: Vec<_> = p.arrivals.iter().map(|a| a.key()).collect();
    match &p.goal {
        Goal::Passive { far } => format!("passive{} [{}]", if *far { " far" } else { "" }, arr.join(",")),
        Goal::Leave => format!("leave [{}]", arr.join(",")),
        Goal::Hop { kind, away } => format!("hop {}{} [{}]", kind.key(), if *away { " away" } else { "" }, arr.join(",")),
        Goal::Explore => format!("explore [{}]", arr.join(",")),
        Goal::FocusEdge => format!("focus_edge [{}]", arr.join(",")),
        Goal::Rescue => format!("rescue [{}]", arr.join(",")),
        Goal::Follow => format!("follow [{}]", arr.join(",")),
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
                    let a = Arrived { approach, arrival: p.arrivals[0], covering, why: p.why };
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
            // θ·fill = 8–15 min (LOCKED #17), not before the launch grace.
            assert!((600.0..=900.0 + 1e-6).contains(&first), "seed {seed}: first bid at {first}");
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
        assert_eq!(ma.iter().map(|m| m.0).collect::<Vec<_>>(), mb.iter().map(|m| m.0).collect::<Vec<_>>(), "the near sense flickering moves nothing");
        assert!(ma[0].0 >= b.tuning.first_roam_min_s - 1e-6, "first move {} is on the roam clock", ma[0].0);
        // Only her own clocks move her (a focus hop needs a real focus event, `focus_hop_*`).
        assert!(ma.iter().all(|(_, p)| matches!(p.why, "roam" | "micro")));
        assert!(a.snapshot(1500.0)["recent_moves"].as_array().is_some_and(|v| v.iter().all(|m| m["why"] == "roam" || m["why"] == "micro")));
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
        let ev = [(500.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (505.0, BrainEvent::Interaction(Touch::Headpat))];
        let r = run(&mut b, 0.0, 975.0, false, &here, &ev);
        let first = bids(&r);
        assert_eq!(first, vec![500.0]);
        assert!(r.log.iter().any(|(t, i)| *t == 505.0 && *i == Intent::Emote(EmoteKey::Satisfied { pat: true })));
        // If the approach covered his window, she leaves 3 s after the emote; never later.
        let covered = moves(&r).iter().any(|(t, p)| *t == 500.0 && matches!(&p.goal, Goal::Active { approaches, .. } if approaches[0].covers()));
        let left = moves(&r).iter().any(|(t, p)| *t == 508.0 && p.goal == Goal::Leave);
        assert_eq!(covered, left);
        assert!(!b.covering());
        let r = run(&mut b, 975.0, 2400.0, false, &here, &[]);
        assert!(bids(&r).first().is_none_or(|&t| t >= 500.0 + 480.0), "next bid {:?}", bids(&r));
    }

    #[test]
    fn ignored_escalates_then_sulks() {
        let mut b = brain(5);
        let theta0 = b.theta;
        let r = run(&mut b, 0.0, 600.0, false, &here, &[(500.0, BrainEvent::Control(Ctl::Force(Force::Seek)))]);
        let insists: Vec<_> = r.log.iter().filter(|(_, i)| matches!(i, Intent::Emote(EmoteKey::Insist(_)))).map(|x| x.0).collect();
        let distinct: Vec<u8> = r.log.iter().filter_map(|(_, i)| if let Intent::Emote(EmoteKey::Insist(s)) = i { Some(*s) } else { None }).collect();
        assert!(insists[0] == 520.0 && distinct.contains(&1) && distinct.contains(&2) && !distinct.contains(&3), "{insists:?} {distinct:?}");
        let sulk = r.log.iter().find(|(_, i)| *i == Intent::Emote(EmoteKey::Sulk)).expect("sulk").0;
        assert_eq!(sulk, 560.0);
        // She stays low after the sulk (r6 critics): no move then; her next roam goes far.
        assert!(moves(&r).into_iter().all(|(t, _)| t <= sulk), "no move right after the sulk");
        assert!(b.theta >= b.tuning.theta.0 + 0.3 - 1e-6 && b.theta != theta0);
        let r = run(&mut b, 600.0, 2000.0, false, &here, &[]);
        let next = moves(&r).into_iter().next().expect("a roam later");
        assert_eq!(next.1.goal, Goal::Passive { far: true });
        assert!(next.0 >= 566.0 + 45.0 - 1e-6, "on the roam clock: {}", next.0);
    }

    #[test]
    fn fling_offends() {
        let mut b = brain(6);
        let ev = [(500.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (530.0, BrainEvent::Flung { speed: 1500.0 })];
        let r = run(&mut b, 0.0, 6000.0, false, &here, &ev);
        let bids = bids(&r);
        // From -0.8 back up to θ (>= 0.32): at least 1.12 fills.
        assert!(bids[1] >= 530.0 + (0.8 + b.tuning.theta.0) * b.tuning.attn_fill_s - 1.0, "{bids:?}");
        let mut b = brain(6);
        let ev = [(500.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (505.0, BrainEvent::Interaction(Touch::Click))];
        run(&mut b, 0.0, 900.0, false, &here, &ev);
        let a0 = b.attn_at(900.0);
        b.on_event(BrainEvent::Flung { speed: 1500.0 }, 900.0);
        assert!((b.attn_at(900.0) - (a0 - 0.3).max(-0.8)).abs() < 1e-9, "a plain fling: -0.3");
        assert_ne!(b.mode_key(), "sulking");
    }

    #[test]
    fn summon_bypasses_rate_but_not_gate() {
        let mut b = brain(7);
        // A bid at 500, then a prompt at 510: the summon goes at once (no gap, no grace).
        let ev = [
            (500.0, BrainEvent::Control(Ctl::Force(Force::Seek))),
            (510.0, BrainEvent::Session(SessionSignal::Prompt(1))),
            (515.0, BrainEvent::Session(SessionSignal::Prompt(1))),
        ];
        let r = run(&mut b, 0.0, 560.0, false, &here, &ev);
        let s: Vec<_> = moves(&r).into_iter().filter(|(_, p)| matches!(p.goal, Goal::Active { why: Why::Summon(_), .. })).collect();
        assert_eq!(s.len(), 1, "one summon per prompt id: {s:?}");
        assert_eq!(s[0].0, 510.0);
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
        // The screen: focus changes, the pointer, windows, workspaces, every 20–260 s.
        let mut t = 0.0;
        while t < 24.0 * 3600.0 {
            t += 20.0 + rng.range(0.0, 240.0) as f64;
            let sc = match rng.pick(7) {
                0 | 1 => Screen::Focus,
                2 => Screen::CursorNear,
                3 => Screen::CursorHover,
                4 => Screen::CursorSwipe { speed: rng.range(800.0, 3000.0) },
                5 => Screen::PerchClosed { target: rng.pick(4) > 0 },
                _ => Screen::Workspace { away: rng.pick(2) == 0, stranded: rng.pick(3) == 0 },
            };
            ev.push(((t / 15.0).floor() * 15.0 + 11.0, BrainEvent::Screen(sc)));
        }
        // Sampled at the whole second (a step at 44.6 sees what a tick at 45 sees): the world
        // changes at most once a second, so ticking every second can match the exact wakes.
        let f = move |t: f64| {
            let i = ((t.ceil() / 15.0).floor() as usize).min(n);
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
            // Autonomous moves (not summons, not leaving): <= 45 in any rolling hour, a small hop
            // counting a quarter (LOCKED #17).
            let auto: Vec<(f64, f64)> = m
                .iter()
                .filter(|(_, p)| !matches!(p.goal, Goal::Active { why: Why::Summon(_), .. } | Goal::Leave))
                .map(|(t, p)| (*t, weight_of(p.why, 0.25)))
                .collect();
            for (i, &(t, _)) in auto.iter().enumerate() {
                let load: f64 = auto[..=i].iter().filter(|x| x.0 > t - 3600.0).map(|x| x.1).sum();
                assert!(load <= 45.0 + 1e-9, "seed {seed}: {load} moves in the hour before {t}");
            }
            assert!(m.iter().any(|(_, p)| p.why == "micro"), "seed {seed}: micro-hops happen");
            // Roams at least ROAM_MIN (45 s) after the last roam, visit, summon or sulk move
            // (reactions and hops don't feed the roam clock).
            let clock: Vec<(f64, &str)> = m.iter().filter(|(_, p)| !matches!(p.why, "micro" | "dodge" | "startle_drag" | "focus" | "workspace" | "perch_closed")).map(|(t, p)| (*t, p.why)).collect();
            for (i, &(t, why)) in clock.iter().enumerate() {
                if i > 0 && matches!(why, "roam" | "explore") {
                    let prev = clock[i - 1].0;
                    assert!(t - prev >= 45.0 - 1e-6, "seed {seed}: {why} at {t} only {} s after {prev}", t - prev);
                }
            }
            let bids = bids(&r);
            for w in bids.windows(2) {
                assert!(w[1] - w[0] >= 480.0 - 1e-6, "seed {seed}: bids {w:?}");
            }
        }
    }

    #[test]
    fn calm_day_lively_rates() {
        // LOCKED #17: roams every 0.75–3 min, micro-hops every 20–60 s, <= 45 an hour (a hop a
        // quarter): longer gaps only where the hourly cap held one back.
        for seed in [31, 32, 33] {
            let mut b = brain(seed);
            b.settings.attention = Attention::Off;
            let m = moves(&run(&mut b, 0.0, 6.0 * 3600.0, false, &here, &[]));
            let load_before = |t: f64| -> f64 { m.iter().filter(|x| x.0 > t - 3600.0 - 1e-6 && x.0 < t).map(|x| weight_of(x.1.why, 0.25)).sum() };
            let roams: Vec<f64> = m.iter().filter(|x| x.1.why == "roam").map(|x| x.0).collect();
            let micros: Vec<f64> = m.iter().filter(|x| x.1.why == "micro").map(|x| x.0).collect();
            assert!(roams.len() > 6 * 20 && micros.len() > 6 * 40, "seed {seed}: {} roams, {} micro-hops in 6 h", roams.len(), micros.len());
            for w in roams.windows(2) {
                let g = w[1] - w[0];
                let capped = load_before(w[1]) + 1.0 > 45.0 - 1e-6;
                assert!((45.0 - 1e-6..=180.0 + 1e-6).contains(&g) || (g > 180.0 && capped), "seed {seed}: roam gap {g} at {}", w[1]);
            }
            let uncapped = roams.windows(2).filter(|w| w[1] - w[0] <= 180.0 + 1e-6).count();
            assert!(uncapped * 10 >= (roams.len() - 1) * 7, "seed {seed}: the cap held back {} of {} roams", roams.len() - 1 - uncapped, roams.len() - 1);
            for w in micros.windows(2) {
                assert!(w[1] - w[0] >= 20.0 - 1e-6, "seed {seed}: micro-hops {w:?}");
            }
            // Mean micro gap: 20–60 s, plus the beats skipped for the cap or a roam.
            let mean = (micros.last().unwrap() - micros[0]) / (micros.len() - 1) as f64;
            assert!((25.0..=100.0).contains(&mean), "seed {seed}: mean micro gap {mean}");
            for x in &m {
                assert!(load_before(x.0 + 1e-3) <= 45.0 + 1e-9, "seed {seed}: over the cap at {}", x.0);
            }
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
        // (The last second: the every-second run's next tick falls past the end.)
        let fmt = |r: &Run| r.log.iter().filter(|(t, _)| *t < 6.0 * 3600.0 - 1.0).map(|(t, i)| format!("{:.6} {i:?}", t)).collect::<Vec<_>>();
        let (fa, fc) = (fmt(&ra), fmt(&rc));
        if let Some(k) = (0..fa.len().max(fc.len())).find(|&k| fa.get(k) != fc.get(k)) {
            let near = |v: &Vec<String>| v[k.saturating_sub(3)..(k + 3).min(v.len())].to_vec();
            panic!("ticking every second != ticking at wake_at from #{k}:\n wake: {:#?}\n every second: {:#?}", near(&fa), near(&fc));
        }
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
        // Polls, micro-hop beats (>= 20 s) and roam crossings (>= 45 s).
        assert!(wakes <= 3600 / 15 + 3600 / 20 + 3600 / 45 + 20, "{wakes} wakes in an hour");
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
        assert!(moves(&r).iter().all(|(t, _)| *t >= 50.0 + 30.0 - 1e-6), "first move no sooner than FIRST_ROAM_MIN (30 s)");
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
        let ev: Vec<_> = (0..6).map(|k| (500.0 + 1300.0 * k as f64, BrainEvent::Control(Ctl::Force(Force::Seek)))).collect();
        let pats: Vec<_> = (0..6).map(|k| (505.0 + 1300.0 * k as f64, BrainEvent::Interaction(Touch::Headpat))).collect();
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

    // ---- lively (LOCKED #17) -------------------------------------------------------------

    fn screen(at: f64, sc: Screen) -> (f64, BrainEvent) {
        (at, BrainEvent::Screen(sc))
    }
    fn whys(r: &Run) -> Vec<(f64, &'static str)> {
        moves(r).into_iter().map(|(t, p)| (t, p.why)).collect()
    }
    fn reacts(r: &Run) -> Vec<(f64, React)> {
        r.log.iter().filter_map(|(t, i)| if let Intent::React(x) = i { Some((*t, *x)) } else { None }).collect()
    }
    /// Attention off (no visits in the way). Seeds 2k and 2k+1 would be the same rng: odd only.
    fn calm(seed: u64) -> Brain {
        let s = BrainSettings { attention: Attention::Off, ..Default::default() };
        Brain::new(2 * seed + 1, s.clone(), Tuning::new(&s), None, 0.0)
    }

    #[test]
    fn lively_tuning_is_the_lock() {
        let t = Tuning::new(&BrainSettings::default());
        assert_eq!(t.roam_s, (45.0, 180.0), "roams every 0.75–3 min");
        assert_eq!(t.active_gap_min_s, 480.0, "visits no closer than 8 min");
        assert!((t.theta.0 * t.attn_fill_s - 480.0).abs() < 1e-6 && (t.theta.1 * t.attn_fill_s - 900.0).abs() < 1e-6, "visits every 8–15 min");
        assert_eq!(t.move_cap_per_hour, 45);
        assert_eq!(t.hop_weight, 0.25, "a micro-hop counts a quarter");
        assert_eq!(t.first_roam_min_s, 30.0);
        assert_eq!(t.linger_s, (20.0, 60.0));
        assert_eq!(t.micro_s, (20.0, 60.0));
        assert!((0.5..=0.6).contains(&t.close_teleport_p), "Haseeb 22:55: 50–60 %");
        assert!((t.focus_p - 1.0 / 3.0).abs() < 1e-9 && t.focus_delay_s == (2.0, 5.0));
    }

    #[test]
    fn first_roam_30s_after_launch() {
        for seed in 1..60 {
            let mut b = calm(seed);
            let m = moves(&run(&mut b, 0.0, 400.0, false, &here, &[]));
            let first_roam = m.iter().find(|x| x.1.why == "roam").expect("a roam").0;
            assert!((first_roam - 30.0).abs() < 1e-6, "seed {seed}: first roam at {first_roam}");
            assert!(m[0].0 >= 30.0 - 1e-6, "seed {seed}: nothing before 30 s ({})", m[0].0);
        }
    }

    #[test]
    fn linger_after_a_visit_is_20_to_60_s() {
        for seed in 1..30 {
            let mut b = brain(seed);
            let ev = [(500.0, BrainEvent::Control(Ctl::Force(Force::Seek))), (505.0, BrainEvent::Interaction(Touch::Headpat))];
            let r = run(&mut b, 0.0, 505.0 + 0.5, false, &here, &ev);
            assert!(r.log.iter().any(|(_, i)| *i == Intent::Emote(EmoteKey::Satisfied { pat: true })));
            let Mode::Satisfied { until, .. } = b.mode else { panic!("seed {seed}: {:?}", b.mode) };
            assert!((525.0 - 1e-3..=565.0 + 1e-3).contains(&until), "seed {seed}: linger until {until}");
        }
    }

    #[test]
    fn micro_hops_only_while_out_of_the_way() {
        let only = |pk: PlaceKind| move |t: f64| Senses { placement: pk, ..here(t) };
        for (pk, want) in [(PlaceKind::Ground, true), (PlaceKind::Perch, true), (PlaceKind::Float, false), (PlaceKind::Hang, false)] {
            let mut b = calm(5);
            let r = run(&mut b, 0.0, 1800.0, false, &only(pk), &[]);
            let n = whys(&r).iter().filter(|x| x.1 == "micro").count();
            assert_eq!(n > 0, want, "{pk:?}: {n} micro-hops");
        }
        // Busy (a bubble, the Line, a menu...) at the polls: no micro-hop, no roam.
        let mut b = calm(6);
        let busy = |t: f64| Senses { busy: vec!["line"], ..here(t) };
        assert!(moves(&run(&mut b, 0.0, 1800.0, false, &busy, &[])).is_empty());
        // Sitting low after a sulk: no micro-hop until the far roam.
        for seed in 1..20 {
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 2000.0, false, &here, &[(500.0, BrainEvent::Control(Ctl::Force(Force::Seek)))]);
            let sulk = r.log.iter().find(|(_, i)| *i == Intent::Emote(EmoteKey::Sulk)).expect("sulk").0;
            let after: Vec<_> = moves(&r).into_iter().filter(|x| x.0 >= sulk && x.1.why != "covering").collect();
            assert!(after.first().is_some_and(|x| x.1.goal == Goal::Passive { far: true }), "seed {seed}: {:?}", after.first());
        }
    }

    #[test]
    fn typing_only_micro_hops_then_the_pause_brings_the_roam() {
        // Typing fast 0–1200 s: no roams or visits, micro-hops keep away from his window.
        let typing = |t: f64| Senses { typing: t < 1200.0, ..here(t) };
        for seed in 1..15 {
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 1200.0, false, &typing, &[]);
            assert!(bids(&r).is_empty(), "seed {seed}: no visit while he types");
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 1600.0, false, &typing, &[]);
            let m = moves(&r);
            assert!(m.iter().filter(|x| x.0 < 1200.0).all(|x| x.1.goal == Goal::Hop { kind: HopKind::Micro, away: true }), "seed {seed}: {m:?}");
            assert!(m.iter().any(|x| x.0 < 1200.0), "seed {seed}: micro-hops while typing");
            // The pause: the held roam goes at the first poll (<= 15 s), the bigger hop.
            let roam = m.iter().find(|x| x.0 >= 1200.0 && x.1.why == "roam").expect("roam after the pause").0;
            assert!(roam <= 1200.0 + 15.0 + 1e-6, "seed {seed}: roam at {roam}");
        }
    }

    #[test]
    fn idle_two_minutes_explores() {
        let mut b = calm(9);
        let idle = |t: f64| Senses { idle_s: if t < 600.0 { 0.0 } else { (t - 600.0).min(250.0) }, ..here(t) };
        let r = run(&mut b, 0.0, 2000.0, false, &idle, &[]);
        let m = moves(&r);
        assert!(m.iter().filter(|x| x.0 < 600.0).all(|x| x.1.why != "explore"));
        assert!(m.iter().any(|x| x.1.goal == Goal::Explore && x.0 >= 720.0), "{:?}", whys(&r));
        assert!(m.iter().filter(|x| x.1.goal == Goal::Explore).all(|x| x.0 >= 720.0 - 1e-6), "only after 2 min idle");
    }

    #[test]
    fn focus_hop_about_one_in_three_2_to_5_s_later() {
        let (mut hops, mut n) = (0, 0);
        for seed in 0..1500u64 {
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 60.0, false, &here, &[screen(40.0, Screen::Focus)]);
            n += 1;
            if let Some((t, p)) = moves(&r).into_iter().find(|x| x.1.why == "focus") {
                hops += 1;
                assert!((42.0 - 1e-6..=45.0 + 1e-6).contains(&t), "seed {seed}: focus hop at {t}");
                assert_eq!(p.goal, Goal::FocusEdge);
                assert_eq!(p.arrivals[0], Arrival::Hop, "a hop when it's close");
            }
        }
        let share = hops as f64 / n as f64;
        assert!((share - 1.0 / 3.0).abs() <= 0.05, "focus hops {share:.3}");
        // Never while typing, busy, under DND or fullscreen; a newer focus change replaces the
        // waiting hop; at most one in 15 s.
        let busy_at = |t: f64| Senses { busy: if (39.0..50.0).contains(&t) { vec!["menu"] } else { vec![] }, ..here(t) };
        let typing_at = |t: f64| Senses { typing: (39.0..50.0).contains(&t), ..here(t) };
        for seed in 0..300u64 {
            for (k, s) in [&busy_at as &dyn Fn(f64) -> Senses, &typing_at].into_iter().enumerate() {
                let mut b = calm(seed);
                let r = run(&mut b, 0.0, 60.0, false, s, &[screen(40.0, Screen::Focus)]);
                assert!(whys(&r).iter().all(|x| x.1 != "focus"), "seed {seed} case {k}");
            }
            let mut b = calm(seed);
            let fs = |t: f64| Senses { fullscreen: true, ..here(t) };
            assert!(whys(&run(&mut b, 0.0, 60.0, false, &fs, &[screen(40.0, Screen::Focus)])).is_empty());
            let mut b = calm(seed);
            let ev = [(10.0, BrainEvent::Control(Ctl::Dnd(600.0))), screen(40.0, Screen::Focus)];
            assert!(whys(&run(&mut b, 0.0, 60.0, false, &here, &ev)).is_empty());
            let mut b = calm(seed);
            let ev: Vec<_> = (0..20).map(|k| screen(40.0 + 6.0 * k as f64, Screen::Focus)).collect();
            let f: Vec<f64> = whys(&run(&mut b, 0.0, 200.0, false, &here, &ev)).into_iter().filter(|x| x.1 == "focus").map(|x| x.0).collect();
            assert!(f.windows(2).all(|w| w[1] - w[0] >= 15.0 - 1e-6), "seed {seed}: {f:?}");
        }
    }

    #[test]
    fn fast_drag_startles_her_off() {
        let drag = |v: f32| Screen::PerchDragged { speed: v };
        let perch = |t: f64| Senses { placement: PlaceKind::Perch, ..here(t) };
        let mut b = calm(3);
        let r = run(&mut b, 0.0, 50.0, false, &perch, &[screen(40.0, drag(2500.0)), screen(41.0, drag(2600.0))]);
        let s: Vec<_> = moves(&r).into_iter().filter(|x| x.1.why == "startle_drag").collect();
        assert_eq!(s.len(), 1, "one startle per {} s: {s:?}", b.tuning.drag_gap_s);
        assert_eq!(s[0].1.goal, Goal::Hop { kind: HopKind::Startle, away: false });
        // A slow drag: she rides it (as before).
        let mut b = calm(3);
        assert!(whys(&run(&mut b, 0.0, 50.0, false, &perch, &[screen(40.0, drag(600.0))])).iter().all(|x| x.1 != "startle_drag"));
        // Busy or DND: she rides it.
        let mut b = calm(3);
        let busy = |t: f64| Senses { busy: vec!["bubble"], ..perch(t) };
        assert!(whys(&run(&mut b, 0.0, 50.0, false, &busy, &[screen(40.0, drag(3000.0))])).is_empty());
        let mut b = calm(3);
        let ev = [(10.0, BrainEvent::Control(Ctl::Dnd(600.0))), screen(40.0, drag(3000.0))];
        assert!(whys(&run(&mut b, 0.0, 50.0, false, &perch, &ev)).is_empty());
    }

    #[test]
    fn cursor_look_startle_dodge_or_lean_rate_limited() {
        // Near every 2 s for a minute: a look at most every 15 s.
        let mut b = calm(4);
        let ev: Vec<_> = (0..30).map(|k| screen(31.0 + 2.0 * k as f64, Screen::CursorNear)).collect();
        let looks: Vec<f64> = reacts(&run(&mut b, 0.0, 95.0, false, &here, &ev)).into_iter().filter(|x| x.1 == React::Look).map(|x| x.0).collect();
        assert!(looks.len() >= 4 && looks.windows(2).all(|w| w[1] - w[0] >= 15.0 - 1e-6), "{looks:?}");
        // Swipes: a fast one startles (at most every 30 s), a slow one doesn't.
        let mut b = calm(4);
        let ev = [screen(31.0, Screen::CursorSwipe { speed: 900.0 }), screen(33.0, Screen::CursorSwipe { speed: 2400.0 }), screen(40.0, Screen::CursorSwipe { speed: 2400.0 }), screen(64.0, Screen::CursorSwipe { speed: 2400.0 })];
        let st: Vec<f64> = reacts(&run(&mut b, 0.0, 70.0, false, &here, &ev)).into_iter().filter(|x| x.1 == React::Startle).map(|x| x.0).collect();
        assert_eq!(st, vec![33.0, 64.0]);
        // Hover: a dodge (a hop away, a quarter move) or a lean, about half each, every 45 s at most.
        let (mut dodges, mut leans) = (0, 0);
        for seed in 0..1000u64 {
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &here, &[screen(31.0, Screen::CursorHover), screen(35.0, Screen::CursorHover)]);
            let d = whys(&r).iter().filter(|x| x.1 == "dodge").count();
            let l = reacts(&r).iter().filter(|x| x.1 == React::Lean).count();
            assert_eq!(d + l, 1, "seed {seed}: one per 45 s");
            dodges += d;
            leans += l;
        }
        assert!((dodges as f64 / 1000.0 - 0.5).abs() <= 0.05, "{dodges} dodges, {leans} leans");
        // None of it under DND, fullscreen, or with something up.
        let fs: fn(f64) -> Senses = |t: f64| Senses { fullscreen: true, ..here(t) };
        for (ev, s) in [(vec![(10.0, BrainEvent::Control(Ctl::Dnd(600.0)))], here as fn(f64) -> Senses), (vec![], fs)] {
            let mut b = calm(4);
            let mut ev = ev;
            ev.extend([screen(31.0, Screen::CursorNear), screen(32.0, Screen::CursorSwipe { speed: 3000.0 }), screen(33.0, Screen::CursorHover)]);
            let r = run(&mut b, 0.0, 40.0, false, &s, &ev);
            assert!(reacts(&r).is_empty() && moves(&r).is_empty(), "{:?}", r.log);
        }
        let mut b = calm(4);
        let busy = |t: f64| Senses { busy: vec!["prompt"], ..here(t) };
        let ev = [screen(31.0, Screen::CursorNear), screen(32.0, Screen::CursorHover)];
        assert!(reacts(&run(&mut b, 0.0, 40.0, false, &busy, &ev)).is_empty());
    }

    #[test]
    fn perch_close_teleport_or_fall_split() {
        // A window she sits on closes: 55 % a teleport to another window's top, else the fall.
        let perch = |t: f64| Senses { placement: PlaceKind::Perch, ..here(t) };
        let (mut tele, n) = (0, 4000u64);
        for seed in 0..n {
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &perch, &[screen(40.0, Screen::PerchClosed { target: true })]);
            let m: Vec<_> = moves(&r).into_iter().filter(|x| x.0 == 40.0).collect();
            let f = reacts(&r).iter().filter(|x| x.1 == React::Fall).count();
            match (m.first(), f) {
                (Some((_, p)), 0) => {
                    assert_eq!((p.goal.clone(), p.arrivals.clone(), p.why), (Goal::Rescue, vec![Arrival::Teleport], "perch_closed"));
                    tele += 1;
                }
                (None, 1) => {}
                other => panic!("seed {seed}: {other:?}"),
            }
        }
        let share = tele as f64 / n as f64;
        assert!((share - 0.55).abs() <= 0.03 && (0.5..=0.6).contains(&share), "teleport share {share:.3}");
        // No other window with room on top: she always falls.
        for seed in 0..500 {
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &perch, &[screen(40.0, Screen::PerchClosed { target: false })]);
            assert!(moves(&r).iter().all(|x| x.0 != 40.0) && reacts(&r) == vec![(40.0, React::Fall)], "seed {seed}");
        }
        // DND, fullscreen, something up, or mid-visit: the fall (no decision to move).
        for seed in 0..200 {
            let mut b = calm(seed);
            let ev = [(10.0, BrainEvent::Control(Ctl::Dnd(600.0))), screen(40.0, Screen::PerchClosed { target: true })];
            assert_eq!(reacts(&run(&mut b, 0.0, 41.0, false, &perch, &ev)), vec![(40.0, React::Fall)]);
            let mut b = calm(seed);
            let busy = |t: f64| Senses { busy: if t >= 35.0 { vec!["line"] } else { vec![] }, ..perch(t) };
            assert_eq!(reacts(&run(&mut b, 0.0, 41.0, false, &busy, &[screen(40.0, Screen::PerchClosed { target: true })])), vec![(40.0, React::Fall)]);
            let mut b = brain(seed);
            let ev = [(500.0, BrainEvent::Control(Ctl::Force(Force::Seek))), screen(505.0, Screen::PerchClosed { target: true })];
            let r = run(&mut b, 0.0, 506.0, false, &perch, &ev);
            assert!(reacts(&r).contains(&(505.0, React::Fall)) && whys(&r).iter().all(|x| x.1 != "perch_closed"));
        }
    }

    #[test]
    fn workspace_follow_within_two_seconds() {
        for seed in 0..200 {
            // On another monitor than his: she follows 0.8–2 s later.
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 45.0, false, &here, &[screen(40.0, Screen::Workspace { away: true, stranded: false })]);
            let f: Vec<_> = moves(&r).into_iter().filter(|x| x.1.why == "workspace").collect();
            assert_eq!(f.len(), 1, "seed {seed}");
            assert!((40.8 - 1e-6..=42.0 + 1e-6).contains(&f[0].0) && f[0].1.goal == Goal::Follow, "seed {seed}: {f:?}");
            // Her perch went with the old workspace: she follows right away.
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &here, &[screen(40.0, Screen::Workspace { away: true, stranded: true })]);
            assert!(moves(&r).iter().any(|x| x.0 == 40.0 && x.1.goal == Goal::Follow));
            // On his monitor already: nothing.
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 45.0, false, &here, &[screen(40.0, Screen::Workspace { away: false, stranded: false })]);
            assert!(whys(&r).iter().all(|x| x.1 != "workspace"));
            // DND: stranded falls, away stays.
            let mut b = calm(seed);
            let ev = [(10.0, BrainEvent::Control(Ctl::Dnd(600.0))), screen(40.0, Screen::Workspace { away: true, stranded: true }), screen(42.0, Screen::Workspace { away: true, stranded: false })];
            let r = run(&mut b, 0.0, 45.0, false, &here, &ev);
            assert!(moves(&r).is_empty() && reacts(&r) == vec![(40.0, React::Fall)]);
        }
    }

    #[test]
    fn protected_summons_win_over_reactions() {
        // A focus hop waits for its beat; a prompt summon comes first and the hop is dropped.
        for seed in 0..200 {
            let mut b = calm(seed);
            let ev = [screen(700.0, Screen::Focus), (701.0, BrainEvent::Session(SessionSignal::Prompt(1))), screen(702.0, Screen::CursorHover)];
            let r = run(&mut b, 0.0, 760.0, false, &here, &ev);
            let m = whys(&r);
            assert!(m.iter().any(|x| x == &(701.0, "summon")), "seed {seed}: {m:?}");
            assert!(m.iter().filter(|x| x.0 >= 701.0).all(|x| x.1 == "summon"), "seed {seed}: nothing moves her off a summon {m:?}");
        }
    }

    #[test]
    fn protected_dnd_and_fullscreen_hold_everything_new() {
        // A day of screen events: DND 2 h and fullscreen blocks: no move, no reaction but the
        // fall (physics) while they hold.
        for seed in [91, 92] {
            let (s, mut ev) = day_script(seed);
            ev.push((3600.0, BrainEvent::Control(Ctl::Dnd(7200.0))));
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 6.0 * 3600.0, false, &s, &ev);
            for (t, i) in &r.log {
                let quiet = (3600.0..10800.0).contains(t) || s(*t).fullscreen;
                if quiet {
                    let leave = matches!(i, Intent::Move(p) if p.goal == Goal::Leave);
                    assert!(leave || matches!(i, Intent::React(React::Fall) | Intent::Hide(_) | Intent::Pose(_) | Intent::Emote(_)), "seed {seed}: {i:?} at {t}");
                }
            }
        }
    }

    #[test]
    fn protected_busy_holds_moves() {
        // The Line open 600–1800 s (polled): no autonomous move once a poll saw it (<= 15 s).
        for seed in 0..40 {
            let mut b = brain(seed);
            let s = |t: f64| Senses { busy: if (600.0..1800.0).contains(&t) { vec!["line"] } else { vec![] }, ..here(t) };
            let ev = [screen(900.0, Screen::Focus), screen(1000.0, Screen::CursorHover)];
            let m = whys(&run(&mut b, 0.0, 2400.0, false, &s, &ev));
            assert!(m.iter().all(|x| !(615.0..1800.0).contains(&x.0)), "seed {seed}: {m:?}");
        }
        // And the view's last check holds every plan but a summon or leaving his window.
        let plan = |goal: Goal, why: &'static str| MovePlan { goal, why, arrivals: vec![Arrival::Teleport] };
        for busy in [&["bubble"][..], &["line"], &["prompt"], &["menu"], &["field"], &["drag"]] {
            for p in [plan(Goal::Passive { far: false }, "roam"), plan(Goal::Hop { kind: HopKind::Micro, away: false }, "micro"), plan(Goal::FocusEdge, "focus"), plan(Goal::Rescue, "perch_closed"), plan(Goal::Follow, "workspace"), plan(Goal::Active { why: Why::Attention, approaches: vec![] }, "bid")] {
                assert!(held_by(busy, &p).is_some(), "{busy:?} {p:?}");
            }
            assert_eq!(held_by(busy, &plan(Goal::Active { why: Why::Summon(SummonKind::Done), approaches: vec![] }, "summon")), None, "summons win");
            assert_eq!(held_by(busy, &plan(Goal::Leave, "covering")), None);
        }
        assert_eq!(held_by(&["music", "session", "reaction"], &plan(Goal::Passive { far: false }, "roam")), None);
    }

    #[test]
    fn hang_only_still_only_hangs() {
        // His settings.toml: attention = "hang_only". Every visit leads with the hang; no
        // lively reaction asks for an approach.
        let s = BrainSettings { attention: Attention::HangOnly, ..Default::default() };
        let mut b = Brain::new(12, s.clone(), Tuning::new(&s), None, 0.0);
        let (sc, ev) = day_script(12);
        let r = run(&mut b, 0.0, 12.0 * 3600.0, false, &sc, &ev);
        let visits: Vec<_> = moves(&r).into_iter().filter_map(|(_, p)| if let Goal::Active { why: Why::Attention, approaches } = p.goal { Some(approaches) } else { None }).collect();
        assert!(visits.len() >= 3, "{} visits", visits.len());
        assert!(visits.iter().all(|a| a == &vec![Approach::Hang, Approach::Beside]), "{visits:?}");
    }

    #[test]
    fn hop_weight_is_a_quarter_and_persists() {
        let mut b = calm(13);
        run(&mut b, 0.0, 1200.0, false, &here, &[]);
        let full = b.moves.iter().filter(|m| m.1 == 1.0).count() as f64;
        let hops = b.moves.iter().filter(|m| m.1 == 0.25).count() as f64;
        assert!(hops > 0.0 && (b.load(1200.0) - (full + hops / 4.0)).abs() < 1e-9);
        let s = b.saved(1200.0, 2_000_000.0);
        let back = Saved::from_json(&s.to_json()).unwrap();
        assert_eq!(back.hops_unix.len() as f64, hops);
        let st = BrainSettings::default();
        let c = Brain::new(14, st.clone(), Tuning::new(&st), Some((&back, 0.0)), 1200.0);
        assert!((c.load(1200.0) - b.load(1200.0)).abs() < 1e-9, "the hour's load survives a restart");
        // An old state.json (no hopsUnix) still loads.
        let mut v = s.to_json();
        v.as_object_mut().unwrap().remove("hopsUnix");
        assert!(Saved::from_json(&v).is_some_and(|x| x.hops_unix.is_empty()));
    }

    #[test]
    fn hop_zero_means_no_hops() {
        // `arrivals = { hop = 0 }`: no micro-hops, no dodges, no startled hops; she still roams.
        let mut st = BrainSettings { attention: Attention::Off, ..Default::default() };
        st.arrivals.iter_mut().filter(|a| a.0 == Arrival::Hop).for_each(|a| a.1 = 0.0);
        for seed in 0..50u64 {
            let mut b = Brain::new(2 * seed + 1, st.clone(), Tuning::new(&st), None, 0.0);
            let perch = |t: f64| Senses { placement: PlaceKind::Perch, ..here(t) };
            let ev = [screen(100.0, Screen::CursorHover), screen(200.0, Screen::PerchDragged { speed: 4000.0 }), screen(300.0, Screen::Focus)];
            let r = run(&mut b, 0.0, 1200.0, false, &perch, &ev);
            let m = moves(&r);
            assert!(m.iter().all(|x| !matches!(x.1.goal, Goal::Hop { .. }) && !x.1.arrivals.contains(&Arrival::Hop)), "seed {seed}: {m:?}");
            assert!(m.iter().any(|x| x.1.why == "roam"));
        }
    }

    #[test]
    fn roams_draw_from_all_four_arrivals() {
        let mut b = calm(17);
        let m = moves(&run(&mut b, 0.0, 6.0 * 3600.0, false, &here, &[]));
        let firsts: Vec<Arrival> = m.iter().filter(|x| x.1.why == "roam").map(|x| x.1.arrivals[0]).collect();
        let n = |a: Arrival| firsts.iter().filter(|&&x| x == a).count() as f64 / firsts.len() as f64;
        // hop 4 : teleport 2 : drop_in 2 : glitch 1 (the view takes the first that fits).
        assert!((n(Arrival::Hop) - 4.0 / 9.0).abs() < 0.08 && (n(Arrival::Glitch) - 1.0 / 9.0).abs() < 0.06, "{:?}", (n(Arrival::Hop), n(Arrival::Teleport), n(Arrival::DropIn), n(Arrival::Glitch)));
        assert!(m.iter().filter(|x| x.1.why == "roam").all(|x| x.1.arrivals.last() == Some(&Arrival::Teleport) || x.1.arrivals.contains(&Arrival::Teleport)));
    }

    #[test]
    fn protected_no_repeat_with_lively_rates() {
        // 8 h of natural visits (attention mix, every 8–15 min): the lead approach never repeats
        // one of the last `no_repeat` (2), and no two visits closer than 8 min.
        for seed in [3u64, 9, 27] {
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 8.0 * 3600.0, false, &here, &[]);
            let firsts: Vec<(f64, Approach)> = moves(&r)
                .into_iter()
                .filter_map(|(t, p)| match p.goal {
                    Goal::Active { why: Why::Attention, approaches } => Some((t, approaches[0])),
                    _ => None,
                })
                .collect();
            // (Ignored visits raise θ: the gaps grow past 15 min when nobody answers.)
            assert!(firsts.len() >= 15, "seed {seed}: {} visits in 8 h", firsts.len());
            for w in firsts.windows(3) {
                assert!(w[2].1 != w[1].1 && w[2].1 != w[0].1, "seed {seed}: approach repeated within 2: {w:?}");
            }
            for w in firsts.windows(2) {
                assert!(w[1].0 - w[0].0 >= 480.0 - 1e-6, "seed {seed}: visits {:.0} s apart", w[1].0 - w[0].0);
            }
        }
    }

    // ---- asleep (Haseeb: "when she sleeps, she still walks"; "if I place her somewhere, she
    // stays there till she isn't sleeping") --------------------------------------------------

    /// He works on while she sleeps: focus changes, workspace switches, the pointer at her,
    /// her perch dragged.
    fn busy_screen(from: f64, to: f64) -> Vec<(f64, BrainEvent)> {
        let mut ev = Vec::new();
        let mut t = from;
        while t < to {
            ev.push(screen(t, Screen::Focus));
            ev.push(screen(t + 5.0, Screen::CursorHover));
            ev.push(screen(t + 10.0, Screen::CursorNear));
            ev.push(screen(t + 15.0, Screen::CursorSwipe { speed: 5000.0 }));
            ev.push(screen(t + 20.0, Screen::Workspace { away: true, stranded: false }));
            ev.push(screen(t + 25.0, Screen::PerchDragged { speed: 5000.0 }));
            t += 60.0;
        }
        ev
    }

    #[test]
    fn asleep_an_hour_without_a_move_of_her_own() {
        let sleepy = |t: f64| Senses { asleep: true, placement: PlaceKind::Ground, ..here(t) };
        let awake = |t: f64| Senses { placement: PlaceKind::Ground, ..here(t) };
        for seed in 0..40 {
            // Awake, the same hour has roams, hops, bids and reactions (the test means something).
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 3600.0, true, &awake, &busy_screen(30.0, 3600.0));
            assert!(moves(&r).len() >= 5 && !reacts(&r).is_empty(), "seed {seed}: awake {:?}", whys(&r));
            // Asleep (from the first tick): nothing at all, ticked every second.
            let mut b = brain(seed);
            let r = run(&mut b, 0.0, 3600.0, true, &sleepy, &busy_screen(30.0, 3600.0));
            assert!(moves(&r).is_empty(), "seed {seed}: moved in her sleep {:?}", whys(&r));
            assert!(reacts(&r).is_empty() && !r.log.iter().any(|(_, i)| matches!(i, Intent::Emote(_))), "seed {seed}: {:?}", r.log);
            // The drives slept too: no visit stored up for the moment she wakes.
            assert!(b.attn_at(3600.0) < 1e-9 && b.roam_at(3600.0) < 1.0, "seed {seed}");
            assert_eq!(b.snapshot(3600.0)["asleep"], true);
        }
    }

    #[test]
    fn asleep_a_due_roam_waits_even_between_polls() {
        // She falls asleep 1 s before her roam is due (polls 15 s apart): it doesn't go.
        for seed in 0..40 {
            let mut b = calm(seed);
            let due = b.t + (1.0 - b.roam) * b.t_roam;
            let s = move |t: f64| Senses { asleep: t >= due - 1.0, placement: PlaceKind::Ground, ..here(t) };
            let r = run(&mut b, 0.0, due + 600.0, false, &s, &[]);
            assert!(moves(&r).iter().all(|m| m.0 < due - 1.0), "seed {seed}: {:?} (due {due:.1})", whys(&r));
        }
    }

    #[test]
    fn asleep_a_closed_perch_still_rescues_or_falls() {
        let perch = |t: f64| Senses { asleep: true, placement: PlaceKind::Perch, ..here(t) };
        let mut tele = 0;
        for seed in 0..400 {
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &perch, &[screen(40.0, Screen::PerchClosed { target: true })]);
            match (whys(&r).as_slice(), reacts(&r).as_slice()) {
                ([(40.0, "perch_closed")], []) => tele += 1,
                ([], [(40.0, React::Fall)]) => {}
                other => panic!("seed {seed}: {other:?}"),
            }
            // Her perch left with the workspace: she follows him (or falls), asleep or not.
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 41.0, false, &perch, &[screen(40.0, Screen::Workspace { away: true, stranded: true })]);
            assert_eq!(whys(&r), vec![(40.0, "workspace")], "seed {seed}");
        }
        assert!((150..=290).contains(&tele), "teleports {tele}/400");
    }

    #[test]
    fn asleep_a_summon_still_goes() {
        // The session's news wakes her (the view stands her up first); the brain doesn't hold it,
        // even on senses a poll old.
        let sleepy = |t: f64| Senses { asleep: true, ..here(t) };
        for seed in 0..40 {
            for sig in [SessionSignal::Done, SessionSignal::Prompt(7), SessionSignal::Error { own_abort: false }] {
                let mut b = calm(seed);
                let r = run(&mut b, 0.0, 700.0, false, &sleepy, &[(650.0, BrainEvent::Session(sig))]);
                assert_eq!(whys(&r), vec![(650.0, "summon")], "seed {seed} {sig:?}");
            }
            // A forced one too (`brain force summon`).
            let mut b = calm(seed);
            let r = run(&mut b, 0.0, 100.0, false, &sleepy, &[(50.0, BrainEvent::Control(Ctl::Force(Force::Summon)))]);
            assert_eq!(whys(&r), vec![(50.0, "summon")], "seed {seed}");
        }
    }

    #[test]
    fn woken_she_stays_a_moment_then_lives_again() {
        // Asleep until 1000, then awake: nothing of her own for the grace, then life.
        for seed in 0..40 {
            let mut b = brain(seed);
            let s = |t: f64| Senses { asleep: t < 1000.0, placement: PlaceKind::Ground, ..here(t) };
            let r = run(&mut b, 0.0, 2600.0, true, &s, &busy_screen(1000.0, 1100.0));
            let m = whys(&r);
            let grace = b.tuning.wake_grace_s;
            assert!(m.iter().all(|x| x.0 >= 1000.0 + grace - 1e-6), "seed {seed}: {m:?}");
            assert!(!m.is_empty(), "seed {seed}: life again after the grace");
        }
        // The decision log says so.
        let mut b = brain(1);
        let s = |t: f64| Senses { asleep: t < 100.0, ..here(t) };
        run(&mut b, 0.0, 130.0, true, &s, &[]);
        assert!(b.log().1.iter().any(|l| l.what == "asleep") && b.log().1.iter().any(|l| l.what == "awake (sleepy over)"));
    }

    #[test]
    fn asleep_where_he_puts_her() {
        // Sleepy: he carries her onto his window and throws her once more. No sulk, no far
        // move, never moved off his window, not even after she wakes.
        for seed in 0..40 {
            let mut b = calm(seed);
            b.last_active_arrival = Some(0.0);
            let s = |t: f64| Senses { asleep: t < 600.0, placement: PlaceKind::Perch, busy: if (300.0..304.0).contains(&t) { vec!["drag"] } else { vec![] }, ..here(t) };
            let ev = [
                (300.0, BrainEvent::Interaction(Touch::DragStart)),
                (303.0, BrainEvent::Flung { speed: 4000.0 }),
                (304.0, BrainEvent::Landed { hard: true, ground: false }),
            ];
            b.covering = true;
            let r = run(&mut b, 0.0, 600.0, false, &s, &ev);
            assert!(moves(&r).is_empty() && b.mode_key() == "passive" && !b.far_next && !b.covering(), "seed {seed}: {:?}", whys(&r));
            let r = run(&mut b, 600.0, 1400.0, false, &s, &[]);
            assert!(whys(&r).iter().all(|x| !matches!(x.1, "covering" | "sulk_far")), "seed {seed}: {:?}", whys(&r));
        }
    }

    #[test]
    fn napping_where_he_puts_her() {
        // Away from 100 s (idle), the nap at ~1005. At 1100 he carries her 4 s and walks off
        // again; at 1500 he's back for real. Carrying her isn't him being back: she naps on,
        // in the nap's pose where she landed, and nothing moves her until 1500.
        for seed in 0..40 {
            let mut b = brain(seed);
            let idle = |t: f64| match t {
                t if t < 100.0 => 0.0,
                t if t < 1100.0 => t - 100.0,
                t if t < 1106.0 => 0.0,
                t if t < 1500.0 => t - 1106.0,
                _ => 0.0,
            };
            let s = move |t: f64| Senses { idle_s: idle(t), placement: PlaceKind::Ground, busy: if (1100.0..1104.0).contains(&t) { vec!["drag"] } else { vec![] }, ..here(t) };
            let ev = [
                (1100.0, BrainEvent::Interaction(Touch::DragStart)),
                (1104.0, BrainEvent::Landed { hard: false, ground: true }),
                (1200.0, BrainEvent::Interaction(Touch::Headpat)),
                (1250.0, BrainEvent::Interaction(Touch::Menu)),
            ];
            let mut ev = ev.to_vec();
            ev.extend(busy_screen(1100.0, 1500.0));
            let r = run(&mut b, 0.0, 1499.0, true, &s, &ev);
            let poses: Vec<_> = r.log.iter().filter_map(|(t, i)| if let Intent::Pose(p) = i { Some((*t, *p)) } else { None }).collect();
            assert_eq!(poses.iter().map(|p| p.1).collect::<Vec<_>>(), vec![Some(RestPose::Nap), Some(RestPose::Nap)], "seed {seed}: {poses:?}");
            assert_eq!(poses[1].0, 1104.0, "the nap's pose again where she landed");
            assert_eq!(b.mode_key(), "napping", "seed {seed}");
            // (Away from the first poll with idle >= 300, t = 405: her life until then.)
            assert!(moves(&r).iter().all(|m| m.0 <= 405.0), "seed {seed}: {:?}", whys(&r));
            assert!(reacts(&r).is_empty(), "seed {seed}");
            // Back for real: up at the next poll, then the grace.
            let r = run(&mut b, 1499.0, 1700.0, true, &s, &[]);
            let up = r.log.iter().find(|(_, i)| *i == Intent::Pose(None)).expect("up").0;
            assert!((1500.0..=1515.0).contains(&up), "seed {seed}: up at {up}");
            assert!(moves(&r).iter().all(|m| m.0 >= up + b.tuning.wake_grace_s - 1e-6), "seed {seed}: {:?}", whys(&r));
        }
        // Talking to her wakes her from the nap at once (a pat doesn't).
        let mut b = brain(3);
        let s = |t: f64| Senses { idle_s: if t < 100.0 { 0.0 } else { t - 100.0 }, placement: PlaceKind::Ground, ..here(t) };
        let ev = [(1100.0, BrainEvent::Interaction(Touch::Headpat)), (1150.0, BrainEvent::Interaction(Touch::Typing))];
        let r = run(&mut b, 0.0, 1151.0, false, &s, &ev);
        let up: Vec<_> = r.log.iter().filter(|(_, i)| *i == Intent::Pose(None)).map(|x| x.0).collect();
        assert_eq!(up, vec![1150.0]);
    }
}
