//! The Line, from the avatar's side: the summon key (`talk press|release`), where the Line
//! opens, the thread it shows (the session's transcript), what it asks for, the unread strip
//! a missed reply leaves on her tag, and copy. A child module of `view`.

use super::*;
use crate::beside::{self, Panel, Place};
use crate::line::{Ask, Line, Row, Snap, Who};

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
        self.open_line(cx)
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
        let snap = self.line_snap();
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
            let view = cx.new(|cx| Line::new(snap, draft, sent, name, font, pal, max_thread, face, on_ask, window, cx));
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
        self.line = Some(LineWin { view: view.clone(), handle: handle.into(), xid, prev, at: (x, bottom, w, h), parked: waits, waiting: waits, her, tail: Some((place.side, place.tail)) });
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
        if self.unseen.strip.as_ref().is_some_and(|u| !u.refused) {
            let (v, hd) = (view, handle);
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.show_last_reply(cx))).ok();
            });
        }
        self.ack_unread();
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
        let Some(s) = &self.session else { return Snap { status, glyph, flag, alarm, live: false, face, key, ..Default::default() } };
        let items = s.link.transcript().items();
        let mut rows = Vec::new();
        for it in items.iter().rev() {
            use synaps_client::transcript::ItemKind as K;
            let row = match it.kind {
                K::Thinking => continue,
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
                K::Notice => Row { who: Who::Sys, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
                // One she can name is said in her words, as in the bubble (the daemon's text
                // names slash commands the Line doesn't have).
                K::Error => match crate::session::Trouble::of(&it.text, s.link.model()) {
                    // The command on a paragraph of its own: a chip.
                    Some(t) => Row { who: Who::Her, text: t.says().replace("\n`", "\n\n`"), detail: String::new(), streaming: false },
                    None => Row { who: Who::Error, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
                },
                _ => continue,
            };
            rows.push(row);
            if rows.len() >= ROWS_MAX {
                break;
            }
        }
        rows.reverse();
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
        }
    }

    /// Bring an open Line up to date (deferred: the Line may call back).
    pub(super) fn line_push(&mut self, cx: &mut Context<Self>) {
        let Some(l) = &self.line else { return };
        let snap = self.line_snap();
        let (view, handle) = (l.view.clone(), l.handle);
        if self.bubble.as_ref().is_some_and(|b| b.stream) {
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
        self.unseen.missed(turn, text, self.line.is_some());
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
