//! His music on Windows: the system media transport controls (SMTC), the sessions the media
//! flyout shows (Spotify, browsers, the Media Player app: every player that publishes to it).
//! One thread: the manager's `SessionsChanged` and each session's `PlaybackInfoChanged` wake it,
//! it re-reads every session, and the view hears only when the sum (`media::aggregate`)
//! changes. A slow re-read (`RECHECK`) covers a player whose events never come. SMTC carries no
//! tempo; it does say whether a session plays video.

use crate::media::{Filter, Playback, aggregate};
use ::windows::Foundation::TypedEventHandler;
use ::windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs,
};
use ::windows::Media::MediaPlaybackType;
use anyhow::{Context as _, Result};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

/// The slow re-read (and re-subscribe) when no event came.
const RECHECK: Duration = Duration::from_secs(15);
/// Events come in bursts (status, then type, then the next session): read once after them.
const SETTLE: Duration = Duration::from_millis(60);

/// What woke the watcher: the sessions changed (re-subscribe), or one's playback did.
enum Wake {
    Sessions,
    Playback,
}

/// The status as MPRIS words it (what `media::aggregate` reads: `Playing` counts).
fn word(s: Status) -> &'static str {
    if s == Status::Playing {
        "Playing"
    } else if s == Status::Paused {
        "Paused"
    } else if s == Status::Stopped {
        "Stopped"
    } else {
        ""
    }
}

/// One session now: its app's AUMID (what the filter reads), its status, whether it is a video.
fn read(s: &Session) -> Option<(String, &'static str, bool)> {
    let name = s.SourceAppUserModelId().ok()?.to_string();
    let info = s.GetPlaybackInfo().ok()?;
    let status = word(info.PlaybackStatus().ok()?);
    let video = info.PlaybackType().ok().and_then(|t| t.Value().ok()) == Some(MediaPlaybackType::Video);
    Some((name, status, video))
}

/// Watch the media sessions; `on_change` gets the sum whenever it changes (first call: now).
pub fn watch_media(filter: Filter, on_change: impl Fn(Playback) + Send + Sync + 'static) -> Result<()> {
    watch(filter, on_change).context("smtc")
}

fn watch(filter: Filter, on_change: impl Fn(Playback) + Send + Sync + 'static) -> Result<()> {
    let (ready_tx, ready_rx) = channel::<Result<()>>();
    std::thread::Builder::new().name("media-smtc".into()).spawn(move || {
        // WinRT on a thread of its own, in the multithreaded apartment (blocking waits are fine).
        let _ = unsafe { ::windows::Win32::System::Com::CoInitializeEx(None, ::windows::Win32::System::Com::COINIT_MULTITHREADED) };
        let (tx, rx) = channel::<Wake>();
        let started = (|| -> ::windows::core::Result<Manager> {
            let mgr = Manager::RequestAsync()?.join()?;
            let t = tx.clone();
            mgr.SessionsChanged(&TypedEventHandler::<Manager, SessionsChangedEventArgs>::new(move |_, _| {
                t.send(Wake::Sessions).ok();
                Ok(())
            }))?;
            Ok(mgr)
        })();
        let mgr = match started {
            Ok(m) => m,
            Err(e) => {
                ready_tx.send(Err(e.into())).ok();
                return;
            }
        };
        ready_tx.send(Ok(())).ok();
        let mut sessions: Vec<(Session, Option<i64>)> = Vec::new();
        let (mut last, mut resubscribe) = (None, true);
        loop {
            if resubscribe {
                for (s, t) in sessions.drain(..) {
                    if let Some(t) = t {
                        s.RemovePlaybackInfoChanged(t).ok();
                    }
                }
                if let Ok(list) = mgr.GetSessions() {
                    for s in &list {
                        let t = tx.clone();
                        let token = s
                            .PlaybackInfoChanged(&TypedEventHandler::<Session, PlaybackInfoChangedEventArgs>::new(move |_, _| {
                                t.send(Wake::Playback).ok();
                                Ok(())
                            }))
                            .ok();
                        sessions.push((s, token));
                    }
                }
            }
            let reads: Vec<(String, &'static str, bool)> = sessions.iter().filter_map(|(s, _)| read(s)).collect();
            let now = aggregate(reads.iter().map(|(n, s, v)| (n.as_str(), *s, None, *v)), &filter);
            if last != Some(now) {
                last = Some(now);
                let names: Vec<String> = reads
                    .iter()
                    .filter(|r| filter.counts(&r.0))
                    .map(|(n, s, v)| format!("{n}={}{}", if s.is_empty() { "?" } else { s }, if *v { " (video)" } else { "" }))
                    .collect();
                elog!("synaps-avatar: smtc {} ({})", if now.playing { "playing" } else { "quiet" }, names.join(", "));
                on_change(now);
            }
            resubscribe = match rx.recv_timeout(RECHECK) {
                Ok(w) => {
                    std::thread::sleep(SETTLE);
                    let mut any = matches!(w, Wake::Sessions);
                    while let Ok(w) = rx.try_recv() {
                        any |= matches!(w, Wake::Sessions);
                    }
                    any
                }
                Err(RecvTimeoutError::Timeout) => true,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    })?;
    ready_rx.recv()?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_read_as_mpris_words() {
        assert_eq!(word(Status::Playing), "Playing");
        assert_eq!(word(Status::Paused), "Paused");
        assert_eq!(word(Status::Stopped), "Stopped");
        assert_eq!(word(Status::Changing), "");
        assert_eq!(word(Status::Closed), "");
        // Only `Playing` counts in the sum.
        let f = Filter::default();
        assert!(aggregate([("Spotify.exe", word(Status::Playing), None, false)].into_iter(), &f).playing);
        assert!(!aggregate([("Spotify.exe", word(Status::Changing), None, false)].into_iter(), &f).playing);
    }
}
