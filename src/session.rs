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
/// `[session]` plus the pack's persona). Applied only to HER session, never to one she rides
/// (`--session`) or to `subconscious`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Persona {
    pub system: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
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
}

/// `name` (or a name given for an id) is the guarded session.
pub fn guarded(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(GUARDED)
}

/// A prompt's answer: never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
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
    pub prompt: Option<PromptView>,
    /// Every pending prompt, oldest first (`prompt` is the first), whole text.
    pub prompts: Vec<PromptView>,
    /// Attached and connected: input can go out.
    pub live: bool,
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
    turn: u64,
    echo: Option<String>,
    flash: Option<(Mood, Instant)>,
    last_active: Instant,
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
            turn: 0,
            echo: None,
            flash: None,
            last_active: now,
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
            persona_sent: now,
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
        let diffs = self.persona_diffs(view);
        if diffs.is_empty() {
            self.persona_done = Some(id);
            return Vec::new();
        }
        let ours = if self.mode == AttachMode::Mirror { self.owner == Some(self.me) } else { self.owner.is_none() };
        if !ours {
            elog!("synaps-avatar: persona: another client owns input on her session; not set");
            self.persona_done = Some(id);
            return Vec::new();
        }
        if self.mode != AttachMode::Mirror {
            return self.upgrade();
        }
        self.persona_done = Some(id);
        self.persona_wait += diffs.len();
        self.persona_sent = now;
        self.touched = now;
        elog!("synaps-avatar: persona: setting {} on her session", diffs.iter().map(Setting::name).collect::<Vec<_>>().join(", "));
        diffs.into_iter().map(Act::Set).collect()
    }

    /// The `--model` she was started with.
    pub fn cli_model(&self) -> Option<String> {
        self.opts.model.clone()
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
        self.opts.persona = p;
        self.persona_done = None;
        self.persona_step(now)
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
                self.view = Some(attached.view.clone());
                self.me = attached.client;
                self.owner = attached.input_owner;
                // A mode switch on the session we follow: same body, no new bubble.
                let switch = self.switching && !reconnect && self.follow.as_deref() == Some(attached.meta.id.as_str());
                let again = reconnect || self.lost;
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
            }
            Event::InputOwner { client, owner, .. } => {
                self.me = client;
                self.owner = owner;
            }
            Event::SettingApplied { setting, view, .. } => {
                elog!("synaps-avatar: persona: {setting} set");
                self.view = Some(view);
                self.persona_wait = self.persona_wait.saturating_sub(1);
            }
            Event::Error { error: Error::SettingRefused { setting, reason, .. }, .. } => {
                // Logged, never shown: she just keeps the session's own settings.
                elog!("synaps-avatar: persona: {setting} not set: {reason}");
                self.persona_wait = self.persona_wait.saturating_sub(1);
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
            Event::Session { event, .. } => {
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
            SessionEvent::TurnStarted { user_text, .. } => {
                self.busy = true;
                self.phase = Phase::Thinking;
                self.raw.clear();
                self.response_at = 0;
                self.notice = None;
                self.flash = None;
                self.turn += 1;
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
                    }
                    LlmEvent::ToolUseStart { tool_name, .. } | LlmEvent::ToolUse { tool_name, .. } => {
                        self.phase = Phase::Working(tool_name);
                    }
                    LlmEvent::ToolResult { .. } => self.phase = Phase::Thinking,
                    _ => {}
                },
                StreamEvent::Session(SessionStreamEvent::Error { message }) => {
                    self.notice = Some(format!("‹error: {}›", one_line(&message, 90)));
                    self.flash = Some((Mood::Annoyed, now + ANNOYED_FOR));
                }
                _ => {}
            },
            SessionEvent::Idle => {
                self.busy = false;
                self.echo = None;
                self.last_active = now;
                self.touched = now;
                if !self.flash.is_some_and(|f| f.0 == Mood::Annoyed) {
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
                elog!("synaps-avatar: compaction failed: {message}");
            }
            SessionEvent::Cleared { .. } => {
                self.raw.clear();
                self.response_at = 0;
                self.notice = None;
                self.turn += 1;
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
        idle.then_some(if self.persona_wait > 0 { self.persona_sent + PERSONA_WAIT } else { self.touched + MIRROR_LINGER })
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
        if let Some((_, until)) = self.flash {
            t.push(until);
        }
        if self.live() && !self.busy {
            t.push(self.last_active + self.opts.sleepy_after);
        }
        t.into_iter().filter(|&d| d > now).min()
    }

    pub fn body(&self, now: Instant) -> Body {
        let mut label = None;
        let mood = if !self.live() {
            if !self.ever_live && now < self.started + BOOT_GRACE { Mood::Idle } else { Mood::Away }
        } else if !self.prompts.is_empty() {
            Mood::NeedsYou
        } else if self.compacting {
            label = Some("COMPACTING".into());
            Mood::Sleepy
        } else if self.busy {
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
            m
        } else if now >= self.last_active + self.opts.sleepy_after {
            Mood::Sleepy
        } else {
            Mood::Idle
        };
        let mut reply = clean(&self.raw);
        if let Some(n) = &self.notice {
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(n);
        }
        // Whole: a security decision is made on the full text (the bubble cuts its own copy).
        let prompts: Vec<PromptView> = self
            .prompts
            .iter()
            .map(|p| PromptView { id: p.id, title: p.title.trim().to_string(), text: p.prompt.trim_end().to_string(), secret: p.kind.as_str() != Some("confirm") })
            .collect();
        let prompt = prompts.first().cloned();
        Body {
            mood,
            label,
            reply,
            raw: self.raw.trim().to_string(),
            turn: self.turn,
            busy: self.busy,
            echo: self.echo.clone().filter(|_| self.busy),
            prompt,
            prompts,
            live: self.live(),
        }
    }
}

/// A tool name for the tag: short and safe (never arguments).
fn tool_label(name: &str) -> String {
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

    fn persona() -> Persona {
        Persona { system: Some("You are Capri.".into()), model: Some("anthropic/claude-sonnet-4-6".into()), thinking: Some("medium".into()) }
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

    /// The fixture's real turn (thinking, text, a bash tool, more text, idle) in moods.
    #[test]
    fn a_real_turn_drives_the_moods() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        assert_eq!(l.body(t0).mood, Mood::Idle);
        let mut seen: Vec<(Mood, Option<String>)> = Vec::new();
        for ev in turn(1) {
            l.on_event(ev, t0);
            let b = l.body(t0);
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
    fn the_echo_shows_while_the_turn_runs() {
        let t0 = Instant::now();
        let mut l = live_link(t0);
        let evs = turn(1);
        l.on_event(evs[0].clone(), t0); // client_joined
        l.on_event(evs[1].clone(), t0); // turn_started
        let b = l.body(t0);
        assert!(b.busy);
        assert!(b.echo.as_deref().unwrap().starts_with("Think briefly, then write"));
        assert_eq!(b.reply, "");
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
