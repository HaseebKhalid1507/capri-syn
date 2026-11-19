//! The Linux process: what has to be set before GPUI starts (GPU choice, the allocator), and
//! the session bus's services (media players, notifications, logind, a tray icon some day).
//! Shared by every Linux backend (X11, Hyprland).

mod media;
mod notifications;

pub use media::watch_media;
pub use notifications::notify;

use super::BaseDir;
use std::ffi::OsString;
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
pub fn voice_sidecar(_on: bool) {}

/// Process setup that has to happen before any other thread exists. Each knob was
/// measured on bella (release, `ps` RSS 6 s after start, see LOG.md):
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
    // SA_BACKEND=x11 on a Wayland desktop: GPUI and we both take X11 (XWayland).
    if std::env::var("SA_BACKEND").is_ok_and(|v| v == "x11") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            elog!("synaps-avatar: SA_BACKEND=x11 on Hyprland: through XWayland she sees X clients only, and can't stay above native windows");
        }
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

/// Another process's environment variable (his own processes: `/proc/<pid>/environ`). None when
/// that environment can't be read (gone, not his); Some(None) when the variable isn't set or is
/// blank.
pub fn process_env(pid: u32, key: &str) -> Option<Option<String>> {
    let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let want = format!("{key}=");
    Some(
        env.split(|&b| b == 0)
            .find_map(|kv| kv.strip_prefix(want.as_bytes()))
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .filter(|v| !v.trim().is_empty()),
    )
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
            .spawn()
            .expect("sleep");
        let pid = c.id();
        // Until its exec the child's environ is still ours: give it a moment.
        let t0 = std::time::Instant::now();
        while process_env(pid, "SA_TEST_KEY").flatten().is_none() && t0.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(process_env(pid, "SA_TEST_KEY"), Some(Some("sk-x".into())));
        assert_eq!(process_env(pid, "SA_TEST_EMPTY"), Some(None), "blank is unset");
        assert_eq!(process_env(pid, "SA_TEST_KE"), Some(None), "a whole name, not a prefix");
        c.kill().ok();
        c.wait().ok();
        assert_eq!(process_env(pid, "SA_TEST_KEY"), None, "gone: can't be read");
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
