//! The restore chip: while she is hidden (HIDE in her menu), a small tag at the edge of her
//! monitor, her name and a dot. A click brings her back.

use crate::theme::{self, Cut, Fill, tracked};
use gpui_kit::*;

pub type OnClick = Box<dyn FnOnce(&mut App)>;

pub struct Chip {
    name: SharedString,
    font: SharedString,
    pal: crate::reader::Pal,
    on_click: Option<OnClick>,
}

impl Chip {
    pub fn new(name: SharedString, font: SharedString, pal: crate::reader::Pal, on_click: OnClick) -> Self {
        Chip { name, font, pal, on_click: Some(on_click) }
    }
}

/// Logical size of the chip window.
pub const SIZE: (f32, f32) = (132.0, 28.0);

impl Render for Chip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal;
        div()
            .id("restore-chip")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(7.0))
            // A slanted plate in the dark ring (its window's corners), a red diamond, her name.
            .relative()
            .bg(rgb(pal.ring))
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        let (x0, y0, x1, y1) = (f32::from(b.left()) + 1.0, f32::from(b.top()) + 1.0, f32::from(b.right()) - 1.0, f32::from(b.bottom()) - 1.0);
                        theme::paint(window, &theme::outline(Cut::Slant(7.0), x0, y0, x1, y1), Fill::framed(pal.panel, pal.border, 2.0));
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .font_family(self.font.clone())
            .cursor_pointer()
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        let (cx, cy) = (f32::from(b.left()) + 4.5, f32::from(b.top()) + 4.5);
                        theme::fill_poly(window, &[(cx, cy - 4.5), (cx + 4.5, cy), (cx, cy + 4.5), (cx - 4.5, cy)], rgb(pal.accent));
                    },
                )
                .size(px(9.0)),
            )
            .child(tracked(&self.name.to_uppercase(), 1.4).text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(pal.text)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(f) = this.on_click.take() {
                        cx.defer(f);
                    }
                }),
            )
    }
}
