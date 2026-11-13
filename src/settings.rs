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
}

impl Default for BrainSettings {
    fn default() -> Self {
        BrainSettings {
            enabled: true,
            approaches: vec![
                (Approach::Hang, 3.0),
                (Approach::HangUpsideDown, 2.0),
                (Approach::Cling, 2.0),
                (Approach::SitEdge, 3.0),
                (Approach::Peek, 1.0),
                (Approach::Beside, 1.0),
            ],
            // drop_in needs the ragdoll (R-P3): weight 0 until then, whatever the file says.
            // glitch lost both r6 critic pairs to teleport: 1 by default (still selectable).
            arrivals: vec![(Arrival::Teleport, 3.0), (Arrival::Glitch, 1.0), (Arrival::DropIn, 0.0), (Arrival::Hop, 1.0)],
            no_repeat: 2,
            cover_max_pct: 12.0,
            passive_every_min: (3.0, 10.0),
            bid_every_min: (20.0, 30.0),
            max_moves_per_hour: 15,
            fullscreen: Fullscreen::Hide,
            attention: Attention::Mix,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Settings {
    pub brain: BrainSettings,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileRaw {
    #[serde(default)]
    brain: Option<BrainRaw>,
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
        let Some(r) = raw.brain else { return Ok(Settings { brain: b }) };
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
        // The ragdoll isn't in yet: drop_in can't happen.
        b.arrivals.iter_mut().filter(|p| p.0 == Arrival::DropIn).for_each(|p| p.1 = 0.0);
        Ok(Settings { brain: b })
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
    fn defaults_without_a_file_or_keys() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
        assert_eq!(Settings::parse("[brain]\n").unwrap(), Settings::default());
        let d = BrainSettings::default();
        assert!(d.enabled && d.no_repeat == 2 && d.cover_max_pct == 12.0 && d.max_moves_per_hour == 15);
        assert_eq!(d.fullscreen, Fullscreen::Hide);
        assert_eq!(d.arrivals.iter().find(|a| a.0 == Arrival::Glitch).unwrap().1, 1.0, "glitch: 1 by default since r6");
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
        assert_eq!(s.brain.arrivals.iter().find(|a| a.0 == Arrival::DropIn).unwrap().1, 0.0, "drop_in waits for the ragdoll");
        let s = Settings::parse("[brain]\napproaches = { hang = 0, peek = 2.5 }\npassive_every_min = [1, 2]\n").unwrap();
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Hang).unwrap().1, 0.0);
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Peek).unwrap().1, 2.5);
        assert_eq!(s.brain.approaches.iter().find(|a| a.0 == Approach::Cling).unwrap().1, 2.0, "unlisted keep their default");
        assert_eq!(s.brain.passive_every_min, (1.0, 2.0));
    }

    #[test]
    fn bad_files_are_errors() {
        for bad in [
            "[brain\n",
            "[brain]\nfullscreen = \"maybe\"\n",
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
