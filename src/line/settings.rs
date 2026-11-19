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
        ("left" | "right" | "space" | "enter", SetRow::Wake) => Act::Pref(Pref::Wake(!p.wake)),
        ("enter", SetRow::About) => Act::Copy,
        _ => Act::Nothing,
    };
    (sel, act)
}

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

    /// The cog: a 28 px box, the gear in it. Idle a text-dim frame, red on hover; Settings up:
    /// a solid red plate, an ink gear.
    pub(super) fn cog_el(&self, cx: &Context<Line>) -> Stateful<Div> {
        let pal = self.pal;
        let on = self.nav.page == Page::Settings;
        let (turn, _) = self.cog_turn(Instant::now());
        let (fg, bg) = if on { (pal.ink, pal.accent) } else { (pal.dim, pal.field) };
        div()
            .id("cog")
            .group("cog")
            .relative()
            .flex_none()
            .w(px(28.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(on, |d| d.child(theme::backdrop(Cut::Corners(6.0), Fill::solid(pal.accent))))
            .when(!on, |d| {
                d.child(theme::backdrop(Cut::Corners(6.0), Fill::framed(pal.field, pal.dim, 1.0)))
                    .child(div().absolute().top_0().left_0().size_full().invisible().group_hover("cog", |s| s.visible()).child(theme::backdrop(Cut::Corners(6.0), Fill::framed(pal.field, pal.accent, 1.0))))
            })
            .child(gear(fg, bg, 14.0, turn))
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
        self.swap = Some(Instant::now());
        if self.nav.page == Page::Settings {
            self.sel = 0;
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
                let prefs = self.snap.prefs.clone().unwrap_or_default();
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

    /// The Settings page: her line, the rows by section, the bar back to chat.
    pub(super) fn settings_el(&self, cx: &mut Context<Line>) -> Vec<AnyElement> {
        let pal = self.pal;
        let p = self.snap.prefs.clone().unwrap_or_default();
        let rows = set_rows(&p);
        let sel = self.sel.min(rows.len() - 1);
        let mut out: Vec<AnyElement> = Vec::new();
        // Her one line: her face, what she says.
        if let Some(says) = &p.says {
            out.push(
                div()
                    .mx(px(PAD))
                    .mt(px(10.0))
                    .px(px(10.0))
                    .py(px(5.0))
                    .border_l_3()
                    .border_color(rgb(pal.accent))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .children(self.snap.face.as_ref().map(|f| img(ImageSource::Render(f.0.clone())).w(px(22.0)).h(px(22.0)).flex_none()))
                    .child(div().text_size(px(14.5)).text_color(rgb(pal.text)).child(format!("\u{201c}{says}\u{201d}")))
                    .into_any_element(),
            );
        }
        let section = |name: &str| {
            div()
                .px(px(PAD))
                .pt(px(12.0))
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
        // A choice: its segments, the one in force on a solid red plate (ink type).
        let seg = |label: &'static str, on: bool, live: bool, id: SharedString| {
            let c = if on { pal.ink } else if live { pal.text } else { pal.dim };
            div()
                .id(id)
                .relative()
                .h(px(22.0))
                .px(px(9.0))
                .flex()
                .items_center()
                .text_size(px(11.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(c))
                .when(on, |d| d.child(theme::backdrop(Cut::Corners(4.0), Fill::solid(if live { pal.accent } else { pal.dim }))))
                .child(tracked(label, 1.2))
        };
        let label = |name: &str, on: bool, sub: Option<&str>| {
            // The row picked: its label on the menu's red blade.
            let plate = div()
                .relative()
                .h(px(22.0))
                .pl(px(8.0))
                .pr(px(12.0))
                .flex()
                .items_center()
                .text_size(px(12.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(if on { pal.ink } else { pal.text }))
                .when(on, |d| d.child(theme::backdrop(Cut::Tab(8.0), Fill::solid(pal.accent))))
                .child(tracked(name, 1.4));
            div()
                .w(px(170.0))
                .flex_none()
                .flex()
                .flex_col()
                .child(plate)
                .children(sub.map(|s| div().pl(px(8.0)).text_size(px(11.0)).text_color(rgb(pal.dim)).child(s.to_string())))
        };
        let row = |ix: usize| {
            div()
                .id(SharedString::from(format!("set-row-{ix}")))
                .px(px(PAD - 6.0))
                .py(px(4.0))
                .min_h(px(32.0))
                .flex()
                .items_center()
                .gap(px(8.0))
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
                    out.push(row(ix).child(label("ATTENTION", on, None)).child(div().flex_1().flex().justify_end().gap(px(4.0)).children(segs)).into_any_element());
                }
                SetRow::Wake => {
                    out.push(section("VOICE"));
                    let sub = (!p.voice).then_some("voice is off in settings.toml");
                    let segs = [("ON", true), ("OFF", false)].map(|(l, v)| {
                        seg(l, p.wake == v, p.voice, SharedString::from(format!("set-wake-{l}"))).cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.sel = ix;
                                if this.snap.prefs.as_ref().is_some_and(|p| p.wake != v) {
                                    this.settings_act(Act::Pref(Pref::Wake(v)), cx);
                                }
                                cx.notify();
                            }),
                        )
                    });
                    out.push(row(ix).child(label("\u{201c}HEY CAPRI\u{201d}", on, sub)).child(div().flex_1().flex().justify_end().gap(px(4.0)).children(segs)).into_any_element());
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
                        row(ix)
                            .child(div().flex_1().min_w(px(0.0)).pl(px(8.0)).text_size(px(12.5)).text_color(rgb(if on { pal.text } else { pal.dim })).overflow_hidden().whitespace_nowrap().child(p.about.clone()))
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
        // The bar where the field is: the cog (on), back to chat, and what came meanwhile.
        out.push(
            div()
                .mt(px(10.0))
                .px(px(PAD))
                .pt(px(10.0))
                .border_t_1()
                .border_color(rgb(pal.rule))
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(self.cog_el(cx))
                .child(
                    div()
                        .id("set-chat")
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(pal.text))
                        .child(tracked("‹ CHAT", 1.4))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.show(Page::Chat, window, cx);
                            }),
                        ),
                )
                .when(p.new > 0, |d| {
                    d.child(
                        theme::plate(Cut::Swallow(4.0), Fill::solid(pal.accent))
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
    use super::{Act, Nav, Page, Pref, Prefs, SetRow, list_key, set_rows};
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
    }
}
