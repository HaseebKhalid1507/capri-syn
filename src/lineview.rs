//! The Line, from the avatar's side: the summon key (`talk press|release`), where the Line
//! opens, the thread it shows (the session's transcript), what it asks for, the unread strip
//! a missed reply leaves on her tag, and copy. A child module of `view`.

use super::*;
use crate::beside::{self, Panel, Place};
use crate::line::{Ask, Line, Page, Pref, Prefs, Row, Snap, Who};
use crate::provider::signin::Signing;
use crate::provider::{self, Kind};
use crate::session::Creds;
use crate::settings::{Attention, Settings, WriteQueue};

/// A press held this long is a hold (voice, later); shorter is a tap (type).
pub(super) const TAP_MS: u128 = 250;
/// A press without its release for this long is stale (the release went missing): the next
/// press or tap starts afresh.
const TALK_STALE: Duration = Duration::from_secs(120);
/// A press this young may still open its mic (at `TAP_MS`): a tap leaves it be.
const TAP_GUARD: Duration = Duration::from_secs(1);
const LINE_W: f32 = 780.0;
/// Squeezed beside her, down to this wide (logical px); narrower, it goes above her or over.
const LINE_MIN_W: f32 = 480.0;
/// The least the Line is tall (logical px), and its gap to her.
const LINE_MIN_H: f32 = 60.0;
const LINE_GAP: f32 = 12.0;
/// The Line's first height (logical px), until it measures itself.
const LINE_H0: f32 = 120.0;
/// Rows the Line holds (the transcript's tail).
const ROWS_MAX: usize = 240;

/// settings.toml writes, one at a time (`settings_write`).
pub(super) type PrefsIo = WriteQueue<PrefJob>;

/// A look at her brain is good this long: a Settings page opened again within it doesn't look
/// again, and the Brain screen, open, looks again this often (a sign-in he ran in a terminal).
const LOOK_EVERY: Duration = Duration::from_secs(5);
/// A local model server answers in this or isn't there.
const PROBE_WITHIN: Duration = Duration::from_millis(800);
/// The local server is asked again (each ask a line in Ollama's log) after `LOOK_EVERY`, the wait
/// doubling while it answers the same, to this; a new URL, or a page opened, asks at once.
const PROBE_MAX: Duration = Duration::from_secs(40);

/// The local server's last probe: its URL (as shown: `provider::url_shown`), its answer, when,
/// and the wait before the next.
#[derive(Clone, Debug)]
pub(super) struct Probed {
    url: String,
    local: provider::Local,
    at: Instant,
    wait: Duration,
}

/// Whether a look probes `url` now: never probed, another URL, or the wait is over.
fn probe_due(last: Option<&Probed>, url: &str, now: Instant) -> bool {
    last.is_none_or(|p| p.url != url || now >= p.at + p.wait)
}

/// A probe's answer at `now`. Only a server that answers, with her model if she's on one of its
/// (`want`), is asked less: the same answer as the last (same URL) doubles the wait (each ask a
/// line in its log). One that's off logs nothing, and he may be starting it, or pulling her
/// model ("I'll look again"): every look.
fn probed(last: Option<&Probed>, url: String, local: provider::Local, want: Option<&str>, now: Instant) -> Probed {
    let settled = matches!(local, provider::Local::Up(_)) && want.is_none_or(|m| local.has(m));
    let wait = last.filter(|p| settled && p.url == url && p.local == local).map_or(LOOK_EVERY, |p| (p.wait * 2).clamp(LOOK_EVERY, PROBE_MAX));
    Probed { url, local, at: now, wait }
}

/// What a look at her brain found, off her UI thread, when a Settings page opens: her local
/// model server, how he runs `synaps`, the accounts with a login on this machine.
#[derive(Clone, Debug, Default)]
pub(super) struct BrainLook {
    pub local: Option<(String, provider::Local)>,
    pub synaps: String,
    pub signed: Vec<&'static str>,
    /// Gemini can be signed in to (`provider::GEMINI_CLIENT_ID` set, hers or the daemon's).
    pub gemini: bool,
    /// When it was taken (None: never, or since gone stale), and whether one is under way.
    pub at: Option<Instant>,
    pub busy: bool,
    /// It went stale while one was under way (a login landed): look again once that one is in.
    pub again: bool,
    /// The local server's last probe (`probe_due`), and the config's local URL as the daemon read
    /// it (`creds::url_at_start`): each look's, for the next.
    pub probed: Option<Probed>,
    pub url_read: Option<crate::creds::UrlAtStart>,
}

impl BrainLook {
    /// A look may start now: none under way, and the last one older than `LOOK_EVERY` (or stale).
    fn start(&mut self, now: Instant) -> bool {
        if self.busy || self.at.is_some_and(|t| now < t + LOOK_EVERY) {
            return false;
        }
        self.busy = true;
        true
    }

    /// What it shows is stale (her login changed): the next look goes at once, and one under way
    /// now is followed by another (it may have read the files before the change).
    fn stale(&mut self) {
        self.at = None;
        self.again |= self.busy;
    }

    /// A look is in. true: look again now (it went stale meanwhile: what it found is shown, but
    /// not as fresh, so it can't hold off the next).
    fn done(&mut self, look: BrainLook, now: Instant) -> bool {
        let again = std::mem::take(&mut self.again);
        // A page opened while it was under way: that look didn't probe for it, so it's still owed.
        let owed = self.probed.as_ref().filter(|p| p.wait.is_zero()).map(|p| p.at);
        *self = BrainLook { at: (!again).then_some(now), busy: false, again: false, ..look };
        if let (Some(at), Some(p)) = (owed, &mut self.probed)
            && p.at == at
        {
            p.wait = Duration::ZERO;
        }
        again
    }

    /// A Settings page opened: its look asks the local server whatever the wait (he may have
    /// just started it).
    pub(super) fn opened(&mut self) {
        if let Some(p) = &mut self.probed {
            p.wait = Duration::ZERO;
        }
    }

    /// When the open Brain screen looks again (None: one is under way).
    fn due(&self, now: Instant) -> Option<Instant> {
        (!self.busy).then(|| self.at.map_or(now, |t| t + LOOK_EVERY))
    }
}

/// The look itself (`here`: the daemon runs on this machine; else nothing here is its; `last`:
/// the last look's probe and local URL; `want`: her local model, if she's on one). Synaps' files
/// and the daemon's environment are read once for all of it; the local server is asked only when
/// `probe_due`.
fn look_now(profile: Option<String>, pid: Option<u32>, here: bool, last: (Option<Probed>, Option<crate::creds::UrlAtStart>), want: Option<String>) -> BrainLook {
    let exe = provider::daemon_exe(&synaps_client::discovery::run_dir(), profile.as_deref());
    let synaps = provider::synaps_cmd(exe.as_deref(), std::env::var_os("PATH").as_deref());
    if !here {
        return BrainLook { synaps, ..BrainLook::default() };
    }
    let seen = crate::creds::Seen::read(profile.as_deref(), pid);
    let (url, url_read) = seen.local_url(last.1.as_ref());
    let shown = provider::url_shown(&url);
    let probe = match last.0 {
        Some(p) if !probe_due(Some(&p), &shown, Instant::now()) => p,
        p => {
            let local = provider::probe(&url, PROBE_WITHIN);
            probed(p.as_ref(), shown, local, want.as_deref(), Instant::now())
        }
    };
    let signed = provider::ALL.iter().filter(|p| p.kind != Kind::Local).filter(|p| p.models.first().is_some_and(|m| seen.check(m.0).0 == Creds::Have)).map(|p| p.id).collect();
    let gemini = seen.env_either(provider::GEMINI_CLIENT_ID);
    BrainLook { local: Some((probe.url.clone(), probe.local.clone())), synaps, signed, gemini, probed: Some(probe), url_read, ..BrainLook::default() }
}

/// A MODEL pick her session can't take now: her next start sets it.
const LATER: &str = "Saved for my next start.";

/// Why a MODEL pick isn't in her session now, as she says it; None: it is, or is on its way
/// (`sent`: Sets or the Mirror switch for them went out; `has`: her session has it, or asked).
fn model_held(sent: bool, has: bool, blocked: bool) -> Option<&'static str> {
    match (sent || has, blocked) {
        (true, _) => None,
        (false, true) => Some("Saved for my next start: another client has my session's input."),
        (false, false) => Some(LATER),
    }
}

/// What a settings.toml write applies once it is in: the change, and whether her menu asked
/// (else the Settings page).
#[derive(Clone, Copy, Debug)]
pub(super) struct PrefJob {
    pub pref: Pref,
    pub menu: bool,
    /// Her bubble says it too once it's in (a sign-in's switch: "Opus 5.5 it is").
    pub aloud: bool,
}

pub(super) struct LineWin {
    pub view: Entity<Line>,
    pub handle: AnyWindowHandle,
    pub xid: u32,
    /// Who had the keyboard before.
    pub prev: Option<u32>,
    /// Device px: x, bottom edge, width, height.
    pub at: (i32, i32, i32, i32),
    pub parked: bool,
    /// Opened without the keyboard and never had it (the desktop hands it over only on his
    /// input: `Caps::keyboard_on_input`): parked, and her tag says how to type.
    pub waiting: bool,
    /// Her body box it was placed beside (she moves: it follows), and its side of her.
    pub her: WinRect,
    pub tail: Option<(beside::Side, i32)>,
    /// The page it shows (chat, or her Settings).
    pub page: Page,
}

/// A reply he didn't see before its bubble went: one line on her tag until he acknowledges
/// it (LOCKED #11.3), or a refused message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Unread {
    pub n: u32,
    pub text: String,
    pub refused: bool,
}

/// Replies the strip remembers by turn (older ones still count in its "N NEW").
const MISSED_KEEP: usize = 32;

/// What he hasn't seen: her strip, the last turn he has seen, and the replies behind the strip.
#[derive(Clone, Debug, Default)]
pub(super) struct Unseen {
    pub strip: Option<Unread>,
    seen: u64,
    /// The replies he missed, oldest first: (turn, first line). The strip is their count and
    /// the newest one's line.
    missed: Vec<(u64, String)>,
    /// Missed ones past `MISSED_KEEP`, forgotten but counted.
    older: u32,
}

impl Unseen {
    /// He has seen up to the shown turn (the Line showed it, he clicked the strip, he sent the
    /// next message).
    pub fn ack(&mut self, shown: Option<u64>) {
        self.strip = None;
        self.missed.clear();
        self.older = 0;
        self.seen = self.seen.max(shown.unwrap_or(0));
    }

    /// A reply's bubble went: if he never saw it (and the Line isn't open), it leaves a strip,
    /// counting the replies missed in a row.
    pub fn missed(&mut self, turn: u64, text: &[char], line_open: bool) {
        if line_open || turn <= self.seen {
            return;
        }
        let first: String = text.iter().collect::<String>().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
        if first.is_empty() {
            return;
        }
        // A refused message gives way to the reply (counted from one, as ever).
        if self.strip.as_ref().is_some_and(|u| u.refused) {
            self.strip = None;
            self.missed.clear();
            self.older = 0;
        }
        match self.missed.iter_mut().find(|m| m.0 == turn) {
            Some(m) => m.1 = first,
            None => {
                self.missed.push((turn, first));
                if self.missed.len() > MISSED_KEEP {
                    self.missed.remove(0);
                    self.older += 1;
                }
            }
        }
        self.restrip();
    }

    /// A reply she said aloud and he heard whole (`voice::Said::heard`; cut or said in part, it
    /// never comes here): seen, whenever its bubble goes. If it already left the strip (it went
    /// behind the chip mid-speech), only it goes: the others stay, the newest of them shown.
    pub fn heard(&mut self, turn: u64) {
        self.seen = self.seen.max(turn);
        self.missed.retain(|m| m.0 != turn);
        self.restrip();
    }

    /// The strip from the replies missed (a refused message stays as it is).
    fn restrip(&mut self) {
        if self.strip.as_ref().is_some_and(|u| u.refused) {
            return;
        }
        if self.missed.is_empty() {
            self.older = 0;
        }
        self.strip = self.missed.last().map(|(_, text)| Unread { n: self.older + self.missed.len() as u32, text: text.clone(), refused: false });
    }

    /// The strip on her tag: no "N NEW" while hands-free is on (he hears her replies; one
    /// from before comes back when it ends). A refused message always shows.
    pub fn shown(&self, handsfree: bool) -> Option<&Unread> {
        self.strip.as_ref().filter(|u| u.refused || !handsfree)
    }

    pub fn refused(&mut self, text: String) {
        self.missed.clear();
        self.older = 0;
        self.strip = Some(Unread { n: 1, text, refused: true });
    }
}

impl Avatar {
    /// `talk press`: open the Line (or resume it parked, or close it).
    pub(super) fn talk_press(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        // Held keys repeat: a press while down, or inside the key-up window, is the same press.
        if self.voice_repeat() || self.talk_held() {
            return "ok held".into();
        }
        self.talk_down = Some(now);
        // A toggled mic is open: the talk key is a tap of the toggle (its release is ignored).
        if self.voice.toggle.is_some() {
            return self.voice_toggle_tap_key(cx);
        }
        let hushed = self.voice_press(now, cx);
        self.brain_touch(crate::brain::Touch::Typing);
        if let Some(l) = &self.line {
            // Open and his: a tap closes it, a hold (voice) keeps it: decided at key-up. A
            // tap that hushed her only hushed her.
            if !l.parked {
                self.tap_closes = !hushed;
                return "ok line open".into();
            }
            let (view, handle) = (l.view.clone(), l.handle);
            // Deferred: closing calls back into this view.
            cx.defer(move |cx| {
                handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.summon(window, cx))).ok();
            });
            return "ok line summoned".into();
        }
        self.open_line_summoned(cx)
    }

    /// `talk release`: a tap typed (the Line is open); a hold sends what she heard (voice
    /// on), after the auto-repeat window.
    pub(super) fn talk_release(&mut self, cx: &mut Context<Self>) -> String {
        self.talk_release_as(true, cx)
    }

    /// The talk key is down: a press without its release yet (and not stale).
    fn talk_held(&self) -> bool {
        self.talk_down.is_some_and(|t| t.elapsed() < TALK_STALE)
    }

    /// A `talk tap` waits out a press still waiting for its release only while that press
    /// is a hold worth keeping: its mic is asked for or open (up to `TALK_STALE`, the sidecar's
    /// own hold cap), or it is too young to have opened one (`TAP_GUARD`). Any other
    /// press lost its release (a binding that never sent it): the tap clears it.
    fn tap_waits(&self) -> bool {
        self.talk_down.is_some_and(|t| press_holds(t.elapsed(), self.voice_listening()))
    }

    /// `talk tap`: the press and its release now (a grabbed key reports its press only). The
    /// key is never asked whether it is still down, so this is never a hold. While a hold
    /// listens (`talk press` without its release yet) a tap is ignored: its release would end
    /// that hold early (`tap_waits`).
    pub(super) fn talk_tap(&mut self, cx: &mut Context<Self>) -> String {
        if self.tap_waits() {
            return "ok held (tap ignored)".into();
        }
        if let Some(t) = self.talk_down.take() {
            elog!("synaps-avatar: talk: a press {} s old never had its release: the tap clears it", t.elapsed().as_secs());
        }
        let press = self.talk_press(cx);
        let release = self.talk_release_as(false, cx);
        format!("{press}; {release}")
    }

    /// `physical`: the release came from the key itself, which may still be down (an
    /// auto-repeat pair); a tap's release didn't.
    fn talk_release_as(&mut self, physical: bool, cx: &mut Context<Self>) -> String {
        let Some(t0) = self.talk_down else { return "ok (no press)".into() };
        let ms = t0.elapsed().as_millis();
        if !self.voice_up(physical, cx) {
            return "ok repeat (key still down)".into();
        }
        if std::mem::take(&mut self.tap_closes)
            && ms < TAP_MS
            && let Some(l) = &self.line
        {
            let (view, handle) = (l.view.clone(), l.handle);
            cx.defer(move |cx| {
                handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.summon(window, cx))).ok();
            });
        }
        if ms >= TAP_MS {
            if !self.voice_on() {
                elog!("synaps-avatar: talk held {ms} ms: voice is off ([voice] enabled = false); the Line opened for typing");
            }
            format!("ok hold {ms} ms")
        } else {
            format!("ok tap {ms} ms")
        }
    }

    /// The Line's place (device px), `h` tall: beside her on her monitor (`beside::place`).
    /// Its work area and her body box come along.
    fn line_place(&self, h: i32) -> (Place, WinRect, WinRect) {
        let sf = self.layout.sf;
        let her = self.her_body();
        let area = self.her_area().unwrap_or_else(|| {
            let m = self.os.area_at(her.center().0, her.center().1);
            WinRect::new(m.x, m.y, m.w, m.h)
        });
        let d = |v: f32| (v * sf) as i32;
        // Above her only with room for what it grows to (the thread is a third of the area).
        let grown = h.max(area.h / 3 + d(90.0));
        let p = Panel { w: d(LINE_W), min_w: d(LINE_MIN_W), h, min_h: grown };
        (beside::place(area, her, p, d(LINE_GAP), self.placement == Placement::Ground, None), area, her)
    }

    /// Place the open Line again beside her (she moved, or it measured `h` tall): its window
    /// and its tail follow.
    pub(super) fn line_follow(&mut self, h: Option<i32>, cx: &mut Context<Self>) {
        let Some(l) = &self.line else { return };
        let h = h.unwrap_or(l.at.3);
        let (p, _, her) = self.line_place(h);
        let sf = self.layout.sf;
        let Some(l) = &mut self.line else { return };
        let r = p.rect;
        l.her = her;
        let at = (r.x, r.bottom(), r.w, r.h);
        if at != l.at {
            l.at = at;
            self.os.place(l.xid, r.x, r.y, r.w, r.h).ok();
        }
        let tail = Some((p.side, p.tail));
        if tail != l.tail {
            l.tail = tail;
            let (v, hd) = (l.view.clone(), l.handle);
            let t = Some((p.side, p.tail as f32 / sf));
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.set_tail(t, cx))).ok();
            });
        }
    }

    pub(super) fn open_line(&mut self, cx: &mut Context<Self>) -> String {
        self.open_line_on(Page::Chat, cx)
    }

    /// Her summon's Line (the key, a click on her): on chat, or straight on Settings ▸ Brain
    /// while she has no brain (Esc from there: chat).
    pub(super) fn open_line_summoned(&mut self, cx: &mut Context<Self>) -> String {
        self.open_line_on(if self.no_brain() { Page::Brain } else { Page::Chat }, cx)
    }

    /// No login for her model: her tag reads NO BRAIN YET (a read of the files, or the daemon's
    /// own word).
    pub(super) fn no_brain(&self) -> bool {
        self.session.as_ref().is_some_and(|s| matches!(s.link.creds(), Creds::Missing(_)))
    }

    /// Settings, or the Brain screen while she has no brain (her menu's SET UP MY BRAIN).
    pub(super) fn settings_page(&self) -> Page {
        if self.no_brain() { Page::Brain } else { Page::Settings }
    }

    /// Her Settings: the Line opens on them, or an open one flips to them (the menu's
    /// SETTINGS row, `avatarctl settings open [brain]`); `Page::Chat` flips it back.
    pub(super) fn show_line_page(&mut self, page: Page, cx: &mut Context<Self>) -> String {
        match &self.line {
            Some(l) => {
                let (view, handle) = (l.view.clone(), l.handle);
                cx.defer(move |cx| {
                    handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.show(page, window, cx))).ok();
                });
                format!("ok line on {}", page.key())
            }
            None if page.settings() => self.open_line_on(page, cx),
            None => "ok no line".into(),
        }
    }

    pub(super) fn open_line_on(&mut self, page: Page, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        let sf = self.layout.sf;
        let h0 = (LINE_H0 * sf) as i32;
        let (place, area, her) = self.line_place(h0);
        let (x, w, h) = (place.rect.x, place.rect.w, place.rect.h);
        let bottom = place.rect.bottom();
        let prev = self.os.prev_focus();
        // The bubble's field gives way: one place to type.
        if self.input.as_ref().is_some_and(|i| i.secret.is_none()) {
            self.close_input(false);
        }
        let bounds = Bounds::new(point(px(x as f32 / sf), px((bottom - h) as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf)));
        // The keyboard comes with the Line when this opening is his doing (his key, his click
        // on her: `Os::may_take_keyboard`). Else the Line opens waiting for him, never taking
        // the keyboard from the window he works in.
        let take = self.os.may_take_keyboard();
        let options = self.os.window_options(Role::Line, bounds, take);
        self.prefs_says = None;
        let mut snap = self.line_snap();
        if page.settings() {
            snap.prefs = Some(self.prefs_now());
            self.brain_look.opened();
            self.look_at_brain(cx);
        }
        let me = cx.entity().downgrade();
        let (name, font, pal) = (self.name.to_string(), self.font.clone(), self.reader_pal());
        let draft = std::mem::take(&mut self.draft);
        let sent = self.sent.clone();
        let max_thread = area.h as f32 / sf / 3.0;
        let face = self.stills.as_ref().and_then(|s| s.face.clone());
        let xid = Rc::new(Cell::new(0u32));
        let (xid2, os) = (xid.clone(), self.os.clone());
        let built: Rc<RefCell<Option<Entity<Line>>>> = Rc::default();
        let built2 = built.clone();
        crate::line::kit_init(cx);
        let opened = cx.open_window(options, move |window, cx| {
            xid2.set(os.adopt(Role::Line, window).unwrap_or(0));
            let on_ask: crate::line::OnAsk = Box::new(move |ask, cx: &mut App| me.update(cx, |a, cx| a.line_ask(ask, cx)).unwrap_or_else(|_| Err(crate::line::GONE.into())));
            let view = cx.new(|cx| Line::new(snap, draft, sent, name, font, pal, max_thread, face, page, on_ask, window, cx));
            *built2.borrow_mut() = Some(view.clone());
            cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
        });
        let handle = match opened {
            Ok(h) => h,
            Err(e) => return format!("error: line: {e:#}"),
        };
        let Some(view) = built.borrow_mut().take() else { return "error: line: not built".into() };
        let xid = xid.get();
        if xid != 0 {
            self.set_icon_on(xid);
        }
        let mut waits = !take;
        if xid != 0
            && take
            && std::env::var_os("SA_LINE_NOFOCUS").is_none()
            && let Err(e) = self.os.grab_keyboard(xid)
            && self.os.keyboard_on_input()
        {
            elog!("synaps-avatar: line: the keyboard stayed with his window: {e:#}");
            waits = true;
        }
        self.line = Some(LineWin { view: view.clone(), handle: handle.into(), xid, prev, at: (x, bottom, w, h), parked: waits, waiting: waits, her, tail: Some((place.side, place.tail)), page });
        // Clicks elsewhere park a Line that holds the keyboard (a waiting one holds nothing).
        self.os.arm_clicks(!waits);
        if waits {
            let (v, hd) = (view.clone(), handle);
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.wait(cx))).ok();
            });
        }
        self.line_opened_ms = Some(now.elapsed().as_millis());
        // He is reading the thread now: nothing is unread, and the bubble gives way. Opened
        // with a reply unread (her strip, the summon key): the thread starts at that reply.
        // On Settings: unread stays so until he goes to chat.
        if self.unseen.strip.as_ref().is_some_and(|u| !u.refused) {
            let (v, hd) = (view, handle);
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.show_last_reply(cx))).ok();
            });
        }
        if page == Page::Chat {
            self.ack_unread();
        }
        if let Some(b) = &mut self.bubble {
            b.collapsed = true;
            b.hide_at = None;
        }
        // She stays where she is (asleep too): the Line is beside her, its tail at her.
        if let Some(l) = &self.line {
            let (v, hd) = (l.view.clone(), l.handle);
            let t = Some((place.side, place.tail as f32 / sf));
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.set_tail(t, cx))).ok();
            });
        }
        elog!(
            "synaps-avatar: line open at {x},{} {w}x{h} (0x{xid:x}) in {} ms, {} of her{}",
            bottom - h,
            now.elapsed().as_millis(),
            crate::line::side_key(place.side),
            if waits { ", waiting for his key or a click" } else { "" }
        );
        cx.notify();
        self.schedule(cx);
        if waits { format!("ok line {x},{} {w}x{h} waiting", bottom - h) } else { format!("ok line {x},{} {w}x{h}", bottom - h) }
    }

    /// How he presses her summon key here (`$MOD+C`, `WIN+ALT+C`), for the hints.
    pub(super) fn summon_label(&self) -> String {
        self.os.key_label(crate::platform::KeyEv::Summon, self.voice.cfg.key)
    }

    /// The thread, from the session's transcript (thinking left out).
    pub(super) fn line_snap(&self) -> Snap {
        let st = self.status();
        let (flag, alarm) = (st.invert && st.plate == crate::theme::Plate::Flag, st.key == "alert");
        let (status, glyph) = (st.label, st.glyph);
        let face = self.stills.as_ref().and_then(|s| s.moods.get(self.mood.key()).cloned()).map(crate::line::Face);
        let key = self.summon_label();
        let prefs = self.line.as_ref().filter(|l| l.page.settings()).map(|_| self.prefs_now());
        let (glitch, motion) = (self.hud_cfg.glitch, self.hud_cfg.motion);
        let Some(s) = &self.session else { return Snap { status, glyph, flag, alarm, live: false, face, key, prefs, glitch, motion, ..Default::default() } };
        let rows = thread_rows(s.link.transcript().items(), s.link.model(), |id| s.link.unpicked_error(id));
        let b = s.shown.as_ref();
        Snap {
            rows,
            status,
            glyph,
            flag,
            alarm,
            busy: s.shown.as_ref().is_some_and(|b| b.busy),
            live: b.is_some_and(|b| b.live),
            prompts: b.map(|b| b.prompts.clone()).unwrap_or_default(),
            speaking: self.voice_speaking(),
            listening: self.voice_listening(),
            key,
            mic: self.voice_mic_row(),
            caption: self.voice_caption_now(),
            face,
            trouble: b.and_then(|b| b.trouble.as_ref()).map(|t| t.tip().to_string()),
            prefs,
            glitch,
            motion,
        }
    }

    /// Her settings as asked (a change on its way to settings.toml), else as she runs them:
    /// for the Settings page, which steps on from them as her menu does.
    pub(super) fn prefs_now(&self) -> Prefs {
        // ABOUT: the versions, and her model as the page says it (`SONNET 4.6`; her menu's too).
        let mark = self.session.as_ref().and_then(|s| s.link.mark()).unwrap_or_else(|| "SYNAPS · OFFLINE".into());
        let running = self.brain.as_ref().map_or(Attention::Off, |h| h.settings.brain.attention);
        let (attention, wake) = crate::line::asked(self.prefs_io.pending().map(|j| &j.pref), running, self.voice.cfg.wake_word);
        Prefs {
            attention,
            brain: self.attention_live(),
            wake,
            voice: self.voice.cfg.enabled,
            about: format!("CAPRI {} · {mark}", env!("CARGO_PKG_VERSION")),
            mind: self.mind_now(),
            says: self.prefs_says.clone(),
            new: self.unseen.strip.as_ref().filter(|u| !u.refused).map_or(0, |u| u.n),
            signing: self.signing.clone(),
        }
    }

    /// A sign-in from her Brain screen (`signin`): Synaps' `login` with no window, his browser
    /// opened by Synaps; `key`: an API key, to it on its stdin. One at a time: another asked
    /// stops the one running. Not for a daemon on another machine (a login here isn't its).
    pub(super) fn sign_in(&mut self, p: &'static provider::Provider, key: Option<crate::session::Secret>, cx: &mut Context<Self>) {
        let ((profile, _), here) = self.session.as_ref().map_or(((None, None), false), |s| (s.link.daemon_at(), s.link.local()));
        if let Some(mut r) = self.signin.take() {
            r.kill();
        }
        self.signing = None;
        if here {
            let exe = provider::daemon_exe(&synaps_client::discovery::run_dir(), profile.as_deref()).unwrap_or_else(|| std::path::PathBuf::from("synaps"));
            match provider::signin::Run::start(&exe, profile.as_deref(), p, key.as_ref().map(|k| k.0.as_str())) {
                Ok(r) => {
                    elog!("synaps-avatar: sign-in: {} started{}", p.id, if key.is_some() { " (a key)" } else { "" });
                    self.signin = Some(r);
                    self.signing = Some(Signing::new(p.id));
                    let said = if key.is_some() { format!("Saving your {} key.", p.name) } else { format!("Your browser's open: sign in to {} there.", p.name) };
                    if key.is_none() {
                        self.say(said.clone(), Instant::now());
                    }
                    self.prefs_says = Some(said);
                }
                Err(e) => {
                    elog!("synaps-avatar: sign-in: {} didn't start: {e}", p.id);
                    let why = provider::signin::why(&e.to_string());
                    self.prefs_says = Some(format!("Couldn't run synaps login: {why}."));
                    self.signing = Some(Signing { failed: Some(why), ..Signing::new(p.id) });
                }
            }
        }
        self.line_push(cx);
        self.schedule(cx);
    }

    /// The sign-in running stops: Synaps killed.
    pub(super) fn sign_in_stop(&mut self, cx: &mut Context<Self>) {
        if let Some(mut r) = self.signin.take() {
            r.kill();
            elog!("synaps-avatar: sign-in: {} stopped", r.provider.id);
            self.prefs_says = Some("Stopped. Nothing's signed in.".into());
        }
        self.signing = None;
        self.line_push(cx);
    }

    /// The tick's part: what the sign-in running said (its code, its page, its end). Never its
    /// code or URL in her log.
    pub(super) fn sign_in_tick(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(r) = &mut self.signin else { return };
        let p = r.provider;
        let evs = r.poll(now);
        if evs.is_empty() {
            return;
        }
        for ev in evs {
            match ev {
                provider::signin::Ev::Code(code) => {
                    elog!("synaps-avatar: sign-in: {} has a code to type", p.id);
                    // Copilot's page doesn't carry it: he types it there, so it's copied. Kimi's and
                    // Muse's carry theirs: shown to check against.
                    self.prefs_says = Some(if p.id == "github-copilot" {
                        copy_text(code.clone(), cx);
                        format!("GitHub's page is open: type {code} there. It's on your clipboard.")
                    } else {
                        format!("{}'s page is open: check it shows {code}.", p.name)
                    });
                    if let Some(s) = &mut self.signing {
                        s.code = Some(code);
                    }
                }
                provider::signin::Ev::Url(u) => {
                    // Its own sign-in page only (`allowed`, as parsed): opened whole where
                    // Synaps' was cut (Windows), and kept for the Brain screen's row that opens it
                    // again.
                    if let Some(page) = provider::signin::reopen(cfg!(windows), p, &u) {
                        elog!("synaps-avatar: sign-in: {} page opened whole (Synaps' was cut at its &)", p.id);
                        self.line_ask(Ask::OpenUrl(page), cx).ok();
                    }
                    if let Some(page) = provider::signin::allowed(p, &u)
                        && let Some(s) = &mut self.signing
                    {
                        s.url = Some(page);
                    }
                }
                provider::signin::Ev::Done(Ok(())) => {
                    elog!("synaps-avatar: sign-in: {} signed in", p.id);
                    self.signin = None;
                    self.signing = None;
                    self.signed_in(p, now, cx);
                }
                provider::signin::Ev::Done(Err(why)) => {
                    elog!("synaps-avatar: sign-in: {} didn't work: {why}", p.id);
                    self.signin = None;
                    let said = format!("Couldn't sign in to {}: {why}.", p.name);
                    // Her bubble: why, and the line that does it in a terminal (a chip, copied
                    // on a click). The page's ⏎ AGAIN is the other way.
                    self.say(format!("{said} Again from Settings, or in a terminal:\n`{}`", self.mind_now().login_cmd(Some(p))), now);
                    self.prefs_says = Some(said);
                    self.signing = Some(Signing { failed: Some(why), ..Signing::new(p.id) });
                }
            }
        }
        self.line_push(cx);
    }

    /// Signed in from her: the logins here and her own read again now, and her model on the
    /// account (its default), unless she's on it already or her model is locked.
    fn signed_in(&mut self, p: &'static provider::Provider, now: Instant, cx: &mut Context<Self>) {
        self.brain_look.stale();
        self.look_at_brain(cx);
        if let Some(s) = &mut self.session {
            s.link.creds_now(now);
        }
        let m = self.mind_now();
        // Its default model, once its write is in: "X it is" said then, not before.
        let said = format!("Signed in to {}.", p.name);
        self.say(said.clone(), now);
        self.prefs_says = Some(said);
        if let Some(d) = p.models.first()
            && m.provider() != Some(p)
            && m.lock.is_none()
        {
            self.settings_write(PrefJob { pref: Pref::Model(d.0), menu: false, aloud: true }, cx);
        }
    }

    /// Her brain as the Settings page shows it: her model (as asked: a MODEL write on its way),
    /// its login, what locks it, and the last look (`look_at_brain`).
    pub(super) fn mind_now(&self) -> provider::Status {
        let look = &self.brain_look;
        let mut m = provider::Status {
            signed: look.signed.clone(),
            synaps: look.synaps.clone(),
            local: look.local.clone(),
            gemini: look.gemini,
            windows: cfg!(windows),
            ..provider::Status::default()
        };
        let Some(s) = &self.session else { return m };
        m.model = crate::line::asked_model(self.prefs_io.pending().map(|j| &j.pref), s.link.model_on_way().or(s.link.model()));
        m.signed_in = match s.link.creds() {
            Creds::Have => Some(true),
            Creds::Missing(_) => Some(false),
            Creds::Unknown => None,
        };
        m.lock = if s.link.rides_other() {
            Some(provider::Lock::Theirs)
        } else if s.link.cli_model().is_some() {
            Some(provider::Lock::Cli)
        } else {
            None
        };
        m.here = s.link.local();
        m.tried = s.link.local_tried().map(str::to_string);
        m.expired = s.link.trouble() == Some(&crate::session::Trouble::LoggedOut);
        m
    }

    /// Look at her brain (off her UI thread): her local server, how he runs `synaps`, the
    /// accounts with a login here. At most every `LOOK_EVERY`; the page gets it when it's in.
    pub(super) fn look_at_brain(&mut self, cx: &mut Context<Self>) {
        if !self.brain_look.start(Instant::now()) {
            return;
        }
        let ((profile, pid), here) = self.session.as_ref().map_or(((None, None), false), |s| (s.link.daemon_at(), s.link.local()));
        let last = (self.brain_look.probed.clone(), self.brain_look.url_read.clone());
        let want = self.mind_now().model.filter(|m| provider::of(m).is_some_and(|p| p.kind == Kind::Local));
        cx.spawn(async move |this, cx| {
            let look = crate::off_thread(move || Ok(look_now(profile, pid, here, last, want))).await.unwrap_or_default();
            this.update(cx, |a, cx| {
                if look.signed != a.brain_look.signed || look.local != a.brain_look.local {
                    // The URL as shown (never a `user:password@`; it is already, and once more here).
                    let local = look.local.as_ref().map(|(u, l)| (provider::url_shown(u), l));
                    elog!("synaps-avatar: brain: local {local:?}, signed in: {:?}", look.signed);
                }
                if a.brain_look.done(look, Instant::now()) {
                    a.look_at_brain(cx);
                }
                a.line_push(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Her login changed (a read found one, or lost it): the open Settings page shows it, and
    /// looks again at once (the accounts signed in here), after the look under way if one is.
    pub(super) fn brain_news(&mut self, cx: &mut Context<Self>) {
        self.brain_look.stale();
        if self.line.as_ref().is_some_and(|l| l.page.settings()) {
            self.look_at_brain(cx);
            self.line_push(cx);
        }
    }

    /// The Brain screen, open, looks again every `LOOK_EVERY` (a sign-in to another account he
    /// ran in a terminal: her own login's read doesn't see that one). When it next does.
    pub(super) fn brain_look_due(&self, now: Instant) -> Option<Instant> {
        self.line.as_ref().filter(|l| l.page == Page::Brain).and_then(|_| self.brain_look.due(now))
    }

    /// The tick's part: the Brain screen's look, when due.
    pub(super) fn brain_look_tick(&mut self, now: Instant, cx: &mut Context<Self>) {
        if self.brain_look_due(now).is_some_and(|t| t <= now) {
            self.look_at_brain(cx);
        }
    }

    /// A change on the Settings page: written to settings.toml (only that key; a file she
    /// can't read or parse is left alone), off her UI thread; once it is in, that key alone is
    /// applied (his other hand edits wait for `settings reload`). Her line says which.
    pub(super) fn set_pref(&mut self, p: Pref, cx: &mut Context<Self>) {
        self.settings_write(PrefJob { pref: p, menu: false, aloud: false }, cx);
    }

    /// settings.toml written in the order asked, one write at a time, each off the UI thread
    /// (its fsync, Windows' retries). Its change is applied when it is in, menu or page alike.
    pub(super) fn settings_write(&mut self, job: PrefJob, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.prefs_io.push(vec![job.pref.edit()], job);
        if self.prefs_io.stuck_now(now) {
            self.settings_stuck(cx);
        }
        self.settings_write_next(cx);
        // The page shows it as asked at once (it steps on from it, as the menu does).
        self.line_push(cx);
    }

    fn settings_write_next(&mut self, cx: &mut Context<Self>) {
        let Some((seq, edits)) = self.prefs_io.start(Instant::now()) else { return };
        // A MODEL write with her brain off ([brain] enabled = false: no settings held): the rest
        // of her `[session]` is read with it, off her UI thread too.
        let session_too = self.brain.is_none() && matches!(self.prefs_io.pending().next().map(|j| j.pref), Some(Pref::Model(_)));
        cx.spawn(async move |this, cx| {
            let r = crate::off_thread(move || {
                let w = Settings::write(&edits);
                let session = if session_too && w.is_ok() { Settings::load().map(|s| s.session).ok() } else { None };
                Ok((w, session))
            })
            .await;
            let (r, session) = r.unwrap_or_else(|e| (Err(crate::settings::WriteError::Failed(format!("{e:#}"))), None));
            this.update(cx, |a, cx| {
                if let Some((job, reload)) = a.prefs_io.finish(seq) {
                    a.settings_written(job, r, session, reload, cx);
                }
                a.settings_write_next(cx);
            })
            .ok();
        })
        .detach();
        // A write that hangs (a dead network home): she says so, rather than queue in silence.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(crate::settings::WRITE_STUCK).await;
            this.update(cx, |a, cx| {
                if a.prefs_io.stuck(seq, Instant::now()) {
                    a.settings_stuck(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn settings_stuck(&mut self, cx: &mut Context<Self>) {
        let n = self.prefs_io.pending().count();
        self.menu_save = self.menu_save.stuck(self.prefs_io.pending().any(|j| j.menu));
        elog!("synaps-avatar: settings.toml: a write is stuck ({n} change(s) waiting)");
        self.prefs_says = Some(format!("Your settings file isn't answering. {n} change{} waiting.", if n == 1 { "" } else { "s" }));
        self.line_push(cx);
        cx.notify();
    }

    /// A write is done: its change applied if it went in (nothing if refused: she says why);
    /// then the reload asked for while it was under way, once the last is in.
    fn settings_written(&mut self, job: PrefJob, r: Result<bool, crate::settings::WriteError>, session: Option<crate::settings::SessionSettings>, reload: bool, cx: &mut Context<Self>) {
        let p = job.pref;
        self.menu_save = self.menu_save.written(p, job.menu, r.is_ok());
        self.prefs_says = Some(match r {
            Ok(_) => {
                let held = self.pref_apply(p, session);
                elog!("synaps-avatar: settings: {p:?}: ok (that key only{})", if job.menu { ", her menu" } else { "" });
                match p {
                    Pref::Attention(a) => {
                        // The menu's click was already news to her brain.
                        if !job.menu {
                            self.brain_touch(crate::brain::Touch::Menu);
                        }
                        match a {
                            Attention::Mix => "Mixing it up. Hang, cling, sit, peek.",
                            Attention::HangOnly => "Hanging only. I like it up there.",
                            Attention::Off => "Fine. I'll stay put.",
                        }
                        .to_string()
                    }
                    Pref::Wake(true) => "Say my name, I'll hear it.".into(),
                    Pref::Wake(false) => "I'll stop listening for my name.".into(),
                    Pref::Model(m) => match held {
                        None => {
                            let said = format!("{} it is. Your next message runs on it.", provider::model_spoken(m));
                            // A sign-in's switch: in, so said where he is.
                            if job.aloud {
                                self.say(format!("{} it is.", provider::model_spoken(m)), Instant::now());
                            }
                            said
                        }
                        Some(why) => why.into(),
                    },
                }
            }
            Err(e) => {
                elog!("synaps-avatar: settings: {p:?} not saved: {e}");
                e.says()
            }
        });
        if reload {
            let r = self.settings_reload(Instant::now());
            elog!("synaps-avatar: settings reload, again now the writes are in: {r}");
        }
        self.line_push(cx);
        cx.notify();
    }

    /// A change written: its key in what she runs with, nothing else. Some: why her session
    /// didn't take a model now (her next start sets it), as she says it. `session`: her
    /// `[session]` as read with the write (her brain off: none held here).
    fn pref_apply(&mut self, p: Pref, session: Option<crate::settings::SessionSettings>) -> Option<&'static str> {
        match p {
            Pref::Attention(a) => {
                if let Some(h) = &mut self.brain {
                    p.apply(&mut h.settings);
                    h.brain.settings.attention = a;
                }
            }
            Pref::Wake(on) => {
                if self.voice.cfg.wake_word != on {
                    self.voice.cfg.wake_word = on;
                    self.voice_config();
                }
                if let Some(h) = &mut self.brain {
                    p.apply(&mut h.settings);
                }
            }
            // Her own session's model, live (`Setting::Model`, as a reload sends it): the rest of
            // `[session]` as she runs it. Never on a session she rides (`--session`), and `--model`
            // wins (the page locks both).
            Pref::Model(m) => {
                let mut session = match (&self.brain, session) {
                    (Some(h), _) => h.settings.session.clone(),
                    (None, Some(s)) => s,
                    // Her brain went off while it was written: the model alone.
                    (None, None) => Default::default(),
                };
                session.model = Some(m.to_string());
                if let Some(h) = &mut self.brain {
                    p.apply(&mut h.settings);
                }
                let (acts, held) = match &mut self.session {
                    Some(sl) if !sl.link.rides_other() => {
                        let persona = crate::session_persona(&session, &self.voice.cfg, sl.link.cli_model(), &self.pack.dir);
                        let acts = sl.link.pick_persona(persona, Instant::now());
                        let has = sl.link.model() == Some(m) || sl.link.model_on_way() == Some(m);
                        let held = model_held(!acts.is_empty(), has, sl.link.persona_blocked());
                        (acts, held)
                    }
                    _ => (Vec::new(), Some(LATER)),
                };
                self.run_acts(acts);
                return held;
            }
        }
        None
    }

    /// Bring an open Line up to date (deferred: the Line may call back).
    pub(super) fn line_push(&mut self, cx: &mut Context<Self>) {
        let Some(l) = &self.line else { return };
        let snap = self.line_snap();
        let (view, handle) = (l.view.clone(), l.handle);
        if self.bubble.as_ref().is_some_and(|b| b.stream) && l.page == Page::Chat {
            self.ack_unread();
        }
        cx.defer(move |cx| {
            handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.set_snap(snap, window, cx))).ok();
        });
    }

    /// What the Line asks for.
    pub(super) fn line_ask(&mut self, ask: Ask, cx: &mut Context<Self>) -> Result<(), String> {
        let now = Instant::now();
        let r = match ask {
            Ask::Send(text) => {
                self.ack_unread();
                let r = self.send(text);
                if r.is_ok()
                    && let Some(s) = &mut self.session
                {
                    // Mirror lingers 3 s after the send, then Observe (keystrokes, not the window).
                    let acts = s.link.set_typing(false, now);
                    self.run_acts(acts);
                }
                r
            }
            Ask::Abort => {
                self.abort();
                Ok(())
            }
            Ask::Hush => {
                if self.voice_speaking() {
                    self.voice_hush("esc");
                }
                Ok(())
            }
            Ask::CancelListen => {
                elog!("synaps-avatar: voice: esc in the Line while listening");
                self.voice_esc(cx);
                Ok(())
            }
            Ask::Answer(id, value) => {
                self.brain_touch(crate::brain::Touch::PromptAnswer);
                if self.prompt_ui.as_ref().is_some_and(|u| u.view.id == id) {
                    self.prompt_ui = None;
                }
                match &mut self.session {
                    Some(s) if s.link.live() => {
                        let acts = s.link.answer(id, value, now);
                        self.run_acts(acts);
                        Ok(())
                    }
                    _ => Err("not attached to a session".into()),
                }
            }
            Ask::Typing => {
                if let Some(s) = &mut self.session {
                    let acts = s.link.set_typing(true, now);
                    self.run_acts(acts);
                }
                Ok(())
            }
            Ask::Closed { restore, draft, sent } => {
                self.os.arm_clicks(false);
                if let Some(l) = self.line.take() {
                    self.draft = draft;
                    self.sent = sent;
                    if let Some(s) = &mut self.session {
                        let acts = s.link.set_typing(false, now);
                        self.run_acts(acts);
                    }
                    if restore
                        && std::env::var_os("SA_READER_NO_RESTORE").is_none()
                        && let Some(w) = l.prev
                    {
                        self.os.release_keyboard(l.xid, Some(w)).ok();
                    }
                    // A reply still coming finishes in her bubble.
                    if let Some(b) = &mut self.bubble
                        && b.stream
                        && b.more
                    {
                        b.collapsed = false;
                    }
                }
                Ok(())
            }
            Ask::Parked(p) => {
                // Back to the Line: it takes the keyboard. Where the desktop hands it over only
                // on his input, without it (a summon from elsewhere) the Line stays parked.
                let xid = self.line.as_ref().map_or(0, |l| l.xid);
                let refused = if !p && xid != 0 && std::env::var_os("SA_LINE_NOFOCUS").is_none() {
                    match self.os.grab_keyboard(xid) {
                        Err(e) if self.os.keyboard_on_input() => Some(format!("{e:#}")),
                        _ => None,
                    }
                } else {
                    None
                };
                match refused {
                    Some(e) => {
                        elog!("synaps-avatar: line: stays parked, the keyboard stayed with his window: {e}");
                        Err(format!("{}: {e}", crate::line::NO_KEYBOARD))
                    }
                    None => {
                        self.os.arm_clicks(!p && self.line.is_some());
                        if let Some(l) = &mut self.line {
                            l.parked = p;
                            l.waiting &= p;
                        }
                        Ok(())
                    }
                }
            }
            Ask::Height(h) => {
                let sf = self.layout.sf;
                if let Some(l) = &self.line {
                    // No taller than her work area allows (it scrolls inside).
                    let area_h = self.her_area().map_or(i32::MAX / 2, |a| a.h);
                    let hd = ((h * sf) as i32).min(area_h - (36.0 * sf) as i32).max((LINE_MIN_H * sf) as i32);
                    if hd != l.at.3 {
                        self.line_follow(Some(hd), cx);
                    }
                }
                Ok(())
            }
            Ask::Copy(t) => {
                copy_text(t, cx);
                Ok(())
            }
            Ask::Page(page) => {
                if let Some(l) = &mut self.line {
                    l.page = page;
                }
                if page.settings() {
                    self.brain_look.opened();
                    self.look_at_brain(cx);
                }
                // To chat: he sees the thread (what came meanwhile is read); her line goes.
                if page == Page::Chat {
                    self.ack_unread();
                    self.prefs_says = None;
                }
                self.line_push(cx);
                Ok(())
            }
            // MODEL is locked under `--model` and `--session` (the page says so and asks nothing;
            // this holds for anything else that asks).
            Ask::Pref(Pref::Model(m)) if self.mind_now().lock.is_some() => {
                elog!("synaps-avatar: settings: model {m} not written: locked ({:?})", self.mind_now().lock);
                // The page shows the model in force, not one it may have shown as asked.
                self.line_push(cx);
                Ok(())
            }
            Ask::Pref(p) => {
                self.set_pref(p, cx);
                Ok(())
            }
            Ask::SignIn(p) => {
                self.sign_in(p, None, cx);
                Ok(())
            }
            Ask::Key(p, key) => {
                self.sign_in(p, Some(key), cx);
                Ok(())
            }
            Ask::SignInCancel => {
                self.sign_in_stop(cx);
                Ok(())
            }
            Ask::OpenUrl(u) => {
                match std::env::var_os("SA_URL_FILE") {
                    Some(p) => {
                        std::fs::write(p, &u).ok();
                    }
                    None => cx.open_url(&u),
                }
                Ok(())
            }
        };
        cx.notify();
        self.schedule(cx);
        r
    }

    /// A click while the Line holds the keyboard: outside it, the Line parks and the keyboard
    /// goes back to the window he clicked if that is the one he came from (i3 thinks it is
    /// focused already and wouldn't move the keyboard); any other window i3 focuses itself.
    pub(super) fn line_click_elsewhere(&mut self, x: i32, y: i32, cx: &mut Context<Self>) {
        let Some(l) = &mut self.line else { return };
        let (lx, bottom, w, h) = l.at;
        if l.parked || (x >= lx && x < lx + w && y >= bottom - h && y < bottom) {
            return;
        }
        l.parked = true;
        self.os.arm_clicks(false);
        let back = l.prev.filter(|&p| self.os.toplevel_rect(p).is_some_and(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom()));
        if let Some(p) = back {
            self.os.release_keyboard(l.xid, Some(p)).ok();
        }
        let (view, handle) = (l.view.clone(), l.handle);
        cx.defer(move |cx| {
            handle.update(cx, |_, _, cx| view.update(cx, |l, cx| l.park(cx))).ok();
        });
        elog!("synaps-avatar: click at {x},{y} outside the Line: parked{}", if back.is_some() { ", keyboard back" } else { "" });
    }

    /// Close the Line from here (deferred: it calls back).
    pub(super) fn close_line(&mut self, restore: bool, cx: &mut Context<Self>) {
        if let Some(l) = &self.line {
            let (view, handle) = (l.view.clone(), l.handle);
            cx.defer(move |cx| {
                handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.close(restore, window, cx))).ok();
            });
        }
    }

    /// A refused message, back to him where he can see it.
    pub(super) fn line_restore(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        let Some(l) = &self.line else { return false };
        let (view, handle) = (l.view.clone(), l.handle);
        cx.defer(move |cx| {
            handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.restore(text, "the session refused it", window, cx))).ok();
        });
        true
    }

    pub(super) fn line_json(&self, cx: &App) -> serde_json::Value {
        match &self.line {
            Some(l) => {
                let mut v = l.view.read(cx).state_json(cx);
                v["xid"] = serde_json::json!(l.xid);
                v["at"] = serde_json::json!([l.at.0, l.at.1 - l.at.3, l.at.2, l.at.3]);
                v["focused"] = serde_json::json!(self.os.focused_window() == Some(l.xid));
                v["open_ms"] = serde_json::json!(self.line_opened_ms);
                v
            }
            None => serde_json::Value::Null,
        }
    }

    /// The turn whose reply he has seen (the Line showed it, he hovered or clicked the strip,
    /// he sent the next message).
    pub(super) fn ack_unread(&mut self) {
        self.unseen.ack(self.session.as_ref().and_then(|s| s.shown.as_ref()).map(|b| b.turn));
    }

    /// The bubble goes behind the chip before its hold ran out (she teleports, hides): a
    /// reply he hasn't seen leaves the strip as if it had.
    pub(super) fn fold_reply(&mut self) {
        let Some(b) = &mut self.bubble else { return };
        let was_open = !b.collapsed;
        b.collapsed = true;
        if was_open && b.stream && !b.text.is_empty() {
            b.hide_at = None;
            let (turn, text) = (b.turn, b.text.clone());
            self.reply_missed(turn, &text);
        }
    }

    /// The bubble is replaced or dropped (a new turn, her own words): a reply still open in it
    /// (cut off, its hold not out yet; or down only while she is carried) leaves a strip as its
    /// hold running out would have (#68). Already behind the chip, it had its chance.
    pub(super) fn bubble_gone(&mut self) {
        let carried_open = self.rag_hides_bubble();
        if let Some((turn, text)) = self.bubble.as_ref().and_then(|b| b.open_reply(carried_open)) {
            self.reply_missed(turn, &text);
        }
    }

    /// A reply's bubble went: if he never saw it (or heard it whole), it leaves a strip.
    pub(super) fn reply_missed(&mut self, turn: u64, text: &[char]) {
        // The Line on Settings: he isn't reading the thread.
        self.unseen.missed(turn, text, self.line_on_chat());
    }

    /// The Line open on its thread.
    pub(super) fn line_on_chat(&self) -> bool {
        self.line.as_ref().is_some_and(|l| l.page == Page::Chat)
    }

    /// Her strip as drawn on her tag (none of "N NEW" while hands-free is on).
    pub(super) fn unread_shown(&self) -> Option<&Unread> {
        self.unseen.shown(self.voice.hf.is_some())
    }

    /// Copy the last reply as written (the reader's or not): `avatarctl copy`, the menu.
    pub(super) fn copy_last_reply(&mut self, cx: &mut Context<Self>) -> String {
        let raw = self.session.as_ref().and_then(|s| s.shown.as_ref()).map(|b| b.raw.clone()).filter(|r| !r.trim().is_empty());
        let raw = raw.or_else(|| self.bubble.as_ref().map(|b| if b.raw.trim().is_empty() { b.text.iter().collect() } else { b.raw.clone() }));
        match raw.filter(|r| !r.trim().is_empty()) {
            Some(t) => {
                let n = t.len();
                copy_text(t, cx);
                format!("ok copied {n} bytes")
            }
            None => "error: no reply".into(),
        }
    }
}

/// To the clipboard, or to `$SA_CLIP_FILE` under test (never the desktop's clipboard).
pub(super) fn copy_text(t: String, cx: &mut App) {
    match std::env::var_os("SA_CLIP_FILE") {
        Some(p) => {
            std::fs::write(p, &t).ok();
        }
        None => cx.write_to_clipboard(ClipboardItem::new_string(t)),
    }
}

#[cfg(test)]
mod unseen_tests {
    use super::{Bubble, Unread, Unseen};

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    /// A session reply's bubble, open and typed out.
    fn reply(turn: u64, text: &str) -> Bubble {
        let text = chars(text);
        Bubble {
            shown: text.len(),
            text,
            per_tick: 2,
            next_char: None,
            hide_at: None,
            wrap: None,
            page: None,
            raw: String::new(),
            collapsed: false,
            stream: true,
            more: false,
            width: 0.0,
            serial: turn,
            turn,
            cap: super::LINES_PER_PAGE,
        }
    }

    /// As `bubble_gone` does when the next reply's bubble replaces `old`.
    fn replace(u: &mut Unseen, old: &Bubble, carried: bool, line_open: bool) {
        if let Some((turn, text)) = old.open_reply(carried) {
            u.missed(turn, &text, line_open);
        }
    }

    #[test]
    fn a_cut_reply_still_up_when_the_next_opens_leaves_its_strip() {
        // #68: turn 2 was cut off (a tap, music, a restart: never heard whole); its bubble is
        // still up when turn 3's opens. Turn 2 is on the strip, in its own words.
        let mut u = Unseen::default();
        let two = reply(2, "Turn two: the build is green.\nMore after.");
        replace(&mut u, &two, false, false);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "Turn two: the build is green.".into(), refused: false }));
        // Turn 3 heard whole, its bubble out in turn: turn 2 stays, alone.
        u.heard(3);
        let three = reply(3, "Turn three.");
        replace(&mut u, &three, false, false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((1, "Turn two: the build is green.")));
    }

    #[test]
    fn a_reply_heard_seen_or_already_behind_the_chip_leaves_nothing_more() {
        // Heard whole before the next came: read.
        let mut u = Unseen::default();
        u.heard(2);
        replace(&mut u, &reply(2, "Heard."), false, false);
        assert_eq!(u.strip, None);
        // Seen (he sent the next message, the Line showed it).
        let mut u = Unseen::default();
        u.ack(Some(4));
        replace(&mut u, &reply(4, "Seen."), false, false);
        assert_eq!(u.strip, None);
        // The Line open: he is reading it there.
        let mut u = Unseen::default();
        replace(&mut u, &reply(5, "In the Line."), false, true);
        assert_eq!(u.strip, None);
        // Behind the chip already (its hold ran out: counted then): not twice.
        let mut u = Unseen::default();
        let mut six = reply(6, "Six.");
        u.missed(6, &six.text, false);
        six.collapsed = true;
        replace(&mut u, &six, false, false);
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
        // Her own words (`say`): never a strip.
        let mut own = reply(0, "Hey. I'm Capri.");
        own.stream = false;
        assert_eq!(own.open_reply(false), None);
    }

    #[test]
    fn a_reply_put_down_only_for_a_carry_still_counts() {
        // Carried: the bubble is down for the ride, back at the landing. A new turn then is
        // the same as one replacing it open.
        let mut u = Unseen::default();
        let mut two = reply(2, "Mid-air.");
        two.collapsed = true;
        replace(&mut u, &two, true, false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((1, "Mid-air.")));
    }

    #[test]
    fn one_still_speaking_when_replaced_goes_once_it_is_heard_whole() {
        // An event reply waits behind the one she is saying: turn 2's bubble gives way to
        // turn 3's while turn 2 still plays. It goes on the strip; heard to its end, it goes.
        let mut u = Unseen::default();
        replace(&mut u, &reply(2, "Still talking."), false, false);
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
        u.heard(2);
        assert_eq!(u.strip, None);
    }

    #[test]
    fn outside_handsfree_the_strip_is_as_ever() {
        let mut u = Unseen::default();
        u.missed(5, &chars("\n  First line\nmore"), false);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "First line".into(), refused: false }));
        // The same reply again (it folded, then its hold ran out): still one.
        u.missed(5, &chars("First line"), false);
        u.missed(6, &chars("Second"), false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((2, "Second")));
        // He saw them: gone, and a seen turn leaves none.
        u.ack(Some(6));
        assert_eq!(u.shown(false), None);
        u.missed(6, &chars("Second"), false);
        assert_eq!(u.strip, None);
        // The Line open: he is reading it.
        u.missed(7, &chars("Third"), true);
        assert_eq!(u.strip, None);
        // A push-to-talk / typed / event reply (never `heard`): a strip.
        u.missed(8, &chars("Fourth"), false);
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
    }

    #[test]
    fn no_new_pill_while_handsfree_is_on() {
        let mut u = Unseen::default();
        u.missed(5, &chars("A reply"), false);
        assert!(u.strip.is_some() && u.shown(true).is_none(), "kept, not shown");
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
        // A refused message is no "N NEW": it shows, hands-free or not.
        u.refused("NOT SENT (no session) · \"hi\"".into());
        assert!(u.shown(true).is_some_and(|s| s.refused));
    }

    #[test]
    fn a_handsfree_reply_is_seen_once_spoken_even_after_handsfree_ends() {
        let mut u = Unseen::default();
        u.ack(Some(6)); // his hands-free turn 7 went out
        u.heard(7); // her reply to it is said whole
        // Hands-free timed out (60 s quiet) before the bubble's hold ran out: no pill after.
        u.missed(7, &chars("Her spoken reply"), false);
        assert_eq!((u.strip.as_ref(), u.shown(false)), (None, None));
        // It went behind the chip mid-speech (a teleport) before it was said: dropped when said.
        u.missed(8, &chars("Folded early"), false);
        u.heard(8);
        assert_eq!(u.strip, None);
        // ...and only it, when others were missed before it.
        let mut u = Unseen::default();
        u.missed(3, &chars("Older"), false);
        u.missed(4, &chars("Folded early"), false);
        u.heard(4);
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
    }

    #[test]
    fn a_spoken_reply_is_read_only_if_heard_whole() {
        use crate::voice::Said;
        let mut u = Unseen::default();
        // Said whole (his spoken turn, an event's reply; hands-free or not): read before its
        // bubble goes, so no strip after it.
        let whole = Said { sent: 2, ended: 2, done: true, cut: false };
        assert!(whole.heard(false));
        u.heard(5);
        u.missed(5, &chars("Build's green, choom."), false);
        assert_eq!((u.strip.as_ref(), u.shown(false)), (None, None));
        // Cut (a tap, the talk key, music, the sidecar gone) or said in part: never heard,
        // so its bubble going leaves a strip, "1 NEW" (the tag, the Line, `state` agree: all
        // are this strip).
        for said in [Said { sent: 3, ended: 1, done: true, cut: false }, Said { sent: 2, ended: 2, done: true, cut: true }] {
            assert!(!said.heard(false));
        }
        assert!(!whole.heard(true), "said in part");
        u.missed(6, &chars("Rebasing polish-e onto main."), false);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "Rebasing polish-e onto main.".into(), refused: false }));
        // A long one went behind the chip mid-speech (a teleport): counted, then heard whole.
        u.missed(7, &chars("Checking CI."), false);
        assert_eq!(u.shown(false).map(|s| s.n), Some(2));
        u.heard(7);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((1, "Rebasing polish-e onto main.")), "the cut one stays");
    }

    #[test]
    fn heard_drops_only_its_own_reply_from_the_strip() {
        // Turn 6 missed; turn 7 goes behind the chip mid-speech (a teleport), then is heard
        // whole: 1 NEW, turn 6's line (never 7's).
        let mut u = Unseen::default();
        u.missed(6, &chars("Six: CI is red on main."), false);
        u.missed(7, &chars("Seven: reran it."), false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((2, "Seven: reran it.")));
        u.heard(7);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "Six: CI is red on main.".into(), refused: false }));
        // Turn 7's bubble hold runs out after: still turn 6's strip.
        u.missed(7, &chars("Seven: reran it."), false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((1, "Six: CI is red on main.")));
        // Two went behind the chip (6 and 7), then 6 is heard whole: 7 stays, and only it.
        let mut u = Unseen::default();
        u.missed(6, &chars("Six"), false);
        u.missed(7, &chars("Seven"), false);
        u.heard(6);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "Seven".into(), refused: false }));
        u.heard(7);
        assert_eq!(u.strip, None);
        // Past MISSED_KEEP the oldest are forgotten but still counted.
        let mut u = Unseen::default();
        for t in 1..=40 {
            u.missed(t, &chars(&format!("Reply {t}")), false);
        }
        u.heard(40);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((39, "Reply 39")));
        // A refused message, then a reply: it gives way, counted from one.
        u.refused("NOT SENT (no session) · \"hi\"".into());
        u.missed(41, &chars("After"), false);
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "After".into(), refused: false }));
    }

    /// `$mod+c` while she speaks (talk_press, unchanged): voice_press hushes her (a cut, so
    /// `heard` never comes), then the press opens the Line, or finds it open.
    /// The reply is read as seen there, never as heard. A body tap hushes and opens no Line:
    /// it stays unread.
    #[test]
    fn mod_c_hush_is_read_as_seen_on_the_line_not_as_heard() {
        use crate::voice::Said;
        // voice_stop_speech starts Said again: never heard whole (its say-ends don't count).
        assert!(!Said::default().heard(false), "a hush is a cut");
        // The Line closed: open_line acks the shown turn (ack_unread); its hold running out
        // later leaves nothing, Line open or closed again by then.
        let mut u = Unseen::default();
        u.ack(Some(9));
        u.missed(9, &chars("Spoken, hushed by mod+c"), true);
        u.missed(9, &chars("Spoken, hushed by mod+c"), false);
        assert_eq!(u.strip, None);
        // The Line already open: he is reading it there, so its bubble going leaves no strip.
        let mut u = Unseen::default();
        u.missed(9, &chars("Spoken, hushed by mod+c"), true);
        assert_eq!(u.strip, None);
        // A tap on her body: the same cut, no Line, so the strip.
        let mut u = Unseen::default();
        u.missed(9, &chars("Spoken, hushed by a tap"), false);
        assert_eq!(u.shown(false).map(|s| s.n), Some(1));
    }

    #[test]
    fn unread_from_before_handsfree_comes_back_when_it_ends() {
        let mut u = Unseen::default();
        u.missed(5, &chars("Before hands-free"), false);
        // Hands-free: hidden; her replies there (turns 6, 7) are heard, the count doesn't grow.
        assert_eq!(u.shown(true), None);
        for t in [6, 7] {
            u.heard(t);
            u.missed(t, &chars("Spoken"), false);
        }
        // It ends: the one from before is back, as it was.
        assert_eq!(u.shown(false), Some(&Unread { n: 1, text: "Before hands-free".into(), refused: false }));
        // A reply after it, not heard (typed): counted on top.
        u.missed(8, &chars("Typed reply"), false);
        assert_eq!(u.shown(false).map(|s| (s.n, s.text.as_str())), Some((2, "Typed reply")));
    }
}

/// A press `age` old, still without its release, is a hold a tap must not end: too young to
/// have opened its mic yet, or its mic (`listening`) is asked for or open, until `TALK_STALE`.
fn press_holds(age: Duration, listening: bool) -> bool {
    age < TAP_GUARD || (listening && age < TALK_STALE)
}

/// The Line's thread from the session's transcript, oldest first, the newest `ROWS_MAX` (thinking
/// left out). A rollover's new window shows as one quiet row where its head is (`ROLLED_OVER`);
/// Synaps' notices about it go to her log, not here.
fn thread_rows(items: &[synaps_client::transcript::Item], model: Option<&str>, unpicked: impl Fn(u64) -> bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for it in items.iter().rev() {
        use synaps_client::transcript::ItemKind as K;
        let row = match it.kind {
            K::Thinking => continue,
            // A rolled-over context window's head is the model's note, not his words.
            K::User if crate::session::rollover_head(&it.text) => {
                Row { who: Who::Sys, text: crate::session::ROLLED_OVER.into(), detail: String::new(), streaming: false }
            }
            K::User => match crate::session::origin(&it.text) {
                Some((who, kind, text)) => Row { who: Who::Relay(who, kind), text, detail: String::new(), streaming: false },
                None => match crate::voice::spoken(&it.text) {
                    Some(said) => Row { who: Who::You, text: format!("◉ {}", said.trim()), detail: String::new(), streaming: false },
                    None => Row { who: Who::You, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
                },
            },
            K::Assistant => {
                if it.text.trim().is_empty() {
                    continue;
                }
                Row { who: Who::Her, text: it.text.trim().to_string(), detail: String::new(), streaming: it.streaming }
            }
            K::Tool => {
                let Some(t) = &it.tool else { continue };
                let mark = match t.status {
                    Some(st) if st.is_terminal() => match st {
                        synaps_client::transcript::ToolStatus::Done => "✓",
                        synaps_client::transcript::ToolStatus::Denied => "⊘ denied",
                        synaps_client::transcript::ToolStatus::Cancelled => "✕ cancelled",
                        _ => "✕",
                    },
                    _ => "…",
                };
                let name = t.name.rsplit("__").next().unwrap_or(&t.name);
                // One line of the input (LOCKED #11.4), only here, in the thread he opened.
                Row { who: Who::Tool, text: format!("{name} {mark}"), detail: crate::session::one_line(&t.input, 110), streaming: false }
            }
            // Synaps' rollover notices go to her log (the head's row says it here).
            K::Notice if crate::session::rollover_notice(&it.text) => continue,
            K::Notice => Row { who: Who::Sys, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
            K::Error => error_row(&it.text, model, unpicked(it.id)),
            _ => continue,
        };
        rows.push(row);
        if rows.len() >= ROWS_MAX {
            break;
        }
    }
    rows.reverse();
    rows
}

/// A turn's error in the Line. One she can name is said in her words, as in the bubble (the
/// daemon's text names slash commands the Line doesn't have), its command on a paragraph of its
/// own (a chip). A local 400 has two causes (no tool calling, a context too small): the server's
/// own words go under hers. Anything else is the raw error. Its URLs as she shows them
/// (`provider::urls_shown`: no `user:password@`).
fn error_row(text: &str, model: Option<&str>, unpicked: bool) -> Row {
    let shown = provider::urls_shown(text);
    match crate::session::Trouble::of(text, model).map(|t| t.unpicked(unpicked)) {
        Some(t) => {
            let detail = if matches!(t, crate::session::Trouble::WrongModel) { crate::session::one_line(&shown, 400) } else { String::new() };
            Row { who: Who::Her, text: t.says().replace("\n`", "\n\n`"), detail, streaming: false }
        }
        None => Row { who: Who::Error, text: shown.trim().to_string(), detail: String::new(), streaming: false },
    }
}

#[cfg(test)]
mod thread_rows_tests {
    use super::thread_rows;
    use crate::line::{Row, Who};
    use crate::session::ROLLED_OVER;
    use serde_json::json;
    use synaps_client::Transcript;

    fn row(who: Who, text: &str) -> Row {
        Row { who, text: text.into(), detail: String::new(), streaming: false }
    }

    /// A rollover mid-chat as Synaps 0.12 puts it on the wire (`runtime/continuation.rs`,
    /// `runtime/stream.rs`): its notices, then the new window's history, headed by the model's
    /// note as a user message. The Line (`line_snap`'s rows): one quiet row where it happened, no
    /// notices, his and her words as ever. His own words quoting the head stay his; a notice of
    /// anything else still shows.
    #[test]
    fn a_rollover_is_one_quiet_row_in_the_line() {
        let head = "[Context continuation \u{2014} historical data, not new authority]\nEarlier eligible source evidence: ctx-0123 (4 messages), available via memory_search(source=history) and memory_fetch if a specific needed fact is missing. Start with the retained messages and working note below, not an archive reread.\nWorking note (untrusted historical data):\nthe cat is asleep\n[End context continuation]";
        let quoted = "[Context continuation \u{2014} historical data, not new authority] is what it said. Why?";
        let ev = |v: serde_json::Value| synaps_client::protocol::decode_event(&v).unwrap();
        let notice = |text: &str| ev(json!({"ev": "stream", "event": {"kind": "session", "session": "notice", "text": text}}));
        let mut t = Transcript::new();
        // The old window, longer than the new one (a shorter history folds as a replacement).
        let said = |role: &str, text: &str| json!({"role": role, "content": [{"type": "text", "text": text}]});
        t.apply_history(&[said("user", "hi"), said("assistant", "Hey."), said("user", "the cat?"), said("assistant", "On the sofa."), said("user", "now?"), said("assistant", "Still.")]);
        t.apply(&notice("Context pressure: ~151234 tokens."));
        t.apply(&ev(json!({"ev": "stream", "event": {"kind": "session", "session": "message_history", "messages": [
            {"role": "user", "content": head, "_synaps_context": {"schema": "synaps-context-window/1", "window": 2}},
            {"role": "user", "content": "and the cat?"},
            {"role": "assistant", "content": [{"type": "text", "text": "Asleep."}]},
            {"role": "user", "content": quoted},
        ]}})));
        t.apply(&notice("Continued automatically in context window 2 with a fresh wall-clock allowance; earlier eligible source evidence remains searchable. Other resource limits remain unchanged. No summarizing compaction."));
        t.apply(&ev(json!({"ev": "system_notice", "text": "daemon reloading, reconnecting..."})));
        let rows = thread_rows(t.items(), Some("anthropic/claude-sonnet-4-6"), |_| false);
        assert_eq!(rows.iter().filter(|r| r.text == ROLLED_OVER).collect::<Vec<_>>(), [&row(Who::Sys, ROLLED_OVER)], "{rows:#?}");
        let at = |r: &Row| rows.iter().position(|x| x == r);
        assert!(at(&row(Who::Sys, ROLLED_OVER)) < at(&row(Who::You, "and the cat?")), "{rows:#?}");
        for r in [row(Who::You, "and the cat?"), row(Who::Her, "Asleep."), row(Who::You, quoted), row(Who::Sys, "daemon reloading, reconnecting...")] {
            assert!(at(&r).is_some(), "{r:?} in {rows:#?}");
        }
        assert!(!rows.iter().any(|r| r.text.contains("Context pressure") || r.text.starts_with("Continued automatically") || r.text.contains("Working note")), "{rows:#?}");
    }
}

#[cfg(test)]
mod error_row_tests {
    use super::error_row;
    use crate::line::Who;

    /// No brain on a model no one picked: a brain to pick (SET ME UP), not Claude's login. A
    /// model he picked: its own login, named.
    #[test]
    fn no_brain_picked_points_at_the_brain_screen() {
        let raw = "No Anthropic credentials. Run `synaps login`, or switch to a provider model with `/model groq/llama-3.3-70b-versatile`.";
        let r = error_row(raw, Some("claude-sonnet-4-6"), true);
        assert!(r.text.ends_with("\n\n`SET ME UP ▸`") && !r.text.contains("synaps login") && !r.text.contains("Claude"), "{}", r.text);
        let r = error_row(raw, Some("claude-sonnet-4-6"), false);
        assert!(r.text.ends_with("\n\n`synaps login --provider claude`"), "{}", r.text);
        let codex = "request failed: credential error: No credentials for openai-codex at /home/u/.synaps/auth.json. Run `synaps login`.";
        assert!(error_row(codex, Some(crate::provider::CHATGPT_DEFAULT.0), false).text.contains("ChatGPT"));
    }

    #[test]
    fn a_local_400_keeps_the_servers_words() {
        let raw = "openai request failed: broker transport error: provider request failed: 400 Bad Request: registry.ollama.ai/library/gemma3:4b does not support tools";
        let r = error_row(raw, Some("local/gemma3:4b"), false);
        assert_eq!(r.who, Who::Her);
        assert!(r.text.starts_with("My local model turned that down."), "{}", r.text);
        assert_eq!(r.detail, raw, "which of the two 400s it was");
        let r = error_row("openai request failed: broker transport error: provider request failed: 404 Not Found [not_found_error]", Some("local/qwen3:8b"), false);
        assert_eq!((r.who, r.detail.as_str()), (Who::Her, ""), "her words say it all");
        assert!(r.text.ends_with("\n\n`ollama pull qwen3:8b`"), "the command a paragraph of its own: {}", r.text);
        let r = error_row("tool registry: unknown tool", Some("local/qwen3:8b"), false);
        assert_eq!((r.who, r.text.as_str(), r.detail.as_str()), (Who::Error, "tool registry: unknown tool", ""));
        // Synaps quotes its local URL whole: shown without its `user:password@`.
        let r = error_row("proxy at http://bob:hunter2@10.0.0.2:3128/ said no", Some("local/qwen3:8b"), false);
        assert_eq!((r.who, r.text.as_str()), (Who::Error, "proxy at http://10.0.0.2:3128/ said no"));
        let r = error_row("provider request failed: 400 Bad Request at http://u:pw@lm:1234/v1: context too small", Some("local/qwen3:8b"), false);
        assert!(r.detail.contains("http://lm:1234/v1") && !r.detail.contains("pw"), "{}", r.detail);
    }
}

#[cfg(test)]
mod tap_tests {
    // Not `super::*`: view's globs bring GPUI's own `test` macro, which `#[test]` would resolve to.
    use super::{TALK_STALE, press_holds};
    use std::time::Duration;

    #[test]
    fn a_tap_clears_a_press_that_lost_its_release() {
        let s = Duration::from_secs;
        // Its mic may still open (at TAP_MS): the tap waits.
        assert!(press_holds(Duration::from_millis(300), false));
        // No mic past that: the release went missing (voice off, the sidecar away, a lost
        // `--release`). The tap goes ahead instead of being ignored for TALK_STALE.
        assert!(!press_holds(s(2), false));
        // A hold that listens is his: the tap waits, up to TALK_STALE.
        assert!(press_holds(s(90), true));
        assert!(!press_holds(TALK_STALE, true));
    }
}

#[cfg(test)]
mod brain_look_tests {
    use super::{BrainLook, LOOK_EVERY};
    use std::time::{Duration, Instant};

    fn found(signed: &[&'static str]) -> BrainLook {
        BrainLook { signed: signed.to_vec(), synaps: "synaps".into(), ..BrainLook::default() }
    }

    /// One look at a time, at most every LOOK_EVERY; the open Brain screen's next is due then.
    #[test]
    fn a_look_at_most_every_look_every() {
        let t0 = Instant::now();
        let mut b = BrainLook::default();
        assert_eq!(b.due(t0), Some(t0), "never looked: now");
        assert!(b.start(t0));
        assert!(!b.start(t0), "one under way");
        assert_eq!(b.due(t0), None);
        assert!(!b.done(found(&[]), t0));
        assert_eq!(b.due(t0), Some(t0 + LOOK_EVERY));
        assert!(!b.start(t0 + LOOK_EVERY - Duration::from_millis(1)));
        assert!(b.start(t0 + LOOK_EVERY), "the open screen looks again (a sign-in in his terminal)");
        assert!(!b.done(found(&["openai-codex"]), t0 + LOOK_EVERY));
        assert_eq!(b.signed, ["openai-codex"]);
    }

    /// A login lands while a look is under way (it may have read the files before): what it
    /// found shows, but stale (no timestamp), and another look follows at once.
    #[test]
    fn a_login_during_a_look_looks_again() {
        let t0 = Instant::now();
        let mut b = BrainLook::default();
        assert!(b.start(t0));
        b.stale();
        assert!(!b.start(t0), "still one at a time");
        let t1 = t0 + Duration::from_millis(300);
        assert!(b.done(found(&[]), t1), "again");
        assert_eq!((b.at, b.busy, b.again), (None, false, false), "the stale look doesn't hold off the next");
        assert!(b.start(t1));
        assert!(!b.done(found(&["anthropic"]), t1));
        assert_eq!((b.at, b.signed.clone()), (Some(t1), vec!["anthropic"]));
        // Stale with none under way: the next look goes at once, nothing owed after it.
        b.stale();
        assert_eq!((b.due(t1), b.again), (Some(t1), false));
    }
}

#[cfg(test)]
mod probe_tests {
    use super::{BrainLook, LOOK_EVERY, PROBE_MAX, probe_due, probed};
    use crate::provider::Local;
    use std::time::{Duration, Instant};

    /// A server that answers (with her model) is asked again after LOOK_EVERY, the wait doubling
    /// (to PROBE_MAX) while it answers the same; a new answer or URL starts over, and a page
    /// opened asks at once. One that's off, or without her model, is asked every look (he may be
    /// starting it, or pulling her model).
    #[test]
    fn the_local_server_is_asked_less_while_nothing_changes() {
        let t0 = Instant::now();
        let url = "http://localhost:11434/v1";
        let up = || Local::Up(vec!["qwen3:8b".into()]);
        assert!(probe_due(None, url, t0), "never asked");
        let mut p = probed(None, url.into(), up(), None, t0);
        assert_eq!(p.wait, LOOK_EVERY);
        assert!(!probe_due(Some(&p), url, t0 + LOOK_EVERY - Duration::from_millis(1)));
        assert!(probe_due(Some(&p), "http://localhost:1234/v1", t0), "another URL: at once");
        let mut t = t0;
        let mut waits = Vec::new();
        for _ in 0..6 {
            t = p.at + p.wait;
            assert!(probe_due(Some(&p), url, t));
            p = probed(Some(&p), url.into(), up(), Some("local/qwen3:8b"), t);
            waits.push(p.wait.as_secs());
        }
        assert_eq!(waits, [10, 20, 40, 40, 40, 40]);
        p = probed(Some(&p), url.into(), Local::Up(vec!["llama3:8b".into()]), None, t);
        assert_eq!(p.wait, LOOK_EVERY, "a model pulled: back to every look");
        p = probed(Some(&p), "http://localhost:1234/v1".into(), up(), None, t);
        assert_eq!(p.wait, LOOK_EVERY, "another URL starts over");
        // Off: every look, however long (he may be starting it).
        let mut off = probed(None, url.into(), Local::Off, None, t0);
        for _ in 0..4 {
            off = probed(Some(&off), url.into(), Local::Off, None, off.at + off.wait);
            assert_eq!(off.wait, LOOK_EVERY, "off: asked every look");
        }
        // Up without her model (he's pulling it): every look too.
        let mut pulling = probed(None, url.into(), Local::Up(vec!["llama3:8b".into()]), Some("local/qwen3:8b"), t0);
        pulling = probed(Some(&pulling), url.into(), Local::Up(vec!["llama3:8b".into()]), Some("local/qwen3:8b"), pulling.at + pulling.wait);
        assert_eq!(pulling.wait, LOOK_EVERY, "her model not there yet: asked every look");
        // A Settings page opened: asked on its look, whatever the wait.
        p.wait = PROBE_MAX;
        let mut b = BrainLook { probed: Some(p), ..BrainLook::default() };
        assert!(!probe_due(b.probed.as_ref(), "http://localhost:1234/v1", t + LOOK_EVERY));
        b.opened();
        assert!(probe_due(b.probed.as_ref(), "http://localhost:1234/v1", t));
    }

    /// A page opened while a look is under way: that look (which didn't probe) doesn't take back
    /// the probe owed; one that did probe stands.
    #[test]
    fn a_page_opened_mid_look_still_gets_its_probe() {
        let t = Instant::now();
        let url = "http://localhost:11434/v1";
        let p = probed(None, url.into(), Local::Up(vec!["qwen3:8b".into()]), None, t);
        let mut b = BrainLook { probed: Some(p.clone()), ..BrainLook::default() };
        assert!(b.start(t));
        let unprobed = BrainLook { probed: Some(p.clone()), ..BrainLook::default() };
        b.opened();
        b.done(unprobed, t + Duration::from_secs(1));
        assert!(probe_due(b.probed.as_ref(), url, t + Duration::from_secs(1)), "still owed");
        // The next look probes: its wait stands.
        assert!(b.start(t + LOOK_EVERY + Duration::from_secs(1)));
        b.opened();
        let fresh = probed(Some(&p), url.into(), Local::Up(vec!["qwen3:8b".into()]), None, t + Duration::from_secs(2));
        b.done(BrainLook { probed: Some(fresh), ..BrainLook::default() }, t + Duration::from_secs(3));
        assert!(!probe_due(b.probed.as_ref(), url, t + Duration::from_secs(3)), "it probed: its wait stands");
    }
}

#[cfg(test)]
mod model_held_tests {
    use super::{LATER, model_held};

    /// What she says after a MODEL write: nothing of her session when it has the model or it's
    /// on its way; else saved for her next start, and why when another client owns its input.
    #[test]
    fn a_model_her_session_cant_take_now_is_for_her_next_start() {
        assert_eq!(model_held(true, false, false), None, "sent");
        assert_eq!(model_held(false, true, false), None, "has it, or asked it already");
        assert_eq!(model_held(true, false, true), None, "sent wins over an old block");
        assert_eq!(model_held(false, false, true), Some("Saved for my next start: another client has my session's input."));
        assert_eq!(model_held(false, false, false), Some(LATER));
    }
}
