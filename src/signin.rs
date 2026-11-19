//! Signing in from her (Settings ▸ Brain): `synaps login --provider <login>` run by her with no
//! window, as her installer's sign-in does (`packaging/windows/steps.ps1` `Invoke-Login`). Synaps
//! opens his browser itself: an account's own page with its sign-in ready (Claude, ChatGPT,
//! Gemini, Grok: a PKCE link and a localhost callback; Kimi, Muse: a device link, its code filled
//! in), or GitHub's device page (Copilot: its code is read off Synaps' words, shown large in the
//! Line and copied). An API key goes to Synaps on its stdin: never on its command line, never in
//! a log, never kept.
//!
//! Synaps' stdin is a pipe held open meanwhile: at end of input its 0.12 login gives up ("Manual
//! input channel closed"), and once signed in it may wait on that read. So it closes once
//! `auth.json` has the login or Synaps says it's done; Synaps is killed if it hasn't ended
//! `WIND_DOWN` later, at `WITHIN`, on a stop, and when she quits. Neither its URL nor its code is
//! ever logged.

use crate::provider::{Kind, Provider};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

/// A sign-in gets this long (a device code lasts about as long).
pub const WITHIN: Duration = Duration::from_secs(600);
/// Its login on file (stdin closed): Synaps gets this long to end by itself.
const WIND_DOWN: Duration = Duration::from_secs(15);
/// Synaps ended: its last words get this long to come in.
const LAST_WORDS: Duration = Duration::from_millis(800);
/// How often a sign-in running is looked at.
pub const EVERY: Duration = Duration::from_millis(300);

/// What the Line shows of a sign-in she runs: running, or failed (`failed`: why, in her words).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Signing {
    /// The provider's id.
    pub id: &'static str,
    /// The code he types on the provider's page (Copilot's): shown large, copied.
    pub code: Option<String>,
    /// The page Synaps opened: the Line's way to open it himself.
    pub url: Option<String>,
    pub failed: Option<String>,
}

impl Signing {
    pub fn new(id: &'static str) -> Signing {
        Signing { id, ..Signing::default() }
    }
}

/// Never its code or its URL, wherever a `{:?}` goes.
impl std::fmt::Debug for Signing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signing").field("id", &self.id).field("code", &self.code.is_some()).field("url", &self.url.is_some()).field("failed", &self.failed).finish()
    }
}

/// What a sign-in running says (`Run::poll`).
#[derive(Debug, PartialEq, Eq)]
pub enum Ev {
    /// A code to type on the provider's page.
    Code(String),
    /// The page Synaps opened (or would have).
    Url(String),
    /// It ended: signed in, or why not.
    Done(Result<(), String>),
}

/// One of Synaps' lines, as she reads it.
#[derive(Debug, PartialEq, Eq)]
enum Said {
    Code(String),
    Url(String),
    Ok,
    Failed(String),
}

/// A line without its colours (`\x1b[1m`, `\x1b[0m`).
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A device code: letters, digits and `-`, as GitHub's, Kimi's and Meta's are (`ABCD-1234`).
fn code(t: &str) -> Option<String> {
    let t = t.trim();
    ((4..=24).contains(&t.len()) && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| t.to_string())
}

/// A page to open: https, one word.
fn url(t: &str) -> Option<String> {
    let t = t.trim();
    (t.starts_with("https://") && t.len() < 4096 && !t.contains(char::is_whitespace) && !t.contains(char::is_control)).then(|| t.to_string())
}

/// What Synaps' line says (v0.12.0 `src/cmd/login.rs`, its providers' `login`s): a code, a page,
/// done, or why not. Anything else is its banner.
fn read(line: &str) -> Vec<Said> {
    let l = plain(line);
    let l = l.trim();
    let after = |p: &str| l.split_once(p).map(|(_, r)| r.trim());
    // A cloud login's: `Open <uri> and enter code <code>`.
    if let Some(rest) = l.strip_prefix("Open ")
        && let Some((u, c)) = rest.split_once(" and enter code ")
    {
        return [url(u).map(Said::Url), code(c).map(Said::Code)].into_iter().flatten().collect();
    }
    if let Some(c) = l.strip_prefix("Enter code:").or_else(|| l.strip_prefix("Confirm code:")) {
        return code(c).map(Said::Code).into_iter().collect();
    }
    if let Some(u) = l.strip_prefix("At:") {
        return url(u).map(Said::Url).into_iter().collect();
    }
    if let Some(u) = url(l) {
        return vec![Said::Url(u)];
    }
    if l.starts_with("✓ Login successful") || l.starts_with("✓ API key saved") || l.ends_with("login successful") {
        return vec![Said::Ok];
    }
    if let Some(why) = after("Login failed:").or_else(|| after("✗ Not stored:")).or_else(|| l.strip_prefix("error:").map(str::trim)) {
        return vec![Said::Failed(why.to_string())];
    }
    Vec::new()
}

/// Why, as she says it: Synaps' words without a URL (a reqwest error names its endpoint; never
/// a credential, but she shows no link that isn't the sign-in's) and short.
pub fn why(t: &str) -> String {
    let words: Vec<&str> = t.split_whitespace().map(|w| if w.contains("://") { "(a link)" } else { w }).collect();
    let w = words.join(" ");
    let w = w.trim_end_matches('.');
    if w.chars().count() > 90 { format!("{}…", w.chars().take(89).collect::<String>()) } else { w.to_string() }
}

/// On Windows Synaps 0.12 opens the page with `cmd /c start <url>`, and cmd cuts the URL at its
/// first `&`: the page opens without its sign-in. Its whole URL is on Synaps' stderr: she opens
/// it herself (the shell leaves it whole), when it's the provider's own sign-in page.
pub fn reopen(p: &Provider, u: &str) -> bool {
    let Some(rest) = u.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    // No `user@`, no port: the page itself.
    if host.contains(['@', ':']) || !u.contains('&') {
        return false;
    }
    let ok: &[&str] = match p.id {
        "anthropic" => &["claude.ai", "anthropic.com"],
        "openai-codex" => &["openai.com"],
        "google-gemini" => &["google.com"],
        _ => &[],
    };
    ok.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

fn mtime(f: &Path) -> Option<SystemTime> {
    std::fs::metadata(f).and_then(|m| m.modified()).ok()
}

/// `auth.json` has `p`'s login (its keys and types only: no value is kept): an account's slot
/// (`<id>`, `<id>@<label>`), a service's key (`type: api_key`).
fn on_file(auth: &Path, p: &Provider) -> bool {
    let Ok(t) = std::fs::read_to_string(auth) else { return false };
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(&t) else { return false };
    root.iter().any(|(k, v)| {
        let ty = v.get("type").and_then(|t| t.as_str());
        match p.kind {
            Kind::Key => k == p.id && ty == Some("api_key"),
            _ => (k == p.id || k.strip_prefix(p.id).is_some_and(|r| r.starts_with('@'))) && ty.is_some(),
        }
    })
}

/// A sign-in running: Synaps' `login`, its words read off its stdout and stderr.
pub struct Run {
    pub provider: &'static Provider,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    auth: PathBuf,
    was: Option<SystemTime>,
    started: Instant,
    /// Its stdin closed (it's done, or its login is on file): when.
    closed: Option<Instant>,
    /// Its login is on file (a change to `auth.json` since it started).
    signed: bool,
    said_ok: bool,
    failed: Option<String>,
    code: bool,
    url: bool,
    exited: Option<(ExitStatus, Instant)>,
    done: bool,
}

impl Run {
    /// `synaps` (the daemon's own, else the one on PATH) runs `login --provider <login>` for `p`;
    /// `key`: an API key, written to its stdin, which closes.
    pub fn start(synaps: &Path, profile: Option<&str>, p: &'static Provider, key: Option<&str>) -> std::io::Result<Run> {
        let auth = crate::creds::synaps_file(&synaps_client::discovery::base_dir(), profile, "auth.json");
        Run::start_at(synaps, profile, p, key, auth)
    }

    /// `start`, its login looked for in `auth`.
    fn start_at(synaps: &Path, profile: Option<&str>, p: &'static Provider, key: Option<&str>, auth: PathBuf) -> std::io::Result<Run> {
        let mut c = Command::new(synaps);
        if let Some(pr) = profile.filter(|x| !x.trim().is_empty()) {
            c.args(["--profile", pr]);
        }
        c.args(["login", "--provider", p.login]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }
        let was = mtime(&auth);
        let mut child = c.spawn()?;
        let (tx, lines) = mpsc::channel();
        let outs: [Option<Box<dyn std::io::Read + Send>>; 2] = [child.stdout.take().map(|o| Box::new(o) as _), child.stderr.take().map(|e| Box::new(e) as _)];
        for r in outs.into_iter().flatten() {
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("signin-words".into())
                .spawn(move || {
                    for l in std::io::BufReader::new(r).lines() {
                        let Ok(l) = l else { break };
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                })
                .ok();
        }
        let mut stdin = child.stdin.take();
        if let Some(k) = key
            && let Some(mut i) = stdin.take()
        {
            // Its one line, then end of input: Synaps reads a key off a pipe as a line.
            i.write_all(k.as_bytes()).and_then(|_| i.write_all(b"\n")).ok();
        }
        let closed = stdin.is_none().then(Instant::now);
        Ok(Run { provider: p, child, stdin, lines, auth, was, started: Instant::now(), closed, signed: false, said_ok: false, failed: None, code: false, url: false, exited: None, done: false })
    }

    fn close(&mut self, now: Instant) {
        if self.stdin.take().is_some() || self.closed.is_none() {
            self.closed = Some(now);
        }
    }

    /// When to look again.
    pub fn next(&self, now: Instant) -> Instant {
        now + EVERY
    }

    /// What it said since, and whether it ended.
    pub fn poll(&mut self, now: Instant) -> Vec<Ev> {
        let mut ev = Vec::new();
        if self.done {
            return ev;
        }
        let mut gone = false;
        loop {
            match self.lines.try_recv() {
                Ok(l) => {
                    for s in read(&l) {
                        match s {
                            Said::Code(c) if !self.code => {
                                self.code = true;
                                ev.push(Ev::Code(c));
                            }
                            Said::Url(u) if !self.url => {
                                self.url = true;
                                ev.push(Ev::Url(u));
                            }
                            Said::Ok => {
                                self.said_ok = true;
                                self.close(now);
                            }
                            Said::Failed(w) => {
                                self.failed.get_or_insert(why(&w));
                                self.close(now);
                            }
                            _ => {}
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        // Its login on file (written after it started): it's in, whatever Synaps waits on.
        if !self.signed && mtime(&self.auth).is_some_and(|t| self.was.is_none_or(|w| t > w)) && on_file(&self.auth, self.provider) {
            self.signed = true;
            self.close(now);
        }
        let ok = self.signed || self.said_ok;
        let end = match (self.exited, self.child.try_wait()) {
            (Some((st, at)), _) => (gone || now >= at + LAST_WORDS).then_some(st.success() || ok),
            (None, Ok(Some(st))) => {
                self.exited = Some((st, now));
                gone.then_some(st.success() || ok)
            }
            (None, Ok(None)) if now >= self.started + WITHIN => {
                self.kill();
                self.failed.get_or_insert_with(|| "no sign-in in 10 minutes".into());
                Some(ok)
            }
            (None, Ok(None)) if self.closed.is_some_and(|t| now >= t + WIND_DOWN) => {
                self.kill();
                Some(ok)
            }
            (None, Ok(None)) => None,
            (None, Err(e)) => {
                self.kill();
                self.failed.get_or_insert_with(|| why(&e.to_string()));
                Some(ok)
            }
        };
        if let Some(ok) = end {
            self.done = true;
            let code = self.exited.and_then(|(st, _)| st.code());
            ev.push(Ev::Done(if ok {
                Ok(())
            } else {
                Err(self.failed.clone().unwrap_or_else(|| match code {
                    Some(c) => format!("synaps login ended ({c})"),
                    None => "synaps login stopped".into(),
                }))
            }));
        }
        ev
    }

    /// Stopped: Synaps killed (a stop, a timeout, her quit).
    pub fn kill(&mut self) {
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            self.child.kill().ok();
            self.child.wait().ok();
        }
        self.done = true;
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.done {
            self.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synaps 0.12.0's own lines (`src/cmd/login.rs`, `auth/mod.rs`, `github_copilot.rs`,
    /// `kimi_code.rs`, `muse.rs`), colours and all.
    #[test]
    fn synaps_words_are_read() {
        assert_eq!(read("  Enter code: \x1b[36mABCD-1234\x1b[0m"), [Said::Code("ABCD-1234".into())]);
        assert_eq!(read("  Confirm code: \x1b[36mKX9-22\x1b[0m"), [Said::Code("KX9-22".into())]);
        assert_eq!(read("  At:         \x1b[36mhttps://github.com/login/device\x1b[0m\n"), [Said::Url("https://github.com/login/device".into())]);
        assert_eq!(read("\x1b[36mhttps://claude.ai/oauth/authorize?code=true&client_id=x&state=y\x1b[0m"), [Said::Url("https://claude.ai/oauth/authorize?code=true&client_id=x&state=y".into())]);
        assert_eq!(read("Open https://device.sso.example/ and enter code WXYZ-9876"), [Said::Url("https://device.sso.example/".into()), Said::Code("WXYZ-9876".into())]);
        assert_eq!(read("\n\x1b[32m✓ Login successful!\x1b[0m"), [Said::Ok]);
        assert_eq!(read("\x1b[32m✓ API key saved!\x1b[0m"), [Said::Ok]);
        assert_eq!(read("\x1b[31m✗ Login failed: OAuth denied: access_denied\x1b[0m"), [Said::Failed("OAuth denied: access_denied".into())]);
        assert_eq!(read("\x1b[31m✗ Not stored: this Claude seat is already connected as account 'work'.\x1b[0m"), [Said::Failed("this Claude seat is already connected as account 'work'.".into())]);
        assert_eq!(read("error: unknown provider 'acme'"), [Said::Failed("unknown provider 'acme'".into())]);
        for banner in ["╔══════════════════════════════════════╗", "\x1b[1mOpening browser to sign in...\x1b[0m", "Or paste the redirect URL here (must include `state`):", "  Enter code: not a code!", "At: http://insecure"] {
            assert_eq!(read(banner), [], "{banner}");
        }
    }

    #[test]
    fn why_has_no_link_and_is_short() {
        assert_eq!(why("error sending request for url (https://auth.openai.com/oauth/token?x=1)."), "error sending request for url (a link)");
        assert!(why(&"x".repeat(300)).chars().count() <= 90);
    }

    /// Windows: only the provider's own sign-in page, whole, is opened again.
    #[test]
    fn a_cut_url_is_opened_whole_only_for_its_provider() {
        let claude = crate::provider::by_id("anthropic").unwrap();
        let codex = crate::provider::by_id("openai-codex").unwrap();
        let copilot = crate::provider::by_id("github-copilot").unwrap();
        assert!(reopen(claude, "https://claude.ai/oauth/authorize?code=true&client_id=x"));
        assert!(reopen(codex, "https://auth.openai.com/oauth/authorize?a=1&b=2"));
        assert!(!reopen(codex, "https://claude.ai/oauth/authorize?a=1&b=2"), "another's page");
        assert!(!reopen(claude, "https://claude.ai/oauth/authorize?a=1"), "nothing to cut");
        assert!(!reopen(claude, "https://evil.example/claude.ai?a=1&b=2"));
        assert!(!reopen(claude, "https://claude.ai.evil.example/?a=1&b=2"));
        assert!(!reopen(claude, "https://u@claude.ai/?a=1&b=2"));
        assert!(!reopen(claude, "https://claude.ai:8443/?a=1&b=2"));
        assert!(!reopen(copilot, "https://github.com/login/device?a=1&b=2"), "its page isn't cut");
    }

    #[test]
    fn a_signing_shown_never_has_its_code_or_url() {
        let s = Signing { id: "github-copilot", code: Some("ABCD-1234".into()), url: Some("https://github.com/login/device".into()), failed: None };
        let d = format!("{s:?}");
        assert!(!d.contains("ABCD") && !d.contains("github.com"), "{d}");
    }

    /// A stand-in `synaps` (a shell script): its login written, then it waits on its stdin as
    /// Synaps 0.12 does; closed, it ends. Then one that fails, and one that never ends.
    #[cfg(unix)]
    #[test]
    fn a_sign_in_ends_signed_in_or_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("sa-signin-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        let fake = |name: &str, body: &str| {
            let f = d.join(name);
            std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            f
        };
        let auth = d.join("auth.json");
        let copilot = crate::provider::by_id("github-copilot").unwrap();
        let ok = fake(
            "ok",
            &format!(
                r#"echo "  Enter code: ABCD-1234" >&2
echo "  At:         https://github.com/login/device" >&2
sleep 0.3
echo '{{"github-copilot": {{"type": "oauth"}}}}' > '{}'
read line
echo "done" >&2"#,
                auth.display()
            ),
        );
        let mut r = Run::start_at(&ok, None, copilot, None, auth.clone()).unwrap();
        let t0 = Instant::now();
        let mut seen = Vec::new();
        while !seen.iter().any(|e| matches!(e, Ev::Done(_))) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
            seen.extend(r.poll(Instant::now()));
        }
        assert_eq!(seen, [Ev::Code("ABCD-1234".into()), Ev::Url("https://github.com/login/device".into()), Ev::Done(Ok(()))]);
        let bad = fake("bad", r#"printf '\033[31m✗ Login failed: OAuth denied: access_denied\033[0m\n' >&2; exit 1"#);
        std::fs::remove_file(d.join("auth.json")).ok();
        let mut r = Run::start_at(&bad, None, copilot, None, auth.clone()).unwrap();
        let t0 = Instant::now();
        let mut last = Vec::new();
        while !last.iter().any(|e| matches!(e, Ev::Done(_))) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
            last.extend(r.poll(Instant::now()));
        }
        assert_eq!(last, [Ev::Done(Err("OAuth denied: access_denied".into()))]);
        // Never ending: past WITHIN it's killed (its clock moved on here).
        let hang = fake("hang", "sleep 600");
        let mut r = Run::start_at(&hang, None, copilot, None, auth.clone()).unwrap();
        assert_eq!(r.poll(Instant::now() + WITHIN), [Ev::Done(Err("no sign-in in 10 minutes".into()))]);
        assert!(r.child.try_wait().unwrap().is_some(), "killed");
        // An API key: on its stdin, one line.
        let key = fake("key", r#"read k; [ "$k" = "gsk-123" ] && printf '✓ API key saved!\n' >&2"#);
        let groq = crate::provider::by_id("groq").unwrap();
        let mut r = Run::start_at(&key, None, groq, Some("gsk-123"), auth.clone()).unwrap();
        let t0 = Instant::now();
        let mut last = Vec::new();
        while !last.iter().any(|e| matches!(e, Ev::Done(_))) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
            last.extend(r.poll(Instant::now()));
        }
        assert_eq!(last, [Ev::Done(Ok(()))]);
        std::fs::remove_dir_all(&d).ok();
    }
}
