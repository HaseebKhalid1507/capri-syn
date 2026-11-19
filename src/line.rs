//! THE LINE: the conversation in its own window (ADVICE-ux §3.3). The thread's tail above a
//! real text field: Enter sends and the Line stays, Shift+Enter is a newline, ↑ recalls,
//! Esc closes (Esc Esc aborts a running turn), losing the keyboard parks it. A pending
//! prompt shows whole, answered with A / D (armed after 400 ms) or a masked field for a
//! secret. An opaque window, like the reader (no shape, nothing to slab): the panel is cut
//! inside it, the dark ring outside its cuts.
//!
//! The avatar owns the session; the Line gets snapshots ([`Snap`]) and asks for things
//! ([`Ask`]).

mod settings;
pub use settings::{Page, Pref, Prefs};

use crate::reader::{Block, Mark, Pal, inline, parse};
use crate::theme::{self, Cut, Fill, glyph, tracked};
use crate::session::PromptView;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::{Duration, Instant};

/// Prompt keys do nothing this long after a prompt shows (as the buttons).
pub const ARM: Duration = Duration::from_millis(400);
/// A risky prompt (a tool activation, shell/exec/write-class tools) allows only on A (or a
/// click on ALLOW) held this long; a tap does nothing there. No Shift+A: that is just a capital
/// A, which typing into the wrong focus would send.
pub const HOLD: Duration = Duration::from_millis(450);
const ESC_AGAIN: Duration = Duration::from_millis(1000);
const PAD: f32 = 14.0;
const FONT: f32 = 16.0;
const LINE_H: f32 = 23.0;
/// The › prompt's drop in the field: its middle on the first line's x-height (r21: it sat
/// 2 px high of the type).
const PROMPT_DROP: f32 = 11.0;
const MONO: &str = "DejaVu Sans Mono";
/// Her portrait beside her rows, logical px tall.
pub const FACE_PX: f32 = 30.0;
/// The panel inside its window: the 1 px ring, then the 2 px frame.
const INSET: f32 = 3.0;

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
    /// Tool rows: their input. Her rows: what her words came from, when it helps (a local
    /// server's own error), shown small under them.
    pub detail: String,
    pub streaming: bool,
}

/// What the Line shows, from the avatar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snap {
    pub rows: Vec<Row>,
    pub status: String,
    /// The status's glyph, and whether it is a solid swallowtail (NEEDS YOU, the mic live)
    /// and a bone one (NEEDS YOU).
    pub glyph: String,
    pub flag: bool,
    pub alarm: bool,
    pub busy: bool,
    pub live: bool,
    pub prompts: Vec<PromptView>,
    /// She is speaking a spoken reply (Esc hushes her first).
    pub speaking: bool,
    /// The mic is held open (Esc cancels: nothing sent).
    pub listening: bool,
    /// The mic row while he holds the key (`● MIC 0:02 ▮▮▯▯▯`), or what follows key-up.
    pub mic: Option<String>,
    /// How he presses her summon key (`$MOD+C`, `WIN+ALT+C`): the parked hints.
    pub key: String,
    /// The sentence she is saying now (the caption, paced by her voice).
    pub caption: Option<String>,
    /// Her face now (her mood's, `moods.json` `lineFaces`): on her latest row; the portrait on
    /// the others.
    pub face: Option<Face>,
    /// The last turn failed for a reason with a fix (`session::Trouble::tip`): a quiet line in
    /// the footer when it comes up.
    pub trouble: Option<String>,
    /// Her settings as she runs them: while the Settings page is up.
    pub prefs: Option<Prefs>,
    /// `[hud] glitch` and `motion` (reduced motion: false): the page swap's split, the cog's turn.
    pub glitch: bool,
    pub motion: bool,
}

/// A face image in the snapshot: the same face is the same image.
#[derive(Clone)]
pub struct Face(pub std::sync::Arc<RenderImage>);

impl PartialEq for Face {
    fn eq(&self, o: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &o.0)
    }
}

impl Eq for Face {}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Face")
    }
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
    /// Esc while she speaks: stop talking (the reply stays in the thread).
    Hush,
    /// Esc while the mic is held: nothing is transcribed or sent.
    CancelListen,
    /// The page it shows now (the avatar fills `Snap::prefs` while it is Settings).
    Page(Page),
    /// A setting changed on the Settings page: written to settings.toml and applied.
    Pref(Pref),
}

/// The command a paragraph (or a bubble line) is, when it is nothing but one `` `inline code` ``
/// span: whole, or still typing out (no closing backtick yet).
pub fn command(t: &str) -> Option<&str> {
    let c = t.trim().strip_prefix('`')?;
    let c = c.strip_suffix('`').unwrap_or(c).trim();
    (!c.is_empty() && !c.contains('`')).then_some(c)
}

pub type OnAsk = Box<dyn Fn(Ask, &mut App) -> Result<(), String>>;

/// How the avatar's refusal of `Ask::Parked(false)` starts: the desktop kept the keyboard with
/// his window (it hands it over only on his input). The Line waits for his key or click.
pub const NO_KEYBOARD: &str = "no keyboard";
/// What an ask gets once the avatar is gone.
pub const GONE: &str = "she is gone";

/// A failed `Ask::Parked(false)` that leaves the Line waiting for him: the desktop refused
/// the keyboard. Any other failure (she is gone) is no wait.
fn keyboard_refused(e: &str) -> bool {
    e.starts_with(NO_KEYBOARD)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    Prompt,
}

pub struct Line {
    snap: Snap,
    /// Her face beside her rows (lachy's portrait), if the pack has it.
    face: Option<std::sync::Arc<RenderImage>>,
    pal: Pal,
    font: SharedString,
    name: String,
    max_thread: f32,
    input: Entity<TextareaState>,
    secret: Option<(u64, Entity<InputState>)>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// ALLOW's box (window px, from the last paint), for tests.
    allow_box: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// The thread's height cut to whole rows (from the last layout), so at the bottom its top
    /// starts at a row, never half of one under the header.
    fit_h: Option<f32>,
    /// Scrolled up by him: new rows don't pull the view down.
    pinned_up: bool,
    sent: Vec<String>,
    recall: Option<usize>,
    esc_at: Option<Instant>,
    note: Option<String>,
    /// A quiet tip under a failed turn (`Trouble::tip`): not a warning, so not the note's style.
    tip: Option<String>,
    /// The command chip last copied (its label reads `copied`).
    copied_cmd: Option<String>,
    parked: bool,
    /// Opened without the keyboard and never had it (`wait`): its hint says how to type.
    waiting: bool,
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
    /// Its side of her (`beside::place`) and where along the edge facing her its tail points
    /// (logical px); None: not beside her.
    tail: Option<(crate::beside::Side, f32)>,
    /// The mic row's rec dot blinks at 1 Hz from here; a wake-up is pending.
    born: Instant,
    blink_armed: bool,
    /// Chat or Settings, and where Esc goes back to.
    nav: settings::Nav,
    /// The Settings row picked (↑↓).
    sel: usize,
    /// The last page swap (its RGB split, the cog's turn).
    swap: Option<Instant>,
    /// The tallest the panel was on a page it left: both pages are at least as tall from then
    /// on, so swapping back and forth never moves its top (only the first can, if Settings
    /// needs more than chat had).
    keep_h: f32,
    /// The field's height as last laid out (one line: the cog's; Settings' bar is as tall).
    field_h: std::rc::Rc<std::cell::Cell<f32>>,
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
        face: Option<std::sync::Arc<RenderImage>>,
        page: Page,
        on_ask: OnAsk,
        window: &mut Window,
        cx: &mut Context<Line>,
    ) -> Line {
        let mut title = name.to_lowercase();
        if let Some(c) = title.get(..1) {
            title = c.to_uppercase() + &title[1..];
        }
        let placeholder = format!("talk to {title}…");
        // The caret in bone, the focus colour (1 px wide in the kit: the colour has to carry it).
        // The placeholder in text-dim, a selection in accent-dim (bone stays readable on it).
        let colors = &mut gpui_kit::component::Theme::global_mut(cx).colors;
        colors.caret = rgb(pal.focus).into();
        colors.muted_foreground = rgb(pal.dim).into();
        colors.selection = rgb(pal.accent_dim).into();
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
                        this.unpark(window, cx).ok();
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
            face,
            snap,
            pal,
            font,
            name,
            max_thread,
            input,
            secret: None,
            focus,
            scroll: ScrollHandle::new(),
            allow_box: Default::default(),
            fit_h: None,
            pinned_up: false,
            sent,
            recall: None,
            esc_at: None,
            note: None,
            tip: None,
            copied_cmd: None,
            parked: false,
            waiting: false,
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
            tail: None,
            born: Instant::now(),
            blink_armed: false,
            nav: settings::Nav::on(page),
            sel: 0,
            swap: None,
            keep_h: 0.0,
            field_h: std::rc::Rc::new(std::cell::Cell::new(settings::FIELD_H0)),
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
        // Settings: the Line's own handle (keys never type into the hidden field).
        if self.nav.page == Page::Settings {
            window.focus(&self.focus, cx);
            return;
        }
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
        if snap.trouble != self.snap.trouble {
            self.tip = snap.trouble.clone();
        }
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
            // A prompt arriving while he types (or on Settings) is announced, never takes the field.
            if !had_prompts && self.mode == Mode::Text && self.nav.page == Page::Chat && self.input.read(cx).value().trim().is_empty() && !self.parked {
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
            "waiting": self.waiting,
            "rows": self.snap.rows.len(),
            "draft_chars": self.input.read(cx).value().chars().count(),
            "renders": self.renders.get(),
            "side": self.tail.map(|t| side_key(t.0)),
            "tail": self.tail.map(|t| t.1),
            "holding": self.hold_at.is_some(),
            "allow": self.allow_box.get().map(|b| [f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)]),
            "risky": self.prompt().is_some_and(risky),
            "note": self.note,
            "tip": self.tip,
            "prompts": self.snap.prompts.len(),
            "prompt_ix": self.prompt_ix,
            "height": self.height,
            "scroll": [f32::from(self.scroll.offset().y), f32::from(self.scroll.max_offset().y)],
            "page": self.nav.page.key(),
            "sel": self.sel,
        })
    }

    /// Parked from outside (a click elsewhere): it shows so until the summon key or a click.
    pub fn park(&mut self, cx: &mut Context<Line>) {
        self.parked = true;
        cx.notify();
    }

    /// Opened without the keyboard (the desktop hands it over only on his input): parked, its
    /// hint saying how to type, until his key or his click.
    pub fn wait(&mut self, cx: &mut Context<Line>) {
        self.parked = true;
        self.waiting = true;
        cx.notify();
    }

    /// Back from parked, if the avatar can take the keyboard for it (it can't on a summon from
    /// elsewhere where the desktop hands it over only on his input: `keyboard_refused`).
    fn unpark(&mut self, window: &mut Window, cx: &mut Context<Line>) -> Result<(), String> {
        let r = (self.on_ask)(Ask::Parked(false), cx);
        if r.is_ok() {
            self.parked = false;
            self.waiting = false;
            self.refocus(window, cx);
        }
        r
    }

    /// The summon key on an open Line: back to it (parked), or close it. Only a refusal of
    /// the keyboard leaves it waiting; an avatar that is gone says so.
    pub fn summon(&mut self, window: &mut Window, cx: &mut Context<Line>) -> String {
        if self.parked || !window.is_window_active() {
            let r = self.unpark(window, cx);
            cx.notify();
            match r {
                Ok(()) => "resumed".into(),
                Err(e) if keyboard_refused(&e) => "waiting".into(),
                Err(e) => {
                    elog!("synaps-avatar: line: summoned, but {e}");
                    format!("error: {e}")
                }
            }
        } else {
            self.close(true, window, cx);
            "closed".into()
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
                self.tip = None;
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
        // Ctrl+, : the cog's key.
        if ctrl && !m.alt && k.key == "," {
            self.nav.cog();
            self.page_changed(window, cx);
            return true;
        }
        if self.nav.page == Page::Settings {
            return self.settings_key(k, window, cx);
        }
        match (k.key.as_str(), self.mode) {
            ("escape", _) if self.snap.listening => {
                (self.on_ask)(Ask::CancelListen, cx).ok();
                true
            }
            ("escape", _) if self.snap.speaking => {
                (self.on_ask)(Ask::Hush, cx).ok();
                self.note = Some("HUSHED · THE REST IS HERE".into());
                cx.notify();
                true
            }
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
                if !risky {
                    self.hold_at = None;
                    self.answer(id, Some("y".into()), window, cx);
                } else if self.hold_at.is_none() {
                    // Held: it allows at HOLD unless A comes up first (key repeats ignored).
                    self.start_hold(id, now, window, cx);
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

    /// A risky allow, held (A or a click on ALLOW): it fires at HOLD unless let go first.
    fn start_hold(&mut self, id: u64, now: Instant, window: &mut Window, cx: &mut Context<Line>) {
        self.hold_at = Some(now);
        self.note = None;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            this.update_in(cx, |l, window, cx| {
                if settings::hold_fires(l.hold_at, now, l.nav.page, l.prompt().is_some_and(|p| p.id == id)) {
                    l.hold_at = None;
                    l.answer(id, Some("y".into()), window, cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// A let go before HOLD on a risky prompt: nothing allowed, and he is told how.
    fn let_go(&mut self, cx: &mut Context<Line>) {
        if let Some(t0) = self.hold_at.take() {
            if t0.elapsed() < HOLD {
                self.note = Some(format!("NOT ALLOWED · HOLD A FOR {:.2} s TO ALLOW THIS ONE", HOLD.as_secs_f32()));
            }
            cx.notify();
        }
    }

    fn key_up(&mut self, k: &Keystroke, cx: &mut Context<Line>) {
        if k.key == "a" {
            self.a_down = false;
            self.let_go(cx);
        }
    }

    /// Her side of the Line: a thick edge and a pointer at her, so it reads as hers.
    fn tail(&self) -> Option<AnyElement> {
        let (side, at) = self.tail?;
        let p = self.pal;
        Some(if self.parked { tail_overlay(side, at, p.dim, p.dim) } else { tail_overlay(side, at, p.border, p.accent) })
    }

    /// Where it is beside her (from the avatar, each time it is placed).
    pub fn set_tail(&mut self, tail: Option<(crate::beside::Side, f32)>, cx: &mut Context<Line>) {
        if self.tail != tail {
            self.tail = tail;
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
            Who::You => ("YOU".into(), p.text),
            Who::Her => (self.name.to_uppercase(), p.accent),
            Who::Relay(c, _) => (format!("⇄ {c}"), p.dim),
            Who::Tool => ("⚙".into(), p.dim),
            Who::Sys => ("SYS".into(), p.dim),
            Who::Error => ("SYS".into(), p.danger),
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
                    Mark::Code => HighlightStyle { color: Some(rgb(pal.text).into()), background_color: Some(rgb(pal.raise).into()), ..Default::default() },
                    Mark::Em => HighlightStyle { font_style: Some(FontStyle::Italic), color: Some(rgb(pal.text).into()), ..Default::default() },
                };
                (r, s)
            })
            .collect();
        StyledText::new(plain).with_highlights(hl)
    }

    fn her_body(&self, row: usize, md: &str, cx: &mut Context<Line>) -> Div {
        let pal = self.pal;
        let mut col = div().flex().flex_col().gap(px(6.0)).min_w(px(0.0)).flex_1();
        let lang_of = |lang: &str| if lang.is_empty() { "CODE".to_string() } else { lang.to_uppercase() };
        for (bi, b) in parse(md).into_iter().enumerate() {
            col = col.child(match b {
                Block::Heading(_, t) => div().font_weight(FontWeight::BOLD).text_color(rgb(pal.accent)).child(self.styled(&t)).into_any_element(),
                // A paragraph that is one command (`synaps login`): a chip, its text copied on a click.
                Block::Para(t) if command(&t).is_some() => {
                    let c = command(&t).unwrap_or_default().to_string();
                    let copied = self.copied_cmd.as_deref() == Some(c.as_str());
                    div()
                        .flex()
                        .child(
                            div()
                                .id(SharedString::from(format!("cmd-{row}-{bi}")))
                                .relative()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .px(px(10.0))
                                .py(px(3.0))
                                .child(theme::backdrop(Cut::Corners(5.0), Fill::framed(pal.code, pal.accent_dim, 1.0)))
                                .cursor_pointer()
                                .child(div().flex_none().font_family(MONO).text_color(rgb(pal.text)).child(format!("$ {c}")))
                                .child(
                                    div()
                                        .flex_none()
                                        .pl(px(8.0))
                                        .border_l_1()
                                        .border_color(rgb(pal.rule))
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(if copied { pal.accent } else { pal.text }))
                                        .child(if copied { "copied ✓" } else { "copy" }),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    (this.on_ask)(Ask::Copy(c.clone()), cx).ok();
                                    this.copied_cmd = Some(c.clone());
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                }
                Block::Para(t) => div().text_color(rgb(pal.text)).child(self.styled(&t)).into_any_element(),
                Block::Bullet(indent, marker, t) => div()
                    .pl(px(indent as f32 * 16.0))
                    .flex()
                    .gap(px(6.0))
                    .text_color(rgb(pal.text))
                    .child(div().flex_none().text_color(rgb(pal.accent)).child(marker))
                    .child(div().flex_1().min_w(px(0.0)).child(self.styled(&t)))
                    .into_any_element(),
                Block::Rule => div().h(px(1.0)).bg(rgb(pal.rule)).into_any_element(),
                Block::Code { lang, text } if !self.open.contains(&(row, bi)) => {
                    // One line until clicked: the language, the first line, the size. A
                    // raised strip, cut 5 px, a dark-red hairline.
                    let n = text.lines().count();
                    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                    div()
                        .id(SharedString::from(format!("code-{row}-{bi}")))
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(2.0))
                        .child(theme::backdrop(Cut::Corners(5.0), Fill::framed(pal.raise, pal.rule, 1.0)))
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .child(div().flex_none().text_color(rgb(pal.accent)).child("▸"))
                        .child(div().flex_none().font_weight(FontWeight::BOLD).text_color(rgb(pal.accent)).child(tracked(&lang_of(&lang), 1.0)))
                        .child(div().flex_1().min_w(px(0.0)).overflow_hidden().whitespace_nowrap().font_family(MONO).text_color(rgb(pal.text)).child(one(&first, 70)))
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
                    .relative()
                    .child(theme::backdrop(Cut::Corners(9.0), Fill::framed(pal.code, pal.rule, 1.5)))
                    .child(
                        div().px(px(8.0)).py(px(3.0)).border_b(px(1.5)).border_color(rgb(pal.rule)).flex().child(
                            theme::plate(Cut::Tab(5.0), Fill::solid(pal.accent))
                                .h(px(16.0))
                                .pl(px(7.0))
                                .pr(px(10.0))
                                .text_size(px(11.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(pal.ink))
                                .child(tracked(&lang_of(&lang), 1.4)),
                        ),
                    )
                    .child(
                        div()
                            .p(px(8.0))
                            .font_family(MONO)
                            .text_size(px(13.5))
                            .line_height(px(19.0))
                            .text_color(rgb(pal.text))
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
        let mut head = div().flex_none().w(px(74.0)).text_size(px(11.5)).font_weight(FontWeight::BOLD).text_color(rgb(tone)).pt(px(3.0));
        // Her portrait over her name (lachy's face.png, cut to its opaque box); on her latest
        // row, her face now (her mood's), if the pack has one.
        let latest = self.snap.rows.iter().rposition(|r| r.who == Who::Her) == Some(ix);
        let face = if latest { self.snap.face.as_ref().map(|f| &f.0).or(self.face.as_ref()) } else { self.face.as_ref() };
        if r.who == Who::Her
            && let Some(f) = face
        {
            let s = f.size(0);
            let h = FACE_PX;
            let w = h * u32::from(s.width) as f32 / (u32::from(s.height) as f32).max(1.0);
            let tile = img(ImageSource::Render(f.clone())).w(px(w)).h(px(h));
            // Her mood's face: a hairline of her red around it (its art fills the tile).
            let tile = if latest && self.snap.face.is_some() {
                div().flex_none().w(px(w + 2.0)).h(px(h + 2.0)).p(px(1.0)).bg(rgb(pal.accent_dim)).child(tile).into_any_element()
            } else {
                tile.into_any_element()
            };
            head = head.flex().flex_col().gap(px(4.0)).child(tile);
        }
        let head = head.child(tracked(&plate, 1.4));
        let body: AnyElement = match &r.who {
            Who::Her => {
                let mut b = self.her_body(ix, &r.text, cx);
                // What she said comes from (a local server's own error): small, under it.
                if !r.detail.is_empty() {
                    b = b.child(div().font_family(MONO).text_size(px(12.0)).text_color(rgb(pal.dim)).child(r.detail.clone()));
                }
                if r.streaming {
                    b = b.child(div().w(px(9.0)).h(px(3.0)).bg(rgb(pal.accent)));
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
                    .child(div().flex_none().child(if open { "▾" } else { "▸" }))
                    .child(div().flex_none().font_weight(FontWeight::BOLD).text_color(rgb(pal.text)).child(r.text.clone()))
                    .child(detail)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.open.remove(&(ix, 0)) {
                            this.open.insert((ix, 0));
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            }
            // Another session's words: a text-dim bar.
            Who::Relay(_, kind) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .child(div().text_size(px(11.0)).text_color(rgb(pal.dim)).child(kind.clone()))
                .child(div().border_l_2().border_color(rgb(pal.dim)).pl(px(8.0)).text_color(rgb(pal.text)).child(r.text.clone()))
                .into_any_element(),
            // His words: a slanted text-dim sliver, a step under bone.
            Who::You => {
                let bar_c = pal.dim;
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .relative()
                    .pl(px(12.0))
                    .text_color(rgb(pal.you))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |b, _, window, _| {
                                let (x, t, bt) = (f32::from(b.left()), f32::from(b.top()) + 4.0, f32::from(b.bottom()) - 4.0);
                                let lean = 0.249 * (bt - t) / 2.0;
                                theme::fill_poly(window, &[(x + lean, t), (x + 3.0 + lean, t), (x + 3.0 - lean, bt), (x - lean, bt)], rgb(bar_c));
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    )
                    .child(r.text.clone())
                    .into_any_element()
            }
            Who::Sys | Who::Error => div().flex_1().min_w(px(0.0)).text_size(px(13.5)).text_color(rgb(tone)).child(r.text.clone()).into_any_element(),
        };
        // His turns sit on a faint bar, so a run of them doesn't read as one (r21).
        let you = matches!(r.who, Who::You);
        div()
            .flex()
            .gap(px(10.0))
            .when(you, |d| d.relative().mx(px(-8.0)).px(px(8.0)).py(px(5.0)).child(theme::backdrop(Cut::Corners(6.0), Fill::solid(pal.you_bar))))
            .child(head)
            .child(body)
            .into_any_element()
    }

    fn prompt_el(&self, window: &mut Window, cx: &mut Context<Line>) -> Option<Div> {
        let pal = self.pal;
        let p = self.prompt()?;
        let n = self.snap.prompts.len();
        let armed = Instant::now() >= self.prompt_at + ARM;
        // A key: its letter on a slanted plate, then what it does. DENY is bone, ALLOW red,
        // LATER and TAB text-dim.
        let key = |k: &str, label: &str, tone: u32| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(theme::plate(Cut::Slant(6.6), Fill::line(tone, 1.5)).h(px(20.0)).px(px(9.0)).text_color(rgb(tone)).font_weight(FontWeight::BOLD).child(k.to_string()))
                .child(div().text_color(rgb(if armed { tone } else { pal.dim })).font_weight(FontWeight::BOLD).child(tracked(label, 1.0)))
        };
        let focus_c = pal.focus;
        // DENY first, with the focus ring on it.
        let focus_ring = canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let (x0, y0, x1, y1) = (f32::from(b.left()) - 6.0, f32::from(b.top()) - 6.0, f32::from(b.right()) + 6.0, f32::from(b.bottom()) + 6.0);
                theme::paint(window, &theme::outline(Cut::Tag(theme::Plate::Square), x0, y0, x1, y1), Fill::line(focus_c, 2.0));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(PAD))
            .border_t_2()
            .border_color(rgb(pal.accent))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .child(
                        // NEEDS YOU: the bone swallowtail, deep-red type, as on her nameplate.
                        theme::plate(Cut::Swallow(6.0), Fill::solid(pal.text))
                            .h(px(18.0))
                            .pl(px(8.0))
                            .pr(px(14.0))
                            .gap(px(5.0))
                            .text_color(rgb(pal.deep))
                            .child(glyph("bang", pal.deep, pal.text))
                            .child(tracked("NEEDS YOU", 1.2)),
                    )
                    .child(div().text_color(rgb(pal.text)).child(tracked(&p.title.to_uppercase(), 1.2)))
                    .child(div().flex_1())
                    .child(tracked(&if n > 1 { format!("{} OF {n}  ←/→", self.prompt_ix + 1) } else { "1 OF 1".into() }, 1.2).flex_none().whitespace_nowrap().text_color(rgb(pal.dim))),
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
                .child(div().relative().child(theme::backdrop(Cut::Corners(9.0), Fill::framed(pal.field, pal.accent, 2.0))).px(px(10.0)).py(px(6.0)).child(Input::new(s).appearance(false)))
                .child(div().text_size(px(11.5)).text_color(rgb(pal.dim)).child("⏎ answer · esc later · never spoken, never logged"));
        } else {
            let id = p.id;
            let risky = risky(p);
            // Deny first; a risky allow says how it's held, and fills while A is down: red
            // from the left, its label ink under the fill, ALLOWED ✓ when it's done.
            let allow = if risky {
                let fill = self.hold_at.map_or(0.0, |t| (t.elapsed().as_secs_f32() / HOLD.as_secs_f32()).min(1.0));
                if self.hold_at.is_some() {
                    window.request_animation_frame();
                }
                let tone = if armed { pal.accent } else { pal.dim };
                let done = fill >= 1.0;
                let word = if done { "ALLOWED ✓" } else { "ALLOW" };
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(theme::plate(Cut::Slant(6.6), Fill::line(pal.accent, 1.5)).h(px(20.0)).px(px(9.0)).text_color(rgb(pal.accent)).font_weight(FontWeight::BOLD).child("HOLD A"))
                    .child(div().text_color(rgb(pal.text)).child(format!("· {:.2} s", HOLD.as_secs_f32())))
                    .child(
                        theme::plate(Cut::Slant(6.6), Fill::line(tone, 1.5))
                            .h(px(20.0))
                            .px(px(12.0))
                            .font_weight(FontWeight::BOLD)
                            .child(div().text_color(rgb(tone)).child(tracked(word, 1.0)))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(relative(fill))
                                    .overflow_hidden()
                                    .child(theme::backdrop(if done { Cut::Slant(6.6) } else { Cut::Tab(7.0) }, Fill::solid(pal.accent)))
                                    .child(div().absolute().left(px(12.0)).top_0().bottom_0().flex().items_center().text_color(rgb(pal.ink)).child(tracked(word, 1.0))),
                            ),
                    )
            } else {
                key("A", "ALLOW", pal.accent)
            };
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(22.0))
                    .text_size(px(12.5))
                    .child(
                        div()
                            .id("deny")
                            .relative()
                            .cursor_pointer()
                            .child(focus_ring)
                            .child(key("D", "DENY", pal.text))
                            .on_click(cx.listener(move |this, _, window, cx| this.answer(id, None, window, cx))),
                    )
                    .child(
                        div()
                            .id("allow")
                            .relative()
                            .cursor_pointer()
                            .child(allow)
                            .child({
                                let cell = self.allow_box.clone();
                                canvas(move |b, _, _| cell.set(Some(b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
                            })
                            // A click allows a benign one; a risky one wants the press held.
                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                if Instant::now() < this.prompt_at + ARM {
                                    return;
                                }
                                if !risky {
                                    this.answer(id, Some("y".into()), window, cx);
                                } else if this.hold_at.is_none() {
                                    this.start_hold(id, Instant::now(), window, cx);
                                }
                            }))
                            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.let_go(cx)))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.let_go(cx))),
                    )
                    .child(key("ESC", "LATER", pal.dim))
                    .child(key("TAB", "SAY SOMETHING INSTEAD", pal.dim)),
            );
            if risky {
                // Danger: the bone plate with teeth, ink type, a red ⚠.
                col = col.child(
                    theme::plate(Cut::Teeth, Fill::solid(pal.danger).ringed(pal.ring))
                        .mt(px(4.0))
                        .py(px(9.0))
                        .items_center()
                        .text_size(px(14.0))
                        .line_height(px(19.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(pal.ink))
                        .child(div().flex_none().pl(px(14.0)).pr(px(6.0)).child(theme::glyph_at("warn", pal.accent, pal.danger, 16.0)))
                        .child(div().flex_1().min_w(px(0.0)).pl(px(8.0)).pr(px(14.0)).child(format!(
                            "This one can run or change things on your machine. Hold A (or hold the click) for {:.2} s to allow; a tap does nothing.",
                            HOLD.as_secs_f32()
                        ))),
                );
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
        let status = match (self.parked, self.waiting) {
            (true, true) => format!("{} TO TYPE", self.snap.key),
            (true, false) => format!("PARKED · {} TO RESUME", self.snap.key),
            _ => self.snap.status.clone(),
        };
        // Her name on a red blade, LINE, and her state at the right: NEEDS YOU (and the live
        // mic) on a red swallowtail; parked, all text-dim.
        let state: AnyElement = if self.snap.flag && !self.parked {
            // NEEDS YOU: the bone swallowtail, deep-red type (the nameplate's first beat); the
            // mic live: red, ink.
            let (fill_c, ink_c) = if self.snap.alarm { (pal.text, pal.deep) } else { (pal.accent, pal.ink) };
            theme::plate(Cut::Swallow(6.0), Fill::solid(fill_c))
                .h(px(18.0))
                .pl(px(8.0))
                .pr(px(14.0))
                .gap(px(6.0))
                .text_color(rgb(ink_c))
                .child(glyph(&self.snap.glyph, ink_c, fill_c))
                .child(theme::metered(&status, 1.4, ink_c))
                .into_any_element()
        } else {
            // Her dot red, the word text-dim (r21: red on red on red; nothing stood out).
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_color(rgb(pal.dim))
                .when(!self.parked, |d| d.child(glyph(&self.snap.glyph, pal.accent, pal.panel)))
                .child(theme::metered(&status, 1.4, pal.dim))
                .into_any_element()
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .px(px(PAD))
            .h(px(30.0))
            .border_b_1()
            .border_color(rgb(pal.rule))
            .text_size(px(12.0))
            .font_weight(FontWeight::BOLD)
            .child(
                theme::plate(Cut::Tab(7.0), Fill::solid(if self.parked { pal.dim } else { pal.accent }))
                    .ml(px(6.0))
                    .h(px(18.0))
                    .pl(px(10.0))
                    .pr(px(12.0))
                    .text_color(rgb(pal.ink))
                    .child(tracked(&self.name.to_uppercase(), 1.6)),
            )
            .child(div().text_color(rgb(pal.dim)).child(tracked(if self.nav.page == Page::Settings { "SETTINGS" } else { "LINE" }, 1.6)))
            .child(div().flex_1())
            .child(state);
        let on_settings = self.nav.page == Page::Settings;
        let rows: Vec<AnyElement> = if on_settings { Vec::new() } else { self.snap.rows.iter().enumerate().map(|(i, r)| self.row_el(i, r, cx)).collect() };
        // At the bottom the thread starts at a whole row: its height is cut to the rows that
        // fit (measured on the last layout; one more frame when that changes).
        let fit = if on_settings {
            self.fit_h
        } else {
            let top = self.scroll.bounds().top();
            let items: Vec<Bounds<Pixels>> = (0..).map_while(|i| self.scroll.bounds_for_item(i)).collect();
            items.last().and_then(|last| {
                let total = f32::from(last.bottom() - top) + 10.0;
                (total > self.max_thread)
                    .then(|| items.iter().map(|b| f32::from(b.top() - top)).find(|c| total - (c - 6.0) <= self.max_thread).map(|c| total - c + 6.0))
                    .flatten()
            })
        };
        if fit != self.fit_h {
            self.fit_h = fit;
            if !self.pinned_up {
                self.scroll.scroll_to_bottom();
            }
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        // Scrolled up mid-row: the top edge fades out instead of cutting a line in half.
        let at_bottom = self.scroll.offset().y <= -self.scroll.max_offset().y + px(1.0);
        let scrolled = self.scroll.offset().y < px(-1.0) && !at_bottom;
        let thread = (!rows.is_empty()).then(|| {
            let fade = linear_gradient(180.0, linear_color_stop(rgb(pal.panel), 0.0), linear_color_stop(theme::alpha(pal.panel, 0.0), 1.0));
            div()
                .relative()
                .child(
                    div()
                        .id("thread")
                        .max_h(px(self.fit_h.unwrap_or(self.max_thread)))
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
        let prompt = (self.mode == Mode::Prompt && !on_settings).then(|| self.prompt_el(window, cx)).flatten();
        let pending = ((self.mode == Mode::Text || on_settings) && !self.snap.prompts.is_empty()).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(PAD))
                .py(px(4.0))
                .bg(rgb(pal.accent))
                .text_color(rgb(pal.ink))
                .text_size(px(12.0))
                .font_weight(FontWeight::BOLD)
                .child(glyph("bang", pal.ink, pal.accent))
                .child(tracked(&format!("{} PROMPT{} WAITING · TAB TO ANSWER", self.snap.prompts.len(), if self.snap.prompts.len() > 1 { "S" } else { "" }), 1.0))
        });
        // A text box you can't miss: filled, cut 9 px, edged in her red while it has the keyboard.
        let typing = self.mode == Mode::Text && !self.parked;
        let input = (self.mode == Mode::Text && !on_settings).then(|| {
            // [cog] [field]: the cog as tall as the field (the row stretches it), beside it.
            let field_h = self.field_h.clone();
            div().px(px(PAD)).pt(px(10.0)).border_t_1().border_color(rgb(pal.rule)).flex().gap(px(8.0)).child(self.cog_el(cx)).child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .relative()
                    .child(canvas(move |b, _, _| field_h.set(f32::from(b.size.height)), |_, _, _, _| {}).absolute().size_full())
                    .child(theme::backdrop(Cut::Corners(9.0), if typing { Fill::framed(pal.field, pal.accent, 2.0) } else { Fill::framed(pal.field, pal.dim, 1.0) }))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .child({
                        // The prompt mark: a drawn chevron (the font's › sits small and high).
                        let c = if typing { pal.accent } else { pal.dim };
                        canvas(
                            |_, _, _| (),
                            move |b, _, window, _| {
                                let (x, y) = (f32::from(b.left()) + 1.0, f32::from(b.top()) + f32::from(b.size.height) / 2.0);
                                theme::fill_poly(window, &[(x, y - 6.0), (x + 3.5, y - 6.0), (x + 9.0, y), (x + 3.5, y + 6.0), (x, y + 6.0), (x + 5.5, y)], rgb(c));
                            },
                        )
                        .flex_none()
                        .mt(px(PROMPT_DROP))
                        .w(px(10.0))
                        .h(px(LINE_H))
                    })
                    .child(
                        div().flex_1().min_w(px(0.0)).child(
                            Textarea::new(&self.input).appearance(false).text_size(px(FONT)).line_height(px(LINE_H)).text_color(rgb(pal.text)),
                        ),
                    ),
            )
        });
        // Voice: the mic row while he holds the key (a solid red plate, ink type, its bone rec
        // dot blinking at 1 Hz); the caption while she speaks (a red bar, bone words).
        let blink = self.snap.mic.is_some() && (self.born.elapsed().as_millis() / 500) % 2 == 1;
        if self.snap.mic.is_some() && !self.blink_armed {
            self.blink_armed = true;
            let left = 500 - (self.born.elapsed().as_millis() % 500) as u64;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(left + 2)).await;
                this.update(cx, |l, cx| {
                    l.blink_armed = false;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let voice_row = self.snap.mic.clone().map(|m| {
            // `● MIC 0:04 ▮▮▯▯▯  ·  listening · release to send`: the dot is drawn, the rest
            // says what follows key-up.
            let m = m.trim_start_matches(['●', '◉']).trim_start().to_string();
            let (label, hint) = m.split_once("  ·  ").map_or((m.clone(), String::new()), |(a, b)| (a.to_string(), b.to_string()));
            let mark = if label.starts_with("HEARD") {
                "dots"
            } else if label.contains("HANDS-FREE") {
                "hf"
            } else {
                "rec"
            };
            let mark_c = if blink && mark != "dots" { theme::mix(pal.text, pal.accent, 0.85) } else { pal.text };
            div()
                .mx(px(PAD))
                .mt(px(6.0))
                .px(px(10.0))
                .py(px(7.0))
                .relative()
                .child(theme::backdrop(Cut::Corners(8.0), Fill::solid(pal.accent).ringed(pal.ring)))
                .flex()
                .items_center()
                .gap(px(10.0))
                .text_size(px(14.0))
                .line_height(px(18.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(pal.ink))
                .child(glyph(mark, mark_c, pal.accent))
                .child(theme::metered(&label, 0.6, pal.ink))
                .child(div().flex_1().min_w(px(0.0)).flex().justify_end().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).overflow_hidden().whitespace_nowrap().child(hint))
        });
        let caption = self.snap.caption.clone().filter(|_| !on_settings).map(|c| {
            div()
                .mx(px(PAD))
                .mt(px(6.0))
                .px(px(10.0))
                .py(px(6.0))
                .border_l_3()
                .border_color(rgb(pal.accent))
                .flex()
                .gap(px(10.0))
                .text_size(px(FONT))
                .child(div().flex_none().pt(px(6.0)).child(glyph("play", pal.accent, pal.panel)))
                .child(div().flex_1().text_color(rgb(pal.text)).child(c))
        });
        // A failed turn's tip stands alone: the legend would bury it.
        let tip = self.tip.clone().filter(|_| self.note.is_none());
        let hint = if on_settings {
            Some(settings::hint(&self.snap.prefs.clone().unwrap_or_default(), self.sel).into())
        } else if self.mode == Mode::Prompt || tip.is_some() {
            None
        } else if self.snap.caption.is_some() {
            Some("esc or $mod+c hush · the whole reply is in the thread".into())
        } else if !self.snap.live {
            Some("OFFLINE · not sent while offline · draft is kept".to_string())
        } else if self.snap.busy {
            Some("⏎ steer · esc esc abort · ctrl+y copy reply · pgup thread · esc close".into())
        } else {
            Some("⏎ send · ⇧⏎ newline · ↑ last message · ctrl+y copy reply · ctrl+o open link · esc close".into())
        };
        // A long note puts the hints on a row of their own, not past the edge.
        let footer = div()
            .flex()
            .flex_wrap()
            .gap_x(px(10.0))
            .gap_y(px(2.0))
            .px(px(PAD))
            .pt(px(4.0))
            .pb(px(8.0))
            .text_size(px(11.5))
            .children(self.note.clone().map(|n| div().text_color(rgb(pal.accent)).font_weight(FontWeight::BOLD).child(n)))
            .children(tip.map(|n| div().text_size(px(13.0)).text_color(rgb(pal.text)).child(n)))
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
            .min_h(px(self.keep_h))
            .child(header)
            // Kept taller than its thread (after Settings): the room is under the header, the
            // thread stays on the field.
            .when(!on_settings, |d| d.child(div().flex_1()))
            .children(thread)
            .children(pending)
            .children(prompt)
            .children(caption)
            .children(voice_row)
            .children(input)
            .children(if on_settings { self.settings_el(cx) } else { Vec::new() })
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
                                    // The ring and the frame, top and bottom.
                                    (l.on_ask)(Ask::Height(h + 2.0 * INSET), cx).ok();
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
        // The page swap: the RGB split for its two frames, the cog's turn (a frame each until
        // done; nothing after).
        let now = Instant::now();
        let (split, owed) = self.swap_split(now);
        let (_, turning) = self.cog_turn(now);
        if owed || turning {
            window.request_animation_frame();
        }
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
                        this.unpark(window, cx).ok();
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
            // Outside the panel's cuts: the dark ring. Parked: a text-dim frame.
            .bg(rgb(pal.ring))
            .child(panel_backdrop(pal, self.parked))
            .child(div().absolute().top(px(INSET)).left(px(INSET)).right(px(INSET)).bottom(px(INSET)).overflow_hidden().child(content))
            .children((split > 0.0).then(|| Self::swap_overlay(split)))
            .children(self.tail())
    }
}


/// `state`'s word for a panel's side of her.
pub fn side_key(s: crate::beside::Side) -> &'static str {
    match s {
        crate::beside::Side::Left => "left",
        crate::beside::Side::Right => "right",
        crate::beside::Side::Above => "above",
    }
}

/// The panel cut inside its window (the Line's, the reader's): the dark ring left round it,
/// the 1 px dark-red frame (text-dim when parked; r21: a 2 px red one made everything red, so
/// nothing stood out), the blade notch with three mane slivers in her red.
pub fn panel_backdrop(pal: Pal, parked: bool) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let (x0, y0, x1, y1) = (f32::from(b.left()) + 1.0, f32::from(b.top()) + 1.0, f32::from(b.right()) - 1.0, f32::from(b.bottom()) - 1.0);
            let (frame, sliver) = if parked { (pal.dim, pal.dim) } else { (pal.border, pal.accent) };
            theme::paint(window, &theme::outline(Cut::Panel, x0, y0, x1, y1), Fill::framed(pal.panel, frame, 1.0));
            if x1 - x0 >= 300.0 {
                for p in theme::slivers(x1 - 100.0, y0, 52.0, theme::NOTCH) {
                    theme::fill_poly(window, &p, rgb(sliver));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// A panel's edge facing her, thick along its straight run (between the cuts, in `edge_c`),
/// with a pointer at her `at` along it (logical px, in `point_c`): the Line's and the
/// reader's. `side`: the panel's side of her (Right: she is at its left edge).
pub fn tail_overlay(side: crate::beside::Side, at: f32, edge_c: u32, point_c: u32) -> AnyElement {
    use crate::beside::Side;
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            // The panel inside the ring.
            let (l, t, r, bt) = (f32::from(b.left()) + 1.0, f32::from(b.top()) + 1.0, f32::from(b.right()) - 1.0, f32::from(b.bottom()) - 1.0);
            let big = theme::BIG_CUT.min((bt - t) * 0.2);
            let sm = theme::SMALL_CUT;
            // Its point stops short of the rows' padding (r35: it lay over her face tile).
            let (w, half, depth) = (3.0f32, 12.0f32, 8.0f32);
            let (edge, notch): (Vec<theme::Pt>, Vec<theme::Pt>) = match side {
                Side::Right | Side::Left => {
                    // Left edge: from the big cut down to the small one; right: the other way.
                    let (a, z) = if side == Side::Right { (t + big, bt - sm) } else { (t + sm, bt - big) };
                    let y = (t + at).min(z - half).max(a + half);
                    let (x, inward) = if side == Side::Right { (l, 1.0) } else { (r, -1.0) };
                    (
                        vec![(x, a), (x + inward * w, a + w), (x + inward * w, z - w), (x, z)],
                        vec![(x + inward * (w + depth), y - half), (x, y), (x + inward * (w + depth), y + half)],
                    )
                }
                Side::Above => {
                    let (a, z) = (l + sm, r - big);
                    let x = (l + at).min(z - half).max(a + half);
                    (vec![(a, bt), (a + w, bt - w), (z - w, bt - w), (z, bt)], vec![(x - half, bt - w - depth), (x, bt), (x + half, bt - w - depth)])
                }
            };
            theme::fill_poly(window, &edge, rgb(edge_c));
            theme::fill_poly(window, &notch, rgb(point_c));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{GONE, NO_KEYBOARD, command, keyboard_refused, links, risky};
    use crate::session::PromptView;

    fn confirm(title: &str, text: &str) -> PromptView {
        PromptView { id: 1, title: title.into(), text: text.into(), secret: false }
    }

    #[test]
    fn a_line_that_is_one_command_is_a_chip() {
        assert_eq!(command("`synaps login`"), Some("synaps login"));
        assert_eq!(command("`synaps lo"), Some("synaps lo"), "typing out");
        assert_eq!(command("Run `synaps login` now"), None);
        assert_eq!(command("`a` and `b`"), None);
        assert_eq!(command("``"), None);
        assert_eq!(command("`"), None);
    }

    #[test]
    fn only_a_refused_keyboard_leaves_the_line_waiting() {
        assert!(keyboard_refused(&format!("{NO_KEYBOARD}: SetForegroundWindow: the foreground lock")));
        assert!(!keyboard_refused(GONE), "she is gone: no wait, its own reply");
        assert!(!keyboard_refused(""));
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
