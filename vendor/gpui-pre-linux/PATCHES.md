# gpui-pre-linux 0.3.12, patched for synaps-avatar

Vendored from crates.io (Apache-2.0, LICENSE-APACHE) with one change, in
`src/linux/x11/window.rs`, `X11WindowStatePtr::set_bounds` (every ConfigureNotify):

- a pure move (same size) no longer calls the `resize` or `moved` callbacks: both end in
  `Window::bounds_changed` -> `refresh()`, a full render and present per XMoveWindow
  (`moved` has no other use on X11: `on_display_changed` is a no-op there; the platform
  bounds are updated as before, so `Window::bounds()` stays current);
- and no longer queries the drawable size (`query_render_extent`, a synchronous GetGeometry
  round trip on the main thread).

A resize behaves as before. Measured on bella
(release, 4 s XTEST drag): see the PR. Drop this patch if upstream GPUI stops repainting on moves.

## Wayland layer-shell control (PL-1..PL-4, BRIEF-wayland §1.7), for the Hyprland backend

`src/linux/wayland/window.rs`, module `layer_ctl` (re-exported as `gpui_linux::layer_ctl`), plus
four lines in `WaylandWindowStatePtr::frame`/`draw`, `WaylandWindow::new`/`drop` and
`set_input_region`. Stock GPUI sets a layer surface's margins, anchor and keyboard
interactivity once, at creation, and commits every input-region change on its own.

- **The mechanism (one, shared):** layer windows register by `wl_surface` pointer (what
  `RawWindowHandle::Wayland` carries; removed on drop). `frame()` marks the window `in_frame`
  around GPUI's paint: state set then is committed by that frame's present, with the pixels
  it belongs to. State set outside a frame is committed on the next loop turn, unless a frame
  is scheduled by then (its present carries it; if GPUI skips the draw, the end of the frame
  commits it).
- **PL-1** `set_margin(surface, [top, right, bottom, left], Commit)`, `set_anchor(...)`: she
  moves by margins; `Commit::WithFrame` keeps a move and the frame drawn for it in one commit.
- **PL-2** `set_keyboard_interactivity(surface, k)`: committed now (the Line's keyboard).
- **PL-3** stock `Window::set_input_region` no longer commits inside a frame (the region lands
  with its pixels, not a frame early); outside a frame it commits as before.
  `layer_ctl::set_input_region` is the same with `Commit::WithFrame`.
- **PL-4** `set_layer(surface, Layer)` (v2+): committed now (`[brain] fullscreen = "stay"`, later).
- `commit_pending(surface)`: commit and flush now (tests drive the stage with the loop blocked).

Drop these if upstream GPUI grows runtime layer-surface setters.
