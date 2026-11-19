//! Her BRAIN, as the Settings page means it: the model she thinks with (`brain.rs` is her
//! behaviour, not this). Which Synaps provider a model runs on and how it is signed in to, a short
//! list of models per provider (the Brain screen's MODEL), the `synaps` that signs in (the
//! daemon's `daemon.json`), and a look at a local model server. From Synaps 0.12's own tables
//! (`auth/provider.rs`, `auth/static_providers.rs`, `runtime/openai/mod.rs` `resolve_route`). Never
//! a secret: files are read by key and type only, a key's value is never kept.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Signing in from her: Synaps' `login`, run with no window (`Run`).
#[path = "signin.rs"]
pub mod signin;

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

/// Claude: Synaps' own default model's provider (`claude-sonnet-4-6`, nothing set), the way in
/// its first-run check knows. On the Brain screen it is one account among the others.
pub const CLAUDE: Provider = oauth("anthropic", "claude", "Claude", &["ANTHROPIC_API_KEY"], &[
        ("claude-sonnet-4-6", "SONNET 4.6"), ("claude-fable-5-2", "FABLE 5.2"),
        ("claude-fable-5-1", "FABLE 5.1"), ("claude-fable-5", "FABLE 5"), ("claude-opus-5-5", "OPUS 5.5"),
        ("claude-opus-4-7", "OPUS 4.7"), ("claude-opus-4-6", "OPUS 4.6"),
        ("claude-haiku-4-5-20251001", "HAIKU 4.5"),
    ]);

/// A model server on his machine: `local/<name>`. Its models are the server's (the probe).
pub const LOCAL: Provider = Provider { id: "local", login: "local", name: "Local", kind: Kind::Local, env: &[], models: &[] };

/// Every provider: the accounts in SIGN IN's order (by name: none comes first by favour), the
/// local server, then the API-key services (Synaps' `STATIC_PROVIDERS`). Their models are Synaps
/// 0.12.0's own catalogs (the accounts' static ones, `agent-engine` `openai/catalog/*.rs` and
/// `core/models.rs`; the services', `openai/registry.rs`), its aliases (`grok-latest`) aside;
/// each one's first is what a sign-in switches her to.
pub static ALL: &[Provider] = &[
    oauth("openai-codex", "openai-codex", "ChatGPT", &[], &[
            ("openai-codex/gpt-5.5", "GPT-5.5"), ("openai-codex/gpt-6.1-sol", "GPT-6.1 SOL"),
            ("openai-codex/gpt-6-astra", "GPT-6 ASTRA"), ("openai-codex/gpt-5.6-sol", "GPT-5.6 SOL"),
            ("openai-codex/gpt-5.6-terra", "GPT-5.6 TERRA"), ("openai-codex/gpt-5.6-luna", "GPT-5.6 LUNA"),
            ("openai-codex/gpt-5.4", "GPT-5.4"), ("openai-codex/gpt-5.4-mini", "GPT-5.4 MINI"),
            ("openai-codex/gpt-5.3-codex-spark", "GPT-5.3 CODEX SPARK"),
        ]),
    CLAUDE,
    oauth("github-copilot", "github-copilot", "Copilot", &[], &[
            ("github-copilot/claude-sonnet-4.6", "SONNET 4.6"),
            ("github-copilot/gpt-5.3-codex", "GPT-5.3 CODEX"), ("github-copilot/gpt-5.4", "GPT-5.4"),
            ("github-copilot/gpt-5.4-mini", "GPT-5.4 MINI"), ("github-copilot/gpt-5.6-luna", "GPT-5.6 LUNA"),
            ("github-copilot/gpt-5.6-terra", "GPT-5.6 TERRA"), ("github-copilot/gpt-5-mini", "GPT-5 MINI"),
            ("github-copilot/gpt-5.5", "GPT-5.5"), ("github-copilot/claude-fable-5", "FABLE 5"),
            ("github-copilot/claude-opus-4.7", "OPUS 4.7"), ("github-copilot/claude-opus-4.8", "OPUS 4.8"),
            ("github-copilot/claude-opus-4.8-fast", "OPUS 4.8 FAST"),
            ("github-copilot/claude-sonnet-5", "SONNET 5"), ("github-copilot/claude-haiku-4.5", "HAIKU 4.5"),
            ("github-copilot/gemini-3.1-pro-preview", "GEMINI 3.1 PRO"),
            ("github-copilot/gemini-3.5-flash", "GEMINI 3.5 FLASH"),
        ]),
    oauth("google-gemini", "google-gemini", "Gemini", &[], &[
            ("google-gemini/gemini-2.5-pro", "2.5 PRO"),
            ("google-gemini/gemini-3.1-pro-preview", "3.1 PRO PREVIEW"),
            ("google-gemini/gemini-3-pro-preview", "3 PRO PREVIEW"),
            ("google-gemini/gemini-3.5-flash", "3.5 FLASH"),
            ("google-gemini/gemini-3-flash-preview", "3 FLASH PREVIEW"),
            ("google-gemini/gemini-3.1-flash-lite", "3.1 FLASH LITE"),
            ("google-gemini/gemini-2.5-flash", "2.5 FLASH"),
        ]),
    oauth("xai-auth", "xai-auth", "Grok", &[], &[
            ("xai-auth/grok-4.3", "GROK 4.3"), ("xai-auth/grok-4.20-0309-reasoning", "GROK 4.20 REASONING"),
            ("xai-auth/grok-4.20-0309-non-reasoning", "GROK 4.20 NON-REASONING"),
            ("xai-auth/grok-4.20-multi-agent-0309", "GROK 4.20 MULTI-AGENT"),
            ("xai-auth/grok-4.5", "GROK 4.5"), ("xai-auth/grok-4.6", "GROK 4.6"),
            ("xai-auth/grok-4.7", "GROK 4.7"),
        ]),
    oauth("kimi-code", "kimi-code", "Kimi", &[], &[
            ("kimi-code/k3", "K3"), ("kimi-code/k3-256k", "K3 256K"),
            ("kimi-code/kimi-for-coding", "K2.7 CODING"),
            ("kimi-code/kimi-for-coding-highspeed", "K2.7 CODING HIGHSPEED"),
        ]),
    oauth("muse", "muse", "Muse", &["META_API_KEY"], &[("muse/muse-wonder-1.0", "WONDER 1.0")]),
    LOCAL,
    key("groq", "Groq", &["GROQ_API_KEY"], &[
            ("groq/llama-3.3-70b-versatile", "LLAMA 3.3 70B"), ("groq/llama-3.1-8b-instant", "LLAMA 3.1 8B"),
            ("groq/meta-llama/llama-4-scout-17b-16e-instruct", "LLAMA 4 SCOUT"),
            ("groq/meta-llama/llama-4-maverick-17b-128e-instruct", "LLAMA 4 MAVERICK"),
        ]),
    key("cerebras", "Cerebras", &["CEREBRAS_API_KEY"], &[
            ("cerebras/llama3.1-8b", "LLAMA 3.1 8B"),
            ("cerebras/qwen-3-235b-a22b-instruct-2507", "QWEN3 235B"),
        ]),
    key("nvidia", "NVIDIA", &["NVIDIA_API_KEY"], &[
            ("nvidia/meta/llama-3.3-70b-instruct", "LLAMA 3.3 70B"),
            ("nvidia/qwen/qwen3-coder-480b-a35b-instruct", "QWEN3 CODER 480B"),
            ("nvidia/mistralai/mistral-large-3-675b-instruct-2512", "MISTRAL LARGE 675B"),
            ("nvidia/meta/llama-4-maverick-17b-128e-instruct", "LLAMA 4 MAVERICK"),
            ("nvidia/meta/llama-4-scout-17b-16e-instruct", "LLAMA 4 SCOUT"),
            ("nvidia/nvidia/llama-3.1-nemotron-ultra-253b-v1", "NEMOTRON ULTRA 253B"),
            ("nvidia/mistralai/devstral-2-123b-instruct-2512", "DEVSTRAL 2 123B"),
            ("nvidia/minimaxai/minimax-m2.5", "MINIMAX M2.5"),
            ("nvidia/stepfun-ai/step-3.5-flash", "STEP 3.5 FLASH"),
        ]),
    key("sambanova", "SambaNova", &["SAMBANOVA_API_KEY"], &[
            ("sambanova/Meta-Llama-3.3-70B-Instruct", "LLAMA 3.3 70B"), ("sambanova/QwQ-32B", "QWQ 32B"),
            ("sambanova/Meta-Llama-3.1-8B-Instruct", "LLAMA 3.1 8B"),
            ("sambanova/DeepSeek-R1", "DEEPSEEK R1"),
            ("sambanova/DeepSeek-R1-Distill-Llama-70B", "R1 DISTILL 70B"),
            ("sambanova/Qwen3-32B", "QWEN3 32B"),
        ]),
    key("openrouter", "OpenRouter", &["OPENROUTER_API_KEY"], &[
            ("openrouter/meta-llama/llama-3.3-70b-instruct", "LLAMA 3.3 70B"),
            ("openrouter/qwen/qwen3-coder", "QWEN3 CODER"),
            ("openrouter/deepseek/deepseek-chat-v3-0324", "DEEPSEEK V3"),
            ("openrouter/google/gemma-3-27b-it", "GEMMA 3 27B"),
            ("openrouter/mistralai/mistral-small-3.1-24b-instruct", "MISTRAL SMALL 3.1"),
        ]),
    key("google", "Google AI Studio", &["GOOGLE_API_KEY"], &[
            ("google/gemini-2.5-flash", "GEMINI 2.5 FLASH"), ("google/gemini-2.0-flash", "GEMINI 2.0 FLASH"),
            ("google/gemma-3-27b-it", "GEMMA 3 27B"),
        ]),
    key("deepinfra", "DeepInfra", &["DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN"], &[
            ("deepinfra/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B"),
            ("deepinfra/Qwen/Qwen2.5-Coder-32B-Instruct", "QWEN2.5 CODER 32B"),
            ("deepinfra/deepseek-ai/DeepSeek-V3-0324", "DEEPSEEK V3"),
        ]),
    key("huggingface", "Hugging Face", &["HUGGINGFACE_API_KEY", "HF_TOKEN"], &[
            ("huggingface/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B"),
            ("huggingface/Qwen/Qwen2.5-72B-Instruct", "QWEN2.5 72B"),
        ]),
    key("fireworks", "Fireworks", &["FIREWORKS_API_KEY"], &[
            ("fireworks/accounts/fireworks/models/llama-v3p3-70b-instruct", "LLAMA 3.3 70B"),
            ("fireworks/accounts/fireworks/models/qwen2p5-coder-32b-instruct", "QWEN2.5 CODER 32B"),
        ]),
    key("hyperbolic", "Hyperbolic", &["HYPERBOLIC_API_KEY"], &[
            ("hyperbolic/meta-llama/Llama-3.3-70B-Instruct", "LLAMA 3.3 70B"),
            ("hyperbolic/Qwen/Qwen2.5-Coder-32B-Instruct", "QWEN2.5 CODER 32B"),
            ("hyperbolic/deepseek-ai/DeepSeek-V3-0324", "DEEPSEEK V3"),
        ]),
    key("scaleway", "Scaleway", &["SCALEWAY_API_KEY"], &[
            ("scaleway/llama-3.3-70b-instruct", "LLAMA 3.3 70B"), ("scaleway/qwen3-235b-a22b", "QWEN3 235B"),
        ]),
    key("siliconflow", "SiliconFlow", &["SILICONFLOW_API_KEY"], &[
            ("siliconflow/Qwen/Qwen3-8B", "QWEN3 8B"),
            ("siliconflow/deepseek-ai/DeepSeek-R1", "DEEPSEEK R1"),
        ]),
    key("together", "Together", &["TOGETHER_API_KEY"], &[
            ("together/meta-llama/Llama-3.3-70B-Instruct-Turbo", "LLAMA 3.3 70B"),
            ("together/Qwen/Qwen2.5-Coder-32B-Instruct", "QWEN2.5 CODER 32B"),
            ("together/deepseek-ai/DeepSeek-V3", "DEEPSEEK V3"),
        ]),
    key("chutes", "Chutes", &["CHUTES_API_KEY"], &[("chutes/deepseek-ai/DeepSeek-V3-0324", "DEEPSEEK V3")]),
    key("codestral", "Codestral", &["CODESTRAL_API_KEY"], &[("codestral/codestral-latest", "CODESTRAL")]),
    key("perplexity", "Perplexity", &["PERPLEXITY_API_KEY", "PPLX_API_KEY"], &[("perplexity/llama-3.1-sonar-large-128k-online", "SONAR LARGE")]),
    key("ovhcloud", "OVHcloud", &["OVH_AI_ENDPOINTS_ACCESS_TOKEN"], &[
            ("ovhcloud/Meta-Llama-3.3-70B-Instruct", "LLAMA 3.3 70B"), ("ovhcloud/Qwen/QwQ-32B", "QWQ 32B"),
        ]),
    key("inferx", "InferX", &["INFERX_API_KEY"], &[("inferx/deepseek-v4-flash", "DEEPSEEK V4 FLASH")]),
    key("kimi", "Moonshot", &["MOONSHOT_API_KEY", "KIMI_API_KEY"], &[
            ("kimi/kimi-k3.1-pro", "KIMI K3.1 PRO"),
            ("kimi/kimi-k3.1-pro-highspeed", "KIMI K3.1 PRO HIGHSPEED"), ("kimi/kimi-k3", "KIMI K3"),
            ("kimi/kimi-k2.7-code", "KIMI K2.7 CODE"),
            ("kimi/kimi-k2.7-code-highspeed", "KIMI K2.7 CODE HIGHSPEED"), ("kimi/kimi-k2.6", "KIMI K2.6"),
        ]),
];

/// SIGN IN's accounts, in order (`Status::accounts`: the ones it lists).
pub fn accounts() -> impl Iterator<Item = &'static Provider> {
    ALL.iter().filter(|p| p.kind == Kind::OAuth)
}

/// API KEY's services, in order.
pub fn services() -> impl Iterator<Item = &'static Provider> {
    ALL.iter().filter(|p| p.kind == Kind::Key)
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
        None => m.to_ascii_lowercase().starts_with("claude").then(|| by_id(CLAUDE.id)).flatten(),
    }
}

/// The provider Synaps names in an error (`no credential configured for 'kimi-code'`).
pub fn by_id(id: &str) -> Option<&'static Provider> {
    ALL.iter().find(|p| p.id == id)
}

/// Synaps' own default model (`core/models.rs` `default_model`: Claude's first): hers when no one
/// picked one.
pub fn synaps_default(model: &str) -> bool {
    same(model, CLAUDE.models[0].0)
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

/// The local server's base URL as Synaps finds it: `provider.local.url` in its config (`set`: its
/// value, `config_value`), then `LOCAL_ENDPOINT` (the daemon's, where it can be read; else hers),
/// then Ollama's (`http://localhost:11434/v1`).
pub fn local_url(set: Option<&str>, env: Option<String>) -> String {
    let set = set.map(|v| v.trim().trim_matches('"').to_string());
    set.filter(|u| !u.is_empty())
        .or(env.filter(|u| !u.trim().is_empty()))
        .unwrap_or_else(|| "http://localhost:11434/v1".into())
        .trim_end_matches('/')
        .to_string()
}

/// A URL as she shows it, in her log, her bubble or on a page: nothing up to its last `@` (a
/// password may hold `/`, `?`, `#` unencoded), then nothing from a query's or fragment's first
/// mark on (`?#&=;`: one with an `@` in it leaves only its tail after the cut, `x&key=sk`). An
/// `@` past the host (no local server has one) costs only what's shown.
pub fn url_shown(u: &str) -> String {
    let u = u.trim();
    let (scheme, rest) = u.split_once("://").unwrap_or(("", u));
    let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let rest = rest.split_once(['?', '#', '&', '=', ';']).map_or(rest, |(a, _)| a);
    if scheme.is_empty() { rest.to_string() } else { format!("{scheme}://{rest}") }
}

/// Every URL in a daemon's message as she shows one (`url_shown`; Synaps quotes its local URL
/// whole: `can't reach local endpoint at …`). The rest, its whitespace too, as it is.
pub fn urls_shown(s: &str) -> String {
    s.split_inclusive(char::is_whitespace)
        .map(|w| {
            let t = w.trim_end_matches(char::is_whitespace);
            if t.contains("://") { format!("{}{}", url_shown(t), &w[t.len()..]) } else { w.to_string() }
        })
        .collect()
}

/// The server a URL names, for the probe: its `user:password@` (never sent: a local server takes
/// none) and its query off, the host and path as a URL parser reads them.
fn url_target(u: &str) -> String {
    let u = u.trim();
    let u = u.split_once(['?', '#']).map_or(u, |(a, _)| a);
    let (scheme, rest) = u.split_once("://").unwrap_or(("", u));
    let (host, path) = rest.split_once('/').map_or((rest, None), |(h, p)| (h, Some(p)));
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let scheme = if scheme.is_empty() { String::new() } else { format!("{scheme}://") };
    match path {
        Some(p) => format!("{scheme}{host}/{p}"),
        None => format!("{scheme}{host}"),
    }
}

/// Name lookups (`probe`) still running, left behind by a deadline: no more than `DNS_MAX` at once.
static DNS_LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const DNS_MAX: usize = 2;

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
    let end = std::time::Instant::now() + timeout;
    let left = || end.checked_duration_since(std::time::Instant::now()).filter(|d| !d.is_zero());
    // No `user:password@` (never sent: a local server takes none) and no query.
    let Some(rest) = url_target(url).strip_prefix("http://").map(str::to_string) else { return Local::Odd(0) };
    let (host, path) = rest.split_once('/').map_or((rest.as_str(), ""), |(h, p)| (h, p));
    let host = if host.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) { host.to_string() } else { format!("{host}:80") };
    let path = format!("/{}/models", path.trim_matches('/')).replace("//", "/");
    if !resolvable(&host) {
        return Local::Odd(0);
    }
    let Some(addrs) = resolve(&host, left) else { return Local::Off };
    // What's left split across the addresses (`localhost`: 127.0.0.1, then ::1), so a refused
    // first one (Windows takes 2 s to say so) leaves the next its share.
    let n = addrs.len();
    let Some(mut s) = addrs.iter().enumerate().find_map(|(i, a)| std::net::TcpStream::connect_timeout(a, left()? / (n - i) as u32).ok()) else { return Local::Off };
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

/// `host:port` is an address, or a name a resolver could answer: never a `user:pa` left by a `/`,
/// `?` or `#` unencoded in a password (`url_target` cuts the host at the first), which would go
/// to DNS.
fn resolvable(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    host.parse::<std::net::SocketAddr>().is_ok() || (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)))
}

/// `host:port`'s addresses, by `left()`'s deadline. An address as it is; a name on a thread of
/// its own (`localhost` is ::1 and 127.0.0.1: Ollama listens on the second only), left behind if
/// it hangs, and none started while `DNS_MAX` of those still hang (None: no answer).
fn resolve(host: &str, left: impl Fn() -> Option<Duration>) -> Option<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;
    use std::sync::atomic::Ordering;
    if let Ok(a) = host.parse::<std::net::SocketAddr>() {
        return Some(vec![a]);
    }
    // `localhost` (the default) without a lookup: 127.0.0.1 first (Ollama's), then ::1.
    if let Some((h, port)) = host.rsplit_once(':')
        && h.eq_ignore_ascii_case("localhost")
        && let Ok(port) = port.parse::<u16>()
    {
        return Some(vec![(std::net::Ipv4Addr::LOCALHOST, port).into(), (std::net::Ipv6Addr::LOCALHOST, port).into()]);
    }
    if DNS_LIVE.fetch_add(1, Ordering::SeqCst) >= DNS_MAX {
        DNS_LIVE.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let name = host.to_string();
    let looked = std::thread::Builder::new().name("probe-dns".into()).spawn(move || {
        tx.send(name.to_socket_addrs().map(|a| a.collect::<Vec<_>>()).unwrap_or_default()).ok();
        DNS_LIVE.fetch_sub(1, Ordering::SeqCst);
    });
    if looked.is_err() {
        DNS_LIVE.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    rx.recv_timeout(left()?).ok()
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

/// One of MODEL's models.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What picking it writes (`[session] model`); None: hers, set by hand, shown as it is.
    pub pick: Option<&'static str>,
    pub label: String,
    /// The one in force.
    pub on: bool,
}

/// MODEL's menu, by provider: an account signed in to, a service with a key, the local server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The provider's name on the page (`CLAUDE`, `GROQ`, `LOCAL`).
    pub name: String,
    pub models: Vec<Choice>,
}

/// A local model as `[session] model` takes it (`local/qwen3:8b`), for good: a server's names
/// are few, each kept once (`Pref` is `Copy`, its model a `&'static str` like the tables').
pub fn local_model(id: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static KEPT: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let m = format!("local/{}", id.trim());
    let mut kept = KEPT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&k) = kept.iter().find(|k| **k == m) {
        return k;
    }
    let k: &'static str = Box::leak(m.into_boxed_str());
    kept.push(k);
    k
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
    /// The local server her daemon last couldn't reach (`Link::local_tried`, as shown).
    pub tried: Option<String>,
    /// Her login ran out, or was turned down (her last turn's `LOGGED OUT`): her provider's row
    /// signs in again.
    pub expired: bool,
    /// She runs on Windows (`cfg!(windows)`; a test says which): what Synaps 0.12 can't sign in
    /// to from there (`signin::needs_newer`).
    pub windows: bool,
}

impl Status {
    /// Her model's provider.
    pub fn provider(&self) -> Option<&'static Provider> {
        self.model.as_deref().and_then(of)
    }

    /// The accounts SIGN IN lists: Gemini only where it can be signed in to, or has a login.
    pub fn accounts(&self) -> Vec<&'static Provider> {
        accounts().filter(|&p| p.id != "google-gemini" || self.gemini || self.signed.contains(&p.id) || self.provider() == Some(p)).collect()
    }

    /// `p` has a login here (an account's, a service's key), or is hers with one.
    pub fn has(&self, p: &Provider) -> bool {
        self.signed.contains(&p.id) || (self.provider() == Some(p) && self.signed_in == Some(true))
    }

    /// Her daemon couldn't reach another local server than the one she looks at (`tried`): its
    /// config's URL changed after it started (Synaps reads it then; `synaps daemon reload`).
    pub fn stale_url(&self) -> bool {
        let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
        matches!((&self.tried, &self.local), (Some(t), Some((u, _))) if norm(t) != norm(u))
    }

    /// No brain: no login for her model (her tag's NO BRAIN YET).
    pub fn none(&self) -> bool {
        self.signed_in == Some(false)
    }

    /// A working brain, as far as she can tell: a login (on file, or her model answered), or her
    /// local model on a server that answers, the one her daemon runs with.
    pub fn ok(&self) -> bool {
        match (self.provider().map(|p| p.kind), &self.local) {
            (Some(Kind::Local), Some((_, l))) => !self.stale_url() && self.model.as_deref().is_some_and(|m| l.has(m)),
            _ => self.signed_in == Some(true),
        }
    }

    /// MODEL's menu: every model of every account with a login here and every service with a key
    /// (`signed`, or hers with a login confirmed), and the local server's (`local/<id>`), by
    /// provider in the table's order. Nothing for a login she can't confirm (a fresh install, a
    /// daemon whose files she can't read): no Claude models just because Claude is Synaps'
    /// default. Hers is checked; set by hand and not in its list, it's there as it is, not one to
    /// pick again. Empty: nothing to switch to (sign in first).
    pub fn menu(&self) -> Vec<Group> {
        let mine = self.model.as_deref();
        let hers = self.provider();
        let on = |id: &str| mine.is_some_and(|m| same(id, m) || same(id, &format!("{m}:latest")));
        let mut v = Vec::new();
        for p in ALL {
            let is_hers = hers == Some(p);
            let mut models: Vec<Choice> = match (p.kind, &self.local) {
                (Kind::Local, Some((_, Local::Up(ids)))) if self.here => ids
                    .iter()
                    .map(|id| {
                        let m = local_model(id);
                        Choice { pick: Some(m), label: model_label(m), on: on(m) }
                    })
                    .collect(),
                (Kind::Local, _) => Vec::new(),
                _ if self.has(p) => p.models.iter().map(|&(id, l)| Choice { pick: Some(id), label: l.to_string(), on: on(id) }).collect(),
                _ => Vec::new(),
            };
            if let Some(m) = mine.filter(|_| is_hers && (self.has(p) || p.kind == Kind::Local) && !models.iter().any(|c| c.on)) {
                models.push(Choice { pick: None, label: model_label(m), on: true });
            }
            if !models.is_empty() {
                v.push(Group { name: p.name.to_uppercase(), models });
            }
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

    /// Synaps' tables: 7 accounts by name (none first by favour), the local server, 19 API-key
    /// services; ids unique; every model on its own provider, once, with a label of its own.
    /// The installers' account list is this one (their parity test).
    #[test]
    fn the_table_is_synaps_own() {
        assert_eq!(ALL.iter().filter(|p| p.kind == Kind::OAuth).count(), 7);
        assert_eq!(ALL.iter().filter(|p| p.kind == Kind::Key).count(), 19);
        assert_eq!(accounts().map(|p| p.name).collect::<Vec<_>>(), ["ChatGPT", "Claude", "Copilot", "Gemini", "Grok", "Kimi", "Muse"]);
        assert_eq!(accounts().map(|p| p.login).collect::<Vec<_>>(), ["openai-codex", "claude", "github-copilot", "google-gemini", "xai-auth", "kimi-code", "muse"]);
        assert_eq!(services().count(), 19);
        assert_eq!(by_id("anthropic"), Some(&CLAUDE));
        let mut ids: Vec<&str> = ALL.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
        for p in ALL {
            let mut labels: Vec<&str> = p.models.iter().map(|m| m.1).collect();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(labels.len(), p.models.len(), "{}: a label each", p.id);
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
        assert_eq!(model_label("groq/llama-3.1-8b-instant"), "LLAMA 3.1 8B");
        assert_eq!(model_label("groq/qwen-x"), "QWEN-X", "not in the table: itself");
        assert_eq!(model_spoken("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_spoken("openai-codex/gpt-5.4-mini"), "GPT-5.4 Mini");
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
        let from = |config: &str, env: Option<&str>| local_url(config_value(config, "provider.local.url"), env.map(str::to_string));
        assert_eq!(local_url(None, None), "http://localhost:11434/v1");
        assert_eq!(from("# provider.local.url = http://x\nmodel = a\n", None), "http://localhost:11434/v1", "a comment isn't a setting");
        assert_eq!(from("provider.local.url = http://localhost:1234/v1/\n", Some("http://y/v1")), "http://localhost:1234/v1", "the config first");
        assert_eq!(from("provider.local.url = \"http://localhost:1234/v1\"\n", None), "http://localhost:1234/v1", "quoted");
        assert_eq!(from("", Some("http://127.0.0.1:8080/v1")), "http://127.0.0.1:8080/v1");
        assert_eq!(from("provider.local.url = http://a:1/v1\nprovider.local.url = http://b:2/v1\n", None), "http://b:2/v1", "its last line, as Synaps reads it");
        assert_eq!(from("provider.local.url = http://a:1/v1\nprovider.local.url =\n", Some("http://y/v1")), "http://y/v1", "blanked: the variable");
    }

    /// No credential in a URL she shows (her log, her bubble, the page): everything up to its
    /// last `@`, a query and a fragment go, a password holding `/`, `?` or `#` unencoded too.
    #[test]
    fn a_url_shown_has_no_credentials() {
        assert_eq!(url_shown("http://localhost:11434/v1"), "http://localhost:11434/v1");
        assert_eq!(url_shown("http://bob:hunter2@10.0.0.2:8080/v1"), "http://10.0.0.2:8080/v1");
        assert_eq!(url_shown("http://bob:p@ss@host"), "http://host", "an @ in the password");
        assert_eq!(url_shown("https://tok@host/v1/?api_key=sk-x#frag"), "https://host/v1/");
        assert_eq!(url_shown("http://bob:p/ss@host:1/v1"), "http://host:1/v1", "a / in the password");
        assert_eq!(url_shown("http://bob:p?ss@host:1/v1"), "http://host:1/v1", "a ? in the password");
        assert_eq!(url_shown("http://bob:p#ss@host:1/v1"), "http://host:1/v1", "a # in the password");
        assert_eq!(url_shown("http://host/a@b"), "http://b", "an @ past the host: less shown, never more");
        assert_eq!(url_shown(" http://h:1 "), "http://h:1");
        assert_eq!(url_shown("localhost:11434"), "localhost:11434");
        assert_eq!(url_shown("u:pw@localhost:11434/v1"), "localhost:11434/v1", "no scheme");
        assert_eq!(url_shown("http://[::1]:11434/v1"), "http://[::1]:11434/v1");
        assert_eq!(url_shown("http://h/v1?u=me@x&key=sk"), "http://x", "an @ in a query: none of its key");
        assert_eq!(url_shown("http://h/v1;key=sk"), "http://h/v1");
        assert!(!format!("{:?}", Some((url_shown("http://u:secret@h/v1"), Local::Off))).contains("secret"));
        // The probe's own parse: the server as a URL parser reads it.
        assert_eq!(url_target("http://bob:hunter2@10.0.0.2:8080/v1?k=x"), "http://10.0.0.2:8080/v1");
        assert_eq!(url_target("http://host/a@b"), "http://host/a@b", "an @ past the host is the path's");
    }

    /// Synaps quotes its local URL whole in its errors: every URL in one goes as she shows it,
    /// the rest of it (lines too) as it was.
    #[test]
    fn a_daemons_message_shows_no_credentials() {
        let m = "can't reach local endpoint at http://bob:p/ss@10.0.0.2:8080/v1: connection refused";
        assert_eq!(urls_shown(m), "can't reach local endpoint at http://10.0.0.2:8080/v1: connection refused");
        assert_eq!(urls_shown("a\n  (https://t@h/v1?key=sk-1)\tb"), "a\n  (https://h/v1\tb", "a URL's query and what follows it in the word go");
        assert_eq!(urls_shown("no url here: a@b ?x"), "no url here: a@b ?x");
        assert!(!urls_shown("x http://u:secret@h/v1 y http://v:hunter2@g/").contains("secret") && !urls_shown("x http://u:secret@h/v1 y http://v:hunter2@g/").contains("hunter2"));
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
            let req = String::from_utf8_lossy(&req[..n]).into_owned();
            assert!(req.starts_with("GET /v1/models HTTP/1.0\r\n") && !req.contains("bob") && !req.contains("pw"), "{req}");
            c.write_all(b"HTTP/1.0 200 OK\r\n\r\n{\"data\":[{\"id\":\"qwen3:8b\"}]}").unwrap();
        });
        assert_eq!(probe(&format!("http://bob:pw@127.0.0.1:{port}/v1?x=1"), Duration::from_secs(2)), Local::Up(vec!["qwen3:8b".into()]), "no user:password in the request");
        t.join().unwrap();
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert_eq!(probe(&format!("http://127.0.0.1:{free}/v1"), Duration::from_millis(500)), Local::Off);
    }

    /// An address isn't looked up, nor `localhost`; a name is, unless `DNS_MAX` lookups left
    /// behind still hang. (Not that one comes back down: CI can't count on DNS.)
    #[test]
    fn name_lookups_are_capped_and_localhost_needs_none() {
        use std::sync::atomic::Ordering;
        let left = || Some(Duration::from_secs(2));
        assert_eq!(resolve("127.0.0.1:11434", left), Some(vec!["127.0.0.1:11434".parse().unwrap()]));
        DNS_LIVE.fetch_add(DNS_MAX, Ordering::SeqCst);
        assert_eq!(resolve("ollama.invalid:11434", left), None, "no new lookup");
        assert_eq!(resolve("[::1]:11434", left).map(|a| a.len()), Some(1), "an address still goes");
        let local: Vec<std::net::SocketAddr> = vec!["127.0.0.1:11434".parse().unwrap(), "[::1]:11434".parse().unwrap()];
        assert_eq!(resolve("localhost:11434", left), Some(local.clone()), "localhost: no lookup, so not capped");
        assert_eq!(resolve("LocalHost:11434", left), Some(local));
        DNS_LIVE.fetch_sub(DNS_MAX, Ordering::SeqCst);
    }

    /// A `user:pa` a `/` in a password leaves as the host is never sent to DNS: the probe stops
    /// before any lookup (Odd, not the Off a failed one gives).
    #[test]
    fn a_password_fragment_is_never_looked_up() {
        assert_eq!(probe("http://admin:s3cr/et@lm.lan:1234/v1", Duration::from_millis(400)), Local::Odd(0));
        assert_eq!(probe("http://admin:s3cr?et@lm.lan:1234/v1", Duration::from_millis(400)), Local::Odd(0), "a ?");
        assert!(!resolvable("admin:s3cr:80") && !resolvable("[admin:s3cr]:80") && !resolvable(":80"));
        assert!(resolvable("lm.lan:1234") && resolvable("my-box_2:80") && resolvable("127.0.0.1:1234") && resolvable("[::1]:11434") && resolvable("localhost:11434"));
    }

    /// Her daemon runs with the URL it read at its start: a failure naming another than the one
    /// she looks at isn't a brain, however her look's server answers.
    #[test]
    fn a_local_url_the_daemon_isnt_on_is_no_brain() {
        let up = Local::Up(vec!["qwen3:8b".into()]);
        let mut s = Status { model: Some("local/qwen3:8b".into()), here: true, local: Some(("http://localhost:1234/v1".into(), up)), ..Status::default() };
        assert!(s.ok() && !s.stale_url());
        s.tried = Some("http://localhost:1234/v1/".into());
        assert!(s.ok() && !s.stale_url(), "the same server: it's just down");
        s.tried = Some("http://localhost:11434/v1".into());
        assert!(s.stale_url() && !s.ok(), "Ollama's, from before LM Studio's was set");
        s.local = None;
        assert!(!s.stale_url(), "nothing looked at yet");
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
        assert!(s("claude-sonnet-4-6", Some(false)).none());
        assert!(s("claude-sonnet-4-6", Some(true)).ok());
        assert!(!s("claude-sonnet-4-6", None).ok() && !s("claude-sonnet-4-6", None).none(), "can't tell: neither");
        let mut lo = s("local/qwen3:8b", None);
        assert!(!lo.ok() && !lo.none());
        lo.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["llama3.2:latest".into(), "qwen3:8b".into()])));
        assert!(lo.ok());
        lo.local = Some(("http://localhost:11434/v1".into(), Local::Off));
        assert!(!lo.ok());
    }

    /// MODEL's menu: every model of every login here and the local server's, by provider in
    /// the table's order, hers checked; no cap.
    #[test]
    fn the_menu_is_every_model_she_can_switch_to() {
        let names = |s: &Status| s.menu().iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        let on = |s: &Status| s.menu().iter().flat_map(|g| g.models.clone()).filter(|c| c.on).map(|c| c.label).collect::<Vec<_>>();
        let mut st = Status { model: Some("claude-sonnet-4-6".into()), signed_in: Some(true), here: true, ..Status::default() };
        st.signed = vec!["anthropic", "openai-codex", "groq"];
        st.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["qwen3:8b".into(), "llama3.2:latest".into()])));
        assert_eq!(names(&st), ["CHATGPT", "CLAUDE", "LOCAL", "GROQ"]);
        let menu = st.menu();
        assert_eq!(menu[1].models.len(), CLAUDE.models.len(), "no cap");
        assert_eq!(on(&st), ["SONNET 4.6"]);
        assert_eq!(menu[2].models.iter().map(|c| c.pick).collect::<Vec<_>>(), [Some("local/qwen3:8b"), Some("local/llama3.2:latest")]);
        assert_eq!(local_model("qwen3:8b").as_ptr(), local_model("qwen3:8b").as_ptr(), "kept once");
        // On a local model, untagged: the server's `:latest` is hers.
        st.model = Some("local/llama3.2".into());
        st.signed_in = None;
        assert_eq!(on(&st), ["LLAMA3.2:LATEST"]);
        // His own model, by hand: there, in force, not one to pick again.
        st.model = Some("claude-opus-4-1".into());
        st.signed_in = Some(true);
        let mine: Vec<_> = st.menu()[1].models.iter().filter(|c| c.on).cloned().collect();
        assert_eq!(mine, [Choice { pick: None, label: "OPUS 4.1".into(), on: true }]);
        // Hers without a login: not hers to pick; can't tell (a fresh install): nothing either,
        // no Claude models for Synaps' default.
        let none = Status { model: Some("claude-sonnet-4-6".into()), signed_in: Some(false), here: true, ..Status::default() };
        assert!(none.menu().is_empty(), "nothing signed in, no local model: sign in first");
        let unsure = Status { signed_in: None, ..none.clone() };
        assert!(unsure.menu().is_empty(), "no confirmed login: no models");
        let confirmed = Status { signed_in: Some(true), ..none.clone() };
        assert_eq!(names(&confirmed), ["CLAUDE"], "her model answered: its login, confirmed");
        // A local model of hers the server doesn't have: there, in force, not one to pick.
        let off = Status { model: Some("local/qwen3:8b".into()), signed_in: None, here: true, local: Some(("http://localhost:11434/v1".into(), Local::Off)), ..Status::default() };
        assert_eq!(off.menu(), [Group { name: "LOCAL".into(), models: vec![Choice { pick: None, label: "QWEN3:8B".into(), on: true }] }]);
        assert!(synaps_default("claude-sonnet-4-6") && synaps_default("anthropic/claude-sonnet-4-6") && !synaps_default("claude-opus-5-5"));
        // A daemon on another machine: its local server isn't this one's.
        let far = Status { here: false, local: Some(("http://localhost:11434/v1".into(), Local::Up(vec!["qwen3:8b".into()]))), ..none };
        assert!(far.menu().is_empty());
        assert_eq!(Status::default().menu(), Vec::new(), "no model yet: nothing");
    }
}
