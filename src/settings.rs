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
            // glitch lost both r6 critic pairs to teleport: 1 by default (still selectable).
            // drop_in exists now (the ragdoll round): 2, as in the brief's file.
            arrivals: vec![(Arrival::Teleport, 3.0), (Arrival::Glitch, 1.0), (Arrival::DropIn, 2.0), (Arrival::Hop, 1.0)],
            no_repeat: 2,
            cover_max_pct: 12.0,
            passive_every_min: (3.0, 10.0),
            bid_every_min: (20.0, 30.0),
            max_moves_per_hour: 15,
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
}

/// `[mirror]`: she faces where she goes. `drag`: the way she is dragged (and flung); `self`:
/// her own turns (travel, what she cares about, an idle look-around every `idle_every_min`
/// minutes). `turn_ms`: the turn's squeeze (0: an instant flip). `[drag] mirror` is an alias
/// of `[mirror] drag` (`[mirror]` wins when both are set).
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorSettings {
    pub drag: bool,
    pub own: bool,
    pub idle_every_min: (f32, f32),
    pub turn_ms: u32,
}

impl Default for MirrorSettings {
    fn default() -> Self {
        MirrorSettings { drag: true, own: true, idle_every_min: (1.0, 4.0), turn_ms: 150 }
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MirrorRaw {
    drag: Option<bool>,
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
        let Some(r) = raw.brain else { return Ok(Settings { brain: b, ragdoll, session, music, display, mirror }) };
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
        Ok(Settings { brain: b, ragdoll, session, music, display, mirror })
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
        assert!(d.enabled && d.no_repeat == 2 && d.cover_max_pct == 12.0 && d.max_moves_per_hour == 15);
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
        assert_eq!(d, MirrorSettings { drag: true, own: true, idle_every_min: (1.0, 4.0), turn_ms: 150 });
        let m = Settings::parse("[mirror]\ndrag = false\nself = false\nidle_every_min = [0.5, 2]\nturn_ms = 0\n").unwrap().mirror;
        assert_eq!(m, MirrorSettings { drag: false, own: false, idle_every_min: (0.5, 2.0), turn_ms: 0 });
        assert!(!Settings::parse("[drag]\nmirror = false\n").unwrap().mirror.drag);
        assert!(Settings::parse("[drag]\nmirror = false\n[mirror]\ndrag = true\n").unwrap().mirror.drag);
        assert!(Settings::parse("[mirror]\nidle_every_min = [4, 1]\n").is_err());
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
