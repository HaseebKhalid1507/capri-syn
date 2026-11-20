//! The Line's second page: her Settings (SPEC-settings §1–2; PR 1 the shell, PR 3 her brain). The
//! cog left of the field (or `ctrl+,`) flips the Line between chat and Settings; Esc goes back one
//! step. Settings' BRAIN row opens the Brain screen (`Page::Brain`): three ways to give her one,
//! none first (SIGN IN with an account, API KEY, RUN LOCAL), and MODEL, a menu of every model
//! she can switch to. With no brain her summon opens it.
//! Every value is a `settings.toml` key the avatar writes (`Settings::write`) and applies
//! (`settings_reload`): the Line only shows what she runs with ([`Prefs`]) and asks for a
//! change ([`Pref`]).

use super::{Ask, Line, Mode, PAD};
use crate::provider::signin::Signing;
use gpui_kit::component::input::{Input, InputEvent, InputState};
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
    /// `CAPRI 1.1.0 · SYNAPS 0.12.0 · SONNET 4.6`.
    pub about: String,
    /// Her BRAIN: the model she thinks with and whether it has a login (`brain` above is her
    /// behaviour engine's switch).
    pub mind: provider::Status,
    /// Her one line: a change made, or why it wasn't.
    pub says: Option<String>,
    /// Replies that came while Settings was up (unseen: `N NEW`).
    pub new: u32,
    /// A sign-in she runs (or ran, and it failed): its row says so.
    pub signing: Option<Signing>,
}

/// A change he asks for (on the page, or her menu's ATTENTION row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    Attention(Attention),
    Wake(bool),
    /// Her own session's model (`[session] model`), always written out: a removed key wouldn't
    /// move a live session back. One of `provider::ALL`'s models, or a local server's
    /// (`provider::local_model`).
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
    /// One of the Brain screen's lists opened in place (None: the open one closed).
    Fold(Option<Fold>),
    /// Signing in to this account, run by her with no window (`signin`): Synaps opens his
    /// browser.
    SignIn(&'static Provider),
    /// The sign-in running stops (its process killed).
    Cancel,
    /// An API key for this service: her masked field opens, its key goes to Synaps on its stdin.
    Key(&'static Provider),
    /// RUN LOCAL with no local model to pick. Until her Ollama setup (PR 5): the steps, opened.
    Local,
    /// The sign-in running's own page, opened again (his browser didn't): `signin::allowed`.
    Open(String),
}

/// API KEY's field, open for one service: masked, never logged, never kept. It goes with its
/// subscription, its text cleared first (`Line::keying_off`).
pub(super) struct Keying {
    pub to: &'static Provider,
    pub field: Entity<InputState>,
    _sub: Subscription,
}

/// The Brain screen's lists that open in place, one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// SIGN IN's accounts.
    SignIn,
    /// API KEY's services.
    Key,
    /// RUN LOCAL's models: the local server's (MODEL's LOCAL group).
    Local,
    /// MODEL's menu.
    Model,
}

/// The Brain screen's rows, top to bottom: three ways in, none first by favour, then MODEL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainRow {
    /// SIGN IN: an account (a subscription), its list opened in place.
    SignIn,
    /// One of SIGN IN's accounts (`Status::accounts`, its index).
    Account(usize),
    /// Under the account signing in: its own page, to open again (`sign_in_page`).
    Page,
    /// API KEY: one of Synaps' API-key services, its list opened in place.
    Key,
    /// One of API KEY's services (`provider::services`, its index).
    Service(usize),
    /// RUN LOCAL: a model on his machine; with the local server's models, their list opened in
    /// place.
    Local,
    /// One of RUN LOCAL's models (`local_picks`, its index).
    LocalPick(usize),
    /// MODEL: every model she can switch to, its menu opened in place.
    Model,
    /// One of MODEL's models (`Status::menu`, its index across the groups).
    Pick(usize),
}

impl BrainRow {
    /// The list a row heads or is in.
    pub fn fold(self) -> Option<Fold> {
        match self {
            BrainRow::SignIn | BrainRow::Account(_) | BrainRow::Page => Some(Fold::SignIn),
            BrainRow::Key | BrainRow::Service(_) => Some(Fold::Key),
            BrainRow::Local | BrainRow::LocalPick(_) => Some(Fold::Local),
            BrainRow::Model | BrainRow::Pick(_) => Some(Fold::Model),
        }
    }

    /// The row heading `f`.
    fn head(f: Fold) -> BrainRow {
        match f {
            Fold::SignIn => BrainRow::SignIn,
            Fold::Key => BrainRow::Key,
            Fold::Local => BrainRow::Local,
            Fold::Model => BrainRow::Model,
        }
    }

    /// One of a list's own rows (not its head).
    pub fn inside(self) -> bool {
        matches!(self, BrainRow::Account(_) | BrainRow::Page | BrainRow::Service(_) | BrainRow::LocalPick(_) | BrainRow::Pick(_))
    }

    /// A row that heads a list.
    fn heads(self) -> bool {
        matches!(self, BrainRow::SignIn | BrainRow::Key | BrainRow::Local | BrainRow::Model)
    }
}

/// MODEL's menu, flat: its models in the groups' order.
pub fn picks(m: &provider::Status) -> Vec<provider::Choice> {
    m.menu().into_iter().flat_map(|g| g.models).collect()
}

/// RUN LOCAL's list: the local server's models (MODEL's LOCAL group).
pub fn local_picks(m: &provider::Status) -> Vec<provider::Choice> {
    let local = provider::LOCAL.name.to_uppercase();
    m.menu().into_iter().find(|g| g.name == local).map_or_else(Vec::new, |g| g.models)
}

/// RUN LOCAL opens its list: a server here with models to pick, and her model free to change.
/// Else its ⏎ opens the steps.
fn local_opens(m: &provider::Status) -> bool {
    m.lock.is_none() && local_picks(m).iter().any(|c| c.pick.is_some())
}

/// The sign-in running's own page (its provider's, `signin::allowed`: as parsed), to open again.
pub fn sign_in_page(p: &Prefs) -> Option<(&'static Provider, String)> {
    let s = p.signing.as_ref().filter(|s| s.failed.is_none())?;
    let to = provider::by_id(s.id)?;
    s.url.as_deref().and_then(|u| provider::signin::allowed(to, u)).map(|u| (to, u))
}

/// The Brain screen's rows (`fold`: the list open). MODEL once she has a model (attached).
pub fn brain_rows(p: &Prefs, fold: Option<Fold>) -> Vec<BrainRow> {
    let m = &p.mind;
    let mut v = vec![BrainRow::SignIn];
    if fold == Some(Fold::SignIn) {
        let page = sign_in_page(p).map(|(to, _)| to.id);
        for (i, a) in m.accounts().iter().enumerate() {
            v.push(BrainRow::Account(i));
            if page == Some(a.id) {
                v.push(BrainRow::Page);
            }
        }
    }
    v.push(BrainRow::Key);
    if fold == Some(Fold::Key) {
        v.extend((0..provider::services().count()).map(BrainRow::Service));
    }
    v.push(BrainRow::Local);
    if fold == Some(Fold::Local) {
        v.extend((0..local_picks(m).len()).map(BrainRow::LocalPick));
    }
    if m.model.is_some() {
        v.push(BrainRow::Model);
        if fold == Some(Fold::Model) {
            v.extend((0..picks(m).len()).map(BrainRow::Pick));
        }
    }
    v
}

/// Where the Brain screen opens: SIGN IN picked, every list closed; her login ran out (SIGN IN
/// AGAIN ▸ brought him here), her own account's row (or her service's), its list open.
pub fn brain_landing(p: &Prefs) -> (usize, Option<Fold>) {
    let m = &p.mind;
    let Some(to) = m.provider().filter(|_| m.expired && m.here) else { return (0, None) };
    let (fold, row) = match to.kind {
        Kind::Key => (Fold::Key, provider::services().position(|q| q == to).map(BrainRow::Service)),
        Kind::Local => return (0, None),
        _ => (Fold::SignIn, m.accounts().iter().position(|&q| q == to).map(BrainRow::Account)),
    };
    let Some(row) = row else { return (0, None) };
    let rows = brain_rows(p, Some(fold));
    rows.iter().position(|&r| r == row).map_or((0, None), |i| (i, Some(fold)))
}

/// The Brain screen's row `sel` once its rows change from `old`'s to `new`'s (a look found
/// Gemini's client id, a login or a local model; her brain changed): the same row, the same
/// account or model, where it is now. A row gone: the same place, kept in the list.
pub fn brain_reselect(old: &Prefs, new: &Prefs, sel: usize, fold: Option<Fold>) -> usize {
    let id = |p: &Prefs, r: BrainRow| match r {
        BrainRow::Account(i) => (BrainRow::Account(0), p.mind.accounts().get(i).map(|a| a.id.to_string())),
        BrainRow::Pick(i) => (BrainRow::Pick(0), picks(&p.mind).get(i).map(|c| c.pick.map_or_else(|| c.label.clone(), str::to_string))),
        BrainRow::LocalPick(i) => (BrainRow::LocalPick(0), local_picks(&p.mind).get(i).map(|c| c.pick.map_or_else(|| c.label.clone(), str::to_string))),
        r => (r, None),
    };
    let now = brain_rows(new, fold);
    let keep = sel.min(now.len() - 1);
    match brain_rows(old, fold).get(sel) {
        Some(&r) => {
            let was = id(old, r);
            now.iter().position(|&x| id(new, x) == was).unwrap_or(keep)
        }
        None => keep,
    }
}

/// The sign-in running (or failed) for `to`, if it's that one.
fn signing<'a>(p: &'a Prefs, to: &Provider) -> Option<&'a Signing> {
    p.signing.as_ref().filter(|s| s.id == to.id)
}

/// An account's or a service's row: what ⏎ on it does. One with a login here, not hers: switch
/// her to it (its default model). Without one: sign in (an account: in his browser; a service:
/// its key in her field), not for a daemon on another machine (a sign-in here wouldn't reach
/// it), nor Grok on Windows (`signin::needs_newer`). Hers, signed in: nothing; its login ran
/// out: sign in again, the same way. Its sign-in running: stop it.
fn use_provider(p: &Prefs, to: &'static Provider) -> Act {
    let m = &p.mind;
    if signing(p, to).is_some_and(|s| s.failed.is_none()) {
        return Act::Cancel;
    }
    let sign_in = || match to.kind {
        Kind::Key => Act::Key(to),
        _ if provider::signin::needs_newer(m.windows, to) => Act::Nothing,
        _ => Act::SignIn(to),
    };
    let hers = m.provider() == Some(to);
    match (m.has(to), m.here) {
        (_, true) if hers && m.expired => sign_in(),
        (true, _) if hers => Act::Nothing,
        (true, _) if m.lock.is_none() => to.models.first().map_or(Act::Nothing, |d| Act::Pref(Pref::Model(d.0))),
        (false, true) => sign_in(),
        _ => Act::Nothing,
    }
}

/// PgUp/PgDn move this many rows; Home/End go to the ends (of the list open, when he's in it).
const PAGE: usize = 7;

/// MODEL's ⏎: its menu opens, unless there's nothing to pick (nothing signed in, no local
/// model) or her model is locked (`--model`, `--session`).
fn model_opens(p: &Prefs) -> bool {
    p.mind.lock.is_none() && picks(&p.mind).iter().any(|c| c.pick.is_some() && !c.on)
}

/// A key on the Brain screen at row `sel` (`fold`: the list open): the selection after it, and
/// what it does. ↑↓ move; ⏎ (→, Space) takes a way in, opens or closes a list, picks a model;
/// ← closes the list open.
pub fn brain_key(key: &str, sel: usize, p: &Prefs, fold: Option<Fold>) -> (usize, Act) {
    let rows = brain_rows(p, fold);
    let sel = sel.min(rows.len() - 1);
    let row = rows[sel];
    // The selection once `f` is the list open: on `on`, where that row is then.
    let at = |f: Option<Fold>, on: BrainRow| brain_rows(p, f).iter().position(|r| *r == on).unwrap_or(0);
    let go = matches!(key, "enter" | "right" | "space");
    let head = |f: Fold| BrainRow::head(f);
    // A head's list opens: MODEL's and RUN LOCAL's only with something to pick.
    let opens = |r: BrainRow| match r {
        BrainRow::Model => model_opens(p),
        BrainRow::Local => local_opens(&p.mind),
        r => r.heads(),
    };
    // PgUp/PgDn/Home/End: within the list open while he's in it (or on its head), else the screen.
    let (lo, hi) = match fold.filter(|&f| row.fold() == Some(f)) {
        Some(f) => {
            let mine: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.inside() && r.fold() == Some(f)).map(|(i, _)| i).collect();
            (mine.first().copied().unwrap_or(0), mine.last().copied().unwrap_or(rows.len() - 1))
        }
        None => (0, rows.len() - 1),
    };
    let act = match (key, row) {
        ("up", _) => return (sel.saturating_sub(1), Act::Nothing),
        ("down", _) => return ((sel + 1).min(rows.len() - 1), Act::Nothing),
        ("home", _) => return (lo, Act::Nothing),
        ("end", _) => return (hi, Act::Nothing),
        ("pageup", _) => return (sel.saturating_sub(PAGE).max(lo), Act::Nothing),
        ("pagedown", _) => return ((sel + PAGE).clamp(lo, hi), Act::Nothing),
        ("left", r) if fold.is_some() && r.fold() == fold => {
            let f = fold.unwrap_or(Fold::SignIn);
            return (at(None, head(f)), Act::Fold(None));
        }
        (_, BrainRow::Model) if go && !model_opens(p) => Act::Nothing,
        ("right", r) if r.heads() && opens(r) && fold != r.fold() => return (at(r.fold(), r), Act::Fold(r.fold())),
        ("enter" | "space", r) if r.heads() && (opens(r) || fold == r.fold()) => {
            let f = if fold == r.fold() { None } else { r.fold() };
            return (at(f, r), Act::Fold(f));
        }
        (_, BrainRow::Page) if go => sign_in_page(p).map_or(Act::Nothing, |(_, u)| Act::Open(u)),
        (_, BrainRow::Account(i)) if go => p.mind.accounts().get(i).map_or(Act::Nothing, |&to| use_provider(p, to)),
        (_, BrainRow::Service(i)) if go => provider::services().nth(i).map_or(Act::Nothing, |to| use_provider(p, to)),
        (_, BrainRow::Pick(i)) if go => match picks(&p.mind).get(i) {
            Some(c) if !c.on && p.mind.lock.is_none() => match c.pick {
                // Picked: the menu closes on MODEL.
                Some(to) => return (at(None, BrainRow::Model), Act::Pref(Pref::Model(to))),
                None => Act::Nothing,
            },
            _ => Act::Nothing,
        },
        (_, BrainRow::LocalPick(i)) if go => match local_picks(&p.mind).get(i) {
            Some(c) if !c.on && p.mind.lock.is_none() => match c.pick {
                // Picked: the list closes on RUN LOCAL.
                Some(to) => return (at(None, BrainRow::Local), Act::Pref(Pref::Model(to))),
                None => Act::Nothing,
            },
            _ => Act::Nothing,
        },
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

/// The Brain screen's keys for the row picked: what ⏎ does there (signs in, stops a sign-in,
/// switches her, opens or closes a list, picks), ← where it closes one.
pub fn brain_hint(p: &Prefs, sel: usize, fold: Option<Fold>) -> &'static str {
    let rows = brain_rows(p, fold);
    let row = rows[sel.min(rows.len() - 1)];
    let open = fold.is_some() && row.fold() == fold;
    let again = |to: &Provider| p.mind.expired && p.mind.provider() == Some(to);
    match (row, brain_key("enter", sel, p, fold).1, open) {
        (BrainRow::SignIn | BrainRow::Key | BrainRow::Local | BrainRow::Model, Act::Fold(None), _) => "↑↓ pick · ⏎ close · esc back",
        (BrainRow::SignIn | BrainRow::Key | BrainRow::Local | BrainRow::Model, Act::Fold(Some(_)), _) => "↑↓ pick · ⏎ open · esc back",
        (_, Act::SignIn(to), true) if again(to) => "↑↓ pick · ⏎ sign in again · ← close · esc back",
        (_, Act::SignIn(_), true) => "↑↓ pick · ⏎ sign in · ← close · esc back",
        (_, Act::Open(_), _) => "↑↓ pick · ⏎ open it in your browser · ← close · esc back",
        (_, Act::Cancel, true) => "↑↓ pick · ⏎ stop signing in · ← close · esc back",
        (_, Act::Key(_), true) => "↑↓ pick · ⏎ paste a key · ← close · esc back",
        (BrainRow::Pick(_) | BrainRow::LocalPick(_), Act::Pref(_), _) => "↑↓ pgup pgdn · ⏎ use it · ← close · esc back",
        (_, Act::Pref(_), true) => "↑↓ pick · ⏎ switch to it · ← close · esc back",
        (_, _, true) => "↑↓ pick · ← close · esc back",
        (_, Act::Local, _) => "↑↓ pick · ⏎ the steps · esc back",
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
/// A list open on the Brain screen shows this much before it scrolls (SIGN IN's seven accounts).
const FOLD_MAX: f32 = ROW_H * 7.5;
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
        // API KEY's field goes with the page (what was typed in it too).
        self.keying_off(window, cx);
        // A held ALLOW lets go: its prompt is off screen now (or was, a moment ago).
        self.hold_at = None;
        // What was said on the page left goes with it (COPIED, ESC AGAIN TO ABORT), and so does
        // a pending Esc's close: a cog click in its beat closed the Line on Settings.
        page_left(&mut self.note, &mut self.esc_at, self.nav.page);
        // The page left was this tall: both stay at least that (chat's draft or prompt strip
        // let go of it when they go: `relax`). Not the Brain screen's OTHER, open: it's a list
        // shown for a moment, not a height to keep.
        let other_open = self.nav.left == Page::Brain && self.fold.is_some();
        if self.height > self.keep_h && !other_open {
            self.keep_h = self.height;
            self.keep_chat = self.nav.left == Page::Chat;
        }
        if self.nav.page == Page::Settings {
            self.sel = 0;
        } else if self.nav.page == Page::Brain {
            // The ways in, SIGN IN picked, every list closed; her login ran out: her own row.
            (self.bsel, self.fold) = self.snap.prefs.as_ref().map_or((0, None), brain_landing);
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
        // API KEY's field open: its keys are its own (⏎ saves); Esc closes it.
        if self.keying.is_some() {
            if k.key == "escape" {
                self.keying_off(window, cx);
                self.refocus(window, cx);
                cx.notify();
                return true;
            }
            return false;
        }
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
                    let (sel, act) = brain_key(key, self.bsel, &prefs, self.fold);
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
                // A model picked from MODEL's menu (or RUN LOCAL's): it closes (`brain_key` is on
                // its head already).
                if matches!(p, Pref::Model(_)) && matches!(self.fold, Some(Fold::Model | Fold::Local)) {
                    self.fold = None;
                }
                // Shown as asked at once: a second key steps on from it, not from the value
                // the first one's write hasn't changed yet (her next snapshot says the same).
                if let Some(prefs) = &mut self.snap.prefs {
                    let was = prefs.clone();
                    p.show(prefs);
                    self.bsel = brain_reselect(&was, prefs, self.bsel, self.fold);
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
            Act::Fold(f) => {
                self.fold = f;
                self.keying_off(window, cx);
                self.refocus(window, cx);
            }
            Act::SignIn(to) => {
                // Shown at once; her snapshot says the same, or why not.
                if let Some(prefs) = &mut self.snap.prefs {
                    prefs.signing = Some(Signing::new(to.id));
                }
                (self.on_ask)(Ask::SignIn(to), cx).ok();
            }
            Act::Cancel => {
                (self.on_ask)(Ask::SignInCancel, cx).ok();
            }
            Act::Key(to) => {
                self.keying_off(window, cx);
                let f = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(format!("your {} API key (hidden, never logged)", to.name)));
                let sub = cx.subscribe_in(&f, window, move |this: &mut Line, st: &Entity<InputState>, ev: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = ev {
                        let key = crate::session::Secret(st.read(cx).value().trim().to_string());
                        if !key.0.is_empty() {
                            this.keying_off(window, cx);
                            if let Some(prefs) = &mut this.snap.prefs {
                                prefs.signing = Some(Signing::new(to.id));
                            }
                            (this.on_ask)(Ask::Key(to, key), cx).ok();
                            this.refocus(window, cx);
                            cx.notify();
                        }
                    }
                });
                self.keying = Some(Keying { to, field: f, _sub: sub });
                self.refocus(window, cx);
            }
            Act::Local => {
                (self.on_ask)(Ask::OpenUrl(LOCAL_STEPS.into()), cx).ok();
                self.say(page, "OPENED THE STEPS", cx);
            }
            Act::Open(u) => {
                (self.on_ask)(Ask::OpenUrl(u), cx).ok();
                self.say(page, "OPENED THE SIGN-IN PAGE", cx);
            }
            Act::Nothing => {}
        }
    }

    /// API KEY's field closed: its text cleared before it goes, and its subscription with it.
    pub(super) fn keying_off(&mut self, window: &mut Window, cx: &mut Context<Line>) {
        if let Some(k) = self.keying.take() {
            k.field.update(cx, |s, cx| s.set_value("", window, cx));
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
                            if let Some(prefs) = this.snap.prefs.clone() {
                                let (sel, act) = brain_key("enter", ix, &prefs, this.fold);
                                this.bsel = sel;
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
            let rows = brain_rows(&p, self.fold);
            let sel = self.bsel.min(rows.len() - 1);
            out.push(section("WAYS IN"));
            // A list open: its rows in a box of their own, scrolled to the row picked.
            let mut fold: Vec<AnyElement> = Vec::new();
            let mut fold_at: Option<usize> = None;
            let menu = m.menu();
            let picks: Vec<(usize, provider::Choice)> = menu.iter().enumerate().flat_map(|(g, gr)| gr.models.iter().cloned().map(move |c| (g, c))).collect();
            let locals = local_picks(m);
            let flush = |fold: &mut Vec<AnyElement>, out: &mut Vec<AnyElement>| {
                if !fold.is_empty() {
                    out.push(
                        div()
                            .id("brain-fold")
                            .max_h(px(FOLD_MAX))
                            .overflow_y_scroll()
                            .track_scroll(&self.fold_scroll)
                            .children(std::mem::take(fold))
                            .into_any_element(),
                    );
                }
            };
            for (ix, r) in rows.iter().enumerate() {
                let on = ix == sel;
                if !r.inside() {
                    flush(&mut fold, &mut out);
                }
                if *r == BrainRow::Model {
                    out.push(section("MODEL"));
                }
                // A way in: its name, what it takes (or ✓ where she has it), ▸ (▾: open).
                let way = |name: &str, blurb: String, mark: Option<&'static str>, live: bool, open: Option<bool>| {
                    let c = if !live { theme::mix(pal.dim, pal.panel, 0.3) } else if mark == Some("check") || mark == Some("disc") { pal.accent } else { small_c };
                    let arrow = if open == Some(true) { "down" } else { "play" };
                    row(ix, on)
                        .child(label(name, on && live, None, 230.0))
                        .child(div().flex_1())
                        .child(word(blurb, mark, c))
                        .child(div().w(px(14.0)).flex().justify_center().child(theme::glyph_at(arrow, if live { pal.accent } else { c }, pal.panel, 9.0)))
                        .into_any_element()
                };
                // ✓ on a provider with a login here; IN USE on hers (its login ran out: a red dot
                // and ⏎ to sign in again; Grok on Windows can't, `signin::needs_newer`).
                let state = |to: &'static Provider, blurb: &str| -> (String, Option<&'static str>) {
                    match (m.has(to), m.provider() == Some(to)) {
                        (_, true) if m.expired && m.here && to.kind == Kind::Key => ("KEY TURNED DOWN · ⏎ NEW KEY".into(), Some("disc")),
                        (_, true) if m.expired && m.here && provider::signin::needs_newer(m.windows, to) => ("LOGGED OUT · NEEDS A NEWER SYNAPS".into(), Some("disc")),
                        (_, true) if m.expired && m.here => ("LOGGED OUT · ⏎ SIGN IN AGAIN".into(), Some("disc")),
                        (true, true) => ("IN USE".into(), Some("check")),
                        (true, false) => ("SIGNED IN".into(), Some("check")),
                        _ if !m.here => ("ON ITS MACHINE".into(), None),
                        _ => (blurb.into(), None),
                    }
                };
                // Its sign-in, running or failed: in place of what it takes.
                let running = |to: &'static Provider| p.signing.as_ref().filter(|s| s.id == to.id);
                let target = if r.inside() { &mut fold } else { &mut out };
                if r.inside() && on {
                    fold_at = Some(target.len());
                }
                match *r {
                    BrainRow::SignIn => {
                        let accounts = m.accounts();
                        let n = accounts.iter().filter(|to| m.has(to)).count();
                        let (b, mark) = match m.provider().filter(|to| to.kind == Kind::OAuth && (m.has(to) || m.expired)) {
                            Some(to) if m.expired => (format!("LOGGED OUT · {}", to.name.to_uppercase()), Some("disc")),
                            Some(to) => (format!("IN USE · {}", to.name.to_uppercase()), Some("check")),
                            None if n > 0 => (format!("{n} SIGNED IN"), Some("check")),
                            None if !m.here => ("ON ITS MACHINE".into(), None),
                            None => (format!("{} +{}", accounts.iter().take(3).map(|a| a.name).collect::<Vec<_>>().join(" · "), accounts.len().saturating_sub(3)), None),
                        };
                        target.push(way("SIGN IN", b, mark, true, Some(self.fold == Some(Fold::SignIn))));
                    }
                    BrainRow::Account(i) => {
                        let Some(&to) = m.accounts().get(i) else { continue };
                        let blurb = match to.id {
                            "anthropic" => "Pro or Max · browser",
                            "openai-codex" => "Plus or Pro · browser",
                            "github-copilot" => "browser · a code to type",
                            _ => "browser",
                        };
                        let name = format!("   {}", to.name.to_uppercase());
                        match running(to) {
                            // Its code, large: Copilot's he types on GitHub's page (it's copied);
                            // Kimi's and Muse's are on theirs already, to check.
                            Some(sg) if sg.failed.is_none() && sg.code.is_some() => {
                                let code = sg.code.clone().unwrap_or_default();
                                let how = if to.id == "github-copilot" { "COPIED · TYPE IT" } else { "ON THE PAGE TOO" };
                                target.push(
                                    row(ix, on)
                                        .child(label(&name, on, None, 230.0))
                                        .child(div().flex_1())
                                        .child(div().text_size(px(10.5)).font_weight(FontWeight::BOLD).text_color(rgb(small_c)).child(tracked(how, 1.2)))
                                        .child(div().font_family(super::MONO).text_size(px(19.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.accent)).child(tracked(&code, 2.0)))
                                        .into_any_element(),
                                );
                            }
                            Some(sg) if sg.failed.is_none() => target.push(way(&name, "SIGNING IN… · ⏎ STOP".into(), None, true, None)),
                            Some(_) => target.push(way(&name, "DIDN'T WORK · ⏎ AGAIN".into(), Some("disc"), true, None)),
                            // Grok on Windows: Synaps 0.12.0 can't open its page whole.
                            None if !m.has(to) && m.here && provider::signin::needs_newer(m.windows, to) => target.push(way(&name, "NEEDS A NEWER SYNAPS".into(), None, false, None)),
                            None => {
                                let (b, mark) = state(to, blurb);
                                target.push(way(&name, b, mark, m.here || m.has(to), None));
                            }
                        }
                    }
                    // Under the account signing in: its page, for when his browser didn't open
                    // (Copilot's: GitHub's, where its code goes).
                    BrainRow::Page => {
                        let github = sign_in_page(&p).is_some_and(|(to, _)| to.id == "github-copilot");
                        target.push(way("      NO BROWSER?", if github { "OPEN GITHUB'S PAGE" } else { "OPEN THE SIGN-IN PAGE" }.into(), None, true, None));
                    }
                    BrainRow::Key => {
                        let (b, mark) = key_word(m);
                        target.push(way("API KEY", b, mark, true, Some(self.fold == Some(Fold::Key))));
                    }
                    BrainRow::Service(i) => {
                        let Some(to) = provider::services().nth(i) else { continue };
                        let name = format!("   {}", to.name.to_uppercase());
                        match running(to) {
                            Some(sg) if sg.failed.is_none() => target.push(way(&name, "SAVING…".into(), None, true, None)),
                            Some(_) => target.push(way(&name, "DIDN'T TAKE · ⏎ AGAIN".into(), Some("disc"), true, None)),
                            None => {
                                let (b, mark) = state(to, "paste a key");
                                let (b, mark) = if mark.is_some() && b == "SIGNED IN" { ("KEY SET".to_string(), mark) } else { (b, mark) };
                                target.push(way(&name, b, mark, m.here || m.has(to), None));
                            }
                        }
                        // Its key, typed: masked, never logged, never kept.
                        if let Some(Keying { field: f, .. }) = self.keying.as_ref().filter(|k| k.to.id == to.id) {
                            target.push(
                                div()
                                    .px(px(PAD))
                                    .py(px(4.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(div().flex_1().relative().child(theme::backdrop(Cut::Corners(7.0), Fill::framed(pal.field, pal.accent, 1.5))).px(px(10.0)).py(px(4.0)).child(Input::new(f).appearance(false)))
                                    .child(div().text_size(px(11.5)).text_color(rgb(pal.dim)).whitespace_nowrap().child("⏎ save · esc cancel · never logged"))
                                    .into_any_element(),
                            );
                        }
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
                        let opens = local_opens(m);
                        target.push(way("RUN LOCAL", b, mark, m.here, opens.then_some(self.fold == Some(Fold::Local))));
                    }
                    BrainRow::LocalPick(i) => {
                        let Some(c) = locals.get(i) else { continue };
                        let mut r = row(ix, on).child(label(&format!("   {}", c.label), on, None, 420.0)).child(div().flex_1());
                        if c.on {
                            r = r.child(word("IN USE".into(), Some("check"), pal.accent));
                        }
                        target.push(r.into_any_element());
                    }
                    BrainRow::Model => {
                        let why = match m.lock {
                            Some(provider::Lock::Cli) => Some("set by --model"),
                            Some(provider::Lock::Theirs) => Some("their session"),
                            None => None,
                        };
                        let opens = model_opens(&p);
                        let now = brain_word(m);
                        let mut r = row(ix, on).child(label("MODEL", on, None, 150.0)).child(div().flex_1()).children(why.map(lock));
                        r = if menu.is_empty() {
                            // Nothing signed in, no local model: nothing to switch to.
                            r.child(word("sign in first".into(), None, small_c))
                        } else {
                            r.child(word(now, None, if opens { pal.text } else { small_c }))
                        };
                        let arrow = if self.fold == Some(Fold::Model) { "down" } else { "play" };
                        target.push(r.child(div().w(px(14.0)).flex().justify_center().when(opens, |d| d.child(theme::glyph_at(arrow, pal.accent, pal.panel, 9.0)))).into_any_element());
                    }
                    BrainRow::Pick(i) => {
                        let Some((g, c)) = picks.get(i) else { continue };
                        // Its provider's name over its first model.
                        if picks.iter().position(|(x, _)| x == g) == Some(i) {
                            target.push(
                                div()
                                    .px(px(PAD + 18.0))
                                    .pt(px(5.0))
                                    .pb(px(2.0))
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(pal.dim))
                                    .child(tracked(&menu[*g].name, 1.6))
                                    .into_any_element(),
                            );
                            if on {
                                fold_at = Some(target.len());
                            }
                        }
                        let mut r = row(ix, on).child(label(&format!("      {}", c.label), on, None, 420.0)).child(div().flex_1());
                        if c.on {
                            r = r.child(word("IN USE".into(), Some("check"), pal.accent));
                        }
                        target.push(r.into_any_element());
                    }
                }
            }
            flush(&mut fold, &mut out);
            // The row picked in view: once per move (a wheel scroll stays where he put it).
            if let Some(k) = fold_at
                && self.fold_seen.get() != Some((self.fold, sel))
            {
                self.fold_scroll.scroll_to_item(k);
                self.fold_seen.set(Some((self.fold, sel)));
            }
            // Where his login goes (and where it doesn't: a daemon on another machine). A sign-in's
            // page, for when his browser didn't open, is a row under its account (`BrainRow::Page`).
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
                        // No brain, or its login ran out: a red dot and SET UP / SIGN IN AGAIN.
                        let none = m.none() || m.expired;
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
                                .child(go(match (m.none(), m.expired) {
                                    (false, true) => button("set-brain", "SIGN IN AGAIN ▸", true),
                                    (true, _) => button("set-brain", "SET UP ▸", true),
                                    _ => button("set-brain", "CHANGE ▸", false),
                                }))
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
        let hint = if brain { brain_hint(&p, self.bsel, self.fold) } else { hint(&p, self.sel) };
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
        _ if m.expired && m.provider().is_some_and(|p| p.kind == Kind::Key) => "My key was turned down. Paste a new one under API KEY.",
        _ if m.expired && m.provider().is_some_and(|p| provider::signin::needs_newer(m.windows, p)) => {
            "My Grok login ran out, and this Synaps can't sign in to Grok on Windows. Pick another brain for now."
        }
        _ if m.expired => "My login ran out. Sign me in again under SIGN IN.",
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
    let set: Vec<&'static Provider> = provider::services().filter(|q| m.has(q)).collect();
    if let Some(to) = m.provider().filter(|p| set.contains(p)) {
        // Others set too: MODEL's menu has theirs.
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
    if m.expired {
        w.push_str(" · LOGGED OUT");
    }
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
    use super::{Act, BrainRow, FLASH, Fold, MenuSave, Mode, Nav, Note, Page, Pref, Prefs, SetRow, Signing, asked, asked_model, brain_hint, brain_key, brain_landing, brain_line, brain_reselect, brain_rows, brain_word, esc_closes, hint, hold_fires, key_word, list_key, next_attention, page_left, set_rows, spot_lift};
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
        Prefs { attention: Attention::Mix, brain: true, wake: true, voice: true, about: "CAPRI 1.1.0".into(), mind: mind("claude-sonnet-4-6", Some(true)), says: None, new: 0, signing: None }
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

    /// No brain: three ways in, none first (SIGN IN, API KEY, RUN LOCAL). SIGN IN opens in
    /// place, its accounts by name (Gemini only with its client id, a login or hers); ⏎ on one
    /// signs in from her; ← closes it. MODEL: nothing to switch to, sign in first.
    #[test]
    fn no_brain_three_ways_in_none_first() {
        let p = Prefs { mind: mind("claude-sonnet-4-6", Some(false)), ..prefs() };
        let in_ = Some(Fold::SignIn);
        assert_eq!(brain_rows(&p, None), vec![BrainRow::SignIn, BrainRow::Key, BrainRow::Local, BrainRow::Model]);
        assert_eq!(brain_key("enter", 0, &p, None), (0, Act::Fold(in_)));
        assert_eq!(brain_key("right", 0, &p, None), (0, Act::Fold(in_)));
        assert_eq!(brain_key("space", 0, &p, in_), (0, Act::Fold(None)), "⏎ (Space) on it, open: it closes");
        assert_eq!(brain_rows(&p, in_).len(), 4 + 6);
        let name = |p: &Prefs| p.mind.accounts().iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(name(&p), ["ChatGPT", "Claude", "Copilot", "Grok", "Kimi", "Muse"], "by name; Gemini needs its client id");
        let by = |id: &str| provider::by_id(id).unwrap();
        assert_eq!(brain_key("enter", 1, &p, in_), (1, Act::SignIn(by("openai-codex"))));
        assert_eq!(brain_key("enter", 2, &p, in_), (2, Act::SignIn(by("anthropic"))));
        assert_eq!(brain_key("enter", 4, &p, in_), (4, Act::SignIn(by("xai-auth"))), "Grok, where Gemini would be");
        // Windows: Synaps 0.12.0 can't open Grok's page whole (it prints none): its row does
        // nothing (it says it needs a newer Synaps). The others as anywhere.
        let mut w = p.clone();
        w.mind.windows = true;
        assert_eq!(brain_key("enter", 4, &w, in_), (4, Act::Nothing));
        assert_eq!(brain_hint(&w, 4, in_), "↑↓ pick · ← close · esc back");
        assert_eq!(brain_key("enter", 2, &w, in_), (2, Act::SignIn(by("anthropic"))));
        w.mind.signed.push("xai-auth");
        assert_eq!(brain_key("enter", 4, &w, in_), (4, Act::Pref(Pref::Model("xai-auth/grok-4.3"))), "a login on file: a switch, no sign-in");
        // Hers, its login ran out: no sign-in again either; her line says so, not SIGN IN.
        let mut g = Prefs { mind: mind("xai-auth/grok-4.3", Some(true)), ..prefs() };
        g.mind.windows = true;
        g.mind.expired = true;
        assert_eq!(brain_key("enter", 4, &g, in_), (4, Act::Nothing));
        assert_eq!(brain_hint(&g, 4, in_), "↑↓ pick · ← close · esc back");
        assert_eq!(brain_line(&g.mind), "My Grok login ran out, and this Synaps can't sign in to Grok on Windows. Pick another brain for now.");
        g.mind.windows = false;
        assert_eq!(brain_key("enter", 4, &g, in_), (4, Act::SignIn(by("xai-auth"))), "elsewhere: signs in again");
        assert_eq!(brain_key("left", 4, &p, in_), (0, Act::Fold(None)), "← closes it, back on SIGN IN");
        assert_eq!(brain_key("down", 9, &p, in_), (9, Act::Nothing));
        // API KEY from under SIGN IN's accounts: SIGN IN closes, API KEY opens, picked.
        assert_eq!(brain_key("enter", 7, &p, in_), (1, Act::Fold(Some(Fold::Key))));
        let keys = brain_rows(&p, Some(Fold::Key));
        assert_eq!((keys[2], keys.len()), (BrainRow::Service(0), 4 + 19));
        assert_eq!(brain_key("enter", 2, &p, Some(Fold::Key)), (2, Act::Key(by("groq"))), "no key: her field for one");
        assert_eq!(brain_key("enter", 2, &p, None), (2, Act::Local));
        // MODEL: nothing signed in, no local model.
        assert!(p.mind.menu().is_empty());
        assert_eq!(brain_key("enter", 3, &p, None), (3, Act::Nothing), "sign in first");
        assert_eq!(brain_hint(&p, 3, None), "↑↓ pick · esc back");
        // Gemini: with its client id set, a login on file, or hers.
        let mut g = p.clone();
        g.mind.gemini = true;
        assert_eq!(name(&g), ["ChatGPT", "Claude", "Copilot", "Gemini", "Grok", "Kimi", "Muse"]);
        assert_eq!(brain_key("enter", 4, &g, in_), (4, Act::SignIn(by("google-gemini"))));
        let mut g = p.clone();
        g.mind.signed.push("google-gemini");
        assert_eq!(name(&g).len(), 7, "signed in already");
        let g = Prefs { mind: mind("google-gemini/gemini-2.5-pro", Some(false)), ..prefs() };
        assert_eq!(name(&g).len(), 7, "hers");
        assert_eq!(brain_hint(&p, 0, None), "↑↓ pick · ⏎ open · esc back");
        assert_eq!(brain_hint(&p, 0, in_), "↑↓ pick · ⏎ close · esc back");
        assert_eq!(brain_hint(&p, 2, in_), "↑↓ pick · ⏎ sign in · ← close · esc back");
        assert_eq!(brain_hint(&p, 2, Some(Fold::Key)), "↑↓ pick · ⏎ paste a key · ← close · esc back");
        assert_eq!(brain_hint(&p, 2, None), "↑↓ pick · ⏎ the steps · esc back");
        assert_eq!(brain_line(&p.mind), "Give me a brain and I'll talk back.");
        assert_eq!(brain_word(&p.mind), "NO BRAIN YET");
    }

    /// Signed in on Claude: MODEL opens its menu in place, every model of every login here and
    /// the local server's, by provider; a pick switches her (`[session] model`) and closes it.
    /// An account with a login switches her to its default model, no sign-in.
    #[test]
    fn with_a_brain_model_menu_and_a_switch() {
        let p = prefs();
        let model = 3;
        let m = Some(Fold::Model);
        assert_eq!(brain_rows(&p, None)[model], BrainRow::Model);
        assert_eq!(brain_key("enter", model, &p, None), (model, Act::Fold(m)));
        let open = brain_rows(&p, m);
        assert_eq!((open.len(), open[4]), (4 + provider::CLAUDE.models.len(), BrainRow::Pick(0)), "all of them, no cap");
        assert_eq!(brain_key("enter", 4, &p, m), (4, Act::Nothing), "hers already");
        assert_eq!(brain_key("enter", 8, &p, m), (model, Act::Pref(Pref::Model("claude-opus-5-5"))), "picked: the menu closes on MODEL");
        assert_eq!(brain_key("left", 6, &p, m), (model, Act::Fold(None)));
        assert_eq!(brain_hint(&p, 8, m), "↑↓ pgup pgdn · ⏎ use it · ← close · esc back");
        // PgUp/PgDn a page, Home/End its ends: within the menu while he's in it (or on MODEL).
        let last = open.len() - 1;
        assert_eq!(brain_key("home", 9, &p, m), (4, Act::Nothing));
        assert_eq!(brain_key("end", 4, &p, m), (last, Act::Nothing));
        assert_eq!(brain_key("pagedown", 4, &p, m).0, (4 + 7).min(last));
        assert_eq!(brain_key("pagedown", last - 1, &p, m).0, last);
        assert_eq!(brain_key("pageup", last, &p, m).0, last - 7);
        assert_eq!(brain_key("pageup", 5, &p, m).0, 4, "never past its first");
        assert_eq!(brain_key("pagedown", model, &p, m).0, 4 + 7 - 1, "on MODEL: into its menu");
        assert_eq!(brain_key("end", 0, &p, None).0, model, "no list open: the screen's ends");
        assert_eq!(brain_key("home", model, &p, None).0, 0);
        assert_eq!(brain_hint(&p, 4, m), "↑↓ pick · ← close · esc back", "hers: nothing to do");
        assert_eq!(brain_hint(&p, model, None), "↑↓ pick · ⏎ open · esc back");
        assert_eq!(brain_word(&p.mind), "CLAUDE · SONNET 4.6");
        assert_eq!(brain_line(&p.mind), "That's what I think with. Swap it any time.");
        // Two accounts and a local model: their models, by provider; ChatGPT's first (by name).
        let mut q = prefs();
        q.mind.signed.push("openai-codex");
        q.mind.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["qwen3:8b".into()])));
        assert_eq!(q.mind.menu().iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["CHATGPT", "CLAUDE", "LOCAL"]);
        let last = brain_rows(&q, m).len() - 1;
        assert_eq!(brain_key("enter", last, &q, m), (model, Act::Pref(Pref::Model("local/qwen3:8b"))));
        assert_eq!(brain_key("enter", 4, &q, m), (model, Act::Pref(Pref::Model("openai-codex/gpt-5.5"))));
        let in_ = Some(Fold::SignIn);
        assert_eq!(brain_key("enter", 1, &q, in_), (1, Act::Pref(Pref::Model("openai-codex/gpt-5.5"))), "ChatGPT's login on file: a switch");
        assert_eq!(brain_hint(&q, 1, in_), "↑↓ pick · ⏎ switch to it · ← close · esc back");
        assert_eq!(brain_key("enter", 2, &q, in_), (2, Act::Nothing), "Claude is hers already");
        // On ChatGPT: Claude's row (its login on file) switches back.
        let mut r = Prefs { mind: mind("openai-codex/gpt-5.5", Some(true)), ..prefs() };
        r.mind.signed.push("anthropic");
        assert_eq!(brain_key("enter", 2, &r, in_), (2, Act::Pref(Pref::Model("claude-sonnet-4-6"))));
        assert_eq!(brain_word(&r.mind), "CHATGPT · GPT-5.5");
        // Locked: started with --model, or riding his session.
        for lock in [Lock::Cli, Lock::Theirs] {
            let mut l = prefs();
            l.mind.lock = Some(lock);
            assert_eq!(brain_key("enter", model, &l, None).1, Act::Nothing, "{lock:?}: MODEL doesn't open");
            assert_eq!(brain_hint(&l, model, None), "↑↓ pick · esc back");
            assert_eq!(brain_key("enter", 5, &l, m).1, Act::Nothing, "nor a pick");
            l.mind.signed.push("openai-codex");
            assert_eq!(brain_key("enter", 1, &l, in_).1, Act::Nothing, "no switch either");
        }
        // His own model, by hand: in the menu, in force, not one to pick again.
        let own = Prefs { mind: mind("claude-opus-4-1", Some(true)), ..prefs() };
        let picks = super::picks(&own.mind);
        let at = picks.iter().position(|c| c.on).unwrap();
        assert_eq!(picks[at].pick, None);
        assert_eq!(brain_key("enter", 4 + at, &own, m).1, Act::Nothing);
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

    /// API KEY's services: one with a key switches her to it (not hers, not locked); one without
    /// opens her field for its key (locked too: a key is no switch); another machine's: nothing.
    #[test]
    fn api_key_services_switch_or_take_a_key() {
        let mut p = prefs();
        p.mind.signed.push("groq");
        let cerebras = provider::by_id("cerebras").unwrap();
        let k = Some(Fold::Key);
        assert_eq!(brain_key("enter", 2, &p, k), (2, Act::Pref(Pref::Model("groq/llama-3.3-70b-versatile"))));
        assert_eq!(brain_hint(&p, 2, k), "↑↓ pick · ⏎ switch to it · ← close · esc back");
        assert_eq!(brain_key("enter", 3, &p, k), (3, Act::Key(cerebras)));
        let mut on = p.clone();
        on.mind.model = Some("groq/llama-3.3-70b-versatile".into());
        assert_eq!(brain_key("enter", 2, &on, k).1, Act::Nothing, "hers");
        let mut locked = p.clone();
        locked.mind.lock = Some(Lock::Cli);
        assert_eq!(brain_key("enter", 2, &locked, k).1, Act::Nothing, "--model: no switch");
        assert_eq!(brain_key("enter", 3, &locked, k).1, Act::Key(cerebras), "a key can still be set");
        let mut far = p.clone();
        far.mind.here = false;
        far.mind.signed.clear();
        assert_eq!(brain_key("enter", 3, &far, k).1, Act::Nothing, "a daemon on another machine");
        assert_eq!(brain_hint(&far, 3, k), "↑↓ pick · ← close · esc back");
    }

    /// A sign-in running: its row stops it; failed, ⏎ tries again; another account's starts its
    /// own (the avatar stops the first).
    #[test]
    fn a_sign_in_running_stops_on_its_row() {
        let mut p = Prefs { mind: mind("claude-sonnet-4-6", Some(false)), ..prefs() };
        let in_ = Some(Fold::SignIn);
        p.signing = Some(Signing::new("anthropic"));
        assert_eq!(brain_key("enter", 2, &p, in_).1, Act::Cancel);
        assert_eq!(brain_hint(&p, 2, in_), "↑↓ pick · ⏎ stop signing in · ← close · esc back");
        assert_eq!(brain_key("enter", 1, &p, in_).1, Act::SignIn(provider::by_id("openai-codex").unwrap()));
        p.signing = Some(Signing { failed: Some("OAuth denied".into()), ..Signing::new("anthropic") });
        assert_eq!(brain_key("enter", 2, &p, in_).1, Act::SignIn(provider::by_id("anthropic").unwrap()), "again");
        // Its own page (its provider's, `signin::allowed`): a row under it opens it, by keys or
        // a click, for when his browser didn't.
        let page = "https://claude.ai/oauth/authorize?code=true&client_id=x&state=y";
        p.signing = Some(Signing { url: Some(page.into()), ..Signing::new("anthropic") });
        let rows = brain_rows(&p, in_);
        assert_eq!(&rows[2..5], &[BrainRow::Account(1), BrainRow::Page, BrainRow::Account(2)]);
        assert_eq!(brain_key("down", 2, &p, in_), (3, Act::Nothing));
        assert_eq!(brain_key("enter", 3, &p, in_), (3, Act::Open(page.into())));
        assert_eq!(brain_hint(&p, 3, in_), "↑↓ pick · ⏎ open it in your browser · ← close · esc back");
        assert_eq!(brain_key("left", 3, &p, in_), (0, Act::Fold(None)));
        assert_eq!(brain_rows(&p, None), [BrainRow::SignIn, BrainRow::Key, BrainRow::Local, BrainRow::Model], "closed: not there");
        // Not its own page: no row (a redirect off its domain, another's, a failed sign-in's).
        for url in ["https://www.google.com/url?q=https://claude.ai/&a=1", "https://evil.com\\.claude.ai/?a=1&b=2", "https://auth.openai.com/oauth/authorize?a=1&b=2"] {
            p.signing = Some(Signing { url: Some(url.into()), ..Signing::new("anthropic") });
            assert!(!brain_rows(&p, in_).contains(&BrainRow::Page), "{url}");
        }
        p.signing = Some(Signing { url: Some(page.into()), failed: Some("x".into()), ..Signing::new("anthropic") });
        assert!(!brain_rows(&p, in_).contains(&BrainRow::Page), "failed: its page is gone");
        // Copilot's: GitHub's device page, where its code is typed (exactly that page).
        let github = "https://github.com/login/device";
        p.signing = Some(Signing { url: Some(github.into()), code: Some("ABCD-1234".into()), ..Signing::new("github-copilot") });
        assert_eq!(&brain_rows(&p, in_)[3..6], &[BrainRow::Account(2), BrainRow::Page, BrainRow::Account(3)]);
        assert_eq!(brain_key("enter", 4, &p, in_), (4, Act::Open(github.into())));
        for url in ["https://github.com/login/device?user_code=ABCD-1234", "https://github.com/login/device/x", "https://www.kimi.com/code/authorize_device?user_code=X"] {
            p.signing = Some(Signing { url: Some(url.into()), ..Signing::new("github-copilot") });
            assert!(!brain_rows(&p, in_).contains(&BrainRow::Page), "{url}");
        }
        p.signing = Some(Signing { url: Some("https://www.kimi.com/code/authorize_device?user_code=X".into()), ..Signing::new("kimi-code") });
        assert!(!brain_rows(&p, in_).contains(&BrainRow::Page), "Kimi's: not one of hers to open");
    }

    /// Her login ran out (`LOGGED OUT`): her account's row signs in again, the same way as a
    /// first sign-in (no terminal); a key turned down takes a new one in her field.
    #[test]
    fn a_login_that_ran_out_signs_in_again() {
        let mut p = prefs();
        p.mind.expired = true;
        let in_ = Some(Fold::SignIn);
        assert_eq!(brain_key("enter", 2, &p, in_).1, Act::SignIn(provider::by_id("anthropic").unwrap()));
        assert_eq!(brain_hint(&p, 2, in_), "↑↓ pick · ⏎ sign in again · ← close · esc back");
        assert_eq!(brain_line(&p.mind), "My login ran out. Sign me in again under SIGN IN.");
        assert_eq!(brain_word(&p.mind), "CLAUDE · SONNET 4.6 · LOGGED OUT");
        // Another account with a login: a switch, as ever.
        p.mind.signed.push("openai-codex");
        assert_eq!(brain_key("enter", 1, &p, in_).1, Act::Pref(Pref::Model("openai-codex/gpt-5.5")));
        let mut k = Prefs { mind: mind("groq/llama-3.3-70b-versatile", Some(true)), ..prefs() };
        k.mind.expired = true;
        assert_eq!(brain_key("enter", 2, &k, Some(Fold::Key)).1, Act::Key(provider::by_id("groq").unwrap()));
        assert_eq!(brain_line(&k.mind), "My key was turned down. Paste a new one under API KEY.");
        // SIGN IN AGAIN ▸ lands on her own row, its list open.
        assert_eq!(brain_landing(&p), (2, in_));
        assert_eq!(brain_landing(&k), (2, Some(Fold::Key)));
        assert_eq!(brain_landing(&prefs()), (0, None), "not ran out: SIGN IN, closed");
        // On another machine: nothing to do here.
        p.mind.here = false;
        assert_eq!(brain_key("enter", 2, &p, in_).1, Act::Nothing);
        assert_eq!(brain_landing(&p), (0, None));
    }

    /// A look that adds or drops a row (Gemini's client id found, a login on file) keeps the row
    /// picked on its account, or its model; one that's gone keeps the place.
    #[test]
    fn a_look_keeps_the_provider_picked() {
        let p = Prefs { mind: mind("claude-sonnet-4-6", Some(false)), ..prefs() };
        let mut g = p.clone();
        g.mind.gemini = true;
        let in_ = Some(Fold::SignIn);
        // Grok picked (row 4 without Gemini); Gemini comes in before it: Grok is row 5 now.
        assert_eq!(brain_reselect(&p, &g, 4, in_), 5);
        assert_eq!(brain_reselect(&g, &p, 5, in_), 4, "and back");
        assert_eq!(brain_reselect(&p, &g, 3, in_), 3, "Copilot, above it: as it was");
        assert_eq!(brain_reselect(&p, &g, 7, in_), 8, "API KEY");
        assert_eq!(brain_reselect(&g, &p, 4, in_), 4, "Gemini itself gone: the same place (Grok)");
        assert_eq!(brain_reselect(&p, &g, 1, None), 1, "closed: nothing moves");
        // MODEL's menu: a login found puts ChatGPT's models before Claude's: Opus stays picked.
        let a = prefs();
        let mut b = a.clone();
        b.mind.signed.push("openai-codex");
        let codex = provider::by_id("openai-codex").unwrap().models.len();
        assert_eq!(brain_reselect(&a, &b, 8, Some(Fold::Model)), 8 + codex);
    }

    /// A daemon through `--socket` (another machine's): no sign-in here reaches it, so the ways
    /// in do nothing and say where; MODEL is hers, still live.
    #[test]
    fn a_remote_daemon_signs_in_on_its_machine() {
        let mut p = Prefs { mind: mind("claude-sonnet-4-6", None), ..prefs() };
        p.mind.here = false;
        let in_ = Some(Fold::SignIn);
        assert_eq!(brain_key("enter", 0, &p, None).1, Act::Fold(in_), "the list opens: it says where");
        assert_eq!(brain_key("enter", 2, &p, in_).1, Act::Nothing);
        assert_eq!(brain_key("enter", 2, &p, Some(Fold::Key)).1, Act::Nothing, "API KEY");
        assert_eq!(brain_key("enter", 2, &p, None).1, Act::Nothing, "RUN LOCAL: its machine's server, not this one's");
        assert_eq!(brain_hint(&p, 2, None), "↑↓ pick · esc back");
        assert_eq!(brain_hint(&p, 2, in_), "↑↓ pick · ← close · esc back");
        // MODEL: its login there can't be told from here: no models until her model answers.
        assert_eq!(brain_key("enter", 3, &p, None).1, Act::Nothing, "no confirmed login: nothing to pick");
        assert_eq!(brain_line(&p.mind), "My brain runs on another machine. Sign in there, then say hi.");
        assert_eq!(brain_word(&p.mind), "CLAUDE · SONNET 4.6");
        // It answered: a brain, there or here; its provider's models.
        p.mind.signed_in = Some(true);
        assert_eq!(brain_line(&p.mind), "That's what I think with. Swap it any time.");
        assert_eq!(brain_key("enter", 3, &p, None).1, Act::Fold(Some(Fold::Model)));
        assert_eq!(brain_key("enter", 8, &p, Some(Fold::Model)).1, Act::Pref(Pref::Model("claude-opus-5-5")));
    }

    /// A local model: the server's look says whether it's there; MODEL holds the server's.
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
        let model = brain_rows(&p, None).len() - 1;
        assert_eq!(brain_key("enter", model, &p, None).1, Act::Nothing, "her one model: nothing to pick");
        p.mind.local = Some(("http://localhost:11434/v1".into(), Local::Up(vec!["qwen3:8b".into(), "gemma3:4b".into()])));
        assert_eq!(brain_key("enter", model, &p, None).1, Act::Fold(Some(Fold::Model)));
        assert_eq!(brain_key("enter", model + 2, &p, Some(Fold::Model)).1, Act::Pref(Pref::Model("local/gemma3:4b")));
        // RUN LOCAL with the server's models: their list in place, not a web page; a pick
        // switches her and closes it on RUN LOCAL.
        let (local, l) = (2, Some(Fold::Local));
        assert_eq!(brain_key("enter", local, &p, None), (local, Act::Fold(l)));
        assert_eq!(brain_hint(&p, local, None), "↑↓ pick · ⏎ open · esc back");
        assert_eq!(&brain_rows(&p, l)[local..], &[BrainRow::Local, BrainRow::LocalPick(0), BrainRow::LocalPick(1), BrainRow::Model]);
        assert_eq!(brain_key("enter", 3, &p, l).1, Act::Nothing, "hers");
        assert_eq!(brain_key("enter", 4, &p, l), (local, Act::Pref(Pref::Model("local/gemma3:4b"))));
        assert_eq!(brain_hint(&p, 4, l), "↑↓ pgup pgdn · ⏎ use it · ← close · esc back");
        assert_eq!(brain_key("left", 4, &p, l), (local, Act::Fold(None)));
        assert_eq!(brain_key("enter", local, &p, l), (local, Act::Fold(None)), "open: ⏎ closes it");
        // Its server down, or her model locked: the steps.
        let mut off = p.clone();
        off.mind.local = Some(("http://localhost:11434/v1".into(), Local::Off));
        assert_eq!(brain_key("enter", local, &off, None).1, Act::Local);
        let mut locked = p.clone();
        locked.mind.lock = Some(Lock::Cli);
        assert_eq!(brain_key("enter", local, &locked, None).1, Act::Local);
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
