//! Her body's hosts where a surface shows on one output only (`Caps::host_per_output`): one
//! window per output, each a `HostView` showing her (the `Avatar` entity) while she is on its
//! output, empty otherwise. The core picks the host (`Os::move_to`); the view flips them
//! (`Avatar::hosts_sync`). Dormant hosts take no input and draw nothing after their last frame.

use crate::platform::{Monitor, Os, Role, WinId};
use crate::view::Avatar;
use gpui_kit::*;
use std::rc::Rc;

pub struct HostView {
    pub avatar: Entity<Avatar>,
    pub active: bool,
}

impl Render for HostView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.active.then(|| self.avatar.clone()))
    }
}

/// GPUI's display holding the centre of `bounds` (GPUI px): where a window with them opens.
pub fn display_for(cx: &App, bounds: Bounds<Pixels>) -> Option<DisplayId> {
    let c = bounds.center();
    cx.displays().into_iter().find(|d| d.bounds().contains(&c)).map(|d| d.id())
}

/// A host's id, view and window (`Avatar::add_host`).
pub type Host = (WinId, WeakEntity<HostView>, AnyWindowHandle);

/// Open a dormant host on monitor `m` (her window's size, at the monitor's origin).
pub fn open_dormant(cx: &mut App, os: &Rc<Os>, avatar: &Entity<Avatar>, m: &Monitor, size: (i32, i32), sf: f32) -> Option<Host> {
    let bounds = Bounds::new(point(px(m.x as f32 / sf), px(m.y as f32 / sf)), gpui_kit::size(px(size.0 as f32 / sf), px(size.1 as f32 / sf)));
    let mut options = os.window_options(Role::Body, bounds, false);
    options.display_id = display_for(cx, bounds);
    let mut id = 0;
    let mut view = None;
    let av = avatar.clone();
    let opened = cx.open_window(options, |window, cx| {
        id = os.adopt(Role::Body, window).unwrap_or(0);
        let v = cx.new(|_| HostView { avatar: av, active: false });
        view = Some(v.downgrade());
        v
    });
    match (opened, view) {
        (Ok(h), Some(v)) if id != 0 => Some((id, v, h.into())),
        (Err(e), _) => {
            elog!("synaps-avatar: a body host on {m:?}: {e:#}");
            None
        }
        _ => None,
    }
}
