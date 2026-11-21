//! Pack loading and validation. A pack is a directory of PNG frames plus JSON:
//! `sprites.json` (the sprite catalog), `poses/poses.json` (docking poses, validated only)
//! and `moods.json` (the pack's behaviour sidecar: mood table, idle activities, click
//! reactions, HUD labels). The code ships no character ids; everything comes from here.

use crate::anim::Seq;
use crate::variants::PoolKey;
use anyhow::{Context as _, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Deserialize, Clone, Debug)]
pub struct Anchor {
    pub name: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, ..*self }
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect { x, y, width: self.right().max(o.right()) - x, height: self.bottom().max(o.bottom()) - y }
    }
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Sprite {
    pub id: String,
    pub file: String,
    #[serde(default)]
    pub kind: String,
    pub anchor: Anchor,
    pub visible_bounds: Rect,
    #[serde(default)]
    pub sha256: Option<String>,
    /// An outfit's sprite only: a face painted on at decode, like an extra frame's (`patch`,
    /// its file from the pack's root).
    #[serde(default)]
    pub patch: Option<Patch>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ClipFrame {
    pub id: String,
    pub ms: u32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Clip {
    pub id: String,
    pub frames: Vec<ClipFrame>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Action {
    pub id: String,
    pub animation: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Autonomy {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "first_ms")]
    pub first_activity_ms: u64,
    #[serde(default = "gap_ms")]
    pub activity_gap_ms: u64,
}

fn yes() -> bool {
    true
}
fn first_ms() -> u64 {
    18_000
}
fn gap_ms() -> u64 {
    12_000
}

impl Default for Autonomy {
    fn default() -> Self {
        Autonomy { enabled: true, first_activity_ms: first_ms(), activity_gap_ms: gap_ms() }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    #[serde(default)]
    pub character: Option<String>,
    pub default_sprite: String,
    pub canvas: Size,
    pub canonical_anchor: Point,
    pub sprites: Vec<Sprite>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub autonomy: Autonomy,
}

#[derive(Deserialize, Debug)]
pub struct PoseCatalog {
    pub sprites: Vec<Sprite>,
}

/// One step of a scripted sequence: a catalog clip id, or one frame held for `ms`; in a one-shot
/// beat, also a middle repeated a few times (`RepeatSteps`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Step {
    Clip(String),
    Hold { frame: String, ms: u32 },
    Repeat(Box<RepeatSteps>),
}

/// The most times a one-shot beat's loop repeats (`repeat: [min, max]`, max at most this).
pub const MAX_REPEAT: u32 = 64;

/// A one-shot beat's `{"entry": [...], "loop": [...], "exit": [...], "repeat": [min, max]}`:
/// the entry once, the loop `min..=max` times (picked each time it plays), the exit once (a
/// hang fidget's rep: a few pull-ups, not one, not forever).
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct RepeatSteps {
    #[serde(default)]
    pub entry: Vec<Step>,
    pub r#loop: Vec<Step>,
    #[serde(default)]
    pub exit: Vec<Step>,
    pub repeat: [u32; 2],
}

impl Step {
    /// The plain steps it plays (a repeat's entry, loop and exit, each once; else itself).
    pub fn leaves(&self) -> Vec<&Step> {
        match self {
            Step::Repeat(r) => r.entry.iter().chain(&r.r#loop).chain(&r.exit).flat_map(Step::leaves).collect(),
            s => vec![s],
        }
    }
}

/// A one-shot beat as written: its steps, or one repeat range (a step of its own).
#[derive(Deserialize)]
#[serde(untagged)]
enum BeatDoc {
    List(Vec<Step>),
    One(Box<RepeatSteps>),
}

impl From<BeatDoc> for Vec<Step> {
    fn from(b: BeatDoc) -> Vec<Step> {
        match b {
            BeatDoc::List(v) => v,
            BeatDoc::One(r) => vec![Step::Repeat(r)],
        }
    }
}

/// A list of one-shot beats (`BeatDoc`s).
fn beats<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Vec<Step>>, D::Error> {
    Ok(Vec::<BeatDoc>::deserialize(d)?.into_iter().map(Into::into).collect())
}

/// Named one-shot beats (`BeatDoc`s).
fn beat_map<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Vec<Step>>, D::Error> {
    Ok(BTreeMap::<String, BeatDoc>::deserialize(d)?.into_iter().map(|(k, b)| (k, b.into())).collect())
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MoodSpec {
    /// Frame shown while the mood holds.
    pub base: String,
    /// Looped over the base (e.g. blink, speech): a catalog clip id or inline steps.
    #[serde(default)]
    pub r#loop: Option<Steps>,
    /// Played once on entering the mood.
    #[serde(default)]
    pub entry: Option<Vec<Step>>,
    /// Played once on leaving the mood (e.g. stand up after sitting).
    #[serde(default)]
    pub exit: Option<Vec<Step>>,
    /// Idle autonomy runs only in moods that allow it.
    #[serde(default)]
    pub autonomy: bool,
    /// Status tag text (upper-case HUD label).
    #[serde(default)]
    pub status: Option<String>,
    /// The state's tone key (online, think, busy, speak, music, alert, done, error, sleep,
    /// offline): its colour (`theme::Pal::tone`, or the pack's `tones`) and its look.
    #[serde(default)]
    pub tone: Option<String>,
    /// The nameplate's silhouette: slant, arrow, flag, square or split (`theme::Plate`).
    #[serde(default)]
    pub plate: Option<String>,
    /// Automatically switch to another mood after `afterMs`.
    #[serde(default)]
    pub then: Option<Then>,
    /// A small hop of the whole figure about every `hopEveryMs` (attention states).
    #[serde(default)]
    pub hop_every_ms: Option<u64>,
    /// A glitch burst about every `glitchEveryMs` while the mood holds (error states).
    #[serde(default)]
    pub glitch_every_ms: Option<u64>,
    /// Draw the status tag inverted (solid tone plate, dark text) for states that want
    /// attention.
    #[serde(default)]
    pub invert: bool,
    /// Small drawn glyph in the status tag: dot, ring, gear, play, check, bang, cross, moon,
    /// dots, note.
    #[serde(default)]
    pub glyph: Option<String>,
    /// Was: a mood change into or out of this mood is a big one (long glitch). Since r42 only a
    /// summon (NEEDS YOU) gets the big burst; still parsed, so older packs load.
    #[serde(default)]
    #[allow(dead_code)]
    pub big: bool,
}

/// A looped list: a clip id, steps written inline (all of it loops), or steps with a way in and
/// out, `{"entry": [...], "loop": [...], "exit": [...]}` (`LoopSteps`).
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Steps {
    Clip(String),
    Inline(Vec<Step>),
    Ranged(LoopSteps),
}

impl Default for Steps {
    fn default() -> Self {
        Steps::Inline(Vec::new())
    }
}

/// A looped list's range: `entry` plays once on entry, `loop` repeats while the loop holds,
/// `exit` plays once when it ends in place (not when a drag, a teleport or sleep cuts it).
#[derive(Deserialize, Clone, Debug, Default)]
pub struct LoopSteps {
    #[serde(default)]
    pub entry: Vec<Step>,
    #[serde(default)]
    pub r#loop: Vec<Step>,
    #[serde(default)]
    pub exit: Vec<Step>,
}

impl Steps {
    /// Every step, in the order they first play (entry, loop, exit).
    pub fn as_steps(&self) -> Vec<Step> {
        self.parts().concat()
    }

    /// Entry, loop, exit (a plain list: all loop).
    pub fn parts(&self) -> [Vec<Step>; 3] {
        match self {
            Steps::Clip(id) => [Vec::new(), vec![Step::Clip(id.clone())], Vec::new()],
            Steps::Inline(v) => [Vec::new(), v.clone(), Vec::new()],
            Steps::Ranged(r) => [r.entry.clone(), r.r#loop.clone(), r.exit.clone()],
        }
    }
}

/// Where moods.json takes a loop range: its looped lists (`Steps`), and (with `repeat`) its
/// one-shot beats.
const LOOPED_KEYS: &str = "a mood's `loop`, `energy.<band>.idle.loop`, `brain.floorSit.loop`, `brain.sulkSit.loop` or `music.windowLoop`";
const BEAT_KEYS: &str = "`activities`, `life.fidgets`, `life.poses`, `energy.<band>.fidgets`/`activities`, `brain.hangFidgets` or `perch.activities`";

fn looped_path(p: &[&str]) -> bool {
    matches!(p, ["moods", _, "loop"] | ["energy", _, "idle", "loop"] | ["brain", "floorSit" | "sulkSit", "loop"] | ["music", "windowLoop"])
}

/// A one-shot beat (`beats`, `beat_map`), or a step of one.
fn beat_path(p: &[&str]) -> bool {
    let slot = |p: &[&str]| {
        matches!(
            p,
            ["activities", _] | ["life", "fidgets" | "poses", _] | ["energy", _, "fidgets" | "activities", _] | ["brain", "hangFidgets", _] | ["perch", "activities", _]
        )
    };
    slot(p) || p.split_last().is_some_and(|(i, rest)| i.parse::<usize>().is_ok() && slot(rest))
}

/// A loop range where it can't go, or with a key it doesn't know, read off the raw moods.json
/// (the typed parse would only say "invalid type" or "did not match any variant"). A range is an
/// object of nothing but `entry`, `loop`, `exit` and `repeat` (a mood has its `base`).
fn loop_range_errors(v: &serde_json::Value, path: &mut Vec<String>, errors: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(o) => {
            let p: Vec<&str> = path.iter().map(String::as_str).collect();
            let at = if path.is_empty() { "(top)".to_string() } else { path.join(".") };
            let unknown = |allowed: &[&str], errors: &mut Vec<String>| {
                for k in o.keys().filter(|k| !allowed.contains(&k.as_str())) {
                    errors.push(format!("moods.json {at}: unknown key `{k}` in a loop range ({})", allowed.join("|")));
                }
            };
            if looped_path(&p) {
                if o.contains_key("repeat") {
                    errors.push(format!("moods.json {at}: `repeat` is for one-shot beats ({BEAT_KEYS}); a looped list repeats while it holds"));
                }
                unknown(&["entry", "loop", "exit", "repeat"], errors);
            } else if beat_path(&p) && (o.contains_key("loop") || o.contains_key("repeat")) {
                unknown(&["entry", "loop", "exit", "repeat"], errors);
                match o.get("repeat") {
                    None => errors.push(format!("moods.json {at}: a one-shot beat's loop range needs `repeat`: [min, max] (it plays once, not forever)")),
                    Some(r) if repeat_range(r).is_none() => errors.push(format!(
                        "moods.json {at}: `repeat` is {r}; it takes [min, max], whole numbers with 1 <= min <= max <= {MAX_REPEAT}"
                    )),
                    Some(_) => {}
                }
            } else if !o.is_empty() && o.keys().all(|k| matches!(k.as_str(), "entry" | "loop" | "exit" | "repeat")) {
                errors.push(format!("moods.json {at}: a loop range only goes on a looped list ({LOOPED_KEYS}) or, with `repeat`, a one-shot beat ({BEAT_KEYS})"));
            }
            for (k, x) in o {
                path.push(k.clone());
                loop_range_errors(x, path, errors);
                path.pop();
            }
        }
        serde_json::Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                path.push(format!("{i}"));
                loop_range_errors(x, path, errors);
                path.pop();
            }
        }
        _ => {}
    }
}

/// A `repeat` as written: `[min, max]`, 1 <= min <= max <= `MAX_REPEAT`.
fn repeat_range(v: &serde_json::Value) -> Option<[u32; 2]> {
    let a = v.as_array().filter(|a| a.len() == 2)?;
    let n = |x: &serde_json::Value| x.as_u64().filter(|&n| n <= u64::from(MAX_REPEAT)).map(|n| n as u32);
    let (lo, hi) = (n(&a[0])?, n(&a[1])?);
    (1..=hi).contains(&lo).then_some([lo, hi])
}

/// A looped list's range is well formed: something to loop between its entry and exit.
fn check_loop_range(what: &str, l: &Steps, errors: &mut Vec<String>) {
    if let Steps::Ranged(r) = l
        && r.r#loop.is_empty()
    {
        errors.push(format!("moods.json {what}: the loop range's `loop` is empty (entry and exit need steps to loop between)"));
    }
}

/// Idle life tuning. Everything here is cheap: breath and sway redraw only when their
/// rounded px value changes, sparkles at a low rate only while one is alive.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Life {
    /// Breathing: everything above the waist rises this many device px (0 = off), and its period.
    pub bob_px: i32,
    pub bob_period_ms: u64,
    /// Hair sway: the top of the figure shears this many device px each way (0 = off).
    pub sway_px: i32,
    pub sway_period_ms: u64,
    /// Glints on the upper part of the figure (the hair): a new one every
    /// `sparkleEveryMs[0]..[1]` ms, each alive `sparkleMs`.
    pub sparkle_every_ms: [u64; 2],
    pub sparkle_ms: u64,
    /// Override the catalog's autonomy timing.
    pub first_activity_ms: Option<u64>,
    pub activity_gap_ms: Option<u64>,
    /// Her micro-fidgets: short beats in place (a blink, a look, a mouth, a wink, a sleepy
    /// blink), a rotation pool the restless idle draws from every few seconds, never the same
    /// twice running. Empty: the activities' face beats.
    #[serde(deserialize_with = "beats")]
    pub fidgets: Vec<Vec<Step>>,
    /// Her bigger idle poses by name (`lean`, `sit_floor`, ...): in, a hold, out, in place. The
    /// restless idle reaches for them as she gets bored. Empty: the activities' gestures.
    #[serde(deserialize_with = "beat_map")]
    pub poses: BTreeMap<String, Vec<Step>>,
}

impl Default for Life {
    fn default() -> Self {
        Life {
            bob_px: 2,
            bob_period_ms: 3_600,
            sway_px: 3,
            sway_period_ms: 5_200,
            sparkle_every_ms: [1_300, 3_000],
            sparkle_ms: 560,
            first_activity_ms: None,
            activity_gap_ms: None,
            fidgets: Vec::new(),
            poses: BTreeMap::new(),
        }
    }
}

/// Sitting on another window's top edge: the seat anchor of `base` goes on the edge.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PerchSpec {
    pub base: String,
    /// Seat frame per mood key; moods not listed use `base`.
    #[serde(default)]
    pub moods: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "beats")]
    pub activities: Vec<Vec<Step>>,
    #[serde(default)]
    pub click: Vec<Step>,
    /// A headpat while perched (seat-anchored frames): her perch frame with the headpat face.
    #[serde(default)]
    pub headpat: Vec<Step>,
    /// A clean 1 px rim around the dark limbs of perch frames below the seat line, drawn
    /// at decode time behind the art, so they stay visible over dark windows
    /// (`#rrggbb`; empty = off).
    #[serde(default)]
    pub rim: String,
    /// Contact sliver under the seat and thighs: thickness in logical px (0 = off), colour.
    #[serde(default = "sliver_px_default")]
    pub sliver_px: f32,
    #[serde(default = "sliver_rgb_default")]
    pub sliver: String,
}

fn sliver_px_default() -> f32 {
    4.0
}
fn sliver_rgb_default() -> String {
    "#05060a".into()
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Then {
    pub mood: String,
    pub after_ms: u64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtraFrame {
    pub id: String,
    pub file: String,
    /// `effect`: an overlay (`click.hearts`), like a catalog sprite of that kind: not a frame.
    #[serde(default)]
    pub kind: Option<String>,
    /// Borrow anchor and visible bounds from this catalog sprite (same registration)…
    #[serde(default)]
    pub like: Option<String>,
    /// …or give them here.
    #[serde(default)]
    pub anchor: Option<Anchor>,
    #[serde(default)]
    pub visible_bounds: Option<Rect>,
    #[serde(default)]
    pub sha256: Option<String>,
    /// Paint another file's face onto this frame (canvas px): the pixels inside `rect` of
    /// `file` that belong to her face, moved by (`dx`, `dy`). See `frames::face_patch`.
    #[serde(default)]
    pub patch: Option<Patch>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub file: String,
    pub rect: Rect,
    #[serde(default)]
    pub dx: i32,
    #[serde(default)]
    pub dy: i32,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Arrival sequences (CapriCore's intro-v1 frames, PROPOSAL-assets §4.1), played as she
/// reappears: at launch after the plain red bolt, on a $mod+c summon, on some teleports, on
/// a window top. All optional.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ArriveSpec {
    pub launch: Vec<Step>,
    /// After the cinematic close-up (it was the greeting): one landing, then her idle. Empty:
    /// `launch`.
    pub cinematic: Vec<Step>,
    pub summon: Vec<Step>,
    pub teleport: Vec<Step>,
    /// Onto a window top (seat-anchored frames only).
    pub perch: Vec<Step>,
    /// A dip at the touchdown: the squash starts this long (ms) before the change to her
    /// sitting pose (0: none).
    pub perch_dip_ms: u32,
    /// The chance a passive or bid teleport lands with `teleport` / `perch` when the last one
    /// didn't (never twice running: 0.5 gives 1 in 3).
    pub teleport_chance: f32,
    /// Other drawings of an arrival, by key (`launch`, `cinematic`, `summon`, `teleport`,
    /// `perch`): each arrival picks among the key's own steps and these, never the same twice
    /// running. Empty: always the key's own.
    pub alt: BTreeMap<String, Vec<Vec<Step>>>,
}

/// The rare air kiss after a headpat: its steps, the chance per headpat, the hearts' start
/// into the steps, and the least time between two.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KissSpec {
    pub steps: Vec<Step>,
    pub chance: f32,
    pub hearts_at_ms: u64,
    pub gap_ms: u64,
}

/// Music mode: a headphones pose (floor and window-edge variants), note sprites drifting up
/// from emitters on the beat, a head-bob at a fixed tempo. No audio is read.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MusicSpec {
    pub floor: String,
    #[serde(default)]
    pub window: Option<String>,
    /// On a window top: these seat-anchored steps loop instead of the `window` frame (a perched
    /// music loop, typically a strip; a loop range takes an entry and an exit). Empty, not
    /// worn, or its cells not in yet: `window`.
    #[serde(default)]
    pub window_loop: Steps,
    #[serde(default = "bpm_default")]
    pub bpm: f32,
    /// Note images (any size, square), used in turn.
    pub notes: Vec<String>,
    /// Note emitters per frame id, canonical canvas px of that frame.
    #[serde(default)]
    pub emitters: BTreeMap<String, Vec<[i32; 2]>>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub plate: Option<String>,
    #[serde(default)]
    pub glyph: Option<String>,
    /// Autonomy may start a session: chance per activity pick, and how long it lasts.
    #[serde(default)]
    pub activity_chance: f32,
    #[serde(default = "music_ms_default")]
    pub activity_ms: u64,
}

fn bpm_default() -> f32 {
    110.0
}
fn music_ms_default() -> u64 {
    24_000
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClickSpec {
    #[serde(default)]
    pub head: Vec<Step>,
    #[serde(default)]
    pub body: Vec<Vec<Step>>,
    #[serde(default)]
    pub annoyed: Vec<Step>,
    /// Rapid clicks (each within `streakMs` of the last) before `annoyed` plays.
    #[serde(default = "annoyed_after")]
    pub annoyed_after: u32,
    #[serde(default = "streak_ms")]
    pub streak_ms: u64,
    /// Fraction of the frame's visible height counted as the head, from the top.
    #[serde(default = "head_fraction")]
    pub head_fraction: f32,
    /// The hearts (a headpat, the kiss, a happy emote) drawn from this effect sprite (a catalog
    /// sprite or extra frame of kind `effect`): its outline, filled in its colour over a white
    /// rim. Absent: the drawn hearts.
    #[serde(default)]
    pub hearts: Option<String>,
}

fn annoyed_after() -> u32 {
    5
}
fn streak_ms() -> u64 {
    1_600
}
fn head_fraction() -> f32 {
    0.3
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Behaviour {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub font_files: Vec<String>,
    #[serde(default)]
    pub moods: BTreeMap<String, MoodSpec>,
    #[serde(default, deserialize_with = "beats")]
    pub activities: Vec<Vec<Step>>,
    /// Her energy's bands (BRIEF-alive S11): their own activities, gaps and idle; absent, the
    /// defaults (`activities`, `life.activityGapMs`, the idle mood).
    #[serde(default)]
    pub energy: EnergySpec,
    #[serde(default)]
    pub extra_frames: Vec<ExtraFrame>,
    #[serde(default)]
    pub click: ClickSpec,
    #[serde(default)]
    pub tones: BTreeMap<String, String>,
    #[serde(default)]
    pub life: Life,
    #[serde(default)]
    pub perch: Option<PerchSpec>,
    /// The way the art faces unmirrored: "right" (default) or "left" (Capri: her hair sweeps
    /// back to the right). Every facing decision goes through it (`art_faces_right`).
    #[serde(default, alias = "facing")]
    pub faces: Option<String>,
    #[serde(default)]
    pub music: Option<MusicSpec>,
    #[serde(default)]
    pub brain: BrainSpec,
    /// The ragdoll (BRIEF-ragdoll §2.4): held, flung, landing frames. All optional.
    #[serde(default)]
    pub drag: Option<DragSpec>,
    #[serde(default)]
    pub fling: Option<FlingSpec>,
    #[serde(default)]
    pub land: Option<LandSpec>,
    #[serde(default)]
    pub arrive: Option<ArriveSpec>,
    /// Her icon (a square PNG in the pack) for her windows' `_NET_WM_ICON`, where his own isn't
    /// there (`crate::brand::icon`).
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub kiss: Option<KissSpec>,
    /// Variant pools: more than one art option per state (`crate::variants`).
    #[serde(default)]
    pub variants: Option<crate::variants::VariantsSpec>,
    /// Drawn in-betweens from the desktop art (`anim/`), by change (`TRANSITIONS`). Without
    /// `anim/` they don't exist and the pack's own frames play.
    #[serde(default)]
    pub transitions: BTreeMap<String, String>,
    /// How often a drawn floor arrival starts from the `form` bloom (0..1).
    #[serde(default)]
    pub form_chance: f32,
    /// The desktop art's motions (`MotionsSpec`). Without `anim/` none of them exists.
    #[serde(default)]
    pub motions: MotionsSpec,
    /// Cut-ins beside her for big moments (`CutInSpec`): local images only (`anim/mochi`).
    #[serde(default)]
    pub cutins: Option<CutInSpec>,
    /// Her face in the Line per mood (`moods` names to `anim/mochi` images; local only). A mood
    /// without one, or a pack without the image, keeps the portrait (lachy's face.png).
    #[serde(default)]
    pub line_faces: BTreeMap<String, LineFace>,
}

/// A face for the Line: an `anim/mochi` stem, and the square of it that is her face (eyes and
/// mouth: `centre` and `size`, source px). Without them, the whole image.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum LineFace {
    Whole(String),
    Crop { image: String, centre: (u32, u32), size: u32 },
}

impl LineFace {
    pub fn image(&self) -> &str {
        match self {
            LineFace::Whole(s) | LineFace::Crop { image: s, .. } => s,
        }
    }

    /// (centre, size) in source px.
    pub fn crop(&self) -> Option<((u32, u32), u32)> {
        match *self {
            LineFace::Whole(_) => None,
            LineFace::Crop { centre, size, .. } => Some((centre, size)),
        }
    }
}

/// `moods.json` `cutins`: a show still beside her for a beat (`anim/mochi` stems by beat:
/// `needsYou`, `done`, `laugh`, `summon`, `annoyed`). A beat without its image doesn't show.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CutInSpec {
    pub beats: BTreeMap<String, String>,
    /// On screen this long, its entrance and exit included.
    #[serde(default = "cutin_ms")]
    pub ms: u64,
    /// At most one per this long.
    #[serde(default = "cutin_gap")]
    pub gap_ms: u64,
    /// `done` only after a turn at least this long.
    #[serde(default = "cutin_done_after")]
    pub done_after_ms: u64,
    /// `laugh`: the share of her laugh reactions to a click that get it.
    #[serde(default = "cutin_laugh")]
    pub laugh_chance: f32,
    /// Its height, logical px.
    #[serde(default = "cutin_h")]
    pub height: f32,
}

fn cutin_ms() -> u64 {
    1200
}
fn cutin_gap() -> u64 {
    12_000
}
fn cutin_done_after() -> u64 {
    120_000
}
fn cutin_laugh() -> f32 {
    0.34
}
fn cutin_h() -> f32 {
    150.0
}

/// `moods.json` `motions` (B2c): lachy's motion strips and how often they may play. Each one is
/// rate-capped (`gapMs`) and each falls back to what she did before (a hop-scoot, the
/// headphones, the ragdoll, nothing) when it can't.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MotionsSpec {
    pub walk: Option<WalkSpec>,
    pub dance: Option<DanceSpec>,
    pub dangle: Option<DangleSpec>,
    pub click: Option<ClickFxSpec>,
}

/// Short moves walked instead of hop-scooted (the floor) or leapt (along a window top).
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WalkSpec {
    pub strip: String,
    /// Chance a move that may be walked is.
    #[serde(default = "half")]
    pub chance: f32,
    /// Logical px per second.
    #[serde(default = "walk_pace")]
    pub px_per_s: f32,
    /// Shorter moves hop (logical px).
    #[serde(default = "walk_min")]
    pub min_px: f32,
    #[serde(default = "walk_gap")]
    pub gap_ms: u64,
    /// Along a window top too (where there's room to stand).
    #[serde(default = "yes")]
    pub edges: bool,
}

/// Music on the floor with energy: she dances a while, then the headphones.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DanceSpec {
    pub strip: String,
    /// The tempo with energy (a player's `xesam:audioBPM`).
    #[serde(default = "dance_bpm")]
    pub min_bpm: f32,
    /// Without a tempo: the chance a song gets a dance.
    #[serde(default = "half")]
    pub chance: f32,
    /// How long a dance lasts (ms), then the headphones for the rest.
    #[serde(default = "dance_ms")]
    pub ms: [u64; 2],
    /// The least time from one dance's start to the next.
    #[serde(default = "dance_gap")]
    pub gap_ms: u64,
}

/// Carried by the head: now and then she dangles by her hair instead of the ragdoll's poses.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DangleSpec {
    pub strip: String,
    #[serde(default = "half")]
    pub chance: f32,
    /// The grab's own frames play this long first.
    #[serde(default = "dangle_after")]
    pub after_ms: u32,
    #[serde(default = "dangle_gap")]
    pub gap_ms: u64,
}

/// A click on her: the burst under the pointer.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClickFxSpec {
    pub strip: String,
    #[serde(default = "click_gap")]
    pub gap_ms: u64,
}

fn half() -> f32 {
    0.5
}
fn walk_pace() -> f32 {
    70.0
}
fn walk_min() -> f32 {
    90.0
}
fn walk_gap() -> u64 {
    30_000
}
fn dance_bpm() -> f32 {
    100.0
}
fn dance_ms() -> [u64; 2] {
    [40_000, 90_000]
}
fn dance_gap() -> u64 {
    600_000
}
fn dangle_after() -> u32 {
    260
}
fn dangle_gap() -> u64 {
    20_000
}
fn click_gap() -> u64 {
    250
}

/// `motions`, resolved: strip indices into `anim` (the click: an effect's name).
#[derive(Clone, Debug, Default)]
pub struct Motions {
    pub walk: Option<usize>,
    pub dance: Option<usize>,
    pub dangle: Option<usize>,
    pub click: Option<String>,
}

impl Motions {
    fn strips(&self) -> impl Iterator<Item = usize> + '_ {
        [self.walk, self.dance, self.dangle].into_iter().flatten()
    }
}

/// `moods.json` `motions` -> strips: walk and dance loop on her feet, dangle loops from its grip,
/// the click is an effect. A pack without `anim/` has none.
fn resolve_motions(b: &Behaviour, anim: Option<&crate::art::Anim>, errors: &mut Vec<String>) -> Motions {
    let mut out = Motions::default();
    let Some(a) = anim else { return out };
    let m = &b.motions;
    let mut strip = |key: &str, name: Option<&String>, want: &str| -> Option<usize> {
        let name = name?;
        match a.by_name.get(name).map(|&i| (i, &a.strips[i])) {
            Some((i, s)) if s.anchor.0 == want && s.looping => Some(i),
            Some(_) => {
                errors.push(format!("moods.json motions.{key}: strip `{name}` needs a {want} anchor and to loop"));
                None
            }
            None => {
                errors.push(format!("moods.json motions.{key}: strip `{name}` is not in anim/anim.json"));
                None
            }
        }
    };
    out.walk = strip("walk", m.walk.as_ref().map(|w| &w.strip), "ground");
    out.dance = strip("dance", m.dance.as_ref().map(|d| &d.strip), "ground");
    out.dangle = strip("dangle", m.dangle.as_ref().map(|d| &d.strip), "grip");
    if let Some(c) = &m.click {
        if a.effects.contains_key(&c.strip) {
            out.click = Some(c.strip.clone());
        } else {
            errors.push(format!("moods.json motions.click: `{}` is not an effect strip (family fx, no anchor) in anim/anim.json", c.strip));
        }
    }
    out
}

/// The transitions a pack may name, and the anchor each needs.
/// `perchLeave`: off a window top into the bolt; `floorArrive` / `floorLeave`: a teleport's
/// landing on / leaving from the floor; `wake`: up from her sleep on the floor; `form`: the rare
/// bloom she forms from before `floorArrive` (`formChance`).
pub const TRANSITIONS: [(&str, &str); 5] = [("perchLeave", "seat"), ("floorArrive", "ground"), ("floorLeave", "ground"), ("wake", "ground"), ("form", "ground")];

impl Behaviour {
    /// The art faces right unmirrored (`faces`, default right).
    pub fn art_faces_right(&self) -> bool {
        !self.faces.as_deref().is_some_and(|f| f.trim().eq_ignore_ascii_case("left"))
    }
}

/// While held (BRIEF-ragdoll §2.2).
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DragSpec {
    pub grab: Vec<Step>,
    pub hang: Option<String>,
    pub still: Vec<StillStep>,
    pub swing: Vec<Step>,
    pub shake: Option<String>,
    pub dizzy: Vec<Step>,
    /// Held by the legs: upside down (a grip anchor, not ground/seat).
    pub ankle: Option<String>,
    /// Head-zone variant: hands up gripping the cursor (off by default).
    pub cling: Option<String>,
    /// Grip zones by height: head below zones[0], torso below zones[1], legs after.
    pub zones: Option<[f32; 2]>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StillStep {
    pub after_ms: u32,
    pub frame: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FlingSpec {
    pub launch: Vec<Step>,
    pub fly: Option<String>,
    pub fall: Option<String>,
    pub bounce: Vec<Step>,
    pub tuck: Vec<Step>,
    pub annoyed_after: Option<u32>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LandSpec {
    pub soft: Vec<Step>,
    pub hard: Vec<Step>,
    /// Seat-anchored frames only.
    pub perch: Vec<Step>,
    pub hard_glitch: Option<String>,
}

/// What the brain plays (BRIEF-brain §6.4, §14.1). Steps may use the clip ids `@look` and
/// `@point`: the view turns them toward the cursor (`idle-look_left/right/up`, `point_left/right`).
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BrainSpec {
    pub bid: Vec<Vec<Step>>,
    pub insist: Vec<Vec<Step>>,
    pub satisfied: SatisfiedSpec,
    pub sulk: Vec<Step>,
    pub wave: Vec<Step>,
    /// He's back (an unlock, a long absence): this; the first of a morning `greetMorning`.
    /// Empty: the wave.
    pub greet: Vec<Step>,
    pub greet_morning: Vec<Step>,
    /// Up after a long nap (`stretchAfterMs`, brain time): this inside the stand-up, on the
    /// floor only, at most 4 an hour.
    pub stretch: Vec<Step>,
    pub stretch_after_ms: Option<u64>,
    pub floor_sit: Option<MoodSpec>,
    /// After a sulk she sits down where she landed and stays low: this base and loop.
    pub sulk_sit: Option<MoodSpec>,
    pub nap: Option<String>,
    /// After `nap_deep_after_ms` of the nap: asleep on her side (a soft glitch into it), on the
    /// floor `nap_deep`, on a window top `nap_perch` (seat-anchored) if the top is long
    /// enough. Waking from it: a soft glitch and `nap_wake` (floor) / the perch base.
    pub nap_deep: Option<String>,
    pub nap_deep_after_ms: Option<u64>,
    pub nap_perch: Option<String>,
    pub nap_wake: Vec<Step>,
    pub hop_crouch: Option<String>,
    /// The crouch before a big leap (up or far) and in a startle.
    pub big_hop_crouch: Option<String>,
    /// A startle in place (the pointer swiped past her).
    pub startle: Vec<Step>,
    /// The pointer still beside her (BRIEF-alive S1): a lean toward it, then a look
    /// (`@lookAt`). Its frames are drawn facing the pointer.
    pub curious: Vec<Step>,
    /// A wiggle near her (S2): a crouch, a paw-bat (`@point`), a smirk.
    pub bat: Vec<Step>,
    /// Landed beside the pointer she pounced at: "missed!".
    pub missed: Vec<Step>,
    /// Patted through a sulk or its cold shoulder (S14).
    pub make_up: Vec<Step>,
    /// The cold shoulder's start (after the sulk's own chain): her face, her back to him.
    pub cold: Vec<Step>,
    /// Up out of a nap with a greeting due: a quicker get-up than `napWake`.
    pub nap_wake_greet: Vec<Step>,
    /// The pointer right in front of her (or on her): this frame instead of a side look.
    pub look_front: Option<String>,
    /// Her tag through a sulk and its cold shoulder (r42: not a cheerful ONLINE): a status
    /// word, tone and glyph, like a mood's.
    pub sulk_status: Option<SulkStatus>,
    /// Approach → frame: `hang`, `hang_upside_down`, `cling_left`, `cling_right`, `peek`. A
    /// missing one gives that approach weight 0.
    pub poses: BTreeMap<String, String>,
    /// Hanging from a top bar or a window (`poses.hang`): fidgets by name (`pullup`, `catch`,
    /// `wrist_glance`, ...), `ceiling`-anchored frames or strips, registered on the hang pose's
    /// grip. Empty: the hang's swing alone, as before.
    #[serde(deserialize_with = "beat_map")]
    pub hang_fidgets: BTreeMap<String, Vec<Step>>,
    /// A glance (the pointer coming by, a window gaining focus), then back: steps,
    /// `@look` turning toward it. Empty: `@look` alone.
    pub glance: Vec<Step>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SulkStatus {
    pub status: String,
    pub tone: Option<String>,
    pub glyph: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SatisfiedSpec {
    pub pat: Vec<Step>,
    pub other: Vec<Step>,
}

/// `moods.json` `variants` → the pools, resolved against `anim/`. A strip variant needs the pack's
/// desktop art: without it the variant is dropped, so an old pack's pool is its own art alone.
fn resolve_pools(b: &Behaviour, anim: Option<&crate::art::Anim>, errors: &mut Vec<String>) -> HashMap<PoolKey, Pool> {
    let mut out = HashMap::new();
    let Some(spec) = &b.variants else { return out };
    for (key, ps) in &spec.pools {
        let Some(k) = PoolKey::parse(key) else {
            errors.push(format!("moods.json variants.pools: unknown pool `{key}` (stand|perch|sleepFloor|sleepWindow)"));
            continue;
        };
        // A window top takes seat strips, the floor standing ones.
        let want = if matches!(k, PoolKey::SleepFloor | PoolKey::Stand) { "ground" } else { "seat" };
        let mut pool = Pool { shift_ms: ps.shift_ms, shift_stays: ps.shift_stays, bridge: None, bridge_ms: ps.bridge_ms.unwrap_or([4000, 10000]), variants: Vec::new() };
        for v in &ps.variants {
            let at = format!("moods.json variants.pools.{key} `{}`", v.id);
            if v.weight.is_nan() || v.weight < 0.0 || pool.variants.iter().any(|x: &Variant| x.id == v.id) {
                errors.push(format!("{at}: weight {} (>= 0) and ids unique", v.weight));
                continue;
            }
            let art = match (&v.strip, anim) {
                (None, _) => None,
                (Some(_), None) => continue,
                (Some(base), Some(a)) => {
                    let mut get = |what: &str, name: &str| -> Option<usize> {
                        match a.by_name.get(name) {
                            Some(&i) if a.strips[i].anchor.0 == want => Some(i),
                            Some(_) => {
                                errors.push(format!("{at}: {what} strip `{name}` needs a {want} anchor"));
                                None
                            }
                            None => {
                                errors.push(format!("{at}: {what} strip `{name}` is not in anim/anim.json"));
                                None
                            }
                        }
                    };
                    let Some(bi) = get("strip", base) else { continue };
                    let talk = v.talk.as_deref().and_then(|t| get("talk", t));
                    let music = v.music.as_deref().and_then(|t| get("music", t));
                    let blinks = v.blinks.iter().filter_map(|w| Some((get("blink", &w.strip)?, w.weight))).collect();
                    for si in std::iter::once(bi).chain(talk).chain(music) {
                        if !a.strips[si].looping {
                            errors.push(format!("{at}: strip `{}` is a one-shot, not a loop", a.strips[si].name));
                        }
                    }
                    Some(VariantArt { base: bi, talk, music, blinks, blink_ms: v.blink_ms.unwrap_or([3000, 6000]), every: v.every.unwrap_or(1).max(1), hold_ms: v.hold_ms })
                }
            };
            let energy = v
                .energy
                .iter()
                .filter_map(|b| {
                    let band = crate::life::Band::parse(b);
                    if band.is_none() {
                        errors.push(format!("moods.json variants.pools.{key}: variant `{}` energy `{b}` (low|mid|high)", v.id));
                    }
                    band
                })
                .collect();
            pool.variants.push(Variant { id: v.id.clone(), weight: v.weight, lying: v.lying, after_ms: v.after_ms, stay_ms: v.stay_ms, energy, art });
        }
        if let Some(b) = &ps.bridge {
            match pool.by_id(b) {
                Some(i) if pool.variants[i].art.is_some() => pool.bridge = Some(i),
                Some(_) => errors.push(format!("moods.json variants.pools.{key}: bridge `{b}` is the pack's own art")),
                None if anim.is_some() => errors.push(format!("moods.json variants.pools.{key}: bridge `{b}` is not a variant")),
                None => {}
            }
        }
        if !pool.variants.is_empty() {
            out.insert(k, pool);
        }
    }
    out
}

/// `moods.json` `transitions` → strip indices. Each must be a one-shot with the anchor its change
/// needs. A pack without `anim/` has none.
fn resolve_transitions(b: &Behaviour, anim: Option<&crate::art::Anim>, errors: &mut Vec<String>) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for (key, name) in &b.transitions {
        let Some(&(_, want)) = TRANSITIONS.iter().find(|t| t.0 == key) else {
            let known: Vec<&str> = TRANSITIONS.iter().map(|t| t.0).collect();
            errors.push(format!("moods.json transitions: unknown change `{key}` ({})", known.join("|")));
            continue;
        };
        let Some(a) = anim else { continue };
        match a.by_name.get(name).map(|&i| (i, &a.strips[i])) {
            Some((i, s)) if s.anchor.0 == want && !s.looping => {
                out.insert(key.clone(), i);
            }
            Some(_) => errors.push(format!("moods.json transitions.{key}: strip `{name}` needs a {want} anchor and to be a one-shot")),
            None => errors.push(format!("moods.json transitions.{key}: strip `{name}` is not in anim/anim.json")),
        }
    }
    out
}

/// Every clip id the sidecar's steps name (where a strip may stand in as a clip), and every
/// strip a held frame is a cell of (`{"frame": "<strip>#<cell>"}`: its cells become frames).
fn step_clip_ids(b: &Behaviour) -> Vec<&str> {
    let mut lists: Vec<&[Step]> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    let bands = [&b.energy.low, &b.energy.mid, &b.energy.high];
    let moods = b.moods.values().chain(bands.iter().filter_map(|e| e.idle.as_ref())).chain(b.brain.floor_sit.iter()).chain(b.brain.sulk_sit.iter());
    for m in moods {
        lists.extend(m.entry.as_deref());
        lists.extend(m.exit.as_deref());
        match &m.r#loop {
            Some(Steps::Inline(l)) => lists.push(l),
            Some(Steps::Clip(c)) => ids.push(c),
            Some(Steps::Ranged(r)) => lists.extend([&r.entry, &r.r#loop, &r.exit].map(Vec::as_slice)),
            None => {}
        }
    }
    for e in bands {
        lists.extend(e.activities.iter().chain(e.fidgets.iter()).map(Vec::as_slice));
    }
    lists.extend(b.activities.iter().chain(b.life.fidgets.iter()).chain(b.life.poses.values()).map(Vec::as_slice));
    let c = &b.click;
    lists.extend(c.body.iter().chain([&c.head, &c.annoyed]).map(Vec::as_slice));
    if let Some(p) = &b.perch {
        lists.extend(p.activities.iter().chain([&p.click, &p.headpat]).map(Vec::as_slice));
    }
    if let Some(a) = &b.arrive {
        lists.extend([&a.launch, &a.cinematic, &a.summon, &a.teleport, &a.perch].into_iter().chain(a.alt.values().flatten()).map(Vec::as_slice));
    }
    if let Some(k) = &b.kiss {
        lists.push(&k.steps);
    }
    if let Some(m) = &b.music {
        match &m.window_loop {
            Steps::Inline(l) => lists.push(l),
            Steps::Clip(c) => ids.push(c),
            Steps::Ranged(r) => lists.extend([&r.entry, &r.r#loop, &r.exit].map(Vec::as_slice)),
        }
    }
    let br = &b.brain;
    let own = [&br.satisfied.pat, &br.satisfied.other, &br.sulk, &br.wave, &br.greet, &br.greet_morning, &br.stretch, &br.nap_wake, &br.startle, &br.curious, &br.bat, &br.missed, &br.make_up, &br.cold, &br.nap_wake_greet, &br.glance];
    lists.extend(br.bid.iter().chain(br.insist.iter()).chain(own).chain(br.hang_fidgets.values()).map(Vec::as_slice));
    if let Some(d) = &b.drag {
        lists.extend([&d.grab, &d.swing, &d.dizzy].map(Vec::as_slice));
    }
    if let Some(f) = &b.fling {
        lists.extend([&f.launch, &f.bounce, &f.tuck].map(Vec::as_slice));
    }
    if let Some(l) = &b.land {
        lists.extend([&l.soft, &l.hard, &l.perch].map(Vec::as_slice));
    }
    ids.extend(lists.iter().flat_map(|l| l.iter()).flat_map(Step::leaves).filter_map(|s| match s {
        Step::Clip(c) => Some(c.as_str()),
        Step::Hold { frame, .. } => frame.rsplit_once('#').filter(|(n, i)| !n.starts_with('~') && i.parse::<u32>().is_ok()).map(|(n, _)| n),
        Step::Repeat(_) => None,
    }));
    ids
}

/// The single frames the sidecar names (not sequences): an outfit must draw all of them (one
/// it lacked would leave her nothing to show in that state).
fn required_frames(b: &Behaviour, catalog: &Catalog) -> Vec<String> {
    let mut v = vec![catalog.default_sprite.clone()];
    v.extend(b.moods.values().map(|m| m.base.clone()));
    v.extend([&b.energy.low, &b.energy.mid, &b.energy.high].into_iter().filter_map(|e| e.idle.as_ref()).map(|m| m.base.clone()));
    if let Some(p) = &b.perch {
        v.push(p.base.clone());
        v.extend(p.moods.values().cloned());
    }
    if let Some(m) = &b.music {
        v.push(m.floor.clone());
        v.extend(m.window.clone());
    }
    if let Some(d) = &b.drag {
        v.extend([&d.hang, &d.shake, &d.ankle, &d.cling].into_iter().flatten().cloned());
        v.extend(d.still.iter().map(|s| s.frame.clone()));
    }
    if let Some(f) = &b.fling {
        v.extend([&f.fly, &f.fall].into_iter().flatten().cloned());
    }
    let br = &b.brain;
    v.extend([&br.nap, &br.nap_deep, &br.nap_perch, &br.hop_crouch, &br.big_hop_crouch, &br.look_front].into_iter().flatten().cloned());
    v.extend([&br.floor_sit, &br.sulk_sit].into_iter().flatten().map(|m| m.base.clone()));
    v.sort();
    v.dedup();
    v
}

/// `variants.outfits` -> looks: the pack's own first (the outfit without `sprites`, or `pack`),
/// then each other one's catalog against the pack's frame ids. One that lacks a single frame the
/// sidecar names is refused.
fn resolve_outfits(dir: &Path, b: &Behaviour, catalog: &Catalog, frames: &[FrameSource], index: &HashMap<String, usize>, errors: &mut Vec<String>) -> Vec<Outfit> {
    let specs = b.variants.as_ref().map_or(&[][..], |v| v.outfits.as_slice());
    if specs.is_empty() {
        return Vec::new();
    }
    let own = specs.iter().find(|o| o.sprites.is_none());
    let mut out = vec![Outfit { id: own.map_or_else(|| "pack".to_string(), |o| o.id.clone()), weight: own.map_or(1.0, |o| o.weight), frames: HashMap::new() }];
    let need = required_frames(b, catalog);
    let (cw, ch) = (catalog.canvas.width as i32, catalog.canvas.height as i32);
    for o in specs {
        let Some(rel) = o.sprites.as_deref() else { continue };
        let at = format!("moods.json variants.outfits `{}`", o.id);
        if o.weight.is_nan() || o.weight < 0.0 || out.iter().any(|x| x.id == o.id) {
            errors.push(format!("{at}: weight {} (>= 0) and ids unique", o.weight));
            continue;
        }
        let p = dir.join(rel);
        let cat = std::fs::read_to_string(&p).map_err(anyhow::Error::from).and_then(|t| serde_json::from_str::<PoseCatalog>(&t).map_err(Into::into));
        let cat = match cat {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{at}: {}: {e:#}", p.display()));
                continue;
            }
        };
        let base = p.parent().unwrap_or(dir);
        let mut mine = HashMap::new();
        for sp in cat.sprites {
            let Some(&fi) = index.get(&sp.id) else {
                errors.push(format!("{at}: sprite `{}` is not one of the pack's frames", sp.id));
                continue;
            };
            let path = base.join(&sp.file);
            if !path.is_file() {
                errors.push(format!("{at}: sprite `{}`: file {} is missing", sp.id, sp.file));
            }
            let bd = sp.visible_bounds;
            if bd.x < 0 || bd.y < 0 || bd.right() > cw || bd.bottom() > ch || bd.width <= 0 || bd.height <= 0 {
                errors.push(format!("{at}: sprite `{}`: visibleBounds {bd:?} outside the {cw}x{ch} canvas", sp.id));
            }
            if frames[fi].cell.is_some() {
                errors.push(format!("{at}: sprite `{}` is a strip cell (tag the strip with its outfit instead)", sp.id));
                continue;
            }
            let patch = sp.patch.as_ref().map(|p| (dir.join(&p.file), p.rect, p.dx, p.dy, p.sha256.clone()));
            if let Some((pp, ..)) = &patch
                && !pp.is_file()
            {
                errors.push(format!("{at}: sprite `{}`: patch {} is missing", sp.id, pp.display()));
            }
            mine.insert(fi, FrameSource { id: sp.id, path, anchor: sp.anchor, bounds: bd, sha256: sp.sha256, patch, cell: None });
        }
        let lacks: Vec<&str> = need.iter().filter(|id| index.get(id.as_str()).is_some_and(|fi| !mine.contains_key(fi))).map(String::as_str).collect();
        if !lacks.is_empty() {
            errors.push(format!("{at}: lacks {} (it must draw every single frame the sidecar names; sequences it lacks are skipped)", lacks.join(", ")));
            continue;
        }
        out.push(Outfit { id: o.id.clone(), weight: o.weight, frames: mine });
    }
    out
}

/// A frame to decode: catalog sprite or sidecar extra.
#[derive(Clone, Debug)]
pub struct FrameSource {
    pub id: String,
    pub path: PathBuf,
    pub anchor: Anchor,
    pub bounds: Rect,
    pub sha256: Option<String>,
    /// A face painted on at decode (file, rect, dx, dy, sha256).
    pub patch: Option<(PathBuf, Rect, i32, i32, Option<String>)>,
    /// A cell of a desktop-art strip (`anim/`): cut from `path` at decode.
    pub cell: Option<crate::art::CellSrc>,
}

/// Her energy bands (`crate::life::Band`).
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EnergySpec {
    pub low: EnergyBand,
    pub mid: EnergyBand,
    pub high: EnergyBand,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EnergyBand {
    /// Instead of `activities` (empty: those).
    #[serde(deserialize_with = "beats")]
    pub activities: Vec<Vec<Step>>,
    /// Instead of `life.fidgets` in this band (empty: those): sleepy blinks at night.
    #[serde(deserialize_with = "beats")]
    pub fidgets: Vec<Vec<Step>>,
    /// The gap between activities [min, max] (absent: `life.activityGapMs`).
    pub gap_ms: Option<[u64; 2]>,
    /// The idle mood's base and loop in this band (a tired face at night).
    pub idle: Option<MoodSpec>,
    /// Her breath in this band (absent: `life.bobPx`, `life.bobPeriodMs`, `life.swayPx`): a
    /// quick small bounce by day, slow and deep at night.
    pub bob_px: Option<i32>,
    pub bob_period_ms: Option<u64>,
    pub sway_px: Option<i32>,
    /// Standing, her head hangs this far (px at 1x) and leans this far (the hair's top): tired.
    pub droop_px: i32,
    pub lean_px: i32,
    /// A slow head nod (px at 1x, ms a nod): dozing off on her feet or sitting.
    pub nod_px: i32,
    pub nod_period_ms: u64,
    /// Sparkles in this band (absent: `life.sparkleEveryMs`).
    pub sparkle_every_ms: Option<[u64; 2]>,
}

impl EnergySpec {
    pub fn band(&self, b: crate::life::Band) -> &EnergyBand {
        match b {
            crate::life::Band::Low => &self.low,
            crate::life::Band::Mid => &self.mid,
            crate::life::Band::High => &self.high,
        }
    }
}

/// A variant of a pooled state, resolved (`crate::variants`).
#[derive(Clone, Debug)]
pub struct Variant {
    pub id: String,
    pub weight: f32,
    pub lying: bool,
    /// Eligible only this long into a stay (`VariantSpec::after_ms`).
    pub after_ms: Option<u64>,
    /// Its own length when picked (`VariantSpec::stay_ms`).
    pub stay_ms: Option<[u64; 2]>,
    /// Only in these energy bands (`VariantSpec::energy`; empty: any).
    pub energy: Vec<crate::life::Band>,
    /// None: the pack's own art for the state, as it behaves without pools.
    pub art: Option<VariantArt>,
}

/// A strip variant: strip indices into `Pack::anim`.
#[derive(Clone, Debug)]
pub struct VariantArt {
    pub base: usize,
    pub talk: Option<usize>,
    pub music: Option<usize>,
    pub blinks: Vec<(usize, f32)>,
    pub blink_ms: [u64; 2],
    pub every: u32,
    pub hold_ms: Option<[u64; 2]>,
}

#[derive(Clone, Debug, Default)]
pub struct Pool {
    pub shift_ms: Option<[u64; 2]>,
    pub shift_stays: bool,
    /// The variant a drawn arrival or wake ends in, held `bridge_ms` (`PoolSpec::bridge`).
    pub bridge: Option<usize>,
    pub bridge_ms: [u64; 2],
    pub variants: Vec<Variant>,
}

impl Pool {
    pub fn by_id(&self, id: &str) -> Option<usize> {
        self.variants.iter().position(|v| v.id == id)
    }
}

pub struct Pack {
    pub dir: PathBuf,
    pub name: String,
    pub catalog: Catalog,
    pub behaviour: Behaviour,
    pub frames: Vec<FrameSource>,
    pub index: HashMap<String, usize>,
    pub poses: Vec<FrameSource>,
    /// Seat-anchored frames used on a perch (base, per-mood frames, music's window pose)
    /// are registered to this seat height (the perch base's), so they all sit on the same
    /// edge line. Other seat frames keep their own canvas registration.
    pub seat_ref: Option<i32>,
    pub perch_frames: std::collections::HashSet<String>,
    /// The desktop art (`anim/anim.json`), if the pack has it.
    pub anim: Option<crate::art::Anim>,
    /// Per strip of `anim`: the frame index of its cell 0, if a pool uses it; and of its
    /// `mirrorArt` twin's cells, stored mirrored.
    pub strip_first: Vec<Option<usize>>,
    pub mirror_first: Vec<Option<usize>>,
    /// Per strip: the thinnings it plays at (the `every` of the variants using it; a strip
    /// named as a clip plays at its pools' finest, else whole; a transition or a motion whole),
    /// ascending. Only the positions those play (`Strip::played`) are decoded.
    pub strip_every: Vec<Vec<u32>>,
    /// The variant pools (`moods.json` `variants`); a strip variant is dropped when the pack
    /// has no `anim/`, so an old pack's pool is its own art alone.
    pub pools: HashMap<crate::variants::PoolKey, Pool>,
    /// `moods.json` `transitions`, resolved: strip indices into `anim`.
    pub transitions: HashMap<String, usize>,
    /// `moods.json` `motions`, resolved.
    pub motions: Motions,
    pub cache_bytes: usize,
    /// `variants.outfits`, resolved: the pack's own look first. Empty: one look.
    pub outfits: Vec<Outfit>,
    /// The outfit she wears (an index into `outfits`): every frame decodes in it.
    worn: AtomicUsize,
    /// The repeat rolls' xorshift state (`roll`; `seed_rolls` from her own rng).
    rolls: AtomicU64,
    /// Effect sprites (`kind: "effect"`: catalog sprites, extra frames) by id: overlays, never
    /// her frames (`click.hearts`).
    pub effects: HashMap<String, FrameSource>,
    /// The hang pose's grip (`brain.poses.hang`'s anchor, canvas px): a `ceiling` strip's cells
    /// register on it.
    pub hang_ref: Option<(i32, i32)>,
    /// Strips standing in as clips in the sidecar's steps (`Clip(strip name)`): decoded when
    /// wanted, like a pool's.
    pub step_strips: Vec<usize>,
    /// A strip tagged for one outfit and named like a catalog clip: that outfit's drawing of
    /// the clip (outfit id, the strip's cells as the clip), played instead while she wears it.
    pub outfit_clips: Vec<(String, Clip)>,
}

/// One look (`variants.outfits`), resolved: its own drawing of the pack's frames, by frame index.
/// The pack's own look has none (every frame is its own).
#[derive(Clone, Debug)]
pub struct Outfit {
    pub id: String,
    pub weight: f32,
    pub frames: HashMap<usize, FrameSource>,
}

impl Pack {
    pub fn frame(&self, id: &str) -> Result<usize> {
        self.index.get(id).copied().ok_or_else(|| anyhow!("unknown frame id `{id}`"))
    }

    /// The outfit she wears (an index into `outfits`; 0 without any).
    pub fn wearing(&self) -> usize {
        self.worn.load(Ordering::Relaxed)
    }

    /// The repeat rolls (`Step::Repeat`) go on from her rng's `seed`.
    pub fn seed_rolls(&self, seed: u64) {
        self.rolls.store(seed, Ordering::Relaxed);
    }

    /// One repeat roll: 0..n.
    fn roll(&self, n: usize) -> usize {
        let mut r = crate::anim::Rng::new(self.rolls.load(Ordering::Relaxed));
        let k = r.pick(n);
        self.rolls.store(r.next(), Ordering::Relaxed);
        k
    }

    /// Wear outfit `i` (ignored past the list). Frames decoded after this are in it: the view
    /// swaps them all at once, only where it can't be seen.
    pub fn wear(&self, i: usize) {
        if i < self.outfits.len() {
            self.worn.store(i, Ordering::Relaxed);
        }
    }

    /// The id of the outfit she wears (empty without any).
    pub fn outfit_id(&self) -> &str {
        self.outfits.get(self.wearing()).map_or("", |o| o.id.as_str())
    }

    pub fn outfit_by_id(&self, id: &str) -> Option<usize> {
        self.outfits.iter().position(|o| o.id == id)
    }

    /// Outfit `o` draws frame `fi` (the pack's own look draws every frame).
    pub fn draws(&self, o: usize, fi: usize) -> bool {
        o == 0 || self.outfits.get(o).is_some_and(|x| x.frames.contains_key(&fi))
    }

    /// Frame `fi` as outfit `o` draws it (the pack's own where it has none).
    pub fn source_in(&self, fi: usize, o: usize) -> &FrameSource {
        self.outfits.get(o).and_then(|x| x.frames.get(&fi)).unwrap_or(&self.frames[fi])
    }

    /// Frame `fi` is drawn in the outfit she wears: a strip cell of a strip tagged for it (or
    /// `any`), else one of its frames. She never shows another.
    pub fn wears(&self, fi: usize) -> bool {
        match self.frames.get(fi).and_then(|f| f.cell.as_ref()) {
            Some(c) => self.strip_wears(c.strip),
            None => self.wearing() == 0 || self.outfits.get(self.wearing()).is_none_or(|o| o.frames.contains_key(&fi)),
        }
    }

    /// Strip `si` is drawn for the outfit she wears (untagged and `any`: every outfit).
    pub fn strip_wears(&self, si: usize) -> bool {
        self.anim.as_ref().and_then(|a| a.strips.get(si)).is_none_or(|s| s.worn_in(self.outfit_id()))
    }

    /// A glance's steps (the pointer came by, a window gained focus): the pack's `brain.glance`
    /// when it's filled, else the look (`@look`). Unresolved: `@look` in either still turns
    /// toward what she glances at (the view's `resolve_steps`).
    pub fn glance_steps(&self) -> Vec<Step> {
        match &self.behaviour.brain.glance {
            g if g.is_empty() => vec![Step::Clip("@look".into())],
            g => g.clone(),
        }
    }

    pub fn clip(&self, id: &str) -> Option<&Clip> {
        let id = self.catalog.actions.iter().find(|a| a.id == id).map_or(id, |a| a.animation.as_str());
        let worn = self.outfit_id();
        let own = self.outfit_clips.iter().find(|(o, c)| c.id == id && o == worn).map(|(_, c)| c);
        own.or_else(|| self.catalog.clips.iter().find(|c| c.id == id))
    }

    /// Where a frame is drawn on the canonical canvas: its own anchor moved onto the
    /// canonical ground point. Seat-anchored frames keep their vertical registration
    /// (they are authored to the seat line, not the floor).
    pub fn offset(&self, f: &FrameSource) -> (i32, i32) {
        let c = self.catalog.canonical_anchor;
        let dy = match (f.anchor.name.as_str(), self.seat_ref) {
            ("ground", _) => c.y - f.anchor.y,
            ("seat", Some(s)) if self.perch_frames.contains(&f.id) => s - f.anchor.y,
            // Hanging from its grip (lachy's dangle): its feet where hers are, so the body
            // hangs where she stood and a drop lands without a jump.
            ("grip", _) => c.y - (f.bounds.y + f.bounds.height),
            // A hang fidget's cell: its grip on the hang pose's (the bar she holds).
            ("ceiling", _) if f.cell.is_some() => self.hang_ref.map_or(0, |h| h.1 - f.anchor.y),
            _ => 0,
        };
        // Grip anchors (ceiling, dock-left/right) keep their canvas registration: centring a
        // wall grip at x would push the body off the canvas.
        let dx = match f.anchor.name.as_str() {
            "ground" | "seat" | "grip" => c.x - f.anchor.x,
            "ceiling" if f.cell.is_some() => self.hang_ref.map_or(0, |h| h.0 - f.anchor.x),
            _ => 0,
        };
        (dx, dy)
    }

    /// A cell of an effect strip (the form bloom): it never sizes her window.
    pub fn fx_frame(&self, f: &FrameSource) -> bool {
        f.cell.as_ref().zip(self.anim.as_ref()).is_some_and(|(c, a)| a.strips[c.strip].fx)
    }

    /// Union of every body frame's visible bounds after anchor registration (not an effect's:
    /// it is cut to them). Every frame is cropped to this one rect so they stay aligned.
    pub fn crop_rect(&self) -> Rect {
        let mut r: Option<Rect> = None;
        let worn = self.outfits.iter().flat_map(|o| o.frames.values());
        for f in self.frames.iter().filter(|f| !self.fx_frame(f)).chain(worn) {
            let (dx, dy) = self.offset(f);
            let b = f.bounds.offset(dx, dy);
            r = Some(r.map_or(b, |r| r.union(&b)));
        }
        let r = r.expect("pack has frames");
        let ch = self.catalog.canvas.height as i32;
        let y = (r.y - 2).max(0);
        // Symmetric about the canonical anchor (padded past the canvas where it must be): a
        // frame mirrored about the rect's centre keeps her feet where they were.
        let ax = self.catalog.canonical_anchor.x;
        let reach = (ax - r.x).max(r.right() - ax) + 2;
        Rect { x: ax - reach, y, width: 2 * reach, height: (r.bottom() + 2).min(ch) - y }
    }

    pub fn load(dir: &Path) -> Result<Pack> {
        let read = |rel: &str| -> Result<String> {
            let p = dir.join(rel);
            std::fs::read_to_string(&p).with_context(|| format!("pack: cannot read {}", p.display()))
        };
        let mut catalog: Catalog = serde_json::from_str(&read("sprites.json")?)
            .with_context(|| format!("pack: {}/sprites.json is not a valid sprite catalog", dir.display()))?;
        if catalog.schema_version != 2 {
            bail!("pack: sprites.json schemaVersion {} (expected 2)", catalog.schema_version);
        }
        let behaviour: Behaviour = match std::fs::read_to_string(dir.join("moods.json")) {
            Ok(s) => {
                // A loop range where it can't go: said plainly, before the typed parse's
                // "invalid type: map".
                if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&s) {
                    let mut bad = Vec::new();
                    loop_range_errors(&raw, &mut Vec::new(), &mut bad);
                    if !bad.is_empty() {
                        bail!("pack {} failed validation:\n  - {}", dir.display(), bad.join("\n  - "));
                    }
                }
                serde_json::from_str(&s).with_context(|| format!("pack: {}/moods.json is invalid", dir.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Behaviour::default(),
            Err(e) => return Err(e).context("pack: cannot read moods.json"),
        };

        let mut errors = Vec::new();
        let mut frames = Vec::new();
        let mut index = HashMap::new();
        let mut effects = HashMap::new();
        let (cw, ch) = (catalog.canvas.width as i32, catalog.canvas.height as i32);
        for s in &catalog.sprites {
            // Effects (e.g. a heart burst) are overlays, not body frames.
            if s.kind == "effect" {
                let src = FrameSource { id: s.id.clone(), path: dir.join(&s.file), anchor: s.anchor.clone(), bounds: s.visible_bounds, sha256: s.sha256.clone(), patch: None, cell: None };
                effects.insert(s.id.clone(), src);
                continue;
            }
            let path = dir.join(&s.file);
            if !path.is_file() {
                errors.push(format!("sprite `{}`: file {} is missing", s.id, s.file));
            }
            let b = s.visible_bounds;
            if b.x < 0 || b.y < 0 || b.right() > cw || b.bottom() > ch || b.width <= 0 || b.height <= 0 {
                errors.push(format!("sprite `{}`: visibleBounds {:?} outside the {cw}x{ch} canvas", s.id, b));
            }
            if index.insert(s.id.clone(), frames.len()).is_some() {
                errors.push(format!("sprite id `{}` is defined twice", s.id));
            }
            frames.push(FrameSource {
                id: s.id.clone(),
                path,
                anchor: s.anchor.clone(),
                bounds: b,
                sha256: s.sha256.clone(),
                patch: None,
                cell: None,
            });
        }
        for x in &behaviour.extra_frames {
            let path = dir.join(&x.file);
            if x.kind.as_deref() == Some("effect") {
                if path.is_file() {
                    let (anchor, bounds) = (x.anchor.clone().unwrap_or(Anchor { name: "effect".into(), x: 0, y: 0 }), x.visible_bounds.unwrap_or(Rect { x: 0, y: 0, width: 1, height: 1 }));
                    effects.insert(x.id.clone(), FrameSource { id: x.id.clone(), path, anchor, bounds, sha256: x.sha256.clone(), patch: None, cell: None });
                } else {
                    elog!("synaps-avatar: pack effect `{}`: {} is missing; skipped", x.id, x.file);
                }
                continue;
            }
            if !path.is_file() {
                // Optional add-ons (e.g. music/) may be absent; their users check for the id.
                if x.like.is_none() {
                    elog!("synaps-avatar: pack extra frame `{}`: {} is missing; skipped", x.id, x.file);
                    continue;
                }
                errors.push(format!("moods.json extraFrames `{}`: file {} is missing", x.id, x.file));
            }
            let patch = x.patch.as_ref().map(|p| (dir.join(&p.file), p.rect, p.dx, p.dy, p.sha256.clone()));
            if let Some((pp, ..)) = &patch
                && !pp.is_file()
            {
                elog!("synaps-avatar: pack extra frame `{}`: patch {} is missing; skipped", x.id, pp.display());
                continue;
            }
            let src = match (&x.like, &x.anchor, x.visible_bounds) {
                (Some(like), _, _) => match index.get(like) {
                    Some(&i) => FrameSource { id: x.id.clone(), path, sha256: None, patch, ..frames[i].clone() },
                    None => {
                        errors.push(format!("moods.json extraFrames `{}`: `like` frame `{like}` is unknown", x.id));
                        continue;
                    }
                },
                (None, Some(a), Some(b)) => {
                    FrameSource { id: x.id.clone(), path, anchor: a.clone(), bounds: b, sha256: x.sha256.clone(), patch, cell: None }
                }
                _ => {
                    errors.push(format!("moods.json extraFrames `{}`: needs `like` or `anchor` + `visibleBounds`", x.id));
                    continue;
                }
            };
            index.insert(x.id.clone(), frames.len());
            frames.push(src);
        }

        let has = |id: &str| index.contains_key(id);
        if !has(&catalog.default_sprite) {
            errors.push(format!("defaultSprite `{}` is not a sprite id", catalog.default_sprite));
        }
        for c in &catalog.clips {
            for f in &c.frames {
                if !has(&f.id) {
                    errors.push(format!("clip `{}`: frame `{}` does not resolve", c.id, f.id));
                }
            }
        }
        for a in &catalog.actions {
            if !catalog.clips.iter().any(|c| c.id == a.animation) {
                errors.push(format!("action `{}`: clip `{}` does not exist", a.id, a.animation));
            }
        }

        let pack_name = behaviour
            .name
            .clone()
            .or_else(|| catalog.character.clone())
            .unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());

        let seat_ref = behaviour
            .perch
            .as_ref()
            .and_then(|p| index.get(&p.base))
            .map(|&i| &frames[i])
            .or_else(|| frames.iter().find(|f| f.anchor.name == "seat"))
            .filter(|f| f.anchor.name == "seat")
            .map(|f| f.anchor.y);
        let mut perch_frames = std::collections::HashSet::new();
        if let Some(p) = &behaviour.perch {
            perch_frames.insert(p.base.clone());
            perch_frames.extend(p.moods.values().cloned());
        }
        if let Some(w) = behaviour.music.as_ref().and_then(|m| m.window.clone()) {
            perch_frames.insert(w);
        }
        // Landing on an edge, a headpat or a nap there: registered on the seat like the perch base.
        let seat_steps = behaviour.land.iter().flat_map(|l| l.perch.iter()).chain(behaviour.arrive.iter().flat_map(|a| a.perch.iter()));
        for st in seat_steps.chain(behaviour.perch.iter().flat_map(|p| p.headpat.iter())) {
            if let Step::Hold { frame, .. } = st {
                perch_frames.insert(frame.clone());
            }
        }
        perch_frames.extend(behaviour.brain.nap_perch.clone());
        let hang_ref = behaviour.brain.poses.get("hang").and_then(|id| index.get(id)).map(|&i| (frames[i].anchor.x, frames[i].anchor.y));
        let outfits = resolve_outfits(dir, &behaviour, &catalog, &frames, &index, &mut errors);
        let ref_h = index.get(&catalog.default_sprite).map_or(1000, |&i| frames[i].bounds.height);
        let anim = crate::art::Anim::load(dir, ref_h)?;
        let n_strips = anim.as_ref().map_or(0, |a| a.strips.len());
        let (mut strip_first, mut mirror_first) = (vec![None; n_strips], vec![None; n_strips]);
        let pools = resolve_pools(&behaviour, anim.as_ref(), &mut errors);
        let transitions = resolve_transitions(&behaviour, anim.as_ref(), &mut errors);
        let motions = resolve_motions(&behaviour, anim.as_ref(), &mut errors);
        // A strip named where a clip goes: its cells play as that clip.
        let mut step_strips: Vec<usize> = Vec::new();
        let mut outfit_clips: Vec<(String, Clip)> = Vec::new();
        if let Some(a) = &anim {
            for id in step_clip_ids(&behaviour) {
                let known = catalog.clips.iter().any(|c| c.id == id) || catalog.actions.iter().any(|x| x.id == id);
                // A clip's name on a strip of one outfit: that outfit's drawing of it.
                if let Some(&si) = a.by_name.get(id)
                    && (!known || a.strips[si].outfit_only().is_some())
                    && !step_strips.contains(&si)
                {
                    step_strips.push(si);
                }
            }
            let ids: Vec<&str> = outfits.iter().map(|o| o.id.as_str()).collect();
            for st in &a.strips {
                if let Some(o) = &st.outfit
                    && !o.eq_ignore_ascii_case("any")
                    && !ids.contains(&o.as_str())
                {
                    errors.push(format!("anim strip `{}`: outfit `{o}` is not in moods.json variants.outfits", st.name));
                }
            }
        }
        let mut strip_every: Vec<Vec<u32>> = vec![Vec::new(); n_strips];
        for v in pools.values().flat_map(|p| &p.variants).filter_map(|v| v.art.as_ref()) {
            for si in std::iter::once(v.base).chain(v.talk).chain(v.music).chain(v.blinks.iter().map(|b| b.0)) {
                strip_every[si].push(v.every.max(1));
            }
        }
        // A strip named as a clip plays at its pools' finest thinning (their keys, when thinned),
        // else whole; a transition and a motion whole.
        let clip_every: Vec<u32> = strip_every.iter().map(|e| e.iter().copied().min().unwrap_or(1)).collect();
        for si in transitions.values().copied().chain(motions.strips()) {
            strip_every[si].push(1);
        }
        for &si in &step_strips {
            strip_every[si].push(clip_every[si]);
        }
        for e in &mut strip_every {
            if e.is_empty() {
                e.push(1);
            }
            e.sort_unstable();
            e.dedup();
        }
        if let Some(a) = &anim {
            // Every strip a pool uses (and its mirrorArt twin, stored mirrored), as frames.
            let mut used: Vec<usize> = pools.values().flat_map(|p| &p.variants).filter_map(|v| v.art.as_ref()).flat_map(|v| {
                std::iter::once(v.base).chain(v.talk).chain(v.music).chain(v.blinks.iter().map(|b| b.0))
            })
            .chain(transitions.values().copied())
            .chain(motions.strips())
            .chain(step_strips.iter().copied())
            .collect();
            used.sort_unstable();
            used.dedup();
            for si in used {
                let twin = a.strips[si].mirror_art.as_ref().and_then(|m| a.by_name.get(m).copied());
                for (ti, mirror) in std::iter::once((si, false)).chain(twin.map(|t| (t, true))) {
                    let first = frames.len();
                    for (id, src) in a.cells(ti, mirror) {
                        if src.anchor.name == "seat" {
                            if seat_ref.is_none() {
                                errors.push(format!("anim strip `{}`: a seat strip needs the pack's perch base", a.strips[ti].name));
                            }
                            perch_frames.insert(id.clone());
                        }
                        index.insert(id, frames.len());
                        frames.push(src);
                    }
                    if mirror {
                        mirror_first[si] = Some(first);
                    } else {
                        strip_first[si] = Some(first);
                    }
                }
            }
            for &si in &step_strips {
                let s = &a.strips[si];
                let cells = s.played(clip_every[si]).into_iter().map(|(c, ms)| ClipFrame { id: s.cell_id(c), ms }).collect();
                let clip = Clip { id: s.name.clone(), frames: cells };
                let known = catalog.clips.iter().any(|c| c.id == s.name) || catalog.actions.iter().any(|x| x.id == s.name);
                match s.outfit_only() {
                    Some(o) if known => outfit_clips.push((o.to_string(), clip)),
                    _ => catalog.clips.push(clip),
                }
            }
        }
        let cache_bytes = behaviour.variants.as_ref().and_then(|v| v.cache_mb).unwrap_or(64) as usize * 1_000_000;
        let mut pack = Pack {
            dir: dir.to_owned(),
            name: pack_name,
            catalog,
            behaviour,
            frames,
            index,
            poses: Vec::new(),
            seat_ref,
            perch_frames,
            anim,
            strip_first,
            mirror_first,
            strip_every,
            pools,
            transitions,
            motions,
            cache_bytes,
            outfits,
            worn: AtomicUsize::new(0),
            rolls: AtomicU64::new(crate::anim::Rng::seeded().next()),
            effects,
            hang_ref,
            step_strips,
            outfit_clips,
        };

        // Behaviour references.
        let check_steps = |what: &str, steps: &[Step], errors: &mut Vec<String>, pack: &Pack| {
            for st in steps {
                if let Step::Repeat(r) = st {
                    if r.r#loop.is_empty() {
                        errors.push(format!("moods.json {what}: the repeat's `loop` is empty"));
                    }
                    if r.repeat[0] == 0 || r.repeat[0] > r.repeat[1] || r.repeat[1] > MAX_REPEAT {
                        errors.push(format!("moods.json {what}: `repeat` is {:?}; it takes [min, max], 1 <= min <= max <= {MAX_REPEAT}", r.repeat));
                    }
                }
                // A repeat's own `@look`s resolve as they play, as a beat's do.
                let nested = matches!(st, Step::Repeat(_));
                for s in st.leaves() {
                    match s {
                        Step::Clip(id) if pack.clip(id).is_none() && !(nested && id.starts_with('@')) => {
                            errors.push(format!("moods.json {what}: clip `{id}` does not exist"))
                        }
                        Step::Hold { frame, .. } if !pack.index.contains_key(frame) => {
                            errors.push(format!("moods.json {what}: frame `{frame}` does not resolve"))
                        }
                        _ => {}
                    }
                }
            }
        };
        // Steps shown on a window top: seat-anchored frames only.
        let seat_only = |what: &str, steps: &[Step], errors: &mut Vec<String>, pack: &Pack| {
            for st in steps.iter().flat_map(Step::leaves) {
                let ids: Vec<String> = match st {
                    Step::Hold { frame, .. } => vec![frame.clone()],
                    Step::Clip(c) => pack.clip(c).map(|c| c.frames.iter().map(|f| f.id.clone()).collect()).unwrap_or_default(),
                    Step::Repeat(_) => Vec::new(),
                };
                for id in ids {
                    if pack.index.get(&id).is_some_and(|&i| pack.frames[i].anchor.name != "seat") {
                        errors.push(format!("moods.json {what}: frame `{id}` has no seat anchor"));
                    }
                }
            }
        };
        for (name, m) in &pack.behaviour.moods {
            if crate::anim::Mood::parse(name).is_none() {
                errors.push(format!("moods.json: unknown mood `{name}`"));
            }
            if !pack.index.contains_key(&m.base) {
                errors.push(format!("moods.json mood `{name}`: base frame `{}` does not resolve", m.base));
            }
            if let Some(l) = &m.r#loop {
                check_steps(&format!("mood `{name}` loop"), &l.as_steps(), &mut errors, &pack);
                check_loop_range(&format!("mood `{name}` loop"), l, &mut errors);
            }
            check_steps(&format!("mood `{name}` entry"), m.entry.as_deref().unwrap_or(&[]), &mut errors, &pack);
            check_steps(&format!("mood `{name}` exit"), m.exit.as_deref().unwrap_or(&[]), &mut errors, &pack);
            if let Some(t) = &m.then
                && crate::anim::Mood::parse(&t.mood).is_none()
            {
                errors.push(format!("moods.json mood `{name}`: then.mood `{}` is unknown", t.mood));
            }
        }
        for (i, a) in pack.behaviour.activities.iter().enumerate() {
            check_steps(&format!("activity #{i}"), a, &mut errors, &pack);
        }
        for (name, band) in [("low", &pack.behaviour.energy.low), ("mid", &pack.behaviour.energy.mid), ("high", &pack.behaviour.energy.high)] {
            for (i, a) in band.activities.iter().enumerate() {
                let a: Vec<Step> = a.iter().filter(|x| !matches!(x, Step::Clip(id) if id.starts_with('@'))).cloned().collect();
                check_steps(&format!("energy.{name}.activities #{i}"), &a, &mut errors, &pack);
            }
            if let Some(m) = &band.idle {
                if !pack.index.contains_key(&m.base) {
                    errors.push(format!("moods.json energy.{name}.idle: base `{}` does not resolve", m.base));
                }
                if let Some(l) = &m.r#loop {
                    check_steps(&format!("energy.{name}.idle loop"), &l.as_steps(), &mut errors, &pack);
                    check_loop_range(&format!("energy.{name}.idle loop"), l, &mut errors);
                }
            }
            if band.gap_ms.is_some_and(|[a, b]| a == 0 || a > b) {
                errors.push(format!("moods.json energy.{name}.gapMs: [min, max], min > 0"));
            }
        }
        if let Some(p) = &pack.behaviour.perch {
            for (what, id) in std::iter::once(("base".to_string(), &p.base)).chain(p.moods.iter().map(|(k, v)| (format!("moods.{k}"), v))) {
                match pack.index.get(id.as_str()) {
                    None => errors.push(format!("moods.json perch {what}: frame `{id}` does not resolve")),
                    Some(&i) if pack.frames[i].anchor.name != "seat" => {
                        errors.push(format!("moods.json perch {what}: frame `{id}` has no seat anchor"))
                    }
                    _ => {}
                }
            }
            for (i, a) in p.activities.iter().enumerate() {
                check_steps(&format!("perch.activities #{i}"), a, &mut errors, &pack);
            }
            check_steps("perch.click", &p.click, &mut errors, &pack);
            check_steps("perch.headpat", &p.headpat, &mut errors, &pack);
            seat_only("perch.headpat", &p.headpat, &mut errors, &pack);
        }
        if let Some(a) = &pack.behaviour.arrive {
            for (k, st) in [("launch", &a.launch), ("cinematic", &a.cinematic), ("summon", &a.summon), ("teleport", &a.teleport), ("perch", &a.perch)] {
                check_steps(&format!("arrive.{k}"), st, &mut errors, &pack);
            }
            seat_only("arrive.perch", &a.perch, &mut errors, &pack);
            if !(0.0..=1.0).contains(&pack.behaviour.form_chance) {
                errors.push(format!("moods.json formChance: {} (0..1)", pack.behaviour.form_chance));
            }
            if !(0.0..=1.0).contains(&a.teleport_chance) {
                errors.push(format!("moods.json arrive.teleportChance: {} (0..1)", a.teleport_chance));
            }
        }
        if let Some(k) = &pack.behaviour.kiss {
            check_steps("kiss.steps", &k.steps, &mut errors, &pack);
            if !(0.0..=1.0).contains(&k.chance) {
                errors.push(format!("moods.json kiss.chance: {} (0..1)", k.chance));
            }
        }
        check_steps("click.head", &pack.behaviour.click.head, &mut errors, &pack);
        check_steps("click.annoyed", &pack.behaviour.click.annoyed, &mut errors, &pack);
        for (i, b) in pack.behaviour.click.body.iter().enumerate() {
            check_steps(&format!("click.body #{i}"), b, &mut errors, &pack);
        }
        {
            let b = &pack.behaviour.brain;
            for (i, s) in b.bid.iter().chain(b.insist.iter()).enumerate() {
                let s: Vec<Step> = s.iter().filter(|x| !matches!(x, Step::Clip(id) if id.starts_with('@'))).cloned().collect();
                check_steps(&format!("brain bid/insist #{i}"), &s, &mut errors, &pack);
            }
            check_steps("brain.satisfied.pat", &b.satisfied.pat, &mut errors, &pack);
            check_steps("brain.satisfied.other", &b.satisfied.other, &mut errors, &pack);
            check_steps("brain.sulk", &b.sulk, &mut errors, &pack);
            check_steps("brain.wave", &b.wave, &mut errors, &pack);
            check_steps("brain.greet", &b.greet, &mut errors, &pack);
            check_steps("brain.greetMorning", &b.greet_morning, &mut errors, &pack);
            check_steps("brain.stretch", &b.stretch, &mut errors, &pack);
            for (k, st) in [("curious", &b.curious), ("bat", &b.bat), ("missed", &b.missed), ("makeUp", &b.make_up), ("cold", &b.cold), ("napWakeGreet", &b.nap_wake_greet)] {
                let st: Vec<Step> = st.iter().filter(|x| !matches!(x, Step::Clip(id) if id.starts_with('@'))).cloned().collect();
                check_steps(&format!("brain.{k}"), &st, &mut errors, &pack);
            }
            if let Some(fs) = &b.floor_sit {
                if !pack.index.contains_key(&fs.base) {
                    errors.push(format!("moods.json brain.floorSit: base `{}` does not resolve", fs.base));
                }
                check_steps("brain.floorSit entry", fs.entry.as_deref().unwrap_or(&[]), &mut errors, &pack);
                check_steps("brain.floorSit exit", fs.exit.as_deref().unwrap_or(&[]), &mut errors, &pack);
            }
            for (k, sit) in [("floorSit", &b.floor_sit), ("sulkSit", &b.sulk_sit)] {
                if let Some(l) = sit.as_ref().and_then(|s| s.r#loop.as_ref()) {
                    check_loop_range(&format!("brain.{k} loop"), l, &mut errors);
                }
            }
            if let Some(ss) = &b.sulk_sit
                && !pack.index.contains_key(&ss.base)
            {
                errors.push(format!("moods.json brain.sulkSit: base `{}` does not resolve", ss.base));
            }
            for id in b.nap.iter().chain(b.hop_crouch.iter()).chain(b.nap_deep.iter()).chain(b.big_hop_crouch.iter()).chain(b.look_front.iter()) {
                if !pack.index.contains_key(id) {
                    errors.push(format!("moods.json brain: frame `{id}` does not resolve"));
                }
            }
            if let Some(id) = &b.nap_perch {
                match pack.index.get(id) {
                    None => errors.push(format!("moods.json brain.napPerch: frame `{id}` does not resolve")),
                    Some(&i) if pack.frames[i].anchor.name != "seat" => errors.push(format!("moods.json brain.napPerch: frame `{id}` has no seat anchor")),
                    _ => {}
                }
            }
            check_steps("brain.napWake", &b.nap_wake, &mut errors, &pack);
            check_steps("brain.startle", &b.startle, &mut errors, &pack);
            for (k, id) in &b.poses {
                let want = match k.as_str() {
                    "hang" => "ceiling",
                    "hang_upside_down" => "ceiling-upside-down",
                    "cling_left" => "dock-left",
                    "cling_right" => "dock-right",
                    "peek" => "ground",
                    _ => {
                        errors.push(format!("moods.json brain.poses: unknown approach `{k}`"));
                        continue;
                    }
                };
                match pack.index.get(id) {
                    None => errors.push(format!("moods.json brain.poses.{k}: frame `{id}` does not resolve")),
                    Some(&i) if pack.frames[i].anchor.name != want => {
                        errors.push(format!("moods.json brain.poses.{k}: frame `{id}` needs a `{want}` anchor"))
                    }
                    _ => {}
                }
            }
        }
        // The ragdoll's frames (BRIEF-ragdoll §2.4).
        if let Some(d) = &pack.behaviour.drag {
            check_steps("drag.grab", &d.grab, &mut errors, &pack);
            check_steps("drag.swing", &d.swing, &mut errors, &pack);
            check_steps("drag.dizzy", &d.dizzy, &mut errors, &pack);
            for (k, id) in [("hang", &d.hang), ("shake", &d.shake)] {
                if let Some(id) = id
                    && !pack.index.contains_key(id)
                {
                    errors.push(format!("moods.json drag.{k}: frame `{id}` does not resolve"));
                }
            }
            for st in &d.still {
                if !pack.index.contains_key(&st.frame) {
                    errors.push(format!("moods.json drag.still: frame `{}` does not resolve", st.frame));
                }
            }
            for (k, id) in [("ankle", &d.ankle), ("cling", &d.cling)] {
                match id.as_ref().map(|id| (id, pack.index.get(id))) {
                    Some((id, None)) => errors.push(format!("moods.json drag.{k}: frame `{id}` does not resolve")),
                    Some((id, Some(&i))) if matches!(pack.frames[i].anchor.name.as_str(), "ground" | "seat") => {
                        errors.push(format!("moods.json drag.{k}: frame `{id}` needs a grip anchor (not ground/seat)"))
                    }
                    _ => {}
                }
            }
            if let Some([a, b]) = d.zones
                && !(0.0 < a && a < b && b < 1.0)
            {
                errors.push(format!("moods.json drag.zones: [{a}, {b}] must be ascending inside (0, 1)"));
            }
        }
        if let Some(f) = &pack.behaviour.fling {
            check_steps("fling.launch", &f.launch, &mut errors, &pack);
            check_steps("fling.bounce", &f.bounce, &mut errors, &pack);
            check_steps("fling.tuck", &f.tuck, &mut errors, &pack);
            for (k, id) in [("fly", &f.fly), ("fall", &f.fall)] {
                if let Some(id) = id
                    && !pack.index.contains_key(id)
                {
                    errors.push(format!("moods.json fling.{k}: frame `{id}` does not resolve"));
                }
            }
        }
        if let Some(l) = &pack.behaviour.land {
            check_steps("land.soft", &l.soft, &mut errors, &pack);
            check_steps("land.hard", &l.hard, &mut errors, &pack);
            check_steps("land.perch", &l.perch, &mut errors, &pack);
            for st in l.perch.iter().flat_map(Step::leaves) {
                let ids: Vec<String> = match st {
                    Step::Hold { frame, .. } => vec![frame.clone()],
                    Step::Clip(c) => pack.clip(c).map(|c| c.frames.iter().map(|f| f.id.clone()).collect()).unwrap_or_default(),
                    Step::Repeat(_) => Vec::new(),
                };
                for id in ids {
                    if pack.index.get(&id).is_some_and(|&i| pack.frames[i].anchor.name != "seat") {
                        errors.push(format!("moods.json land.perch: frame `{id}` has no seat anchor"));
                    }
                }
            }
            if let Some(g) = &l.hard_glitch
                && !matches!(g.as_str(), "land" | "burst" | "none")
            {
                errors.push(format!("moods.json land.hardGlitch: `{g}` (land|burst|none)"));
            }
        }
        // The restless idle's pools, the glance, the hang fidgets, the perched music loop, the
        // arrivals' other drawings, the hearts' sprite.
        {
            let anchored = |what: &str, want: &str, steps: &[Step], errors: &mut Vec<String>, pack: &Pack| {
                for st in steps.iter().flat_map(Step::leaves) {
                    let ids: Vec<String> = match st {
                        Step::Hold { frame, .. } => vec![frame.clone()],
                        Step::Clip(c) => pack.clip(c).map(|c| c.frames.iter().map(|f| f.id.clone()).collect()).unwrap_or_default(),
                        Step::Repeat(_) => Vec::new(),
                    };
                    for id in ids {
                        if pack.index.get(&id).is_some_and(|&i| pack.frames[i].anchor.name != want) {
                            errors.push(format!("moods.json {what}: frame `{id}` needs a `{want}` anchor"));
                        }
                    }
                }
            };
            let no_at = |st: &[Step]| -> Vec<Step> { st.iter().filter(|x| !matches!(x, Step::Clip(id) if id.starts_with('@'))).cloned().collect() };
            let b = &pack.behaviour;
            for (i, f) in b.life.fidgets.iter().enumerate() {
                check_steps(&format!("life.fidgets #{i}"), &no_at(f), &mut errors, &pack);
            }
            for (k, st) in &b.life.poses {
                check_steps(&format!("life.poses.{k}"), &no_at(st), &mut errors, &pack);
            }
            for (name, band) in [("low", &b.energy.low), ("mid", &b.energy.mid), ("high", &b.energy.high)] {
                for (i, f) in band.fidgets.iter().enumerate() {
                    check_steps(&format!("energy.{name}.fidgets #{i}"), &no_at(f), &mut errors, &pack);
                }
            }
            check_steps("brain.glance", &no_at(&b.brain.glance), &mut errors, &pack);
            for (k, st) in &b.brain.hang_fidgets {
                check_steps(&format!("brain.hangFidgets.{k}"), st, &mut errors, &pack);
                anchored(&format!("brain.hangFidgets.{k}"), "ceiling", st, &mut errors, &pack);
            }
            if let Some(m) = &b.music {
                let wl = m.window_loop.as_steps();
                check_steps("music.windowLoop", &wl, &mut errors, &pack);
                anchored("music.windowLoop", "seat", &wl, &mut errors, &pack);
                check_loop_range("music.windowLoop", &m.window_loop, &mut errors);
            }
            if let Some(a) = &b.arrive {
                for (k, alts) in &a.alt {
                    if !matches!(k.as_str(), "launch" | "cinematic" | "summon" | "teleport" | "perch") {
                        errors.push(format!("moods.json arrive.alt: unknown arrival `{k}` (launch|cinematic|summon|teleport|perch)"));
                        continue;
                    }
                    for (i, st) in alts.iter().enumerate() {
                        check_steps(&format!("arrive.alt.{k} #{i}"), st, &mut errors, &pack);
                        if k == "perch" {
                            anchored(&format!("arrive.alt.perch #{i}"), "seat", st, &mut errors, &pack);
                        }
                    }
                }
            }
            if let Some(h) = &b.click.hearts
                && !pack.effects.contains_key(h)
            {
                errors.push(format!("moods.json click.hearts: `{h}` is not an effect sprite (kind \"effect\")"));
            }
        }
        for f in &pack.behaviour.font_files {
            if !dir.join(f).is_file() {
                errors.push(format!("moods.json fontFiles: {f} is missing"));
            }
        }

        // Docking poses: phase 3 uses them; validate now so a broken pack fails early.
        match std::fs::read_to_string(dir.join("poses/poses.json")) {
            Ok(s) => match serde_json::from_str::<PoseCatalog>(&s) {
                Ok(p) => {
                    for s in p.sprites {
                        let path = dir.join("poses").join(&s.file);
                        if !path.is_file() {
                            errors.push(format!("pose `{}`: file poses/{} is missing", s.id, s.file));
                        }
                        let (ax, ay) = (s.anchor.x, s.anchor.y);
                        if ax < 0 || ay < 0 || ax > cw || ay > ch {
                            errors.push(format!("pose `{}`: anchor ({ax},{ay}) outside the canvas", s.id));
                        }
                        pack.poses.push(FrameSource {
                            id: s.id,
                            path,
                            anchor: s.anchor,
                            bounds: s.visible_bounds,
                            sha256: s.sha256,
                            patch: None,
                            cell: None,
                        });
                    }
                }
                Err(e) => errors.push(format!("poses/poses.json is invalid: {e}")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("poses/poses.json unreadable: {e}")),
        }

        if !errors.is_empty() {
            bail!("pack {} failed validation:\n  - {}", dir.display(), errors.join("\n  - "));
        }
        Ok(pack)
    }

    /// A strip's cells as a sequence (`Strip::played`): each position its own ms (`ms`, a
    /// WebP's delays, else `fps`); thinned to every `every`-th (with `keys`: its keys), each held
    /// until the next one played.
    pub fn strip_seq(&self, si: usize, every: u32) -> Seq {
        let (Some(a), Some(Some(first))) = (&self.anim, self.strip_first.get(si)) else { return Vec::new() };
        a.strips[si].played(every).into_iter().map(|(c, ms)| (first + c as usize, ms)).collect()
    }

    /// The cells of strip `si` (or its twin's) that are ever played, at every thinning it plays
    /// at (`strip_every`): the ones to decode.
    pub fn strip_cells(&self, si: usize, twin: bool) -> Vec<usize> {
        let first = if twin { self.mirror_first.get(si) } else { self.strip_first.get(si) };
        let (Some(Some(f)), Some(a)) = (first, &self.anim) else { return Vec::new() };
        let s = &a.strips[si];
        let everys = self.strip_every.get(si).map_or(&[1][..], Vec::as_slice);
        let mut v: Vec<usize> = everys.iter().flat_map(|&e| s.played(e)).map(|(c, _)| *f + c as usize).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The pre-mirrored twin of cell frame `fi` (its strip's `mirrorArt`), if the pack has it.
    pub fn twin(&self, fi: usize) -> Option<usize> {
        let c = self.frames.get(fi)?.cell.as_ref()?;
        if c.mirror {
            return None;
        }
        Some(self.mirror_first.get(c.strip).copied().flatten()? + c.i as usize)
    }

    /// The seam family of frame `fi`: the pack's own art, or a strip family.
    pub fn art_of(&self, fi: usize) -> crate::variants::Art {
        match (self.frames.get(fi).and_then(|f| f.cell.as_ref()), &self.anim) {
            (Some(c), Some(a)) if !a.strips[c.strip].own => crate::variants::Art::Family(a.strips[c.strip].family_id),
            _ => crate::variants::Art::Pack,
        }
    }

    /// Flatten steps into (frame index, ms) pairs.
    pub fn flatten(&self, steps: &[Step]) -> Vec<(usize, u32)> {
        let mut out = Vec::new();
        self.flatten_into(steps, &mut out);
        // A frame the outfit she wears doesn't draw: the whole sequence is skipped (never one
        // drawing from another look in it).
        if out.iter().any(|&(i, _)| !self.wears(i)) {
            return Vec::new();
        }
        out
    }

    fn flatten_into(&self, steps: &[Step], out: &mut Vec<(usize, u32)>) {
        for s in steps {
            match s {
                Step::Clip(id) => {
                    if let Some(c) = self.clip(id) {
                        out.extend(c.frames.iter().filter_map(|f| Some((*self.index.get(&f.id)?, f.ms))));
                    }
                }
                Step::Hold { frame, ms } => {
                    if let Some(&i) = self.index.get(frame) {
                        out.push((i, *ms));
                    }
                }
                // Its loop a few times (`repeat`, picked as it plays), between its entry and exit.
                Step::Repeat(r) => {
                    let lo = r.repeat[0].clamp(1, MAX_REPEAT);
                    let hi = r.repeat[1].clamp(lo, MAX_REPEAT);
                    let n = lo + self.roll((hi - lo + 1) as usize) as u32;
                    self.flatten_into(&r.entry, out);
                    for _ in 0..n {
                        self.flatten_into(&r.r#loop, out);
                    }
                    self.flatten_into(&r.exit, out);
                }
            }
        }
    }

    /// A state's colour: the pack's `tones` entry for `key`, else the theme's.
    pub fn tone(&self, key: Option<&str>) -> u32 {
        let pal = self.palette();
        key.map_or(pal.accent, |k| self.tone_hex(k).unwrap_or_else(|| pal.tone(k)))
    }

    /// A `#rrggbb` colour from the sidecar's `tones`.
    pub fn tone_hex(&self, key: &str) -> Option<u32> {
        self.behaviour.tones.get(key).and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok())
    }

    /// The UI's tokens: SHARD, with any the pack's `tones` name overridden.
    pub fn palette(&self) -> crate::theme::Pal {
        crate::theme::Pal::SHARD.with(|k| self.tone_hex(k))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    /// A tiny pack on disk: 8×8 PNGs, a ground `neutral`, a seat `sit`, `moods` as given.
    fn pack_with(name: &str, moods: serde_json::Value) -> Result<Pack> {
        pack_in(name, moods, false, |p| p)
    }

    /// `pack_with`, optionally with `crate::art`'s test strips in `anim/`; `keep` sees the pack
    /// before its directory goes (decode tests read the files).
    pub fn pack_in<T>(name: &str, moods: serde_json::Value, anim: bool, keep: impl FnOnce(Result<Pack>) -> T) -> T {
        pack_setup(
            name,
            moods,
            |d| {
                if anim {
                    crate::art::tests::write_anim(d);
                }
            },
            keep,
        )
    }

    /// `pack_in` with `setup` adding files to the pack's directory before it loads.
    pub fn pack_setup<T>(name: &str, moods: serde_json::Value, setup: impl FnOnce(&Path), keep: impl FnOnce(Result<Pack>) -> T) -> T {
        let dir = std::env::temp_dir().join(format!("sa-pack-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let png = |f: &str| image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255])).save(dir.join(f)).unwrap();
        for f in ["n.png", "s.png", "l.png"] {
            png(f);
        }
        let sprite = |id: &str, file: &str, anchor: &str| {
            json!({"id": id, "file": file, "anchor": {"name": anchor, "x": 4, "y": 7}, "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}})
        };
        let cat = json!({"schemaVersion": 2, "defaultSprite": "neutral", "canvas": {"width": 8, "height": 8},
            "canonicalAnchor": {"x": 4, "y": 7}, "sprites": [sprite("neutral", "n.png", "ground"), sprite("sit", "s.png", "seat")]});
        std::fs::write(dir.join("sprites.json"), cat.to_string()).unwrap();
        std::fs::write(dir.join("moods.json"), moods.to_string()).unwrap();
        setup(&dir);
        let out = keep(Pack::load(&dir));
        std::fs::remove_dir_all(&dir).ok();
        out
    }

    /// The test strips plus `crate::art`'s cel sheet, animated WebP and hang strip.
    pub fn pack_cels<T>(name: &str, moods: serde_json::Value, outfit: Option<&str>, keep: impl FnOnce(Result<Pack>) -> T) -> T {
        pack_setup(
            name,
            moods,
            |d| {
                crate::art::tests::write_anim(d);
                crate::art::tests::write_cels(d, outfit);
            },
            keep,
        )
    }

    /// An outfit `alt` in `outfits/alt/`: red frames for `ids` (of the test pack's).
    pub fn write_outfit(dir: &Path, ids: &[&str]) {
        let o = dir.join("outfits/alt");
        std::fs::create_dir_all(&o).unwrap();
        let sprites: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| {
                image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255])).save(o.join(format!("{id}.png"))).unwrap();
                let anchor = match *id {
                    "sit" => "seat",
                    "hang" => "ceiling",
                    _ => "ground",
                };
                json!({"id": id, "file": format!("{id}.png"), "anchor": {"name": anchor, "x": 4, "y": 7},
                       "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}})
            })
            .collect();
        std::fs::write(o.join("sprites.json"), json!({ "sprites": sprites }).to_string()).unwrap();
    }

    fn outfitted() -> serde_json::Value {
        json!({"perch": {"base": "sit"}, "extraFrames": [{"id": "wink", "file": "l.png", "like": "neutral"}],
               "activities": [[{"frame": "wink", "ms": 300}], [{"frame": "neutral", "ms": 300}]],
               "variants": {"outfits": [{"id": "main", "weight": 2}, {"id": "alt", "sprites": "outfits/alt/sprites.json"}]}})
    }

    #[test]
    fn a_strip_named_as_a_clip_plays_its_cells() {
        let moods = json!({"activities": [["sheet"], ["greet"]], "brain": {"greet": ["greet"], "bid": [["sheet", "@look"]]}});
        pack_cels("stripclip", moods, None, |p| {
            let p = p.unwrap();
            let a = p.anim.as_ref().unwrap();
            let (sheet, greet) = (a.by_name["sheet"], a.by_name["greet"]);
            assert!(p.step_strips.contains(&sheet) && p.step_strips.contains(&greet));
            assert!(!p.step_strips.contains(&a.by_name["pullup"]), "only the strips steps name");
            let c = p.clip("greet").unwrap();
            assert_eq!(c.frames.iter().map(|f| (f.id.as_str(), f.ms)).collect::<Vec<_>>(), [("greet#0", 80), ("greet#1", 120), ("greet#2", 200)]);
            let seq = p.flatten(&[Step::Clip("sheet".into())]);
            assert_eq!(seq.iter().map(|s| s.1).collect::<Vec<_>>(), [100, 50, 300, 50, 100], "keys and in-betweens, their own timing");
            // A clip plays every cell: all of them decode.
            assert_eq!(p.strip_cells(sheet, false).len(), 5);
            // Registered on her feet like any standing strip.
            let f = &p.frames[seq[0].0];
            let (dx, dy) = p.offset(f);
            let c = p.catalog.canonical_anchor;
            assert_eq!((f.anchor.x + dx, f.anchor.y + dy), (c.x, c.y));
        });
        // Without anim/ the name is just an unknown clip, as before.
        let e = pack_with("stripclip-none", json!({"activities": [["greet"]]})).err().unwrap();
        assert!(format!("{e:#}").contains("clip `greet` does not exist"));
    }

    #[test]
    fn per_cell_timing_and_keys_play_wherever_a_strip_plays() {
        // The cel sheet (keys 0 and 2; 100, 50, 300, 50, 100 ms) blinks over a pool thinned to every
        // 2nd cell and plays as a clip; the animated WebP (80, 120, 200 ms) is a transition.
        let moods = json!({"activities": [["sheet"]], "transitions": {"floorArrive": "greet"},
            "variants": {"pools": {"sleepFloor": {"variants": [{"id": "pack"}, {"id": "doze", "strip": "stand", "every": 2, "blinks": [{"strip": "sheet"}]}]}}}});
        pack_cels("timing", moods, None, |p| {
            let p = p.unwrap();
            let a = p.anim.as_ref().unwrap();
            let (sheet, greet) = (a.by_name["sheet"], a.by_name["greet"]);
            let (s0, g0) = (p.strip_first[sheet].unwrap(), p.strip_first[greet].unwrap());
            // Thinned (the pool's blink): its keys, each held until the next.
            assert_eq!(p.strip_seq(sheet, 2), vec![(s0, 150), (s0 + 2, 450)]);
            // A transition: the WebP's own delays, not their mean.
            assert_eq!(p.strip_seq(greet, 1), vec![(g0, 80), (g0 + 1, 120), (g0 + 2, 200)]);
            // As a clip it plays at its pool's thinning: the keys again.
            assert_eq!(p.flatten(&[Step::Clip("sheet".into())]), vec![(s0, 150), (s0 + 2, 450)]);
            // Decoded: what plays, nothing more.
            assert_eq!(p.strip_cells(sheet, false), vec![s0, s0 + 2]);
            assert_eq!(p.strip_cells(greet, false), vec![g0, g0 + 1, g0 + 2]);
            // A plain strip thinned as before.
            let st0 = p.strip_first[a.by_name["stand"]].unwrap();
            assert_eq!(p.strip_seq(a.by_name["stand"], 2), vec![(st0, 200), (st0 + 2, 200)]);
        });
    }

    #[test]
    fn a_glance_is_the_packs_when_filled_else_the_look() {
        let glance = json!([{"frame": "neutral", "ms": 400}, "@look"]);
        let p = pack_with("glance", json!({"brain": {"glance": glance}})).unwrap();
        assert_eq!(p.glance_steps(), vec![Step::Hold { frame: "neutral".into(), ms: 400 }, Step::Clip("@look".into())]);
        let p = pack_with("glance-none", json!({})).unwrap();
        assert_eq!(p.glance_steps(), vec![Step::Clip("@look".into())], "left out: the look");
    }

    #[test]
    fn hang_fidgets_register_on_the_hang_pose_and_need_its_anchor() {
        let hang = json!({"id": "hang", "file": "l.png", "anchor": {"name": "ceiling", "x": 4, "y": 1}, "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}});
        let moods = json!({"extraFrames": [hang], "brain": {"poses": {"hang": "hang"}, "hangFidgets": {"pullup": ["pullup"]}}});
        pack_cels("hangfidget", moods.clone(), None, |p| {
            let p = p.unwrap();
            assert_eq!(p.hang_ref, Some((4, 1)));
            let fi = p.flatten(&[Step::Clip("pullup".into())])[0].0;
            let f = &p.frames[fi];
            let (dx, dy) = p.offset(f);
            assert_eq!((f.anchor.x + dx, f.anchor.y + dy), (4, 1), "its grip on the bar she holds");
        });
        let mut bad = moods;
        bad["brain"]["hangFidgets"]["catch"] = json!(["sheet"]);
        bad["music"] = json!({"floor": "neutral", "notes": [], "windowLoop": ["greet"]});
        bad["arrive"] = json!({"alt": {"launch": [["greet"]], "warp": [["greet"]]}});
        let e = pack_cels("hangfidget-bad", bad, None, |p| format!("{:#}", p.err().unwrap()));
        assert!(e.contains("brain.hangFidgets.catch: frame `sheet#0` needs a `ceiling` anchor"), "{e}");
        assert!(e.contains("music.windowLoop: frame `greet#0` needs a `seat` anchor"), "{e}");
        assert!(e.contains("arrive.alt: unknown arrival `warp`") && !e.contains("arrive.alt.launch"), "{e}");
    }

    #[test]
    fn a_looped_list_takes_a_loop_range_and_a_plain_list_loops_whole() {
        let sit = |ms: u32| json!({"frame": "sit", "ms": ms});
        let moods = json!({"perch": {"base": "sit"},
            "moods": {"idle": {"base": "neutral", "entry": [{"frame": "neutral", "ms": 20}],
                               "loop": {"entry": [{"frame": "neutral", "ms": 50}], "loop": [{"frame": "neutral", "ms": 100}]}},
                      "thinking": {"base": "neutral", "loop": [{"frame": "neutral", "ms": 100}]}},
            "music": {"floor": "neutral", "notes": [], "windowLoop": {"entry": [sit(130)], "loop": [sit(80), sit(110)], "exit": [sit(190)]}}});
        let p = pack_with("looprange", moods).unwrap();
        let [entry, body, exit] = p.behaviour.music.as_ref().unwrap().window_loop.parts();
        assert_eq!((entry.len(), body.len(), exit.len()), (1, 2, 1));
        assert_eq!(p.flatten(&body), vec![(p.index["sit"], 80), (p.index["sit"], 110)]);
        // A mood's own entry is the mood's; its loop's entry is the loop's.
        let idle = &p.behaviour.moods["idle"];
        assert_eq!(idle.entry.as_ref().map(Vec::len), Some(1));
        assert!(matches!(idle.r#loop.as_ref().unwrap(), Steps::Ranged(r) if r.entry.len() == 1 && r.exit.is_empty()));
        // A plain list: all of it loops (no entry, no exit), as before.
        let [i, b, o] = p.behaviour.moods["thinking"].r#loop.as_ref().unwrap().parts();
        assert!(i.is_empty() && b.len() == 1 && o.is_empty());
        let [i, b, o] = Steps::Clip("blink".into()).parts();
        assert!(i.is_empty() && b == vec![Step::Clip("blink".into())] && o.is_empty());
    }

    #[test]
    fn a_one_shot_beat_repeats_its_middle_a_few_times() {
        let n = |ms: u32| json!({"frame": "neutral", "ms": ms});
        let rep = json!({"entry": [n(10)], "loop": [n(20), n(30)], "exit": [n(40)], "repeat": [2, 4]});
        let hang = json!({"id": "hang", "file": "l.png", "anchor": {"name": "ceiling", "x": 4, "y": 1}, "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}});
        let h = |ms: u32| json!({"frame": "hang", "ms": ms});
        let pullups = json!({"entry": [h(100)], "loop": [h(200), h(200)], "exit": [h(100)], "repeat": [2, 5]});
        let moods = json!({"life": {"fidgets": [rep.clone(), [n(5), rep.clone()]], "poses": {"lean": rep.clone()}}, "extraFrames": [hang],
                           "brain": {"poses": {"hang": "hang"}, "hangFidgets": {"pullup": pullups}}, "activities": [[n(5)]]});
        let p = pack_with("repeat", moods).unwrap();
        let b = &p.behaviour;
        // As a beat, or as a step of one: one `Repeat` step.
        assert!(matches!(b.life.fidgets[0].as_slice(), [Step::Repeat(r)] if r.repeat == [2, 4]));
        assert!(matches!(b.life.fidgets[1].as_slice(), [Step::Hold { .. }, Step::Repeat(_)]));
        assert!(matches!(b.life.poses["lean"].as_slice(), [Step::Repeat(_)]));
        assert!(matches!(b.brain.hang_fidgets["pullup"].as_slice(), [Step::Repeat(r)] if r.repeat == [2, 5]), "a hang fidget's rep");
        // Each play: in once, the middle 2-4 times, out once.
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let seq = p.flatten(&b.life.fidgets[0]);
            let ms: Vec<u32> = seq.iter().map(|s| s.1).collect();
            let reps = (ms.len() - 2) / 2;
            assert!((2..=4).contains(&reps) && ms.len() == 2 + 2 * reps, "{ms:?}");
            assert_eq!((ms[0], ms[ms.len() - 1]), (10, 40));
            assert!(ms[1..ms.len() - 1].chunks(2).all(|c| c == [20, 30]));
            seen.insert(reps);
        }
        assert!(seen.len() > 1, "the count varies");
        // The rolls go on from her rng: the same seed, the same counts.
        let counts = |seed: u64| {
            p.seed_rolls(seed);
            (0..20).map(|_| p.flatten(&b.life.fidgets[0]).len()).collect::<Vec<_>>()
        };
        assert_eq!(counts(42), counts(42));
        // Past the cap (a pack built by hand, never through --check): held to it as it plays.
        let huge = Step::Repeat(Box::new(RepeatSteps { entry: vec![], r#loop: vec![Step::Hold { frame: "neutral".into(), ms: 1 }], exit: vec![], repeat: [1_000_000, u32::MAX] }));
        assert_eq!(p.flatten(&[huge]).len(), MAX_REPEAT as usize);
        // Validated like any steps: a bad repeat, an empty middle, a frame that isn't there.
        let bad = |r: serde_json::Value| format!("{:#}", pack_with("repeat-bad", json!({"life": {"fidgets": [r]}})).err().unwrap());
        let shape = |r: serde_json::Value, got: &str| {
            let e = bad(json!({"loop": [n(1)], "repeat": r}));
            assert!(e.contains(&format!("moods.json life.fidgets.0: `repeat` is {got}; it takes [min, max], whole numbers with 1 <= min <= max <= 64")), "{e}");
        };
        shape(json!([0, 2]), "[0,2]");
        shape(json!([3, 2]), "[3,2]");
        shape(json!([1, 65]), "[1,65]");
        shape(json!([1, 1_000_000_000]), "[1,1000000000]");
        shape(json!(3), "3");
        shape(json!([2]), "[2]");
        shape(json!(["2", 3]), "[\"2\",3]");
        assert!(pack_with("repeat-max", json!({"life": {"fidgets": [{"loop": [n(1)], "repeat": [64, 64]}]}})).is_ok(), "64 is fine");
        assert!(bad(json!({"loop": [], "repeat": [1, 2]})).contains("life.fidgets #0: the repeat's `loop` is empty"));
        assert!(bad(json!({"loop": [{"frame": "nope", "ms": 1}], "repeat": [1, 2]})).contains("frame `nope` does not resolve"));
        assert!(bad(json!({"entry": [n(1)], "loop": [n(1)]})).contains("moods.json life.fidgets.0: a one-shot beat's loop range needs `repeat`"));
    }

    #[test]
    fn a_bad_loop_range_fails_the_check_plainly() {
        let n = |ms: u32| json!({"frame": "neutral", "ms": ms});
        let err = |name: &str, m: serde_json::Value| format!("{:#}", pack_with(name, m).err().unwrap());
        // An empty loop between an entry and an exit.
        let e = err("lr-empty", json!({"moods": {"idle": {"base": "neutral", "loop": {"entry": [n(50)], "loop": [], "exit": [n(50)]}}}}));
        assert!(e.contains("mood `idle` loop: the loop range's `loop` is empty"), "{e}");
        let e = err("lr-empty-w", json!({"music": {"floor": "neutral", "notes": [], "windowLoop": {"entry": [n(50)]}}}));
        assert!(e.contains("music.windowLoop: the loop range's `loop` is empty"), "{e}");
        // A range on a list that doesn't loop: said where, not "invalid type: map".
        let e = err("lr-oneshot", json!({"moods": {"idle": {"base": "neutral", "entry": {"entry": [n(50)], "loop": [n(50)]}}}}));
        assert!(e.contains("moods.json moods.idle.entry: a loop range only goes on a looped list"), "{e}");
        let e = err("lr-click", json!({"click": {"head": {"loop": [n(50)], "exit": [n(50)]}}}));
        assert!(e.contains("moods.json click.head: a loop range only goes on a looped list"), "{e}");
        // A key it doesn't know (`outro`), or `repeat` on a list that loops anyway.
        let e = err("lr-key", json!({"energy": {"low": {"idle": {"base": "neutral", "loop": {"loop": [n(50)], "outro": [n(50)]}}}}}));
        assert!(e.contains("energy.low.idle.loop: unknown key `outro` in a loop range (entry|loop|exit|repeat)"), "{e}");
        let e = err("lr-rep", json!({"music": {"floor": "neutral", "notes": [], "windowLoop": {"loop": [n(50)], "repeat": [1, 2]}}}));
        assert!(e.contains("music.windowLoop: `repeat` is for one-shot beats"), "{e}");
        // Ranges don't nest.
        let e = err("lr-nest", json!({"moods": {"idle": {"base": "neutral", "loop": {"loop": [{"loop": [n(5)], "repeat": [1, 2]}]}}}}));
        assert!(e.contains("moods.idle.loop.loop.0: a loop range only goes on a looped list"), "{e}");
        // Well formed: loads.
        assert!(pack_with("lr-ok", json!({"brain": {"floorSit": {"base": "neutral", "loop": {"loop": [n(50)], "exit": [n(50)]}}}})).is_ok());
    }

    #[test]
    fn the_idle_pools_and_the_glance_resolve_like_activities() {
        let moods = json!({"life": {"fidgets": [[{"frame": "neutral", "ms": 200}], ["@look"]], "poses": {"lean": ["sheet"]}},
                           "energy": {"low": {"fidgets": [["greet"]]}}, "brain": {"glance": ["@look", {"frame": "neutral", "ms": 400}]}});
        pack_cels("pools-idle", moods, None, |p| {
            let p = p.unwrap();
            let b = &p.behaviour;
            assert_eq!((b.life.fidgets.len(), b.life.poses.len(), b.energy.low.fidgets.len(), b.brain.glance.len()), (2, 1, 1, 2));
            assert!(p.step_strips.len() == 2, "the lean and the sleepy fidget are strips");
        });
        let e = pack_with("pools-bad", json!({"life": {"fidgets": [[{"frame": "nope", "ms": 1}]]}})).err().unwrap();
        assert!(format!("{e:#}").contains("life.fidgets #0: frame `nope` does not resolve"));
    }

    #[test]
    fn effect_sprites_are_overlays_and_the_hearts_may_use_one() {
        let moods = json!({"extraFrames": [{"id": "heart", "file": "l.png", "kind": "effect"}], "click": {"hearts": "heart"}});
        let p = pack_with("hearts", moods).unwrap();
        assert!(p.effects.contains_key("heart") && !p.index.contains_key("heart"), "never one of her frames");
        assert_eq!(p.behaviour.click.hearts.as_deref(), Some("heart"));
        let e = pack_with("hearts-bad", json!({"click": {"hearts": "neutral"}})).err().unwrap();
        assert!(format!("{e:#}").contains("click.hearts: `neutral` is not an effect sprite"));
        // Without one: the drawn hearts (nothing to resolve).
        assert!(pack_with("hearts-none", json!({})).unwrap().behaviour.click.hearts.is_none());
    }

    #[test]
    fn an_outfit_draws_her_frames_and_she_never_mixes_two() {
        pack_setup("outfit", outfitted(), |d| write_outfit(d, &["neutral", "sit"]), |p| {
            let p = p.unwrap();
            assert_eq!(p.outfits.iter().map(|o| (o.id.as_str(), o.weight)).collect::<Vec<_>>(), [("main", 2.0), ("alt", 1.0)]);
            let (n, w) = (p.index["neutral"], p.index["wink"]);
            assert!(p.source_in(n, 1).path.ends_with("outfits/alt/neutral.png") && p.source_in(n, 0).path.ends_with("n.png"));
            assert_eq!(p.source_in(w, 1).path, p.frames[w].path, "a frame it doesn't draw decodes as the pack's");
            assert_eq!((p.wearing(), p.outfit_id()), (0, "main"));
            assert!(p.wears(w) && !p.flatten(&[Step::Hold { frame: "wink".into(), ms: 1 }]).is_empty());
            p.wear(1);
            assert_eq!(p.outfit_id(), "alt");
            assert!(p.wears(n) && !p.wears(w) && p.draws(1, n) && !p.draws(1, w));
            // A sequence with a frame it doesn't draw is skipped whole, never half one look.
            assert!(p.flatten(&[Step::Hold { frame: "neutral".into(), ms: 1 }, Step::Hold { frame: "wink".into(), ms: 1 }]).is_empty());
            assert_eq!(p.flatten(&[Step::Hold { frame: "neutral".into(), ms: 1 }]).len(), 1);
            p.wear(7);
            assert_eq!(p.wearing(), 1, "no such outfit: unchanged");
            assert_eq!(p.outfit_by_id("main"), Some(0));
        });
    }

    #[test]
    fn strips_are_tagged_per_outfit_and_an_outfit_must_draw_every_single_frame() {
        let mut moods = outfitted();
        moods["brain"] = json!({"hangFidgets": {"pullup": ["pullup"]}, "poses": {"hang": "hang"}});
        moods["extraFrames"].as_array_mut().unwrap().push(json!({"id": "hang", "file": "l.png", "anchor": {"name": "ceiling", "x": 4, "y": 1}, "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}}));
        let setup = |d: &Path| {
            crate::art::tests::write_anim(d);
            crate::art::tests::write_cels(d, Some("alt"));
            write_outfit(d, &["neutral", "sit", "hang"]);
        };
        pack_setup("outfit-strips", moods.clone(), setup, |p| {
            let p = p.unwrap();
            let a = p.anim.as_ref().unwrap();
            let (pullup, greet) = (a.by_name["pullup"], a.by_name["greet"]);
            assert!(!p.strip_wears(pullup) && p.strip_wears(greet), "the pack's own look: not the alt's strip");
            assert!(p.flatten(&[Step::Clip("pullup".into())]).is_empty());
            p.wear(1);
            assert!(p.strip_wears(pullup) && p.strip_wears(greet), "untagged: every look");
            assert_eq!(p.flatten(&[Step::Clip("pullup".into())]).len(), 1);
        });
        // It lacks the perch base (a single frame): refused.
        let e = pack_setup("outfit-lacks", outfitted(), |d| write_outfit(d, &["neutral"]), |p| format!("{:#}", p.err().unwrap()));
        assert!(e.contains("variants.outfits `alt`: lacks sit"), "{e}");
        // A strip tagged for an outfit the pack doesn't have.
        let e = pack_cels("outfit-unknown", json!({}), Some("beach"), |p| format!("{:#}", p.err().unwrap()));
        assert!(e.contains("anim strip `pullup`: outfit `beach` is not in moods.json variants.outfits"), "{e}");
    }

    /// Two pools over the test strips (`crate::art::tests::write_anim`).
    pub fn pooled() -> serde_json::Value {
        json!({"perch": {"base": "sit"}, "variants": {"cacheMb": 2, "pools": {
            "perch": {"shiftMs": [1000, 2000], "variants": [{"id": "sit", "weight": 2}, {"id": "lie", "strip": "lie", "lying": true, "afterMs": 240000, "stayMs": [60000, 180000]}]},
            "sleepFloor": {"variants": [{"id": "pack"}, {"id": "doze", "strip": "stand", "every": 2, "holdMs": [500, 900]}]}}}})
    }

    #[test]
    fn pools_resolve_and_their_strips_become_registered_frames() {
        pack_in("pools", pooled(), true, |p| {
            let p = p.unwrap();
            let a = p.anim.as_ref().unwrap();
            let (stand, lie) = (a.by_name["stand"], a.by_name["lie"]);
            let perch = &p.pools[&PoolKey::Perch];
            assert_eq!(perch.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["sit", "lie"]);
            assert!(perch.variants[0].art.is_none() && perch.variants[1].lying);
            // A beat for a long stay, with its own length.
            assert_eq!((perch.variants[0].after_ms, perch.variants[1].after_ms), (None, Some(240000)));
            assert_eq!(perch.variants[1].stay_ms, Some([60000, 180000]));
            assert_eq!(perch.variants[1].art.as_ref().unwrap().base, lie);
            assert_eq!(p.cache_bytes, 2_000_000);
            // Cells: lie ×2 on the seat line, stand ×3 on the ground, stand's twin ×3 mirrored.
            let l0 = p.strip_first[lie].unwrap();
            assert_eq!(p.frames[l0].id, "lie#0");
            assert!(p.perch_frames.contains("lie#1") && !p.perch_frames.contains("stand#0"));
            let (_, dy) = p.offset(&p.frames[l0]);
            assert_eq!(p.frames[l0].anchor.y + dy, p.seat_ref.unwrap(), "her surface on the seat line");
            let s0 = p.strip_first[stand].unwrap();
            let c = p.catalog.canonical_anchor;
            let (dx, dy) = p.offset(&p.frames[s0]);
            assert_eq!((p.frames[s0].anchor.x + dx, p.frames[s0].anchor.y + dy), (c.x, c.y), "feet on the ground point");
            let t1 = p.twin(s0 + 1).unwrap();
            assert_eq!(p.frames[t1].id, "~stand_flipped#1");
            assert!(p.frames[t1].cell.as_ref().unwrap().mirror && p.twin(t1).is_none() && p.twin(l0).is_none());
            assert_eq!(p.strip_seq(stand, 2), vec![(s0, 200), (s0 + 2, 200)], "every 2nd cell, held twice as long");
            // Only the cells it plays are decoded (its twin's too); a strip no variant thins keeps all.
            let tw = p.mirror_first[stand].unwrap();
            assert_eq!((p.strip_cells(stand, false), p.strip_cells(stand, true)), (vec![s0, s0 + 2], vec![tw, tw + 2]));
            assert_eq!(p.strip_cells(lie, false), vec![l0, l0 + 1]);
            let doze = &p.pools[&PoolKey::SleepFloor].variants[1];
            assert_eq!(doze.art.as_ref().unwrap().hold_ms, Some([500, 900]));
            assert_eq!(p.art_of(s0), p.art_of(t1));
            assert_ne!(p.art_of(s0), p.art_of(l0));
            assert_eq!(p.art_of(p.index["neutral"]), crate::variants::Art::Pack);
            assert_eq!(p.frames.iter().filter(|f| f.cell.is_some()).count(), 8);
        });
    }

    #[test]
    fn without_anim_a_pool_is_the_packs_own_art() {
        let p = pack_with("noanim", pooled()).unwrap();
        assert!(p.anim.is_none() && p.frames.iter().all(|f| f.cell.is_none()));
        assert_eq!(p.pools[&PoolKey::Perch].variants.len(), 1);
        assert_eq!(p.pools[&PoolKey::SleepFloor].variants[0].id, "pack");
        // No `variants` at all: no pools.
        assert!(pack_with("novariants", json!({})).unwrap().pools.is_empty());
    }

    #[test]
    fn a_pool_takes_the_right_anchor_and_loops_only() {
        let bad = |pool: &str, v: serde_json::Value| {
            let mut m = pooled();
            m["variants"]["pools"][pool]["variants"] = v;
            pack_in("badpool", m, true, |p| p.err().map(|e| format!("{e:#}")).unwrap_or_default())
        };
        assert!(bad("perch", json!([{"id": "x", "strip": "stand"}])).contains("strip `stand` needs a seat anchor"));
        assert!(bad("sleepFloor", json!([{"id": "x", "strip": "lie"}])).contains("needs a ground anchor"));
        assert!(bad("perch", json!([{"id": "x", "strip": "nope"}])).contains("strip `nope` is not in anim/anim.json"));
        assert!(bad("perch", json!([{"id": "x"}, {"id": "x"}])).contains("ids unique"));
        let mut m = pooled();
        m["variants"]["pools"]["nap"] = json!({"variants": [{"id": "a"}]});
        assert!(pack_in("badkey", m, true, |p| format!("{:#}", p.err().unwrap())).contains("unknown pool `nap`"));
    }

    #[test]
    fn motions_name_looping_strips_with_their_anchor_and_the_click_an_effect() {
        let with = |mo: serde_json::Value, anim: bool| {
            let mut m = pooled();
            m["motions"] = mo;
            pack_in("motions", m, anim, |p| {
                p.map_err(|e| format!("{e:#}")).map(|p| {
                    let grip = p.anim.as_ref().and_then(|a| a.by_name.get("hang").copied()).and_then(|si| p.strip_first[si]).map(|i| p.offset(&p.frames[i]));
                    (p.motions.clone(), p.strip_first.clone(), p.anim.as_ref().map(|a| a.by_name.clone()), grip, p.catalog.canonical_anchor)
                })
            })
        };
        let (mo, first, names, grip, c) = with(json!({"walk": {"strip": "stand"}, "dance": {"strip": "stand"}, "dangle": {"strip": "hang"}, "click": {"strip": "click"}}), true).unwrap();
        let names = names.unwrap();
        assert_eq!((mo.walk, mo.dance, mo.dangle), (Some(names["stand"]), Some(names["stand"]), Some(names["hang"])));
        assert_eq!(mo.click.as_deref(), Some("click"));
        assert!(first[names["stand"]].is_some() && first[names["hang"]].is_some(), "their cells are frames");
        // The dangle hangs where she stands: its grip on her x, its feet on her ground line.
        let (dx, dy) = grip.unwrap();
        assert!(dx != 0 || dy != 0, "registered on the canonical anchor ({c:?}), not the canvas corner");
        assert!(with(json!({"walk": {"strip": "perch_in"}}), true).unwrap_err().contains("motions.walk: strip `perch_in` needs a ground anchor and to loop"));
        assert!(with(json!({"dangle": {"strip": "stand"}}), true).unwrap_err().contains("needs a grip anchor"));
        assert!(with(json!({"dance": {"strip": "nope"}}), true).unwrap_err().contains("is not in anim/anim.json"));
        assert!(with(json!({"click": {"strip": "stand"}}), true).unwrap_err().contains("not an effect strip"));
        // Without anim/: none of them, and no error.
        let (mo, ..) = with(json!({"walk": {"strip": "walk"}, "click": {"strip": "click"}}), false).unwrap();
        assert!(mo.walk.is_none() && mo.click.is_none());
    }

    /// `outfitted`'s looks, `main` and `alt`, with catalog clip `wave` (neutral, sit).
    fn waving() -> serde_json::Value {
        json!({"perch": {"base": "sit"}, "activities": [["wave"]],
               "variants": {"outfits": [{"id": "main"}, {"id": "alt", "sprites": "outfits/alt/sprites.json"}]}})
    }

    fn add_wave_clip(d: &Path) {
        let p = d.join("sprites.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        v["clips"] = json!([{"id": "wave", "frames": [{"id": "neutral", "ms": 100}, {"id": "sit", "ms": 100}]}]);
        std::fs::write(&p, v.to_string()).unwrap();
    }

    #[test]
    fn a_strip_drawn_for_one_outfit_plays_in_place_of_the_clip_it_is_named_for() {
        let setup = |d: &Path| {
            crate::art::tests::write_anim(d);
            crate::art::tests::write_own(d, "wave", Some("alt"));
            write_outfit(d, &["neutral", "sit"]);
            add_wave_clip(d);
        };
        pack_setup("outfitclip", waving(), setup, |p| {
            let p = p.unwrap();
            let ids = |p: &Pack| p.clip("wave").unwrap().frames.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
            assert_eq!(ids(&p), ["neutral", "sit"], "the pack's look: the catalog's clip");
            assert_eq!(p.catalog.clips.iter().filter(|c| c.id == "wave").count(), 1);
            p.wear(1);
            assert_eq!(ids(&p), ["wave#0", "wave#1"], "the alt look: its own drawing");
            let seq = p.flatten(&[Step::Clip("wave".into())]);
            assert_eq!(seq.iter().map(|s| s.1).collect::<Vec<_>>(), [100, 100]);
            assert!(seq.iter().all(|s| p.wears(s.0)));
            // The pack's own art: no seam to or from it.
            assert_eq!(p.art_of(seq[0].0), crate::variants::Art::Pack);
            p.wear(0);
            assert!(!p.wears(seq[0].0), "never shown in the other look");
        });
    }

    #[test]
    fn a_held_cell_of_a_strip_is_a_frame() {
        let moods = json!({"activities": [[{"frame": "sheet#2", "ms": 400}]]});
        pack_cels("cellframe", moods, None, |p| {
            let p = p.unwrap();
            let sheet = p.anim.as_ref().unwrap().by_name["sheet"];
            assert!(p.step_strips.contains(&sheet), "its cells are frames");
            assert_eq!(p.flatten(&p.behaviour.activities[0]), [(p.index["sheet#2"], 400)]);
            assert_eq!(p.frames[p.index["sheet#2"]].cell.as_ref().map(|c| c.strip), Some(sheet));
        });
        let e = pack_cels("cellframe-bad", json!({"activities": [[{"frame": "sheet#9", "ms": 400}]]}), None, |p| format!("{:#}", p.err().unwrap()));
        assert!(e.contains("frame `sheet#9` does not resolve"), "{e}");
    }

    #[test]
    fn an_outfit_sprite_takes_a_patch() {
        let patched = |d: &Path, file: &str| {
            write_outfit(d, &["neutral", "sit"]);
            let p = d.join("outfits/alt/sprites.json");
            let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            v["sprites"][1]["patch"] = json!({"file": file, "rect": {"x": 0, "y": 0, "width": 8, "height": 4}, "dx": -2, "dy": 1});
            std::fs::write(&p, v.to_string()).unwrap();
        };
        pack_setup("outfitpatch", outfitted(), |d| patched(d, "n.png"), |p| {
            let p = p.unwrap();
            let src = &p.outfits[1].frames[&p.index["sit"]];
            let (file, rect, dx, dy, _) = src.patch.as_ref().unwrap();
            assert!(file.ends_with("n.png"), "from the pack's root");
            assert_eq!((rect.height, *dx, *dy), (4, -2, 1));
            assert!(p.outfits[1].frames[&p.index["neutral"]].patch.is_none());
        });
        let e = pack_setup("outfitpatch-bad", outfitted(), |d| patched(d, "gone.png"), |p| format!("{:#}", p.err().unwrap()));
        assert!(e.contains("sprite `sit`: patch") && e.contains("gone.png is missing"), "{e}");
    }

    #[test]
    fn transitions_name_one_shot_strips_with_their_anchor() {
        let with = |t: serde_json::Value, anim: bool| {
            let mut m = pooled();
            m["transitions"] = t;
            pack_in("trans", m, anim, |p| p.map_err(|e| format!("{e:#}")).map(|p| (p.transitions.clone(), p.strip_first.clone(), p.anim.as_ref().map(|a| a.by_name.clone()))))
        };
        let (t, first, names) = with(json!({"perchLeave": "perch_in"}), true).unwrap();
        let si = names.unwrap()["perch_in"];
        assert_eq!(t.get("perchLeave"), Some(&si));
        assert!(first[si].is_some(), "its cells are frames");
        assert!(with(json!({"perchLeave": "lie"}), true).unwrap_err().contains("needs a seat anchor and to be a one-shot"), "a loop");
        assert!(with(json!({"perchLeave": "stand"}), true).unwrap_err().contains("needs a seat anchor"));
        assert!(with(json!({"wake": "perch_in"}), true).unwrap_err().contains("needs a ground anchor"));
        assert!(with(json!({"perchLeave": "gone"}), true).unwrap_err().contains("`gone` is not in anim/anim.json"));
        assert!(with(json!({"perchArrive": "perch_in"}), true).unwrap_err().contains("unknown change `perchArrive` (perchLeave|floorArrive|floorLeave|wake|form)"));
        // A pack without anim/: none, and no error.
        assert!(with(json!({"perchLeave": "perch_in"}), false).unwrap().0.is_empty());
    }

    fn extra(id: &str, anchor: &str) -> serde_json::Value {
        json!({"id": id, "file": "l.png", "anchor": {"name": anchor, "x": 4, "y": 7}, "visibleBounds": {"x": 0, "y": 2, "width": 8, "height": 5}})
    }

    #[test]
    fn arrivals_nap_and_kiss_load_and_register_on_the_seat() {
        let p = pack_with(
            "ok",
            json!({
                "extraFrames": [extra("land", "ground"), extra("land-seat", "seat"), extra("lie", "ground"), extra("lie-seat", "seat"),
                                {"id": "pat-seat", "file": "s.png", "anchor": {"name": "seat", "x": 4, "y": 7},
                                 "visibleBounds": {"x": 1, "y": 1, "width": 6, "height": 6}, "patch": {"file": "n.png", "rect": {"x": 0, "y": 0, "width": 8, "height": 4}, "dx": -1}}],
                "perch": {"base": "sit", "headpat": [{"frame": "pat-seat", "ms": 900}]},
                "arrive": {"launch": [{"frame": "land", "ms": 300}, {"frame": "neutral", "ms": 300}], "teleport": [{"frame": "land", "ms": 300}],
                           "perch": [{"frame": "land-seat", "ms": 300}], "teleportChance": 0.5},
                "kiss": {"steps": [{"frame": "neutral", "ms": 100}], "chance": 0.1, "heartsAtMs": 50, "gapMs": 1000},
                "brain": {"nap": "neutral", "napDeep": "lie", "napPerch": "lie-seat", "napWake": [{"frame": "neutral", "ms": 100}],
                          "bigHopCrouch": "land", "startle": [{"frame": "land", "ms": 100}], "lookFront": "neutral"},
                "icon": "n.png"
            }),
        )
        .unwrap();
        for id in ["land-seat", "lie-seat", "pat-seat", "sit"] {
            assert!(p.perch_frames.contains(id), "{id} sits on the seat line");
        }
        assert!(!p.perch_frames.contains("land") && !p.perch_frames.contains("lie"));
        let pat = &p.frames[p.index["pat-seat"]];
        assert_eq!(pat.patch.as_ref().map(|x| (x.1.width, x.2, x.3)), Some((8, -1, 0)));
        assert_eq!(p.flatten(&p.behaviour.arrive.as_ref().unwrap().launch).len(), 2);
        assert_eq!(p.behaviour.brain.nap_deep.as_deref(), Some("lie"));
        assert_eq!(p.behaviour.icon.as_deref(), Some("n.png"));
    }

    #[test]
    fn a_window_top_takes_seat_frames_only() {
        let base = |k: &str, v: serde_json::Value| {
            let mut m = json!({"extraFrames": [extra("land", "ground")], "perch": {"base": "sit"}});
            match k {
                "arrive" => m["arrive"] = v,
                "headpat" => m["perch"]["headpat"] = v,
                "napPerch" => m["brain"] = json!({"napPerch": v}),
                _ => m[k] = v,
            }
            m
        };
        let err = |n: &str, m| pack_with(n, m).err().map(|e| format!("{e:#}")).unwrap_or_default();
        assert!(err("a", base("arrive", json!({"perch": [{"frame": "land", "ms": 1}]}))).contains("arrive.perch: frame `land` has no seat anchor"));
        assert!(err("b", base("headpat", json!([{"frame": "land", "ms": 1}]))).contains("perch.headpat: frame `land` has no seat anchor"));
        assert!(err("c", base("napPerch", json!("land"))).contains("napPerch: frame `land` has no seat anchor"));
        assert!(err("d", base("arrive", json!({"teleportChance": 1.5}))).contains("teleportChance"));
        assert!(err("e", base("arrive", json!({"launch": [{"frame": "nope", "ms": 1}]}))).contains("arrive.launch: frame `nope` does not resolve"));
        assert!(err("f", base("kiss", json!({"steps": ["nope"]}))).contains("kiss.steps: clip `nope` does not exist"));
    }

    #[test]
    fn the_capri_moods_use_the_assets_as_approved() {
        let b: Behaviour = serde_json::from_str(include_str!("../packs/capri/moods.json")).expect("moods.json");
        let a = b.arrive.as_ref().expect("arrive");
        let ids = |s: &[Step]| -> Vec<String> {
            s.iter().map(|x| match x {
                Step::Hold { frame, .. } => frame.clone(),
                Step::Clip(c) => c.clone(),
                Step::Repeat(_) => String::new(),
            })
            .collect()
        };
        // Her plain red bolt replaces the intro's first frame (Haseeb 08:06 §6.1); 07 is neutral.
        assert_eq!(ids(&a.launch), ["intro-gaze", "intro-arrival", "intro-landing", "intro-greeting", "intro-wink", "neutral"]);
        assert!(!ids(&a.summon).contains(&"intro-wink".to_string()), "no wink on a summon");
        // Every mood has a face cropped to her eyes and mouth (r35: whole images read as hair).
        assert!(crate::anim::Mood::ALL.iter().all(|m| b.line_faces.get(m.key()).is_some_and(|f| f.crop().is_some())));
        let w: LineFace = serde_json::from_str("\"face-calm\"").unwrap();
        assert_eq!((w.image(), w.crop()), ("face-calm", None));
        // r30: the touchdown is the crouch, then the sit inside a dip (no dissolve: no double
        // image, and not `transition-edge_perch`: r29 had it as legs through the edge). r31's
        // pop is answered by the sit's legs unfolding over the edge (`arriveview::UNFOLD_MS`).
        assert_eq!(ids(&a.perch), ["intro-landing-seat"]);
        // B2c: every motion is in, rate-capped, and the walk never outpaces its drawing.
        let m = &b.motions;
        let (w, d, g) = (m.walk.as_ref().unwrap(), m.dance.as_ref().unwrap(), m.dangle.as_ref().unwrap());
        assert!((w.strip.as_str(), d.strip.as_str(), g.strip.as_str()) == ("walk", "dance", "dangle"));
        assert!(w.gap_ms >= 10_000 && d.gap_ms >= 300_000 && g.gap_ms >= 10_000);
        // Haseeb 11-17: no click burst (the rings where you click her). The strip stays loadable.
        assert!(m.click.is_none(), "no click burst");
        assert!(w.px_per_s <= 90.0 && w.min_px >= 60.0 && (0.0..=1.0).contains(&w.chance));
        assert!(d.ms[1] <= 120_000 && d.min_bpm >= 90.0, "a dance is a while, not the song");
        assert!((0.0..=1.0).contains(&g.chance), "the dangle is an A/B with the ragdoll");
        assert!(a.perch_dip_ms > 0 && a.perch_dip_ms < 80, "the swap near the squash's trough (80 ms)");
        // One perch frame (§6.2): no per-mood or activity swaps while perched.
        let p = b.perch.as_ref().unwrap();
        assert!(p.moods.is_empty() && p.activities.is_empty() && p.click.is_empty());
        assert_eq!(b.brain.nap_deep.as_deref(), Some("pose-sleep"));
        assert_eq!(b.moods["sleepy"].base, "pose-sleep");
        assert_eq!(b.moods["away"].base, "recline-sleepy", "OFFLINE stays distinguishable");
        let k = b.kiss.as_ref().unwrap();
        assert!(k.chance <= 0.15 && k.gap_ms >= 300_000, "rare");
        // No 1.1-only frames: the v1.0.4 look (§6.3) plus the one pinned pose-sleep.
        let files: Vec<&str> = b.extra_frames.iter().map(|x| x.file.as_str()).collect();
        assert!(files.iter().all(|f| !f.contains("01-signal") && !f.contains("07-ready")));
        assert_eq!(files.iter().filter(|f| f.contains("capri-sleep")).count(), 2);
        // v1.2.2 (Haseeb 11-21): the no-jacket look is her second outfit and waves with its own
        // drawing (the `wave` strip, tagged for it); the jacket look keeps the catalog's wave.
        let outfits: Vec<&str> = b.variants.as_ref().unwrap().outfits.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(outfits, ["jacket", "no-jacket"]);
        assert_eq!(ids(&b.brain.greet).first().map(String::as_str), Some("wave"));
        let cels = |s: &[Step], strip: &str| ids(s).iter().any(|f| f.starts_with(&format!("{strip}#")));
        assert!(cels(&b.music.as_ref().unwrap().window_loop, "ledge_keytar"), "the keytar on a window top");
        assert!(cels(&b.brain.hang_fidgets["pullup"], "hang_pullup"));
    }
}
