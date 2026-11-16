//! The brain in the view (BRIEF-brain B2): senses at wake-ups, intents → spots → arrivals,
//! emotes, fullscreen hide, the brain's control commands, persistence and the decision log.
//!
//! A child module of `view`, so it reaches the avatar's state directly. Nothing here ticks on
//! its own: the brain's next wake-up joins `next_deadline()`.

use super::*;
use crate::brain::{
    Approach, Arrival, Arrived, Brain, BrainEvent, Ctl, EmoteKey, Goal, Intent, MovePlan, PlaceKind, RestPose, Saved, Senses,
    SessionSignal, Touch, Tuning, Why,
};
use crate::control::{BrainCmd, Sense};
use super::arriveview::ArriveKind;
use crate::moves;
use crate::places::{self, Boxes, Spot, SpotKind, World};
use crate::settings::{Attention, Settings};
use std::io::Write as _;

/// Knees of a hanging frame, as a fraction of its visible height from the top.
const KNEE_AT: f32 = 0.73;
/// He is near her: her body within this of the focused window (logical px), same monitor.
const NEAR_PX: f32 = 250.0;
/// How often the drives are written to state.json at most (wall clock).
const SAVE_EVERY: Duration = Duration::from_secs(10);
/// The bid marker (a "!" badge by her head): effect layers after the glints.
const MARK_LAYER: usize = GLINT_LAYER + 2 * GLINTS_MAX;
const MARK_RIM: u32 = 0x0b0e14;
/// r7 critics: the white badge was lost in her hair. Warning yellow, a black "!".
const MARK_FILL: u32 = 0xffd83a;
const MARK_GLYPH: u32 = 0x0b0e14;
/// Badge radius, logical px.
const MARK_R: f32 = 19.0;
/// How long a summon's NEEDS YOU pulses after she lands, and its half period.
const PULSE_FOR: Duration = Duration::from_millis(6_000);
const PULSE_HALF_MS: u128 = 450;
/// The settled summon's border breath.
const GLOW_S: f32 = 1.8;

/// The bid marker: up from the first bid emote to the end of the bid.
#[derive(Clone, Copy)]
pub(super) struct Marker {
    t0: Instant,
    /// The last beat (a pop).
    pop: Instant,
    /// Insist level 2: "!!".
    double: bool,
    /// Her work area when it went up: the badge stays inside it.
    area: Option<WinRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FsHide {
    Shown,
    Hiding,
    Hidden,
}

#[derive(Default)]
pub(super) struct Overrides {
    /// `Some(None)`: nothing focused.
    focus: Option<Option<u32>>,
    cursor: Option<(i32, i32)>,
    /// (brain time it was set, idle seconds then)
    idle: Option<(f64, f64)>,
    fullscreen: Option<bool>,
    near: Option<bool>,
    arena: Option<WinRect>,
    /// Top-level ids she may use.
    windows: Option<Vec<u32>>,
}

/// The bid swing: a ragdoll chain from her grip, pumped at each zero crossing toward a
/// target amplitude (a kid on a swing), stepped at 30 Hz. Bands from it replace the shear;
/// they're computed once per tick (held: no 1 px flicker), with the next tick's for the
/// commit-ahead underlay.
pub(super) struct BidSwing {
    sw: crate::ragdoll::PumpedSwing,
    frame: usize,
    grip: i32,
    last: Instant,
    pub due: Instant,
    bands: (Bands, Bands),
}

type Bands = Vec<(i32, i32, i32, i32)>;

/// An insist beat's leg kick: out and back once over this long (LOCKED #9: >= 400 ms).
const LEG_KICK_S: f32 = 0.8;

/// Feet amplitude she pumps the swing to (logical px), and the tick.
const SWING_AMP: f32 = 30.0;
const SWING_TICK: Duration = Duration::from_millis(33);

/// Float to float: legs of at most `DRIFT_LEG` px, a pause between them, so a drift is
/// bursts of 1.5–4 s and never more than ~15 s of a minute.
pub(super) struct DriftMove {
    legs: Vec<moves::Drift>,
    i: usize,
    t0: Instant,
    pause_until: Option<Instant>,
}
const DRIFT_LEG: f32 = 220.0;
const DRIFT_PAUSE: Duration = Duration::from_millis(1500);
const DRIFT_TICK: Duration = Duration::from_millis(33);
/// The float bob: 5 px over 3.2 s in 1 px steps, 20 s on, 30 s still.
const FLOAT_BOB_ON: f32 = 20.0;
const FLOAT_BOB_CYCLE: f32 = 50.0;
/// drop_in falls from this far above the spot (logical px): standing, posed.
const DROP_IN_H: (f32, f32) = (240.0, 70.0);
/// The pointer this close to her side (logical px), level with her face or chest: the look is
/// straight ahead (`brain.lookFront`), not to the side.
const FRONT_PX: f32 = 60.0;
/// A leap this far up or across (logical px) takes the deeper crouch (`brain.bigHopCrouch`).
const BIG_LEAP_UP: f32 = 150.0;
const BIG_LEAP_DX: f32 = 220.0;

pub(super) struct InFlight {
    approach: Option<Approach>,
    arrival: Arrival,
    spot: Spot,
    active: bool,
    /// A summon for a pending prompt: she lands loud (a hop, a point at his window).
    summon_prompt: bool,
    /// Which way she faces once there (`[mirror] self`): his pointer on a bid or summon;
    /// else into the screen near a monitor's edge, a window's middle from its edge.
    face: Option<bool>,
    /// The plan's rule.
    why: &'static str,
}

/// The world as read for one decision (owned; `Avatar::world` lends it out as a `World`).
pub(super) struct WorldBuf {
    stack: Vec<WinRect>,
    eligible: Vec<bool>,
    areas: Vec<WinRect>,
    monitors: Vec<WinRect>,
    fullscreen: Vec<bool>,
    focused: Option<WinRect>,
    cursor: (i32, i32),
    her: Spot,
    senses_fs: bool,
    hud_h: i32,
    only: Option<usize>,
    prefer: Option<usize>,
}

/// A small hop under way: her feet along `leap` (device px), then she settles into `place` at
/// `origin`.
pub(super) struct LeapMove {
    leap: moves::Leap,
    t0: Instant,
    origin: (i32, i32),
    place: Placement,
    crouched: bool,
    /// A startled hop: the surprised face in the air.
    startled: bool,
}

pub(super) struct Scoot {
    hops: Vec<moves::Hop>,
    i: usize,
    t0: Instant,
    feet_y: i32,
    crouched: bool,
}

pub(super) struct BrainHost {
    pub brain: Brain,
    pub settings: Settings,
    base_t: Instant,
    base_b: f64,
    pub scale: f64,
    pub wake: Option<Instant>,
    ov: Overrides,
    flight: Option<InFlight>,
    scoot: Option<Scoot>,
    /// A move decided while another was still landing: done right after it.
    next_plan: Option<MovePlan>,
    pub rest: Option<RestPose>,
    last_wins: Vec<u32>,
    last_kind: Option<SpotKind>,
    spot: Option<Spot>,
    /// Emotes held while she moves, and ones waiting for a reaction to end.
    queued: Vec<EmoteKey>,
    later: Vec<(Instant, EmoteKey)>,
    log: Option<std::fs::File>,
    log_seen: u64,
    /// Brain time of the next idle look-around (`[mirror] idle_every_min`), its own rng.
    look_b: Option<f64>,
    look_rng: Rng,
    rng: Rng,
    saved_at: Option<Instant>,
    save_due: bool,
    pub fs: FsHide,
    /// The fullscreen window she hid for (the focused client when the hide began).
    pub fs_cover: Option<u32>,
    boxes: Boxes,
    pub ticks: u64,
    last_abort: Option<Instant>,
    last_senses: Option<serde_json::Value>,
    typed_at: Option<Instant>,
    /// A bid's swing kick while hanging: (start, px at the bottom).
    kick: Option<(Instant, f32)>,
    /// An insist beat's leg kick while hanging (start, direction).
    leg_kick: Option<(Instant, f32)>,
    /// Hanging or clinging: the ragdoll chain she swings on (a gentle pumped pendulum).
    pub swing: Option<BidSwing>,
    /// Float to float: an eased drift in short bursts (moves only, no redraw).
    drift: Option<DriftMove>,
    /// The drop_in arrival: falling the last stretch onto the spot (start, height px, posed).
    drop_in: Option<(Instant, f32, bool)>,
    marker: Option<Marker>,
    /// What the marker layers show now: (origin, size, scale step), to skip no-op updates.
    marker_shown: Option<(i32, i32, i32, bool)>,
    /// A summon landed: NEEDS YOU pulses from here.
    pulse: Option<Instant>,
    pulse_last: bool,
    /// A summon landed: once the blink is over, the bubble's border glows warm while the
    /// prompt waits (from this time), and the last glow level drawn.
    glow: Option<Instant>,
    glow_last: Option<u8>,
    /// Sitting low after a sulk (the floor sit uses the sulk loop).
    sulk_sit: bool,
    // ---- lively (LOCKED #17) ----
    /// A small hop in the air.
    leap: Option<LeapMove>,
    /// The focused top-level last seen (a focus change is a different one).
    focus_seen: Option<u32>,
    /// He switched workspace at: and the check of where she is, a beat later (after a perch
    /// that went with it has had its grace).
    desk_at: Option<Instant>,
    desk_check: Option<Instant>,
    /// The windows she may sit on, by monitor (`places::desk_windows`), at the last look, and
    /// as they were before the switch under way: did it change her monitor?
    desk_seen: Option<Vec<(u32, usize)>>,
    desk_before: Option<Vec<(u32, usize)>>,
    /// Her perch window's last position and when (a fast drag startles her), and the drag
    /// already reported.
    perch_seen: Option<(u32, Instant, i32, i32)>,
    drag_told: Option<Instant>,
    /// The body box last given to the pointer watcher, and its arming.
    watch_box: Option<(Option<WinRect>, bool)>,
}

fn env_rect(k: &str) -> Option<WinRect> {
    let v: Vec<i32> = std::env::var(k).ok()?.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 4).then(|| WinRect::new(v[0], v[1], v[2], v[3]))
}

fn rect_json(r: &WinRect) -> serde_json::Value {
    serde_json::json!([r.x, r.y, r.w, r.h])
}

impl BrainHost {
    pub fn queued_mut(&mut self) -> &mut Vec<EmoteKey> {
        &mut self.queued
    }

    /// A move's own motion is running (a scoot, a drift, a drop): the landing waits for it.
    fn moving(&self) -> bool {
        self.scoot.is_some() || self.drift.is_some() || self.drop_in.is_some() || self.leap.is_some()
    }

    pub(super) fn bnow(&self, now: Instant) -> f64 {
        self.base_b + now.saturating_duration_since(self.base_t).as_secs_f64() * self.scale
    }

    fn instant(&self, b: f64) -> Instant {
        let d = ((b - self.base_b) / self.scale).clamp(0.0, 1.0e7);
        self.base_t + Duration::from_secs_f64(d)
    }

    pub(super) fn write(&mut self, v: serde_json::Value) {
        if let Some(f) = &mut self.log {
            writeln!(f, "{v}").ok();
        }
    }

    /// Mirror the brain's new decision lines into the log.
    fn sync_log(&mut self) {
        let (n, lines) = self.brain.log();
        let new = (n - self.log_seen).min(lines.len() as u64) as usize;
        let start = self.brain.start();
        let out: Vec<serde_json::Value> = lines
            .iter()
            .skip(lines.len() - new)
            .map(|l| serde_json::json!({ "t": ((l.t - start) * 1000.0).round() / 1000.0, "k": "brain", "what": l.what }))
            .collect();
        self.log_seen = n;
        for v in out {
            self.write(v);
        }
    }
}

impl Avatar {
    /// The pack's frame for an approach pose (`hang`, `cling_left`, …).
    fn brain_pose(&self, key: &str) -> Option<usize> {
        self.pack.behaviour.brain.poses.get(key).and_then(|id| self.pack.index.get(id)).copied()
    }

    /// Window-local device px of frame `i`'s anchor point.
    pub(super) fn anchor_px(&self, i: usize) -> (i32, i32) {
        let l = &self.layout;
        let f = &self.pack.frames[i];
        let (dx, dy) = self.pack.offset(f);
        let (x, y) = l.geo.to_device(f.anchor.x + dx, f.anchor.y + dy);
        (l.avatar_x + x.round() as i32, l.avatar_y + y.round() as i32)
    }

    /// Frame `i`'s visible box, relative to its anchor point (device px).
    fn box_rel(&self, i: usize) -> WinRect {
        let l = &self.layout;
        let f = &self.pack.frames[i];
        let (dx, dy) = self.pack.offset(f);
        let (x0, y0) = l.geo.to_device(f.bounds.x + dx, f.bounds.y + dy);
        let (x1, y1) = l.geo.to_device(f.bounds.x + f.bounds.width + dx, f.bounds.y + f.bounds.height + dy);
        let (ax, ay) = self.anchor_px(i);
        WinRect::new(l.avatar_x + x0.round() as i32 - ax, l.avatar_y + y0.round() as i32 - ay, (x1 - x0).round() as i32, (y1 - y0).round() as i32)
    }

    fn brain_boxes(&self) -> Boxes {
        let default = self.pack.frame(&self.pack.catalog.default_sprite).unwrap_or(0);
        let stand = self.box_rel(default);
        let sit = self.perch_spec().and_then(|p| self.pack.index.get(&p.base)).map_or(stand, |&i| {
            // Seat-registered: the anchor is the seat line at the ground x.
            let b = self.box_rel(i);
            let (ax, ay) = self.anchor_px(i);
            let seat = (self.layout.ground_x, self.layout.seat_y.unwrap_or(ay));
            WinRect::new(b.x + ax - seat.0, b.y + ay - seat.1, b.w, b.h)
        });
        let floor_sit = self.pack.behaviour.brain.floor_sit.as_ref().and_then(|f| self.pack.index.get(&f.base)).map_or(stand, |&i| self.box_rel(i));
        let pose = |k: &str| self.brain_pose(k).map(|i| self.box_rel(i));
        Boxes {
            stand,
            sit,
            floor_sit,
            hang: pose("hang"),
            hang_ud: pose("hang_upside_down"),
            cling_l: pose("cling_left"),
            cling_r: pose("cling_right"),
            peek: pose("peek"),
        }
    }

    /// Start the brain (settings, saved drives, test senses from the environment).
    pub(super) fn brain_start(&mut self) {
        let settings = match Settings::load() {
            Ok(s) => s,
            Err(e) => {
                elog!("synaps-avatar: {e:#} (using the defaults)");
                Settings::default()
            }
        };
        let mut bs = settings.brain.clone();
        // What the pack can't do gets weight 0.
        for (a, w) in bs.approaches.iter_mut() {
            let ok = match a {
                Approach::Hang => self.brain_pose("hang").is_some(),
                Approach::HangUpsideDown => self.brain_pose("hang_upside_down").is_some(),
                Approach::Cling => self.brain_pose("cling_left").is_some() && self.brain_pose("cling_right").is_some(),
                Approach::Peek => self.brain_pose("peek").is_some(),
                Approach::SitEdge => self.layout.seat_y.is_some(),
                Approach::Beside => true,
            };
            if !ok {
                *w = 0.0;
            }
        }
        let scale = std::env::var("SA_BRAIN_SCALE").ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(1.0);
        let now = Instant::now();
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let saved = state::load_brain().and_then(|v| Saved::from_json(&v));
        let seed = std::env::var("SA_BRAIN_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or_else(|| self.rng.next());
        let mut brain = Brain::new(seed, bs.clone(), Tuning::new(&bs), saved.as_ref().map(|s| (s, (unix - s.saved_unix).max(0.0) * scale)), 0.0);
        brain.sulk_leaves = std::env::var_os("SA_SULK_INPLACE").is_none();
        let log = std::env::var_os("SA_BRAIN_LOG").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
        let mut ov = Overrides { arena: env_rect("SA_BRAIN_ARENA"), ..Overrides::default() };
        if let Ok(w) = std::env::var("SA_BRAIN_WINDOWS") {
            ov.windows = Some(w.split([',', ' ']).filter_map(|x| u32::from_str_radix(x.trim().trim_start_matches("0x"), 16).ok()).collect());
        }
        if std::env::var_os("SA_BRAIN_SYNTH").is_some() {
            // Tests: never read his real focus, pointer, input or fullscreen.
            ov.focus = Some(None);
            ov.cursor = Some(ov.arena.map_or((0, 0), |a| (a.right() - 10, a.y + 10)));
            ov.idle = Some((0.0, 0.0));
            ov.fullscreen = Some(false);
        }
        let boxes = self.brain_boxes();
        let rng = Rng::new(seed ^ 0x9e37_79b9);
        self.brain = Some(BrainHost {
            brain,
            settings,
            base_t: now,
            base_b: 0.0,
            scale,
            wake: Some(now),
            ov,
            flight: None,
            scoot: None,
            next_plan: None,
            rest: None,
            last_wins: Vec::new(),
            last_kind: None,
            spot: None,
            queued: Vec::new(),
            later: Vec::new(),
            log,
            log_seen: 0,
            look_b: None,
            look_rng: Rng::new(seed ^ 0x51ab_1007),
            rng,
            saved_at: None,
            save_due: false,
            fs: FsHide::Shown,
            fs_cover: None,
            boxes,
            ticks: 0,
            last_abort: None,
            last_senses: None,
            typed_at: None,
            kick: None,
            swing: None,
            drift: None,
            drop_in: None,
            leg_kick: None,
            marker: None,
            marker_shown: None,
            pulse: None,
            pulse_last: false,
            glow: None,
            glow_last: None,
            sulk_sit: false,
            leap: None,
            focus_seen: None,
            desk_at: None,
            desk_check: None,
            desk_seen: None,
            desk_before: None,
            perch_seen: None,
            drag_told: None,
            watch_box: None,
        });
        if let Some(h) = &mut self.brain {
            let v = serde_json::json!({ "t": 0.0, "k": "start", "scale": scale, "seed": seed, "settings": h.brain.snapshot(0.0)["settings"] });
            h.write(v);
        }
    }

    // ---- reading the world ----------------------------------------------------------------

    fn placement_kind(&self) -> PlaceKind {
        match self.placement {
            Placement::Ground => PlaceKind::Ground,
            Placement::Perch { .. } => PlaceKind::Perch,
            Placement::Float => PlaceKind::Float,
            Placement::Pose { .. } => PlaceKind::Hang,
        }
    }

    pub(super) fn posed(&self) -> bool {
        matches!(self.placement, Placement::Pose { .. })
    }

    /// She shows standing frames (emotes, activities and clicks may play).
    pub(super) fn standing(&self) -> bool {
        matches!(self.placement, Placement::Ground | Placement::Float) && self.brain.as_ref().is_none_or(|h| h.rest.is_none())
    }

    /// The focused top-level (synthetic or real), never hers, never outside the arena.
    fn brain_focused(&self) -> Option<WinRect> {
        let h = self.brain.as_ref()?;
        let id = match h.ov.focus {
            Some(f) => f?,
            None => self.os.active_window()?,
        };
        let top = self.os.toplevel_of(id)?;
        if top == self.os.body() {
            return None;
        }
        if let Some(w) = &h.ov.windows
            && !w.iter().any(|&x| x == top || x == id)
        {
            return None;
        }
        self.os.toplevel_rect(top)
    }

    /// A brain move is under way (its landing decides which way she faces).
    pub(super) fn brain_in_flight(&self) -> bool {
        self.brain.as_ref().is_some_and(|h| h.flight.is_some())
    }

    pub(super) fn brain_cursor(&self) -> (i32, i32) {
        match self.brain.as_ref().and_then(|h| h.ov.cursor) {
            Some(c) => c,
            None => self.os.pointer().unwrap_or((0, 0)),
        }
    }

    /// Her current spot (kind, anchor, body).
    fn her_spot(&self, areas: &[WinRect], monitors: &[WinRect]) -> Spot {
        let l = &self.layout;
        let frame = self.anim.base();
        let (anchor, kind) = match self.placement {
            Placement::Perch { win, .. } => ((self.origin.0 + l.ground_x, self.origin.1 + l.seat_y.unwrap_or(l.ground_y)), SpotKind::Edge { win }),
            Placement::Pose { frame } => {
                let a = self.anchor_px(frame);
                ((self.origin.0 + a.0, self.origin.1 + a.1), SpotKind::Peek)
            }
            Placement::Float => (self.ground(), SpotKind::Float),
            Placement::Ground => (self.ground(), SpotKind::Floor),
        };
        let rel = match self.placement {
            Placement::Pose { frame } => self.box_rel(frame),
            Placement::Perch { .. } => self.brain.as_ref().map_or(self.box_rel(frame), |h| h.boxes.sit),
            _ => self.brain.as_ref().map_or(self.box_rel(frame), |h| h.boxes.stand),
        };
        let body = WinRect::new(anchor.0 + rel.x, anchor.1 + rel.y, rel.w, rel.h);
        let area = areas
            .iter()
            .position(|a| a.contains(anchor.0, anchor.1 - 1))
            .or_else(|| monitors.iter().position(|m| m.contains(anchor.0, anchor.1 - 1)))
            .unwrap_or(0);
        Spot { kind, anchor, body, area, approach: None, cover_pct: 0.0 }
    }

    fn brain_busy(&self) -> Vec<&'static str> {
        let now = Instant::now();
        let mut v = Vec::new();
        if self.bubble.as_ref().is_some_and(|b| !b.collapsed) {
            v.push("bubble");
        }
        if self.input.is_some() {
            v.push("field");
        }
        if self.line.is_some() {
            v.push("line");
        }
        if self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed) {
            v.push("prompt");
        }
        if self.menu.is_some() {
            v.push("menu");
        }
        if self.reaction_until.is_some_and(|t| t > now) {
            v.push("reaction");
        }
        if (self.teleport.is_some() && !self.arrive.launching) || self.brain.as_ref().is_some_and(|h| h.flight.is_some() || h.moving()) {
            v.push("teleport");
        }
        if self.music.is_some() {
            v.push("music");
        }
        if self.parked.is_some() {
            v.push("hidden");
        }
        if self.press.is_some() || self.rag.is_some() {
            v.push("drag");
        }
        if self.session.as_ref().and_then(|s| s.shown.as_ref()).is_some_and(|b| b.busy) {
            v.push("session");
        }
        v
    }

    /// Work areas and monitors, cut to the test arena when there is one (its bottom is her
    /// floor there, its sides are the walls).
    pub(super) fn arena_filter(&self, areas: Vec<WinRect>, monitors: Vec<WinRect>) -> (Vec<WinRect>, Vec<WinRect>) {
        match self.brain.as_ref().and_then(|h| h.ov.arena) {
            Some(a) => areas.into_iter().zip(monitors).filter_map(|(w, m)| Some((w.intersect(&a)?, m.intersect(&a)?))).unzip(),
            None => (areas, monitors),
        }
    }

    /// Work areas and monitors as she flies in them (cut to the test arena). Asked once per
    /// release, never per frame.
    pub(super) fn rag_areas(&self) -> (Vec<WinRect>, Vec<WinRect>) {
        self.arena_filter(
            self.os.work_areas().iter().map(crate::platform::Monitor::rect).collect(),
            self.os.monitors().iter().map(crate::platform::Monitor::rect).collect(),
        )
    }

    /// The windows she may use (a test's `brain sense windows`), None = any.
    pub(super) fn brain_allowed_windows(&self) -> Option<Vec<u32>> {
        self.brain.as_ref().and_then(|h| h.ov.windows.clone())
    }

    /// Picked up mid-move: the move is dropped (she is where the hand is).
    pub(super) fn brain_cancel_move(&mut self) {
        if let Some(h) = &mut self.brain {
            h.flight = None;
            h.scoot = None;
            h.leap = None;
            h.next_plan = None;
        }
    }

    /// Her work area (cut to the test arena), for clamping effects.
    /// Her body box now (device px; the pose's mask box, as `places` sees her): what a panel
    /// opens beside (`beside::place`).
    pub(super) fn her_body(&self) -> WinRect {
        self.her_spot(&[], &[]).body
    }

    pub(super) fn her_area(&self) -> Option<WinRect> {
        let (areas, monitors) = self.arena_filter(
            self.os.work_areas().iter().map(crate::platform::Monitor::rect).collect(),
            self.os.monitors().iter().map(crate::platform::Monitor::rect).collect(),
        );
        let her = self.her_spot(&areas, &monitors);
        areas.get(her.area).copied()
    }

    fn brain_senses(&self, now: Instant) -> Senses {
        let Some(h) = &self.brain else { return Senses::default() };
        let b = h.bnow(now);
        let idle_s = match h.ov.idle {
            // Negative: he is busy at the keyboard (idle stays 0).
            Some((_, v)) if v < 0.0 => 0.0,
            Some((t0, v)) => v + (b - t0).max(0.0),
            None => self.os.idle_ms().map_or(0.0, |ms| ms as f64 / 1000.0 * h.scale),
        };
        let focused = self.brain_focused();
        let (areas, monitors) = self.arena_filter(
            self.os.work_areas().iter().map(crate::platform::Monitor::rect).collect(),
            self.os.monitors().iter().map(crate::platform::Monitor::rect).collect(),
        );
        let her = self.her_spot(&areas, &monitors);
        let her_mon = monitors.get(her.area).copied();
        let (fullscreen, fullscreen_here) = match h.ov.fullscreen {
            Some(f) => (f, f),
            None => {
                let fs = focused.is_some_and(|f| {
                    let client = h.ov.focus.flatten().or_else(|| self.os.active_window()).unwrap_or(f.id);
                    self.os.is_fullscreen(client) || monitors.iter().any(|m| m.x == f.x && m.y == f.y && m.w == f.w && m.h == f.h)
                });
                let here = fs && focused.zip(her_mon).is_some_and(|(f, m)| m.intersect(&f).is_some());
                (fs, here)
            }
        };
        let near = h.ov.near.unwrap_or_else(|| focused.zip(her_mon).is_some_and(|(f, m)| {
            let reach = (NEAR_PX * self.layout.sf) as i32;
            m.intersect(&f).is_some() && f.grow(reach).intersect(&her.body).is_some()
        }));
        // Typing fast: input within the last 1.5 s with the pointer still for 3 s (the watcher
        // sees the pointer; keys are never read). A test's negative idle means "typing".
        let typing = match h.ov.idle {
            Some((_, v)) => v < 0.0,
            None => idle_s < 1.5 * h.scale && self.os.pointer_still_ms().is_some_and(|ms| ms >= 3000),
        };
        let asleep = self.mood == Mood::Sleepy;
        Senses { now: b, idle_s, fullscreen, fullscreen_here, busy: self.brain_busy(), near, placement: self.placement_kind(), typing, asleep }
    }

    // ---- the loop -----------------------------------------------------------------------------

    /// Run the brain if it is due (or `force`: after an event), and apply what it decided.
    pub(super) fn brain_run(&mut self, now: Instant, force: bool) {
        let Some(h) = &self.brain else { return };
        if !force && h.wake.is_none_or(|w| w > now) {
            return;
        }
        let senses = self.brain_senses(now);
        let Some(h) = &mut self.brain else { return };
        h.ticks += 1;
        let d = h.brain.tick(&senses);
        h.wake = Some(h.instant(d.wake_at));
        let sv = serde_json::json!({ "idle": (senses.idle_s / 30.0).floor() * 30.0, "fs": senses.fullscreen, "fs_here": senses.fullscreen_here,
                                     "near": senses.near, "busy": senses.busy, "typing": senses.typing });
        if h.last_senses.as_ref() != Some(&sv) {
            let t = ((senses.now - h.brain.start()) * 1000.0).round() / 1000.0;
            h.write(serde_json::json!({ "t": t, "k": "senses", "s": sv }));
            h.last_senses = Some(sv);
        }
        h.sync_log();
        self.brain_apply(d.intents, now);
        // Woken (or fallen asleep) by this decision: the nap's frames follow at once.
        self.step_nap(now);
    }

    pub(super) fn brain_feed(&mut self, ev: BrainEvent, now: Instant) {
        let Some(h) = &mut self.brain else { return };
        let b = h.bnow(now);
        h.brain.on_event(ev, b);
        self.brain_run(now, true);
    }

    fn brain_apply(&mut self, intents: Vec<(f64, Intent)>, now: Instant) {
        for (_, i) in intents {
            match i {
                Intent::Move(p) => self.brain_move(p, now),
                Intent::Emote(e) => self.brain_emote(e, now),
                Intent::Pose(p) => {
                    // The nap's pose ends (he is back, he talked to her): she stands up first,
                    // before any move decided with it.
                    let up = p.is_none() && self.brain.as_ref().is_some_and(|h| h.rest == Some(RestPose::Nap));
                    if let Some(h) = &mut self.brain {
                        h.rest = p;
                    }
                    if up {
                        self.arrive.deep = false;
                    }
                    self.relayer(now);
                    if up && self.standing() {
                        self.play_nap_wake(now);
                    }
                }
                Intent::Hide(hide) => self.brain_hide(hide, now),
                Intent::React(r) => self.brain_react(r, now),
                Intent::Unfold => {
                    if let Some(b) = &mut self.bubble {
                        b.collapsed = false;
                        if b.next_char.is_none() && !b.more {
                            b.hide_at = Some(now + Duration::from_secs(10));
                        }
                    }
                }
                Intent::Save => {
                    if let Some(h) = &mut self.brain {
                        h.save_due = true;
                    }
                }
            }
        }
        self.brain_save(now, false);
    }

    pub(super) fn brain_save(&mut self, now: Instant, force: bool) {
        let Some(h) = &mut self.brain else { return };
        if !h.save_due || (!force && h.saved_at.is_some_and(|t| now.duration_since(t) < SAVE_EVERY)) {
            return;
        }
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let s = h.brain.saved(h.bnow(now), unix);
        state::save_brain(s.to_json());
        h.save_due = false;
        h.saved_at = Some(now);
    }

    /// The brain's part of the next wake-up.
    pub(super) fn brain_deadline(&self, now: Instant) -> Option<Instant> {
        // `tick` runs the brain only once the frames are in (add_frames schedules then): a
        // past `wake` before that spun the loop through the whole load.
        if !self.loaded {
            return None;
        }
        let h = self.brain.as_ref()?;
        let mut t = h.wake;
        let mut take = |d: Option<Instant>| {
            if let Some(d) = d {
                t = Some(t.map_or(d, |t| t.min(d)));
            }
        };
        if h.scoot.is_some() || h.leap.is_some() || h.drop_in.is_some() || (h.flight.is_some() && self.teleport.is_none() && h.drift.is_none()) {
            take(Some(now + FRAME_FAST));
        }
        take(h.desk_check);
        if let Some(d) = &h.drift {
            take(Some(d.pause_until.unwrap_or(now + DRIFT_TICK)));
        }
        take(self.float_bob_next(now));
        take(h.later.iter().map(|l| l.0).min());
        take(h.look_b.map(|b| h.instant(b)));
        if h.next_plan.is_some() {
            take(Some(now + Duration::from_millis(100)));
        }
        if h.fs == FsHide::Hiding {
            take(Some(now + FRAME_FAST));
        }
        if h.save_due {
            take(h.saved_at.map(|s| s + SAVE_EVERY));
        }
        take(self.marker_next_change(now));
        take(self.glow_next_change(now));
        take(self.nap_deadline());
        if let Some(p) = h.pulse {
            let d = now.saturating_duration_since(p);
            if d < PULSE_FOR + Duration::from_millis(PULSE_HALF_MS as u64) {
                let next = (d.as_millis() / PULSE_HALF_MS + 1) * PULSE_HALF_MS;
                take(Some(p + Duration::from_millis(next as u64)));
            }
        }
        t
    }

    /// Per tick: the scoot, landings, held emotes, the fullscreen hide, then the brain if due.
    /// True if the view must redraw.
    pub(super) fn brain_tick(&mut self, now: Instant) -> bool {
        let pulse = self.summon_pulse_on(now);
        let glow = self.summon_glow(now);
        let waiting = self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed);
        let Some(h) = &mut self.brain else { return false };
        let mut dirty = false;
        if pulse != h.pulse_last {
            h.pulse_last = pulse;
            dirty = true;
        }
        if glow != h.glow_last {
            h.glow_last = glow;
            dirty = true;
        }
        if !waiting && h.glow.is_some_and(|g| now.saturating_duration_since(g) > Duration::from_secs(1)) {
            h.glow = None;
        }
        if h.pulse.is_some_and(|p| now.saturating_duration_since(p) > PULSE_FOR + Duration::from_secs(1)) {
            h.pulse = None;
        }
        let later: Vec<EmoteKey> = {
            let (due, keep): (Vec<_>, Vec<_>) = h.later.drain(..).partition(|l| l.0 <= now);
            h.later = keep;
            due.into_iter().map(|l| l.1).collect()
        };
        if h.fs == FsHide::Hiding && self.glitch.is_none() {
            h.fs = FsHide::Hidden;
            self.hidden = true;
            self.fx_hide_all();
            dirty = true;
        }
        dirty |= self.step_scoot(now);
        dirty |= self.step_leap(now);
        dirty |= self.step_drift(now);
        if self.brain.as_ref().and_then(|h| h.desk_check).is_some_and(|d| d <= now) {
            self.desk_settled(now);
        }
        dirty |= self.step_drop_in(now);
        let landed = self.brain.as_ref().is_some_and(|h| h.flight.is_some() && !h.moving()) && self.teleport.is_none() && self.rag.is_none();
        if landed {
            self.brain_landed(now);
            dirty = true;
        }
        for e in later {
            self.play_emote(e, now);
            dirty = true;
        }
        let free = self.teleport.is_none() && self.rag.is_none() && !self.anim.busy() && !self.waking(now) && self.brain.as_ref().is_some_and(|h| h.flight.is_none() && !h.moving());
        if free && let Some(p) = self.brain.as_mut().and_then(|h| h.next_plan.take()) {
            self.brain_move(p, now);
            dirty = true;
        }
        // The desk at launch: the first switch is told from it.
        if self.loaded && self.brain.as_ref().is_some_and(|h| h.desk_seen.is_none() && h.desk_check.is_none()) {
            let d = self.desk_windows();
            if let Some(h) = &mut self.brain {
                h.desk_seen = Some(d);
            }
        }
        // The focus at launch: the first change after it is a focus change.
        if self.loaded && self.brain.as_ref().is_some_and(|h| h.focus_seen.is_none()) {
            let f = self.brain_focused().map(|r| r.id);
            if let Some(h) = &mut self.brain {
                h.focus_seen = f;
            }
        }
        self.brain_run(now, false);
        dirty |= self.step_nap(now);
        dirty |= self.idle_look(now);
        self.brain_save(now, false);
        dirty
    }

    // ---- moves --------------------------------------------------------------------------------

    /// The world for a decision: the stack (her own layers out, the arena and the test's
    /// window list applied), work areas, his focus, the pointer, where she is.
    fn world_buf(&self, goal: &Goal) -> Option<WorldBuf> {
        let h = self.brain.as_ref()?;
        let (mut stack, mut eligible): (Vec<WinRect>, Vec<bool>) = self.os.stack().into_iter().unzip();
        let (areas, monitors) = self.arena_filter(
            self.os.work_areas().iter().map(crate::platform::Monitor::rect).collect(),
            self.os.monitors().iter().map(crate::platform::Monitor::rect).collect(),
        );
        if areas.is_empty() {
            return None;
        }
        if let Some(a) = h.ov.arena {
            for (r, e) in stack.iter().zip(eligible.iter_mut()) {
                *e &= a.intersect(r).is_some_and(|o| o == WinRect::new(r.x, r.y, r.w, r.h));
            }
        }
        if let Some(w) = &h.ov.windows {
            for (r, e) in stack.iter().zip(eligible.iter_mut()) {
                *e &= w.contains(&r.id);
            }
        }
        // Not her own layers (the stage backdrop isn't eligible: the arena's windows list).
        let keep: Vec<bool> = stack.iter().map(|r| r.id != self.os.body()).collect();
        let mut k = keep.iter();
        stack.retain(|_| *k.next().unwrap_or(&true));
        let mut k = keep.iter();
        eligible.retain(|_| *k.next().unwrap_or(&true));
        let focused = self.brain_focused();
        let cursor = self.brain_cursor();
        let her = self.her_spot(&areas, &monitors);
        let senses_fs = match h.ov.fullscreen {
            Some(f) => f,
            None => focused.is_some_and(|f| self.os.is_fullscreen(h.ov.focus.flatten().unwrap_or(f.id))),
        };
        let fullscreen: Vec<bool> = monitors.iter().map(|m| senses_fs && (focused.is_none_or(|f| m.intersect(&f).is_some()))).collect();
        let l = self.layout;
        let hud_h = ((TOP_MARGIN + PANEL_MAX_H + TAIL_H + TIP_GAP) * l.sf) as i32;
        // monitor = "follow": roams, perches and teleports prefer the cursor's monitor
        // (places::CURSOR_MONITOR_P); leaving his window stays on his monitor; small hops are
        // local; visits go to his window anyway.
        let follow = h.settings.brain.monitor == crate::settings::MonitorPolicy::Follow;
        let cursor_area = areas.iter().position(|a| a.contains(cursor.0, cursor.1)).or_else(|| monitors.iter().position(|m| m.contains(cursor.0, cursor.1)));
        let only = if follow && *goal == Goal::Leave { places::follow_area(&monitors, focused, cursor) } else { None };
        let prefer = if follow && matches!(goal, Goal::Passive { .. } | Goal::Explore | Goal::Rescue) { cursor_area } else { None };
        Some(WorldBuf { stack, eligible, areas, monitors, fullscreen, focused, cursor, her, senses_fs, hud_h, only, prefer })
    }

    fn world<'a>(&'a self, b: &'a WorldBuf) -> Option<World<'a>> {
        let h = self.brain.as_ref()?;
        let l = self.layout;
        Some(World {
            stack: &b.stack,
            eligible: &b.eligible,
            focused: b.focused,
            areas: &b.areas,
            monitors: &b.monitors,
            fullscreen: &b.fullscreen,
            cursor: b.cursor,
            her: b.her,
            boxes: h.boxes,
            contact: self.contact_or_default(true),
            margin: self.edge_margin(),
            floor_pad: l.foot_pad + GROUND_ROOM,
            sf: l.sf,
            cover_max_pct: h.settings.brain.cover_max_pct,
            last_wins: &h.last_wins,
            last_kind: h.last_kind,
            hud_h: b.hud_h,
            only: b.only,
            prefer: b.prefer,
        })
    }

    fn brain_move(&mut self, plan: MovePlan, now: Instant) {
        let t = self.brain.as_ref().map_or(0.0, |h| h.bnow(now) - h.brain.start());
        let moving = self.teleport.is_some() || self.rag.is_some() || self.brain.as_ref().is_some_and(|h| h.moving() || h.flight.is_some());
        // A passive move waits for the emote on screen to finish (the sulk plays out first);
        // a closed perch or a lost workspace can't wait (she'd hang in the air).
        let urgent = matches!(plan.goal, Goal::Rescue | Goal::Follow);
        // A summon wakes her (one she sleeps through only by a mood set by hand); nothing
        // but a rescue starts while she stands up.
        if matches!(plan.goal, Goal::Active { why: Why::Summon(_), .. }) {
            self.wake_up("summon", now);
        }
        let emoting = !urgent && ((!matches!(plan.goal, Goal::Active { .. }) && self.anim.busy()) || self.waking(now));
        if (moving || emoting) && self.press.is_none() && !self.quitting {
            // Still landing from the last one: this one goes next (a sulk's far move after
            // the leave, a summon after a passive hop).
            if let Some(h) = &mut self.brain {
                h.next_plan = Some(plan);
            }
            return;
        }
        // The last check: a bubble, a field, the Line, a prompt, the menu or a drag holds every
        // move but a summon or leaving his window (the brain reads `busy` only at its polls).
        let busy = self.brain_busy();
        let held = crate::brain::held_by(&busy, &plan);
        if self.press.is_some() || self.quitting || self.brain.as_ref().is_some_and(|h| h.fs != FsHide::Shown) || held.is_some() {
            if let Some(h) = &mut self.brain {
                h.write(serde_json::json!({ "t": t, "k": "fail", "busy": true, "why": plan.why, "held_by": held }));
            }
            self.brain_feed(BrainEvent::MoveFailed { busy: true, why: plan.why }, now);
            self.move_failed_fallback(&plan, now);
            return;
        }
        let Some(wb) = self.world_buf(&plan.goal) else { return };
        let Some(world) = self.world(&wb) else { return };
        let Some(h) = &self.brain else { return };
        let mut rng = h.rng.clone();
        let res = places::resolve(&plan.goal, &world, &mut rng);
        let dnd = h.brain.dnd_left(h.bnow(now)).is_some();
        let enabled = h.brain.enabled();
        let goal_s = match &plan.goal {
            Goal::Passive { far } => if *far { "passive_far" } else { "passive" }.to_string(),
            Goal::Leave => "leave".into(),
            Goal::Active { why: Why::Attention, .. } => "bid".into(),
            Goal::Active { why: Why::Summon(k), .. } => format!("summon_{k:?}").to_lowercase(),
            Goal::Hop { kind, .. } => format!("hop_{}", kind.key()),
            Goal::Explore => "explore".into(),
            Goal::FocusEdge => "focus_edge".into(),
            Goal::Rescue => "rescue".into(),
            Goal::Follow => "follow".into(),
        };
        let (stack, areas, monitors, focused, cursor, her, senses_fs, only, prefer) = (&wb.stack, &wb.areas, &wb.monitors, wb.focused, wb.cursor, wb.her, wb.senses_fs, wb.only, wb.prefer);
        let spot = match res {
            Ok(s) => s,
            Err(why) => {
                let why: Vec<String> = why.iter().map(|(k, b)| format!("{k}:{b:?}")).collect();
                if let Some(h) = &mut self.brain {
                    h.rng = rng;
                    h.write(serde_json::json!({ "t": t, "k": "fail", "goal": goal_s, "why": why }));
                }
                self.brain_feed(BrainEvent::MoveFailed { busy: false, why: plan.why }, now);
                self.move_failed_fallback(&plan, now);
                return;
            }
        };
        // The first arrival that fits the distance.
        let l = self.layout;
        let (dx, dy) = (spot.anchor.0 - her.anchor.0, spot.anchor.1 - her.anchor.1);
        let dist = ((dx * dx + dy * dy) as f32).sqrt() / l.sf;
        let ground_kind = |k: SpotKind| matches!(k, SpotKind::Floor | SpotKind::Beside | SpotKind::FloorSit);
        // A leap: a small hop between any two spots (edge, floor) within reach.
        let can_leap = h.rest != Some(RestPose::Nap)
            && matches!(self.placement, Placement::Ground | Placement::Perch { .. })
            && matches!(spot.kind, SpotKind::Edge { .. } | SpotKind::SitEdge { .. } | SpotKind::Floor | SpotKind::Beside)
            && (dx.abs() as f32) / l.sf <= places::HOP_MAX + 1.0
            && (-dy as f32) / l.sf <= places::HOP_UP_MAX + 1.0;
        let can_hop = self.placement == Placement::Ground
            && h.rest.is_none()
            && ground_kind(spot.kind)
            && dy.abs() <= 4
            && (dx.abs() as f32) / l.sf <= moves::HOP_ARRIVAL_MAX;
        let active = matches!(plan.goal, Goal::Active { .. });
        let small = matches!(plan.goal, Goal::Hop { .. });
        let pose_kind = |k: SpotKind| matches!(k, SpotKind::Hang { .. } | SpotKind::Cling { .. } | SpotKind::Peek);
        let arrival = plan
            .arrivals
            .iter()
            .copied()
            .find(|a| match a {
                Arrival::Teleport => true,
                Arrival::Glitch => dist <= 1400.0,
                Arrival::Hop => can_hop || ((small || plan.why == "focus") && can_leap),
                Arrival::DropIn => ground_kind(spot.kind) || pose_kind(spot.kind),
                Arrival::Drift => false,
            })
            .unwrap_or(if small { Arrival::Hop } else { Arrival::Teleport });
        if small && !can_leap {
            // A small hop that can't be leapt (from a float or a rest pose): not now.
            if let Some(h) = &mut self.brain {
                h.rng = rng;
                h.write(serde_json::json!({ "t": t, "k": "fail", "goal": goal_s, "why": ["no_leap"] }));
            }
            self.brain_feed(BrainEvent::MoveFailed { busy: false, why: plan.why }, now);
            return;
        }
        let leap = arrival == Arrival::Hop && !can_hop && can_leap;
        // Passive transports by distance (§4.1): float to float drifts; along the same floor
        // within SCOOT_MAX she hop-scoots half the time, and always under MIN_TELEPORT.
        // `hop = 0` in settings: she never hops (no scoot either).
        let hops_on = h.settings.brain.arrivals.iter().any(|a| a.0 == Arrival::Hop && a.1 > 0.0);
        let can_scoot = self.placement == Placement::Ground
            && hops_on
            && h.rest.is_none()
            && ground_kind(spot.kind)
            && dy.abs() <= 4
            && (dx.abs() as f32) / l.sf <= moves::SCOOT_MAX
            && dist >= moves::HOP_DX.0;
        let reactive = matches!(plan.goal, Goal::Rescue | Goal::Follow | Goal::FocusEdge);
        let arrival = if active || small || reactive {
            arrival
        } else if self.placement == Placement::Float && spot.kind == SpotKind::Float && dist <= 600.0 {
            Arrival::Drift
        } else if can_scoot && (dist < moves::MIN_TELEPORT || h.rng.clone().pick(2) == 0) {
            Arrival::Hop
        } else {
            arrival
        };
        let cover_ok = |s: &Spot| s.cover_pct <= world.cover_max_pct + 1e-9;
        let line = serde_json::json!({
            "t": t, "k": "move", "goal": goal_s, "approach": spot.approach.map(Approach::key), "arrival": if leap { "leap" } else { arrival.key() },
            "spot": spot.kind.key(), "anchor": [spot.anchor.0, spot.anchor.1], "body": rect_json(&spot.body),
            "cursor": [cursor.0, cursor.1], "focus": focused.as_ref().map(rect_json), "cover_pct": (spot.cover_pct * 100.0).round() / 100.0,
            "cover_ok": cover_ok(&spot), "fs": senses_fs, "dnd": dnd, "enabled": enabled, "area": rect_json(&areas[spot.area.min(areas.len() - 1)]),
            "clear_px": (CURSOR_CLEAR_LOGICAL * l.sf).round(), "from": [her.anchor.0, her.anchor.1],
            "why": plan.why, "only": only, "prefer": prefer,
        });
        // One line per move on stderr too (rare): what moved her, from where to where.
        elog!(
            "synaps-avatar: brain move why={} goal={goal_s} spot={} {},{} -> {},{} by {}",
            plan.why,
            spot.kind.key(),
            her.anchor.0,
            her.anchor.1,
            spot.anchor.0,
            spot.anchor.1,
            if leap { "leap" } else { arrival.key() }
        );
        // Facing (`[mirror] self`): the way she goes, then what she cares about once there.
        let own = self.mirror_cfg.own;
        let dead = (crate::facing::DEAD_PX * l.sf) as i32;
        let mid = |r: &WinRect| r.x + r.w / 2;
        let ax = spot.anchor.0;
        let travel = crate::facing::toward(her.anchor.0, ax, dead).filter(|_| own && spot.kind != SpotKind::InPlace);
        let face = if !own {
            None
        } else if active {
            crate::facing::toward(ax, cursor.0, dead).or_else(|| focused.and_then(|f| crate::facing::toward(ax, mid(&f), dead)))
        } else {
            monitors.iter().find(|m| m.x <= ax && ax < m.x + m.w).and_then(|m| crate::facing::inward(ax, m.x, m.w)).or_else(|| match spot.kind {
                SpotKind::Edge { win } | SpotKind::SitEdge { win } => stack.iter().find(|r| r.id == win).and_then(|r| crate::facing::toward(ax, mid(r), dead)),
                SpotKind::Beside | SpotKind::FloatBeside => focused.and_then(|f| crate::facing::toward(ax, mid(&f), dead)),
                _ => None,
            })
        };
        let her_anchor = her.anchor;
        if let Some(r) = travel {
            self.face(r, "travel", now, true);
        }
        let Some(h) = &mut self.brain else { return };
        h.rng = rng;
        h.write(line);
        let approach = if active { spot.approach } else { None };
        let summon_prompt = matches!(plan.goal, Goal::Active { why: Why::Summon(crate::brain::SummonKind::Prompt(_)), .. });
        h.flight = Some(InFlight { approach, arrival, spot, active, summon_prompt, face, why: plan.why });
        // Any move ends the bid marker and a sulk sit.
        h.marker = None;
        h.sulk_sit = false;
        if spot.kind == SpotKind::InPlace {
            // Bid right where she is.
            self.glitch = Some(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
            return;
        }
        let (origin, place) = self.spot_origin(&spot);
        // Leaving a floor sit or a pose.
        if let Some(h) = &mut self.brain {
            h.rest = None;
        }
        if leap {
            self.start_leap(her_anchor, &spot, origin, place, plan.why == "startle_drag", now);
            return;
        }
        // Passive moves (roams, leaving his window) teleport quietly; bids, summons and a
        // closed perch's rescue don't (the plain red bolt, LOCKED #1).
        let quiet = !active && plan.why != "perch_closed";
        match arrival {
            Arrival::Hop => {
                let mut rng = Rng::new(self.rng.next());
                let sf = l.sf;
                let from = her.anchor.0 as f32 / sf;
                let to = spot.anchor.0 as f32 / sf;
                let hops = moves::plan_scoot(from, to, &mut rng).unwrap_or_default();
                // Every hop's landing re-checks the rules: never into his window, never at the
                // cursor. A hop that would is where the scoot stops.
                let clear = places::CURSOR_CLEAR * sf;
                let body = spot.body;
                let ok = |x: f32| {
                    let b = WinRect { x: body.x + (x * sf).round() as i32 - spot.anchor.0, ..body };
                    let (cx, cy) = cursor;
                    let dx = (cx - b.x.clamp(cx.min(b.x), b.right())).abs().min((cx - b.right()).abs()).min((cx - b.x).abs());
                    let inside_x = cx >= b.x && cx < b.right();
                    let dy = if cy >= b.y && cy < b.bottom() { 0 } else { (cy - b.y).abs().min((cy - b.bottom()).abs()) };
                    let d = if inside_x { dy as f32 } else { ((dx * dx + dy * dy) as f32).sqrt() };
                    focused.is_none_or(|f| f.intersect(&b).is_none()) && d >= clear
                };
                let hops = moves::scoot_until(&hops, ok);
                let hops: Vec<moves::Hop> = hops.into_iter().map(|hp| moves::Hop { from_x: hp.from_x * sf, to_x: hp.to_x * sf }).collect();
                if let Some(h) = &mut self.brain {
                    h.scoot = Some(Scoot { hops, i: 0, t0: now, feet_y: spot.anchor.1, crouched: false });
                }
                self.relayer(now);
            }
            Arrival::Glitch => self.start_glitch_move(origin, place, now, quiet),
            Arrival::Drift => {
                let from = (self.origin.0 as f32, self.origin.1 as f32);
                let to = (origin.0 as f32, origin.1 as f32);
                let d = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
                let n = (d / (DRIFT_LEG * l.sf)).ceil().max(1.0) as usize;
                let mut rng = Rng::new(self.rng.next());
                let legs: Vec<moves::Drift> = (0..n)
                    .map(|k| {
                        let a = k as f32 / n as f32;
                        let b = (k + 1) as f32 / n as f32;
                        let p = |u: f32| (from.0 + (to.0 - from.0) * u, from.1 + (to.1 - from.1) * u);
                        // moves::Drift caps the speed in logical px/s: plan in logical, move in device.
                        let dr = moves::Drift::new((p(a).0 / l.sf, p(a).1 / l.sf), (p(b).0 / l.sf, p(b).1 / l.sf), &mut rng);
                        moves::Drift { from: p(a), to: p(b), secs: dr.secs }
                    })
                    .collect();
                if let Some(h) = &mut self.brain {
                    h.drift = Some(DriftMove { legs, i: 0, t0: now, pause_until: None });
                }
            }
            Arrival::DropIn => self.start_drop_in(origin, place, now),
            Arrival::Teleport => {
                self.start_teleport_q(origin, place, now, quiet);
                // Some land with the intro's arrival (on a window top: onto it, then sit); a
                // closed perch's rescue always does (LOCKED #18). A summon has its own landing.
                match plan.why {
                    "perch_closed" => self.arrive_with(ArriveKind::Rescue),
                    "summon" => {}
                    _ => self.arrive_with(ArriveKind::Teleport),
                }
            }
        }
    }

    /// Window origin and placement for a spot.
    fn spot_origin(&self, s: &Spot) -> ((i32, i32), Placement) {
        let l = &self.layout;
        let feet = |a: (i32, i32)| (a.0 - l.ground_x, a.1 - l.ground_y);
        let pose = |key: &str, a: (i32, i32)| -> ((i32, i32), Placement) {
            match self.brain_pose(key) {
                Some(f) => {
                    let p = self.anchor_px(f);
                    ((a.0 - p.0, a.1 - p.1), Placement::Pose { frame: f })
                }
                None => (feet(a), Placement::Ground),
            }
        };
        match s.kind {
            SpotKind::Edge { win } | SpotKind::SitEdge { win } => {
                let rx = self.os.toplevel_rect(win).map_or(s.anchor.0, |r| r.x);
                let sit = l.seat_y.is_some();
                let o = (s.anchor.0 - l.ground_x, s.anchor.1 - l.seat_y.unwrap_or(l.ground_y));
                (o, Placement::Perch { win, sit, dx: s.anchor.0 - rx })
            }
            SpotKind::Hang { upside_down } => pose(if upside_down { "hang_upside_down" } else { "hang" }, s.anchor),
            SpotKind::Cling { left } => pose(if left { "cling_left" } else { "cling_right" }, s.anchor),
            SpotKind::Peek => pose("peek", s.anchor),
            SpotKind::Float | SpotKind::FloatBeside => (feet(s.anchor), Placement::Float),
            SpotKind::Floor | SpotKind::FloorSit | SpotKind::Beside => (feet(s.anchor), Placement::Ground),
            SpotKind::InPlace => (self.origin, self.placement),
        }
    }

    /// The `glitch` arrival: vanish, then land there; no bolt.
    fn start_glitch_move(&mut self, to: (i32, i32), place: Placement, now: Instant, quiet: bool) {
        self.start_teleport_q(to, place, now, quiet);
        if let Some(tp) = &mut self.teleport {
            tp.no_bolt = true;
        }
    }

    /// Appear above the spot (a quick soft glitch) and fall onto it: a squash on the floor,
    /// a swing on a grip.
    fn start_drop_in(&mut self, to: (i32, i32), place: Placement, now: Instant) {
        let l = self.layout;
        let posed = matches!(place, Placement::Pose { .. });
        let mut h = (if posed { DROP_IN_H.1 } else { DROP_IN_H.0 }) * l.sf;
        // Not above the top of her work area (standing); a grip may start over the bar.
        if !posed && let Some(a) = self.her_area_at(to) {
            let top = to.1 + l.avatar_y + self.frame_or_default(self.anim.base()).map_or(0, |f| f.top);
            h = h.min((top - a.y).max(0) as f32);
        }
        self.os.set_target(0);
        self.arrive(to, place, now);
        if let Some(hb) = &mut self.brain {
            hb.drop_in = Some((now, h, posed));
        }
        self.apply_offset(now);
        let mut g = Glitch::new(GlitchKind::Appear, now, self.rng.next());
        g.soft = true;
        self.glitch = Some(g);
    }

    /// The window offset of a drop_in at `now` (up = negative), falling under gravity.
    pub(super) fn drop_in_offset(&self, now: Instant) -> i32 {
        let Some((t0, h, _)) = self.brain.as_ref().and_then(|b| b.drop_in) else { return 0 };
        let g = ragdoll_g(self.layout.sf);
        let t = now.saturating_duration_since(t0).as_secs_f32();
        let fallen = 0.5 * g * t * t;
        -((h - fallen).max(0.0)).round() as i32
    }

    fn step_drop_in(&mut self, now: Instant) -> bool {
        let Some((_, _, posed)) = self.brain.as_ref().and_then(|b| b.drop_in) else { return false };
        if self.drop_in_offset(now) < 0 {
            return false;
        }
        if let Some(h) = &mut self.brain {
            h.drop_in = None;
        }
        if posed {
            // Caught the line: a big swing.
            let side = if self.rng.pick(2) == 0 { 1.0 } else { -1.0 };
            self.swing_tick(now);
            self.swing_push(side * 26.0);
        } else {
            self.squash = Some(now);
        }
        self.apply_offset(now);
        true
    }

    fn her_area_at(&self, o: (i32, i32)) -> Option<WinRect> {
        let (areas, _) = self.rag_areas();
        let (gx, gy) = (o.0 + self.layout.ground_x, o.1 + self.layout.ground_y - 1);
        areas.iter().find(|a| a.contains(gx, gy)).or(areas.first()).copied()
    }

    fn step_drift(&mut self, now: Instant) -> bool {
        let Some(h) = &mut self.brain else { return false };
        let Some(d) = &mut h.drift else { return false };
        if d.pause_until.is_some_and(|p| p > now) {
            return false;
        }
        if let Some(p) = d.pause_until.take() {
            d.t0 = p;
        }
        let Some(leg) = d.legs.get(d.i).copied() else {
            h.drift = None;
            return true;
        };
        let t = now.saturating_duration_since(d.t0).as_secs_f32();
        let p = leg.at(t);
        if leg.done(t) {
            d.i += 1;
            if d.i < d.legs.len() {
                d.pause_until = Some(now + DRIFT_PAUSE);
            } else {
                h.drift = None;
            }
        }
        self.set_origin((p.0.round() as i32, p.1.round() as i32));
        false
    }

    /// The float bob (window offset, up = negative): 20 s of a slow ±2.5 px bob in 1 px
    /// steps, then 30 s still.
    pub(super) fn float_bob(&self, now: Instant) -> i32 {
        if self.placement != Placement::Float || self.brain.as_ref().is_none_or(|h| h.drift.is_some()) {
            return 0;
        }
        let t = now.saturating_duration_since(self.bob_t0).as_secs_f32() % FLOAT_BOB_CYCLE;
        if t >= FLOAT_BOB_ON {
            return 0;
        }
        let env = (t / 1.0).min((FLOAT_BOB_ON - t) / 1.0).min(1.0);
        -(2.5 * self.layout.sf * env * (t * std::f32::consts::TAU / 3.2).sin()).round() as i32
    }

    fn float_bob_next(&self, now: Instant) -> Option<Instant> {
        if self.placement != Placement::Float {
            return None;
        }
        let cur = self.float_bob(now);
        let step = Duration::from_millis(40);
        (1..=200).map(|k| now + step * k).find(|&t| self.float_bob(t) != cur).or(Some(now + Duration::from_secs(8)))
    }

    fn step_scoot(&mut self, now: Instant) -> bool {
        let Some(h) = &mut self.brain else { return false };
        let Some(sc) = &mut h.scoot else { return false };
        let l = self.layout;
        if sc.i >= sc.hops.len() {
            h.scoot = None;
            return true;
        }
        let hop = sc.hops[sc.i];
        let ms = now.saturating_duration_since(sc.t0).as_millis() as u32;
        let crouch = self.pack.behaviour.brain.hop_crouch.as_ref().and_then(|c| self.pack.index.get(c)).copied();
        if !sc.crouched {
            sc.crouched = true;
            if let Some(c) = crouch {
                self.anim.play(vec![(c, moves::HOP_CROUCH_MS)], now);
            }
        }
        let (ox, oy) = hop.offset(ms.min(moves::Hop::MS));
        let x = (hop.from_x + ox).round() as i32;
        let lift = (oy * l.sf).round() as i32;
        let feet_y = sc.feet_y;
        if ms >= moves::Hop::MS {
            sc.i += 1;
            sc.t0 = now;
            sc.crouched = false;
        } else if ms >= moves::HOP_CROUCH_MS + moves::HOP_AIR_MS && self.squash.is_none() {
            self.squash = Some(now);
        }
        let done = sc.i >= sc.hops.len();
        self.set_origin((x - l.ground_x, feet_y + lift - l.ground_y));
        if done {
            if let Some(h) = &mut self.brain {
                h.scoot = None;
            }
            self.placement = Placement::Ground;
            state::save_ground(self.ground());
        }
        true
    }

    fn brain_landed(&mut self, now: Instant) {
        let Some(h) = &mut self.brain else { return };
        let Some(f) = h.flight.take() else { return };
        let b = h.bnow(now) - h.brain.start();
        if let SpotKind::Edge { win } = f.spot.kind {
            h.last_wins.push(win);
            if h.last_wins.len() > 2 {
                h.last_wins.remove(0);
            }
        }
        if !f.active {
            h.last_kind = Some(f.spot.kind);
        }
        h.spot = Some(f.spot);
        let covering = f.active && f.spot.covers_focus();
        h.write(serde_json::json!({ "t": b, "k": "arrived", "spot": f.spot.kind.key(), "arrival": f.arrival.key(),
                                    "approach": f.approach.map(Approach::key), "covering": covering, "body": rect_json(&f.spot.body) }));
        let queued = std::mem::take(&mut h.queued);
        if f.spot.kind == SpotKind::FloorSit {
            h.rest = Some(RestPose::FloorSit);
            let entry = self.pack.behaviour.brain.floor_sit.as_ref().and_then(|s| s.entry.clone()).unwrap_or_default();
            self.relayer(now);
            let seq = self.pack.flatten(&entry);
            if !seq.is_empty() {
                self.anim.play(seq, now);
            }
        } else {
            self.relayer(now);
        }
        if self.placement == Placement::Ground {
            self.home = self.ground();
        }
        if let Some(r) = f.face {
            self.face(r, "arrive", now, false);
        }
        self.brain_feed(BrainEvent::Arrived(Arrived { approach: f.approach, arrival: f.arrival, covering, why: f.why }), now);
        if f.summon_prompt {
            self.summon_landed(now);
        }
        for e in queued {
            self.play_emote(e, now);
        }
    }

    /// A hop moves the whole window: with a bubble up (a summon's prompt) it would jump
    /// with her, so she squashes in place instead.
    fn hop_or_squash(&mut self, now: Instant) {
        if self.panel_up() {
            self.squash = Some(now);
        } else {
            self.hop = Some(now);
        }
    }

    /// A summon for his prompt landed: one hop, a point at his window, then the attentive
    /// lean (the NeedsYou mood) with the status pulsing for a few seconds.
    fn summon_landed(&mut self, now: Instant) {
        if self.placement == Placement::Ground {
            self.hop_or_squash(now);
            let right = self.brain_focused().is_some_and(|f| f.center().0 > self.ground().0);
            let steps = [crate::pack::Step::Clip(if right { "point_right" } else { "point_left" }.into())];
            let seq = self.pack.flatten(&steps);
            if !seq.is_empty() {
                self.anim.play(seq, now);
            }
        }
        if let Some(h) = &mut self.brain {
            h.pulse = Some(now);
            h.glow = Some(now);
        }
    }

    /// The settled summon's warm border: 0..=6 on a slow ~1.8 s breath, once the landing
    /// blink is over, while the prompt waits.
    pub(super) fn summon_glow(&self, now: Instant) -> Option<u8> {
        let t0 = self.brain.as_ref()?.glow?;
        if !self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed) {
            return None;
        }
        let d = now.saturating_duration_since(t0);
        if d < PULSE_FOR {
            return None;
        }
        let ph = ((d - PULSE_FOR).as_secs_f32() / GLOW_S).fract();
        Some((6.0 * (0.5 - 0.5 * (std::f32::consts::TAU * ph).cos())).round() as u8)
    }

    fn glow_next_change(&self, now: Instant) -> Option<Instant> {
        let t0 = self.brain.as_ref()?.glow?;
        let cur = self.summon_glow(now);
        if cur.is_none() {
            let start = t0 + PULSE_FOR;
            return (start > now && self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed)).then_some(start);
        }
        let step = Duration::from_millis(50);
        (1..=40).map(|k| now + step * k).find(|&t| self.summon_glow(t) != cur)
    }

    /// NEEDS YOU inverted (the pulse's "on" half) at `now`.
    pub(super) fn summon_pulse_on(&self, now: Instant) -> bool {
        let Some(t0) = self.brain.as_ref().and_then(|h| h.pulse) else { return false };
        let d = now.saturating_duration_since(t0);
        d < PULSE_FOR && (d.as_millis() / PULSE_HALF_MS).is_multiple_of(2) && self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed)
    }

    // ---- lively: small hops, reactions, the screen ------------------------------------------------

    /// A move that couldn't run: a closed perch's rescue or a stranded workspace follow
    /// falls instead (she'd be sitting on nothing).
    fn move_failed_fallback(&mut self, plan: &MovePlan, now: Instant) {
        if matches!(plan.why, "perch_closed" | "workspace")
            && let Placement::Perch { win, .. } = self.placement
            && self.os.toplevel_rect(win).is_none()
        {
            self.fall("perch_gone", now);
        }
    }

    /// Start a small hop from `from` (her anchor: feet or seat) to the spot: standing frames
    /// in the air, her feet along the arc, then she settles into the spot's pose.
    fn start_leap(&mut self, from: (i32, i32), spot: &Spot, origin: (i32, i32), place: Placement, startled: bool, now: Instant) {
        let l = self.layout;
        let sf = l.sf;
        self.os.set_target(0);
        self.hop = None;
        self.menu = None;
        let to = spot.anchor;
        let leap = moves::Leap { from: (from.0 as f32 / sf, from.1 as f32 / sf), to: (to.0 as f32 / sf, to.1 as f32 / sf) };
        self.placement = Placement::Ground;
        if let Some(h) = &mut self.brain {
            h.leap = Some(LeapMove { leap, t0: now, origin, place, crouched: false, startled });
        }
        self.set_origin((from.0 - l.ground_x, from.1 - l.ground_y));
        self.relayer(now);
        if startled {
            self.glitch.get_or_insert(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
        }
    }

    fn step_leap(&mut self, now: Instant) -> bool {
        let Some(h) = &mut self.brain else { return false };
        let Some(lp) = &mut h.leap else { return false };
        let l = self.layout;
        let ms = now.saturating_duration_since(lp.t0).as_millis() as u32;
        if !lp.crouched {
            lp.crouched = true;
            let startled = lp.startled;
            // A big leap (well up, or far) or a startled one: the deeper crouch.
            let big = startled || lp.leap.from.1 - lp.leap.to.1 > BIG_LEAP_UP || (lp.leap.to.0 - lp.leap.from.0).abs() > BIG_LEAP_DX;
            let b = &self.pack.behaviour.brain;
            let id = if big { b.big_hop_crouch.as_ref().or(b.hop_crouch.as_ref()) } else { b.hop_crouch.as_ref() };
            let crouch = id.and_then(|c| self.pack.index.get(c)).copied();
            let air = lp.leap.air_ms();
            let mut seq: Vec<(usize, u32)> = crouch.map(|c| vec![(c, moves::HOP_CROUCH_MS)]).unwrap_or_default();
            // Startled: wide-eyed in the air.
            if startled {
                seq.extend(self.pack.flatten(&[crate::pack::Step::Hold { frame: "surprised".into(), ms: air + moves::HOP_LAND_MS }]));
            }
            if !seq.is_empty() {
                self.anim.play(seq, now);
            }
        }
        let Some(h) = &mut self.brain else { return false };
        let Some(lp) = &mut h.leap else { return false };
        let (x, y) = lp.leap.at(ms);
        let feet = ((x * l.sf).round() as i32, (y * l.sf).round() as i32);
        if lp.leap.done(ms) {
            let (origin, place) = (lp.origin, lp.place);
            h.leap = None;
            self.squash = Some(now);
            self.arrive(origin, place, now);
            if place == Placement::Ground {
                self.home = self.ground();
                state::save_ground(self.ground());
            }
            return true;
        }
        self.set_origin((feet.0 - l.ground_x, feet.1 - l.ground_y));
        true
    }

    /// A reaction in place (the brain checked the gate and the rate limits).
    fn brain_react(&mut self, r: crate::brain::React, now: Instant) {
        use crate::brain::React;
        if r == React::Fall {
            if matches!(self.placement, Placement::Perch { .. }) {
                self.fall("perch_gone", now);
            }
            return;
        }
        // Never over a move, a drag, a hidden body or a panel.
        if self.hidden || self.parked.is_some() || self.teleport.is_some() || self.rag.is_some() || self.press.is_some() || self.brain_in_flight() || self.panel_up() {
            return;
        }
        let c = self.brain_cursor();
        let (gx, _) = self.ground();
        let right = c.0 > gx;
        let standing = self.standing() && !self.anim.busy();
        let what = match r {
            React::Look => {
                if self.mirror_cfg.own {
                    self.face(right, "look", now, false);
                }
                if standing {
                    let seq = self.pack.flatten(&self.resolve_steps(&[crate::pack::Step::Clip("@look".into())]));
                    if !seq.is_empty() {
                        self.anim.play(seq, now);
                    }
                }
                "look"
            }
            React::Startle => {
                self.glitch.get_or_insert(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
                if self.hop_allowed() {
                    self.hop_or_squash(now);
                } else {
                    self.squash = Some(now);
                }
                if standing {
                    let steps = match &self.pack.behaviour.brain.startle {
                        s if s.is_empty() => vec![crate::pack::Step::Hold { frame: "surprised".into(), ms: 600 }],
                        s => s.clone(),
                    };
                    let seq = self.pack.flatten(&steps);
                    if !seq.is_empty() {
                        self.anim.play(seq, now);
                    }
                }
                "startle"
            }
            React::Lean => {
                if self.mirror_cfg.own {
                    self.face(right, "look", now, false);
                }
                let seq = if standing { self.pack.flatten(&[crate::pack::Step::Hold { frame: "lean_in-flirty".into(), ms: 2200 }]) } else { Vec::new() };
                if seq.is_empty() {
                    self.hearts = Some((now, self.head_point()));
                } else {
                    self.anim.play(seq, now);
                }
                "lean"
            }
            React::Fall => unreachable!(),
        };
        if let Some(h) = &mut self.brain {
            let t = h.bnow(now) - h.brain.start();
            h.write(serde_json::json!({ "t": t, "k": "react", "what": what, "cursor": [c.0, c.1] }));
        }
    }

    /// Focus, workspace and pointer events from the X watchers.
    pub(super) fn brain_watch(&mut self, ev: WatchEvent, now: Instant) {
        use crate::brain::Screen;
        if self.brain.is_none() || !self.loaded {
            return;
        }
        match ev {
            WatchEvent::Focus => {
                // A look at the desk (not while a switch is being looked at: that one is "before").
                let desk = self.brain.as_ref().is_some_and(|h| h.desk_check.is_none()).then(|| self.desk_windows());
                let f = self.brain_focused().map(|r| r.id);
                let Some(h) = &mut self.brain else { return };
                if desk.is_some() {
                    h.desk_seen = desk;
                }
                if f.is_none() || f == h.focus_seen {
                    return;
                }
                let first = h.focus_seen.is_none();
                h.focus_seen = f;
                // A workspace switch moves the focus too: the workspace rule has it.
                if first || h.desk_at.is_some_and(|d| now.duration_since(d) < Duration::from_millis(1500)) {
                    return;
                }
                self.brain_feed(BrainEvent::Screen(Screen::Focus), now);
            }
            WatchEvent::Desktop => {
                if let Some(h) = &mut self.brain {
                    // The desk before it (a second switch within the beat keeps the first's).
                    if h.desk_check.is_none() {
                        h.desk_before = h.desk_seen.take();
                    }
                    h.desk_at = Some(now);
                    // Where she is, once a perch that went with the old workspace has had its
                    // grace (edges::PERCH_GONE_GRACE) and the WM has settled.
                    h.desk_check = Some(now + edges::PERCH_GONE_GRACE + Duration::from_millis(150));
                }
            }
            WatchEvent::Pointer(p) => {
                let s = match p {
                    crate::platform::PointerEv::Near => Screen::CursorNear,
                    crate::platform::PointerEv::Swipe(v) => Screen::CursorSwipe { speed: v / self.layout.sf },
                    crate::platform::PointerEv::Hover => Screen::CursorHover,
                };
                self.brain_feed(BrainEvent::Screen(s), now);
            }
            _ => {}
        }
    }

    /// The windows she may sit on now, by monitor (the test's arena and window list applied).
    fn desk_windows(&self) -> Vec<(u32, usize)> {
        let (areas, monitors) = self.rag_areas();
        let allowed = self.brain_allowed_windows();
        let (stack, eligible): (Vec<WinRect>, Vec<bool>) =
            self.os.stack().into_iter().map(|(r, e)| (r, e && r.id != self.os.body() && allowed.as_ref().is_none_or(|w| w.contains(&r.id)))).unzip();
        places::desk_windows(&stack, &eligible, &areas, &monitors)
    }

    /// A beat after a workspace switch: did it change her monitor (`places::workspace_switch`)?
    fn desk_settled(&mut self, now: Instant) {
        let after = self.desk_windows();
        let before = self.brain.as_mut().and_then(|h| {
            h.desk_check = None;
            h.desk_seen = Some(after.clone());
            h.desk_before.take()
        });
        // Mid-move, carried, or her perch's own verdict is still out: that decides.
        if self.teleport.is_some() || self.rag.is_some() || self.press.is_some() || self.brain_in_flight() || self.perch_gone.is_some() {
            return;
        }
        let (areas, monitors) = self.rag_areas();
        let her = self.her_spot(&areas, &monitors);
        let cursor = self.brain_cursor();
        let focused = self.brain_focused();
        let s = places::workspace_switch(&monitors, focused, cursor, her.area, false, before.as_deref(), &after);
        let here = matches!(s, crate::brain::Screen::Workspace { here: true, .. });
        elog!("synaps-avatar: workspace switch {} (her monitor {})", if here { "on her monitor: she moves on it" } else { "not on her monitor: she stays put" }, her.area);
        if let Some(h) = &mut self.brain {
            let t = h.bnow(now) - h.brain.start();
            h.write(serde_json::json!({ "t": t, "k": "desk", "her_area": her.area, "cursor": [cursor.0, cursor.1], "focus": focused.as_ref().map(rect_json),
                                        "before": before.map(|b| b.len()), "after": after.len(), "here": here }));
        }
        self.brain_feed(BrainEvent::Screen(s), now);
    }

    /// Her perch went away for good (past the grace): closed (another window's top or the
    /// fall, the brain decides), or gone with the workspace he left (she follows him).
    pub(super) fn brain_perch_gone(&mut self, win: u32, now: Instant) {
        let Some(h) = &self.brain else {
            self.fall("perch_gone", now);
            return;
        };
        let switched = h.desk_at.is_some_and(|d| now.duration_since(d) < Duration::from_millis(1500));
        let exists = self.os.window_exists(win);
        let s = if switched && exists {
            elog!("synaps-avatar: perch 0x{win:x} left with the workspace: she moves on her monitor");
            let after = self.desk_windows();
            let before = self.brain.as_mut().and_then(|h| {
                h.desk_check = None;
                h.desk_seen = Some(after.clone());
                h.desk_before.take()
            });
            let (areas, monitors) = self.rag_areas();
            let her = self.her_spot(&areas, &monitors);
            places::workspace_switch(&monitors, self.brain_focused(), self.brain_cursor(), her.area, true, before.as_deref(), &after)
        } else {
            let wb = self.world_buf(&Goal::Rescue);
            let res = wb.as_ref().and_then(|b| self.world(b)).map(|w| places::rescue_target(&w));
            let target = matches!(res, Some(Ok(())));
            let why = match &res {
                Some(Err(why)) => why.iter().map(|(k, b)| format!("{k}:{b:?}")).collect::<Vec<_>>().join(" "),
                _ => String::new(),
            };
            elog!(
                "synaps-avatar: perch 0x{win:x} closed ({}): teleport or fall{}",
                if target { "another window has room" } else { "no other window with room: falls" },
                if why.is_empty() { String::new() } else { format!(" [{why}]") }
            );
            crate::brain::Screen::PerchClosed { target }
        };
        self.brain_feed(BrainEvent::Screen(s), now);
        // Nothing decided (the brain is off, say): she mustn't stay up on nothing.
        let decided = self.teleport.is_some() || self.brain_in_flight() || self.rag.is_some() || !matches!(self.placement, Placement::Perch { win: w, .. } if w == win);
        if !decided && self.brain.as_ref().is_some_and(|h| h.next_plan.is_none()) {
            self.fall("perch_gone", now);
        }
    }

    /// Her perch moved (she rides it): dragged fast, the brain may startle her off it.
    pub(super) fn brain_perch_moved(&mut self, r: &WinRect, now: Instant) {
        let Some(h) = &mut self.brain else { return };
        let prev = h.perch_seen.filter(|p| p.0 == r.id);
        h.perch_seen = Some((r.id, now, r.x, r.y));
        let Some((_, t0, x0, y0)) = prev else { return };
        let dt = now.saturating_duration_since(t0).as_secs_f32();
        if !(0.008..0.25).contains(&dt) {
            return;
        }
        let v = ((((r.x - x0).pow(2) + (r.y - y0).pow(2)) as f32).sqrt() / dt) / self.layout.sf;
        let told = h.drag_told.is_some_and(|t| now.duration_since(t) < Duration::from_millis(800));
        if v >= h.brain.tuning.drag_speed && !told {
            h.drag_told = Some(now);
            self.brain_feed(BrainEvent::Screen(crate::brain::Screen::PerchDragged { speed: v }), now);
        }
    }

    /// Her body box and arming for the pointer watcher (only when they change).
    pub(super) fn sync_pointer_watch(&mut self) {
        let armed = self.loaded
            && !self.hidden
            && self.parked.is_none()
            && self.brain.as_ref().is_some_and(|h| h.brain.enabled() && h.fs == FsHide::Shown && h.brain.dnd_left(h.bnow(Instant::now())).is_none());
        let bx = if armed {
            let fi = self.anim.frame();
            self.frame_or_default(fi).and_then(|f| {
                let l = self.layout;
                let cols: Vec<usize> = f.profile.iter().enumerate().filter(|(_, y)| **y != i16::MAX).map(|(x, _)| x).collect();
                let (c0, c1) = (*cols.first()? as i32, *cols.last()? as i32);
                let (bx, by) = (self.applied.0 + l.avatar_x, self.applied.1 + l.avatar_y);
                Some(WinRect::new(bx + c0, by + f.top, c1 - c0 + 1, f.bottom - f.top))
            })
        } else {
            None
        };
        let Some(h) = &mut self.brain else { return };
        if h.watch_box == Some((bx, armed)) {
            return;
        }
        h.watch_box = Some((bx, armed));
        self.os.set_pointer_watch(bx, armed);
    }

    // ---- emotes, hide ---------------------------------------------------------------------------

    fn brain_emote(&mut self, e: EmoteKey, now: Instant) {
        let Some(h) = &mut self.brain else { return };
        // Moving or carried: after the landing.
        if h.flight.is_some() || h.moving() || self.rag.is_some() {
            h.queued.push(e);
            return;
        }
        if let Some(r) = self.reaction_until.filter(|&r| r > now) {
            h.later.push((r, e));
            return;
        }
        self.play_emote(e, now);
    }

    /// An idle look-around once in a while (`[mirror] idle_every_min`, brain clock): never
    /// while she speaks, a panel is up, mid-gesture or on the move, nor within
    /// `LOOK_QUIET_S` of another turn.
    fn idle_look(&mut self, now: Instant) -> bool {
        let every = self.mirror_cfg.idle_every_min;
        let own = self.mirror_cfg.own;
        let Some(h) = &mut self.brain else { return false };
        let b = h.bnow(now);
        if !own || !h.brain.enabled() {
            // No wake-ups for it while off; scheduled again once she's on.
            h.look_b = None;
            return false;
        }
        let Some(at) = h.look_b else {
            let u = h.look_rng.range(0.0, 1.0);
            h.look_b = Some(b + crate::facing::look_gap(every, u));
            return false;
        };
        if at > b {
            return false;
        }
        let since = self.last_turn.map_or(f64::MAX, |t| b - h.bnow(t));
        let busy = h.moving()
            || h.flight.is_some()
            || since < crate::facing::LOOK_QUIET_S
            || !self.loaded
            || self.hidden
            || self.bubble.is_some()
            || self.reader.is_some()
            || self.panel_up()
            || self.anim.busy()
            || self.rag.is_some()
            || self.teleport.is_some()
            || self.glitch.is_some()
            || self.press.is_some()
            || self.turn_busy()
            || self.asleep()
            || matches!(self.mood, Mood::Talking | Mood::Thinking | Mood::Working | Mood::NeedsYou)
            || !self.can_mirror_now();
        let Some(h) = &mut self.brain else { return false };
        if busy {
            // Try again in a little while.
            let u = h.look_rng.range(8.0, 20.0) as f64;
            h.look_b = Some(b + u);
            return false;
        }
        let u = h.look_rng.range(0.0, 1.0);
        h.look_b = Some(b + crate::facing::look_gap(every, u));
        h.write(serde_json::json!({ "t": b - h.brain.start(), "k": "look" }));
        let right = !self.facing_right();
        self.face(right, "look", now, false);
        true
    }

    /// `@look` / `@point` toward the cursor.
    fn resolve_steps(&self, steps: &[crate::pack::Step]) -> Vec<crate::pack::Step> {
        use crate::pack::Step;
        let c = self.brain_cursor();
        let (gx, gy) = self.ground();
        let head_y = gy - (self.layout.geo.height as f32 * 0.8) as i32;
        // The look and point frames are drawn to a side: mirrored, the other one points there.
        let right = (c.0 > gx) != self.turn_target();
        steps
            .iter()
            .map(|s| match s {
                Step::Clip(id) if id == "@look" => {
                    let sf = self.layout.sf;
                    // Close to her body, level with her face or chest (or on her): straight at him.
                    let reach = (FRONT_PX * sf) as i32;
                    let front = self.brain.as_ref().and_then(|h| h.watch_box?.0).is_some_and(|b| {
                        (b.x - reach..=b.right() + reach).contains(&c.0) && (b.y..=b.y + b.h * 3 / 5).contains(&c.1)
                    });
                    let f = if let Some(f) = self.pack.behaviour.brain.look_front.as_deref().filter(|_| front) {
                        f
                    } else if c.1 < head_y - (200.0 * sf) as i32 && (c.0 - gx).abs() < (300.0 * sf) as i32 {
                        "idle-look_up"
                    } else if right {
                        "idle-look_right"
                    } else {
                        "idle-look_left"
                    };
                    Step::Hold { frame: f.into(), ms: 1400 }
                }
                Step::Clip(id) if id == "@point" => Step::Clip(if right { "point_right" } else { "point_left" }.into()),
                s => s.clone(),
            })
            .collect()
    }

    pub(super) fn head_point(&self) -> Point<Pixels> {
        let f = self.frame_or_default(self.anim.frame());
        let (x, y) = f.map_or((self.layout.geo.width as i32 / 2, 40), |f| (f.mask.w as i32 / 2, f.head_line / 2));
        let sf = self.layout.sf;
        point(px((self.layout.avatar_x + x) as f32 / sf), px((self.layout.avatar_y + y) as f32 / sf))
    }

    pub(super) fn play_emote(&mut self, e: EmoteKey, now: Instant) {
        let spec = self.pack.behaviour.brain.clone();
        let pick = |v: &Vec<Vec<crate::pack::Step>>, i: usize| v.get(i % v.len().max(1)).cloned();
        let steps = match e {
            EmoteKey::Bid(i) => pick(&spec.bid, i),
            EmoteKey::Insist(s) => pick(&spec.insist, (s as usize).saturating_sub(1)),
            EmoteKey::Satisfied { pat: true } => Some(spec.satisfied.pat.clone()),
            EmoteKey::Satisfied { pat: false } => Some(spec.satisfied.other.clone()),
            EmoteKey::Sulk => Some(spec.sulk.clone()),
            EmoteKey::Wave => Some(spec.wave.clone()),
        }
        .unwrap_or_default();
        let standing = self.standing();
        // Rarely, a headpat that settled a bid gets the air kiss instead.
        let steps = match e {
            EmoteKey::Satisfied { pat: true } if standing => self.maybe_kiss(0, now).unwrap_or(steps),
            _ => steps,
        };
        let fx_kind = match e {
            EmoteKey::Bid(i) => i % 4,
            EmoteKey::Insist(s) => s as usize,
            EmoteKey::Satisfied { .. } => 1,
            EmoteKey::Sulk => 2,
            EmoteKey::Wave => 0,
        };
        let mut played = "fx";
        // The bid marker: up on a bid, a pop on every beat, gone when it's over.
        if let Some(h) = &mut self.brain {
            match e {
                EmoteKey::Bid(_) | EmoteKey::Insist(_) => {
                    let double = matches!(e, EmoteKey::Insist(s) if s >= 2);
                    let (t0, area) = h.marker.map_or((now, None), |m| (m.t0, m.area));
                    h.marker = Some(Marker { t0, pop: now, double, area });
                }
                EmoteKey::Satisfied { .. } | EmoteKey::Sulk => h.marker = None,
                EmoteKey::Wave => {}
            }
        }
        if self.brain.as_ref().and_then(|h| h.marker).is_some_and(|m| m.area.is_none()) {
            let area = self.her_area();
            if let Some(m) = self.brain.as_mut().and_then(|h| h.marker.as_mut()) {
                m.area = area;
            }
        }
        let sulk_sit = e == EmoteKey::Sulk
            && standing
            && self.placement == Placement::Ground
            && self.pack.behaviour.brain.floor_sit.is_some()
            && self.pack.behaviour.brain.sulk_sit.is_some();
        if sulk_sit {
            // Arms crossed, sad, then down onto the floor, and she stays low.
            if let Some(h) = &mut self.brain {
                h.rest = Some(RestPose::FloorSit);
                h.sulk_sit = true;
            }
            self.relayer(now);
            // The sulk sit's own entry (a head-down crouch, no standing beat), else the floor sit's.
            let b = &self.pack.behaviour.brain;
            let entry = b.sulk_sit.as_ref().and_then(|s| s.entry.clone()).or_else(|| b.floor_sit.as_ref().and_then(|s| s.entry.clone())).unwrap_or_default();
            let mut seq = self.pack.flatten(&self.resolve_steps(&steps));
            seq.extend(self.pack.flatten(&entry));
            if !seq.is_empty() {
                self.anim.play(seq, now);
            }
            played = "clip+sit";
        } else if standing && !steps.is_empty() {
            let seq = self.pack.flatten(&self.resolve_steps(&steps));
            if !seq.is_empty() {
                self.anim.play(seq, now);
                played = "clip";
            }
        }
        // Each insist beat: a hop on the floor or a window side, a leg kick from the bar.
        if let EmoteKey::Insist(_) = e {
            let hang = matches!(self.placement, Placement::Pose { frame } if self.brain_pose("hang") == Some(frame) || self.brain_pose("hang_upside_down") == Some(frame));
            if self.placement == Placement::Ground || (self.posed() && !hang) {
                self.hop_or_squash(now);
            }
            if self.posed() {
                let dir = if self.rng.pick(2) == 0 { 1.0 } else { -1.0 };
                if let Some(h) = &mut self.brain {
                    h.leg_kick = Some((now, dir));
                }
            }
        }
        if played == "fx" && self.posed() {
            // Hanging: the emote is a kick of the swing (and hearts for the heart one).
            let (amp, extra) = match e {
                EmoteKey::Bid(i) => ([14.0, 9.0, 7.0, 11.0][i % 4], i % 4),
                EmoteKey::Insist(s) => (if s >= 2 { 18.0 } else { 15.0 }, if s >= 2 { 2 } else { 0 }),
                EmoteKey::Satisfied { .. } => (6.0, 1),
                EmoteKey::Sulk => (-12.0, 2),
                EmoteKey::Wave => (12.0, 0),
            };
            let sign = if self.rng.pick(2) == 0 { 1.0 } else { -1.0 };
            if let Some(h) = &mut self.brain {
                h.kick = Some((now, amp * sign * self.layout.sf));
            }
            self.swing_push(amp * sign);
            match extra {
                1 => self.hearts = Some((now, self.head_point())),
                2 => {
                    self.glitch.get_or_insert(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
                }
                _ => {}
            }
        } else if played == "fx" || matches!(e, EmoteKey::Satisfied { pat: true }) {
            // No standing frames (a floor sit): the emote is the body's effects.
            match fx_kind % 4 {
                0 => {
                    self.squash = Some(now);
                    self.glitch.get_or_insert(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
                }
                1 => self.hearts = Some((now, self.head_point())),
                2 => {
                    self.glitch.get_or_insert(Glitch::new(GlitchKind::Big, now, self.rng.next()));
                }
                _ => self.squash = Some(now),
            }
        }
        if let Some(h) = &mut self.brain {
            let t = h.bnow(now) - h.brain.start();
            h.write(serde_json::json!({ "t": t, "k": "emote", "key": format!("{e:?}"), "how": played }));
        }
    }

    fn brain_hide(&mut self, hide: bool, now: Instant) {
        let cover = if hide {
            let ov = self.brain.as_ref().and_then(|h| h.ov.focus.flatten());
            ov.or_else(|| self.os.active_window()).or_else(|| self.os.focused_window()).filter(|&w| w > 1)
        } else {
            None
        };
        let Some(h) = &mut self.brain else { return };
        // One line each way: a fullscreen hide leaves her as a 1 px shape, easy to take for gone.
        if hide && h.fs == FsHide::Shown {
            let user = if self.parked.is_some() { " (already hidden from her menu)" } else { "" };
            elog!("synaps-avatar: fullscreen-hide: on, a fullscreen window covers her monitor ({}){user}", cover.map_or("unknown".into(), |w| format!("0x{w:x}")));
        } else if !hide && h.fs != FsHide::Shown {
            elog!("synaps-avatar: fullscreen-hide: off, back after {}", h.fs_cover.map_or("a fullscreen window".into(), |w| format!("0x{w:x} left fullscreen")));
        }
        h.fs_cover = if hide { cover.or(h.fs_cover) } else { None };
        let t = h.bnow(now) - h.brain.start();
        h.write(serde_json::json!({ "t": t, "k": if hide { "hide" } else { "show" } }));
        if self.parked.is_some() {
            // Hidden from her menu: the fullscreen hide only keeps its books.
            h.fs = if hide { FsHide::Hidden } else { FsHide::Shown };
            return;
        }
        if hide {
            if h.fs != FsHide::Shown {
                return;
            }
            h.fs = FsHide::Hiding;
            self.menu = None;
            self.fold_reply();
            self.glitch = Some(Glitch::new(GlitchKind::Vanish, now, self.rng.next()));
        } else {
            if h.fs == FsHide::Shown {
                return;
            }
            h.fs = FsHide::Shown;
            self.hidden = false;
            self.glitch = Some(Glitch::new(GlitchKind::Appear, now, self.rng.next()));
        }
    }

    /// Hanging life, device px in 2 px steps: (the pendulum at the bottom, the shins' extra
    /// kick from the knee). A slow sway (when the chain swing is off), a bid's kick decaying
    /// over ~3 s, and one slow leg kick on an insist beat; no kicking at rest.
    /// The bid swing runs: posed (hanging, clinging), there, not carried (SA_BID_SWING=0: off,
    /// the r7 pose-step sway instead).
    pub(super) fn swing_on(&self) -> bool {
        self.posed()
            && self.rag.is_none()
            && !self.hidden
            && self.teleport.is_none()
            && self.loaded
            && self.rag_strength.k() > 0.0
            && std::env::var("SA_BID_SWING").map_or(true, |v| v != "0")
    }

    /// Step the bid swing (create it for a new pose frame). True: redraw.
    pub(super) fn swing_tick(&mut self, now: Instant) -> bool {
        let on = self.swing_on();
        let fi = self.anim.frame();
        if !on {
            return self.brain.as_mut().and_then(|h| h.swing.take()).is_some();
        }
        let sf = self.layout.sf;
        if self.brain.as_ref().and_then(|h| h.swing.as_ref()).is_none_or(|s| s.frame != fi) {
            let Some((top, bottom)) = self.frame_or_default(fi).map(|f| (f.top, f.bottom)) else { return false };
            let grip = (self.anchor_px(fi).1 - self.layout.avatar_y).clamp(top, bottom - 1);
            let p = self.rag_strength.params().scaled(sf);
            let amp = SWING_AMP * self.rag_strength.k() * sf;
            let chain = crate::ragdoll::Chain::new(&p, &[], &crate::ragdoll::Chain::rows(8, (bottom - grip).max(8), false), (0.0, 0.0));
            let side = if self.rng.pick(2) == 0 { 1.0 } else { -1.0 };
            let mut s = BidSwing { sw: crate::ragdoll::PumpedSwing::new(chain, amp, side), frame: fi, grip, last: now, due: now + SWING_TICK, bands: (Vec::new(), Vec::new()) };
            s.bands = swing_bands_pair(&mut s, (top, bottom), self.swing_band_px());
            if let Some(h) = &mut self.brain {
                h.swing = Some(s);
            }
            return true;
        }
        let rows = self.frame_or_default(fi).map(|f| (f.top, f.bottom));
        let bp = self.swing_band_px();
        let Some(s) = self.brain.as_mut().and_then(|h| h.swing.as_mut()) else { return false };
        if now + Duration::from_millis(1) < s.due {
            return false;
        }
        let dt = if super::ragview::lockstep() { SWING_TICK.as_secs_f32() } else { now.saturating_duration_since(s.last).as_secs_f32().min(0.1) };
        s.last = now;
        s.due = now.max(s.due) + SWING_TICK;
        s.sw.step(dt);
        if let Some(rows) = rows {
            s.bands = swing_bands_pair(s, rows, bp);
        }
        true
    }

    fn swing_band_px(&self) -> i32 {
        ((6.0 * self.layout.sf).round() as i32).max(2)
    }

    /// An insist beat or a bid emote: a push on the swing.
    pub(super) fn swing_push(&mut self, px: f32) {
        let sf = self.layout.sf;
        let k = self.rag_strength.k();
        if let Some(s) = self.brain.as_mut().and_then(|h| h.swing.as_mut()) {
            s.sw.push(px * k * sf * 5.0);
        }
    }

    /// The swing's bands now, or one tick ahead (`ahead`: what the next frame will draw).
    pub(super) fn swing_bands(&self, fi: usize, _f: &Frame, ahead: bool) -> Option<Bands> {
        let s = self.brain.as_ref()?.swing.as_ref().filter(|s| s.frame == fi)?;
        let b = if ahead { &s.bands.1 } else { &s.bands.0 };
        (!b.is_empty()).then(|| b.clone())
    }

    pub(super) fn hang_swing(&self, now: Instant) -> (i32, i32, bool) {
        use std::f32::consts::{PI, TAU};
        let sf = self.layout.sf;
        let t = now.saturating_duration_since(self.bob_t0).as_secs_f32();
        let chain = self.brain.as_ref().is_some_and(|h| h.swing.is_some());
        let mut v = if chain { 0.0 } else { 7.0 * sf * (t * TAU / 3.2).sin() };
        if let Some((t0, amp)) = self.brain.as_ref().and_then(|h| h.kick) {
            let k = now.saturating_duration_since(t0).as_secs_f32();
            if k < 3.5 && !chain {
                v += amp * (-k / 1.1).exp() * (k * TAU / 1.25).sin();
            }
        }
        // Legs hang still and follow the swing (LOCKED #9); an insist beat is one slow kick.
        let (mut right, mut leg) = (true, 0.0);
        if let Some((t0, dir)) = self.brain.as_ref().and_then(|h| h.leg_kick) {
            let k = now.saturating_duration_since(t0).as_secs_f32() / LEG_KICK_S;
            if k < 1.0 {
                right = dir > 0.0;
                leg = 24.0 * sf * dir * (PI * k).sin();
            }
        }
        let q = |x: f32| ((x / 2.0).round() * 2.0) as i32;
        (q(v), q(leg), right)
    }

    /// The knee row of a hanging frame (kick from here), or past the bottom (no kick: hanging
    /// by the feet, standing).
    pub(super) fn knee_row(&self, fi: usize, f: &Frame) -> i32 {
        let kick = match self.placement {
            Placement::Pose { frame } if frame == fi => self.brain_pose("hang_upside_down") != Some(fi),
            _ => false,
        };
        if !kick {
            return i32::MAX / 2;
        }
        let vb = self.pack.frames[fi].bounds;
        f.top + (vb.height as f32 * self.layout.geo.scale * KNEE_AT) as i32
    }

    // ---- the bid marker -------------------------------------------------------------------------

    /// The marker's bob (device px) and pop scale at `now`, quantized (what is drawn).
    fn marker_state(&self, now: Instant) -> Option<(i32, i32)> {
        let m = self.brain.as_ref()?.marker?;
        let sf = self.layout.sf;
        let t = now.saturating_duration_since(m.t0).as_secs_f32();
        let bob = ((2.5 * sf * (t * std::f32::consts::TAU / 1.2).sin() / 2.0).round() * 2.0) as i32;
        let p = now.saturating_duration_since(m.pop).as_secs_f32();
        let scale = if p < 0.12 {
            0.45 + 0.8 * p / 0.12
        } else if p < 0.3 {
            1.25 - 0.25 * (p - 0.12) / 0.18
        } else {
            1.0
        };
        Some((bob, (scale * 20.0).round() as i32))
    }

    pub(super) fn marker_next_change(&self, now: Instant) -> Option<Instant> {
        let cur = self.marker_state(now)?;
        let step = Duration::from_millis(33);
        (1..=60).map(|k| now + step * k).find(|&t| self.marker_state(t) != Some(cur)).or(Some(now + Duration::from_secs(2)))
    }

    /// Draw (or take down) the "!" badge by her head: shaped solid-colour layers like the
    /// hearts, so it costs no GPUI redraw.
    pub(super) fn update_marker(&mut self, now: Instant) {
        let show = self.loaded && !self.hidden && !self.quitting && self.teleport.is_none() && self.rag.is_none();
        let st = if show { self.marker_state(now) } else { None };
        let geo = st.and_then(|(bob, sc)| {
            let fi = self.anim.frame();
            let f = self.frame_or_default(fi)?;
            let l = self.layout;
            let sf = l.sf;
            let base = ((self.applied.0 + l.avatar_x) as f32, (self.applied.1 + l.avatar_y) as f32);
            // Upside down her head is at the bottom: by her shoulders instead.
            let ud = self.brain_pose("hang_upside_down").is_some_and(|u| u == fi && self.posed());
            let head = if ud {
                let h = self.pack.frames.get(fi).map_or(300.0, |s| s.bounds.height as f32 * l.geo.scale);
                (base.0 + f.head_x as f32, base.1 + f.top as f32 + 0.5 * h)
            } else {
                (base.0 + f.head_x as f32, base.1 + f.face.0 as f32)
            };
            // Next to her face, never over his window: beside her outline at head height on
            // the side away from it; with no room there, above her head (leaning away); the
            // whole badge (rim, tail, pop) clamped inside her work area, so never cut off.
            let away = self.brain_focused().map_or(1.0, |w| if head.0 < w.center().0 as f32 { -1.0 } else { 1.0 });
            let cols: Vec<usize> = f.profile.iter().enumerate().filter(|(_, y)| **y != i16::MAX).map(|(x, _)| x).collect();
            let (c0, c1) = (base.0 + *cols.first()? as f32, base.0 + *cols.last()? as f32);
            let r = MARK_R * sf * 1.25;
            // Reach from the centre: the popped disc, the rim and the tail.
            let reach = r * 1.55 + 3.0 * sf;
            let gap = 4.0 * sf;
            let area = self.brain.as_ref().and_then(|h| h.marker).and_then(|m| m.area).or_else(|| self.her_area());
            let inside = |x: f32, y: f32| area.is_none_or(|a| x - reach >= a.x as f32 && x + reach <= a.right() as f32 && y - reach >= a.y as f32);
            let beside = (if away > 0.0 { c1 + gap + r } else { c0 - gap - r }, head.1 - 6.0 * sf);
            let top = base.1 + f.top as f32;
            let above = (head.0 + away * 0.8 * r, top - gap - r);
            let mut c = if inside(beside.0, beside.1) || !inside(above.0, above.1) { beside } else { above };
            c.1 += bob as f32;
            if let Some(a) = area {
                c.0 = c.0.clamp(a.x as f32 + reach, (a.right() as f32 - reach).max(a.x as f32 + reach));
                c.1 = c.1.clamp(a.y as f32 + reach, (a.bottom() as f32 - reach).max(a.y as f32 + reach));
            }
            Some((c, head, sc as f32 / 20.0, away))
        });
        let double = self.brain.as_ref().and_then(|h| h.marker).is_some_and(|m| m.double);
        let key = geo.map(|(c, _, sc, _)| (c.0.round() as i32, c.1.round() as i32, (sc * 20.0) as i32, double));
        let Some(h) = &mut self.brain else { return };
        if key == h.marker_shown {
            return;
        }
        h.marker_shown = key;
        let Some((c, head, sc, away)) = geo else {
            for k in 0..3 {
                self.os.fx_unmap(MARK_LAYER + k);
            }
            self.os.flush();
            return;
        };
        let r = MARK_R * self.layout.sf * sc;
        let place = |polys: &[Vec<(f32, f32)>]| {
            let all: Vec<(f32, f32)> = polys.iter().flatten().copied().collect();
            let x0 = all.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
            let y0 = all.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
            let x1 = all.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
            let y1 = all.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
            let mut rows = Vec::new();
            for poly in polys {
                let local: Vec<(f32, f32)> = poly.iter().map(|&(x, y)| (x - x0 as f32, y - y0 as f32)).collect();
                shape::poly_rows(&local, &mut rows);
            }
            ((x0, y0), (x1 - x0, y1 - y0), rows)
        };
        let disc = |rad: f32| -> Vec<(f32, f32)> {
            (0..28).map(|i| {
                let a = i as f32 / 28.0 * std::f32::consts::TAU;
                (c.0 + rad * a.cos(), c.1 + rad * a.sin())
            }).collect()
        };
        // A short tail toward her head.
        let to = ((head.0 - c.0), (head.1 - c.1));
        let len = (to.0 * to.0 + to.1 * to.1).sqrt().max(1.0);
        let (ux, uy) = (to.0 / len, to.1 / len);
        let tail = |rad: f32| -> Vec<(f32, f32)> {
            let (px_, py_) = (-uy, ux);
            vec![
                (c.0 + ux * rad * 0.7 + px_ * rad * 0.42, c.1 + uy * rad * 0.7 + py_ * rad * 0.42),
                (c.0 + ux * rad * 0.7 - px_ * rad * 0.42, c.1 + uy * rad * 0.7 - py_ * rad * 0.42),
                (c.0 + ux * (rad + r * 0.55), c.1 + uy * (rad + r * 0.55)),
            ]
        };
        let edge = 3.0 * self.layout.sf;
        let bang = |cx: f32, w: f32| -> Vec<Vec<(f32, f32)>> {
            let dot: Vec<(f32, f32)> = (0..12)
                .map(|i| {
                    let a = i as f32 / 12.0 * std::f32::consts::TAU;
                    (cx + r * 0.15 * a.cos(), c.1 + r * 0.47 + r * 0.15 * a.sin())
                })
                .collect();
            vec![vec![(cx - w, c.1 - r * 0.62), (cx + w, c.1 - r * 0.62), (cx + w * 0.6, c.1 + r * 0.2), (cx - w * 0.6, c.1 + r * 0.2)], dot]
        };
        let glyph: Vec<Vec<(f32, f32)>> = if double {
            [c.0 - r * 0.27, c.0 + r * 0.27].iter().flat_map(|&x| bang(x, r * 0.15)).collect()
        } else {
            bang(c.0, r * 0.22)
        };
        let _ = away;
        let (o, sz, rows) = place(&glyph);
        self.os.fx_set(MARK_LAYER + 2, MARK_GLYPH, o, sz, &rows).ok();
        let (o, sz, rows) = place(&[disc(r), tail(r)]);
        self.os.fx_set_below(MARK_LAYER + 1, MARK_LAYER + 2, MARK_FILL, o, sz, &rows).ok();
        let (o, sz, rows) = place(&[disc(r + edge), tail(r + edge)]);
        self.os.fx_set_below(MARK_LAYER, MARK_LAYER + 1, MARK_RIM, o, sz, &rows).ok();
        self.os.flush();
    }

    // ---- layer ----------------------------------------------------------------------------------

    /// The brain's part of `layer()`: an approach pose always; a rest pose only while idle or
    /// sleepy (a session mood wins over it).
    pub(super) fn brain_layer(&self, m: Mood) -> Option<(usize, Option<Seq>)> {
        if let Placement::Pose { frame } = self.placement {
            return Some((frame, None));
        }
        let rest = self.brain.as_ref()?.rest?;
        if !matches!(m, Mood::Idle | Mood::Sleepy) || self.placement != Placement::Ground {
            return None;
        }
        let b = &self.pack.behaviour.brain;
        match rest {
            RestPose::FloorSit => {
                let sulky = self.brain.as_ref().is_some_and(|h| h.sulk_sit);
                let fs = if sulky { b.sulk_sit.as_ref().or(b.floor_sit.as_ref())? } else { b.floor_sit.as_ref()? };
                Some((self.pack.index.get(&fs.base).copied()?, self.loop_seq(fs.r#loop.as_ref())))
            }
            RestPose::Nap => Some((self.nap_frame()?, None)),
        }
    }

    // ---- events from the view ----------------------------------------------------------------

    pub(super) fn brain_touch(&mut self, t: Touch) {
        let now = Instant::now();
        // Talking to her wakes her; a pat or a drag doesn't (she sleeps where he puts her).
        if t.wakes() {
            self.wake_up(&format!("{t:?}").to_lowercase(), now);
        }
        if t == Touch::Typing
            && let Some(h) = &mut self.brain
        {
            // Every keystroke isn't news: one a second.
            if h.typed_at.is_some_and(|x| now.duration_since(x) < Duration::from_secs(1)) {
                return;
            }
            h.typed_at = Some(now);
        }
        self.brain_feed(BrainEvent::Interaction(t), now);
    }

    pub(super) fn brain_aborted(&mut self) {
        if let Some(h) = &mut self.brain {
            h.last_abort = Some(Instant::now());
        }
    }

    /// Session changes (from `sync_body`): a new prompt, answered, a finished turn, an error.
    pub(super) fn brain_session(&mut self, prev_prompt: Option<u64>, prompt: Option<u64>, prev_mood: Option<Mood>, mood: Mood) {
        let now = Instant::now();
        let mut sig = Vec::new();
        if prompt != prev_prompt {
            match prompt {
                Some(id) => sig.push(SessionSignal::Prompt(id)),
                None => sig.push(SessionSignal::Cleared),
            }
        }
        if prev_mood.is_some() && prev_mood != Some(mood) {
            match mood {
                Mood::Done => sig.push(SessionSignal::Done),
                Mood::Annoyed => {
                    let own = self.brain.as_ref().and_then(|h| h.last_abort).is_some_and(|t| now.duration_since(t) < Duration::from_secs(3));
                    sig.push(SessionSignal::Error { own_abort: own });
                }
                _ => {}
            }
        }
        for s in sig {
            self.brain_feed(BrainEvent::Session(s), now);
        }
    }

    // ---- commands -----------------------------------------------------------------------------

    pub(super) fn brain_cmd(&mut self, c: BrainCmd, now: Instant) -> String {
        if self.brain.is_none() {
            return "error: no brain".into();
        }
        match c {
            BrainCmd::State => return self.brain_state(now).to_string(),
            BrainCmd::On => self.brain_feed(BrainEvent::Control(Ctl::On), now),
            BrainCmd::Off => self.brain_feed(BrainEvent::Control(Ctl::Off), now),
            BrainCmd::Dnd(None) => self.brain_feed(BrainEvent::Control(Ctl::Undnd), now),
            BrainCmd::Dnd(Some(min)) => self.brain_feed(BrainEvent::Control(Ctl::Dnd(min * 60.0)), now),
            BrainCmd::Scale(k) => {
                if let Some(h) = &mut self.brain {
                    h.base_b = h.bnow(now);
                    h.base_t = now;
                    h.scale = k;
                    h.wake = Some(now);
                }
                self.brain_run(now, true);
            }
            BrainCmd::Seed(s) => {
                if let Some(h) = &mut self.brain {
                    h.rng = Rng::new(s ^ 0x9e37_79b9);
                }
                self.brain_feed(BrainEvent::Control(Ctl::Seed(s)), now);
            }
            BrainCmd::Force(f) => self.brain_feed(BrainEvent::Control(Ctl::Force(f)), now),
            BrainCmd::Event(s) => {
                // A synthetic summon brings her back from HIDE like a real one.
                if matches!(s, SessionSignal::Prompt(_)) && self.parked.is_some() {
                    self.unpark_req = Some("summon");
                }
                self.brain_feed(BrainEvent::Session(s), now)
            }
            BrainCmd::Sense(s) => {
                if let Some(h) = &mut self.brain {
                    let b = h.bnow(now);
                    match s {
                        Sense::Focus(f) => h.ov.focus = f,
                        Sense::Cursor(c) => h.ov.cursor = c,
                        Sense::Idle(i) => h.ov.idle = i.map(|v| (b, v)),
                        Sense::Fullscreen(f) => h.ov.fullscreen = f,
                        Sense::Near(n) => h.ov.near = n,
                        Sense::Arena(a) => h.ov.arena = a.map(|(x, y, w, hh)| WinRect::new(x, y, w, hh)),
                        Sense::Windows(w) => {
                            h.ov.windows = w.map(|ids| ids.into_iter().map(|id| self.os.toplevel_of(id).unwrap_or(id)).collect())
                        }
                        Sense::Real => h.ov = Overrides::default(),
                    }
                    let v = serde_json::json!({ "t": b - h.brain.start(), "k": "sense", "focus": format!("{:?}", h.ov.focus), "cursor": h.ov.cursor,
                                                "idle": h.ov.idle.map(|i| i.1), "fs": h.ov.fullscreen });
                    h.write(v);
                    // Read now (a real change waits for the next poll; a test wants it in order).
                    h.brain.poll_now();
                }
                self.brain_run(now, true);
            }
        }
        "ok".into()
    }

    pub(super) fn settings_reload(&mut self, now: Instant) -> String {
        match Settings::load() {
            Ok(s) => {
                self.rag_strength = crate::ragdoll::Strength(s.ragdoll.strength);
                self.mirror_cfg = s.mirror.clone();
                if self.voice.cfg != s.voice {
                    self.voice.cfg = s.voice.clone();
                    self.voice_config();
                }
                if crate::env_composited().is_none() && self.os.force_composited(s.display.composited) {
                    elog!("synaps-avatar: settings: {} mode", if self.os.composited() { "alpha" } else { "shape" });
                    self.alpha_switched();
                }
                // Her own session's persona, compared (and set) again if it changed.
                if let Some(sl) = &mut self.session {
                    let model = sl.link.cli_model();
                    let acts = if sl.link.rides_other() {
                        Vec::new()
                    } else {
                        sl.link.set_persona(crate::session_persona(&s.session, &s.voice, model, &self.pack.dir), now)
                    };
                    self.run_acts(acts);
                }
                let Some(h) = &mut self.brain else { return format!("ok ragdoll.strength={} (no brain)", s.ragdoll.strength) };
                let mut bs = s.brain.clone();
                // Keep the pack's zeros.
                for (a, w) in bs.approaches.iter_mut() {
                    if h.brain.settings.approaches.iter().any(|(b, bw)| b == a && *bw == 0.0 && h.settings.brain.approaches.iter().any(|(c, cw)| c == a && *cw > 0.0)) {
                        *w = 0.0;
                    }
                }
                let enabled_changed = s.brain.enabled != h.settings.brain.enabled;
                h.brain.tuning = Tuning::new(&bs);
                h.brain.settings = bs;
                h.settings = s.clone();
                if enabled_changed {
                    self.brain_feed(BrainEvent::Control(if s.brain.enabled { Ctl::On } else { Ctl::Off }), now);
                }
                self.brain_run(now, true);
                format!(
                    "ok attention={} fullscreen={:?} ragdoll.strength={} composited={}",
                    s.brain.attention.key(),
                    s.brain.fullscreen,
                    s.ragdoll.strength,
                    self.os.composited()
                )
            }
            Err(e) => format!("error: {e:#} (kept the old settings)"),
        }
    }

    /// The menu's ATTENTION line: MIX → HANG ONLY → OFF, written back to settings.toml.
    pub(super) fn cycle_attention(&mut self) -> Attention {
        let Some(h) = &mut self.brain else { return Attention::Mix };
        let next = h.settings.brain.attention.next();
        h.settings.brain.attention = next;
        h.brain.settings.attention = next;
        if let Err(e) = Settings::save_attention(next) {
            elog!("synaps-avatar: settings.toml: {e:#}");
        }
        let now = Instant::now();
        self.brain_feed(BrainEvent::Interaction(Touch::Menu), now);
        next
    }

    pub(super) fn attention_label(&self) -> Option<&'static str> {
        Some(self.brain.as_ref()?.settings.brain.attention.label())
    }

    pub(super) fn dnd_on(&self) -> bool {
        self.brain.as_ref().is_some_and(|h| h.brain.dnd_left(h.bnow(Instant::now())).is_some())
    }

    pub(super) fn toggle_dnd(&mut self) {
        let now = Instant::now();
        let ctl = if self.dnd_on() { Ctl::Undnd } else { Ctl::Dnd(3600.0) };
        self.brain_feed(BrainEvent::Control(ctl), now);
    }

    fn brain_state(&self, now: Instant) -> serde_json::Value {
        let Some(h) = &self.brain else { return serde_json::Value::Null };
        let b = h.bnow(now);
        let mut v = h.brain.snapshot(b);
        v["clock"] = serde_json::json!({ "t": ((b - h.brain.start()) * 10.0).round() / 10.0, "scale": h.scale,
                                         "wake_in_ms": h.wake.map(|w| w.saturating_duration_since(now).as_millis() as u64) });
        v["view"] = serde_json::json!({
            "placement": format!("{:?}", self.placement),
            "rest": h.rest.map(|r| format!("{r:?}")),
            "spot": h.spot.map(|s| s.kind.key()),
            "moving": h.flight.as_ref().map(|f| (f.spot.kind.key(), f.arrival.key())),
            "fs_hide": format!("{:?}", h.fs),
            "ticks": h.ticks,
            "view_ticks": self.view_ticks,
            "renders": self.renders,
            "moves": self.moves,
            "body": h.watch_box.and_then(|w| w.0).as_ref().map(rect_json),
            "overrides": { "focus": format!("{:?}", h.ov.focus), "cursor": h.ov.cursor, "idle": h.ov.idle.map(|i| i.1),
                           "fullscreen": h.ov.fullscreen, "arena": h.ov.arena.as_ref().map(rect_json), "windows": h.ov.windows },
        });
        v
    }
}

/// `CURSOR_CLEAR` from places.rs (logical px), for the log.
const CURSOR_CLEAR_LOGICAL: f32 = places::CURSOR_CLEAR;


/// At each zero crossing of the feet, push in the direction of travel by what the last
/// half-swing fell short of the target amplitude.
/// The swing's bands now and one tick ahead (a clone stepped one tick: what the next frame
/// draws), both held.
fn swing_bands_pair(s: &mut BidSwing, rows: (i32, i32), bp: i32) -> (Bands, Bands) {
    let now = s.sw.bands(bp, rows.0, rows.1, s.grip);
    let mut ahead = s.sw.clone();
    ahead.step(SWING_TICK.as_secs_f32());
    (now, ahead.bands(bp, rows.0, rows.1, s.grip))
}

/// The ragdoll's flight gravity in device px/s².
fn ragdoll_g(sf: f32) -> f32 {
    crate::ragdoll::G_FLIGHT * sf
}
