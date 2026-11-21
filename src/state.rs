//! Persisted state in `$XDG_STATE_HOME/synaps-avatar/state.json`: the ground point (feet) in
//! root pixels, so a changed `--height` still stands on the same spot, and the brain's
//! drives. Each writer merges its own key into the file (read, merge, write a temp file of
//! its own, rename), one at a time (`WRITE`), so saving one never wipes another, from any
//! thread.

use std::path::{Path, PathBuf};

/// One read-merge-write at a time: a save off the UI thread (`save_outfit`) never loses a key
/// another writer set meanwhile.
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn locked() -> std::sync::MutexGuard<'static, ()> {
    WRITE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The state file (`settings::sweep_temps` clears its temp files left by a crash).
pub fn state_path() -> PathBuf {
    path()
}

fn path() -> PathBuf {
    let base = crate::platform::base_dir(crate::platform::BaseDir::State).unwrap_or_else(std::env::temp_dir);
    base.join("synaps-avatar/state.json")
}

fn read(p: &Path) -> serde_json::Value {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Set `key` in the JSON object at `p`, keeping every other key.
fn merge(p: &Path, key: &str, val: serde_json::Value) {
    let _w = locked();
    let mut v = read(p);
    v[key] = val;
    write(p, &v);
}

fn write(p: &Path, v: &serde_json::Value) {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    // Her own temp file (`<name>.<pid>.tmp`, as the settings' writes): never one another
    // process (a second instance) is writing; one a crash left is swept at the next start.
    let name = p.file_name().map_or_else(|| "state.json".into(), |n| n.to_string_lossy().into_owned());
    let tmp = p.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, v.to_string()).is_ok() && std::fs::rename(&tmp, p).is_err() {
        std::fs::remove_file(&tmp).ok();
    }
}

fn ground_at(p: &Path) -> Option<(i32, i32)> {
    let v = read(p);
    let g = v.get("ground")?.as_array()?;
    Some((g.first()?.as_i64()? as i32, g.get(1)?.as_i64()? as i32))
}

pub fn load_ground() -> Option<(i32, i32)> {
    ground_at(&path())
}

pub fn save_ground(ground: (i32, i32)) {
    merge(&path(), "ground", serde_json::json!([ground.0, ground.1]));
}

/// Hidden from her menu (HIDE): she stays hidden across restarts until restored.
pub fn load_hidden() -> bool {
    read(&path()).get("hidden").and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn save_hidden(hidden: bool) {
    merge(&path(), "hidden", serde_json::json!(hidden));
}

/// The outfit she wears (`moods.json` `variants.outfits` id): a restart keeps it.
pub fn load_outfit() -> Option<String> {
    outfit_at(&path())
}

fn outfit_at(p: &Path) -> Option<String> {
    read(p).get("outfit").and_then(serde_json::Value::as_str).map(str::to_string)
}

/// Off the UI thread (a file write, at a bolt or a wake).
pub fn save_outfit(id: String) {
    std::thread::spawn(move || merge(&path(), "outfit", serde_json::json!(id)));
}

/// The brain's drives (brain.rs `Saved`).
pub fn load_brain() -> Option<serde_json::Value> {
    read(&path()).get("brain").cloned()
}

pub fn save_brain(v: serde_json::Value) {
    merge(&path(), "brain", v);
}

/// Her last sign of life (unix s): written every minute while she runs and at a quit, so a
/// crash or a `systemctl restart` counts as an exit too (`[intro] cinematic_min_gap_min`).
pub fn load_seen() -> Option<f64> {
    read(&path()).get("seenUnix").and_then(serde_json::Value::as_f64)
}

/// When her cinematic close-up last played (unix s): `[intro] cinematic_when = "daily"`.
pub fn load_intro_played() -> Option<f64> {
    read(&path()).get("introPlayedUnix").and_then(serde_json::Value::as_f64)
}

pub fn save_intro_played(unix: f64) {
    merge(&path(), "introPlayedUnix", serde_json::json!(unix));
}

/// Where she sat at her last sign of life, if on a window top: restored at the next launch if
/// that window is still there, the same size in the same place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedPerch {
    pub win: u32,
    pub sit: bool,
    pub dx: i32,
    pub rect: (i32, i32, i32, i32),
}

pub fn load_perch() -> Option<SavedPerch> {
    perch_at(&path())
}

fn perch_at(p: &Path) -> Option<SavedPerch> {
    let v = read(p);
    let q = v.get("perch")?;
    let i = |k: &str| q.get(k).and_then(serde_json::Value::as_i64);
    let r = q.get("rect")?.as_array()?;
    let r = |n: usize| r.get(n).and_then(serde_json::Value::as_i64).map(|v| v as i32);
    Some(SavedPerch { win: i("win")? as u32, sit: q.get("sit")?.as_bool()?, dx: i("dx")? as i32, rect: (r(0)?, r(1)?, r(2)?, r(3)?) })
}

fn seen_at(p: &Path, unix: f64, perch: Option<SavedPerch>) {
    let _w = locked();
    let mut v = read(p);
    v["seenUnix"] = serde_json::json!(unix);
    v["perch"] = match perch {
        Some(s) => serde_json::json!({"win": s.win, "sit": s.sit, "dx": s.dx, "rect": [s.rect.0, s.rect.1, s.rect.2, s.rect.3]}),
        None => serde_json::Value::Null,
    };
    write(p, &v);
}

pub fn save_seen(unix: f64, perch: Option<SavedPerch>) {
    seen_at(&path(), unix, perch);
}

/// Her first launch ever: she hasn't introduced herself, and nothing of hers is in the file
/// yet (no spot, no heartbeat, no drives). Someone who ran her before the first words existed
/// is marked met here, quietly: she never greets an old friend as a stranger.
pub fn first_meeting() -> bool {
    first_meeting_at(&path())
}

fn first_meeting_at(p: &Path) -> bool {
    let v = read(p);
    if v.get("met").and_then(serde_json::Value::as_bool) == Some(true) {
        return false;
    }
    if ["ground", "seenUnix", "brain", "perch"].iter().any(|k| v.get(*k).is_some_and(|x| !x.is_null())) {
        merge(p, "met", serde_json::json!(true));
        return false;
    }
    true
}

/// She said her first words (once in her life).
pub fn save_met() {
    merge(&path(), "met", serde_json::json!(true));
}

fn sessions_path() -> PathBuf {
    path().with_file_name("sessions.json")
}

/// Her own session's id for this daemon (`key`: the profile or socket), from last time.
pub fn remembered_session(key: &str) -> Option<String> {
    read(&sessions_path()).get(key)?.as_str().map(str::to_string)
}

pub fn remember_session(key: &str, id: &str) {
    merge(&sessions_path(), key, serde_json::Value::String(id.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers_on_threads_keep_each_others_keys_and_leave_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("sa-state-threads-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let p = dir.join("state.json");
        let ts: Vec<_> = (0..8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || merge(&p, &format!("k{i}"), serde_json::json!(i)))
            })
            .collect();
        for t in ts {
            t.join().unwrap();
        }
        let v = read(&p);
        assert!((0..8).all(|i| v.get(format!("k{i}")) == Some(&serde_json::json!(i))), "{v}");
        let left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["state.json"], "its own temp file, renamed into place");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_outfit_round_trips_beside_the_rest() {
        let p = std::env::temp_dir().join(format!("sa-state-outfit-{}.json", std::process::id()));
        std::fs::remove_file(&p).ok();
        assert_eq!(outfit_at(&p), None);
        merge(&p, "ground", serde_json::json!([1, 2]));
        merge(&p, "outfit", serde_json::json!("alt"));
        assert_eq!(outfit_at(&p).as_deref(), Some("alt"));
        assert_eq!(ground_at(&p), Some((1, 2)), "the other keys stay");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn saving_one_key_keeps_the_others() {
        let dir = std::env::temp_dir().join(format!("sa-state-test-{}", std::process::id()));
        let p = dir.join("state.json");
        std::fs::remove_dir_all(&dir).ok();
        merge(&p, "ground", serde_json::json!([10, 20]));
        merge(&p, "brain", serde_json::json!({"v": 1, "attention": 0.4}));
        assert_eq!(ground_at(&p), Some((10, 20)));
        merge(&p, "ground", serde_json::json!([30, 40]));
        let v = read(&p);
        assert_eq!(v["brain"]["attention"], 0.4, "a ground save must not wipe the brain");
        assert_eq!(ground_at(&p), Some((30, 40)));
        // A broken file is replaced, not a reason to lose the new value.
        std::fs::write(&p, "{not json").unwrap();
        merge(&p, "ground", serde_json::json!([1, 2]));
        assert_eq!(ground_at(&p), Some((1, 2)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn first_meeting_once_and_never_for_an_old_friend() {
        let dir = std::env::temp_dir().join(format!("sa-state-met-{}", std::process::id()));
        let p = dir.join("state.json");
        std::fs::remove_dir_all(&dir).ok();
        assert!(first_meeting_at(&p), "no file: a new user");
        assert!(first_meeting_at(&p), "asking doesn't use it up");
        merge(&p, "met", serde_json::json!(true));
        assert!(!first_meeting_at(&p));
        // Ran her before (a heartbeat, a spot): met, quietly, and the rest kept.
        std::fs::remove_dir_all(&dir).ok();
        seen_at(&p, 99.0, None);
        assert!(!first_meeting_at(&p));
        assert_eq!(read(&p)["met"], true);
        assert_eq!(read(&p)["seenUnix"], 99.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn seen_and_perch_round_trip_and_keep_the_rest() {
        let dir = std::env::temp_dir().join(format!("sa-state-seen-{}", std::process::id()));
        let p = dir.join("state.json");
        std::fs::remove_dir_all(&dir).ok();
        merge(&p, "ground", serde_json::json!([10, 20]));
        let s = SavedPerch { win: 0x2600001, sit: true, dx: 140, rect: (100, 200, 800, 600) };
        seen_at(&p, 1234.5, Some(s));
        assert_eq!(perch_at(&p), Some(s));
        assert_eq!(read(&p)["seenUnix"], 1234.5);
        assert_eq!(ground_at(&p), Some((10, 20)));
        merge(&p, "introPlayedUnix", serde_json::json!(1200.0));
        seen_at(&p, 1300.0, None);
        assert_eq!(read(&p)["introPlayedUnix"], 1200.0, "the heartbeat keeps it");
        assert_eq!(perch_at(&p), None, "on the floor: no perch to restore");
        std::fs::remove_dir_all(&dir).ok();
    }
}
