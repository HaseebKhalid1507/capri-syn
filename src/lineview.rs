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
        self.talk_down = Some(now);
        self.brain_touch(crate::brain::Touch::Typing);
        if let Some(l) = &self.line {
            let (view, handle) = (l.view.clone(), l.handle);
            // Deferred: closing calls back into this view.
            cx.defer(move |cx| {
                handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.summon(window, cx))).ok();
            });
            return "ok line summoned".into();
        }
        self.open_line(cx)
    }

    /// `talk release`: a hold is voice's hook (not built yet: it typed, as a tap).
    pub(super) fn talk_release(&mut self) -> String {
        let Some(t0) = self.talk_down.take() else { return "ok (no press)".into() };
        let ms = t0.elapsed().as_millis();
        if ms >= TAP_MS {
            elog!("synaps-avatar: talk held {ms} ms: push-to-talk isn't built yet; the Line opened for typing");
            format!("ok hold {ms} ms")
        } else {
            format!("ok tap {ms} ms")
        }
    }

    /// The Line's place (device px): bottom centre of the work area of the monitor he works
    /// on (his focused window's, else the pointer's).
    fn line_place(&self, h: i32) -> (i32, i32, i32, i32, Monitor) {
        let sf = self.layout.sf;
        let ours = self.x11.win;
        let at = self
            .x11
            .active_window()
            .or_else(|| self.x11.focused_window().ok())
            .filter(|&w| w > 1 && w != ours && self.line.as_ref().is_none_or(|l| l.xid != w))
            .and_then(|w| self.x11.toplevel_rect(w))
            .map(|r| r.center())
            .or_else(|| self.x11.pointer().ok())
            .unwrap_or_else(|| {
                let g = self.ground();
                (g.0, g.1 - 1)
            });
        let area = self.x11.area_at(at.0, at.1);
        let gap = (18.0 * sf) as i32;
        let w = ((LINE_W * sf) as i32).min(area.w - 2 * gap).max(1);
        let x = area.x + (area.w - w) / 2;
        let bottom = area.y + area.h - gap;
        let h = h.min(area.h - 2 * gap).max(1);
        (x, bottom, w, h, area)
    }

    pub(super) fn open_line(&mut self, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        let sf = self.layout.sf;
        let h0 = (LINE_H0 * sf) as i32;
        let (x, bottom, w, h, area) = self.line_place(h0);
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
        self.line = Some(LineWin { view, handle: handle.into(), xid, prev, at: (x, bottom, w, h), parked: false });
        self.x11.clicks.store(true, std::sync::atomic::Ordering::Relaxed);
        self.line_opened_ms = Some(now.elapsed().as_millis());
        // He is reading the thread now: nothing is unread, and the bubble gives way.
        self.ack_unread();
        if let Some(b) = &mut self.bubble {
            b.collapsed = true;
            b.hide_at = None;
        }
        self.line_fetch_her(area, x, w, now);
        elog!("synaps-avatar: line open at {x},{} {w}x{h} (0x{xid:x}) in {} ms", bottom - h, now.elapsed().as_millis());
        cx.notify();
        self.schedule(cx);
        format!("ok line {x},{} {w}x{h}", bottom - h)
    }

    /// She comes to the Line if she is on another monitor (the plain red teleport), to the
    /// floor beside it; on his monitor already, she stays.
    fn line_fetch_her(&mut self, area: Monitor, lx: i32, lw: i32, now: Instant) {
        let (gx, gy) = self.ground();
        let here = self.x11.monitor_at(gx, gy - 1);
        let there = self.x11.monitor_at(area.x + area.w / 2, area.y + area.h / 2);
        if (here.x, here.y) == (there.x, there.y) || self.rag.is_some() || self.hidden || !self.loaded {
            return;
        }
        let l = self.layout;
        let half = l.geo.width as i32 / 2;
        let gap = (40.0 * l.sf) as i32;
        let left = lx - gap - half;
        let right = lx + lw + gap + half;
        let x = if left - half >= area.x { left } else if right + half <= area.x + area.w { right } else { area.x + half };
        let floor = area.y + area.h;
        let o = Self::clamp(&self.x11, &self.layout, (x - l.ground_x, floor - l.ground_y));
        self.brain_cancel_move();
        self.start_teleport(o, Placement::Ground, now);
        self.home = (o.0 + l.ground_x, o.1 + l.ground_y);
        elog!("synaps-avatar: line on another monitor: she comes over to {},{}", self.home.0, self.home.1);
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
                    None => Row { who: Who::You, text: it.text.trim().to_string(), detail: String::new(), streaming: false },
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
        Snap { rows, status, busy: s.link.busy(), live: b.is_some_and(|b| b.live), prompts: b.map(|b| b.prompts.clone()).unwrap_or_default() }
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
                self.x11.clicks.store(false, std::sync::atomic::Ordering::Relaxed);
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
                self.x11.clicks.store(!p && self.line.is_some(), std::sync::atomic::Ordering::Relaxed);
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
        self.x11.clicks.store(false, std::sync::atomic::Ordering::Relaxed);
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
