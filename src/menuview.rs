//! Her menu from its one model (`crate::menu`), on both surfaces: the rows of her right-click
//! menu (framed and placed in view.rs), her tray icon's tooltip and menu (`Os::tray_show`), and
//! the one dispatch a pick on either goes through (`menu_do`). A child module of `view`.

use super::*;
use crate::menu::{self, Via};
use crate::settings::Attention;

impl Avatar {
    /// What her menu's rows depend on, now.
    pub(super) fn menu_state(&self) -> menu::State {
        menu::State {
            hidden: self.parked.is_some(),
            dnd: self.brain.is_some().then(|| self.dnd_on()),
            attention: self.attention_label().and_then(Attention::parse),
            handsfree: (self.voice_on() && self.voice.cfg.handsfree).then_some(self.voice.hf.is_some()),
            quit_armed: self.quit_armed.is_some(),
        }
    }

    /// Her tray icon's tooltip (`capri-syn · ONLINE`) and menu, from her status and her menu's
    /// model, every schedule: the desktop hears only a change (`Os::tray_show`). Nothing where
    /// she has no tray.
    pub(super) fn tray_sync(&self) {
        if !self.os.has_tray() {
            return;
        }
        // The tray's QUIT quits: it never asks twice there.
        let s = menu::State { quit_armed: false, ..self.menu_state() };
        self.os.tray_show(&crate::brand::tooltip(&self.status_now().label), menu::tray_items(&menu::rows(&s)));
    }

    /// A pick on her menu or her tray's: the one place each item acts.
    pub(super) fn menu_do(&mut self, item: menu::Item, via: Via, cx: &mut Context<Self>) -> String {
        let now = Instant::now();
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
            menu::Item::Attention(to) => {
                // Her menu's row: a step. A choice in the tray's: as many of the same steps.
                let from = self.attention_label().and_then(Attention::parse);
                let steps = match (from, to) {
                    (Some(a), Some(b)) => menu::attention_steps(a, b),
                    (Some(_), None) => 1,
                    (None, _) => 0,
                };
                for _ in 0..steps {
                    self.cycle_attention();
                }
                if via == Via::Her
                    && let Some(m) = &mut self.menu
                {
                    m.1 = now + Duration::from_secs(8);
                }
                format!("ok attention {}", self.attention_label().unwrap_or("none"))
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
            menu::Item::Settings => {
                self.menu = None;
                self.open_settings(cx)
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
        elog!("synaps-avatar: menu ({}): {item:?}: {r}", if via == Via::Her { "hers" } else { "tray" });
        cx.notify();
        self.schedule(cx);
        r
    }

    /// SETTINGS: settings.toml in his editor (the system's app for it; its folder until there is
    /// one). Her Settings page takes this over when it lands.
    fn open_settings(&mut self, cx: &mut Context<Self>) -> String {
        let p = crate::settings::Settings::path();
        let at = if p.is_file() {
            p
        } else {
            let Some(dir) = p.parent().map(|d| d.to_path_buf()) else { return "error: no settings folder".into() };
            std::fs::create_dir_all(&dir).ok();
            dir
        };
        cx.open_with_system(&at);
        format!("ok opened {}", at.display())
    }

    /// Her right-click menu's rows, the model's: red items, the hovered one on a red blade with
    /// ink type, an armed QUIT on a bone blade (danger). A row with choices cycles them (▸). Each
    /// is in `state`'s buttons as `menu-<key>`.
    pub(super) fn menu_rows(&self, pal: Pal, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (accent_c, ink_c, danger_c) = (pal.accent, pal.ink, pal.danger);
        let blade = |c: u32| div().absolute().top_0().left_0().size_full().child(theme::backdrop(Cut::Tab(8.0), Fill::solid(c)));
        let mut out = Vec::new();
        for row in menu::rows(&self.menu_state()) {
            if row.rule {
                out.push(div().h(px(1.0)).mx(px(10.0)).my(px(3.0)).bg(rgb(pal.rule)).into_any_element());
            }
            let label = if row.choices.is_empty() { row.label.to_string() } else { format!("{} ▸", row.label) };
            let (armed, item) = (row.armed, row.item);
            let group = SharedString::from(format!("menu-{label}"));
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
                    .text_color(rgb(if armed { ink_c } else { accent_c }))
                    .when(armed, |d| d.child(blade(danger_c)))
                    .when(!armed, |d| d.hover(|s| s.text_color(rgb(ink_c))).child(blade(accent_c).invisible().group_hover(group, |s| s.visible())))
                    .child(tracked(&label, 1.2))
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
