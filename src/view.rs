//! The avatar view: paints the current frame and the HUD (status tag, speech bubble,
//! menu, hearts), runs the effects, handles pointer input and placement (ground, perch,
//! teleport), and keeps the X input/bounding shape in step with what is drawn. State
//! changes go through `apply` (commands), `tick` (time) or the pointer handlers.

#[path = "brainview.rs"]
mod brainview;
#[path = "ragview.rs"]
mod ragview;
#[path = "lineview.rs"]
mod lineview;
#[path = "voiceview.rs"]
mod voiceview;
#[path = "arriveview.rs"]
mod arriveview;
#[path = "artview.rs"]
mod artview;
#[path = "motionview.rs"]
mod motionview;
#[path = "introview.rs"]
pub(crate) mod introview;
#[path = "cutinview.rs"]
mod cutinview;

use crate::anim::{Animator, Mood, Rng, Seq};
use crate::variants::PoolKey;
use crate::control::Command;
use crate::edges;
use crate::frames::{Frame, Geometry, NoteMasks};
use crate::fx::{self, BodyPlan, Bolt, Glint, Glitch, GlitchKind, Transition};
use crate::pack::{MoodSpec, MusicSpec, Pack, PerchSpec, Steps};
use crate::platform::{Os, Role, WatchEvent, WinRect};
use crate::session::{Act, Body, Link, Once, PromptView};
use crate::shape::{self, ShapeRect};
use crate::state;
use crate::ticker::Ticker;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use synaps_client::{Client, ClientOptions};

/// lachy's stills at her size: her portrait (the Line), capri.png (the menu header), the cursor
/// over her head. Each optional: a pack without `anim/` (or a file that failed) has none.
pub struct Stills {
    pub face: Option<Arc<RenderImage>>,
    /// Her face in the Line per mood (`moods.json` `lineFaces`, local images), by mood key.
    pub moods: std::collections::HashMap<String, Arc<RenderImage>>,
    pub capri: Option<Arc<RenderImage>>,
    pub capri_path: Option<std::path::PathBuf>,
    pub cursor: Option<crate::platform::PointerImage>,
}

/// The menu header's height (logical px) and the cursor's (her head-hover pointer).
const MENU_HEADER_PX: f32 = 96.0;
/// Her first words wait this long after the frames are in (the launch lands first), then
/// look again this often until nothing else is on.
const FIRST_WORDS_AFTER: Duration = Duration::from_millis(1_500);
const FIRST_WORDS_RETRY: Duration = Duration::from_millis(700);
const FIRST_WORDS_WITHIN: Duration = Duration::from_secs(180);
/// Her right-click menu's width, and its maker's mark's type size (logical px).
const MENU_W: f32 = 210.0;
const MARK_PX: f32 = 9.0;
const CURSOR_PX: f32 = 32.0;

/// A mood's face for the Line: the mood, its image (path, sha256), and its square (centre, size).
type MoodFace = (String, (std::path::PathBuf, Option<String>), Option<((u32, u32), u32)>);

impl Stills {
    fn decode(
        st: &std::collections::HashMap<String, (std::path::PathBuf, Option<String>)>,
        moods: &[MoodFace],
        sf: f32,
    ) -> Stills {
        // A mood's face: its square (eyes and mouth) at a whole-number scale, or all of it.
        let at = |name: &str, (p, sha): &(std::path::PathBuf, Option<String>), crop: Option<((u32, u32), u32)>, h: f32| {
            let px = (h * sf).round() as u32;
            let r = match crop {
                Some((c, s)) => crate::frames::face_tile(p, sha.as_deref(), c, s, px),
                None => crate::frames::still(p, sha.as_deref(), px),
            };
            match r {
                Ok(i) => Some(i),
                Err(e) => {
                    elog!("synaps-avatar: still `{name}`: {e:#}");
                    None
                }
            }
        };
        let get = |name: &str, h: f32| {
            let (p, sha) = st.get(name)?;
            match crate::frames::still(p, sha.as_deref(), (h * sf).round() as u32) {
                Ok(i) => Some(i),
                Err(e) => {
                    elog!("synaps-avatar: still `{name}`: {e:#}");
                    None
                }
            }
        };
        Stills {
            face: get("face", crate::line::FACE_PX).map(crate::frames::still_image),
            capri: get("capri", MENU_HEADER_PX).map(crate::frames::still_image),
            capri_path: st.get("capri").map(|s| std::fs::canonicalize(&s.0).unwrap_or_else(|_| s.0.clone())),
            // Its tip is the opaque box's top-left corner.
            cursor: get("cursor", CURSOR_PX).map(|i| crate::frames::still_pointer(&i, (0.0, 0.0), 1)),
            moods: moods.iter().filter_map(|(m, f, c)| Some((m.clone(), crate::frames::still_image(at(m, f, *c, crate::line::FACE_PX)?)))).collect(),
        }
    }
}

/// Window layout in device pixels (the display server works in these; GPUI gets them / scale factor).
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub sf: f32,
    pub geo: Geometry,
    pub win_w: i32,
    pub win_h: i32,
    pub avatar_x: i32,
    pub avatar_y: i32,
    /// Ground point (the feet) inside the window.
    pub ground_x: i32,
    pub ground_y: i32,
    /// Seat point of the perch frames inside the window (sitting on an edge).
    pub seat_y: Option<i32>,
    /// How far the lowest frame pixel reaches below the ground point.
    pub foot_pad: i32,
}

use crate::theme::{self, Cut, Fill, Pal, Plate, glyph, tracked};

// Logical px. The HUD follows Capri's CSS (cyberpunk.css, avatar-detach.css).
const MARGIN: f32 = 8.0;
/// `SA_FRAME_LOG`: every change of the frame on screen goes to the log (critic timing).
fn frame_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SA_FRAME_LOG").is_some())
}

/// After falling asleep or waking, a change of art family is the light swap this long (the
/// wake, a stretch and the greeting fit in it).
const QUIET_SEAMS: Duration = Duration::from_secs(8);
const TOP_MARGIN: f32 = 6.0;
const BODY_FONT: f32 = 20.0;
/// The command chip's face (the Line's and the reader's).
const MONO: &str = "DejaVu Sans Mono";
const LINE_H: f32 = 27.0;
/// The bubble grows to this many lines (and scrolls with the wheel past them).
const LINES_PER_PAGE: usize = 8;
/// A long reply (more lines than the bubble, or code) shows its opening lines and READ.
const OPENING_LINES: usize = 3;
/// Replies shorter than this type out; longer ones appear at once.
const TYPE_MAX_CHARS: usize = 160;
const PAD_T: f32 = 11.0;
const PAD_B: f32 = 8.0;
const PAD_L: f32 = 20.0;
const PAD_R: f32 = 15.0;
const HEADER_H: f32 = 12.0;
const HEADER_GAP: f32 = 8.0;
const FOOTER_GAP: f32 = 5.0;
const FOOTER_H: f32 = 21.0;
const PANEL_MIN_W: f32 = 340.0;
const PANEL_MAX_W: f32 = 470.0;
const PANEL_MAX_H: f32 = PAD_T + HEADER_H + HEADER_GAP + LINES_PER_PAGE as f32 * LINE_H + FOOTER_GAP + FOOTER_H + PAD_B;
const CUT: f32 = 18.0;
/// The menu's chamfers.
const MENU_CUT: Cut = Cut::Corners(12.0);
/// The mane slivers' height over the bubble's top edge.
const SLIVER_H: f32 = 8.0;
const TAIL_H: f32 = 16.0;
const TIP_GAP: f32 = 2.0;
const TAG_FONT: f32 = 13.0;
/// WORKING's sweep: it crosses the red block in `SWEEP_STEPS` steps of `SWEEP_STEP_MS`, once
/// every `SWEEP_EVERY_MS` (no redraws between).
const SWEEP_EVERY_MS: u64 = 2_400;
const SWEEP_STEPS: u64 = 10;
const SWEEP_STEP_MS: u64 = 70;
/// Clear air between the status tag and the hair right under it (logical px).
const TAG_GAP: f32 = 13.0;
const MIN_WIDTH: f32 = 500.0;
/// Everything that touches a perch edge keeps this far from the window's corners (logical px).
const EDGE_MARGIN: f32 = 60.0;
/// Where a perch sits by default, as a fraction of the window's width.
const PERCH_AT: f32 = 0.75;
/// Feet stay this far above the monitor's bottom edge (device px).
const GROUND_ROOM: i32 = 6;
/// The most a perch slides sideways to stay on an exposed stretch (logical px).
const LAND_SLIDE: f32 = 16.0;

/// x intervals, root px.
type Ranges = Vec<(i32, i32)>;

/// The point of `ranges` (inclusive) nearest `x`.
fn nearest_in(ranges: &[(i32, i32)], x: i32) -> Option<i32> {
    ranges.iter().map(|&(a, b)| x.clamp(a, b)).min_by_key(|&v| (v - x).abs())
}

impl Layout {
    pub fn new(pack: &Pack, sf: f32, body_height: f32) -> Layout {
        let geo = Geometry::new(pack, body_height * sf);
        let d = |l: f32| (l * sf).round() as i32;
        let win_w = (geo.width as i32).max(d(MIN_WIDTH));
        let avatar_x = (win_w - geo.width as i32) / 2;
        // Room above the highest frame for the bubble and its tail.
        let min_top = pack
            .frames
            .iter()
            .filter(|f| !pack.fx_frame(f))
            .map(|f| geo.to_device(0, f.bounds.y + pack.offset(f).1).1)
            .fold(f32::MAX, f32::min);
        let avatar_y = d(TOP_MARGIN + PANEL_MAX_H + TAIL_H + TIP_GAP) - min_top.round() as i32;
        let c = pack.catalog.canonical_anchor;
        let (gx, gy) = geo.to_device(c.x, c.y);
        let seat_y = pack.behaviour.perch.as_ref().and_then(|p| pack.index.get(&p.base)).map(|&i| {
            let f = &pack.frames[i];
            let (dx, dy) = pack.offset(f);
            avatar_y + geo.to_device(f.anchor.x + dx, f.anchor.y + dy).1.round() as i32
        });
        // Standing frames only (seat-anchored ones hang over an edge, not the floor).
        let foot_pad = pack
            .frames
            .iter()
            .filter(|f| f.anchor.name == "ground")
            .map(|f| {
                let (_, dy) = pack.offset(f);
                (geo.to_device(0, f.bounds.y + f.bounds.height + dy).1 - gy).ceil() as i32
            })
            .max()
            .unwrap_or(0)
            .max(0);
        Layout {
            sf,
            geo,
            foot_pad,
            win_w,
            // Room under the feet for the ragdoll's stretch (the shape hides it otherwise).
            win_h: avatar_y + geo.height as i32 + d(ragview::DRAG_PAD_B),
            avatar_x,
            avatar_y,
            ground_x: avatar_x + gx.round() as i32,
            ground_y: avatar_y + gy.round() as i32,
            seat_y,
        }
    }

    fn lp(&self, d: i32) -> Pixels {
        px(d as f32 / self.sf)
    }
}

/// An image's offset and size inside the avatar rect (device px).
type ImageBox = ((i32, i32), (i32, i32));

#[derive(Clone, Debug)]
struct Wrapped {
    /// Char ranges of the wrapped lines (a line keeps its trailing space).
    lines: Vec<(usize, usize)>,
    /// Widest line, logical px.
    width: f32,
}

struct Bubble {
    text: Vec<char>,
    shown: usize,
    per_tick: usize,
    next_char: Option<Instant>,
    hide_at: Option<Instant>,
    wrap: Option<Wrapped>,
    /// The first line shown (the wheel scrolls it); None: from the top.
    page: Option<usize>,
    /// The reply as written (markdown), for the reader.
    raw: String,
    collapsed: bool,
    /// A session reply: it collapses to the chip instead of vanishing, and the mood is the
    /// session's business.
    stream: bool,
    /// The turn still runs: more text may come.
    more: bool,
    /// Widest the panel has been: it never narrows while open (a narrower shape would
    /// land before the narrower present and clip the old one for a frame).
    width: f32,
}

/// How long a typed-out bubble stays open.
fn hold_for(chars: usize) -> Duration {
    Duration::from_millis((3_000 + 45 * chars.min(240) as u64).min(12_000))
}

/// Laid-out bubble buttons (for test clicks via `state`).
type Buttons = Rc<RefCell<Vec<(&'static str, Bounds<Pixels>)>>>;

/// The attached Synaps session.
pub struct SessionLink {
    pub client: Client,
    pub link: Link,
    /// The body as last applied.
    shown: Option<Body>,
    /// Shutting down: Detach + Bye are on their way.
    closing: bool,
    /// How to make the client again (after its thread stopped or a long outage).
    options: ClientOptions,
    /// Where her session id is remembered (profile or socket).
    key: String,
    /// Bumped per client: a stopped pump of an old client is not news.
    generation: u64,
    respawn_at: Option<Instant>,
}

/// Re-create the client when it has been offline this long (its own reconnects gave up,
/// or the tunnel is still coming back). `SA_RESPAWN_AFTER=<secs>` for tests.
fn respawn_after() -> Duration {
    std::env::var("SA_RESPAWN_AFTER").ok().and_then(|v| v.parse().ok()).map_or(Duration::from_secs(90), Duration::from_secs_f64)
}
/// After the client's thread stopped by itself, start a new one this soon.
const RESPAWN_SOON: Duration = Duration::from_secs(5);

/// The text field in the bubble.
struct InputBox {
    text: String,
    /// The window that had the keyboard (`_NET_ACTIVE_WINDOW`): it gets it back.
    prev: Option<u32>,
    /// Escape pressed once while a turn runs: a second one within a second aborts.
    esc_at: Option<Instant>,
    /// Answering this secret prompt (masked) instead of sending a message.
    secret: Option<u64>,
}

struct PromptUi {
    view: PromptView,
    shown_at: Instant,
    /// Escape or the chevron: hidden behind the chip, still pending.
    dismissed: bool,
    /// From the dev `prompt` command: answers go nowhere.
    fake: bool,
}

/// Buttons of a prompt ignore clicks this long after it shows (a click meant for
/// something else must not answer it).
const PROMPT_ARM: Duration = Duration::from_millis(400);
/// Escape twice within this, while a turn runs, aborts it. One Escape closes the field
/// when it runs out.
const ESC_AGAIN: Duration = Duration::from_millis(1_000);
const INPUT_MAX: usize = 4_000;

impl Bubble {
    /// More lines than the bubble holds, or code: the opening lines plus READ.
    fn long(&self) -> bool {
        self.wrap.as_ref().is_some_and(|w| w.lines.len() > LINES_PER_PAGE) || self.raw.contains("```") || self.raw.contains("~~~")
    }

    /// Lines the bubble shows at once.
    fn rows(&self) -> usize {
        let total = self.wrap.as_ref().map_or(1, |w| w.lines.len().max(1));
        if self.long() { OPENING_LINES.min(total) } else { total.min(LINES_PER_PAGE) }
    }

    /// (first line shown, line count). Before wrapping: (0, 1).
    fn pages(&self) -> (usize, usize) {
        let Some(w) = &self.wrap else { return (0, 1) };
        let total = w.lines.len().max(1);
        (self.page.unwrap_or(0).min(total.saturating_sub(self.rows())), total)
    }

    /// Lines in the reader: the raw reply's.
    fn read_lines(&self) -> usize {
        self.raw.lines().count().max(self.wrap.as_ref().map_or(1, |w| w.lines.len()))
    }

    /// The revealed text in view, one string per line.
    fn page_lines(&self) -> Vec<String> {
        let Some(w) = &self.wrap else { return Vec::new() };
        let (top, _) = self.pages();
        w.lines
            .iter()
            .skip(top)
            .take(self.rows())
            .take_while(|&&(s, _)| s < self.shown)
            .map(|&(s, e)| self.text[s..e.min(self.shown)].iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    /// The page's lines, its last one ending "…" when the text goes on past the page (r21: it
    /// was cut mid-sentence with no sign).
    fn page_lines_marked(&self) -> Vec<String> {
        let mut lines = self.page_lines();
        let (top, total) = self.pages();
        if top + self.rows() < total
            && self.shown >= self.text.len()
            && let Some(last) = lines.last_mut()
        {
            *last = goes_on(last);
        }
        lines
    }

    /// Type out short text only; anything longer appears at once.
    fn reveal_all_if_long(&mut self) {
        if self.text.len() >= TYPE_MAX_CHARS {
            self.shown = self.text.len();
        }
    }
}

/// Where her menu's top left goes (window-local logical px, `x` and `y` its allowed ranges: her
/// work area less its size): under the click, moved as far as it must to fit, and then off her
/// face (`face`: x0, y0, x1, y1) if that put it over it: beside the face (the click's side
/// first), else under it, else above it; over it only when none fits.
fn menu_at(click: (f32, f32), (w, h): (f32, f32), (min_x, max_x): (f32, f32), (min_y, max_y): (f32, f32), face: Option<(f32, f32, f32, f32)>) -> (f32, f32) {
    const GAP: f32 = 8.0;
    let x = click.0.min(max_x).max(min_x);
    let y = click.1.min(max_y).max(min_y);
    let Some((f0, f1, f2, f3)) = face else { return (x, y) };
    let over = |x: f32, y: f32| x < f2 + GAP && x + w > f0 - GAP && y < f3 + GAP && y + h > f1 - GAP;
    if !over(x, y) {
        return (x, y);
    }
    let (right, left) = (f2 + GAP, f0 - GAP - w);
    let sides = if click.0 >= (f0 + f2) / 2.0 { [right, left] } else { [left, right] };
    if let Some(sx) = sides.into_iter().find(|&sx| sx >= min_x && sx <= max_x) {
        return (sx, y);
    }
    if f3 + GAP <= max_y {
        return (x, f3 + GAP);
    }
    if f1 - GAP - h >= min_y {
        return (x, f1 - GAP - h);
    }
    (x, y)
}

/// An open menu at `at` that her face (after a pose change) is now under: the place across the
/// face (its far side), same height, else the near side, whichever is in `x` first; else None
/// (it stays).
fn menu_reflow(at: (f32, f32), (w, h): (f32, f32), (min_x, max_x): (f32, f32), face: Option<(f32, f32, f32, f32)>) -> Option<(f32, f32)> {
    const GAP: f32 = 8.0;
    let (f0, f1, f2, f3) = face?;
    let over = at.0 < f2 && at.0 + w > f0 && at.1 < f3 && at.1 + h > f1;
    if !over {
        return None;
    }
    let (right, left) = (f2 + GAP, f0 - GAP - w);
    // The face in the menu's right half: across it is its right.
    let sides = if (f0 + f2) / 2.0 > at.0 + w / 2.0 { [right, left] } else { [left, right] };
    sides.into_iter().find(|x| (min_x..=max_x).contains(x)).map(|x| (x, at.1))
}

/// `line` with "…" in place of its last word(s): at least three characters give way, so it is
/// never wider than the wrapped line it replaces.
fn goes_on(line: &str) -> String {
    let mut t = line.trim_end();
    let full = t.chars().count();
    while full - t.chars().count() < 3 {
        match t.trim_end().rfind(' ') {
            Some(i) if i > 0 => t = t[..i].trim_end(),
            _ => {
                let keep = full.saturating_sub(3);
                return format!("{}…", line.chars().take(keep).collect::<String>().trim_end());
            }
        }
    }
    format!("{}…", t.trim_end_matches([',', ';', ':', '-', '—']))
}

const TYPE_TICK: Duration = Duration::from_millis(35);
const HEARTS_MS: u64 = 1_150;
/// The effect layer of the teleport bolt: one plain red line (LOCKED #1).
const BOLT_LAYER: usize = 0;
/// The bubble and tag come back this long after a loud teleport has reformed her.
const HUD_AFTER_TELEPORT: Duration = Duration::from_millis(200);
/// After a landing: her first sparkle, breath step and blink this soon.
const LIFE_SPARKLE: Duration = Duration::from_millis(150);
const LIFE_BREATH: Duration = Duration::from_millis(300);
const LIFE_BLINK_MS: u32 = 500;
/// A drawn floor leave from another stand drawing starts this many cells in (inside its tear).
const LEAVE_CUT_IN: usize = 2;
/// First effect layer of the hearts: a rim and a fill per heart.
const HEART_LAYER: usize = 6;
/// The contact shadow under a perch, stacked right below the avatar window.
const SHADOW_LAYER: usize = 14;
/// Over her while a drawn touchdown is its red silhouette (`sync_cover`).
const COVER_LAYER: usize = GLINT_LAYER + 2 * GLINTS_MAX + 3;
/// It comes this long after an arrival's last frame (the pose's present lands a frame later).
const SHADOW_AFTER_ARRIVAL: Duration = Duration::from_millis(50);
/// Music notes: an outline and a fill layer each, at most seven notes.
const NOTE_LAYER: usize = 15;
const NOTES_MAX: usize = 7;
/// Notes are drawn this much larger than Capri's canvas sizes (our figure is smaller).
const NOTE_SCALE: f32 = 1.15;
/// The farthest a pose step moves any row (nod, breath, sway), logical px. Posed frames
/// paint the art at this reach (and half of it) in 8 directions underneath the bands: the
/// bounding shape is set when a frame paints but its present lands up to a frame later, so
/// where a nod or sway step grows the shape, the strip shows the previous present (black
/// rim on the hair for one frame without this).
const POSE_REACH: f32 = 5.0;
/// Notes fade toward this tint (no alpha without a compositor).
const NOTE_TINT: u32 = 0x3a4656;
/// Note animation rate (shaped X windows without a compositor: no GPUI redraw).
const NOTE_TICK: Duration = Duration::from_millis(40);
/// Hair glints: a glow and a core layer each, at most four at once. Effect layers like the
/// hearts: without a compositor shaped X windows (a glint costs no GPUI redraw), else painted
/// in her window (`Os::fx_staged`).
const GLINT_LAYER: usize = NOTE_LAYER + 2 * NOTES_MAX;
const GLINTS_MAX: usize = 4;
/// A glint's smallest size, of its full size; its hot centre (a 3 px core, the arms' inner
/// half), and its pink edge (the arms' outer half).
const GLINT_MIN: f32 = 0.4;
const GLINT_HOT: u32 = 0xfff6fa;
const GLINT_EDGE: u32 = 0xff7ab8;
/// When a glint can't show (a glitch, a turn, a frame without hair), the next try.
const GLINT_RETRY_MS: u64 = 300;
/// A bob or sway step (1-2 px) alone waits this long for another redraw to ride on (a
/// blink, the other axis): at rest every pose step was a render of its own.
const POSE_SLACK: Duration = Duration::from_millis(250);
/// How long a glint step waits for a frame drawn anyway, where every redraw is a full present
/// (`Os::redraw_costly`). Idle draws a breath or blink step every ~260 ms, so a glint's 560 ms life
/// shows on two or three of those; alone (a still pose) it steps at this pace.
const GLINT_SLACK: Duration = Duration::from_millis(600);

/// Device px of the largest note sprite for this geometry.
pub fn note_px(geo: &Geometry) -> u32 {
    (120.0 * geo.scale * NOTE_SCALE).ceil().max(8.0) as u32
}

struct MusicOn {
    t0: Instant,
    /// Autonomy sessions end on their own; `music on` runs until `music off`.
    until: Option<Instant>,
    /// A player's tempo (MPRIS metadata), over the pack's.
    bpm: Option<f32>,
    /// Started by a playing media player (it stops with it).
    auto: bool,
    /// She dances until then (B2c: music with energy, on the floor); rolled once a session.
    dance_until: Option<Instant>,
    dance_rolled: bool,
}

/// After a player pauses or stops, the headphones stay this long (a track change, a seek).
const MUSIC_GRACE: Duration = Duration::from_secs(5);
const FRAME_FAST: Duration = Duration::from_millis(16);
/// The least jump that glides (logical px; less snaps).
const GLIDE_MIN: f32 = 24.0;
/// Moves closer together than this are a stream (a live drag): she snaps to each.
const GLIDE_STREAM: Duration = Duration::from_millis(150);
const FRAME_FX: Duration = Duration::from_millis(33);
/// Milliseconds since `t0` (0 before it).
fn now_ms(t0: Instant, now: Instant) -> f32 {
    now.saturating_duration_since(t0).as_secs_f32() * 1000.0
}

/// An idle weight shift (`@lean`): the upper body this far (px at 1x).
const LEAN_BEAT_PX: i32 = 14;
/// An idle beat that changes her outline (a hop, a hand on her hip, arms crossed, her chin on
/// her hand, a point, a shrug), not just her face.
fn gesture(steps: &[crate::pack::Step]) -> bool {
    use crate::pack::Step;
    const BODY: [&str; 7] = ["hand_on_hip", "arms_crossed", "chin_rest", "idle-point", "idle-shrug", "point_", "shrug"];
    steps.iter().any(|s| match s {
        Step::Clip(id) => id == "@hop" || BODY.iter().any(|b| id.starts_with(b)),
        Step::Hold { frame, .. } => BODY.iter().any(|b| frame.starts_with(b)),
    })
}

/// A bend toward the pointer eases in over this, and back upright over `BEND_OUT` (r43).
const BEND_IN: Duration = Duration::from_millis(600);
pub(super) const BEND_OUT: Duration = Duration::from_millis(900);
/// The hair above her brow trails a bend by this much (r44: 1-2 frames of a 10 fps look).
const HAIR_LAG: Duration = Duration::from_millis(140);
/// ONLINE at night: its red this far toward the panel.
const NIGHT_TAG_DIM: f32 = 0.45;
/// Into a drawn sleep whose cells are still decoding: the drawing before holds this long.
const SLEEP_DECODE_HOLD: Duration = Duration::from_millis(100);
/// A quiet arrival's sliver widens back into her over ~3 frames.
const SLIVER: Duration = Duration::from_millis(100);

struct Press {
    root: (i32, i32),
    origin: (i32, i32),
    head: bool,
    dragging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    Ground,
    /// Standing in the air (a float spot), not clamped to the floor.
    Float,
    /// A brain approach pose (hang, cling, peek): this frame, whatever the mood.
    Pose { frame: usize },
    /// On `win`'s top edge; `dx` = seat (or feet) x minus the window's left edge.
    Perch { win: u32, sit: bool, dx: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leg {
    Vanish,
    Bolt(Instant),
    Appear,
}

struct Teleport {
    /// Seeds the vanish glitch, the bolt and the landing glitch.
    seed: u64,
    leg: Leg,
    to: (i32, i32),
    place: Placement,
    bolt: Option<Bolt>,
    moved: bool,
    /// The brain's `glitch` arrival: vanish, then land there (no bolt).
    no_bolt: bool,
    /// A passive move: a shorter dissolve, a ~150 ms rebuild with little fringing, and the
    /// tag back with the body.
    quiet: bool,
    /// A drawn leave (`perchLeave` / `floorLeave`) plays until then; then the bolt.
    leave_until: Option<Instant>,
}

pub struct Avatar {
    pack: Arc<Pack>,
    pub layout: Layout,
    os: Rc<Os>,
    name: SharedString,
    font: SharedString,
    frames: Vec<Option<Frame>>,
    loaded: bool,
    anim: Animator,
    mood: Mood,
    mood_then: Option<(Instant, Mood)>,
    next_activity: Option<Instant>,
    /// `SA_FRAME_LOG`: the frame last logged (each change is a line).
    frame_logged: usize,
    /// Her energy's band changed: her idle layer follows at the next calm moment.
    band_relayer: bool,
    autonomy_started: bool,
    rng: Rng,
    bubble: Option<Bubble>,
    hearts: Option<(Instant, Point<Pixels>)>,
    /// The HUD stays down until then (a beat after a teleport).
    hud_after: Option<Instant>,
    /// Woken: the stand-up plays until then, and no move starts before it ends.
    waking_until: Option<Instant>,
    /// The work-area top (root px) for the hearts burst that started at the key.
    hearts_area: (Option<Instant>, Option<i32>),
    /// The bubble panel and tag as last painted: a part of the HUD only goes into the X shape
    /// once it has been painted where it is (present lag: else stale pixels show for a frame).
    panel_last: Rc<Cell<Option<PanelGeom>>>,
    /// The tag's bounds last frame and for how many frames in a row they have been laid out.
    tag_last: Rc<Cell<(Option<Bounds<Pixels>>, u8)>>,
    /// Click reaction in progress: the status tag steps aside until then.
    reaction_until: Option<Instant>,
    menu: Option<(Point<Pixels>, Instant)>,
    press: Option<Press>,
    streak: (u32, Instant),
    /// Window position without the bob/hop offset.
    origin: (i32, i32),
    /// Where the window really is (origin + offset).
    applied: (i32, i32),
    ticker: Ticker,
    _tick_loop: Task<()>,
    last_shape: Rc<RefCell<(Vec<ShapeRect>, Vec<ShapeRect>)>>,
    /// The bounding rects of the 2 paints before the last (dev `shape`: a present lands up to
    /// two frames after its paint).
    shape_hist: Rc<RefCell<[Vec<ShapeRect>; 2]>>,
    /// The drawing the last render painted (a change not laid underneath ahead lays it
    /// underneath once).
    drawn_last: Option<DrawnLast>,
    /// The cut of her frames (`anim`'s deadline) a render has already laid underneath ahead.
    precut: Option<Instant>,
    /// An idle beat (an activity) plays until then: a reaction to the pointer may cut it short.
    beat_until: Option<Instant>,
    /// Shown mirrored (ground and seat frames only: grip poses are side-specific).
    pub(super) flip: bool,
    /// A turn's squeeze in progress: (start, the flip it turns to). The art swaps halfway.
    turn: Option<(Instant, bool)>,
    /// Shape mode: the turn's painted frames so far (`facing::turn_band`).
    turn_frame: u32,
    /// Turns so far, the last one's reason, and when.
    turns: u32,
    turn_why: &'static str,
    pub(super) last_turn: Option<Instant>,
    /// Drag facing (hysteresis on the drag's own pointer velocity).
    pub(super) turner: crate::facing::Turner,
    pub(super) mirror_cfg: crate::settings::MirrorSettings,
    /// The art faces right unmirrored (the pack's `facing`).
    art_right: bool,
    /// Per frame: may mirror (anchored at the ground or a seat).
    flippable: Vec<bool>,
    t_start: Instant,
    /// SA_TIMING=1: when Enter sent the last turn, and whether its first reply text came in.
    timing: Option<Option<(Instant, bool)>>,
    first_frame: Rc<Cell<bool>>,
    focus: FocusHandle,
    focus_probe: Option<String>,
    warmed: bool,
    /// lachy's stills, decoded after the frames (`Stills`); her head-hover pointer is on.
    pub(super) stills: Option<Rc<Stills>>,
    pointer_on: bool,
    /// Horizontal device-px range of the window that is on the avatar's monitor.
    vis: (i32, i32),
    // Life and effects.
    bob_t0: Instant,
    hop: Option<Instant>,
    next_hop: Option<Instant>,
    /// A perch glide: since, the offset it started from, and the curve (`glide_offset`).
    glide: Option<(Instant, (i32, i32), crate::platform::MoveAnim)>,
    /// When the perch window last moved.
    perch_moved: Option<Instant>,
    /// Her body hosts (`Caps::host_per_output`): id, view, window. Empty: one window.
    hosts: Vec<crate::host::Host>,
    /// The monitors changed: hosts to open or close (`hosts_sync`).
    hosts_stale: bool,
    /// Effect hosts (`crate::host`): what reaches outside her window.
    fx_hosts: Vec<crate::host::FxHost>,
    glitch: Option<Glitch>,
    /// Frames drawn in the current glitch (keyed by its start): each gets a new pattern.
    glitch_step: Cell<(Option<Instant>, u64)>,
    /// The time the glitch's next step was laid at (underneath): that step is laid at it
    /// again when it's drawn, so it's exactly what was painted under the last frame.
    glitch_at: Cell<Option<Instant>>,
    next_glitch: Option<Instant>,
    /// A quiet teleport's arrival: a sliver widening back into her (from this time).
    sliver: Option<Instant>,
    squash: Option<Instant>,
    /// An idle beat's weight shift (`@lean` in an activity): from, how long, which way.
    lean_beat: Option<(Instant, Duration, i32)>,
    /// Her idle beats' rounds (`activity_pick`).
    beats: crate::life::Beats,
    /// Her upper body bent toward the pointer (r42, a linger or a wiggle): px at 1x at her neck
    /// from, to, since (eased over `BEND_IN` / `BEND_OUT`).
    bend: (f32, f32, Instant),
    glints: Vec<Glint>,
    next_glint: Option<Instant>,
    teleport: Option<Teleport>,
    /// Arrival sequences, the deep nap, the air kiss (arriveview).
    arrive: arriveview::ArriveState,
    /// The variant pools on screen and the strips' pixels (artview).
    art: artview::ArtState,
    intro: introview::IntroState,
    cutins: cutinview::CutInState,
    /// Body not drawn (between dematerialise and materialise).
    hidden: bool,
    quitting: bool,
    /// A guard tripped: quit with exit code 2.
    fatal: bool,
    /// The open panel's bottom edge (logical px). While it is open it never moves down,
    /// and a prompt or the text field doesn't move at all: buttons stay under the pointer
    /// when her pose changes.
    panel_bottom: Option<f32>,
    /// A prompt panel's height while that prompt is up (a re-wrap never shrinks it: the
    /// summon's panel dropped a line, 16 px, when her header changed at the settle).
    prompt_h: Option<(Instant, f32)>,
    /// Quit at the next tick (from where there is no `cx`).
    quit_requested: bool,
    placement: Placement,
    /// Ground point to fall back to when a perch ends.
    home: (i32, i32),
    /// What touches a perch edge, x relative to the seat (sitting) / feet (standing).
    contact: [Option<(i32, i32)>; 2],
    /// Contact shadow: rects for the frame it was built from (and whether that frame was
    /// mirrored: a turn mirrors the shadow with her), and where it is shown.
    shadow: Option<((usize, bool), Vec<ShapeRect>)>,
    shadow_at: Option<(i32, i32)>,
    /// The red cover over a drawn touchdown's silhouette is up (`sync_cover`).
    cover_on: bool,
    /// Last laid-out width of the status tag (logical px), to place the next one.
    tag_w: Rc<Cell<f32>>,
    /// Her menu's height as last painted (logical px; 0: not yet): it opens upward when it
    /// wouldn't fit under the click.
    menu_h: Rc<Cell<(u32, f32)>>,
    /// Where the open menu goes (the click, x, y), decided once: its side never changes while
    /// it is open, whatever her pose does.
    menu_place: Option<(Point<Pixels>, f32, f32, bool)>,
    /// The HUD's top while a bubble is up: (resting frame, avatar-local device px).
    hud_top: Cell<Option<(usize, i32)>>,
    /// Last laid-out box of the status tag (window logical px), to tear it in a glitch.
    tag_box: Cell2,
    /// The HUD's next motion step (the flag's flip, the thinking slivers, the rec dot's blink,
    /// the unread pill's nudge), set by the frame that shows it.
    motion_at: Cell<Option<Instant>>,
    music: Option<MusicOn>,
    /// The desktop art's motions (`motionview`).
    motion: motionview::MotionState,
    notes: Vec<NoteMasks>,
    notes_shown: usize,
    glints_shown: usize,
    /// When the note layers last moved.
    notes_at: Instant,
    /// When the glint layers last moved to their samples, and how many glints there were.
    glints_at: (Instant, usize),
    /// Breath/sway/nod as last drawn.
    pose_last: fx::Pose,
    /// A pose step not drawn yet, since (POSE_SLACK).
    pose_wait: Option<Instant>,
    /// A glint step not drawn yet, since (GLINT_SLACK; only where a redraw is costly).
    glint_wait: Option<Instant>,
    session: Option<SessionLink>,
    input: Option<InputBox>,
    /// Text typed but not sent (closed field, refused input).
    draft: String,
    prompt_ui: Option<PromptUi>,
    /// The user's line of the running turn.
    echo: Option<String>,
    /// Status label for one mood (`WORKING · bash`).
    label_override: Option<(Mood, String)>,
    /// A short note in the input footer, until then.
    hint: Option<(String, Instant)>,
    _subs: Vec<Subscription>,
    /// Laid-out bubble buttons (window logical px), for the dev `state` (test clicks).
    buttons: Buttons,
    /// The READER window (a long reply), its X id, and who had the keyboard before it.
    reader: Option<(WindowHandle<crate::reader::Reader>, u32, Option<u32>)>,
    /// Her body box the reader was placed beside (she moves: it follows), and its place.
    reader_at: Option<(WinRect, crate::beside::Place)>,
    /// The LINE (the conversation window), when open.
    line: Option<lineview::LineWin>,
    /// The summon key went down then (`talk press`): a tap or a hold at release.
    talk_down: Option<Instant>,
    /// The talk key went down on an open Line: a tap (not a hold) closes it at key-up.
    tap_closes: bool,
    /// Push-to-talk and spoken replies (the `capri-voice` sidecar).
    voice: voiceview::VoiceState,
    /// What he sent this run, oldest first (the Line's ↑).
    sent: Vec<String>,
    /// A missed reply (or a refused message): one line on her tag until he acknowledges it.
    unseen: lineview::Unseen,
    /// A refused message to hand back to the open Line.
    line_restore_text: Option<String>,
    /// Her first launch ever (`state::first_meeting`, read before the heartbeat writes).
    first_meeting: bool,
    /// When to try her first words (once she has landed and nothing else is on), and when to
    /// give up for this launch (she tries again at the next).
    first_words: Option<(Instant, Instant)>,
    /// How long the last Line took to open (ms).
    line_opened_ms: Option<u128>,
    /// The perch window unmapped; she falls if it isn't back within the grace.
    perch_gone: Option<edges::PerchGone>,
    /// The player stopped: the headphones come off at this time (MUSIC_GRACE).
    music_stop_at: Option<Instant>,
    /// What the media players add up to, last heard.
    player: crate::media::Playback,
    /// HIDE from her menu: vanishing (false) or hidden (true); the process keeps running.
    parked: Option<bool>,
    /// The restore chip's window while she is hidden, and where (device px).
    chip: Option<WindowHandle<crate::chip::Chip>>,
    chip_rect: Option<(i32, i32, i32, i32, u32)>,
    /// Restore asked for where there was no window context (a session summon).
    unpark_req: Option<&'static str>,
    /// QUIT in the menu was clicked once: a second click before this exits.
    quit_armed: Option<Instant>,
    /// The bubble's command chip (`synaps login`) was clicked: copied.
    cmd_copied: Option<String>,
    /// The wheel's fractional lines over the bubble.
    wheel: Cell<f32>,
    /// The speech panel as last laid out (window logical px): the reader keeps clear of it.
    panel_rect: Cell<Option<(f32, f32, f32, f32)>>,
    /// The cat brain (None: SA_NO_BRAIN).
    brain: Option<brainview::BrainHost>,
    /// Carried, flung, landing (None: not ragdolling).
    rag: Option<ragview::Rag>,
    /// settings.toml `[ragdoll] strength` (read at start and by `settings reload`).
    rag_strength: crate::ragdoll::Strength,
    /// Ticks run so far (wake-ups), for the dev `state`.
    view_ticks: u64,
    /// Renders and window moves since start (`brain state`, the perf meters).
    renders: u64,
    moves: u64,
    /// The held drag's window follow (ragview::rag_follow, once a frame): last move, the
    /// shortest gap between moves, and the pointer change it last saw (`Os::pointer_seq`).
    follow_at: Option<Instant>,
    follow_period: Duration,
    follow_seq: u64,
    /// The grip it followed with (a turn moves the grip: the next frame follows at once).
    follow_grip: Option<(i32, i32)>,
}

pub struct Init {
    pub pack: Arc<Pack>,
    pub layout: Layout,
    pub os: Rc<Os>,
    pub name: String,
    pub origin: (i32, i32),
    pub t_start: Instant,
    pub focus_probe: bool,
}

/// A period with ±25 % jitter, never under 250 ms (a 0 in the pack must not spin).
fn jitter(rng: &mut Rng, ms: u64) -> Duration {
    Duration::from_millis(((ms as f32 * rng.range(0.75, 1.3)) as u64).max(250))
}

impl Avatar {
    pub fn new(init: Init, window: &mut Window, cx: &mut Context<Self>) -> Avatar {
        let pack = init.pack;
        let art_right = pack.behaviour.art_faces_right();
        let flippable = pack.frames.iter().map(|f| matches!(f.anchor.name.as_str(), "ground" | "seat" | "grip")).collect();
        let default = pack.frame(&pack.catalog.default_sprite).unwrap_or(0);
        let font = pack.behaviour.font.clone().unwrap_or_else(|| "Fira Sans".into());
        let vis = Self::visible_range(&init.os, &init.layout, init.origin);
        let (ticker, wake) = Ticker::start();
        let tick_loop = cx.spawn(async move |this, cx| {
            while wake.recv().await.is_ok() {
                if this.update(cx, |v, cx| v.tick(cx)).is_err() {
                    break;
                }
            }
        });
        // Keyboard focus went elsewhere (a click on another window): close the field,
        // keep the text.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.input.is_some() {
                this.close_input(false);
                cx.notify();
                this.schedule(cx);
            }
        });
        let l = init.layout;
        let home = (init.origin.0 + l.ground_x, init.origin.1 + l.ground_y);
        let now = Instant::now();
        let mut a = Avatar {
            frames: (0..pack.frames.len()).map(|_| None).collect(),
            anim: Animator::new(default),
            name: init.name.to_uppercase().into(),
            font: font.into(),
            pack,
            layout: l,
            os: init.os,
            loaded: false,
            mood: Mood::Idle,
            mood_then: None,
            next_activity: None,
            frame_logged: usize::MAX,
            band_relayer: false,
            autonomy_started: false,
            rng: Rng::seeded(),
            bubble: None,
            hearts: None,
            hud_after: None,
            waking_until: None,
            hearts_area: (None, None),
            panel_last: Rc::new(Cell::new(None)),
            tag_last: Rc::new(Cell::new((None, 0))),
            reaction_until: None,
            menu: None,
            press: None,
            streak: (0, now),
            origin: init.origin,
            applied: init.origin,
            ticker,
            _tick_loop: tick_loop,
            last_shape: Rc::new(RefCell::new((vec![(0, 0, 1, 1)], vec![]))),
            shape_hist: Rc::new(RefCell::new([Vec::new(), Vec::new()])),
            drawn_last: None,
            precut: None,
            beat_until: None,
            flip: false,
            turn: None,
            turn_frame: 0,
            turns: 0,
            turn_why: "",
            last_turn: None,
            turner: Default::default(),
            mirror_cfg: crate::settings::Settings::load().map(|s| s.mirror).unwrap_or_default(),
            voice: voiceview::VoiceState::new(crate::settings::Settings::load().map(|s| s.voice).unwrap_or_default()),
            art_right,
            flippable,
            t_start: init.t_start,
            timing: std::env::var_os("SA_TIMING").map(|_| None),
            first_frame: Rc::new(Cell::new(false)),
            focus: cx.focus_handle(),
            focus_probe: init.focus_probe.then(String::new),
            warmed: false,
            stills: None,
            pointer_on: false,
            vis,
            bob_t0: now,
            hop: None,
            next_hop: None,
            glide: None,
            perch_moved: None,
            hosts: Vec::new(),
            hosts_stale: false,
            fx_hosts: Vec::new(),
            glitch: None,
            glitch_step: Cell::new((None, 0)),
            glitch_at: Cell::new(None),
            next_glitch: None,
            sliver: None,
            squash: None,
            lean_beat: None,
            beats: crate::life::Beats::default(),
            bend: (0.0, 0.0, Instant::now()),
            glints: Vec::new(),
            next_glint: None,
            teleport: None,
            arrive: Default::default(),
            intro: introview::IntroState::new(),
            first_words: None,
            first_meeting: state::first_meeting(),
            cutins: cutinview::CutInState::default(),
            art: artview::ArtState::new(std::env::var("SA_ART_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| Rng::seeded().next())),
            hidden: false,
            quitting: false,
            fatal: false,
            panel_bottom: None,
            prompt_h: None,
            quit_requested: false,
            placement: Placement::Ground,
            home,
            contact: [None, None],
            shadow: None,
            shadow_at: None,
            cover_on: false,
            tag_w: Rc::new(Cell::new(150.0)),
            menu_h: Rc::new(Cell::new((0, 0.0))),
            menu_place: None,
            hud_top: Cell::new(None),
            tag_box: Rc::new(Cell::new(None)),
            motion_at: Cell::new(None),
            music: None,
            motion: Default::default(),
            notes: Vec::new(),
            notes_shown: 0,
            glints_shown: 0,
            glints_at: (Instant::now(), 0),
            notes_at: Instant::now(),
            pose_last: fx::Pose::default(),
            pose_wait: None,
            glint_wait: None,
            session: None,
            input: None,
            draft: String::new(),
            prompt_ui: None,
            echo: None,
            label_override: None,
            hint: None,
            _subs: vec![activation],
            buttons: Rc::default(),
            reader: None,
            reader_at: None,
            line: None,
            talk_down: None,
            tap_closes: false,
            sent: Vec::new(),
            unseen: Default::default(),
            line_restore_text: None,
            line_opened_ms: None,
            perch_gone: None,
            parked: None,
            music_stop_at: None,
            player: crate::media::Playback::default(),
            chip: None,
            chip_rect: None,
            unpark_req: None,
            quit_armed: None,
            cmd_copied: None,
            wheel: Cell::new(0.0),
            panel_rect: Cell::new(None),
            brain: None,
            rag: None,
            rag_strength: crate::ragdoll::Strength(crate::settings::Settings::load().map_or(0.5, |s| s.ragdoll.strength)),
            view_ticks: 0,
            renders: 0,
            moves: 0,
            follow_at: None,
            follow_period: Duration::from_millis(16),
            follow_seq: 0,
            follow_grip: None,
        };
        if std::env::var_os("SA_NO_BRAIN").is_none() {
            a.brain_start();
        }
        a
    }

    pub fn set_notes(&mut self, notes: Vec<NoteMasks>) {
        self.notes = notes;
    }

    // ---- music --------------------------------------------------------------------

    fn music_spec(&self) -> Option<&MusicSpec> {
        self.pack.behaviour.music.as_ref()
    }

    /// The music pose for the current placement, if music is on and the pack has it.
    fn music_frame(&self) -> Option<usize> {
        self.music.as_ref()?;
        if let Some(i) = self.dance_frame() {
            return Some(i);
        }
        if let Some(i) = self.art_music_frame() {
            return Some(i);
        }
        let spec = self.music_spec()?;
        let idx = |id: &str| self.pack.index.get(id).copied();
        if self.sitting() {
            spec.window.as_deref().and_then(idx).filter(|&i| self.pack.frames[i].anchor.name == "seat")
        } else {
            idx(&spec.floor).filter(|&i| self.pack.frames[i].anchor.name == "ground")
        }
    }

    fn music_on(&mut self, now: Instant, until: Option<Instant>) -> anyhow::Result<()> {
        let Some(spec) = self.music_spec() else { anyhow::bail!("this pack has no music") };
        if !self.pack.index.contains_key(&spec.floor) {
            anyhow::bail!("music frame `{}` is missing from the pack", spec.floor);
        }
        if self.mood != Mood::Idle {
            self.set_mood(Mood::Idle, now);
        }
        let was = self.music.replace(MusicOn { t0: now, until, bpm: None, auto: false, dance_until: None, dance_rolled: false });
        self.relayer(now);
        if was.is_none() && self.loaded && self.teleport.is_none() {
            self.glitch = Glitch::for_cut(Transition::Music, now, self.rng.next());
        }
        Ok(())
    }

    /// A media player started (`playing`) or stopped: real LISTENING, off after a grace.
    fn playback(&mut self, p: crate::media::Playback, now: Instant) -> String {
        self.player = p;
        // Music started under an unprompted reply: she stops.
        self.voice_guard();
        if p.playing && (self.mood != Mood::Idle || !self.loaded) {
            // The session's mood first; the headphones go on when she is idle again (tick).
            return "ok (later)".into();
        }
        if p.playing {
            self.music_stop_at = None;
            match &mut self.music {
                Some(m) if m.until.is_none() && !m.auto => {} // `music on` by hand: left alone
                Some(m) => {
                    m.until = None;
                    m.auto = true;
                    m.bpm = p.bpm;
                }
                None => {
                    if let Err(e) = self.music_on(now, None) {
                        return format!("error: {e}");
                    }
                    if let Some(m) = &mut self.music {
                        m.auto = true;
                        m.bpm = p.bpm;
                    }
                }
            }
            self.dance_decide(now);
            if p.video { "ok listening (video)".into() } else { "ok listening".into() }
        } else {
            if self.music.as_ref().is_some_and(|m| m.auto) {
                self.music_stop_at = Some(now + MUSIC_GRACE);
            }
            "ok quiet".into()
        }
    }

    fn music_off(&mut self, now: Instant) {
        self.music_stop_at = None;
        if self.music.take().is_some() {
            // Out of a dance: the light seam (a burst over a moving drawing smeared her legs).
            let t = if self.motion_cell(self.shown_frame()) { Transition::Motion } else { Transition::Music };
            self.relayer(now);
            self.update_notes(now);
            if self.loaded && self.teleport.is_none() {
                self.glitch = Glitch::for_cut(t, now, self.rng.next());
            }
        }
    }

    /// Move the note layers to `now`; unmap the ones not in use.
    fn update_notes(&mut self, now: Instant) {
        let mut used = 0;
        let show = self.loaded && !self.hidden && !self.quitting && self.teleport.is_none() && !self.notes.is_empty();
        if let (true, Some(m), Some(fi), Some(spec)) = (show, self.music.as_ref(), self.music_frame(), self.music_spec()) {
            let src = &self.pack.frames[fi];
            // A strip's emitters are keyed by the strip, in cell px.
            let strip = src.cell.as_ref().zip(self.pack.anim.as_ref()).map(|(c, a)| &a.strips[c.strip]);
            let scaled: Vec<[i32; 2]> = strip
                .and_then(|s| Some((s, spec.emitters.get(&s.name)?)))
                .map(|(s, e)| e.iter().map(|p| [(p[0] as f32 * s.k).round() as i32, (p[1] as f32 * s.k).round() as i32]).collect())
                .unwrap_or_default();
            let emitters = if strip.is_some() { scaled.as_slice() } else { spec.emitters.get(&src.id).map(Vec::as_slice).unwrap_or(&[]) };
            let (ox, oy) = self.pack.offset(src);
            let l = self.layout;
            let beat = 60.0 / self.music.as_ref().and_then(|m| m.bpm).unwrap_or(spec.bpm).clamp(40.0, 240.0);
            let parts = fx::music_notes(now.saturating_duration_since(m.t0).as_secs_f32(), beat, emitters);
            let base = (self.applied.0 + l.avatar_x, self.applied.1 + l.avatar_y);
            for n in parts.iter().take(NOTES_MAX) {
                // No alpha: fade by shrinking and stepping the colour toward a dim tint, then
                // pop out. Four steps, no dither.
                if n.alpha < 0.14 {
                    continue;
                }
                let k = if n.alpha > 0.8 {
                    1.0
                } else if n.alpha > 0.55 {
                    0.72
                } else if n.alpha > 0.32 {
                    0.48
                } else {
                    0.28
                };
                let sprite = &self.notes[n.sprite % self.notes.len()];
                let (dx, dy) = l.geo.to_device(n.x.round() as i32 + ox, n.y.round() as i32 + oy);
                let dx = if self.mirrored(fi) { l.geo.width as f32 - dx } else { dx };
                let size = ((n.size * l.geo.scale * NOTE_SCALE * (0.55 + 0.45 * k)).round() as i32).clamp(4, sprite.size as i32);
                let o = (base.0 + dx.round() as i32 - size / 2, base.1 + dy.round() as i32 - size / 2);
                let ai = frames_angle(n.deg);
                let (outline, fill) = &sprite.variants[ai];
                let scale = sprite.size as f32 / size as f32;
                let runs = |mask: &crate::frames::Mask| {
                    let mut out = Vec::new();
                    for y in 0..size {
                        let mut x = 0;
                        let on = |x: i32| mask.get((x as f32 * scale) as i32, (y as f32 * scale) as i32);
                        while x < size {
                            if on(x) {
                                let s0 = x;
                                while x < size && on(x) {
                                    x += 1;
                                }
                                out.push((s0 as i16, y as i16, (x - s0) as u16, 1u16));
                            } else {
                                x += 1;
                            }
                        }
                    }
                    out
                };
                let (ro, rf) = (runs(outline), runs(fill));
                let (lo, lf) = (NOTE_LAYER + 2 * used, NOTE_LAYER + 2 * used + 1);
                self.os.fx_set(lf, mix_rgb(NOTE_TINT, sprite.fill_rgb, k), o, (size, size), &rf).ok();
                self.os.fx_set_below(lo, lf, mix_rgb(NOTE_TINT, sprite.outline_rgb, k), o, (size, size), &ro).ok();
                used += 1;
            }
        }
        for k in used..self.notes_shown {
            self.os.fx_unmap(NOTE_LAYER + 2 * k);
            self.os.fx_unmap(NOTE_LAYER + 2 * k + 1);
        }
        if used > 0 || self.notes_shown > 0 {
            self.os.flush();
        }
        self.notes_shown = used;
        self.notes_at = now;
    }

    /// A glint sample (or a glint's end) has passed since the layers last moved.
    fn glint_step_due(&self, now: Instant) -> bool {
        let life_ms = self.pack.behaviour.life.sparkle_ms;
        let (at, n) = self.glints_at;
        self.glints.len() != n || self.glints.iter().any(|g| g.next_sample(at, life_ms).is_some_and(|t| t <= now))
    }

    /// Move the glint layers to `now`; unmap the ones not in use.
    fn update_glints(&mut self, now: Instant) {
        let life_ms = self.pack.behaviour.life.sparkle_ms;
        let show = self.loaded
            && !self.hidden
            && !self.quitting
            && self.glitch.is_none()
            && self.teleport.is_none()
            && self.squash.is_none()
            && self.rag.is_none()
            && self.turn.is_none();
        let l = self.layout;
        let base = ((self.applied.0 + l.avatar_x) as f32, (self.applied.1 + l.avatar_y) as f32);
        let mut used = 0;
        // Only on her: a glint picked on one frame's hair isn't left in the air when the
        // frame changes under it (a landing's crouch, a pose).
        // A glint that couldn't show yet (a dip, a glitch) starts its life now: it never pops in
        // grown, a lone white cross (r34).
        if show {
            for g in self.glints.iter_mut().filter(|g| !g.live) {
                g.live = true;
                g.t0 = g.t0.max(now);
            }
        }
        let body = self.frame_or_default(self.shown_frame()).map(|f| &f.mask);
        if show {
            for g in &self.glints {
                if used >= GLINTS_MAX {
                    break;
                }
                // All of it on her hair, or not at all.
                if !body.is_some_and(|m| g.reach().iter().all(|&(x, y)| m.get(x.round() as i32, y.round() as i32))) {
                    continue;
                }
                let Some(sc) = g.scale(now, life_ms).filter(|&v| v > 0.02) else { continue };
                let (dx, dy) = g.offset(now, life_ms);
                // Never smaller than ~40 % (a 1-3 px star vanished into her hair's glitter).
                let (cx, cy, r) = (base.0 + g.x + dx, base.1 + g.y + dy, g.size * (GLINT_MIN + (1.0 - GLINT_MIN) * sc));
                let place = |pts: Vec<(f32, f32)>| {
                    let x0 = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
                    let y0 = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
                    let x1 = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
                    let y1 = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
                    let local: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (x - x0 as f32, y - y0 as f32)).collect();
                    let mut rows = Vec::new();
                    shape::poly_rows(&local, &mut rows);
                    ((x0, y0), (x1 - x0, y1 - y0), rows)
                };
                let (core, glow) = (GLINT_LAYER + 2 * used + 1, GLINT_LAYER + 2 * used);
                // A solid near-white core (3 px) with arms 2 px at the base tapering to points,
                // pink only on their outer half: a hot centre fading into her hair (r36: a 1 px
                // pink cross vanished into it; main's flat white star read as a stamp). 1-bit
                // windows, no alpha: two layers.
                let sf = l.sf;
                let (k, b) = (1.5 * sf, 1.6 * sf);
                let (o, sz, rows) = place(fx::glint_star(cx, cy, (r * 0.5).max(2.6 * sf), k, b, g.spin));
                self.os.fx_set(core, GLINT_HOT, o, sz, &rows).ok();
                let (o, sz, rows) = place(fx::glint_star(cx, cy, r, k, b, g.spin));
                self.os.fx_set_below(glow, core, GLINT_EDGE, o, sz, &rows).ok();
                used += 1;
            }
        }
        for k in used..self.glints_shown {
            self.os.fx_unmap(GLINT_LAYER + 2 * k);
            self.os.fx_unmap(GLINT_LAYER + 2 * k + 1);
        }
        if used > 0 || self.glints_shown > 0 {
            self.os.flush();
        }
        self.glints_shown = used;
        self.glints_at = (now, self.glints.len());
    }

    // ---- idle/music body motion --------------------------------------------------------

    fn pose_active(&self) -> bool {
        self.loaded
            && !self.hidden
            && !self.quitting
            && self.teleport.is_none()
            && self.press.is_none()
            && self.squash.is_none()
            && self.glitch.is_none()
            && !self.art_shows_strip()
    }

    /// Her bend toward the pointer now (px at 1x), eased.
    pub(super) fn bend_px(&self, now: Instant) -> f32 {
        let (from, to, t0) = self.bend;
        let ease = if to == 0.0 { BEND_OUT } else { BEND_IN };
        let k = (now.saturating_duration_since(t0).as_secs_f32() / ease.as_secs_f32()).min(1.0);
        from + (to - from) * k * k * (3.0 - 2.0 * k)
    }

    /// Bend toward the pointer (0: straighten up), from wherever she is now.
    pub(super) fn bend_to(&mut self, px: f32, now: Instant) {
        if (self.bend.1 - px).abs() < 0.05 {
            return;
        }
        self.bend = (self.bend_px(now), px, now);
    }

    fn bending(&self, now: Instant) -> bool {
        self.bend.1 != 0.0 || now < self.bend.2 + BEND_OUT + HAIR_LAG
    }

    fn pose_at(&self, now: Instant) -> fx::Pose {
        let mut p = self.pose_at_body(now);
        // Bent toward the pointer: on any art, on her feet on the floor (feet planted).
        if self.loaded && !self.hidden && self.teleport.is_none() && self.rag.is_none() && self.placement == Placement::Ground && self.standing() {
            p.bend = (self.bend_px(now) * self.layout.sf).round() as i32;
            p.bend_hair = (self.bend_px(now.checked_sub(HAIR_LAG).unwrap_or(now)) * self.layout.sf).round() as i32;
            if p.bend != 0 || p.bend_hair != 0 {
                p.swing = 0;
                p.kick = 0;
            }
        }
        p
    }

    fn pose_at_body(&self, now: Instant) -> fx::Pose {
        // Before and after a walk: her lean (on any art; feet planted).
        if let Some(sway) = self.motion.lean.and_then(|l| l.px(now, self.layout.sf))
            && self.loaded
            && !self.hidden
            && self.teleport.is_none()
            && self.rag.is_none()
        {
            return fx::Pose { sway, ..fx::Pose::default() };
        }
        if !self.pose_active() {
            return fx::Pose::default();
        }
        let sf = self.layout.sf;
        if let (Some(m), Some(spec), Some(_)) = (&self.music, self.music_spec(), self.music_frame()) {
            // Held on twos (12 fps) like Capri's music body.
            let hold = 1000.0 / 12.0;
            let t = (now.saturating_duration_since(m.t0).as_secs_f32() * 1000.0 / hold).floor() * hold;
            let beats = t / (60_000.0 / self.music.as_ref().and_then(|m| m.bpm).unwrap_or(spec.bpm).clamp(40.0, 240.0));
            let p = beats.fract();
            // Head down on the beat and back up (4 px), the upper body tilting side to side
            // over two beats (4 px at the top of the hair).
            let nod = (4.0 * sf * (0.5 + 0.5 * (p * std::f32::consts::TAU).cos())).round() as i32;
            let sway = (4.0 * sf * (beats * std::f32::consts::PI).sin()).round() as i32;
            return fx::Pose { breath: 0, nod, sway, ..fx::Pose::default() };
        }
        if self.posed() {
            // Hanging: no breath or hair sway (they'd pull the hands off the grip), a slow
            // pendulum instead, kicked by the bid emotes.
            let (swing, kick, kick_right) = self.hang_swing(now);
            return fx::Pose { swing, kick, kick_right, ..fx::Pose::default() };
        }
        let life = &self.pack.behaviour.life;
        let (bob_px, bob_period_ms, sway_px, droop, lean) = self.breath_now(now);
        let t = now.saturating_duration_since(self.bob_t0);
        let breath = fx::bob(t, bob_px, bob_period_ms);
        // In 2 px steps once it's wide enough: every step is a redraw.
        let sway = if sway_px > 0 && life.sway_period_ms > 0 {
            let step = if sway_px >= 4 { 2.0 } else { 1.0 };
            let v = sway_px as f32 * (t.as_secs_f32() * 1000.0 / life.sway_period_ms as f32 * std::f32::consts::TAU).sin();
            ((v / step).round() * step) as i32
        } else {
            0
        };
        fx::Pose { breath, nod: droop, sway: sway + lean, ..fx::Pose::default() }
    }

    /// Her breath now (bob px, period ms, sway px) and, tired and standing on the floor, her
    /// head's droop and lean (device px): her energy band's, else `life`'s.
    fn breath_now(&self, now: Instant) -> (i32, u64, i32, i32, i32) {
        let life = &self.pack.behaviour.life;
        let e = self.pack.behaviour.energy.band(self.band());
        let standing = self.placement == Placement::Ground && self.standing();
        let sf = self.layout.sf;
        let px = |v: i32| if standing { (v as f32 * sf).round() as i32 } else { 0 };
        // A slow nod, on her feet or on the floor (dozing).
        let nod = if self.placement == Placement::Ground && e.nod_px > 0 && e.nod_period_ms > 0 {
            let t = now_ms(self.bob_t0, now) / e.nod_period_ms as f32;
            let u = (0.5 - 0.5 * (t * std::f32::consts::TAU).cos()).powf(3.0);
            (e.nod_px as f32 * sf * u).round() as i32
        } else {
            0
        };
        // An idle beat's weight shift: eased in and out, in 2 px steps.
        let lean = self.lean_beat.filter(|_| standing).map_or(0, |(t0, d, px)| {
            let (t, n) = (now_ms(t0, now), d.as_secs_f32() * 1000.0);
            let k = (t / 260.0).min(1.0).min(((n - t) / 260.0).max(0.0));
            let v = px as f32 * sf * k * k * (3.0 - 2.0 * k);
            ((v / 2.0).round() * 2.0) as i32
        });
        (
            e.bob_px.unwrap_or(life.bob_px),
            e.bob_period_ms.unwrap_or(life.bob_period_ms),
            e.sway_px.unwrap_or(life.sway_px),
            px(e.droop_px) + nod,
            px(e.lean_px) + lean,
        )
    }

    /// An activity's `@hop` (a small hop in place, a squash as she lands) and `@lean` (a
    /// weight shift for the activity's length), by day: no new art.
    fn activity_beats(&mut self, steps: &[crate::pack::Step], len: Duration, now: Instant) {
        use crate::pack::Step;
        let has = |id: &str| steps.iter().any(|s| matches!(s, Step::Clip(c) if c == id));
        let standing = self.placement == Placement::Ground && self.standing();
        if has("@hop") && standing && self.hop_allowed() && self.hop.is_none() {
            // The squash on the big beat's landing; the little beat after it is the rebound
            // (r42: a squash at the end read as a slump).
            self.hop = Some(now);
            self.squash = Some(now + Duration::from_millis((fx::HOP_MS * fx::HOP_LAND_AT) as u64));
        }
        if has("@lean") && standing {
            let dir = if self.rng.pick(2) == 0 { 1 } else { -1 };
            self.lean_beat = Some((now, len.max(Duration::from_millis(1600)), dir * LEAN_BEAT_PX));
        }
    }

    /// The next idle beat (`life::Beats`): a gesture and a face beat take turns.
    fn activity_pick(&mut self, n: usize) -> usize {
        let gesture: Vec<bool> = self.activities().iter().take(n).map(|a| gesture(a)).collect();
        self.beats.pick(&gesture, &mut self.rng)
    }

    fn pose_next_change(&self, now: Instant) -> Option<Instant> {
        let (bob_px, _, sway_px, droop, lean) = self.breath_now(now);
        let bend = self.bending(now);
        if !bend && (!self.pose_active() || (self.music_frame().is_none() && bob_px <= 0 && sway_px <= 0 && droop == 0 && lean == 0 && !self.posed())) {
            return None;
        }
        let cur = self.pose_at(now);
        let step = Duration::from_millis(20);
        (1..=100).map(|k| now + step * k).find(|&t| self.pose_at(t) != cur).or(Some(now + Duration::from_secs(2)))
    }

    /// The rows a pose acts on for frame `fi`.
    fn pose_rows(&self, fi: usize, f: &Frame) -> fx::PoseRows {
        let l = &self.layout;
        let seat = self.pack.frames[fi].anchor.name == "seat";
        let line = if seat { l.seat_y.unwrap_or(l.ground_y) } else { l.ground_y } - l.avatar_y;
        let span = (line - f.top).max(1) as f32;
        let waist = line - (span * if seat { 0.3 } else { 0.5 }) as i32;
        let neck = f.face.1 + (span * 0.04) as i32;
        let music = self.music.is_some() && self.music_frame() == Some(fi);
        let grip = match self.placement {
            Placement::Pose { frame } if frame == fi => self.anchor_px(fi).1 - l.avatar_y,
            _ => 0,
        };
        let knee = self.knee_row(fi, f);
        let split = f.head_x;
        fx::PoseRows {
            y0: -l.avatar_y,
            h: l.geo.height as i32,
            top: f.top,
            shear_end: if music { waist } else { f.face.0 },
            neck,
            brow: f.face.0,
            waist,
            grip,
            knee,
            split,
        }
    }

    /// Frames arrive from the background decoder: the default first, then the rest.
    pub fn add_frames(&mut self, decoded: Vec<(usize, Frame)>, complete: bool, cx: &mut Context<Self>) {
        let first = !self.frames.iter().any(Option::is_some);
        for (i, f) in decoded {
            self.frames[i] = Some(f);
        }
        let now = Instant::now();
        // Materialise with a glitch, like Capri coming online; with a launch intro she stays
        // hidden until the frames are in, then arrives on the bolt.
        if first && !self.launch_hold() {
            self.glitch = Some(Glitch::new(GlitchKind::Appear, now, self.rng.next()));
        }
        if first && self.arrive.launch_wait && !state::load_hidden() {
            self.intro_prepare(cx);
        }
        if complete && !self.loaded && state::load_hidden() {
            // Hidden last time (HIDE): stay hidden, the chip shows where she is.
            self.parked = Some(true);
            self.hidden = true;
            self.glitch = None;
            self.open_chip(cx);
        }
        if complete && !self.loaded {
            self.loaded = true;
            self.contact = [self.measure_contact(true), self.measure_contact(false)];
            self.set_mood(Mood::Idle, now);
            let held = self.arrive.launch_wait;
            if held && !self.launch_intro(now, cx) && self.parked.is_none() {
                self.hidden = false;
                self.glitch = Some(Glitch::new(GlitchKind::Appear, now, self.rng.next()));
            }
            self.heartbeat_start(cx);
            if self.first_meeting {
                self.first_words = Some((now + FIRST_WORDS_AFTER, now + FIRST_WORDS_WITHIN));
            }
            self.stills_decode(cx);
            self.burst_decode(cx);
            let win = self.os.body();
            self.set_icon_on(win);
            self.next_glint = Some(now + Duration::from_millis(700));
            self.bob_t0 = now;
            // Moods held back until the frames were in.
            if let Some(s) = &mut self.session {
                s.shown = None;
            }
            self.sync_body(now);
        }
        cx.notify();
        self.schedule(cx);
    }

    /// lachy's stills, off the main thread (a few ms each).
    fn stills_decode(&mut self, cx: &mut Context<Self>) {
        let Some(anim) = self.pack.anim.as_ref() else { return };
        let st = anim.stills.clone();
        // The Line's faces per mood: those whose image the pack has.
        let moods: Vec<MoodFace> = self.pack.behaviour.line_faces.iter().filter_map(|(m, f)| Some((m.clone(), anim.mochi.get(f.image())?.clone(), f.crop()))).collect();
        if st.is_empty() && moods.is_empty() {
            return;
        }
        let sf = self.layout.sf;
        cx.spawn(async move |this, cx| {
            let s = crate::off_thread(move || Ok(Stills::decode(&st, &moods, sf))).await;
            this.update(cx, |v, cx| {
                if let Ok(s) = s {
                    v.stills = Some(Rc::new(s));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Her head under the pointer: lachy's cursor (a headpat waiting to happen). Applied on every
    /// move over it (GPUI sets its own when a HUD element's changes).
    fn hover_pointer(&mut self, p: Point<Pixels>) {
        let Some(c) = self.stills.as_ref().and_then(|s| s.cursor.as_ref()) else { return };
        let on = self.hit(p) == Some(true) && self.press.is_none() && self.rag.is_none();
        if on {
            self.os.pointer_image(Some(c));
        } else if self.pointer_on {
            self.os.pointer_image(None);
        }
        self.pointer_on = on;
    }

    /// A speech panel is open: the text field, a pending prompt, or a reply.
    pub(super) fn panel_up(&self) -> bool {
        self.input.is_some() || self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed) || self.bubble.as_ref().is_some_and(|b| !b.collapsed)
    }

    fn spec(&self, m: Mood) -> Option<&MoodSpec> {
        self.pack.behaviour.moods.get(m.key())
    }

    fn perch_spec(&self) -> Option<&PerchSpec> {
        self.pack.behaviour.perch.as_ref()
    }

    fn sitting(&self) -> bool {
        matches!(self.placement, Placement::Perch { sit: true, .. })
    }

    fn frame_or_default(&self, i: usize) -> Option<&Frame> {
        self.fr(i).or_else(|| self.fr(self.pack.frame(&self.pack.catalog.default_sprite).ok()?))
    }

    /// Frame `i` as shown: mirrored while she faces the other way (if it may mirror).
    pub(super) fn fr(&self, i: usize) -> Option<&Frame> {
        let own = self.frames.get(i).and_then(Option::as_ref);
        // A strip's cell mirrored: its mirrorArt twin (decoded mirrored). Either way round, the
        // other set mirrored on the fly stands in until the right one is in.
        let twin = self.pack.twin(i).and_then(|t| self.frames.get(t)).and_then(Option::as_ref);
        if self.mirrored(i) {
            return twin.or_else(|| own.map(Frame::mirrored));
        }
        own.or_else(|| twin.map(Frame::mirrored))
    }

    /// Frame `i` is shown mirrored: she faces the other way from the way it was drawn (a
    /// strip may be drawn facing the other way from the pack's art).
    pub(super) fn mirrored(&self, i: usize) -> bool {
        let drawn_other = match (self.pack.frames.get(i).and_then(|f| f.cell.as_ref()), &self.pack.anim) {
            (Some(c), Some(a)) => a.strips[c.strip].faces_right != self.art_right,
            _ => false,
        };
        self.flippable.get(i).copied().unwrap_or(false) && self.flip != drawn_other
    }

    /// She faces right (or will, once a turn in progress is through).
    pub(super) fn facing_right(&self) -> bool {
        crate::facing::faces_right(self.art_right, self.turn.map_or(self.flip, |t| t.1))
    }

    /// Turn to face `right`. `snap`: at once (a drag, a departure); else the turn's squeeze
    /// (`[mirror] turn_ms`), where the art swaps edge-on.
    pub(super) fn face(&mut self, right: bool, why: &'static str, now: Instant, snap: bool) {
        // Her cold shoulder holds her back to him through every turn she makes on her own
        // (moves face their way; a drag or a fling is his).
        let right = match self.cold_away() {
            Some(away) if matches!(why, "look" | "arrive" | "needs_you" | "cold") => away,
            _ => right,
        };
        // In the air or landing she keeps the way she was flung (and her chain its grip).
        if self.facing_right() == right || (self.rag.is_some() && !self.rag_held()) {
            return;
        }
        let to = crate::facing::mirrored_for(self.art_right, right);
        self.turns += 1;
        self.turn_why = why;
        self.last_turn = Some(now);
        // (A light seam is one frame: the squeeze turn goes on over it, r42.)
        let still = self.loaded && !self.hidden && self.rag.is_none() && self.teleport.is_none() && self.glitch.is_none_or(|g| g.kind == GlitchKind::Seam);
        // Grip poses don't mirror: nothing to squeeze, the art swaps when she next stands.
        let shows = self.flippable.get(self.anim.frame()).copied().unwrap_or(false);
        if snap || self.mirror_cfg.turn_ms == 0 || !still || !shows {
            self.turn = None;
            self.set_flip(to);
        } else {
            self.turn = Some((now, to));
            self.turn_frame = 0;
        }
    }

    /// The flip she is turning to (or shows).
    pub(super) fn turn_target(&self) -> bool {
        self.turn.map_or(self.flip, |t| t.1)
    }

    pub(super) fn turn_busy(&self) -> bool {
        self.turn.is_some()
    }

    /// The frame on screen may mirror (not a grip pose).
    pub(super) fn can_mirror_now(&self) -> bool {
        self.flippable.get(self.anim.frame()).copied().unwrap_or(false)
    }

    /// Swap the art now. Glints in flight and a held drag's grip follow it.
    fn set_flip(&mut self, f: bool) {
        if self.flip == f {
            return;
        }
        self.flip = f;
        let aw = self.layout.geo.width as f32;
        for g in &mut self.glints {
            g.x = aw - g.x;
        }
        self.rag_mirrored();
    }

    /// The turn's (sx, sy) at `at` (`facing::turn_shape` over `turn_ms`), and the flip shown then.
    fn turn_at(&self, at: Instant) -> Option<((f32, f32), bool)> {
        if !self.os.composited() {
            return None;
        }
        let (t0, to) = self.turn?;
        let ms = self.mirror_cfg.turn_ms.max(1) as f32;
        let u = at.saturating_duration_since(t0).as_secs_f32() * 1000.0 / ms;
        let shown = if u >= crate::facing::TURN_SWAP_U { to } else { self.flip };
        crate::facing::turn_shape(u).map(|k| (k, shown))
    }

    /// Frame `i` as drawn in the pack (never mirrored), else the default sprite.
    fn raw_frame_or_default(&self, i: usize) -> Option<&Frame> {
        self.frames
            .get(i)
            .and_then(|f| f.as_ref())
            .or_else(|| self.fr(i))
            .or_else(|| self.frames.get(self.pack.frame(&self.pack.catalog.default_sprite).ok()?)?.as_ref())
    }

    /// Advance a turn: the art swaps edge-on (`facing::TURN_SWAP_U`), it ends at `turn_ms`.
    fn step_turn(&mut self, now: Instant) -> bool {
        let Some((t0, to)) = self.turn else { return false };
        // Anything else taking her over (a glitch, a teleport, a grab) ends it at once.
        if self.glitch.is_some() || self.teleport.is_some() || self.rag.is_some() || self.hidden {
            self.turn = None;
            self.set_flip(to);
            return true;
        }
        if !self.os.composited() {
            match crate::facing::turn_band(self.turn_frame) {
                Some((_, new)) => {
                    let shown = if new { to } else { !to };
                    if shown != self.flip {
                        self.set_flip(shown);
                    }
                }
                None => {
                    self.turn = None;
                    self.set_flip(to);
                }
            }
            return true;
        }
        let ms = now.saturating_duration_since(t0).as_secs_f32() * 1000.0;
        let full = self.mirror_cfg.turn_ms as f32;
        // The art swaps on the turn's own clock (stepped or not).
        let shown = self.turn_at(now).map_or(to, |t| t.1);
        if shown != self.flip {
            self.set_flip(shown);
        }
        if ms >= full {
            self.turn = None;
        }
        true
    }

    fn loop_seq(&self, l: Option<&Steps>) -> Option<Seq> {
        let seq = self.pack.flatten(&l?.as_steps());
        (!seq.is_empty()).then_some(seq)
    }

    fn autonomy_gap(&mut self) -> Duration {
        // Her energy's own gaps (BRIEF-alive S11: 5-9 / 7-12 / 12-25 s), after the first.
        if self.autonomy_started
            && let Some([lo, hi]) = self.pack.behaviour.energy.band(self.band()).gap_ms
        {
            return Duration::from_millis(self.rng.range(lo as f32, hi.max(lo) as f32) as u64);
        }
        let a = &self.pack.catalog.autonomy;
        let life = &self.pack.behaviour.life;
        let base = if self.autonomy_started {
            life.activity_gap_ms.unwrap_or(a.activity_gap_ms)
        } else {
            life.first_activity_ms.unwrap_or(a.first_activity_ms)
        };
        Duration::from_millis(((base as f32 * self.rng.range(0.75, 1.35)) as u64).max(500))
    }

    /// Base frame and loop for a mood, given where the avatar is (standing or perched).
    fn layer(&self, m: Mood) -> (usize, Option<Seq>) {
        let default = self.pack.frame(&self.pack.catalog.default_sprite).unwrap_or(0);
        // A strip variant on screen (a perch, a sleep): its own loops for every mood.
        if let Some(l) = self.art_layer(m) {
            return l;
        }
        if let Some(l) = self.dance_layer() {
            return l;
        }
        if let Some(i) = self.music_frame() {
            return (i, None);
        }
        if let Some(i) = self.voice_layer() {
            return (i, None);
        }
        if let Some(i) = self.perch_sleep_frame(m) {
            return (i, None);
        }
        if self.sitting()
            && let Some(p) = self.perch_spec()
        {
            let id = p.moods.get(m.key()).unwrap_or(&p.base);
            return (self.pack.index.get(id).copied().unwrap_or(default), None);
        }
        if let Some(l) = self.brain_layer(m) {
            return l;
        }
        // Idle, low on energy: her tired face (the band's idle), else the mood's own.
        if m == Mood::Idle
            && let Some(t) = &self.pack.behaviour.energy.band(self.band()).idle
            && let Some(&b) = self.pack.index.get(&t.base)
        {
            return (b, self.loop_seq(t.r#loop.as_ref()));
        }
        let spec = self.spec(m);
        let base = spec.and_then(|s| self.pack.index.get(&s.base).copied()).unwrap_or(default);
        (base, self.loop_seq(spec.and_then(|s| s.r#loop.as_ref())))
    }

    fn activities(&self) -> &[Vec<crate::pack::Step>] {
        // Standing clips would break a hang or a floor sit; a strip variant blinks on its own.
        if self.posed() || (!self.sitting() && !self.standing()) || self.art_strip().is_some() {
            return &[];
        }
        match self.perch_spec() {
            Some(p) if self.sitting() => &p.activities,
            _ => {
                let own = &self.pack.behaviour.energy.band(self.band()).activities;
                if own.is_empty() { &self.pack.behaviour.activities } else { own }
            }
        }
    }

    /// Her energy's band now (mid without a brain or a time of day).
    pub(super) fn band(&self) -> crate::life::Band {
        self.brain.as_ref().and_then(|h| h.brain.band(h.bnow(Instant::now()))).unwrap_or(crate::life::Band::Mid)
    }

    pub fn set_mood(&mut self, m: Mood, now: Instant) {
        if m != Mood::Idle {
            self.music = None;
        }
        let prev = self.mood;
        let shown_before = self.anim.frame();
        self.cutin_mood(prev, m, now);
        // NEEDS YOU while she's hidden under a fullscreen window: a desktop notification with
        // lachy's capri.png (he can't see her tag).
        if m == Mood::NeedsYou
            && prev != m
            && self.loaded
            && self.brain.as_ref().is_some_and(|h| h.fs != brainview::FsHide::Shown)
        {
            let image = self.stills.as_ref().and_then(|s| s.capri_path.clone());
            crate::notify::send(format!("{} // NEEDS YOU", self.name.to_uppercase()), "Waiting on you behind the fullscreen window.".into(), image);
        }
        // NEEDS YOU: she looks at him (his pointer).
        if m == Mood::NeedsYou && prev != m && self.mirror_cfg.own && self.loaded {
            let x = self.origin.0 + self.layout.ground_x;
            if let Some(r) = crate::facing::toward(x, self.brain_cursor().0, (crate::facing::DEAD_PX * self.layout.sf) as i32) {
                self.face(r, "needs_you", now, false);
            }
        }
        let mut seq: Seq = Vec::new();
        // The pools see the new mood (a sleep pool in or out). A strip variant has its own way
        // in and out: the mood's CapriCore entry/exit don't play over it.
        let was_strip = self.art_strip().is_some();
        let floor_strip_sleep = prev == Mood::Sleepy && m != prev && was_strip && self.placement == Placement::Ground;
        self.mood = m;
        self.art_sync(now);
        // Out of a lachy floor sleep: its drawn wake (it stands her in the bridge drawing).
        // A stretch due after the nap replaces it (one seam into the stretch, not two).
        let drawn_wake = floor_strip_sleep && self.art.floor_strip && self.art_transition("wake").is_some() && !self.stretch_pending(now);
        self.mood = prev;
        let to_strip = self.art_strip().is_some();
        // An arrival sequence (the launch intro) plays through; the mood's entry follows it.
        let arriving = self.arriving(now);
        // Out of her sleep lying on a window top: she sits up there a beat before anything
        // moves her.
        if prev == Mood::Sleepy && m != prev && self.perch_sleep_frame(prev).is_some() {
            self.waking_until = Some(now + arriveview::SIT_UP);
        }
        if !self.sitting() && !self.posed() {
            if m != prev
                && !was_strip
                && let Some(exit) = self.spec(prev).and_then(|s| s.exit.clone())
            {
                seq.extend(self.pack.flatten(&exit));
                // Out of her sleep: she stands up before anything moves her.
                if prev == Mood::Sleepy {
                    let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
                    self.waking_until = Some(now + Duration::from_millis(ms));
                }
            }
            if !to_strip && let Some(entry) = self.spec(m).and_then(|s| s.entry.clone()) {
                seq.extend(self.pack.flatten(&entry));
            }
        }
        let (base, looping) = self.layer(m);
        let rests = self.art_rests(m);
        self.anim.set_layer_resting(base, looping, rests, now);
        if !seq.is_empty() && arriving {
            self.anim.then(seq, now);
        } else if !seq.is_empty() {
            self.anim.play(seq, now);
        } else if m != prev && !arriving {
            self.anim.stop_oneshot(now);
        }
        // A change of signal (its tone key: the colours are one red now) glitches, like
        // Capri's state bursts.
        // Speaking comes and goes with every reply; that alone is not worth a burst.
        let chatter = m == Mood::Talking || prev == Mood::Talking;
        // Into or out of her sleep: one light glitch on the swap frame, the tag whole, and the
        // family changes right after it (the wake, a stretch, the greeting) the same.
        let sleep_cut = m != prev && (m == Mood::Sleepy || prev == Mood::Sleepy);
        if sleep_cut && self.loaded && self.teleport.is_none() {
            self.art.quiet_seams = Some(now + QUIET_SEAMS);
            // Into a drawn sleep still decoding (~20-40 ms): the drawing before holds until it's
            // in, so the cut is straight into it (r39: a frame of the pack's floor sit first).
            let mut at = now;
            if m == Mood::Sleepy && to_strip && self.art_layer(m).is_none() && self.rag.is_none() && !arriving {
                self.anim.play(vec![(shown_before, SLEEP_DECODE_HOLD.as_millis() as u32)], now);
                self.art.cut_hold = Some(now + SLEEP_DECODE_HOLD);
                at = now + SLEEP_DECODE_HOLD;
            }
            match &mut self.rag {
                Some(r) => r.pending_glitch = r.pending_glitch.or(Some(GlitchKind::Seam)),
                None => self.glitch = Glitch::for_cut(Transition::Sleep, at, self.rng.next()),
            }
        } else if m != prev && !chatter && !drawn_wake && self.loaded && self.teleport.is_none() && self.status_of(m).key != self.status_of(prev).key {
            // A summon (into or out of NEEDS YOU): the big burst. Any other: the light seam (r42),
            // the tag's new word says it.
            let summon = m == Mood::NeedsYou || prev == Mood::NeedsYou;
            let t = if summon { Transition::Summon } else { Transition::Mood };
            match &mut self.rag {
                // Carried: kept (the strongest) and fired after the landing.
                Some(r) => r.pending_glitch = Some(if summon || r.pending_glitch == Some(GlitchKind::Big) { GlitchKind::Big } else { GlitchKind::Seam }),
                None => self.glitch = Glitch::for_cut(t, now, self.rng.next()),
            }
        }
        self.mood = m;
        if drawn_wake {
            self.art.floor_strip = false;
            self.play_drawn_wake(now);
        }
        let spec = self.spec(m).cloned();
        self.mood_then = spec
            .as_ref()
            .and_then(|s| s.then.as_ref())
            .and_then(|t| Some((now + Duration::from_millis(t.after_ms), Mood::parse(&t.mood)?)));
        let autonomous = spec.as_ref().map_or(m == Mood::Idle, |s| s.autonomy);
        self.next_activity = if autonomous && self.pack.catalog.autonomy.enabled && !self.activities().is_empty() {
            Some(now + self.autonomy_gap())
        } else {
            None
        };
        self.next_hop = spec.as_ref().and_then(|s| s.hop_every_ms).filter(|&p| p > 0).map(|p| now + jitter(&mut self.rng, p) / 2);
        if self.rag.is_some() {
            self.next_activity = None;
        }
        self.next_glitch = spec.as_ref().and_then(|s| s.glitch_every_ms).filter(|&p| p > 0).map(|p| now + jitter(&mut self.rng, p));
    }

    /// Re-apply the current mood's layer after the placement changed (no entry/exit).
    fn relayer(&mut self, now: Instant) {
        self.art.fits.borrow_mut().clear();
        self.art_sync(now);
        let before = self.anim.frame();
        let (base, looping) = self.layer(self.mood);
        let rests = self.art_rests(self.mood);
        self.anim.set_layer_resting(base, looping, rests, now);
        // An arrival sequence plays through the landing's relayer, a walk through its pool's
        // art coming in, a stand-up out of her sleep through the nap's end, a cut's hold.
        let holding = self.art.cut_hold.is_some_and(|t| now < t);
        if !self.arriving(now) && !self.brain_walking() && !self.waking(now) && !holding {
            self.anim.stop_oneshot(now);
        }
        self.lying_cut(before, now);
        let autonomous =
            self.music.is_none() && self.spec(self.mood).map_or(self.mood == Mood::Idle, |s| s.autonomy) && !self.activities().is_empty();
        let next = if autonomous { Some(now + self.autonomy_gap()) } else { None };
        self.next_activity = next;
    }

    fn status_of(&self, m: Mood) -> Status {
        let spec = self.spec(m);
        let key = spec.and_then(|s| s.tone.clone()).unwrap_or_default();
        Status {
            label: spec.and_then(|s| s.status.clone()).unwrap_or_else(|| m.default_status().into()),
            tone: self.pack.tone(spec.and_then(|s| s.tone.as_deref())),
            invert: spec.is_some_and(|s| s.invert),
            glyph: spec.and_then(|s| s.glyph.clone()).unwrap_or_else(|| "dot".into()),
            plate: spec.and_then(|s| s.plate.as_deref()).and_then(Plate::parse).unwrap_or_default(),
            key,
        }
    }

    fn hidden_reason(&self) -> Option<&'static str> {
        if self.parked.is_some() {
            Some("user")
        } else if self.brain.as_ref().is_some_and(|h| h.fs != brainview::FsHide::Shown) {
            Some("fullscreen")
        } else if !self.hidden {
            None
        } else if self.teleport.is_some() {
            Some("teleport")
        } else {
            Some("other")
        }
    }

    /// Her tag. While the Line waits for his key or a click (the desktop didn't hand her the
    /// keyboard), it says how to type.
    fn status(&self) -> Status {
        let mut s = self.status_now();
        if self.line.as_ref().is_some_and(|l| l.waiting) {
            s.label = format!("{} · {} TO TYPE", s.label, self.summon_label());
        }
        s
    }

    fn status_now(&self) -> Status {
        if let Some(s) = self.voice_status() {
            return s;
        }
        if let (Some(_), Some(spec)) = (&self.music, self.music_spec()) {
            return Status {
                label: spec.status.clone().unwrap_or_else(|| "LISTENING".into()),
                tone: self.pack.tone(spec.tone.as_deref()),
                invert: false,
                glyph: spec.glyph.clone().unwrap_or_else(|| "note".into()),
                plate: spec.plate.as_deref().and_then(Plate::parse).unwrap_or(Plate::Arrow),
                key: spec.tone.clone().unwrap_or_else(|| "music".into()),
            };
        }
        // Sulking (and her cold shoulder after it): the pack's sulk word, not ONLINE.
        if self.mood == Mood::Idle
            && self.brain.as_ref().is_some_and(|h| h.sulky())
            && let Some(ss) = &self.pack.behaviour.brain.sulk_status
        {
            return Status {
                label: ss.status.clone(),
                tone: self.pack.tone(ss.tone.as_deref()),
                invert: false,
                glyph: ss.glyph.clone().unwrap_or_else(|| "dot".into()),
                plate: Plate::default(),
                key: ss.tone.clone().unwrap_or_default(),
            };
        }
        let mut s = self.status_of(self.mood);
        if let Some((m, l)) = &self.label_override
            && *m == self.mood
        {
            s.label = l.clone();
        }
        if self.voice_wake_idle() && s.glyph == "dot" {
            s.glyph = "mic".into();
        }
        s
    }

    // ---- perch shadow ---------------------------------------------------------------

    /// Contact sliver for a perch: a dark band a few px thick on the window face right under
    /// the edge, under every column where the figure touches the edge (seat, thighs, the
    /// planted hand: opaque in the rows just above the line), widened by a pixel each side,
    /// minus the legs hanging in front. Each stretch tapers to 1 px at its ends (a soft
    /// contact, not a hard bar). 1-bit friendly: no dither. Avatar-local device px.
    fn shadow_rects(&self, frame: usize) -> Vec<ShapeRect> {
        let l = &self.layout;
        let Some(p) = self.perch_spec() else { return Vec::new() };
        let (Some(f), Some(seat)) = (self.fr(frame), l.seat_y) else { return Vec::new() };
        let thick = (p.sliver_px * l.sf).round() as i32;
        if thick <= 0 {
            return Vec::new();
        }
        let line = seat - l.avatar_y;
        let (w, h) = (f.mask.w as i32, f.mask.h as i32);
        let touch_rows = (line - (3.0 * l.sf).round() as i32).max(0)..line.min(h);
        let pad = l.sf.round().max(1.0) as i32;
        let mut under = vec![false; w as usize];
        for x in 0..w {
            if touch_rows.clone().any(|y| f.mask.get(x, y)) {
                for k in (x - pad).max(0)..=(x + pad).min(w - 1) {
                    under[k as usize] = true;
                }
            }
        }
        // Thickness per column: 1 px at a stretch's ends, `thick` from `ramp` px in.
        let ramp = (4.0 * l.sf).round().max(1.0) as i32;
        let mut depth = vec![0i32; w as usize];
        let mut x = 0;
        while x < w {
            if !under[x as usize] {
                x += 1;
                continue;
            }
            let a = x;
            while x < w && under[x as usize] {
                x += 1;
            }
            for c in a..x {
                let d = (c - a).min(x - 1 - c);
                depth[c as usize] = (1 + d * (thick - 1) / ramp).min(thick);
            }
        }
        let mut out = Vec::new();
        for y in line.max(0)..(line + thick).min(h) {
            let mut x = 0;
            let on = |x: i32| depth[x as usize] > (y - line) && !f.mask.get(x, y);
            while x < w {
                if !on(x) {
                    x += 1;
                    continue;
                }
                let s0 = x;
                while x < w && on(x) {
                    x += 1;
                }
                out.push((s0 as i16, y as i16, (x - s0) as u16, 1));
            }
        }
        out
    }

    /// Show, move or hide the contact shadow to match the placement.
    fn sync_shadow(&mut self) {
        let now = Instant::now();
        // None under an arrival's touchdown (a dark bar under the crouch's hand right after the
        // contact): it comes a beat after her pose is on screen, the pose's own.
        let settled = self.arrive.until.is_none_or(|t| now >= t + SHADOW_AFTER_ARRIVAL);
        let want = self.sitting() && self.loaded && !self.hidden && self.teleport.is_none() && settled && !self.quitting && self.press.is_none() && self.rag.is_none();
        // The resting frame of the current layer (music swaps the pose), else the perch base.
        let shown = self.anim.base();
        let base = Some(shown)
            .filter(|&i| self.pack.frames.get(i).is_some_and(|f| f.anchor.name == "seat"))
            .or_else(|| self.perch_spec().and_then(|p| self.pack.index.get(&p.base).copied()));
        let (Some(base), true) = (base, want) else {
            if self.shadow_at.take().is_some() {
                self.os.fx_unmap(SHADOW_LAYER);
                self.os.flush();
            }
            return;
        };
        let key = (base, self.mirrored(base));
        if self.shadow.as_ref().map(|s| s.0) != Some(key) {
            self.shadow = Some((key, self.shadow_rects(base)));
            if self.shadow_at.take().is_some() {
                self.os.fx_unmap(SHADOW_LAYER);
            }
        }
        let l = self.layout;
        let at = (self.applied.0 + l.avatar_x, self.applied.1 + l.avatar_y);
        if self.shadow_at != Some(at) {
            let rects = self.shadow.as_ref().map(|s| s.1.clone()).unwrap_or_default();
            let reshape = self.shadow_at.is_none();
            // Cut off at the window's bottom edge: the shadow falls on the window, not past it.
            let win = match self.placement {
                Placement::Perch { win, .. } => self.os.toplevel_rect(win),
                _ => None,
            };
            let h = win.map_or(l.geo.height as i32, |r| (r.y + r.h - at.1).clamp(1, l.geo.height as i32));
            let rgb = self.perch_spec().and_then(|p| u32::from_str_radix(p.sliver.trim_start_matches('#'), 16).ok()).unwrap_or(0x05060a);
            self.os.fx_under(SHADOW_LAYER, rgb, at, (l.geo.width as i32, h), reshape.then_some(&rects[..])).ok();
            self.shadow_at = Some(at);
        }
    }

    /// Unmap every effect layer (the shadow too).
    fn fx_hide_all(&mut self) {
        self.os.fx_hide();
        self.shadow_at = None;
        self.cover_on = false;
    }

    /// A drawn touchdown's red silhouette, server-painted over her as well: her window shows
    /// its old pixels inside a shape that grows until its present lands (r34: the bolt's
    /// stretch onto a lachy perch read as an empty edge, then a smear). Shape mode only.
    fn sync_cover(&mut self, shape: &[ShapeRect], size: (i32, i32), red: bool) {
        if !red {
            if std::mem::take(&mut self.cover_on) {
                self.os.fx_unmap(COVER_LAYER);
                self.os.flush();
            }
            return;
        }
        let l = self.layout;
        let at = (self.applied.0 + l.avatar_x, self.applied.1 + l.avatar_y);
        self.os.fx_set(COVER_LAYER, fx::BOLT_RED, at, size, shape).ok();
        self.os.flush();
        self.cover_on = true;
    }

    fn fx_hide_bolt(&self) {
        self.os.fx_unmap(BOLT_LAYER);
        self.os.flush();
    }

    // ---- timing -------------------------------------------------------------------

    fn hop_allowed(&self) -> bool {
        self.loaded
            && !matches!(self.placement, Placement::Perch { sit: true, .. } | Placement::Pose { .. })
            && self.press.is_none()
            && self.rag.is_none()
            && self.teleport.is_none()
            && self.music.is_none()
    }

    fn next_deadline(&self) -> Option<Instant> {
        let now = Instant::now();
        let mut t: Option<Instant> = None;
        let mut take = |d: Option<Instant>| {
            if let Some(d) = d {
                t = Some(t.map_or(d, |t| t.min(d)));
            }
        };
        take(self.anim.deadline());
        // A frame before each cut of her frames: a render that lays the next drawing
        // underneath (the look-ahead), so the cut's new outline never shows pixels the window
        // hasn't drawn (r43: a held wave frame cut to the next showed the SLEEPING tag of
        // seconds before and dashed edges along her arm for a frame).
        if let Some(d) = self.anim.deadline().filter(|&d| self.precut != Some(d) && !self.hidden) {
            take(Some(d.checked_sub(FRAME_FX).unwrap_or(d).max(now)));
        }
        take(self.kiss_deadline());
        take(self.art.deadline());
        take(self.dance_deadline());
        take(self.burst_deadline());
        take(self.voice_deadline(now));
        if !self.anim.busy() {
            take(self.next_activity);
        }
        take(self.mood_then.map(|m| m.0));
        if let Some(b) = &self.bubble {
            take(b.next_char);
            take(b.hide_at);
        }
        if let Some((t0, _)) = self.hearts {
            take(Some((now + FRAME_FX).min(t0 + Duration::from_millis(HEARTS_MS))));
        }
        take(self.reaction_until);
        take(self.perch_gone.map(|g| g.due()));
        take(self.quit_armed);
        take(self.first_words.map(|f| f.0));
        take(self.music_stop_at);
        if self.unpark_req.is_some() {
            take(Some(now));
        }
        take(self.hud_after.filter(|&t| t > now));
        take(self.motion_at.get());
        take(self.menu.map(|m| m.1));
        if let Some(s) = &self.session {
            take(s.link.deadline(now));
            take(s.respawn_at);
            take(s.link.offline_for(now).map(|d| now + respawn_after().saturating_sub(d)));
        }
        if self.quit_requested {
            take(Some(now));
        }
        take(self.hint.as_ref().map(|h| h.1));
        take(self.input.as_ref().and_then(|i| i.esc_at).map(|t| t + ESC_AGAIN));
        take(self.prompt_ui.as_ref().map(|p| p.shown_at + PROMPT_ARM).filter(|&t| t > now));
        if self.glitch.is_some() {
            // As often as GPUI presents, so every presented frame has a new pattern.
            take(Some(now + FRAME_FAST));
        }
        if self.teleport.as_ref().is_some_and(|t| matches!(t.leg, Leg::Bolt(_))) {
            take(Some(now + FRAME_FAST));
        }
        take(self.next_glitch);
        if self.squash.is_some() || self.sliver.is_some() || self.turn.is_some() || self.arrive.unfold.is_some() || self.motion.lean.is_some() {
            take(Some(now + FRAME_FAST));
        }
        take(self.teleport.as_ref().and_then(|t| t.leave_until));
        take(self.arrive.red_until.filter(|&t| t > now));
        take(self.cutins.due());
        take(self.arrive.until.map(|t| t + SHADOW_AFTER_ARRIVAL).filter(|&t| t > now));
        // Glints: at their slice centres (fx::GLINT_STEPS per life), not a free-running tick;
        // a step waiting for company (GLINT_SLACK) at the end of its wait.
        let life_ms = self.pack.behaviour.life.sparkle_ms;
        match self.glint_wait {
            Some(t) => take(Some(t + GLINT_SLACK)),
            None => take(self.glints.iter().filter_map(|g| g.next_sample(now, life_ms)).min()),
        }
        take(self.next_glint);
        // One wake per pose step, at its slack's end (not one at the step and one after).
        match self.pose_wait {
            Some(t) => take(Some(t + POSE_SLACK)),
            None => take(self.pose_next_change(now).map(|t| t + POSE_SLACK)),
        }
        if self.music.is_some() {
            take(Some(now + NOTE_TICK));
            take(self.music.as_ref().and_then(|m| m.until));
        }
        if self.glide.is_some() {
            take(Some(now + FRAME_FAST));
        }
        if self.hosts_stale {
            take(Some(now + Duration::from_millis(250)));
        }
        if self.hop.is_some() {
            take(Some(now + FRAME_FAST));
        } else if self.hop_allowed() {
            take(self.next_hop);
        }
        take(self.brain_deadline(now));
        take(self.rag_deadline(now));
        take(self.follow_deadline(now));
        if self.swing_on() {
            take(self.brain.as_ref().and_then(|h| h.swing.as_ref()).map_or(Some(now), |s| Some(s.due)));
        }
        t
    }

    /// A body host of hers (`crate::host`).
    pub fn add_host(&mut self, h: crate::host::Host) {
        self.hosts.push(h);
    }

    pub fn set_fx_hosts(&mut self, fx: Vec<crate::host::FxHost>) {
        self.fx_hosts = fx;
    }

    /// She moved to another host: it shows her, the others nothing. The monitors changed:
    /// a dormant host for each new one, the hosts of the ones gone closed.
    fn hosts_sync(&mut self, cx: &mut Context<Self>) {
        crate::host::fx_hosts_sync(cx, &self.os, &mut self.fx_hosts);
        if self.hosts.is_empty() {
            return;
        }
        if std::mem::take(&mut self.hosts_stale) {
            let gone = self.os.hosts_gone();
            for (id, _, w) in self.hosts.iter().filter(|h| gone.contains(&h.0)) {
                elog!("synaps-avatar: body host 0x{id:x} closed (its monitor went away)");
                w.update(cx, |_, window, _| window.remove_window()).ok();
            }
            self.hosts.retain(|h| !gone.contains(&h.0));
            let me = cx.entity();
            let size = (self.layout.win_w, self.layout.win_h);
            for m in self.os.hosts_wanted() {
                match crate::host::open_dormant(cx, &self.os, &me, &m, size, self.layout.sf) {
                    Some(h) => self.hosts.push(h),
                    None => self.hosts_stale = true,
                }
            }
            let fx = std::mem::take(&mut self.fx_hosts);
            crate::host::close_fx_hosts(cx, fx);
            self.fx_hosts = crate::host::open_fx_hosts(cx, &self.os, self.layout.sf);
            // Where she is now picks the host (hers may be gone).
            self.applied = (i32::MIN, i32::MIN);
            self.apply_offset(Instant::now());
        }
        if let Some(active) = self.os.take_host_change() {
            for (id, v, _) in &self.hosts {
                v.update(cx, |h, cx| {
                    h.active = *id == active;
                    cx.notify();
                })
                .ok();
            }
            cx.notify();
        }
    }

    fn schedule(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.hosts_sync(cx);
        // A pool in or out by a path that didn't relayer (a move along, a teleport's target).
        if self.art_sync(now) && self.loaded && self.layer(self.mood).0 != self.anim.base() {
            self.relayer(now);
        }
        self.art_seam(now);
        self.art_fetch(cx);
        self.cutin_fetch(cx);
        self.sync_shadow();
        self.sync_pointer_watch();
        if self.os.fx_take_dirty() {
            cx.notify();
        }
        self.ticker.set(self.next_deadline());
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.view_ticks += 1;

        if self.sliver.is_some_and(|t| now.saturating_duration_since(t) > SLIVER) {
            self.sliver = None;
        }
        let before = self.anim.frame();
        let mut dirty = self.anim.tick(now);
        if let Some(d) = self.anim.deadline()
            && self.precut != Some(d)
            && now + FRAME_FX >= d
        {
            self.precut = Some(d);
            dirty |= now < d && !self.hidden && self.anim.frame_at(d) != self.anim.frame();
        }
        self.voice_keepalive(now, cx);
        if dirty {
            self.lying_cut(before, now);
        }
        dirty |= self.step_kiss(now);
        dirty |= self.art_tick(now);
        dirty |= self.dance_tick(now);
        self.burst_tick(now);
        dirty |= self.voice_tick(now, cx);
        dirty |= self.step_turn(now);
        if let Some((at, m)) = self.mood_then
            && at <= now
        {
            self.set_mood(m, now);
            dirty = true;
        }
        if self.band_relayer && self.mood == Mood::Idle && !self.anim.busy() && !self.asleep() && self.rag.is_none() && self.teleport.is_none() {
            self.band_relayer = false;
            self.relayer(now);
            dirty = true;
        }
        if let Some(at) = self.next_activity
            && at <= now
            && !self.anim.busy()
            && !self.activities().is_empty()
            && !self.following()
        {
            let music = self
                .music_spec()
                .filter(|m| m.activity_chance > 0.0 && self.mood == Mood::Idle && self.music.is_none())
                .map(|m| (m.activity_chance, m.activity_ms));
            if let Some((chance, ms)) = music
                && self.rng.range(0.0, 1.0) < chance
                && self.music_on(now, Some(now + Duration::from_millis(ms))).is_ok()
            {
                self.autonomy_started = true;
            } else {
                let n = self.activities().len();
                let i = self.activity_pick(n);
                let steps = self.activities()[i].clone();
                let seq = self.pack.flatten(&self.resolve_steps(&steps));
                let len: u64 = seq.iter().map(|s| s.1 as u64).sum();
                self.activity_beats(&steps, Duration::from_millis(len), now);
                self.anim.play(seq, now);
                self.beat_until = Some(now + Duration::from_millis(len));
                self.autonomy_started = true;
                self.next_activity = Some(now + Duration::from_millis(len) + self.autonomy_gap());
            }
            dirty = true;
        }
        let mut typed_out = false;
        let mut stream = false;
        if let Some(b) = &mut self.bubble {
            stream = b.stream;
            if let Some(at) = b.next_char
                && at <= now
            {
                // A stream keeps pace with the text as it arrives.
                let step = if b.stream { ((b.text.len() - b.shown) / 6).max(b.per_tick) } else { b.per_tick };
                b.shown = (b.shown + step).min(b.text.len());
                dirty = true;
                if b.shown == b.text.len() {
                    b.next_char = None;
                    typed_out = true;
                    if !b.more {
                        b.hide_at = Some(now + hold_for(b.text.len()));
                    }
                } else {
                    b.next_char = Some(now + TYPE_TICK);
                }
            }
            if let Some(at) = b.hide_at
                && at <= now
            {
                if b.stream {
                    // The last reply stays behind the chip; unseen, it leaves a strip.
                    b.collapsed = true;
                    b.hide_at = None;
                    let text = b.text.clone();
                    self.reply_missed(&text);
                } else {
                    self.bubble = None;
                }
                dirty = true;
            }
        }
        // Keep SPEAKING up a beat after the last word, then settle.
        if typed_out && !stream && self.mood == Mood::Talking {
            self.mood_then = Some((now + Duration::from_millis(1_400), Mood::Idle));
        }
        if let Some((t0, _)) = self.hearts {
            dirty = true;
            if now.duration_since(t0) >= Duration::from_millis(HEARTS_MS) {
                self.hearts = None;
            }
        }
        if let Some(t) = self.reaction_until
            && t <= now
        {
            self.reaction_until = None;
            dirty = true;
        }
        if self.hud_after.is_some_and(|t| t <= now) {
            self.hud_after = None;
            dirty = true;
        }
        if let Some((_, until)) = self.menu
            && until <= now
        {
            self.menu = None;
            dirty = true;
        }
        if self.hint.as_ref().is_some_and(|h| h.1 <= now) {
            self.hint = None;
            dirty = true;
        }
        // One Escape during a turn: the field closes when the second didn't come.
        if self.input.as_ref().and_then(|i| i.esc_at).is_some_and(|t| t + ESC_AGAIN <= now) {
            self.close_input(true);
            dirty = true;
        }
        if self.prompt_ui.as_ref().is_some_and(|p| p.shown_at + PROMPT_ARM <= now && p.shown_at + PROMPT_ARM + FRAME_FAST > now) {
            dirty = true;
        }
        if let Some(s) = &self.session
            && !s.closing
            && (s.respawn_at.is_some_and(|t| t <= now) || s.link.offline_for(now).is_some_and(|d| d >= respawn_after()))
        {
            self.respawn(now, cx);
            dirty = true;
        }
        if self.quit_requested && !self.quitting {
            self.quit_requested = false;
            self.vanish_quit(now);
            dirty = true;
        }
        if let Some(why) = self.unpark_req.take() {
            dirty |= self.unpark(why, now, cx);
        }
        if self.music_stop_at.is_some_and(|t| now >= t) {
            self.music_off(now);
            dirty = true;
        }
        if self.player.playing && self.music.is_none() && self.loaded && self.mood == Mood::Idle && self.parked != Some(false) {
            let p = self.player;
            self.playback(p, now);
            dirty = true;
        }
        if self.quit_armed.is_some_and(|t| now >= t) {
            self.quit_armed = None;
            dirty = true;
        }
        // The HUD's next motion step (render books the next one).
        if self.motion_at.get().is_some_and(|t| now >= t) {
            self.motion_at.set(None);
            dirty = true;
        }
        if let Some(acts) = self.session.as_mut().map(|s| s.link.tick(now)) {
            self.run_acts(acts);
            let before = self.mood;
            self.sync_body(now);
            dirty |= self.mood != before;
        }

        // Effects.
        if let Some(g) = self.glitch {
            dirty = true;
            if g.done(now) {
                self.glitch = None;
                self.glitch_done(now, cx);
            }
        }
        if let Some(at) = self.next_glitch
            && at <= now
        {
            if self.glitch.is_none() && self.teleport.is_none() && self.loaded {
                self.glitch = Some(Glitch::new(GlitchKind::Burst, now, self.rng.next()));
                dirty = true;
            }
            let p = self.spec(self.mood).and_then(|s| s.glitch_every_ms).filter(|&p| p > 0);
            self.next_glitch = p.map(|p| now + jitter(&mut self.rng, p));
        }
        if let Some(t0) = self.squash {
            dirty = true;
            if self.squash_at(t0, now).is_none() {
                self.squash = None;
                self.arrive.squash_end = None;
            }
        }
        let life_ms = self.pack.behaviour.life.sparkle_ms;
        if !self.glints.is_empty() {
            self.glints.retain(|g| g.scale(now, life_ms).is_some());
        }
        if let Some(at) = self.next_glint
            && at <= now
        {
            self.spawn_glints(now);
        }
        if self.teleport.as_ref().is_some_and(|t| matches!(t.leg, Leg::Vanish) && t.leave_until.is_some_and(|u| u <= now)) {
            self.vanish_done(now);
            dirty = true;
        }
        dirty |= self.step_teleport(now);
        if let Some(Leg::Bolt(t0)) = self.teleport.as_ref().map(|t| t.leg) {
            self.intro_bolt_drawn(now.saturating_duration_since(t0), cx);
        }
        if self.music.as_ref().and_then(|m| m.until).is_some_and(|u| u <= now) {
            self.music_off(now);
            dirty = true;
        }

        // Window offset: the attention hop moves the window, no redraw.
        if let Some(at) = self.next_hop
            && at <= now
            && self.hop.is_none()
        {
            if self.hop_allowed() {
                self.hop = Some(now);
            }
            let p = self.spec(self.mood).and_then(|s| s.hop_every_ms).filter(|&p| p > 0);
            self.next_hop = p.map(|p| now + jitter(&mut self.rng, p));
        }
        if let Some(t0) = self.hop
            && fx::hop(now.duration_since(t0), 1.0).is_none()
        {
            self.hop = None;
        }
        dirty |= self.check_perch_gone(now);
        dirty |= self.rag_follow_due();
        dirty |= self.rag_tick(now);
        dirty |= self.swing_tick(now);
        if self.loaded {
            dirty |= self.brain_tick(now);
        }
        if self.first_words.is_some_and(|f| f.0 <= now) {
            dirty |= self.first_words_try(now);
        }
        self.apply_offset(now);
        self.panels_follow(cx);
        let costly = self.os.redraw_costly();
        if !costly {
            self.update_notes(now);
            self.update_glints(now);
        }
        self.update_marker(now);
        // Any redraw draws the current pose (render latches it); a pose step alone waits
        // POSE_SLACK for company.
        let pose = self.pose_at(now);
        // Her bend toward the pointer eases over 0.6-0.9 s: every step is drawn (waiting for
        // company held it back until it was over, r42).
        if pose.bend != self.pose_last.bend || pose.bend_hair != self.pose_last.bend_hair {
            dirty = true;
        }
        if pose != self.pose_last && !dirty {
            // Since when this pose has held (20 ms steps back): the step's time, even when
            // this tick is the first to see it.
            if self.pose_wait.is_none() {
                let held = (1..=20)
                    .map_while(|k| now.checked_sub(Duration::from_millis(20 * k)))
                    .take_while(|&t| self.pose_at(t) == pose)
                    .last();
                self.pose_wait = Some(held.unwrap_or(now));
            }
            let since = self.pose_wait.unwrap_or(now);
            dirty = now.saturating_duration_since(since) + Duration::from_millis(20) >= POSE_SLACK;
        }
        // Where every redraw is a full present, a glint step rides the next frame drawn anyway
        // (the breath and blink steps) and waits GLINT_SLACK at most.
        let glint_step = costly && self.glint_step_due(now);
        if glint_step && !dirty {
            let since = *self.glint_wait.get_or_insert(now);
            dirty = now.saturating_duration_since(since) + Duration::from_millis(20) >= GLINT_SLACK;
        }
        // Latched here, not in render: a wait left set until GPUI got round to rendering
        // was a past deadline (it spun ~900 ticks/s).
        if dirty || pose == self.pose_last {
            self.pose_last = pose;
            self.pose_wait = None;
        }
        if dirty || !glint_step {
            self.glint_wait = None;
        }
        // Notes ride the frames drawn anyway too, and move at least every fourth note tick
        // (6 fps; X11 without a compositor moves them at 25 without a redraw). Measured in the rig: music at 12 fps
        // drew 500 frames a minute (1.3 % CPU), the same idle without it 186.
        if costly && !dirty && self.music.is_some() && now.saturating_duration_since(self.notes_at) + Duration::from_millis(5) >= 4 * NOTE_TICK {
            dirty = true;
        }
        if costly && dirty {
            self.update_notes(now);
            self.update_glints(now);
        }
        // Effects painted in her own window (no effect surfaces on this desktop) need a frame.
        if dirty || self.os.fx_take_dirty() {
            cx.notify();
        }
        self.schedule(cx);
    }

    fn spawn_glints(&mut self, now: Instant) {
        let [lo, hi] = self.pack.behaviour.energy.band(self.band()).sparkle_every_ms.unwrap_or(self.pack.behaviour.life.sparkle_every_ms);
        if hi == 0 || self.pack.behaviour.life.sparkle_ms == 0 {
            self.next_glint = None;
            return;
        }
        // One at a time on an even beat (r32: pairs and long random gaps left dead stretches);
        // when she can't show one now, soon again rather than a whole gap later.
        let gap = (self.rng.range(lo as f32, hi.max(lo) as f32) as u64).max(250);
        self.next_glint = Some(now + Duration::from_millis(GLINT_RETRY_MS));
        if !self.loaded || self.hidden || self.teleport.is_some() || self.rag.is_some() || self.arriving(now) || self.glitch.is_some() || self.turn.is_some() {
            return;
        }
        let n = self.frame_or_default(self.shown_frame()).map_or(0, |f| f.glints.len());
        if n == 0 {
            return;
        }
        // A point where all of it stays on the hair (a few tries; else none this beat).
        let sf = self.layout.sf;
        let tries: Vec<(usize, f32, f32, f32)> = (0..6)
            .map(|_| {
                let (i, spin) = (self.rng.pick(n), self.rng.range(-0.25, 0.25));
                let travel = self.rng.range(6.0, 10.0) * sf;
                let dir = if self.rng.range(0.0, 1.0) < 0.5 { -1.0 } else { 1.0 };
                (i, spin, travel * dir, self.rng.range(8.0, 9.5) * sf)
            })
            .collect();
        let Some(f) = self.frame_or_default(self.shown_frame()) else { return };
        let pick = tries.into_iter().map(|(i, spin, travel, size)| {
            let (x, y) = f.glints[i];
            Glint {
                t0: now,
                x: x as f32,
                y: y as f32,
                // Big enough to read over her hair's own glitter, a quarter under main's.
                size,
                spin,
                // A little travel along its long arm (the strand it sits on).
                drift: (spin.cos() * travel, spin.sin() * travel),
                live: false,
            }
        });
        let fits = |g: &Glint| g.reach().iter().all(|&(x, y)| f.mask.get(x.round() as i32, y.round() as i32));
        if let Some(g) = pick.into_iter().find(fits) {
            self.next_glint = Some(now + Duration::from_millis(gap));
            self.glints.push(g);
        }
    }

    /// The vanish is over (its glitch, or a drawn leave): the bolt, or straight there.
    fn vanish_done(&mut self, now: Instant) {
        let Some(tp) = &mut self.teleport else { return };
        tp.leave_until = None;
        let (to, place, seed, no_bolt) = (tp.to, tp.place, tp.seed, tp.no_bolt);
        if no_bolt {
            // Vanished: appear there at once, the landing glitch reassembles her.
            self.arrive(to, place, now);
            self.hidden = false;
            self.glitch = Some(self.land_glitch(now, seed));
            self.arrive_appeared(now);
            if let Some(tp) = &mut self.teleport {
                tp.moved = true;
                tp.leg = Leg::Appear;
            }
            return;
        }
        self.hidden = true;
        let from = self.center_root(self.origin);
        let to = self.center_root(to);
        let bolt = Bolt::new(from, to, seed ^ 0xb017_5eed, 220.0 * self.layout.sf);
        if let Some(tp) = &mut self.teleport {
            tp.bolt = Some(bolt);
            tp.leg = Leg::Bolt(now);
        }
    }

    fn glitch_done(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self.quitting {
            self.quit(cx);
            return;
        }
        if self.parked == Some(false) {
            self.parked_done(cx);
            return;
        }
        let Some(leg) = self.teleport.as_ref().map(|t| t.leg) else { return };
        match leg {
            Leg::Vanish => self.vanish_done(now),
            Leg::Appear => {
                self.teleport_done(now);
                if self.placement == Placement::Ground {
                    state::save_ground(self.ground());
                }
            }
            Leg::Bolt(_) => {}
        }
    }

    /// The bolt leg of a teleport. True if the view must redraw.
    fn step_teleport(&mut self, now: Instant) -> bool {
        let Some(tp) = &mut self.teleport else { return false };
        let Leg::Bolt(t0) = tp.leg else { return false };
        let u = now.duration_since(t0).as_secs_f32() * 1000.0 / fx::BOLT_MS;
        let (to, place, mut moved, seed) = (tp.to, tp.place, tp.moved, tp.seed);
        let mut redraw = false;
        // Onto a window top in a drawn pose (lachy's perch): she is there, its red silhouette,
        // from the moment the bolt reaches her (no stretch of bare trail before she forms).
        // A frame before, it is painted (still outside her shape): when the shape takes it in,
        // the screen has it already (no dark first frame).
        let mut early = false;
        let quiet = self.teleport.as_ref().is_some_and(|t| t.quiet);
        let pre = FRAME_FX.as_secs_f32() * 1000.0 / fx::BOLT_MS;
        if (fx::BOLT_ARRIVE - pre..fx::BOLT_LAND).contains(&u) && self.hidden && !quiet && self.arrive.pending() && matches!(place, Placement::Perch { sit: true, .. }) {
            if !moved {
                moved = true;
                if let Some(tp) = &mut self.teleport {
                    tp.moved = true;
                }
                self.arrive(to, place, now);
            }
            if self.drawn_touchdown(now) {
                early = u >= fx::BOLT_ARRIVE;
                self.arrive.prepaint = !early;
                redraw = true;
            }
        }
        // The trail is short and closing in: the landing glitch starts under it.
        if (u >= fx::BOLT_LAND || early) && self.hidden {
            self.hidden = false;
            self.arrive.prepaint = false;
            self.glitch = Some(self.land_glitch(now, seed));
            if self.teleport.as_ref().is_some_and(|t| t.quiet) {
                self.sliver = Some(now);
            }
            if !moved {
                moved = true;
                if let Some(tp) = &mut self.teleport {
                    tp.moved = true;
                }
                self.arrive(to, place, now);
            }
            self.arrive_appeared(now);
            redraw = true;
        }
        // The trail has drained into her: the landing glitch finishes the teleport.
        if u >= fx::BOLT_END {
            self.fx_hide_bolt();
            if self.glitch.is_none() {
                // The landing already finished (a stalled frame): done.
                self.teleport_done(now);
                if self.placement == Placement::Ground {
                    state::save_ground(self.ground());
                }
            } else if let Some(tp) = &mut self.teleport {
                tp.leg = Leg::Appear;
            }
            return true;
        }
        let Some(tp) = &self.teleport else { return redraw };
        if let Some(b) = &tp.bolt {
            let (bx, by, bw, bh) = b.bbox();
            let o = (bx, by);
            // One plain red layer, nothing else (LOCKED #1). A passive move draws a short
            // thin shard instead of the full strike (r7: the zig-zag was the loudest frame).
            let poly = if tp.quiet { b.streak(u, 90.0 * self.layout.sf, 6.0 * self.layout.sf) } else { b.frame(u) };
            self.os.fx_set(BOLT_LAYER, fx::BOLT_LINE, o, (bw, bh), &fx::bolt_rects(&poly, o)).ok();
            self.os.flush();
        }
        if u >= fx::BOLT_ARRIVE && !moved {
            if let Some(tp) = &mut self.teleport {
                tp.moved = true;
            }
            self.arrive(to, place, now);
        }
        redraw
    }

    /// Land at the teleport target (window moved while the body is hidden).
    fn arrive(&mut self, to: (i32, i32), place: Placement, now: Instant) {
        let was_sitting = self.sitting();
        self.placement = place;
        self.set_origin(to);
        match place {
            Placement::Perch { win, .. } => self.os.set_target(win),
            _ => self.os.set_target(0),
        }
        if was_sitting != self.sitting() || self.brain.is_some() {
            self.relayer(now);
        }
        // Set down somewhere (not by a brain move: its landing knows what she came for; not at
        // a walk's end: she stops facing the way she walked):
        // near a monitor's edge she faces into the screen; perched, the window's middle.
        if self.mirror_cfg.own && self.loaded && !self.brain_in_flight() && !self.brain_walking() {
            let l = self.layout;
            let (x, y) = (to.0 + l.ground_x, to.1 + l.ground_y);
            let dead = (crate::facing::DEAD_PX * l.sf) as i32;
            let m = self.os.monitor_at(x, y - 1);
            let face = crate::facing::inward(x, m.x, m.w).or_else(|| match place {
                Placement::Perch { win, .. } => self.os.toplevel_rect(win).and_then(|r| crate::facing::toward(x, r.x + r.w / 2, dead)),
                _ => None,
            });
            if let Some(r) = face {
                self.face(r, "arrive", now, false);
            }
        }
    }

    fn center_root(&self, o: (i32, i32)) -> (f32, f32) {
        let l = &self.layout;
        ((o.0 + l.avatar_x) as f32 + l.geo.width as f32 / 2.0, (o.1 + l.avatar_y) as f32 + l.geo.height as f32 * 0.55)
    }

    // ---- commands -------------------------------------------------------------------

    pub fn apply(&mut self, cmd: Command, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        // While she is carried: placing commands are refused; in the air or landing, the rag
        // gives way first. hide/quit always win.
        if self.rag.is_some() {
            let places = matches!(cmd, Command::Pos(..) | Command::Perch { .. } | Command::Unperch | Command::Drop);
            if places && self.rag_held() {
                return "error: held".into();
            }
            if places || matches!(cmd, Command::Hide | Command::Quit) {
                self.rag_cancel(now);
            }
        }
        let reply = match cmd {
            Command::Mood(m) => {
                self.set_mood(m, now);
                "ok".into()
            }
            Command::Clip(id) => {
                let steps = [crate::pack::Step::Clip(id.clone())];
                let seq = self.pack.flatten(&steps);
                if seq.is_empty() {
                    match self.pack.frame(&id) {
                        Ok(i) => {
                            self.anim.play(vec![(i, 1_500)], now);
                            "ok (frame)".into()
                        }
                        Err(_) => format!("error: no clip, action or frame `{id}`"),
                    }
                } else {
                    self.anim.play(seq, now);
                    "ok".into()
                }
            }
            Command::Say(text) => {
                self.say(text, now);
                "ok".into()
            }
            Command::Pos(x, y, instant) => {
                let o = Self::clamp(&self.os, &self.layout, (x - self.layout.ground_x, y - self.layout.ground_y));
                let (dx, dy) = (o.0 - self.origin.0, o.1 - self.origin.1);
                self.home = (o.0 + self.layout.ground_x, o.1 + self.layout.ground_y);
                if instant || !self.loaded || dx * dx + dy * dy < 48 * 48 {
                    self.teleport = None;
                    self.hidden = false;
                    self.fx_hide_bolt();
                    self.arrive(o, Placement::Ground, now);
                    state::save_ground(self.ground());
                } else {
                    self.start_teleport(o, Placement::Ground, now);
                }
                format!("ok {} {}", self.home.0, self.home.1)
            }
            Command::Perch { sit, win } => match self.perch(sit, win, now) {
                Ok(r) => format!("ok perch 0x{:x} {} {} {}x{}", r.id, r.x, r.y, r.w, r.h),
                Err(e) => format!("error: {e}"),
            },
            Command::Menu => {
                self.brain_touch(crate::brain::Touch::Menu);
                self.menu = Some((self.head_point(), now + Duration::from_secs(8)));
                "ok".into()
            }
            Command::Drop => {
                if self.teleport.is_some() {
                    "error: teleporting".into()
                } else {
                    if matches!(self.placement, Placement::Perch { .. }) {
                        let was_sitting = self.sitting();
                        self.placement = Placement::Ground;
                        self.os.set_target(0);
                        if was_sitting {
                            self.relayer(now);
                        }
                    }
                    self.drop_here(now);
                    match self.placement {
                        Placement::Perch { win, dx, .. } => format!("ok perch 0x{win:x} dx {dx}"),
                        _ => "ok ground".into(),
                    }
                }
            }
            Command::Stack => {
                let stack: Vec<_> = self.os.stack().into_iter().map(|(r, e)| serde_json::json!([format!("0x{:x}", r.id), r.x, r.y, r.w, r.h, e])).collect();
                let areas: Vec<_> = self.os.work_areas().into_iter().map(|m| serde_json::json!([m.x, m.y, m.w, m.h])).collect();
                serde_json::json!({ "stack": stack, "areas": areas }).to_string()
            }
            Command::Unperch => {
                if matches!(self.placement, Placement::Perch { .. }) {
                    self.fall("unperch", now);
                    "ok".into()
                } else {
                    "error: not perched".into()
                }
            }
            Command::Shape => {
                let h = self.shape_hist.borrow();
                serde_json::json!({ "composited": self.os.composited(), "input": self.last_shape.borrow().0, "bounding": self.last_shape.borrow().1, "before": [h[0], h[1]] }).to_string()
            }
            Command::State => {
                let reader = self.reader.as_ref().and_then(|(h, xid, _)| h.read_with(cx, |r, _| (*xid, r.scroll_info(), r.buttons())).ok());
                let line = self.line_json(cx);
                let s = self.state_json(reader);
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(mut v) => {
                        v["line"] = line;
                        v["unread"] = serde_json::json!(self.unseen.strip.as_ref().map(|u| serde_json::json!({ "n": u.n, "text": u.text, "refused": u.refused, "shown": self.unread_shown().is_some() })));
                        v["voice"] = self.voice_state_json();
                        v.to_string()
                    }
                    Err(_) => s,
                }
            }
            Command::Hide => {
                self.park(now);
                "ok hidden".into()
            }
            Command::Show => {
                if self.unpark("show", now, cx) { "ok shown".into() } else { "ok (not hidden)".into() }
            }
            Command::Quit => {
                self.quit(cx);
                "bye".into()
            }
            Command::Face(to) => {
                let right = to.unwrap_or(!self.facing_right());
                self.face(right, "ctl", now, false);
                format!("ok facing={}", if right { "right" } else { "left" })
            }
            Command::Glitch => {
                match &mut self.rag {
                    Some(r) => r.pending_glitch = Some(GlitchKind::Big),
                    None => self.glitch = Some(Glitch::new(GlitchKind::Big, now, self.rng.next())),
                }
                "ok".into()
            }
            Command::Drag(ops) => self.drag_cmd(ops, now),
            Command::Art { pool, id } => self.art_force(&pool, &id, now),
            Command::Intro { skip } => self.intro_command(skip, now, cx),
            Command::Walk(x, scoot) => self.walk_to(x, scoot, now),
            Command::Music(on, bpm) => {
                if on {
                    match self.music_on(now, None) {
                        Ok(()) => {
                            if let Some(m) = &mut self.music {
                                m.bpm = bpm.or(m.bpm);
                            }
                            self.dance_decide(now);
                            "ok".into()
                        }
                        Err(e) => format!("error: {e}"),
                    }
                } else {
                    self.music_off(now);
                    "ok".into()
                }
            }
            Command::Page(d) => self.page(d, now),
            Command::Playback(p) => self.playback(p, now),
            Command::Scroll(d) => self.scroll_bubble(d, now),
            Command::Reader(op) => self.reader_cmd(op, cx),
            Command::Collapse(c) => match &mut self.bubble {
                Some(b) => {
                    b.collapsed = c;
                    if b.next_char.is_none() {
                        b.hide_at = Some(now + Duration::from_secs(10));
                    }
                    "ok".into()
                }
                None => "error: no bubble".into(),
            },
            Command::Watch(WatchEvent::Click(x, y)) => {
                self.line_click_elsewhere(x, y, cx);
                "ok".into()
            }
            Command::Watch(WatchEvent::Escape) => {
                self.intro_skip("esc", cx);
                "ok".into()
            }
            Command::Watch(ev) => {
                self.watch(ev, now);
                "ok".into()
            }
            Command::Submit(text) => match self.send(text) {
                Ok(()) => "ok".into(),
                Err(e) => format!("error: {e}"),
            },
            Command::Talk(press) => {
                if press {
                    self.talk_press(cx)
                } else {
                    self.talk_release(cx)
                }
            }
            Command::TalkTap => self.talk_tap(cx),
            Command::TalkToggle => self.voice_toggle(cx),
            Command::TalkHandsfree(on) => {
                if on {
                    self.voice_hf_start(cx)
                } else if self.voice.hf.is_some() {
                    self.voice_hf_end("HANDS-FREE OFF", cx);
                    "ok hands-free off".into()
                } else {
                    "ok (not hands-free)".into()
                }
            }
            Command::TalkCancel => {
                if self.voice_listening() {
                    self.voice_cancel(cx);
                    "ok cancelled".into()
                } else {
                    "ok (no mic)".into()
                }
            }
            Command::LineClose => {
                if self.line.is_some() {
                    self.close_line(true, cx);
                    "ok".into()
                } else {
                    "error: no line".into()
                }
            }
            Command::Copy => self.copy_last_reply(cx),
            Command::Abort => match &self.session {
                Some(_) => {
                    self.abort();
                    "ok".into()
                }
                None => "error: no session".into(),
            },
            Command::FakePromptOff => {
                if self.prompt_ui.as_ref().is_some_and(|p| p.fake) {
                    self.answer(None);
                    "ok".into()
                } else {
                    "error: no dev prompt".into()
                }
            }
            Command::Brain(c) => self.brain_cmd(c, now),
            Command::SettingsReload => self.settings_reload(now),
            Command::Tap { head } => {
                if self.hidden || self.teleport.is_some() || !self.loaded {
                    "error: not there".into()
                } else {
                    let at = self.tap_point(head);
                    self.click_reaction(head, at, now);
                    self.brain_touch(if head { crate::brain::Touch::Headpat } else { crate::brain::Touch::Click });
                    "ok".into()
                }
            }
            Command::FakePrompt { secret, text } => {
                let view = PromptView { id: 0, title: if secret { "password required".into() } else { "Confirm tool call".into() }, text, secret };
                self.prompt_ui = Some(PromptUi { view, shown_at: now, dismissed: false, fake: true });
                self.set_mood(Mood::NeedsYou, now);
                "ok".into()
            }
        };
        if let Some(t) = self.next_activity
            && t < now + Duration::from_secs(4)
        {
            self.next_activity = Some(now + Duration::from_secs(4));
        }
        cx.notify();
        self.schedule(cx);
        reply
    }

    /// Her first words, once in her life (touchpoint 7): who she is and how to call her. Not a
    /// word about what runs her: the first minute is hers. Waits until she has landed and
    /// nothing else is on; if he found the Line first, he knows the key and she skips it.
    fn first_words_try(&mut self, now: Instant) -> bool {
        if self.line.is_some() {
            self.first_words = None;
            state::save_met();
            elog!("synaps-avatar: first words: skipped (he opened the Line first)");
            return false;
        }
        let busy = self.hidden
            || self.parked.is_some()
            || self.teleport.is_some()
            || self.glitch.is_some()
            || self.intro_playing()
            || self.arriving(now)
            || self.rag.is_some()
            || self.bubble.as_ref().is_some_and(|b| !b.collapsed)
            || self.input.is_some()
            || self.prompt_ui.is_some()
            || self.menu.is_some()
            || self.session.as_ref().is_some_and(|s| s.link.busy())
            || self.brain_moving();
        if busy {
            self.first_words = self.first_words.filter(|f| now < f.1).map(|f| (now + FIRST_WORDS_RETRY, f.1));
            if self.first_words.is_none() {
                elog!("synaps-avatar: first words: never a quiet moment; next launch");
            }
            return false;
        }
        self.first_words = None;
        state::save_met();
        let name = self.pack.name.trim();
        let mut c = name.chars();
        let name = c.next().map_or_else(|| "Capri".to_string(), |f| f.to_uppercase().chain(c).collect());
        let text = format!("Hey. I'm {name}. Tap {} when you want me. Headpats also accepted.", self.summon_label());
        elog!("synaps-avatar: first words: {text}");
        self.say(text, now);
        true
    }

    fn say(&mut self, text: String, now: Instant) {
        // The bubble shows it as a session reply would be (markdown cleaned); the reader
        // gets it as written.
        let raw = text;
        let text: Vec<char> = crate::session::clean(&raw).chars().collect();
        // Reveal in at most ~5 s however long the text is.
        let per_tick = (text.len() / (5_000 / TYPE_TICK.as_millis() as usize)).max(2);
        let long = text.len() >= TYPE_MAX_CHARS;
        self.bubble = Some(Bubble {
            // Long: all at once, finished by the next tick like a typed-out reply (the hold,
            // SPEAKING settling to idle).
            shown: if long { text.len() } else { 0 },
            text,
            per_tick,
            next_char: Some(now),
            hide_at: None,
            wrap: None,
            page: None,
            raw,
            collapsed: false,
            stream: false,
            more: false,
            width: 0.0,
        });
        if self.mood != Mood::Talking {
            self.set_mood(Mood::Talking, now);
        } else {
            self.mood_then = None;
        }
    }

    /// Scroll the bubble by `d` views (`page next|prev`).
    fn page(&mut self, d: i32, now: Instant) -> String {
        let rows = self.bubble.as_ref().map_or(1, |b| b.rows()) as i32;
        self.scroll_bubble(d * rows, now)
    }

    /// Scroll the bubble by `d` lines (the wheel).
    fn scroll_bubble(&mut self, d: i32, now: Instant) -> String {
        let Some(b) = &mut self.bubble else { return "error: no bubble".into() };
        let (cur, total) = b.pages();
        let next = (cur as i32 + d).clamp(0, total.saturating_sub(b.rows()) as i32) as usize;
        b.page = Some(next);
        b.collapsed = false;
        if b.next_char.is_none() {
            b.hide_at = Some(now + Duration::from_secs(10));
        }
        format!("ok line {}/{}", next + 1, total)
    }

    /// The UI's palette (for panels drawn elsewhere: the reader, the Line, the chip).
    fn reader_pal(&self) -> Pal {
        self.pack.palette()
    }

    /// Where the reader goes (device px): beside her and her bubble (`beside::place`: the
    /// roomier side of her monitor, whole inside her work area), clear of the cursor if the
    /// other side has room; up to 40 % of the monitor wide and 60 % tall.
    fn reader_rect(&self, raw: &str) -> crate::beside::Place {
        let l = self.layout;
        let sf = l.sf;
        let mut body = self.her_body();
        // Her and her bubble: the reader goes beside both.
        if let Some((px_, py_, pw, ph)) = self.panel_rect.get() {
            let (x0, y0) = (self.applied.0 + (px_ * sf) as i32, self.applied.1 + (py_ * sf) as i32);
            let (x1, y1) = (x0 + (pw * sf) as i32, y0 + (ph * sf) as i32);
            let (bx1, by1) = (body.right().max(x1), body.bottom().max(y1));
            body.x = body.x.min(x0);
            body.y = body.y.min(y0);
            body.w = bx1 - body.x;
            body.h = by1 - body.y;
        }
        let (bcx, bcy) = body.center();
        let m = self.os.monitor_at(bcx, bcy);
        let area = self.her_area().unwrap_or(WinRect { id: 0, x: m.x, y: m.y, w: m.w, h: m.h });
        let gap = (12.0 * sf) as i32;
        let min_w = (420.0 * sf) as i32;
        let w = ((0.40 * m.w as f32) as i32).max(min_w).min(area.w);
        let cursor = self.os.pointer().ok();
        let floor = self.placement == Placement::Ground;
        let at = |w: i32| {
            let est = (crate::reader::estimate_height(&crate::reader::parse(raw), w as f32 / sf) * sf) as i32;
            let h = est.clamp((180.0 * sf) as i32, (0.60 * m.h as f32) as i32).min(area.h);
            crate::beside::place(area, body, crate::beside::Panel { w, min_w, h, min_h: (180.0 * sf) as i32 }, gap, floor, cursor)
        };
        // Squeezed narrower beside her: measured again at that width.
        let p = at(w);
        if p.rect.w < w { at(p.rect.w) } else { p }
    }

    /// The open reader and Line follow her (a drag, a summon, a fall), once she has landed.
    fn panels_follow(&mut self, cx: &mut Context<Self>) {
        if (self.line.is_none() && self.reader.is_none()) || self.teleport.is_some() || self.rag.is_some() || self.press.is_some() || self.brain_in_flight() {
            return;
        }
        let her = self.her_body();
        if self.line.as_ref().is_some_and(|l| l.her != her) {
            self.line_follow(None, cx);
        }
        if let Some((was, _)) = self.reader_at
            && was != her
            && let Some((h, xid, _)) = &self.reader
        {
            let (h, xid) = (*h, *xid);
            let raw = self.bubble.as_ref().map(|b| if b.raw.trim().is_empty() { b.text.iter().collect::<String>() } else { b.raw.clone() }).unwrap_or_default();
            let p = self.reader_rect(&raw);
            let r = p.rect;
            if self.reader_at.is_none_or(|(_, q)| q.rect != r) {
                self.os.place(xid, r.x, r.y, r.w, r.h).ok();
            }
            self.reader_at = Some((her, p));
            let t = Some((p.side, p.tail as f32 / self.layout.sf));
            h.update(cx, |rd, _, cx| rd.set_tail(t, cx)).ok();
        }
    }

    /// Open the reader on the current reply (or bring the open one up to date).
    fn open_reader(&mut self, take_focus: bool, cx: &mut Context<Self>) -> String {
        let Some(b) = &self.bubble else { return "error: no reply".into() };
        let raw = if b.raw.trim().is_empty() { b.text.iter().collect::<String>() } else { b.raw.clone() };
        if let Some((h, _, _)) = &self.reader {
            h.update(cx, |r, _, cx| r.set_text(raw, cx)).ok();
            return "ok (open)".into();
        }
        // Tests on the live desktop never take the keyboard (SA_READER_NOFOCUS).
        let take_focus = take_focus && std::env::var_os("SA_READER_NOFOCUS").is_none();
        let sf = self.layout.sf;
        let place = self.reader_rect(&raw);
        let (x, y, w, h) = (place.rect.x, place.rect.y, place.rect.w, place.rect.h);
        let bounds = Bounds::new(point(px(x as f32 / sf), px(y as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf)));
        let options = self.os.window_options(Role::Reader, bounds, take_focus);
        let prev = if take_focus { self.os.prev_focus() } else { None };
        let me = cx.entity().downgrade();
        let (title, font, pal) = (self.name.to_uppercase(), self.font.clone(), self.reader_pal());
        let xid = Rc::new(Cell::new(0u32));
        let (xid2, os) = (xid.clone(), self.os.clone());
        let opened = cx.open_window(options, move |window, cx| {
            xid2.set(os.adopt(Role::Reader, window).unwrap_or(0));
            let on_close: crate::reader::OnClose = Box::new(move |restore, cx: &mut App| {
                me.update(cx, |a, cx| a.reader_closed(restore, cx)).ok();
            });
            cx.new(|cx| crate::reader::Reader::new(raw, title, font, pal, on_close, window, cx))
        });
        match opened {
            Ok(handle) => {
                let xid = xid.get();
                if take_focus && xid != 0 {
                    self.os.grab_keyboard(xid).ok();
                }
                self.reader = Some((handle, xid, prev));
                self.reader_at = Some((self.her_body(), place));
                let t = Some((place.side, place.tail as f32 / sf));
                handle.update(cx, |r, _, cx| r.set_tail(t, cx)).ok();
                if let Some(b) = &mut self.bubble {
                    b.hide_at = None;
                }
                elog!("synaps-avatar: reader open at {x},{y} {w}x{h} (0x{xid:x})");
                format!("ok reader {x},{y} {w}x{h}")
            }
            Err(e) => format!("error: reader: {e:#}"),
        }
    }

    /// The reader closed (Esc, ✕, a click elsewhere): `restore` hands the keyboard back.
    fn reader_closed(&mut self, restore: bool, cx: &mut Context<Self>) {
        let Some((_, xid, prev)) = self.reader.take() else { return };
        self.reader_at = None;
        if restore
            && std::env::var_os("SA_READER_NO_RESTORE").is_none()
            && let Some(w) = prev
        {
            self.os.release_keyboard(xid, Some(w)).ok();
        }
        if let Some(b) = &mut self.bubble
            && b.next_char.is_none()
            && !b.more
        {
            b.hide_at = Some(Instant::now() + Duration::from_secs(6));
        }
        cx.notify();
        self.schedule(cx);
    }

    /// Close the reader from here (deferred: it calls back into this view).
    fn close_reader(&mut self, restore: bool, cx: &mut Context<Self>) {
        if let Some((h, _, _)) = &self.reader {
            let h = *h;
            cx.defer(move |cx| {
                h.update(cx, |r, window, cx| r.close(restore, window, cx)).ok();
            });
        }
    }

    /// `reader open|close|copy [n]|scroll <px>|state`.
    fn reader_cmd(&mut self, op: crate::control::ReaderOp, cx: &mut Context<Self>) -> String {
        use crate::control::ReaderOp;
        match op {
            ReaderOp::Open { focus } => self.open_reader(focus, cx),
            ReaderOp::Close => {
                self.close_reader(false, cx);
                "ok".into()
            }
            ReaderOp::Copy(n) => match &self.reader {
                Some((h, _, _)) => h.update(cx, |r, _, cx| r.copy_block(n, cx)).unwrap_or_else(|e| format!("error: {e}")),
                None => "error: no reader".into(),
            },
            ReaderOp::Scroll(dy) => match &self.reader {
                Some((h, _, _)) => h.update(cx, |r, _, cx| r.scroll_by(dy, cx)).map_or_else(|e| format!("error: {e}"), |_| "ok".into()),
                None => "error: no reader".into(),
            },
        }
    }

    /// A click on the reply skips the typing.
    fn skip_typing(&mut self, now: Instant) -> bool {
        let Some(b) = &mut self.bubble else { return false };
        if b.shown >= b.text.len() {
            return false;
        }
        // The tick finishes it like a typed-out reply (the hold, SPEAKING settling).
        b.shown = b.text.len();
        b.next_char = Some(now);
        true
    }

    /// The end: vanish, then exit (QUIT, `quit_requested`).
    fn vanish_quit(&mut self, now: Instant) {
        self.brain_save(now, true);
        self.menu = None;
        self.bubble = None;
        self.quitting = true;
        self.glitch = Some(Glitch::new(GlitchKind::Vanish, now, self.rng.next()));
    }

    /// HIDE: she glitches out and stays out (the process keeps running); a chip at the edge
    /// of her monitor brings her back, as do a session summon and `show`.
    fn park(&mut self, now: Instant) {
        if self.parked.is_some() || self.quitting {
            return;
        }
        self.brain_save(now, true);
        self.menu = None;
        self.quit_armed = None;
        self.fold_reply();
        if self.input.is_some() {
            self.close_input(true);
        }
        self.parked = Some(false);
        state::save_hidden(true);
        elog!("synaps-avatar: hidden (HIDE); the chip brings her back");
        if self.hidden {
            // Already out of sight (a fullscreen hide): parked at once.
            self.parked = Some(true);
            self.unpark_req = None;
        } else {
            self.glitch = Some(Glitch::new(GlitchKind::Vanish, now, self.rng.next()));
        }
    }

    /// The vanish is over: hidden, and the chip up.
    fn parked_done(&mut self, cx: &mut Context<Self>) {
        self.parked = Some(true);
        self.hidden = true;
        self.fx_hide_all();
        self.open_chip(cx);
    }

    /// Back from HIDE with the appear glitch where she was. False: she wasn't hidden.
    fn unpark(&mut self, why: &str, now: Instant, cx: &mut Context<Self>) -> bool {
        if self.parked.is_none() {
            return false;
        }
        self.parked = None;
        self.unpark_req = None;
        state::save_hidden(false);
        if let Some(h) = self.chip.take() {
            h.update(cx, |_, window, _| window.remove_window()).ok();
        }
        self.chip_rect = None;
        elog!("synaps-avatar: shown again ({why})");
        let fs_hidden = self.brain.as_ref().is_some_and(|h| h.fs != brainview::FsHide::Shown);
        if !fs_hidden {
            self.hidden = false;
            self.glitch = Some(Glitch::new(GlitchKind::Appear, now, self.rng.next()));
        }
        cx.notify();
        self.schedule(cx);
        true
    }

    /// The restore chip: her name and a dot at the bottom corner of her monitor's work
    /// area (the side nearer her), on top, never taking the keyboard.
    fn open_chip(&mut self, cx: &mut Context<Self>) {
        if self.chip.is_some() {
            return;
        }
        let l = self.layout;
        let sf = l.sf;
        let (gx, gy) = self.ground();
        let m = self.os.monitor_at(gx, gy.saturating_sub(10));
        let area = self.her_area().unwrap_or(WinRect { id: 0, x: m.x, y: m.y, w: m.w, h: m.h });
        let (cw, ch) = ((crate::chip::SIZE.0 * sf) as i32, (crate::chip::SIZE.1 * sf) as i32);
        let pad = (14.0 * sf) as i32;
        let right = gx >= area.x + area.w / 2;
        let x = if right { area.right() - cw - pad } else { area.x + pad };
        let y = area.bottom() - ch - pad;
        let bounds = Bounds::new(point(px(x as f32 / sf), px(y as f32 / sf)), size(px(crate::chip::SIZE.0), px(crate::chip::SIZE.1)));
        let options = self.os.window_options(Role::Chip, bounds, false);
        let me = cx.entity().downgrade();
        let (name, font, pal) = (self.name.clone(), self.font.clone(), self.reader_pal());
        let xid = Rc::new(Cell::new(0u32));
        let (xid2, os) = (xid.clone(), self.os.clone());
        match cx.open_window(options, move |window, cx| {
            xid2.set(os.adopt(Role::Chip, window).unwrap_or(0));
            let on_click: crate::chip::OnClick = Box::new(move |cx: &mut App| {
                me.update(cx, |a, cx| {
                    a.unpark("chip", Instant::now(), cx);
                })
                .ok();
            });
            cx.new(|_| crate::chip::Chip::new(name, font, pal, on_click))
        }) {
            Ok(h) => {
                self.chip = Some(h);
                self.chip_rect = Some((x, y, cw, ch, xid.get()));
                elog!("synaps-avatar: restore chip at {x},{y} {cw}x{ch}");
            }
            Err(e) => elog!("synaps-avatar: restore chip: {e:#}"),
        }
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        // Detach + Bye first, so the daemon drops us from the roster now; the pump ends
        // when the connection thread stops (or give it 800 ms).
        if let Some(s) = &mut self.session {
            if s.closing {
                return;
            }
            s.closing = true;
            if s.client.shutdown().is_ok() {
                let (tx, rx) = async_channel::bounded::<()>(1);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(800));
                    tx.send_blocking(()).ok();
                });
                cx.spawn(async move |this, cx| {
                    rx.recv().await.ok();
                    this.update(cx, |v, cx| v.finish_quit(cx)).ok();
                })
                .detach();
                return;
            }
        }
        self.finish_quit(cx);
    }

    /// A client's connection thread stopped: the end of a quit, or (by itself) a client to
    /// make again.
    pub fn session_closed(&mut self, generation: u64, cx: &mut Context<Self>) {
        let Some(s) = &mut self.session else { return };
        if s.closing {
            self.finish_quit(cx);
        } else if s.generation == generation {
            elog!("synaps-avatar: the session client stopped; a new one in {} s", RESPAWN_SOON.as_secs());
            s.respawn_at = Some(Instant::now() + RESPAWN_SOON);
            cx.notify();
            self.schedule(cx);
        }
    }

    fn finish_quit(&mut self, cx: &mut Context<Self>) {
        if self.placement == Placement::Ground && self.teleport.is_none() {
            state::save_ground(self.ground());
        }
        self.save_seen();
        self.fx_hide_all();
        std::fs::remove_file(crate::control::socket_path()).ok();
        if self.fatal {
            std::process::exit(2);
        }
        cx.quit();
    }

    fn state_json(&self, reader: Option<crate::reader::ReaderState>) -> String {
        let sf = self.layout.sf;
        let frame = self.frame_or_default(self.shown_frame()).map(|f| f.id.clone()).unwrap_or_default();
        let (gx, gy) = self.ground();
        let m = self.os.monitor_at(gx, gy - 1);
        let (page, pages) = self.bubble.as_ref().map_or((0, 0), |b| {
            let (p, n) = b.pages();
            (p + 1, n)
        });
        let placement = match self.placement {
            Placement::Ground => serde_json::json!({ "kind": "ground" }),
            Placement::Perch { win, sit, dx } => serde_json::json!({ "kind": if sit { "sit" } else { "stand" }, "win": win, "dx": dx }),
            Placement::Float => serde_json::json!({ "kind": "float" }),
            Placement::Pose { frame } => serde_json::json!({ "kind": "pose", "frame": self.pack.frames.get(frame).map(|f| f.id.clone()) }),
        };
        let facing = serde_json::json!({ "right": self.facing_right(), "mirrored": self.flip, "turns": self.turns, "why": self.turn_why });
        let mut v = serde_json::json!({
            "mood": self.mood.key(),
            "xid": self.os.body(),
            "monitor": { "x": m.x, "y": m.y, "w": m.w, "h": m.h },
            "frame": frame,
            "rag": self.rag_json(),
            "status": self.status().label,
            "window": { "x": self.origin.0, "y": self.origin.1, "w": self.layout.win_w, "h": self.layout.win_h },
            "ground": [gx, gy],
            "avatar": { "x": self.origin.0 + self.layout.avatar_x, "y": self.origin.1 + self.layout.avatar_y,
                        "w": self.layout.geo.width, "h": self.layout.geo.height },
            "bubble": self.bubble.as_ref().map(|b| b.text.iter().collect::<String>()),
            "page": [page, pages],
            "read": self.bubble.as_ref().is_some_and(|b| b.long()),
            "hidden_by_user": self.parked.is_some(),
            // Why she isn't drawn: "user" (HIDE), "fullscreen" (hid for a fullscreen window:
            // `hidden_cover` is it), "teleport" (between the legs), "other"; null when shown.
            "hidden": self.hidden_reason(),
            "hidden_cover": self.brain.as_ref().filter(|h| h.fs != brainview::FsHide::Shown).and_then(|h| h.fs_cover).map(|w| format!("0x{w:x}")),
            "chip": self.chip_rect.map(|r| [r.0 as i64, r.1 as i64, r.2 as i64, r.3 as i64, r.4 as i64]),
            "reader": reader.map(|(xid, (scroll, max), buttons)| {
                let g = self.os.toplevel_rect(xid).map_or((0, 0, 0, 0), |r| (r.x, r.y, r.w, r.h));
                let b: serde_json::Map<String, serde_json::Value> = buttons
                    .into_iter()
                    .map(|(id, r)| (id, serde_json::json!([g.0 + (r[0] * sf) as i32, g.1 + (r[1] * sf) as i32, (r[2] * sf) as i32, (r[3] * sf) as i32])))
                    .collect();
                serde_json::json!({ "xid": xid, "rect": [g.0, g.1, g.2, g.3], "scroll": scroll, "max": max, "buttons": b })
            }),
            "collapsed": self.bubble.as_ref().is_some_and(|b| b.collapsed),
            "placement": placement,
            "teleporting": self.teleport.is_some(),
            "glitch": self.glitch.is_some(),
            "composited": self.os.composited(),
            "facing": facing,
            "compositor": self.os.compositor_running(),
            "composited_forced": self.os.forced_mode(),
            "loaded": self.loaded,
            "counters": { "ticks": self.view_ticks, "renders": self.renders, "moves": self.moves },
            "scale_factor": self.layout.sf,
            "focus_probe": self.focus_probe,
            "music": self.music.is_some(),
            "session": self.session.as_ref().map(|s| {
                let b = s.link.body(Instant::now());
                serde_json::json!({ "live": b.live, "busy": b.busy, "mood": b.mood.key(), "label": b.label, "reply": b.reply,
                                    "echo": b.echo, "prompt": b.prompt.map(|p| p.title),
                                    "mode": format!("{:?}", s.link.mode()).to_lowercase(), "client": s.generation })
            }),
            "input": self.input.as_ref().map(|i| serde_json::json!({ "len": i.text.chars().count(), "secret": i.secret.is_some(),
                                                                       "prev": i.prev, "esc": i.esc_at.is_some() })),
            "draft": self.draft.chars().count(),
            "prompt_ui": self.prompt_ui.as_ref().map(|p| serde_json::json!({ "title": p.view.title, "dismissed": p.dismissed })),
            "buttons": self.buttons.borrow().iter().copied().chain(self.tag_box.get().map(|b| ("chip", b))).map(|(id, b)| {
                let sf = self.layout.sf;
                let (x, y) = (self.origin.0 as f32 + f32::from(b.origin.x) * sf, self.origin.1 as f32 + f32::from(b.origin.y) * sf);
                (id.to_string(), serde_json::json!([x.round() as i32, y.round() as i32, (f32::from(b.size.width) * sf) as i32, (f32::from(b.size.height) * sf) as i32]))
            }).collect::<serde_json::Map<_, _>>(),
        });
        v["art"] = self.art_json();
        v["motion"] = self.motion_json();
        v["intro"] = self.intro_json();
        v.to_string()
    }


    // ---- session ---------------------------------------------------------------------

    /// Start the session client and pump its events into the view.
    pub fn connect_session(&mut self, options: ClientOptions, link: Link, key: String, cx: &mut Context<Self>) {
        let (client, rx) = match Client::spawn(options.clone()) {
            Ok(c) => c,
            Err(e) => {
                elog!("synaps-avatar: cannot start the session client: {e}");
                std::process::exit(1);
            }
        };
        self.session = Some(SessionLink { client, link, shown: None, closing: false, options, key, generation: 0, respawn_at: None });
        self.pump(rx, 0, cx);
        self.sync_body(Instant::now());
        cx.notify();
        self.schedule(cx);
    }

    /// Drain a client's events in batches whether or not the window draws (lynx's H4: a
    /// hidden overlay that only applied events in render would let the queue grow).
    fn pump(&mut self, rx: async_channel::Receiver<synaps_client::Event>, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                let mut batch = Vec::with_capacity(rx.len() + 1);
                batch.push(first);
                while batch.len() < 1024
                    && let Ok(e) = rx.try_recv()
                {
                    batch.push(e);
                }
                if this.update(cx, |v, cx| v.session_events(generation, batch, cx)).is_err() {
                    return;
                }
                YieldNow(false).await;
            }
            this.update(cx, |v, cx| v.session_closed(generation, cx)).ok();
        })
        .detach();
    }

    /// A new client in place of one that stopped or stayed offline too long.
    fn respawn(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(s) = &mut self.session else { return };
        s.respawn_at = None;
        let (client, rx) = match Client::spawn(s.options.clone()) {
            Ok(c) => c,
            Err(e) => {
                elog!("synaps-avatar: cannot start a new session client: {e}");
                s.respawn_at = Some(now + respawn_after());
                return;
            }
        };
        let old = std::mem::replace(&mut s.client, client);
        old.shutdown().ok();
        s.generation += 1;
        s.link.reconnect(now);
        let generation = s.generation;
        elog!("synaps-avatar: new session client (#{generation})");
        self.pump(rx, generation, cx);
    }

    /// A batch of daemon events (the pump drains the channel whether or not we draw).
    pub fn session_events(&mut self, generation: u64, batch: Vec<synaps_client::Event>, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(s) = &mut self.session else { return };
        if s.generation != generation {
            return; // an old client's last words
        }
        let mut acts = Vec::new();
        for ev in batch {
            acts.extend(s.link.on_event(ev, now));
        }
        self.run_acts(acts);
        self.sync_body(now);
        if let Some(t) = self.line_restore_text.take()
            && !self.line_restore(t.clone(), cx)
        {
            self.draft = t;
        }
        self.line_push(cx);
        cx.notify();
        self.schedule(cx);
    }

    fn run_acts(&mut self, acts: Vec<Act>) {
        let Some(s) = &mut self.session else { return };
        for a in acts {
            let r = match a {
                Act::Attach(id, mode) => {
                    if std::env::var_os("SA_DEBUG").is_some() {
                        elog!("synaps-avatar: attach {id} as {mode:?}");
                    }
                    s.client.attach(id, mode).map(|seq| s.link.attaching(seq))
                }
                Act::Create(o) => s.client.create(o).map(|seq| s.link.attaching(seq)),
                Act::Resume(name, o) => s.client.resume(name, o).map(|seq| s.link.attaching(seq)),
                Act::List => s.client.list_sessions(),
                Act::Resync => s.client.resync(),
                Act::Submit(t) => s.client.submit(t),
                Act::Steer(t) => s.client.steer(t),
                Act::Answer(id, v) => s.client.answer(id, v.map(|v| v.0)),
                Act::Abort => s.client.abort(),
                Act::Detach => s.client.detach(),
                Act::Set(setting) => s.client.set(setting).map(drop),
                Act::Remember(id) => {
                    state::remember_session(&s.key, &id);
                    Ok(())
                }
                Act::Fatal(msg) => {
                    elog!("synaps-avatar: {msg}");
                    self.fatal = true;
                    self.quit_requested = true;
                    Ok(())
                }
            };
            if let Err(e) = r {
                elog!("synaps-avatar: {e}");
            }
        }
    }

    /// Bring the body in line with the session: a mood only when the session's changes (so
    /// an entry clip or glitch plays once per real change), the reply into the bubble, the
    /// pending prompt into its panel.
    fn sync_body(&mut self, now: Instant) {
        let Some(s) = &mut self.session else { return };
        if s.closing {
            return;
        }
        let b = s.link.body(now);
        let once = s.link.take_once();
        let prev = s.shown.replace(b.clone());
        for o in once {
            match o {
                Once::Appear => {
                    if self.loaded {
                        self.glitch = Some(Glitch::new(GlitchKind::Appear, now, self.rng.next()));
                    }
                }
                Once::Restore(t) => {
                    elog!("synaps-avatar: input not accepted; kept as a draft");
                    if self.line.is_some() {
                        // Back into the Line's field, with the reason.
                        self.line_restore_text = Some(t);
                    } else {
                        match &mut self.input {
                            Some(i) if i.secret.is_none() && i.text.is_empty() => i.text = t,
                            _ => self.draft = t,
                        }
                        // Seen where he looks: her tag, until he acknowledges it.
                        self.unseen.refused("NOT SENT · draft kept · $mod+c".into());
                    }
                    self.hint = Some(("NOT SENT · TEXT KEPT".into(), now + Duration::from_secs(4)));
                }
            }
        }
        if self.loaded && prev.as_ref().map(|p| p.mood) != Some(b.mood) {
            self.set_mood(b.mood, now);
        }
        let (pp, pm) = (prev.as_ref().and_then(|p| p.prompt.as_ref().map(|x| x.id)), prev.as_ref().map(|p| p.mood));
        // Hidden (HIDE): a session that needs him brings her back.
        if self.parked.is_some() && b.prompt.is_some() && pp != b.prompt.as_ref().map(|x| x.id) {
            self.unpark_req = Some("summon");
        }
        if pp != b.prompt.as_ref().map(|x| x.id) || pm != Some(b.mood) {
            self.brain_session(pp, b.prompt.as_ref().map(|x| x.id), pm, b.mood);
        }
        self.label_override = b.label.clone().map(|l| (b.mood, l));
        self.echo = b.echo.clone();
        if prev.as_ref().map(|p| &p.prompt) != Some(&b.prompt) {
            match (&mut self.prompt_ui, &b.prompt) {
                (Some(u), Some(p)) if u.view.id == p.id && !u.fake => u.view = p.clone(),
                _ => {
                    if self.input.as_ref().is_some_and(|i| i.secret.is_some() && i.secret != b.prompt.as_ref().map(|p| p.id)) {
                        self.close_input(true);
                    }
                    self.prompt_ui = b.prompt.clone().map(|view| PromptUi { view, shown_at: now, dismissed: false, fake: false });
                }
            }
        }
        let new_turn = prev.as_ref().map(|p| p.turn) != Some(b.turn);
        if new_turn || prev.as_ref().is_some_and(|p| p.reply != b.reply || p.busy != b.busy) {
            self.stream_reply(&b, new_turn, now);
        }
        // A prompt that came up mid-reply stops an unprompted one.
        self.voice_guard();
    }

    /// The session's reply into the bubble: a new turn opens a new one (an old reply, on
    /// attach, goes straight behind the chip); growing text types on from what is shown.
    fn stream_reply(&mut self, b: &Body, new_turn: bool, now: Instant) {
        if new_turn {
            self.cmd_copied = None;
            if b.reply.is_empty() && !b.busy {
                self.bubble = self.bubble.take().filter(|x| !x.stream);
                return;
            }
            self.bubble = Some(Bubble {
                text: Vec::new(),
                shown: 0,
                per_tick: 2,
                next_char: None,
                hide_at: None,
                wrap: None,
                page: None,
                raw: String::new(),
                collapsed: !b.busy,
                stream: true,
                more: b.busy,
                width: 0.0,
            });
        }
        // A spoken turn: its sentences go to the sidecar; the bubble is the caption.
        if self.voice_reply(b, new_turn) {
            return;
        }
        if let Some(Some((t0, got))) = &mut self.timing
            && !*got
            && !b.reply.is_empty()
        {
            *got = true;
            elog!("timing: first reply text from the daemon {} ms after Enter", t0.elapsed().as_millis());
        }
        let annoyed = b.mood == Mood::Annoyed;
        let Some(bb) = self.bubble.as_mut().filter(|x| x.stream) else { return };
        let text: Vec<char> = b.reply.chars().collect();
        let same = bb.text.iter().zip(&text).take_while(|(a, c)| a == c).count();
        if same != bb.text.len() || text.len() != bb.text.len() {
            bb.wrap = None;
        }
        // An old reply (on attach) shows whole; a failed turn's line (it never showed as busy:
        // `session::QUIET_START`) types out like any reply.
        bb.shown = if new_turn && !b.busy && b.trouble.is_none() { text.len() } else { bb.shown.min(same) };
        bb.text = text;
        bb.raw = b.raw.clone();
        bb.reveal_all_if_long();
        bb.more = b.busy;
        if (annoyed || b.trouble.is_some()) && !b.busy {
            // The reason (aborted, error, no login) must be seen.
            bb.collapsed = false;
        }
        if bb.shown < bb.text.len() {
            bb.next_char = Some(bb.next_char.unwrap_or(now));
            bb.hide_at = None;
        } else if b.busy {
            bb.hide_at = None;
        } else if bb.hide_at.is_none() && !bb.collapsed {
            bb.hide_at = Some(now + hold_for(bb.text.len()));
        }
    }

    // ---- input -----------------------------------------------------------------------

    /// Open the text field (or a masked one for a secret prompt) and take the keyboard.
    fn open_input(&mut self, secret: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.as_ref().is_some_and(|i| i.secret != secret) {
            self.close_input(false);
        }
        if self.input.is_none() {
            let prev = self.os.prev_focus();
            let text = if secret.is_some() { String::new() } else { std::mem::take(&mut self.draft) };
            self.input = Some(InputBox { text, prev, esc_at: None, secret });
            if let Some(s) = &mut self.session {
                let acts = s.link.set_typing(true, Instant::now());
                self.run_acts(acts);
            }
        }
        if let Err(e) = self.os.grab_keyboard(self.os.body()) {
            elog!("synaps-avatar: cannot take the keyboard: {e:#}");
        }
        window.focus(&self.focus, cx);
        cx.notify();
        self.schedule(cx);
    }

    /// Close the field. Its text stays as the draft (never a secret's). `restore`: give
    /// the keyboard back to the window that had it.
    fn close_input(&mut self, restore: bool) {
        let Some(i) = self.input.take() else { return };
        if i.secret.is_none() {
            self.draft = i.text;
        }
        if let Some(s) = &mut self.session {
            let acts = s.link.set_typing(false, Instant::now());
            self.run_acts(acts);
        }
        if restore && let Some(w) = i.prev {
            self.os.release_keyboard(self.os.body(), Some(w)).ok();
        }
    }

    /// Send a message (submit, or steer while a turn runs).
    fn send(&mut self, text: String) -> Result<(), String> {
        self.brain_touch(crate::brain::Touch::Submit);
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("empty".into());
        }
        let Some(s) = &mut self.session else { return Err("no session (--no-session)".into()) };
        let acts = s.link.submit(text, Instant::now())?;
        self.run_acts(acts);
        if let Some(t) = &mut self.timing {
            *t = Some((Instant::now(), false));
        }
        Ok(())
    }

    fn abort(&mut self) {
        self.brain_aborted();
        if let Some(s) = &mut self.session {
            let acts = s.link.abort(Instant::now());
            self.run_acts(acts);
        }
    }

    fn answer(&mut self, value: Option<String>) {
        let Some(u) = self.prompt_ui.take() else { return };
        self.brain_touch(crate::brain::Touch::PromptAnswer);
        if !u.fake
            && let Some(s) = &mut self.session
        {
            let acts = s.link.answer(u.view.id, value, Instant::now());
            self.run_acts(acts);
        } else {
            // A dev prompt: back to the session's mood (or idle).
            let m = self.session.as_ref().and_then(|s| s.shown.as_ref()).map_or(Mood::Idle, |b| b.mood);
            self.set_mood(m, Instant::now());
        }
    }

    fn input_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.brain_touch(crate::brain::Touch::Typing);
        let now = Instant::now();
        let busy = self.session.as_ref().is_some_and(|s| s.link.busy());
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control || k.modifiers.platform;
        let Some(i) = &mut self.input else { return };
        if k.key != "escape" {
            i.esc_at = None;
        }
        match k.key.as_str() {
            "escape" => {
                if i.secret.is_some() {
                    // No answer: it stays pending behind the chip.
                    if let Some(u) = &mut self.prompt_ui {
                        u.dismissed = true;
                    }
                    self.close_input(true);
                } else if busy && i.esc_at.is_some() {
                    self.abort();
                    self.close_input(true);
                } else if busy {
                    i.esc_at = Some(now);
                } else {
                    self.close_input(true);
                }
            }
            "enter" if !k.modifiers.shift => {
                let text = std::mem::take(&mut i.text);
                if let Some(id) = i.secret {
                    if !text.is_empty() && self.prompt_ui.as_ref().is_some_and(|u| u.view.id == id) {
                        self.answer(Some(text));
                    }
                    self.close_input(true);
                } else if text.trim().is_empty() {
                    self.close_input(true);
                } else {
                    match self.send(text.clone()) {
                        Ok(()) => self.close_input(true),
                        Err(e) => {
                            if let Some(i) = &mut self.input {
                                i.text = text;
                            }
                            self.hint = Some((format!("NOT SENT · {}", e.to_uppercase()), now + Duration::from_secs(4)));
                        }
                    }
                }
            }
            "backspace" if ctrl => {
                let t = i.text.trim_end().len();
                let cut = i.text[..t].rfind(char::is_whitespace).map_or(0, |p| p + 1);
                i.text.truncate(cut);
            }
            "backspace" => {
                i.text.pop();
            }
            "u" if ctrl => i.text.clear(),
            "v" if ctrl => {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    let room = INPUT_MAX.saturating_sub(i.text.chars().count());
                    i.text.extend(t.chars().map(|c| if c.is_control() { ' ' } else { c }).take(room));
                }
            }
            _ => {
                if let Some(c) = &k.key_char
                    && !ctrl
                    && !c.chars().any(char::is_control)
                    && i.text.chars().count() < INPUT_MAX
                {
                    i.text.push_str(c);
                }
            }
        }
        let _ = window;
        cx.notify();
        self.schedule(cx);
    }

    // ---- placement -------------------------------------------------------------------

    pub fn ground(&self) -> (i32, i32) {
        (self.origin.0 + self.layout.ground_x, self.origin.1 + self.layout.ground_y)
    }

    /// Keep the avatar (not the bubble area) in the work area under it (the monitor minus
    /// docks); feet no lower than its bottom edge.
    pub fn clamp(os: &Os, l: &Layout, (x, y): (i32, i32)) -> (i32, i32) {
        let (aw, ah) = (l.geo.width as i32, l.geo.height as i32);
        let (cx, cy) = (x + l.avatar_x + aw / 2, y + l.avatar_y + ah / 2);
        let m = os.area_at(cx, cy);
        let ax = (x + l.avatar_x).clamp(m.x, (m.x + m.w - aw).max(m.x));
        let gy = (y + l.ground_y).min(m.y + m.h - l.foot_pad - GROUND_ROOM);
        let top = gy - l.ground_y + l.avatar_y;
        let gy = if top < m.y { gy + (m.y - top) } else { gy };
        (ax - l.avatar_x, gy - l.ground_y)
    }

    pub fn default_origin(os: &Os, l: &Layout) -> (i32, i32) {
        let m = os.primary_area();
        let x = m.x + m.w - (l.avatar_x + l.geo.width as i32) - (24.0 * l.sf) as i32;
        let y = m.y + m.h - l.foot_pad - GROUND_ROOM - l.ground_y;
        (x, y)
    }

    /// Which part of the window is on the avatar's monitor; the bubble stays inside it.
    fn visible_range(os: &Os, l: &Layout, o: (i32, i32)) -> (i32, i32) {
        let m = os.monitor_at(o.0 + l.avatar_x + l.geo.width as i32 / 2, o.1 + l.avatar_y + l.geo.height as i32 / 2);
        ((m.x - o.0).max(0), (m.x + m.w - o.0).min(l.win_w))
    }

    /// Move the window (origin without offsets) and keep the bob/hop offset on top.
    fn set_origin(&mut self, o: (i32, i32)) {
        self.origin = o;
        self.vis = Self::visible_range(&self.os, &self.layout, o);
        self.apply_offset(Instant::now());
    }

    fn move_to(&mut self, o: (i32, i32), persist: bool) {
        let o = Self::clamp(&self.os, &self.layout, o);
        self.set_origin(o);
        if persist {
            state::save_ground(self.ground());
        }
    }

    /// What's left of a perch glide (`Os::perch_glide`): on the compositor's move curve, so she
    /// rides the window (an overshoot included).
    fn glide_offset(&self, now: Instant) -> (i32, i32) {
        let Some((t0, (dx, dy), a)) = self.glide.filter(|_| matches!(self.placement, Placement::Perch { .. })) else { return (0, 0) };
        let k = 1.0 - a.progress(now.saturating_duration_since(t0).as_secs_f32() * 1000.0 / a.ms);
        ((dx as f32 * k).round() as i32, (dy as f32 * k).round() as i32)
    }

    fn apply_offset(&mut self, now: Instant) {
        let hop = self.hop.and_then(|t| fx::hop(now.saturating_duration_since(t), 16.0 * self.layout.sf)).unwrap_or(0);
        if self.glide.is_some_and(|(t0, _, a)| now.saturating_duration_since(t0).as_secs_f32() * 1000.0 >= a.ms) {
            self.glide = None;
        }
        let g = self.glide_offset(now);
        let pos = (self.origin.0 + g.0, self.origin.1 + g.1 - hop + self.drop_in_offset(now) + self.float_bob(now));
        if pos != self.applied {
            self.moves += 1;
            self.os.move_to(pos.0, pos.1).ok();
            self.applied = pos;
            if ragview::atomic_log() {
                elog!("atomic move {:.6} {},{}", ragview::mono_s(), pos.0, pos.1);
            }
        }
    }

    fn start_teleport(&mut self, to: (i32, i32), place: Placement, now: Instant) {
        // She looks the way she is about to go.
        if self.mirror_cfg.own && self.loaded {
            let dead = (crate::facing::DEAD_PX * self.layout.sf) as i32;
            if let Some(r) = crate::facing::toward(self.origin.0, to.0, dead) {
                self.face(r, "travel", now, true);
            }
        }
        self.os.set_target(0);
        self.menu = None;
        self.hop = None;
        self.fold_reply();
        let seed = self.rng.next();
        self.glitch = Some(Glitch::new(GlitchKind::Vanish, now, seed));
        self.arrive_reset();
        // Leaving the art on screen with its own drawn leave: that, then the bolt (no glitch).
        let leave = self.drawn_leave();
        let leave_until = leave.as_ref().map(|s| now + Duration::from_millis(s.iter().map(|x| x.1 as u64).sum()));
        if let Some(seq) = leave {
            self.glitch = None;
            self.anim.play(seq, now);
        }
        self.teleport = Some(Teleport { seed, leg: Leg::Vanish, to, place, bolt: None, moved: false, no_bolt: false, quiet: false, leave_until });
    }

    /// The drawn leave for the art on screen: lachy's perch → `perchLeave`; a lachy stand →
    /// `floorLeave` (from its first frame on the legacy drawing it starts from, else cut into
    /// its tear). None: the glitch's vanish.
    fn drawn_leave(&self) -> Option<Seq> {
        if self.sitting() && self.art_strip_is(PoolKey::Perch) {
            return self.art_transition("perchLeave");
        }
        if self.placement != Placement::Ground || !self.standing() || !self.art_strip_is(PoolKey::Stand) {
            return None;
        }
        let seq = self.art_transition("floorLeave")?;
        let same = self.pack.art_of(self.shown_frame()) == self.pack.art_of(seq[0].0);
        Some(if same { seq } else { seq.into_iter().skip(LEAVE_CUT_IN).collect() })
    }

    /// A teleport, quiet (passive moves) or not. A quiet one never has a drawn leave.
    fn start_teleport_q(&mut self, to: (i32, i32), place: Placement, now: Instant, quiet: bool) {
        self.start_teleport(to, place, now);
        if quiet && let Some(tp) = &mut self.teleport {
            tp.quiet = true;
            if tp.leave_until.take().is_some() {
                self.anim.stop_oneshot(now);
            }
            self.glitch = Some(Glitch::soft(GlitchKind::Vanish, now, tp.seed));
        }
    }

    /// The landing glitch of the teleport in flight.
    fn land_glitch(&self, now: Instant, seed: u64) -> Glitch {
        if self.teleport.as_ref().is_some_and(|t| t.quiet) {
            Glitch::soft(GlitchKind::Land, now, seed ^ 0x1a4d)
        } else {
            Glitch::new(GlitchKind::Land, now, seed ^ 0x1a4d)
        }
    }

    /// The HUD comes back a beat after a loud teleport has fully reformed her, and never
    /// before a drawn arrival has formed her (`arrive.formed`).
    fn teleport_done(&mut self, now: Instant) {
        let quiet = self.teleport.as_ref().is_some_and(|t| t.quiet);
        let formed = self.arrive.formed.take().filter(|&t| t > now);
        self.teleport = None;
        self.arrive_reset();
        self.life_after_landing(now);
        if !quiet {
            self.hud_after = Some((now + HUD_AFTER_TELEPORT).max(formed.unwrap_or(now)));
        } else if formed.is_some() {
            self.hud_after = formed;
        }
    }

    /// Landed: her idle life starts at once, not after a still hold: a sparkle, a breath step
    /// and a blink within half a second.
    fn life_after_landing(&mut self, now: Instant) {
        self.next_glint = Some(now + LIFE_SPARKLE);
        let period = self.pack.behaviour.life.bob_period_ms;
        if period > 0 {
            // The breath's first 1 px step (a sixth of its period in) lands LIFE_BREATH from now.
            let back = Duration::from_millis(period / 6).saturating_sub(LIFE_BREATH);
            self.bob_t0 = now.checked_sub(back).unwrap_or(now);
        }
        self.anim.rest_soon(LIFE_BLINK_MS, now);
    }

    /// Window origin that puts the seat (or the feet) on `r`'s top edge at `r.x + dx`.
    fn perch_origin(&self, r: &WinRect, sit: bool, dx: i32) -> (i32, i32) {
        let l = &self.layout;
        let x = r.x + dx;
        match (sit, l.seat_y) {
            (true, Some(sy)) => (x - l.ground_x, r.y - sy),
            _ => (x - l.ground_x, r.y - l.ground_y),
        }
    }

    /// What touches the edge line in the perch frame (seat, thighs, planted hands) and
    /// the legs hanging below it: x extent relative to the seat (or feet), device px.
    fn measure_contact(&self, sit: bool) -> Option<(i32, i32)> {
        let id = if sit { self.perch_spec()?.base.clone() } else { self.pack.catalog.default_sprite.clone() };
        self.contact_of(*self.pack.index.get(&id)?, sit)
    }

    /// The feet's (or seat's) span about the ground x, as she faces now.
    fn contact_at(&self, sit: bool) -> Option<(i32, i32)> {
        self.contact[sit as usize].map(|(lo, hi)| if self.flip { (-hi, -lo) } else { (lo, hi) })
    }

    fn contact_of(&self, fi: usize, sit: bool) -> Option<(i32, i32)> {
        let l = &self.layout;
        let f = self.frames.get(fi)?.as_ref()?;
        let line = if sit { l.seat_y? } else { l.ground_y } - l.avatar_y;
        let cx = l.ground_x - l.avatar_x;
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        let from = if sit { line - (8.0 * l.sf) as i32 } else { line - (30.0 * l.sf) as i32 };
        for y in from..f.mask.h as i32 {
            for x in 0..f.mask.w as i32 {
                if f.mask.get(x, y) {
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        (lo <= hi).then_some((lo - cx, hi - cx))
    }

    /// Where the seat/feet may go along `r`'s top edge so that everything touching it is
    /// on the window, EDGE_MARGIN from both corners. None: the window is too narrow.
    fn perch_range(&self, r: &WinRect, sit: bool) -> Option<(i32, i32)> {
        let m = (EDGE_MARGIN * self.layout.sf).round() as i32;
        let (l, h) = self.contact_at(sit).unwrap_or((-m, m));
        let (lo, hi) = (m - l, r.w - m - h);
        (lo <= hi).then_some((lo, hi))
    }

    /// The seat/feet x on `r` (relative to its left edge): `want`, or PERCH_AT across by
    /// default, clamped into the safe range.
    fn perch_dx(&self, r: &WinRect, sit: bool, want: Option<i32>) -> Option<i32> {
        let (lo, hi) = self.perch_range(r, sit)?;
        Some(want.unwrap_or((r.w as f32 * PERCH_AT) as i32).clamp(lo, hi))
    }

    /// Whether a perch on `r` keeps the figure in its work area (under the docks) and
    /// `r` is not fullscreen.
    fn perch_fits(&self, r: &WinRect, o: (i32, i32), sit: bool) -> bool {
        let m = self.os.monitor_at(r.x + r.w / 2, r.y + 1);
        if r.x == m.x && r.y == m.y && r.w == m.w && r.h == m.h {
            return false; // fullscreen
        }
        let a = self.os.area_at(r.x + r.w / 2, r.y + 1);
        let base = if sit { self.perch_spec().map(|p| p.base.clone()) } else { None }.unwrap_or_else(|| self.pack.catalog.default_sprite.clone());
        let top = self.pack.index.get(&base).and_then(|&i| self.frames[i].as_ref()).map_or(0, |f| f.top);
        o.1 + self.layout.avatar_y + top >= a.y && r.y > a.y
    }

    fn edge_margin(&self) -> i32 {
        (EDGE_MARGIN * self.layout.sf).round() as i32
    }

    /// Contact extent for `sit` (relative to the seat or feet), or a margin-wide guess.
    fn contact_or_default(&self, sit: bool) -> (i32, i32) {
        let m = self.edge_margin();
        self.contact_at(sit).unwrap_or((-m, m))
    }

    /// The exposed parts of `r`'s top edge and the seat (or feet) x ranges on them, root px.
    /// `r` may be newer than the stack's copy of it (a window that just moved).
    fn exposed_seats(&self, r: &WinRect, sit: bool) -> (Ranges, Ranges) {
        let (mut stack, _): (Vec<WinRect>, Vec<bool>) = self.os.stack().into_iter().unzip();
        let i = match stack.iter().position(|w| w.id == r.id) {
            Some(i) => {
                stack[i] = *r;
                i
            }
            None => {
                stack.push(*r);
                stack.len() - 1
            }
        };
        (edges::exposed_spans(&stack, i), edges::seat_ranges(&stack, i, self.contact_or_default(sit), self.edge_margin()))
    }

    fn perch(&mut self, sit: bool, win: Option<u32>, now: Instant) -> anyhow::Result<WinRect> {
        let sit = sit && self.layout.seat_y.is_some();
        let id = match win {
            Some(w) => w,
            None => self.os.active_window().ok_or_else(|| anyhow::anyhow!("no focused window (_NET_ACTIVE_WINDOW)"))?,
        };
        let top = self.os.toplevel_of(id).ok_or_else(|| anyhow::anyhow!("window 0x{id:x} not found"))?;
        if top == self.os.body() {
            anyhow::bail!("that's me");
        }
        let r = self.os.toplevel_rect(top).ok_or_else(|| anyhow::anyhow!("window 0x{top:x} is not visible"))?;
        self.perch_dx(&r, sit, None).ok_or_else(|| anyhow::anyhow!("window 0x{top:x} is too narrow to sit on"))?;
        // PERCH_AT across, moved to the nearest seat on an exposed stretch of the edge.
        let want = r.x + (r.w as f32 * PERCH_AT) as i32;
        let (spans, seats) = self.exposed_seats(&r, sit);
        let x = nearest_in(&seats, want).ok_or_else(|| {
            anyhow::anyhow!("edge hidden: window 0x{top:x} has no exposed stretch wide enough (exposed {spans:?}, contact {:?})", self.contact_or_default(sit))
        })?;
        let dx = x - r.x;
        let o = self.perch_origin(&r, sit, dx);
        if !self.perch_fits(&r, o, sit) {
            anyhow::bail!("no room above window 0x{top:x} (or it is fullscreen)");
        }
        if self.placement == Placement::Ground {
            self.home = self.ground();
        }
        self.start_teleport(o, Placement::Perch { win: top, sit, dx }, now);
        Ok(r)
    }

    /// The perch window is gone, fullscreen or out of room: teleport back to the ground.
    /// Her perch went away or has no room left: she falls where she is, straight down on
    /// the same monitor (the ragdoll's drop, a surprised face), onto an exposed edge below or
    /// that monitor's floor. Never a teleport to `home` (it may be on another monitor).
    fn fall(&mut self, why: &'static str, now: Instant) {
        self.perch_gone = None;
        self.os.set_target(0);
        if self.teleport.is_some() || self.rag.is_some() {
            return;
        }
        elog!("synaps-avatar: falls in place (why={why}) at {} ms", self.t_start.elapsed().as_millis());
        if let Some(h) = &mut self.brain {
            let t = h.bnow(now);
            h.brain.note_external(t, why);
            h.write(serde_json::json!({ "t": t, "k": "fall", "why": why, "from": [self.origin.0, self.origin.1] }));
        }
        self.brain_cancel_move();
        if self.rag_drop(now) {
            return;
        }
        // Stiff: straight down to this monitor's floor.
        if let Some(h) = &mut self.brain {
            h.rest = None;
        }
        self.placement = Placement::Ground;
        let o = Self::clamp(&self.os, &self.layout, (self.origin.0, self.origin.1 + 100_000));
        self.move_to(o, true);
        self.home = self.ground();
        state::save_ground(self.ground());
        self.relayer(now);
    }

    /// The perch window's grace ran out (or it came back): fall, or stay.
    fn check_perch_gone(&mut self, now: Instant) -> bool {
        let Some(g) = self.perch_gone else { return false };
        let back = matches!(g.verdict(now, false), edges::GoneVerdict::Fall) && self.os.toplevel_rect(g.win).is_some();
        match g.verdict(now, back) {
            edges::GoneVerdict::Wait => false,
            edges::GoneVerdict::Stay => {
                self.perch_gone = None;
                elog!("synaps-avatar: perch 0x{:x} came back: stays", g.win);
                false
            }
            edges::GoneVerdict::Fall => {
                let still = matches!(self.placement, Placement::Perch { win, .. } if win == g.win);
                self.perch_gone = None;
                if still {
                    // Closed: another window's top (the brain decides, 55 %) or the fall;
                    // gone with the workspace he left: she follows him.
                    self.brain_perch_gone(g.win, now);
                }
                still
            }
        }
    }

    /// Alpha <-> shape mode changed: send both shapes again on the next paint (the bounding
    /// one is the mode: none in alpha mode, the figure's rects in shape mode), and show each
    /// effect layer where the mode puts it.
    pub(super) fn alpha_switched(&mut self) {
        *self.last_shape.borrow_mut() = (vec![(0, 0, 0, 0)], vec![(0, 0, 0, 0)]);
        self.os.fx_reroute();
    }

    fn watch(&mut self, ev: WatchEvent, now: Instant) {
        match ev {
            WatchEvent::Compositor(c) => {
                elog!(
                    "synaps-avatar: compositor {}: {} mode{}",
                    if c { "appeared" } else { "went away" },
                    if self.os.composited() { "alpha" } else { "shape" },
                    if self.os.forced_mode().is_some() { " (forced)" } else { "" }
                );
                self.alpha_switched();
            }
            WatchEvent::TargetMoved(r) => {
                let Placement::Perch { win, sit, dx } = self.placement else { return };
                if win != r.id || self.teleport.is_some() {
                    return;
                }
                self.brain_perch_moved(&r, now);
                if self.placement != (Placement::Perch { win, sit, dx }) {
                    // Startled off it.
                    return;
                }
                if self.perch_dx(&r, sit, Some(dx)).is_none() {
                    self.fall("perch_no_room", now);
                    return;
                }
                // Stay on an exposed stretch: clamp into it, but never slide across a
                // window that now covers her seat (more than a nudge: she falls).
                let x = r.x + dx;
                let (spans, ranges) = self.exposed_seats(&r, sit);
                let Some(nx) = nearest_in(&ranges, x) else {
                    self.fall("perch_covered", now);
                    return;
                };
                let hidden = x >= r.x && x < r.right() && !spans.iter().any(|&(a, b)| a <= x && x < b);
                if hidden && (nx - x).abs() > (LAND_SLIDE * self.layout.sf) as i32 {
                    self.fall("perch_covered", now);
                    return;
                }
                let dx = nx - r.x;
                let o = self.perch_origin(&r, sit, dx);
                if self.perch_fits(&r, o, sit) {
                    self.placement = Placement::Perch { win, sit, dx };
                    // A jump (not a stream of moves: a live drag) to a goal the compositor
                    // animates the window to: she rides along from where she shows now.
                    let shown = self.glide_offset(now);
                    let jump = (self.origin.0 + shown.0 - o.0, self.origin.1 + shown.1 - o.1);
                    let far = (jump.0 * jump.0 + jump.1 * jump.1) as f32 > (GLIDE_MIN * self.layout.sf).powi(2);
                    let stream = self.perch_moved.is_some_and(|t| now.duration_since(t) < GLIDE_STREAM);
                    self.perch_moved = Some(now);
                    // On the curve from when the move began (the poller's stamp, not when this
                    // runs: found late, or handled a frame later, she rides it from where the
                    // window is by now).
                    let since = self.os.target_moved_at().filter(|t| *t <= now && now.duration_since(*t) < Duration::from_secs(1)).unwrap_or(now);
                    self.glide = self.os.perch_glide().filter(|_| far && !stream).map(|a| (since, jump, a));
                    self.set_origin(o);
                } else {
                    self.fall("perch_no_room", now);
                }
            }
            WatchEvent::TargetGone(id) => {
                // Not at once: a workspace flicked back within the grace keeps her put.
                if matches!(self.placement, Placement::Perch { win, .. } if win == id) && self.perch_gone.is_none() {
                    elog!("synaps-avatar: perch 0x{id:x} unmapped at {} ms: falls in 300 ms unless it comes back", self.t_start.elapsed().as_millis());
                    self.perch_gone = Some(edges::PerchGone { win: id, since: now });
                }
            }
            WatchEvent::TargetBack(id) => {
                if self.perch_gone.is_some_and(|g| g.win == id) {
                    self.perch_gone = None;
                }
            }
            WatchEvent::Click(..) | WatchEvent::Escape => {}
            WatchEvent::Layout => {
                self.os.forget_layout();
                self.hosts_stale = true;
                if self.rag.is_some() || self.teleport.is_some() {
                    return;
                }
                let l = &self.layout;
                let c = (self.origin.0 + l.avatar_x + l.geo.width as i32 / 2, self.origin.1 + l.avatar_y + l.geo.height as i32 / 2);
                if !self.os.monitors().iter().any(|m| m.contains(c.0, c.1)) {
                    // Her monitor went away: onto the floor of the nearest one, quietly.
                    let o = Self::clamp(&self.os, &self.layout, (self.origin.0, self.origin.1 + 100_000));
                    elog!("synaps-avatar: her monitor went away: to {},{}", o.0, o.1);
                    self.start_teleport_q(o, Placement::Ground, now, true);
                } else if self.placement == Placement::Ground && Self::clamp(&self.os, &self.layout, self.origin) != self.origin {
                    // A panel took her floor.
                    self.fall("layout", now);
                }
            }
            WatchEvent::Focus | WatchEvent::Desktop | WatchEvent::Pointer(_) | WatchEvent::Session(_) => self.brain_watch(ev, now),
        }
    }

    /// After a drag: an exposed stretch of a window's top edge near the seat or the feet
    /// (`edges::choose_snap`: a stretch hidden by a window above is never a seat).
    fn snap_target(&self) -> Option<(WinRect, i32)> {
        let l = &self.layout;
        let (gx, gy) = self.ground();
        let sy = l.seat_y.map_or(gy, |s| self.origin.1 + s);
        let tol = (36.0 * l.sf) as i32;
        let (stack, eligible): (Vec<WinRect>, Vec<bool>) = self.os.stack().into_iter().unzip();
        let fits = |i: usize, x: i32| {
            let r = &stack[i];
            self.perch_fits(r, self.perch_origin(r, true, x - r.x), true)
        };
        let (i, x) = edges::choose_snap(&stack, &eligible, (gx, sy), (gx, gy), tol, self.contact_or_default(true), self.edge_margin(), &fits)?;
        Some((stack[i], x - stack[i].x))
    }

    /// A drag ended where she is now: sit on an exposed edge near the seat or the feet,
    /// else stay there (the new home).
    fn drop_here(&mut self, now: Instant) {
        if let Some((r, dx)) = self.snap_target() {
            self.home = self.ground();
            let o = self.perch_origin(&r, true, dx);
            self.placement = Placement::Perch { win: r.id, sit: self.layout.seat_y.is_some(), dx };
            self.set_origin(o);
            self.os.set_target(r.id);
            self.relayer(now);
            self.glitch = Glitch::for_cut(Transition::Perch, now, self.rng.next());
        } else {
            self.home = self.ground();
            state::save_ground(self.ground());
        }
    }

    // ---- pointer --------------------------------------------------------------------

    fn local(&self, p: Point<Pixels>) -> (i32, i32) {
        let sf = self.layout.sf;
        ((f32::from(p.x) * sf) as i32 - self.layout.avatar_x, (f32::from(p.y) * sf) as i32 - self.layout.avatar_y)
    }

    fn hit(&self, p: Point<Pixels>) -> Option<bool> {
        // A teleport can be grabbed in its vanish and landing legs (not as the bolt).
        if self.hidden || self.quitting || self.teleport.as_ref().is_some_and(|t| matches!(t.leg, Leg::Bolt(_))) {
            return None;
        }
        let f = self.frame_or_default(self.shown_frame())?;
        let (x, y) = self.local(p);
        f.mask.get(x, y).then_some(y < f.head_line)
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
            self.schedule(cx);
        }
        let Some(head) = self.hit(ev.position) else { return };
        // A press alone never stops her talking: grabbing, dragging, flinging and catching her
        // mid-flight leave her speech alone. Only a plain tap on her body stops her (mouse_up).
        // Tests drive her with synthetic taps only: a real press in a recording means a person
        // touched her (the clip is thrown away).
        elog!("synaps-avatar: real pointer press ({:?}, {})", ev.button, if head { "head" } else { "body" });
        // A real press beats a scripted drag: the script is dropped, the press goes on.
        if self.rag.as_ref().is_some_and(|r| matches!(r.feed, ragview::Feed::Script { .. })) {
            self.rag_cancel(Instant::now());
        }
        match ev.button {
            MouseButton::Left => {
                let root = self.os.pointer().unwrap_or((0, 0));
                // Caught mid-flight: held again at once.
                let now = Instant::now();
                let caught = self.rag_catch(self.local(ev.position), now);
                self.press = Some(Press { root, origin: self.origin, head, dragging: caught });
                if head {
                    self.dangle_want();
                }
                if !caught && self.rag.is_none() && self.teleport.is_none() {
                    // Felt at once: she gives under the press (it went unanswered until the
                    // release or 5 px of drag).
                    self.squash = Some(now);
                }
                cx.notify();
                self.schedule(cx);
            }
            MouseButton::Right => {
                self.brain_touch(crate::brain::Touch::Menu);
                self.menu = Some((ev.position, Instant::now() + Duration::from_secs(8)));
                cx.notify();
                self.schedule(cx);
            }
            _ => {}
        }
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if ev.pressed_button.is_none() {
            self.hover_pointer(ev.position);
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if ragview::follow_log() {
            elog!("motion {:.6} pressed {:?} press {}", ragview::mono_s(), ev.pressed_button, self.press.is_some());
        }
        let Some(p) = &mut self.press else { return };
        if p.dragging {
            // Held: the window follows the pointer in the next frame (`rag_follow`, in
            // render): any number of motion events, one move a frame.
            cx.notify();
            return;
        }
        // Before the drag starts the window hasn't moved since the press: the event's
        // position is exact.
        let sf = self.layout.sf;
        let root = (self.applied.0 + (f32::from(ev.position.x) * sf) as i32, self.applied.1 + (f32::from(ev.position.y) * sf) as i32);
        let (dx, dy) = (root.0 - p.root.0, root.1 - p.root.1);
        if dx * dx + dy * dy < 25 {
            return;
        }
        p.dragging = true;
        // The press point on her, avatar-local (where the pointer went down).
        let local = (p.root.0 - p.origin.0 - self.layout.avatar_x, p.root.1 - p.origin.1 - self.layout.avatar_y);
        self.squash = None;
        self.rag_grab(local, ragview::Feed::Real, Instant::now());
        cx.notify();
        self.schedule(cx);
    }

    fn mouse_up(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        let Some(p) = self.press.take() else { return };
        let now = Instant::now();
        if p.dragging {
            if self.rag.as_ref().is_some_and(|r| matches!(r.feed, ragview::Feed::Real)) {
                self.rag_release(now);
            }
            cx.notify();
            self.schedule(cx);
            return;
        }
        // A plain tap on her body (no drag) stops her talking: the explicit "hush" a click has
        // always been. A headpat doesn't: petting her while she talks isn't asking her to stop.
        if !p.head && self.voice_speaking() {
            self.voice_hush("click");
        }
        self.click_reaction(p.head, ev.position, now);
        // A tap on the body (not a headpat) opens the field to talk to the session.
        if !p.head && self.session.is_some() {
            self.open_input(None, window, cx);
        }
        self.brain_touch(if p.head { crate::brain::Touch::Headpat } else { crate::brain::Touch::Click });
        cx.notify();
        self.schedule(cx);
    }

    /// Where a synthetic tap lands (window logical px): the head, or the middle of the body.
    fn tap_point(&self, head: bool) -> Point<Pixels> {
        let sf = self.layout.sf;
        let (w, h, hl) = self.frame_or_default(self.anim.frame()).map_or((self.layout.geo.width as i32, self.layout.geo.height as i32, 100), |f| {
            (f.mask.w as i32, f.mask.h as i32, f.head_line)
        });
        let y = if head { hl / 2 } else { (hl + h) / 2 };
        point(px((self.layout.avatar_x + w / 2) as f32 / sf), px((self.layout.avatar_y + y) as f32 / sf))
    }

    /// A click on her (real or `tap`): the reaction clip, hearts on a headpat, the squash.
    fn click_reaction(&mut self, head: bool, at: Point<Pixels>, now: Instant) {
        self.burst_start(at, now);
        self.streak = if now.duration_since(self.streak.1) < Duration::from_millis(self.pack.behaviour.click.streak_ms) {
            (self.streak.0 + 1, now)
        } else {
            (1, now)
        };
        let click = self.pack.behaviour.click.clone();
        let perch_click = self.perch_spec().filter(|_| self.sitting()).map(|p| if head && !p.headpat.is_empty() { p.headpat.clone() } else { p.click.clone() });
        // On a strip variant: one of its own blinks (the pack's reactions are another drawing).
        let art_click = if self.asleep() { None } else { self.art_click() };
        let steps = if art_click.is_some() {
            if head {
                self.hearts = Some((now, at));
            }
            Vec::new()
        } else if (!self.sitting() && !self.standing()) || self.asleep() {
            // Hanging, sitting on the floor or asleep (a pat doesn't wake her): no standing
            // clip, hearts on the head.
            if head {
                self.hearts = Some((now, at));
            }
            Vec::new()
        } else if let Some(steps) = perch_click {
            if head {
                self.hearts = Some((now, at));
            }
            steps
        } else if self.streak.0 >= click.annoyed_after && !click.annoyed.is_empty() {
            self.streak.0 = 0;
            click.annoyed
        } else if head && !click.head.is_empty() {
            self.hearts = Some((now, at));
            // Rarely the headpat ends in an air kiss.
            let ms: u64 = self.pack.flatten(&click.head).iter().map(|s| s.1 as u64).sum();
            let mut steps = click.head;
            if let Some(k) = self.maybe_kiss(ms, now) {
                steps.extend(k);
            }
            steps
        } else if !click.body.is_empty() {
            let i = self.rng.pick(click.body.len());
            if matches!(click.body[i].as_slice(), [crate::pack::Step::Clip(c)] if c == "laugh") {
                self.cutin_laugh(now);
            }
            click.body[i].clone()
        } else {
            Vec::new()
        };
        let seq = art_click.unwrap_or_else(|| self.pack.flatten(&steps));
        let len: u64 = seq.iter().map(|s| s.1 as u64).sum();
        if !seq.is_empty() {
            self.anim.play(seq, now);
        }
        // A press already started it (mouse_down): a click keeps that one going.
        if self.squash.is_none_or(|t| now.saturating_duration_since(t) > Duration::from_millis(400)) {
            self.squash = Some(now);
        }
        let hold = if self.hearts.is_some() { len.max(HEARTS_MS) } else { len.max(600) };
        self.reaction_until = Some(now + Duration::from_millis(hold));
        if self.next_activity.is_some() {
            self.next_activity = Some(now + self.autonomy_gap());
        }
    }

    /// The status tag: reopen what is behind the chip (a prompt put off, the last reply),
    /// else open the field.
    fn chip_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        // An unread reply (or a refused draft): the plate opens the Line, at that reply.
        if self.unread_shown().is_some() && self.session.is_some() {
            self.brain_touch(crate::brain::Touch::Chip);
            let r = self.open_line(cx);
            elog!("synaps-avatar: unread plate clicked: {r}");
            return;
        }
        self.ack_unread();
        self.brain_touch(crate::brain::Touch::Chip);
        if let Some(p) = self.prompt_ui.as_mut().filter(|p| p.dismissed) {
            p.dismissed = false;
            p.shown_at = now;
        } else if let Some(b) = self.bubble.as_mut().filter(|b| b.collapsed) {
            b.collapsed = false;
            if b.next_char.is_none() && !b.more {
                b.hide_at = Some(now + Duration::from_secs(10));
            }
        } else if self.session.is_some() {
            self.open_input(None, window, cx);
        }
        cx.notify();
        self.schedule(cx);
    }

    fn reset_position(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        let now = Instant::now();
        let o = Self::default_origin(&self.os, &self.layout);
        self.home = (o.0 + self.layout.ground_x, o.1 + self.layout.ground_y);
        if self.loaded {
            self.start_teleport(o, Placement::Ground, now);
        } else {
            self.move_to(o, true);
        }
        cx.notify();
        self.schedule(cx);
    }

    /// Squash pivot: the seat when sitting on an edge, else the feet (avatar-local).
    /// A quiet arrival: her width from a sliver back to full over `SLIVER` (the exit, reversed).
    fn sliver_sx(&self, at: Instant) -> Option<f32> {
        let t = at.saturating_duration_since(self.sliver?).as_secs_f32() / SLIVER.as_secs_f32();
        (t < 1.0).then_some(0.06 + 0.94 * (t * t * (3.0 - 2.0 * t)))
    }

    /// The squash begun at `t0`, now: a drawn perch's contact settle (to rest at its cut), else
    /// the landing squash.
    fn squash_at(&self, t0: Instant, now: Instant) -> Option<(f32, f32)> {
        match self.arrive.squash_end {
            // (One left from an earlier settle is before this squash: not this one's.)
            Some(end) if end > t0 => fx::settle(now.saturating_duration_since(t0), end.saturating_duration_since(t0)),
            _ => fx::squash(now.saturating_duration_since(t0)),
        }
    }

    fn pivot(&self) -> (f32, f32) {
        let l = &self.layout;
        let y = match (self.sitting(), l.seat_y) {
            (true, Some(s)) => s,
            _ => l.ground_y,
        };
        ((l.ground_x - l.avatar_x) as f32, (y - l.avatar_y) as f32)
    }

    /// The menu's last row: `SYNAPS 0.12.0 · CLAUDE-OPUS-5`, the model cut to fit the menu.
    fn menu_mark(&self, window: &Window) -> Option<String> {
        let mark = self.session.as_ref()?.link.mark()?.to_uppercase();
        let fits = |s: &str| self.tracked_width(s, MARK_PX, FontWeight::MEDIUM, 0.8, window) <= MENU_W - 20.0;
        if fits(&mark) {
            return Some(mark);
        }
        let mut c: Vec<char> = mark.chars().collect();
        while c.len() > 8 {
            c.pop();
            let s = format!("{}…", c.iter().collect::<String>().trim_end());
            if fits(&s) {
                return Some(s);
            }
        }
        None
    }

    /// Laid-out width of `tracked(s, spacing)` at `size`/`weight` (logical px).
    fn tracked_width(&self, s: &str, size: f32, weight: FontWeight, spacing: f32, window: &Window) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        // `tracked` lays out one box per glyph: no kerning, each box rounded up.
        let f = Font { weight, ..font(self.font.clone()) };
        s.chars()
            .map(|c| {
                let t = if c == ' ' { "\u{00A0}".to_string() } else { c.to_string() };
                let run = TextRun { len: t.len(), font: f.clone(), color: black(), background_color: None, underline: None, strikethrough: None };
                f32::from(window.text_system().layout_line(&t, px(size), &[run], None).width).ceil() + spacing
            })
            .sum()
    }

    fn wrap_text(&self, text: &[char], window: &Window, cx: &App) -> Wrapped {
        let f = Font { weight: FontWeight::SEMIBOLD, ..font(self.font.clone()) };
        let wrap_w = PANEL_MAX_W - PAD_L - PAD_R;
        let mut wrapper = cx.text_system().line_wrapper(f.clone(), px(BODY_FONT));
        let s: String = text.iter().collect();
        let mut lines = Vec::new();
        let mut base = 0usize;
        for para in s.split('\n') {
            let breaks: Vec<usize> = wrapper
                .wrap_line(&[LineFragment::text(para)], px(wrap_w), IndentAdjustment::NoIndent)
                .map(|b| b.ix)
                .collect();
            let mut start = 0usize;
            for ix in breaks.into_iter().chain(std::iter::once(para.len())) {
                lines.push((base + para[..start].chars().count(), base + para[..ix].chars().count()));
                start = ix;
            }
            base += para.chars().count() + 1;
        }
        let width = lines
            .iter()
            .map(|&(a, b)| {
                let t: String = text[a..b.min(text.len())].iter().collect();
                let t = t.trim_end();
                if t.is_empty() {
                    return 0.0;
                }
                let run = TextRun { len: t.len(), font: f.clone(), color: black(), background_color: None, underline: None, strikethrough: None };
                f32::from(window.text_system().layout_line(t, px(BODY_FONT), &[run], None).width)
            })
            .fold(0.0, f32::max);
        Wrapped { lines, width }
    }
}

type Cell2 = Rc<Cell<Option<Bounds<Pixels>>>>;

/// Wraps `child` in a plain box whose laid-out bounds (= the child's border box) land
/// in `cell` during prepaint, for the X shape.
fn probed(child: impl IntoElement, cell: &Cell2) -> Div {
    let cell = cell.clone();
    div()
        .relative()
        .child(child)
        .child(canvas(move |b, _, _| cell.set(Some(b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    Reply,
    Input { secret: bool },
    Prompt,
    Probe,
}

fn to_device(b: Bounds<Pixels>, sf: f32) -> ShapeRect {
    let x0 = (f32::from(b.origin.x) * sf).floor();
    let y0 = (f32::from(b.origin.y) * sf).floor();
    let x1 = (f32::from(b.origin.x + b.size.width) * sf).ceil();
    let y1 = (f32::from(b.origin.y + b.size.height) * sf).ceil();
    (x0 as i16, y0 as i16, (x1 - x0).max(1.0) as u16, (y1 - y0).max(1.0) as u16)
}

fn with_alpha(rgb: u32, a: f32) -> Rgba {
    rgba((rgb << 8) | (a.clamp(0.0, 1.0) * 255.0).round() as u32)
}

/// A bubble button's kind: DENY (bone, the focus ring), a go key (solid red), a plain one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Deny,
    Go,
    Plain,
}

#[derive(Clone, Copy)]
enum Chevron {
    Down,
    Up,
}

/// A small stroked chevron (Capri's reply controls draw theirs as SVG strokes).
fn chevron(dir: Chevron, colour: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let pts: [(f32, f32); 3] = match dir {
                Chevron::Down => [(1.5, 3.0), (5.0, 7.0), (8.5, 3.0)],
                Chevron::Up => [(1.5, 7.0), (5.0, 3.0), (8.5, 7.0)],
            };
            let mut p = PathBuilder::stroke(px(1.8));
            p.move_to(point(px(x + pts[0].0), px(y + pts[0].1)));
            p.line_to(point(px(x + pts[1].0), px(y + pts[1].1)));
            p.line_to(point(px(x + pts[2].0), px(y + pts[2].1)));
            if let Ok(path) = p.build() {
                window.paint_path(path, colour);
            }
        },
    )
    .w(px(10.0))
    .h(px(10.0))
}

struct Status {
    label: String,
    tone: u32,
    invert: bool,
    glyph: String,
    /// The nameplate's silhouette.
    plate: Plate,
    /// Its tone key (moods.json): what kind of state it is.
    key: String,
}

/// The tag's outline at `b` (its ring, and the thinking slivers at `trail`'s step) as X shape
/// rows, device px.
fn tag_rows(cut: Cut, trail: Option<u64>, b: Bounds<Pixels>, sf: f32) -> Vec<ShapeRect> {
    let (x0, y0, x1, y1) = (f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom()));
    let dev = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| (x * sf, y * sf)).collect::<Vec<_>>();
    let mut out = Vec::new();
    for p in theme::outline(cut, x0, y0, x1, y1) {
        shape::poly_rows(&dev(&theme::offset(&p, theme::RING)), &mut out);
    }
    for (p, _) in trail.map(|s| theme::trail(x1, y0, y1, s)).unwrap_or_default() {
        shape::poly_rows(&dev(&p), &mut out);
    }
    if let Cut::Tag(plate) = cut {
        for p in theme::tag_marks(plate, x0, y0, x1, y1) {
            shape::poly_rows(&dev(&theme::offset(&p, theme::RING)), &mut out);
        }
    }
    out
}

/// The rows both `a` and `b` cover (1 px tall rows, as `shape::poly_rows` makes them).
fn rows_and(a: &[ShapeRect], b: &[ShapeRect]) -> Vec<ShapeRect> {
    let mut by_y: std::collections::HashMap<i16, Vec<(i32, i32)>> = std::collections::HashMap::new();
    for r in b {
        by_y.entry(r.1).or_default().push((r.0 as i32, r.0 as i32 + r.2 as i32));
    }
    let mut out = Vec::new();
    for r in a {
        let (a0, a1) = (r.0 as i32, r.0 as i32 + r.2 as i32);
        for &(b0, b1) in by_y.get(&r.1).map_or(&[][..], |v| &v[..]) {
            let (x0, x1) = (a0.max(b0), a1.min(b1));
            if x1 > x0 {
                out.push((x0 as i16, r.1, (x1 - x0) as u16, r.3));
            }
        }
    }
    out
}

/// Bubble panel geometry, logical px in window coordinates.
#[derive(Clone, Copy, PartialEq)]
struct PanelGeom {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tail_x: f32,
    tip: (f32, f32),
}

impl PanelGeom {
    /// The outline: SHARD's cuts (18 top-left and bottom-right, 6 on the others) and the
    /// blade of a tail on the bottom edge, leaning out at her.
    fn poly(&self) -> Vec<(f32, f32)> {
        theme::bubble(self.x, self.y, self.x + self.w, self.y + self.h, self.tail_x, self.tip)
    }

    /// The mane slivers over its top edge, near the left end.
    fn slivers(&self) -> Vec<Vec<(f32, f32)>> {
        theme::slivers(self.x + 30.0, self.y - SLIVER_H, 36.0, SLIVER_H)
    }

    /// Everything it puts on screen: the panel with its dark ring, and the slivers.
    fn outline(&self) -> Vec<Vec<(f32, f32)>> {
        let mut v = vec![theme::offset(&self.poly(), theme::RING)];
        v.extend(self.slivers());
        v
    }
}

const HEARTS: [(f32, f32, f32, u32); 4] = [
    // dx, size, delay ms, colour
    (-14.0, 36.0, 0.0, 0xff3e83),
    (32.0, 28.0, 90.0, 0xff79b8),
    (-48.0, 23.0, 170.0, 0xff1f63),
    (12.0, 18.0, 250.0, 0xffb3d9),
];

impl Render for Avatar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let now = Instant::now();
        // Held: the window goes where the pointer was for this frame, and the next frame is
        // asked for while the pointer moves.
        self.rag_follow(now);
        if self.rag_following(now) {
            window.request_animation_frame();
        }
        // This frame draws the current pose: a pending step rode on it.
        self.pose_last = self.pose_at(now);
        self.pose_wait = None;
        let l = self.layout;
        let sf = l.sf;
        let pal = self.pack.palette();
        if let Some(Some((t0, true))) = self.timing
            && self.bubble.as_ref().is_some_and(|b| b.stream && b.shown > 0 && !b.collapsed)
        {
            elog!("timing: first word on screen {} ms after Enter", t0.elapsed().as_millis());
            self.timing = Some(None);
        }
        if let Some(b) = &self.bubble
            && b.wrap.is_none()
        {
            let w = self.wrap_text(&b.text, window, cx);
            if let Some(b) = &mut self.bubble {
                b.wrap = Some(w);
            }
        }
        // Carried: the rag's committed frame and plan on top, its next one underneath.
        let rag_shown = self.rag.as_ref().map(|r| (r.cur.clone().or_else(|| r.next.clone()), r.next.clone()));
        self.rag_painted();
        self.log_drawn();
        let rag_flight = self.rag_json().get("phase").and_then(|p| p.as_str()).is_some_and(|p| p == "fling" || p == "drop");
        let shown_i = rag_shown.as_ref().and_then(|(c, _)| c.as_ref().map(|c| c.frame)).unwrap_or_else(|| self.anim.frame());
        if frame_log() && std::mem::replace(&mut self.frame_logged, shown_i) != shown_i {
            let id = self.pack.frames.get(shown_i).map_or("?", |f| f.id.as_str());
            elog!("frame {} {id}{}", self.t_start.elapsed().as_millis(), self.glitch.as_ref().map_or(String::new(), |g| format!(" glitch {:?}", g.kind)));
        }
        if self.arrive.unfold.is_some_and(|t0| now.saturating_duration_since(t0).as_millis() as u64 >= arriveview::UNFOLD_MS) {
            self.arrive.unfold = None;
        }
        let frame = self.frame_or_default(shown_i);
        // HUD placement follows the mood's resting frame, so loops don't make it jitter.
        let base = self.frame_or_default(self.anim.base()).or(frame);
        // …but a taller one-shot (an entry clip) pushes it up so it never sits on the hair.
        // The tag's column is the art's own (unmirrored) head: a turn never moves it (a moved
        // tag sits out of the shape for two frames: it blinked out mid-turn).
        let raw_base = self.raw_frame_or_default(self.anim.base());
        let raw_frame = self.raw_frame_or_default(shown_i);
        let head_x = raw_base.or(raw_frame).map_or(l.geo.width as i32 / 2, |f| f.head_x);
        let mut base_top = base.map_or(0, |f| f.top.min(frame.map_or(f.top, |c| c.top)));
        // While a bubble is up its height holds (per resting frame): a one-shot raising a hand
        // made it jump ~10 px and back.
        if self.panel_up() {
            match self.hud_top.get() {
                Some((b, t)) if b == self.anim.base() => base_top = t,
                _ => self.hud_top.set(Some((self.anim.base(), base_top))),
            }
        } else {
            self.hud_top.set(None);
        }
        let top_l = (l.avatar_y + base_top) as f32 / sf;
        let head_l = (l.avatar_x + head_x) as f32 / sf;
        // The status tag sits just clear of the hair under the tag itself (this frame and
        // the resting one), not of the whole silhouette.
        let tag_hair = {
            let tw = self.tag_w.get();
            let (c0, c1) = (((head_l - tw / 2.0) * sf) as i32 - l.avatar_x - 4, ((head_l + tw / 2.0) * sf) as i32 - l.avatar_x + 4);
            // Clear of the hair under it facing either way (the profile mirrors by reversal):
            // its height doesn't change on a turn either.
            let both = self.mirror_cfg.drag || self.mirror_cfg.own;
            let under = |f: Option<&Frame>| {
                f.and_then(|f| {
                    let n = f.profile.len() as i32;
                    let cols = c0.max(0)..c1.min(n);
                    let own = cols.clone().map(|x| f.profile[x as usize]).min();
                    let other = if both { cols.map(|x| f.profile[(n - 1 - x) as usize]).min() } else { None };
                    own.into_iter().chain(other).min()
                })
                .filter(|&v| v != i16::MAX)
                .map(i32::from)
            };
            under(raw_base).into_iter().chain(under(raw_frame)).min().unwrap_or(base_top)
        };
        let image = frame.map(|f| f.image.clone());
        // Where the image sits in the avatar rect (a strip's cell is its opaque box).
        let image_at = frame.map(|f| (f.at, f.image_size()));
        let rects = frame.map(|f| f.rects.clone()).unwrap_or_default();
        let hidden = self.hidden || frame.is_none();
        // Painted while still outside her shape (a drawn touchdown, the frame before it shows).
        let prepaint = hidden && frame.is_some() && self.arrive.prepaint && self.teleport.is_some();
        let unpainted = hidden && !prepaint;
        let has_frame = frame.is_some() && !hidden;
        // P6: while she's carried or flying, the bounding shape's rows merged within 1 px
        // (inward only): Xorg's per-move cost scales with the shape's rects (1,220 -> ~570).
        let motion_tol = shape_tol().or(self.rag.is_some().then_some(1));
        let (aw, ah) = (l.geo.width as i32, l.geo.height as i32);

        // Body plan: squash, glitch.
        let mut plan = BodyPlan::plain();
        plan.pivot = self.pivot();
        if let Some(t0) = self.squash
            && let Some((sx, sy)) = self.squash_at(t0, now)
            && self.rag.is_none()
        {
            plan.sx = sx;
            plan.sy = sy;
        }
        if let Some((Some(c), _)) = &rag_shown {
            plan = c.plan.clone();
        }
        // The sit's legs unfolding over the edge (a CapriCore perch arrival): rows below the
        // seat line come in top-down.
        let unfold_at = |at: Instant| {
            let (Some(t0), Some(f), Some(sy)) = (self.arrive.unfold, frame, l.seat_y) else { return None };
            if !self.sitting() || self.rag.is_some() || at < t0 {
                return None;
            }
            let ms = at.saturating_duration_since(t0).as_millis() as u64;
            let seat = sy - l.avatar_y;
            Some(arriveview::unfold_bands(f.top.min(seat), seat, f.bottom, arriveview::unfold_depth(ms), arriveview::UNFOLD_SEED))
        };
        let unfolding = unfold_at(now).is_some();
        if let Some(bands) = unfold_at(now) {
            plan.slices = Some(if plan.sx != 1.0 || plan.sy != 1.0 { fx::scale_bands(&bands, plan.pivot, plan.sx, plan.sy) } else { bands });
        }
        // Its next steps go underneath: the legs reach further down each frame, and where they
        // grow past this frame's, last frame's paint shows until this one lands (r34: a faint
        // dark copy of the lower legs). Every step up to two frames ahead, so there's no gap
        // between where they are and where they'll be.
        // All of the sit underneath while it does (and the frames before it starts: a present
        // lands up to two frames late), so every row that comes in shows her legs at once.
        let unfold_next = unfolding || self.arrive.unfold.is_some_and(|t0| t0 > now && t0 <= now + FRAME_FX * 2);
        let unfold_first = self.arrive.unfold.filter(|&t0| t0 > now && t0 <= now + FRAME_FX * 2).map(|t0| self.anim.frame_at(t0));
        // A drawn arrival's first frames: the plain red land silhouette (its window's old
        // pixels would show through the new shape for a frame).
        if self.arrive.red_until.is_some_and(|t| now < t) || prepaint {
            plan.tint = Some(fx::BOLT_RED);
        }
        let sliver_px = (l.ground_x - l.avatar_x) as f32;
        if let Some(sx) = self.sliver_sx(now) {
            plan.sx = sx;
            plan.pivot.0 = sliver_px;
        }
        // A turn: its shape now; its next one goes underneath (with the art it shows then),
        // not the unsquashed frame (that peeked out at her edges as shards on the rebound).
        let turning = self.rag.is_none() && self.turn.is_some();
        if let Some(((kx, ky), _)) = self.turn_at(now).filter(|_| turning) {
            plan.sx *= kx;
            plan.sy *= ky;
            plan.pivot = (sliver_px, plan.pivot.1);
        }
        // The next frame's plan, painted underneath this one. The bounding shape is set when
        // a frame paints, but its present lands up to a frame later, so wherever the next
        // shape grows, this frame's pixels show for a moment. Without an underlay they're
        // transparent (black slabs without a compositor); with it, they're what comes next.
        let mut next: Option<BodyPlan> = None;
        if let Some(sx) = self.glitch.and_then(|g| g.squeeze_sx(now)) {
            // A quiet exit: squeezed in one piece; the next width goes underneath.
            plan.sx = sx;
            plan.pivot.0 = sliver_px;
            let mut n = plan.clone();
            n.sx = self.glitch.and_then(|g| g.squeeze_sx(now + FRAME_FX)).unwrap_or(sx);
            next = Some(n);
        } else if let Some(g) = self.glitch {
            let (top, face, bx0, bx1) = frame.map_or((0, None, 0, aw), |f| {
                let (x0, x1) = f.profile.iter().enumerate().filter(|p| *p.1 != i16::MAX).fold((aw, 0), |a, (i, _)| (a.0.min(i as i32), a.1.max(i as i32)));
                (f.top, g.keeps_face().then_some(f.face), x0, x1.max(x0 + 1))
            });
            // Last frame's tag rows (avatar-local device px), so a cut tears the plate.
            let tag = self.tag_box.get().filter(|_| g.tears_tag).map(|b| {
                let y = f32::from(b.origin.y) * sf - l.avatar_y as f32;
                (y as i32, (y + f32::from(b.size.height) * sf) as i32)
            });
            let (step, laid_at) = match self.glitch_step.get() {
                (Some(t0), n) if t0 == g.t0 => (n, self.glitch_at.get()),
                _ => (0, None),
            };
            self.glitch_step.set((Some(g.t0), step + 1));
            let base = plan.clone();
            // Landing on a window top: the rows at the edge and below stay put (her legs never
            // tear across the edge line, the touchdown's dark parts never smear along it).
            // Touching down into a drawn pose (lachy's art): it lands clean on the cut frame (the
            // red silhouette before it is the touchdown's; tears after it frayed her hair).
            let drawn_land = g.kind == GlitchKind::Land && self.sitting() && self.pack.frames.get(shown_i).is_some_and(|f| f.cell.is_some());
            let still_legs = (matches!(g.kind, GlitchKind::Land | GlitchKind::Seam) && self.sitting())
                .then_some(l.seat_y)
                .flatten()
                .map(|s| s - l.avatar_y - (24.0 * sf).round() as i32);
            // Carried or landing, the next frame is the rag's next plan: the pattern goes
            // underneath on that (its squash and frame), not on this one.
            let rag_next = rag_shown.as_ref().and_then(|(_, n)| n.clone());
            let lay_on = |b: &BodyPlan, at: Instant, step: u64| {
                let mut p = b.clone();
                p.glitch(fx::GlitchStep {
                    strength: g.strength(at),
                    coverage: g.coverage(at),
                    seed: g.step_seed(step),
                    order: g.seed(),
                    y0: -l.avatar_y,
                    h: ah,
                    body: (bx0, top, bx1 - bx0, ah - top),
                    face,
                    solid: g.solid(at),
                    tag,
                    sf,
                    calm: g.kind == GlitchKind::Land,
                    still_from: still_legs,
                    light: g.kind == GlitchKind::Seam,
                });
                // The drawn pose lands as itself on the cut frame (no red silhouette of it).
                p.tint = if drawn_land { None } else { g.tint(at) };
                p
            };
            // Out of a still figure, the first frame stays still and lays the first pattern
            // underneath (one frame late, no black slabs on the onset).
            // A swap's frames before its cut: the same (its step goes on the cut frame).
            let pre_roll = (step == 0 && matches!(g.kind, GlitchKind::Burst | GlitchKind::Big | GlitchKind::Vanish | GlitchKind::Seam)) || now < g.t0;
            if drawn_land {
                // Clean from the cut on: this frame and the next as they are.
            } else if !pre_roll {
                // At the time it was laid underneath last frame (the frame came later or
                // sooner than planned: a pattern laid at `now` instead would grow the shape
                // where nothing was painted, a black slab without a compositor).
                plan = lay_on(&base, laid_at.unwrap_or(now), step);
            }
            let at = (now + FRAME_FX).max(g.t0);
            self.glitch_at.set(Some(at));
            let next_base = rag_next.as_ref().map_or(base, |n| n.plan.clone());
            next = Some(if g.done(at) || drawn_land { next_base.clone() } else { lay_on(&next_base, at, step + 1) });
        }
        if unfold_next && unfold_first.is_none() && next.is_none() {
            let mut n = plan.clone();
            n.slices = None;
            next = Some(n);
        }
        // The sliver widens next frame: that width goes underneath.
        if let Some(sx) = self.sliver_sx(now + FRAME_FX).or(self.sliver.map(|_| 1.0)) {
            let n = next.get_or_insert_with(|| {
                let mut p = BodyPlan::plain();
                p.pivot = plan.pivot;
                p
            });
            n.sx = sx;
            n.pivot.0 = sliver_px;
        }
        // The rag's next plan (and frame) goes underneath (§4: commit one frame ahead).
        let mut next_img: Option<(Arc<RenderImage>, ImageBox)> = None;
        if let Some((_, Some(n))) = &rag_shown {
            // Under a glitch, `next` is already the pattern laid on this plan.
            if self.glitch.is_none() {
                next = Some(n.plan.clone());
            }
            if n.frame != shown_i {
                next_img = self.frame_or_default(n.frame).map(|f| (f.image.clone(), (f.at, f.image_size())));
            }
        }
        // The frame changes before the next one (an arrival's cut, a blink): that frame goes
        // underneath, its silhouette in the tint when tinted (where it grows past this one, the
        // window showed what it hadn't drawn: a dark slab of her new legs at a perch cut).
        let mut next_rects: Option<Arc<Vec<ShapeRect>>> = None;
        if self.rag.is_none() && !hidden {
            let nf = unfold_first.unwrap_or_else(|| self.anim.frame_at(now + FRAME_FX));
            if nf != shown_i
                && let Some(f) = self.frame_or_default(nf)
            {
                next_img = Some((f.image.clone(), (f.at, f.image_size())));
                next_rects = Some(f.rects.clone());
                if next.is_none() || unfold_first.is_some() {
                    let mut n = next.take().unwrap_or_else(|| plan.clone());
                    n.slices = None;
                    next = Some(n);
                }
            }
        }
        // A drawing changed without warning (a snap turn, a walk setting off): the last one goes
        // underneath for this paint. The screen holds the last present until this one lands (a
        // frame late): clipped to the new outline at once, that was the old drawing cut to the
        // new one's shape (a shredded frame). One frame of the old outline around the new
        // drawing instead. A change laid underneath ahead (the look-ahead above) needs none; on
        // the cut frame this one wins over laying the next cell ahead (a strip's next cell is
        // about the same outline).
        let prev_drawn = self.drawn_last.take();
        if let Some(img) = &image {
            self.drawn_last = Some(DrawnLast {
                img: img.clone(),
                at: image_at.unwrap_or_default(),
                rects: rects.clone(),
                next: next_img.as_ref().map(|n| n.0.clone()),
            });
        }
        if self.rag.is_none()
            && !hidden
            && let (Some(p), Some(img)) = (prev_drawn, &image)
            && !Arc::ptr_eq(&p.img, img)
            && !p.next.as_ref().is_some_and(|n| Arc::ptr_eq(n, img))
        {
            // As it was (a seam's next pattern on it would only fray it).
            next_img = Some((p.img, p.at));
            next_rects = Some(p.rects);
            let mut n = plan.clone();
            n.slices = None;
            next = Some(n);
        }
        // Alpha mode (a compositor shows the window's own alpha): nothing is clipped by a
        // bounding shape, so everything painted shows, and there is no shape lag to cover.
        // Every underlay below (the next plan, the 8-way smear under posed bands, the
        // unsquashed frame, the tag's next strips) would show as stacked offset copies of her
        // edges: none of them in alpha mode.
        let alpha = self.os.composited();
        if alpha {
            next = None;
            next_img = None;
        }
        // Device px the art is also painted at underneath the posed bands (see the body).
        let mut under = 0.0f32;
        if plan.slices.is_none() && plan.sx == 1.0 && plan.sy == 1.0 && !hidden && self.rag.is_none() {
            let pose = self.pose_at(now);
            let fi = self.anim.frame();
            let swing = self.fr(fi).and_then(|f| self.swing_bands(fi, f, false));
            if (pose != fx::Pose::default() || swing.is_some())
                && let Some(f) = self.fr(fi)
            {
                let rows = self.pose_rows(fi, f);
                let bands = swing.unwrap_or_else(|| fx::pose_bands(pose, rows));
                // Mirrored, the pose's kicking leg is on the other side of the split.
                let mir = self.mirrored(fi);
                plan.leg = fx::pose_leg(pose, rows, &bands).map(|mut k| {
                    k.right ^= mir;
                    k
                });
                plan.slices = Some(bands);
                under = if alpha { 0.0 } else { (POSE_REACH * sf).ceil() };
                // The chain swing moves more per frame than the smear reaches: its next
                // bands go underneath (commit-ahead).
                if next.is_none()
                    && !alpha
                    && let Some(nb) = self.swing_bands(fi, f, true)
                {
                    let np = self.pose_at(now + Duration::from_millis(33));
                    let mut n = BodyPlan::plain();
                    n.leg = fx::pose_leg(np, rows, &nb).map(|mut k| {
                        k.right ^= mir;
                        k
                    });
                    n.slices = Some(nb);
                    next = Some(n);
                }
            }
        }
        // The glitch ends next frame: the plain or posed figure comes back.
        if !turning && next.as_ref().is_some_and(|n| n.slices.is_none()) {
            under = (POSE_REACH * sf).ceil();
        }
        let nrects: &[ShapeRect] = next_rects.as_deref().map_or(&rects[..], |v| &v[..]);
        let next_ghosts = next.as_ref().filter(|n| n.has_ghosts() && !hidden).map(|n| n.ghost_rects(nrects)).unwrap_or_default();
        let next_fill: Vec<(ShapeRect, u32)> = next
            .as_ref()
            .filter(|_| !hidden)
            .map(|n| match n.tint {
                Some(c) => n.body_rects(nrects).into_iter().map(|r| (r, c)).collect(),
                None => n.solid_rects(nrects),
            })
            .unwrap_or_default();

        let next = next.filter(|_| !hidden).map(Rc::new);
        let next_tag = next.clone();
        let plan = Rc::new(plan);
        let mut body_shape: Vec<ShapeRect> = if hidden {
            Vec::new()
        } else if plan.is_plain() {
            rects.to_vec()
        } else {
            plan.shape(&rects)
        };
        // Shape mode turn: the art as it is (it's on screen already), shown through a band
        // about her feet (`facing::turn_band`); only the shape moves, so nothing lands late.
        if turning && !alpha {
            if let Some((frac, _)) = crate::facing::turn_band(self.turn_frame)
                && !hidden
            {
                let p = sliver_px as i32;
                let x0 = body_shape.iter().map(|r| r.0 as i32).min().unwrap_or(p);
                let x1 = body_shape.iter().map(|r| r.0 as i32 + r.2 as i32).max().unwrap_or(p);
                let half = (((p - x0).max(x1 - p)) as f32 * frac).max(3.0) as i32;
                let (a, b) = (p - half, p + half);
                let clipped: Vec<ShapeRect> = body_shape
                    .iter()
                    .filter_map(|r| {
                        let (rx0, rx1) = ((r.0 as i32).max(a), (r.0 as i32 + r.2 as i32).min(b));
                        (rx1 > rx0).then(|| (rx0 as i16, r.1, (rx1 - rx0) as u16, r.3))
                    })
                    .collect();
                if !clipped.is_empty() {
                    body_shape = clipped;
                }
            }
            self.turn_frame += 1;
        }
        if !alpha {
            let red = self.arrive.red_until.is_some_and(|t| now < t) && !hidden && self.teleport.is_some();
            let shape = body_shape.clone();
            self.sync_cover(&shape, (aw, ah), red);
        }
        let ghost_rects = if !plan.has_ghosts() || hidden { Vec::new() } else { plan.ghost_rects(&rects) };
        let solid_rects = if plan.solid.iter().all(Option::is_none) || hidden { Vec::new() } else { plan.solid_rects(&rects) };
        let tint_rects = if plan.tint.is_some() && !unpainted { plan.body_rects(&rects) } else { Vec::new() };
        let bar_rects = if hidden || plan.tint.is_some() { Vec::new() } else { plan.bar_rects(&rects) };
        let entity = cx.entity();
        let warm: Vec<Arc<RenderImage>> = if self.loaded && !self.warmed {
            self.warmed = true;
            self.frames.iter().flatten().map(|f| f.image.clone()).collect()
        } else {
            Vec::new()
        };

        let menu_cell: Cell2 = Rc::new(Cell::new(None));
        // The menu laid out unseen this frame (to be measured), and its rows' signature.
        let (mut menu_unseen, mut menu_sig) = (false, 0u32);
        let tag_cell: Cell2 = Rc::new(Cell::new(None));
        let mut root = div().size_full().relative().font_family(self.font.clone()).text_color(rgb(pal.text));
        // Effect layers under her (the perch shadow), when her window paints them.
        if self.os.fx_in_scene() {
            let os = self.os.clone();
            root = root.child(canvas(|_, _, _| (), move |_, _, window, _| os.paint_fx(window, true, sf)).absolute().top_0().left_0().size_full());
        }

        // The body.
        let plan2 = plan.clone();
        root = root.child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if !warm.is_empty() {
                        let off = ContentMask { bounds: Bounds::new(point(px(-8.0), px(-8.0)), size(px(1.0), px(1.0))) };
                        window.with_content_mask(Some(off), |window| {
                            for img in warm {
                                window.paint_image(bounds, bounds, Corners::default(), img, 0, false).ok();
                            }
                        });
                    }
                    let (ax, ay) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                    let lp = |v: f32| v / sf;
                    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(ax + lp(x)), px(ay + lp(y))), size(px(lp(w)), px(lp(h))));
                    // The image's bounds when the whole avatar rect is drawn at (x, y, w, h).
                    let cur_at = image_at.unwrap_or(((0, 0), (aw, ah)));
                    let put = |at: ((i32, i32), (i32, i32)), x: f32, y: f32, w: f32, h: f32| {
                        let (a, b, c, d) = crate::frames::place_in(at, (x, y, w, h), aw, ah);
                        rect(a, b, c, d)
                    };
                    if let (Some(img), false) = (image, unpainted) {
                        // Underlays first: invisible outside this frame's shape and covered
                        // inside it, they fill the strips the next shape grows into.
                        if under > 0.0 {
                            let (ix, iy, iw, ih) = plan2.image_rect(aw, ah);
                            for k in [1.0, 0.5] {
                                for (ox, oy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
                                    let r = put(cur_at, ix + ox * under * k, iy + oy * under * k, iw, ih);
                                    window.paint_image(r, r, Corners::default(), img.clone(), 0, false).ok();
                                }
                            }
                        }
                        if let Some(n) = &next {
                            for &(r, c) in next_ghosts.iter().chain(&next_fill) {
                                window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(c)));
                            }
                            let (nx, ny, nw, nh) = n.image_rect(aw, ah);
                            let (img, n_at) = next_img.clone().unwrap_or_else(|| (img.clone(), cur_at));
                            match (&n.slices, n.tint) {
                                (_, Some(_)) => {}
                                (None, None) => {
                                    let r = put(n_at, nx, ny, nw, nh);
                                    window.paint_image(r, r, Corners::default(), img.clone(), 0, false).ok();
                                }
                                (Some(bands), None) => {
                                    for (i, &(y0, y1, dx, dy)) in bands.iter().enumerate() {
                                        if n.solid.get(i).is_some_and(Option::is_some) {
                                            continue;
                                        }
                                        let shifted = put(n_at, nx + dx as f32, ny + dy as f32, nw, nh);
                                        let clip = rect(nx + dx as f32, (y0 + dy) as f32, nw, (y1 - y0) as f32);
                                        window.paint_image(clip, shifted, Corners::default(), img.clone(), 0, false).ok();
                                    }
                                }
                            }
                        }
                        for &(r, c) in &ghost_rects {
                            window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(c)));
                        }
                        let (ix, iy, iw, ih) = plan2.image_rect(aw, ah);
                        let ib = rect(ix, iy, iw, ih);
                        match &plan2.slices {
                            _ if plan2.tint.is_some() => {
                                let c = rgb(plan2.tint.unwrap_or(fx::BOLT_RED));
                                for r in &tint_rects {
                                    window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), c));
                                }
                            }
                            None => {
                                // Squashed: the unsquashed frame goes underneath. Outside the
                                // (squashed) bounding shape it's invisible, but it fills the
                                // window's backing pixels there, so the band the shape grows
                                // back into on the rebound shows hair, not stale black.
                                if (plan2.sy < 1.0 || plan2.sx < 1.0) && !alpha && !turning {
                                    let full = rect(0.0, 0.0, aw as f32, ah as f32);
                                    window.paint_image(full, put(cur_at, 0.0, 0.0, aw as f32, ah as f32), Corners::default(), img.clone(), 0, false).ok();
                                }
                                window.paint_image(ib, put(cur_at, ix, iy, iw, ih), Corners::default(), img, 0, false).ok();
                            }
                            Some(bands) => {
                                for (r, c) in &solid_rects {
                                    window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(*c)));
                                }
                                for (i, &(y0, y1, dx, dy)) in bands.iter().enumerate() {
                                    if plan2.solid.get(i).is_some_and(Option::is_some) {
                                        continue;
                                    }
                                    let kick = plan2.leg.as_ref().and_then(|l| Some((l, *l.dx.get(i)?))).filter(|(_, d)| *d != 0);
                                    if let Some((l, e)) = kick {
                                        // The kicking leg's columns move on their own.
                                        let s = l.split as f32;
                                        let (still, moving) = if l.right { ((ix, s - ix), (s, ix + iw - s)) } else { ((s, ix + iw - s), (ix, s - ix)) };
                                        for ((x0, w), d) in [(still, dx), (moving, dx + e)] {
                                            let shifted = put(cur_at, ix + d as f32, iy + dy as f32, iw, ih);
                                            let clip = rect(x0 + d as f32, (y0 + dy) as f32, w, (y1 - y0) as f32);
                                            window.paint_image(clip, shifted, Corners::default(), img.clone(), 0, false).ok();
                                        }
                                        continue;
                                    }
                                    let shifted = put(cur_at, ix + dx as f32, iy + dy as f32, iw, ih);
                                    let clip = rect(ix + dx as f32, (y0 + dy) as f32, iw, (y1 - y0) as f32);
                                    window.paint_image(clip, shifted, Corners::default(), img.clone(), 0, false).ok();
                                }
                            }
                        }
                        if plan2.scan > 0.02 {
                            let (dark, light) = (with_alpha(0x02040a, plan2.scan), with_alpha(0x02040a, plan2.scan * 0.4));
                            let in_face = |y: i32| plan2.face.is_some_and(|(a, b)| y >= a && y < b);
                            let mut y = 0;
                            while y < ah {
                                let c = if in_face(y) { light } else { dark };
                                window.paint_quad(fill(rect(-120.0, y as f32, aw as f32 + 240.0, (1.0 * sf).max(1.0)), c));
                                y += (3.0 * sf).round() as i32;
                            }
                        }
                        for &(r, c) in &bar_rects {
                            window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(c)));
                        }
                    }
                    let e = entity.clone();
                    window.on_mouse_event(move |ev: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            e.update(cx, |v, cx| v.mouse_down(ev, cx));
                        }
                    });
                    let e = entity.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            e.update(cx, |v, cx| v.mouse_move(ev, cx));
                        }
                    });
                    let e = entity.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            e.update(cx, |v, cx| v.mouse_up(ev, window, cx));
                        }
                    });
                },
            )
            .absolute()
            .left(l.lp(l.avatar_x))
            .top(l.lp(l.avatar_y))
            .w(l.lp(aw))
            .h(l.lp(ah)),
        );

        let hud = !hidden && self.teleport.is_none() && self.rag.is_none() && !self.quitting && self.hud_after.is_none_or(|t| t <= now);
        // A quiet (passive) teleport brings the tag back with the body as it reforms.
        let tag_hud = hud
            || (!hidden
                && !self.quitting
                && self.rag.is_none()
                && self.arrive.formed.is_none_or(|t| t <= now)
                && self.teleport.as_ref().is_some_and(|t| t.quiet && matches!(t.leg, Leg::Appear)));
        let Status { label, tone, invert, glyph: glyph_kind, plate: plate_kind, key: tone_key } = self.status();
        // A summon's NEEDS YOU pulses for a few seconds after she lands (the plate's bone half).
        let pulse = self.summon_pulse_on(now);
        let glow = self.summon_glow(now);
        // What the bubble shows: the text field, a pending prompt, the reply, or the probe.
        let mode = if !hud {
            None
        } else if let Some(i) = &self.input {
            Some(PanelMode::Input { secret: i.secret.is_some() })
        } else if self.line.is_some() {
            // The Line shows the words (one place for them).
            None
        } else if self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed) {
            Some(PanelMode::Prompt)
        } else if self.focus_probe.is_some() {
            Some(PanelMode::Probe)
        } else if self.bubble.as_ref().is_some_and(|b| !b.collapsed) {
            Some(PanelMode::Reply)
        } else {
            None
        };
        let bubble_open = mode.is_some();
        // The chip: a collapsed reply or a prompt put off, one click away.
        let collapsed =
            !bubble_open && (self.bubble.as_ref().is_some_and(|b| b.collapsed) || self.prompt_ui.as_ref().is_some_and(|p| p.dismissed));

        // Status tag above the hair, unless the bubble carries the status or a reaction
        // needs the space.
        // Hanging from the bar or clinging to a window side, the tag would sit in the bar or
        // over his window: it comes back when she lands (a prompt or reply still shows it).
        let posed_quiet = self.posed() && self.prompt_ui.is_none() && !self.bubble.as_ref().is_some_and(|b| b.collapsed);
        // A swap in place's held frame: the tag sits it out, and comes back at its new place.
        let swap_hold = self.art.cut_hold.is_some_and(|t| now < t);
        let show_tag = tag_hud && self.frames.iter().any(Option::is_some) && !bubble_open && self.reaction_until.is_none() && !posed_quiet && !swap_hold;
        // In a glitch the tag tears with the body: strips of it, shifted by the same bands.
        let mut tag_strips: Vec<ShapeRect> = Vec::new();
        let torn = show_tag && !collapsed && self.glitch.is_some_and(|g| g.tears_tag);
        // The tag's cut and its trailing slivers' step (for the shape), and the HUD's next
        // motion step.
        let mut tag_cut: Option<(Cut, Option<u64>)> = None;
        let mut motion: Option<Instant> = None;
        if show_tag {
            // An unread reply is a notification, not a status: a red blade with a bone "N NEW"
            // badge, her name, the reply's first words and READ; the whole plate opens the
            // Line there. It nudges toward her every 2.2 s.
            let unread = self.unread_shown();
            let fresh = unread.is_some_and(|u| !u.refused);
            // The beat: NEEDS YOU's bone plate turns black and the rec dot dims on its second
            // half (1 Hz; a summon's pulse holds it bone), the thinking slivers step every
            // 400 ms, WORKING's sweep crosses its red block every 2.4 s.
            let ms = now.saturating_duration_since(self.t_start).as_millis() as u64;
            let mut look = pal.look(plate_kind, &tone_key, tone, invert, (ms / 500) % 2 == 1 && !pulse);
            // Her night (low energy): ONLINE dimmed, a screen turned down.
            if tone_key == "online" && self.band() == crate::life::Band::Low {
                let dim = |c: u32| theme::mix(c, pal.panel, NIGHT_TAG_DIM);
                (look.edge, look.ink, look.mark) = (dim(look.edge), dim(look.ink), dim(look.mark));
            }
            let step = ms / 400;
            let nudge = if fresh && ms % 2200 < 240 { 5.0 } else { 0.0 };
            let beats = invert && matches!(look.plate, Plate::Flag);
            let in_sweep = ms % SWEEP_EVERY_MS < SWEEP_STEPS * SWEEP_STEP_MS;
            let sweep = (look.half && in_sweep).then(|| ((ms % SWEEP_EVERY_MS) / SWEEP_STEP_MS) as f32 / (SWEEP_STEPS - 1) as f32);
            let next_ms = [
                beats.then_some((ms / 500 + 1) * 500),
                look.trail.then_some((ms / 400 + 1) * 400),
                look.half.then_some(if in_sweep { (ms / SWEEP_STEP_MS + 1) * SWEEP_STEP_MS } else { ms - ms % SWEEP_EVERY_MS + SWEEP_EVERY_MS }),
                fresh.then_some(if ms % 2200 < 240 { ms - ms % 2200 + 240 } else { ms - ms % 2200 + 2200 }),
            ]
            .into_iter()
            .flatten()
            .min();
            motion = next_ms.map(|m| self.t_start + Duration::from_millis(m));
            tag_cut = Some(if fresh { (Cut::Unread, None) } else { (Cut::Tag(look.plate), look.trail.then_some(step)) });
            let badge = unread.filter(|u| !u.refused).map(|u| format!("{} NEW", u.n));
            let text = match unread {
                Some(u) if u.refused => u.text.clone(),
                Some(u) => {
                    // Inside her window: the plate is centred on her.
                    let head = format!("{} NEW  READ ▸  ", u.n);
                    let per = TAG_FONT * 0.62 + 1.5;
                    let fit = ((l.win_w as f32 / sf - 80.0) / per) as usize;
                    crate::session::one_line(&u.text, fit.saturating_sub(head.chars().count()).clamp(8, 44))
                }
                // Her state only, no name (Haseeb, 2026-11-16: "just online, sleeping etc").
                None => label.clone(),
            };
            // WORKING: the label on the red block (ink).
            let (name_part, rest) = if look.half && unread.is_none() { (Some(label.clone()), String::new()) } else { (None, text.clone()) };
            let shade = [pal.accent, theme::mix(pal.accent, pal.bg, 0.45), pal.accent_dim];
            let make_ring = || -> Div {
                if let Some(b) = badge.clone() {
                    return theme::plate(Cut::Unread, Fill::framed(pal.panel, pal.accent, 2.0).ringed(pal.ring))
                        .h(px(28.0))
                        .gap(px(9.0))
                        .pr(px(22.0))
                        .text_size(px(TAG_FONT))
                        .line_height(px(TAG_FONT + 1.0))
                        .font_weight(FontWeight::BOLD)
                        .child(
                            theme::plate(Cut::Badge, Fill::solid(pal.text))
                                .h_full()
                                .gap(px(6.0))
                                .pl(px(11.0))
                                .pr(px(14.0))
                                .text_color(rgb(pal.ink))
                                .child(glyph("dot", pal.accent, pal.text))
                                .child(tracked(&b, 1.0)),
                        )
                        .child(div().text_color(rgb(pal.text)).font_weight(FontWeight::SEMIBOLD).child(tracked(&text, 0.2)))
                        // It opens: say so, inside the plate.
                        .child(theme::plate(Cut::Slant(6.0), Fill::solid(pal.accent)).h(px(18.0)).px(px(8.0)).text_color(rgb(pal.ink)).child(tracked("READ ▸", 1.2)));
                }
                let (pad_l, pad_r) = look.plate.pad();
                let fill = Fill::framed(look.fill, look.edge, 1.5).dashed(look.dash).ringed(pal.ring);
                let split = look.plate == Plate::Split;
                // DONE's tick is drawn through the plate's corner, not as a glyph.
                let mark = match look.plate {
                    Plate::Split if glyph_kind == "cross" => "shard".to_string(),
                    Plate::Check => String::new(),
                    _ => glyph_kind.clone(),
                };
                let (ring_c, block_c, sweep_c) = (pal.ring, pal.accent, theme::mix(pal.accent, pal.text, 0.45));
                let block = name_part.clone().map(|n| {
                    div()
                        .relative()
                        .h_full()
                        .ml(px(-pad_l))
                        .pl(px(pad_l))
                        .pr(px(13.0))
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .text_color(rgb(pal.ink))
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |b, _, window, _| {
                                    let (x0, y0, x1, y1) = (f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom()));
                                    let (blk, sw) = theme::half_block(x0, y0, x1, y1, sweep);
                                    theme::fill_poly(window, &blk, rgb(block_c));
                                    if let Some(sw) = sw {
                                        theme::fill_poly(window, &sw, rgb(sweep_c));
                                    }
                                },
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        )
                        .child(glyph(&mark, pal.ink, pal.accent))
                        .child(tracked(&n, 1.5))
                });
                let half = block.is_some();
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .h(px(25.0))
                    .pl(px(pad_l))
                    .pr(px(pad_r))
                    .text_size(px(TAG_FONT))
                    .line_height(px(TAG_FONT + 1.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(look.ink))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |b, _, window, _| {
                                let (x0, y0, x1, y1) = (f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom()));
                                let polys = theme::outline(Cut::Tag(look.plate), x0, y0, x1, y1);
                                match theme::tag_inner(look.plate, x0, y0, x1, y1, 1.5) {
                                    Some(inner) => {
                                        theme::paint(window, &polys, Fill::solid(look.edge).ringed(ring_c));
                                        for p in inner {
                                            theme::fill_poly(window, &p, rgb(look.fill));
                                        }
                                    }
                                    None => theme::paint(window, &polys, fill),
                                }
                                if look.trail {
                                    for (p, lvl) in theme::trail(x1, y0, y1, step) {
                                        theme::fill_poly(window, &p, rgb(shade[lvl as usize]));
                                    }
                                }
                                for p in theme::tag_marks(look.plate, x0, y0, x1, y1) {
                                    theme::fill_poly(window, &theme::offset(&p, theme::RING), rgb(ring_c));
                                    theme::fill_poly(window, &p, rgb(look.mark));
                                }
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .children(block)
                    // The ✕'s shard broken off with the plate's piece.
                    .when(!half, |d| {
                        d.child(match look.halo {
                            // The disc: 15 px, the glyph centred in it.
                            Some(h) => div()
                                .relative()
                                .size(px(15.0))
                                .mx(px(-2.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().absolute().top_0().left_0().child(theme::glyph_at("disc", h, h, 15.0)))
                                .child(glyph(&mark, look.mark, h)),
                            None => div().when(split, |d| d.relative().left(px(-3.0)).top(px(3.0))).child(glyph(&mark, look.mark, look.fill)),
                        })
                    })
                    // ERROR: the type starts inside the toothed piece, clear of the break.
                    .child(theme::metered(&rest, 1.5, look.ink).when(split, |d| d.ml(px(8.0))))
                    // A reply put away behind the plate: a small up-chevron inside it.
                    .when(collapsed, |d| d.child(chevron(Chevron::Up, rgb(look.ink))))
            };
            if let (true, Some(b), Some(bands)) = (torn, self.tag_box.get(), plan.slices.as_ref()) {
                let (bx, by) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let (bw, bh) = (f32::from(b.size.width), f32::from(b.size.height));
                // The strips reach past the box: the ring, a point or swallowtail, the slivers.
                let (ml, mr, mt, mb) = (4.0, 34.0, 9.0, 6.0);
                let t0 = (by - mt) * sf - l.avatar_y as f32;
                let th = bh + mt + mb;
                // Next frame's strips first (underneath, same reason as the body's underlay),
                // then this frame's on top. Only this frame's go into the shape.
                let under_bands: Vec<(i32, i32, i32, i32)> = if alpha {
                    Vec::new()
                } else {
                    next_tag.as_ref().and_then(|n| n.slices.clone()).unwrap_or_else(|| vec![(i32::MIN / 2, i32::MAX / 2, 0, 0)])
                };
                let strip = |s0: f32, s1: f32, dx: f32| {
                    div()
                        .absolute()
                        .left(px(bx + dx - ml))
                        .top(px(by - mt + s0))
                        .w(px(bw + ml + mr))
                        .h(px(s1 - s0))
                        .overflow_hidden()
                        .child(div().absolute().left(px(ml)).top(px(mt - s0)).w(px(bw)).h(px(bh)).child(make_ring()))
                };
                for &(y0, y1, dx, _) in &under_bands {
                    let (a, c) = ((y0 as f32).max(t0), (y1 as f32).min(t0 + th * sf));
                    if c <= a {
                        continue;
                    }
                    let (s0, s1, dx) = ((a - t0) / sf, (c - t0) / sf, dx as f32 / sf);
                    root = root.child(strip(s0, s1, dx));
                }
                let (cut, trail) = tag_cut.unwrap_or((Cut::Tag(Plate::Slant), None));
                for &(y0, y1, dx, _) in bands {
                    let (a, c) = ((y0 as f32).max(t0), (y1 as f32).min(t0 + th * sf));
                    if c <= a {
                        continue;
                    }
                    let (s0, s1, dxl) = ((a - t0) / sf, (c - t0) / sf, dx as f32 / sf);
                    let moved = Bounds::new(point(px(bx + dxl), px(by)), b.size);
                    let rows = tag_rows(cut, trail, moved, sf);
                    let (d0, d1) = (((by - mt + s0) * sf).round() as i16, ((by - mt + s1) * sf).round() as i16);
                    tag_strips.extend(rows.into_iter().filter(|r| r.1 >= d0 && r.1 < d1));
                    root = root.child(strip(s0, s1, dxl));
                }
            }
            let ring = make_ring();
            let mut row = div().id("tag").flex().items_end().gap(px(3.0)).cursor_pointer();
            if collapsed || self.session.is_some() || self.unread_shown().is_some() {
                row = row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.chip_click(window, cx);
                    }),
                );
            }
            row = row.child(ring);
            let centre = head_l + nudge;
            // Asleep, the plate sits a little lower.
            let tag_bottom = (l.avatar_y + tag_hair) as f32 / sf - TAG_GAP + look.drop;
            let mut holder = div()
                .absolute()
                .left_0()
                .w(l.lp(l.win_w))
                .bottom(px(l.win_h as f32 / sf - tag_bottom))
                .flex()
                .justify_center()
                .pl(px((2.0 * centre - l.win_w as f32 / sf).max(0.0)))
                .pr(px((l.win_w as f32 / sf - 2.0 * centre).max(0.0)));
            // Torn: the whole tag still lays out (invisibly) so its box stays current.
            if torn {
                holder = holder.invisible();
            }
            root = root.child(holder.child(probed(row, &tag_cell)));
        }
        if motion != self.motion_at.get() {
            self.motion_at.set(motion);
            self.ticker.set(self.next_deadline());
        }

        // Speech bubble: the text field, a pending prompt, the reply (or the focus probe).
        let mut panel_geom: Option<PanelGeom> = None;
        self.buttons.borrow_mut().clear();
        // As shown: a turn in its quiet start (`session::QUIET_START`) offers no ABORT yet.
        let busy = self.session.as_ref().and_then(|sess| sess.shown.as_ref()).is_some_and(|b| b.busy);
        if mode.is_none() {
            self.panel_bottom = None;
            self.panel_rect.set(None);
        }
        if let Some(mode) = mode {
            let line_strings = |chars: &[char], w: &Wrapped| -> Vec<String> {
                w.lines.iter().map(|&(a, b)| chars[a..b.min(chars.len())].iter().collect::<String>().trim_end().to_string()).collect()
            };
            // (lines, pages, text width, all lines, header label, placeholder)
            let mut placeholder: Option<String> = None;
            let (lines, pages, text_w, total_lines, header_label): (Vec<String>, Option<(usize, usize)>, f32, usize, String) = match mode {
                PanelMode::Reply => {
                    let b = self.bubble.as_ref().expect("reply mode has a bubble");
                    // The reader open: the bubble is a stub (header + CLOSE READER), not a
                    // second copy of the text.
                    let lines = if self.reader.is_some() { Vec::new() } else { b.page_lines_marked() };
                    if lines.is_empty() && self.reader.is_none() {
                        placeholder = Some("…".into());
                    }
                    let label = match &self.echo {
                        _ if self.reader.is_some() => "IN THE READER".into(),
                        // A turn another session woke is theirs, not his.
                        Some(e) => match crate::session::origin(e) {
                            Some((who, _, text)) => format!("⇄ {who} › {}", crate::session::one_line(&text, 24)),
                            None => match crate::voice::spoken(e) {
                                Some(said) => format!("◉ YOU › {}", crate::session::one_line(said, 24)),
                                None => format!("YOU › {}", crate::session::one_line(e, 26)),
                            },
                        },
                        None => "INCOMING TRANSMISSION".into(),
                    };
                    (lines, Some(b.pages()), b.wrap.as_ref().map_or(260.0, |w| w.width), b.wrap.as_ref().map_or(1, |w| w.lines.len()), label)
                }
                PanelMode::Input { secret } => {
                    let i = self.input.as_ref().expect("input mode has a field");
                    let shown: Vec<char> = if secret { i.text.chars().map(|_| '•').collect() } else { i.text.chars().collect() };
                    let w = self.wrap_text(&shown, window, cx);
                    let all = line_strings(&shown, &w);
                    let lines: Vec<String> = all[all.len().saturating_sub(LINES_PER_PAGE)..].to_vec();
                    if i.text.is_empty() {
                        placeholder = Some(if secret { "type the answer (hidden)".into() } else { format!("say something to {}", self.name.to_lowercase()) });
                    }
                    let label = if secret { "SECRET ANSWER" } else { "TRANSMIT" };
                    let n = lines.len().max(1);
                    (lines, None, w.width.max(260.0), n, label.into())
                }
                PanelMode::Prompt => {
                    let u = self.prompt_ui.as_ref().expect("prompt mode has a prompt");
                    let chars: Vec<char> = u.view.text.chars().collect();
                    let w = self.wrap_text(&chars, window, cx);
                    let mut lines = line_strings(&chars, &w);
                    if lines.len() > LINES_PER_PAGE {
                        lines.truncate(LINES_PER_PAGE);
                        if let Some(l) = lines.last_mut() {
                            l.push('…');
                        }
                    }
                    let n = lines.len().max(1);
                    // More waiting: a count (all of them, whole, in the Line: $mod+c).
                    let more = self.session.as_ref().and_then(|s| s.shown.as_ref()).map_or(0, |b| b.prompts.len());
                    let title = crate::session::one_line(&u.view.title.to_uppercase(), 28);
                    (lines, None, w.width.max(260.0), n, if more > 1 { format!("{title} · 1 OF {more}") } else { title })
                }
                PanelMode::Probe => {
                    let text = self.focus_probe.clone().unwrap_or_default();
                    let body = if text.is_empty() { "click here, then type".to_string() } else { text };
                    (vec![body], None, 260.0, 1, "FOCUS PROBE // CLICK ME".into())
                }
            };
            let probe = mode == PanelMode::Probe;
            let (vis_l, vis_r) = (self.vis.0 as f32 / sf, self.vis.1 as f32 / sf);
            let max_w = (vis_r - vis_l - 2.0 * MARGIN).min(PANEL_MAX_W);
            // Never narrower than the header line (name plate, label .. glyph status).
            let header_w = PAD_L
                + PAD_R
                + self.tracked_width(&self.name, 10.5, FontWeight::BOLD, 1.8, window)
                + 16.0
                + 15.0
                + self.tracked_width(&header_label, 9.5, FontWeight::SEMIBOLD, 1.5, window)
                + self.tracked_width(&label, 9.5, FontWeight::BOLD, 1.5, window)
                + 3.0 * 8.0
                + 11.0
                + 5.0
                + 32.0;
            let min_w = PANEL_MIN_W.max(header_w).min(max_w);
            let mut w = (text_w + PAD_L + PAD_R + 2.0).clamp(min_w, max_w);
            if mode == PanelMode::Reply && self.reader.is_some() {
                // The stub (reader open): only as wide as its header; the reply's width is kept
                // for when the reader closes.
                w = min_w;
            } else if mode == PanelMode::Reply
                && let Some(b) = &mut self.bubble
            {
                // A streaming reply takes the full width at once: no growth mid-stream.
                w = if b.stream && b.more { max_w } else { w.max(b.width).min(max_w) };
                b.width = w;
            }
            // Fit the page: the lines it will hold once typed out (so it doesn't grow while
            // streaming), never an empty band above the footer.
            let stub = mode == PanelMode::Reply && self.reader.is_some();
            let rows = match (mode, &self.bubble) {
                _ if stub => 0,
                (PanelMode::Reply, Some(b)) => b.rows().max(1),
                _ => total_lines.clamp(1, LINES_PER_PAGE),
            };
            // A reply ending on a command chip with nothing else for the footer: the collapse key
            // sits on the chip's row, no empty band under it.
            let chip_last = mode == PanelMode::Reply
                && !stub
                && !busy
                && self.bubble.as_ref().is_some_and(|b| {
                    !b.long()
                        && b.wrap.as_ref().and_then(|w| w.lines.last()).is_some_and(|&(a, e)| {
                            let t: String = b.text[a.min(b.text.len())..e.min(b.text.len())].iter().collect();
                            crate::line::command(&t).is_some()
                        })
                });
            let footer = !probe && !chip_last;
            let mut h = PAD_T + HEADER_H + HEADER_GAP + rows as f32 * LINE_H + if footer { FOOTER_GAP + FOOTER_H } else { 0.0 } + PAD_B;
            if mode == PanelMode::Prompt
                && let Some(key) = self.prompt_ui.as_ref().map(|p| p.shown_at)
            {
                match self.prompt_h {
                    Some((k, held)) if k == key => h = h.max(held),
                    _ => {}
                }
                self.prompt_h = Some((key, h));
            }
            let x = (head_l - w * 0.45).clamp(vis_l + MARGIN, (vis_r - MARGIN - w).max(vis_l + MARGIN));
            let want = top_l - TIP_GAP - TAIL_H;
            let bottom = match self.panel_bottom {
                Some(b) if matches!(mode, PanelMode::Prompt | PanelMode::Input { .. }) => b,
                Some(b) => b.min(want),
                None => want,
            };
            self.panel_bottom = Some(bottom);
            let y = (bottom - h).max(TOP_MARGIN);
            // The blade's root: 28 px, offset toward the bubble's end it is nearer.
            let lo = x + CUT + 16.0;
            let hi = x + w - CUT - 26.0;
            let tail_x = head_l.clamp(lo, hi.max(lo));
            let g = PanelGeom { x, y, w, h, tail_x, tip: (head_l.clamp(tail_x - 9.0, tail_x + 9.0), y + h + TAIL_H) };
            self.panel_rect.set(Some((x, y, w, h + TAIL_H)));
            panel_geom = Some(g);
            // Settled after a summon: the border breathes toward bone; the pulse turns it bone.
            let frame_c = match glow {
                _ if pulse => pal.focus,
                Some(k) => theme::mix(pal.accent, pal.focus, k as f32 / 12.0),
                None => pal.accent,
            };
            let warm = pulse || glow.is_some();
            let (panel_c, ring_c, sliver_c) = (pal.panel, pal.ring, pal.accent);
            let bg = canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    // The dark ring outside the outline (the X shape is the ringed outline), the
                    // 2 px red edge inside it, then the mane slivers over the top edge.
                    theme::paint(window, &[g.poly()], Fill::framed(panel_c, frame_c, if warm { 3.0 } else { 2.0 }).ringed(ring_c));
                    for p in g.slivers() {
                        theme::fill_poly(window, &p, rgb(sliver_c));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();
            let prompt_mode = matches!(mode, PanelMode::Prompt);
            let header_label = if probe {
                if self.focus.is_focused(window) { "FOCUS PROBE // KEYS OK".to_string() } else { header_label }
            } else {
                header_label
            };
            let header = div()
                .absolute()
                .top(px(PAD_T))
                .left(px(PAD_L))
                .right(px(PAD_R))
                .h(px(HEADER_H))
                .flex()
                .items_center()
                .gap(px(8.0))
                .line_height(px(HEADER_H))
                // Her name on a red blade, ink type.
                .child(
                    theme::plate(Cut::Tab(5.0), Fill::solid(pal.accent))
                        .h(px(14.0))
                        .pl(px(7.0))
                        .pr(px(9.0))
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(pal.ink))
                        .child(tracked(&self.name, 1.8)),
                )
                .child(tracked(&header_label, 1.5).text_size(px(9.5)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(if prompt_mode { pal.text } else { pal.dim })))
                .child(div().flex_1())
                .child(if invert || pulse {
                    // The nameplate's own paint, on its first beat (NEEDS YOU: the bone
                    // swallowtail; ERROR: black, red type).
                    let hl = pal.look(plate_kind, &tone_key, tone, true, false);
                    let cut = if plate_kind == Plate::Flag || pulse { Cut::Swallow(4.0) } else { Cut::Tab(4.0) };
                    theme::plate(cut, Fill::framed(hl.fill, hl.edge, 1.5))
                        .h(px(14.0))
                        .pl(px(6.0))
                        .pr(px(9.0))
                        .gap(px(5.0))
                        .child(glyph(&glyph_kind, hl.mark, hl.fill))
                        .child(tracked(&label, 1.5).text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(rgb(hl.ink)))
                } else {
                    // Calm: a bone glyph, the state in text-dim (r21: a red dot read as
                    // "recording" or "offline").
                    let glyph_c = if tone == pal.accent { pal.text } else { tone };
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(glyph(&glyph_kind, glyph_c, pal.panel))
                        .child(theme::metered(&label, 1.5, pal.dim).text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(rgb(pal.dim)))
                });
            let caret = matches!(mode, PanelMode::Input { .. });
            let body_weight = if prompt_mode { FontWeight::BOLD } else { FontWeight::SEMIBOLD };
            let n_lines = lines.len();
            let caret_c = pal.focus;
            let mut body = div()
                .absolute()
                .top(px(PAD_T + HEADER_H + HEADER_GAP))
                .left(px(PAD_L))
                .right(px(PAD_R))
                .flex()
                .flex_col()
                .text_size(px(BODY_FONT))
                .line_height(px(LINE_H))
                .font_weight(body_weight)
                .text_color(rgb(pal.text));
            let caret_bar = move || div().ml(px(1.0)).w(px(2.0)).h(px(BODY_FONT)).bg(rgb(caret_c));
            if let Some(ph) = &placeholder {
                let mut row = div().h(px(LINE_H)).flex().items_center();
                if caret {
                    row = row.child(caret_bar());
                }
                body = body.child(row.child(div().ml(px(4.0)).text_color(rgb(pal.dim)).whitespace_nowrap().child(ph.clone())));
            } else {
                for (k, t) in lines.into_iter().enumerate() {
                    // A line that is one command (`synaps login`, typed out or whole): a chip,
                    // copied on a click.
                    if mode == PanelMode::Reply
                        && let Some(c) = crate::line::command(&t)
                    {
                        let copied = self.cmd_copied.as_deref() == Some(c);
                        let c = c.to_string();
                        let cmd_probes = self.buttons.clone();
                        let chip = div()
                            .child(canvas(move |b, _, _| cmd_probes.borrow_mut().push(("cmd", b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                            .child(theme::backdrop(Cut::Corners(4.0), Fill::framed(pal.code, pal.accent, 1.0)))
                            .relative()
                            .id("cmd")
                            .h(px(LINE_H - 4.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_pointer()
                            .font_family(MONO)
                            .text_size(px(BODY_FONT - 3.0))
                            .font_weight(FontWeight::NORMAL)
                            .child(div().text_color(rgb(pal.dim)).child("$"))
                            .child(div().text_color(rgb(pal.text)).child(c.clone()))
                            .child(
                                div()
                                    .ml(px(2.0))
                                    .pl(px(8.0))
                                    .border_l_1()
                                    .border_color(rgb(pal.rule))
                                    .text_size(px(12.5))
                                    .font_family(self.font.clone())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(if copied { pal.accent } else { pal.text }))
                                    .child(if copied { "copied ✓" } else { "copy" }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    lineview::copy_text(c.clone(), cx);
                                    this.cmd_copied = Some(c.clone());
                                    cx.notify();
                                }),
                            );
                        body = body.child(div().h(px(LINE_H)).flex().items_center().child(chip));
                        continue;
                    }
                    // A chip's opening backtick, typed out before its first letter: not shown.
                    let t = if mode == PanelMode::Reply && t.trim() == "`" { String::new() } else { t };
                    let mut row = div().h(px(LINE_H)).flex().items_center().whitespace_nowrap().child(t);
                    if caret && k + 1 == n_lines {
                        row = row.child(caret_bar());
                    }
                    body = body.child(row);
                }
            }
            let mut panel = div().id("bubble").absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).child(bg).child(header).child(body);
            // Slanted keys: a hollow red chevron key, DENY bone with the focus ring, the rest
            // (ALLOW, REVIEW, ANSWER) solid red with ink type. Unarmed, they sit back.
            let btn = |id: &'static str, dir: Chevron, on: bool| {
                let edge = if on { pal.accent } else { theme::mix(pal.accent, pal.panel, 0.7) };
                theme::plate(Cut::Slant(6.0), Fill::line(edge, 1.25))
                    .id(id)
                    .w(px(24.0))
                    .h(px(19.0))
                    .justify_center()
                    .child(chevron(dir, with_alpha(pal.text, if on { 0.95 } else { 0.3 })))
            };
            let probes = self.buttons.clone();
            let tbtn = |id: &'static str, text: &str, kind: Key, on: bool| {
                let probes = probes.clone();
                let back = |c: u32| if on { c } else { theme::mix(c, pal.panel, 0.6) };
                let (fill, ink) = match kind {
                    Key::Deny => (Fill::line(back(pal.focus), 1.5), back(pal.focus)),
                    Key::Go => (Fill::solid(back(pal.accent)), pal.ink),
                    Key::Plain => (Fill::line(back(pal.accent), 1.5), back(pal.accent)),
                };
                let ring_c = pal.focus;
                div()
                    .child(canvas(move |b, _, _| probes.borrow_mut().push((id, b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                    .child(theme::backdrop(Cut::Slant(6.0), fill))
                    .when(kind == Key::Deny && on, |d| {
                        // The focus ring: DENY is where a stray Enter lands.
                        d.child(
                            canvas(
                                |_, _, _| (),
                                move |b, _, window, _| {
                                    for p in theme::outline(Cut::Slant(6.0), f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom())) {
                                        theme::paint(window, &[theme::offset(&p, 3.0)], Fill::line(ring_c, 1.25));
                                    }
                                },
                            )
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full(),
                        )
                    })
                    .relative()
                    .id(id)
                    .h(px(19.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(px(10.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(ink))
                    .child(tracked(text, 1.2))
            };
            let abort_btn = |cx: &mut Context<Self>| {
                tbtn("abort", "✕ ABORT", Key::Plain, true).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.abort();
                        cx.notify();
                    }),
                )
            };
            let collapse_btn = |cx: &mut Context<Self>| {
                btn("collapse", Chevron::Down, true).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if this.prompt_ui.as_ref().is_some_and(|p| !p.dismissed) && this.input.is_none() {
                            if let Some(p) = &mut this.prompt_ui {
                                p.dismissed = true;
                            }
                        } else if let Some(b) = &mut this.bubble {
                            b.collapsed = true;
                        }
                        cx.notify();
                        this.schedule(cx);
                    }),
                )
            };
            let mut row = div().absolute().bottom(px(PAD_B)).right(px(PAD_R - 4.0)).h(px(FOOTER_H)).flex().items_center().gap(px(4.0));
            match mode {
                PanelMode::Reply => {
                    // More than a page: where it is (‹ 2/3 ›), bottom left; the arrows page.
                    let rows_shown = self.bubble.as_ref().map_or(1, |b| b.rows().max(1));
                    if let Some((top, total)) = pages.filter(|_| !stub)
                        && total > rows_shown
                    {
                        let n = total.div_ceil(rows_shown);
                        let cur = if top + rows_shown >= total { n } else { top / rows_shown + 1 };
                        let arrow = |id: &'static str, glyph_s: &'static str, live: bool, d: i32| {
                            div()
                                .id(id)
                                .min_w(px(19.0))
                                .h(px(19.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if live { rgb(pal.dim) } else { with_alpha(pal.dim, 0.3) })
                                .child(glyph_s)
                                .when(live, |e| {
                                    e.cursor_pointer().on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.page(d, Instant::now());
                                            cx.notify();
                                            this.schedule(cx);
                                        }),
                                    )
                                })
                        };
                        panel = panel.child(
                            div()
                                .absolute()
                                .bottom(px(PAD_B))
                                .left(px(PAD_L - 4.0))
                                .h(px(FOOTER_H))
                                .flex()
                                .items_center()
                                .gap(px(2.0))
                                .child(arrow("page-up", "‹", cur > 1, -1))
                                .child(div().px(px(3.0)).text_size(px(11.5)).text_color(rgb(pal.dim)).child(format!("{cur}/{n}")))
                                .child(arrow("page-down", "›", cur < n, 1)),
                        );
                    }
                    if busy {
                        row = row.child(abort_btn(cx)).child(div().w(px(4.0)));
                    }
                    let open = self.reader.is_some();
                    if let Some(b) = self.bubble.as_ref().filter(|b| b.long()) {
                        // The bubble's one primary control: a bone slanted pill, ink type and a
                        // red arrow (17:1; r21: ink on red was too weak, and the red pager
                        // beside it read as a second way on; the pager is text-dim now). While
                        // the reader is open the bubble is just this, full width: the control
                        // for the panel beside her, no empty body.
                        let label = if open { format!("CLOSE READER · {} LINES", b.read_lines()) } else { format!("READ · {} LINES", b.read_lines()) };
                        let read_probes = self.buttons.clone();
                        let (fill_c, ink_c, arrow_c) = (pal.text, pal.ink, pal.deep);
                        if open {
                            row = row.left(px(PAD_L));
                        }
                        let chip = div()
                            .when(open, |d| d.flex_1().justify_center())
                            .child(canvas(move |b, _, _| read_probes.borrow_mut().push(("read", b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                            .child(theme::backdrop(Cut::Slant(7.0), Fill::solid(fill_c)))
                            .relative()
                            .id("read")
                            .h(px(FOOTER_H))
                            .px(px(14.0))
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .text_color(rgb(ink_c))
                            .child(glyph(if open { "back" } else { "play" }, arrow_c, fill_c))
                            .child(tracked(&label, 1.3));
                        row = row
                            .child(chip.on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    if this.reader.is_some() {
                                        this.close_reader(true, cx);
                                    } else {
                                        this.open_reader(true, cx);
                                    }
                                    cx.notify();
                                    this.schedule(cx);
                                }),
                            ))
                            .when(!open, |d| d.child(div().w(px(4.0))));
                    }
                    if !open {
                        row = row.child(collapse_btn(cx));
                    }
                }
                PanelMode::Input { secret } => {
                    let i = self.input.as_ref().expect("input mode has a field");
                    let note = match &self.hint {
                        Some((h, _)) => h.clone(),
                        None if secret => "ENTER ANSWER · ESC LATER".into(),
                        None if i.esc_at.is_some() => "ESC AGAIN · ABORT".into(),
                        None if busy => "ENTER STEER · ESC ESC ABORT".into(),
                        None => "ENTER SEND · ESC CLOSE".into(),
                    };
                    let warn = self.hint.is_some() || i.esc_at.is_some();
                    panel = panel.child(
                        div()
                            .absolute()
                            .bottom(px(PAD_B))
                            .left(px(PAD_L))
                            .h(px(FOOTER_H))
                            .flex()
                            .items_center()
                            .child(tracked(&note, 1.3).text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(rgb(if warn { pal.accent } else { pal.dim }))),
                    );
                    if busy && !secret {
                        row = row.child(abort_btn(cx));
                    }
                }
                PanelMode::Prompt => {
                    let u = self.prompt_ui.as_ref().expect("prompt mode has a prompt");
                    let armed = now >= u.shown_at + PROMPT_ARM;
                    let (secret, risky) = (u.view.secret, crate::line::risky(&u.view));
                    // DENY first. A risky one (a tool activation, a tool that runs or changes
                    // things) has no one-click ALLOW here: REVIEW opens the Line, where it
                    // takes Shift+A or a held A.
                    if !secret {
                        row = row.child(tbtn("deny", "DENY", Key::Deny, armed).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if this.prompt_ui.as_ref().is_some_and(|u| Instant::now() >= u.shown_at + PROMPT_ARM) {
                                    this.answer(None);
                                    cx.notify();
                                    this.schedule(cx);
                                }
                            }),
                        ));
                    }
                    if secret {
                        row = row.child(tbtn("answer", "ANSWER", Key::Go, armed).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                let Some(u) = &this.prompt_ui else { return };
                                if Instant::now() >= u.shown_at + PROMPT_ARM {
                                    let id = u.view.id;
                                    this.open_input(Some(id), window, cx);
                                }
                            }),
                        ));
                    } else if risky {
                        row = row.child(tbtn("review", "REVIEW ▸", Key::Go, armed).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if this.session.is_some() {
                                    let r = this.open_line(cx);
                                    elog!("synaps-avatar: REVIEW clicked: {r}");
                                }
                            }),
                        ));
                    } else {
                        row = row.child(tbtn("allow", "ALLOW", Key::Go, armed).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if this.prompt_ui.as_ref().is_some_and(|u| Instant::now() >= u.shown_at + PROMPT_ARM) {
                                    this.answer(Some("y".into()));
                                    cx.notify();
                                    this.schedule(cx);
                                }
                            }),
                        ));
                    }
                    if secret {
                        row = row.child(tbtn("deny", "DENY", Key::Deny, armed).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if this.prompt_ui.as_ref().is_some_and(|u| Instant::now() >= u.shown_at + PROMPT_ARM) {
                                    this.answer(None);
                                    cx.notify();
                                    this.schedule(cx);
                                }
                            }),
                        ));
                    }
                    row = row.child(div().w(px(4.0))).child(collapse_btn(cx));
                }
                PanelMode::Probe => {}
            }
            if footer {
                panel = panel.child(row);
            } else if chip_last {
                // The collapse key on the chip's row, at its right.
                panel = panel.child(div().absolute().bottom(px(PAD_B + 2.0)).right(px(PAD_R - 4.0)).h(px(LINE_H - 4.0)).flex().items_center().child(collapse_btn(cx)));
            }
            if mode == PanelMode::Reply {
                panel = panel
                    .on_scroll(cx.listener(|this, ev: &ScrollEvent, _, cx| {
                        let dy = f32::from(ev.delta.pixel_delta(px(LINE_H)).y);
                        let acc = this.wheel.get() - dy / LINE_H;
                        let n = acc.trunc() as i32;
                        this.wheel.set(acc - n as f32);
                        if n != 0 {
                            this.scroll_bubble(n, Instant::now());
                            cx.notify();
                            this.schedule(cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if this.skip_typing(Instant::now()) {
                                cx.notify();
                                this.schedule(cx);
                            }
                        }),
                    );
            }
            if matches!(mode, PanelMode::Input { .. } | PanelMode::Probe) {
                panel = panel
                    .track_focus(&self.focus)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let r = this.os.grab_keyboard(this.os.body());
                            window.focus(&this.focus, cx);
                            if this.focus_probe.is_some() {
                                let fw = this.os.focused_window().unwrap_or(0);
                                elog!("focus-probe: SetInputFocus err={:?}; X focus now 0x{fw:x} (ours 0x{:x})", r.err(), this.os.body());
                            }
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        if this.input.is_some() {
                            this.input_key(ev, window, cx);
                            return;
                        }
                        elog!("focus-probe: key down `{}` char {:?}", ev.keystroke.key, ev.keystroke.key_char);
                        if let Some(t) = &mut this.focus_probe {
                            match ev.keystroke.key.as_str() {
                                "backspace" => {
                                    t.pop();
                                }
                                "enter" => t.push(' '),
                                _ => {
                                    if let Some(c) = &ev.keystroke.key_char {
                                        t.push_str(c);
                                    }
                                }
                            }
                        }
                        cx.notify();
                    }));
            }
            root = root.child(panel);
        }

        // Context menu.
        if self.menu.is_none() {
            self.menu_place = None;
        }
        if let Some((at, _)) = self.menu.filter(|_| hud) {
            // Red items; the hovered one on a red blade with ink type. QUIT? (armed) is a bone
            // blade: danger.
            let (accent_c, ink_c, danger_c) = (pal.accent, pal.ink, pal.danger);
            let blade = |c: u32| div().absolute().top_0().left_0().size_full().child(theme::backdrop(Cut::Tab(8.0), Fill::solid(c)));
            let item_as = |label: &'static str, armed: bool| {
                let group = SharedString::from(format!("menu-{label}"));
                div()
                    .group(group.clone())
                    .relative()
                    .px(px(10.0))
                    .py(px(5.0))
                    .text_size(px(11.0))
                    .font_weight(if armed { FontWeight::BOLD } else { FontWeight::SEMIBOLD })
                    .text_color(rgb(if armed { ink_c } else { accent_c }))
                    .when(armed, |d| d.child(blade(danger_c)))
                    .when(!armed, |d| d.hover(|s| s.text_color(rgb(ink_c))).child(blade(accent_c).invisible().group_hover(group, |s| s.visible())))
                    .child(tracked(label, 1.2))
            };
            let item = |label: &'static str| item_as(label, false);
            let mark = self.menu_mark(window);
            // Under the click if it fits in her window, else as far up as it must (a perched
            // her: a click low on her body cut the menu's bottom rows off). Before its first
            // paint, its height is estimated from its rows.
            let rows = 6 + usize::from(self.attention_label().is_some()) * 2 + usize::from(self.voice_on() && self.voice.cfg.handsfree);
            let header_on = self.stills.as_ref().is_some_and(|s| s.capri.is_some());
            // Its height as painted for this set of rows: placed once that is known (a menu
            // never painted with these rows is laid out unseen for a frame and measured).
            let sig = (rows as u32) << 2 | u32::from(mark.is_some()) << 1 | u32::from(header_on);
            let measured = self.menu_h.get().0 == sig;
            // Her work area (under the bar, on her monitor), window-local logical px: the menu's
            // top left stays within it less the menu's size.
            let o = self.origin;
            let (mut x0, mut y0) = (f32::from(l.lp(self.vis.0)), 0.0f32);
            let (mut x1, mut y1) = (f32::from(l.lp(self.vis.1)), f32::from(l.lp(l.win_h)));
            if let Some(a) = self.her_area() {
                x0 = x0.max((a.x - o.0) as f32 / sf);
                y0 = y0.max((a.y - o.1) as f32 / sf);
                x1 = x1.min((a.x + a.w - o.0) as f32 / sf);
                y1 = y1.min((a.y + a.h - o.1) as f32 / sf);
            }
            let h = self.menu_h.get().1;
            let (xr, yr) = ((x0 + 4.0, x1 - MENU_W - 4.0), (y0 + 4.0, y1 - h - 4.0));
            // Her face as drawn now: the menu keeps off it.
            let face = self.frame_or_default(self.anim.frame()).and_then(|f| f.face_box).map(|(fx0, fy0, fx1, fy1)| {
                let (ax, ay) = (l.avatar_x as f32, l.avatar_y as f32);
                ((ax + fx0 as f32) / sf, (ay + fy0 as f32) / sf, (ax + fx1 as f32) / sf, (ay + fy1 as f32) / sf)
            });
            let place = match self.menu_place {
                // Placed; a pose change that put her face under it moves it once, to the far side.
                Some((a, x, y, moved)) if a == at => match menu_reflow((x, y), (MENU_W, h), xr, face).filter(|_| !moved) {
                    Some(p) => {
                        self.menu_place = Some((at, p.0, p.1, true));
                        Some(p)
                    }
                    None => Some((x, y)),
                },
                _ if measured => {
                    let p = menu_at((f32::from(at.x), f32::from(at.y)), (MENU_W, h), xr, yr, face);
                    self.menu_place = Some((at, p.0, p.1, false));
                    Some(p)
                }
                _ => None,
            };
            // lachy's capri.png over the items: who this menu is.
            let header = self.stills.as_ref().and_then(|s| s.capri.clone()).map(|i| {
                let s = i.size(0);
                let w = MENU_HEADER_PX * u32::from(s.width) as f32 / (u32::from(s.height) as f32).max(1.0);
                div().flex().justify_center().pt(px(6.0)).pb(px(2.0)).child(img(ImageSource::Render(i)).w(px(w)).h(px(MENU_HEADER_PX)))
            });
            let menu = div()
                .relative()
                .w(px(MENU_W))
                .flex()
                .flex_col()
                .py(px(4.0))
                .child(theme::backdrop(MENU_CUT, Fill::framed(pal.panel, pal.accent, 2.0).ringed(pal.ring)))
                .children(header)
                .child(item("RESET POSITION").id("reset").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.reset_position(cx);
                    }),
                ))
                .when_some(self.attention_label(), |m, att| {
                    let dnd = self.dnd_on();
                    m.child(item(if dnd { "DND OFF" } else { "DND 1 HOUR" }).id("dnd").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.menu = None;
                            this.toggle_dnd();
                            cx.notify();
                            this.schedule(cx);
                        }),
                    ))
                    .child(item(match att {
                        "MIX" => "ATTENTION: MIX ▸",
                        "HANG ONLY" => "ATTENTION: HANG ONLY ▸",
                        _ => "ATTENTION: OFF ▸",
                    })
                    .id("attention")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.cycle_attention();
                            if let Some(m) = &mut this.menu {
                                m.1 = Instant::now() + Duration::from_secs(8);
                            }
                            cx.notify();
                            this.schedule(cx);
                        }),
                    ))
                })
                .when(self.voice_on() && self.voice.cfg.handsfree, |m| {
                    let on = self.voice.hf.is_some();
                    m.child(item(if on { "END HANDS-FREE" } else { "HANDS-FREE TALK" }).id("handsfree").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.menu = None;
                            let r = if on {
                                this.voice_hf_end("HANDS-FREE OFF", cx);
                                "ok hands-free off".to_string()
                            } else {
                                this.voice_hf_start(cx)
                            };
                            elog!("synaps-avatar: menu hands-free: {r}");
                            cx.notify();
                            this.schedule(cx);
                        }),
                    ))
                })
                .child(item("COPY LAST REPLY").id("copy").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.menu = None;
                        let r = this.copy_last_reply(cx);
                        elog!("synaps-avatar: menu copy: {r}");
                        cx.notify();
                        this.schedule(cx);
                    }),
                ))
                .child(item("HIDE").id("hide").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.park(Instant::now());
                        cx.notify();
                        this.schedule(cx);
                    }),
                ))
                .child(div().h(px(1.0)).mx(px(10.0)).my(px(3.0)).bg(rgb(pal.rule)))
                .child(
                    item_as(if self.quit_armed.is_some() { "QUIT? CLICK AGAIN" } else { "QUIT" }, self.quit_armed.is_some()).id("quit").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let now = Instant::now();
                            if this.quit_armed.is_some_and(|t| now < t) {
                                this.quit_armed = None;
                                this.vanish_quit(now);
                            } else {
                                // A second click within 2 s exits; the menu stays up that long.
                                this.quit_armed = Some(now + Duration::from_secs(2));
                                if let Some(m) = &mut this.menu {
                                    m.1 = m.1.max(now + Duration::from_secs(3));
                                }
                            }
                            cx.notify();
                            this.schedule(cx);
                        }),
                    ),
                )
                // Her maker's mark: what runs her, for whoever wonders. Dim, small, not a button;
                // under a gap and a hairline, a footer.
                .when_some(mark, |m, mark| {
                    m.child(div().h(px(1.0)).mx(px(10.0)).mt(px(6.0)).bg(rgb(theme::mix(pal.rule, pal.panel, 0.4))))
                    .child(
                        div()
                            .px(px(10.0))
                            .pt(px(5.0))
                            .pb(px(3.0))
                            .text_size(px(MARK_PX))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(pal.dim))
                            .child(tracked(&mark, 0.8)),
                    )
                });
            let (menu_x, menu_y) = place.unwrap_or((f32::from(at.x), f32::from(at.y)));
            menu_unseen = place.is_none();
            menu_sig = sig;
            root = root.child(div().absolute().left(px(menu_x)).top(px(menu_y)).when(menu_unseen, |d| d.invisible()).child(probed(menu, &menu_cell)));
        }

        // Last: hearts, then the X input/bounding shape for exactly what this frame draws.
        let (ax, ay) = (l.avatar_x as i16, l.avatar_y as i16);
        let os = self.os.clone();
        let last = self.last_shape.clone();
        let hist = self.shape_hist.clone();
        let first = self.first_frame.clone();
        let t_start = self.t_start;
        let ground_px = (l.ground_x as i16, (l.ground_y - 1) as i16, 1u16, 1u16);
        let hearts = self.hearts.filter(|_| hud).map(|(t0, at)| (now.saturating_duration_since(t0).as_secs_f32() * 1000.0, at));
        // The hearts stay inside her work area (under the bar, on screen).
        if let Some((t0, _)) = self.hearts
            && self.hearts_area.0 != Some(t0)
        {
            self.hearts_area = (Some(t0), self.her_area().map(|a| a.y));
        }
        let hearts_ceiling = self.hearts_area.1.map(|y| (y - self.origin.1) as f32 / sf);
        let (panel_last, tag_last) = (self.panel_last.clone(), self.tag_last.clone());
        let worigin = self.origin;
        let applied = self.applied;
        // A press on her: the whole window takes the pointer until the release (`press_takes_window`).
        let held_all = self.press.is_some() && self.os.press_takes_window();
        let win_wh = (l.win_w, l.win_h);
        let tag_clickable = collapsed || self.session.is_some();
        let tag_w = self.tag_w.clone();
        let menu_h = self.menu_h.clone();
        let tag_box = self.tag_box.clone();
        root.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let mut visible: Vec<ShapeRect> = body_shape.iter().map(|r| (r.0 + ax, r.1 + ay, r.2, r.3)).collect();
                    // In the air she can be caught where she is drawn.
                    let mut input: Vec<ShapeRect> = if hidden {
                        Vec::new()
                    } else if held_all {
                        vec![(0, 0, win_wh.0 as u16, win_wh.1 as u16)]
                    } else if rag_flight {
                        visible.clone()
                    } else {
                        rects.iter().map(|r| (r.0 + ax, r.1 + ay, r.2, r.3)).collect()
                    };
                    let dev = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| (x * sf, y * sf)).collect::<Vec<_>>();
                    // What was on screen last frame is what shows inside this frame's shape
                    // (present lag): a panel goes into the shape only where it was painted.
                    let prev = panel_last.replace(panel_geom);
                    if let Some(g) = panel_geom {
                        input.push(to_device(Bounds::new(point(px(g.x), px(g.y)), size(px(g.w), px(g.h))), sf));
                        let same_column = |p: &PanelGeom| p.x == g.x && p.w == g.w && p.tail_x == g.tail_x && p.y + p.h == g.y + g.h;
                        let shown = match prev {
                            // Alpha mode: no shape to lag behind; the rects are only the record.
                            _ if alpha => Some(g),
                            Some(p) if p == g => Some(g),
                            // Grown or shrunk upward: the smaller of the two was painted both times.
                            Some(p) if same_column(&p) => Some(if p.h < g.h { p } else { g }),
                            _ => None,
                        };
                        match shown {
                            Some(s) => {
                                for p in s.outline() {
                                    shape::poly_rows(&dev(&p), &mut visible);
                                }
                            }
                            None => window.request_animation_frame(),
                        }
                        if shown != Some(g) {
                            window.request_animation_frame();
                        }
                    }
                    if let Some(b) = menu_cell.get() {
                        menu_h.set((menu_sig, f32::from(b.size.height)));
                    }
                    if menu_unseen {
                        // Measured: placed and shown next frame.
                        window.request_animation_frame();
                    } else if let Some(b) = menu_cell.get() {
                        input.push(to_device(b, sf));
                        for p in theme::outline(MENU_CUT, f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom())) {
                            shape::poly_rows(&dev(&theme::offset(&p, theme::RING)), &mut visible);
                        }
                    }
                    let (prev_tag, run) = tag_last.get();
                    let run = if prev_tag.is_some() && prev_tag == tag_cell.get() { run.saturating_add(1) } else { 0 };
                    tag_last.set((tag_cell.get(), run));
                    if let Some(b) = tag_cell.get() {
                        tag_w.set(f32::from(b.size.width));
                        tag_box.set(Some(b));
                        // New or moved: in the shape only once two frames have painted it (GPUI
                        // presents up to two frames late: one frame of lag left a black tag at
                        // a landing). Torn (a glitch) included.
                        if run < 2 && !alpha {
                            window.request_animation_frame();
                            // Moved a little (her bob steps it a pixel): where it was and is now
                            // both painted it, so that stays (the whole tag blinked out for two
                            // frames on every bob step).
                            // Only a step of a px or two: a swap's move (a lounge) left the old
                            // plate's pixels in the new rows, a doubled, torn tag (r36).
                            if !torn
                                && let Some(p) = prev_tag
                                && p.size == b.size
                                && (f32::from(p.origin.x) - f32::from(b.origin.x)).abs() <= 2.0
                                && (f32::from(p.origin.y) - f32::from(b.origin.y)).abs() <= 2.0
                                && let Some((cut, trail)) = tag_cut
                            {
                                let i = p.intersect(&b);
                                if f32::from(i.size.height) >= f32::from(b.size.height) * 0.5 && f32::from(i.size.width) > 0.0 {
                                    visible.extend(rows_and(&tag_rows(cut, trail, p, sf), &tag_rows(cut, trail, b, sf)));
                                }
                            }
                        } else if torn {
                            visible.extend(tag_strips.iter().copied());
                        } else if let Some((cut, trail)) = tag_cut {
                            // The plate's own outline (a slant, a point, a swallowtail), not its box.
                            visible.extend(tag_rows(cut, trail, b, sf));
                        }
                        if tag_clickable {
                            input.push(to_device(b, sf));
                        }
                    }
                    // Hearts are solid-colour shaped X windows, like the teleport bolt, not
                    // pixels in this window. This ARGB window is implicitly redirected (its
                    // depth differs from the root's), and presents only reach the backing
                    // pixmap inside the bounding shape. So wherever the shape grows, stale
                    // pixmap content shows (dark blobs on rising hearts) until the next present
                    // lands. The server paints a window's background together with its shape.
                    let heart_win = |layer: usize, over: Option<usize>, rgb: u32, pts: &[(f32, f32)]| {
                        let root: Vec<(f32, f32)> =
                            pts.iter().map(|&(x, y)| (x * sf + worigin.0 as f32, y * sf + worigin.1 as f32)).collect();
                        let x0 = root.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() as i32;
                        let y0 = root.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
                        let x1 = root.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() as i32;
                        let y1 = root.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
                        let local: Vec<(f32, f32)> = root.iter().map(|&(x, y)| (x - x0 as f32, y - y0 as f32)).collect();
                        let mut r = Vec::new();
                        shape::poly_rows(&local, &mut r);
                        match over {
                            Some(j) => os.fx_set_below(layer, j, rgb, (x0, y0), (x1 - x0, y1 - y0), &r).ok(),
                            None => os.fx_set(layer, rgb, (x0, y0), (x1 - x0, y1 - y0), &r).ok(),
                        };
                    };
                    // Rise from the click to clear air above the hair, never past the work area's
                    // top (the bar, the screen edge).
                    let target = (top_l - 22.0).max(16.0).max(hearts_ceiling.map_or(f32::MIN, |c| c + 3.0 * 16.0 + 26.0));
                    for (i, (dx, size, delay, colour)) in HEARTS.iter().enumerate() {
                        let (rim, fill) = (HEART_LAYER + 2 * i, HEART_LAYER + 2 * i + 1);
                        let geom = hearts.and_then(|(t, at)| {
                            let (ox, oy) = (f32::from(at.x), f32::from(at.y));
                            let p = ((t - delay) / 950.0).clamp(0.0, 1.0);
                            if p <= 0.0 || p >= 1.0 {
                                return None;
                            }
                            let ease = 1.0 - (1.0 - p).powi(3);
                            let x = ox + dx * ease;
                            let y = oy + (target - i as f32 * 16.0 - oy) * ease;
                            let scale = if p < 0.15 {
                                0.3 + 0.8 * p / 0.15
                            } else if p < 0.72 {
                                1.1 - 0.1 * (p - 0.15) / 0.57
                            } else {
                                1.0 - (p - 0.72) / 0.28
                            };
                            (scale >= 0.1).then_some((x, y, size * scale))
                        });
                        match geom {
                            Some((x, y, s)) => {
                                heart_win(fill, None, *colour, &shape::heart(x, y, s));
                                heart_win(rim, Some(fill), 0xffffff, &shape::heart(x, y, s + 5.0));
                            }
                            None => {
                                os.fx_unmap(rim);
                                os.fx_unmap(fill);
                            }
                        }
                    }
                    os.flush();
                    if ragview::atomic_log() {
                        let b = shape::bbox(&visible).unwrap_or_default();
                        elog!("atomic paint {:.6} {},{} {},{},{},{}", ragview::mono_s(), applied.0, applied.1, b.0, b.1, b.2, b.3);
                    }
                    // When her window paints the effects: the ones over her, last.
                    os.paint_fx(window, false, sf);
                    // Never an empty bounding shape: X then reports the window fully obscured
                    // and GPUI stops drawing it for good.
                    if visible.is_empty() {
                        visible.push(ground_px);
                    }
                    // P6 sweep (measurement switches): one box, or rows merged within `tol` px
                    // (inward only: never more than what was painted).
                    if let Some(tol) = motion_tol {
                        visible = crate::shape::merge_tol(&visible, tol);
                    }
                    if shape_box() {
                        visible = crate::shape::bbox(&visible).into_iter().collect();
                    }
                    let mut last = last.borrow_mut();
                    let (di, dv) = (last.0 != input, last.1 != visible);
                    if di || dv {
                        os.set_shapes(di.then_some(&input[..]), dv.then_some(&visible[..])).ok();
                        if dv {
                            let mut h = hist.borrow_mut();
                            h.swap(0, 1);
                            h[0] = std::mem::take(&mut last.1);
                        }
                        *last = (input, visible);
                    }
                    if has_frame && !first.get() {
                        first.set(true);
                        elog!("first-frame: painted {} ms after process start", t_start.elapsed().as_millis());
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

/// The drawing a render painted, and the one it laid underneath ahead (see `drawn_last`).
struct DrawnLast {
    img: Arc<RenderImage>,
    at: ImageBox,
    rects: Arc<Vec<ShapeRect>>,
    next: Option<Arc<RenderImage>>,
}

/// Index of the pre-rotated note variant nearest `deg`.
fn frames_angle(deg: f32) -> usize {
    crate::frames::NOTE_ANGLES
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - deg).abs().total_cmp(&(b.1 - deg).abs()))
        .map_or(3, |p| p.0)
}

/// `SA_SHAPE_TOL=n`: the bounding shape's rows merged within n px (P6 sweep).
fn shape_tol() -> Option<i32> {
    static T: std::sync::OnceLock<Option<i32>> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var("SA_SHAPE_TOL").ok().and_then(|v| v.parse().ok()).filter(|&t| t > 0))
}

/// `SA_SHAPE_BOX=1`: the bounding shape as one box (P6 sweep: numbers only, she shows a box).
fn shape_box() -> bool {
    static B: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *B.get_or_init(|| std::env::var("SA_SHAPE_BOX").is_ok_and(|v| v == "1"))
}

/// `a` → `b` by `k` (0..1), per channel.
fn mix_rgb(a: u32, b: u32, k: f32) -> u32 {
    let ch = |sh: u32| {
        let (x, y) = (((a >> sh) & 0xff) as f32, ((b >> sh) & 0xff) as f32);
        ((x + (y - x) * k).round() as u32 & 0xff) << sh
    };
    ch(16) | ch(8) | ch(0)
}

/// Pending once, so a frame can run between two event batches.
struct YieldNow(bool);

impl std::future::Future for YieldNow {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if std::mem::replace(&mut self.0, true) {
            return std::task::Poll::Ready(());
        }
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

#[cfg(test)]
mod menu_tests {
    use super::menu_at;

    // A 500×695 window, her face at 190..240 × 300..340, a 210×300 menu.
    const FACE: Option<(f32, f32, f32, f32)> = Some((190.0, 300.0, 240.0, 340.0));
    const SIZE: (f32, f32) = (210.0, 300.0);
    const X: (f32, f32) = (0.0, 280.0);
    const Y: (f32, f32) = (0.0, 391.0);

    #[test]
    fn under_the_click_when_it_fits_and_misses_her_face() {
        assert_eq!(menu_at((60.0, 360.0), SIZE, X, (0.0, 500.0), FACE), (60.0, 360.0), "under her face: as clicked");
        assert_eq!(menu_at((100.0, 50.0), SIZE, X, Y, None), (100.0, 50.0));
    }

    #[test]
    fn moved_up_over_her_face_it_goes_beside_it() {
        // Clicked on her face's right half: at the click it would cover it.
        let (x, y) = menu_at((220.0, 200.0), SIZE, X, Y, FACE);
        assert_eq!((x, y), (248.0, 200.0), "right of her face, the click's side");
        // Clicked left of her face with no room left of it: the right side.
        assert_eq!(menu_at((150.0, 200.0), SIZE, X, Y, FACE), (248.0, 200.0));
        // Room on the left (a wide window): the click's side.
        assert_eq!(menu_at((150.0, 200.0), SIZE, (-100.0, 280.0), Y, FACE), (-28.0, 200.0));
    }

    #[test]
    fn no_room_beside_it_goes_under_then_above_then_over() {
        let narrow = (0.0, 150.0);
        assert_eq!(menu_at((140.0, 200.0), SIZE, narrow, (0.0, 600.0), FACE), (140.0, 348.0), "under her face");
        assert_eq!(menu_at((140.0, 700.0), (210.0, 200.0), narrow, (0.0, 300.0), FACE), (140.0, 92.0), "moved up over it, above it");
        assert_eq!(menu_at((140.0, 200.0), SIZE, narrow, (0.0, 300.0), FACE), (140.0, 200.0), "over it: nothing else fits");
    }

    #[test]
    fn a_face_that_moves_under_it_sends_it_to_the_far_side() {
        use super::menu_reflow;
        // Placed left of her face (at 30); her pose moves the face onto its right half.
        let moved = Some((200.0, 300.0, 230.0, 330.0));
        assert_eq!(menu_reflow((30.0, 200.0), SIZE, X, moved), Some((238.0, 200.0)), "to the face's right, same height");
        // Onto its left half: across it is its left, if the work area has room; else its right.
        let left = Some((60.0, 300.0, 90.0, 330.0));
        assert_eq!(menu_reflow((40.0, 200.0), SIZE, (-200.0, 280.0), left), Some((-158.0, 200.0)));
        assert_eq!(menu_reflow((40.0, 200.0), SIZE, X, left), Some((98.0, 200.0)), "no room across: the near side");
        assert_eq!(menu_reflow((40.0, 200.0), SIZE, (0.0, 50.0), left), None, "no room either side: it stays");
        // Not under it, or no face: it stays.
        assert_eq!(menu_reflow((250.0, 200.0), SIZE, X, FACE), None);
        assert_eq!(menu_reflow((30.0, 200.0), SIZE, X, None), None);
    }

    #[test]
    fn it_stays_in_her_work_area() {
        // The window reaches off the screen's left edge and above the bar: the menu doesn't.
        let (x, y) = menu_at((5.0, 2.0), (210.0, 200.0), (40.0, 280.0), (76.0, 391.0), None);
        assert_eq!((x, y), (40.0, 76.0));
        // Beside her face only where the work area has room: left is off it, so right.
        assert_eq!(menu_at((200.0, 300.0), SIZE, (40.0, 280.0), Y, FACE), (248.0, 300.0));
        // Nothing beside, nothing under: above her face, not above the bar.
        assert_eq!(menu_at((140.0, 300.0), (210.0, 200.0), (0.0, 150.0), (120.0, 300.0), FACE), (140.0, 300.0), "above would cross the bar: over it");
    }
}
