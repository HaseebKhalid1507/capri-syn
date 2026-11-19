//! The Linux process: what has to be set before GPUI starts (GPU choice, the allocator), and
//! the session bus's services (media players, notifications, logind, a tray icon some day).
//! Shared by every Linux backend (X11, Hyprland).

mod media;
mod notifications;

pub use media::watch_media;
pub use notifications::notify;

use super::BaseDir;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// XDG's base directories: `$XDG_<KIND>_HOME`, else under `$HOME`.
pub fn base_dir(d: BaseDir) -> Option<PathBuf> {
    base_dir_in(d, |k| std::env::var_os(k))
}

fn base_dir_in(d: BaseDir, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let under = match d {
        BaseDir::Config => ".config",
        BaseDir::Data => ".local/share",
        BaseDir::State => ".local/state",
    };
    var(d.xdg_var()).map(PathBuf::from).or_else(|| var("HOME").map(|h| PathBuf::from(h).join(under)))
}

/// `$HOME`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Her end of the voice sidecar's link.
pub type VoiceLink = std::os::unix::net::UnixStream;

/// The sidecar's socket (`$CAPRI_VOICE_SOCK`, else `$XDG_RUNTIME_DIR/capri-voice.sock`, else in
/// the temp dir) and a link to it.
pub fn voice_link() -> (String, std::io::Result<VoiceLink>) {
    let p = match std::env::var_os("CAPRI_VOICE_SOCK") {
        Some(p) => PathBuf::from(p),
        None => std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("capri-voice.sock"),
    };
    let s = VoiceLink::connect(&p);
    (p.display().to_string(), s)
}

/// `voice_link` doesn't block here (a local socket: it is there or not), so she calls it inline.
pub const VOICE_LINK_BLOCKS: bool = false;

/// `voice_link` on a thread of its own (unused here: `VOICE_LINK_BLOCKS` is false).
pub fn voice_link_bg(done: impl FnOnce(String, std::io::Result<VoiceLink>) + Send + 'static) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("voice-connect".into())
        .spawn(move || {
            let (p, s) = voice_link();
            done(p, s)
        })
        .map(|_| ())
}

/// A systemd user unit runs the sidecar here (`tools/systemd/capri-voice.service`): she starts
/// nothing.
pub fn voice_sidecar(_on: bool, _mic: &str) {}

/// The display server she runs on (`backend`). GPUI picks its own from the same variables, so
/// `process_setup` unsets `WAYLAND_DISPLAY` for both XWayland kinds before it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// X11: no Wayland session (`WAYLAND_DISPLAY` unset or blank).
    X11,
    /// Wayland itself: Hyprland (`hyprland_in`), or `SA_BACKEND=wayland` (Hyprland or nothing:
    /// it's the only Wayland desktop she has a backend for).
    Wayland,
    /// X11 through XWayland, asked for: `SA_BACKEND=x11`.
    XWayland,
    /// X11 through XWayland, picked for him: a Wayland desktop that isn't Hyprland (GNOME, KDE,
    /// sway), with XWayland running (`DISPLAY` set).
    XWaylandAuto,
    /// A Wayland desktop that isn't Hyprland, without XWayland (`DISPLAY` unset or blank):
    /// nothing she can run on.
    NoXWayland,
    /// `SA_BACKEND=wayland` with no Wayland session (`WAYLAND_DISPLAY` unset or blank): asked
    /// for, so no fallback to X11.
    NoWayland,
}

/// What `SA_BACKEND` asks for: `x11` (XWayland) or `wayland`, in any case, trimmed. None for a
/// blank value or one she doesn't know (`process_setup` says so).
fn asked(v: &OsStr) -> Option<Backend> {
    match v.to_str()?.trim().to_ascii_lowercase().as_str() {
        "x11" => Some(Backend::XWayland),
        "wayland" => Some(Backend::Wayland),
        _ => None,
    }
}

/// Whether this is Hyprland's session, as install.sh's `desktop` reads it: its signature
/// (`HYPRLAND_INSTANCE_SIGNATURE`) naming one that runs (`live`: its request socket is there; a
/// signature left over from an earlier Hyprland login, in GNOME's environment or a tmux server's,
/// names none), or `XDG_CURRENT_DESKTOP` or `XDG_SESSION_DESKTOP` saying Hyprland (one whose
/// signature got lost on the way to her, as systemd or D-Bus activation can: `connect` then says
/// what's missing, rather than her taking XWayland unasked).
fn hyprland_in(var: &impl Fn(&str) -> Option<OsString>, live: &impl Fn(&str) -> bool) -> bool {
    let sig = var("HYPRLAND_INSTANCE_SIGNATURE").and_then(|v| v.into_string().ok()).unwrap_or_default();
    if !sig.trim().is_empty() && live(sig.as_str()) {
        return true;
    }
    ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
        .into_iter()
        .any(|k| var(k).is_some_and(|v| v.to_string_lossy().to_lowercase().contains("hyprland")))
}

/// Which display server she runs on, from her environment (`var`: a variable, None when unset;
/// `live`: whether a Hyprland signature names one that runs). `SA_BACKEND` wins whenever it's set
/// to one she knows (`x11`, `wayland`); anything else in it is ignored. Without it, a Wayland
/// session is Hyprland's (natively) or, anywhere else, XWayland's when it has one.
fn backend_in(var: impl Fn(&str) -> Option<OsString>, live: impl Fn(&str) -> bool) -> Backend {
    let set = |k: &str| var(k).is_some_and(|v| !v.is_empty());
    let ask = var("SA_BACKEND").and_then(|v| asked(&v));
    if !set("WAYLAND_DISPLAY") {
        return if ask == Some(Backend::Wayland) { Backend::NoWayland } else { Backend::X11 };
    }
    if let Some(b) = ask {
        return b;
    }
    if hyprland_in(&var, &live) {
        Backend::Wayland
    } else if set("DISPLAY") {
        Backend::XWaylandAuto
    } else {
        Backend::NoXWayland
    }
}

/// `backend_in` on her own environment. After `process_setup` an XWayland pick reads as X11
/// (`WAYLAND_DISPLAY` is gone by then).
pub fn backend() -> Backend {
    backend_in(|k| std::env::var_os(k), hypr_live)
}

/// `hyprland_in` on her own environment.
pub fn hyprland() -> bool {
    hyprland_in(&|k: &str| std::env::var_os(k), &hypr_live)
}

/// Whether the Hyprland instance `sig` runs: its request socket is there
/// (`$XDG_RUNTIME_DIR/hypr/<sig>/.socket.sock`, as `hypr::ipc` connects to it).
fn hypr_live(sig: &str) -> bool {
    std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|r| PathBuf::from(r).join("hypr").join(sig).join(".socket.sock").exists())
}

/// Process setup that has to happen before any other thread exists:
///
/// * Her display server (`backend`): through XWayland, `WAYLAND_DISPLAY` is unset for GPUI
///   and for `connect`, with one line in her log when she picked that herself.
///
/// Each knob below was measured on bella (release, `ps` RSS 6 s after start, see LOG.md):
///
/// * Render on the boot VGA device (the iGPU on a hybrid laptop) unless `ZED_DEVICE_ID` is
///   set. On bella every vkQueuePresent to this 32-bit override-redirect window
///   busy-waited ~2.3 s on the NVIDIA dGPU (0.4 fps); the Intel iGPU presents at once. A
///   desktop pet belongs on the low-power GPU anyway.
/// * Keep GPUI's GL fallback from probing EGL when a Vulkan driver is installed. The probe
///   loads every EGL vendor (NVIDIA's, Mesa's llvmpipe with LLVM) and 50 threads that stay
///   for the life of the process: 371 → 226 MB RSS. `SA_GL=1` keeps it.
/// * glibc malloc: two arenas and a 1 MiB trim threshold, so the full-size PNG decode
///   buffers go back to the OS after scaling: 226 → 84 MB.
pub fn process_setup() {
    // Her display server, and a line for each thing about it she ignored or picked herself.
    let b = backend();
    if let Some(v) = std::env::var_os("SA_BACKEND").filter(|v| !v.to_string_lossy().trim().is_empty() && asked(v).is_none()) {
        elog!("synaps-avatar: SA_BACKEND={v:?} isn't x11 or wayland: ignored");
    }
    if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        && !sig.trim().is_empty()
        && !hypr_live(&sig)
    {
        elog!("synaps-avatar: HYPRLAND_INSTANCE_SIGNATURE={sig} names no Hyprland that runs (no $XDG_RUNTIME_DIR/hypr/{sig}/.socket.sock): one left over from an earlier session, ignored");
    }
    if b == Backend::XWayland && hyprland() {
        elog!("synaps-avatar: SA_BACKEND=x11 on Hyprland: through XWayland she sees X clients only, and can't stay above native windows");
    }
    if b == Backend::XWaylandAuto {
        let display = std::env::var("DISPLAY").unwrap_or_default();
        elog!("synaps-avatar: Wayland, not Hyprland: through XWayland (DISPLAY={display}), as SA_BACKEND=x11 does; she sees X clients only");
    }
    // Through XWayland (SA_BACKEND=x11, or a Wayland desktop that isn't Hyprland): GPUI and we
    // both take X11.
    if matches!(b, Backend::XWayland | Backend::XWaylandAuto) {
        // SAFETY: first thing in main, single-threaded.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    if std::env::var_os("SA_GL").is_none() && has_vulkan_driver() {
        // SAFETY (all set_var calls here): first thing in main, single-threaded.
        unsafe { std::env::set_var("__EGL_VENDOR_LIBRARY_DIRS", "/nonexistent") };
    }
    #[cfg(target_env = "gnu")]
    // SAFETY: plain libc calls with valid parameters.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 1 << 20);
        // Pixel buffers (a frame, a strip's cells, the intro's close-ups) in maps of their own,
        // handed back whole when freed: a fixed threshold (glibc's moving one put them in the
        // heap after the first free, where they stayed after an intro replay).
        libc::mallopt(libc::M_MMAP_THRESHOLD, 256 << 10);
    }
    if std::env::var_os("ZED_DEVICE_ID").is_some() || std::env::var_os("SA_ANY_GPU").is_some() {
        return;
    }
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut cards = Vec::new();
    for e in std::fs::read_dir("/sys/class/drm").into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev = e.path().join("device");
        cards.push((read(dev.join("device")), read(dev.join("boot_vga")) == "1", read(dev.join("vendor"))));
    }
    if cards.len() < 2 {
        return;
    }
    if let Some((id, _, vendor)) = cards.iter().find(|c| c.1 && !c.0.is_empty()) {
        elog!("synaps-avatar: {} GPUs, rendering on the boot VGA device {id} (SA_ANY_GPU=1 to let GPUI pick)", cards.len());
        unsafe { std::env::set_var("ZED_DEVICE_ID", id) };
        // Only that vendor's Vulkan driver: the loader otherwise maps every ICD, and the
        // unused NVIDIA one alone was ~52 MB of RSS (libnvidia-gpucomp/rtcore/glcore).
        if std::env::var_os("VK_DRIVER_FILES").is_none()
            && std::env::var_os("VK_ICD_FILENAMES").is_none()
            && let Some(files) = vendor_icds(vendor)
        {
            unsafe { std::env::set_var("VK_DRIVER_FILES", &files) };
        }
    }
}

/// The Vulkan driver manifests of a PCI vendor (`0x8086` …), joined for `VK_DRIVER_FILES`.
fn vendor_icds(vendor: &str) -> Option<String> {
    let prefixes: &[&str] = match vendor {
        "0x8086" => &["intel"],
        "0x1002" => &["radeon", "amd"],
        "0x10de" => &["nvidia"],
        _ => return None,
    };
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<std::path::PathBuf> = data_dirs.split(':').map(|d| std::path::Path::new(d).join("vulkan/icd.d")).collect();
    dirs.push("/etc/vulkan/icd.d".into());
    let mut files: Vec<String> = Vec::new();
    for d in dirs {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && prefixes.iter().any(|p| name.starts_with(p)) {
                files.push(e.path().to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    files.dedup();
    (!files.is_empty()).then(|| files.join(":"))
}

/// Another process's environment (his own processes: `/proc/<pid>/environ`), read once: the
/// names set, not blank (no value kept), and its `LOCAL_ENDPOINT`. None when it can't be read
/// (gone, not his).
pub fn process_environ(pid: u32) -> Option<super::Environ> {
    let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let mut e = super::Environ::default();
    for (k, v) in env.split(|&b| b == 0).filter_map(|kv| kv.iter().position(|&b| b == b'=').map(|i| (&kv[..i], kv[i + 1..].trim_ascii()))) {
        let k = String::from_utf8_lossy(k).into_owned();
        if k == "LOCAL_ENDPOINT" {
            e.local_endpoint = (!v.is_empty()).then(|| String::from_utf8_lossy(v).into_owned());
        }
        if !v.is_empty() {
            e.set.insert(k);
        }
    }
    Some(e)
}

/// Hand freed heap back to the OS (after the frame decode).
pub fn trim_heap() {
    #[cfg(target_env = "gnu")]
    // SAFETY: plain libc call.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Whether the Vulkan loader will find at least one driver manifest.
fn has_vulkan_driver() -> bool {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for var in ["VK_DRIVER_FILES", "VK_ICD_FILENAMES"] {
        if let Some(v) = std::env::var_os(var) {
            return std::env::split_paths(&v).any(|p| p.exists());
        }
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data_dirs.split(':').map(|d| std::path::Path::new(d).join("vulkan/icd.d")));
    dirs.push("/etc/vulkan/icd.d".into());
    dirs.iter().any(|d| std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "json")))
}

/// CLOCK_MONOTONIC seconds (the same clock as Python's `time.perf_counter`: dev logs line up
/// with the test tools').
pub fn monotonic_s() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a plain clock read into a local.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 * 1e-9
}

/// Tests: run `f` with stderr (fd 2) going into a pipe whose reader is gone; fd 2 is put back
/// after.
#[cfg(test)]
pub fn stderr_into_closed_pipe<R>(f: impl FnOnce() -> R) -> R {
    use std::os::fd::AsRawFd as _;
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    // SAFETY: plain fd juggling on this process's own descriptors; fd 2 is restored.
    let saved = unsafe { libc::dup(2) };
    assert!(saved >= 0 && unsafe { libc::dup2(writer.as_raw_fd(), 2) } == 2, "stderr into the pipe");
    let out = f();
    unsafe {
        libc::dup2(saved, 2);
        libc::close(saved);
    }
    drop(writer);
    out
}

/// The UTC offset (seconds east) of local time at unix second `unix` (DST included).
pub fn utc_offset_s(unix: i64) -> i32 {
    let t: libc::time_t = unix as libc::time_t;
    // SAFETY: localtime_r writes into a zeroed local tm; both pointers are valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i32
}

/// The local calendar day of a unix time (years since 1900, day of the year).
pub fn local_day(unix: f64) -> Option<(i32, i32)> {
    let t = unix as libc::time_t;
    // SAFETY: localtime_r only writes the tm we own; a null return is checked.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    (!r.is_null()).then_some((tm.tm_year, tm.tm_yday))
}

/// Her tray icon: none on Linux yet. X11 and Hyprland both route `Desktop::tray` here (as they
/// do `watch_session` to logind), so one StatusNotifierItem serves both:
///
/// - `linux/tray.rs`, on the session bus with the zbus that `media.rs` uses: an
///   `org.kde.StatusNotifierItem` object (`IconPixmap` from `icon`, ARGB32 big-endian; `ToolTip`;
///   `Status`), registered with `org.kde.StatusNotifierWatcher` (`RegisterStatusNotifierItem`),
///   and its menu as `com.canonical.dbusmenu` (`GetLayout` from the same `TrayItem`s, ids as they
///   are; `Event "clicked"` → `TrayEv::Pick(id)`). `Activate` → `TrayEv::Open`.
/// - The returned `Tray`: `set_tip` → `NewToolTip`, `set_icon` → `NewIcon`, `set_menu` →
///   `LayoutUpdated`, `remove` → the name released (the watcher drops the item).
/// - Its bus name `org.kde.StatusNotifierItem-<pid>-1`; `instance` goes in its `Id`, so two of
///   her (another control socket) show as two.
/// - No watcher on the bus (`NameHasOwner` false; plain i3bar has only an XEmbed tray, GNOME
///   needs the AppIndicator extension): Ok(None), as now; the restore chip stays the way back.
pub fn tray(_icon: super::TrayIcon, _instance: &str, _on: Box<dyn Fn(super::TrayEv) + Send>) -> anyhow::Result<Option<Box<dyn super::Tray>>> {
    Ok(None)
}

const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_SESSION: &str = "org.freedesktop.login1.Session";
const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";

/// logind on the system bus, one blocked thread (BRIEF-alive §4.7): the screen locked
/// (`LockedHint`, the `Lock`/`Unlock` requests) and the machine going to sleep and back
/// (`PrepareForSleep`). Booleans only. The session is ours (`XDG_SESSION_ID`), else the
/// user's display session.
pub fn watch_logind(on: Box<dyn Fn(super::SessionEv) + Send>) -> anyhow::Result<()> {
    use zbus::MatchRule;
    use zbus::blocking::{Connection, MessageIterator};
    use zbus::message::Type;
    let conn = Connection::system()?;
    let session = session_path(&conn)?;
    let rule = MatchRule::builder().msg_type(Type::Signal).sender(LOGIND)?.build();
    // Subscribed before the first read: nothing in between is missed.
    let msgs = MessageIterator::for_match_rule(rule, &conn, Some(64))?;
    let locked = {
        let session = session.clone();
        move |conn: &Connection| -> Option<bool> {
            let m = conn
                .call_method(Some(LOGIND), session.as_str(), Some("org.freedesktop.DBus.Properties"), "Get", &(LOGIND_SESSION, "LockedHint"))
                .ok()?;
            bool::try_from(m.body().deserialize::<zbus::zvariant::OwnedValue>().ok()?).ok()
        }
    };
    if locked(&conn) == Some(true) {
        on(super::SessionEv::Locked);
    }
    elog!("synaps-avatar: logind: watching session {session}");
    std::thread::Builder::new().name("avatar-logind".into()).spawn(move || {
        for m in msgs {
            let Ok(m) = m else { continue };
            match session_event(&m, &session) {
                Some(Seen::Ev(e)) => on(e),
                // LockedHint invalidated (not sent): read it.
                Some(Seen::Reread) => match locked(&conn) {
                    Some(true) => on(super::SessionEv::Locked),
                    Some(false) => on(super::SessionEv::Unlocked),
                    None => {}
                },
                None => {}
            }
        }
    })?;
    Ok(())
}

/// Our login session's object path.
fn session_path(conn: &zbus::blocking::Connection) -> anyhow::Result<zbus::zvariant::OwnedObjectPath> {
    use zbus::zvariant::OwnedObjectPath;
    let path_of = |r: zbus::Result<zbus::Message>| -> Option<OwnedObjectPath> { r.ok()?.body().deserialize::<OwnedObjectPath>().ok() };
    let path = "/org/freedesktop/login1";
    let by_id = |id: &str| path_of(conn.call_method(Some(LOGIND), path, Some(LOGIND_MANAGER), "GetSession", &(id,)));
    if let Some(p) = std::env::var("XDG_SESSION_ID").ok().filter(|s| !s.is_empty()).and_then(|id| by_id(&id)) {
        return Ok(p);
    }
    if let Some(p) = by_id("auto") {
        return Ok(p);
    }
    // A service outside the session (a systemd user unit): the user's display session.
    // SAFETY: getuid never fails.
    let uid = unsafe { libc::getuid() };
    let user = path_of(conn.call_method(Some(LOGIND), path, Some(LOGIND_MANAGER), "GetUser", &(uid,))).ok_or_else(|| anyhow::anyhow!("logind: no user {uid}"))?;
    let m = conn.call_method(Some(LOGIND), user.as_str(), Some("org.freedesktop.DBus.Properties"), "Get", &("org.freedesktop.login1.User", "Display"))?;
    let v = m.body().deserialize::<zbus::zvariant::OwnedValue>()?;
    let (_, p) = <(String, OwnedObjectPath)>::try_from(v)?;
    anyhow::ensure!(p.as_str() != "/", "logind: no display session");
    Ok(p)
}

/// What a logind signal says about us.
#[derive(Debug, PartialEq)]
enum Seen {
    Ev(super::SessionEv),
    /// Our LockedHint changed without its value: read it.
    Reread,
}

/// A logind signal → our session's event (the parser, unit-tested on recorded signals).
fn session_event(m: &zbus::Message, session: &zbus::zvariant::OwnedObjectPath) -> Option<Seen> {
    use super::SessionEv;
    use std::collections::HashMap;
    use zbus::zvariant::OwnedValue;
    let h = m.header();
    let member = h.member()?.as_str().to_string();
    let ours = h.path().is_some_and(|p| p.as_str() == session.as_str());
    let iface = h.interface().map(|i| i.as_str().to_string()).unwrap_or_default();
    match (iface.as_str(), member.as_str()) {
        (LOGIND_MANAGER, "PrepareForSleep") => {
            let start: bool = m.body().deserialize().ok()?;
            Some(Seen::Ev(if start { SessionEv::Sleeping } else { SessionEv::Resumed }))
        }
        (LOGIND_SESSION, "Lock") if ours => Some(Seen::Ev(SessionEv::Locked)),
        (LOGIND_SESSION, "Unlock") if ours => Some(Seen::Ev(SessionEv::Unlocked)),
        ("org.freedesktop.DBus.Properties", "PropertiesChanged") if ours => {
            let (i, changed, invalid): (String, HashMap<String, OwnedValue>, Vec<String>) = m.body().deserialize().ok()?;
            if i != LOGIND_SESSION {
                return None;
            }
            if let Some(v) = changed.get("LockedHint") {
                let on = bool::try_from(v.try_clone().ok()?).ok()?;
                return Some(Seen::Ev(if on { SessionEv::Locked } else { SessionEv::Unlocked }));
            }
            invalid.iter().any(|k| k == "LockedHint").then_some(Seen::Reread)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::SessionEv;

    /// The daemon's environment, read from outside (a key exported in the shell that started it).
    #[test]
    fn another_processs_env_is_read() {
        let mut c = std::process::Command::new("sleep")
            .arg("5")
            .env("SA_TEST_KEY", "sk-x")
            .env("SA_TEST_EMPTY", " ")
            .env("LOCAL_ENDPOINT", "http://10.0.0.2:8080/v1")
            .spawn()
            .expect("sleep");
        let pid = c.id();
        // Until its exec the child's environ is still ours: give it a moment.
        let t0 = std::time::Instant::now();
        let has = |k: &str| process_environ(pid).map(|e| e.set.contains(k));
        while has("SA_TEST_KEY") != Some(true) && t0.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(has("SA_TEST_KEY"), Some(true));
        assert_eq!(has("SA_TEST_EMPTY"), Some(false), "blank is unset");
        assert_eq!(has("SA_TEST_KE"), Some(false), "a whole name, not a prefix");
        assert_eq!(process_environ(pid).and_then(|e| e.local_endpoint), Some("http://10.0.0.2:8080/v1".into()));
        c.kill().ok();
        c.wait().ok();
        assert_eq!(process_environ(pid), None, "gone: can't be read");
    }

    /// Her display server: Hyprland natively (a signature naming one that runs, or the desktop
    /// variables saying Hyprland), any other Wayland desktop through XWayland when it runs, X11
    /// as it is; `SA_BACKEND` wins whenever it says x11 or wayland, in any case.
    #[test]
    fn wayland_off_hyprland_runs_through_xwayland() {
        // Hyprland `abc_123` runs (its socket is there); any other signature names none.
        fn pick(vars: &[(&str, &str)]) -> Backend {
            backend_in(|k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v)), |sig: &str| sig == "abc_123")
        }
        let (wl, hypr, x) = (("WAYLAND_DISPLAY", "wayland-0"), ("HYPRLAND_INSTANCE_SIGNATURE", "abc_123"), ("DISPLAY", ":0"));
        let stale = ("HYPRLAND_INSTANCE_SIGNATURE", "old_456");
        // X11: no Wayland session (a blank WAYLAND_DISPLAY is none, as GPUI reads it).
        assert_eq!(pick(&[]), Backend::X11);
        assert_eq!(pick(&[x]), Backend::X11);
        assert_eq!(pick(&[("WAYLAND_DISPLAY", ""), x]), Backend::X11);
        assert_eq!(pick(&[x, ("SA_BACKEND", "x11")]), Backend::X11, "on X11 there's nothing to unset");
        // Hyprland: natively, XWayland or not.
        assert_eq!(pick(&[wl, hypr, x]), Backend::Wayland);
        assert_eq!(pick(&[wl, hypr]), Backend::Wayland);
        // GNOME, KDE, sway: XWayland when it runs (she says so), else nothing she can run on.
        assert_eq!(pick(&[wl, x]), Backend::XWaylandAuto);
        assert_eq!(pick(&[wl]), Backend::NoXWayland);
        assert_eq!(pick(&[wl, ("DISPLAY", "")]), Backend::NoXWayland, "a blank DISPLAY is none");
        assert_eq!(pick(&[wl, ("HYPRLAND_INSTANCE_SIGNATURE", ""), x]), Backend::XWaylandAuto, "a blank signature isn't Hyprland");
        assert_eq!(pick(&[wl, ("HYPRLAND_INSTANCE_SIGNATURE", " "), x]), Backend::XWaylandAuto, "nor a blank one");
        // A signature left over from an earlier Hyprland login (in GNOME's environment, a tmux
        // server's) names no Hyprland that runs: not Hyprland.
        assert_eq!(pick(&[wl, stale, x, ("XDG_CURRENT_DESKTOP", "GNOME")]), Backend::XWaylandAuto);
        assert_eq!(pick(&[wl, stale]), Backend::NoXWayland);
        // Hyprland whose signature got lost (systemd or D-Bus activation): the desktop variables
        // say Hyprland (as install.sh reads them), and Hyprland's connect says what's missing.
        assert_eq!(pick(&[wl, x, ("XDG_CURRENT_DESKTOP", "Hyprland")]), Backend::Wayland);
        assert_eq!(pick(&[wl, x, ("XDG_SESSION_DESKTOP", "hyprland")]), Backend::Wayland);
        assert_eq!(pick(&[wl, stale, x, ("XDG_CURRENT_DESKTOP", "Hyprland")]), Backend::Wayland);
        assert_eq!(pick(&[wl, x, ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"), ("XDG_SESSION_DESKTOP", "ubuntu")]), Backend::XWaylandAuto);
        assert_eq!(pick(&[wl, x, ("XDG_CURRENT_DESKTOP", "KDE"), ("XDG_SESSION_DESKTOP", "plasma")]), Backend::XWaylandAuto);
        // SA_BACKEND wins: x11 is XWayland as asked (no note of her own), wayland never falls back.
        assert_eq!(pick(&[wl, x, ("SA_BACKEND", "x11")]), Backend::XWayland);
        assert_eq!(pick(&[wl, hypr, x, ("SA_BACKEND", "x11")]), Backend::XWayland);
        assert_eq!(pick(&[wl, ("SA_BACKEND", "x11")]), Backend::XWayland, "asked for without XWayland: X11's connect says why");
        assert_eq!(pick(&[wl, x, ("SA_BACKEND", "wayland")]), Backend::Wayland);
        assert_eq!(pick(&[wl, x, ("SA_BACKEND", " x11\n")]), Backend::XWayland, "trimmed");
        assert_eq!(pick(&[wl, hypr, x, ("SA_BACKEND", "X11")]), Backend::XWayland, "in any case");
        assert_eq!(pick(&[wl, x, ("SA_BACKEND", "Wayland")]), Backend::Wayland);
        assert_eq!(pick(&[wl, x, ("XDG_CURRENT_DESKTOP", "Hyprland"), ("SA_BACKEND", "x11")]), Backend::XWayland);
        // wayland with no Wayland session: no fallback to X11.
        assert_eq!(pick(&[x, ("SA_BACKEND", "wayland")]), Backend::NoWayland);
        assert_eq!(pick(&[("WAYLAND_DISPLAY", ""), x, ("SA_BACKEND", "WAYLAND")]), Backend::NoWayland);
        // A value she doesn't know (or a blank one) is ignored.
        assert_eq!(pick(&[wl, x, ("SA_BACKEND", "gnome")]), Backend::XWaylandAuto);
        assert_eq!(pick(&[wl, hypr, ("SA_BACKEND", "")]), Backend::Wayland);
        assert_eq!(pick(&[x, ("SA_BACKEND", "gnome")]), Backend::X11);
        // The ones process_setup names as ignored: set, not blank, not one she knows.
        assert_eq!(asked(OsStr::new("gnome")), None);
        assert_eq!(asked(OsStr::new(" ")), None);
        assert_eq!(asked(OsStr::new("Wayland ")), Some(Backend::Wayland));
    }

    #[test]
    fn base_dirs_are_xdgs() {
        fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
            move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
        }
        let home: &'static [(&str, &str)] = &[("HOME", "/home/h")];
        assert_eq!(base_dir_in(BaseDir::Config, env(home)), Some(PathBuf::from("/home/h/.config")));
        assert_eq!(base_dir_in(BaseDir::Data, env(home)), Some(PathBuf::from("/home/h/.local/share")));
        assert_eq!(base_dir_in(BaseDir::State, env(home)), Some(PathBuf::from("/home/h/.local/state")));
        // Each kind's own XDG variable wins over $HOME, and only for that kind.
        let xdg: &'static [(&str, &str)] =
            &[("HOME", "/home/h"), ("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/data"), ("XDG_STATE_HOME", "/state")];
        assert_eq!(base_dir_in(BaseDir::Config, env(xdg)), Some(PathBuf::from("/cfg")));
        assert_eq!(base_dir_in(BaseDir::Data, env(xdg)), Some(PathBuf::from("/data")));
        assert_eq!(base_dir_in(BaseDir::State, env(xdg)), Some(PathBuf::from("/state")));
        let data_only: &'static [(&str, &str)] = &[("HOME", "/home/h"), ("XDG_DATA_HOME", "/data")];
        assert_eq!(base_dir_in(BaseDir::Config, env(data_only)), Some(PathBuf::from("/home/h/.config")));
        assert_eq!(base_dir_in(BaseDir::State, env(data_only)), Some(PathBuf::from("/home/h/.local/state")));
        // XDG set, HOME unset: the XDG path; the kinds without one have none.
        let no_home: &'static [(&str, &str)] = &[("XDG_CONFIG_HOME", "/cfg"), ("XDG_STATE_HOME", "/state")];
        assert_eq!(base_dir_in(BaseDir::Config, env(no_home)), Some(PathBuf::from("/cfg")));
        assert_eq!(base_dir_in(BaseDir::State, env(no_home)), Some(PathBuf::from("/state")));
        assert_eq!(base_dir_in(BaseDir::Data, env(no_home)), None);
        // Pinned as it is (the pre-P1c lookup did the same): a SET but EMPTY XDG value is taken
        // as given, an empty path (her files then resolve against the working directory), not as
        // unset. The XDG spec says to treat it as unset; changing that is a behaviour change.
        let empty: &'static [(&str, &str)] = &[("HOME", "/home/h"), ("XDG_CONFIG_HOME", ""), ("XDG_STATE_HOME", "")];
        assert_eq!(base_dir_in(BaseDir::Config, env(empty)), Some(PathBuf::new()));
        assert_eq!(base_dir_in(BaseDir::State, env(empty)), Some(PathBuf::new()));
        assert_eq!(base_dir_in(BaseDir::Data, env(empty)), Some(PathBuf::from("/home/h/.local/share")));
        assert_eq!(base_dir_in(BaseDir::State, env(&[])), None);
    }
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, Value};

    fn sig(path: &str, iface: &str, member: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> zbus::Message {
        zbus::Message::signal(path, iface, member).unwrap().build(body).unwrap()
    }

    #[test]
    fn logind_signals_to_session_events() {
        let me = OwnedObjectPath::try_from("/org/freedesktop/login1/session/_32").unwrap();
        let other = "/org/freedesktop/login1/session/c7";
        let props = |path: &str, k: &str, v: bool| {
            let mut changed: HashMap<&str, Value> = HashMap::new();
            changed.insert(k, Value::from(v));
            sig(path, "org.freedesktop.DBus.Properties", "PropertiesChanged", &(LOGIND_SESSION, changed, Vec::<&str>::new()))
        };
        let ev = |m: &zbus::Message| session_event(m, &me);
        assert_eq!(ev(&props(me.as_str(), "LockedHint", true)), Some(Seen::Ev(SessionEv::Locked)));
        assert_eq!(ev(&props(me.as_str(), "LockedHint", false)), Some(Seen::Ev(SessionEv::Unlocked)));
        assert_eq!(ev(&props(other, "LockedHint", true)), None, "another session's lock");
        assert_eq!(ev(&props(me.as_str(), "IdleHint", true)), None, "only the lock");
        let invalid = sig(me.as_str(), "org.freedesktop.DBus.Properties", "PropertiesChanged", &(LOGIND_SESSION, HashMap::<&str, Value>::new(), vec!["LockedHint"]));
        assert_eq!(ev(&invalid), Some(Seen::Reread));
        assert_eq!(ev(&sig(me.as_str(), LOGIND_SESSION, "Lock", &())), Some(Seen::Ev(SessionEv::Locked)));
        assert_eq!(ev(&sig(me.as_str(), LOGIND_SESSION, "Unlock", &())), Some(Seen::Ev(SessionEv::Unlocked)));
        assert_eq!(ev(&sig(other, LOGIND_SESSION, "Lock", &())), None);
        let mgr = "/org/freedesktop/login1";
        assert_eq!(ev(&sig(mgr, LOGIND_MANAGER, "PrepareForSleep", &(true,))), Some(Seen::Ev(SessionEv::Sleeping)));
        assert_eq!(ev(&sig(mgr, LOGIND_MANAGER, "PrepareForSleep", &(false,))), Some(Seen::Ev(SessionEv::Resumed)));
        assert_eq!(ev(&sig(mgr, LOGIND_MANAGER, "SessionNew", &("c9", OwnedObjectPath::try_from(other).unwrap()))), None);
    }

    #[test]
    fn the_utc_offset_is_whole_minutes() {
        assert_eq!(utc_offset_s(1_772_953_199) % 60, 0);
    }
}
