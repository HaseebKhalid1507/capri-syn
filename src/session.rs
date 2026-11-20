//! The session link: one Synaps session drives the body. Pure state, no GPUI: daemon
//! events go in, client actions and a body snapshot come out. The view applies the
//! snapshot (a mood only when it changes, the reply into the bubble, the pending prompt)
//! and runs the actions on the `Client`.
//!
//! Rules from lynx's client notes: watch in `Observe` (a Mirror attach keeps the session
//! from parking and makes it eligible for keep-warm pings, which cost tokens) and switch
//! to `Mirror` only to submit, answer or abort, back to Observe once the turn is done;
//! drop every event whose `attach_seq` is not the current one, treat `Attached` as a full
//! replace (also after a reload or a mode switch), rebuild on a history `Replace` and
//! `resync` when the transcript refuses an update, and hand refused input back to the user.
//! Never attach to `subconscious` (pinned; the memory keeper).

use crate::anim::Mood;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use synaps_client::protocol::{LlmEvent, PromptRequest, SessionStreamEvent, StreamEvent};
use synaps_client::transcript::ItemKind;
use synaps_client::{AttachMode, CreateOptions, Cwd, Error, Event, SessionEvent, SessionMeta, Setting, Status, Transcript};

/// The session the avatar never attaches to, by name or by id (pinned; the memory keeper).
pub const GUARDED: &str = "subconscious";
/// After the turn is done and nothing is pending, stay in Mirror this long, then Observe.
const MIRROR_LINGER: Duration = Duration::from_secs(3);
/// Done shows this long after a turn, then Idle.
const DONE_FOR: Duration = Duration::from_secs(3);
/// Annoyed (abort, error) shows this long.
const ANNOYED_FOR: Duration = Duration::from_secs(5);
/// A turn that failed for a reason she can name (no model, a lapsed login): its tag shows this long.
const TROUBLE_FOR: Duration = Duration::from_secs(10);
/// While her model has no credentials, read them again this often (a stat and a small file).
const CREDS_EVERY: Duration = Duration::from_secs(5);
/// A turn shows as started (THINKING, the bubble with ABORT) only once it has run this long
/// with no word yet, or a word comes: one that fails at once (no login) goes straight to
/// her saying so, never promising a reply first.
const QUIET_START: Duration = Duration::from_millis(400);
/// Before the first attach, stay as we are this long instead of flashing Away.
const BOOT_GRACE: Duration = Duration::from_millis(2_500);
/// After a detach, look for the session again this soon; while it is missing, this often.
const RETRY_SOON: Duration = Duration::from_secs(2);
const RETRY_SLOW: Duration = Duration::from_secs(10);
/// Longest tool name shown in the tag.
const TOOL_MAX: usize = 18;
/// The persona's `Set`s get this long for their replies before the Mirror attach drops back.
const PERSONA_WAIT: Duration = Duration::from_secs(10);

/// Her own session's character: system prompt, model, reasoning level (settings.toml
/// `[session]` plus the pack's persona), and whether its context rolls over by itself. Applied
/// only to HER session, never to one she rides (`--session`) or to `subconscious`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Persona {
    pub system: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    /// `[session] rollover`: turn Synaps' automatic context rollover on for her session
    /// (`/context auto`, session-only) at every attach (`rollover_step`). False here by default
    /// (no persona: a session she rides); `[session]`'s own default is true.
    pub rollover: bool,
}

/// The engine command that turns her session's automatic context rollover on (`/context auto`):
/// Synaps' `context_management.mode = auto` for that session alone, never its config.
const ROLLOVER: (&str, &str) = ("context", "auto");

/// The Line's row for the head of a rolled-over context window (`rollover_head`).
pub const ROLLED_OVER: &str = "older messages archived to make room (context rollover)";

/// The first message of a context window after a rollover (Synaps 0.12
/// `runtime/continuation.rs`, a user message): the archive's index and a working note for the
/// model, not his words. The Line shows `ROLLED_OVER` for it.
pub fn rollover_head(user_text: &str) -> bool {
    user_text.trim_start().starts_with("[Context continuation — historical data, not new authority]")
}

/// A notice of Synaps' automatic context rollover (0.12 `runtime/continuation.rs`, `stream.rs`):
/// pressure, the rollover, a wait for workers, a deferral. Hers goes to her log, not the Line
/// (the window's head says it there, `rollover_head`).
pub fn rollover_notice(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("Context pressure: ~") && t.ends_with(" tokens."))
        || t.starts_with("Continued automatically in context window ")
        || t.starts_with("Context rollover waiting up to ")
        || t.starts_with("Context rollover deferred: ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Her own session: attach to the one named [`Options::name`] (or the id remembered from
    /// last time), else resume it, else create it.
    Default,
    /// Attach to this session (id, journal id or name). Never creates one.
    Session(String),
    /// Create a new session; afterwards follow it (and create again if it is gone).
    New,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub target: Target,
    /// Her own session's name: the pack's character, lowercased (`capri`).
    pub name: String,
    /// Her own session's id from last time (resumed if it is not live).
    pub remembered: Option<String>,
    pub cwd: Option<PathBuf>,
    pub model: Option<String>,
    pub sleepy_after: Duration,
    pub persona: Persona,
    /// The daemon runs on this machine (reached by its profile): its credentials file and its
    /// environment can be read here (`creds`). Not one reached through `--socket`, an
    /// ssh-forwarded remote daemon's: its login and its pid are another machine's, and its
    /// first error says what it lacks.
    pub local: bool,
}

/// `name` (or a name given for an id) is the guarded session.
pub fn guarded(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(GUARDED)
}

/// A prompt's answer, an API key: never printed, and wiped when it goes.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl Secret {
    /// Its text, handed on (to the daemon's answer): the caller holds it now, this one is empty.
    pub fn take(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Its bytes zeroed before its memory is freed (volatile: the writes aren't optimised away).
impl Drop for Secret {
    fn drop(&mut self) {
        // SAFETY: zero bytes are valid UTF-8; the length stays as it is until it's cleared.
        for b in unsafe { self.0.as_mut_vec() }.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        self.0.clear();
    }
}

/// What the view should do on the client.
#[derive(Debug, Clone)]
pub enum Act {
    Attach(String, AttachMode),
    Create(CreateOptions),
    Resume(String, CreateOptions),
    List,
    Resync,
    Submit(String),
    Steer(String),
    Answer(u64, Option<Secret>),
    Abort,
    Detach,
    /// A setting of her own session (the persona), from the Mirror attach.
    Set(Setting),
    /// A runtime-backed slash command on her own session (`/context auto`: `[session]
    /// rollover`), from the Mirror attach: name and argument.
    EngineCommand(String, String),
    /// Her own session's id: remember it for next time.
    Remember(String),
    /// A guard tripped: say why and stop.
    Fatal(String),
}

/// Input that needs the Mirror attach.
#[derive(Debug, Clone)]
enum Input {
    Text(String),
    Answer(u64, Option<Secret>),
    Abort,
}

/// One-off effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Once {
    /// Re-attached after a loss: play the appear glitch.
    Appear,
    /// Input the daemon refused: give it back to the user.
    Restore(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptView {
    pub id: u64,
    pub title: String,
    pub text: String,
    /// A secret (password, API key): masked input. Otherwise a yes/no confirm.
    pub secret: bool,
}

/// What the body shows, derived from the session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub mood: Mood,
    /// Status label override (`WORKING · bash`, `COMPACTING`).
    pub label: Option<String>,
    /// The reply of the current (or last) turn, cleaned for the bubble, plus a notice.
    pub reply: String,
    /// The reply as written (markdown), for the reader.
    pub raw: String,
    /// Bumps for every new reply: a new bubble.
    pub turn: u64,
    /// The turn still runs (more text may come).
    pub busy: bool,
    /// The user's line of the running turn.
    pub echo: Option<String>,
    /// Who started the current turn: the daemon's `TurnStarted.trigger` (`user`, `event_auto`,
    /// ...); empty for the reply an attach shows, and after a clear.
    pub trigger: String,
    pub prompt: Option<PromptView>,
    /// Every pending prompt, oldest first (`prompt` is the first), whole text.
    pub prompts: Vec<PromptView>,
    /// Attached and connected: input can go out.
    pub live: bool,
    /// The last turn failed for a reason she says in her own words.
    pub trouble: Option<Trouble>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attach {
    None,
    Pending(u64),
    Live(u64),
    Reattaching(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Thinking,
    Talking,
    Working(String),
}

pub struct Link {
    opts: Options,
    started: Instant,
    connected: bool,
    attach: Attach,
    ever_live: bool,
    /// Lost a live attachment since: the next attach plays the appear glitch.
    lost: bool,
    /// The session we follow once attached (or the one `--session` named).
    follow: Option<String>,
    /// Where the default target's attach is: resume the remembered id, create by name, ...
    creating: Option<Creating>,
    /// Mode of the current (or pending) attachment.
    mode: AttachMode,
    /// The pending attach only switches the mode on the session we follow: the body stays.
    switching: bool,
    /// Input waiting for the switch to Mirror.
    queued: Vec<Input>,
    /// The text field is open: go to (and stay in) Mirror.
    typing: bool,
    /// Last user input or turn end: Mirror lingers this long after it.
    touched: Instant,
    /// Not connected since (the view re-creates the client after a long outage).
    down_since: Option<Instant>,
    busy: bool,
    phase: Phase,
    compacting: bool,
    prompts: Vec<PromptRequest>,
    /// This turn's assistant text, raw.
    raw: String,
    /// Where the current response's text starts in `raw` (a reset drops it).
    response_at: usize,
    notice: Option<String>,
    /// The last turn's failure, when she can name it (cleared by the next turn).
    trouble: Option<Trouble>,
    /// Whether her model has credentials (`creds`): known before his first message, so a fresh
    /// install reads NO BRAIN YET from the start, not ONLINE. The daemon's own no-login error and
    /// any turn that answers settle it too.
    creds: Creds,
    /// When to read the credentials again (`creds_due`): at each attach or model change, and every
    /// `CREDS_EVERY` while missing (a `synaps login` in a terminal clears it without a message).
    creds_at: Option<Instant>,
    /// Who stands behind `creds` (#76): what a read can take back, and what it can't.
    creds_by: Said,
    /// The local server the daemon last couldn't reach, as its error names it (`local_tried`):
    /// the URL it runs with, which may not be its config's now (Synaps reads that at its start).
    /// Gone with an answer or a (re)connect (`synaps daemon reload`).
    local_tried: Option<String>,
    /// The last settled read's own answer (`set_creds`), whatever `creds` kept: none since the
    /// last model change or answer.
    creds_read: Option<(Creds, crate::creds::Again)>,
    /// Synaps' config sets a `model` (a read's; None: not read, or unreadable): with none, and
    /// none of hers, her model is Synaps' default, picked by no one (`unpicked`).
    synaps_model: Option<bool>,
    /// The thread's error rows made while no one had picked her model (`unpicked` then), by
    /// transcript id: their words stay as they were said (a pick made later doesn't reword them).
    unpicked_errors: Vec<u64>,
    /// The daemon's profile and pid (`Connected`): where its credentials are, and its env.
    daemon: Option<(Option<String>, u32)>,
    /// The daemon's version (`Welcome`), for her maker's mark.
    daemon_version: Option<String>,
    /// When the running turn started (`QUIET_START`), and the last turn's reply, raw text and
    /// trigger, shown until it does.
    turn_at: Instant,
    before: (String, String, String),
    turn: u64,
    echo: Option<String>,
    trigger: String,
    flash: Option<(Mood, Instant)>,
    last_active: Instant,
    /// Asleep, as her day has it (`brain::Brain::napping`: away, locked, a doze; BRIEF-alive
    /// Q1). None (no brain): the old rule, Sleepy after `sleepy_after` of quiet.
    resting: Option<bool>,
    retry_at: Option<Instant>,
    /// Sent but not yet seen as a turn or steer: comes back if refused.
    pending_input: Option<String>,
    once: Vec<Once>,
    transcript: Transcript,
    /// The session that is hers (created or resumed by her, or named as her): the only one
    /// the persona is ever applied to.
    own: Option<String>,
    /// The session's settings as last seen (`Attached.view`, `SettingApplied.view`).
    view: Option<serde_json::Value>,
    /// This connection's client id and who owns input.
    me: u64,
    owner: Option<u64>,
    /// The persona was compared (and set, or skipped) on this session: once per session.
    persona_done: Option<String>,
    /// `Set`s waiting for their reply, and since when.
    persona_wait: usize,
    persona_sent: Instant,
    /// What those `Set`s make the view (its keys): compared against while they're on their way,
    /// so a change asked back before the reply (A, B, A) still goes out.
    persona_asked: serde_json::Map<String, serde_json::Value>,
    /// The last compare found another client owning her session's input: nothing was set.
    persona_blocked: bool,
    /// Rollover was turned on (or tried, or skipped) on this session since it last came back:
    /// `/context auto` is session-only, so a re-attach after a loss (a daemon reload, an unpark,
    /// a resume: Synaps builds the session again from its config) sends it again.
    rollover_done: Option<String>,
    /// Its command on its way, since: Mirror stays for the reply (`PERSONA_WAIT` at most).
    rollover_sent: Option<Instant>,
    /// Synaps keeps its context archive here (0.12: Unix only). A daemon on this machine is on
    /// this OS; one through `--socket` may be another's: it is asked, and says.
    archive_here: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Creating {
    ResumeId,
    Named,
    ResumeName,
    Unnamed,
}

impl Link {
    pub fn new(opts: Options, now: Instant) -> Link {
        let follow = match &opts.target {
            Target::Session(s) => Some(s.clone()),
            _ => None,
        };
        Link {
            opts,
            started: now,
            connected: false,
            attach: Attach::None,
            ever_live: false,
            lost: false,
            follow,
            creating: None,
            mode: AttachMode::Observe,
            switching: false,
            queued: Vec::new(),
            typing: false,
            touched: now,
            down_since: Some(now),
            busy: false,
            phase: Phase::Thinking,
            compacting: false,
            prompts: Vec::new(),
            raw: String::new(),
            response_at: 0,
            notice: None,
            trouble: None,
            creds: Creds::Unknown,
            creds_at: None,
            creds_by: Said::Read,
            creds_read: None,
            synaps_model: None,
            unpicked_errors: Vec::new(),
            local_tried: None,
            daemon: None,
            daemon_version: None,
            turn_at: now,
            before: Default::default(),
            turn: 0,
            echo: None,
            trigger: String::new(),
            flash: None,
            last_active: now,
            resting: None,
            retry_at: None,
            pending_input: None,
            once: Vec::new(),
            transcript: Transcript::new(),
            own: None,
            view: None,
            me: 0,
            owner: None,
            persona_done: None,
            persona_wait: 0,
            persona_asked: serde_json::Map::new(),
            persona_blocked: false,
            persona_sent: now,
            rollover_done: None,
            rollover_sent: None,
            archive_here: cfg!(unix),
        }
    }

    /// What differs between the persona and the session's settings, in the order to set
    /// them (a model change may clamp the reasoning level; the prompt last).
    fn persona_diffs(&self, view: &serde_json::Value) -> Vec<Setting> {
        let p = &self.opts.persona;
        let mut out = Vec::new();
        if let Some(m) = &p.model
            && view.get("model").and_then(|v| v.as_str()) != Some(m.as_str())
        {
            out.push(Setting::model(m.clone()));
        }
        if let Some(t) = &p.thinking
            && !view.get("reasoning_level").and_then(|v| v.as_str()).is_some_and(|v| v.eq_ignore_ascii_case(t))
        {
            out.push(Setting::reasoning_level(t.clone()));
        }
        if let Some(s) = &p.system
            && view.get("system_prompt").and_then(|v| v.as_str()).map(str::trim) != Some(s.trim())
        {
            out.push(Setting::system_prompt(s.clone()));
        }
        out
    }

    /// Bring her own session in line with the persona: compare its settings, switch to
    /// Mirror only if something differs (Set is the input owner's), send just that, once.
    fn persona_step(&mut self, now: Instant) -> Vec<Act> {
        let Some(id) = self.follow.clone() else { return Vec::new() };
        if self.own.as_deref() != Some(id.as_str()) || self.persona_done.as_deref() == Some(id.as_str()) || self.switching || !self.live() {
            return Vec::new();
        }
        // The guard, again: never anyone else's session, never the memory keeper's.
        if matches!(self.opts.target, Target::Session(_)) || guarded(&id) || guarded(&self.opts.name) {
            self.persona_done = Some(id);
            return Vec::new();
        }
        let Some(view) = &self.view else { return Vec::new() };
        // Her Sets on their way count as in (their replies bring the view).
        let mut view = view.clone();
        if self.persona_wait > 0
            && now < self.persona_sent + PERSONA_WAIT
            && let Some(v) = view.as_object_mut()
        {
            v.extend(self.persona_asked.clone());
        }
        let diffs = self.persona_diffs(&view);
        self.persona_blocked = false;
        if diffs.is_empty() {
            self.persona_done = Some(id);
            return Vec::new();
        }
        let ours = if self.mode == AttachMode::Mirror { self.owner == Some(self.me) } else { self.owner.is_none() };
        if !ours {
            elog!("synaps-avatar: persona: another client owns input on her session; not set");
            self.persona_done = Some(id);
            self.persona_blocked = true;
            return Vec::new();
        }
        if self.mode != AttachMode::Mirror {
            return self.upgrade();
        }
        self.persona_done = Some(id);
        self.persona_wait += diffs.len();
        self.persona_sent = now;
        self.touched = now;
        let p = &self.opts.persona;
        for d in &diffs {
            let v = match d.name() {
                "model" => &p.model,
                "reasoning_level" => &p.thinking,
                _ => &p.system,
            };
            if let Some(v) = v {
                self.persona_asked.insert(d.name().into(), v.clone().into());
            }
        }
        elog!("synaps-avatar: persona: setting {} on her session", diffs.iter().map(Setting::name).collect::<Vec<_>>().join(", "));
        diffs.into_iter().map(Act::Set).collect()
    }

    /// `[session] rollover`: turn Synaps' automatic context rollover on for her own session
    /// (`/context auto`), so a companion that never ends doesn't fill her context. Session-only on
    /// Synaps' side (its config's `context_management.mode`, and every other session, untouched),
    /// so once per attach that may have a new session under it (`rollover_done`). Needs input, as
    /// a Set does: from the Mirror attach, never past another client's. One log line for what came
    /// of it: on, refused and why (`rollover_replied`), or skipped and why.
    fn rollover_step(&mut self, now: Instant) -> Vec<Act> {
        let Some(id) = self.follow.clone() else { return Vec::new() };
        if !self.opts.persona.rollover || self.own.as_deref() != Some(id.as_str()) || self.rollover_done.as_deref() == Some(id.as_str()) || self.switching || !self.live() {
            return Vec::new();
        }
        if matches!(self.opts.target, Target::Session(_)) || guarded(&id) || guarded(&self.opts.name) {
            self.rollover_done = Some(id);
            return Vec::new();
        }
        // Synaps 0.12 refuses it off Unix ("context archive is currently supported on Unix only"):
        // not asked for, so no Mirror switch for nothing. A daemon through `--socket` is asked.
        if !self.archive_here && self.opts.local {
            elog!("synaps-avatar: rollover: not turned on: Synaps keeps its context archive on Linux and macOS only (not on Windows yet)");
            self.rollover_done = Some(id);
            return Vec::new();
        }
        let ours = if self.mode == AttachMode::Mirror { self.owner == Some(self.me) } else { self.owner.is_none() };
        if !ours {
            elog!("synaps-avatar: rollover: not turned on: another client owns input on her session");
            self.rollover_done = Some(id);
            return Vec::new();
        }
        if self.mode != AttachMode::Mirror {
            return self.upgrade();
        }
        self.rollover_done = Some(id);
        self.rollover_sent = Some(now);
        self.touched = now;
        vec![Act::EngineCommand(ROLLOVER.0.into(), ROLLOVER.1.into())]
    }

    /// The reply to `/context auto`: its `QueryResult` value (`{kind, text}`), one log line.
    fn rollover_replied(&mut self, value: &serde_json::Value) {
        self.rollover_sent = None;
        let text = value.get("text").and_then(|t| t.as_str()).unwrap_or("");
        match value.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            // `context management: auto | window 1 | capacity 200000 | pressure 140000 | ...`
            "output" => elog!("synaps-avatar: rollover: on for her session ({})", one_line(text.lines().next().unwrap_or(""), 160)),
            "unhandled" => elog!("synaps-avatar: rollover: not turned on: this Synaps has no /context command"),
            "error" => elog!("synaps-avatar: rollover: not turned on: {}", one_line(text, 200)),
            other => elog!("synaps-avatar: rollover: not turned on: an answer of kind `{other}`: {}", one_line(text, 160)),
        }
    }

    /// The model she has asked her session for and it hasn't taken yet (her persona's: `--model`
    /// or `[session] model`): the Settings page shows it meanwhile. None once it is in, or once
    /// it couldn't be set (another client owns her session's input: the daemon's stands).
    pub fn model_on_way(&self) -> Option<&str> {
        self.opts.persona.model.as_deref().filter(|_| self.persona_wait > 0 || self.persona_done.is_none())
    }

    /// The last compare of her persona found another client owning her session's input.
    pub fn persona_blocked(&self) -> bool {
        self.persona_blocked
    }

    /// The `--model` she was started with.
    pub fn cli_model(&self) -> Option<String> {
        self.opts.model.clone()
    }

    /// The session's model as the daemon last said it (`claude-sonnet-4-6`, `local/qwen3:8b`).
    pub fn model(&self) -> Option<&str> {
        self.view.as_ref().and_then(|v| v.get("model")).and_then(|m| m.as_str()).map(str::trim).filter(|m| !m.is_empty())
    }

    /// Whether her model has credentials, as far as she can tell (her tag's NO BRAIN YET).
    pub fn creds(&self) -> Creds {
        self.creds
    }

    /// The local server the daemon last couldn't reach (`url_shown`, its `/chat/completions`
    /// off), until an answer or a (re)connect.
    pub fn local_tried(&self) -> Option<&str> {
        self.local_tried.as_deref()
    }

    /// Her credentials have been looked at since she attached: a read settled, the daemon or an
    /// answer said, or there is nothing here to read (a daemon through `--socket`). Her first
    /// words wait for it a little (a fresh install's are about her brain).
    pub fn creds_settled(&self) -> bool {
        !self.opts.local || self.creds != Creds::Unknown || self.creds_read.is_some() || self.creds_by != Said::Read
    }

    /// The daemon runs on this machine (not one through `--socket`): its logins are here.
    pub fn local(&self) -> bool {
        self.opts.local
    }

    /// The daemon's profile and pid, once connected (where its files are; its environment).
    pub fn daemon_at(&self) -> (Option<String>, Option<u32>) {
        self.daemon.clone().map_or((None, None), |(p, pid)| (p, Some(pid)))
    }

    /// When the next credentials read is: only while live, and only for a daemon on this machine.
    fn creds_next(&self) -> Option<Instant> {
        self.creds_at.filter(|_| self.opts.local && self.live())
    }

    /// A credentials read is due (`creds::check`): what to read them for.
    pub fn creds_due(&self, now: Instant) -> Option<crate::creds::Query> {
        if self.creds_next().is_none_or(|t| now < t) {
            return None;
        }
        let (profile, pid) = self.daemon_at();
        Some(crate::creds::Query { model: self.model()?.to_string(), profile, pid })
    }

    /// What the read found, and whether reading again can change it (`creds::Again`): if so, read
    /// again in `CREDS_EVERY`; if not (a login found, another provider, a remote broker), the
    /// reads end until the next attach or model change. A read never says she has no brain once
    /// she has answered on this model and daemon (only the daemon's own error can). A settled
    /// Unknown takes back a Missing a read said, once it can't tell (a remote broker set up
    /// instead of a login, #76). One the daemon said stands while the files read as they did
    /// when it spoke (another provider, the daemon's environment unread): its next answer, or a
    /// read that finds the login, clears that; a read that changed (a remote broker since) takes
    /// it back too. A file mid-write or unreadable (Unknown, read again) changes neither.
    /// true: what she shows of her login changed (`creds`).
    pub fn set_creds(&mut self, c: Creds, again: crate::creds::Again, now: Instant) -> bool {
        use crate::creds::Again;
        if self.creds_by == Said::Answer {
            self.creds_at = None;
            return false;
        }
        // A file mid-write or unreadable says nothing of the setup: whoever said Missing, it
        // stands and she reads again. Not what the daemon spoke over, nor a change under it.
        let settled = (c, again) != (Creds::Unknown, Again::Yes);
        if settled {
            self.creds_read = Some((c, again));
        }
        let missing = matches!(self.creds, Creds::Missing(_));
        let keep = missing
            && c == Creds::Unknown
            && (!settled
                || match &mut self.creds_by {
                    // No read when it spoke (it was quicker): the first settled one is what it spoke over.
                    Said::Daemon(over) => *over.get_or_insert((c, again)) == (c, again),
                    _ => false,
                });
        let c = if keep { self.creds } else { c };
        let again = match again {
            Again::Yes => true,
            Again::IfMissing => matches!(c, Creds::Missing(_)),
            Again::Never => false,
        };
        self.creds_at = again.then_some(now + CREDS_EVERY);
        let changed = c != self.creds;
        if changed {
            elog!("synaps-avatar: her model's credentials: {c:?}");
        }
        self.creds = c;
        if !keep {
            self.creds_by = Said::Read;
        }
        changed
    }

    /// The last turn's failure she can name (cleared by the next turn).
    pub fn trouble(&self) -> Option<&Trouble> {
        self.trouble.as_ref()
    }

    /// What a read found of Synaps' config `model` (`creds::check`); None says nothing new.
    pub fn set_synaps_model(&mut self, set: Option<bool>) {
        if set.is_some() {
            self.synaps_model = set;
        }
    }

    /// No one picked her model: no `[session] model`, no `--model`, not his session, Synaps'
    /// config sets none (read on this machine), and her session is on Synaps' default (a session
    /// resumed on another model was put on it: picked). It's Synaps' default (Claude's), not a
    /// choice: with no login for it, she says to pick a brain, not to sign in to Claude.
    pub fn unpicked(&self) -> bool {
        self.synaps_model == Some(false)
            && self.opts.persona.model.is_none()
            && self.opts.model.is_none()
            && !self.rides_other()
            && self.model().is_none_or(crate::provider::synaps_default)
    }

    /// The error row `id` (a transcript item) was made while no one had picked her model: its
    /// words ask for a pick, as they did then.
    pub fn unpicked_error(&self, id: u64) -> bool {
        self.unpicked_errors.contains(&id)
    }

    /// A sign-in from her just landed: her credentials are read again now (not after an answer
    /// on this model: that stands), and a login that ran out (`LOGGED OUT`) is in again.
    pub fn creds_now(&mut self, now: Instant) {
        if self.creds_by != Said::Answer {
            self.creds_at = Some(now);
        }
        if self.trouble == Some(Trouble::LoggedOut) {
            self.trouble = None;
        }
    }

    /// `model` (a token count's, when the daemon names one) is hers: none named, or hers without
    /// its provider (`claude-sonnet-4-6` is `anthropic/claude-sonnet-4-6`).
    fn hers(&self, model: Option<&str>) -> bool {
        let bare = |m: &str| m.trim().rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        match model.filter(|m| !m.trim().is_empty()) {
            None => true,
            Some(m) => self.model().is_some_and(|me| bare(me) == bare(m)),
        }
    }

    /// The daemon answered on her model (a word, a thought, a tool call, the provider's token
    /// count): a working brain, whatever a read of the files says. Reads stop until the next
    /// model change; only the daemon's own no-login error puts NO BRAIN YET back.
    fn answered(&mut self) {
        if self.creds_by != Said::Answer {
            elog!("synaps-avatar: her model answered: a brain");
        }
        self.creds = Creds::Have;
        self.creds_by = Said::Answer;
        self.creds_read = None;
        self.creds_at = None;
        self.local_tried = None;
    }

    /// Her maker's mark, for the menu and Settings' ABOUT: `SYNAPS 0.12.0 · SONNET 4.6` (the
    /// daemon's version and her session's model as the page names it, `provider::model_label`).
    /// None until the daemon has said either.
    pub fn mark(&self) -> Option<String> {
        let model = self.model().map(crate::provider::model_label);
        match (self.daemon_version.as_deref(), model) {
            (None, None) => None,
            (Some(v), None) => Some(format!("SYNAPS {v}")),
            (None, Some(m)) => Some(format!("SYNAPS · {m}")),
            (Some(v), Some(m)) => Some(format!("SYNAPS {v} · {m}")),
        }
    }

    /// Riding someone else's session (`--session`): no persona, ever.
    pub fn rides_other(&self) -> bool {
        matches!(self.opts.target, Target::Session(_))
    }

    /// `settings reload`: a new persona is compared (and set) again.
    pub fn set_persona(&mut self, p: Persona, now: Instant) -> Vec<Act> {
        if p == self.opts.persona {
            return Vec::new();
        }
        self.pick_persona(p, now)
    }

    /// The Settings page's MODEL: compared (and set) even when it is the persona already, as her
    /// session may have moved off it (his TUI set another). A `rollover` turned on since goes out
    /// too (one already on this session isn't sent again; one turned off stays as it is there).
    pub fn pick_persona(&mut self, p: Persona, now: Instant) -> Vec<Act> {
        self.opts.persona = p;
        self.persona_done = None;
        let mut acts = self.persona_step(now);
        acts.extend(self.rollover_step(now));
        acts
    }

    fn current(&self) -> Option<u64> {
        match self.attach {
            Attach::None => None,
            Attach::Pending(s) | Attach::Live(s) | Attach::Reattaching(s) => Some(s),
        }
    }

    /// Attached (or only switching modes on the session): input can be taken.
    pub fn live(&self) -> bool {
        self.connected && (matches!(self.attach, Attach::Live(_)) || (self.switching && matches!(self.attach, Attach::Pending(_))))
    }

    /// The mode the link is in (or switching to).
    pub fn mode(&self) -> AttachMode {
        self.mode
    }

    /// Not connected to the daemon for this long.
    pub fn offline_for(&self, now: Instant) -> Option<Duration> {
        self.down_since.map(|t| now.saturating_duration_since(t))
    }

    /// A new client replaces the old one: everything attached is gone.
    pub fn reconnect(&mut self, now: Instant) {
        if matches!(self.attach, Attach::Live(_) | Attach::Reattaching(_)) {
            self.lost = true;
        }
        self.attach = Attach::None;
        self.connected = false;
        self.down_since = Some(now);
        self.switching = false;
        self.mode = AttachMode::Observe;
        self.restore_queued();
        self.busy = false;
        self.prompts.clear();
        self.retry_at = None;
        self.persona_wait = 0;
        self.persona_asked.clear();
        self.rollover_sent = None;
    }

    /// The user's text (Enter): it goes out in Mirror, now or once the switch is done.
    pub fn submit(&mut self, text: String, now: Instant) -> Result<Vec<Act>, String> {
        if !self.live() {
            return Err("not attached to a session".into());
        }
        Ok(self.input(Input::Text(text), now))
    }

    pub fn answer(&mut self, prompt_id: u64, value: Option<String>, now: Instant) -> Vec<Act> {
        if !self.live() {
            return Vec::new();
        }
        self.input(Input::Answer(prompt_id, value.map(Secret)), now)
    }

    pub fn abort(&mut self, now: Instant) -> Vec<Act> {
        if !self.live() {
            return Vec::new();
        }
        self.input(Input::Abort, now)
    }

    /// He woke her (the field, the Line, $mod+c, a summon): Sleepy ends now (her day's nap
    /// ends with it; without one, it comes back after `sleepy_after` more of quiet).
    pub fn wake(&mut self, now: Instant) {
        self.last_active = self.last_active.max(now);
        if let Some(r) = &mut self.resting {
            *r = false;
        }
    }

    /// Whether she's asleep by her day (present and quiet is awake). The view sets it from the
    /// brain at every decision; until then (or without one) the quiet clock decides.
    pub fn set_resting(&mut self, on: bool) {
        self.resting = Some(on);
    }

    pub fn resting(&self) -> Option<bool> {
        self.resting
    }

    /// The text field opened (switch to Mirror ahead of Enter) or closed.
    pub fn set_typing(&mut self, on: bool, now: Instant) -> Vec<Act> {
        self.typing = on;
        self.touched = now;
        if on { self.upgrade() } else { Vec::new() }
    }

    fn input(&mut self, i: Input, now: Instant) -> Vec<Act> {
        self.touched = now;
        if self.mode == AttachMode::Mirror && !self.switching && matches!(self.attach, Attach::Live(_)) {
            return self.send(i);
        }
        self.queued.push(i);
        self.upgrade()
    }

    /// Switch the followed session to Mirror (unless it is there or on the way).
    fn upgrade(&mut self) -> Vec<Act> {
        if self.mode == AttachMode::Mirror || !self.live() {
            return Vec::new();
        }
        let Some(id) = self.follow.clone() else { return Vec::new() };
        self.mode = AttachMode::Mirror;
        self.switching = true;
        vec![Act::Attach(id, AttachMode::Mirror)]
    }

    fn send(&mut self, i: Input) -> Vec<Act> {
        match i {
            Input::Text(t) => {
                self.pending_input = Some(t.clone());
                vec![if self.busy { Act::Steer(t) } else { Act::Submit(t) }]
            }
            Input::Answer(id, v) => vec![Act::Answer(id, v)],
            Input::Abort => vec![Act::Abort],
        }
    }

    /// Queued text goes back to the user; answers and aborts are dropped.
    fn restore_queued(&mut self) {
        for i in std::mem::take(&mut self.queued) {
            if let Input::Text(t) = i {
                self.once.push(Once::Restore(t));
            }
        }
    }

    fn refuse(&mut self, what: &str) -> Vec<Act> {
        let msg = format!("refusing to attach to `{what}`: that is the `{GUARDED}` session (pinned, the memory keeper)");
        self.retry_at = None;
        vec![Act::Fatal(msg)]
    }

    /// Everything said in the session as the client holds it (the Line's thread).
    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// The view sent an attach/create/resume that returned `seq`.
    pub fn attaching(&mut self, seq: u64) {
        self.attach = Attach::Pending(seq);
    }

    #[cfg(test)]
    /// The user's text went out (submit or steer).
    pub fn submitted(&mut self, text: String) {
        self.pending_input = Some(text);
    }

    pub fn take_once(&mut self) -> Vec<Once> {
        std::mem::take(&mut self.once)
    }

    pub fn on_event(&mut self, ev: Event, now: Instant) -> Vec<Act> {
        if let Some(s) = ev.attach_seq()
            && Some(s) != self.current()
        {
            return Vec::new(); // stale: an attachment we have left
        }
        let mut acts = Vec::new();
        match ev {
            Event::Status(s) => {
                let up = matches!(s, Status::Connected { .. });
                if let Status::Connected { daemon_version, profile, pid, .. } = &s {
                    // A reload reads its config again: the URL it couldn't reach is the old one's.
                    self.local_tried = None;
                    self.daemon_version = Some(daemon_version.trim().to_string()).filter(|v| !v.is_empty());
                    // Another daemon process is another brain: an answer from the last one proves
                    // nothing here (#76). The attach reads again.
                    if self.creds_by == Said::Answer && self.daemon.as_ref().is_some_and(|(p, id)| p != profile || id != pid) {
                        self.creds = Creds::Unknown;
                        self.creds_by = Said::Read;
                        self.creds_read = None;
                    }
                    self.daemon = Some((profile.clone(), *pid));
                }
                if self.connected && !up && matches!(self.attach, Attach::Live(_)) {
                    self.lost = true;
                }
                if up {
                    self.down_since = None;
                } else if self.down_since.is_none() {
                    self.down_since = Some(now);
                }
                self.connected = up;
            }
            Event::Sessions(list) => {
                if self.connected && self.attach == Attach::None && self.retry_at.is_none_or(|t| t <= now) {
                    self.retry_at = None;
                    acts.extend(self.resolve(&list, now));
                }
            }
            Event::Attached { attach_seq, attached, reconnect, mode, .. } => {
                if let Some(name) = attached.meta.name.as_deref().filter(|n| guarded(n)) {
                    // Got here by an id: leave at once.
                    self.attach = Attach::None;
                    let mut acts = vec![Act::Detach];
                    acts.extend(self.refuse(&format!("{} ({name})", attached.meta.id)));
                    return acts;
                }
                if let Err(e) = self.transcript.load_attached(&attached) {
                    elog!("synaps-avatar: attach: {e}");
                }
                // Hers: created or resumed by her (not for --session), or carrying her name (or
                // no name and the id she remembers). A renamed session is someone's now.
                let id = attached.meta.id.as_str();
                let mine = match &self.opts.target {
                    Target::Session(_) => false,
                    Target::New => self.creating.is_some() || self.own.as_deref() == Some(id),
                    Target::Default => match attached.meta.name.as_deref() {
                        Some(n) => n == self.opts.name,
                        None => self.creating.is_some() || self.own.as_deref() == Some(id) || self.opts.remembered.as_deref() == Some(id),
                    },
                };
                self.own = mine.then(|| id.to_string());
                {
                    // One line per attach (rare): which session, how, and what its settings are,
                    // so a persona Set (or its absence) can be read off the log.
                    let v = &attached.view;
                    let sys = v.get("system_prompt").and_then(|s| s.as_str());
                    let persona_sys = self.opts.persona.system.as_deref().map(str::trim);
                    elog!(
                        "synaps-avatar: attached {} ({}) {mode:?}{} mine={mine}: model={} reasoning={} system={}",
                        id.get(..8).unwrap_or(id),
                        attached.meta.name.as_deref().unwrap_or("-"),
                        if reconnect { " reconnect" } else { "" },
                        v.get("model").and_then(|m| m.as_str()).unwrap_or("?"),
                        v.get("reasoning_level").and_then(|m| m.as_str()).unwrap_or("?"),
                        match sys {
                            None => "absent".to_string(),
                            Some(t) => format!("{} chars{}", t.len(), if persona_sys == Some(t.trim()) { " (the persona)" } else { "" }),
                        }
                    );
                }
                // Another model is another brain: read for it afresh. An answer on this one
                // stands (#76): no read.
                let model = attached.view.get("model");
                if self.view.as_ref().and_then(|v| v.get("model")) != model {
                    self.creds = Creds::Unknown;
                    self.creds_by = Said::Read;
                    self.creds_read = None;
                }
                self.view = Some(attached.view.clone());
                self.creds_at = (self.creds_by != Said::Answer).then_some(now);
                self.me = attached.client;
                self.owner = attached.input_owner;
                // A mode switch on the session we follow: same body, no new bubble.
                let switch = self.switching && !reconnect && self.follow.as_deref() == Some(attached.meta.id.as_str());
                let again = reconnect || self.lost;
                // Back after a loss: the session may be another runtime now (a reload, an unpark),
                // without the session-only rollover. A mode switch keeps it.
                if again {
                    self.rollover_done = None;
                }
                self.attach = Attach::Live(attach_seq);
                self.mode = mode;
                self.switching = false;
                self.follow = Some(attached.meta.id.clone());
                self.creating = None;
                self.ever_live = true;
                self.lost = false;
                self.busy = attached.streaming;
                self.phase = Phase::Thinking;
                self.compacting = false;
                self.prompts = attached.pending_prompts.clone();
                self.pending_input = None;
                if !switch {
                    self.flash = None;
                    self.echo = None;
                    self.notice = None;
                    self.last_active = now;
                }
                self.touched = now;
                // The last reply stays reachable from the chip.
                let items = self.transcript.items();
                let mut parts: Vec<&str> = items
                    .iter()
                    .rev()
                    .take_while(|i| i.kind != ItemKind::User)
                    .filter(|i| i.kind == ItemKind::Assistant)
                    .map(|i| i.text.as_str())
                    .collect();
                parts.reverse();
                let raw = parts.join("\n\n");
                // A switch keeps what was streamed if the snapshot says the same.
                // (A turn with no user message reads back with the replies before it.)
                let same = |a: &str, b: &str| a == b || (!b.is_empty() && a.ends_with(b));
                if !(switch && same(&clean(&raw), &clean(&self.raw))) {
                    self.turn += 1;
                    self.trigger.clear();
                    self.raw = raw;
                }
                self.response_at = self.raw.len();
                if attached.rejected.total() > 0 {
                    elog!("synaps-avatar: attach snapshot: {} entries did not decode", attached.rejected);
                }
                if again {
                    self.once.push(Once::Appear);
                }
                if self.opts.target == Target::Default && self.opts.remembered.as_deref() != Some(attached.meta.id.as_str()) {
                    self.opts.remembered = Some(attached.meta.id.clone());
                    acts.push(Act::Remember(attached.meta.id.clone()));
                }
                if mode == AttachMode::Mirror {
                    for i in std::mem::take(&mut self.queued) {
                        acts.extend(self.send(i));
                    }
                }
                acts.extend(self.persona_step(now));
                acts.extend(self.rollover_step(now));
            }
            Event::InputOwner { client, owner, .. } => {
                self.me = client;
                self.owner = owner;
            }
            Event::SettingApplied { setting, view, .. } => {
                elog!("synaps-avatar: persona: {setting} set");
                self.view_now(view, now);
                self.persona_replied();
            }
            Event::Error { error: Error::SettingRefused { setting, reason, .. }, .. } => {
                // Logged, never shown: she just keeps the session's own settings.
                elog!("synaps-avatar: persona: {setting} not set: {reason}");
                self.persona_replied();
            }
            // Her `/context auto`: logged, never shown (nor any text of his given back).
            Event::EngineReply { name, value, .. } if name == ROLLOVER.0 => self.rollover_replied(&value),
            Event::Error { error: Error::EngineCommandRefused { name, reason, .. }, .. } if name == ROLLOVER.0 => {
                self.rollover_sent = None;
                elog!("synaps-avatar: rollover: not turned on: {}", one_line(&reason, 200));
            }
            Event::Reattaching { attach_seq, reason, .. } => {
                elog!("synaps-avatar: session connection lost ({reason}); re-attaching");
                self.attach = Attach::Reattaching(attach_seq);
                self.lost = true;
                if let Some(t) = self.pending_input.take() {
                    self.once.push(Once::Restore(t));
                }
            }
            Event::Detached { reason, .. } => {
                elog!("synaps-avatar: detached: {reason}");
                let was = self.attach;
                self.attach = Attach::None;
                if matches!(was, Attach::Live(_) | Attach::Reattaching(_)) || self.switching {
                    self.lost = true;
                }
                // A failed resume or create of her own session: the next way in.
                if matches!(was, Attach::Pending(_)) && !self.switching {
                    self.creating = match self.creating {
                        Some(Creating::ResumeId) => Some(Creating::Named),
                        Some(Creating::Named) => Some(Creating::ResumeName),
                        Some(Creating::ResumeName) => Some(Creating::Unnamed),
                        c => c,
                    };
                }
                self.switching = false;
                self.mode = AttachMode::Observe;
                self.persona_wait = 0;
                self.persona_asked.clear();
                self.rollover_sent = None;
                self.restore_queued();
                self.busy = false;
                self.compacting = false;
                self.prompts.clear();
                self.echo = None;
                if let Some(t) = self.pending_input.take() {
                    self.once.push(Once::Restore(t));
                }
                self.retry_at = Some(now + RETRY_SOON);
            }
            Event::History { update, .. } => {
                if self.transcript.apply_update(&update).is_err() {
                    acts.push(Act::Resync);
                }
            }
            Event::Session { event, raw, .. } => {
                // A setting changed by anyone (his TUI's `/model`, `/thinking`): the daemon sends
                // the new view to every attached client, and the client makes only her own Sets'
                // replies `SettingApplied`. Hers come here too, with the view she has already.
                if raw.get("ev").and_then(|e| e.as_str()) == Some("setting_changed")
                    && raw.pointer("/applied/ok").and_then(|o| o.as_bool()) == Some(true)
                    && let Some(view) = raw.pointer("/applied/view").filter(|v| v.is_object())
                {
                    self.view_now(view.clone(), now);
                }
                self.transcript.apply(&event);
                self.session_event(event, now);
            }
            Event::Error { error, .. } => match error {
                Error::Refused { command, reason } => {
                    elog!("synaps-avatar: {} refused: {reason}", command.as_deref().unwrap_or("command"));
                    if let Some(t) = self.pending_input.take() {
                        self.once.push(Once::Restore(t));
                    }
                }
                Error::Decode(e) => elog!("synaps-avatar: session event did not decode: {e}"),
                other => elog!("synaps-avatar: {other}"),
            },
            Event::CwdFallback { missing, .. } => elog!("synaps-avatar: cwd {missing} is gone on the daemon's machine; running in ~"),
            _ => {}
        }
        acts
    }

    fn session_event(&mut self, ev: SessionEvent, now: Instant) {
        match ev {
            SessionEvent::TurnStarted { user_text, trigger, .. } => {
                self.before = (self.reply_text(), self.raw.trim().to_string(), std::mem::take(&mut self.trigger));
                self.trigger = trigger;
                self.busy = true;
                self.phase = Phase::Thinking;
                self.raw.clear();
                self.response_at = 0;
                self.notice = None;
                self.trouble = None;
                self.flash = None;
                self.turn += 1;
                self.turn_at = now;
                self.echo = user_text.map(|t| one_line(&t, 200)).filter(|t| !t.is_empty());
                self.pending_input = None;
                self.last_active = now;
            }
            SessionEvent::Stream { event } => match event {
                StreamEvent::Llm(l) => match l {
                    LlmEvent::Thinking { .. } => {
                        if !matches!(self.phase, Phase::Working(_)) {
                            self.phase = Phase::Thinking;
                        }
                        self.answered();
                    }
                    LlmEvent::ResponseStart => {
                        self.phase = Phase::Thinking;
                        self.response_at = self.raw.len();
                    }
                    LlmEvent::ResponseReset => {
                        self.raw.truncate(self.response_at);
                        self.phase = Phase::Thinking;
                    }
                    LlmEvent::Text { text } => {
                        if self.raw.len() == self.response_at && self.response_at > 0 && !text.trim().is_empty() {
                            self.raw.push_str("\n\n");
                            self.response_at = self.raw.len();
                        }
                        self.raw.push_str(&text);
                        self.phase = Phase::Talking;
                        self.answered();
                    }
                    LlmEvent::ToolUseStart { tool_name, .. } | LlmEvent::ToolUse { tool_name, .. } => {
                        self.phase = Phase::Working(tool_name);
                        self.answered();
                    }
                    LlmEvent::ToolResult { .. } => self.phase = Phase::Thinking,
                    _ => {}
                },
                // The provider's token count: it answered (Synaps sends one per response, unnamed).
                // A named one is a subagent's (`subagent/oneshot.rs`): only her own model counts.
                StreamEvent::Session(SessionStreamEvent::Usage { input_tokens, output_tokens, model }) if (input_tokens > 0 || output_tokens > 0) && self.hers(model.as_deref()) => self.answered(),
                // Her session's context rolling over: the Line doesn't show these, her log does.
                StreamEvent::Session(SessionStreamEvent::Notice { text, .. }) if rollover_notice(&text) => {
                    elog!("synaps-avatar: rollover: {}", one_line(&text, 200));
                }
                StreamEvent::Session(SessionStreamEvent::Error { message }) => match Trouble::of(&message, self.model()).map(|t| t.unpicked(self.unpicked())) {
                    Some(t) => {
                        // Its row in the thread (just made from this event) keeps these words.
                        if t == Trouble::NoPick
                            && let Some(it) = self.transcript.items().iter().rev().find(|i| matches!(i.kind, synaps_client::transcript::ItemKind::Error))
                        {
                            self.unpicked_errors.push(it.id);
                        }
                        // Her log and bubble show its URLs as she does (`urls_shown`): Synaps
                        // quotes its local URL whole, a `user:password@` too.
                        elog!("synaps-avatar: turn error ({}): {}", t.key(), one_line(&crate::provider::urls_shown(&message), 300));
                        if matches!(t, Trouble::LocalDown { .. }) {
                            self.local_tried = local_tried(&message);
                        }
                        if let Some(on) = t.no_brain() {
                            // The daemon's word: missing until a check or an answer says otherwise,
                            // or the files read otherwise than when it spoke (#76).
                            self.creds_by = Said::Daemon(self.creds_read);
                            self.creds = Creds::Missing(on);
                            self.creds_at = Some(now + CREDS_EVERY);
                        }
                        self.notice = Some(t.says());
                        self.flash = Some((t.mood(), now + TROUBLE_FOR));
                        self.trouble = Some(t);
                    }
                    None => {
                        self.notice = Some(format!("‹error: {}›", one_line(&crate::provider::urls_shown(&message), 90)));
                        self.flash = Some((Mood::Annoyed, now + ANNOYED_FOR));
                    }
                },
                _ => {}
            },
            SessionEvent::Idle => {
                self.busy = false;
                self.echo = None;
                self.last_active = now;
                self.touched = now;
                if !self.flash.is_some_and(|f| f.0 == Mood::Annoyed) && self.trouble.is_none() {
                    self.flash = Some((Mood::Done, now + DONE_FOR));
                }
            }
            SessionEvent::Aborted { .. } => {
                self.busy = false;
                self.touched = now;
                self.echo = None;
                self.notice = Some("‹aborted›".into());
                self.flash = Some((Mood::Annoyed, now + ANNOYED_FOR));
                self.last_active = now;
            }
            SessionEvent::Prompt { request } => {
                self.prompts.retain(|p| p.id != request.id);
                self.prompts.push(request);
                self.last_active = now;
            }
            SessionEvent::PromptResolved { prompt_id } => self.prompts.retain(|p| p.id != prompt_id),
            SessionEvent::Steered { .. } | SessionEvent::Dequeued { .. } => self.pending_input = None,
            SessionEvent::CompactionStarted => self.compacting = true,
            SessionEvent::CompactionApplied { .. } | SessionEvent::CompactionCancelled => self.compacting = false,
            SessionEvent::CompactionFailed { message } => {
                self.compacting = false;
                elog!("synaps-avatar: compaction failed: {}", crate::provider::urls_shown(&message));
            }
            SessionEvent::Cleared { .. } => {
                self.raw.clear();
                self.response_at = 0;
                self.notice = None;
                self.turn += 1;
                self.trigger.clear();
            }
            _ => {}
        }
    }

    /// Pick (or make) the session to attach to, from a fresh session list.
    fn resolve(&mut self, list: &[SessionMeta], now: Instant) -> Vec<Act> {
        let find = |key: &str| list.iter().find(|m| m.is(key) || m.name.as_deref() == Some(key));
        if let Some(key) = self.follow.clone() {
            if guarded(&key) {
                return self.refuse(&key);
            }
            if let Some(m) = find(&key) {
                if m.name.as_deref().is_some_and(guarded) {
                    return self.refuse(&format!("{key} ({})", m.name.as_deref().unwrap_or_default()));
                }
                return vec![Act::Attach(m.id.clone(), AttachMode::Observe)];
            }
        }
        match self.opts.target.clone() {
            Target::Session(s) => {
                elog!("synaps-avatar: no live session `{s}`; looking again in {} s", RETRY_SLOW.as_secs());
                self.retry_at = Some(now + RETRY_SLOW);
                Vec::new()
            }
            Target::Default => {
                let name = self.opts.name.clone();
                let mine = list.iter().find(|m| m.name.as_deref() == Some(name.as_str()) || self.opts.remembered.as_deref().is_some_and(|r| m.is(r)));
                if let Some(m) = mine {
                    if m.name.as_deref().is_some_and(guarded) {
                        return self.refuse(&m.id);
                    }
                    return vec![Act::Attach(m.id.clone(), AttachMode::Observe)];
                }
                // Not live (a saved session keeps its name reserved): resume it by the id
                // from last time, else create it by name, else resume the name, else unnamed.
                let first = if self.opts.remembered.is_some() { Creating::ResumeId } else { Creating::Named };
                let c = self.creating.unwrap_or(first);
                self.creating = Some(c);
                match (c, self.opts.remembered.clone()) {
                    (Creating::ResumeId, Some(id)) => vec![Act::Resume(id, self.create_options())],
                    (Creating::ResumeId | Creating::Named, _) => {
                        self.creating = Some(Creating::Named);
                        vec![Act::Create(self.create_options().name(name))]
                    }
                    (Creating::ResumeName, _) => vec![Act::Resume(name, self.create_options())],
                    (Creating::Unnamed, _) => vec![Act::Create(self.create_options())],
                }
            }
            Target::New => {
                self.creating = Some(Creating::Unnamed);
                vec![Act::Create(self.create_options())]
            }
        }
    }

    /// Only ever for her own session (Default or New: `resolve` never creates for --session).
    fn create_options(&self) -> CreateOptions {
        let mut o = CreateOptions::new().mode(AttachMode::Observe);
        if let Some(c) = &self.opts.cwd {
            o = o.cwd(Cwd::Path(c.clone())).fallback_cwd(Cwd::Home);
        }
        if let Some(m) = self.opts.model.as_ref().or(self.opts.persona.model.as_ref()) {
            o = o.model(m.clone());
        }
        if !matches!(self.opts.target, Target::Session(_))
            && let Some(s) = &self.opts.persona.system
        {
            o = o.system(s.clone());
        }
        o
    }

    /// Timers: retry a lost session; back to Observe once nothing needs Mirror.
    pub fn tick(&mut self, now: Instant) -> Vec<Act> {
        // Replies not in after PERSONA_WAIT are lost (an older daemon, a frame-level error, or the
        // downgrade leaving the attach they'd come on): the view is the daemon's alone again, or
        // MODEL would show the asked model for good and stale asks would come back. The cost: a
        // round trip longer than this (a Muse model fetching its catalog first, a parked session
        // waking) brings back A, B, A's stranding for that pick.
        if self.persona_wait > 0 && now >= self.persona_sent + PERSONA_WAIT {
            elog!("synaps-avatar: persona: {} repl{} not in after {}s; the daemon's view stands", self.persona_wait, if self.persona_wait == 1 { "y" } else { "ies" }, PERSONA_WAIT.as_secs());
            self.persona_wait = 0;
            self.persona_asked.clear();
        }
        if self.rollover_sent.is_some_and(|t| now >= t + PERSONA_WAIT) {
            elog!("synaps-avatar: rollover: no reply to /context auto in {}s; not known to be on", PERSONA_WAIT.as_secs());
            self.rollover_sent = None;
        }
        if self.retry_at.is_some_and(|t| t <= now) && self.attach == Attach::None && self.connected {
            // The answer (`Sessions`) resolves.
            self.retry_at = None;
            return vec![Act::List];
        }
        if let Some(t) = self.downgrade_at()
            && t <= now
            && let Some(id) = self.follow.clone()
        {
            self.mode = AttachMode::Observe;
            self.switching = true;
            return vec![Act::Attach(id, AttachMode::Observe)];
        }
        Vec::new()
    }

    /// The session's settings as the daemon says them now: another model reads its login afresh.
    fn view_now(&mut self, view: serde_json::Value, now: Instant) {
        if view.get("model") != self.view.as_ref().and_then(|v| v.get("model")) {
            self.creds = Creds::Unknown;
            self.creds_by = Said::Read;
            self.creds_read = None;
            self.creds_at = Some(now);
        }
        self.view = Some(view);
    }

    /// A `Set`'s reply: the last one in, the view is the daemon's alone again.
    fn persona_replied(&mut self) {
        self.persona_wait = self.persona_wait.saturating_sub(1);
        if self.persona_wait == 0 {
            self.persona_asked.clear();
        }
    }

    /// When the Mirror attach is no longer needed (turn done, nothing pending, field shut).
    fn downgrade_at(&self) -> Option<Instant> {
        let idle = self.mode == AttachMode::Mirror
            && !self.switching
            && matches!(self.attach, Attach::Live(_))
            && self.connected
            && !self.typing
            && !self.busy
            && self.prompts.is_empty()
            && self.queued.is_empty()
            && self.pending_input.is_none();
        // A reply on its way holds it (its attach must stay up for it), PERSONA_WAIT at most.
        let waits = [(self.persona_wait > 0).then_some(self.persona_sent), self.rollover_sent].into_iter().flatten().max();
        idle.then_some(match waits {
            Some(t) => t + PERSONA_WAIT,
            None => self.touched + MIRROR_LINGER,
        })
    }

    /// The next instant the body or the link changes by itself.
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        let mut t: Vec<Instant> = Vec::new();
        if self.attach == Attach::None
            && self.connected
            && let Some(r) = self.retry_at
        {
            t.push(r.max(now));
        }
        if !self.ever_live {
            t.push(self.started + BOOT_GRACE);
        }
        if let Some(d) = self.downgrade_at() {
            t.push(d.max(now + Duration::from_millis(1)));
        }
        if self.persona_wait > 0 {
            t.push((self.persona_sent + PERSONA_WAIT).max(now + Duration::from_millis(1)));
        }
        if let Some(s) = self.rollover_sent {
            t.push((s + PERSONA_WAIT).max(now + Duration::from_millis(1)));
        }
        if let Some((_, until)) = self.flash {
            t.push(until);
        }
        if let Some(c) = self.creds_next() {
            t.push(c.max(now));
        }
        if self.quiet_start(now) {
            t.push(self.turn_at + QUIET_START);
        }
        if self.live() && !self.busy && self.resting.is_none() {
            t.push(self.last_active + self.opts.sleepy_after);
        }
        t.into_iter().filter(|&d| d > now).min()
    }

    /// The turn's reply as the bubble shows it: the text cleaned, then the notice.
    fn reply_text(&self) -> String {
        let mut reply = clean(&self.raw);
        if let Some(n) = &self.notice {
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(n);
        }
        reply
    }

    /// A turn that has just started with no word yet: shown as not started (`QUIET_START`).
    fn quiet_start(&self, now: Instant) -> bool {
        self.busy && self.raw.is_empty() && self.phase == Phase::Thinking && self.prompts.is_empty() && now < self.turn_at + QUIET_START
    }

    pub fn body(&self, now: Instant) -> Body {
        let mut label = None;
        let quiet = self.quiet_start(now);
        let mood = if !self.live() {
            if !self.ever_live && now < self.started + BOOT_GRACE { Mood::Idle } else { Mood::Away }
        } else if !self.prompts.is_empty() {
            Mood::NeedsYou
        } else if self.compacting {
            label = Some("COMPACTING".into());
            Mood::Sleepy
        } else if self.busy && !quiet {
            match &self.phase {
                Phase::Thinking => Mood::Thinking,
                Phase::Talking => Mood::Talking,
                Phase::Working(tool) => {
                    label = Some(format!("WORKING · {}", tool_label(tool)));
                    Mood::Working
                }
            }
        } else if let Some((m, until)) = self.flash
            && now < until
        {
            label = self.trouble.as_ref().map(|t| t.tag().to_string());
            m
        } else if self.resting.unwrap_or(now >= self.last_active + self.opts.sleepy_after) {
            Mood::Sleepy
        } else {
            Mood::Idle
        };
        // At rest with no brain: her tag says so (not ONLINE), before his first message too.
        if let Creds::Missing(on) = self.creds
            && label.is_none()
            && matches!(mood, Mood::Idle | Mood::Sleepy)
        {
            label = Some(Trouble::NoBrain { on }.tag().to_string());
        }
        // Whole: a security decision is made on the full text (the bubble cuts its own copy).
        let prompts: Vec<PromptView> = self
            .prompts
            .iter()
            .map(|p| PromptView { id: p.id, title: p.title.trim().to_string(), text: p.prompt.trim_end().to_string(), secret: p.kind.as_str() != Some("confirm") })
            .collect();
        let prompt = prompts.first().cloned();
        let (reply, raw, trigger) = if quiet {
            self.before.clone()
        } else {
            (self.reply_text(), self.raw.trim().to_string(), self.trigger.clone())
        };
        Body {
            mood,
            label,
            reply,
            raw,
            turn: if quiet { self.turn.saturating_sub(1) } else { self.turn },
            busy: self.busy && !quiet,
            echo: self.echo.clone().filter(|_| self.busy && !quiet),
            trigger,
            prompt,
            prompts,
            live: self.live(),
            trouble: self.trouble.clone(),
        }
    }
}

/// Who stands behind what she says of her model's credentials (`Link::creds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    /// A read of this machine's files (`creds::check`), or nothing yet.
    Read,
    /// The daemon's own no-login error, and the settled read it spoke over (none yet: the next).
    Daemon(Option<(Creds, crate::creds::Again)>),
    /// An answer on her model: a working brain, whatever the files say.
    Answer,
}

/// Whether her model has credentials, as far as she can tell without asking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Creds {
    /// Not read, or not hers to decide (another provider, a remote broker, an unreadable file).
    Unknown,
    Have,
    /// None for the provider (its name, as `Trouble::NoBrain` says it).
    Missing(Option<&'static str>),
}

/// A failed turn she can explain in her own words. The daemon's text is written for a terminal
/// (it names slash commands the Line doesn't have, and the bubble cut it mid-word); these are
/// the common ones, matched on the daemon's own wording (Synaps classifies its auth errors on
/// `synaps login` too). Anything else keeps the raw `‹error: …›`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// No model credentials: a fresh install before `synaps login`. `on`: the provider her
    /// model is on, when the daemon names it (`No credentials for anthropic`: Claude).
    NoBrain { on: Option<&'static str> },
    /// No brain, and none picked: her model is Synaps' default, chosen by no one (`unpicked`).
    /// Not a Claude login to ask for: a brain to pick, in Settings ▸ Brain.
    NoPick,
    /// A login that lapsed or was rejected.
    LoggedOut,
    /// The provider's rate or usage limit.
    Limited,
    /// The provider is overloaded or erroring (5xx).
    Swamped,
    /// The model can't be reached (network, timeout, a dropped stream).
    Offline,
    /// Her local model's server isn't answering (`local/…`; `ollama`: on Ollama's port).
    LocalDown { ollama: bool },
    /// Her local model isn't on its server (a 404 for `local/<model>`: Ollama hasn't pulled it).
    ModelMissing { model: String },
    /// Her local model's server refused the request (a 400): a model without tool calling
    /// (Ollama), or a context too small for her prompt (llama.cpp, LM Studio, vLLM). Ollama itself
    /// cuts an overlong prompt without a word: no error to name there.
    WrongModel,
}

/// The local server a `can't reach local endpoint at <url> — …` error names (Synaps' base URL
/// and `/chat/completions`), as she shows one (`provider::url_shown`), the path it adds off.
fn local_tried(message: &str) -> Option<String> {
    const AT: &str = "local endpoint at ";
    let at = message.to_ascii_lowercase().find(AT)? + AT.len();
    let url = crate::provider::url_shown(message[at..].split_whitespace().next()?);
    let url = url.trim_end_matches('/');
    Some(url.strip_suffix("/chat/completions").unwrap_or(url).trim_end_matches('/').to_string()).filter(|u| !u.is_empty())
}

/// The command a no-brain line ends on, on a line of its own (the bubble draws it as a chip).
pub const LOGIN_CMD: &str = "synaps login";

/// A model name she can put in a command: Ollama's characters only, and short (her line stays
/// a bubble's).
fn model_word(m: &str) -> Option<&str> {
    (!m.is_empty() && m.len() <= 46 && m.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/'))).then_some(m)
}

impl Trouble {
    /// `model`: her session's (`local/…` tells a local server's 404 and 400 apart from others).
    pub fn of(message: &str, model: Option<&str>) -> Option<Trouble> {
        let m = message.to_ascii_lowercase();
        let any = |keys: &[&str]| keys.iter().any(|k| m.contains(k));
        if m.contains("synaps login") || m.contains("(authentication_error)") {
            if any(&["no anthropic credentials", "no api key or oauth token"]) {
                return Some(Trouble::NoBrain { on: Some("Claude") });
            }
            // Synaps 0.12: `… credential error: No credentials for anthropic at …/auth.json. Run
            // synaps login --provider anthropic …` (wrapped in a "Token refresh failed").
            if let Some(rest) = m.split("no credentials for ").nth(1) {
                return Some(Trouble::NoBrain { on: provider_name(rest.split_whitespace().next().unwrap_or("")) });
            }
            if let Some(rest) = m.split("no credential configured for '").nth(1) {
                return Some(Trouble::NoBrain { on: provider_name(rest.split('\'').next().unwrap_or("")) });
            }
            if m.contains("no oauth credential at ") {
                return Some(Trouble::NoBrain { on: None });
            }
            return Some(Trouble::LoggedOut);
        }
        // `can't reach local endpoint at http://localhost:11434/v1/… — is Ollama/LM Studio running?`
        if m.contains("reach local endpoint") {
            return Some(Trouble::LocalDown { ollama: m.contains(":11434/") });
        }
        if let Some(local) = model.and_then(|m| m.trim().strip_prefix("local/")) {
            // `provider request failed: 404 Not Found [not_found_error]` (Ollama: not pulled).
            if m.contains("request failed: 404") {
                return Some(Trouble::ModelMissing { model: local.to_string() });
            }
            if m.contains("request failed: 400") {
                return Some(Trouble::WrongModel);
            }
        }
        if any(&["rate limit", "(http 429", "usage limit", "quota", "(billing_error)"]) {
            Some(Trouble::Limited)
        } else if any(&["overloaded", "server error (http 5"]) {
            Some(Trouble::Swamped)
        } else if any(&["could not reach", "timed out", "connection failed", "connection lost"]) {
            Some(Trouble::Offline)
        } else {
            None
        }
    }

    /// NO BRAIN YET on a model no one picked: a brain to pick, not its login to name.
    pub fn unpicked(self, unpicked: bool) -> Trouble {
        match self {
            Trouble::NoBrain { .. } if unpicked => Trouble::NoPick,
            t => t,
        }
    }

    /// No brain (its provider, as named): her tag's NO BRAIN YET.
    pub fn no_brain(&self) -> Option<Option<&'static str>> {
        match self {
            Trouble::NoBrain { on } => Some(*on),
            Trouble::NoPick => Some(None),
            _ => None,
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Trouble::NoBrain { .. } => "no_brain",
            Trouble::NoPick => "no_pick",
            Trouble::LoggedOut => "logged_out",
            Trouble::Limited => "limited",
            Trouble::Swamped => "swamped",
            Trouble::Offline => "offline",
            Trouble::LocalDown { .. } => "local_down",
            Trouble::ModelMissing { .. } => "model_missing",
            Trouble::WrongModel => "wrong_model",
        }
    }

    /// The command that signs in for this provider (`--provider` skips Synaps' picker).
    fn login_cmd(on: Option<&str>) -> String {
        match on.and_then(crate::provider::by_name) {
            Some(p) => format!("{LOGIN_CMD} --provider {}", p.login),
            None => LOGIN_CMD.to_string(),
        }
    }

    /// What she says (the bubble, and her row in the Line). A fix that is a command ends on it,
    /// on its own line, in backticks (a chip; a click copies it).
    pub fn says(&self) -> String {
        match self {
            // `synaps login` signs in; it doesn't pick a model. Her model is the daemon's default
            // (Claude) or her `[session] model` (applied live on a settings reload): the other way
            // out. Not `/model`: the daemon takes no slash commands, and the Line would send it to
            // the model as words.
            Trouble::NoBrain { on: Some(p) } => format!("No brain yet, choom. Synaps has no {p} login. Log in, or set another model ([session] model in my settings):\n`{}`", Self::login_cmd(Some(p))),
            Trouble::NoBrain { on: None } => format!("No brain yet, choom. Synaps has no login for my model. Log in, or set another model ([session] model in my settings):\n`{LOGIN_CMD}`"),
            // His pick: an account, a key or a local model, none first (`SET ME UP ▸`: the Brain screen).
            Trouble::NoPick => format!("No brain yet, choom. Pick one: an account, an API key, or a model on this machine.\n`{}`", crate::line::SET_ME_UP),
            // Signed in again from her (Settings ▸ Brain: her account's row), no terminal.
            Trouble::LoggedOut => format!("My login ran out. Sign me in again:\n`{}`", crate::line::SIGN_IN_AGAIN),
            Trouble::Limited => "Hit my model's limit for now. Give it a bit, then ask me again.".into(),
            Trouble::Swamped => "My model's swamped right now. Try me again in a minute.".into(),
            Trouble::Offline => "Can't reach my model. Check the connection, then ask me again.".into(),
            Trouble::LocalDown { ollama: true } => "Ollama's not answering, so I've got no brain. Start Ollama, then ask me again.".into(),
            Trouble::LocalDown { ollama: false } => "My local model's server isn't answering. Start it, then ask me again.".into(),
            Trouble::ModelMissing { model } => match model_word(model) {
                // One made from a Modelfile, as the docs make `capri-qwen3` (a 16k context): `capri-`
                // and no `:tag`. Anything else is a name Ollama's library has (`mistral`,
                // `llama3.2`, `qwen3:8b`): pulled.
                Some(w) if w.starts_with("capri-") && !w.contains([':', '/']) => format!("{w} isn't on my local server. Create it from its Modelfile, then ask me again:\n`ollama create {w} -f Modelfile`"),
                Some(w) => format!("{w} isn't on my local server. Pull it, then ask me again:\n`ollama pull {w}`"),
                None => "My model isn't on my local server. Pull it there, then ask me again.".into(),
            },
            Trouble::WrongModel => "My local model turned that down. I need one with tool calling and a 16k+ context: set another ([session] model in my settings).".into(),
        }
    }

    /// Her tag while it shows.
    pub fn tag(&self) -> &'static str {
        match self {
            Trouble::NoBrain { .. } | Trouble::NoPick => "NO BRAIN YET",
            Trouble::LoggedOut => "LOGGED OUT",
            Trouble::Limited => "AT THE LIMIT",
            Trouble::Swamped => "MODEL BUSY",
            Trouble::Offline => "NO CONNECTION",
            Trouble::LocalDown { ollama: true } => "OLLAMA IS OFF",
            Trouble::LocalDown { ollama: false } => "LOCAL MODEL OFF",
            Trouble::ModelMissing { .. } => "MODEL MISSING",
            Trouble::WrongModel => "WRONG MODEL",
        }
    }

    /// The Line's quiet footer tip: that his message isn't lost (her row says the fix).
    pub fn tip(&self) -> &'static str {
        match self {
            Trouble::NoBrain { .. } => "after synaps login, ↑ resends your message",
            Trouble::LoggedOut => "signed in again, ↑ resends your message",
            Trouble::NoPick => "once I have a brain, ↑ resends your message",
            Trouble::LocalDown { .. } => "once it's up, ↑ resends your message",
            Trouble::ModelMissing { .. } => "once it's pulled, ↑ resends your message",
            _ => "↑ resends your message",
        }
    }

    /// Not her fault and not his: no login, or a local model to set up, is calm; a provider's
    /// trouble annoys her.
    fn mood(&self) -> Mood {
        match self {
            Trouble::NoBrain { .. } | Trouble::NoPick | Trouble::LoggedOut | Trouble::LocalDown { .. } | Trouble::ModelMissing { .. } | Trouble::WrongModel => Mood::Idle,
            _ => Mood::Annoyed,
        }
    }
}

/// A provider key as Synaps names it (`no credential configured for 'kimi-code'`), as she'd say it
/// (`provider::ALL`: its accounts and API-key services).
fn provider_name(key: &str) -> Option<&'static str> {
    let key = key.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
    crate::provider::by_id(key).filter(|p| p.kind != crate::provider::Kind::Local).map(|p| p.name)
}

/// A tool name for the tag: short and safe (never arguments).
pub(crate) fn tool_label(name: &str) -> String {
    let n = name.rsplit("__").next().unwrap_or(name);
    let n: String = n.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')).collect();
    if n.chars().count() > TOOL_MAX { format!("{}…", n.chars().take(TOOL_MAX - 1).collect::<String>()) } else { n }
}

/// Who woke a turn, from its user text: `[comms] #12 ask from spike: …` is another session
/// (Some(("SPIKE", "ask", text))); anything else is him (None).
pub fn origin(user_text: &str) -> Option<(String, String, String)> {
    let rest = user_text.trim_start().strip_prefix("[comms]")?.trim_start();
    let rest = rest.strip_prefix('#')?;
    let (_, rest) = rest.split_once(' ')?;
    let (kind, rest) = rest.trim_start().split_once(' ')?;
    let rest = rest.trim_start().strip_prefix("from ")?;
    let (who, text) = rest.split_once(':')?;
    let who = who.trim();
    (!who.is_empty() && !who.contains(char::is_whitespace)).then(|| (who.to_uppercase(), kind.to_string(), text.trim().to_string()))
}

/// Whitespace collapsed to single spaces, at most `max` chars (with an ellipsis).
pub fn one_line(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>().trim_end()) } else { s }
}

/// Markdown-lite for the bubble: fenced code becomes `‹code›`, `**`/`__` go, headings
/// lose their `#`s, bullets become `•`, links keep their text, inline code stays as is,
/// blank lines collapse (the bubble is small). Stable under streaming: cleaning a prefix
/// of the text gives (nearly always) a prefix of the clean text.
pub fn clean(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    for line in raw.split('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            if !in_code {
                out.push("‹code›".into());
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let l = match t.trim_start_matches('#') {
            rest if rest.len() < t.len() && (rest.is_empty() || rest.starts_with(' ')) => rest.trim_start(),
            _ => t,
        };
        let bullet = ["- ", "* ", "+ "].iter().find_map(|b| l.strip_prefix(b));
        let mut s = match bullet {
            Some(rest) => format!("• {rest}"),
            None => l.to_string(),
        };
        s = s.replace("**", "").replace("__", "");
        s = crate::voice::unmark(&links(&s));
        let s = s.trim_end().to_string();
        if s.is_empty() {
            continue;
        }
        out.push(s);
    }
    out.join("\n")
}

/// `[text](url)` → `text`.
fn links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let after = &rest[i + 1..];
        if let Some(j) = after.find("](")
            && let Some(k) = after[j + 2..].find(')')
            && !after[..j].contains('[')
        {
            out.push_str(&rest[..i]);
            out.push_str(&after[..j]);
            rest = &after[j + 2 + k + 1..];
        } else {
            out.push_str(&rest[..=i]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creds::Again;
    use std::sync::Arc;
    use synaps_client::protocol::{DaemonFrame, decode_frame};

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn opts() -> Options {
        Options {
            target: Target::Default,
            name: "capri".into(),
            remembered: None,
            cwd: None,
            model: None,
            sleepy_after: Duration::from_secs(600),
            persona: Persona::default(),
            local: true,
        }
    }

    fn attached(seq: u64, name: &str) -> Event {
        attached_as(seq, name, AttachMode::Observe)
    }

    fn attached_as(seq: u64, name: &str, mode: AttachMode) -> Event {
        let DaemonFrame::Attached(a) = decode_frame(fixture(name).trim()).unwrap() else { panic!() };
        Event::Attached { attach_seq: seq, attached: a, mode, reconnect: false }
    }

    fn meta(id: &str, name: Option<&str>) -> SessionMeta {
        serde_json::from_value(serde_json::json!({"id": id, "name": name, "model": "m", "lifecycle": "live"})).unwrap()
    }

    fn turn(seq: u64) -> Vec<Event> {
        fixture("turn_events.ndjson")
            .lines()
            .map(|l| {
                let DaemonFrame::Event(env) = decode_frame(l).unwrap() else { panic!() };
                let event = env.decode().unwrap();
                Event::Session { attach_seq: seq, session_id: "s".into(), seq: env.seq, event, raw: Arc::new(env.event.clone()) }
            })
            .collect()
    }

    fn connected() -> Event {
        Event::Status(Status::Connected { daemon_version: "0.11.1".into(), profile: None, pid: 1, features: vec![] })
    }

    fn live_link(t0: Instant) -> Link {
        let mut l = Link::new(opts(), t0);
        l.on_event(connected(), t0);
        l.attaching(1);
        l.on_event(attached(1, "attached_create.json"), t0);
        assert!(l.live());
        l
    }

    /// `on_event` without the `Remember` (her id saved for next time).
    fn on(l: &mut Link, ev: Event, t: Instant) -> Vec<Act> {
        l.on_event(ev, t).into_iter().filter(|a| !matches!(a, Act::Remember(_))).collect()
    }

    /// The persona the persona tests set (no rollover: those tests are about the Sets).
    fn persona() -> Persona {
        Persona { system: Some("You are Capri.".into()), model: Some("anthropic/claude-sonnet-4-6".into()), thinking: Some("medium".into()), rollover: false }
    }

    /// Her session (named `capri`) attached in `mode`; `owner`: who owns input (she is client 1).
    fn mine(seq: u64, mode: AttachMode, owner: Option<u64>, name: Option<&str>, view: Option<&Persona>) -> Event {
        let Event::Attached { mut attached, .. } = attached(seq, "attached_create.json") else { panic!() };
        attached.meta.name = name.map(str::to_string);
        attached.input_owner = owner;
        if let Some(p) = view {
            attached.view["model"] = serde_json::json!(p.model);
            attached.view["reasoning_level"] = serde_json::json!(p.thinking);
            attached.view["system_prompt"] = serde_json::json!(p.system);
        }
        Event::Attached { attach_seq: seq, attached, mode, reconnect: false }
    }

    fn applied(seq: u64, setting: &str, p: &Persona) -> Event {
        let view = serde_json::json!({"model": p.model, "reasoning_level": p.thinking, "system_prompt": p.system});
        Event::SettingApplied { attach_seq: seq, session_id: "s".into(), id: 0, setting: setting.into(), message: None, view }
    }

    const SID: &str = "20261113-073427-5fde";

    /// Created by her: the create carries the persona's prompt and model; after it attaches
    /// (Observe) what still differs (the thinking level) is set from a Mirror attach, then
    /// she drops back to Observe. Nothing more on the next attach (idempotent).
    #[test]
    fn her_new_session_gets_the_persona() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![]), t0);
        let [Act::Create(o)] = acts.as_slice() else { panic!("{acts:?}") };
        assert_eq!((o.system.as_deref(), o.model.as_deref(), o.name.as_deref()), (Some("You are Capri."), Some("anthropic/claude-sonnet-4-6"), Some("capri")));
        l.attaching(1);
        let created = Persona { thinking: Some("low".into()), ..persona() };
        let acts = on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&created)), t0);
        let [Act::Attach(id, AttachMode::Mirror)] = acts.as_slice() else { panic!("{acts:?}") };
        assert_eq!(id, SID);
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&created)), t0);
        match acts.as_slice() {
            [Act::Set(s)] => assert_eq!(s, &Setting::reasoning_level("medium")),
            other => panic!("{other:?}"),
        }
        // Waits for the reply, then lingers, then Observe again.
        assert!(l.tick(t0 + MIRROR_LINGER).is_empty(), "still waiting for the reply");
        l.on_event(applied(2, "reasoning_level", &persona()), t0);
        let acts = l.tick(t0 + MIRROR_LINGER);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Observe)]), "{acts:?}");
        l.attaching(3);
        assert!(on(&mut l, mine(3, AttachMode::Observe, None, Some("capri"), Some(&persona())), t0).is_empty());
        assert_eq!(l.mode(), AttachMode::Observe);
    }

    /// Her existing session, all three different: model, then thinking, then the prompt;
    /// a session that already matches gets nothing (no Mirror switch at all).
    #[test]
    fn her_existing_session_gets_only_what_differs() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Observe)]), "{acts:?}");
        l.attaching(1);
        let acts = on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), None), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), None), t0);
        let names: Vec<&str> = acts.iter().map(|a| if let Act::Set(s) = a { s.name() } else { "?" }).collect();
        assert_eq!(names, ["model", "reasoning_level", "system_prompt"]);

        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&persona())), t0).is_empty());
    }

    /// MODEL on the Settings page shows the model she asked for until her session has it: while
    /// it's on its way (the Mirror switch, the Set and its reply). If another client owns her
    /// session's input it isn't set: the daemon's model stands (shown), and set_persona says so
    /// by asking nothing.
    #[test]
    fn the_model_on_its_way_shows_until_it_is_in() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        let old = Persona { model: Some("claude-haiku-4-5-20251001".into()), ..persona() };
        on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&old)), t0);
        assert_eq!(l.model(), Some("claude-haiku-4-5-20251001"));
        assert_eq!(l.model_on_way(), Some("anthropic/claude-sonnet-4-6"), "the Mirror switch on its way");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&old)), t0);
        assert!(matches!(acts.as_slice(), [Act::Set(_)]), "{acts:?}");
        assert_eq!(l.model_on_way(), Some("anthropic/claude-sonnet-4-6"), "the Set sent, no reply yet");
        l.on_event(applied(2, "model", &persona()), t0);
        assert_eq!((l.model(), l.model_on_way()), (Some("anthropic/claude-sonnet-4-6"), None), "in");
        // Another client owns the input: nothing asked, nothing on its way.
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, Some(7), Some("capri"), Some(&old)), t0).is_empty());
        assert_eq!(l.model_on_way(), None);
        let acts = l.set_persona(Persona { model: Some("claude-opus-5-5".into()), ..persona() }, t0);
        assert!(acts.is_empty() && l.model_on_way().is_none(), "{acts:?}");
        assert!(l.persona_blocked(), "it says another client has her input");
        // The model it has already: nothing to set, and nothing blocked it.
        assert!(l.pick_persona(old.clone(), t0).is_empty());
        assert!(!l.persona_blocked());
    }

    /// Two MODEL picks within one round trip (A, B, A): B's Set is on its way, so A again is a
    /// change and goes out after it; she ends on A. The same pick twice is one Set. A session his
    /// TUI moved off her model gets it again on a pick (a reload with the persona as it was
    /// leaves it).
    #[test]
    fn a_model_asked_back_before_its_reply_still_goes_out() {
        let t0 = Instant::now();
        let a = persona();
        let b = Persona { model: Some("claude-opus-5-5".into()), ..persona() };
        let set = |acts: &[Act], m: &str| matches!(acts, [Act::Set(s)] if *s == Setting::model(m));
        let mut l = Link::new(Options { persona: a.clone(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&a)), t0).is_empty());
        let acts = l.pick_persona(b.clone(), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&a)), t0);
        assert!(set(acts.as_slice(), "claude-opus-5-5"), "{acts:?}");
        assert!(l.pick_persona(b.clone(), t0).is_empty(), "B again: on its way already");
        let acts = l.pick_persona(a.clone(), t0);
        assert!(set(acts.as_slice(), "anthropic/claude-sonnet-4-6"), "A again, before B's reply: {acts:?}");
        l.on_event(applied(2, "model", &b), t0);
        assert_eq!((l.model(), l.model_on_way()), (Some("claude-opus-5-5"), Some("anthropic/claude-sonnet-4-6")));
        l.on_event(applied(2, "model", &a), t0);
        assert_eq!((l.model(), l.model_on_way()), (Some("anthropic/claude-sonnet-4-6"), None), "she ends on A");
        // His TUI moves her session to Haiku.
        let haiku = Persona { model: Some("claude-haiku-4-5-20251001".into()), ..persona() };
        l.on_event(applied(2, "model", &haiku), t0);
        assert!(l.set_persona(a.clone(), t0).is_empty(), "a reload, the persona unchanged: his choice stands");
        let acts = l.pick_persona(a.clone(), t0);
        assert!(set(acts.as_slice(), "anthropic/claude-sonnet-4-6"), "MODEL picks it: {acts:?}");
        assert!(!l.persona_blocked());
    }

    /// His TUI's `/model` on her session: the daemon broadcasts `setting_changed` with the
    /// engine command's reserved id (not a reply to any Set of hers, so the client sends it on
    /// only as a raw session event). Her view takes it: MODEL shows it, her login is read again
    /// for it, and a pick of her model sends it back. A refused change, or her own Set's echo
    /// (its reply already in), changes nothing.
    #[test]
    fn his_tui_s_model_change_is_seen() {
        let t0 = Instant::now();
        let a = persona();
        let wire = |e: serde_json::Value| {
            let line = serde_json::json!({"type": "event", "session_id": SID, "seq": 77, "ts": "t", "event": e}).to_string();
            let DaemonFrame::Event(env) = decode_frame(&line).unwrap() else { panic!() };
            let event = env.decode().unwrap();
            Event::Session { attach_seq: 1, session_id: SID.into(), seq: env.seq, event, raw: Arc::new(env.event.clone()) }
        };
        let changed = |ok: bool, model: &str| {
            let view = serde_json::json!({"model": model, "reasoning_level": a.thinking, "system_prompt": a.system});
            wire(serde_json::json!({"ev": "setting_changed", "applied": {"id": (1u64 << 63) + 3, "setting": "model", "ok": ok, "message": null, "view": view, "clamp": null}}))
        };
        let mut l = Link::new(Options { persona: a.clone(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&a)), t0).is_empty());
        l.set_creds(Creds::Have, Again::Never, t0);
        assert_eq!(l.creds(), Creds::Have);
        let haiku = "claude-haiku-4-5-20251001";
        assert!(on(&mut l, changed(false, haiku), t0).is_empty());
        assert_eq!(l.model(), Some("anthropic/claude-sonnet-4-6"), "refused: nothing changed");
        on(&mut l, changed(true, haiku), t0);
        assert_eq!((l.model(), l.model_on_way()), (Some(haiku), None));
        assert_eq!(l.creds(), Creds::Unknown, "another model: its login read afresh");
        assert!(l.creds_due(t0).is_some_and(|q| q.model == haiku));
        // MODEL picks hers again: compared against Haiku, so it goes out.
        let acts = l.pick_persona(a.clone(), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(2);
        let mut view = a.clone();
        view.model = Some(haiku.into());
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&view)), t0);
        assert!(matches!(acts.as_slice(), [Act::Set(s)] if *s == Setting::model("anthropic/claude-sonnet-4-6")), "{acts:?}");
        l.on_event(applied(2, "model", &a), t0);
        l.set_creds(Creds::Have, Again::Never, t0);
        // Her own Set's raw echo: the view she has already, her login kept.
        let mut echo = changed(true, "anthropic/claude-sonnet-4-6");
        if let Event::Session { attach_seq, .. } = &mut echo {
            *attach_seq = 2;
        }
        on(&mut l, echo, t0);
        assert_eq!((l.model(), l.creds()), (Some("anthropic/claude-sonnet-4-6"), Creds::Have));
    }

    /// A Set's reply that never comes (an older daemon, a daemon error, the downgrade leaving
    /// its attach): after PERSONA_WAIT the daemon's view stands again. MODEL stops showing the
    /// asked model, and the lost ask isn't compared against on the next pick.
    #[test]
    fn a_lost_reply_lets_go_after_persona_wait() {
        let t0 = Instant::now();
        let a = persona();
        let b = Persona { model: Some("claude-opus-5-5".into()), ..persona() };
        let mut l = Link::new(Options { persona: a.clone(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&a)), t0);
        l.pick_persona(b.clone(), t0);
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&a)), t0);
        assert!(matches!(acts.as_slice(), [Act::Set(_)]), "{acts:?}");
        assert_eq!(l.model_on_way(), Some("claude-opus-5-5"));
        assert!(l.deadline(t0).is_some_and(|d| d <= t0 + PERSONA_WAIT));
        assert!(l.tick(t0 + PERSONA_WAIT - Duration::from_millis(1)).is_empty());
        assert_eq!(l.model_on_way(), Some("claude-opus-5-5"), "not yet");
        // No reply: let go (and back to Observe, as before).
        let acts = l.tick(t0 + PERSONA_WAIT);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Observe)]), "{acts:?}");
        assert_eq!((l.model(), l.model_on_way()), (Some("anthropic/claude-sonnet-4-6"), None), "the daemon's view stands");
        // Picking A now is no change from the daemon's view: nothing to send, the lost B ignored.
        let t1 = t0 + PERSONA_WAIT + Duration::from_secs(1);
        l.attaching(3);
        on(&mut l, mine(3, AttachMode::Observe, None, Some("capri"), Some(&a)), t1);
        assert!(l.pick_persona(a.clone(), t1).is_empty());
        // And B again goes out (it isn't on its way any more).
        assert!(matches!(l.pick_persona(b.clone(), t1).as_slice(), [Act::Attach(_, AttachMode::Mirror)]));
    }

    /// The hard guard: never a session she rides with --session (even named like her), never
    /// a renamed one she finds by the remembered id, never `subconscious`.
    #[test]
    fn the_persona_never_touches_anyone_elses_session() {
        let t0 = Instant::now();
        // --session capri: someone else's session, even with her name and a persona.
        let mut l = Link::new(Options { target: Target::Session("capri".into()), persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Observe)]));
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), None), t0).is_empty());
        // Typing switches it to Mirror: still no Set.
        let acts = l.set_typing(true, t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]));
        l.attaching(2);
        assert!(on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), None), t0).iter().all(|a| !matches!(a, Act::Set(_))));
        assert!(l.set_persona(Persona { thinking: Some("high".into()), ..persona() }, t0).is_empty());
        // Her remembered id, renamed by someone to `work`: theirs now.
        let mut l = Link::new(Options { remembered: Some(SID.into()), persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![meta(SID, Some("work"))]), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Observe)]), "{acts:?}");
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("work"), None), t0).is_empty());
        // `subconscious`: refused before any persona.
        let mut l = Link::new(Options { remembered: Some(SID.into()), persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, None)]), t0);
        l.attaching(1);
        let acts = on(&mut l, mine(1, AttachMode::Observe, None, Some("subconscious"), None), t0);
        assert!(matches!(acts.as_slice(), [Act::Detach, Act::Fatal(_)]), "{acts:?}");
    }

    /// Someone else owns input: no Mirror switch, no Set. A refusal is logged, never shown.
    #[test]
    fn a_refused_persona_is_quiet() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, Some(7), Some("capri"), None), t0).is_empty(), "the TUI owns input");
        // Owner gone, a Set refused anyway: no restore, no flash, back to Observe in time.
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), None), t0);
        l.attaching(2);
        assert_eq!(on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), None), t0).len(), 3);
        for s in ["model", "reasoning_level", "system_prompt"] {
            let e = Error::SettingRefused { id: 1, setting: s.into(), reason: "input is owned by client #7".into() };
            l.on_event(Event::Error { attach_seq: Some(2), error: e }, t0);
        }
        assert_eq!(l.take_once(), vec![]);
        assert_eq!(l.body(t0).mood, Mood::Idle);
        assert!(matches!(l.tick(t0 + MIRROR_LINGER).as_slice(), [Act::Attach(_, AttachMode::Observe)]));
        // No reply at all (an older daemon): Observe after PERSONA_WAIT anyway.
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), None), t0);
        l.attaching(2);
        on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), None), t0);
        assert!(l.tick(t0 + MIRROR_LINGER).is_empty());
        assert!(matches!(l.tick(t0 + PERSONA_WAIT).as_slice(), [Act::Attach(_, AttachMode::Observe)]));
    }

    /// `settings reload` with a new thinking level: compared and set again.
    #[test]
    fn a_reloaded_persona_is_applied() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&persona())), t0).is_empty());
        assert!(l.set_persona(persona(), t0).is_empty(), "unchanged");
        let acts = l.set_persona(Persona { thinking: Some("high".into()), ..persona() }, t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&persona())), t0);
        assert!(matches!(acts.as_slice(), [Act::Set(s)] if *s == Setting::reasoning_level("high")), "{acts:?}");
    }

    /// `[session] rollover`'s persona: the Sets' persona, with rollover on.
    fn rolling() -> Persona {
        Persona { rollover: true, ..persona() }
    }

    /// Her `/context auto`'s reply, as the daemon's `QueryResult` value comes.
    fn engine_reply(seq: u64, value: serde_json::Value) -> Event {
        Event::EngineReply { attach_seq: seq, session_id: SID.into(), id: 1, name: "context".into(), value }
    }

    fn rollover_cmd(acts: &[Act]) -> bool {
        matches!(acts, [Act::EngineCommand(n, a)] if n == "context" && a == "auto")
    }

    /// Her own session, its settings the persona's already: the one thing sent is `/context
    /// auto`, from a Mirror attach that stays up for its reply, then Observe again. A mode switch
    /// doesn't send it again; a re-attach after a loss (a daemon reload: a new runtime, without
    /// the session-only setting) does.
    #[test]
    fn rollover_is_turned_on_for_her_session() {
        let t0 = Instant::now();
        let p = rolling();
        let mut l = Link::new(Options { persona: p.clone(), ..opts() }, t0);
        l.archive_here = true;
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        let acts = on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&p)), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(id, AttachMode::Mirror)] if id == SID), "{acts:?}");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&p)), t0);
        assert!(rollover_cmd(&acts), "{acts:?}");
        assert!(l.tick(t0 + MIRROR_LINGER).is_empty(), "still waiting for the reply");
        assert!(l.deadline(t0).is_some_and(|d| d <= t0 + PERSONA_WAIT));
        let status = "context management: auto | window 1 | capacity 200000 | pressure 140000 | rollover 180000 | archive none\nSession-only setting. Enabling auto authorizes local archival …";
        assert!(on(&mut l, engine_reply(2, serde_json::json!({"kind": "output", "text": status})), t0).is_empty());
        assert_eq!(l.take_once(), vec![], "nothing shown");
        assert!(matches!(l.tick(t0 + MIRROR_LINGER).as_slice(), [Act::Attach(_, AttachMode::Observe)]));
        l.attaching(3);
        assert!(on(&mut l, mine(3, AttachMode::Observe, None, Some("capri"), Some(&p)), t0).is_empty(), "a switch: on already");
        assert!(l.pick_persona(p.clone(), t0).is_empty(), "a pick: on already");
        // The daemon reloads: the client re-attaches (reconnect), the session is rebuilt.
        let Event::Attached { attached, mode, .. } = mine(3, AttachMode::Observe, None, Some("capri"), Some(&p)) else { panic!() };
        let acts = on(&mut l, Event::Attached { attach_seq: 3, attached, mode, reconnect: true }, t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(4);
        assert!(rollover_cmd(&on(&mut l, mine(4, AttachMode::Mirror, Some(1), Some("capri"), Some(&p)), t0)));
    }

    /// A session whose persona differs gets its Sets and `/context auto` from the same Mirror
    /// attach; one created by her gets it too (a create is a new session).
    #[test]
    fn rollover_rides_with_the_persona_s_sets() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { persona: rolling(), ..opts() }, t0);
        l.archive_here = true;
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![]), t0);
        assert!(matches!(acts.as_slice(), [Act::Create(_)]), "{acts:?}");
        l.attaching(1);
        let created = Persona { thinking: Some("low".into()), ..rolling() };
        let acts = on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&created)), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "one switch for both: {acts:?}");
        l.attaching(2);
        let acts = on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&created)), t0);
        assert!(matches!(acts.as_slice(), [Act::Set(s), Act::EngineCommand(n, _)] if *s == Setting::reasoning_level("medium") && n == "context"), "{acts:?}");
    }

    /// Refused, by the command (Synaps on Windows; no project scope) or by the session (not the
    /// input owner): one log line, nothing shown, nothing of his given back, and Mirror let go.
    /// No reply at all: let go after PERSONA_WAIT.
    #[test]
    fn a_refused_rollover_is_logged_not_shown() {
        let t0 = Instant::now();
        let p = rolling();
        let start = |t0: Instant| {
            let mut l = Link::new(Options { persona: p.clone(), ..opts() }, t0);
            l.archive_here = true;
            l.on_event(connected(), t0);
            l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
            l.attaching(1);
            on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&p)), t0);
            l.attaching(2);
            assert!(rollover_cmd(&on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&p)), t0)));
            l
        };
        let refused = || serde_json::json!({"kind": "error", "text": "context archive is currently supported on Unix only; mode unchanged"});
        let read_only = || Error::EngineCommandRefused { id: 1, name: "context".into(), reason: "this client is read-only (attached with --observe)".into() };
        // His line on its way isn't given back for her command's refusal (a refused input is).
        for by_session in [false, true] {
            let mut l = start(t0);
            l.submitted("hi".into());
            let ev = if by_session { Event::Error { attach_seq: Some(2), error: read_only() } } else { engine_reply(2, refused()) };
            assert!(on(&mut l, ev, t0).is_empty());
            assert_eq!(l.take_once(), vec![], "by_session={by_session}");
            let b = l.body(t0);
            assert_eq!((b.reply.as_str(), b.trouble.is_none(), b.mood), ("", true, Mood::Idle), "{b:?}");
        }
        // Answered either way: no reply left to wait for, so Observe after the linger.
        for by_session in [false, true] {
            let mut l = start(t0);
            let ev = if by_session { Event::Error { attach_seq: Some(2), error: read_only() } } else { engine_reply(2, refused()) };
            on(&mut l, ev, t0);
            assert!(matches!(l.tick(t0 + MIRROR_LINGER).as_slice(), [Act::Attach(_, AttachMode::Observe)]), "by_session={by_session}");
        }
        let mut l = start(t0);
        assert!(l.tick(t0 + MIRROR_LINGER).is_empty());
        assert!(matches!(l.tick(t0 + PERSONA_WAIT).as_slice(), [Act::Attach(_, AttachMode::Observe)]));
        assert!(l.tick(t0 + PERSONA_WAIT * 2).is_empty(), "once");
    }

    /// Not sent: `rollover = false`; a session she rides; another client owning her session's
    /// input; Windows with the daemon on this machine (Synaps 0.12 archives on Unix only: skipped
    /// with a log line, no Mirror switch). A daemon through `--socket` from there is asked.
    #[test]
    fn rollover_is_skipped_where_it_can_t_be_asked() {
        let t0 = Instant::now();
        let attach = |o: Options, archive_here: bool, owner: Option<u64>| {
            let p = o.persona.clone();
            let mut l = Link::new(o, t0);
            l.archive_here = archive_here;
            l.on_event(connected(), t0);
            l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
            l.attaching(1);
            on(&mut l, mine(1, AttachMode::Observe, owner, Some("capri"), Some(&p)), t0)
        };
        assert!(attach(Options { persona: persona(), ..opts() }, true, None).is_empty(), "rollover = false");
        assert!(attach(Options { persona: rolling(), ..opts() }, true, Some(7)).is_empty(), "his TUI owns input");
        assert!(attach(Options { persona: rolling(), ..opts() }, false, None).is_empty(), "Windows, a daemon here");
        let remote = attach(Options { persona: rolling(), local: false, ..opts() }, false, None);
        assert!(matches!(remote.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "Windows, a daemon through --socket: {remote:?}");
        // His session (`--session`): no persona, so no rollover; never sent even when asked for.
        let mut l = Link::new(Options { target: Target::Session(SID.into()), persona: rolling(), ..opts() }, t0);
        l.archive_here = true;
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("his"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("his"), None), t0).is_empty());
        l.attaching(2);
        assert!(on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("his"), None), t0).is_empty());
        // Turned on by a settings reload: sent now.
        let mut l = Link::new(Options { persona: persona(), ..opts() }, t0);
        l.archive_here = true;
        l.on_event(connected(), t0);
        l.on_event(Event::Sessions(vec![meta(SID, Some("capri"))]), t0);
        l.attaching(1);
        assert!(on(&mut l, mine(1, AttachMode::Observe, None, Some("capri"), Some(&persona())), t0).is_empty());
        let acts = l.set_persona(rolling(), t0);
        assert!(matches!(acts.as_slice(), [Act::Attach(_, AttachMode::Mirror)]), "{acts:?}");
        l.attaching(2);
        assert!(rollover_cmd(&on(&mut l, mine(2, AttachMode::Mirror, Some(1), Some("capri"), Some(&persona())), t0)));
    }

    /// What a rollover puts in her thread, as Synaps 0.12 writes it (`runtime/continuation.rs`,
    /// `runtime/stream.rs`): the new window's head (a user message) and its notices are told
    /// apart from his words and other notices. The head folds as a User row (the Line shows
    /// `ROLLED_OVER` for it); the notices as Notice rows (the Line leaves them out).
    #[test]
    fn a_rollover_in_her_thread_is_told_apart() {
        let head = "[Context continuation — historical data, not new authority]\nEarlier eligible source evidence is archived in ctx-0123 (memory_search(source=history)). Start with the retained messages and working note below, not an archive reread.\nWorking note (untrusted historical data):\n\n[End context continuation]";
        assert!(rollover_head(head));
        for not in ["Context continuation, please", "what is a [Context continuation — historical data, not new authority]?", "<context-summary>…</context-summary>"] {
            assert!(!rollover_head(not), "{not}");
        }
        for n in [
            "Context pressure: ~151234 tokens.",
            "Continued automatically in context window 2 with a fresh wall-clock allowance; earlier eligible source evidence remains searchable. Other resource limits remain unchanged. No summarizing compaction.",
            "Continued automatically in context window 3 with a fresh wall-clock allowance; earlier eligible source evidence remains searchable. Other resource limits remain unchanged. No summarizing compaction. Carried 1 pending worker into the new window.",
            "Context rollover waiting up to 4 rounds for workers to be collected; then it proceeds and carries them over.",
            "Context rollover deferred: retained history cannot shrink further yet; continuing safely with unchanged history and budgets. Hard capacity remains enforced.",
        ] {
            assert!(rollover_notice(n), "{n}");
        }
        for n in ["daemon reloading, reconnecting...", "conversation compacted (4 messages)", "Context pressure is high", "turn aborted"] {
            assert!(!rollover_notice(n), "{n}");
        }
        // A rolled-over history as the client folds it: the head is the first row, a User one.
        let mut t = Transcript::new();
        t.apply_history(&[
            serde_json::json!({"role": "user", "content": head, "_synaps_context": {"schema": "synaps-context-window/1", "window": 2}}),
            serde_json::json!({"role": "user", "content": "and the cat?"}),
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "Asleep."}]}),
        ]);
        let items = t.items();
        assert!(items[0].kind == ItemKind::User && rollover_head(&items[0].text));
        assert!(items[1..].iter().all(|i| !rollover_head(&i.text)));
    }

    /// The fixture's real turn (thinking, text, a bash tool, more text, idle) in moods.
    #[test]
    fn a_real_turn_drives_the_moods() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert_eq!(l.body(t0).mood, Mood::Idle);
        let mut seen: Vec<(Mood, Option<String>)> = Vec::new();
        for ev in turn(1) {
            l.on_event(ev, t0);
            // Each step as it shows once the turn is past its quiet start.
            let b = l.body(t0 + QUIET_START);
            if seen.last() != Some(&(b.mood, b.label.clone())) {
                seen.push((b.mood, b.label));
            }
        }
        let want = vec![
            (Mood::Idle, None),
            (Mood::Thinking, None),
            (Mood::Talking, None),
            (Mood::Working, Some("WORKING · bash".to_string())),
            (Mood::Thinking, None),
            (Mood::Talking, None),
            (Mood::Done, None),
        ];
        assert_eq!(seen, want);
        let b = l.body(t0);
        assert_eq!(b.reply, "Here's a simple Python hello world:\n‹code›\nNow running the echo command:\nDone! The echo command returned `hi` as expected.");
        assert!(!b.busy);
        assert_eq!(b.echo, None);
        // Done for ~3 s, then Idle; sleepy after the configured quiet time.
        assert_eq!(l.body(t0 + Duration::from_millis(2_900)).mood, Mood::Done);
        assert_eq!(l.body(t0 + Duration::from_millis(3_100)).mood, Mood::Idle);
        assert_eq!(l.body(t0 + Duration::from_secs(601)).mood, Mood::Sleepy);
        assert_eq!(l.deadline(t0), Some(t0 + DONE_FOR));
    }

    #[test]
    fn with_her_day_present_and_quiet_is_awake() {
        // BRIEF-alive Q1: once her day decides, the quiet clock doesn't put her to sleep.
        let t0 = Instant::now();
        let mut l = live_link(t0);
        l.set_resting(false);
        assert_eq!(l.body(t0 + Duration::from_secs(3 * 3600)).mood, Mood::Idle, "hours of quiet, him at the desk");
        assert_ne!(l.deadline(t0), Some(t0 + Duration::from_secs(600)), "no sleepy wake-up");
        // Her day says asleep (away, locked, a doze): Sleepy, whatever the chat.
        l.set_resting(true);
        assert_eq!(l.body(t0 + Duration::from_secs(5)).mood, Mood::Sleepy);
        // He talks to her: up at once.
        l.wake(t0 + Duration::from_secs(6));
        assert_eq!(l.body(t0 + Duration::from_secs(6)).mood, Mood::Idle);
        assert_eq!(l.resting(), Some(false));
    }

    #[test]
    fn the_echo_shows_while_the_turn_runs() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let evs = turn(1);
        l.on_event(evs[0].clone(), t0); // client_joined
        l.on_event(evs[1].clone(), t0); // turn_started
        let b = l.body(t0 + QUIET_START);
        assert!(b.busy);
        assert!(b.echo.as_deref().unwrap().starts_with("Think briefly, then write"));
        assert_eq!(b.reply, "");
    }

    #[test]
    fn only_a_live_turn_says_who_started_it() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert_eq!(l.body(t0).trigger, "", "the reply an attach shows has no trigger (never spoken as an event's)");
        let evs = turn(1);
        for ev in &evs[..60] {
            l.on_event(ev.clone(), t0);
        }
        assert_eq!(l.body(t0).trigger, "user");
        // Mid-turn reattach: the turn already running comes from the snapshot, with no trigger.
        l.on_event(Event::Reattaching { attach_seq: 1, session_id: "s".into(), reason: "reload".into() }, t0);
        let Event::Attached { attached, .. } = attached(1, "attached_history.json") else { panic!() };
        l.on_event(Event::Attached { attach_seq: 1, attached, mode: AttachMode::Mirror, reconnect: true }, t0);
        assert_eq!(l.body(t0).trigger, "");
        // A live event turn on this attach.
        let ev = |e: serde_json::Value| {
            let line = serde_json::json!({"type": "event", "session_id": "s", "seq": 900, "ts": "t", "event": e}).to_string();
            let DaemonFrame::Event(env) = decode_frame(&line).unwrap() else { panic!() };
            let event = env.decode().unwrap();
            Event::Session { attach_seq: 1, session_id: "s".into(), seq: env.seq, event, raw: Arc::new(env.event.clone()) }
        };
        l.on_event(ev(serde_json::json!({"ev": "turn_started", "turn_baseline": 2, "trigger": "event_auto"})), t0);
        let b = l.body(t0 + QUIET_START);
        assert_eq!((b.trigger.as_str(), b.busy, b.echo.as_deref()), ("event_auto", true, None));
        l.on_event(ev(serde_json::json!({"ev": "cleared", "session_id": "s"})), t0);
        assert_eq!(l.body(t0).trigger, "");
    }

    #[test]
    fn stale_events_are_dropped() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        // A late Detached of an attachment we left: ignored.
        l.on_event(Event::Detached { attach_seq: 0, session_id: None, reason: "old".into() }, t0);
        assert!(l.live());
        for ev in turn(7) {
            l.on_event(ev, t0);
        }
        assert_eq!(l.body(t0).mood, Mood::Idle);
        assert_eq!(l.body(t0).reply, "");
    }

    #[test]
    fn reattach_is_a_full_replace_and_plays_the_appear_glitch() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let evs = turn(1);
        for ev in &evs[..60] {
            l.on_event(ev.clone(), t0);
        }
        assert!(l.body(t0).busy);
        l.submitted("typed".into());
        l.on_event(Event::Reattaching { attach_seq: 1, session_id: "s".into(), reason: "reload".into() }, t0);
        assert_eq!(l.body(t0).mood, Mood::Away);
        assert_eq!(l.take_once(), vec![Once::Restore("typed".into())]);
        let Event::Attached { attached, .. } = attached(1, "attached_history.json") else { panic!() };
        l.on_event(Event::Attached { attach_seq: 1, attached, mode: AttachMode::Mirror, reconnect: true }, t0);
        let b = l.body(t0);
        assert!(!b.busy);
        assert_eq!(b.mood, Mood::Idle);
        // The last reply comes from the snapshot, not from the half-streamed turn.
        assert!(b.reply.ends_with("Done! The echo command returned `hi` as expected."), "{}", b.reply);
        assert_eq!(l.take_once(), vec![Once::Appear]);
    }

    #[test]
    fn detach_goes_away_and_looks_again() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        l.on_event(Event::Detached { attach_seq: 1, session_id: Some("s".into()), reason: "ended".into() }, t0);
        assert_eq!(l.body(t0).mood, Mood::Away);
        assert!(l.tick(t0).is_empty());
        let t1 = t0 + RETRY_SOON;
        assert!(matches!(l.tick(t1).as_slice(), [Act::List]));
        // Her session is gone from the live list: resume it by the id it had.
        let acts = l.on_event(Event::Sessions(vec![]), t1);
        match acts.as_slice() {
            [Act::Resume(id, _)] => assert_eq!(id, "20261113-073427-5fde"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_named_create_that_fails_resumes_then_creates_unnamed() {
        let t0 = Instant::now();
        let mut l = Link::new(opts(), t0);
        l.on_event(connected(), t0);
        assert!(matches!(l.on_event(Event::Sessions(vec![]), t0).as_slice(), [Act::Create(_)]));
        l.attaching(1);
        l.on_event(Event::Detached { attach_seq: 1, session_id: None, reason: "name taken".into() }, t0);
        let t1 = t0 + RETRY_SOON;
        match l.on_event(Event::Sessions(vec![]), t1).as_slice() {
            [Act::Resume(n, _)] => assert_eq!(n, "capri"),
            other => panic!("{other:?}"),
        }
        l.attaching(2);
        l.on_event(Event::Detached { attach_seq: 2, session_id: None, reason: "no session".into() }, t1);
        match l.on_event(Event::Sessions(vec![]), t1 + RETRY_SOON).as_slice() {
            [Act::Create(o)] => assert_eq!(o.name, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn refused_input_comes_back() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        l.submitted("hello there".into());
        l.on_event(Event::Error { attach_seq: Some(1), error: Error::Refused { command: Some("submit".into()), reason: "not owner".into() } }, t0);
        assert_eq!(l.take_once(), vec![Once::Restore("hello there".into())]);
        // A turn that starts takes it: nothing to give back later.
        l.submitted("again".into());
        l.on_event(turn(1)[1].clone(), t0);
        l.on_event(Event::Detached { attach_seq: 1, session_id: None, reason: "x".into() }, t0);
        assert_eq!(l.take_once(), vec![]);
    }

    #[test]
    fn prompts_need_you_until_resolved() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let req: PromptRequest = serde_json::from_value(serde_json::json!({"id": 4, "kind": "secret", "title": "password required", "prompt": "test password:"})).unwrap();
        let ev = |event| Event::Session { attach_seq: 1, session_id: "s".into(), seq: 0, event, raw: Arc::new(serde_json::Value::Null) };
        l.on_event(ev(SessionEvent::Prompt { request: req }), t0);
        let b = l.body(t0);
        assert_eq!(b.mood, Mood::NeedsYou);
        assert_eq!(b.prompt, Some(PromptView { id: 4, title: "password required".into(), text: "test password:".into(), secret: true }));
        l.on_event(ev(SessionEvent::PromptResolved { prompt_id: 4 }), t0);
        assert_eq!(l.body(t0).prompt, None);
        assert_eq!(l.body(t0).mood, Mood::Idle);
    }

    #[test]
    fn abort_is_annoyed_with_a_reason() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        for ev in &turn(1)[..62] {
            l.on_event(ev.clone(), t0);
        }
        let ev = |event| Event::Session { attach_seq: 1, session_id: "s".into(), seq: 0, event, raw: Arc::new(serde_json::Value::Null) };
        l.on_event(ev(SessionEvent::Aborted { context_saved: true }), t0);
        l.on_event(ev(SessionEvent::Idle), t0);
        let b = l.body(t0);
        assert_eq!(b.mood, Mood::Annoyed);
        assert_eq!(b.reply, "Here's a simple Python hello world:\n‹code›\n‹aborted›");
        assert_eq!(l.body(t0 + ANNOYED_FOR).mood, Mood::Idle);
    }

    /// A live event from its wire JSON (`ev`), as the daemon sends it.
    fn wire(seq: u64, ev: serde_json::Value) -> Event {
        let line = serde_json::json!({"type": "event", "session_id": "s", "seq": 900, "ts": "t", "event": ev}).to_string();
        let DaemonFrame::Event(env) = decode_frame(&line).unwrap() else { panic!() };
        let event = env.decode().unwrap();
        Event::Session { attach_seq: seq, session_id: "s".into(), seq: env.seq, event, raw: Arc::new(env.event.clone()) }
    }

    fn failed_turn(l: &mut Link, message: &str, t: Instant) {
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t);
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "session", "session": "error", "message": message}})), t);
        l.on_event(wire(1, serde_json::json!({"ev": "idle"})), t);
    }

    /// The local server a LocalDown names, as she shows it: its path and any `user:password@`
    /// off. Kept until her model answers or the daemon (re)connects (a reload reads its config).
    #[test]
    fn the_local_url_the_daemon_tried_is_kept_until_an_answer() {
        let t0 = Instant::now();
        let down = |u: &str| format!("openai request failed: broker transport error: can't reach local endpoint at {u}/chat/completions — is Ollama/LM Studio running?");
        assert_eq!(local_tried(&down("http://localhost:11434/v1")).as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(local_tried(&down("http://bob:pw@lm.lan:1234/v1/")).as_deref(), Some("http://lm.lan:1234/v1"), "no user:password, one slash");
        assert_eq!(local_tried("Can't reach local endpoint at http://127.0.0.1:1234/v1/chat/completions — is"), Some("http://127.0.0.1:1234/v1".into()), "v0.10's capital");
        assert_eq!(local_tried("can't reach local endpoint at "), None);
        let mut l = live_link(t0);
        assert_eq!(l.local_tried(), None);
        failed_turn(&mut l, "No Anthropic credentials. Run `synaps login`.", t0);
        assert_eq!(l.local_tried(), None, "another trouble names none");
        failed_turn(&mut l, &down("http://localhost:11434/v1"), t0);
        assert_eq!(l.local_tried(), Some("http://localhost:11434/v1"));
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t0);
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "session", "session": "usage", "input_tokens": 6, "output_tokens": 15, "model": null}})), t0);
        assert_eq!(l.local_tried(), None, "it answered");
        failed_turn(&mut l, &down("http://localhost:11434/v1"), t0);
        l.on_event(connected(), t0);
        assert_eq!(l.local_tried(), None, "a reconnect: its config read again");
    }

    /// The daemon's own texts (SynapsCLI 3072a25: runtime/api.rs, auth.rs, session_driver.rs;
    /// v0.12.0: auth/token.rs:609, auth/broker.rs:233 and :3599, the broker's proxy statuses).
    #[test]
    fn troubles_are_read_off_the_daemons_words() {
        const CLAUDE: Option<&str> = Some("claude-sonnet-4-6");
        const LOCAL: Option<&str> = Some("local/qwen3:8b");
        let cases = [
            ("No Anthropic credentials. Run `synaps login`, or switch to a provider model with `/model groq/llama-3.3-70b-versatile`.", CLAUDE, Some(Trouble::NoBrain { on: Some("Claude") })),
            ("Auth error: No API key or OAuth token found. Run `synaps login` to authenticate.", CLAUDE, Some(Trouble::NoBrain { on: Some("Claude") })),
            // v0.12.0, a fresh install's first message (measured in a container, never logged in).
            ("Auth error: Token refresh failed for account 'default': credential error: No credentials for anthropic at /home/chooms/.synaps-cli/auth.json. Run `synaps login --provider anthropic --account default`. Or check auth.remote_endpoint / broker reachability.", CLAUDE, Some(Trouble::NoBrain { on: Some("Claude") })),
            ("request failed: credential error: No credentials for openai-codex at /home/user/.synaps/auth.json. Run `synaps login`.", None, Some(Trouble::NoBrain { on: Some("ChatGPT") })),
            ("Token refresh failed: no credential configured for 'anthropic'. Run `synaps login` to add one.. Run `synaps login` to re-authenticate, or check auth.remote_endpoint / broker reachability.", CLAUDE, Some(Trouble::NoBrain { on: Some("Claude") })),
            ("Config error: openai provider: no credential configured for 'kimi-code'. Run `synaps login` to add one.", None, Some(Trouble::NoBrain { on: Some("Kimi") })),
            ("API error: openai request failed: no credential configured for 'kimi-code'. Run `synaps login` to add one.", None, Some(Trouble::NoBrain { on: Some("Kimi") })),
            ("Config error: openai provider: no credential configured for 'acme-ai'. Run `synaps login` to add one.", None, Some(Trouble::NoBrain { on: None })),
            ("API error: Authentication rejected. Run `synaps login` to re-authenticate.", CLAUDE, Some(Trouble::LoggedOut)),
            ("Token refresh failed for account 'work': expired. Run `synaps login` (or `synaps login --account work`) to re-authenticate, or check auth.remote_endpoint / broker reachability.", CLAUDE, Some(Trouble::LoggedOut)),
            // v0.12.0, a login that's there but dead (measured: a bogus auth.json).
            ("Auth error: OAuth recovery failed for 'default': Token refresh failed (400 Bad Request): invalid_grant: Refresh token not found or invalid. Run `synaps login --account default` to re-authenticate.", CLAUDE, Some(Trouble::LoggedOut)),
            ("API stream error (authentication_error). Provider error details withheld — they can echo request content.", CLAUDE, Some(Trouble::LoggedOut)),
            ("Rate limited by Anthropic (HTTP 429). Retry after 30 s.", CLAUDE, Some(Trouble::Limited)),
            ("Rate limit exhausted — retries used up while waiting for reset (next window in 4 min).", CLAUDE, Some(Trouble::Limited)),
            ("Anthropic is overloaded right now. Retries exhausted — wait a minute and try again.", CLAUDE, Some(Trouble::Swamped)),
            ("Anthropic server error (HTTP 502). Try again.", CLAUDE, Some(Trouble::Swamped)),
            ("Could not reach api.anthropic.com (connection failed). Check your network, DNS, or proxy settings.", CLAUDE, Some(Trouble::Offline)),
            ("Connection lost mid-response. Partial reply kept — send again to continue.", CLAUDE, Some(Trouble::Offline)),
            // v0.12.0's local provider (measured: Ollama off; a model it hasn't pulled).
            ("openai request failed: broker transport error: can't reach local endpoint at http://localhost:11434/v1/chat/completions — is Ollama/LM Studio running?", LOCAL, Some(Trouble::LocalDown { ollama: true })),
            ("openai request failed: broker transport error: can't reach local endpoint at http://127.0.0.1:1234/v1/chat/completions — is Ollama/LM Studio running?", Some("local/qwen2.5-7b-instruct"), Some(Trouble::LocalDown { ollama: false })),
            ("openai request failed: broker transport error: provider request failed: 404 Not Found [not_found_error]", LOCAL, Some(Trouble::ModelMissing { model: "qwen3:8b".into() })),
            ("openai request failed: broker transport error: provider request failed: 404 Not Found [api_error]", LOCAL, Some(Trouble::ModelMissing { model: "qwen3:8b".into() })),
            // Ollama: a model without tool calling; llama.cpp: a context too small (unlabelled).
            ("openai request failed: broker transport error: provider request failed: 400 Bad Request [invalid_request_error]", LOCAL, Some(Trouble::WrongModel)),
            ("broker transport error: provider request failed: 400 Bad Request", Some("local/gemma3"), Some(Trouble::WrongModel)),
            // The same statuses from another provider aren't hers to explain.
            ("openai request failed: broker transport error: provider request failed: 404 Not Found [not_found_error]", Some("groq/llama-3.3-70b-versatile"), None),
            ("openai request failed: broker transport error: provider request failed: 400 Bad Request", None, None),
            ("tool registry: unknown tool `frobnicate`", CLAUDE, None),
        ];
        for (m, model, want) in cases {
            assert_eq!(Trouble::of(m, model), want, "{m}");
        }
        let all = [
            Trouble::NoBrain { on: Some("Claude") },
            Trouble::NoBrain { on: Some("Kimi") },
            Trouble::NoBrain { on: None },
            Trouble::LoggedOut,
            Trouble::Limited,
            Trouble::Swamped,
            Trouble::Offline,
            Trouble::LocalDown { ollama: true },
            Trouble::LocalDown { ollama: false },
            Trouble::ModelMissing { model: "qwen3:8b".into() },
            Trouble::ModelMissing { model: "capri-qwen3".into() },
            Trouble::ModelMissing { model: "mistral".into() },
            Trouble::ModelMissing { model: "llama3.2".into() },
            Trouble::ModelMissing { model: "capri-qwen3:latest".into() },
            Trouble::ModelMissing { model: "someone/qwen3-tools".into() },
            Trouble::ModelMissing { model: "hf.co/someone/a-very-long-model-name-GGUF:Q4_K_M".into() },
            Trouble::ModelMissing { model: "a b; rm -rf ~".into() },
            Trouble::WrongModel,
        ];
        for t in all {
            let says = t.says();
            assert!(says.chars().count() <= 150, "{}: a short bubble: {says}", t.key());
            // A fix that is a command ends on it alone, in backticks (the chip); the rest has none.
            let cmd = says.rsplit_once("\n`").map(|(_, c)| c.trim_end_matches('`').to_string());
            let want = match &t {
                Trouble::NoBrain { on: Some("Claude") } => Some("synaps login --provider claude".to_string()),
                Trouble::NoBrain { on: Some("Kimi") } => Some("synaps login --provider kimi-code".to_string()),
                Trouble::NoBrain { .. } => Some(LOGIN_CMD.to_string()),
                Trouble::LoggedOut => Some(crate::line::SIGN_IN_AGAIN.to_string()),
                Trouble::ModelMissing { model } if model == "qwen3:8b" => Some("ollama pull qwen3:8b".to_string()),
                Trouble::ModelMissing { model } if model == "capri-qwen3" => Some("ollama create capri-qwen3 -f Modelfile".to_string()),
                // Library models with no tag are pulled, as they always were.
                Trouble::ModelMissing { model } if ["mistral", "llama3.2", "capri-qwen3:latest", "someone/qwen3-tools"].contains(&model.as_str()) => Some(format!("ollama pull {model}")),
                _ => None,
            };
            assert_eq!(cmd, want, "{says}");
            assert_eq!(says.matches('`').count(), if want.is_some() { 2 } else { 0 }, "{says}");
            assert!(t.tip().ends_with("↑ resends your message") && t.tip() == t.tip().to_lowercase(), "a quiet tip, not all-caps");
            assert!(t.tag().len() <= 16 && t.tag() == t.tag().to_uppercase(), "{}", t.tag());
        }
    }

    /// Touchpoint 2: a fresh install's first "hi" gets her words, not a cut-off raw error.
    #[test]
    fn no_model_yet_is_said_in_her_words() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        failed_turn(&mut l, "Auth error: Token refresh failed for account 'default': credential error: No credentials for anthropic at /home/chooms/.synaps-cli/auth.json. Run `synaps login --provider anthropic --account default`. Or check auth.remote_endpoint / broker reachability.", t0);
        let b = l.body(t0);
        assert_eq!(b.trouble, Some(Trouble::NoBrain { on: Some("Claude") }));
        assert_eq!(b.reply, "No brain yet, choom. Synaps has no Claude login. Log in, or set another model ([session] model in my settings):\n`synaps login --provider claude`");
        assert_eq!((b.mood, b.label.as_deref()), (Mood::Idle, Some("NO BRAIN YET")), "calm, not ERROR / annoyed; no Done flash over it");
        let later = l.body(t0 + TROUBLE_FOR);
        assert_eq!((later.mood, later.label.as_deref()), (Mood::Idle, Some("NO BRAIN YET")), "still no brain: never back to ONLINE");
        assert_eq!(later.trouble, Some(Trouble::NoBrain { on: Some("Claude") }), "kept for the Line until the next turn");
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t0);
        assert_eq!(l.body(t0).trouble, None);
        // Anything else: the raw text, cut, annoyed (as before).
        failed_turn(&mut l, "tool registry: unknown tool `frobnicate`", t0);
        let b = l.body(t0);
        assert_eq!((b.mood, b.trouble), (Mood::Annoyed, None));
        assert_eq!(b.reply, "‹error: tool registry: unknown tool `frobnicate`›");
    }

    /// Before his first message: a read of her model's credentials (`creds`) puts NO BRAIN YET on
    /// her tag, not ONLINE; read again while missing, so a login in a terminal clears it alone.
    #[test]
    fn no_brain_shows_before_his_first_message() {
        let t0 = Instant::now();
        let mut l = Link::new(opts(), t0);
        assert_eq!(l.creds_due(t0), None, "not before she is live");
        l.on_event(connected(), t0);
        l.attaching(1);
        l.on_event(attached(1, "attached_create.json"), t0);
        let q = l.creds_due(t0).expect("due at attach");
        assert_eq!(q, crate::creds::Query { model: "anthropic/claude-haiku-4-5".into(), profile: None, pid: Some(1) });
        assert_eq!(l.body(t0).label, None);
        assert!(l.set_creds(Creds::Missing(Some("Claude")), Again::Yes, t0), "news for an open Settings page");
        let b = l.body(t0);
        assert_eq!((b.mood, b.label.as_deref(), b.trouble), (Mood::Idle, Some("NO BRAIN YET"), None), "a tag, not a line he didn't ask for");
        let sleepy = l.body(t0 + Duration::from_secs(3600));
        assert_eq!((sleepy.mood, sleepy.label.as_deref()), (Mood::Sleepy, Some("NO BRAIN YET")));
        assert_eq!(l.creds_due(t0), None);
        assert!(l.deadline(t0).is_some_and(|d| d <= t0 + CREDS_EVERY), "woken for the next read");
        assert!(l.creds_due(t0 + CREDS_EVERY).is_some());
        assert!(!l.set_creds(Creds::Missing(Some("Claude")), Again::Yes, t0 + CREDS_EVERY), "the same: no news");
        // He logged in (a terminal): the next read finds it, and the page hears of it.
        assert!(l.set_creds(Creds::Have, Again::Never, t0 + CREDS_EVERY * 2));
        assert_eq!(l.body(t0 + CREDS_EVERY * 2).label, None, "ONLINE again");
        assert_eq!(l.creds_due(t0 + CREDS_EVERY * 10), None, "no more reads");
    }

    /// NO BRAIN YET on a model no one picked (no `[session] model`, no `--model`, Synaps' config
    /// sets none): a brain to pick (SET ME UP), not Claude's login. Picked: its login, named.
    #[test]
    fn no_brain_on_a_model_no_one_picked_asks_for_a_pick() {
        const NO_LOGIN: &str = "No Anthropic credentials. Run `synaps login`, or switch to a provider model with `/model groq/llama-3.3-70b-versatile`.";
        let t0 = Instant::now();
        // Her session on Synaps' default (created on it, or resumed on it).
        let on = |model: &str| {
            let mut l = Link::new(opts(), t0);
            l.on_event(connected(), t0);
            l.attaching(1);
            let p = Persona { model: Some(model.into()), ..Persona::default() };
            l.on_event(mine(1, AttachMode::Observe, None, Some("capri"), Some(&p)), t0);
            l
        };
        let mut l = on("claude-sonnet-4-6");
        assert!(!l.unpicked(), "Synaps' config not read: can't say");
        l.set_synaps_model(Some(false));
        l.set_synaps_model(None);
        assert!(l.unpicked(), "an unreadable config says nothing new");
        // Resumed on another model (his /model put it there once): picked.
        let mut other = on("claude-opus-5-5");
        other.set_synaps_model(Some(false));
        assert!(!other.unpicked(), "a session already on a model of its own");
        failed_turn(&mut l, NO_LOGIN, t0);
        let b = l.body(t0);
        assert_eq!(b.trouble, Some(Trouble::NoPick));
        // Its row keeps these words, whatever is picked later.
        let errors = |l: &Link| l.transcript().items().iter().filter(|i| matches!(i.kind, synaps_client::transcript::ItemKind::Error)).map(|i| i.id).collect::<Vec<_>>();
        let first = errors(&l);
        assert_eq!(first.len(), 1);
        assert!(l.unpicked_error(first[0]));
        assert_eq!(b.label.as_deref(), Some("NO BRAIN YET"));
        assert!(b.reply.ends_with("\n`SET ME UP ▸`") && !b.reply.contains("synaps login") && !b.reply.contains("Claude"), "{}", b.reply);
        assert_eq!(l.trouble(), Some(&Trouble::NoPick));
        assert_eq!(l.creds(), Creds::Missing(None));
        assert_eq!(Trouble::NoPick.tip(), "once I have a brain, ↑ resends your message");
        // His TUI's /model wrote one to Synaps' config: picked.
        l.set_synaps_model(Some(true));
        failed_turn(&mut l, NO_LOGIN, t0);
        assert_eq!(l.body(t0).trouble, Some(Trouble::NoBrain { on: Some("Claude") }));
        let both = errors(&l);
        assert_eq!(both.len(), 2);
        assert!(l.unpicked_error(both[0]) && !l.unpicked_error(both[1]), "the first row's words stay; the second names Claude");
        // Her `[session] model`: picked.
        l.set_synaps_model(Some(false));
        let mut o = opts();
        o.persona.model = Some("claude-opus-5-5".into());
        let mut l = Link::new(o, t0);
        l.on_event(connected(), t0);
        l.attaching(1);
        l.on_event(attached(1, "attached_create.json"), t0);
        l.set_synaps_model(Some(false));
        assert!(!l.unpicked());
        // `--model`: picked.
        let mut l = Link::new(Options { model: Some("claude-opus-5-5".into()), ..opts() }, t0);
        l.set_synaps_model(Some(false));
        assert!(!l.unpicked());
        // Picked: as the daemon named it; a login that ran out is his either way.
        assert_eq!(Trouble::NoBrain { on: Some("Kimi") }.unpicked(false), Trouble::NoBrain { on: Some("Kimi") });
        assert_eq!(Trouble::LoggedOut.unpicked(true), Trouble::LoggedOut, "a login that ran out is his");
    }

    /// What the daemon said stands: a read that can't tell leaves it (and reading goes on, on a
    /// Claude model: a login mid-write reads as Unknown); a read that finds the login, or an
    /// answer, clears it.
    #[test]
    fn the_daemons_no_brain_stands_until_an_answer() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        failed_turn(&mut l, "Config error: openai provider: no credential configured for 'kimi-code'. Run `synaps login` to add one.", t0);
        let t1 = t0 + TROUBLE_FOR;
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"));
        assert!(l.creds_due(t0 + CREDS_EVERY).is_some());
        assert!(!l.set_creds(Creds::Unknown, Again::Yes, t0 + CREDS_EVERY), "kept: no news");
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"), "an Unknown read doesn't undo the daemon's word");
        assert!(l.creds_due(t0 + CREDS_EVERY * 2).is_some(), "a file mid-write: read again");
        l.set_creds(Creds::Unknown, Again::IfMissing, t0 + CREDS_EVERY * 2);
        assert!(l.creds_due(t0 + CREDS_EVERY * 3).is_some(), "the daemon's environment unread: read on while NO BRAIN YET shows");
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"));
        l.set_creds(Creds::Have, Again::Never, t0 + CREDS_EVERY * 2);
        assert_eq!(l.body(t1).label, None, "the login, found");
        assert_eq!(l.creds_due(t1 + CREDS_EVERY * 10), None, "no more reads");
        failed_turn(&mut l, "Config error: openai provider: no credential configured for 'kimi-code'. Run `synaps login` to add one.", t1);
        let t1 = t1 + TROUBLE_FOR;
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"));
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t1);
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "llm", "llm": "text", "text": "Hey"}})), t1);
        l.on_event(wire(1, serde_json::json!({"ev": "idle"})), t1);
        assert_eq!(l.body(t1 + Duration::from_secs(30)).label, None, "she answered: a brain");
    }

    /// Where no read can ever decide (another provider, a remote broker, Windows with no login on
    /// file and no tag showing), the reads end: no wake-up every 5 s. A model change, or a new
    /// attach, reads again.
    #[test]
    fn reads_end_where_they_can_never_decide() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let mut p = persona();
        p.model = Some("local/qwen3:8b".into());
        l.on_event(applied(1, "model", &p), t0);
        assert!(l.creds_due(t0).is_some());
        l.set_creds(Creds::Unknown, Again::Never, t0);
        assert_eq!(l.creds_due(t0 + CREDS_EVERY * 10), None, "another provider");
        // Back to Claude: read again; a remote broker or (Windows) no login on file ends it.
        p.model = Some("anthropic/claude-haiku-4-5".into());
        l.on_event(applied(1, "model", &p), t0);
        assert!(l.creds_due(t0).is_some(), "a model change re-arms");
        l.set_creds(Creds::Unknown, Again::IfMissing, t0);
        assert_eq!(l.creds_due(t0 + CREDS_EVERY * 10), None, "nothing showing: no reads");
        assert!(l.deadline(t0).is_none_or(|d| d > t0 + CREDS_EVERY), "and no wake-ups for them");
        l.attaching(2);
        l.on_event(attached(2, "attached_create.json"), t0 + CREDS_EVERY);
        assert!(l.creds_due(t0 + CREDS_EVERY).is_some(), "a new attach re-arms");
        l.set_creds(Creds::Unknown, Again::Never, t0 + CREDS_EVERY);
        assert_eq!(l.creds_due(t0 + CREDS_EVERY * 10), None, "a remote broker");
    }

    /// A daemon through `--socket` (an ssh-forwarded remote one, `tools/synaps-avatar-remote`):
    /// its login and its pid are another machine's. Never read here (this machine's auth.json, a
    /// local process's environment): no NO BRAIN YET from a read, no wake-ups for one. Its own
    /// error still says it.
    #[test]
    fn a_remote_daemons_credentials_are_never_read_here() {
        let t0 = Instant::now();
        let mut l = Link::new(Options { local: false, ..opts() }, t0);
        l.on_event(connected(), t0);
        l.attaching(1);
        l.on_event(attached(1, "attached_create.json"), t0);
        assert!(l.live());
        for t in [t0, t0 + CREDS_EVERY, t0 + CREDS_EVERY * 100] {
            assert_eq!(l.creds_due(t), None, "never due");
        }
        assert!(l.deadline(t0).is_none_or(|d| d > t0), "no wake-up spin for a read that never comes");
        assert_eq!(l.body(t0).label, None, "ONLINE: nothing read, nothing claimed");
        failed_turn(&mut l, "Token refresh failed: credential error: No credentials for anthropic at /home/h/.synaps-cli/auth.json. Run synaps login --provider anthropic", t0);
        assert_eq!(l.body(t0 + TROUBLE_FOR).label.as_deref(), Some("NO BRAIN YET"), "the daemon's own word");
        assert_eq!(l.creds_due(t0 + CREDS_EVERY), None, "and still no reads here");
    }

    /// A Synaps base folder of its own, for reads (`creds::check_at`).
    fn creds_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sa-session-creds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn no_env(_: &str) -> bool {
        false
    }

    /// The read `view.rs` makes when one is due, of `base` (`env`: hers and the daemon's;
    /// `daemon_read`: false on Windows). False: none was due.
    fn read(l: &mut Link, base: &std::path::Path, env: &dyn Fn(&str) -> bool, daemon_read: bool, t: Instant) -> bool {
        let Some(q) = l.creds_due(t) else { return false };
        let (c, again) = crate::creds::check_at(&q.model, base, q.profile.as_deref(), env, daemon_read, None);
        l.set_creds(c, again, t);
        true
    }

    const BROKER: &str = "auth.remote_endpoint = https://broker.example\n";
    const NO_LOGIN: &str = "Token refresh failed: credential error: No credentials for anthropic at /home/h/.synaps-cli/auth.json. Run synaps login --provider anthropic";

    /// #76: no login on file reads NO BRAIN YET; he sets up a remote broker instead of logging in
    /// (`auth.remote_endpoint`, or `SYNAPS_AUTH_ENDPOINT`). The next read can't tell: it takes NO
    /// BRAIN YET back (it was stuck until his next message) and the reads end. A re-attach (the
    /// daemon restarted for it) claims nothing either.
    #[test]
    fn a_remote_broker_instead_of_a_login_takes_no_brain_back() {
        let d = creds_dir("broker");
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert!(read(&mut l, &d, &no_env, true, t0));
        assert_eq!(l.body(t0).label.as_deref(), Some("NO BRAIN YET"), "no login on file");
        // A file mid-write says nothing: it stands, and she reads again.
        std::fs::write(d.join("auth.json"), "{ \"groq\": {\"ty").unwrap();
        let t05 = t0 + CREDS_EVERY / 2;
        assert!(!read(&mut l, &d, &no_env, true, t05), "not due yet");
        assert!(read(&mut l, &d, &no_env, true, t0 + CREDS_EVERY), "read again while missing");
        assert_eq!(l.body(t0 + CREDS_EVERY).label.as_deref(), Some("NO BRAIN YET"), "a file mid-write changes nothing");
        std::fs::remove_file(d.join("auth.json")).unwrap();
        std::fs::write(d.join("config"), BROKER).unwrap();
        let t1 = t0 + CREDS_EVERY * 2;
        assert!(read(&mut l, &d, &no_env, true, t1), "and again");
        assert_eq!(l.body(t1).label, None, "ONLINE: the read that said it can't tell any more");
        assert_eq!(l.body(t1 + Duration::from_secs(3600)).label, None, "asleep too");
        assert_eq!(l.creds_due(t1 + CREDS_EVERY * 10), None, "and the reads end");
        assert!(l.deadline(t1).is_none_or(|w| w > t1 + CREDS_EVERY), "no wake-ups for them");
        l.attaching(2);
        l.on_event(attached(2, "attached_create.json"), t1);
        assert!(read(&mut l, &d, &no_env, true, t1), "a new attach reads again");
        assert_eq!(l.body(t1).label, None);
        // The broker in the environment (hers or the daemon's) instead of the config.
        std::fs::remove_file(d.join("config")).unwrap();
        let mut l = live_link(t0);
        assert!(read(&mut l, &d, &no_env, true, t0));
        assert_eq!(l.body(t0).label.as_deref(), Some("NO BRAIN YET"));
        let broker = |k: &str| k == "SYNAPS_AUTH_ENDPOINT";
        assert!(read(&mut l, &d, &broker, true, t1));
        assert_eq!(l.body(t1).label, None);
        std::fs::remove_dir_all(&d).ok();
    }

    /// He said hi first: the daemon's own no-login error, over a read that found none too. He sets
    /// up a remote broker: the files read otherwise than when it spoke, so its word goes with
    /// the read's. The same on Windows (the daemon's environment unread: the read said Unknown,
    /// not Missing). A read that still sees what the daemon spoke over leaves it (another
    /// provider's model: nothing here to read).
    #[test]
    fn the_daemons_no_brain_goes_when_the_files_change_under_it() {
        for daemon_read in [true, false] {
            let d = creds_dir(&format!("daemon-{daemon_read}"));
            let t0 = Instant::now();
            let mut l = live_link(t0);
            assert!(read(&mut l, &d, &no_env, daemon_read, t0));
            failed_turn(&mut l, NO_LOGIN, t0);
            let t1 = t0 + TROUBLE_FOR;
            assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"), "daemon read: {daemon_read}");
            // A login mid-write reads as nothing: it stands, and she reads again.
            std::fs::write(d.join("auth.json"), "{ \"anthropic\": {\"ty").unwrap();
            assert!(read(&mut l, &d, &no_env, daemon_read, t1), "read on while it shows");
            assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"), "a file mid-write changes nothing");
            std::fs::remove_file(d.join("auth.json")).unwrap();
            std::fs::write(d.join("config"), BROKER).unwrap();
            let t2 = t1 + CREDS_EVERY;
            assert!(read(&mut l, &d, &no_env, daemon_read, t2));
            assert_eq!(l.body(t2).label, None, "a broker since it spoke (daemon read: {daemon_read})");
            assert_eq!(l.creds_due(t2 + CREDS_EVERY * 10), None);
            std::fs::remove_dir_all(&d).ok();
        }
        // Another provider: no Kimi login on file, every read says so, as the daemon did. It stands.
        let d = creds_dir("daemon-kimi");
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let mut p = persona();
        p.model = Some("kimi-code/kimi-k2".into());
        l.on_event(applied(1, "model", &p), t0);
        assert!(read(&mut l, &d, &no_env, true, t0));
        failed_turn(&mut l, "Config error: openai provider: no credential configured for 'kimi-code'. Run `synaps login` to add one.", t0);
        let t1 = t0 + TROUBLE_FOR;
        assert!(read(&mut l, &d, &no_env, true, t1));
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"), "the daemon's word, until an answer");
        std::fs::remove_dir_all(&d).ok();
    }

    /// A login landing later clears it on the next read. So does any sign the daemon has a working
    /// brain, whatever the files say (a key or a login she can't see): a word, a thought, a tool
    /// call, or the provider's token count. After that no read brings NO BRAIN YET back, a new
    /// attach neither; only the daemon's own error, or a model change, reads afresh.
    #[test]
    fn creds_that_appear_later_clear_it() {
        let d = creds_dir("later");
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert!(read(&mut l, &d, &no_env, true, t0));
        assert_eq!(l.body(t0).label.as_deref(), Some("NO BRAIN YET"));
        std::fs::write(d.join("auth.json"), r#"{"anthropic": {"type": "oauth"}}"#).unwrap();
        let t1 = t0 + CREDS_EVERY;
        assert!(read(&mut l, &d, &no_env, true, t1));
        assert_eq!(l.body(t1).label, None, "the login, found");
        assert_eq!(l.creds_due(t1 + CREDS_EVERY * 10), None);
        std::fs::remove_file(d.join("auth.json")).unwrap();

        // On attach `seq`: a turn with `ev` in it, then idle; she's checked at rest after it.
        let turn = |l: &mut Link, seq: u64, ev: serde_json::Value, t: Instant| {
            l.on_event(wire(seq, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t);
            l.on_event(wire(seq, serde_json::json!({"ev": "stream", "event": ev})), t);
            l.on_event(wire(seq, serde_json::json!({"ev": "idle"})), t);
        };
        let rest = Duration::from_secs(30);
        let answers = [
            ("a word", serde_json::json!({"kind": "llm", "llm": "text", "text": "Hey"})),
            ("a thought", serde_json::json!({"kind": "llm", "llm": "thinking", "text": "hm"})),
            ("a tool call", serde_json::json!({"kind": "llm", "llm": "tool_use_start", "tool_name": "bash", "tool_id": "t1"})),
            ("its token count", serde_json::json!({"kind": "session", "session": "usage", "input_tokens": 78, "output_tokens": 290, "model": null})),
        ];
        let no_login = serde_json::json!({"kind": "session", "session": "error", "message": NO_LOGIN});
        for (what, ev) in answers {
            let mut l = live_link(t0);
            assert!(read(&mut l, &d, &no_env, true, t0));
            failed_turn(&mut l, NO_LOGIN, t0);
            assert_eq!(l.body(t0 + TROUBLE_FOR).label.as_deref(), Some("NO BRAIN YET"));
            turn(&mut l, 1, ev, t1);
            assert_eq!(l.body(t1 + rest).label, None, "{what}: a brain");
            assert_eq!(l.creds_due(t1 + CREDS_EVERY), None, "{what}: no reads after it");
            l.set_creds(Creds::Missing(Some("Claude")), crate::creds::Again::Yes, t1);
            assert_eq!(l.body(t1 + rest).label, None, "{what}: a read can't say otherwise");
            l.attaching(2);
            l.on_event(attached(2, "attached_create.json"), t1);
            assert_eq!(l.creds_due(t1), None, "{what}: nor a new attach");
            assert_eq!(l.body(t1 + rest).label, None);
            turn(&mut l, 2, no_login.clone(), t1);
            assert_eq!(l.body(t1 + TROUBLE_FOR).label.as_deref(), Some("NO BRAIN YET"), "{what}: the daemon's own word still does");
        }
        // An empty token count (a request that never ran) proves nothing; nor does a subagent's on
        // another model (Synaps names the model only there). Hers, named without its provider, does.
        let usage = |model: Option<&str>, tokens: u64| serde_json::json!({"kind": "session", "session": "usage", "input_tokens": tokens, "output_tokens": tokens, "model": model});
        let mut l = live_link(t0);
        failed_turn(&mut l, NO_LOGIN, t0);
        turn(&mut l, 1, usage(None, 0), t1);
        assert_eq!(l.body(t1 + rest).label.as_deref(), Some("NO BRAIN YET"), "an empty token count");
        turn(&mut l, 1, usage(Some("kimi-k2"), 50), t1);
        assert_eq!(l.body(t1 + rest).label.as_deref(), Some("NO BRAIN YET"), "a subagent's, on another model");
        turn(&mut l, 1, usage(Some("claude-haiku-4-5"), 50), t1);
        assert_eq!(l.body(t1 + rest).label, None, "hers (anthropic/claude-haiku-4-5)");
        // Another daemon process (a restart: he dropped the key it had) is another brain: the
        // attach reads again. The same daemon reconnecting doesn't.
        let daemon = |pid: u32| Event::Status(Status::Connected { daemon_version: "0.12.0".into(), profile: None, pid, features: vec![] });
        let mut l = live_link(t0);
        turn(&mut l, 1, serde_json::json!({"kind": "llm", "llm": "text", "text": "Hey"}), t0);
        l.on_event(daemon(1), t1);
        l.attaching(2);
        l.on_event(attached(2, "attached_create.json"), t1);
        assert_eq!(l.creds_due(t1), None, "the same daemon: its answer stands");
        l.on_event(daemon(2), t1);
        l.attaching(3);
        l.on_event(attached(3, "attached_create.json"), t1);
        assert!(l.creds_due(t1).is_some(), "a new daemon: read again");
        assert!(read(&mut l, &d, &no_env, true, t1));
        assert_eq!(l.body(t1 + rest).label.as_deref(), Some("NO BRAIN YET"), "and nothing on file");
        // A model change reads afresh: the answer was another model's.
        let mut l = live_link(t0);
        turn(&mut l, 1, serde_json::json!({"kind": "llm", "llm": "text", "text": "Hey"}), t0);
        let mut p = persona();
        p.model = Some("claude-sonnet-4-6".into());
        l.on_event(applied(1, "model", &p), t1);
        assert!(read(&mut l, &d, &no_env, true, t1), "a model change reads again");
        assert_eq!(l.body(t1 + rest).label.as_deref(), Some("NO BRAIN YET"));
        std::fs::remove_dir_all(&d).ok();
    }

    /// A daemon through `--socket` with no login on this machine: nothing here is read, so nothing
    /// here says NO BRAIN YET. Only its own error does, and its first answer (or token count)
    /// takes it back. A daemon on this machine with a remote broker and no `auth.json` at all
    /// claims nothing either, and stops reading.
    #[test]
    fn a_remote_brain_with_no_local_file_is_never_no_brain() {
        let d = creds_dir("remote");
        let t0 = Instant::now();
        let mut l = Link::new(Options { local: false, ..opts() }, t0);
        l.on_event(connected(), t0);
        l.attaching(1);
        l.on_event(attached(1, "attached_create.json"), t0);
        for t in [t0, t0 + CREDS_EVERY * 100] {
            assert!(!read(&mut l, &d, &no_env, true, t), "never read here");
            assert_eq!(l.body(t).label, None);
        }
        failed_turn(&mut l, NO_LOGIN, t0);
        let t1 = t0 + TROUBLE_FOR;
        assert_eq!(l.body(t1).label.as_deref(), Some("NO BRAIN YET"), "its own word");
        assert!(!read(&mut l, &d, &no_env, true, t1));
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t1);
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "session", "session": "usage", "input_tokens": 6, "output_tokens": 15, "model": null}})), t1);
        l.on_event(wire(1, serde_json::json!({"ev": "idle"})), t1);
        assert_eq!(l.body(t1 + Duration::from_secs(30)).label, None, "it answered");
        // It restarts: nothing to read here, nothing claimed.
        l.on_event(Event::Status(Status::Connected { daemon_version: "0.12.0".into(), profile: None, pid: 2, features: vec![] }), t1);
        l.attaching(2);
        l.on_event(attached(2, "attached_create.json"), t1);
        assert!(!read(&mut l, &d, &no_env, true, t1 + CREDS_EVERY));
        assert_eq!(l.body(t1 + Duration::from_secs(30)).label, None, "a restart claims nothing");
        // On this machine, a broker and no auth.json: Unknown, no tag, no more reads.
        std::fs::write(d.join("config"), BROKER).unwrap();
        let mut l = live_link(t0);
        assert!(read(&mut l, &d, &no_env, true, t0));
        assert_eq!(l.body(t0).label, None);
        assert_eq!(l.creds_due(t0 + CREDS_EVERY * 10), None);
        std::fs::remove_dir_all(&d).ok();
    }

    /// Local-model troubles are calm and named, with the model she runs on.
    #[test]
    fn a_missing_local_model_is_named() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let mut p = persona();
        p.model = Some("local/qwen3:8b".into());
        l.on_event(applied(1, "model", &p), t0);
        assert_eq!(l.creds_due(t0).map(|q| q.model).as_deref(), Some("local/qwen3:8b"), "a model change reads again");
        failed_turn(&mut l, "openai request failed: broker transport error: provider request failed: 404 Not Found [not_found_error]", t0);
        let b = l.body(t0);
        assert_eq!((b.mood, b.label.as_deref()), (Mood::Idle, Some("MODEL MISSING")));
        assert_eq!(b.reply, "qwen3:8b isn't on my local server. Pull it, then ask me again:\n`ollama pull qwen3:8b`");
        failed_turn(&mut l, "openai request failed: broker transport error: can't reach local endpoint at http://localhost:11434/v1/chat/completions — is Ollama/LM Studio running?", t0);
        assert_eq!(l.body(t0).label.as_deref(), Some("OLLAMA IS OFF"));
    }

    /// A 404 on a library model with no tag (`local/mistral`, `local/llama3.2`) is pulled; only the
    /// docs' own Modelfile model (`capri-…`) is created.
    #[test]
    fn a_missing_untagged_model_is_pulled_unless_its_the_docs_own() {
        let e404 = "openai request failed: broker transport error: provider request failed: 404 Not Found [not_found_error]";
        let chip = |model: &str| Trouble::of(e404, Some(model)).map(|t| t.says()).and_then(|s| s.rsplit_once("\n`").map(|(_, c)| c.trim_end_matches('`').to_string()));
        assert_eq!(chip("local/mistral").as_deref(), Some("ollama pull mistral"));
        assert_eq!(chip("local/llama3.2").as_deref(), Some("ollama pull llama3.2"));
        assert_eq!(chip("local/qwen3:8b").as_deref(), Some("ollama pull qwen3:8b"));
        assert_eq!(chip("local/capri-qwen3").as_deref(), Some("ollama create capri-qwen3 -f Modelfile"));
    }

    /// A turn that fails at once never shows as started: no THINKING, no busy bubble (ABORT)
    /// promising a reply. One that runs on shows as started after `QUIET_START`, or at its
    /// first word.
    #[test]
    fn a_turn_shows_as_started_only_once_it_runs() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let before = l.body(t0);
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "hi"})), t0);
        let quiet = l.body(t0 + Duration::from_millis(100));
        assert_eq!((quiet.busy, quiet.turn, quiet.mood, quiet.echo.as_deref()), (false, before.turn, before.mood, None), "not started yet");
        assert_eq!(l.deadline(t0 + Duration::from_millis(100)), Some(t0 + QUIET_START), "woken when it shows");
        let on = l.body(t0 + QUIET_START);
        assert_eq!((on.busy, on.turn, on.mood, on.echo.as_deref()), (true, before.turn + 1, Mood::Thinking, Some("hi")));
        // Fails within the quiet start: straight to her line, never busy.
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "session", "session": "error", "message": "No Anthropic credentials. Run `synaps login`."}})), t0);
        l.on_event(wire(1, serde_json::json!({"ev": "idle"})), t0);
        let failed = l.body(t0 + Duration::from_millis(50));
        assert_eq!((failed.busy, failed.turn, failed.label.as_deref()), (false, before.turn + 1, Some("NO BRAIN YET")));
        assert!(failed.reply.starts_with("No brain yet"));
        // A first word shows the turn at once.
        let t1 = t0 + Duration::from_secs(20);
        l.on_event(wire(1, serde_json::json!({"ev": "turn_started", "turn_baseline": 1, "trigger": "user", "user_text": "again"})), t1);
        assert_eq!(l.body(t1).reply, failed.reply, "the last reply holds meanwhile");
        l.on_event(wire(1, serde_json::json!({"ev": "stream", "event": {"kind": "llm", "llm": "text", "text": "Hey"}})), t1);
        let b = l.body(t1);
        assert_eq!((b.busy, b.turn, b.reply.as_str()), (true, before.turn + 2, "Hey"));
    }

    /// Touchpoint 8: the daemon's version and her session's model as the Settings page names it
    /// (her menu's last row and ABOUT say the same).
    #[test]
    fn her_mark_names_the_daemon_and_model() {
        let t0 = Instant::now();
        assert_eq!(Link::new(opts(), t0).mark(), None);
        let mut l = live_link(t0);
        assert_eq!(l.mark().as_deref(), Some("SYNAPS 0.11.1 · HAIKU 4.5"));
        let mut p = persona();
        p.model = Some("local/llama3.2".into());
        l.on_event(applied(1, "model", &p), t0);
        assert_eq!(l.mark().as_deref(), Some("SYNAPS 0.11.1 · LLAMA3.2"));
    }

    #[test]
    fn boot_grace_then_away() {
        let t0 = Instant::now();
        let l = Link::new(opts(), t0);
        assert_eq!(l.body(t0).mood, Mood::Idle);
        assert_eq!(l.body(t0 + BOOT_GRACE).mood, Mood::Away);
    }

    #[test]
    fn markdown_lite() {
        assert_eq!(clean("# Title\n\n**bold** and `code`\n- one\n* two\n[link](http://x) end"), "Title\nbold and `code`\n• one\n• two\nlink end");
        assert_eq!(clean("a\n```rust\nfn x() {}\n```\nb"), "a\n‹code›\nb");
        assert_eq!(clean("a\n```rust\nfn x"), "a\n‹code›");
        assert_eq!(clean("#hashtag"), "#hashtag");
        assert_eq!(clean("I'm *not* signing that, 2*3 and a * b, `*.rs`"), "I'm not signing that, 2*3 and a * b, `*.rs`", "her stressed word shows plain");
        assert_eq!(tool_label("mcp__github__create_pull_request"), "create_pull_request".chars().take(17).collect::<String>() + "…");
        assert_eq!(tool_label("bash"), "bash");
        assert_eq!(one_line("  a \n b  ", 10), "a b");
    }

    /// Her own session by name; a saved one (not in the live list on 0.11.1, but its name
    /// stays taken) is resumed by the id from last time instead of a create that fails.
    #[test]
    fn her_own_session_is_found_by_name_or_resumed_by_id() {
        let t0 = Instant::now();
        let mut l = Link::new(opts(), t0);
        l.on_event(connected(), t0);
        let list = vec![meta("20261113-090000-aaaa", Some("work")), meta("20261113-163211-d09d", Some("capri"))];
        match l.on_event(Event::Sessions(list), t0).as_slice() {
            [Act::Attach(id, AttachMode::Observe)] => assert_eq!(id, "20261113-163211-d09d"),
            other => panic!("{other:?}"),
        }
        // Remembered and not live: resume that id first, never a create that collides.
        let mut l = Link::new(Options { remembered: Some("20261113-163211-d09d".into()), ..opts() }, t0);
        l.on_event(connected(), t0);
        match l.on_event(Event::Sessions(vec![meta("x", Some("work"))]), t0).as_slice() {
            [Act::Resume(id, o)] => {
                assert_eq!(id, "20261113-163211-d09d");
                assert_eq!(o.mode, AttachMode::Observe);
            }
            other => panic!("{other:?}"),
        }
        // That resume fails (the journal is gone): create by name.
        l.attaching(1);
        l.on_event(Event::Detached { attach_seq: 1, session_id: None, reason: "no such session".into() }, t0);
        match l.on_event(Event::Sessions(vec![]), t0 + RETRY_SOON).as_slice() {
            [Act::Create(o)] => assert_eq!(o.name.as_deref(), Some("capri")),
            other => panic!("{other:?}"),
        }
        // Attached: the id is remembered.
        l.attaching(2);
        let acts = l.on_event(attached(2, "attached_create.json"), t0);
        assert!(acts.iter().any(|a| matches!(a, Act::Remember(id) if id == "20261113-073427-5fde")), "{acts:?}");
    }

    #[test]
    fn the_subconscious_session_is_never_attached() {
        let t0 = Instant::now();
        let fatal = |acts: &[Act]| acts.iter().any(|a| matches!(a, Act::Fatal(m) if m.contains("subconscious")));
        // By name.
        let mut l = Link::new(Options { target: Target::Session("Subconscious".into()), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![meta("20261101-000000-5c5c", Some("subconscious"))]), t0);
        assert!(fatal(&acts) && !acts.iter().any(|a| matches!(a, Act::Attach(..))), "{acts:?}");
        // By id.
        let mut l = Link::new(Options { target: Target::Session("20261101-000000-5c5c".into()), ..opts() }, t0);
        l.on_event(connected(), t0);
        let acts = l.on_event(Event::Sessions(vec![meta("20261101-000000-5c5c", Some("subconscious"))]), t0);
        assert!(fatal(&acts) && !acts.iter().any(|a| matches!(a, Act::Attach(..))), "{acts:?}");
        // Landed in it anyway (a resume by an id): detach at once.
        let mut l = Link::new(opts(), t0);
        l.on_event(connected(), t0);
        l.attaching(1);
        let Event::Attached { mut attached, .. } = attached(1, "attached_create.json") else { panic!() };
        attached.meta.name = Some("subconscious".into());
        let acts = l.on_event(Event::Attached { attach_seq: 1, attached, mode: AttachMode::Observe, reconnect: false }, t0);
        assert!(matches!(acts.first(), Some(Act::Detach)) && fatal(&acts), "{acts:?}");
        assert!(!l.live());
        assert!(guarded(" SUBCONSCIOUS ") && !guarded("subconscious-2"));
    }

    /// Observe → Mirror (to submit) → the turn → Observe again once it is done; every switch
    /// is a full-replace `Attached` under a new attach_seq, and the old seq's events drop.
    #[test]
    fn observe_mirror_observe_cycle() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert_eq!(l.mode(), AttachMode::Observe);
        let turn_before = l.body(t0).turn;
        // Enter in Observe: no submit yet, a switch to Mirror.
        let acts = l.submit("hi capri".into(), t0).unwrap();
        match acts.as_slice() {
            [Act::Attach(id, AttachMode::Mirror)] => assert_eq!(id, "20261113-073427-5fde"),
            other => panic!("{other:?}"),
        }
        l.attaching(2);
        assert!(l.live(), "the body stays while the mode switches");
        assert_eq!(l.body(t0).mood, Mood::Idle);
        // A late event of the Observe attachment: dropped.
        assert!(l.on_event(turn(1)[1].clone(), t0).is_empty());
        assert!(!l.body(t0).busy);
        // Mirror attached: the queued text goes out, and the same reply is not a new bubble.
        let acts = l.on_event(attached_as(2, "attached_create.json", AttachMode::Mirror), t0);
        assert!(matches!(acts.as_slice(), [Act::Submit(t)] if t == "hi capri"), "{acts:?}");
        assert_eq!(l.body(t0).turn, turn_before);
        assert_eq!(l.take_once(), vec![]);
        // In Mirror, more input goes straight out.
        assert!(matches!(l.abort(t0).as_slice(), [Act::Abort]));
        // The turn runs on seq 2; Mirror stays while it does.
        let evs = turn(2);
        for ev in &evs[..60] {
            l.on_event(ev.clone(), t0);
        }
        assert!(l.body(t0).busy);
        assert!(l.tick(t0 + Duration::from_secs(60)).is_empty());
        for ev in &evs[60..] {
            l.on_event(ev.clone(), t0);
        }
        assert_eq!(l.body(t0).mood, Mood::Done);
        // Done, then idle a moment: back to Observe.
        assert!(l.tick(t0 + MIRROR_LINGER - Duration::from_millis(1)).is_empty());
        assert_eq!(l.deadline(t0), Some(t0 + MIRROR_LINGER));
        let t1 = t0 + MIRROR_LINGER;
        match l.tick(t1).as_slice() {
            [Act::Attach(_, AttachMode::Observe)] => {}
            other => panic!("{other:?}"),
        }
        l.attaching(3);
        assert!(l.live());
        let reply = l.body(t1).reply;
        let turn_n = l.body(t1).turn;
        let Event::Attached { attached: a, .. } = attached(3, "attached_history.json") else { panic!() };
        l.on_event(Event::Attached { attach_seq: 3, attached: a, mode: AttachMode::Observe, reconnect: false }, t1);
        assert_eq!(l.mode(), AttachMode::Observe);
        assert_eq!(l.body(t1).reply, reply, "the same last reply after the switch");
        assert_eq!(l.body(t1).turn, turn_n);
        assert!(l.tick(t1 + Duration::from_secs(60)).is_empty(), "no more switching");
    }

    /// The field opening switches early; a failed switch gives the queued text back.
    #[test]
    fn typing_switches_early_and_a_failed_switch_restores_input() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert!(matches!(l.set_typing(true, t0).as_slice(), [Act::Attach(_, AttachMode::Mirror)]));
        l.attaching(2);
        assert!(l.submit("queued".into(), t0).unwrap().is_empty(), "already switching");
        l.on_event(Event::Detached { attach_seq: 2, session_id: None, reason: "refused".into() }, t0);
        assert_eq!(l.take_once(), vec![Once::Restore("queued".into())]);
        assert_eq!(l.mode(), AttachMode::Observe);
        assert!(!l.live());
    }

    #[test]
    fn origin_attributes_a_relay_turn_and_leaves_his_own_alone() {
        let o = origin("[comms] #12 ask from spike: CI is red on main. Rebase?");
        assert_eq!(o, Some(("SPIKE".into(), "ask".into(), "CI is red on main. Rebase?".into())));
        assert_eq!(origin("what's spike doing?"), None);
        assert_eq!(origin("[comms] from nobody"), None);
    }
}
