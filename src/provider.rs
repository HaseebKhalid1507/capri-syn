//! Her BRAIN, as the Settings page means it: the model she thinks with (`brain.rs` is her
//! behaviour, not this). Which Synaps provider a model runs on and how it is signed in to, a short
//! list of models per provider (the Brain screen's MODEL), the `synaps` that signs in (the
//! daemon's `daemon.json`), and a look at a local model server. From Synaps 0.12's own tables
//! (`auth/provider.rs`, `auth/static_providers.rs`, `runtime/openai/mod.rs` `resolve_route`). Never
//! a secret: files are read by key and type only, a key's value is never kept.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How a provider is signed in to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An account in the browser, or a device code: `synaps login --provider <login>`.
    OAuth,
    /// An API key (`synaps login --provider <id>` takes it on stdin), or the broker's env vars.
    Key,
    /// An OpenAI-compatible server on his machine (Ollama, LM Studio): no login at all.
    Local,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Provider {
    /// Synaps' id: `auth.json`'s key and the model's prefix (`openai-codex/gpt-5.5`). Claude is
    /// `anthropic` (and its models may be bare: `claude-sonnet-4-6`).
    pub id: &'static str,
    /// What `synaps login --provider` takes (Claude: `claude`).
    pub login: &'static str,
    /// What she calls it.
    pub name: &'static str,
    pub kind: Kind,
    /// Where Synaps' broker also takes its key from (her environment or the daemon's).
    pub env: &'static [&'static str],
    /// MODEL's choices: the model as `[session] model` takes it, and its name on the page. The
    /// first is the provider's default (what a sign-in switches her to).
    pub models: &'static [(&'static str, &'static str)],
}

const fn oauth(id: &'static str, login: &'static str, name: &'static str, env: &'static [&'static str], models: &'static [(&'static str, &'static str)]) -> Provider {
    Provider { id, login, name, kind: Kind::OAuth, env, models }
}

const fn key(id: &'static str, name: &'static str, env: &'static [&'static str], models: &'static [(&'static str, &'static str)]) -> Provider {
    Provider { id, login: id, name, kind: Kind::Key, env, models }
}

/// Claude: the default, and the first way in.
pub const CLAUDE: Provider = oauth(
    "anthropic",
    "claude",
    "Claude",
    &["ANTHROPIC_API_KEY"],
    &[("claude-sonnet-4-6", "SONNET 4.6"), ("claude-opus-5-5", "OPUS 5.5"), ("claude-haiku-4-5-20251001", "HAIKU 4.5")],
);

/// A model server on his machine: `local/<name>`. Its models are the server's (the probe).
pub const LOCAL: Provider = Provider { id: "local", login: "local", name: "Local", kind: Kind::Local, env: &[], models: &[] };

/// Every provider: Claude, the other accounts in the order OTHER lists them, the local server,
/// then the API-key services (Synaps' `STATIC_PROVIDERS`, with their default models).
pub static ALL: &[Provider] = &[
    CLAUDE,
    oauth("openai-codex", "openai-codex", "ChatGPT", &[], &[("openai-codex/gpt-5.5", "GPT-5.5"), ("openai-codex/gpt-5.4", "GPT-5.4"), ("openai-codex/gpt-5.4-mini", "5.4 MINI")]),
    oauth("github-copilot", "github-copilot", "Copilot", &[], &[("github-copilot/claude-sonnet-4.6", "SONNET 4.6"), ("github-copilot/gpt-5.5", "GPT-5.5")]),
    oauth("google-gemini", "google-gemini", "Gemini", &[], &[("google-gemini/gemini-2.5-pro", "2.5 PRO"), ("google-gemini/gemini-2.5-flash", "2.5 FLASH")]),
    oauth("xai-auth", "xai-auth", "Grok", &[], &[("xai-auth/grok-4.3", "GROK 4.3"), ("xai-auth/grok-4.7", "GROK 4.7")]),
    oauth("kimi-code", "kimi-code", "Kimi", &[], &[("kimi-code/k3", "K3"), ("kimi-code/k3-256k", "K3 256K")]),
    oauth("muse", "muse", "Muse", &["META_API_KEY"], &[("muse/muse-wonder-1.0", "WONDER"), ("muse/muse-spark-1.3", "SPARK")]),
    LOCAL,
    key("groq", "Groq", &["GROQ_API_KEY"], &[("groq/llama-3.3-70b-versatile", "LLAMA 3.3 70B")]),
    key("cerebras", "Cerebras", &["CEREBRAS_API_KEY"], &[("cerebras/llama3.1-8b", "LLAMA 3.1 8B")]),
    key("nvidia", "NVIDIA", &["NVIDIA_API_KEY"], &[("nvidia/meta/llama-3.3-70b-instruct", "LLAMA 3.3 70B")]),
    key("sambanova", "SambaNova", &["SAMBANOVA_API_KEY"], &[("sambanova/Meta-Llama-3.3-70B-Instruct", "LLAMA 3.3 70B")]),
    key("openrouter", "OpenRouter", &["OPENROUTER_API_KEY"], &[("openrouter/meta-llama/llama-3.3-70b-instruct", "LLAMA 3.3 70B")]),
    key("google", "Google AI Studio", &["GOOGLE_API_KEY"], &[("google/gemini-2.5-flash", "GEMINI 2.5 FLASH")]),
    key("deepinfra", "DeepInfra", &["DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN"], &[("deepinfra/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B")]),
    key("huggingface", "Hugging Face", &["HUGGINGFACE_API_KEY", "HF_TOKEN"], &[("huggingface/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B")]),
    key("fireworks", "Fireworks", &["FIREWORKS_API_KEY"], &[("fireworks/accounts/fireworks/models/llama-v3p3-70b-instruct", "LLAMA 3.3 70B")]),
    key("hyperbolic", "Hyperbolic", &["HYPERBOLIC_API_KEY"], &[("hyperbolic/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B")]),
    key("scaleway", "Scaleway", &["SCALEWAY_API_KEY"], &[("scaleway/llama-3.3-70b-instruct", "LLAMA 3.3 70B")]),
    key("siliconflow", "SiliconFlow", &["SILICONFLOW_API_KEY"], &[("siliconflow/Qwen/Qwen3-8B", "QWEN3 8B")]),
    key("together", "Together", &["TOGETHER_API_KEY"], &[("together/meta-llama/Llama-3.3-70B-Instruct-Turbo", "LLAMA 3.3 70B")]),
    key("chutes", "Chutes", &["CHUTES_API_KEY"], &[("chutes/deepseek-ai/DeepSeek-V3-0324", "DEEPSEEK V3")]),
    key("codestral", "Codestral", &["CODESTRAL_API_KEY"], &[("codestral/codestral-latest", "CODESTRAL")]),
    key("perplexity", "Perplexity", &["PERPLEXITY_API_KEY", "PPLX_API_KEY"], &[("perplexity/llama-3.1-sonar-large-128k-online", "SONAR LARGE")]),
    key("ovhcloud", "OVHcloud", &["OVH_AI_ENDPOINTS_ACCESS_TOKEN"], &[("ovhcloud/Meta-Llama-3.3-70B-Instruct", "LLAMA 3.3 70B")]),
    key("inferx", "InferX", &["INFERX_API_KEY"], &[("inferx/deepseek-v4-flash", "DEEPSEEK V4 FLASH")]),
    key("kimi", "Moonshot", &["MOONSHOT_API_KEY", "KIMI_API_KEY"], &[("kimi/kimi-k3.1-pro", "KIMI K3.1 PRO")]),
];

/// The accounts OTHER can list, in order: every sign-in but Claude's (`Status::others`: the
/// ones it does).
pub fn others() -> impl Iterator<Item = &'static Provider> {
    ALL.iter().filter(|p| p.kind == Kind::OAuth && p.id != CLAUDE.id)
}

/// Gemini's sign-in in Synaps 0.12 needs its Google OAuth client from this variable
/// (`google_gemini.rs`, its login and its refresh): without it there's no signing in to it.
pub const GEMINI_CLIENT_ID: &str = "SYNAPS_GOOGLE_GEMINI_CLIENT_ID";

/// The provider a model runs on, as Synaps routes it: bare `claude-*` and `anthropic/…` on
/// Claude, `<id>/…` on that provider. None: a prefix Synaps doesn't know (it fails closed), or
/// a bare model that isn't Claude's.
pub fn of(model: &str) -> Option<&'static Provider> {
    let m = model.trim();
    match m.split_once('/') {
        Some((prefix, rest)) if !rest.is_empty() => ALL.iter().find(|p| p.id == prefix),
        Some(_) => None,
        None => m.to_ascii_lowercase().starts_with("claude").then_some(&ALL[0]),
    }
}

/// The provider Synaps names in an error (`no credential configured for 'kimi-code'`).
pub fn by_id(id: &str) -> Option<&'static Provider> {
    ALL.iter().find(|p| p.id == id)
}

/// The provider she calls `name` (her no-brain line's).
pub fn by_name(name: &str) -> Option<&'static Provider> {
    ALL.iter().find(|p| p.name == name)
}

/// A model's name on the page: its MODEL label where the table has it (`SONNET 4.6`), else the
/// model without its provider (`QWEN3:8B`).
pub fn model_label(model: &str) -> String {
    let m = model.trim();
    if let Some((_, l)) = of(m).and_then(|p| p.models.iter().find(|(id, _)| same(id, m))) {
        return l.to_string();
    }
    let bare = match m.split_once('/') {
        Some((p, rest)) if by_id(p).is_some() => rest,
        _ => m,
    };
    // Claude's own names: `claude-<family>-<major>-<minor>[-<date>]` as its family and version.
    if let Some(rest) = undated(bare).strip_prefix("claude-")
        && let Some((family, v)) = rest.split_once('-')
        && family.chars().all(|c| c.is_ascii_alphabetic())
        && !v.is_empty()
        && v.split('-').all(|n| !n.is_empty() && n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()))
    {
        return format!("{} {}", family.to_uppercase(), v.replace('-', "."));
    }
    bare.to_uppercase()
}

/// A model name without its snapshot's date (`claude-haiku-4-5-20251001`: `claude-haiku-4-5`).
fn undated(m: &str) -> &str {
    match m.rsplit_once('-') {
        Some((head, d)) if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) => head,
        _ => m,
    }
}

/// A model as she says it: its label with its words in title case (`Opus 5.5`, `GPT-5.5`).
pub fn model_spoken(model: &str) -> String {
    model_label(model)
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) if w.chars().all(|x| x.is_ascii_alphabetic()) => f.to_string() + &c.as_str().to_lowercase(),
                _ => w.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `daemon.json` (Synaps' run registry: `<base>/run/daemon[-<profile>].json`): the `synaps` the
/// daemon runs from (`exe`), so a sign-in needn't find it on PATH. None: no file, or no `exe`.
pub fn daemon_exe(run_dir: &Path, profile: Option<&str>) -> Option<PathBuf> {
    daemon_json(run_dir, profile, "exe").map(PathBuf::from)
}

/// When the daemon started (`daemon.json`'s `started_at`; a reload is a new start): Synaps reads
/// its `config` then, once (`core/config.rs`, `PROVIDER_KEYS`). None: no file, or no time in it.
pub fn daemon_started(run_dir: &Path, profile: Option<&str>) -> Option<std::time::SystemTime> {
    utc(&daemon_json(run_dir, profile, "started_at")?)
}

/// A string of `daemon.json`'s (pid, profile, paths, times: never a secret), not blank.
fn daemon_json(run_dir: &Path, profile: Option<&str>, key: &str) -> Option<String> {
    let f = run_dir.join(format!("{}.json", synaps_client::discovery::stem(profile)));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok()?;
    v.get(key).and_then(|e| e.as_str()).map(str::trim).filter(|e| !e.is_empty()).map(str::to_string)
}

/// An RFC 3339 time in UTC as chrono writes it (`2026-11-14T03:12:45.123456789Z`, or `+00:00`).
fn utc(t: &str) -> Option<std::time::SystemTime> {
    let t = t.trim();
    let t = t.strip_suffix('Z').or_else(|| t.strip_suffix("+00:00"))?;
    let (date, time) = t.split_once('T')?;
    let num = |x: Option<&str>| x.filter(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())).and_then(|x| x.parse::<u64>().ok());
    let mut d = date.split('-');
    let (y, m, day) = (num(d.next())?, num(d.next())?, num(d.next())?);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut c = hms.split(':');
    let (h, mi, sec) = (num(c.next())?, num(c.next())?, num(c.next())?);
    if d.next().is_some() || c.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || mi > 59 || sec > 60 || y < 1970 {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let nanos = format!("{frac:0<9}")[..9].parse::<u32>().unwrap_or(0);
    // Days since 1970-01-01 (H. Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let (era, yoe) = (y / 400, y % 400);
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + day - 1;
    let days = (era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy).checked_sub(719_468)?;
    Some(std::time::UNIX_EPOCH + Duration::new(days * 86_400 + h * 3_600 + mi * 60 + sec, nanos))
}

/// `name` is a program on `path` (`$PATH`: Windows tries `.exe` too).
fn on_path(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let Some(path) = path else { return false };
    std::env::split_paths(path).any(|d| d.join(name).is_file() || (cfg!(windows) && d.join(format!("{name}.exe")).is_file()))
}

/// How he runs `synaps` in a terminal: `synaps` when it's on his PATH, else the daemon's own
/// (`daemon.json`), quoted where it has a space; else `synaps` (the installer puts it on PATH).
pub fn synaps_cmd(exe: Option<&Path>, path: Option<&std::ffi::OsStr>) -> String {
    match exe.filter(|_| !on_path("synaps", path)) {
        Some(e) => {
            let e = e.display().to_string();
            match (e.contains(' '), cfg!(windows)) {
                (false, _) => e,
                (true, true) => format!("& \"{e}\""),
                (true, false) => format!("'{e}'"),
            }
        }
        None => "synaps".into(),
    }
}

/// The value Synaps' `config` gives `key`: its last `key = value` line (`#` comments skipped),
/// as Synaps reads it into its map, trimmed. Some(""): set blank (unset, to Synaps).
pub fn config_value<'a>(config: &'a str, key: &str) -> Option<&'a str> {
    config.lines().map(str::trim).filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('=')).filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim()).next_back()
}

/// The local server's base URL as Synaps finds it: `provider.local.url` in its config, then
/// `LOCAL_ENDPOINT` (the daemon's, where it can be read; else hers), then Ollama's
/// (`http://localhost:11434/v1`).
pub fn local_url(config: Option<&str>, env: Option<String>) -> String {
    let set = config.and_then(|c| config_value(c, "provider.local.url")).map(|v| v.trim_matches('"').to_string());
    set.filter(|u| !u.is_empty())
        .or(env.filter(|u| !u.trim().is_empty()))
        .unwrap_or_else(|| "http://localhost:11434/v1".into())
        .trim_end_matches('/')
        .to_string()
}

/// What a look at the local server found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Local {
    /// Nothing answers there (Ollama isn't running).
    Off,
    /// It answers, with these models.
    Up(Vec<String>),
    /// Something answers, but not with a model list (its HTTP status), or it's https (not looked at).
    Odd(u16),
}

impl Local {
    /// It serves `model` (`local/qwen3:8b`'s `qwen3:8b`; Ollama lists an untagged one as `:latest`).
    pub fn has(&self, model: &str) -> bool {
        let Local::Up(ids) = self else { return false };
        let m = model.trim().strip_prefix("local/").unwrap_or(model.trim());
        ids.iter().any(|i| i == m || (!m.contains(':') && *i == format!("{m}:latest")))
    }
}

/// `GET <url>/models` on a plain-http server (a local one: no TLS here), all of it within
/// `timeout`: the name's lookup (on a thread of its own, left behind if it hangs), the connect,
/// the request and the reply (1 MiB at most). A server that never stops talking ends at it too.
pub fn probe(url: &str, timeout: Duration) -> Local {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let end = std::time::Instant::now() + timeout;
    let left = || end.checked_duration_since(std::time::Instant::now()).filter(|d| !d.is_zero());
    let Some(rest) = url.trim().strip_prefix("http://") else { return Local::Odd(0) };
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let host = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let path = format!("/{}/models", path.trim_matches('/')).replace("//", "/");
    // `localhost` is ::1 and 127.0.0.1: Ollama listens on the second only.
    let (tx, rx) = std::sync::mpsc::channel();
    let name = host.clone();
    let looked = std::thread::Builder::new().name("probe-dns".into()).spawn(move || {
        tx.send(name.to_socket_addrs().map(|a| a.collect::<Vec<_>>()).unwrap_or_default()).ok();
    });
    let Some(addrs) = looked.ok().and_then(|_| rx.recv_timeout(left()?).ok()) else { return Local::Off };
    let Some(mut s) = addrs.iter().find_map(|a| std::net::TcpStream::connect_timeout(a, left()?).ok()) else { return Local::Off };
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    if left().is_none_or(|d| s.set_write_timeout(Some(d)).is_err()) || s.write_all(req.as_bytes()).is_err() {
        return Local::Off;
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    while buf.len() < 1 << 20 {
        let Some(d) = left() else { break };
        s.set_read_timeout(Some(d)).ok();
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if buf.is_empty() {
        return Local::Off;
    }
    models_reply(&String::from_utf8_lossy(&buf[..buf.len().min(1 << 20)]))
}

/// An HTTP reply to `GET /models`: an OpenAI-style list (`{"data": [{"id": …}]}`).
fn models_reply(reply: &str) -> Local {
    let status = reply.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    let body = reply.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    if status != 200 {
        return Local::Odd(status);
    }
    match serde_json::from_str::<serde_json::Value>(body.trim()) {
        Ok(v) => match v.get("data").and_then(|d| d.as_array()) {
            Some(list) => Local::Up(list.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str())).map(str::to_string).collect()),
            None => Local::Odd(status),
        },
        Err(_) => Local::Odd(status),
    }
}

/// Why her model can't be changed from the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    /// She was started with `--model`: it wins over `[session] model`.
    Cli,
    /// She rides another session (`--session`): its model is its own.
    Theirs,
}

/// One of MODEL's choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What picking it writes (`[session] model`); None: his own, set by hand, shown as it is.
    pub pick: Option<&'static str>,
    pub label: String,
    /// The one in force.
    pub on: bool,
}

/// The same model: `anthropic/claude-x` is `claude-x`.
fn same(a: &str, b: &str) -> bool {
    let bare = |m: &str| {
        let m = m.trim().to_ascii_lowercase();
        let m = m.strip_prefix("anthropic/").unwrap_or(&m);
        undated(m).to_string()
    };
    bare(a) == bare(b)
}

/// What the Settings page shows of her brain (its BRAIN row and the Brain screen).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Her session's model: the one asked (its write on the way), else as the daemon says it.
    /// None: not attached yet.
    pub model: Option<String>,
    /// It has a login: Some(false) none (NO BRAIN YET), None she can't tell (`creds::check`).
    pub signed_in: Option<bool>,
    /// The accounts with a login here, and the key services with a key (`creds::check` of each
    /// one's default model): the Brain screen's ✓, and a switch to an account with one is a
    /// MODEL write, no sign-in.
    pub signed: Vec<&'static str>,
    /// Why MODEL is locked.
    pub lock: Option<Lock>,
    /// The daemon runs on this machine: a sign-in here reaches it (not one through `--socket`).
    pub here: bool,
    /// How he runs `synaps` in a terminal (`synaps_cmd`).
    pub synaps: String,
    /// The local server (`local_url`), and what a look at it found (None: not looked yet).
    pub local: Option<(String, Local)>,
    /// `GEMINI_CLIENT_ID` is set (hers, or the daemon's): Gemini can be signed in to.
    pub gemini: bool,
}

impl Status {
    /// Her model's provider.
    pub fn provider(&self) -> Option<&'static Provider> {
        self.model.as_deref().and_then(of)
    }

    /// The accounts OTHER lists: Gemini only where it can be signed in to, or has a login.
    pub fn others(&self) -> Vec<&'static Provider> {
        others().filter(|&p| p.id != "google-gemini" || self.gemini || self.signed.contains(&p.id) || self.provider() == Some(p)).collect()
    }

    /// No brain: no login for her model (her tag's NO BRAIN YET).
    pub fn none(&self) -> bool {
        self.signed_in == Some(false)
    }

    /// A working brain, as far as she can tell: a login (on file, or her model answered), or her
    /// local model on a server that answers.
    pub fn ok(&self) -> bool {
        match (self.provider().map(|p| p.kind), &self.local) {
            (Some(Kind::Local), Some((_, l))) => self.model.as_deref().is_some_and(|m| l.has(m)),
            _ => self.signed_in == Some(true),
        }
    }

    /// MODEL's choices: her provider's table (his own model, set by hand, in place of its last),
    /// or hers alone where there is none (a local model: the server's are RUN LOCAL's).
    pub fn choices(&self) -> Vec<Choice> {
        let Some(m) = self.model.as_deref() else { return Vec::new() };
        let mut v: Vec<Choice> = self.provider().map_or(&[][..], |p| p.models).iter().map(|&(id, l)| Choice { pick: Some(id), label: l.to_string(), on: same(id, m) }).collect();
        if !v.iter().any(|c| c.on) {
            v.truncate(2);
            v.push(Choice { pick: None, label: model_label(m), on: true });
        }
        v
    }

    /// The command that signs in to `p` in a terminal (None: Synaps' own list, the API keys).
    pub fn login_cmd(&self, p: Option<&Provider>) -> String {
        let s = if self.synaps.is_empty() { "synaps" } else { self.synaps.as_str() };
        match p {
            Some(p) => format!("{s} login --provider {}", p.login),
            None => format!("{s} login"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_runs_on_the_provider_synaps_routes_it_to() {
        assert_eq!(of("claude-sonnet-4-6").map(|p| p.name), Some("Claude"));
        assert_eq!(of("anthropic/claude-opus-5-5").map(|p| p.name), Some("Claude"));
        assert_eq!(of("openai-codex/gpt-5.5").map(|p| p.name), Some("ChatGPT"));
        assert_eq!(of("xai-auth/grok-4.3").map(|p| p.name), Some("Grok"));
        assert_eq!(of("kimi-code/k3").map(|p| p.name), Some("Kimi"));
        assert_eq!(of("kimi/kimi-k3.1-pro").map(|p| p.name), Some("Moonshot"), "the API key, not the Kimi Code account");
        assert_eq!(of("google/gemini-2.5-flash").map(|p| p.kind), Some(Kind::Key));
        assert_eq!(of("google-gemini/gemini-2.5-pro").map(|p| p.kind), Some(Kind::OAuth));
        assert_eq!(of("openrouter/meta-llama/llama-3.3-70b-instruct").map(|p| p.id), Some("openrouter"));
        assert_eq!(of("local/qwen3:8b").map(|p| p.kind), Some(Kind::Local));
        for m in ["", "gpt-5", "acme/x", "groq/", "xai/grok-4.3"] {
            assert_eq!(of(m), None, "{m}: not one Synaps routes");
        }
    }

    /// Synaps' tables: 7 accounts (Claude first), the local server, 19 API-key services; ids
    /// unique; every default model on its own provider; at most 3 MODEL choices (the page's room).
    #[test]
    fn the_table_is_synaps_own() {
        assert_eq!(ALL[0], CLAUDE);
        assert_eq!(ALL.iter().filter(|p| p.kind == Kind::OAuth).count(), 7);
        assert_eq!(ALL.iter().filter(|p| p.kind == Kind::Key).count(), 19);
        assert_eq!(others().map(|p| p.name).collect::<Vec<_>>(), ["ChatGPT", "Copilot", "Gemini", "Grok", "Kimi", "Muse"]);
        let mut ids: Vec<&str> = ALL.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
        for p in ALL {
            assert!(p.models.len() <= 3, "{}", p.id);
            for (m, label) in p.models {
                assert_eq!(of(m), Some(p), "{m}");
                assert_eq!(model_label(m), *label);
                assert!(!m.contains(char::is_whitespace), "{m}: [session] model takes no spaces");
            }
            assert_eq!(by_name(p.name), Some(p));
        }
        assert_eq!(by_id("anthropic").map(|p| p.login), Some("claude"));
    }

    #[test]
    fn a_model_is_named_by_its_label_or_itself() {
        assert_eq!(model_label("claude-sonnet-4-6"), "SONNET 4.6");
        assert_eq!(model_label("openai-codex/gpt-5.5"), "GPT-5.5");
        assert_eq!(model_label("local/qwen3:8b"), "QWEN3:8B");
        assert_eq!(model_label("claude-opus-4-1"), "OPUS 4.1");
        assert_eq!(model_label("anthropic/claude-haiku-4-5"), "HAIKU 4.5", "the table's, its date aside");
        assert_eq!(model_label("claude-3-7-sonnet-20250219"), "CLAUDE-3-7-SONNET-20250219", "an older shape: as it is");
        assert_eq!(model_spoken("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(model_label("groq/llama-3.1-8b-instant"), "LLAMA-3.1-8B-INSTANT");
        assert_eq!(model_spoken("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_spoken("openai-codex/gpt-5.4-mini"), "5.4 Mini");
        assert_eq!(model_spoken("openai-codex/gpt-5.5"), "GPT-5.5");
        assert_eq!(model_spoken("kimi-code/k3-256k"), "K3 256K");
    }

    #[test]
    fn the_synaps_that_signs_in_is_the_daemons() {
        let d = std::env::temp_dir().join(format!("sa-provider-run-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(daemon_exe(&d, None), None, "no daemon.json");
        std::fs::write(d.join("daemon.json"), r#"{"pid": 7, "protocol_version": 3, "socket": "/r/daemon.sock", "exe": "/home/u/.cargo/bin/synaps"}"#).unwrap();
        std::fs::write(d.join("daemon-work.json"), r#"{"pid": 8, "protocol_version": 3, "socket": "/r/w.sock"}"#).unwrap();
        let exe = daemon_exe(&d, None);
        assert_eq!(exe.as_deref(), Some(Path::new("/home/u/.cargo/bin/synaps")));
        assert_eq!(daemon_exe(&d, Some("work")), None, "a profile's own file, no exe in it");
        // On PATH: the plain word; not: the daemon's own, quoted with a space in it.
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "synaps.exe" } else { "synaps" }), "").unwrap();
        let path = std::env::join_paths([bin.clone()]).unwrap();
        assert_eq!(synaps_cmd(exe.as_deref(), Some(path.as_os_str())), "synaps");
        assert_eq!(synaps_cmd(exe.as_deref(), None), "/home/u/.cargo/bin/synaps");
        assert_eq!(synaps_cmd(None, None), "synaps");
        let spaced = Path::new("/home/First Last/.cargo/bin/synaps");
        assert_eq!(synaps_cmd(Some(spaced), None), if cfg!(windows) { "& \"/home/First Last/.cargo/bin/synaps\"" } else { "'/home/First Last/.cargo/bin/synaps'" });
        let s = Status { synaps: "synaps".into(), ..Status::default() };
        assert_eq!(s.login_cmd(Some(&CLAUDE)), "synaps login --provider claude");
        assert_eq!(s.login_cmd(None), "synaps login", "Synaps' own list of services");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_local_server_is_where_synaps_looks() {
        assert_eq!(local_url(None, None), "http://localhost:11434/v1");
        assert_eq!(local_url(Some("# provider.local.url = http://x\nmodel = a\n"), None), "http://localhost:11434/v1", "a comment isn't a setting");
        assert_eq!(local_url(Some("provider.local.url = http://localhost:1234/v1/\n"), Some("http://y/v1".into())), "http://localhost:1234/v1", "the config first");
        assert_eq!(local_url(Some(""), Some("http://127.0.0.1:8080/v1".into())), "http://127.0.0.1:8080/v1");
        assert_eq!(local_url(Some("provider.local.url = http://a:1/v1\nprovider.local.url = http://b:2/v1\n"), None), "http://b:2/v1", "its last line, as Synaps reads it");
        assert_eq!(local_url(Some("provider.local.url = http://a:1/v1\nprovider.local.url =\n"), Some("http://y/v1".into())), "http://y/v1", "blanked: the variable");
    }

    #[test]
    fn a_model_list_reply_is_read() {
        let ok = "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"object\":\"list\",\"data\":[{\"id\":\"qwen3:8b\"},{\"id\":\"llama3.2:latest\"}]}";
        let l = models_reply(ok);
        assert_eq!(l, Local::Up(vec!["qwen3:8b".into(), "llama3.2:latest".into()]));
        assert!(l.has("local/qwen3:8b") && l.has("llama3.2") && !l.has("local/mistral"));
        assert_eq!(models_reply("HTTP/1.1 404 Not Found\r\n\r\n404 page not found"), Local::Odd(404));
        assert_eq!(models_reply("HTTP/1.1 200 OK\r\n\r\n<html>"), Local::Odd(200));
        assert_eq!(probe("https://example.invalid/v1", Duration::from_millis(50)), Local::Odd(0), "https isn't looked at");
    }

    /// A real server on loopback: a list comes back; a port nothing listens on is Off.
    #[test]
    fn a_probe_reads_a_server_on_this_machine() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = [0u8; 512];
            let n = c.read(&mut req).unwrap();
            assert!(String::from_utf8_lossy(&req[..n]).starts_with("GET /v1/models HTTP/1.0\r\n"));
            c.write_all(b"HTTP/1.0 200 OK\r\n\r\n{\"data\":[{\"id\":\"qwen3:8b\"}]}").unwrap();
        });
        assert_eq!(probe(&format!("http://127.0.0.1:{port}/v1"), Duration::from_secs(2)), Local::Up(vec!["qwen3:8b".into()]));
        t.join().unwrap();
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_eq!(probe(&format!("http://127.0.0.1:{free}/v1"), Duration::from_millis(500)), Local::Off);
    }

    /// One deadline for all of it: a server that never stops talking (each read in time) ends
    /// there, as does a name that doesn't resolve.
    #[test]
    fn a_probe_ends_at_its_deadline() {
        use std::io::Write;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            c.write_all(b"HTTP/1.0 200 OK\r\n\r\n{\"data\":[").ok();
            while c.write_all(b" ").is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let t0 = std::time::Instant::now();
        assert_eq!(probe(&format!("http://127.0.0.1:{port}/v1"), Duration::from_millis(600)), Local::Odd(200), "a list that never ends isn't one");
        assert!(t0.elapsed() < Duration::from_millis(1500), "{:?}", t0.elapsed());
        let t0 = std::time::Instant::now();
        assert_eq!(probe("http://nowhere.invalid:11434/v1", Duration::from_millis(400)), Local::Off);
        assert!(t0.elapsed() < Duration::from_millis(1400), "{:?}", t0.elapsed());
    }

    #[test]
    fn daemon_json_says_when_it_started() {
        let d = std::env::temp_dir().join(format!("sa-provider-started-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(daemon_started(&d, None), None);
        std::fs::write(d.join("daemon.json"), r#"{"pid": 7, "started_at": "2026-11-14T03:12:45.250Z"}"#).unwrap();
        let t = daemon_started(&d, None).unwrap();
        assert_eq!(t.duration_since(std::time::UNIX_EPOCH).unwrap(), Duration::new(1_794_625_965, 250_000_000));
        assert_eq!(utc("1970-01-01T00:00:00Z"), Some(std::time::UNIX_EPOCH));
        assert_eq!(utc("2000-03-01T00:00:00+00:00").map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()), Some(951_868_800));
        // Read to the nanosecond; held at the clock's own resolution (Windows': 100 ns).
        let at_100ns = |t: std::time::SystemTime| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() / 100;
        assert_eq!(utc("2024-02-29T23:59:59.123456789123Z").map(at_100ns), Some(17_092_511_991_234_567));
        for bad in ["", "2026-11-14T03:12:45", "2026-11-14T03:12:45+02:00", "2026-13-01T00:00:00Z", "2026-11-14 03:12:45Z", "2026-11-14T03:12:45.1é2Z", "x-11-14T03:12:45Z"] {
            assert_eq!(utc(bad), None, "{bad}");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_page_knows_her_brain() {
        let s = |model: &str, signed_in: Option<bool>| Status { model: Some(model.into()), signed_in, here: true, ..Status::default() };
        let labels = |s: &Status| s.choices().iter().map(|c| (c.label.clone(), c.on, c.pick.is_some())).collect::<Vec<_>>();
        assert!(s("claude-sonnet-4-6", Some(false)).none());
        assert!(s("claude-sonnet-4-6", Some(true)).ok());
        assert!(!s("claude-sonnet-4-6", None).ok() && !s("claude-sonnet-4-6", None).none(), "can't tell: neither");
        let l = |x: &str, on: bool, pick: bool| (x.to_string(), on, pick);
        assert_eq!(labels(&s("claude-sonnet-4-6", Some(true))), [l("SONNET 4.6", true, true), l("OPUS 5.5", false, true), l("HAIKU 4.5", false, true)]);
        assert_eq!(labels(&s("anthropic/claude-opus-5-5", Some(true))), [l("SONNET 4.6", false, true), l("OPUS 5.5", true, true), l("HAIKU 4.5", false, true)], "the same model, with its provider");
        // His own model, by hand: there, as it is, in force; not one to pick again.
        assert_eq!(labels(&s("claude-opus-4-1", Some(true))), [l("SONNET 4.6", false, true), l("OPUS 5.5", false, true), l("OPUS 4.1", true, false)]);
        // Local: hers alone; the server's (RUN LOCAL) aren't MODEL's.
        let mut lo = s("local/qwen3:8b", None);
        assert!(!lo.ok() && !lo.none());
        assert_eq!(labels(&lo), [l("QWEN3:8B", true, false)]);
        lo.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["llama3.2:latest".into(), "qwen3:8b".into()])));
        assert!(lo.ok());
        lo.local = Some(("http://localhost:11434/v1".into(), Local::Off));
        assert!(!lo.ok());
        assert_eq!(Status::default().choices(), Vec::new(), "no model yet: no choices");
    }
}
