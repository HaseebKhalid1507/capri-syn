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
mod track;
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
    /// keeps them in a display list and paints them itself: in her window, and the ones
    /// reaching outside it in its effect hosts (`host_per_output`).
    pub fx_layers: bool,
    /// Every redraw of her window is a full present for the compositor to take in. Steps that
    /// can wait (glints) then ride redraws that happen anyway.
    pub costly_redraw: bool,
    /// A press on her keeps the pointer until the release, whatever her input region does
    /// meanwhile (X's implicit grab). Without it the core holds her input region at her whole
    /// window while a press lasts: the window follows the pointer, so the pointer stays inside.
    pub implicit_grab: bool,
    /// A window's rect is where it is going (the compositor animates it there): a perch that
    /// jumps glides after it instead of snapping.
    pub goal_geometry: bool,
    /// Every change of the monitors or of what panels reserve comes as `Hint::Layout`: the
    /// core keeps the layout until one (else it re-reads it every few seconds).
    pub layout_events: bool,
    /// A surface of hers shows on one output only: the core keeps a body host per output
    /// and shows her in the one under her (else one window spans every monitor).
    pub host_per_output: bool,
}

/// How the compositor animates a window to a new rect (`Caps::goal_geometry`): how long, and
/// its timing curve, a cubic bezier through (0,0), (x0,y0), (x1,y1), (1,1) (y may pass 1: an
/// overshoot). `ms` 0: it jumps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveAnim {
    pub ms: f32,
    pub bezier: [f32; 4],
}

impl MoveAnim {
    /// What a jump glides on where the backend can't say: a cubic ease-out over 450 ms.
    pub const EASE_OUT: MoveAnim = MoveAnim { ms: 450.0, bezier: [0.33, 1.0, 0.68, 1.0] };

    /// The fraction of the way there at `u` (of `ms`, clamped to 0..1).
    pub fn progress(&self, u: f32) -> f32 {
        let [x0, y0, x1, y1] = self.bezier;
        let at = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s;
        let u = u.clamp(0.0, 1.0);
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let m = 0.5 * (lo + hi);
            if at(x0, x1, m) < u { lo = m } else { hi = m }
        }
        at(y0, y1, 0.5 * (lo + hi))
    }
}

/// Where a window is, for the target poller (`Desktop::toplevel_place`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub rect: WinRect,
    /// The space it is on (a workspace; 0 where there are none). A target that turns up on
    /// another space has left hers: gone, never followed.
    pub space: i64,
    /// It can move without anything telling the backend (a floating window): polled faster.
    pub loose: bool,
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
    /// Windows opened, closed, or changed space or mode: the core re-reads what it follows.
    Changed,
    /// Monitors or the areas panels reserve changed.
    Layout,
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
    /// Monitors or work areas changed (the cached layout is stale).
    Layout,
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
    /// An output's effect host (`Caps::host_per_output`): paints the effect layers that
    /// reach outside her window (the bolt).
    Fx,
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
    /// How the compositor animates a window to a new rect (`Caps::goal_geometry`); None: it
    /// can't say.
    fn move_anim(&self) -> Option<MoveAnim> {
        None
    }
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
    /// Where a top-level window is, if it is visible (one read: the target poller's).
    fn toplevel_place(&self, w: WinId) -> Option<Place> {
        self.toplevel_rect(w).map(|rect| Place { rect, space: 0, loose: true })
    }
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
    /// Her body's id (0 before `adopt`): the host she shows in.
    fn body(&self) -> WinId;
    /// She shows in body host `s` from now on (`Caps::host_per_output`): body moves, shapes
    /// and the keyboard go to it; the one she left takes no input.
    fn set_body(&self, _s: WinId) {}
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
