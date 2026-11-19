//! The Line's second page: her Settings (SPEC-settings §1–2; PR 1 the shell, PR 3 her brain). The
//! cog left of the field (or `ctrl+,`) flips the Line between chat and Settings; Esc goes back one
//! step. Settings' BRAIN row opens the Brain screen (`Page::Brain`): the ways to give her one
//! (Claude first, the rest one step away) and MODEL. With no brain her summon opens it.
//! Every value is a `settings.toml` key the avatar writes (`Settings::write`) and applies
//! (`settings_reload`): the Line only shows what she runs with ([`Prefs`]) and asks for a
//! change ([`Pref`]).

use super::{Ask, Line, Mode, PAD};
use crate::provider::{self, Kind, Provider};
use crate::settings::{Attention, Edit, Settings};
use crate::theme::{self, Cut, Fill, tracked};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::{Duration, Instant};

/// The Line's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Chat,
    Settings,
    /// Settings ▸ Brain: the ways to give her one, and MODEL.
    Brain,
}

impl Page {
    pub fn key(self) -> &'static str {
        match self {
            Page::Chat => "chat",
            Page::Settings => "settings",
            Page::Brain => "brain",
        }
    }

    /// One of Settings' pages (the list or the Brain screen): no field, its keys stop there.
    pub fn settings(self) -> bool {
        self != Page::Chat
    }
}

/// The page shown and where Esc goes back to, one step at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    pub page: Page,
    back: Vec<Page>,
    /// The page the last change left.
    pub left: Page,
}

impl Nav {
    /// Opened on `page` (the avatar's SETTINGS row): Esc from there goes to chat.
    pub fn on(page: Page) -> Nav {
        Nav { page, back: Vec::new(), left: Page::Chat }
    }

    /// To `p`, remembering where it came from. false: already there.
    pub fn go(&mut self, p: Page) -> bool {
        if p == self.page {
            return false;
        }
        self.back.push(self.page);
        self.left = std::mem::replace(&mut self.page, p);
        true
    }

    /// The cog (`ctrl+,`): Settings from chat; from Settings straight back to chat.
    pub fn cog(&mut self) {
        if self.page == Page::Chat {
            self.go(Page::Settings);
        } else {
            self.back.clear();
            self.left = std::mem::replace(&mut self.page, Page::Chat);
        }
    }

    /// Esc: one step back. false: nowhere back (chat), Esc does what it does there.
    pub fn esc(&mut self) -> bool {
        let to = match self.back.pop() {
            Some(p) => p,
            None if self.page != Page::Chat => Page::Chat,
            None => return false,
        };
        self.left = std::mem::replace(&mut self.page, to);
        true
    }
}

/// What the Settings page shows: the values as asked (a change not written yet: that one,
/// [`asked`]), else as she runs them. Her menu shows and steps from the same.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// `[brain] attention`.
    pub attention: Attention,
    /// Her brain runs (`[brain] enabled`): ATTENTION is locked while it doesn't, and reads OFF.
    pub brain: bool,
    /// `[voice] wake_word`.
    pub wake: bool,
    /// `[voice] enabled`: "HEY CAPRI" is dim while it is off.
    pub voice: bool,
    /// `CAPRI 1.0.0 · SYNAPS 0.12.0 · SONNET 4.6`.
    pub about: String,
    /// Her BRAIN: the model she thinks with and whether it has a login (`brain` above is her
    /// behaviour engine's switch).
    pub mind: provider::Status,
    /// Her one line: a change made, or why it wasn't.
    pub says: Option<String>,
    /// Replies that came while Settings was up (unseen: `N NEW`).
    pub new: u32,
}

/// A change he asks for (on the page, or her menu's ATTENTION row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    Attention(Attention),
    Wake(bool),
    /// Her own session's model (`[session] model`), always written out: a removed key wouldn't
    /// move a live session back. One of `provider::ALL`'s models.
    Model(&'static str),
}

impl Pref {
    /// Its one key in settings.toml.
    pub fn edit(self) -> Edit {
        match self {
            Pref::Attention(a) => Edit::Set("brain", "attention", a.key().into()),
            Pref::Wake(on) => Edit::Set("voice", "wake_word", on.into()),
            Pref::Model(m) => Edit::Set("session", "model", m.into()),
        }
    }

    /// Its key in `s`, as the file has it once written.
    pub fn apply(self, s: &mut Settings) {
        match self {
            Pref::Attention(a) => s.brain.attention = a,
            Pref::Wake(on) => s.voice.wake_word = on,
            Pref::Model(m) => s.session.model = Some(m.to_string()),
        }
    }

    /// Its value in what the page shows (asked: the page steps on from it at once).
    pub fn show(self, p: &mut Prefs) {
        match self {
            Pref::Attention(a) => p.attention = a,
            Pref::Wake(on) => p.wake = on,
            Pref::Model(m) => p.mind.model = Some(m.to_string()),
        }
    }
}

/// Attention and the wake word as asked: the last change of each not written yet, else as
/// she runs them (`attention`, `wake`). The page and her menu both show and step from these,
/// so two quick changes before the first write is in go two steps, wherever they came from.
pub fn asked<'a>(pending: impl Iterator<Item = &'a Pref>, attention: Attention, wake: bool) -> (Attention, bool) {
    pending.fold((attention, wake), |(a, w), p| match *p {
        Pref::Attention(x) => (x, w),
        Pref::Wake(x) => (a, x),
        Pref::Model(_) => (a, w),
    })
}

/// Her model as asked: the last MODEL change not written yet, else `running`.
pub fn asked_model<'a>(pending: impl Iterator<Item = &'a Pref>, running: Option<&str>) -> Option<String> {
    pending.fold(running.map(str::to_string), |m, p| match *p {
        Pref::Model(x) => Some(x.to_string()),
        _ => m,
    })
}

/// The next attention for her menu's row: on from the one asked ([`asked`]).
pub fn next_attention<'a>(pending: impl Iterator<Item = &'a Pref>, running: Attention) -> Attention {
    asked(pending, running, false).0.next()
}

/// Her menu's ATTENTION row, under its value: why its last click didn't stick (the write was
/// refused, or hangs). Until an attention write lands (a reload keeps it: [`MenuSave::reloaded`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuSave {
    #[default]
    Fine,
    Refused,
    Stuck,
}

impl MenuSave {
    /// A write is done (`ok`: it is in), its change from the menu or not.
    pub fn written(self, pref: Pref, menu: bool, ok: bool) -> MenuSave {
        match (pref, ok) {
            (Pref::Attention(_), true) => MenuSave::Fine,
            (_, false) if menu => MenuSave::Refused,
            _ => self,
        }
    }

    /// A write hangs; `menu`: a change of the menu's is among those waiting.
    pub fn stuck(self, menu: bool) -> MenuSave {
        if menu && self == MenuSave::Fine { MenuSave::Stuck } else { self }
    }

    /// A `settings reload`: a refusal stands (until an attention write lands), and so does a
    /// hang while a change of the menu's is still waiting (`menu`).
    pub fn reloaded(self, menu: bool) -> MenuSave {
        match self {
            MenuSave::Stuck if !menu => MenuSave::Fine,
            s => s,
        }
    }

    /// The row's second line while her menu is open (`kept`: what it said in this opening):
    /// as said; gone meanwhile (its write landed), kept unseen as room, so the rows under it
    /// don't move up under his next click. The text, and whether it shows.
    pub fn line(self, kept: &mut Option<&'static str>) -> Option<(&'static str, bool)> {
        match self.says() {
            Some(t) => {
                *kept = Some(t);
                Some((t, true))
            }
            None => kept.map(|t| (t, false)),
        }
    }

    /// The row's second line.
    pub fn says(self) -> Option<&'static str> {
        match self {
            MenuSave::Fine => None,
            MenuSave::Refused => Some("CAN'T SAVE · settings.toml"),
            MenuSave::Stuck => Some("NOT SAVED YET · settings.toml"),
        }
    }
}

/// The page's rows, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetRow {
    /// Her brain: what she thinks with, and the way to the Brain screen.
    Brain,
    Attention,
    Wake,
    About,
}

pub fn set_rows(_: &Prefs) -> Vec<SetRow> {
    vec![SetRow::Brain, SetRow::Attention, SetRow::Wake, SetRow::About]
}

const ATTENTION: [Attention; 3] = [Attention::Mix, Attention::HangOnly, Attention::Off];

/// What a key on the list (or the Brain screen) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Nothing,
    Pref(Pref),
    /// ABOUT's COPY: the versions, for a bug report.
    Copy,
    /// To a page (BRAIN's ⏎: the Brain screen).
    Go(Page),
    /// OTHER opened (true) or closed in place.
    Other(bool),
    /// Signing in to this provider (None: an API key, Synaps' list of services). Until the
    /// sign-in runs in her (SPEC-settings PR 4): the command that does it, copied, for a terminal.
    Login(Option<&'static Provider>),
    /// RUN LOCAL. Until her Ollama setup (PR 5): the steps, opened.
    Local,
}

/// The Brain screen's rows, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainRow {
    /// SIGN IN WITH CLAUDE: the first way in.
    Claude,
    /// RUN LOCAL: a model on his machine.
    Local,
    /// OTHER: the other accounts and API KEY, opened in place.
    Other,
    /// One of OTHER's accounts (`provider::others`, its index).
    Account(usize),
    /// API KEY: one of Synaps' API-key services.
    Key,
    /// MODEL: her provider's models.
    Model,
}

/// The Brain screen's rows (`other`: OTHER is open). MODEL once she has a brain to pick a model
/// of (a login, or a local model).
pub fn brain_rows(p: &Prefs, other: bool) -> Vec<BrainRow> {
    let mut v = vec![BrainRow::Claude, BrainRow::Local, BrainRow::Other];
    if other {
        v.extend((0..p.mind.others().len()).map(BrainRow::Account));
        v.push(BrainRow::Key);
    }
    if p.mind.model.is_some() && !p.mind.none() {
        v.push(BrainRow::Model);
    }
    v
}

/// The Brain screen's row `sel` once its rows change from `old`'s to `new`'s (a look found
/// Gemini's client id or a login, her brain changed): the same row, the same provider, where it
/// is now. A row gone: the same place, kept in the list.
pub fn brain_reselect(old: &Prefs, new: &Prefs, sel: usize, other: bool) -> usize {
    let id = |p: &Prefs, r: BrainRow| match r {
        BrainRow::Account(i) => (BrainRow::Account(0), p.mind.others().get(i).map(|a| a.id)),
        r => (r, None),
    };
    let now = brain_rows(new, other);
    let keep = sel.min(now.len() - 1);
    match brain_rows(old, other).get(sel) {
        Some(&r) => {
            let was = id(old, r);
            now.iter().position(|&x| id(new, x) == was).unwrap_or(keep)
        }
        None => keep,
    }
}

/// A provider's row: what ⏎ on it does. One with a login on file, not hers: switch her to it
/// (its default model). Without one: sign in (not for a daemon on another machine: a sign-in
/// here wouldn't reach it). Hers, signed in: nothing.
fn use_provider(p: &Prefs, to: &'static Provider) -> Act {
    let m = &p.mind;
    let hers = m.provider() == Some(to);
    let signed = m.signed.contains(&to.id) || (hers && m.signed_in == Some(true));
    match (signed, m.here) {
        (true, _) if hers => Act::Nothing,
        (true, _) if m.lock.is_none() => to.models.first().map_or(Act::Nothing, |d| Act::Pref(Pref::Model(d.0))),
        (false, true) => Act::Login(Some(to)),
        _ => Act::Nothing,
    }
}

/// The key services with a key here (`key_word`'s), in the table's order: hers counts when
/// she has a login on it.
fn keys_set(m: &provider::Status) -> Vec<&'static Provider> {
    provider::ALL.iter().filter(|q| q.kind == Kind::Key && (m.signed.contains(&q.id) || (m.provider() == Some(*q) && m.signed_in == Some(true)))).collect()
}

/// API KEY's ⏎ (not under `--model`/`--session`): with keys set here, switch her to one (its
/// default model): the first, or the one after hers when she's on one of several. On the only
/// one, none set, or locked: on this machine, Synaps' list of services, to set one; a daemon on
/// another machine: nothing (a key here wouldn't reach it).
fn key_act(p: &Prefs) -> Act {
    let m = &p.mind;
    let set = keys_set(m);
    let to = match set.iter().position(|q| m.provider() == Some(*q)) {
        _ if m.lock.is_some() => None,
        None => set.first(),
        Some(i) if set.len() > 1 => set.get((i + 1) % set.len()),
        Some(_) => None,
    };
    match to.and_then(|q| q.models.first()) {
        Some(d) => Act::Pref(Pref::Model(d.0)),
        None if m.here => Act::Login(None),
        None => Act::Nothing,
    }
}

/// MODEL stepped by `d` from the one in force: the next one he can pick (his own, set by hand,
/// is shown but not picked again). Locked (`--model`, `--session`): nothing.
fn step_model(p: &Prefs, d: i32) -> Act {
    if p.mind.lock.is_some() {
        return Act::Nothing;
    }
    let ch = p.mind.choices();
    if ch.len() < 2 {
        return Act::Nothing;
    }
    let cur = ch.iter().position(|c| c.on).unwrap_or(0) as i32;
    let n = ch.len() as i32;
    (1..n)
        .map(|k| &ch[(cur + d * k).rem_euclid(n) as usize])
        .find_map(|c| c.pick.filter(|_| !c.on))
        .map_or(Act::Nothing, |m| Act::Pref(Pref::Model(m)))
}

/// A key on the Brain screen at row `sel` (`other`: OTHER is open): the selection after it, and
/// what it does. ↑↓ move, ⏎ (→, Space) takes a way in, ←→ change MODEL, ← closes OTHER.
pub fn brain_key(key: &str, sel: usize, p: &Prefs, other: bool) -> (usize, Act) {
    let rows = brain_rows(p, other);
    let sel = sel.min(rows.len() - 1);
    let go = matches!(key, "enter" | "right" | "space");
    let act = match (key, rows[sel]) {
        ("up", _) => return (sel.saturating_sub(1), Act::Nothing),
        ("down", _) => return ((sel + 1).min(rows.len() - 1), Act::Nothing),
        ("left", BrainRow::Model) => step_model(p, -1),
        ("right" | "space" | "enter", BrainRow::Model) => step_model(p, 1),
        ("right", BrainRow::Other) => Act::Other(true),
        ("enter" | "space", BrainRow::Other) => Act::Other(!other),
        ("left", BrainRow::Other | BrainRow::Account(_) | BrainRow::Key) if other => {
            let at = rows.iter().position(|r| *r == BrainRow::Other).unwrap_or(sel);
            return (at, Act::Other(false));
        }
        (_, BrainRow::Claude) if go => use_provider(p, &provider::CLAUDE),
        (_, BrainRow::Account(i)) if go => p.mind.others().get(i).map_or(Act::Nothing, |&to| use_provider(p, to)),
        (_, BrainRow::Key) if go => key_act(p),
        (_, BrainRow::Local) if go && p.mind.here => Act::Local,
        _ => Act::Nothing,
    };
    (sel, act)
}

/// A key on the Settings list at row `sel`: the selection after it, and what it does. ↑↓ move,
/// ←→ (or Space) change the row's value (it is written at once, no SAVE), ⏎ acts (a toggle
/// flips, ABOUT copies).
pub fn list_key(key: &str, sel: usize, p: &Prefs) -> (usize, Act) {
    let rows = set_rows(p);
    let sel = sel.min(rows.len() - 1);
    let step = |a: Attention, d: i32| {
        let i = ATTENTION.iter().position(|&x| x == a).unwrap_or(0) as i32;
        ATTENTION[(i + d).rem_euclid(3) as usize]
    };
    let act = match (key, rows[sel]) {
        ("up", _) => return (sel.saturating_sub(1), Act::Nothing),
        ("down", _) => return ((sel + 1).min(rows.len() - 1), Act::Nothing),
        // Each locked while what it tunes is off ([brain] / [voice] enabled: his, in the file).
        ("left", SetRow::Attention) if p.brain => Act::Pref(Pref::Attention(step(p.attention, -1))),
        ("right" | "space" | "enter", SetRow::Attention) if p.brain => Act::Pref(Pref::Attention(step(p.attention, 1))),
        ("left" | "right" | "space" | "enter", SetRow::Wake) if p.voice => Act::Pref(Pref::Wake(!p.wake)),
        ("enter", SetRow::About) => Act::Copy,
        ("right" | "space" | "enter", SetRow::Brain) => Act::Go(Page::Brain),
        _ => Act::Nothing,
    };
    (sel, act)
}

/// The footer's keys for the row picked (⏎ copy only where it copies).
pub fn hint(p: &Prefs, sel: usize) -> &'static str {
    let rows = set_rows(p);
    match rows[sel.min(rows.len() - 1)] {
        SetRow::Brain => "↑↓ pick · ⏎ open · esc chat",
        SetRow::Attention if p.brain => "↑↓ pick · ←→ change · esc chat",
        SetRow::Wake if p.voice => "↑↓ pick · ←→ change · esc chat",
        SetRow::Attention | SetRow::Wake => "↑↓ pick · esc chat",
        SetRow::About => "↑↓ pick · ⏎ copy · esc chat",
    }
}

/// The Brain screen's keys for the row picked: what ⏎ does there (copies a sign-in, switches
/// her, opens or closes OTHER), ← where it closes OTHER, nothing where ⏎ does nothing.
pub fn brain_hint(p: &Prefs, sel: usize, other: bool) -> &'static str {
    let rows = brain_rows(p, other);
    let row = rows[sel.min(rows.len() - 1)];
    let inside = other && matches!(row, BrainRow::Account(_) | BrainRow::Key);
    // On a key service with its key (`keys_set`), ⏎ steps on; on one without, it's a switch.
    let on_key = p.mind.provider().is_some_and(|q| keys_set(&p.mind).contains(&q));
    match (row, brain_key("enter", sel, p, other).1) {
        (BrainRow::Model, _) if step_model(p, 1) != Act::Nothing => "↑↓ pick · ←→ change · esc back",
        (BrainRow::Model, _) => "↑↓ pick · esc back",
        (BrainRow::Other, _) if other => "↑↓ pick · ⏎ close · esc back",
        (BrainRow::Other, _) => "↑↓ pick · ⏎ open · esc back",
        (_, Act::Login(_)) if inside => "↑↓ pick · ⏎ copy the sign-in · ← close · esc back",
        (BrainRow::Key, Act::Pref(_)) if inside && on_key => "↑↓ pick · ⏎ the next key · ← close · esc back",
        (_, Act::Pref(_)) if inside => "↑↓ pick · ⏎ use it · ← close · esc back",
        (_, _) if inside => "↑↓ pick · ← close · esc back",
        (_, Act::Login(_)) => "↑↓ pick · ⏎ copy the sign-in · esc back",
        (BrainRow::Key, Act::Pref(_)) if on_key => "↑↓ pick · ⏎ the next key · esc back",
        (_, Act::Pref(_)) => "↑↓ pick · ⏎ use it · esc back",
        (_, Act::Local) => "↑↓ pick · ⏎ the steps · esc back",
        _ => "↑↓ pick · esc back",
    }
}

/// A held ALLOW fires at HOLD only if it is still that hold on that prompt, and the prompt is on
/// screen: chat, in prompt mode (a page swap, or Tab to the field, lets go of it; #74: nothing
/// approved unseen).
pub(super) fn hold_fires(hold_at: Option<Instant>, started: Instant, page: Page, mode: Mode, same_prompt: bool) -> bool {
    hold_at == Some(started) && same_prompt && page == Page::Chat && mode == Mode::Prompt
}

/// A note on Settings (COPIED, HUSHED) goes by itself after this: the page has no field to
/// type in, which is what clears one on chat.
pub(super) const FLASH: Duration = Duration::from_millis(1500);

/// Her note in place of the keys (COPIED, ESC AGAIN TO ABORT, NOT SENT · … · text kept): said
/// on a page and shown only there; on Settings it goes after [`FLASH`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Note {
    text: Option<String>,
    page: Page,
    until: Option<Instant>,
}

impl Note {
    /// Said on `page` at `now`; on Settings, when it goes (its timer calls [`Note::expire`]).
    pub fn say(&mut self, page: Page, text: impl Into<String>, now: Instant) -> Option<Instant> {
        let until = page.settings().then(|| now + FLASH);
        *self = Note { text: Some(text.into()), page, until };
        until
    }

    pub fn clear(&mut self) {
        *self = Note::default();
    }

    /// What `page` shows at `now`.
    pub fn on(&self, page: Page, now: Instant) -> Option<&str> {
        self.text.as_deref().filter(|_| self.page == page && self.until.is_none_or(|u| now < u))
    }

    /// Its timer is up: gone if it is still that note (true: redraw).
    pub fn expire(&mut self, until: Instant) -> bool {
        let gone = self.text.is_some() && self.until == Some(until);
        if gone {
            self.clear();
        }
        gone
    }
}

/// The page changed (to `to`): what was said on the other one goes, and a pending Esc (the one
/// that closes the Line a beat later unless a second aborts) with it.
pub(super) fn page_left(note: &mut Note, esc_at: &mut Option<Instant>, to: Page) {
    if note.page != to {
        note.clear();
    }
    *esc_at = None;
}

/// One Esc during a turn (at `armed`) closes the Line a beat later: only if it is still that
/// Esc, on chat.
pub(super) fn esc_closes(esc_at: Option<Instant>, armed: Instant, page: Page) -> bool {
    esc_at == Some(armed) && page == Page::Chat
}

/// The text field's height (one line) until it has been laid out: Settings' bar is as tall as
/// the field's, so the swap moves nothing under it.
pub(super) const FIELD_H0: f32 = 52.0;
const COG_W: f32 = 34.0;
/// `◂ CHAT`, in the cog's spot on Settings.
const BACK_W: f32 = 84.0;
/// Her line's slot on the Settings page.
const QUOTE_H: f32 = 28.0;
/// Chat's row of keys under its field: the room over and under its line.
pub(super) const KEYS_PT: f32 = 4.0;
pub(super) const KEYS_PB: f32 = 8.0;
/// Under `◂ CHAT` in Settings' bar.
const BAR_PB: f32 = 8.0;
/// Under Settings' bar, always: chat's row of keys less the bar's own room under `◂ CHAT`, so
/// `◂ CHAT` sits where the cog was, whichever page is the taller (r62 gave it only the room
/// to spare: with Settings the taller, as on a first run's empty chat, it sat 27 px low).
const BAR_DROP: f32 = KEYS_PT + super::LINE_H + KEYS_PB - BAR_PB;

/// How far over the panel's bottom the cog (chat, its keys on `key_rows` rows) or `◂ CHAT`
/// (Settings) sits: the panel keeps its bottom edge, so this is where it is on screen. From
/// the constants both pages are laid out with (on screen: `state`'s `spot`).
#[cfg(test)]
fn spot_lift(page: Page, key_rows: usize) -> f32 {
    match page {
        Page::Chat => KEYS_PT + key_rows as f32 * super::LINE_H + key_rows.saturating_sub(1) as f32 * KEYS_GAP + KEYS_PB,
        Page::Settings | Page::Brain => BAR_PB + BAR_DROP,
    }
}

/// Between chat's rows of keys, when a long note puts them on two.
pub(super) const KEYS_GAP: f32 = 2.0;
/// A row's height, a sub-line ("voice is off") inside it: the page is as tall locked or not.
const ROW_H: f32 = 32.0;
/// The page's small type (a version, a lock, a reason, the hints): px.
pub(super) const SMALL: f32 = 12.0;

/// A gear (`theme::glyph`'s, 8 teeth) `size` px square, turned `turn` radians (a tooth: π/4).
fn gear(fg: u32, bg: u32, size: f32, turn: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let k = size / 11.0;
            let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let at = |x: f32, y: f32| (ox + x * k, oy + y * k);
            let teeth: Vec<theme::Pt> = (0..32)
                .map(|i| {
                    let a = (i as f32 + 0.5) / 32.0 * std::f32::consts::TAU + turn;
                    let r = if (i / 2) % 2 == 0 { 5.6 } else { 4.1 };
                    at(5.5 + r * a.cos(), 5.5 + r * a.sin())
                })
                .collect();
            theme::fill_poly(window, &teeth, rgb(fg));
            let hole: Vec<theme::Pt> = (0..16).map(|i| i as f32 / 16.0 * std::f32::consts::TAU).map(|a| at(5.5 + 1.8 * a.cos(), 5.5 + 1.8 * a.sin())).collect();
            theme::fill_poly(window, &hole, rgb(bg));
        },
    )
    .w(px(size))
    .h(px(size))
}

/// The swap's RGB split along the panel's steep edges (cyan out left, pink out right), `split`
/// px: the HUD's hard cut over the new page.
fn split_paint(window: &mut Window, polys: &[Vec<theme::Pt>], split: f32) {
    let d = split + theme::RING;
    for p in polys {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if (b.1 - a.1).abs() < 2.0 * (b.0 - a.0).abs() || (b.1 - a.1).abs() < 3.0 {
                continue;
            }
            for (dx, c) in [(-d, crate::fx::CYAN), (d, crate::fx::PINK)] {
                theme::fill_poly(window, &[a, b, (b.0 + dx, b.1), (a.0 + dx, a.1)], rgb(c));
            }
        }
    }
}

/// The cog turns one tooth this long when the page flips.
const TURN_MS: f32 = 120.0;
/// Where the GitHub button goes.
const REPO: &str = "https://github.com/HaseebKhalid1507/capri-syn";
/// RUN LOCAL's steps, until she sets Ollama up herself (SPEC-settings PR 5).
const LOCAL_STEPS: &str = "https://github.com/HaseebKhalid1507/capri-syn/blob/main/docs/UNDER-THE-HOOD.md#her-brain";

impl Line {
    /// The cog's turn now (radians), and whether it is still turning.
    pub(super) fn cog_turn(&self, now: Instant) -> (f32, bool) {
        match self.swap {
            Some(t0) if self.snap.motion => {
                let u = (now.saturating_duration_since(t0).as_secs_f32() * 1000.0 / TURN_MS).min(1.0);
                (crate::hud::ease(u) * std::f32::consts::FRAC_PI_4, u < 1.0)
            }
            _ => (0.0, false),
        }
    }

    /// The cog, left of the field: a chamfered shard as tall as it, a dim-red edge round a red
    /// gear, solid red on hover. On Settings its spot is `◂ CHAT` ([`Line::back_el`]).
    pub(super) fn cog_el(&self, cx: &Context<Line>) -> Stateful<Div> {
        let pal = self.pal;
        let (turn, _) = self.cog_turn(Instant::now());
        let cut = Cut::Corners(7.0);
        let center = || div().absolute().top_0().left_0().size_full().flex().items_center().justify_center();
        div()
            .id("cog")
            .group("cog")
            .relative()
            .flex_none()
            .w(px(COG_W))
            .cursor_pointer()
            .child(self.spot_probe())
            .child(theme::backdrop(cut, Fill::framed(pal.field, pal.border, 1.5)))
            .child(center().child(gear(pal.accent, pal.field, 15.0, turn)))
            .child(center().invisible().group_hover("cog", |s| s.visible()).child(theme::backdrop(cut, Fill::solid(pal.accent))).child(gear(pal.ink, pal.accent, 15.0, turn)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.nav.cog();
                    this.page_changed(window, cx);
                }),
            )
    }

    /// Where the cog or `◂ CHAT` is painted, for `state` (`spot`).
    fn spot_probe(&self) -> impl IntoElement {
        let spot = self.spot.clone();
        canvas(
            move |b, window, _| {
                let vh = f32::from(window.viewport_size().height);
                spot.set(Some([f32::from(b.left()), vh - f32::from(b.bottom()), f32::from(b.size.width), f32::from(b.size.height)]));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Settings' way back, in the cog's spot (he leaves where he came in): `◂ CHAT` on the list,
    /// `◂ BACK` on the Brain screen (a step back, as Esc), as tall as the cog and edged as it is,
    /// solid red on hover.
    pub(super) fn back_el(&self, cx: &Context<Line>) -> Stateful<Div> {
        let pal = self.pal;
        let cut = Cut::Corners(7.0);
        let word = if self.nav.page == Page::Brain { "BACK" } else { "CHAT" };
        let face = |fg: u32, bg: u32| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(7.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(fg))
                .child(theme::glyph_at("back", fg, bg, 10.0))
                .child(tracked(word, 1.4))
        };
        div()
            .id("set-chat")
            .group("set-chat")
            .relative()
            .flex_none()
            .w(px(BACK_W))
            .cursor_pointer()
            .child(self.spot_probe())
            .child(theme::backdrop(cut, Fill::framed(pal.field, pal.border, 1.5)))
            .child(face(pal.accent, pal.field))
            .child(div().absolute().top_0().left_0().size_full().invisible().group_hover("set-chat", |s| s.visible()).child(theme::backdrop(cut, Fill::solid(pal.accent))).child(face(pal.ink, pal.accent)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if this.nav.page == Page::Brain {
                        this.nav.esc();
                        this.page_changed(window, cx);
                    } else {
                        this.show(Page::Chat, window, cx);
                    }
                }),
            )
    }

    /// The page changed (the cog, `ctrl+,`, Esc, the avatar): the hard cut, the keyboard where
    /// the page types, and the avatar told.
    pub(super) fn page_changed(&mut self, window: &mut Window, cx: &mut Context<Line>) {
        let now = Instant::now();
        self.swap = Some(now);
        // A held ALLOW lets go: its prompt is off screen now (or was, a moment ago).
        self.hold_at = None;
        // What was said on the page left goes with it (COPIED, ESC AGAIN TO ABORT), and so does
        // a pending Esc's close: a cog click in its beat closed the Line on Settings.
        page_left(&mut self.note, &mut self.esc_at, self.nav.page);
        // The page left was this tall: both stay at least that (chat's draft or prompt strip
        // let go of it when they go: `relax`). Not the Brain screen's OTHER, open: it's a list
        // shown for a moment, not a height to keep.
        let other_open = self.nav.left == Page::Brain && self.other;
        if self.height > self.keep_h && !other_open {
            self.keep_h = self.height;
            self.keep_chat = self.nav.left == Page::Chat;
        }
        if self.nav.page == Page::Settings {
            self.sel = 0;
        } else if self.nav.page == Page::Brain {
            // The ways in, Claude picked; OTHER closed.
            self.bsel = 0;
            self.other = false;
        } else if !self.snap.prompts.is_empty() {
            // Back to a prompt he couldn't see: its buttons arm afresh.
            self.prompt_at = now;
        }
        self.refocus(window, cx);
        (self.on_ask)(Ask::Page(self.nav.page), cx).ok();
        cx.notify();
    }

    /// From the avatar (its SETTINGS row, SET ME UP, `avatarctl settings open [brain]|close`).
    pub fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Line>) {
        let changed = match page {
            Page::Settings | Page::Brain => self.nav.go(page),
            Page::Chat if self.nav.page != Page::Chat => {
                self.nav = Nav { left: self.nav.page, ..Nav::on(Page::Chat) };
                true
            }
            Page::Chat => false,
        };
        if changed {
            self.page_changed(window, cx);
        }
        // Asked for from her menu (or avatarctl): it comes with the keyboard, as the summon
        // key's does (only a desktop that hands it over on his input leaves it waiting).
        if page.settings() && (self.parked || !window.is_window_active()) {
            if let Err(e) = self.unpark(window, cx)
                && !super::keyboard_refused(&e)
            {
                elog!("synaps-avatar: line: settings, but {e}");
            }
            cx.notify();
        }
    }

    /// A key on the Settings page (all of them stop here: nothing types into the hidden field).
    pub(super) fn settings_key(&mut self, k: &Keystroke, window: &mut Window, cx: &mut Context<Line>) -> bool {
        let m = k.modifiers;
        let plain = !m.control && !m.platform && !m.alt;
        match k.key.as_str() {
            "escape" => {
                if self.nav.esc() {
                    self.page_changed(window, cx);
                } else {
                    self.close(true, window, cx);
                }
            }
            // A prompt waiting: Tab answers it, in chat.
            "tab" if !self.snap.prompts.is_empty() => {
                self.nav = Nav { left: self.nav.page, ..Nav::on(Page::Chat) };
                self.mode = Mode::Prompt;
                self.sync_secret(window, cx);
                self.page_changed(window, cx);
            }
            key if plain => {
                // Her values not in yet: nothing to change from (never a write from defaults).
                let Some(prefs) = self.snap.prefs.clone() else { return true };
                let act = if self.nav.page == Page::Brain {
                    let (sel, act) = brain_key(key, self.bsel, &prefs, self.other);
                    self.bsel = sel;
                    act
                } else {
                    let (sel, act) = list_key(key, self.sel, &prefs);
                    self.sel = sel;
                    act
                };
                self.settings_act(act, window, cx);
                cx.notify();
            }
            _ => {}
        }
        true
    }

    fn settings_act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Line>) {
        let page = self.nav.page;
        match act {
            Act::Pref(p) => {
                // Shown as asked at once: a second key steps on from it, not from the value
                // the first one's write hasn't changed yet (her next snapshot says the same).
                if let Some(prefs) = &mut self.snap.prefs {
                    let was = prefs.clone();
                    p.show(prefs);
                    self.bsel = brain_reselect(&was, prefs, self.bsel, self.other);
                }
                (self.on_ask)(Ask::Pref(p), cx).ok();
            }
            Act::Copy => {
                let about = self.snap.prefs.as_ref().map(|p| p.about.clone()).unwrap_or_default();
                (self.on_ask)(Ask::Copy(format!("{about} · {} · {}", std::env::consts::OS, std::env::consts::ARCH)), cx).ok();
                self.say(page, "COPIED", cx);
            }
            Act::Go(to) => {
                if self.nav.go(to) {
                    self.page_changed(window, cx);
                }
            }
            Act::Other(open) => self.other = open,
            Act::Login(p) => {
                let cmd = self.snap.prefs.as_ref().map(|s| s.mind.login_cmd(p)).unwrap_or_default();
                if !cmd.is_empty() {
                    (self.on_ask)(Ask::Copy(cmd), cx).ok();
                    self.say(page, "COPIED · RUN IT IN A TERMINAL", cx);
                }
            }
            Act::Local => {
                (self.on_ask)(Ask::OpenUrl(LOCAL_STEPS.into()), cx).ok();
                self.say(page, "OPENED THE STEPS", cx);
            }
            Act::Nothing => {}
        }
    }

    /// The swap's split now (px; 0: none), and whether a frame is still owed to it.
    pub(super) fn swap_split(&self, now: Instant) -> (f32, bool) {
        match self.swap {
            Some(t0) if self.snap.glitch => {
                let g = crate::hud::Glitch { t0 };
                (g.split(now), !g.done(now))
            }
            _ => (0.0, false),
        }
    }

    /// The split painted over the panel (its frame), for the swap's two frames.
    pub(super) fn swap_overlay(split: f32) -> AnyElement {
        canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let (x0, y0, x1, y1) = (f32::from(b.left()) + 1.0, f32::from(b.top()) + 1.0, f32::from(b.right()) - 1.0, f32::from(b.bottom()) - 1.0);
                split_paint(window, &theme::outline(Cut::Panel, x0, y0, x1, y1), split);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// The Settings page (the list, or the Brain screen): her line, the rows by section, the bar
    /// back. Its height moves on no key but OTHER (opened in place): her line has its slot
    /// whether she said something or not.
    pub(super) fn settings_el(&self, cx: &mut Context<Line>) -> Vec<AnyElement> {
        let pal = self.pal;
        let p = self.snap.prefs.clone().unwrap_or_default();
        let brain = self.nav.page == Page::Brain;
        let mut out: Vec<AnyElement> = Vec::new();
        // Her one line (a change made, or why not); before any, what the page does (the list) or
        // where her brain is at (the Brain screen, in her words). One line, its slot always there.
        let said = p.says.clone().or_else(|| brain.then(|| brain_line(&p.mind).to_string()));
        let says = said.as_ref().map_or_else(|| "Changes are saved to settings.toml as you make them.".to_string(), |s| format!("\u{201c}{s}\u{201d}"));
        let said = said.is_some();
        out.push(
            div()
                .mx(px(PAD))
                .mt(px(8.0))
                .h(px(QUOTE_H))
                .px(px(10.0))
                .border_l_3()
                .border_color(rgb(if said { pal.accent } else { pal.rule }))
                .flex()
                .items_center()
                .gap(px(10.0))
                .children(self.snap.face.as_ref().filter(|_| said).map(|f| img(ImageSource::Render(f.0.clone())).w(px(22.0)).h(px(22.0)).flex_none()))
                .child(div().flex_1().min_w(px(0.0)).overflow_hidden().whitespace_nowrap().text_size(px(14.5)).text_color(rgb(if said { pal.text } else { pal.dim })).child(says))
                .into_any_element(),
        );
        // Small type (a version, a lock, a reason): never under the section labels' size.
        let small_c = theme::mix(pal.dim, pal.text, 0.2);
        let section = |name: &str| {
            div()
                .px(px(PAD))
                .pt(px(6.0))
                .pb(px(5.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .line_height(px(14.0))
                .text_size(px(10.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(pal.dim))
                .child(tracked(name, 1.6))
                .child(div().flex_1().h(px(1.0)).bg(rgb(pal.rule)))
                .into_any_element()
        };
        // A value in a group: the one in force on a solid red plate (ink type); locked, on a
        // dim one. Red fill means only that: the row picked is shown by its notch.
        let seg = |label: &str, on: bool, live: bool, id: SharedString| {
            let c = match (on, live) {
                (true, true) => pal.ink,
                (true, false) => pal.panel,
                (false, true) => pal.text,
                (false, false) => theme::mix(pal.dim, pal.panel, 0.4),
            };
            div()
                .id(id)
                .relative()
                .h(px(20.0))
                .px(px(9.0))
                .flex()
                .items_center()
                .text_size(px(11.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(c))
                .when(on, |d| d.child(theme::backdrop(Cut::Corners(4.0), Fill::solid(if live { pal.accent } else { theme::mix(pal.dim, pal.panel, 0.4) }))))
                .child(tracked(label, 1.2))
        };
        // The group's track: one chamfered 1 px dim-red line round it (40 % when locked).
        let track = |live: bool| {
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(2.0))
                .p(px(3.0))
                .child(theme::backdrop(Cut::Corners(7.0), Fill::line(if live { pal.border } else { theme::mix(pal.border, pal.panel, 0.6) }, 1.0)))
        };
        let lock = |why: &str| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .line_height(px(14.0))
                .text_size(px(SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(small_c))
                .child(theme::glyph_at("lock", small_c, pal.panel, 12.0))
                .child(why.to_string())
        };
        let label = |name: &str, on: bool, sub: Option<&str>, w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .flex()
                .flex_col()
                .line_height(px(14.0))
                .child(div().text_size(px(12.5)).font_weight(FontWeight::BOLD).text_color(rgb(if on { pal.text } else { theme::mix(pal.text, pal.dim, 0.55) })).child(tracked(name, 1.4)))
                .children(sub.map(|s| div().overflow_hidden().whitespace_nowrap().text_size(px(SMALL)).font_weight(FontWeight::MEDIUM).text_color(rgb(small_c)).child(s.to_string())))
        };
        // A row: picked, a 3 px red notch on the panel's edge and a faint band across. A click
        // picks it; on the Brain screen it also takes it (each row there is a way in).
        let row = |ix: usize, on: bool| {
            div()
                .id(SharedString::from(format!("{}-row-{ix}", if brain { "brain" } else { "set" })))
                .relative()
                .px(px(PAD))
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap(px(8.0))
                .when(on, |d| {
                    d.child(div().absolute().top_0().left_0().size_full().bg(theme::alpha(pal.accent, 0.09)))
                        .child(div().absolute().top_0().left_0().bottom_0().w(px(3.0)).bg(rgb(pal.accent)))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                        if brain {
                            this.bsel = ix;
                            // MODEL's own values take a click; the row itself only picks it.
                            if let Some(prefs) = this.snap.prefs.clone()
                                && brain_rows(&prefs, this.other).get(ix) != Some(&BrainRow::Model)
                            {
                                let (_, act) = brain_key("enter", ix, &prefs, this.other);
                                this.settings_act(act, window, cx);
                            }
                        } else {
                            this.sel = ix;
                        }
                        cx.notify();
                    }),
                )
        };
        // A button on a row: framed (or solid red: the one thing to do), ▸ where it goes on.
        let button = |id: &'static str, text: &'static str, solid: bool| {
            theme::plate(Cut::Slant(6.0), if solid { Fill::solid(pal.accent) } else { Fill::line(pal.dim, 1.0) })
                .id(id)
                .h(px(22.0))
                .px(px(10.0))
                .cursor_pointer()
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(if solid { pal.ink } else { pal.text }))
                .child(tracked(text, 1.2))
        };
        // The value on a row's right, small: a ✓ (or a red dot) before it where it says so.
        let word = |text: String, mark: Option<&'static str>, c: u32| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(SMALL))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(c))
                .whitespace_nowrap()
                .children(mark.map(|m| theme::glyph_at(m, c, pal.panel, 10.0)))
                .child(tracked(&text, 1.0))
        };
        let m = &p.mind;
        if brain {
            let rows = brain_rows(&p, self.other);
            let sel = self.bsel.min(rows.len() - 1);
            out.push(section("WAYS IN"));
            for (ix, r) in rows.iter().enumerate() {
                let on = ix == sel;
                // A way in: its name, what it takes (or ✓ where she has it), ▸.
                let way = |name: &str, blurb: String, mark: Option<&'static str>, live: bool, open: Option<bool>| {
                    let c = if !live { theme::mix(pal.dim, pal.panel, 0.3) } else if mark == Some("check") { pal.accent } else { small_c };
                    let arrow = if open == Some(true) { "down" } else { "play" };
                    row(ix, on)
                        .child(label(name, on && live, None, 230.0))
                        .child(div().flex_1())
                        .child(word(blurb, mark, c))
                        .child(div().w(px(14.0)).flex().justify_center().child(theme::glyph_at(arrow, if live { pal.accent } else { c }, pal.panel, 9.0)))
                        .into_any_element()
                };
                // ✓ on a provider with a login here; IN USE on hers.
                let has = |to: &'static Provider| m.signed.contains(&to.id) || (m.provider() == Some(to) && m.signed_in == Some(true));
                let state = |to: &'static Provider, blurb: &str| -> (String, Option<&'static str>) {
                    match (has(to), m.provider() == Some(to)) {
                        (true, true) => ("IN USE".into(), Some("check")),
                        (true, false) => ("SIGNED IN".into(), Some("check")),
                        _ if !m.here => ("ON ITS MACHINE".into(), None),
                        _ => (blurb.into(), None),
                    }
                };
                match *r {
                    BrainRow::Claude => {
                        let to = &provider::CLAUDE;
                        let (b, mark) = state(to, "Pro or Max · browser");
                        let name = if has(to) { "CLAUDE" } else { "SIGN IN WITH CLAUDE" };
                        out.push(way(name, b, mark, m.here || has(to), None));
                    }
                    BrainRow::Local => {
                        let ollama = m.local.as_ref().is_none_or(|(u, _)| u.contains(":11434"));
                        let hers = m.provider().is_some_and(|x| x.kind == Kind::Local);
                        let (b, mark) = match m.local.as_ref().map(|l| &l.1) {
                            _ if hers && m.ok() => ("IN USE".to_string(), Some("check")),
                            _ if !m.here => ("ON ITS MACHINE".into(), None),
                            Some(provider::Local::Up(ids)) => (format!("{} · {} MODEL{} HERE", if ollama { "OLLAMA" } else { "SERVER" }, ids.len(), if ids.len() == 1 { "" } else { "S" }), None),
                            Some(provider::Local::Off) if ollama => ("free · offline · Ollama isn't running".into(), None),
                            _ => ("free · offline · Ollama".into(), None),
                        };
                        out.push(way("RUN LOCAL", b, mark, m.here, None));
                    }
                    BrainRow::Other => {
                        let n = m.others().into_iter().filter(|&to| has(to)).count();
                        let b = if n > 0 { format!("{n} SIGNED IN · ChatGPT · Copilot · keys") } else { "ChatGPT · Copilot · keys".into() };
                        out.push(way("OTHER", b, None, true, Some(self.other)));
                    }
                    BrainRow::Account(i) => {
                        let Some(&to) = m.others().get(i) else { continue };
                        let blurb = match to.id {
                            "openai-codex" => "Plus or Pro · browser",
                            "github-copilot" | "kimi-code" | "muse" => "a code to type in",
                            _ => "browser",
                        };
                        let (b, mark) = state(to, blurb);
                        let name = format!("   {}", to.name.to_uppercase());
                        out.push(way(name.as_str(), b, mark, m.here || has(to), None));
                    }
                    BrainRow::Key => {
                        let (b, mark) = key_word(m);
                        out.push(way("   API KEY", b, mark, m.here || mark.is_some(), None));
                    }
                    BrainRow::Model => {
                        out.push(section("MODEL"));
                        let live = m.lock.is_none();
                        let why = match m.lock {
                            Some(provider::Lock::Cli) => Some("set by --model"),
                            Some(provider::Lock::Theirs) => Some("their session"),
                            None => None,
                        };
                        let segs: Vec<_> = m
                            .choices()
                            .into_iter()
                            .enumerate()
                            .map(|(k, c)| {
                                let pick = c.pick.filter(|_| live && !c.on);
                                seg(c.label.as_str(), c.on, live, SharedString::from(format!("brain-model-{k}"))).when_some(pick, |d, to| {
                                    d.cursor_pointer().on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.bsel = ix;
                                            this.settings_act(Act::Pref(Pref::Model(to)), window, cx);
                                            cx.notify();
                                        }),
                                    )
                                })
                            })
                            .collect();
                        out.push(
                            row(ix, on)
                                .child(label("MODEL", on, None, 150.0))
                                .child(div().flex_1())
                                .children(why.map(lock))
                                .child(track(live).children(segs))
                                .into_any_element(),
                        );
                    }
                }
            }
            // Where his login goes (and where it doesn't: a daemon on another machine).
            let foot = if m.here { "your login stays on this machine (~/.synaps-cli)" } else { "my daemon is on another machine (--socket): its logins are there" };
            out.push(div().px(px(PAD)).pt(px(6.0)).text_size(px(SMALL)).font_weight(FontWeight::MEDIUM).text_color(rgb(small_c)).child(foot).into_any_element());
        } else {
            let rows = set_rows(&p);
            let sel = self.sel.min(rows.len() - 1);
            for (ix, r) in rows.iter().enumerate() {
                let on = ix == sel;
                match r {
                    SetRow::Brain => {
                        out.push(section("BRAIN"));
                        let none = m.none();
                        let c = if none { pal.accent } else if m.ok() { pal.text } else { small_c };
                        let mark = if none {
                            Some("disc")
                        } else if m.ok() {
                            Some("check")
                        } else {
                            None
                        };
                        let go = |b: Stateful<Div>| {
                            b.on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.sel = ix;
                                    this.settings_act(Act::Go(Page::Brain), window, cx);
                                    cx.notify();
                                }),
                            )
                        };
                        out.push(
                            row(ix, on)
                                .child(label("BRAIN", on, None, 170.0))
                                .child(div().flex_1().min_w(px(0.0)).flex().justify_end().overflow_hidden().child(word(brain_word(m), mark, c)))
                                .child(go(if none { button("set-brain", "SET UP ▸", true) } else { button("set-brain", "CHANGE ▸", false) }))
                                .into_any_element(),
                        );
                    }
                    SetRow::Attention => {
                        out.push(section("BEHAVIOUR"));
                        // Her brain off: she bids for nothing, so OFF is what runs; locked to the file.
                        let live = p.brain;
                        let cur = if live { p.attention } else { Attention::Off };
                        let segs = ATTENTION.iter().map(|&a| {
                            seg(a.label(), a == cur, live, SharedString::from(format!("set-att-{}", a.key()))).when(live, |d| {
                                d.cursor_pointer().on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.sel = ix;
                                        if this.snap.prefs.as_ref().is_some_and(|p| p.brain && p.attention != a) {
                                            this.settings_act(Act::Pref(Pref::Attention(a)), window, cx);
                                        }
                                        cx.notify();
                                    }),
                                )
                            })
                        });
                        out.push(
                            row(ix, on)
                                .child(label("ATTENTION", on, (!live).then_some("brain is off"), 170.0))
                                .child(div().flex_1())
                                .children((!live).then(|| lock("locked · settings.toml")))
                                .child(track(live).children(segs))
                                .into_any_element(),
                        );
                    }
                    SetRow::Wake => {
                        out.push(section("VOICE"));
                        // Voice off ([voice] enabled = false): the word can't be heard, so OFF is
                        // what runs; the group is locked to the file.
                        let live = p.voice;
                        let cur = p.wake && live;
                        let segs = [("ON", true), ("OFF", false)].map(|(l, v)| {
                            seg(l, cur == v, live, SharedString::from(format!("set-wake-{l}"))).when(live, |d| {
                                d.cursor_pointer().on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.sel = ix;
                                        if this.snap.prefs.as_ref().is_some_and(|p| p.voice && p.wake != v) {
                                            this.settings_act(Act::Pref(Pref::Wake(v)), window, cx);
                                        }
                                        cx.notify();
                                    }),
                                )
                            })
                        });
                        out.push(
                            row(ix, on)
                                .child(label("\u{201c}HEY CAPRI\u{201d}", on, (!live).then_some("voice is off"), 170.0))
                                .child(div().flex_1())
                                .children((!live).then(|| lock("locked · settings.toml")))
                                .child(track(live).children(segs))
                                .into_any_element(),
                        );
                    }
                    SetRow::About => {
                        out.push(section("ABOUT"));
                        out.push(
                            row(ix, on)
                                .child(label("VERSION", on, None, 170.0))
                                .child(div().flex_1().min_w(px(0.0)).text_size(px(13.0)).font_weight(FontWeight::MEDIUM).text_color(rgb(if on { pal.text } else { small_c })).overflow_hidden().whitespace_nowrap().child(p.about.clone()))
                                .child(button("set-github", "GITHUB ▸", false).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.sel = ix;
                                        (this.on_ask)(Ask::OpenUrl(REPO.into()), cx).ok();
                                        cx.notify();
                                    }),
                                ))
                                .child(button("set-copy", "COPY", false).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.sel = ix;
                                        this.settings_act(Act::Copy, window, cx);
                                        cx.notify();
                                    }),
                                ))
                                .into_any_element(),
                        );
                    }
                }
            }
        }
        // As tall as chat was (the content's min height): the bar stays where the field was.
        out.push(div().flex_1().min_h(px(4.0)).into_any_element());
        // The bar where chat's field is (its keys on the same row, no row under it): `◂ CHAT` (or
        // `◂ BACK`) in the cog's spot, the keys (or what she just did: COPIED), what came meanwhile.
        let hint = if brain { brain_hint(&p, self.bsel, self.other) } else { hint(&p, self.sel) };
        let (keys, keys_c, weight) = match self.note.on(self.nav.page, Instant::now()) {
            Some(n) => (n.to_string(), pal.accent, FontWeight::BOLD),
            None => (hint.to_string(), small_c, FontWeight::MEDIUM),
        };
        out.push(
            div()
                .px(px(PAD))
                .pt(px(10.0))
                .pb(px(BAR_PB))
                .border_t_1()
                .border_color(rgb(pal.rule))
                .flex()
                .h(px(10.0 + 1.0 + self.field_h.get() + BAR_PB))
                .gap(px(12.0))
                .child(self.back_el(cx))
                .child(div().self_center().text_size(px(SMALL)).line_height(px(16.0)).font_weight(weight).text_color(rgb(keys_c)).whitespace_nowrap().child(keys))
                .child(div().flex_1())
                .when(p.new > 0, |d| {
                    d.child(
                        theme::plate(Cut::Swallow(4.0), Fill::solid(pal.accent))
                            .self_center()
                            .h(px(16.0))
                            .pl(px(6.0))
                            .pr(px(10.0))
                            .text_size(px(10.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(pal.ink))
                            .child(tracked(&format!("{} NEW", p.new), 1.2)),
                    )
                })
                .into_any_element(),
        );
        // Chat's row of keys, as room: `◂ CHAT` where the cog was, his second click in place
        // goes back. The room the page has over its rows (it is as tall as chat) is above.
        out.push(div().flex_none().h(px(BAR_DROP)).into_any_element());
        out
    }
}

/// Her line on the Brain screen before a change: where her brain is at, in her words.
pub fn brain_line(m: &provider::Status) -> &'static str {
    let local = m.provider().is_some_and(|p| p.kind == Kind::Local);
    let ollama = m.local.as_ref().is_none_or(|(u, _)| u.contains(":11434"));
    match m.local.as_ref().map(|l| &l.1) {
        _ if !m.here && !m.ok() => "My brain runs on another machine. Sign in there, then say hi.",
        _ if m.none() => "Give me a brain and I'll talk back.",
        _ if local && m.stale_url() => "Synaps is still on its old local URL. Run `synaps daemon reload`, then ask me again.",
        _ if m.ok() => "That's what I think with. Swap it any time.",
        Some(provider::Local::Off) if local && ollama => "Ollama's asleep. Start it and I'll look again.",
        Some(provider::Local::Off) if local => "My local server's asleep. Start it and I'll look again.",
        Some(provider::Local::Up(_)) if local => "That model isn't on this machine.",
        _ if m.model.is_none() => "Can't reach my daemon yet. Once I do, I'll know.",
        _ => "Can't see a login from here. Say hi: if I answer, I've got one.",
    }
}

/// API KEY's word: IN USE on hers (a key service with its key), else ✓ and the services with a
/// key set here, else a few of them.
pub fn key_word(m: &provider::Status) -> (String, Option<&'static str>) {
    let set = keys_set(m);
    if let Some(to) = m.provider().filter(|p| set.contains(p)) {
        // Others set too: ⏎ steps to the next (`key_act`).
        let more = if set.len() > 1 { format!(" +{}", set.len() - 1) } else { String::new() };
        return (format!("IN USE · {}{more}", to.name.to_uppercase()), Some("check"));
    }
    if !m.here {
        return ("ON ITS MACHINE".into(), None);
    }
    let keys: Vec<String> = set.iter().map(|p| p.name.to_uppercase()).collect();
    match keys.as_slice() {
        [] => ("Groq · OpenRouter · 17 more".into(), None),
        [k] => (format!("KEY SET · {k}"), Some("check")),
        [x, y] => (format!("KEYS SET · {x} · {y}"), Some("check")),
        [x, y, rest @ ..] => (format!("KEYS SET · {x} · {y} +{}", rest.len()), Some("check")),
    }
}

/// The BRAIN row's word: `CLAUDE · SONNET 4.6`, `NO BRAIN YET`, `LOCAL · QWEN3:8B · OFF`.
pub fn brain_word(m: &provider::Status) -> String {
    let Some(model) = m.model.as_deref() else { return "OFFLINE".into() };
    if m.none() {
        return "NO BRAIN YET".into();
    }
    let label = provider::model_label(model);
    let mut w = match m.provider() {
        Some(p) => format!("{} · {label}", p.name.to_uppercase()),
        None => label,
    };
    if m.provider().is_some_and(|p| p.kind == Kind::Local) {
        match &m.local {
            Some((_, provider::Local::Off)) => w.push_str(" · OFF"),
            Some((_, l @ provider::Local::Up(_))) if !l.has(model) => w.push_str(" · NOT PULLED"),
            _ => {}
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::{Act, BrainRow, FLASH, MenuSave, Mode, Nav, Note, Page, Pref, Prefs, SetRow, asked, asked_model, brain_hint, brain_key, brain_line, brain_reselect, brain_rows, brain_word, esc_closes, hint, hold_fires, key_word, list_key, next_attention, page_left, set_rows, spot_lift};
    use crate::provider::{self, Local, Lock, Status};
    use crate::settings::{Attention, Settings, WriteQueue, write_at};

    #[test]
    fn esc_steps_back_one_page_at_a_time() {
        // From chat by the cog: Esc to chat, then Esc is chat's own (it closes).
        let mut n = Nav::default();
        n.cog();
        assert_eq!(n.page, Page::Settings);
        assert!(n.esc());
        assert_eq!(n.page, Page::Chat);
        assert!(!n.esc(), "on chat, nowhere back: Esc closes as ever");
        // Opened on Settings (the menu's row): Esc to chat first, never closes from there.
        let mut n = Nav::on(Page::Settings);
        assert!(n.esc() && n.page == Page::Chat);
        assert!(!n.esc());
        // Going twice to the same page is one step.
        let mut n = Nav::default();
        assert!(n.go(Page::Settings));
        assert!(!n.go(Page::Settings));
        assert!(n.esc() && n.page == Page::Chat && !n.esc());
    }

    #[test]
    fn the_cog_toggles_the_page() {
        let mut n = Nav::default();
        for k in 0..6 {
            n.cog();
            assert_eq!(n.page, if k % 2 == 0 { Page::Settings } else { Page::Chat });
        }
        // From Settings the cog goes straight to chat, nothing left to step back through.
        let mut n = Nav::on(Page::Settings);
        n.cog();
        assert!(n.page == Page::Chat && !n.esc());
    }

    fn prefs() -> Prefs {
        Prefs { attention: Attention::Mix, brain: true, wake: true, voice: true, about: "CAPRI 1.0.0".into(), mind: mind("claude-sonnet-4-6", Some(true)), says: None, new: 0 }
    }

    /// Her brain on `model`, a daemon on this machine; `signed_in`: a login (None: can't tell).
    fn mind(model: &str, signed_in: Option<bool>) -> Status {
        let signed = if signed_in == Some(true) { provider::of(model).map(|p| vec![p.id]).unwrap_or_default() } else { Vec::new() };
        Status { model: Some(model.into()), signed_in, signed, here: true, synaps: "synaps".into(), ..Status::default() }
    }

    #[test]
    fn its_keys_move_change_and_copy() {
        let p = prefs();
        assert_eq!(set_rows(&p), vec![SetRow::Brain, SetRow::Attention, SetRow::Wake, SetRow::About]);
        assert_eq!(list_key("down", 0, &p), (1, Act::Nothing));
        assert_eq!(list_key("down", 3, &p), (3, Act::Nothing));
        assert_eq!(list_key("up", 0, &p), (0, Act::Nothing));
        for k in ["enter", "right", "space"] {
            assert_eq!(list_key(k, 0, &p), (0, Act::Go(Page::Brain)), "{k}: BRAIN opens the Brain screen");
        }
        assert_eq!(list_key("left", 0, &p), (0, Act::Nothing));
        assert_eq!(list_key("right", 1, &p), (1, Act::Pref(Pref::Attention(Attention::HangOnly))));
        assert_eq!(list_key("left", 1, &p), (1, Act::Pref(Pref::Attention(Attention::Off))));
        assert_eq!(list_key("space", 2, &p), (2, Act::Pref(Pref::Wake(false))));
        assert_eq!(list_key("enter", 3, &p), (3, Act::Copy));
        assert_eq!(list_key("x", 2, &p), (2, Act::Nothing));
        // Her brain off: ATTENTION is there but locked to the file, as "HEY CAPRI" is.
        let q = Prefs { brain: false, ..prefs() };
        assert_eq!(set_rows(&q), vec![SetRow::Brain, SetRow::Attention, SetRow::Wake, SetRow::About]);
        for k in ["left", "right", "space", "enter"] {
            assert_eq!(list_key(k, 1, &q), (1, Act::Nothing), "{k}");
        }
        assert_eq!(list_key("enter", 2, &q), (2, Act::Pref(Pref::Wake(false))));
        assert_eq!(list_key("down", 5, &q), (3, Act::Nothing));
        // Voice off: "HEY CAPRI" is locked to the file, no key changes it.
        let off = Prefs { voice: false, ..prefs() };
        for k in ["left", "right", "space", "enter"] {
            assert_eq!(list_key(k, 2, &off), (2, Act::Nothing), "{k}");
        }
    }

    #[test]
    fn the_hint_is_the_rows_own() {
        let p = prefs();
        assert_eq!(hint(&p, 0), "↑↓ pick · ⏎ open · esc chat");
        assert_eq!(hint(&p, 1), "↑↓ pick · ←→ change · esc chat");
        assert_eq!(hint(&p, 2), "↑↓ pick · ←→ change · esc chat");
        assert_eq!(hint(&p, 3), "↑↓ pick · ⏎ copy · esc chat", "⏎ copy only where it copies");
        assert_eq!(hint(&Prefs { voice: false, ..prefs() }, 2), "↑↓ pick · esc chat", "locked: nothing to change");
        assert_eq!(hint(&Prefs { brain: false, ..prefs() }, 1), "↑↓ pick · esc chat");
        assert_eq!(hint(&p, 9), hint(&p, 3));
    }

    /// Brain → the list → chat; opened on Brain (no brain: her summon, SET ME UP), Esc → chat.
    #[test]
    fn esc_from_the_brain_screen_goes_back_where_he_came_from() {
        let mut n = Nav::default();
        n.cog();
        assert!(n.go(Page::Brain));
        assert!(n.esc() && n.page == Page::Settings, "to the list");
        assert!(n.esc() && n.page == Page::Chat, "then chat");
        assert!(!n.esc());
        let mut n = Nav::on(Page::Brain);
        assert!(n.esc() && n.page == Page::Chat, "opened there: straight to chat");
        // From chat to Brain (her menu's SET UP MY BRAIN with the Line open): Esc back to chat.
        let mut n = Nav::default();
        assert!(n.go(Page::Brain));
        assert!(n.esc() && n.page == Page::Chat);
        // The cog from Brain: chat, nothing left to step through.
        let mut n = Nav::on(Page::Settings);
        n.go(Page::Brain);
        n.cog();
        assert!(n.page == Page::Chat && !n.esc());
        assert!(Page::Brain.settings() && Page::Settings.settings() && !Page::Chat.settings());
        assert_eq!(Page::Brain.key(), "brain");
    }

    /// No brain: the three ways in, Claude picked; ⏎ on Claude copies its sign-in (until PR 4
    /// runs it here). OTHER opens in place (five accounts: Gemini can't sign in without its
    /// client id; then API KEY) and ← closes it.
    #[test]
    fn no_brain_the_ways_in_claude_first() {
        let p = Prefs { mind: mind("claude-sonnet-4-6", Some(false)), ..prefs() };
        assert_eq!(brain_rows(&p, false), vec![BrainRow::Claude, BrainRow::Local, BrainRow::Other], "no MODEL before a brain");
        assert_eq!(brain_key("enter", 0, &p, false), (0, Act::Login(Some(&provider::CLAUDE))));
        assert_eq!(p.mind.login_cmd(Some(&provider::CLAUDE)), "synaps login --provider claude");
        assert_eq!(brain_key("enter", 1, &p, false), (1, Act::Local));
        assert_eq!(brain_key("right", 2, &p, false), (2, Act::Other(true)));
        assert_eq!(brain_key("enter", 2, &p, false), (2, Act::Other(true)));
        assert_eq!(brain_key("enter", 2, &p, true), (2, Act::Other(false)));
        let open = brain_rows(&p, true);
        assert_eq!(open.len(), 3 + 5 + 1);
        assert_eq!(open[3], BrainRow::Account(0));
        assert_eq!(open[8], BrainRow::Key);
        let name = |p: &Prefs| p.mind.others().iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(name(&p), ["ChatGPT", "Copilot", "Grok", "Kimi", "Muse"]);
        let codex = provider::by_id("openai-codex").unwrap();
        assert_eq!(brain_key("enter", 3, &p, true), (3, Act::Login(Some(codex))));
        assert_eq!(brain_key("enter", 5, &p, true), (5, Act::Login(provider::by_id("xai-auth"))), "Grok, where Gemini would be");
        assert_eq!(brain_key("enter", 8, &p, true), (8, Act::Login(None)), "API KEY: Synaps' list of services");
        assert_eq!(brain_key("left", 7, &p, true), (2, Act::Other(false)), "← closes OTHER, back on it");
        assert_eq!(brain_key("down", 8, &p, true), (8, Act::Nothing));
        // Gemini: with its client id set, a login on file, or hers.
        let mut g = p.clone();
        g.mind.gemini = true;
        assert_eq!(name(&g), ["ChatGPT", "Copilot", "Gemini", "Grok", "Kimi", "Muse"]);
        assert_eq!(brain_key("enter", 5, &g, true), (5, Act::Login(provider::by_id("google-gemini"))));
        let mut g = p.clone();
        g.mind.signed.push("google-gemini");
        assert_eq!(name(&g).len(), 6, "signed in already");
        let g = Prefs { mind: mind("google-gemini/gemini-2.5-pro", Some(false)), ..prefs() };
        assert_eq!(name(&g).len(), 6, "hers");
        assert_eq!(brain_key("down", 2, &p, false), (2, Act::Nothing), "closed: nothing under OTHER");
        assert_eq!(brain_hint(&p, 0, false), "↑↓ pick · ⏎ copy the sign-in · esc back");
        assert_eq!(brain_hint(&p, 1, false), "↑↓ pick · ⏎ the steps · esc back");
        assert_eq!(brain_hint(&p, 2, false), "↑↓ pick · ⏎ open · esc back");
        assert_eq!(brain_hint(&p, 2, true), "↑↓ pick · ⏎ close · esc back", "⏎ on OTHER, open, closes it");
        assert_eq!(brain_hint(&p, 3, true), "↑↓ pick · ⏎ copy the sign-in · ← close · esc back", "an account: ⏎ copies its sign-in");
        assert_eq!(brain_hint(&p, 8, true), "↑↓ pick · ⏎ copy the sign-in · ← close · esc back", "API KEY");
        assert_eq!(brain_line(&p.mind), "Give me a brain and I'll talk back.");
        assert_eq!(brain_word(&p.mind), "NO BRAIN YET");
    }

    /// Signed in on Claude: ✓, MODEL steps through its models (written as `[session] model`);
    /// another account with a login on file switches her to its default model, no sign-in.
    #[test]
    fn with_a_brain_model_and_a_switch() {
        let p = prefs();
        let rows = brain_rows(&p, false);
        assert_eq!(rows.last(), Some(&BrainRow::Model));
        let model = rows.len() - 1;
        assert_eq!(brain_key("enter", 0, &p, false), (0, Act::Nothing), "Claude is hers already");
        assert_eq!(brain_key("right", model, &p, false), (model, Act::Pref(Pref::Model("claude-opus-5-5"))));
        assert_eq!(brain_key("left", model, &p, false), (model, Act::Pref(Pref::Model("claude-haiku-4-5-20251001"))));
        assert_eq!(brain_hint(&p, model, false), "↑↓ pick · ←→ change · esc back");
        assert_eq!(brain_word(&p.mind), "CLAUDE · SONNET 4.6");
        assert_eq!(brain_line(&p.mind), "That's what I think with. Swap it any time.");
        // A ChatGPT login on file: its row switches her to GPT-5.5.
        let mut q = prefs();
        q.mind.signed.push("openai-codex");
        assert_eq!(brain_key("enter", 3, &q, true), (3, Act::Pref(Pref::Model("openai-codex/gpt-5.5"))));
        assert_eq!(brain_hint(&q, 3, true), "↑↓ pick · ⏎ use it · ← close · esc back");
        // On ChatGPT: Claude's row (its login on file) switches back.
        let mut r = Prefs { mind: mind("openai-codex/gpt-5.5", Some(true)), ..prefs() };
        r.mind.signed.push("anthropic");
        assert_eq!(brain_key("enter", 0, &r, false), (0, Act::Pref(Pref::Model("claude-sonnet-4-6"))));
        assert_eq!(brain_hint(&r, 0, false), "↑↓ pick · ⏎ use it · esc back");
        assert_eq!(brain_word(&r.mind), "CHATGPT · GPT-5.5");
        // Locked: started with --model, or riding his session.
        for lock in [Lock::Cli, Lock::Theirs] {
            let mut l = prefs();
            l.mind.lock = Some(lock);
            assert_eq!(brain_key("right", model, &l, false).1, Act::Nothing, "{lock:?}");
            assert_eq!(brain_hint(&l, model, false), "↑↓ pick · esc back");
            l.mind.signed.push("openai-codex");
            assert_eq!(brain_key("enter", 3, &l, true).1, Act::Nothing, "no switch either");
        }
        // His own model, by hand: MODEL steps to the table's, never back to his.
        let own = Prefs { mind: mind("claude-opus-4-1", Some(true)), ..prefs() };
        assert_eq!(brain_key("right", model, &own, false).1, Act::Pref(Pref::Model("claude-sonnet-4-6")));
        assert_eq!(brain_key("left", model, &own, false).1, Act::Pref(Pref::Model("claude-opus-5-5")));
    }

    /// API KEY: IN USE on a key service she runs on, ✓ and the services with a key here.
    #[test]
    fn api_key_says_which_keys_are_set() {
        let mut m = mind("claude-sonnet-4-6", Some(true));
        assert_eq!(key_word(&m), ("Groq · OpenRouter · 17 more".to_string(), None));
        m.signed.push("groq");
        assert_eq!(key_word(&m), ("KEY SET · GROQ".to_string(), Some("check")));
        m.signed.push("openrouter");
        assert_eq!(key_word(&m), ("KEYS SET · GROQ · OPENROUTER".to_string(), Some("check")));
        m.signed.push("cerebras");
        assert_eq!(key_word(&m).0, "KEYS SET · GROQ · CEREBRAS +1", "in the table's order");
        let groq = mind("groq/llama-3.3-70b-versatile", Some(true));
        assert_eq!(key_word(&groq), ("IN USE · GROQ".to_string(), Some("check")));
        let mut none = mind("groq/llama-3.3-70b-versatile", Some(false));
        assert_eq!(key_word(&none).1, None, "no key: not in use");
        none.here = false;
        assert_eq!(key_word(&none).0, "ON ITS MACHINE");
    }

    /// API KEY acts on what it says: one service's key set and her on another, ⏎ switches her to
    /// it (its default model); IN USE, several keys, none or locked: Synaps' list (to set one)
    /// on this machine, nothing on another.
    #[test]
    fn api_key_with_one_key_set_switches_her_to_it() {
        let key_row = 8;
        let mut p = Prefs { mind: mind("claude-sonnet-4-6", Some(true)), ..prefs() };
        assert_eq!(brain_rows(&p, true)[key_row], BrainRow::Key);
        assert_eq!(brain_key("enter", key_row, &p, true).1, Act::Login(None), "no key set: Synaps' list");
        p.mind.signed.push("groq");
        assert_eq!(key_word(&p.mind).0, "KEY SET · GROQ");
        for k in ["enter", "right", "space"] {
            assert_eq!(brain_key(k, key_row, &p, true), (key_row, Act::Pref(Pref::Model("groq/llama-3.3-70b-versatile"))), "{k}");
        }
        assert_eq!(brain_hint(&p, key_row, true), "↑↓ pick · ⏎ use it · ← close · esc back");
        let mut locked = p.clone();
        locked.mind.lock = Some(Lock::Cli);
        assert_eq!(brain_key("enter", key_row, &locked, true).1, Act::Login(None), "--model: no switch");
        let mut two = p.clone();
        two.mind.signed.push("openrouter");
        assert_eq!(brain_key("enter", key_row, &two, true).1, Act::Pref(Pref::Model("groq/llama-3.3-70b-versatile")), "two keys: the first");
        // On one of them: ⏎ steps to the next, round.
        let openrouter = provider::by_id("openrouter").unwrap().models[0].0;
        two.mind.model = Some("groq/llama-3.3-70b-versatile".into());
        let at = brain_rows(&two, true).iter().position(|r| *r == BrainRow::Key).unwrap();
        assert_eq!(key_word(&two.mind).0, "IN USE · GROQ +1");
        assert_eq!(brain_key("enter", at, &two, true).1, Act::Pref(Pref::Model(openrouter)));
        assert_eq!(brain_hint(&two, at, true), "↑↓ pick · ⏎ the next key · ← close · esc back");
        two.mind.model = Some(openrouter.into());
        assert_eq!(brain_key("enter", at, &two, true).1, Act::Pref(Pref::Model("groq/llama-3.3-70b-versatile")), "round to the first");
        two.mind.lock = Some(Lock::Theirs);
        assert_eq!(brain_key("enter", at, &two, true).1, Act::Login(None), "their session: no switch");
        let mut on_it = Prefs { mind: mind("groq/llama-3.3-70b-versatile", Some(true)), ..prefs() };
        assert_eq!(key_word(&on_it.mind).0, "IN USE · GROQ");
        let rows = brain_rows(&on_it, true);
        let at = rows.iter().position(|r| *r == BrainRow::Key).unwrap();
        assert_eq!(brain_key("enter", at, &on_it, true).1, Act::Login(None), "in use: set another");
        on_it.mind.here = false;
        on_it.mind.signed.clear();
        assert_eq!(brain_key("enter", at, &on_it, true).1, Act::Nothing, "a daemon on another machine");
        assert_eq!(brain_hint(&on_it, at, true), "↑↓ pick · ← close · esc back");
        // On a key service with no key, a Groq key set: ⏎ is a switch, not a step.
        let mut keyless = Prefs { mind: mind("cerebras/llama3.1-8b", Some(false)), ..prefs() };
        keyless.mind.signed.push("groq");
        let at = brain_rows(&keyless, true).iter().position(|r| *r == BrainRow::Key).unwrap();
        assert_eq!(brain_key("enter", at, &keyless, true).1, Act::Pref(Pref::Model("groq/llama-3.3-70b-versatile")));
        assert_eq!(brain_hint(&keyless, at, true), "↑↓ pick · ⏎ use it · ← close · esc back");
    }

    /// A look that adds or drops a row under OTHER (Gemini's client id found, a login on file)
    /// keeps the row picked on its provider; one that's gone keeps the place.
    #[test]
    fn a_look_keeps_the_provider_picked() {
        let p = Prefs { mind: mind("claude-sonnet-4-6", Some(false)), ..prefs() };
        let mut g = p.clone();
        g.mind.gemini = true;
        // Grok picked (row 5 without Gemini); Gemini comes in before it: Grok is row 6 now.
        assert_eq!(brain_reselect(&p, &g, 5, true), 6);
        assert_eq!(brain_reselect(&g, &p, 6, true), 5, "and back");
        assert_eq!(brain_reselect(&p, &g, 4, true), 4, "Copilot, above it: as it was");
        assert_eq!(brain_reselect(&p, &g, 8, true), 9, "API KEY");
        assert_eq!(brain_reselect(&g, &p, 5, true), 5, "Gemini itself gone: the same place (Grok)");
        assert_eq!(brain_reselect(&p, &g, 2, false), 2, "OTHER closed: nothing moves");
        // A brain now: MODEL comes in at the end; the row picked stays.
        let signed = Prefs { mind: mind("claude-sonnet-4-6", Some(true)), ..prefs() };
        assert_eq!(brain_reselect(&p, &signed, 1, false), 1);
        // No brain any more: MODEL picked, gone: the last row.
        assert_eq!(brain_reselect(&signed, &p, 3, false), 2);
    }

    /// A daemon through `--socket` (another machine's): no sign-in here reaches it, so the ways
    /// in do nothing and say where; MODEL is hers, still live.
    #[test]
    fn a_remote_daemon_signs_in_on_its_machine() {
        let mut p = Prefs { mind: mind("claude-sonnet-4-6", None), ..prefs() };
        p.mind.here = false;
        assert_eq!(brain_key("enter", 0, &p, false).1, Act::Nothing);
        assert_eq!(brain_key("enter", 3, &p, true).1, Act::Nothing);
        assert_eq!(brain_key("enter", 8, &p, true).1, Act::Nothing, "API KEY");
        assert_eq!(brain_key("enter", 1, &p, false).1, Act::Nothing, "RUN LOCAL: its machine's server, not this one's");
        assert_eq!(brain_hint(&p, 1, false), "↑↓ pick · esc back");
        assert_eq!(brain_hint(&p, 0, false), "↑↓ pick · esc back", "nothing to do here: no ⏎");
        assert_eq!(brain_hint(&p, 3, true), "↑↓ pick · ← close · esc back");
        assert_eq!(brain_hint(&p, 8, true), "↑↓ pick · ← close · esc back");
        let model = brain_rows(&p, false).len() - 1;
        assert_eq!(brain_key("right", model, &p, false).1, Act::Pref(Pref::Model("claude-opus-5-5")));
        assert_eq!(brain_line(&p.mind), "My brain runs on another machine. Sign in there, then say hi.");
        assert_eq!(brain_word(&p.mind), "CLAUDE · SONNET 4.6");
        // It answered: a brain, there or here.
        p.mind.signed_in = Some(true);
        assert_eq!(brain_line(&p.mind), "That's what I think with. Swap it any time.");
    }

    /// A local model: the server's look says whether it's there; MODEL holds hers alone.
    #[test]
    fn a_local_brain_is_its_server() {
        let mut p = Prefs { mind: mind("local/qwen3:8b", None), ..prefs() };
        assert_eq!(brain_word(&p.mind), "LOCAL · QWEN3:8B");
        p.mind.local = Some(("http://localhost:11434/v1".into(), Local::Off));
        assert_eq!(brain_word(&p.mind), "LOCAL · QWEN3:8B · OFF");
        assert_eq!(brain_line(&p.mind), "Ollama's asleep. Start it and I'll look again.");
        p.mind.local = Some(("http://localhost:1234/v1".into(), Local::Off));
        assert_eq!(brain_line(&p.mind), "My local server's asleep. Start it and I'll look again.");
        p.mind.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["llama3.2:latest".into()])));
        assert_eq!(brain_word(&p.mind), "LOCAL · QWEN3:8B · NOT PULLED");
        assert_eq!(brain_line(&p.mind), "That model isn't on this machine.");
        p.mind.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["qwen3:8b".into()])));
        assert!(p.mind.ok());
        // The daemon failed on another URL (its config changed after it started): the way out.
        let mut stale = p.clone();
        stale.mind.tried = Some("http://localhost:1234/v1".into());
        assert!(!stale.mind.ok());
        assert_eq!(brain_line(&stale.mind), "Synaps is still on its old local URL. Run `synaps daemon reload`, then ask me again.");
        stale.mind.tried = Some("http://localhost:11434/v1".into());
        assert_eq!(brain_line(&stale.mind), "That's what I think with. Swap it any time.", "the same URL: its server came back");
        stale.mind.model = Some("claude-sonnet-4-6".into());
        stale.mind.signed_in = Some(true);
        stale.mind.tried = Some("http://localhost:1234/v1".into());
        assert_eq!(brain_line(&stale.mind), "That's what I think with. Swap it any time.", "not her brain now");
        let model = brain_rows(&p, false).len() - 1;
        assert_eq!(brain_key("right", model, &p, false).1, Act::Nothing, "one model: nothing to step to");
        assert_eq!(brain_word(&Status::default()), "OFFLINE");
    }

    /// MODEL's write: `[session] model`, explicit, through the safe writer (his comments and
    /// other keys kept); the page shows it as asked until it's in.
    #[test]
    fn the_model_is_written_to_session_model() {
        let d = std::env::temp_dir().join(format!("sa-line-model-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.toml");
        std::fs::write(&f, "# mine\n[session]\nthinking = \"low\" # keep\n[brain]\nattention = \"mix\"\n").unwrap();
        let pref = Pref::Model("claude-opus-5-5");
        assert!(write_at(&f, &[pref.edit()]).unwrap(), "written");
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("# mine") && text.contains("thinking = \"low\" # keep"), "{text}");
        let mut s = Settings::parse(&text).unwrap();
        assert_eq!(s.session.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(s.session.thinking.as_deref(), Some("low"));
        // Again: in place, nothing to write.
        assert!(!write_at(&f, &[pref.edit()]).unwrap(), "in place: nothing written");
        // A model with a provider and slashes in it.
        write_at(&f, &[Pref::Model("openrouter/meta-llama/llama-3.3-70b-instruct").edit()]).unwrap();
        assert_eq!(Settings::parse(&std::fs::read_to_string(&f).unwrap()).unwrap().session.model.as_deref(), Some("openrouter/meta-llama/llama-3.3-70b-instruct"));
        // Applied in memory as the file has it; shown as asked meanwhile.
        Pref::Model("claude-haiku-4-5-20251001").apply(&mut s);
        assert_eq!(s.session.model.as_deref(), Some("claude-haiku-4-5-20251001"));
        let mut p = prefs();
        Pref::Model("claude-opus-5-5").show(&mut p);
        assert_eq!(p.mind.model.as_deref(), Some("claude-opus-5-5"));
        let pending = [Pref::Attention(Attention::Off), Pref::Model("claude-opus-5-5"), Pref::Wake(false)];
        assert_eq!(asked_model(pending.iter(), Some("claude-sonnet-4-6")).as_deref(), Some("claude-opus-5-5"));
        assert_eq!(asked_model(std::iter::empty(), Some("claude-sonnet-4-6")).as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(asked(pending.iter(), Attention::Mix, true), (Attention::Off, false), "MODEL moves neither");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_held_allow_never_fires_off_screen() {
        let t0 = std::time::Instant::now();
        let p = Mode::Prompt;
        // Held on chat, still held at HOLD, the same prompt: it allows.
        assert!(hold_fires(Some(t0), t0, Page::Chat, p, true));
        // He went to Settings meanwhile (the swap lets go: hold_at None), or it is still set
        // but the prompt is off screen: never allowed unseen.
        assert!(!hold_fires(None, t0, Page::Chat, p, true));
        assert!(!hold_fires(Some(t0), t0, Page::Settings, p, true));
        // Tab to the field while holding: the prompt strip is gone, so is the hold.
        assert!(!hold_fires(Some(t0), t0, Page::Chat, Mode::Text, true));
        // A later hold, or another prompt in its place: not this one's.
        assert!(!hold_fires(Some(t0 + std::time::Duration::from_millis(5)), t0, Page::Chat, p, true));
        assert!(!hold_fires(Some(t0), t0, Page::Chat, p, false));
    }

    /// The page and her menu step from the same value: the one asked, not yet written.
    #[test]
    fn page_and_menu_step_from_what_was_asked() {
        // Two quick → on the page, before the first write is in: two steps (the page shows
        // each as asked at once, and steps on from it).
        let mut p = prefs();
        let (_, a) = list_key("right", 1, &p);
        assert_eq!(a, Act::Pref(Pref::Attention(Attention::HangOnly)));
        let Act::Pref(x) = a else { unreachable!() };
        x.show(&mut p);
        assert_eq!(list_key("right", 1, &p).1, Act::Pref(Pref::Attention(Attention::Off)));
        // "HEY CAPRI" twice: off, then on again (not off twice).
        let (_, w) = list_key("space", 2, &p);
        let Act::Pref(x) = w else { unreachable!() };
        x.show(&mut p);
        assert_eq!(list_key("space", 2, &p).1, Act::Pref(Pref::Wake(true)));
        // The menu clicked once (HANG ONLY on its way), then the page: the page's Prefs are as
        // asked, so → goes to OFF, as the menu's next click would.
        let pending = [Pref::Attention(Attention::HangOnly), Pref::Wake(false)];
        let (att, wake) = asked(pending.iter(), Attention::Mix, true);
        assert_eq!((att, wake), (Attention::HangOnly, false));
        let page = Prefs { attention: att, wake, ..prefs() };
        assert_eq!(list_key("right", 1, &page).1, Act::Pref(Pref::Attention(next_attention(pending.iter(), Attention::Mix))));
        assert_eq!(asked(std::iter::empty(), Attention::Off, true), (Attention::Off, true), "nothing asked: as she runs");
    }

    /// Her menu's ATTENTION click refused (a read-only, typo'd or nix-store settings.toml) or
    /// hanging: its row says so, until an attention write lands or a reload.
    #[test]
    fn a_menu_click_that_didnt_stick_says_so_on_its_row() {
        let m = MenuSave::Fine;
        assert_eq!(m.says(), None);
        let att = Pref::Attention(Attention::Off);
        assert_eq!(m.written(att, true, false).says(), Some("CAN'T SAVE · settings.toml"));
        // The page's own refusal is the page's to say (her line there).
        assert_eq!(m.written(att, false, false), MenuSave::Fine);
        // Hanging, with a menu change waiting; not when only the page's are.
        assert_eq!(m.stuck(true).says(), Some("NOT SAVED YET · settings.toml"));
        assert_eq!(m.stuck(false), MenuSave::Fine);
        assert_eq!(MenuSave::Refused.stuck(true), MenuSave::Refused, "refused stays refused");
        // An attention write in (the menu's or the page's): fine again; a wake one: no news.
        assert_eq!(MenuSave::Stuck.written(att, true, true), MenuSave::Fine);
        assert_eq!(MenuSave::Refused.written(att, false, true), MenuSave::Fine);
        assert_eq!(MenuSave::Refused.written(Pref::Wake(true), false, true), MenuSave::Refused);
        // The real thing: a read-only file refuses the menu's write, nothing changes, the row
        // says so; writable again, the next click lands and the row is clear. Root writes
        // anything: nothing to see there (as the writer's own test).
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt;
            let d = std::env::temp_dir().join(format!("sa-menu-save-{}", std::process::id()));
            std::fs::remove_dir_all(&d).ok();
            std::fs::create_dir_all(&d).unwrap();
            let f = d.join("settings.toml");
            std::fs::write(&f, "[brain]\nattention = \"mix\"\n").unwrap();
            let load = || Settings::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
            let mut mem = load();
            let mut q: WriteQueue<(Pref, bool)> = WriteQueue::default();
            let mut row = MenuSave::Fine;
            let t0 = std::time::Instant::now();
            let click = |q: &mut WriteQueue<(Pref, bool)>, mem: &mut Settings, row: &mut MenuSave| {
                let a = next_attention(q.pending().map(|j| &j.0), mem.brain.attention);
                q.push(vec![Pref::Attention(a).edit()], (Pref::Attention(a), true));
                let (n, edits) = q.start(t0).unwrap();
                let r = write_at(&f, &edits);
                let ((p, menu), _) = q.finish(n).unwrap();
                if r.is_ok() {
                    p.apply(mem);
                }
                *row = row.written(p, menu, r.is_ok());
            };
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
            click(&mut q, &mut mem, &mut row);
            assert_eq!((mem.brain.attention, row.says()), (Attention::Mix, Some("CAN'T SAVE · settings.toml")));
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
            click(&mut q, &mut mem, &mut row);
            assert_eq!((mem.brain.attention, row.says()), (Attention::HangOnly, None));
            assert_eq!(load().brain.attention, Attention::HangOnly);
            std::fs::remove_dir_all(&d).ok();
        }
    }

    /// A reload (`settings reload`, or the one owed after the last write) keeps the row's
    /// warning: a refusal until an attention write lands, a hang while the menu's change still
    /// waits. Either order: the reload before the write's end, or after it.
    #[test]
    fn a_reload_keeps_the_menu_warning() {
        let att = Pref::Attention(Attention::HangOnly);
        // Hangs, he reloads while it still waits, then it is refused, then he reloads again.
        let mut row = MenuSave::Fine.stuck(true);
        row = row.reloaded(true);
        assert_eq!(row.says(), Some("NOT SAVED YET · settings.toml"), "still waiting: still says so");
        row = row.written(att, true, false);
        assert_eq!(row.reloaded(false), MenuSave::Refused);
        // Refused first, then a reload (nothing waiting), then a hanging click: still refused.
        let mut row = MenuSave::Fine.written(att, true, false);
        row = row.reloaded(false);
        assert_eq!(row.says(), Some("CAN'T SAVE · settings.toml"));
        row = row.stuck(true).reloaded(true);
        assert_eq!(row, MenuSave::Refused);
        // It lands: clear, and a reload after leaves it so.
        assert_eq!(row.written(att, true, true).reloaded(false), MenuSave::Fine);
        // Hanging, it lands, then the owed reload: clear. A hang with nothing waiting any more
        // (only the page's writes were): a reload clears it.
        assert_eq!(MenuSave::Fine.stuck(true).written(att, true, true).reloaded(false), MenuSave::Fine);
        assert_eq!(MenuSave::Stuck.reloaded(false), MenuSave::Fine);
        // Through the queue: the reload asked while the menu's write is under way.
        let mut q: crate::settings::WriteQueue<(Pref, bool)> = Default::default();
        q.push(vec![att.edit()], (att, true));
        let (n, _) = q.start(std::time::Instant::now()).unwrap();
        let mut row = MenuSave::Fine.stuck(q.pending().any(|j| j.1));
        assert!(q.reload());
        row = row.reloaded(q.pending().any(|j| j.1));
        assert_eq!(row, MenuSave::Stuck);
        let ((p, menu), owed) = q.finish(n).unwrap();
        row = row.written(p, menu, false);
        assert!(owed, "the reload is owed now the last write is in");
        assert_eq!(row.reloaded(q.pending().any(|j| j.1)), MenuSave::Refused);
    }

    /// The row's second line holds its room while the menu is open: its write landing doesn't
    /// pull HANDS-FREE TALK up under his next click.
    #[test]
    fn the_menu_holds_its_rows_till_it_closes() {
        let mut kept = None;
        assert_eq!(MenuSave::Fine.line(&mut kept), None, "nothing said this opening: no line");
        assert_eq!(MenuSave::Stuck.line(&mut kept), Some(("NOT SAVED YET · settings.toml", true)));
        assert_eq!(MenuSave::Fine.line(&mut kept), Some(("NOT SAVED YET · settings.toml", false)), "landed: its room, unseen");
        assert_eq!(MenuSave::Refused.line(&mut kept), Some(("CAN'T SAVE · settings.toml", true)));
        // Closed and opened again (the view lets go of `kept`): no line.
        let mut kept = None;
        assert_eq!(MenuSave::Fine.line(&mut kept), None);
    }

    /// A note belongs to the page it was said on: the page swap drops it (COPIED never comes
    /// back with chat, ESC AGAIN TO ABORT never sits on Settings), and on Settings it goes by
    /// itself. A pending Esc's close goes with the swap.
    #[test]
    fn notes_and_a_pending_esc_stay_on_their_page() {
        let t0 = std::time::Instant::now();
        let mut n = Note::default();
        // COPIED on Settings: shown, gone after FLASH, never on chat.
        let until = n.say(Page::Settings, "COPIED", t0).expect("a flash on Settings");
        assert_eq!(n.on(Page::Settings, t0), Some("COPIED"));
        assert_eq!(n.on(Page::Chat, t0), None);
        assert_eq!(n.on(Page::Settings, t0 + FLASH), None, "gone at FLASH, timer or not");
        assert!(n.expire(until));
        assert_eq!(n, Note::default());
        // A second COPIED in its beat: the first's timer leaves it be.
        let u1 = n.say(Page::Settings, "COPIED", t0).unwrap();
        let u2 = n.say(Page::Settings, "COPIED", t0 + FLASH / 2).unwrap();
        assert!(!n.expire(u1));
        assert_eq!(n.on(Page::Settings, t0 + FLASH), Some("COPIED"));
        assert!(n.expire(u2));
        // Back to chat with COPIED up: gone.
        n.say(Page::Settings, "COPIED", t0);
        let mut esc = None;
        page_left(&mut n, &mut esc, Page::Chat);
        assert_eq!(n.on(Page::Chat, t0), None);
        // Chat's notes stay till typed over (no timer); the cog takes them and the Esc away.
        assert_eq!(n.say(Page::Chat, "ESC AGAIN TO ABORT", t0), None);
        assert_eq!(n.on(Page::Chat, t0 + FLASH * 10), Some("ESC AGAIN TO ABORT"));
        let mut esc = Some(t0);
        assert!(esc_closes(esc, t0, Page::Chat), "one Esc in a turn closes a beat later");
        page_left(&mut n, &mut esc, Page::Settings);
        assert_eq!((n.on(Page::Settings, t0), n.on(Page::Chat, t0), esc), (None, None, None));
        assert!(!esc_closes(esc, t0, Page::Settings), "the cog in its beat: the Line stays open");
        assert!(!esc_closes(Some(t0), t0, Page::Settings), "on Settings it never closes");
        // A chat note that came while on Settings (a message refused late) waits for chat.
        n.say(Page::Chat, "NOT SENT · OFFLINE · text kept", t0);
        assert_eq!(n.on(Page::Settings, t0), None);
        page_left(&mut n, &mut esc, Page::Chat);
        assert_eq!(n.on(Page::Chat, t0), Some("NOT SENT · OFFLINE · text kept"));
    }

    /// `◂ CHAT` is where the cog was: both sit as high over the panel's bottom (which stays
    /// put), Settings' room under its bar fixed, whichever page is the taller.
    #[test]
    fn back_sits_where_the_cog_was() {
        assert_eq!(spot_lift(Page::Settings, 1), spot_lift(Page::Chat, 1));
        assert_eq!(spot_lift(Page::Chat, 1), 35.0);
    }

    /// Her menu's ATTENTION row, a Settings change and a `settings reload`, mixed with the
    /// writes in flight: each change is in memory only once its write is, and after the last
    /// one what she runs with is what the file says.
    #[test]
    fn menu_page_and_reload_end_where_the_file_is() {
        let d = std::env::temp_dir().join(format!("sa-line-prefs-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("settings.toml");
        std::fs::write(&f, "# his\n[brain]\nattention = \"mix\"\n[voice]\nwake_word = true\n[ragdoll]\nstrength = 0.4\n").unwrap();
        let load = || Settings::parse(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let mut mem = load();
        let mut q: WriteQueue<Pref> = WriteQueue::default();
        let t0 = std::time::Instant::now();
        // The menu twice before anything is written: two steps, MIX → HANG ONLY → OFF.
        for _ in 0..2 {
            let a = next_attention(q.pending(), mem.brain.attention);
            q.push(vec![Pref::Attention(a).edit()], Pref::Attention(a));
        }
        // The page: "HEY CAPRI" off.
        q.push(vec![Pref::Wake(false).edit()], Pref::Wake(false));
        assert_eq!((mem.brain.attention, mem.voice.wake_word), (Attention::Mix, true), "nothing applied before it is written");
        let run = |q: &mut WriteQueue<Pref>, mem: &mut Settings| {
            let (n, edits) = q.start(t0).unwrap();
            let r = write_at(&f, &edits);
            let (p, reload) = q.finish(n).unwrap();
            if r.is_ok() {
                p.apply(mem);
            }
            if reload {
                *mem = load();
            }
        };
        run(&mut q, &mut mem);
        assert_eq!(mem.brain.attention, Attention::HangOnly);
        // He edits the file by hand and reloads while two writes are still to go: memory is
        // the file now, and the reload is owed again after the last write.
        let text = std::fs::read_to_string(&f).unwrap().replace("strength = 0.4", "strength = 0.75");
        std::fs::write(&f, text).unwrap();
        mem = load();
        assert!(q.reload());
        run(&mut q, &mut mem);
        assert_eq!(mem.brain.attention, Attention::Off);
        assert!(mem.voice.wake_word, "the page's change isn't in yet");
        run(&mut q, &mut mem);
        assert!(!q.busy());
        assert_eq!(mem, load(), "memory is the file");
        assert_eq!((mem.brain.attention, mem.voice.wake_word, mem.ragdoll.strength), (Attention::Off, false, 0.75));
        assert!(std::fs::read_to_string(&f).unwrap().starts_with("# his\n"));
        // A write refused (read-only): nothing applied, memory still the file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
            let a = next_attention(q.pending(), mem.brain.attention);
            q.push(vec![Pref::Attention(a).edit()], Pref::Attention(a));
            run(&mut q, &mut mem);
            assert_eq!(mem, load());
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        std::fs::remove_dir_all(&d).ok();
    }
}
