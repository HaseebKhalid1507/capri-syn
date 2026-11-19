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
//! vendored GPUI's `layer_ctl`), `windows/` (Win32 + DWM, through the vendored GPUI's Windows
//! crate). Each passes the same contract suite (`contract.rs`: on the in-memory `fake.rs`
//! in every `cargo test`, on Xvfb for X11, in the `tools/wlrig` sandbox for Hyprland, on the
//! interactive desktop for Windows).
//!
//! The OS services the core uses outside the desktop (the local socket, the clock, the
//! calendar day) are re-exported here too, so nothing outside `src/platform/` picks an OS.

use anyhow::Result;
use gpui_kit::{Window, WindowOptions};

#[cfg(target_os = "linux")]
mod hypr;
#[cfg(target_os = "linux")]
mod linux;
mod os;
pub mod pointer;
mod settle;
mod track;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

#[cfg(test)]
mod contract;
#[cfg(test)]
mod fake;

pub use crate::edges::WinRect;
pub use crate::shape::ShapeRect;
pub use os::Os;

// The process and the OS's services beside the desktop: `process_setup`, what must run before
// any other thread exists (Linux: the GPU choice and the allocator, `linux/mod.rs`; Windows: DPI
// awareness), the clock, the calendar, the media players (`watch_media`: `on_change` gets the
// sum whenever it changes, first call now) and notifications (`notify`: blocking, Ok is its id).
#[cfg(target_os = "linux")]
pub use linux::{base_dir, home_dir, local_day, monotonic_s, notify, process_env, process_setup, trim_heap, utc_offset_s, watch_media};
#[cfg(windows)]
pub use self::windows::{base_dir, home_dir, local_day, monotonic_s, notify, process_env, process_setup, trim_heap, utc_offset_s, watch_media};
/// The voice sidecar's link (capri-voice; newline-delimited JSON both ways) and where it is, for
/// her log. Linux: its Unix socket. Windows: loopback TCP after a token handshake (the sidecar is
/// Python, and stock CPython has no AF_UNIX there).
/// `VOICE_LINK_BLOCKS`: whether `voice_link` can block (Windows: a connect and a hello), so she
/// calls `voice_link_bg` instead. `voice_sidecar(on)`: whether the sidecar should run, every tick;
/// Windows starts and keeps it (a job object, dying with her), Linux leaves it to systemd.
#[cfg(target_os = "linux")]
pub use linux::{VOICE_LINK_BLOCKS, VoiceLink, voice_link, voice_link_bg, voice_sidecar};
#[cfg(windows)]
pub use self::windows::{VOICE_LINK_BLOCKS, VoiceLink, voice_link, voice_link_bg, voice_sidecar};

/// Where her files go, per user (`base_dir`): the core adds `synaps-avatar/…`. `$XDG_<KIND>_HOME`
/// wins on every OS; else Linux: under `$HOME` (`.config`, `.local/share`, `.local/state`),
/// Windows: `%APPDATA%` (config, it roams) and `%LOCALAPPDATA%` (data, state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseDir {
    /// `settings.toml`.
    Config,
    /// Packs.
    Data,
    /// `state.json`, logs.
    State,
}

/// His own icon's name beside settings.toml (`crate::brand::icon`), in the OS's icon format.
#[cfg(windows)]
pub const ICON_FILE: &str = "icon.ico";
#[cfg(not(windows))]
pub const ICON_FILE: &str = "icon.png";

impl BaseDir {
    /// Its `XDG_*_HOME` variable.
    pub fn xdg_var(self) -> &'static str {
        match self {
            BaseDir::Config => "XDG_CONFIG_HOME",
            BaseDir::Data => "XDG_DATA_HOME",
            BaseDir::State => "XDG_STATE_HOME",
        }
    }
}
#[cfg(test)]
#[cfg(target_os = "linux")]
pub use linux::stderr_into_closed_pipe;
#[cfg(test)]
#[cfg(windows)]
pub use self::windows::stderr_into_closed_pipe;

/// The local stream socket (the control channel `capri-ctl` and the brain use): a Unix socket on
/// every OS (AF_UNIX on Windows 10 1803+). The voice sidecar's link is `VoiceLink`.
#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as LocalListener, UnixStream as LocalStream};
#[cfg(windows)]
pub use uds_windows::{UnixListener as LocalListener, UnixStream as LocalStream};

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
    /// The desktop hands her the keyboard only right after his own input to her (her key, a
    /// click on one of her windows: Windows' foreground lock). Anything else that opens the Line
    /// shows it waiting for him (`Stage::keyboard_granted`); she never takes the keyboard from
    /// the window he works in.
    pub keyboard_on_input: bool,
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
    // Not sent on Windows: its hooks report each window's own change.
    #[cfg_attr(windows, allow(dead_code))]
    Changed,
    /// Monitors or the areas panels reserve changed.
    Layout,
    /// Whether a compositor runs now, and whether it takes a change of her window's shape
    /// for a resize (`Desktop::compositor_reshapes`).
    // Not sent on Windows: DWM always composites.
    #[cfg_attr(windows, allow(dead_code))]
    Compositor { runs: bool, reshapes: bool },
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
    /// Escape pressed anywhere, while the intro plays.
    Escape,
    /// He focused another window.
    Focus,
    /// He switched workspace.
    Desktop,
    /// Monitors or work areas changed (the cached layout is stale).
    Layout,
    /// The pointer near her, swept past her, or resting on her (`Os::watch_pointer`).
    Pointer(PointerEv),
    /// The login session: the screen locked or unlocked, the machine asleep or back
    /// (`Os::watch_session`).
    Session(SessionEv),
}

/// The login session, as the session manager has it (booleans only, BRIEF-alive §4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEv {
    Locked,
    Unlocked,
    /// Going to sleep (suspend, hibernate).
    Sleeping,
    /// Back from sleep.
    Resumed,
}

/// Her global keys, as letters (settings `[voice]`: `key`, `toggle_key`): the summon key (a
/// tap opens the Line) and the toggle-to-talk chord's. A backend that grabs keys itself
/// (`Desktop::watch_keys`) turns them into its OS's chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub summon: char,
    pub toggle: char,
    /// `[voice] hold_to_talk`: the summon key reports its press and its release (`TalkPress`,
    /// `TalkRelease`: a hold talks, a tap opens the Line) instead of a bare tap.
    pub hold: bool,
}

/// A press of one of her global keys (a backend that grabs them reports presses only).
// Only a backend that grabs keys sends these (Windows; on Linux his desktop binds them and the
// toggle chord is never named).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEv {
    /// The summon key: a tap.
    Summon,
    /// The toggle-to-talk chord.
    TalkToggle,
    /// With `Keys::hold`: the summon key went down ...
    TalkPress,
    /// ... and up again.
    TalkRelease,
}

/// What the pointer watcher reports (device px, px/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEv {
    Near,
    Swipe(f32),
    Hover,
    /// Still beside her (not on her) a while.
    Linger,
    /// The lingering pointer moved off.
    LingerEnd,
    /// A quick left-right wiggle near her (once a burst).
    Wiggle,
    /// The burst goes on: each later turn, where.
    WiggleAt(i32, i32),
    /// The burst stopped (no turn for `wiggle_end`), where.
    WiggleEnd(i32, i32),
}

/// Pointer watcher tuning (device px; `pointer::cfg` scales the defaults by sf).
pub struct PointerCfg {
    pub near_px: f32,
    pub swipe_px: f32,
    pub swipe_speed: f32,
    pub hover: std::time::Duration,
    /// Linger: within this of her body, moving under `still_px`, for `linger`.
    pub linger_px: f32,
    pub still_px: f32,
    pub linger: std::time::Duration,
    /// Wiggle: `wiggle_turns` x-reversals of `wiggle_amp` within `wiggle_window`, within
    /// `wiggle_px` of her.
    pub wiggle_px: f32,
    pub wiggle_amp: f32,
    pub wiggle_window: std::time::Duration,
    pub wiggle_turns: u8,
    /// A burst ends this long after its last turn.
    pub wiggle_end: std::time::Duration,
    /// Within `after_wiggle` of a burst's last turn, a linger is this quick (a pounce's cue).
    pub after_wiggle: std::time::Duration,
    pub linger_after_wiggle: std::time::Duration,
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
    /// The launch's close-up (`crate::intro`): shaped like her body, input on its pixels only.
    Intro,
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
    /// The root's size, px.
    fn root_size(&self) -> (i32, i32);
    /// Device px across what GPUI calls its primary display (her scale `sf` is this ÷ that
    /// display's width in GPUI px): the root where that display is the whole screen (X11), the
    /// primary monitor's width where it is one monitor (Windows).
    fn primary_display_px(&self) -> i32 {
        self.root_size().0
    }
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
    /// The primary (left) button is down now, as the server has it; None: the backend can't
    /// tell (`Os::press_lost` then never fires).
    fn primary_down(&self) -> Option<bool> {
        None
    }
    /// A pointer reader with its own connection, for a watcher thread.
    fn pointer_reader(&self) -> Result<PointerRead>;
    /// Milliseconds since the last keyboard or mouse input anywhere. None: the backend can't
    /// tell (the core then counts from the pointer's last move).
    fn idle_ms(&self) -> Option<u32>;
    /// A key that types `ch` is physically down now.
    fn char_key_down(&self, ch: char) -> Result<bool>;
    /// A compositor runs (at connect; later changes come as `Hint::Compositor`).
    fn compositor_running(&self) -> bool;
    /// The compositor running takes any change of her window's shape (X SHAPE, her input
    /// region's too) for a resize, and a move in the same frame cross-fades her from a snapshot
    /// taken before it, until she holds still (picom's animations: a copy of her facing the old
    /// way through a drag after a turn). The core then keeps the two apart (`Os::settling`).
    /// At connect; later changes come as `Hint::Compositor`.
    fn compositor_reshapes(&self) -> bool {
        false
    }
    /// Start the event source; `hint` runs on its thread.
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()>;
    /// Start reporting the session's lock and sleep (`on` runs on its thread). Linux: logind,
    /// shared by both backends.
    fn watch_session(&self, on: Box<dyn Fn(SessionEv) + Send>) -> Result<()>;
    /// Grab her global keys and report each press (`on` runs on its thread). Ok(false): this
    /// backend grabs none; the desktop's own bindings drive her (`capri-ctl talk ...`).
    fn watch_keys(&self, _keys: Keys, _on: Box<dyn Fn(KeyEv) + Send>) -> Result<bool> {
        Ok(false)
    }
    /// How he presses her key `ev` (its letter `key`) here, for a hint (`WIN+ALT+C`). None: his
    /// desktop binds it (the core's words for it then).
    fn key_label(&self, _ev: KeyEv, _key: char) -> Option<String> {
        None
    }
    /// Start reporting clicks anywhere (root px) while armed; `on` runs on its thread.
    fn watch_clicks(&self, on: Box<dyn Fn(i32, i32) + Send>) -> Result<()>;
    fn arm_clicks(&self, on: bool);
    /// While armed, an Escape press anywhere runs `on` (on the click thread). Nothing is
    /// grabbed: the focused window gets it too. Backends without it never call `on`.
    fn watch_escape(&self, _on: Box<dyn Fn() + Send>) {}
    fn arm_escape(&self, _on: bool) {}
    /// The pointer's image over her surface `s` (None: the desktop's). Backends without it keep
    /// the desktop's.
    fn pointer_image(&self, _s: WinId, _img: Option<&PointerImage>) {}
    /// Put her icon in the desktop's tray; `on` runs on the tray's own thread. `instance`: empty
    /// for her usual run, else what tells this run apart (her control socket), so two of her
    /// never share one icon. Ok(None): no tray she uses here (Linux for now: `linux::tray`).
    fn tray(&self, _icon: TrayIcon, _instance: &str, _on: Box<dyn Fn(TrayEv) + Send>) -> Result<Option<Box<dyn Tray>>> {
        Ok(None)
    }
}

/// A pointer image: premultiplied ARGB, `w`×`h`, its hotspot. `key` tells images apart (a backend
/// may keep the one it made).
// The Windows backend keeps the system pointer for now (`Stage::pointer_image`'s default).
#[cfg_attr(windows, allow(dead_code))]
#[derive(Clone, Debug)]
pub struct PointerImage {
    pub key: u64,
    pub w: u16,
    pub h: u16,
    pub hot: (u16, u16),
    pub argb: Vec<u32>,
}

// ---- her tray icon --------------------------------------------------------------------------
// Only Windows has a tray so far: on Linux these are built and never read (`linux::tray`).

/// Her icon for the desktop's tray (`crate::brand::Icon::tray`): its picture, straight-alpha RGBA
/// `w`×`h` (at most 256 a side), which the backend scales to what its tray wants, and the file
/// it was read from. A backend that can load an icon's own sizes takes them from that file, or
/// with none from hers (Windows: her exe's icon resource), and the picture where that fails.
/// `stamp`: the same icon has the same one (`Os::tray_icon` tells only a new one).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub struct TrayIcon {
    pub file: Option<std::path::PathBuf>,
    pub stamp: u64,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// A row of her tray's menu, as her menu's model lays it out (`crate::menu`): the backend only
/// draws it, and a pick comes back as its `id`.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub enum TrayItem {
    /// A pick (`id` never 0). `checked`: the choice in force; `lead`: the row that matters most
    /// now (bold where the tray can); `enabled` false: shown greyed, not picked (`menu::enabled`).
    Item { id: u32, label: String, checked: bool, lead: bool, enabled: bool },
    /// A row that opens its choices.
    Sub { label: String, items: Vec<TrayItem>, enabled: bool },
    Rule,
}

/// What he does to her tray icon.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEv {
    /// A click on it (or Enter on it): what her summon key's tap does.
    Open,
    /// A row of its menu, by its `TrayItem::Item` id.
    Pick(u32),
}

/// Her icon in the desktop's tray (`Desktop::tray`): it stays while she runs, hidden or not.
/// The core sends only what changed (`Os::tray_tip`, `Os::tray_menu`). Main thread.
pub trait Tray {
    /// Its tooltip (`crate::brand::tooltip`).
    fn set_tip(&self, tip: &str);
    /// Its icon from now on (his own put in or taken away: `settings reload`).
    fn set_icon(&self, icon: TrayIcon);
    /// The menu its next right click opens.
    fn set_menu(&self, items: Vec<TrayItem>);
    /// Out of the tray now, nothing left behind (the end of a quit). Dropping it does the same.
    fn remove(&self);
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
    /// The same for another of her surfaces (the intro's close-up).
    fn shape_of(&self, s: WinId, input: &[ShapeRect], visible: Visible<'_>) -> Result<()>;
    fn set_icon(&self, s: WinId, data: &[u32]) -> Result<()>;
    /// `set_icon` marks her windows here (X11's `_NET_WM_ICON`; elsewhere it does nothing, and
    /// the core doesn't make one).
    fn window_icons(&self) -> bool {
        false
    }
    /// Show effect layer `layer`: solid `rgb` over `rects` (relative to `origin`; `None`
    /// keeps the last ones), `size` px, stacked `z`. Input passes through. Not flushed.
    fn fx_show(&self, layer: usize, rgb: u32, origin: (i32, i32), size: (i32, i32), rects: Option<&[ShapeRect]>, z: FxZ) -> Result<()>;
    /// Hide one layer (not flushed).
    fn fx_hide(&self, layer: usize);
    /// Hide every layer (flushed).
    fn fx_hide_all(&self);
    fn flush(&self);
    /// Keyboard to her surface `s`. Where `Caps::keyboard_on_input`: Err without a grant
    /// (`keyboard_granted`), and the keyboard stays where it is.
    fn grab_keyboard(&self, s: WinId) -> Result<()>;
    /// The desktop would hand her the keyboard now: one of her windows holds it, or he just
    /// gave her his input (her key, a click on her). Only asked where `Caps::keyboard_on_input`.
    fn keyboard_granted(&self) -> bool {
        true
    }
    /// Her surface `s` lets the keyboard go, to `to` when given.
    fn release_keyboard(&self, s: WinId, to: Option<WinId>) -> Result<()>;
}

/// Connect to the desktop she runs on, the same one GPUI picks: Wayland when
/// `WAYLAND_DISPLAY` is set (`SA_BACKEND=x11` unsets it first, in `process_setup`, for
/// XWayland), else X11. On Wayland only Hyprland is supported so far.
#[cfg(target_os = "linux")]
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

/// Connect to the Windows desktop (DWM always composites).
#[cfg(windows)]
pub fn connect() -> Result<Os> {
    let w = std::sync::Arc::new(self::windows::Win32::connect()?);
    Ok(Os::new(w.clone(), Box::new(w)))
}
