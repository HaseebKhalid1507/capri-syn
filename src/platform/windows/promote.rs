//! Her icon out of Windows 11's overflow, once. A new tray icon starts in the `^` flyout there (v1.0.0:
//! "doesn't see her in the tray"). Where an icon sits is one DWORD, `IsPromoted`, in its own subkey of
//! `HKCU\Control Panel\NotifyIconSettings`: Explorer writes it when he moves the icon or flips its
//! switch in Settings, and reads it live.
//!
//! - After her icon went in the first time (`tray::try_add`), a thread of its own looks for her subkey
//!   (never the tray's or the UI's: Explorer writes it a moment after `Shell_NotifyIconW` returns), a few
//!   times over `WAIT`. Hers: `ExecutablePath` is her exe's (Explorer may write a known folder's GUID for
//!   its start: `{F1B32785-…}\Programs\…`), and `IconGuid`, where the subkey has one, is her icon's.
//! - Once only. That it was done is a marker file in her state folder (`tray-promoted`, beside her log;
//!   not settings.toml). With the marker, or an `IsPromoted` in her subkey already (his, or an earlier
//!   install's), she touches nothing: hide her later and she stays hidden.
//! - Windows 10 has no such key (its icons are a binary blob, `IconStreams`): nothing done, no marker.
//!   No marker either where no subkey of hers showed up: the next start looks again.

use super::super::BaseDir;
use ::windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use ::windows::Win32::System::Com::CoTaskMemFree;
use ::windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SAM_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use ::windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use ::windows::core::{GUID, HSTRING, PCWSTR, PWSTR, w};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The shell's per-icon settings, one subkey each.
const KEY: &str = r"Control Panel\NotifyIconSettings";
/// Her marker, in her state folder.
const MARKER: &str = "tray-promoted";
/// What the marker says when she was the one who set it (its first words).
const SET_NOTE: &str = "she put her icon on the taskbar";
/// Before each look after the first (Explorer writes her subkey a moment after her add): ten seconds in all.
const WAIT: [Duration; 4] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(3), Duration::from_secs(4)];

/// How it ended, for her log.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// `IsPromoted` = 1 written, marker made.
    Shown,
    /// His say is there already (`IsPromoted` set, an earlier install's or his): left as it is.
    LeftAsSet,
    /// Done on an earlier start (the marker): nothing written, her subkey only read, for her log (does
    /// Explorer keep what she set?). `set`: the marker says she wrote the 1 (not that it was his already).
    Marked { now: Now, set: bool },
    /// The key is there but no subkey of hers came in `WAIT`: the next start looks again.
    NoEntry,
    /// No key at all (Windows 10): nothing to do.
    NoKey,
    /// Her subkey wouldn't take the value (the registry's error): the next start tries again.
    Failed(u32),
}

/// Her subkey as a later start finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Now {
    /// `IsPromoted` is this.
    Value(u32),
    /// Her subkey is there, `IsPromoted` isn't.
    Unset,
    /// Her subkey isn't there.
    NoEntry,
    /// The key isn't (or can't be read).
    NoKey,
}

impl Outcome {
    fn line(&self) -> String {
        match self {
            Outcome::Shown => "synaps-avatar: tray: shown on the taskbar (first run)".into(),
            Outcome::LeftAsSet => "synaps-avatar: tray: left as he set it".into(),
            Outcome::Marked { now, set } => {
                let then = if *set { "she set 1 on her first run" } else { "it was his already on her first run" };
                match now {
                    Now::Value(n) => format!("synaps-avatar: tray: left as he set it (IsPromoted now {n}; {then})"),
                    Now::Unset => format!("synaps-avatar: tray: left as he set it (IsPromoted now unset; {then})"),
                    Now::NoEntry => format!("synaps-avatar: tray: left as he set it (no entry of hers now; {then})"),
                    Now::NoKey => format!("synaps-avatar: tray: left as he set it (no NotifyIconSettings key now; {then})"),
                }
            }
            Outcome::NoEntry => "synaps-avatar: tray: no NotifyIconSettings entry yet; she looks again at her next start".into(),
            Outcome::NoKey => "synaps-avatar: tray: no NotifyIconSettings key (Windows 10's tray has none): nothing to do".into(),
            Outcome::Failed(e) => format!("synaps-avatar: tray: couldn't set IsPromoted (error {e}); she tries again at her next start"),
        }
    }
}

/// One subkey of `KEY`, as far as her match needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    /// `ExecutablePath`, as written.
    exe: String,
    /// `IconGuid`, where there is one.
    guid: Option<String>,
    /// `IsPromoted`: unset till he (or she) says.
    promoted: Option<u32>,
}

/// The registry and the clock, as `promote` needs them (a script stands in for both in the tests).
trait Settings {
    /// Every subkey of `KEY` with its `ExecutablePath` written. None: no such key.
    fn entries(&mut self) -> Option<Vec<Entry>>;
    /// A known folder's path (`ExecutablePath` may begin with its GUID).
    fn folder(&self, id: u128) -> Option<String>;
    /// `IsPromoted` = 1 in subkey `name`; Err: the registry's error.
    fn promote(&mut self, name: &str) -> Result<(), u32>;
    fn wait(&mut self, d: Duration);
}

/// Her icon's subkey, once: `Shown`, `LeftAsSet`, or what kept it from being either. The marker comes
/// first (with it nothing is written: her subkey is read once, for her log), and is made only once her
/// subkey was looked at.
fn promote(s: &mut impl Settings, marker: &Path, exe: &str, guid: u128) -> Outcome {
    if marker.exists() {
        let set = std::fs::read_to_string(marker).is_ok_and(|t| t.starts_with(SET_NOTE));
        return Outcome::Marked { now: look(s, exe, guid), set };
    }
    let mut key = false;
    for gap in std::iter::once(Duration::ZERO).chain(WAIT) {
        if !gap.is_zero() {
            s.wait(gap);
        }
        let Some(all) = s.entries() else { continue };
        key = true;
        let mine: Vec<&Entry> = all.iter().filter(|e| is_hers(e, exe, guid, &|id| s.folder(id))).collect();
        if mine.is_empty() {
            continue;
        }
        let mut shown = false;
        for e in mine {
            // A value there is his (or an earlier install's): never overwritten.
            if e.promoted.is_some() {
                continue;
            }
            match s.promote(&e.name) {
                Ok(()) => shown = true,
                Err(code) => return Outcome::Failed(code),
            }
        }
        mark(marker, if shown { SET_NOTE } else { "her icon's setting was his already" });
        return if shown { Outcome::Shown } else { Outcome::LeftAsSet };
    }
    if key { Outcome::NoEntry } else { Outcome::NoKey }
}

/// Her subkey now, one look and nothing written.
fn look(s: &mut impl Settings, exe: &str, guid: u128) -> Now {
    let Some(all) = s.entries() else { return Now::NoKey };
    let mine: Vec<&Entry> = all.iter().filter(|e| is_hers(e, exe, guid, &|id| s.folder(id))).collect();
    match mine.first() {
        None => Now::NoEntry,
        Some(e) => e.promoted.map_or(Now::Unset, Now::Value),
    }
}

/// `e` is her icon's subkey: her exe's path, and her icon's GUID where the subkey says one (another of
/// her, by another instance, has one of its own; a subkey of the shell's window-id kind has none).
fn is_hers(e: &Entry, exe: &str, guid: u128, folder: &dyn Fn(u128) -> Option<String>) -> bool {
    same_exe(&e.exe, exe, folder) && e.guid.as_deref().and_then(guid_of).is_none_or(|g| g == guid)
}

/// `ExecutablePath` `value` is `exe`: the same full path, case aside; a known folder's GUID at its start
/// is that folder's path (`folder`; one it doesn't know is no match).
fn same_exe(value: &str, exe: &str, folder: &dyn Fn(u128) -> Option<String>) -> bool {
    let v = value.trim();
    let full = match v.strip_prefix('{').and_then(|r| r.split_once('}')) {
        Some((id, rest)) => match guid_of(id) {
            Some(id) => match folder(id) {
                Some(dir) => format!("{dir}{rest}"),
                None => return false,
            },
            None => v.to_string(),
        },
        None => v.to_string(),
    };
    norm(&full) == norm(exe)
}

/// A path to compare: no `\\?\`, backslashes, none trailing, lower case.
fn norm(p: &str) -> String {
    p.trim().trim_start_matches(r"\\?\").replace('/', r"\").trim_end_matches('\\').to_lowercase()
}

/// A GUID's text (`{…}` or bare, hyphens or none) as its number.
fn guid_of(s: &str) -> Option<u128> {
    let hex: String = s.trim().trim_matches(['{', '}']).chars().filter(|&c| c != '-').collect();
    if hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) { u128::from_str_radix(&hex, 16).ok() } else { None }
}

/// The marker: it was done, and says so for whoever finds it.
fn mark(marker: &Path, what: &str) {
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let note = format!("{what}; once only. Delete this file and she does it again at her next start.\n");
    if let Err(e) = std::fs::write(marker, note) {
        elog!("synaps-avatar: tray: no marker at {} ({e}); she looks again at her next start", marker.display());
    }
}

/// Her marker's path: her state folder (`state::path`'s, beside her log).
fn marker_path() -> Option<PathBuf> {
    Some(super::dirs::base_dir(BaseDir::State)?.join("synaps-avatar").join(MARKER))
}

/// The key `path` of HKCU, as `Settings`.
struct Registry {
    path: String,
}

impl Registry {
    fn hers() -> Self {
        Registry { path: KEY.to_string() }
    }

    /// `path`'s subkey `sub` (or itself, empty), opened with `sam`.
    fn open(&self, sub: &str, sam: REG_SAM_FLAGS) -> Result<HKEY, u32> {
        let path = HSTRING::from(if sub.is_empty() { self.path.clone() } else { format!(r"{}\{sub}", self.path) });
        let mut key = HKEY::default();
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, sam, &mut key) };
        if r == ERROR_SUCCESS { Ok(key) } else { Err(r.0) }
    }
}

impl Settings for Registry {
    fn entries(&mut self) -> Option<Vec<Entry>> {
        let key = self.open("", KEY_READ).ok()?;
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut n = name.len() as u32;
            let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR::from_raw(name.as_mut_ptr())), &mut n, None, None, None, None) };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            // A name too long for her buffer is no subkey of the shell's: next. Any other error ends it.
            if r == ERROR_MORE_DATA {
                continue;
            }
            if r != ERROR_SUCCESS {
                break;
            }
            let name = String::from_utf16_lossy(&name[..n as usize]);
            let sub = HSTRING::from(name.as_str());
            // Not written yet (Explorer makes the subkey, then its values): the next look.
            let Some(exe) = read_sz(key, &sub, w!("ExecutablePath")) else { continue };
            out.push(Entry { name, exe, guid: read_sz(key, &sub, w!("IconGuid")), promoted: read_dword(key, &sub, w!("IsPromoted")) });
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
        Some(out)
    }

    fn folder(&self, id: u128) -> Option<String> {
        let id = GUID::from_u128(id);
        unsafe {
            let p = SHGetKnownFolderPath(&id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        }
    }

    fn promote(&mut self, name: &str) -> Result<(), u32> {
        let key = self.open(name, KEY_SET_VALUE)?;
        let r = unsafe { RegSetValueExW(key, w!("IsPromoted"), None, REG_DWORD, Some(&1u32.to_le_bytes())) };
        unsafe {
            let _ = RegCloseKey(key);
        }
        if r == ERROR_SUCCESS { Ok(()) } else { Err(r.0) }
    }

    fn wait(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// A REG_SZ value of `key`'s subkey `sub`. None: not there, or not a string.
fn read_sz(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<String> {
    let mut bytes = 0u32;
    if unsafe { RegGetValueW(key, sub, name, RRF_RT_REG_SZ, None, None, Some(&mut bytes as *mut u32)) } != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    let mut n = (buf.len() * 2) as u32;
    if unsafe { RegGetValueW(key, sub, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut core::ffi::c_void), Some(&mut n as *mut u32)) } != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// A REG_DWORD value of `key`'s subkey `sub`. None: not there, or not a DWORD.
fn read_dword(key: HKEY, sub: &HSTRING, name: PCWSTR) -> Option<u32> {
    let (mut v, mut n) = (0u32, 4u32);
    let r = unsafe { RegGetValueW(key, sub, name, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut core::ffi::c_void), Some(&mut n as *mut u32)) };
    (r == ERROR_SUCCESS).then_some(v)
}

/// Her promotion now, on this thread (it waits up to `WAIT`): her exe's subkey for her icon `guid`, by `marker`.
/// None: her exe's path isn't known.
pub(super) fn run(guid: u128, marker: &Path) -> Option<Outcome> {
    let exe = std::env::current_exe().ok()?.to_string_lossy().into_owned();
    Some(promote(&mut Registry::hers(), marker, &exe, guid))
}

/// Her icon went in the first time: it is promoted (or left) on a thread of its own, and her log says which.
pub(super) fn spawn(guid: u128) {
    let Some(marker) = marker_path() else {
        elog!("synaps-avatar: tray: no state folder for her marker; left as it is");
        return;
    };
    let t = std::thread::Builder::new().name("tray-promote".into()).spawn(move || match run(guid, &marker) {
        Some(o) => elog!("{}", o.line()),
        None => elog!("synaps-avatar: tray: her exe's path isn't known; left as it is"),
    });
    if let Err(e) = t {
        elog!("synaps-avatar: tray: no thread to promote her icon ({e}); left as it is");
    }
}

/// `IsPromoted` of her icon `guid`'s subkey now (a test reads it back); None: no such subkey, or none set.
#[cfg(test)]
pub(super) fn promoted(guid: u128) -> Option<u32> {
    let mut reg = Registry::hers();
    let exe = std::env::current_exe().ok()?.to_string_lossy().into_owned();
    let all = reg.entries()?;
    all.iter().find(|e| is_hers(e, &exe, guid, &|id| reg.folder(id))).and_then(|e| e.promoted)
}

/// Subkeys that look like hers by a looser rule than the match's: this exe's file name, and her icon's
/// GUID where the subkey has one. A live test fails if there are any and the match found none of them.
#[cfg(test)]
pub(super) fn lookalikes(guid: u128) -> Vec<String> {
    let name = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    let all = Registry::hers().entries().unwrap_or_default();
    let tail = format!(r"\{name}");
    all.iter()
        .filter(|e| !name.is_empty() && norm(&e.exe).ends_with(&tail))
        .filter(|e| e.guid.as_deref().and_then(guid_of).is_none_or(|g| g == guid))
        .map(|e| format!("{}: {} guid {:?} IsPromoted {:?}", e.name, e.exe, e.guid, e.promoted))
        .collect()
}

/// The subkeys of this exe's file name as the shell wrote them (a test's log when none was hers).
#[cfg(test)]
pub(super) fn seen() -> String {
    let name = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    let all = Registry::hers().entries().unwrap_or_default();
    let ours: Vec<String> = all.iter().filter(|e| norm(&e.exe).ends_with(&name)).map(|e| format!("{}: {} guid {:?} IsPromoted {:?}", e.name, e.exe, e.guid, e.promoted)).collect();
    format!("{} subkeys, of this exe: [{}]", all.len(), ours.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::windows::Win32::System::Registry::{REG_OPTION_NON_VOLATILE, REG_SZ, RegCreateKeyExW, RegDeleteTreeW};

    const EXE: &str = r"C:\Users\h\AppData\Local\Programs\synaps-avatar\synaps-avatar.exe";
    const HERS: u128 = 0x5a50_1a2b_3c4d_5e6f_8a9b_0c1d_2e3f_4a5b;
    const ANOTHER: u128 = 0x5a50_1a2b_3c4d_5e6f_8a9b_0c1d_2e3f_4a5c;
    /// FOLDERID_LocalAppData.
    const LOCAL: u128 = 0xf1b3_2785_6fba_4fcf_9d55_7b8e_7f15_7091;

    fn text(g: u128) -> String {
        format!("{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}", g >> 96, (g >> 80) & 0xffff, (g >> 64) & 0xffff, (g >> 48) & 0xffff, g & 0xffff_ffff_ffff)
    }

    fn entry(name: &str, exe: &str, guid: Option<u128>, promoted: Option<u32>) -> Entry {
        Entry { name: name.into(), exe: exe.into(), guid: guid.map(text), promoted }
    }

    /// A marker's path of a test's own, none yet.
    fn marker(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("capri-promote-{}-{tag}", std::process::id())).join(MARKER);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        p
    }

    fn clean(m: &Path) {
        let _ = std::fs::remove_dir_all(m.parent().unwrap());
    }

    /// The registry and the clock from a script: what each look finds (the last one repeats), what was
    /// written, the waits between looks.
    #[derive(Default)]
    struct Fake {
        looks: Vec<Option<Vec<Entry>>>,
        seen: usize,
        wrote: Vec<String>,
        waited: Vec<Duration>,
        refuse: Option<u32>,
    }

    impl Fake {
        fn finds(looks: Vec<Option<Vec<Entry>>>) -> Self {
            Fake { looks, ..Default::default() }
        }
    }

    impl Settings for Fake {
        fn entries(&mut self) -> Option<Vec<Entry>> {
            self.seen += 1;
            self.looks[(self.seen - 1).min(self.looks.len() - 1)].clone()
        }
        fn folder(&self, id: u128) -> Option<String> {
            (id == LOCAL).then(|| r"C:\Users\h\AppData\Local".to_string())
        }
        fn promote(&mut self, name: &str) -> Result<(), u32> {
            match self.refuse {
                Some(e) => Err(e),
                None => {
                    self.wrote.push(name.into());
                    Ok(())
                }
            }
        }
        fn wait(&mut self, d: Duration) {
            self.waited.push(d);
        }
    }

    #[test]
    fn her_exe_is_matched_whole_and_case_aside() {
        let none = |_: u128| -> Option<String> { None };
        assert!(same_exe(EXE, EXE, &none));
        assert!(same_exe(&EXE.to_uppercase(), EXE, &none), "case");
        assert!(same_exe(&EXE.replace('\\', "/"), EXE, &none), "slashes");
        assert!(same_exe(&format!(r"\\?\{EXE}"), EXE, &none), "verbatim");
        assert!(!same_exe(r"C:\Users\h\AppData\Local\Programs\synaps-avatar\other.exe", EXE, &none));
        assert!(!same_exe(r"synaps-avatar.exe", EXE, &none), "a name is not a path");
        assert!(!same_exe(r"D:\x\C:\Users\h\AppData\Local\Programs\synaps-avatar\synaps-avatar.exe", EXE, &none), "the whole path");
        assert!(!same_exe("", EXE, &none));
    }

    #[test]
    fn a_known_folders_guid_stands_for_its_path() {
        let local = |id: u128| -> Option<String> { (id == LOCAL).then(|| r"C:\Users\h\AppData\Local".to_string()) };
        let v = r"{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}\Programs\synaps-avatar\synaps-avatar.exe";
        assert!(same_exe(v, EXE, &local));
        assert!(same_exe(&v.to_lowercase(), EXE, &local), "the GUID's case");
        // A folder it doesn't know (or can't make): no match, never a guess.
        assert!(!same_exe(r"{6D809377-6AF0-444B-8957-A3773F02200E}\Programs\synaps-avatar\synaps-avatar.exe", EXE, &local));
        // Braces that aren't a GUID are part of the path.
        assert!(same_exe(r"{not a guid}\x.exe", r"{not a guid}\x.exe", &local));
    }

    #[test]
    fn a_guid_reads_with_or_without_braces() {
        assert_eq!(guid_of(&text(HERS)), Some(HERS));
        assert_eq!(guid_of(&text(HERS).to_lowercase()), Some(HERS));
        assert_eq!(guid_of(text(HERS).trim_matches(['{', '}'])), Some(HERS));
        assert_eq!(guid_of(&text(HERS).replace('-', "")), Some(HERS));
        for bad in ["", "{}", "{1234}", "not a guid at all, but 32 characters", "+0000000-0000-0000-0000-000000000000"] {
            assert_eq!(guid_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn her_subkey_is_her_exe_with_her_guid() {
        let none = |_: u128| -> Option<String> { None };
        assert!(is_hers(&entry("1", EXE, Some(HERS), None), EXE, HERS, &none));
        // Another of her (another instance's GUID), same exe: not hers.
        assert!(!is_hers(&entry("2", EXE, Some(ANOTHER), None), EXE, HERS, &none));
        // No GUID in the subkey (her window's id): hers by her exe.
        assert!(is_hers(&entry("3", EXE, None, None), EXE, HERS, &none));
        // A GUID it can't read is no GUID said.
        assert!(is_hers(&Entry { guid: Some("?".into()), ..entry("4", EXE, None, None) }, EXE, HERS, &none));
        // Her GUID under another exe: not hers.
        assert!(!is_hers(&entry("5", r"C:\other\x.exe", Some(HERS), None), EXE, HERS, &none));
    }

    #[test]
    fn it_is_done_once_and_the_marker_says_so() {
        let m = marker("once");
        let mine = || entry("1", EXE, Some(HERS), None);
        let mut s = Fake::finds(vec![Some(vec![entry("0", r"C:\other\x.exe", None, None), mine()])]);
        assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::Shown);
        assert_eq!(s.wrote, ["1"], "her subkey only");
        assert!(s.waited.is_empty(), "there at the first look: no waiting");
        assert!(m.is_file(), "the marker's made after the write");

        // The next start: the marker, and her subkey is only read (he may have hidden her since: a 0).
        let mut again = Fake::finds(vec![Some(vec![entry("1", EXE, Some(HERS), Some(0))])]);
        assert_eq!(promote(&mut again, &m, EXE, HERS), Outcome::Marked { now: Now::Value(0), set: true });
        assert_eq!((again.seen, again.wrote.len(), again.waited.len()), (1, 0, 0), "one look, no write, no waiting");

        // Deleted by hand: it does it again.
        std::fs::remove_file(&m).unwrap();
        assert_eq!(promote(&mut Fake::finds(vec![Some(vec![mine()])]), &m, EXE, HERS), Outcome::Shown);
        clean(&m);
    }

    #[test]
    fn a_later_start_only_reads_her_subkey_for_her_log() {
        let m = marker("later");
        mark(&m, SET_NOTE);
        let marked = |now| Outcome::Marked { now, set: true };
        let there = |p| Fake::finds(vec![Some(vec![entry("0", r"C:\other\x.exe", None, Some(0)), entry("1", EXE, Some(HERS), p)])]);
        for (p, now) in [(Some(1), Now::Value(1)), (Some(0), Now::Value(0)), (None, Now::Unset)] {
            let mut s = there(p);
            assert_eq!(promote(&mut s, &m, EXE, HERS), marked(now));
            assert!(s.wrote.is_empty(), "never written");
        }
        assert_eq!(promote(&mut Fake::finds(vec![Some(vec![entry("1", EXE, Some(ANOTHER), Some(1))])]), &m, EXE, HERS), marked(Now::NoEntry));
        assert_eq!(promote(&mut Fake::finds(vec![None]), &m, EXE, HERS), marked(Now::NoKey));
        // A marker that says her subkey was his: said so.
        mark(&m, "her icon's setting was his already");
        assert_eq!(promote(&mut there(Some(1)), &m, EXE, HERS), Outcome::Marked { now: Now::Value(1), set: false });
        // A marker with nothing of hers in it (made by hand): not hers to have set.
        std::fs::write(&m, "").unwrap();
        assert_eq!(promote(&mut there(Some(1)), &m, EXE, HERS), Outcome::Marked { now: Now::Value(1), set: false });
        clean(&m);
    }

    #[test]
    fn what_is_set_already_is_his() {
        for his in [0, 1, 2] {
            let m = marker("his");
            let mut s = Fake::finds(vec![Some(vec![entry("1", EXE, Some(HERS), Some(his))])]);
            assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::LeftAsSet, "IsPromoted = {his}");
            assert!(s.wrote.is_empty(), "never overwritten");
            assert!(m.is_file(), "and it is done: not looked at again");
            clean(&m);
        }
    }

    #[test]
    fn explorer_writes_her_subkey_a_moment_late() {
        let m = marker("late");
        let (none, there) = (Some(vec![entry("0", r"C:\other\x.exe", None, None)]), Some(vec![entry("1", EXE, Some(HERS), None)]));
        let mut s = Fake::finds(vec![none, Some(vec![]), there]);
        assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::Shown);
        assert_eq!(s.waited, [Duration::from_secs(1), Duration::from_secs(2)]);
        assert_eq!(s.wrote, ["1"]);
        clean(&m);
    }

    #[test]
    fn no_subkey_in_ten_seconds_leaves_no_marker() {
        let m = marker("never");
        let mut s = Fake::finds(vec![Some(vec![entry("0", r"C:\other\x.exe", None, None)])]);
        assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::NoEntry);
        assert_eq!(s.seen, 5, "a few looks");
        assert_eq!(s.waited.iter().sum::<Duration>(), Duration::from_secs(10), "over ten seconds");
        assert!(s.wrote.is_empty() && !m.exists(), "nothing written, nothing marked: the next start looks again");
        clean(&m);
    }

    #[test]
    fn windows_10_has_no_key_and_gets_no_marker() {
        let m = marker("w10");
        let mut s = Fake::finds(vec![None]);
        assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::NoKey);
        assert!(s.wrote.is_empty() && !m.exists());
        // The key made late (a first icon of all): looked for again.
        let mut late = Fake::finds(vec![None, Some(vec![entry("1", EXE, Some(HERS), None)])]);
        assert_eq!(promote(&mut late, &m, EXE, HERS), Outcome::Shown);
        clean(&m);
    }

    #[test]
    fn a_subkey_that_wont_take_it_is_tried_again_next_start() {
        let m = marker("refused");
        let mut s = Fake::finds(vec![Some(vec![entry("1", EXE, Some(HERS), None)])]);
        s.refuse = Some(5);
        assert_eq!(promote(&mut s, &m, EXE, HERS), Outcome::Failed(5));
        assert!(!m.exists());
        clean(&m);
    }

    #[test]
    fn her_log_says_which() {
        for (o, said) in [
            (Outcome::Shown, "tray: shown on the taskbar (first run)"),
            (Outcome::LeftAsSet, "tray: left as he set it"),
            (Outcome::NoEntry, "tray: no NotifyIconSettings entry yet"),
            (Outcome::Marked { now: Now::Value(0), set: true }, "tray: left as he set it (IsPromoted now 0; she set 1 on her first run)"),
            (Outcome::Marked { now: Now::Value(1), set: true }, "tray: left as he set it (IsPromoted now 1; she set 1 on her first run)"),
            (Outcome::Marked { now: Now::Unset, set: false }, "(IsPromoted now unset; it was his already on her first run)"),
            (Outcome::Marked { now: Now::NoEntry, set: true }, "(no entry of hers now;"),
            (Outcome::Marked { now: Now::NoKey, set: true }, "(no NotifyIconSettings key now;"),
        ] {
            assert!(o.line().contains(said), "{}", o.line());
        }
        assert!(Outcome::NoKey.line().contains("no NotifyIconSettings key"));
        assert!(Outcome::Failed(5).line().contains("error 5"));
    }

    /// A key of its own under HKCU, standing in for NotifyIconSettings, gone with the test.
    struct Scratch {
        root: String,
        path: String,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root = format!(r"Software\capri-syn-test-{}-{tag}", std::process::id());
            let s = Scratch { path: format!(r"{root}\NotifyIconSettings"), root };
            s.clear();
            s
        }

        fn clear(&self) {
            let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.root.as_str())) };
        }

        fn registry(&self) -> Registry {
            Registry { path: self.path.clone() }
        }

        /// `sub` of it (the key itself, empty), made and open for writing.
        fn create(&self, sub: &str) -> HKEY {
            let path = HSTRING::from(if sub.is_empty() { self.path.clone() } else { format!(r"{}\{sub}", self.path) });
            let mut key = HKEY::default();
            let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &path, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None) };
            assert_eq!(r, ERROR_SUCCESS, "no scratch key {path}");
            key
        }

        /// `sub` of it (the key itself, empty), made with nothing in it (Windows 11 before its first icon).
        fn empty(&self, sub: &str) {
            let _ = unsafe { RegCloseKey(self.create(sub)) };
        }

        /// A subkey as Explorer writes one.
        fn entry(&self, name: &str, exe: &str, guid: Option<u128>, promoted: Option<u32>) {
            let key = self.create(name);
            let sz = |v: &str| v.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
            unsafe {
                assert_eq!(RegSetValueExW(key, w!("ExecutablePath"), None, REG_SZ, Some(&sz(exe))), ERROR_SUCCESS);
                if let Some(g) = guid {
                    assert_eq!(RegSetValueExW(key, w!("IconGuid"), None, REG_SZ, Some(&sz(&text(g)))), ERROR_SUCCESS);
                }
                if let Some(p) = promoted {
                    assert_eq!(RegSetValueExW(key, w!("IsPromoted"), None, REG_DWORD, Some(&p.to_le_bytes())), ERROR_SUCCESS);
                }
                let _ = RegCloseKey(key);
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            self.clear();
        }
    }

    /// The registry's own calls, on a scratch key in the place of NotifyIconSettings: her subkey is
    /// found (by her exe's path, or a known folder's GUID for its start, and her icon's GUID), IsPromoted
    /// is a DWORD of 1 read back, and no other subkey, another instance's included, is touched.
    #[test]
    fn her_subkey_gets_a_dword_and_no_other_does() {
        let s = Scratch::new("subkeys");
        let local = s.registry().folder(LOCAL).expect("the shell knows %LOCALAPPDATA%");
        let exe = format!(r"{local}\Programs\synaps-avatar\synaps-avatar.exe");
        s.entry("1", &exe.to_uppercase(), Some(HERS), None);
        s.entry("2", &exe, Some(ANOTHER), None);
        s.entry("3", r"C:\other\x.exe", Some(HERS), None);
        s.entry("4", r"C:\other\y.exe", None, Some(0));
        let m = marker("registry");
        assert_eq!(promote(&mut s.registry(), &m, &exe, HERS), Outcome::Shown);
        let after: Vec<(String, Option<u32>)> = {
            let mut v: Vec<_> = s.registry().entries().unwrap().into_iter().map(|e| (e.name, e.promoted)).collect();
            v.sort();
            v
        };
        let want: Vec<(String, Option<u32>)> = vec![("1".to_string(), Some(1)), ("2".to_string(), None), ("3".to_string(), None), ("4".to_string(), Some(0))];
        assert_eq!(after, want);
        assert!(m.is_file());
        // Once only: hide her (IsPromoted = 0, as Settings does), and the next start leaves it.
        s.entry("1", &exe, Some(HERS), Some(0));
        assert_eq!(promote(&mut s.registry(), &m, &exe, HERS), Outcome::Marked { now: Now::Value(0), set: true });
        assert_eq!(s.registry().entries().unwrap().iter().find(|e| e.name == "1").unwrap().promoted, Some(0), "stays hidden");
        clean(&m);
    }

    /// Explorer's own spelling of an install under %LOCALAPPDATA%: its folder's GUID, then the rest.
    #[test]
    fn a_known_folders_guid_in_the_registry_is_hers_too() {
        let s = Scratch::new("known");
        let local = s.registry().folder(LOCAL).expect("the shell knows %LOCALAPPDATA%");
        let exe = format!(r"{local}\Programs\synaps-avatar\synaps-avatar.exe");
        s.entry("7", r"{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}\Programs\synaps-avatar\synaps-avatar.exe", Some(HERS), None);
        let m = marker("known");
        assert_eq!(promote(&mut s.registry(), &m, &exe, HERS), Outcome::Shown);
        assert_eq!(s.registry().entries().unwrap()[0].promoted, Some(1));
        clean(&m);
    }

    /// A subkey Explorer hasn't filled in yet (no ExecutablePath) is nobody's, and a key with no
    /// subkeys is looked at, not taken for Windows 10's missing one.
    #[test]
    fn an_unwritten_subkey_and_an_empty_key_are_not_hers() {
        let s = Scratch::new("empty");
        assert_eq!(s.registry().entries(), None, "no key: Windows 10");
        s.empty("");
        assert_eq!(s.registry().entries(), Some(vec![]), "a key, nothing in it yet");
        s.empty("9");
        assert_eq!(s.registry().entries(), Some(vec![]), "a subkey with no ExecutablePath is left for the next look");
    }
}
