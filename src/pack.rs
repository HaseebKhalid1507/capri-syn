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

/// One step of a scripted sequence: a catalog clip id, or one frame held for `ms`.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Step {
    Clip(String),
    Hold { frame: String, ms: u32 },
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

/// A clip id, or steps written inline.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Steps {
    Clip(String),
    Inline(Vec<Step>),
}

impl Steps {
    pub fn as_steps(&self) -> Vec<Step> {
        match self {
            Steps::Clip(id) => vec![Step::Clip(id.clone())],
            Steps::Inline(v) => v.clone(),
        }
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
    #[serde(default)]
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
    #[serde(default)]
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
    pub activities: Vec<Vec<Step>>,
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
    /// Per strip: only every n-th cell is ever played (the `every` of the variants using it),
    /// so only those are decoded.
    pub strip_every: Vec<u32>,
    /// The variant pools (`moods.json` `variants`); a strip variant is dropped when the pack
    /// has no `anim/`, so an old pack's pool is its own art alone.
    pub pools: HashMap<crate::variants::PoolKey, Pool>,
    /// `moods.json` `transitions`, resolved: strip indices into `anim`.
    pub transitions: HashMap<String, usize>,
    /// `moods.json` `motions`, resolved.
    pub motions: Motions,
    pub cache_bytes: usize,
}

impl Pack {
    pub fn frame(&self, id: &str) -> Result<usize> {
        self.index.get(id).copied().ok_or_else(|| anyhow!("unknown frame id `{id}`"))
    }

    pub fn clip(&self, id: &str) -> Option<&Clip> {
        let id = self.catalog.actions.iter().find(|a| a.id == id).map_or(id, |a| a.animation.as_str());
        self.catalog.clips.iter().find(|c| c.id == id)
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
            _ => 0,
        };
        // Grip anchors (ceiling, dock-left/right) keep their canvas registration: centring a
        // wall grip at x would push the body off the canvas.
        let dx = match f.anchor.name.as_str() {
            "ground" | "seat" | "grip" => c.x - f.anchor.x,
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
        for f in self.frames.iter().filter(|f| !self.fx_frame(f)) {
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
        let catalog: Catalog = serde_json::from_str(&read("sprites.json")?)
            .with_context(|| format!("pack: {}/sprites.json is not a valid sprite catalog", dir.display()))?;
        if catalog.schema_version != 2 {
            bail!("pack: sprites.json schemaVersion {} (expected 2)", catalog.schema_version);
        }
        let behaviour: Behaviour = match std::fs::read_to_string(dir.join("moods.json")) {
            Ok(s) => serde_json::from_str(&s)
                .with_context(|| format!("pack: {}/moods.json is invalid", dir.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Behaviour::default(),
            Err(e) => return Err(e).context("pack: cannot read moods.json"),
        };

        let mut errors = Vec::new();
        let mut frames = Vec::new();
        let mut index = HashMap::new();
        let (cw, ch) = (catalog.canvas.width as i32, catalog.canvas.height as i32);
        for s in &catalog.sprites {
            // Effects (e.g. a heart burst) are overlays, not body frames.
            if s.kind == "effect" {
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
        let ref_h = index.get(&catalog.default_sprite).map_or(1000, |&i| frames[i].bounds.height);
        let anim = crate::art::Anim::load(dir, ref_h)?;
        let n_strips = anim.as_ref().map_or(0, |a| a.strips.len());
        let (mut strip_first, mut mirror_first) = (vec![None; n_strips], vec![None; n_strips]);
        let pools = resolve_pools(&behaviour, anim.as_ref(), &mut errors);
        let transitions = resolve_transitions(&behaviour, anim.as_ref(), &mut errors);
        let motions = resolve_motions(&behaviour, anim.as_ref(), &mut errors);
        let mut strip_every = vec![u32::MAX; n_strips];
        for v in pools.values().flat_map(|p| &p.variants).filter_map(|v| v.art.as_ref()) {
            for si in std::iter::once(v.base).chain(v.talk).chain(v.music).chain(v.blinks.iter().map(|b| b.0)) {
                strip_every[si] = strip_every[si].min(v.every);
            }
        }
        let strip_every: Vec<u32> = strip_every.into_iter().map(|e| if e == u32::MAX { 1 } else { e }).collect();
        if let Some(a) = &anim {
            // Every strip a pool uses (and its mirrorArt twin, stored mirrored), as frames.
            let mut used: Vec<usize> = pools.values().flat_map(|p| &p.variants).filter_map(|v| v.art.as_ref()).flat_map(|v| {
                std::iter::once(v.base).chain(v.talk).chain(v.music).chain(v.blinks.iter().map(|b| b.0))
            })
            .chain(transitions.values().copied())
            .chain(motions.strips())
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
        };

        // Behaviour references.
        let check_steps = |what: &str, steps: &[Step], errors: &mut Vec<String>, pack: &Pack| {
            for s in steps {
                match s {
                    Step::Clip(id) if pack.clip(id).is_none() => {
                        errors.push(format!("moods.json {what}: clip `{id}` does not exist"))
                    }
                    Step::Hold { frame, .. } if !pack.index.contains_key(frame) => {
                        errors.push(format!("moods.json {what}: frame `{frame}` does not resolve"))
                    }
                    _ => {}
                }
            }
        };
        // Steps shown on a window top: seat-anchored frames only.
        let seat_only = |what: &str, steps: &[Step], errors: &mut Vec<String>, pack: &Pack| {
            for st in steps {
                let ids: Vec<String> = match st {
                    Step::Hold { frame, .. } => vec![frame.clone()],
                    Step::Clip(c) => pack.clip(c).map(|c| c.frames.iter().map(|f| f.id.clone()).collect()).unwrap_or_default(),
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
            for st in &l.perch {
                let ids: Vec<String> = match st {
                    Step::Hold { frame, .. } => vec![frame.clone()],
                    Step::Clip(c) => pack.clip(c).map(|c| c.frames.iter().map(|f| f.id.clone()).collect()).unwrap_or_default(),
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

    /// A strip's cells as a sequence, every `every`-th cell held `every` times as long.
    pub fn strip_seq(&self, si: usize, every: u32) -> Seq {
        let (Some(a), Some(Some(first))) = (&self.anim, self.strip_first.get(si)) else { return Vec::new() };
        let s = &a.strips[si];
        let e = every.max(1);
        (0..s.frames).step_by(e as usize).map(|c| (first + c as usize, s.ms * e)).collect()
    }

    /// The cells of strip `si` (or its twin's) that are ever played: the ones to decode.
    pub fn strip_cells(&self, si: usize, twin: bool) -> Vec<usize> {
        let first = if twin { self.mirror_first.get(si) } else { self.strip_first.get(si) };
        let (Some(Some(f)), Some(a)) = (first, &self.anim) else { return Vec::new() };
        let e = self.strip_every.get(si).copied().unwrap_or(1).max(1) as usize;
        (*f..*f + a.strips[si].frames as usize).step_by(e).collect()
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
            (Some(c), Some(a)) => crate::variants::Art::Family(a.strips[c.strip].family_id),
            _ => crate::variants::Art::Pack,
        }
    }

    /// Flatten steps into (frame index, ms) pairs.
    pub fn flatten(&self, steps: &[Step]) -> Vec<(usize, u32)> {
        let mut out = Vec::new();
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
            }
        }
        out
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
        let dir = std::env::temp_dir().join(format!("sa-pack-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        if anim {
            crate::art::tests::write_anim(&dir);
        }
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
        let out = keep(Pack::load(&dir));
        std::fs::remove_dir_all(&dir).ok();
        out
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
    }
}
