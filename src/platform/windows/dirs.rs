//! Where her files go on Windows (`super::super::BaseDir`), and her log when she has no console.
//!
//! `$XDG_<KIND>_HOME` wins, as on Linux (scripts and tests set it); else settings go under
//! `%APPDATA%` (they roam with him) and packs, state and the log under `%LOCALAPPDATA%`. The
//! installer (`packaging/windows`) puts her pack where she looks by default:
//! `%LOCALAPPDATA%\synaps-avatar\packs\capri`.

use super::super::BaseDir;
use std::ffi::OsString;
use std::path::PathBuf;

/// The log starts over past this (at start): the previous one is kept as `.old.log`.
const LOG_KEEP: u64 = 4 << 20;

pub fn base_dir(d: BaseDir) -> Option<PathBuf> {
    base_dir_in(d, |k| std::env::var_os(k))
}

fn base_dir_in(d: BaseDir, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let known = match d {
        BaseDir::Config => "APPDATA",
        BaseDir::Data | BaseDir::State => "LOCALAPPDATA",
    };
    var(d.xdg_var()).or_else(|| var(known)).map(PathBuf::from)
}

/// His profile folder: `~` in a path setting.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Started without a console (the installed build is a windowed one: the Start menu, the Run
/// key), her stderr and stdout go to `synaps-avatar.log` beside her state, appended. Started from
/// a terminal (or with either redirected), nothing changes. Rust's stdio reads the std handle at
/// each write, so swapping it here, first thing in main, takes for every later line.
pub fn log_without_console() {
    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    use std::os::windows::io::IntoRawHandle as _;
    // Err: no handle (null), as a windowed process gets from Explorer.
    if unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok() {
        return;
    }
    let Some(dir) = base_dir(BaseDir::State).map(|b| b.join("synaps-avatar")) else { return };
    let path = dir.join("synaps-avatar.log");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if starts_over(std::fs::metadata(&path).map(|m| m.len()).ok()) {
        // A running instance may hold it: Rust opens files sharing delete, so the rename goes
        // through and that one keeps writing to the old file.
        std::fs::rename(&path, dir.join("synaps-avatar.old.log")).ok();
    }
    let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    // Hers for the life of the process.
    let h = HANDLE(f.into_raw_handle());
    unsafe {
        SetStdHandle(STD_ERROR_HANDLE, h).ok();
        if GetStdHandle(STD_OUTPUT_HANDLE).is_err() {
            SetStdHandle(STD_OUTPUT_HANDLE, h).ok();
        }
    }
    elog!("--- synaps-avatar {} (pid {}): no console, logging here", env!("CARGO_PKG_VERSION"), std::process::id());
}

/// The log at start is `len` bytes (None: there is none): start a new one?
fn starts_over(len: Option<u64>) -> bool {
    len.is_some_and(|n| n > LOG_KEEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn settings_roam_packs_and_state_stay_local() {
        let vars = [("APPDATA", r"C:\Users\h\AppData\Roaming"), ("LOCALAPPDATA", r"C:\Users\h\AppData\Local")];
        assert_eq!(base_dir_in(BaseDir::Config, env(&vars)), Some(PathBuf::from(r"C:\Users\h\AppData\Roaming")));
        assert_eq!(base_dir_in(BaseDir::Data, env(&vars)), Some(PathBuf::from(r"C:\Users\h\AppData\Local")));
        assert_eq!(base_dir_in(BaseDir::State, env(&vars)), Some(PathBuf::from(r"C:\Users\h\AppData\Local")));
        // The pack she loads by default is where the installer puts it.
        let pack = base_dir_in(BaseDir::Data, env(&vars)).unwrap().join("synaps-avatar/packs/capri");
        assert!(pack.starts_with(r"C:\Users\h\AppData\Local\synaps-avatar"));
    }

    #[test]
    fn xdg_wins_and_nothing_is_none() {
        let vars = [("XDG_STATE_HOME", r"D:\state"), ("LOCALAPPDATA", r"C:\L")];
        assert_eq!(base_dir_in(BaseDir::State, env(&vars)), Some(PathBuf::from(r"D:\state")));
        assert_eq!(base_dir_in(BaseDir::Data, env(&vars)), Some(PathBuf::from(r"C:\L")));
        assert_eq!(base_dir_in(BaseDir::Config, env(&[])), None);
    }

    #[test]
    fn the_log_starts_over_only_when_big() {
        assert!(!starts_over(None));
        assert!(!starts_over(Some(LOG_KEEP)));
        assert!(starts_over(Some(LOG_KEEP + 1)));
    }
}
