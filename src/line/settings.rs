//! The Line's second page: her Settings (SPEC-settings §1–2, PR 1: the shell). The cog left of
//! the field (or `ctrl+,`) flips the Line between chat and Settings; Esc goes back one step.
//! Every value is a `settings.toml` key the avatar writes (`Settings::write`) and applies
//! (`settings_reload`): the Line only shows what she runs with ([`Prefs`]) and asks for a
//! change ([`Pref`]).

use super::{Ask, Line, Mode, PAD};
use crate::settings::Attention;
use crate::theme::{self, Cut, Fill, tracked};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::time::Instant;

/// The Line's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Chat,
    Settings,
}

impl Page {
    pub fn key(self) -> &'static str {
        match self {
            Page::Chat => "chat",
            Page::Settings => "settings",
        }
    }
}

/// The page shown and where Esc goes back to, one step at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    pub page: Page,
    back: Vec<Page>,
}

impl Nav {
    /// Opened on `page` (the avatar's SETTINGS row): Esc from there goes to chat.
    pub fn on(page: Page) -> Nav {
        Nav { page, back: Vec::new() }
    }

    /// To `p`, remembering where it came from. false: already there.
    pub fn go(&mut self, p: Page) -> bool {
        if p == self.page {
            return false;
        }
        self.back.push(self.page);
        self.page = p;
        true
    }

    /// The cog (`ctrl+,`): Settings from chat; from Settings straight back to chat.
    pub fn cog(&mut self) {
        if self.page == Page::Chat {
            self.go(Page::Settings);
        } else {
            self.back.clear();
            self.page = Page::Chat;
        }
    }

    /// Esc: one step back. false: nowhere back (chat), Esc does what it does there.
    pub fn esc(&mut self) -> bool {
        match self.back.pop() {
            Some(p) => {
                self.page = p;
                true
            }
            None if self.page != Page::Chat => {
                self.page = Page::Chat;
                true
            }
            None => false,
        }
    }
}

/// What the Settings page shows: the values she runs with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// `[brain] attention` (None: her brain is off: no row).
    pub attention: Option<Attention>,
    /// `[voice] wake_word`.
    pub wake: bool,
    /// `[voice] enabled`: "HEY CAPRI" is dim while it is off.
    pub voice: bool,
    /// `CAPRI 0.0.1 · SYNAPS 0.12.0 · sonnet-4-6`.
    pub about: String,
    /// Her one line: a change made, or why it wasn't.
    pub says: Option<String>,
    /// Replies that came while Settings was up (unseen: `N NEW`).
    pub new: u32,
}

/// A change he asks for on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    Attention(Attention),
    Wake(bool),
}

/// The page's rows, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetRow {
    Attention,
    Wake,
    About,
}

pub fn set_rows(p: &Prefs) -> Vec<SetRow> {
    let mut v = Vec::new();
    if p.attention.is_some() {
        v.push(SetRow::Attention);
    }
    v.push(SetRow::Wake);
    v.push(SetRow::About);
    v
}

const ATTENTION: [Attention; 3] = [Attention::Mix, Attention::HangOnly, Attention::Off];

/// What a key on the list does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Nothing,
    Pref(Pref),
    /// ABOUT's COPY: the versions, for a bug report.
    Copy,
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
        ("left", SetRow::Attention) => p.attention.map_or(Act::Nothing, |a| Act::Pref(Pref::Attention(step(a, -1)))),
        ("right" | "space" | "enter", SetRow::Attention) => p.attention.map_or(Act::Nothing, |a| Act::Pref(Pref::Attention(step(a, 1)))),
        // Locked while voice is off ([voice] enabled is his, in the file).
        ("left" | "right" | "space" | "enter", SetRow::Wake) if p.voice => Act::Pref(Pref::Wake(!p.wake)),
        ("enter", SetRow::About) => Act::Copy,
        _ => Act::Nothing,
    };
    (sel, act)
}

/// The footer's keys for the row picked (⏎ copy only where it copies).
pub fn hint(p: &Prefs, sel: usize) -> &'static str {
    let rows = set_rows(p);
    match rows[sel.min(rows.len() - 1)] {
        SetRow::Attention => "↑↓ pick · ←→ change · esc chat",
        SetRow::Wake if p.voice => "↑↓ pick · ←→ change · esc chat",
        SetRow::Wake => "↑↓ pick · esc chat",
        SetRow::About => "↑↓ pick · ⏎ copy · esc chat",
    }
}

/// A held ALLOW fires at HOLD only if it is still that hold on that prompt, and the prompt is on
/// screen: a page swap lets go of it (#74: nothing approved unseen).
pub fn hold_fires(hold_at: Option<Instant>, started: Instant, page: Page, same_prompt: bool) -> bool {
    hold_at == Some(started) && same_prompt && page == Page::Chat
}

/// The text field's height (one line) until it has been laid out: Settings' bar is as tall as
/// the field's, so the swap moves nothing under it.
pub(super) const FIELD_H0: f32 = 52.0;
const COG_W: f32 = 34.0;
/// Her line's slot on the Settings page.
const QUOTE_H: f32 = 30.0;

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

    /// The cog: a chamfered shard as tall as the field, a dim-red edge, red on hover. On
    /// Settings: a solid red plate with a back glyph (the way to chat).
    pub(super) fn cog_el(&self, cx: &Context<Line>) -> Stateful<Div> {
        let pal = self.pal;
        let on = self.nav.page == Page::Settings;
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
            .when(on, |d| d.child(theme::backdrop(cut, Fill::solid(pal.accent))).child(center().child(theme::glyph_at("back", pal.ink, pal.accent, 12.0))))
            .when(!on, |d| {
                d.child(theme::backdrop(cut, Fill::framed(pal.field, pal.border, 1.0)))
                    .child(center().child(gear(pal.dim, pal.field, 15.0, turn)))
                    .child(center().invisible().group_hover("cog", |s| s.visible()).child(theme::backdrop(cut, Fill::solid(pal.accent))).child(gear(pal.ink, pal.accent, 15.0, turn)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.nav.cog();
                    this.page_changed(window, cx);
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
        if self.nav.page == Page::Settings {
            self.sel = 0;
            if self.nav.back.last() == Some(&Page::Chat) {
                self.chat_h = self.height;
            }
        } else if !self.snap.prompts.is_empty() {
            // Back to a prompt he couldn't see: its buttons arm afresh.
            self.prompt_at = now;
        }
        self.refocus(window, cx);
        (self.on_ask)(Ask::Page(self.nav.page), cx).ok();
        cx.notify();
    }

    /// From the avatar (its SETTINGS row, `avatarctl settings open|close`).
    pub fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Line>) {
        let changed = match page {
            Page::Settings => self.nav.go(Page::Settings),
            Page::Chat if self.nav.page != Page::Chat => {
                self.nav = Nav::on(Page::Chat);
                true
            }
            Page::Chat => false,
        };
        if changed {
            self.page_changed(window, cx);
        }
        // Asked for from her menu (or avatarctl): it comes with the keyboard, as the summon
        // key's does (only a desktop that hands it over on his input leaves it waiting).
        if page == Page::Settings && (self.parked || !window.is_window_active()) {
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
                self.nav = Nav::on(Page::Chat);
                self.mode = Mode::Prompt;
                self.sync_secret(window, cx);
                self.page_changed(window, cx);
            }
            key if plain => {
                // Her values not in yet: nothing to change from (never a write from defaults).
                let Some(prefs) = self.snap.prefs.clone() else { return true };
                let (sel, act) = list_key(key, self.sel, &prefs);
                self.sel = sel;
                self.settings_act(act, cx);
                cx.notify();
            }
            _ => {}
        }
        true
    }

    fn settings_act(&mut self, act: Act, cx: &mut Context<Line>) {
        match act {
            Act::Pref(p) => {
                (self.on_ask)(Ask::Pref(p), cx).ok();
            }
            Act::Copy => {
                let about = self.snap.prefs.as_ref().map(|p| p.about.clone()).unwrap_or_default();
                (self.on_ask)(Ask::Copy(format!("{about} · {} · {}", std::env::consts::OS, std::env::consts::ARCH)), cx).ok();
                self.note = Some("COPIED".into());
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

    /// The Settings page: her line, the rows by section, the bar back to chat. Its height moves
    /// on no key: her line has its slot whether she said something or not.
    pub(super) fn settings_el(&self, cx: &mut Context<Line>) -> Vec<AnyElement> {
        let pal = self.pal;
        let p = self.snap.prefs.clone().unwrap_or_default();
        let rows = set_rows(&p);
        let sel = self.sel.min(rows.len() - 1);
        let mut out: Vec<AnyElement> = Vec::new();
        // Her one line (a change made, or why not); before any, what the page does. One line,
        // its slot always there.
        let said = p.says.is_some();
        let says = p.says.as_ref().map_or_else(|| "Changes are saved to settings.toml as you make them.".to_string(), |s| format!("\u{201c}{s}\u{201d}"));
        out.push(
            div()
                .mx(px(PAD))
                .mt(px(10.0))
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
        let section = |name: &str| {
            div()
                .px(px(PAD))
                .pt(px(10.0))
                .pb(px(2.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(10.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(pal.dim))
                .child(tracked(name, 1.6))
                .child(div().flex_1().h(px(1.0)).bg(rgb(pal.rule)))
                .into_any_element()
        };
        // A value in a group: the one in force on a solid red plate (ink type); locked, on a
        // dim one. Red fill means only that: the row picked is shown by its notch.
        let seg = |label: &'static str, on: bool, live: bool, id: SharedString| {
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
        let label = |name: &str, on: bool, sub: Option<&str>| {
            div()
                .w(px(170.0))
                .flex_none()
                .flex()
                .flex_col()
                .child(div().text_size(px(12.5)).font_weight(FontWeight::BOLD).text_color(rgb(if on { pal.text } else { theme::mix(pal.text, pal.dim, 0.55) })).child(tracked(name, 1.4)))
                .children(sub.map(|s| div().text_size(px(11.0)).text_color(rgb(pal.dim)).child(s.to_string())))
        };
        // A row: picked, a 3 px red notch on the panel's edge and a faint band across.
        let row = |ix: usize, on: bool| {
            div()
                .id(SharedString::from(format!("set-row-{ix}")))
                .relative()
                .px(px(PAD))
                .py(px(4.0))
                .min_h(px(34.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .when(on, |d| {
                    d.child(div().absolute().top_0().left_0().size_full().bg(theme::alpha(pal.accent, 0.09)))
                        .child(div().absolute().top_0().left_0().bottom_0().w(px(3.0)).bg(rgb(pal.accent)))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.sel = ix;
                        cx.notify();
                    }),
                )
        };
        for (ix, r) in rows.iter().enumerate() {
            let on = ix == sel;
            match r {
                SetRow::Attention => {
                    out.push(section("BEHAVIOUR"));
                    let cur = p.attention.unwrap_or_default();
                    let segs = ATTENTION.iter().map(|&a| {
                        seg(a.label(), a == cur, true, SharedString::from(format!("set-att-{}", a.key()))).cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.sel = ix;
                                if this.snap.prefs.as_ref().is_some_and(|p| p.attention != Some(a)) {
                                    this.settings_act(Act::Pref(Pref::Attention(a)), cx);
                                }
                                cx.notify();
                            }),
                        )
                    });
                    out.push(row(ix, on).child(label("ATTENTION", on, None)).child(div().flex_1()).child(track(true).children(segs)).into_any_element());
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
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.sel = ix;
                                    if this.snap.prefs.as_ref().is_some_and(|p| p.voice && p.wake != v) {
                                        this.settings_act(Act::Pref(Pref::Wake(v)), cx);
                                    }
                                    cx.notify();
                                }),
                            )
                        })
                    });
                    let lock = (!live).then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(11.0))
                            .text_color(rgb(pal.dim))
                            .child(theme::glyph_at("lock", pal.dim, pal.panel, 11.0))
                            .child("locked · settings.toml")
                    });
                    out.push(
                        row(ix, on)
                            .child(label("\u{201c}HEY CAPRI\u{201d}", on, (!live).then_some("voice is off")))
                            .child(div().flex_1())
                            .children(lock)
                            .child(track(live).children(segs))
                            .into_any_element(),
                    );
                }
                SetRow::About => {
                    out.push(section("ABOUT"));
                    let button = |id: &'static str, text: &'static str| {
                        theme::plate(Cut::Slant(6.0), Fill::line(pal.dim, 1.0))
                            .id(id)
                            .h(px(22.0))
                            .px(px(10.0))
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(pal.text))
                            .child(tracked(text, 1.2))
                    };
                    out.push(
                        row(ix, on)
                            .child(label("VERSION", on, None))
                            .child(div().flex_1().min_w(px(0.0)).text_size(px(12.5)).text_color(rgb(if on { pal.text } else { pal.dim })).overflow_hidden().whitespace_nowrap().child(p.about.clone()))
                            .child(button("set-github", "GITHUB ▸").on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.sel = ix;
                                    (this.on_ask)(Ask::OpenUrl(REPO.into()), cx).ok();
                                    cx.notify();
                                }),
                            ))
                            .child(button("set-copy", "COPY").on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.sel = ix;
                                    this.settings_act(Act::Copy, cx);
                                    cx.notify();
                                }),
                            ))
                            .into_any_element(),
                    );
                }
            }
        }
        // As tall as chat was (the content's min height): the bar stays where the field was.
        out.push(div().flex_1().min_h(px(6.0)).into_any_element());
        // The bar where the field is, as tall as the field's: the cog (back to chat), CHAT, and
        // what came meanwhile.
        out.push(
            div()
                .px(px(PAD))
                .pt(px(10.0))
                .border_t_1()
                .border_color(rgb(pal.rule))
                .flex()
                .h(px(10.0 + 1.0 + self.field_h.get()))
                .gap(px(8.0))
                .child(self.cog_el(cx))
                .child(
                    div()
                        .id("set-chat")
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(pal.text))
                        .child(tracked("CHAT", 1.4))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.show(Page::Chat, window, cx);
                            }),
                        ),
                )
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
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{Act, Nav, Page, Pref, Prefs, SetRow, hint, hold_fires, list_key, set_rows};
    use crate::settings::Attention;

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
        Prefs { attention: Some(Attention::Mix), wake: true, voice: true, about: "CAPRI 0.0.1".into(), says: None, new: 0 }
    }

    #[test]
    fn its_keys_move_change_and_copy() {
        let p = prefs();
        assert_eq!(set_rows(&p), vec![SetRow::Attention, SetRow::Wake, SetRow::About]);
        assert_eq!(list_key("down", 0, &p), (1, Act::Nothing));
        assert_eq!(list_key("down", 2, &p), (2, Act::Nothing));
        assert_eq!(list_key("up", 0, &p), (0, Act::Nothing));
        assert_eq!(list_key("right", 0, &p), (0, Act::Pref(Pref::Attention(Attention::HangOnly))));
        assert_eq!(list_key("left", 0, &p), (0, Act::Pref(Pref::Attention(Attention::Off))));
        assert_eq!(list_key("space", 1, &p), (1, Act::Pref(Pref::Wake(false))));
        assert_eq!(list_key("enter", 2, &p), (2, Act::Copy));
        assert_eq!(list_key("x", 1, &p), (1, Act::Nothing));
        // Her brain off: no ATTENTION row, the rest moves up.
        let q = Prefs { attention: None, ..prefs() };
        assert_eq!(set_rows(&q), vec![SetRow::Wake, SetRow::About]);
        assert_eq!(list_key("enter", 0, &q), (0, Act::Pref(Pref::Wake(false))));
        assert_eq!(list_key("down", 5, &q), (1, Act::Nothing));
        // Voice off: "HEY CAPRI" is locked to the file, no key changes it.
        let off = Prefs { voice: false, ..prefs() };
        for k in ["left", "right", "space", "enter"] {
            assert_eq!(list_key(k, 1, &off), (1, Act::Nothing), "{k}");
        }
    }

    #[test]
    fn the_hint_is_the_rows_own() {
        let p = prefs();
        assert_eq!(hint(&p, 0), "↑↓ pick · ←→ change · esc chat");
        assert_eq!(hint(&p, 1), "↑↓ pick · ←→ change · esc chat");
        assert_eq!(hint(&p, 2), "↑↓ pick · ⏎ copy · esc chat", "⏎ copy only where it copies");
        assert_eq!(hint(&Prefs { voice: false, ..prefs() }, 1), "↑↓ pick · esc chat", "locked: nothing to change");
        assert_eq!(hint(&p, 9), hint(&p, 2));
    }

    #[test]
    fn a_held_allow_never_fires_off_screen() {
        let t0 = std::time::Instant::now();
        // Held on chat, still held at HOLD, the same prompt: it allows.
        assert!(hold_fires(Some(t0), t0, Page::Chat, true));
        // He went to Settings meanwhile (the swap lets go: hold_at None), or it is still set
        // but the prompt is off screen: never allowed unseen.
        assert!(!hold_fires(None, t0, Page::Chat, true));
        assert!(!hold_fires(Some(t0), t0, Page::Settings, true));
        // A later hold, or another prompt in its place: not this one's.
        assert!(!hold_fires(Some(t0 + std::time::Duration::from_millis(5)), t0, Page::Chat, true));
        assert!(!hold_fires(Some(t0), t0, Page::Chat, false));
    }
}
