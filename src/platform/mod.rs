//! The platform layer: one core, thin backends.
//!
//! Every decision about the desktop lives once, in [`Os`] (`os.rs`): caching and freezing the
//! layout, the nearest monitor, which windows she may sit on, the compositor mode and the
//! shape it implies, following the perch target (one change-detection path for every
//! backend), the pointer watcher (near / swipe / hover) and the drag poller, whose keyboard to
//! hand back. A backend only translates: [`Desktop`] answers questions about the world outside
//! her in the plain types below and pushes cheap [`Hint`]s; [`Stage`] runs primitive commands
//! on her own surfaces. Nothing outside `src/platform/` names an OS, a display server or a
//! window system, and nothing inside the core does either: it reads [`Caps`].
//!
//! Backends: `x11/`, `hypr/` (Hyprland: its IPC sockets, and layer-shell surfaces through the
//! vendored GPUI's `layer_ctl`). Windows comes later. Each passes the same contract suite
//! (`contract.rs`: on the in-memory `fake.rs` in every `cargo test`, on Xvfb for X11, in the
//! `tools/wlrig` sandbox for Hyprland).

use anyhow::Result;
use gpui_kit::{Window, WindowOptions};

mod hypr;
mod linux;
mod os;
mod x11;

#[cfg(test)]
mod contract;
#[cfg(test)]
mod fake;

pub use crate::edges::WinRect;
pub use crate::shape::ShapeRect;
pub use os::Os;

/// The process-wide setup that must run before any other thread exists (GPU choice,
/// allocator). See `linux.rs`.
pub use linux::{monotonic_s, process_setup, trim_heap};

/// A window she may sit on is at least this big (and managed).
pub(crate) const SEAT_MIN: (i32, i32) = (120, 60);

/// A window. X11: the XID. Other backends: an interned handle, never reused in a session.
/// 0 is "none".
pub type WinId = u32;

/// A monitor (or its work area), root px.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Monitor {
    pub fn rect(&self) -> WinRect {
        WinRect::new(self.x, self.y, self.w, self.h)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    fn distance2(&self, x: i32, y: i32) -> i64 {
        let dx = (self.x - x).max(0).max(x - (self.x + self.w - 1)) as i64;
        let dy = (self.y - y).max(0).max(y - (self.y + self.h - 1)) as i64;
        dx * dx + dy * dy
    }
}

/// Another window, as a backend sees it: `Desktop::windows` lists the visible ones bottom →
/// top (her own surfaces left out).
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub rect: WinRect,
    /// Placed by the window manager (a client or its frame), not a popup, menu or tooltip.
    pub managed: bool,
}

/// What a backend can do; the core degrades on these, never on the backend's name.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    /// Her body can drop to a 1-bit visible shape when nothing composites (else: always
    /// alpha).
    pub shape_mode: bool,
    /// The backend hints every move, unmap and map of other windows. Without it the core
    /// polls the perch target and diffs the snapshots itself.
    pub target_events: bool,
    /// Effect layers are surfaces of their own (the server paints them). Without it the core
    /// keeps them in a display list and her view paints them in her window.
    pub fx_layers: bool,
}

/// A cheap note from a backend's event source: "something changed". The core turns these
/// into [`WatchEvent`]s (and re-reads what a hint leaves out).
#[derive(Clone, Copy, Debug)]
pub enum Hint {
    /// A window moved, resized or restacked: its new outer rect when the event carried it.
    Moved(WinId, Option<WinRect>),
    /// A window became visible.
    Mapped(WinId),
    /// A window was hidden or closed.
    Gone(WinId),
    /// The focused window changed.
    Focus,
    /// The workspace changed.
    Desktop,
    /// Whether a compositor runs now.
    Compositor(bool),
}

/// What the watchers report to the view.
#[derive(Clone, Copy, Debug)]
pub enum WatchEvent {
    Compositor(bool),
    /// The perch target moved or resized.
    TargetMoved(WinRect),
    /// The perch target was unmapped or destroyed.
    TargetGone(u32),
    /// The target window is mapped again (a workspace flicked back).
    TargetBack(u32),
    /// A click anywhere, at root (x, y), while the Line holds the keyboard.
    Click(i32, i32),
    /// He focused another window.
    Focus,
    /// He switched workspace.
    Desktop,
    /// The pointer near her, swept past her, or resting on her (`Os::watch_pointer`).
    Pointer(PointerEv),
}

/// What the pointer watcher reports (device px, px/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEv {
    Near,
    Swipe(f32),
    Hover,
}

/// Pointer watcher tuning (device px at sf 1, scaled by the caller's sf).
pub struct PointerCfg {
    pub near_px: f32,
    pub swipe_px: f32,
    pub swipe_speed: f32,
    pub hover: std::time::Duration,
}

/// Which of her windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Body,
    Reader,
    Chip,
    Line,
}

/// Where an effect layer stacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxZ {
    /// Above everything.
    Top,
    /// Directly below layer j (above it if j isn't up).
    Below(usize),
    /// Directly below her body.
    UnderBody,
}

/// Her body's visible region: all of it (a compositor blends the alpha), or these rects.
#[derive(Clone, Copy, Debug)]
pub enum Visible<'a> {
    All,
    Rects(&'a [ShapeRect]),
}

/// Reads the pointer (root px) on a connection of its own.
pub type PointerRead = Box<dyn FnMut() -> Option<(i32, i32)> + Send>;

/// Questions about the world outside her, and its event sources. Plain translation: every
/// rule built on these answers is the core's. Thread-safe: watchers call it.
pub trait Desktop: Send + Sync {
    fn caps(&self) -> Caps;
    /// The root's size, px (GPUI's displays are this divided by the scale factor).
    fn root_size(&self) -> (i32, i32);
    /// The monitors, fresh (the core caches).
    fn monitors(&self) -> Vec<Monitor>;
    /// Each of `mons` minus what panels and docks reserve, same order, fresh.
    fn work_areas(&self, mons: &[Monitor]) -> Vec<Monitor>;
    /// The refresh rate (Hz) of the output showing (x, y), if known.
    fn refresh_at(&self, x: i32, y: i32) -> Option<f32>;
    /// Visible windows bottom → top, hers left out.
    fn windows(&self) -> Vec<Win>;
    /// The window he works in.
    fn active_window(&self) -> Option<WinId>;
    /// Whatever holds the keyboard (any window, hers too).
    fn focused_window(&self) -> Option<WinId>;
    /// The top-level window containing `w` (its frame for a managed client).
    fn toplevel_of(&self, w: WinId) -> Option<WinId>;
    /// Outer rect of a top-level window, if it is visible.
    fn toplevel_rect(&self, w: WinId) -> Option<WinRect>;
    /// The window still exists, visible or not (hidden is not closed).
    fn window_exists(&self, w: WinId) -> bool;
    /// `w` asks for fullscreen.
    fn is_fullscreen(&self, w: WinId) -> bool;
    fn pointer(&self) -> Result<(i32, i32)>;
    /// A pointer reader with its own connection, for a watcher thread.
    fn pointer_reader(&self) -> Result<PointerRead>;
    /// Milliseconds since the last keyboard or mouse input anywhere.
    fn idle_ms(&self) -> Option<u32>;
    /// A key that types `ch` is physically down now.
    fn char_key_down(&self, ch: char) -> Result<bool>;
    /// A compositor runs (at connect; later changes come as `Hint::Compositor`).
    fn compositor_running(&self) -> bool;
    /// Start the event source; `hint` runs on its thread.
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()>;
    /// Start reporting clicks anywhere (root px) while armed; `on` runs on its thread.
    fn watch_clicks(&self, on: Box<dyn Fn(i32, i32) + Send>) -> Result<()>;
    fn arm_clicks(&self, on: bool);
}

/// Primitive commands on her own surfaces. Main thread only (a backend may hold GPUI
/// handles). Timing, animation and stacking order are the core's.
pub trait Stage {
    /// The backend's part of a window's options (its kind).
    fn dress(&self, role: Role, o: &mut WindowOptions);
    /// After `open_window`: the backend's id for it (the body's is remembered).
    fn adopt(&self, role: Role, window: &Window) -> Result<WinId>;
    /// Her body's id (0 before `adopt`).
    fn body(&self) -> WinId;
    /// Body origin, root px.
    fn move_to(&self, x: i32, y: i32) -> Result<()>;
    fn raise(&self) -> Result<()>;
    /// Place and size one of her other windows.
    fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()>;
    /// Body input region and visible region; each `None` stays as it is.
    fn set_shapes(&self, input: Option<&[ShapeRect]>, visible: Option<Visible<'_>>) -> Result<()>;
    fn set_icon(&self, s: WinId, data: &[u32]) -> Result<()>;
    /// Show effect layer `layer`: solid `rgb` over `rects` (relative to `origin`; `None`
    /// keeps the last ones), `size` px, stacked `z`. Input passes through. Not flushed.
    fn fx_show(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()>;
    /// Hide one layer (not flushed).
    fn fx_hide(&self, layer: usize);
    /// Hide every layer (flushed).
    fn fx_hide_all(&self);
    fn flush(&self);
    /// Keyboard to her surface `s`.
    fn grab_keyboard(&self, s: WinId) -> Result<()>;
    /// Her surface `s` lets the keyboard go, to `to` when given.
    fn release_keyboard(&self, s: WinId, to: Option<WinId>) -> Result<()>;
}

/// Connect to the desktop she runs on, the same one GPUI picks: Wayland when
/// `WAYLAND_DISPLAY` is set (`SA_BACKEND=x11` unsets it first, in `process_setup`, for
/// XWayland), else X11. On Wayland only Hyprland is supported so far.
pub fn connect() -> Result<Os> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
        let (desk, stage) = hypr::connect().map_err(|e| {
            anyhow::anyhow!("{e:#}; on another Wayland desktop run SA_BACKEND=x11 (through XWayland)")
        })?;
        return Ok(Os::new(desk, Box::new(stage)));
    }
    let x = std::sync::Arc::new(x11::X11::connect()?);
    Ok(Os::new(x.clone(), Box::new(x)))
}
