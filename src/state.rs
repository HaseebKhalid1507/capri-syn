//! Persisted state in `$XDG_STATE_HOME/synaps-avatar/state.json`: the ground point (feet) in
//! root pixels, so a changed `--height` still stands on the same spot, and the brain's
//! drives. Each writer merges its own key into the file (read, merge, write a temp file,
//! rename), so saving one never wipes another.

use std::path::{Path, PathBuf};

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
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
    let mut v = read(p);
    v[key] = val;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        std::fs::rename(&tmp, p).ok();
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

/// The brain's drives (brain.rs `Saved`).
pub fn load_brain() -> Option<serde_json::Value> {
    read(&path()).get("brain").cloned()
}

pub fn save_brain(v: serde_json::Value) {
    merge(&path(), "brain", v);
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
}
