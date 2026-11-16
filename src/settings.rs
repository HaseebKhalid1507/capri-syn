//! What Haseeb allows (the pack says what she *can* do): `~/.config/synaps-avatar/settings.toml`.
//! Every key is optional. Read at start and by `settings reload`; a bad file is logged and the
//! old values stay. The right-click ATTENTION line writes `brain.attention` back, keeping the
//! rest of the file as it was.

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
/// 0.1..0.9 (default 0.5: tuned to ~0.5 false starts an hour on synthetic voices).
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
}

/// Her voice (Haseeb's picks, LOCKED 13 + 14): W3, the bright blend (timbre af_heart/af_bella/
/// af_jessica; prosody af_heart/af_jessica pushed away from af_sarah), pitched to a ~272 Hz
/// median, cleaned; at speed 1/0.77 by pace "stretch", with flow "emphasis" (his r24 "ALT").
pub const VOICE: &str = "af_heart:0.5,af_bella:0.3,af_jessica:0.2|af_heart:0.84,af_jessica:0.56,af_sarah:-0.4@+4.93+clean";
/// His pick: `rubberband -t 0.77`, as a speed.
pub const SPEED: f32 = 1.2987;

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { enabled: false, stt_model: "small.en".into(), voice: VOICE.into(), speed: SPEED, pace: "stretch".into(), flow: "emphasis".into(), duck: "duck".into(), speak_unprompted: true, key: 'c', silence_s: 1.0, toggle_key: 'c', handsfree: true, handsfree_timeout: 60, handsfree_max: 30, echo: "auto".into(), wake_word: true, wake_sensitivity: 0.5 }
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
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SessionRaw {
    model: Option<String>,
    thinking: Option<String>,
    persona: Option<bool>,
    persona_file: Option<String>,
}

/// `~/…` and `$HOME/…` in a path setting.
fn expand_home(p: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
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
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(std::env::temp_dir);
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
        }
        let Some(r) = raw.brain else { return Ok(Settings { brain: b, ragdoll, session, music, display, mirror, voice }) };
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
        Ok(Settings { brain: b, ragdoll, session, music, display, mirror, voice })
    }

    /// The file's settings (defaults when there is none). Err: a bad file (keep the old values).
    pub fn load() -> Result<Settings> {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e).context("settings.toml unreadable"),
        }
    }

    /// Write `brain.attention`, keeping the rest of the file (comments too).
    pub fn save_attention(a: Attention) -> Result<()> {
        let p = Self::path();
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let out = set_attention(&text, a)?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("toml.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }
}

fn set_attention(text: &str, a: Attention) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("settings.toml")?;
    if !doc.contains_key("brain") {
        doc["brain"] = toml_edit::table();
    }
    doc["brain"]["attention"] = toml_edit::value(a.key());
    Ok(doc.to_string())
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

    #[test]
    fn attention_round_trips_and_keeps_comments() {
        let text = "# mine\n[brain]\nno_repeat = 3 # keep\n";
        let out = set_attention(text, Attention::HangOnly).unwrap();
        assert!(out.contains("# mine") && out.contains("no_repeat = 3 # keep"));
        let s = Settings::parse(&out).unwrap();
        assert_eq!((s.brain.attention, s.brain.no_repeat), (Attention::HangOnly, 3));
        let out = set_attention("", Attention::Off).unwrap();
        assert_eq!(Settings::parse(&out).unwrap().brain.attention, Attention::Off);
        assert_eq!(Attention::Mix.next().next().next(), Attention::Mix);
    }
}
