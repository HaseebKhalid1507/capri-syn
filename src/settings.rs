//! What Haseeb allows (the pack says what she *can* do): `~/.config/synaps-avatar/settings.toml`.
//! Every key is optional. Read at start and by `settings reload`; a bad file is logged and the
//! old values stay. Her Settings page and the right-click ATTENTION line write keys back
//! (`Settings::write`): only those keys change, his comments and layout stay, and a file she
//! can't read or parse is never written over.

use crate::brain::{Approach, Arrival};
use anyhow::{Context as _, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Attention {
    /// The approach weights below.
    #[default]
    Mix,
    /// Only `hang` (then the fallbacks).
    HangOnly,
    /// No attention bids; session summons still come.
    Off,
}

impl Attention {
    pub fn key(self) -> &'static str {
        match self {
            Attention::Mix => "mix",
            Attention::HangOnly => "hang_only",
            Attention::Off => "off",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Attention::Mix => "MIX",
            Attention::HangOnly => "HANG ONLY",
            Attention::Off => "OFF",
        }
    }
    pub fn parse(s: &str) -> Option<Attention> {
        match s.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "mix" => Some(Attention::Mix),
            "hang_only" | "hang" => Some(Attention::HangOnly),
            "off" => Some(Attention::Off),
            _ => None,
        }
    }
    /// MIX → HANG ONLY → OFF → MIX.
    pub fn next(self) -> Attention {
        match self {
            Attention::Mix => Attention::HangOnly,
            Attention::HangOnly => Attention::Off,
            Attention::Off => Attention::Mix,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fullscreen {
    /// Glitch out while a fullscreen window is on her monitor, back in after.
    #[default]
    Hide,
    /// Stay, but never move.
    Stay,
}

/// Which monitors her passive hops may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorPolicy {
    /// The one he is working on (the focused window's, else the cursor's).
    Follow,
    /// Any monitor (she wanders).
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrainSettings {
    pub enabled: bool,
    pub approaches: Vec<(Approach, f64)>,
    pub arrivals: Vec<(Arrival, f64)>,
    pub no_repeat: usize,
    pub cover_max_pct: f64,
    pub passive_every_min: (f64, f64),
    pub bid_every_min: (f64, f64),
    pub max_moves_per_hour: u32,
    pub fullscreen: Fullscreen,
    pub attention: Attention,
    pub monitor: MonitorPolicy,
}

impl Default for BrainSettings {
    fn default() -> Self {
        BrainSettings {
            enabled: true,
            approaches: vec![
                // r7 critics: cling > hang > hang_upside_down.
                (Approach::Hang, 2.0),
                (Approach::HangUpsideDown, 1.0),
                (Approach::Cling, 3.0),
                (Approach::SitEdge, 3.0),
                (Approach::Peek, 1.0),
                (Approach::Beside, 1.0),
            ],
            // Lively is her default (LOCKED #17): more hopping, teleports less often.
            arrivals: vec![(Arrival::Teleport, 2.0), (Arrival::Glitch, 1.0), (Arrival::DropIn, 2.0), (Arrival::Hop, 4.0)],
            no_repeat: 2,
            cover_max_pct: 12.0,
            // LOCKED #17: roams every 0.75–3 min, visits every 8–15 min, 45 moves an hour (a
            // micro-hop counts a quarter).
            passive_every_min: (0.75, 3.0),
            bid_every_min: (8.0, 15.0),
            max_moves_per_hour: 45,
            fullscreen: Fullscreen::Hide,
            attention: Attention::Mix,
            monitor: MonitorPolicy::Follow,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Settings {
    pub brain: BrainSettings,
    pub ragdoll: RagdollSettings,
    pub session: SessionSettings,
    pub music: MusicSettings,
    pub display: DisplaySettings,
    pub mirror: MirrorSettings,
    pub voice: VoiceSettings,
    pub intro: IntroSettings,
    pub hud: HudSettings,
}

/// `[hud]`: how her bubble and status tag move. `glitch` (default true): they glitch as they
/// open, close and change (bands, an RGB split, a red tear); false: no glitch (reduced
/// motion). `motion` (default true): sizes ease (the bubble growing to a reply, the tag's
/// width, the pill); false: they change at once.
#[derive(Clone, Debug, PartialEq)]
pub struct HudSettings {
    pub glitch: bool,
    pub motion: bool,
}

impl Default for HudSettings {
    fn default() -> Self {
        HudSettings { glitch: true, motion: true }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct HudRaw {
    glitch: Option<bool>,
    motion: Option<bool>,
}

/// `[intro]`: the launch. `cinematic` (default true): her close-up over the desktop before the
/// bolt, when the pack has the local close-up frames (`make-pack --mochi`), and the bolt onto
/// the window she sat on last time; false: the launch as before (the bolt onto the floor).
/// `cinematic_min_gap_min` (default 10): no close-up when she was running less than this many
/// minutes ago (a crash restart or `systemctl restart` isn't a greeting). `cinematic_when`
/// (default `"every"`): every launch, or `"daily"`: the first launch of the day only.
#[derive(Clone, Debug, PartialEq)]
pub struct IntroSettings {
    pub cinematic: bool,
    pub cinematic_min_gap_min: f64,
    pub cinematic_daily: bool,
}

impl Default for IntroSettings {
    fn default() -> Self {
        IntroSettings { cinematic: true, cinematic_min_gap_min: 10.0, cinematic_daily: false }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct IntroRaw {
    cinematic: Option<bool>,
    cinematic_min_gap_min: Option<toml::Value>,
    cinematic_when: Option<String>,
}

/// `[voice]`: push-to-talk (hold the talk key) and spoken replies, through the `capri-voice`
/// sidecar. Off by default. `stt_model`: "small.en" (default) | "base.en" | "medium.en";
/// `voice`: a Kokoro voice or a voice string (see docs/VOICE.md: blends, timbre|prosody,
/// @semitones, +clean, +glitch); the default is Haseeb's pick; `speed`: 0.5..2 (1/0.77 with the
/// default voice); `pace`: how the speed is made: "stretch" (default: Kokoro at its own speed,
/// the time scaled in the pitch pass, which keeps its pitch shape) or "model" (Kokoro's speed);
/// `flow`: "emphasis" (default: a `*word*` in her reply is stressed), "pauses", "full" or "off";
/// `duck`: what other audio does while she listens or speaks: "duck" (default: lowered
/// -18 dB through PipeWire, then put back exactly), "pause" (MPRIS pause, then play) or "off"; `speak_unprompted`: her
/// replies to what arrives on its own (events, hooks, other sessions' messages) are spoken
/// too (default true; never over music, an approval or a secret); `silence_s`: the pause that
/// ends a toggled turn (`talk toggle`, 0.5..5 s, default 1.0); `toggle_key`: the toggle chord's
/// letter (`$mod+Ctrl+c` in i3): its auto-repeat is one toggle; `handsfree`: the double-tap (and
/// `talk handsfree on`) can start a hands-free conversation (default true; it is always off at
/// launch); `handsfree_timeout`: it ends after this many seconds with no accepted turn (15..600,
/// default 60); `handsfree_max`: and after this many minutes in all (0 = never, default 30);
/// `echo`: "auto" (default: the sink's port says headphones or speakers), "headphones" (talking
/// over her stops her) or "speakers" (the mic is deaf while she speaks); `wake_word`:
/// "hey capri" (default true: while on, a mic stream stays open, shown on her tag; `false` turns it off); `wake_sensitivity`:
/// 0.1..0.9 (default 0.5: tuned to ~0.5 false starts an hour on synthetic voices); `wake_handsfree`: "hey capri"
/// starts a hands-free conversation (default true, with `handsfree` on; its request is the first turn; quiet for
/// `handsfree_timeout`, it ends and the wake word listens again); false: one turn, as before.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub stt_model: String,
    pub voice: String,
    pub speed: f32,
    pub pace: String,
    pub flow: String,
    pub duck: String,
    pub speak_unprompted: bool,
    /// The talk key's letter (`$mod+c` in i3): a release while it is still down is an X
    /// auto-repeat, not his release.
    pub key: char,
    pub silence_s: f32,
    pub toggle_key: char,
    pub handsfree: bool,
    pub handsfree_timeout: u32,
    pub handsfree_max: u32,
    pub echo: String,
    pub wake_word: bool,
    pub wake_sensitivity: f32,
    pub wake_handsfree: bool,
    /// Windows: her summon key talks while held (opt-in; a tap opens the Line either way). Linux:
    /// unused (his desktop's binding sends `talk press` and `talk release` itself).
    pub hold_to_talk: bool,
}

/// Her voice (Haseeb's picks, LOCKED 13 + 14): W3, the bright blend (timbre af_heart/af_bella/
/// af_jessica; prosody af_heart/af_jessica pushed away from af_sarah), pitched to a ~272 Hz
/// median, cleaned; at speed 1/0.77 by pace "stretch", with flow "emphasis" (his r24 "ALT").
pub const VOICE: &str = "af_heart:0.5,af_bella:0.3,af_jessica:0.2|af_heart:0.84,af_jessica:0.56,af_sarah:-0.4@+4.93+clean";
/// His pick: `rubberband -t 0.77`, as a speed.
pub const SPEED: f32 = 1.2987;

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { enabled: false, stt_model: "small.en".into(), voice: VOICE.into(), speed: SPEED, pace: "stretch".into(), flow: "emphasis".into(), duck: "duck".into(), speak_unprompted: true, key: 'c', silence_s: 1.0, toggle_key: 'c', handsfree: true, handsfree_timeout: 60, handsfree_max: 30, echo: "auto".into(), wake_word: true, wake_sensitivity: 0.5, wake_handsfree: true, hold_to_talk: false }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct VoiceRaw {
    enabled: Option<bool>,
    stt_model: Option<String>,
    voice: Option<String>,
    speed: Option<f64>,
    pace: Option<String>,
    flow: Option<String>,
    duck: Option<String>,
    speak_unprompted: Option<bool>,
    key: Option<String>,
    silence_s: Option<f64>,
    toggle_key: Option<String>,
    handsfree: Option<bool>,
    handsfree_timeout: Option<i64>,
    handsfree_max: Option<i64>,
    echo: Option<String>,
    wake_word: Option<bool>,
    wake_sensitivity: Option<f64>,
    wake_handsfree: Option<bool>,
    hold_to_talk: Option<bool>,
}

/// `[mirror]`: she faces where she goes. `drag`: turn while dragged (and flung), looking back
/// where she's pulled from (`drag_facing = "back"`, default) or the way she moves ("forward"); `self`:
/// her own turns (travel, what she cares about, an idle look-around every `idle_every_min`
/// minutes). `turn_ms`: the turn's squeeze (0: an instant flip). `[drag] mirror` is an alias
/// of `[mirror] drag` (`[mirror]` wins when both are set).
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorSettings {
    pub drag: bool,
    /// `drag_facing`: "back" (default: dragged and flung she looks back where she's pulled
    /// from) or "forward" (the way she moves).
    pub drag_back: bool,
    pub own: bool,
    pub idle_every_min: (f32, f32),
    pub turn_ms: u32,
}

impl Default for MirrorSettings {
    fn default() -> Self {
        MirrorSettings { drag: true, drag_back: true, own: true, idle_every_min: (1.0, 4.0), turn_ms: 150 }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MirrorRaw {
    drag: Option<bool>,
    drag_facing: Option<String>,
    #[serde(rename = "self")]
    own: Option<bool>,
    idle_every_min: Option<Vec<f64>>,
    turn_ms: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DragRaw {
    mirror: Option<bool>,
}

/// `[display]`: `composited = "auto"` (default: alpha while a compositor runs, the X shape
/// otherwise), `"on"` (always alpha) or `"off"` (always the shape). `SA_COMPOSITED=0|1` wins.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DisplaySettings {
    pub composited: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Mode {
    Bool(bool),
    Word(String),
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DisplayRaw {
    composited: Option<Mode>,
}

/// `[music]`: LISTENING when a media player is playing (MPRIS), not at random.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicSettings {
    pub detect: bool,
    /// Player bus-name substrings that never count (calls, voice chat).
    pub ignore: Vec<String>,
}

impl Default for MusicSettings {
    fn default() -> Self {
        MusicSettings { detect: true, ignore: vec!["discord".into(), "vesktop".into()] }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MusicRaw {
    detect: Option<bool>,
    ignore: Option<Vec<String>>,
}

/// `[session]`: her OWN session's persona, model and thinking (never applied to a session
/// she rides with `--session`, nor to `subconscious`).
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSettings {
    pub model: Option<String>,
    /// `low` | `medium` | `high` | `xhigh`.
    pub thinking: Option<String>,
    /// Send the pack's persona (`persona.md`) as her session's system prompt.
    pub persona: bool,
    /// A persona file of your own instead of the pack's.
    pub persona_file: Option<PathBuf>,
}

impl Default for SessionSettings {
    fn default() -> Self {
        SessionSettings { model: None, thinking: None, persona: true, persona_file: None }
    }
}

impl SessionSettings {
    /// The persona text: `persona_file`, else the pack's `persona.md`; None when off,
    /// missing or blank. Err: `persona_file` set but unreadable.
    pub fn persona_text(&self, pack_dir: &std::path::Path) -> Result<Option<String>> {
        if !self.persona {
            return Ok(None);
        }
        let text = match &self.persona_file {
            Some(p) => std::fs::read_to_string(p).with_context(|| format!("session.persona_file {}", p.display()))?,
            None => match std::fs::read_to_string(pack_dir.join("persona.md")) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e).context("persona.md"),
            },
        };
        let t = text.trim();
        Ok((!t.is_empty()).then(|| t.to_string()))
    }
}

/// `[ragdoll]`: how much she bends, squashes and swings.
#[derive(Clone, Debug, PartialEq)]
pub struct RagdollSettings {
    /// 0 = rigid (today's drag), 1 = the full-strength chain; 0.5 by default (Haseeb: "too
    /// strong" at 1). Scales the bend, squash/stretch, shake response and bid swing together.
    pub strength: f32,
}

impl Default for RagdollSettings {
    fn default() -> Self {
        RagdollSettings { strength: 0.5 }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileRaw {
    #[serde(default)]
    brain: Option<BrainRaw>,
    #[serde(default)]
    ragdoll: Option<RagdollRaw>,
    #[serde(default)]
    session: Option<SessionRaw>,
    #[serde(default)]
    music: Option<MusicRaw>,
    #[serde(default)]
    display: Option<DisplayRaw>,
    #[serde(default)]
    mirror: Option<MirrorRaw>,
    #[serde(default)]
    drag: Option<DragRaw>,
    #[serde(default)]
    voice: Option<VoiceRaw>,
    #[serde(default)]
    intro: Option<IntroRaw>,
    #[serde(default)]
    hud: Option<HudRaw>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SessionRaw {
    model: Option<String>,
    thinking: Option<String>,
    persona: Option<bool>,
    persona_file: Option<String>,
}

/// `~/…` and `$HOME/…` in a path setting (Windows: his profile folder).
fn expand_home(p: &str) -> PathBuf {
    let home = crate::platform::home_dir();
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("$HOME/")), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => PathBuf::from(p),
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RagdollRaw {
    strength: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct BrainRaw {
    enabled: Option<bool>,
    approaches: Option<toml::Table>,
    arrivals: Option<toml::Table>,
    no_repeat: Option<i64>,
    cover_max_pct: Option<f64>,
    passive_every_min: Option<[f64; 2]>,
    bid_every_min: Option<[f64; 2]>,
    max_moves_per_hour: Option<i64>,
    fullscreen: Option<String>,
    attention: Option<String>,
    monitor: Option<String>,
}

fn weight(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).filter(|w| w.is_finite() && *w >= 0.0)
}

fn range(name: &str, r: [f64; 2], min: f64) -> Result<(f64, f64)> {
    anyhow::ensure!(r[0].is_finite() && r[1].is_finite() && r[0] >= min && r[1] >= r[0], "{name} must be [lo, hi] with {min} <= lo <= hi");
    Ok((r[0], r[1]))
}

impl Settings {
    pub fn path() -> PathBuf {
        let base = crate::platform::base_dir(crate::platform::BaseDir::Config).unwrap_or_else(std::env::temp_dir);
        base.join("synaps-avatar/settings.toml")
    }

    pub fn parse(text: &str) -> Result<Settings> {
        let raw: FileRaw = toml::from_str(text).context("settings.toml")?;
        let mut b = BrainSettings::default();
        let mut ragdoll = RagdollSettings::default();
        if let Some(v) = raw.ragdoll.and_then(|r| r.strength) {
            anyhow::ensure!(v.is_finite() && (0.0..=1.0).contains(&v), "ragdoll.strength must be 0..1");
            ragdoll.strength = v as f32;
        }
        let mut music = MusicSettings::default();
        if let Some(m) = raw.music {
            if let Some(d) = m.detect {
                music.detect = d;
            }
            if let Some(i) = m.ignore {
                music.ignore = i.into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
            }
        }
        let mut display = DisplaySettings::default();
        if let Some(c) = raw.display.and_then(|d| d.composited) {
            display.composited = match c {
                Mode::Bool(b) => Some(b),
                Mode::Word(w) => match w.trim().to_ascii_lowercase().as_str() {
                    "auto" => None,
                    "on" | "true" | "alpha" => Some(true),
                    "off" | "false" | "shape" => Some(false),
                    _ => anyhow::bail!("display.composited must be \"auto\", \"on\" or \"off\""),
                },
            };
        }
        let mut mirror = MirrorSettings::default();
        if let Some(d) = raw.drag.and_then(|d| d.mirror) {
            mirror.drag = d;
        }
        if let Some(m) = raw.mirror {
            if let Some(d) = m.drag {
                mirror.drag = d;
            }
            if let Some(f) = m.drag_facing {
                mirror.drag_back = match f.trim() {
                    "back" => true,
                    "forward" => false,
                    _ => anyhow::bail!("mirror.drag_facing must be \"back\" or \"forward\""),
                };
            }
            if let Some(o) = m.own {
                mirror.own = o;
            }
            if let Some(v) = m.idle_every_min {
                anyhow::ensure!(
                    v.len() == 2 && v.iter().all(|x| x.is_finite() && *x > 0.0) && v[0] <= v[1],
                    "mirror.idle_every_min must be [min, max] minutes, 0 < min <= max"
                );
                mirror.idle_every_min = (v[0] as f32, v[1] as f32);
            }
            if let Some(t) = m.turn_ms {
                anyhow::ensure!(t <= 400, "mirror.turn_ms must be 0..400");
                mirror.turn_ms = t;
            }
        }
        let mut session = SessionSettings::default();
        if let Some(r) = raw.session {
            if let Some(m) = r.model.map(|m| m.trim().to_string()) {
                anyhow::ensure!(!m.is_empty() && !m.contains(char::is_whitespace), "session.model must be a model name");
                session.model = Some(m);
            }
            if let Some(t) = r.thinking.map(|t| t.trim().to_ascii_lowercase()) {
                anyhow::ensure!(["low", "medium", "high", "xhigh"].contains(&t.as_str()), "session.thinking must be \"low\", \"medium\", \"high\" or \"xhigh\"");
                session.thinking = Some(t);
            }
            if let Some(p) = r.persona {
                session.persona = p;
            }
            if let Some(f) = r.persona_file.filter(|f| !f.trim().is_empty()) {
                session.persona_file = Some(expand_home(f.trim()));
            }
        }
        let mut voice = VoiceSettings::default();
        if let Some(v) = raw.voice {
            if let Some(e) = v.enabled {
                voice.enabled = e;
            }
            if let Some(m) = v.stt_model.map(|m| m.trim().to_string()) {
                anyhow::ensure!(["tiny.en", "base.en", "small.en", "medium.en"].contains(&m.as_str()), "voice.stt_model must be \"base.en\", \"small.en\" or \"medium.en\"");
                voice.stt_model = m;
            }
            if let Some(n) = v.voice.map(|n| n.trim().to_string()) {
                anyhow::ensure!(!n.is_empty() && !n.contains(char::is_whitespace), "voice.voice must be a voice name");
                voice.voice = n;
            }
            if let Some(s) = v.speed {
                anyhow::ensure!(s.is_finite() && (0.5..=2.0).contains(&s), "voice.speed must be 0.5..2");
                voice.speed = s as f32;
            }
            if let Some(p) = v.pace.map(|p| p.trim().to_ascii_lowercase()) {
                anyhow::ensure!(["stretch", "model"].contains(&p.as_str()), "voice.pace must be \"stretch\" or \"model\"");
                voice.pace = p;
            }
            if let Some(f) = v.flow.map(|f| f.trim().to_ascii_lowercase()) {
                anyhow::ensure!(["emphasis", "pauses", "full", "off"].contains(&f.as_str()), "voice.flow must be \"emphasis\", \"pauses\", \"full\" or \"off\"");
                voice.flow = f;
            }
            if let Some(d) = v.duck.map(|d| d.trim().to_ascii_lowercase()) {
                anyhow::ensure!(["pause", "duck", "off"].contains(&d.as_str()), "voice.duck must be \"pause\", \"duck\" or \"off\"");
                voice.duck = d;
            }
            if let Some(u) = v.speak_unprompted {
                voice.speak_unprompted = u;
            }
            let letter = |k: String, what: &str| -> Result<char> {
                let mut c = k.trim().chars();
                match (c.next(), c.next()) {
                    (Some(ch), None) if ch.is_ascii_alphanumeric() => Ok(ch),
                    _ => anyhow::bail!("voice.{what} must be one letter or digit (as in $mod+c)"),
                }
            };
            if let Some(k) = v.key {
                voice.key = letter(k, "key")?;
            }
            if let Some(k) = v.toggle_key {
                voice.toggle_key = letter(k, "toggle_key")?;
            }
            if let Some(s) = v.silence_s {
                anyhow::ensure!(s.is_finite() && (0.5..=5.0).contains(&s), "voice.silence_s must be 0.5..5 (seconds)");
                voice.silence_s = s as f32;
            }
            if let Some(h) = v.handsfree {
                voice.handsfree = h;
            }
            if let Some(t) = v.handsfree_timeout {
                anyhow::ensure!((15..=600).contains(&t), "voice.handsfree_timeout must be 15..600 (seconds)");
                voice.handsfree_timeout = t as u32;
            }
            if let Some(m) = v.handsfree_max {
                anyhow::ensure!((0..=600).contains(&m), "voice.handsfree_max must be 0..600 (minutes; 0 = never)");
                voice.handsfree_max = m as u32;
            }
            if let Some(e) = v.echo.map(|e| e.trim().to_ascii_lowercase()) {
                anyhow::ensure!(["auto", "headphones", "speakers"].contains(&e.as_str()), "voice.echo must be \"auto\", \"headphones\" or \"speakers\"");
                voice.echo = e;
            }
            if let Some(w) = v.wake_word {
                voice.wake_word = w;
            }
            if let Some(s) = v.wake_sensitivity {
                anyhow::ensure!(s.is_finite() && (0.1..=0.9).contains(&s), "voice.wake_sensitivity must be 0.1..0.9");
                voice.wake_sensitivity = s as f32;
            }
            if let Some(h) = v.hold_to_talk {
                voice.hold_to_talk = h;
            }
            if let Some(h) = v.wake_handsfree {
                voice.wake_handsfree = h;
            }
        }
        let mut intro = IntroSettings::default();
        if let Some(i) = raw.intro {
            if let Some(c) = i.cinematic {
                intro.cinematic = c;
            }
            if let Some(g) = i.cinematic_min_gap_min {
                let g = weight(&g).filter(|g| *g <= 1440.0).context("intro.cinematic_min_gap_min must be 0..1440 (minutes)")?;
                intro.cinematic_min_gap_min = g;
            }
            if let Some(w) = i.cinematic_when {
                intro.cinematic_daily = match w.as_str() {
                    "every" => false,
                    "daily" => true,
                    _ => anyhow::bail!("intro.cinematic_when must be \"every\" or \"daily\""),
                };
            }
        }
        let mut hud = HudSettings::default();
        if let Some(h) = raw.hud {
            hud.glitch = h.glitch.unwrap_or(hud.glitch);
            hud.motion = h.motion.unwrap_or(hud.motion);
        }
        let Some(r) = raw.brain else { return Ok(Settings { brain: b, ragdoll, session, music, display, mirror, voice, intro, hud }) };
        if let Some(v) = r.enabled {
            b.enabled = v;
        }
        if let Some(t) = r.approaches {
            for (k, v) in &t {
                let a = Approach::parse(k).with_context(|| format!("brain.approaches: unknown approach `{k}`"))?;
                let w = weight(v).with_context(|| format!("brain.approaches.{k}: a weight >= 0"))?;
                b.approaches.iter_mut().filter(|p| p.0 == a).for_each(|p| p.1 = w);
            }
        }
        if let Some(t) = r.arrivals {
            for (k, v) in &t {
                let a = Arrival::parse(k).with_context(|| format!("brain.arrivals: unknown arrival `{k}`"))?;
                let w = weight(v).with_context(|| format!("brain.arrivals.{k}: a weight >= 0"))?;
                b.arrivals.iter_mut().filter(|p| p.0 == a).for_each(|p| p.1 = w);
            }
        }
        if let Some(n) = r.no_repeat {
            anyhow::ensure!((0..=5).contains(&n), "brain.no_repeat must be 0..5");
            b.no_repeat = n as usize;
        }
        if let Some(p) = r.cover_max_pct {
            anyhow::ensure!((0.0..=50.0).contains(&p), "brain.cover_max_pct must be 0..50");
            b.cover_max_pct = p;
        }
        if let Some(v) = r.passive_every_min {
            b.passive_every_min = range("brain.passive_every_min", v, 0.5)?;
        }
        if let Some(v) = r.bid_every_min {
            b.bid_every_min = range("brain.bid_every_min", v, 1.0)?;
        }
        if let Some(n) = r.max_moves_per_hour {
            anyhow::ensure!((1..=120).contains(&n), "brain.max_moves_per_hour must be 1..120");
            b.max_moves_per_hour = n as u32;
        }
        if let Some(m) = r.monitor {
            b.monitor = match m.as_str() {
                "follow" => MonitorPolicy::Follow,
                "any" => MonitorPolicy::Any,
                _ => anyhow::bail!("brain.monitor must be \"follow\" or \"any\""),
            };
        }
        if let Some(f) = r.fullscreen {
            b.fullscreen = match f.as_str() {
                "hide" => Fullscreen::Hide,
                "stay" => Fullscreen::Stay,
                _ => anyhow::bail!("brain.fullscreen must be \"hide\" or \"stay\""),
            };
        }
        if let Some(a) = r.attention {
            b.attention = Attention::parse(&a).context("brain.attention must be \"mix\", \"hang_only\" or \"off\"")?;
        }
        // Drift is a transport (by distance), never drawn from the pool.
        b.arrivals.retain(|p| p.0 != Arrival::Drift);
        Ok(Settings { brain: b, ragdoll, session, music, display, mirror, voice, intro, hud })
    }

    /// The file's settings (defaults when there is none). Err: a bad file (keep the old values).
    pub fn load() -> Result<Settings> {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e).context("settings.toml unreadable"),
        }
    }

    /// Change these keys in his settings file (`Settings::path`), the rest as it was. Ok(true):
    /// written; Ok(false): nothing changed. Err: refused, the file untouched (see `write_at`).
    pub fn write(edits: &[Edit]) -> std::result::Result<bool, WriteError> {
        write_at(&Self::path(), edits)
    }
}

/// settings.toml writes, one at a time in the order asked (each `Settings::write` runs off her UI
/// thread). What a write changes is applied only once it is in the file (`finish`), and a
/// reload asked for meanwhile is owed again once the last write is in: what she runs with is
/// what the file says. `T`: what to apply when its write lands.
pub struct WriteQueue<T> {
    jobs: std::collections::VecDeque<(Vec<Edit>, T)>,
    /// The write under way: its number, when it started, what it applies.
    under_way: Option<(u64, std::time::Instant, T)>,
    seq: u64,
    reload_owed: bool,
}

impl<T> Default for WriteQueue<T> {
    fn default() -> Self {
        WriteQueue { jobs: Default::default(), under_way: None, seq: 0, reload_owed: false }
    }
}

/// A write under way this long is stuck (a hung network home, a dead disk): she says so.
pub const WRITE_STUCK: std::time::Duration = std::time::Duration::from_secs(4);

impl<T> WriteQueue<T> {
    pub fn push(&mut self, edits: Vec<Edit>, then: T) {
        self.jobs.push_back((edits, then));
    }

    /// The next write to run, if none is under way: its number and its edits.
    pub fn start(&mut self, now: std::time::Instant) -> Option<(u64, Vec<Edit>)> {
        if self.under_way.is_some() {
            return None;
        }
        let (edits, then) = self.jobs.pop_front()?;
        self.seq += 1;
        self.under_way = Some((self.seq, now, then));
        Some((self.seq, edits))
    }

    /// Write `seq` is done (written or refused): what it applies, and whether a reload is owed
    /// now (the last write is in).
    pub fn finish(&mut self, seq: u64) -> Option<(T, bool)> {
        if self.under_way.as_ref().is_none_or(|u| u.0 != seq) {
            return None;
        }
        let (_, _, then) = self.under_way.take()?;
        let reload = self.reload_owed && self.jobs.is_empty();
        if reload {
            self.reload_owed = false;
        }
        Some((then, reload))
    }

    /// A reload now: true if writes are pending (it is done again once they are in).
    pub fn reload(&mut self) -> bool {
        self.reload_owed = self.busy();
        self.reload_owed
    }

    pub fn busy(&self) -> bool {
        self.under_way.is_some() || !self.jobs.is_empty()
    }

    /// The writes not in yet, oldest first (the one under way, then the queue).
    pub fn pending(&self) -> impl Iterator<Item = &T> {
        self.under_way.iter().map(|u| &u.2).chain(self.jobs.iter().map(|j| &j.1))
    }

    /// Write `seq` still under way after WRITE_STUCK.
    pub fn stuck(&self, seq: u64, now: std::time::Instant) -> bool {
        self.under_way.as_ref().is_some_and(|u| u.0 == seq && now.saturating_duration_since(u.1) >= WRITE_STUCK)
    }

    /// The write under way has been for WRITE_STUCK.
    pub fn stuck_now(&self, now: std::time::Instant) -> bool {
        self.under_way.as_ref().is_some_and(|u| now.saturating_duration_since(u.1) >= WRITE_STUCK)
    }
}

/// Her temp files left beside `path`'s file by a write that never finished (a crash, a kill):
/// only her own pattern (`<name>.<pid>.tmp`, the pid not running, or this process's), never
/// anything else. How many went.
pub fn sweep_temps(path: &std::path::Path) -> usize {
    let Ok(target) = resolve(path) else { return 0 };
    let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) else { return 0 };
    let name = target.file_name().map_or("settings.toml".into(), |n| n.to_string_lossy().into_owned());
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut gone = 0;
    for e in rd.flatten() {
        let f = e.file_name().to_string_lossy().into_owned();
        let Some(ps) = f.strip_prefix(&format!("{name}.")).and_then(|r| r.strip_suffix(".tmp")) else { continue };
        let Some(pid) = ps.parse::<u32>().ok().filter(|p| p.to_string() == ps) else { continue };
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if (pid == std::process::id() || !alive(pid, &e)) && std::fs::remove_file(e.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// Whether the process `pid` that made temp file `e` may still be writing it. Unix: it is
/// running; elsewhere: the file is under a minute old.
fn alive(pid: u32, e: &std::fs::DirEntry) -> bool {
    #[cfg(unix)]
    {
        let _ = e;
        let Ok(p) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: signal 0 sends nothing; it only checks the pid.
        unsafe { libc::kill(p, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|a| a < std::time::Duration::from_secs(60))
    }
}

/// One change to the settings file.
#[derive(Clone, Debug)]
pub enum Edit {
    /// `[table] key = value`.
    Set(&'static str, &'static str, toml_edit::Value),
}

/// Why a write was refused: what she says, and the detail for her log.
#[derive(Debug)]
pub enum WriteError {
    /// It exists but she can't read it (permissions, not a file): never written over.
    Unreadable(String),
    /// It doesn't parse (`line`, 1-based): never "repaired".
    Typo { line: Option<usize>, detail: String },
    /// The result wouldn't load (a value of the wrong type): not written.
    Invalid(String),
    /// Couldn't put it in place (permissions, a full disk).
    Failed(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Unreadable(e) => write!(f, "settings.toml unreadable: {e}"),
            WriteError::Typo { line: Some(l), detail } => write!(f, "settings.toml line {l}: {detail}"),
            WriteError::Typo { line: None, detail } => write!(f, "settings.toml: {detail}"),
            WriteError::Invalid(e) => write!(f, "settings.toml would not load: {e}"),
            WriteError::Failed(e) => write!(f, "settings.toml not saved: {e}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl WriteError {
    /// Her line for it (the Settings page's SAYS strip).
    pub fn says(&self) -> String {
        match self {
            WriteError::Typo { line: Some(l), .. } => format!("Your settings file has a typo on line {l}. I won't write over it."),
            WriteError::Typo { line: None, .. } => "Your settings file has a typo. I won't write over it.".into(),
            WriteError::Unreadable(_) => "I can't read your settings file. I won't write over it.".into(),
            WriteError::Invalid(_) | WriteError::Failed(_) => "Couldn't save that. Your settings file won't let me.".into(),
        }
    }
}

/// `edits` applied to `path`'s file, the rest kept byte for byte (comments, order, spacing, a
/// changed key's own trailing comment). Through a symlink: its target is written, the link
/// stays. A file that is there but unreadable, read-only to her, or doesn't parse, is refused
/// untouched; a result she couldn't load isn't written. Put in place atomically: a temp file
/// beside it, made with its permissions, synced, renamed over it. Saved by him meanwhile (its
/// time or size changed before the rename): read again and the edits made on that. Ok(false):
/// nothing changed, nothing written.
pub fn write_at(path: &std::path::Path, edits: &[Edit]) -> std::result::Result<bool, WriteError> {
    write_checked(path, edits, |_| {})
}

/// [`write_at`], `meanwhile` run between the temp file and the check before its rename (tests:
/// his editor saving then).
fn write_checked(path: &std::path::Path, edits: &[Edit], mut meanwhile: impl FnMut(&std::path::Path)) -> std::result::Result<bool, WriteError> {
    use std::io::ErrorKind::NotFound;
    let target = resolve(path).map_err(|e| WriteError::Unreadable(format!("{e}")))?;
    let fail = |e: std::io::Error| WriteError::Failed(format!("{e}"));
    // Its time, size and (Unix) inode: an editor's own atomic save is a new inode.
    let stamp = |m: &std::fs::Metadata| {
        #[cfg(unix)]
        let ino = std::os::unix::fs::MetadataExt::ino(m);
        #[cfg(not(unix))]
        let ino = 0u64;
        (m.modified().ok(), m.len(), ino)
    };
    for _ in 0..3 {
        let (text, meta) = match std::fs::metadata(&target) {
            Ok(m) if !m.is_file() => return Err(WriteError::Unreadable("not a file".into())),
            Ok(m) => (std::fs::read_to_string(&target).map_err(|e| WriteError::Unreadable(format!("{e}")))?, Some(m)),
            Err(e) if e.kind() == NotFound => (String::new(), None),
            Err(e) => return Err(WriteError::Unreadable(format!("{e}"))),
        };
        if meta.as_ref().is_some_and(|m| !writable(&target, m)) {
            return Err(WriteError::Failed("read-only".into()));
        }
        let out = edited(&text, edits)?;
        if out == text {
            return Ok(false);
        }
        let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(dir).map_err(fail)?;
        let name = target.file_name().map_or("settings.toml".into(), |n| n.to_string_lossy().into_owned());
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        std::fs::remove_file(&tmp).ok();
        let made = (|| -> std::io::Result<()> {
            use std::io::Write as _;
            let mut f = create_like(&tmp, meta.as_ref())?;
            f.write_all(out.as_bytes())?;
            f.sync_all()
        })();
        if let Err(e) = made {
            std::fs::remove_file(&tmp).ok();
            return Err(fail(e));
        }
        meanwhile(&target);
        let now = std::fs::metadata(&target).ok();
        if now.as_ref().map(stamp) != meta.as_ref().map(stamp) {
            std::fs::remove_file(&tmp).ok();
            continue;
        }
        if let Err(e) = rename_over(&tmp, &target) {
            std::fs::remove_file(&tmp).ok();
            return Err(fail(e));
        }
        // The rename itself, on disk (best effort).
        if cfg!(unix)
            && let Ok(d) = std::fs::File::open(dir)
        {
            d.sync_all().ok();
        }
        return Ok(true);
    }
    Err(WriteError::Failed("it kept changing while I wrote".into()))
}

/// She may write it: Unix, her write permission on it (a 0444 file, or root's, would be
/// renamed over without a word); Windows, not marked read-only.
fn writable(path: &std::path::Path, m: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let _ = m;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return false };
        // SAFETY: a valid NUL-terminated path.
        unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        !m.permissions().readonly()
    }
}

/// A new temp file with `like`'s permissions from the start (Unix: its mode, never wider while
/// his settings are in it; a new file: 0644 less the umask).
fn create_like(tmp: &std::path::Path, like: Option<&std::fs::Metadata>) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = like.map_or(0o644, |m| m.permissions().mode() & 0o7777);
        o.mode(mode);
        let f = o.open(tmp)?;
        // The umask took bits off: exactly his mode (still never wider than it).
        if like.is_some() {
            f.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        Ok(f)
    }
    #[cfg(not(unix))]
    {
        // Windows: the only permission is read-only, and a read-only file was refused.
        let _ = like;
        o.open(tmp)
    }
}

/// The file `path` names: its target when it is a symlink (his dotfiles' link stays a link),
/// even one whose target isn't there yet.
fn resolve(path: &std::path::Path) -> std::io::Result<PathBuf> {
    let mut p = path.to_path_buf();
    for _ in 0..16 {
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let to = std::fs::read_link(&p)?;
                p = if to.is_absolute() { to } else { p.parent().map_or(to.clone(), |d| d.join(&to)) };
            }
            Ok(_) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(p),
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many symlinks"))
}

/// Windows: an editor holding the file fails a rename for a moment; three tries, 30 ms apart
/// (the write runs off her UI thread).
fn rename_over(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    let mut last = None;
    for k in 0..if cfg!(windows) { 3 } else { 1 } {
        if k > 0 {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("rename")))
}

/// `text` with `edits` applied (the document's own layout kept), checked to load.
fn edited(text: &str, edits: &[Edit]) -> std::result::Result<String, WriteError> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| WriteError::Typo {
        line: e.span().map(|s| text.as_bytes()[..s.start.min(text.len())].iter().filter(|&&b| b == b'\n').count() + 1),
        detail: e.message().to_string(),
    })?;
    for e in edits {
        match e {
            Edit::Set(t, k, v) => {
                if !doc.contains_key(t) {
                    // A new table goes at the end, after what was there (a closing comment
                    // stays before it, a blank line between).
                    let lead = doc.trailing().as_str().unwrap_or("").to_string();
                    doc.set_trailing("");
                    let mut tbl = toml_edit::Table::new();
                    let had = !doc.as_table().is_empty() || !lead.trim().is_empty();
                    if had {
                        let lead = if lead.is_empty() || lead.ends_with('\n') { lead } else { lead + "\n" };
                        tbl.decor_mut().set_prefix(format!("{lead}\n"));
                    }
                    doc[*t] = toml_edit::Item::Table(tbl);
                }
                let Some(tbl) = doc.get_mut(t).and_then(|i| i.as_table_like_mut()) else {
                    return Err(WriteError::Invalid(format!("{t} is not a table")));
                };
                let mut v = v.clone();
                match tbl.get_mut(k) {
                    // Its own decor (the spaces round it, its trailing comment) stays.
                    Some(item) if item.is_value() => {
                        if let Some(old) = item.as_value() {
                            *v.decor_mut() = old.decor().clone();
                        }
                        *item = toml_edit::Item::Value(v);
                    }
                    _ => {
                        tbl.insert(k, toml_edit::Item::Value(v));
                    }
                }
            }
        }
    }
    let out = doc.to_string();
    Settings::parse(&out).map_err(|e| WriteError::Invalid(format!("{e:#}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ragdoll_strength() {
        assert_eq!(Settings::parse("").unwrap().ragdoll.strength, 0.5, "the toned-down default");
        assert_eq!(Settings::parse("[ragdoll]\nstrength = 1").unwrap().ragdoll.strength, 1.0);
        assert_eq!(Settings::parse("[ragdoll]\nstrength = 0.25\n[brain]\nenabled = false").unwrap().ragdoll.strength, 0.25);
        assert!(Settings::parse("[ragdoll]\nstrength = 1.5").is_err());
        assert!(Settings::parse("[ragdoll]\nstrenght = 0.5").is_err(), "unknown keys are errors");
    }

    #[test]
    fn defaults_without_a_file_or_keys() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
        assert_eq!(Settings::parse("[brain]\n").unwrap(), Settings::default());
        let d = BrainSettings::default();
        assert!(d.enabled && d.no_repeat == 2 && d.cover_max_pct == 12.0 && d.max_moves_per_hour == 45);
        // LOCKED #17: the lively values are the defaults.
        assert_eq!((d.passive_every_min, d.bid_every_min), ((0.75, 3.0), (8.0, 15.0)));
        let w = |a: Arrival| d.arrivals.iter().find(|x| x.0 == a).unwrap().1;
        assert_eq!((w(Arrival::Hop), w(Arrival::Teleport), w(Arrival::DropIn), w(Arrival::Glitch)), (4.0, 2.0, 2.0, 1.0));
        assert_eq!(d.attention, Attention::Mix);
        assert_eq!(d.fullscreen, Fullscreen::Hide);
        assert_eq!(d.monitor, MonitorPolicy::Follow, "her hops stay on his monitor by default");
        let m = MusicSettings::default();
        assert!(m.detect && m.ignore == ["discord", "vesktop"]);
        let s = Settings::parse("[music]\ndetect = false\nignore = [\"Teams\", \" \"]\n").unwrap().music;
        assert!(!s.detect && s.ignore == ["teams"]);
        assert_eq!(Settings::parse("[brain]\nmonitor = \"any\"\n").unwrap().brain.monitor, MonitorPolicy::Any);
        assert_eq!(d.arrivals.iter().find(|a| a.0 == Arrival::Glitch).unwrap().1, 1.0, "glitch: 1 by default since r6");
    }

    #[test]
    fn haseebs_file_keeps_hang_only_with_the_lively_defaults() {
        // His settings.toml: only `attention = "hang_only"` set; everything else is the default.
        let s = Settings::parse("[brain]\nattention = \"hang_only\"\n# passive_every_min = [3, 10]\n").unwrap();
        assert_eq!(s.brain.attention, Attention::HangOnly);
        assert_eq!(s.brain, BrainSettings { attention: Attention::HangOnly, ..BrainSettings::default() });
        assert_eq!(s.brain.max_moves_per_hour, 45);
    }

    #[test]
    fn session_section() {
        let s = Settings::parse("").unwrap().session;
        assert_eq!(s, SessionSettings { model: None, thinking: None, persona: true, persona_file: None });
        let s = Settings::parse("[session]\nmodel = \"claude-sonnet-4-6\"\nthinking = \"Low\"\npersona = false\npersona_file = \"~/p.txt\"").unwrap().session;
        assert_eq!(s.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(s.thinking.as_deref(), Some("low"));
        assert!(!s.persona);
        assert!(s.persona_file.unwrap().ends_with("p.txt"));
        assert!(Settings::parse("[session]\nthinking = \"ultra\"").is_err());
        assert!(Settings::parse("[session]\nmodel = \"\"").is_err());
        assert!(Settings::parse("[session]\npersonna = true").is_err(), "unknown keys are errors");
    }

    /// The shipped persona is everyone's: no one's name or machines in it, and what runs her is
    /// said only when asked (ADVICE-funnel §5). A personal one goes in `persona_file`.
    #[test]
    fn the_shipped_persona_is_generic() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/capri/persona.md");
        let text = std::fs::read_to_string(p).unwrap();
        let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).map(str::to_lowercase).collect();
        for w in ["haseeb", "bella", "avante", "he", "him", "his"] {
            assert!(!words.iter().any(|x| x == w), "persona.md says `{w}`");
        }
        assert!(text.contains("You never bring it up yourself"));
        assert!(text.contains("synaps --attach capri"));
        assert!(text.contains("Never approve a prompt for them"));
    }

    #[test]
    fn persona_text_sources() {
        let dir = std::env::temp_dir().join(format!("sa-persona-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = SessionSettings::default();
        assert_eq!(s.persona_text(&dir).unwrap(), None, "no persona.md: none");
        std::fs::write(dir.join("persona.md"), "  You are Capri.\n\n").unwrap();
        assert_eq!(s.persona_text(&dir).unwrap().as_deref(), Some("You are Capri."));
        let off = SessionSettings { persona: false, ..SessionSettings::default() };
        assert_eq!(off.persona_text(&dir).unwrap(), None);
        std::fs::write(dir.join("mine.txt"), "Mine.").unwrap();
        let own = SessionSettings { persona_file: Some(dir.join("mine.txt")), ..SessionSettings::default() };
        assert_eq!(own.persona_text(&dir).unwrap().as_deref(), Some("Mine."));
        let gone = SessionSettings { persona_file: Some(dir.join("gone.txt")), ..SessionSettings::default() };
        assert!(gone.persona_text(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mirror_section_and_the_drag_alias() {
        let d = Settings::parse("").unwrap().mirror;
        assert_eq!(d, MirrorSettings { drag: true, drag_back: true, own: true, idle_every_min: (1.0, 4.0), turn_ms: 150 });
        let m = Settings::parse("[mirror]\ndrag = false\ndrag_facing = \"forward\"\nself = false\nidle_every_min = [0.5, 2]\nturn_ms = 0\n").unwrap().mirror;
        assert_eq!(m, MirrorSettings { drag: false, drag_back: false, own: false, idle_every_min: (0.5, 2.0), turn_ms: 0 });
        assert!(Settings::parse("[mirror]\ndrag_facing = \"back\"\n").unwrap().mirror.drag_back);
        assert!(Settings::parse("[mirror]\ndrag_facing = \"sideways\"\n").is_err());
        assert!(!Settings::parse("[drag]\nmirror = false\n").unwrap().mirror.drag);
        assert!(Settings::parse("[drag]\nmirror = false\n[mirror]\ndrag = true\n").unwrap().mirror.drag);
        assert!(Settings::parse("[mirror]\nidle_every_min = [4, 1]\n").is_err());
        let v = Settings::parse("[voice]\nenabled = true\nstt_model = \"base.en\"\nvoice = \"bf_emma+glitch\"\nspeed = 1.1\nduck = \"duck\"\n").unwrap().voice;
        assert_eq!((v.enabled, v.stt_model.as_str(), v.voice.as_str(), v.speed, v.duck.as_str()), (true, "base.en", "bf_emma+glitch", 1.1, "duck"));
        assert_eq!(Settings::parse("").unwrap().voice, VoiceSettings::default());
        assert!(Settings::parse("[voice]\nduck = \"loud\"\n").is_err());
        assert!(Settings::parse("[voice]\nspeed = 3\n").is_err());
        let v = Settings::parse("").unwrap().voice;
        assert_eq!((v.flow.as_str(), v.pace.as_str(), v.speed, v.voice.as_str()), ("emphasis", "stretch", SPEED, VOICE));
        assert!((1.0 / SPEED - 0.77).abs() < 1e-4, "his pick: rubberband -t 0.77");
        assert_eq!(Settings::parse("[voice]\npace = \"model\"\n").unwrap().voice.pace, "model");
        assert!(Settings::parse("[voice]\npace = \"warp\"\n").is_err());
        assert_eq!(Settings::parse("[voice]\nflow = \"Off\"\n").unwrap().voice.flow, "off");
        assert!(Settings::parse("[voice]\nflow = \"loud\"\n").is_err());
        assert!(v.speak_unprompted, "her replies to events are spoken by default");
        assert_eq!(v.duck, "duck", "browser video is lowered, not paused (Haseeb, 22:47)");
        assert!(!Settings::parse("[voice]\nspeak_unprompted = false\n").unwrap().voice.speak_unprompted);
        assert_eq!((v.silence_s, v.toggle_key), (1.0, 'c'), "a toggled turn ends after 1 s of silence (LOCKED 16)");
        let t = Settings::parse("[voice]\nsilence_s = 2.5\ntoggle_key = \"v\"\n").unwrap().voice;
        assert_eq!((t.silence_s, t.toggle_key), (2.5, 'v'));
        assert!(Settings::parse("[voice]\nsilence_s = 0.2\n").is_err() && Settings::parse("[voice]\nsilence_s = 9\n").is_err());
        assert!(Settings::parse("[voice]\ntoggle_key = \"ctrl+c\"\n").is_err());
        assert_eq!((v.handsfree, v.handsfree_timeout, v.handsfree_max, v.echo.as_str()), (true, 60, 30, "auto"));
        assert!(v.wake_word && v.wake_sensitivity == 0.5, "the wake word is on by default");
        let w = Settings::parse("[voice]\nwake_word = false\nwake_sensitivity = 0.3\n").unwrap().voice;
        assert!(!w.wake_word && (w.wake_sensitivity - 0.3).abs() < 1e-6);
        assert!(Settings::parse("[voice]\nwake_sensitivity = 0.95\n").is_err());
        assert!(v.wake_handsfree, "\"hey capri\" starts hands-free by default (Haseeb, 00:57)");
        assert!(!Settings::parse("[voice]\nwake_handsfree = false\n").unwrap().voice.wake_handsfree);
        assert!(Settings::parse("[voice]\nwake_handsfree = \"yes\"\n").is_err());
        assert!(!v.hold_to_talk, "a tap is the default: holding to talk is opt-in");
        assert!(Settings::parse("[voice]\nhold_to_talk = true\n").unwrap().voice.hold_to_talk);
        assert!(Settings::parse("[voice]\nhold_to_talk = 1\n").is_err());
        let h = Settings::parse("[voice]\nhandsfree = false\nhandsfree_timeout = 15\nhandsfree_max = 0\necho = \"Speakers\"\n").unwrap().voice;
        assert_eq!((h.handsfree, h.handsfree_timeout, h.handsfree_max, h.echo.as_str()), (false, 15, 0, "speakers"));
        assert!(Settings::parse("[voice]\nhandsfree_timeout = 5\n").is_err() && Settings::parse("[voice]\nhandsfree_max = 9999\n").is_err());
        assert!(Settings::parse("[voice]\necho = \"aec\"\n").is_err());
        assert!(Settings::parse("[mirror]\nflip = true\n").is_err());
    }

    #[test]
    fn display_composited_auto_on_off() {
        assert_eq!(Settings::parse("").unwrap().display.composited, None);
        assert_eq!(Settings::parse("[display]\ncomposited = \"auto\"\n").unwrap().display.composited, None);
        assert_eq!(Settings::parse("[display]\ncomposited = \"on\"\n").unwrap().display.composited, Some(true));
        assert_eq!(Settings::parse("[display]\ncomposited = \"off\"\n").unwrap().display.composited, Some(false));
        assert_eq!(Settings::parse("[display]\ncomposited = false\n").unwrap().display.composited, Some(false));
        assert!(Settings::parse("[display]\ncomposited = \"maybe\"\n").is_err());
        assert!(Settings::parse("[display]\nblur = 1\n").is_err());
    }

    #[test]
    fn hud_glitch_and_motion() {
        assert_eq!(Settings::parse("").unwrap().hud, HudSettings { glitch: true, motion: true });
        assert_eq!(Settings::parse("[hud]\nglitch = false\n").unwrap().hud, HudSettings { glitch: false, motion: true });
        assert!(!Settings::parse("[hud]\nmotion = false\n").unwrap().hud.motion);
        assert!(Settings::parse("[hud]\nglitch = \"yes\"\n").is_err());
        assert!(Settings::parse("[hud]\nfade = true\n").is_err());
    }

    #[test]
    fn intro_cinematic_and_its_gap() {
        assert_eq!(Settings::parse("").unwrap().intro, IntroSettings { cinematic: true, cinematic_min_gap_min: 10.0, cinematic_daily: false });
        assert!(Settings::parse("[intro]\ncinematic_when = \"daily\"\n").unwrap().intro.cinematic_daily);
        assert!(!Settings::parse("[intro]\ncinematic_when = \"every\"\n").unwrap().intro.cinematic_daily);
        assert!(Settings::parse("[intro]\ncinematic_when = \"weekly\"\n").is_err());
        let i = Settings::parse("[intro]\ncinematic = false\ncinematic_min_gap_min = 2.5\n").unwrap().intro;
        assert_eq!((i.cinematic, i.cinematic_min_gap_min), (false, 2.5));
        assert_eq!(Settings::parse("[intro]\ncinematic_min_gap_min = 0\n").unwrap().intro.cinematic_min_gap_min, 0.0);
        assert!(Settings::parse("[intro]\ncinematic_min_gap_min = -1\n").is_err());
        assert!(Settings::parse("[intro]\nloud = true\n").is_err());
    }

    #[test]
    fn parses_haseebs_example() {
        let s = Settings::parse(
            r#"
[brain]
enabled = true
approaches = { hang = 3, hang_upside_down = 2, cling = 2, sit_edge = 3, peek = 1, beside = 1 }   # 0 = never
arrivals   = { teleport = 3, glitch = 2, drop_in = 2, hop = 1 }
no_repeat = 2
cover_max_pct = 12
passive_every_min = [3, 10]
bid_every_min = [20, 30]
max_moves_per_hour = 15
fullscreen = "stay"
"#,
        )
        .unwrap();
        assert_eq!(s.brain.fullscreen, Fullscreen::Stay);
        assert!(s.brain.arrivals.iter().find(|a| a.0 == Arrival::DropIn).unwrap().1 > 0.0, "drop_in is on with the ragdoll");
        let s = Settings::parse("[brain]\napproaches = { hang = 0, peek = 2.5 }\npassive_every_min = [1, 2]\n").unwrap();
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Hang).unwrap().1, 0.0);
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Peek).unwrap().1, 2.5);
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Cling).unwrap().1, 3.0, "unlisted keep their default");
        assert_eq!(s.brain.passive_every_min, (1.0, 2.0));
    }

    #[test]
    fn bad_files_are_errors() {
        for bad in [
            "[brain\n",
            "[brain]\nfullscreen = \"maybe\"\n",
            "[brain]\nmonitor = \"both\"\n",
            "[music]\ndetect = \"yes\"\n",
            "[brain]\napproaches = { fly = 1 }\n",
            "[brain]\napproaches = { hang = -1 }\n",
            "[brain]\nno_repeat = 9\n",
            "[brain]\nbid_every_min = [30, 20]\n",
            "[brain]\nunknown_key = 1\n",
            "[brian]\n",
        ] {
            assert!(Settings::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    fn att(a: Attention) -> Edit {
        Edit::Set("brain", "attention", a.key().into())
    }

    #[test]
    fn attention_round_trips_and_keeps_comments() {
        let text = "# mine\n[brain]\nno_repeat = 3 # keep\n";
        let out = edited(text, &[att(Attention::HangOnly)]).unwrap();
        assert!(out.contains("# mine") && out.contains("no_repeat = 3 # keep"));
        let s = Settings::parse(&out).unwrap();
        assert_eq!((s.brain.attention, s.brain.no_repeat), (Attention::HangOnly, 3));
        let out = edited("", &[att(Attention::Off)]).unwrap();
        assert_eq!(Settings::parse(&out).unwrap().brain.attention, Attention::Off);
        assert_eq!(Attention::Mix.next().next().next(), Attention::Mix);
    }

    /// A settings file as he keeps it: comments everywhere, blank lines, odd spacing, a key's
    /// own comment, an inline table.
    const HIS: &str = "# Capri, my way.\n\n[brain]\n# where she goes\nattention   =  \"mix\"   # my pick, keep\nno_repeat = 3\n\n[voice]\nenabled = true # mic on\nwake_word = true\n\n# the rest later\n[ragdoll]\nstrength = 0.4\n";

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sa-settings-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_write_changes_its_one_key_and_nothing_else() {
        let d = tmp("one");
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        assert!(write_at(&p, &[Edit::Set("voice", "wake_word", false.into())]).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), HIS.replace("wake_word = true", "wake_word = false"));
        // The key's own spacing and comment stay.
        assert!(write_at(&p, &[att(Attention::Off)]).unwrap());
        let want = HIS.replace("wake_word = true", "wake_word = false").replace("attention   =  \"mix\"   # my pick, keep", "attention   =  \"off\"   # my pick, keep");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), want);
        // The same value again: nothing written.
        assert!(!write_at(&p, &[att(Attention::Off)]).unwrap());
        // A table that isn't there yet comes at the end; the rest stays.
        let fresh = d.join("fresh.toml");
        std::fs::write(&fresh, "# only this\n").unwrap();
        write_at(&fresh, &[Edit::Set("voice", "wake_word", false.into())]).unwrap();
        let out = std::fs::read_to_string(&fresh).unwrap();
        assert_eq!(out, "# only this\n\n[voice]\nwake_word = false\n");
        // After the last table's closing comment, a blank line between.
        let tail = d.join("tail.toml");
        std::fs::write(&tail, "[brain]\nno_repeat = 3\n# end of mine\n").unwrap();
        write_at(&tail, &[Edit::Set("voice", "wake_word", false.into())]).unwrap();
        assert_eq!(std::fs::read_to_string(&tail).unwrap(), "[brain]\nno_repeat = 3\n# end of mine\n\n[voice]\nwake_word = false\n");
        // No file yet: made.
        let none = d.join("sub/none.toml");
        assert!(write_at(&none, &[att(Attention::HangOnly)]).unwrap());
        assert_eq!(Settings::parse(&std::fs::read_to_string(&none).unwrap()).unwrap().brain.attention, Attention::HangOnly);
        // No temp file left behind.
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    #[cfg(unix)]
    fn a_file_she_cant_read_or_parse_is_left_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("refuse");
        // Unreadable (his permissions): refused, not written over (it was read as "" before).
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o200)).unwrap();
        if std::fs::read_to_string(&p).is_err() {
            assert!(matches!(write_at(&p, &[att(Attention::Off)]), Err(WriteError::Unreadable(_))));
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(std::fs::read_to_string(&p).unwrap(), HIS);
        }
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        // A typo: refused with its line, untouched.
        let bad = "[brain]\nattention = \"mix\"\n\n[voice\nenabled = true\n";
        std::fs::write(&p, bad).unwrap();
        match write_at(&p, &[att(Attention::Off)]) {
            Err(e @ WriteError::Typo { line: Some(4), .. }) => assert!(e.says().contains("line 4"), "{}", e.says()),
            other => panic!("{other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), bad);
        // A value she couldn't load: not written.
        std::fs::write(&p, HIS).unwrap();
        assert!(matches!(write_at(&p, &[Edit::Set("brain", "attention", "sideways".into())]), Err(WriteError::Invalid(_))));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), HIS);
        // A directory where the file goes: refused.
        let dir = d.join("dir.toml");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(write_at(&dir, &[att(Attention::Off)]), Err(WriteError::Unreadable(_))));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_read_only_file_is_refused_and_no_temp_is_left() {
        let d = tmp("ro");
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&p, perm.clone()).unwrap();
        // Root writes anything: nothing to see there.
        #[cfg(unix)]
        let root = unsafe { libc::geteuid() } == 0;
        #[cfg(not(unix))]
        let root = false;
        if !root {
            match write_at(&p, &[att(Attention::Off)]) {
                Err(e @ WriteError::Failed(_)) => assert!(e.says().contains("won't let me")),
                other => panic!("{other:?}"),
            }
            assert_eq!(std::fs::read_to_string(&p).unwrap(), HIS);
            assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        }
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&p, perm).unwrap();
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn saved_by_him_meanwhile_is_read_again() {
        let d = tmp("race");
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        // His editor saves a change between her read and her rename: hers is made on his.
        let mut saves = 0;
        let r = write_checked(&p, &[att(Attention::Off)], |t| {
            if saves == 0 {
                std::fs::write(t, HIS.replace("strength = 0.4", "strength = 0.75")).unwrap();
            }
            saves += 1;
        });
        assert!(r.unwrap());
        assert_eq!(saves, 2, "read once more, then put in place");
        let s = Settings::parse(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!((s.brain.attention, s.ragdoll.strength), (Attention::Off, 0.75));
        // One that never stops changing: given up, his file as he left it, no temp.
        let mut n = 0;
        let r = write_checked(&p, &[att(Attention::HangOnly)], |t| {
            n += 1;
            std::fs::write(t, format!("{HIS}{}\n", "#".repeat(n))).unwrap();
        });
        assert!(matches!(r, Err(WriteError::Failed(_))), "{r:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), format!("{HIS}###\n"));
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    #[cfg(unix)]
    fn his_symlink_stays_a_symlink_and_its_target_is_written() {
        let d = tmp("link");
        let real = d.join("dotfiles-settings.toml");
        std::fs::write(&real, HIS).unwrap();
        let link = d.join("settings.toml");
        std::os::unix::fs::symlink("dotfiles-settings.toml", &link).unwrap();
        assert!(write_at(&link, &[Edit::Set("voice", "wake_word", false.into())]).unwrap());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), HIS.replace("wake_word = true", "wake_word = false"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    #[cfg(unix)]
    fn its_permissions_stay() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("perm");
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_at(&p, &[att(Attention::Off)]).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        // Wider than the umask lets a new file be: exactly his still.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o664)).unwrap();
        write_at(&p, &[att(Attention::Mix)]).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o664);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn only_her_own_dead_temps_are_swept() {
        let d = tmp("sweep");
        let p = d.join("settings.toml");
        std::fs::write(&p, HIS).unwrap();
        // Hers, from a pid that can't be running (above any pid_max) and from this process.
        let dead = d.join("settings.toml.4294967290.tmp");
        let mine = d.join(format!("settings.toml.{}.tmp", std::process::id()));
        // Not hers: another name, no pid, a padded pid, not a .tmp, a directory.
        let keep = ["other.toml.12.tmp", "settings.toml.x1.tmp", "settings.toml.0012.tmp", "settings.toml.12.bak", "settings.toml..tmp"];
        for f in [&dead, &mine] {
            std::fs::write(f, "x").unwrap();
        }
        // Windows tells a dead writer's by its age (over a minute).
        #[cfg(not(unix))]
        std::fs::OpenOptions::new().write(true).open(&dead).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120)).unwrap();
        for k in keep {
            std::fs::write(d.join(k), "x").unwrap();
        }
        std::fs::create_dir(d.join("settings.toml.99.tmp")).unwrap();
        assert_eq!(sweep_temps(&p), 2);
        assert!(!dead.exists() && !mine.exists() && p.exists());
        assert!(keep.iter().all(|k| d.join(k).exists()) && d.join("settings.toml.99.tmp").is_dir());
        #[cfg(unix)]
        {
            // A live writer's (pid 1 always runs) stays.
            let live = d.join("settings.toml.1.tmp");
            std::fs::write(&live, "x").unwrap();
            assert_eq!(sweep_temps(&p), 0);
            assert!(live.exists());
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_queue_runs_one_write_at_a_time_and_owes_a_reload() {
        let t0 = std::time::Instant::now();
        let mut q: WriteQueue<u8> = WriteQueue::default();
        assert!(!q.reload(), "nothing pending: a reload is just a reload");
        q.push(vec![att(Attention::Off)], 1);
        q.push(vec![att(Attention::Mix)], 2);
        let (a, _) = q.start(t0).unwrap();
        assert!(q.start(t0).is_none(), "one under way");
        assert_eq!(q.pending().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert!(q.reload(), "owed again once they are in");
        assert!(q.finish(a + 7).is_none(), "not that write");
        assert_eq!(q.finish(a), Some((1, false)), "one still queued: not yet");
        let (b, _) = q.start(t0).unwrap();
        assert!(!q.stuck(b, t0 + WRITE_STUCK / 2) && q.stuck(b, t0 + WRITE_STUCK) && q.stuck_now(t0 + WRITE_STUCK));
        assert_eq!(q.finish(b), Some((2, true)), "the last is in: the reload now");
        assert!(!q.busy() && q.start(t0).is_none());
    }
}
