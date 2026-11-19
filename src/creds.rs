//! Whether her model has credentials, read before she asks it anything: a fresh install shows
//! NO BRAIN YET from the start, not ONLINE. Read where Synaps 0.12 keeps them (`auth.json` in
//! its base folder, a profile's folder first: `core/config.rs` `resolve_read_path`), by key and
//! type only: never a secret. Her model's provider (`provider::of`) decides what counts: an
//! account's login on file, an API key on file, in Synaps' config or in the environment (hers or
//! the daemon's). A local model needs none (whether its server answers is the Settings page's
//! look, `provider::probe`); a provider Synaps doesn't know, a remote broker or a file she can't
//! read is Unknown, and the daemon's first error says it (`session::Trouble`). A daemon reached
//! through `--socket` (an ssh-forwarded remote one) isn't read at all: its files and its pid are
//! another machine's (`session::Options::local`).

use crate::session::Creds;
use std::path::Path;

/// What to read credentials for: her session's model, and the daemon's profile and pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub model: String,
    pub profile: Option<String>,
    pub pid: Option<u32>,
}

/// Whether reading again can change what a read found (her model or session staying the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Again {
    /// It can: a login may land (Missing), or the file was unreadable or mid-write.
    Yes,
    /// Only a login on file can, which matters only while her tag says she has none: no login on
    /// file, and the daemon's environment unread (Windows), so she can't say Missing herself.
    IfMissing,
    /// Nothing on this machine can: a login (Have), another provider's model, a remote broker.
    Never,
}

/// Read them. An env var counts in her environment or the daemon's (a key exported in the shell
/// that started it). Where the daemon's can't be read (Windows, no pid), such a key can't be ruled
/// out: no login on file is Unknown there, not Missing.
pub fn check(q: &Query) -> (Creds, Again) {
    let daemon = |k: &str| q.pid.and_then(|p| crate::platform::process_env(p, k));
    let env = |k: &str| env_set(k) || daemon(k) == Some(true);
    let daemon_read = daemon("ANTHROPIC_API_KEY").is_some();
    check_at(&q.model, &synaps_client::discovery::base_dir(), q.profile.as_deref(), &env, daemon_read)
}

/// A variable is set (not blank) in her environment. Only whether: a key's value is never read.
pub fn env_set(k: &str) -> bool {
    std::env::var_os(k).is_some_and(|v| !v.to_string_lossy().trim().is_empty())
}

/// `env`: a variable is set in her environment or the daemon's; `daemon_read`: whether the
/// daemon's was read.
pub fn check_at(model: &str, base: &Path, profile: Option<&str>, env: &dyn Fn(&str) -> bool, daemon_read: bool) -> (Creds, Again) {
    use crate::provider::Kind;
    const NEVER: (Creds, Again) = (Creds::Unknown, Again::Never);
    const TRY_AGAIN: (Creds, Again) = (Creds::Unknown, Again::Yes);
    let Some(p) = crate::provider::of(model).filter(|p| p.kind != Kind::Local) else {
        return NEVER;
    };
    // A key in an environment counts (Synaps' broker takes it there): one she can't read (the
    // daemon's, unread) can't be ruled out. An account with no env var (ChatGPT, Copilot, ...)
    // has its login on file or none.
    let missing = if daemon_read || p.env.is_empty() { (Creds::Missing(Some(p.name)), Again::Yes) } else { (Creds::Unknown, Again::IfMissing) };
    // A remote broker holds them (`auth.remote_endpoint`, `SYNAPS_AUTH_ENDPOINT`): not here to read.
    if env("SYNAPS_AUTH_ENDPOINT") {
        return NEVER;
    }
    // Synaps' own first-run check: an OAuth login, or `ANTHROPIC_API_KEY` (`anthropic_credential_available`);
    // the API-key services' env vars (`static_key_configured`).
    if p.env.iter().any(|k| env(k)) {
        return (Creds::Have, Again::Never);
    }
    let file = |name: &str| synaps_file(base, profile, name);
    match std::fs::read_to_string(file("config")) {
        Ok(c) if remote_broker(&c) => return NEVER,
        // An API key in Synaps' login config (`provider.groq = …`): set, never read further.
        Ok(c) if p.kind == Kind::Key && config_key(&c, p.id) => return (Creds::Have, Again::Never),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return TRY_AGAIN,
    }
    let text = match std::fs::read_to_string(file("auth.json")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return missing,
        Err(_) => return TRY_AGAIN,
    };
    if text.trim().is_empty() {
        return missing;
    }
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(&text) else {
        return TRY_AGAIN;
    };
    // Its slots: `<id>` (the default account, or the key) and `<id>@<label>`. Any typed entry
    // counts (an expired token is still a login: the broker refreshes it).
    let named = |k: &str| k == p.id || k.strip_prefix(p.id).is_some_and(|r| r.starts_with('@'));
    let has = root.iter().any(|(k, v)| named(k) && v.get("type").and_then(|t| t.as_str()).is_some());
    if has { (Creds::Have, Again::Never) } else { missing }
}

/// One of Synaps' files (`config`, `auth.json`): a profile's own folder first, else the base one
/// (Synaps' `resolve_read_path`).
pub fn synaps_file(base: &Path, profile: Option<&str>, name: &str) -> std::path::PathBuf {
    profile.filter(|p| !p.trim().is_empty()).map(|p| base.join(p).join(name)).filter(|f| f.exists()).unwrap_or_else(|| base.join(name))
}

/// Synaps' login config has a key for `id` (`provider.<id> = <key>`, not empty).
fn config_key(config: &str, id: &str) -> bool {
    let want = format!("provider.{id}");
    config.lines().any(|l| {
        let l = l.trim();
        !l.starts_with('#') && l.split_once('=').is_some_and(|(k, v)| k.trim() == want && !v.trim().is_empty())
    })
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

    fn none(_: &str) -> bool {
        false
    }

    const MISSING: Creds = Creds::Missing(Some("Claude"));

    /// A fresh install: no folder, no file, an empty one, another provider's login only.
    #[test]
    fn no_claude_login_is_missing() {
        let d = dir("missing");
        assert_eq!(check_at("claude-sonnet-4-6", &d.join("nowhere"), None, &none, true).0, MISSING);
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, MISSING);
        std::fs::write(d.join("auth.json"), "").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, MISSING);
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "k"}, "anthropic-ish": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("anthropic/claude-opus-4-1", &d, None, &none, true).0, MISSING);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_claude_login_or_key_is_have() {
        let d = dir("have");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth", "access": "a", "refresh": "r", "expires": 0}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, Creds::Have);
        std::fs::write(d.join("auth.json"), r#"{"anthropic@work": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, Creds::Have, "a named account");
        std::fs::remove_file(d.join("auth.json")).unwrap();
        let key = |k: &str| k == "ANTHROPIC_API_KEY";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &key, true).0, Creds::Have, "Synaps takes ANTHROPIC_API_KEY");
        std::fs::remove_dir_all(&d).ok();
    }

    /// Not hers to decide: a local model (no login: its server is the page's look), a provider
    /// Synaps doesn't know, a remote broker, a file she can't read.
    #[test]
    fn what_she_cant_tell_is_unknown() {
        let d = dir("unknown");
        for m in ["local/qwen3:8b", "acme/x", "gpt-5", ""] {
            assert_eq!(check_at(m, &d, None, &none, true), (Creds::Unknown, Again::Never), "{m}");
        }
        let remote = |k: &str| k == "SYNAPS_AUTH_ENDPOINT";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &remote, true).0, Creds::Unknown);
        std::fs::write(d.join("config"), "# auth.remote_endpoint = x\nmodel = claude-sonnet-4-6\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, MISSING, "a comment isn't a setting");
        std::fs::write(d.join("config"), "auth.remote_endpoint = https://broker.example\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, Creds::Unknown);
        std::fs::remove_file(d.join("config")).unwrap();
        std::fs::write(d.join("auth.json"), "{ not json").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, Creds::Unknown);
        std::fs::remove_dir_all(&d).ok();
    }

    /// The daemon's environment unread (Windows, no pid): a key exported only in its shell can't
    /// be ruled out, so no login on file is Unknown, never a false NO BRAIN YET. A login on file,
    /// or a key in her own environment, still counts.
    #[test]
    fn an_unread_daemon_env_is_unknown_not_missing() {
        let d = dir("unread");
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false).0, Creds::Unknown);
        std::fs::write(d.join("auth.json"), "{}").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false).0, Creds::Unknown);
        let key = |k: &str| k == "ANTHROPIC_API_KEY";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &key, false).0, Creds::Have);
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false).0, Creds::Have);
        std::fs::remove_dir_all(&d).ok();
    }

    /// Whether a read is worth repeating: never where nothing on this machine can change the
    /// answer (another provider, a remote broker, a login found); while missing, or a file that
    /// couldn't be read whole; and with the daemon's environment unread, only while her tag says
    /// she has no brain (a login on file would clear it).
    #[test]
    fn reads_stop_where_nothing_here_can_change_them() {
        let d = dir("again");
        let at = |m: &str, env: &dyn Fn(&str) -> bool, read: bool| check_at(m, &d, None, env, read);
        assert_eq!(at("local/qwen3:8b", &none, true), (Creds::Unknown, Again::Never));
        let remote = |k: &str| k == "SYNAPS_AUTH_ENDPOINT";
        assert_eq!(at("claude-sonnet-4-6", &remote, true), (Creds::Unknown, Again::Never));
        assert_eq!(at("claude-sonnet-4-6", &none, true), (MISSING, Again::Yes));
        assert_eq!(at("claude-sonnet-4-6", &none, false), (Creds::Unknown, Again::IfMissing), "Windows, no login on file");
        std::fs::write(d.join("auth.json"), "{ \"anthropic\": {\"ty").unwrap();
        assert_eq!(at("claude-sonnet-4-6", &none, false), (Creds::Unknown, Again::Yes), "mid-write");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(at("claude-sonnet-4-6", &none, false), (Creds::Have, Again::Never));
        std::fs::write(d.join("config"), "auth.remote_endpoint = https://broker.example\n").unwrap();
        assert_eq!(at("claude-sonnet-4-6", &none, true), (Creds::Unknown, Again::Never));
        std::fs::remove_dir_all(&d).ok();
    }

    /// The other accounts: a login on file (`<id>` or `<id>@<label>`) or none. Nothing in an
    /// environment signs them in, so none on file is Missing even with the daemon's unread
    /// (Windows). Muse takes `META_API_KEY` as Synaps does.
    #[test]
    fn another_account_is_on_file_or_missing() {
        let d = dir("accounts");
        for read in [true, false] {
            assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, read), (Creds::Missing(Some("ChatGPT")), Again::Yes), "daemon read: {read}");
        }
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}, "openai-codexx": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, false).0, Creds::Missing(Some("ChatGPT")), "Claude's login isn't ChatGPT's");
        std::fs::write(d.join("auth.json"), r#"{"openai-codex@work": {"type": "oauth"}, "github-copilot": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, false), (Creds::Have, Again::Never));
        assert_eq!(check_at("github-copilot/gpt-5.5", &d, None, &none, false).0, Creds::Have);
        assert_eq!(check_at("kimi-code/k3", &d, None, &none, true).0, Creds::Missing(Some("Kimi")));
        assert_eq!(check_at("muse/muse-wonder-1.0", &d, None, &none, false).0, Creds::Unknown, "META_API_KEY in an unread env");
        let meta = |k: &str| k == "META_API_KEY";
        assert_eq!(check_at("muse/muse-wonder-1.0", &d, None, &meta, false).0, Creds::Have);
        std::fs::remove_dir_all(&d).ok();
    }

    /// An API-key service: its key on file (`type: api_key`), in Synaps' login config
    /// (`provider.<id>`), or in an environment (its own variables). None anywhere she can read,
    /// with the daemon's environment read: Missing; unread: Unknown, as Claude's key.
    #[test]
    fn an_api_key_is_on_file_in_the_config_or_in_the_env() {
        let d = dir("keys");
        let groq = "groq/llama-3.3-70b-versatile";
        assert_eq!(check_at(groq, &d, None, &none, true), (Creds::Missing(Some("Groq")), Again::Yes));
        assert_eq!(check_at(groq, &d, None, &none, false), (Creds::Unknown, Again::IfMissing));
        let key = |k: &str| k == "GROQ_API_KEY";
        assert_eq!(check_at(groq, &d, None, &key, true), (Creds::Have, Again::Never));
        let other = |k: &str| k == "CEREBRAS_API_KEY";
        assert_eq!(check_at(groq, &d, None, &other, true).0, Creds::Missing(Some("Groq")), "another service's key");
        std::fs::write(d.join("config"), "# provider.groq = old\nprovider.groq =\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, true).0, Creds::Missing(Some("Groq")), "a comment, an empty value");
        std::fs::write(d.join("config"), "provider.groq = gsk-x\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, false), (Creds::Have, Again::Never));
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true).0, MISSING, "a key's config line signs in only its service");
        std::fs::remove_file(d.join("config")).unwrap();
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "gsk"}}"#).unwrap();
        assert_eq!(check_at(groq, &d, None, &none, false).0, Creds::Have);
        assert_eq!(check_at("kimi/kimi-k3.1-pro", &d, None, &none, true).0, Creds::Missing(Some("Moonshot")));
        std::fs::remove_dir_all(&d).ok();
    }

    /// A profile reads its own folder's file first, else the base one (Synaps' `resolve_read_path`).
    #[test]
    fn a_profile_reads_its_folder_first() {
        let d = dir("profile");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none, true).0, Creds::Have, "no folder of its own: the base file");
        std::fs::create_dir_all(d.join("work")).unwrap();
        std::fs::write(d.join("work").join("auth.json"), "{}").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none, true).0, MISSING);
        std::fs::remove_dir_all(&d).ok();
    }
}
