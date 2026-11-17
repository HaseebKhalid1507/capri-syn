//! His music on Linux: MPRIS players on the session bus. Two threads block on D-Bus signals
//! (NameOwnerChanged for players coming and going, PropertiesChanged for PlaybackStatus and
//! Metadata); nothing polls. The view hears only when the sum (`media::aggregate`) changes.

use crate::media::{Filter, Playback, aggregate};
use anyhow::Context as _;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Debug)]
struct Player {
    name: String,
    status: String,
    bpm: Option<f32>,
}

fn unwrap_variant(v: OwnedValue) -> OwnedValue {
    match &*v {
        Value::Value(inner) => OwnedValue::try_from(&**inner).unwrap_or(v),
        _ => v,
    }
}

fn number(v: &Value) -> Option<f32> {
    match v {
        Value::I32(n) => Some(*n as f32),
        Value::I64(n) => Some(*n as f32),
        Value::U32(n) => Some(*n as f32),
        Value::U64(n) => Some(*n as f32),
        Value::F64(n) => Some(*n as f32),
        Value::Value(inner) => number(inner),
        _ => None,
    }
}

fn bpm_of(meta: &HashMap<String, OwnedValue>) -> Option<f32> {
    meta.get("xesam:audioBPM").and_then(|v| number(v)).filter(|b| (40.0..=240.0).contains(b))
}

fn get(conn: &Connection, dest: &str, prop: &str) -> Option<OwnedValue> {
    let m = conn.call_method(Some(dest), PATH, Some("org.freedesktop.DBus.Properties"), "Get", &(PLAYER, prop)).ok()?;
    m.body().deserialize::<OwnedValue>().ok().map(unwrap_variant)
}

/// A player that doesn't count (ignored, or not the test's) is never asked anything.
fn query(conn: &Connection, name: &str, f: &Filter) -> Player {
    if !f.counts(name) {
        return Player { name: name.to_string(), status: String::new(), bpm: None };
    }
    let status = get(conn, name, "PlaybackStatus").and_then(|v| String::try_from(v).ok()).unwrap_or_default();
    let bpm = get(conn, name, "Metadata").and_then(|v| HashMap::<String, OwnedValue>::try_from(v).ok()).and_then(|m| bpm_of(&m));
    Player { name: name.to_string(), status, bpm }
}

fn owner(conn: &Connection, name: &str) -> Option<String> {
    let m = conn.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "GetNameOwner", &(name,)).ok()?;
    m.body().deserialize::<String>().ok()
}

/// Every player on the bus now, by unique owner.
fn scan(conn: &Connection, f: &Filter) -> HashMap<String, Player> {
    let mut out = HashMap::new();
    let Ok(m) = conn.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "ListNames", &()) else { return out };
    let names: Vec<String> = m.body().deserialize().unwrap_or_default();
    for n in names.into_iter().filter(|n| n.starts_with(PREFIX)) {
        if let Some(o) = owner(conn, &n) {
            out.insert(o, query(conn, &n, f));
        }
    }
    out
}

struct Shared {
    players: HashMap<String, Player>,
    last: Playback,
}

/// Watch the session bus; `on_change` gets the sum whenever it changes (first call: now).
pub fn watch_media(filter: Filter, on_change: impl Fn(Playback) + Send + Sync + 'static) -> anyhow::Result<()> {
    watch(filter, on_change).context("mpris")
}

fn watch(filter: Filter, on_change: impl Fn(Playback) + Send + Sync + 'static) -> anyhow::Result<()> {
    let conn = Connection::session()?;
    let owners = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns("org.mpris.MediaPlayer2")?
        .build();
    let props = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(PATH)?
        .arg(0, PLAYER)?
        .build();
    // Subscribed before the scan: nothing in between is missed.
    let owners = MessageIterator::for_match_rule(owners, &conn, Some(64))?;
    let props = MessageIterator::for_match_rule(props, &conn, Some(64))?;
    let shared = Arc::new(Mutex::new(Shared { players: scan(&conn, &filter), last: Playback::default() }));
    let on_change = Arc::new(on_change);
    let publish = {
        let filter = filter.clone();
        let on_change = on_change.clone();
        move |s: &mut Shared, first: bool| {
            let now = aggregate(s.players.values().map(|p| (p.name.as_str(), p.status.as_str(), p.bpm, false)), &filter);
            if first || now != s.last {
                s.last = now;
                let names: Vec<String> =
                    s.players.values().filter(|p| filter.counts(&p.name)).map(|p| format!("{}={}", p.name.trim_start_matches(PREFIX), p.status)).collect();
                elog!("synaps-avatar: mpris {} ({})", if now.playing { "playing" } else { "quiet" }, names.join(", "));
                on_change(now);
            }
        }
    };
    let publish = Arc::new(publish);
    if let Ok(mut s) = shared.lock() {
        publish(&mut s, true);
    }
    {
        let (conn, shared, publish, filter) = (conn.clone(), shared.clone(), publish.clone(), filter.clone());
        let stats = std::env::var_os("SA_MPRIS_STATS").is_some();
        std::thread::Builder::new().name("mpris-props".into()).spawn(move || {
            for m in props {
                let Ok(m) = m else { continue };
                let Some(sender) = m.header().sender().map(|s| s.to_string()) else { continue };
                if stats {
                    elog!("synaps-avatar: mpris signal from {sender}");
                }
                let Ok((_, changed, _)) = m.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() else { continue };
                let Ok(mut s) = shared.lock() else { return };
                if !s.players.contains_key(&sender) {
                    // A player we haven't met (it was there before its name was): rescan.
                    s.players = scan(&conn, &filter);
                }
                if let Some(p) = s.players.get_mut(&sender).filter(|p| filter.counts(&p.name)) {
                    if let Some(v) = changed.get("PlaybackStatus").and_then(|v| String::try_from(v.try_clone().ok()?).ok()) {
                        p.status = v;
                    }
                    if let Some(meta) = changed.get("Metadata").and_then(|v| HashMap::<String, OwnedValue>::try_from(v.try_clone().ok()?).ok()) {
                        p.bpm = bpm_of(&meta);
                    }
                }
                publish(&mut s, false);
            }
        })?;
    }
    std::thread::Builder::new().name("mpris-owners".into()).spawn(move || {
        for m in owners {
            let Ok(m) = m else { continue };
            let Ok((name, old, new)) = m.body().deserialize::<(String, String, String)>() else { continue };
            let Ok(mut s) = shared.lock() else { return };
            if !old.is_empty() {
                s.players.remove(&old);
            }
            if !new.is_empty() && name.starts_with(PREFIX) {
                let p = query(&conn, &name, &filter);
                s.players.insert(new, p);
            }
            publish(&mut s, false);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_from_metadata() {
        let mut m = HashMap::new();
        m.insert("xesam:audioBPM".to_string(), OwnedValue::from(124i32));
        assert_eq!(bpm_of(&m), Some(124.0));
        m.insert("xesam:audioBPM".to_string(), OwnedValue::from(7i32));
        assert_eq!(bpm_of(&m), None, "nonsense tempo ignored");
    }
}
