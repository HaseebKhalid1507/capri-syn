# gpui-pre-windows 0.3.12, patched for synaps-avatar

Vendored from crates.io (Apache-2.0, LICENSE-APACHE), like `vendor/gpui-pre-linux`. The crate is
`#![cfg(target_os = "windows")]` at its root, so on Linux only this manifest is read (for the lock).

Every decision lives in `src/synaps_patches.rs` as a pure function. The call sites are one line
each, marked `synaps-avatar PW-n`. capri-syn's unit tests include that file by path
(`src/platform/windows/mod.rs`, `gpui_patches`) and check each decision without a display, in the
Windows CI job.

| patch | where | what | status |
|---|---|---|---|
| **PW-1** pop-up without a title bar | `window.rs`, `WindowsWindow::new` | `WindowKind::PopUp` is created `WS_POPUP` (stock: `WINDOW_STYLE(0)`, i.e. `WS_OVERLAPPED`, which Windows gives a caption: DWM then draws a frame, a title strip and a shadow around her whole rect, and the client area is inset by them). Ex-styles unchanged (`WS_EX_TOOLWINDOW \| WS_EX_TOPMOST`) | unit-tested; **visual check pending** (VM) |
| **PW-2** a click doesn't activate her | `window.rs` (`click_activates`, from `WindowParams`), `events.rs` `WM_MOUSEACTIVATE` | a pop-up opened with `focus: false` answers `MA_NOACTIVATE` (stock: always `MA_ACTIVATE`, so a click on her took the foreground and the keyboard from his window). Every other window as before: the Line (opened with focus) still activates on a click | unit-tested; **visual check pending** (VM) |
| **PW-3** a pure move doesn't re-render | `events.rs`, `handle_move_msg` (`WM_MOVE`) | the `moved` callback no longer runs: it ends in `Window::bounds_changed` → `refresh()`, a full render and present per `SetWindowPos` (measured in P0 [VM]: 85 renders for 131 moves, 3-5 % of a core on WARP). The origin and the display are still updated (`Window::bounds()` stays current); a size change still comes through `WM_SIZE` and re-renders. The X11 patch's twin | unit-tested; **visual check pending** (VM: no stale frame after a move, idle CPU while dragged) |

With PW-1 and PW-2 in GPUI, capri-syn's Windows backend no longer restyles her windows or
subclasses `WM_MOUSEACTIVATE`; it still adds the layered/click-through ex-styles and turns off
DWM's border and corner rounding (`src/platform/windows/mod.rs`, `adopt_hwnd`).

Drop a patch when upstream GPUI does the same: PW-1 if `PopUp` becomes `WS_POPUP`, PW-2 if
`focus: false` keeps a click from activating, PW-3 if moves stop repainting.
