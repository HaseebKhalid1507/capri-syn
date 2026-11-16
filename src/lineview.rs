//! The Line, from the avatar's side: the summon key (`talk press|release`), where the Line
//! opens, the thread it shows (the session's transcript), what it asks for, the unread strip
//! a missed reply leaves on her tag, and copy. A child module of `view`.

use super::*;
use crate::beside::{self, Panel, Place};
use crate::line::{Ask, Line, Row, Snap, Who};

/// A press held this long is a hold (voice, later); shorter is a tap (type).
pub(super) const TAP_MS: u128 = 250;
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

/// What he hasn't seen: her strip, the last turn he has seen, and the turn the strip is about.
#[derive(Clone, Debug, Default)]
pub(super) struct Unseen {
    pub strip: Option<Unread>,
    seen: u64,
    turn: u64,
}

impl Unseen {
    /// He has seen up to the shown turn (the Line showed it, he clicked the strip, he sent the
    /// next message).
    pub fn ack(&mut self, shown: Option<u64>) {
        self.strip = None;
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
        let n = match &self.strip {
            Some(u) if !u.refused && self.turn != turn => u.n + 1,
            Some(u) if !u.refused => u.n,
            _ => 1,
        };
        self.turn = turn;
        self.strip = Some(Unread { n, text: first, refused: false });
    }

    /// A reply he heard in a hands-free conversation: seen once spoken, whenever its bubble
    /// goes. A strip already about it (it went behind the chip mid-speech) drops it; one about
    /// replies before it stays.
    pub fn heard(&mut self, turn: u64) {
        self.seen = self.seen.max(turn);
        if let Some(u) = &mut self.strip
            && !u.refused
            && self.turn == turn
        {
            if u.n > 1 {
                u.n -= 1;
            } else {
                self.strip = None;
            }
        }
    }

    /// The strip on her tag: no "N NEW" while hands-free is on (he hears her replies; one
    /// from before comes back when it ends). A refused message always shows.
    pub fn shown(&self, handsfree: bool) -> Option<&Unread> {
        self.strip.as_ref().filter(|u| u.refused || !handsfree)
    }

    pub fn refused(&mut self, text: String) {
        self.strip = Some(Unread { n: 1, text, refused: true });
    }
}

impl Avatar {
    /// `talk press`: open the Line (or resume it parked, or close it).
    pub(super) fn talk_press(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        // Held keys repeat: a press while down, or inside the key-up window, is the same press.
        if self.voice_repeat() || self.talk_down.is_some_and(|t| t.elapsed() < Duration::from_secs(120)) {
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
        let Some(t0) = self.talk_down else { return "ok (no press)".into() };
        let ms = t0.elapsed().as_millis();
        if !self.voice_up(cx) {
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
            let m = self.x11.area_at(her.center().0, her.center().1);
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
            self.x11.configure(l.xid, r.x, r.y, r.w, r.h).ok();
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
        let ours = self.x11.win;
        let prev = self.x11.active_window().or_else(|| self.x11.focused_window().ok()).filter(|&w| w > 1 && w != ours);
        // The bubble's field gives way: one place to type.
        if self.input.as_ref().is_some_and(|i| i.secret.is_none()) {
            self.close_input(false);
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(x as f32 / sf), px((bottom - h) as f32 / sf)), size(px(w as f32 / sf), px(h as f32 / sf))))),
            titlebar: Some(TitlebarOptions { title: Some("synaps-avatar line".into()), ..Default::default() }),
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("synaps-avatar".into()),
            ..Default::default()
        };
        let snap = self.line_snap();
        let me = cx.entity().downgrade();
        let (name, font, pal) = (self.name.to_string(), self.font.clone(), self.reader_pal());
        let draft = std::mem::take(&mut self.draft);
        let sent = self.sent.clone();
        let max_thread = area.h as f32 / sf / 3.0;
        let xid = Rc::new(Cell::new(0u32));
        let xid2 = xid.clone();
        let built: Rc<RefCell<Option<Entity<Line>>>> = Rc::default();
        let built2 = built.clone();
        crate::line::kit_init(cx);
        let opened = cx.open_window(options, move |window, cx| {
            if let Ok(RawWindowHandle::Xcb(h)) = HasWindowHandle::window_handle(&*window).map(|h| h.as_raw()) {
                xid2.set(h.window.get());
            } else if let Ok(RawWindowHandle::Xlib(h)) = HasWindowHandle::window_handle(&*window).map(|h| h.as_raw()) {
                xid2.set(h.window as u32);
            }
            let on_ask: crate::line::OnAsk = Box::new(move |ask, cx: &mut App| me.update(cx, |a, cx| a.line_ask(ask, cx)).unwrap_or_else(|_| Err("she is gone".into())));
            let view = cx.new(|cx| Line::new(snap, draft, sent, name, font, pal, max_thread, on_ask, window, cx));
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
        if xid != 0 && std::env::var_os("SA_LINE_NOFOCUS").is_none() {
            self.x11.give_focus(xid).ok();
        }
        self.line = Some(LineWin { view: view.clone(), handle: handle.into(), xid, prev, at: (x, bottom, w, h), parked: false, her, tail: Some((place.side, place.tail)) });
        self.x11.arm_clicks(true);
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
            "synaps-avatar: line open at {x},{} {w}x{h} (0x{xid:x}) in {} ms, {} of her",
            bottom - h,
            now.elapsed().as_millis(),
            crate::line::side_key(place.side)
        );
        cx.notify();
        self.schedule(cx);
        format!("ok line {x},{} {w}x{h}", bottom - h)
    }

    /// The thread, from the session's transcript (thinking left out).
    pub(super) fn line_snap(&self) -> Snap {
        let status = self.status().label;
        let Some(s) = &self.session else { return Snap { status, live: false, ..Default::default() } };
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
                K::Error => Row { who: Who::Error, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
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
            busy: s.link.busy(),
            live: b.is_some_and(|b| b.live),
            prompts: b.map(|b| b.prompts.clone()).unwrap_or_default(),
            speaking: self.voice_speaking(),
            listening: self.voice_listening(),
            mic: self.voice_mic_row(),
            caption: self.voice_caption_now(),
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
                self.x11.arm_clicks(false);
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
                        self.x11.give_focus(w).ok();
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
                self.x11.arm_clicks(!p && self.line.is_some());
                if let Some(l) = &mut self.line {
                    l.parked = p;
                    if !p && l.xid != 0 && std::env::var_os("SA_LINE_NOFOCUS").is_none() {
                        self.x11.give_focus(l.xid).ok();
                    }
                }
                Ok(())
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
        self.x11.arm_clicks(false);
        let back = l.prev.filter(|&p| self.x11.toplevel_rect(p).is_some_and(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom()));
        if let Some(p) = back {
            self.x11.give_focus(p).ok();
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
                v["focused"] = serde_json::json!(self.x11.focused_window().ok() == Some(l.xid));
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
            let text = b.text.clone();
            self.reply_missed(&text);
        }
    }

    /// The bubble's hold ran out: if he never saw this reply, it leaves a strip.
    pub(super) fn reply_missed(&mut self, text: &[char]) {
        let turn = self.session.as_ref().and_then(|s| s.shown.as_ref()).map_or(0, |b| b.turn);
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
    use super::{Unread, Unseen};

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
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
        u.heard(7); // her reply to it is said
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
