//! Whether her model has credentials, read before she asks it anything: a fresh install shows
//! NO BRAIN YET from the start, not ONLINE. Read where Synaps 0.12 keeps them (`auth.json` in
//! its base folder, a profile's folder first: `core/config.rs` `resolve_read_path`), by key and
//! type only: never a secret. Her model's provider (`provider::of`) decides what counts: an
//! account's login on file, an API key on file, in Synaps' config (as the daemon read it at its
//! start) or in the environment (hers or the daemon's). A local model needs none (whether its server answers is the Settings page's
//! look, `provider::probe`); a provider Synaps doesn't know, a remote broker or a file she can't
//! read is Unknown, and the daemon's first error says it (`session::Trouble`). A daemon reached
//! through `--socket` (an ssh-forwarded remote one) isn't read at all: its files and its pid are
//! another machine's (`session::Options::local`).

use crate::session::Creds;
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// A file's time and the daemon's start are told apart at this much, no finer: the coarsest file
/// time (FAT's 2 s; ext3's and HFS+'s 1 s) and Windows' 100 ns clock both round within it, so
/// neither side's rounding can turn "before" into "after".
const MTIME_SLACK: Duration = Duration::from_secs(2);

/// A file written at `written` was there when the daemon started at `started`: older by
/// `MTIME_SLACK` at least. Closer than that, can't tell (false).
pub fn there_at_start(written: SystemTime, started: SystemTime) -> bool {
    written.checked_add(MTIME_SLACK).is_some_and(|w| w <= started)
}

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
/// out: no login on file is Unknown there, not Missing. With whether Synaps' config sets a
/// `model` (`Seen::model_set`).
pub fn check(q: &Query) -> (Creds, Again, Option<bool>) {
    let seen = Seen::read(q.profile.as_deref(), q.pid);
    let (c, again) = seen.check(&q.model);
    (c, again, seen.model_set())
}

/// Synaps' files, the daemon's environment and its start as one read found them: any number of
/// checks off it (the Settings page's look checks every provider), each file read once.
pub struct Seen {
    files: Files,
    /// The daemon's environment (None: unread, as on Windows).
    env: Option<crate::platform::Environ>,
    started: Option<SystemTime>,
}

impl Seen {
    pub fn read(profile: Option<&str>, pid: Option<u32>) -> Seen {
        Seen {
            files: Files::read(&synaps_client::discovery::base_dir(), profile),
            env: pid.and_then(crate::platform::process_environ),
            started: crate::provider::daemon_started(&synaps_client::discovery::run_dir(), profile),
        }
    }

    /// A key the daemon has: in its environment where that can be read (it alone decides: one in
    /// hers only isn't the daemon's), else in hers.
    pub fn env_set(&self, k: &str) -> bool {
        match &self.env {
            Some(e) => e.set.contains(k),
            None => env_set(k),
        }
    }

    /// A variable set in her environment or the daemon's (Gemini's client id: a sign-in in his
    /// terminal reads it there, the daemon to refresh).
    pub fn env_either(&self, k: &str) -> bool {
        env_set(k) || self.env.as_ref().is_some_and(|e| e.set.contains(k))
    }

    /// Synaps' config sets a `model` (its own pick, or his TUI's /model). None: unreadable.
    pub fn model_set(&self) -> Option<bool> {
        self.files.config.as_ref().map(|c| c.contains("model"))
    }

    /// `model`'s credentials (`check`).
    pub fn check(&self, model: &str) -> (Creds, Again) {
        judge(model, &self.files, &|k: &str| self.env_set(k), self.env.is_some(), self.started)
    }

    /// The local server's URL as the daemon has it (`local_url`): its config's
    /// `provider.local.url` as read at its start (`url_at_start`; `known`: what an earlier look
    /// found it read then), its `LOCAL_ENDPOINT` (where it can't be read, hers). With what to
    /// remember of the config's for the next look.
    pub fn local_url(&self, known: Option<&UrlAtStart>) -> (String, Option<UrlAtStart>) {
        let (set, keep) = match self.files.config {
            // Unreadable (held by another process) says nothing of its URL: what was known stands.
            None => (known.filter(|k| Some(k.0) == self.started).and_then(|k| k.1.clone()), known.cloned()),
            Some(_) => url_at_start(self.files.local_url.clone(), self.files.config_at, self.started, known),
        };
        let env = self.env.as_ref().map_or_else(|| std::env::var("LOCAL_ENDPOINT").ok(), |e| e.local_endpoint.clone());
        (crate::provider::local_url(set.as_deref(), env), keep)
    }
}

/// The config's `provider.local.url` as the daemon read it at its start (None: not set then).
pub type UrlAtStart = (SystemTime, Option<String>);

/// The config's `provider.local.url` (`written`) as the daemon has it: Synaps reads it once, at
/// its start, as it does its keys. A config there then (`there_at_start`), or no start known: as
/// written, and remembered for that start. One written since (or too close to tell; his TUI
/// rewrites it on /model, /thinking, /theme): what an earlier look found for that start, else as
/// written, not remembered (that URL rarely changes; dropping it would probe Ollama's port while
/// she answers through his LM Studio).
pub fn url_at_start(written: Option<String>, config_at: Option<SystemTime>, started: Option<SystemTime>, known: Option<&UrlAtStart>) -> (Option<String>, Option<UrlAtStart>) {
    let Some(t) = started else { return (written, None) };
    if config_at.is_none_or(|w| there_at_start(w, t)) {
        return (written.clone(), Some((t, written)));
    }
    match known.filter(|k| k.0 == t) {
        Some(k) => (k.1.clone(), Some(k.clone())),
        None => (written, None),
    }
}

/// Synaps' `config` and `auth.json` as read, by key and type only: no secret kept.
struct Files {
    /// The config's keys set, not blank (its last line for each). None: unreadable.
    config: Option<HashSet<String>>,
    /// Its `provider.local.url` (its last line; a URL, not a secret).
    local_url: Option<String>,
    /// When the config was written (None: no config, or no time).
    config_at: Option<SystemTime>,
    /// `auth.json`'s entries. None: unreadable, or not a JSON object (caught mid-write).
    auth: Option<Vec<Entry>>,
}

/// An `auth.json` entry: its name, its `type`, whether it has a `key` (not empty).
struct Entry {
    name: String,
    kind: Option<String>,
    key: bool,
}

impl Files {
    fn read(base: &Path, profile: Option<&str>) -> Files {
        let file = |name: &str| synaps_file(base, profile, name);
        let gone = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
        let text = std::fs::read_to_string(file("config"));
        let config = match &text {
            Ok(c) => Some(config_set(c)),
            Err(e) if gone(e) => Some(HashSet::new()),
            Err(_) => None,
        };
        let local_url = text.as_deref().ok().and_then(|c| crate::provider::config_value(c, "provider.local.url")).map(str::to_string);
        let config_at = std::fs::metadata(file("config")).and_then(|m| m.modified()).ok();
        let auth = match std::fs::read_to_string(file("auth.json")) {
            Ok(t) if t.trim().is_empty() => Some(Vec::new()),
            Ok(t) => match serde_json::from_str::<serde_json::Value>(&t) {
                Ok(serde_json::Value::Object(root)) => Some(
                    root.iter()
                        .map(|(k, v)| Entry {
                            name: k.clone(),
                            kind: v.get("type").and_then(|t| t.as_str()).map(str::to_string),
                            key: v.get("key").and_then(|k| k.as_str()).is_some_and(|k| !k.is_empty()),
                        })
                        .collect(),
                ),
                _ => None,
            },
            Err(e) if gone(&e) => Some(Vec::new()),
            Err(_) => None,
        };
        Files { config, local_url, config_at, auth }
    }
}

/// The config's keys set, not blank: Synaps' map of it (its last line for each, `#` comments
/// skipped, trimmed: `provider::config_value`).
fn config_set(config: &str) -> HashSet<String> {
    let mut last = std::collections::HashMap::new();
    for (k, v) in config.lines().map(str::trim).filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('=')) {
        last.insert(k.trim(), !v.trim().is_empty());
    }
    last.into_iter().filter(|&(_, set)| set).map(|(k, _)| k.to_string()).collect()
}

/// A variable is set (not blank) in her environment. Only whether: a key's value is never read.
pub fn env_set(k: &str) -> bool {
    std::env::var_os(k).is_some_and(|v| !v.to_string_lossy().trim().is_empty())
}

/// `env`: the daemon has a variable set (`Seen::env_set`: its environment, else hers); `daemon_read`: whether the
/// daemon's was read; `started`: when the daemon started (it read its `config` then, and only
/// then: a key put there since isn't its yet).
#[cfg(test)]
pub fn check_at(model: &str, base: &Path, profile: Option<&str>, env: &dyn Fn(&str) -> bool, daemon_read: bool, started: Option<SystemTime>) -> (Creds, Again) {
    judge(model, &Files::read(base, profile), env, daemon_read, started)
}

/// `check_at` off files already read.
fn judge(model: &str, f: &Files, env: &dyn Fn(&str) -> bool, daemon_read: bool, started: Option<SystemTime>) -> (Creds, Again) {
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
    // A key in Synaps' login config (`provider.groq = …`), set (never read further) before the
    // daemon started: one put there since, or too close to its start to tell, waits for its next
    // start (Unknown, read again; her first answer settles it).
    let Some(config) = &f.config else { return TRY_AGAIN };
    if config.contains("auth.remote_endpoint") {
        return NEVER;
    }
    let mut config_key = None;
    if p.kind == Kind::Key && config.contains(&format!("provider.{}", p.id)) {
        let since = |t: SystemTime| f.config_at.is_some_and(|m| !there_at_start(m, t));
        config_key = Some(if started.is_some_and(since) { TRY_AGAIN } else { (Creds::Have, Again::Never) });
    }
    // One on file (read on every request: the daemon has it now) is a key either way.
    let missing = config_key.unwrap_or(missing);
    let Some(auth) = &f.auth else { return TRY_AGAIN };
    // An account's slots: `<id>` (the default one) and `<id>@<label>`, any typed entry (an
    // expired token is still a login: the broker refreshes it). A key service's: `<id>` alone,
    // `type: api_key` with a key in it (Synaps' `load_static_key`).
    let has = match p.kind {
        Kind::Key => auth.iter().any(|e| e.name == p.id && e.kind.as_deref() == Some("api_key") && e.key),
        _ => auth.iter().any(|e| (e.name == p.id || e.name.strip_prefix(p.id).is_some_and(|r| r.starts_with('@'))) && e.kind.is_some()),
    };
    if has { (Creds::Have, Again::Never) } else { missing }
}

/// One of Synaps' files (`config`, `auth.json`): a profile's own folder first, else the base one
/// (Synaps' `resolve_read_path`).
pub fn synaps_file(base: &Path, profile: Option<&str>, name: &str) -> std::path::PathBuf {
    profile.filter(|p| !p.trim().is_empty()).map(|p| base.join(p).join(name)).filter(|f| f.exists()).unwrap_or_else(|| base.join(name))
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
        assert_eq!(check_at("claude-sonnet-4-6", &d.join("nowhere"), None, &none, true, None).0, MISSING);
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, MISSING);
        std::fs::write(d.join("auth.json"), "").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, MISSING);
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "k"}, "anthropic-ish": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("anthropic/claude-opus-4-1", &d, None, &none, true, None).0, MISSING);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_claude_login_or_key_is_have() {
        let d = dir("have");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth", "access": "a", "refresh": "r", "expires": 0}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, Creds::Have);
        std::fs::write(d.join("auth.json"), r#"{"anthropic@work": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, Creds::Have, "a named account");
        std::fs::remove_file(d.join("auth.json")).unwrap();
        let key = |k: &str| k == "ANTHROPIC_API_KEY";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &key, true, None).0, Creds::Have, "Synaps takes ANTHROPIC_API_KEY");
        std::fs::remove_dir_all(&d).ok();
    }

    /// Not hers to decide: a local model (no login: its server is the page's look), a provider
    /// Synaps doesn't know, a remote broker, a file she can't read.
    #[test]
    fn what_she_cant_tell_is_unknown() {
        let d = dir("unknown");
        for m in ["local/qwen3:8b", "acme/x", "gpt-5", ""] {
            assert_eq!(check_at(m, &d, None, &none, true, None), (Creds::Unknown, Again::Never), "{m}");
        }
        let remote = |k: &str| k == "SYNAPS_AUTH_ENDPOINT";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &remote, true, None).0, Creds::Unknown);
        std::fs::write(d.join("config"), "# auth.remote_endpoint = x\nmodel = claude-sonnet-4-6\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, MISSING, "a comment isn't a setting");
        std::fs::write(d.join("config"), "auth.remote_endpoint = https://broker.example\n").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, Creds::Unknown);
        std::fs::remove_file(d.join("config")).unwrap();
        std::fs::write(d.join("auth.json"), "{ not json").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, Creds::Unknown);
        std::fs::remove_dir_all(&d).ok();
    }

    /// The daemon's environment unread (Windows, no pid): a key exported only in its shell can't
    /// be ruled out, so no login on file is Unknown, never a false NO BRAIN YET. A login on file,
    /// or a key in her own environment, still counts.
    #[test]
    fn an_unread_daemon_env_is_unknown_not_missing() {
        let d = dir("unread");
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false, None).0, Creds::Unknown);
        std::fs::write(d.join("auth.json"), "{}").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false, None).0, Creds::Unknown);
        let key = |k: &str| k == "ANTHROPIC_API_KEY";
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &key, false, None).0, Creds::Have);
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, false, None).0, Creds::Have);
        std::fs::remove_dir_all(&d).ok();
    }

    /// Whether a read is worth repeating: never where nothing on this machine can change the
    /// answer (another provider, a remote broker, a login found); while missing, or a file that
    /// couldn't be read whole; and with the daemon's environment unread, only while her tag says
    /// she has no brain (a login on file would clear it).
    #[test]
    fn reads_stop_where_nothing_here_can_change_them() {
        let d = dir("again");
        let at = |m: &str, env: &dyn Fn(&str) -> bool, read: bool| check_at(m, &d, None, env, read, None);
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
            assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, read, None), (Creds::Missing(Some("ChatGPT")), Again::Yes), "daemon read: {read}");
        }
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}, "openai-codexx": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, false, None).0, Creds::Missing(Some("ChatGPT")), "Claude's login isn't ChatGPT's");
        std::fs::write(d.join("auth.json"), r#"{"openai-codex@work": {"type": "oauth"}, "github-copilot": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("openai-codex/gpt-5.5", &d, None, &none, false, None), (Creds::Have, Again::Never));
        assert_eq!(check_at("github-copilot/gpt-5.5", &d, None, &none, false, None).0, Creds::Have);
        assert_eq!(check_at("kimi-code/k3", &d, None, &none, true, None).0, Creds::Missing(Some("Kimi")));
        assert_eq!(check_at("muse/muse-wonder-1.0", &d, None, &none, false, None).0, Creds::Unknown, "META_API_KEY in an unread env");
        let meta = |k: &str| k == "META_API_KEY";
        assert_eq!(check_at("muse/muse-wonder-1.0", &d, None, &meta, false, None).0, Creds::Have);
        std::fs::remove_dir_all(&d).ok();
    }

    /// An API-key service: its key on file (`type: api_key`), in Synaps' login config
    /// (`provider.<id>`), or in an environment (its own variables). None anywhere she can read,
    /// with the daemon's environment read: Missing; unread: Unknown, as Claude's key.
    #[test]
    fn an_api_key_is_on_file_in_the_config_or_in_the_env() {
        let d = dir("keys");
        let groq = "groq/llama-3.3-70b-versatile";
        assert_eq!(check_at(groq, &d, None, &none, true, None), (Creds::Missing(Some("Groq")), Again::Yes));
        assert_eq!(check_at(groq, &d, None, &none, false, None), (Creds::Unknown, Again::IfMissing));
        let key = |k: &str| k == "GROQ_API_KEY";
        assert_eq!(check_at(groq, &d, None, &key, true, None), (Creds::Have, Again::Never));
        let other = |k: &str| k == "CEREBRAS_API_KEY";
        assert_eq!(check_at(groq, &d, None, &other, true, None).0, Creds::Missing(Some("Groq")), "another service's key");
        std::fs::write(d.join("config"), "# provider.groq = old\nprovider.groq =\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, true, None).0, Creds::Missing(Some("Groq")), "a comment, an empty value");
        std::fs::write(d.join("config"), "provider.groq = gsk-x\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, false, None), (Creds::Have, Again::Never));
        assert_eq!(check_at("claude-sonnet-4-6", &d, None, &none, true, None).0, MISSING, "a key's config line signs in only its service");
        std::fs::remove_file(d.join("config")).unwrap();
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "gsk"}}"#).unwrap();
        assert_eq!(check_at(groq, &d, None, &none, false, None).0, Creds::Have);
        assert_eq!(check_at("kimi/kimi-k3.1-pro", &d, None, &none, true, None).0, Creds::Missing(Some("Moonshot")));
        // On file, Synaps takes `<id>` alone, `type: api_key`, a key in it.
        for not in [r#"{"groq@work": {"type": "api_key", "key": "gsk"}}"#, r#"{"groq": {"type": "oauth", "key": "gsk"}}"#, r#"{"groq": {"type": "api_key", "key": ""}}"#, r#"{"groq": {"type": "api_key"}}"#] {
            std::fs::write(d.join("auth.json"), not).unwrap();
            assert_eq!(check_at(groq, &d, None, &none, true, None).0, Creds::Missing(Some("Groq")), "{not}");
        }
        // The config's last line for a key is the one (Synaps reads them into a map).
        std::fs::write(d.join("config"), "provider.groq = gsk-x\nprovider.groq =\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, true, None).0, Creds::Missing(Some("Groq")), "set, then blanked");
        std::fs::write(d.join("config"), "provider.groq =\nprovider.groq = gsk-x\n").unwrap();
        assert_eq!(check_at(groq, &d, None, &none, true, None).0, Creds::Have);
        std::fs::remove_dir_all(&d).ok();
    }

    /// Synaps reads its config's keys once, when the daemon starts: one put there since isn't the
    /// daemon's until its next start (Unknown, read again; not a ✓ that fails). One on file in
    /// `auth.json` it reads on every request: a key at once.
    #[test]
    fn a_config_key_since_the_daemon_started_waits_for_its_next_start() {
        let d = dir("since");
        let groq = "groq/llama-3.3-70b-versatile";
        std::fs::write(d.join("config"), "provider.groq = gsk-x\n").unwrap();
        let written = std::fs::metadata(d.join("config")).unwrap().modified().unwrap();
        let before = written - Duration::from_secs(60);
        let after = written + Duration::from_secs(60);
        assert_eq!(check_at(groq, &d, None, &none, true, Some(after)), (Creds::Have, Again::Never), "there when it started");
        assert_eq!(check_at(groq, &d, None, &none, true, None), (Creds::Have, Again::Never), "no daemon.json: as written");
        assert_eq!(check_at(groq, &d, None, &none, true, Some(before)), (Creds::Unknown, Again::Yes), "put there since");
        assert_eq!(check_at(groq, &d, None, &none, true, Some(written + Duration::from_millis(500))).0, Creds::Unknown, "too close to its start to tell");
        std::fs::write(d.join("auth.json"), r#"{"groq": {"type": "api_key", "key": "gsk"}}"#).unwrap();
        assert_eq!(check_at(groq, &d, None, &none, true, Some(before)), (Creds::Have, Again::Never), "on file too");
        let key = |k: &str| k == "GROQ_API_KEY";
        std::fs::remove_file(d.join("auth.json")).unwrap();
        assert_eq!(check_at(groq, &d, None, &key, true, Some(before)).0, Creds::Have, "in its environment");
        std::fs::remove_dir_all(&d).ok();
    }

    /// "Written before the daemon started" holds only by a margin no file system's or clock's
    /// rounding reaches: a 2 s file time (FAT) rounded down, a start read at 100 ns (Windows).
    #[test]
    fn before_its_start_is_told_at_a_resolution_both_sides_have() {
        let s = SystemTime::UNIX_EPOCH + Duration::new(1_794_625_965, 250_000_000);
        assert!(there_at_start(s - Duration::from_secs(60), s));
        assert!(there_at_start(s - MTIME_SLACK, s));
        assert!(!there_at_start(s - Duration::from_millis(1_999), s), "within 2 s: can't tell");
        assert!(!there_at_start(s, s));
        assert!(!there_at_start(s + Duration::from_nanos(100), s), "after: never");
        // Written 0.5 s after the start, a FAT time (even seconds, rounded down) reads 1.25 s before.
        let fat = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_625_964);
        assert!(!there_at_start(fat, s));
    }

    /// A profile reads its own folder's file first, else the base one (Synaps' `resolve_read_path`).
    #[test]
    fn a_profile_reads_its_folder_first() {
        let d = dir("profile");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none, true, None).0, Creds::Have, "no folder of its own: the base file");
        std::fs::create_dir_all(d.join("work")).unwrap();
        std::fs::write(d.join("work").join("auth.json"), "{}").unwrap();
        assert_eq!(check_at("claude-sonnet-4-6", &d, Some("work"), &none, true, None).0, MISSING);
        std::fs::remove_dir_all(&d).ok();
    }

    /// One read of the files for any number of checks (the Settings page's look checks every
    /// provider): what it found stands after the files change, until the next read.
    #[test]
    fn a_read_once_answers_every_check() {
        let d = dir("once");
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth", "access": "sk-secret"}, "groq": {"type": "api_key", "key": "gsk"}, "nvidia": {"type": "api_key", "key": ""}}"#).unwrap();
        std::fs::write(d.join("config"), "provider.cerebras = csk\nprovider.local.url = http://localhost:1234/v1\n").unwrap();
        let f = Files::read(&d, None);
        std::fs::remove_dir_all(&d).ok();
        let at = |m: &str| judge(m, &f, &none, true, None).0;
        assert_eq!(at("claude-sonnet-4-6"), Creds::Have);
        assert_eq!(at("groq/llama-3.3-70b-versatile"), Creds::Have);
        assert_eq!(at("cerebras/x"), Creds::Have, "the config's key");
        assert_eq!(at("nvidia/x"), Creds::Missing(Some("NVIDIA")), "an empty key isn't one");
        assert_eq!(f.local_url.as_deref(), Some("http://localhost:1234/v1"));
        // As it was read: no folder now, the same answers; a fresh read finds none.
        assert_eq!(at("groq/llama-3.3-70b-versatile"), Creds::Have);
        assert_eq!(check_at("groq/llama-3.3-70b-versatile", &d, None, &none, true, None).0, Creds::Missing(Some("Groq")));
    }

    /// The config's keys as Synaps' map has them: its last line for each, a blank one unset, a
    /// comment none.
    #[test]
    fn the_configs_keys_are_its_last_lines() {
        let c = config_set("# provider.groq = x\nprovider.groq = a\nprovider.groq =\n provider.cerebras = b \nauth.remote_endpoint = \nprovider.xai=c\n");
        let mut keys: Vec<_> = c.iter().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["provider.cerebras", "provider.xai"]);
    }

    /// `provider.local.url` as the daemon read it at its start, as its keys: written before it
    /// (by 2 s), as written; since, what an earlier look found for that start, else as written
    /// (not remembered: a TUI write after its start mustn't drop a URL that works).
    #[test]
    fn the_local_url_is_the_one_read_at_its_start() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_625_965);
        let (before, since) = (t - Duration::from_secs(60), t + Duration::from_secs(60));
        let a = || Some("http://a:1/v1".to_string());
        assert_eq!(url_at_start(a(), Some(since), None, None), (a(), None), "no daemon.json: as written");
        assert_eq!(url_at_start(a(), Some(before), Some(t), None), (a(), Some((t, a()))), "there at its start");
        assert_eq!(url_at_start(a(), None, Some(t), None), (a(), Some((t, a()))), "no time for it: as written");
        assert_eq!(url_at_start(a(), Some(since), Some(t), None), (a(), None), "written since, nothing seen before: as written");
        assert_eq!(url_at_start(a(), Some(t - Duration::from_secs(1)), Some(t), None), (a(), None), "too close to tell: as written");
        // His TUI writes the config (/theme) after its start: the URL seen there before stands.
        let known = (t, Some("http://b:2/v1".to_string()));
        assert_eq!(url_at_start(a(), Some(since), Some(t), Some(&known)), (known.1.clone(), Some(known.clone())));
        let unset = (t, None);
        assert_eq!(url_at_start(a(), Some(since), Some(t), Some(&unset)).0, None, "none at its start: none");
        let other_start = (t - Duration::from_secs(3600), Some("http://old:3/v1".to_string()));
        assert_eq!(url_at_start(a(), Some(since), Some(t), Some(&other_start)), (a(), None), "another start's: not this one's");
        assert_eq!(url_at_start(None, Some(before), Some(t), Some(&known)), (None, Some((t, None))), "taken out before it: none");
    }

    /// Where the daemon's environment can be read, it alone says which keys it has: one only in
    /// hers isn't the daemon's (a ✓ there would be a switch that fails). Unread: hers counts.
    /// Gemini's client id counts in either (a sign-in in his terminal reads it there).
    #[test]
    fn a_readable_daemon_env_decides_alone() {
        let d = dir("envs");
        let files = || Files::read(&d, None);
        let daemon = |keys: &[&str]| Some(crate::platform::Environ { set: keys.iter().map(|k| k.to_string()).collect(), local_endpoint: None });
        // PATH: set in hers (any test run has one), never in this daemon's.
        let read = Seen { files: files(), env: daemon(&["GROQ_API_KEY"]), started: None };
        assert!(!read.env_set("PATH"), "hers only: not the daemon's");
        assert!(read.env_set("GROQ_API_KEY"));
        assert!(read.env_either("PATH") && read.env_either("GROQ_API_KEY"));
        assert_eq!(read.check("groq/llama-3.3-70b-versatile").0, Creds::Have);
        assert_eq!(read.check("cerebras/llama3.1-8b").0, Creds::Missing(Some("Cerebras")));
        let unread = Seen { files: files(), env: None, started: None };
        assert!(unread.env_set("PATH"), "the daemon's unread: hers");
        std::fs::remove_dir_all(&d).ok();
    }

    /// A config she can't read (another process holds it) says nothing of its URL: what she knew
    /// for this start stands and is kept; never remembered as none.
    #[test]
    fn an_unreadable_config_keeps_the_url_known() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_625_965);
        let env = || Some(crate::platform::Environ::default());
        let held = || Files { config: None, local_url: None, config_at: Some(t - Duration::from_secs(3600)), auth: Some(Vec::new()) };
        let seen = Seen { files: held(), env: env(), started: Some(t) };
        let known = (t, Some("http://lm:1234/v1".to_string()));
        assert_eq!(seen.local_url(Some(&known)), ("http://lm:1234/v1".to_string(), Some(known.clone())));
        assert_eq!(seen.local_url(None), ("http://localhost:11434/v1".to_string(), None), "nothing known: not remembered as none");
        let old = (t - Duration::from_secs(60), Some("http://old:1/v1".to_string()));
        assert_eq!(seen.local_url(Some(&old)).0, "http://localhost:11434/v1", "another start's: not this one's");
        // Readable, written since its start, nothing known: as written (his TUI's /theme mustn't
        // drop a URL that works).
        let since = Seen { files: Files { config: Some(HashSet::new()), local_url: Some("http://lm:1234/v1".into()), config_at: Some(t + Duration::from_secs(60)), auth: Some(Vec::new()) }, env: env(), started: Some(t) };
        assert_eq!(since.local_url(None), ("http://lm:1234/v1".to_string(), None));
    }
}
