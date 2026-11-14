//! The restore chip: while she is hidden (HIDE in her menu), a small tag at the edge of her
//! monitor, her name and a dot. A click brings her back.

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
            .bg(rgb(pal.surface))
            .border_1()
            .border_color(rgb(pal.frame))
            .font_family(self.font.clone())
            .cursor_pointer()
            .hover(|s| s.border_color(rgb(pal.accent)))
            .child(div().size(px(7.0)).rounded_full().bg(rgb(pal.name)))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(pal.text))
                    .child(self.name.to_uppercase()),
            )
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
