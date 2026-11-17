//! What the media players add up to (his music), whatever the OS reports them through: the
//! platform's watcher (`platform::watch_media`) reads the players, this sums them.

/// What the players add up to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub playing: bool,
    /// The tempo (BPM) of the first playing player that reports one.
    pub bpm: Option<f32>,
    /// Every playing player that counts says it plays video: he is watching, not listening.
    /// Only Windows' media controls tell; MPRIS can't (false there).
    pub video: bool,
}

/// One player as a watcher read it: its name (what the filter reads), its status as the OS
/// words it (`Playing` counts), its tempo, and whether it says it plays video.
pub type Seen<'a> = (&'a str, &'a str, Option<f32>, bool);

/// Which players count: none whose name contains an `ignore` substring (calls, voice);
/// with `only` (tests), just the ones containing it. The name is the player's bus name with
/// MPRIS (`org.mpris.MediaPlayer2.spotify`), the app's AUMID on Windows (`Spotify.exe`).
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

/// The sum: playing if any counted player is Playing; video if every one of those is a video.
pub fn aggregate<'a>(players: impl Iterator<Item = Seen<'a>>, f: &Filter) -> Playback {
    let mut out = Playback::default();
    let mut all_video = true;
    for (name, status, bpm, video) in players {
        if f.counts(name) && status == "Playing" {
            out.playing = true;
            out.bpm = out.bpm.or(bpm);
            all_video &= video;
        }
    }
    out.video = out.playing && all_video;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_counts_unless_ignored() {
        let f = Filter { ignore: vec!["discord".into(), "vesktop".into()], only: None };
        let p = |v: &[(&'static str, &'static str, Option<f32>)]| aggregate(v.iter().map(|&(n, s, b)| (n, s, b, false)), &f);
        assert_eq!(p(&[]), Playback { playing: false, bpm: None, video: false });
        assert!(p(&[("spotify", "Playing", None)]).playing);
        assert!(!p(&[("spotify", "Paused", None)]).playing);
        // A call or voice chat doesn't count; a paused one next to a playing one doesn't matter.
        assert!(!p(&[("Discord", "Playing", None)]).playing);
        assert!(!p(&[("vesktop.instance2", "Playing", None), ("mpv", "Stopped", None)]).playing);
        let both = p(&[("mpv", "Paused", Some(90.0)), ("zen", "Playing", Some(128.0))]);
        assert_eq!(both, Playback { playing: true, bpm: Some(128.0), video: false });
        // Tests: only the fake player.
        let only = Filter { ignore: vec![], only: Some("satest".into()) };
        assert!(!aggregate([("spotify", "Playing", None, false)].into_iter(), &only).playing);
        assert!(aggregate([("satest", "Playing", None, false)].into_iter(), &only).playing);
    }

    #[test]
    fn the_filter_reads_full_bus_names() {
        // MPRIS hands the watcher the whole bus name; the settings name the player only.
        let f = Filter { ignore: vec!["discord".into(), "vesktop".into()], only: None };
        assert!(f.counts("org.mpris.MediaPlayer2.spotify"));
        assert!(!f.counts("org.mpris.MediaPlayer2.discord"));
        assert!(!f.counts("org.mpris.MediaPlayer2.vesktop.instance12345"));
        let only = Filter { ignore: vec![], only: Some("satest".into()) };
        assert!(only.counts("org.mpris.MediaPlayer2.satest.instance1"));
        assert!(!only.counts("org.mpris.MediaPlayer2.firefox.instance_1_42"));
        // Windows: the app's AUMID.
        assert!(f.counts("Spotify.exe"));
        assert!(!f.counts("com.squirrel.Discord.Discord"));
    }

    #[test]
    fn video_only_when_every_playing_player_is_one() {
        let f = Filter::default();
        let v = aggregate([("MSEdge", "Playing", None, true)].into_iter(), &f);
        assert_eq!(v, Playback { playing: true, bpm: None, video: true });
        // Music beside the video: he listens too.
        let mixed = aggregate([("MSEdge", "Playing", None, true), ("Spotify.exe", "Playing", None, false)].into_iter(), &f);
        assert!(mixed.playing && !mixed.video);
        // A paused video doesn't make the music a video.
        let paused = aggregate([("MSEdge", "Paused", None, true), ("Spotify.exe", "Playing", None, false)].into_iter(), &f);
        assert!(paused.playing && !paused.video);
        assert!(!aggregate([("MSEdge", "Paused", None, true)].into_iter(), &f).video, "quiet is never a video");
    }
}
