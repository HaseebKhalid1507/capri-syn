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
