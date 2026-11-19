//! Whether her model has credentials, read before she asks it anything: a fresh install shows
//! NO BRAIN YET from the start, not ONLINE. Read where Synaps 0.12 keeps them (`auth.json` in
//! its base folder, a profile's folder first: `core/config.rs` `resolve_read_path`), by key and
//! type only: never a secret. Only the default case is decided, a Claude model on this machine's
//! broker; anything else is Unknown, and the daemon's first error says it (`session::Trouble`).

use crate::session::Creds;
use std::path::Path;

/// What to read credentials for: her session's model, and the daemon's profile and pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub model: String,
    pub profile: Option<String>,
    pub pid: Option<u32>,
}

/// Read them. An env var counts in her environment or the daemon's (a key exported in the shell
/// that started it).
pub fn check(q: &Query) -> Creds {
    let env = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| q.pid.and_then(|p| crate::platform::process_env(p, k)))
    };
    check_at(&q.model, &synaps_client::discovery::base_dir(), q.profile.as_deref(), &env)
}

/// A model Synaps runs on Claude: bare `claude-*` (its default) or `anthropic/…`.
fn on_claude(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.starts_with("anthropic/") || (m.starts_with("claude") && !m.contains('/'))
}

pub fn check_at(model: &str, base: &Path, profile: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Creds {
    if !on_claude(model) {
        return Creds::Unknown;
    }
    // A remote broker holds them (`auth.remote_endpoint`, `SYNAPS_AUTH_ENDPOINT`): not here to read.
    if env("SYNAPS_AUTH_ENDPOINT").is_some() {
        return Creds::Unknown;
    }
    // Synaps' own first-run check: an OAuth login, or `ANTHROPIC_API_KEY` (`anthropic_credential_available`).
    if env("ANTHROPIC_API_KEY").is_some() {
        return Creds::Have;
    }
    let file = |name: &str| {
        profile
            .filter(|p| !p.trim().is_empty())
            .map(|p| base.join(p).join(name))
            .filter(|f| f.exists())
            .unwrap_or_else(|| base.join(name))
    };
    match std::fs::read_to_string(file("config")) {
        Ok(c) if remote_broker(&c) => return Creds::Unknown,
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Creds::Unknown,
    }
    let text = match std::fs::read_to_string(file("auth.json")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Creds::Missing(Some("Claude")),
        Err(_) => return Creds::Unknown,
    };
    if text.trim().is_empty() {
        return Creds::Missing(Some("Claude"));
    }
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Creds::Unknown;
    };
    // Its slots: `anthropic` (the default account) and `anthropic@<label>`. Any typed entry counts
    // (an expired token is still a login: the broker refreshes it).
    let has = root.iter().any(|(k, v)| (k == "anthropic" || k.starts_with("anthropic@")) && v.get("type").and_then(|t| t.as_str()).is_some());
    if has { Creds::Have } else { Creds::Missing(Some("Claude")) }
}

/// The config sets `auth.remote_endpoint` (`key = value` lines, `#` comments).
fn remote_broker(config: &str) -> bool {
    config.lines().any(|l| {
        let l = l.trim();
        !l.starts_with('#') && l.split_once('=').is_some_and(|(k, v)| k.trim() == "auth.remote_endpoint" && !v.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sa-creds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    const MISSING: Creds = Creds::Missing(Some("Claude"));

    /// A fresh install: no folder, no file, an empty one, another provider's login only.
    #[test]
    fn no_claude_login_is_missing() {
        let d = dir("missing");
        assert_eq!(check_at("claude-sonnet-4-6", &d.join("nowhere"), None, &none), MISSING);
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), MISSING);
        std::fs::write(d.join("auth.json"), "").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), MISSING);
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "k"}, "anthropic-ish": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("anthropic/claude-opus-4-1", &d, None, &none), MISSING);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_claude_login_or_key_is_have() {
        let d = dir("have");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth", "access": "a", "refresh": "r", "expires": 0}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), Creds::Have);
        std::fs::write(d.join("auth.json"), r#"{"anthropic@work": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), Creds::Have, "a named account");
        std::fs::remove_file(d.join("auth.json")).unwrap();
        let key = |k: &str| (k == "ANTHROPIC_API_KEY").then(|| "sk-x".to_string());
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &key), Creds::Have, "Synaps takes ANTHROPIC_API_KEY");
        std::fs::remove_dir_all(&d).ok();
    }

    /// Not hers to decide: another provider, a remote broker, a file she can't read.
    #[test]
    fn what_she_cant_tell_is_unknown() {
        let d = dir("unknown");
        for m in ["local/qwen3:8b", "openai-codex/gpt-5.5", "groq/llama-3.3-70b-versatile", ""] {
            assert_eq!(check_at(m, &d, None, &none), Creds::Unknown, "{m}");
        }
        let remote = |k: &str| (k == "SYNAPS_AUTH_ENDPOINT").then(|| "https://broker.example".to_string());
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &remote), Creds::Unknown);
        std::fs::write(d.join("config"), "# auth.remote_endpoint = x\nmodel = claude-sonnet-4-6\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), MISSING, "a comment isn't a setting");
        std::fs::write(d.join("config"), "auth.remote_endpoint = https://broker.example\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), Creds::Unknown);
        std::fs::remove_file(d.join("config")).unwrap();
        std::fs::write(d.join("auth.json"), "{ not json").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none), Creds::Unknown);
        std::fs::remove_dir_all(&d).ok();
    }

    /// A profile reads its own folder's file first, else the base one (Synaps' `resolve_read_path`).
    #[test]
    fn a_profile_reads_its_folder_first() {
        let d = dir("profile");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none), Creds::Have, "no folder of its own: the base file");
        std::fs::create_dir_all(d.join("work")).unwrap();
        std::fs::write(d.join("work").join("auth.json"), "{}").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none), MISSING);
        std::fs::remove_dir_all(&d).ok();
    }
}
