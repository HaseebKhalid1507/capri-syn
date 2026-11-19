//! Her menu from its one model (`crate::menu`), on both surfaces: the rows of her right-click
//! menu (framed and placed in view.rs), her tray icon's tooltip and menu (`Os::tray_tip`,
//! `Os::tray_menu`), and the one dispatch a pick on either goes through (`menu_do`). A child
//! module of `view`.

use super::*;
use crate::menu::{self, Via};
use std::hash::{Hash, Hasher};

impl Avatar {
    /// What her menu's rows depend on, now.
    pub(super) fn menu_state(&self) -> menu::State {
        menu::State {
            quitting: self.quitting,
            teleporting: self.teleport.is_some(),
            held: self.press.is_some() || self.rag.is_some(),
            intro: self.intro_playing(),
            hidden: self.parked.is_some(),
            dnd: self.brain.is_some().then(|| self.dnd_on()),
            attention: self.attention(),
            handsfree: (self.voice_on() && self.voice.cfg.handsfree).then_some(self.voice.hf.is_some()),
            quit_armed: self.quit_armed.is_some(),
            unsaved: self.menu_save.says(),
        }
    }

    /// Her tray icon's tooltip (`capri-syn · ONLINE`, `capri-syn · HIDDEN`; her tag's word,
    /// steady: `brand::tooltip`) and menu (her menu's model, made only when its state changed),
    /// every schedule: the desktop hears only a change (`Os::tray_tip`, `Os::tray_menu`), and her
    /// mic's meter moving her tag never makes the menu again. Nothing where she has no tray.
    pub(super) fn tray_sync(&self) {
        if !self.os.has_tray() {
            return;
        }
        // The tray's QUIT quits: it never asks twice there.
        let s = menu::State { quit_armed: false, ..self.menu_state() };
        self.os.tray_tip(&crate::brand::tooltip(&self.status_now().label, s.hidden));
        let mut h = std::hash::DefaultHasher::new();
        s.hash(&mut h);
        self.os.tray_menu(h.finish(), || menu::tray_items(&menu::rows(&s)));
    }

    /// A click on her tray icon: her summon key's tap (the Line), not while she quits
    /// (`menu::opens`).
    pub(super) fn tray_open(&mut self, cx: &mut Context<Self>) -> String {
        if !menu::opens(&self.menu_state()) {
            elog!("synaps-avatar: tray: a click while she quits: nothing");
            return "error: quitting".into();
        }
        self.talk_tap(cx)
    }

    /// A pick on her menu or her tray's: the one place each item acts. What can't act now
    /// (`menu::enabled`: greyed on both) does nothing, whatever asked: a tray menu opened a
    /// moment ago, a click on a greyed row.
    pub(super) fn menu_do(&mut self, item: menu::Item, via: Via, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
        let via_s = if via == Via::Her { "hers" } else { "tray" };
        if !menu::enabled(item, &self.menu_state()) {
            elog!("synaps-avatar: menu ({via_s}): {item:?}: not now");
            return format!("error: {} not now", item.key());
        }
        let r = match item {
            menu::Item::ShowHide if self.parked.is_some() => {
                self.unpark("menu", now, cx);
                "ok shown".to_string()
            }
            menu::Item::ShowHide => {
                self.park(now);
                "ok hidden".into()
            }
            menu::Item::Reset => {
                // Hidden: back, at home.
                self.unpark("menu", now, cx);
                self.reset_position(cx);
                "ok reset".into()
            }
            menu::Item::Dnd => {
                self.menu = None;
                self.toggle_dnd();
                format!("ok dnd {}", if self.dnd_on() { "on" } else { "off" })
            }
            menu::Item::Attention(pick) => {
                // Her menu's row: the next one, and her menu stays up. A choice in the tray's:
                // that one.
                let asked = self.set_attention(pick, cx);
                if via == Via::Her
                    && let Some(m) = &mut self.menu
                {
                    m.1 = now + Duration::from_secs(8);
                }
                format!("ok attention {}", asked.map_or("none", |a| a.label()))
            }
            menu::Item::HandsFree => {
                self.menu = None;
                if self.voice.hf.is_some() {
                    self.voice_hf_end("HANDS-FREE OFF", cx);
                    "ok hands-free off".into()
                } else {
                    self.voice_hf_start(cx)
                }
            }
            menu::Item::Copy => {
                self.menu = None;
                self.copy_last_reply(cx)
            }
            // Her Settings (the Line's second page).
            menu::Item::Settings => {
                self.menu = None;
                self.show_line_page(crate::line::Page::Settings, cx)
            }
            // Her own menu asks twice: a second click within 2 s quits, and it stays up that long.
            menu::Item::Quit if via == Via::Her && self.quit_armed.is_none_or(|t| now >= t) => {
                self.quit_armed = Some(now + Duration::from_secs(2));
                if let Some(m) = &mut self.menu {
                    m.1 = m.1.max(now + Duration::from_secs(3));
                }
                "ok armed (again to quit)".into()
            }
            menu::Item::Quit => {
                self.quit_armed = None;
                self.vanish_quit(now);
                "ok quitting".into()
            }
        };
        elog!("synaps-avatar: menu ({via_s}): {item:?}: {r}");
        cx.notify();
        self.schedule(cx);
        r
    }

    /// Her right-click menu's rows, the model's: red items, the hovered one on a red blade with
    /// ink type, an armed QUIT on a bone blade (danger), one that can't act now dim (no blade; a
    /// click on it does nothing). A row with choices cycles them (▸). Each is in `state`'s
    /// buttons as `menu-<key>`. `cant`: ATTENTION's second line (why its last click didn't
    /// stick; `false`: landed since this opening, kept unseen as room so the rows hold still).
    pub(super) fn menu_rows(&self, pal: Pal, cant: Option<(&'static str, bool)>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (accent_c, ink_c, danger_c, dim_c) = (pal.accent, pal.ink, pal.danger, pal.dim);
        let blade = |c: u32| div().absolute().top_0().left_0().size_full().child(theme::backdrop(Cut::Tab(8.0), Fill::solid(c)));
        let mut out = Vec::new();
        for row in menu::rows(&self.menu_state()) {
            if row.rule {
                out.push(div().h(px(1.0)).mx(px(10.0)).my(px(3.0)).bg(rgb(pal.rule)).into_any_element());
            }
            let label = if row.choices.is_empty() { row.label.to_string() } else { format!("{} ▸", row.label) };
            let (armed, item, live) = (row.armed, row.item, row.enabled);
            let group = SharedString::from(format!("menu-{label}"));
            let why = cant.filter(|_| matches!(item, menu::Item::Attention(_))).map(|(c, shown)| {
                div()
                    .when(!shown, |d| d.invisible())
                    .pt(px(2.0))
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(pal.text))
                    .group_hover(group.clone(), |s| s.text_color(rgb(ink_c)))
                    .child(c)
            });
            // In `state`'s buttons, by name (a shot's click on it).
            let probe = self.buttons.clone();
            out.push(
                div()
                    .group(group.clone())
                    .relative()
                    .px(px(10.0))
                    .py(px(5.0))
                    .text_size(px(11.0))
                    .font_weight(if armed { FontWeight::BOLD } else { FontWeight::SEMIBOLD })
                    .text_color(rgb(if armed { ink_c } else if live { accent_c } else { dim_c }))
                    .when(armed, |d| d.child(blade(danger_c)))
                    .when(!armed && live, |d| d.hover(|s| s.text_color(rgb(ink_c))).child(blade(accent_c).invisible().group_hover(group, |s| s.visible())))
                    .child(tracked(&label, 1.2))
                    .when_some(why, |d, why| d.flex().flex_col().child(why))
                    .child(canvas(move |b, _, _| probe.borrow_mut().push((item.probe(), b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
                    .id(item.key())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.menu_do(item, Via::Her, cx);
                        }),
                    )
                    .into_any_element(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::voiceview::{hf_label, level_bars, mic_label};
    use crate::brand::tooltip;
    use std::collections::BTreeSet;

    /// Her tag moves with her mic's clock and meter many times a second; her tray's tooltip is
    /// one word for the whole of it (a new one is a `NIM_MODIFY` on Windows).
    #[test]
    fn her_mic_meter_never_moves_the_tray() {
        let levels = [0.0, 0.25, 0.5, 0.75, 1.0];
        let tips = |labels: Vec<String>| labels.iter().map(|l| tooltip(l, false)).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        let hf = (0..200).flat_map(|s| levels.map(|l| hf_label(None, s, l))).collect();
        assert_eq!(tips(hf), ["capri-syn · MIC LIVE · HANDS-FREE"]);
        let ending = (0..10).map(|left| hf_label(Some(left), 0, 0.5)).collect();
        assert_eq!(tips(ending), ["capri-syn · MIC LIVE · HANDS-FREE · ENDING"]);
        let mic = (0..200).flat_map(|s| levels.map(|l| mic_label(s, l, s % 2 == 0))).collect();
        assert_eq!(tips(mic), ["capri-syn · MIC"]);
        let speaking = levels.map(|l| format!("SPEAKING {}", level_bars(l))).to_vec();
        assert_eq!(tips(speaking), ["capri-syn · SPEAKING"]);
    }
}
