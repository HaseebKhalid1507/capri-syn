//! Her body's hosts where a surface shows on one output only (`Caps::host_per_output`): one
//! window per output, each a `HostView` showing her (the `Avatar` entity) while she is on its
//! output, empty otherwise. The core picks the host (`Os::move_to`); the view flips them
//! (`Avatar::hosts_sync`). Dormant hosts take no input and draw nothing after their last frame.

mod startup;

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

/// Production host options: Hyprland resolves the wl_output by name, not GPUI bounds.
/// Reused by startup, dormant/effect hosts and the compositor contract.
pub fn options(cx: &App, os: &Os, role: Role, bounds: Bounds<Pixels>) -> anyhow::Result<WindowOptions> {
    os.panel_options(role, bounds, false, cx)
}

/// wl_output.done/name can lag hyprctl at startup. Yield to GPUI's event loop until the
/// named output is complete; never block dispatch or silently bind to a different output.
/// Keep the startup closure (pack/listener included) until it can run exactly once.
/// A removed/unadvertised output must fail startup, not leave an unserved control socket.
pub fn when_ready(cx: &mut App, os: Rc<Os>, bounds: Bounds<Pixels>, start: impl FnOnce(WindowOptions, &mut App) + 'static) {
    let began = std::time::Instant::now();
    match options(cx, &os, Role::Body, bounds) {
        Ok(options) => start(options, cx),
        Err(mut error) => {
            elog!("synaps-avatar: waiting for the body output's wl_output name: {error:#}");
            cx.spawn(async move |cx| {
                loop {
                    let elapsed = began.elapsed();
                    let Some(delay) = startup::retry_after(elapsed) else {
                        // fatal logs the last lookup (wanted name + GPUI's displays), removes
                        // the bound socket and exits; the event watcher has not started yet.
                        crate::fatal(crate::startfail::Why::Other, error.context(format!("body output unavailable after {} ms", elapsed.as_millis())));
                    };
                    cx.background_executor().timer(delay).await;
                    match cx.update(|cx| options(cx, &os, Role::Body, bounds)) {
                        Ok(options) => {
                            elog!("synaps-avatar: body output ready after {} ms", began.elapsed().as_millis());
                            cx.update(|cx| start(options, cx));
                            break;
                        }
                        Err(next) => error = next,
                    }
                }
            }).detach();
        }
    }
}

/// A host's id, view and window (`Avatar::add_host`).
pub type Host = (WinId, WeakEntity<HostView>, AnyWindowHandle);

/// Open a dormant host on monitor `m` (her window's size, at the monitor's origin). None
/// when GPUI doesn't know that output yet (a new one: try again later).
pub fn open_dormant(cx: &mut App, os: &Rc<Os>, avatar: &Entity<Avatar>, m: &Monitor, size: (i32, i32), sf: f32) -> Option<Host> {
    let bounds = Bounds::new(point(px(m.x as f32 / sf), px(m.y as f32 / sf)), gpui_kit::size(px(size.0 as f32 / sf), px(size.1 as f32 / sf)));
    let options = options(cx, os, Role::Body, bounds).ok()?;
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

/// An output's effect host: the effect layers reaching outside her window (`Os::fx_staged`),
/// clipped to its output. Dormant at 1×1 (nothing to show); woken to the layers' box.
pub struct FxHostView {
    pub os: Rc<Os>,
    /// Its top-left, root px (None: dormant).
    pub at: Option<(i32, i32)>,
    pub sf: f32,
}

impl Render for FxHostView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (os, at, sf) = (self.os.clone(), self.at, self.sf);
        div().size_full().child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    if let Some(at) = at {
                        os.paint_fx_staged(window, at, sf);
                    }
                },
            )
            .size_full(),
        )
    }
}

/// An effect host: its id, view, window, monitor, and the box it covers when awake.
pub struct FxHost {
    id: WinId,
    view: WeakEntity<FxHostView>,
    window: AnyWindowHandle,
    mon: Monitor,
    awake: Option<crate::platform::WinRect>,
}

/// Close effect hosts (the monitors changed: they are opened again).
pub fn close_fx_hosts(cx: &mut App, hosts: Vec<FxHost>) {
    for h in hosts {
        h.window.update(cx, |_, window, _| window.remove_window()).ok();
    }
}

/// One effect host per monitor (`Caps::host_per_output`), dormant. Open them after her body
/// hosts: they stack above her.
pub fn open_fx_hosts(cx: &mut App, os: &Rc<Os>, sf: f32) -> Vec<FxHost> {
    if !os.host_per_output() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for m in os.monitors() {
        let bounds = Bounds::new(point(px(m.x as f32 / sf), px(m.y as f32 / sf)), gpui_kit::size(px(1.0), px(1.0)));
        let Ok(options) = options(cx, os, Role::Fx, bounds) else { continue };
        let (mut id, mut view) = (0, None);
        let os2 = os.clone();
        if let Ok(w) = cx.open_window(options, |window, cx| {
            id = os2.adopt(Role::Fx, window).unwrap_or(0);
            let v = cx.new(|_| FxHostView { os: os2.clone(), at: None, sf });
            view = Some(v.downgrade());
            v
        }) && let Some(view) = view
        {
            out.push(FxHost { id, view, window: w.into(), mon: m, awake: None });
        }
    }
    out
}

/// What the effect hosts show changed: each wakes to the part of it on its monitor (growing
/// only while awake: a resize is a configure round trip), or goes dormant.
pub fn fx_hosts_sync(cx: &mut App, os: &Os, hosts: &mut [FxHost]) {
    if hosts.is_empty() || !os.fx_take_staged() {
        return;
    }
    let bbox = os.fx_staged_box();
    for h in hosts.iter_mut() {
        let want = bbox.and_then(|b| b.intersect(&h.mon.rect()));
        let ext = |r: Option<crate::platform::WinRect>| r.map(|r| (r.x, r.y, r.w, r.h));
        let keep = matches!((want, h.awake), (Some(w), Some(a)) if a.x <= w.x && a.y <= w.y && a.right() >= w.right() && a.bottom() >= w.bottom());
        let sf = h.view.update(cx, |v, _| v.sf).unwrap_or(1.0);
        if !keep && ext(want) != ext(h.awake) {
            let r = want.unwrap_or(crate::platform::WinRect::new(h.mon.x, h.mon.y, 1, 1));
            h.window.update(cx, |_, window, _| window.resize(gpui_kit::size(px(r.w as f32 / sf), px(r.h as f32 / sf)))).ok();
            os.place(h.id, r.x, r.y, r.w, r.h).ok();
            h.awake = want;
        }
        let at = h.awake.map(|r| (r.x, r.y));
        h.view
            .update(cx, |v, cx| {
                v.at = at;
                cx.notify();
            })
            .ok();
    }
}
