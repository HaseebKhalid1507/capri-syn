//! THE LINE: the conversation in its own window (ADVICE-ux §3.3). The thread's tail above a
//! real text field: Enter sends and the Line stays, Shift+Enter is a newline, ↑ recalls,
//! Esc closes (Esc Esc aborts a running turn), losing the keyboard parks it. A pending
//! prompt shows whole, answered with A / D (armed after 400 ms) or a masked field for a
//! secret. A plain opaque rectangle, like the reader: no shape, nothing to slab.
//!
//! The avatar owns the session; the Line gets snapshots ([`Snap`]) and asks for things
//! ([`Ask`]).

use crate::reader::{Block, Mark, Pal, inline, parse};
use crate::session::PromptView;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::{Duration, Instant};

/// Prompt keys do nothing this long after a prompt shows (as the buttons).
pub const ARM: Duration = Duration::from_millis(400);
/// A risky prompt (a tool activation, shell/exec/write-class tools) allows only on Shift+A
/// or A held this long; a bare tap of A does nothing there.
pub const HOLD: Duration = Duration::from_millis(450);
const ESC_AGAIN: Duration = Duration::from_millis(1000);
const PAD: f32 = 14.0;
const FONT: f32 = 16.0;
const LINE_H: f32 = 23.0;
const MONO: &str = "DejaVu Sans Mono";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    Her,
    /// Another session woke her: its callsign and the message kind.
    Relay(String, String),
    Tool,
    Sys,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub who: Who,
    /// Her rows: markdown as written. Tool rows: `name` then a one-line input preview.
    pub text: String,
    pub detail: String,
    pub streaming: bool,
}

/// What the Line shows, from the avatar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snap {
    pub rows: Vec<Row>,
    pub status: String,
    pub busy: bool,
    pub live: bool,
    pub prompts: Vec<PromptView>,
}

/// The kit's components (the Line's text field), its dark theme and caret policy: set up
/// when the first Line opens (it costs ~15 MB of RSS a Line never opened shouldn't pay).
pub fn kit_init(cx: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    gpui_kit::init(cx);
    gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    // The caret blinks while he types, then holds still: an open, idle Line schedules
    // nothing (a 500 ms blink was two repaints a second, forever). gpui-base's Theme is the
    // one the field reads.
    let blink = gpui_kit::base::input::CursorBlink::blinking(Duration::from_millis(530)).with_idle_timeout(Duration::from_secs(10));
    gpui_kit::base::Theme::global_mut(cx).cursor_blink = blink;
    gpui_kit::component::Theme::global_mut(cx).cursor_blink = blink;
}

/// What the Line asks the avatar for. `Send` and `Answer` may be refused (the reason).
pub enum Ask {
    Send(String),
    Abort,
    Answer(u64, Option<String>),
    /// The first key: Mirror follows keystrokes.
    Typing,
    /// Gone (Esc, the summon key): `restore` hands the keyboard back. Its draft and history.
    Closed { restore: bool, draft: String, sent: Vec<String> },
    /// Lost (true) or got (false) the keyboard.
    Parked(bool),
    /// Its laid-out height changed (logical px): the avatar keeps its bottom edge put.
    Height(f32),
    Copy(String),
    OpenUrl(String),
}

pub type OnAsk = Box<dyn Fn(Ask, &mut App) -> Result<(), String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    Prompt,
}

pub struct Line {
    snap: Snap,
    pal: Pal,
    font: SharedString,
    name: String,
    max_thread: f32,
    input: Entity<TextareaState>,
    secret: Option<(u64, Entity<InputState>)>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Scrolled up by him: new rows don't pull the view down.
    pinned_up: bool,
    sent: Vec<String>,
    recall: Option<usize>,
    esc_at: Option<Instant>,
    note: Option<String>,
    parked: bool,
    was_active: bool,
    mode: Mode,
    prompt_ix: usize,
    prompt_at: Instant,
    typed: bool,
    closing: bool,
    copied_at: Option<Instant>,
    height: f32,
    /// SA_TIMING: Enter, until her first word of the reply is drawn.
    sent_at: Option<(Instant, usize)>,
    /// Renders so far (`state`: an idle Line should not render).
    renders: std::cell::Cell<u64>,
    /// Code blocks and tool rows opened by a click: (row, block); tool rows use block 0.
    open: std::collections::HashSet<(usize, usize)>,
    /// A held A on a risky prompt: when it went down (the allow fires at HOLD).
    hold_at: Option<Instant>,
    /// A is down (from a press on a prompt): its key repeats never reach the field, even
    /// after the prompt it allowed has gone.
    a_down: bool,
    /// Which side of the Line she stands on (-1 left, 1 right, 0 not beside it).
    side: i8,
    on_ask: OnAsk,
    _subs: Vec<Subscription>,
}

/// The links in a reply, as written (`[text](url)` and bare `http(s)://`).
pub fn links(md: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = md;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        if let Some(end) = after.find(')') {
            let u = after[..end].trim();
            if u.starts_with("http://") || u.starts_with("https://") {
                out.push(u.to_string());
            }
            rest = &after[end..];
        } else {
            break;
        }
    }
    for w in md.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<' || c == '>') {
        if (w.starts_with("http://") || w.starts_with("https://")) && !w.contains("](") {
            let u = w.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'', '`', ']']).to_string();
            if !out.contains(&u) {
                out.push(u);
            }
        }
    }
    out
}

/// A prompt that can do harm when allowed by accident: any tool activation, or a tool that
/// runs, writes or deletes. Those need Shift+A or a held A; the rest keep a plain A.
pub fn risky(p: &PromptView) -> bool {
    if p.secret {
        return false;
    }
    const WORDS: [&str; 27] = [
        "activation", "activate", "bash", "sh", "zsh", "command", "commands", "terminal", "run", "runs", "write", "edit", "delete",
        "remove", "rm", "sudo", "install", "kill", "patch", "apply", "push", "mv", "chmod", "chown", "create", "mkdir", "dd",
    ];
    const STEMS: [&str; 4] = ["shell", "exec", "write", "delet"];
    let t = format!("{} {}", p.title, p.text).to_lowercase();
    t.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .any(|w| WORDS.contains(&w) || STEMS.iter().any(|s| w.starts_with(s)) || w.contains("shell_"))
}

/// One line: whitespace collapsed, at most `max` chars.
fn one(s: &str, max: usize) -> String {
    crate::session::one_line(s, max)
}

impl Line {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        snap: Snap,
        draft: String,
        sent: Vec<String>,
        name: String,
        font: SharedString,
        pal: Pal,
        max_thread: f32,
        on_ask: OnAsk,
        window: &mut Window,
        cx: &mut Context<Line>,
    ) -> Line {
        let mut title = name.to_lowercase();
        if let Some(c) = title.get(..1) {
            title = c.to_uppercase() + &title[1..];
        }
        let placeholder = format!("talk to {title}…");
        // The caret in her accent (1 px wide in the kit: the colour has to carry it).
        gpui_kit::component::Theme::global_mut(cx).colors.caret = rgb(pal.frame).into();
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 6).submit_on_enter(true).placeholder(placeholder).default_value(draft));
        let focus = cx.focus_handle();
        // Before the field's own bindings (↑ moves its caret, Ctrl+Y is its redo): the
        // Line's keys first, in this window only.
        let me = cx.entity().downgrade();
        let mine = window.window_handle().window_id();
        let keys = cx.intercept_keystrokes(move |ev, window, cx| {
            if window.window_handle().window_id() != mine {
                return;
            }
            let ks = ev.keystroke.clone();
            let handled = me.update(cx, |l, cx| l.key(&ks, window, cx)).unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        let subs = vec![
            keys,
            cx.subscribe_in(&input, window, |this: &mut Line, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                InputEvent::Change => {
                    if !this.typed {
                        this.typed = true;
                        (this.on_ask)(Ask::Typing, cx).ok();
                    }
                    this.note = None;
                    cx.notify();
                }
                _ => {}
            }),
            cx.observe_window_activation(window, |this: &mut Line, window, cx| {
                let active = window.is_window_active();
                if active {
                    this.was_active = true;
                    if this.parked && !this.closing {
                        this.parked = false;
                        (this.on_ask)(Ask::Parked(false), cx).ok();
                        this.refocus(window, cx);
                    }
                } else if this.was_active && !this.parked && !this.closing {
                    // Focus went elsewhere: park (never destroy); an empty idle Line closes.
                    let empty = this.input.read(cx).value().trim().is_empty();
                    if empty && !this.snap.busy && this.snap.prompts.is_empty() {
                        this.close(false, window, cx);
                    } else {
                        this.parked = true;
                        (this.on_ask)(Ask::Parked(true), cx).ok();
                    }
                }
                cx.notify();
            }),
        ];
        let mode = if snap.prompts.is_empty() { Mode::Text } else { Mode::Prompt };
        let mut line = Line {
            snap,
            pal,
            font,
            name,
            max_thread,
            input,
            secret: None,
            focus,
            scroll: ScrollHandle::new(),
            pinned_up: false,
            sent,
            recall: None,
            esc_at: None,
            note: None,
            parked: false,
            was_active: false,
            mode,
            prompt_ix: 0,
            prompt_at: Instant::now(),
            typed: false,
            closing: false,
            copied_at: None,
            height: 0.0,
            sent_at: None,
            renders: Default::default(),
            open: Default::default(),
            hold_at: None,
            a_down: false,
            side: 0,
            on_ask,
            _subs: subs,
        };
        line.sync_secret(window, cx);
        line.refocus(window, cx);
        line.scroll.scroll_to_bottom();
        line
    }

    fn prompt(&self) -> Option<&PromptView> {
        self.snap.prompts.get(self.prompt_ix.min(self.snap.prompts.len().saturating_sub(1)))
    }

    /// The keyboard goes where this state types: the field, the masked field, or the Line
    /// itself (a confirm: no field, so no typed word can answer it).
    fn refocus(&mut self, window: &mut Window, cx: &mut Context<Line>) {
        match (self.mode, &self.secret) {
            (Mode::Prompt, Some((_, s))) => s.update(cx, |s, cx| s.focus(window, cx)),
            (Mode::Prompt, None) => window.focus(&self.focus, cx),
            (Mode::Text, _) => self.input.update(cx, |s, cx| s.focus(window, cx)),
        }
    }

    /// A secret prompt shown: its masked field (one per prompt; never logged, never kept).
    fn sync_secret(&mut self, window: &mut Window, cx: &mut Context<Line>) {
        let want = (self.mode == Mode::Prompt).then(|| self.prompt().filter(|p| p.secret).map(|p| p.id)).flatten();
        if self.secret.as_ref().map(|s| s.0) == want {
            return;
        }
        self.secret = want.map(|id| {
            let s = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("answer (hidden, never logged)"));
            let sub = cx.subscribe_in(&s, window, move |this: &mut Line, st: &Entity<InputState>, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let v = st.read(cx).value().to_string();
                    if !v.is_empty() {
                        this.answer(id, Some(v), window, cx);
                    }
                }
            });
            self._subs.push(sub);
            (id, s)
        });
    }

    /// New state from the avatar.
    pub fn set_snap(&mut self, snap: Snap, window: &mut Window, cx: &mut Context<Line>) {
        if snap == self.snap {
            return;
        }
        let grew = snap.rows.len() != self.snap.rows.len() || snap.rows.last() != self.snap.rows.last();
        let had_prompts = !self.snap.prompts.is_empty();
        let ids: Vec<u64> = self.snap.prompts.iter().map(|p| p.id).collect();
        self.snap = snap;
        if self.snap.prompts.is_empty() {
            if self.mode == Mode::Prompt {
                self.mode = Mode::Text;
                self.secret = None;
                if !self.parked {
                    self.refocus(window, cx);
                }
            }
        } else {
            self.prompt_ix = self.prompt_ix.min(self.snap.prompts.len() - 1);
            let new = self.snap.prompts.iter().any(|p| !ids.contains(&p.id));
            if new {
                self.prompt_at = Instant::now();
            }
            // A prompt arriving while he types is announced, never takes the field.
            if !had_prompts && self.mode == Mode::Text && self.input.read(cx).value().trim().is_empty() && !self.parked {
                self.mode = Mode::Prompt;
                self.refocus(window, cx);
            }
            self.sync_secret(window, cx);
        }
        if grew && !self.pinned_up {
            self.scroll.scroll_to_bottom();
        }
        cx.notify();
    }

    pub fn state_json(&self, cx: &App) -> serde_json::Value {
        serde_json::json!({
            "mode": if self.mode == Mode::Prompt { "prompt" } else { "text" },
            "parked": self.parked,
            "rows": self.snap.rows.len(),
            "draft_chars": self.input.read(cx).value().chars().count(),
            "renders": self.renders.get(),
            "side": self.side,
            "holding": self.hold_at.is_some(),
            "risky": self.prompt().is_some_and(risky),
            "note": self.note,
            "prompts": self.snap.prompts.len(),
            "prompt_ix": self.prompt_ix,
            "height": self.height,
            "scroll": [f32::from(self.scroll.offset().y), f32::from(self.scroll.max_offset().y)],
        })
    }

    /// Parked from outside (a click elsewhere): it shows so until the summon key or a click.
    pub fn park(&mut self, cx: &mut Context<Line>) {
        self.parked = true;
        cx.notify();
    }

    /// The summon key on an open Line: back to it (parked), or close it.
    pub fn summon(&mut self, window: &mut Window, cx: &mut Context<Line>) -> &'static str {
        if self.parked || !window.is_window_active() {
            self.parked = false;
            (self.on_ask)(Ask::Parked(false), cx).ok();
            self.refocus(window, cx);
            cx.notify();
            "resumed"
        } else {
            self.close(true, window, cx);
            "closed"
        }
    }

    /// The field's text, the caret at its end (set_value leaves it at the start).
    fn set_field(&mut self, t: String, window: &mut Window, cx: &mut Context<Line>) {
        let line = t.split('\n').count().saturating_sub(1) as u32;
        let character = t.rsplit('\n').next().map_or(0, |l| l.encode_utf16().count()) as u32;
        let parked = self.parked;
        self.input.update(cx, |s, cx| {
            s.set_value(t, window, cx);
            if !parked {
                s.set_cursor_position(gpui_kit::component::input::Position { line, character }, window, cx);
            }
        });
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Line>) {
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        match (self.on_ask)(Ask::Send(text.clone()), cx) {
            Ok(()) => {
                if std::env::var_os("SA_TIMING").is_some() {
                    self.sent_at = Some((Instant::now(), self.snap.rows.len()));
                }
                self.input.update(cx, |s, cx| s.set_value("", window, cx));
                if self.sent.last() != Some(&text) {
                    self.sent.push(text);
                }
                self.recall = None;
                self.note = None;
                self.pinned_up = false;
                self.scroll.scroll_to_bottom();
            }
            // Refused: the text stays in the field, and he is told why.
            Err(why) => self.note = Some(format!("NOT SENT · {} · text kept", why.to_uppercase())),
        }
        cx.notify();
    }

    /// A refused message came back after it went out: into the field, with the reason.
    pub fn restore(&mut self, text: String, why: &str, window: &mut Window, cx: &mut Context<Line>) {
        if self.input.read(cx).value().trim().is_empty() {
            self.set_field(text, window, cx);
        }
        self.note = Some(format!("NOT SENT · {} · text kept", why.to_uppercase()));
        cx.notify();
    }

    fn answer(&mut self, id: u64, value: Option<String>, window: &mut Window, cx: &mut Context<Line>) {
        if Instant::now() < self.prompt_at + ARM {
            return;
        }
        if let Err(e) = (self.on_ask)(Ask::Answer(id, value), cx) {
            self.note = Some(format!("NOT ANSWERED · {}", e.to_uppercase()));
        }
        self.secret = None;
        let _ = window;
        cx.notify();
    }

    pub fn close(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Line>) {
        if self.closing {
            return;
        }
        self.closing = true;
        let draft = self.input.read(cx).value().to_string();
        let sent = std::mem::take(&mut self.sent);
        (self.on_ask)(Ask::Closed { restore, draft, sent }, cx).ok();
        window.remove_window();
    }

    fn last_reply(&self) -> Option<&str> {
        self.snap.rows.iter().rev().find(|r| r.who == Who::Her && !r.text.trim().is_empty()).map(|r| r.text.as_str())
    }

    fn copy_last(&mut self, cx: &mut Context<Line>) {
        if let Some(t) = self.last_reply().map(str::to_string) {
            (self.on_ask)(Ask::Copy(t), cx).ok();
            self.copied_at = Some(Instant::now());
            self.note = Some("COPIED HER LAST REPLY".into());
            cx.notify();
        }
    }

    fn open_last_link(&mut self, cx: &mut Context<Line>) {
        let last = self.snap.rows.iter().rev().filter(|r| r.who == Who::Her).find_map(|r| links(&r.text).pop());
        match last {
            Some(u) => {
                self.note = Some(format!("OPENED {}", one(&u, 60)));
                (self.on_ask)(Ask::OpenUrl(u), cx).ok();
            }
            None => self.note = Some("NO LINK IN HER REPLIES".into()),
        }
        cx.notify();
    }

    /// Keys before the field sees them: Esc, ↑/↓ recall, paging, copy, prompt answers.
    fn key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Line>) -> bool {
        let m = k.modifiers;
        let ctrl = m.control || m.platform;
        let plain = !ctrl && !m.alt && !m.shift;
        let now = Instant::now();
        if k.key != "escape" {
            self.esc_at = None;
        }
        if k.key == "a" && self.a_down {
            return true;
        }
        match (k.key.as_str(), self.mode) {
            ("escape", Mode::Text) if self.snap.busy => {
                if self.esc_at.is_some_and(|t| now < t + ESC_AGAIN) {
                    self.esc_at = None;
                    (self.on_ask)(Ask::Abort, cx).ok();
                    self.note = Some("ABORTED".into());
                } else {
                    // One Esc closes after a beat unless a second one (abort) follows.
                    self.esc_at = Some(now);
                    self.note = Some("ESC AGAIN TO ABORT".into());
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(ESC_AGAIN).await;
                        this.update_in(cx, |l, window, cx| {
                            if l.esc_at == Some(now) {
                                l.close(true, window, cx);
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
                cx.notify();
                true
            }
            ("escape", _) => {
                self.close(true, window, cx);
                true
            }
            ("up", Mode::Text) if plain => {
                let cur = self.input.read(cx).value().to_string();
                let at_recall = self.recall.and_then(|i| self.sent.get(i)).is_some_and(|s| *s == cur);
                if (cur.is_empty() || at_recall) && !self.sent.is_empty() {
                    let i = match self.recall {
                        Some(i) if at_recall => i.saturating_sub(1),
                        _ => self.sent.len() - 1,
                    };
                    self.recall = Some(i);
                    let t = self.sent[i].clone();
                    self.set_field(t, window, cx);
                    cx.notify();
                    return true;
                }
                false
            }
            ("down", Mode::Text) if plain => {
                let cur = self.input.read(cx).value().to_string();
                if let Some(i) = self.recall
                    && self.sent.get(i).is_some_and(|s| *s == cur)
                {
                    let next = i + 1;
                    let t = if next < self.sent.len() {
                        self.recall = Some(next);
                        self.sent[next].clone()
                    } else {
                        self.recall = None;
                        String::new()
                    };
                    self.set_field(t, window, cx);
                    cx.notify();
                    return true;
                }
                false
            }
            ("pageup" | "pagedown", _) => {
                let step = px(self.max_thread * 0.8);
                let o = self.scroll.offset();
                let max = self.scroll.max_offset();
                let y = if k.key == "pageup" { (o.y + step).min(px(0.0)) } else { (o.y - step).max(-max.y) };
                self.scroll.set_offset(point(o.x, y));
                self.pinned_up = y > -max.y + px(2.0);
                cx.notify();
                true
            }
            ("y", _) if ctrl => {
                self.copy_last(cx);
                true
            }
            ("o", _) if ctrl => {
                self.open_last_link(cx);
                true
            }
            ("d", Mode::Prompt) if plain && self.secret.is_none() => {
                self.hold_at = None;
                if let Some(id) = self.prompt().map(|p| p.id) {
                    self.answer(id, None, window, cx);
                }
                true
            }
            ("a", Mode::Prompt) if !ctrl && !m.alt && self.secret.is_none() => {
                self.a_down = true;
                let Some((id, risky)) = self.prompt().map(|p| (p.id, risky(p))) else { return true };
                if now < self.prompt_at + ARM {
                    return true;
                }
                if !risky || m.shift {
                    self.hold_at = None;
                    self.answer(id, Some("y".into()), window, cx);
                } else if self.hold_at.is_none() {
                    // Held: it allows at HOLD unless A comes up first (key repeats ignored).
                    self.hold_at = Some(now);
                    self.note = None;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(HOLD).await;
                        this.update_in(cx, |l, window, cx| {
                            if l.hold_at == Some(now) && l.prompt().is_some_and(|p| p.id == id) {
                                l.hold_at = None;
                                l.answer(id, Some("y".into()), window, cx);
                            }
                        })
                        .ok();
                    })
                    .detach();
                    cx.notify();
                }
                true
            }
            ("tab", Mode::Prompt) => {
                self.mode = Mode::Text;
                self.secret = None;
                self.refocus(window, cx);
                cx.notify();
                true
            }
            ("tab", Mode::Text) if !self.snap.prompts.is_empty() && self.input.read(cx).value().trim().is_empty() => {
                self.mode = Mode::Prompt;
                self.prompt_at = self.prompt_at.max(now - ARM);
                self.sync_secret(window, cx);
                self.refocus(window, cx);
                cx.notify();
                true
            }
            ("left" | "right", Mode::Prompt) if self.secret.is_none() && self.snap.prompts.len() > 1 => {
                let n = self.snap.prompts.len();
                self.prompt_ix = if k.key == "right" { (self.prompt_ix + 1) % n } else { (self.prompt_ix + n - 1) % n };
                self.prompt_at = now;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// A let go before HOLD on a risky prompt: nothing allowed, and he is told how.
    fn key_up(&mut self, k: &Keystroke, cx: &mut Context<Line>) {
        if k.key == "a" {
            self.a_down = false;
        }
        if k.key == "a"
            && let Some(t0) = self.hold_at.take()
        {
            if t0.elapsed() < HOLD {
                self.note = Some("NOT ALLOWED · HOLD A (OR ⇧A) TO ALLOW THIS ONE".into());
            }
            cx.notify();
        }
    }

    /// Her side of the Line: a thick edge and a pointer at her feet, so it reads as hers.
    fn tail(&self) -> Option<AnyElement> {
        if self.side == 0 {
            return None;
        }
        let (tone, left) = (if self.parked { self.pal.dim } else { self.pal.frame }, self.side < 0);
        let edge = div().absolute().top_0().bottom_0().w(px(5.0)).bg(rgb(tone));
        let edge = if left { edge.left_0() } else { edge.right_0() };
        let arrow = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (h, w) = (px(26.0), px(12.0));
                let y = b.bottom() - px(14.0) - h;
                let x = if left { b.left() + px(5.0) } else { b.right() - px(5.0) };
                let dx = if left { w } else { -w };
                let mut p = PathBuilder::fill();
                p.move_to(point(x, y));
                p.line_to(point(x + dx, y + h / 2.0));
                p.line_to(point(x, y + h));
                p.close();
                if let Ok(path) = p.build() {
                    window.paint_path(path, rgb(tone));
                }
            },
        )
        .absolute()
        .size_full();
        Some(div().absolute().top_0().left_0().size_full().child(edge).child(arrow).into_any_element())
    }

    /// Which side she stands on (from the avatar, after placing the Line).
    pub fn set_side(&mut self, side: i8, cx: &mut Context<Line>) {
        if self.side != side {
            self.side = side;
            cx.notify();
        }
    }

    /// Opened from her unread strip: the reply, from its first line.
    pub fn show_last_reply(&mut self, cx: &mut Context<Line>) {
        if let Some(ix) = self.snap.rows.iter().rposition(|r| r.who == Who::Her) {
            self.scroll.scroll_to_top_of_item(ix);
            cx.notify();
        }
    }

    fn plate(&self, who: &Who) -> (String, u32) {
        let p = self.pal;
        match who {
            Who::You => ("YOU".into(), p.frame),
            Who::Her => (self.name.to_uppercase(), p.name),
            Who::Relay(c, _) => (format!("⇄ {c}"), 0x9fd2ff),
            Who::Tool => ("⚙".into(), p.frame),
            Who::Sys => ("SYS".into(), p.dim),
            Who::Error => ("SYS".into(), p.slash),
        }
    }

    fn styled(&self, text: &str) -> StyledText {
        let pal = self.pal;
        let (plain, marks) = inline(text);
        let hl: Vec<(std::ops::Range<usize>, HighlightStyle)> = marks
            .into_iter()
            .map(|(r, m)| {
                let s = match m {
                    Mark::Bold => HighlightStyle { font_weight: Some(FontWeight::BOLD), color: Some(rgb(pal.text).into()), ..Default::default() },
                    Mark::Code => HighlightStyle { color: Some(rgb(pal.accent).into()), background_color: Some(rgb(0x18242e).into()), ..Default::default() },
                };
                (r, s)
            })
            .collect();
        StyledText::new(plain).with_highlights(hl)
    }

    fn her_body(&self, row: usize, md: &str, cx: &mut Context<Line>) -> Div {
        let pal = self.pal;
        let mut col = div().flex().flex_col().gap(px(6.0)).min_w(px(0.0)).flex_1();
        for (bi, b) in parse(md).into_iter().enumerate() {
            col = col.child(match b {
                Block::Heading(_, t) => div().font_weight(FontWeight::BOLD).text_color(rgb(pal.name)).child(self.styled(&t)).into_any_element(),
                Block::Para(t) => div().text_color(rgb(pal.text)).child(self.styled(&t)).into_any_element(),
                Block::Bullet(indent, marker, t) => div()
                    .pl(px(indent as f32 * 16.0))
                    .flex()
                    .gap(px(6.0))
                    .text_color(rgb(pal.text))
                    .child(div().flex_none().text_color(rgb(pal.frame)).child(marker))
                    .child(div().flex_1().min_w(px(0.0)).child(self.styled(&t)))
                    .into_any_element(),
                Block::Rule => div().h(px(1.0)).bg(rgba((pal.frame << 8) | 0x44)).into_any_element(),
                Block::Code { lang, text } if !self.open.contains(&(row, bi)) => {
                    // One line until clicked: the language, the first line, the size.
                    let n = text.lines().count();
                    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                    div()
                        .id(SharedString::from(format!("code-{row}-{bi}")))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(2.0))
                        .border_1()
                        .border_color(rgba((pal.frame << 8) | 0x44))
                        .bg(rgb(0x070b10))
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .child(div().flex_none().text_color(rgb(pal.frame)).child("▸"))
                        .child(div().flex_none().font_weight(FontWeight::BOLD).text_color(rgb(pal.dim)).child(if lang.is_empty() { "CODE".to_string() } else { lang.to_uppercase() }))
                        .child(div().flex_1().min_w(px(0.0)).overflow_hidden().whitespace_nowrap().font_family(MONO).text_color(rgb(0xd8e6ee)).child(one(&first, 70)))
                        .child(div().flex_none().text_color(rgb(pal.dim)).child(format!("{n} line{}", if n == 1 { "" } else { "s" })))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open.insert((row, bi));
                            cx.notify();
                        }))
                        .into_any_element()
                }
                Block::Code { lang, text } => div()
                    .id(SharedString::from(format!("code-{row}-{bi}")))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open.remove(&(row, bi));
                        cx.notify();
                    }))
                    .border_1()
                    .border_color(rgba((pal.frame << 8) | 0x44))
                    .bg(rgb(0x070b10))
                    .child(div().px(px(8.0)).py(px(2.0)).bg(rgb(0x0e161d)).text_size(px(11.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.dim)).child(if lang.is_empty() {
                        "CODE".to_string()
                    } else {
                        lang.to_uppercase()
                    }))
                    .child(
                        div()
                            .p(px(8.0))
                            .font_family(MONO)
                            .text_size(px(13.5))
                            .line_height(px(19.0))
                            .text_color(rgb(0xd8e6ee))
                            .children(text.split('\n').map(|l| div().whitespace_nowrap().child(if l.is_empty() { " ".to_string() } else { l.replace('\t', "    ") }))),
                    )
                    .into_any_element(),
            });
        }
        // Links keep their address: each one clickable (opens) under the text.
        for (i, u) in links(md).into_iter().enumerate() {
            let url = u.clone();
            col = col.child(
                div()
                    .id(SharedString::from(format!("link-{i}-{}", u.len())))
                    .flex()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .text_size(px(13.0))
                    .text_color(rgb(pal.accent))
                    .child("⌁")
                    .child(div().underline().child(u))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        (this.on_ask)(Ask::OpenUrl(url.clone()), cx).ok();
                    })),
            );
        }
        col
    }

    fn row_el(&self, ix: usize, r: &Row, cx: &mut Context<Line>) -> AnyElement {
        let pal = self.pal;
        let (plate, tone) = self.plate(&r.who);
        let head = div().flex_none().w(px(74.0)).text_size(px(11.5)).font_weight(FontWeight::BOLD).text_color(rgb(tone)).pt(px(3.0)).child(plate);
        let body: AnyElement = match &r.who {
            Who::Her => {
                let mut b = self.her_body(ix, &r.text, cx);
                if r.streaming {
                    b = b.child(div().w(px(9.0)).h(px(3.0)).bg(rgb(pal.frame)));
                }
                b.into_any_element()
            }
            Who::Tool => {
                // One line (its input clipped) until clicked.
                let open = self.open.contains(&(ix, 0));
                let detail = div().flex_1().min_w(px(0.0)).font_family(MONO).text_size(px(12.0));
                let detail = if open { detail.child(r.detail.clone()) } else { detail.overflow_hidden().whitespace_nowrap().child(one(&r.detail, 90)) };
                div()
                    .id(SharedString::from(format!("tool-{ix}")))
                    .flex()
                    .gap(px(8.0))
                    .min_w(px(0.0))
                    .flex_1()
                    .cursor_pointer()
                    .text_size(px(13.0))
                    .text_color(rgb(pal.dim))
                    .child(div().flex_none().text_color(rgb(pal.frame)).child(if open { "▾" } else { "▸" }))
                    .child(div().flex_none().font_weight(FontWeight::BOLD).child(r.text.clone()))
                    .child(detail)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.open.remove(&(ix, 0)) {
                            this.open.insert((ix, 0));
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Who::Relay(_, kind) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .child(div().text_size(px(11.0)).text_color(rgb(pal.dim)).child(kind.clone()))
                .child(div().border_l_2().border_color(rgb(0x9fd2ff)).pl(px(8.0)).text_color(rgb(pal.text)).child(r.text.clone()))
                .into_any_element(),
            Who::You => div()
                .flex_1()
                .min_w(px(0.0))
                .border_l_2()
                .border_color(rgb(pal.frame))
                .bg(rgba((pal.frame << 8) | 0x14))
                .pl(px(8.0))
                .text_color(rgb(pal.text))
                .child(r.text.clone())
                .into_any_element(),
            Who::Sys | Who::Error => div().flex_1().min_w(px(0.0)).text_size(px(13.5)).text_color(rgb(tone)).child(r.text.clone()).into_any_element(),
        };
        div().flex().gap(px(10.0)).child(head).child(body).into_any_element()
    }

    fn prompt_el(&self, window: &mut Window, cx: &mut Context<Line>) -> Option<Div> {
        let pal = self.pal;
        let p = self.prompt()?;
        let n = self.snap.prompts.len();
        let armed = Instant::now() >= self.prompt_at + ARM;
        let key = |k: &str, label: &str, tone: u32| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().px(px(6.0)).border_1().border_color(rgb(tone)).text_color(rgb(tone)).font_weight(FontWeight::BOLD).child(k.to_string()))
                .child(div().text_color(rgb(if armed { tone } else { pal.dim })).font_weight(FontWeight::BOLD).child(label.to_string()))
        };
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(PAD))
            .border_t_1()
            .border_color(rgba((pal.slash << 8) | 0x99))
            .child(
                div()
                    .flex()
                    .gap(px(10.0))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .child(div().text_color(rgb(pal.slash)).child("! NEEDS YOU"))
                    .child(div().text_color(rgb(pal.name)).child(p.title.to_uppercase()))
                    .child(div().flex_1())
                    .child(div().text_color(rgb(pal.dim)).child(if n > 1 { format!("{} OF {n}  ←/→", self.prompt_ix + 1) } else { "1 OF 1".into() })),
            )
            .child(
                div()
                    .id("prompt-text")
                    .max_h(px(self.max_thread))
                    .overflow_y_scroll()
                    .text_size(px(14.5))
                    .line_height(px(21.0))
                    .text_color(rgb(pal.text))
                    .font_family(MONO)
                    .children(p.text.split('\n').map(|l| div().child(if l.is_empty() { " ".to_string() } else { l.to_string() }))),
            );
        if let Some((_, s)) = &self.secret {
            col = col
                .child(div().border_1().border_color(rgb(pal.frame)).px(px(8.0)).py(px(4.0)).child(Input::new(s).appearance(false)))
                .child(div().text_size(px(11.5)).text_color(rgb(pal.dim)).child("⏎ answer · esc later · never spoken, never logged"));
        } else {
            let id = p.id;
            let risky = risky(p);
            // Deny first; a risky allow says how it's held, and fills while A is down.
            let allow = if risky {
                let fill = self.hold_at.map_or(0.0, |t| (t.elapsed().as_secs_f32() / HOLD.as_secs_f32()).min(1.0));
                if self.hold_at.is_some() {
                    window.request_animation_frame();
                }
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().px(px(6.0)).border_1().border_color(rgb(pal.frame)).text_color(rgb(pal.frame)).font_weight(FontWeight::BOLD).child("HOLD A"))
                    .child(div().text_color(rgb(pal.dim)).child("or ⇧A"))
                    .child(
                        div()
                            .relative()
                            .px(px(6.0))
                            .border_1()
                            .border_color(rgb(if armed { pal.frame } else { pal.dim }))
                            .font_weight(FontWeight::BOLD)
                            .child(div().absolute().left_0().top_0().bottom_0().w(relative(fill)).bg(rgba((pal.frame << 8) | 0x88)))
                            .child(div().relative().text_color(rgb(if armed { pal.frame } else { pal.dim })).child("ALLOW")),
                    )
            } else {
                key("A", "ALLOW", pal.frame)
            };
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(22.0))
                    .text_size(px(12.5))
                    .child(div().id("deny").cursor_pointer().child(key("D", "DENY", pal.slash)).on_click(cx.listener(move |this, _, window, cx| this.answer(id, None, window, cx))))
                    .child(div().id("allow").cursor_pointer().child(allow).on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        // A click allows a risky one only with Shift held.
                        if !risky || ev.modifiers().shift {
                            this.answer(id, Some("y".into()), window, cx);
                        } else {
                            this.note = Some("NOT ALLOWED · ⇧CLICK, ⇧A OR HOLD A".into());
                            cx.notify();
                        }
                    })))
                    .child(key("ESC", "LATER", pal.dim))
                    .child(key("TAB", "SAY SOMETHING INSTEAD", pal.dim)),
            );
            if risky {
                col = col.child(div().text_size(px(11.5)).text_color(rgb(pal.slash)).child("this one can run or change things: a tap of A does nothing"));
            }
        }
        if !armed {
            // Re-render when the keys arm.
            let left = (self.prompt_at + ARM).saturating_duration_since(Instant::now());
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(left + Duration::from_millis(5)).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        }
        Some(col)
    }
}

impl Render for Line {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal;
        self.renders.set(self.renders.get() + 1);
        if let Some((t0, n)) = self.sent_at
            && self.snap.rows.iter().skip(n).any(|r| r.who == Who::Her && !r.text.trim().is_empty())
        {
            self.sent_at = None;
            elog!("timing: line: first word of her reply drawn {} ms after Enter", t0.elapsed().as_millis());
        }
        let status = if self.parked { "PARKED · $MOD+C TO RESUME".to_string() } else { self.snap.status.clone() };
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(PAD))
            .h(px(30.0))
            .border_b_1()
            .border_color(rgba((pal.frame << 8) | 0x55))
            .text_size(px(12.0))
            .font_weight(FontWeight::BOLD)
            .child(div().text_color(rgb(pal.name)).child(self.name.to_uppercase()))
            .child(div().text_color(rgb(pal.slash)).child("/"))
            .child(div().text_color(rgb(pal.dim)).child("LINE"))
            .child(div().flex_1())
            .child(div().text_color(rgb(if self.parked { pal.dim } else { pal.frame })).child(status));
        let rows: Vec<AnyElement> = self.snap.rows.iter().enumerate().map(|(i, r)| self.row_el(i, r, cx)).collect();
        // Scrolled: the top edge fades out instead of cutting a line in half.
        let scrolled = self.scroll.offset().y < px(-1.0);
        let thread = (!rows.is_empty()).then(|| {
            let fade = linear_gradient(180.0, linear_color_stop(rgb(pal.surface), 0.0), linear_color_stop(rgba(pal.surface << 8), 1.0));
            div()
                .relative()
                .child(
                    div()
                        .id("thread")
                        .max_h(px(self.max_thread))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .px(px(PAD))
                        .py(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(9.0))
                        .children(rows),
                )
                .children(scrolled.then(|| div().absolute().top_0().left_0().right_0().h(px(LINE_H * 1.2)).bg(fade)))
        });
        let prompt = (self.mode == Mode::Prompt).then(|| self.prompt_el(window, cx)).flatten();
        let pending = (self.mode == Mode::Text && !self.snap.prompts.is_empty()).then(|| {
            div()
                .px(px(PAD))
                .py(px(4.0))
                .bg(rgb(pal.slash))
                .text_color(rgb(pal.surface))
                .text_size(px(12.0))
                .font_weight(FontWeight::BOLD)
                .child(format!("! {} PROMPT{} WAITING · TAB TO ANSWER", self.snap.prompts.len(), if self.snap.prompts.len() > 1 { "S" } else { "" }))
        });
        // A text box you can't miss: filled, outlined, lit in her colour while it has the keyboard.
        let typing = self.mode == Mode::Text && !self.parked;
        let input = (self.mode == Mode::Text).then(|| {
            div().px(px(PAD)).pt(px(10.0)).border_t_1().border_color(rgba((pal.frame << 8) | 0x55)).child(
                div()
                    .p(px(if typing { 0.0 } else { 1.0 }))
                    .border_color(rgb(if typing { pal.frame } else { pal.dim }))
                    .when(typing, |d| d.border_2())
                    .when(!typing, |d| d.border_1())
                    .bg(rgb(0x0c1a24))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .child(div().flex_none().text_color(rgb(if typing { pal.frame } else { pal.dim })).font_weight(FontWeight::BOLD).child("›"))
                    .child(
                        div().flex_1().min_w(px(0.0)).child(
                            Textarea::new(&self.input).appearance(false).text_size(px(FONT)).line_height(px(LINE_H)).text_color(rgb(pal.text)),
                        ),
                    ),
            )
        });
        let hint = if self.mode == Mode::Prompt {
            None
        } else if !self.snap.live {
            Some("OFFLINE · not sent while offline · draft is kept".to_string())
        } else if self.snap.busy {
            Some("⏎ steer · esc esc abort · ctrl+y copy reply · pgup thread · esc close".into())
        } else {
            Some("⏎ send · ⇧⏎ newline · ↑ last message · ctrl+y copy reply · ctrl+o open link · esc close".into())
        };
        let footer = div()
            .flex()
            .gap(px(10.0))
            .px(px(PAD))
            .pt(px(4.0))
            .pb(px(8.0))
            .text_size(px(11.5))
            .children(self.note.clone().map(|n| div().text_color(rgb(pal.slash)).font_weight(FontWeight::BOLD).child(n)))
            .children(hint.map(|h| div().text_color(rgb(pal.dim)).child(h)));
        // Laid out at its natural height (absolute, unconstrained) and measured: the window
        // follows it, bottom edge fixed.
        let probe_h = self.height;
        let me = cx.entity().downgrade();
        let content = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .child(header)
            .children(thread)
            .children(pending)
            .children(prompt)
            .children(input)
            .child(footer)
            .child(
                canvas(
                    move |b, _, cx| {
                        let h = f32::from(b.size.height);
                        if (h - probe_h).abs() > 1.0 {
                            let me = me.clone();
                            cx.defer(move |cx| {
                                me.update(cx, |l, cx| {
                                    l.height = h;
                                    (l.on_ask)(Ask::Height(h + 4.0), cx).ok();
                                })
                                .ok();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let _ = window;
        div()
            .id("line")
            .track_focus(&self.focus)
            .key_context("Line")
            .on_key_up(cx.listener(|this, ev: &KeyUpEvent, _, cx| this.key_up(&ev.keystroke, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    // A click on the parked Line brings it back.
                    if this.parked {
                        this.parked = false;
                        (this.on_ask)(Ask::Parked(false), cx).ok();
                        this.refocus(window, cx);
                        cx.notify();
                    }
                }),
            )
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(self.font.clone())
            .text_size(px(FONT))
            .line_height(px(LINE_H))
            .bg(rgb(pal.surface))
            .border_2()
            .border_color(rgb(if self.parked { pal.dim } else { pal.frame }))
            .child(content)
            .children(self.tail())
    }
}

#[cfg(test)]
mod tests {
    use super::{links, risky};
    use crate::session::PromptView;

    fn confirm(title: &str, text: &str) -> PromptView {
        PromptView { id: 1, title: title.into(), text: text.into(), secret: false }
    }

    #[test]
    fn risky_prompts_are_activations_and_tools_that_run_or_change_things() {
        assert!(risky(&confirm("Confirm tool activation", "activation of 3 tool id(s):\n  builtin:shell_end")));
        assert!(risky(&confirm("Confirm tool call", "bash: rm -rf /tmp/x")));
        assert!(risky(&confirm("Confirm tool call", "write_file: src/main.rs")));
        assert!(risky(&confirm("Confirm tool call", "exec: make")));
        assert!(!risky(&confirm("Confirm tool call", "read: README.md (lines 1-40)")));
        assert!(!risky(&confirm("Confirm compaction", "Summarise older turns now?")));
        assert!(!risky(&PromptView { id: 2, title: "password required".into(), text: "run it".into(), secret: true }));
    }

    #[test]
    fn links_keep_their_address() {
        let md = "see [the docs](https://x.y/z) and https://a.b/c. also `code` [rel](./not)";
        assert_eq!(links(md), vec!["https://x.y/z".to_string(), "https://a.b/c".to_string()]);
        assert!(links("no links here").is_empty());
    }
}
