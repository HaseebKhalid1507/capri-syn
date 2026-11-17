//! What the media players add up to (his music), whatever the OS reports them through: the
//! platform's watcher (`platform::watch_media`) reads the players, this sums them.

/// What the players add up to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub playing: bool,
    /// The tempo (BPM) of the first playing player that reports one.
    pub bpm: Option<f32>,
}

/// Which players count: none whose name contains an `ignore` substring (calls, voice);
/// with `only` (tests), just the ones containing it.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub ignore: Vec<String>,
    pub only: Option<String>,
}

impl Filter {
    pub fn counts(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        !self.ignore.iter().any(|i| !i.is_empty() && n.contains(&i.to_lowercase())) && self.only.as_ref().is_none_or(|o| n.contains(&o.to_lowercase()))
    }
}

/// The sum: playing if any counted player is Playing.
pub fn aggregate<'a>(players: impl Iterator<Item = (&'a str, &'a str, Option<f32>)>, f: &Filter) -> Playback {
    let mut out = Playback::default();
    for (name, status, bpm) in players {
        if f.counts(name) && status == "Playing" {
            out.playing = true;
            out.bpm = out.bpm.or(bpm);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_counts_unless_ignored() {
        let f = Filter { ignore: vec!["discord".into(), "vesktop".into()], only: None };
        let p = |v: &[(&'static str, &'static str, Option<f32>)]| aggregate(v.iter().copied(), &f);
        assert_eq!(p(&[]), Playback { playing: false, bpm: None });
        assert!(p(&[("spotify", "Playing", None)]).playing);
        assert!(!p(&[("spotify", "Paused", None)]).playing);
        // A call or voice chat doesn't count; a paused one next to a playing one doesn't matter.
        assert!(!p(&[("Discord", "Playing", None)]).playing);
        assert!(!p(&[("vesktop.instance2", "Playing", None), ("mpv", "Stopped", None)]).playing);
        let both = p(&[("mpv", "Paused", Some(90.0)), ("zen", "Playing", Some(128.0))]);
        assert_eq!(both, Playback { playing: true, bpm: Some(128.0) });
        // Tests: only the fake player.
        let only = Filter { ignore: vec![], only: Some("satest".into()) };
        assert!(!aggregate([("spotify", "Playing", None)].into_iter(), &only).playing);
        assert!(aggregate([("satest", "Playing", None)].into_iter(), &only).playing);
    }
}
