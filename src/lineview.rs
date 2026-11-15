//! The Line, from the avatar's side: the summon key (`talk press|release`), where the Line
//! opens, the thread it shows (the session's transcript), what it asks for, the unread strip
//! a missed reply leaves on her tag, and copy. A child module of `view`.

use super::*;
use crate::line::{Ask, Line, Row, Snap, Who};
use crate::platform::x11::Monitor;

/// A press held this long is a hold (voice, later); shorter is a tap (type).
pub(super) const TAP_MS: u128 = 250;
const LINE_W: f32 = 780.0;
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
}

/// A reply he didn't see before its bubble went: one line on her tag until he acknowledges
/// it (LOCKED #11.3), or a refused message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Unread {
    pub n: u32,
    pub text: String,
    pub refused: bool,
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

    /// The Line's place (device px) on the monitor he works on (his focused window's, else
    /// the pointer's), at the bottom of its work area with her standing at one edge:
    /// (x, bottom, w, h, area, her ground x, her side).
    fn line_place(&self, h: i32) -> (i32, i32, i32, i32, Monitor, i32, i8) {
        let sf = self.layout.sf;
        let ours = self.x11.win;
        let focused = self
            .x11
            .active_window()
            .or_else(|| self.x11.focused_window().ok())
            .filter(|&w| w > 1 && w != ours && self.line.as_ref().is_none_or(|l| l.xid != w))
            .and_then(|w| self.x11.toplevel_rect(w));
        let at = focused.map(|r| r.center()).or_else(|| self.x11.pointer().ok()).unwrap_or_else(|| {
            let g = self.ground();
            (g.0, g.1 - 1)
        });
        let area = self.x11.area_at(at.0, at.1);
        let l = self.layout;
        let body = (l.geo.width as f32 * 0.62) as i32;
        let margin = (12.0 * sf) as i32;
        let w = ((LINE_W * sf) as i32).min(area.w - body - 3 * margin).max(1);
        let bottom = area.y + area.h - (6.0 * sf) as i32;
        let h = h.min(area.h - 2 * margin).max(1);
        // Where she stands now, if that's on this floor (she stays put if the Line fits there).
        let (gx, gy) = self.ground();
        let on_floor = matches!(self.placement, Placement::Ground) && gy > area.y + area.h * 2 / 3 && gx >= area.x && gx < area.x + area.w;
        let est_h = h.max(area.h / 3 + (90.0 * sf) as i32);
        let p = place_line((area.x, area.y, area.w, area.h), focused.map(|r| (r.x, r.y, r.w, r.h)), on_floor.then_some(gx), w, body, est_h, margin);
        (p.0, bottom, w, h, area, p.1, p.2)
    }

    pub(super) fn open_line(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        let sf = self.layout.sf;
        let h0 = (LINE_H0 * sf) as i32;
        let (x, bottom, w, h, area, her_x, side) = self.line_place(h0);
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
        if xid != 0 && std::env::var_os("SA_LINE_NOFOCUS").is_none() {
            self.x11.give_focus(xid).ok();
        }
        self.line = Some(LineWin { view: view.clone(), handle: handle.into(), xid, prev, at: (x, bottom, w, h), parked: false });
        self.x11.arm_clicks(true);
        self.line_opened_ms = Some(now.elapsed().as_millis());
        // He is reading the thread now: nothing is unread, and the bubble gives way. Opened
        // with a reply unread (her strip, the summon key): the thread starts at that reply.
        if self.unread.as_ref().is_some_and(|u| !u.refused) {
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
        let side = if self.line_fetch_her(area, her_x, now) { side } else { 0 };
        if let Some(l) = &self.line {
            let (v, hd) = (l.view.clone(), l.handle);
            cx.defer(move |cx| {
                hd.update(cx, |_, _, cx| v.update(cx, |l, cx| l.set_side(side, cx))).ok();
            });
        }
        elog!("synaps-avatar: line open at {x},{} {w}x{h} (0x{xid:x}) in {} ms", bottom - h, now.elapsed().as_millis());
        cx.notify();
        self.schedule(cx);
        format!("ok line {x},{} {w}x{h}", bottom - h)
    }

    /// She stands at the Line's edge (on its floor, at `her_x`): a teleport if she is
    /// anywhere else (another monitor: LOCKED #11.2). False if she can't come.
    fn line_fetch_her(&mut self, area: Monitor, her_x: i32, now: Instant) -> bool {
        if self.rag.is_some() || self.hidden || !self.loaded {
            return false;
        }
        let (gx, gy) = self.ground();
        let floor = area.y + area.h;
        let l = self.layout;
        if matches!(self.placement, Placement::Ground) && (gx - her_x).abs() <= (8.0 * l.sf) as i32 && (gy - floor).abs() <= (8.0 * l.sf) as i32 {
            return true;
        }
        let (here, there) = (self.x11.monitor_at(gx, gy - 1), self.x11.monitor_at(area.x + area.w / 2, area.y + area.h / 2));
        let other = (here.x, here.y) != (there.x, there.y);
        let o = Self::clamp(&self.x11, &self.layout, (her_x - l.ground_x, floor - l.ground_y));
        self.brain_cancel_move();
        self.start_teleport(o, Placement::Ground, now);
        self.home = (o.0 + l.ground_x, o.1 + l.ground_y);
        elog!("synaps-avatar: line: she comes {}to its edge at {},{}", if other { "over from another monitor " } else { "" }, self.home.0, self.home.1);
        true
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
                self.voice_cancel(cx);
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
                if let Some(l) = &mut self.line {
                    let (_, _, _, _, area) = {
                        let (x, b, w, _) = l.at;
                        let a = self.x11.area_at(x + w / 2, b - 1);
                        (x, b, w, 0, a)
                    };
                    let hd = ((h * sf) as i32).min(area.h - (36.0 * sf) as i32).max((60.0 * sf) as i32);
                    if hd != l.at.3 {
                        let (x, bottom, w, _) = l.at;
                        l.at.3 = hd;
                        self.x11.configure(l.xid, x, bottom - hd, w, hd).ok();
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
        self.unread = None;
        self.seen_turn = self.session.as_ref().and_then(|s| s.shown.as_ref()).map_or(self.seen_turn, |b| b.turn);
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
        if self.line.is_some() || turn <= self.seen_turn {
            return;
        }
        let first: String = text.iter().collect::<String>().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
        if first.is_empty() {
            return;
        }
        let n = match &self.unread {
            Some(u) if !u.refused && self.unread_turn != turn => u.n + 1,
            Some(u) if !u.refused => u.n,
            _ => 1,
        };
        self.unread_turn = turn;
        self.unread = Some(Unread { n, text: first, refused: false });
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

/// Where the Line goes along the floor of `area` (x, y, w, h): the spot that covers the least
/// of his focused window `fw`, then the one that moves her least (`her`: her ground x if she
/// stands on this floor), with her `body` px wide at one edge. (Line x, her ground x, her
/// side: -1 left of the Line, 1 right.)
pub fn place_line(area: (i32, i32, i32, i32), fw: Option<(i32, i32, i32, i32)>, her: Option<i32>, w: i32, body: i32, h: i32, margin: i32) -> (i32, i32, i8) {
    let (ax, ay, aw, ah) = area;
    let top = ay + ah - h;
    let gap = margin / 2;
    let fits = |lx: i32, side: i8| {
        let (bx0, bx1) = if side < 0 { (lx - gap - body, lx + w) } else { (lx, lx + w + gap + body) };
        bx0 >= ax + margin && bx1 <= ax + aw - margin
    };
    let her_x = |lx: i32, side: i8| if side < 0 { lx - gap - body / 2 } else { lx + w + gap + body / 2 };
    let mut cands: Vec<(i32, i8)> = Vec::new();
    for side in [-1i8, 1] {
        if let Some(g) = her {
            cands.push((if side < 0 { g + body / 2 + gap } else { g - body / 2 - gap - w }, side));
        }
        let left = if side < 0 { ax + margin + body + gap } else { ax + margin };
        let right = if side < 0 { ax + aw - margin - w } else { ax + aw - margin - body - gap - w };
        cands.push((left, side));
        cands.push((right, side));
        cands.push(((left + right) / 2, side));
    }
    let cover = |lx: i32| {
        fw.map_or(0i64, |(fx, fy, fwid, fh)| {
            let ox = (lx + w).min(fx + fwid) - lx.max(fx);
            let oy = (ay + ah).min(fy + fh) - top.max(fy);
            if ox > 0 && oy > 0 { ox as i64 * oy as i64 } else { 0 }
        })
    };
    let best = cands
        .into_iter()
        .filter(|&(lx, side)| fits(lx, side))
        .min_by_key(|&(lx, side)| {
            let moved = her.map_or(0, |g| (her_x(lx, side) - g).abs() as i64);
            // Covering his work outweighs any walk: 1 % of the Line over his window ~ 2 m.
            (cover(lx) * 100 / (w as i64 * h as i64).max(1)) * 4000 + moved
        });
    match best {
        Some((lx, side)) => (lx, her_x(lx, side), side),
        None => {
            let lx = ax + (aw - w) / 2;
            (lx, (ax + body / 2).max(lx - gap - body / 2), -1)
        }
    }
}

#[cfg(test)]
mod place_tests {
    use super::place_line;

    const AREA: (i32, i32, i32, i32) = (0, 28, 1920, 1052);

    #[test]
    fn she_stays_put_and_the_line_opens_beside_her() {
        // His editor fills the work area (tiled): every spot covers the same; she doesn't move.
        let (lx, her, side) = place_line(AREA, Some(AREA), Some(1500), 700, 200, 420, 12);
        assert_eq!(her, 1500);
        assert!(lx + 700 <= 1400 || lx >= 1600, "the Line is beside her, not over her: {lx} {side}");
    }

    #[test]
    fn free_space_beats_covering_his_window() {
        // A floating editor on the left half: the Line goes right, she comes along.
        let ed = (0, 28, 960, 1052);
        let (lx, _, _) = place_line(AREA, Some(ed), Some(300), 700, 200, 420, 12);
        assert!(lx >= 960, "{lx}");
    }

    #[test]
    fn she_is_at_its_edge_and_both_fit_the_monitor() {
        for her in [None, Some(40), Some(1900), Some(960)] {
            let (lx, h, side) = place_line(AREA, None, her, 700, 200, 420, 12);
            assert!(lx >= 0 && lx + 700 <= 1920);
            let edge = if side < 0 { lx } else { lx + 700 };
            assert!((h - edge).abs() <= 6 + 100 + 1, "{her:?}: her {h} edge {edge}");
            assert!(h - 100 >= 0 && h + 100 <= 1920, "{her:?}: {h}");
        }
    }
}
