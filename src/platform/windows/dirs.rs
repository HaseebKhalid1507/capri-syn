//! Where her files go on Windows (`super::super::BaseDir`), and her log when she has no console:
//! started over past `logcap::CAP` at start, and while she runs (`keep_log_under`).
//!
//! `$XDG_<KIND>_HOME` wins, as on Linux (scripts and tests set it); else settings go under
//! `%APPDATA%` (they roam with him) and packs, state and the log under `%LOCALAPPDATA%`. The
//! installer (`packaging/windows`) puts her pack where she looks by default:
//! `%LOCALAPPDATA%\synaps-avatar\packs\capri`.

use super::super::BaseDir;
use ::windows::Win32::Foundation::HANDLE;
use ::windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, IntoRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Her log without a console (`log_without_console`): where it is, and whether her stdout went
/// there too (it had no handle either).
static OWN: OnceLock<(PathBuf, bool)> = OnceLock::new();

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
    // Err: no handle (null), as a windowed process gets from Explorer.
    if unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok() {
        return;
    }
    let Some(dir) = base_dir(BaseDir::State).map(|b| b.join("synaps-avatar")) else { return };
    let path = dir.join("synaps-avatar.log");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if crate::logcap::due(std::fs::metadata(&path).map(|m| m.len()).ok()) {
        // A running instance may hold it: Rust opens files sharing delete, so the rename goes
        // through and that one keeps writing to the old file.
        std::fs::rename(&path, crate::logcap::old_of(&path)).ok();
    }
    let Ok(f) = crate::logcap::open(&path) else { return };
    // Hers for the life of the process (or until it starts over: `keep_log_under`).
    let h = HANDLE(f.into_raw_handle());
    let out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.is_err();
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, h) }.is_err() {
        // SAFETY: just opened, handed to no one.
        drop(unsafe { OwnedHandle::from_raw_handle(h.0) });
        return;
    }
    if out {
        unsafe { SetStdHandle(STD_OUTPUT_HANDLE, h) }.ok();
    }
    OWN.set((path, out)).ok();
    elog!("--- synaps-avatar {} (pid {}): no console, logging here", env!("CARGO_PKG_VERSION"), std::process::id());
}

/// Whether someone reads her stderr as she writes it: a console, or a redirect (a script's). Her
/// own log file (no console): no one does (`crate::startfail`).
pub fn stderr_seen() -> bool {
    OWN.get().is_none()
}

/// The file her stderr goes to, when it's her own log (no console).
pub fn stderr_file() -> Option<PathBuf> {
    OWN.get().map(|(p, _)| p.clone())
}

/// Her log kept under `logcap::CAP` while she runs (`logcap::keep_under`), when it's her own (no
/// console): her stderr (stdout too, when it went there) moved into a new file. A console or a
/// redirect is left as it is.
pub fn keep_log_under() {
    let Some((log, out)) = OWN.get().cloned() else { return };
    crate::logcap::keep_under(log, stderr_len, move |log| into_new_log(log, out));
}

/// The size of the file her stderr writes now (her own handle's: what she wrote, wherever the
/// file's name went).
fn stderr_len() -> Option<u64> {
    let h = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.ok()?;
    // SAFETY: her stderr handle, borrowed: ManuallyDrop never closes it.
    let f = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(h.0) });
    f.metadata().ok().map(|m| m.len())
}

/// `log` started over (`logcap::start_over`), her stderr (and stdout, `out`) moved into the new
/// one. Rust's stdio reads the std handle at each write, so the next line goes there. The old
/// handle comes back, for `keep_under` to close a tick later: a write that read it just before
/// the swap may still be using it.
fn into_new_log(log: &Path, out: bool) -> std::io::Result<OwnedHandle> {
    let old = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.map_err(std::io::Error::other)?;
    let new = OwnedHandle::from(crate::logcap::start_over(log)?);
    let h = HANDLE(new.as_raw_handle());
    unsafe { SetStdHandle(STD_ERROR_HANDLE, h) }.map_err(std::io::Error::other)?;
    if out {
        unsafe { SetStdHandle(STD_OUTPUT_HANDLE, h) }.ok();
    }
    // Hers now, for the life of the process (or the next start-over).
    let _ = new.into_raw_handle();
    // SAFETY: her stderr's handle until just now, opened by `log_without_console` or an earlier
    // start-over, and no longer hers to keep.
    Ok(unsafe { OwnedHandle::from_raw_handle(old.0) })
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

    /// Under `cargo test` her stderr is the runner's (a console or a pipe): no log of her own, so
    /// nothing kept under a cap and no failure said on screen.
    #[test]
    fn a_console_or_a_redirect_is_not_her_log() {
        assert!(stderr_seen() && stderr_file().is_none());
    }
}
