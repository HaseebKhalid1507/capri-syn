//! synaps-avatar's patches to this crate (PATCHES.md), each decision as a pure function. The
//! call sites in `window.rs` and `events.rs` are one line each; capri-syn's tests include this
//! file by path and check the decisions without a display. Only `::windows` and `core` here.

use ::windows::Win32::UI::WindowsAndMessaging::{
    MA_ACTIVATE, MA_NOACTIVATE, WINDOW_EX_STYLE, WINDOW_STYLE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// PW-1: a pop-up's styles. Stock GPUI gives `WindowKind::PopUp` `WINDOW_STYLE(0)`, which is
/// `WS_OVERLAPPED`: Windows adds a caption to it, so DWM draws a frame, a title strip and a
/// shadow around the whole rect (and the client area is inset). A bare `WS_POPUP` has no
/// non-client area at all: the window is its client area.
pub(crate) fn popup_styles() -> (WINDOW_EX_STYLE, WINDOW_STYLE) {
    (WS_EX_TOOLWINDOW | WS_EX_TOPMOST, WS_POPUP)
}

/// PW-2: whether a click activates the window (the reply to `WM_MOUSEACTIVATE`). Stock GPUI
/// always activates; a pop-up opened without focus (`WindowOptions::focus` false) never takes
/// it from a click: the window he works in keeps the keyboard. Any other window as before.
pub(crate) fn click_activates(popup: bool, opened_with_focus: bool) -> bool {
    !popup || opened_with_focus
}

/// The `WM_MOUSEACTIVATE` reply for `click_activates`.
pub(crate) fn mouse_activate_reply(activates: bool) -> isize {
    (if activates { MA_ACTIVATE } else { MA_NOACTIVATE }) as isize
}

/// PW-3: whether `WM_MOVE` (a pure move: a size change comes as `WM_SIZE`, which still
/// re-renders) runs the `moved` callback. Stock GPUI does, and GPUI's `Window::bounds_changed`
/// then re-renders and presents the whole window for every `SetWindowPos`. The platform's
/// origin and display are still updated, so `Window::bounds()` stays current.
pub(crate) fn move_runs_moved_callback() -> bool {
    false
}

/// PW-4: whether `activate()` (`Window::activate_window`) injects a lone Alt press and release
/// (`SendInput`) before `SetForegroundWindow`, to get past the foreground lock. Stock GPUI does:
/// that is synthetic input in his session (menus and key tips react to a lone Alt, a game may
/// get it). She never does: the desktop hands her the keyboard on his own input only.
pub(crate) fn activate_injects_input() -> bool {
    false
}

/// PW-5: whether a window keeps one scale: the one of the display it opens for (`display_id`,
/// else the primary monitor), on every monitor, and a `WM_DPICHANGED` neither rescales nor
/// resizes it. Stock GPUI follows each monitor's DPI. Her pop-ups are pixel art laid out at one
/// scale by capri-syn (its `sf` is the primary monitor's); a rescale would resize her window
/// under her layout. Every other window as before.
pub(crate) fn keeps_one_scale(popup: bool) -> bool {
    popup
}
