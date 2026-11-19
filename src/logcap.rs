//! Her logs stay bounded while she runs (N3), not only at start. A log past `CAP` starts over: it
//! moves aside as `.old.log` (over the one there: one kept) and a new one takes its name.
//!
//! - Her own log, when her stderr is her file (Windows without a console: `synaps-avatar.log`;
//!   Linux: install.sh's `launch.log`): `keep_under` looks at its size every `EVERY` on a thread
//!   of its own (never her UI thread), and past the cap the platform moves her stderr (stdout
//!   with it, when it's the same file) into a new one (`platform::keep_log_under`).
//! - A child's output she keeps (the Windows voice sidecar's): through a pipe into `Capped`, a
//!   writer that starts over by itself (`pump`).

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A log starts over past this many bytes.
pub const CAP: u64 = 4 << 20;
/// How often her own log's size is looked at.
pub const EVERY: Duration = Duration::from_secs(30);

/// A log of `len` bytes (None: there's none) starts over? (At start: Windows' `synaps-avatar.log`.)
#[cfg_attr(not(windows), allow(dead_code))]
pub fn due(len: Option<u64>) -> bool {
    len.is_some_and(|n| n > CAP)
}

/// Where a log goes when it starts over: `x.log` → `x.old.log`.
pub fn old_of(path: &Path) -> PathBuf {
    path.with_extension("old.log")
}

/// Opened for appending (made if it isn't there).
pub fn open(path: &Path) -> io::Result<File> {
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

/// Starts `path` over: moved to its `.old.log` (over the one there), then a new one opened for
/// appending. A writer that holds the old one writes on into `.old.log` (Windows too: Rust opens
/// files sharing delete, so an open log can be renamed) until it's handed the new one.
pub fn start_over(path: &Path) -> io::Result<File> {
    std::fs::rename(path, old_of(path))?;
    open(path)
}

/// When her own log starts over (`keep_under`): past `at`, which is `CAP`, or, after a start-over
/// that failed, a cap past the size it failed at (tried again then, said once each time: never a
/// line every `EVERY`).
#[derive(Debug, PartialEq, Eq)]
pub struct Keeper {
    at: u64,
}

impl Default for Keeper {
    fn default() -> Keeper {
        Keeper { at: CAP }
    }
}

impl Keeper {
    /// Its size now (None: can't be read): start it over now?
    pub fn due(&mut self, len: Option<u64>) -> bool {
        let Some(n) = len else { return false };
        if n < self.at.saturating_sub(CAP) {
            // Smaller than when it failed (someone else started it over): the cap again.
            self.at = CAP;
        }
        n > self.at
    }

    /// The start-over at size `len` went through (`ok`), or didn't.
    pub fn done(&mut self, ok: bool, len: u64) {
        self.at = if ok { CAP } else { len.saturating_add(CAP) };
    }
}

/// Her own log, kept under `CAP` while she runs: a thread of its own looks at its size (`len`)
/// every `EVERY`, and past the cap calls `swap`, which starts it over and hands back what to let
/// go of a tick later (Windows: her old stderr handle, which a write under way may still use).
pub fn keep_under<H: Send + 'static>(
    path: PathBuf,
    len: impl Fn() -> Option<u64> + Send + 'static,
    mut swap: impl FnMut(&Path) -> io::Result<H> + Send + 'static,
) {
    let spawned = std::thread::Builder::new().name("log-cap".into()).spawn(move || {
        let mut keeper = Keeper::default();
        let mut held: Option<H> = None;
        loop {
            std::thread::sleep(EVERY);
            drop(held.take());
            let n = len();
            if !keeper.due(n) {
                continue;
            }
            let n = n.unwrap_or(0);
            match swap(&path) {
                Ok(h) => {
                    held = Some(h);
                    keeper.done(true, n);
                    // The new log's first line: who wrote it, and where the rest went.
                    elog!(
                        "--- synaps-avatar {} (pid {}): her log started over past {} MB, the earlier part is {}",
                        env!("CARGO_PKG_VERSION"),
                        std::process::id(),
                        CAP >> 20,
                        old_of(&path).display()
                    );
                }
                Err(e) => {
                    keeper.done(false, n);
                    elog!("synaps-avatar: her log is past {} MB and can't start over ({e}): tried again a cap later", CAP >> 20);
                }
            }
        }
    });
    if let Err(e) = spawned {
        elog!("synaps-avatar: her log: no thread to keep it under {} MB ({e})", CAP >> 20);
    }
}

/// A log file that starts over by itself past its cap, as it's written: a write that finds it past
/// the cap moves it aside first (`start_over`). One that can't goes on in the same file, and tries
/// again a cap later.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Capped {
    path: PathBuf,
    file: File,
    /// Written since it (re)opened, plus what it had then.
    len: u64,
    /// Starts over past this.
    at: u64,
    cap: u64,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Capped {
    /// `path` (its folder made), appended to; already past `cap`: started over first.
    pub fn open(path: &Path, cap: u64) -> io::Result<Capped> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut at = cap;
        let file = match std::fs::metadata(path).map(|m| m.len()) {
            Ok(n) if n > cap => start_over(path).or_else(|_| {
                at = n.saturating_add(cap);
                open(path)
            })?,
            _ => open(path)?,
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Capped { path: path.to_path_buf(), file, len, at, cap })
    }
}

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.len > self.at {
            match start_over(&self.path) {
                Ok(f) => {
                    self.file = f;
                    self.len = 0;
                    self.at = self.cap;
                }
                Err(_) => self.at = self.len.saturating_add(self.cap),
            }
        }
        let n = self.file.write(buf)?;
        self.len += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Copies `from` into `to` until it ends (the child's last handle on the pipe closed). A write
/// that fails is dropped and the copy goes on: the child never blocks on a full pipe.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pump(mut from: impl Read, mut to: impl Write) {
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                let _ = to.write_all(&buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sa-logcap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn a_log_starts_over_only_past_the_cap() {
        assert!(!due(None));
        assert!(!due(Some(CAP)));
        assert!(due(Some(CAP + 1)));
        assert_eq!(old_of(Path::new("/s/launch.log")), Path::new("/s/launch.old.log"));
        assert_eq!(old_of(Path::new("/s/synaps-avatar.log")), Path::new("/s/synaps-avatar.old.log"));
        assert_eq!(old_of(Path::new("/s/capri-voice.log")), Path::new("/s/capri-voice.old.log"));
    }

    /// Her own log: at the cap, not before; a start-over that failed is tried again a cap later
    /// (not every look), and one that went through goes back to the cap.
    #[test]
    fn her_own_log_starts_over_once_past_the_cap() {
        let mut k = Keeper::default();
        assert!(!k.due(None), "can't be read: left be");
        assert!(!k.due(Some(10)));
        assert!(!k.due(Some(CAP)));
        assert!(k.due(Some(CAP + 1)));
        k.done(false, CAP + 1);
        assert!(!k.due(Some(CAP + 500)), "failed: not again at the next look");
        assert!(!k.due(Some(2 * CAP + 1)));
        assert!(k.due(Some(2 * CAP + 2)), "a cap past where it failed: tried again");
        k.done(true, 2 * CAP + 2);
        assert!(!k.due(Some(100)));
        assert!(k.due(Some(CAP + 1)), "after one that went through: the cap again");
        k.done(false, 3 * CAP);
        assert!(!k.due(Some(10)), "smaller (someone else started it over): left be");
        assert_eq!(k, Keeper::default(), "and the cap again");
    }

    /// A writer that holds a log while it starts over writes on into `.old.log` (on Windows too:
    /// the rename of a file open for writing goes through), the new one is empty, and the next
    /// start-over replaces that `.old.log` (one kept).
    #[test]
    fn an_open_log_starts_over_under_its_writer() {
        let d = dir("open");
        let p = d.join("her.log");
        let mut held = open(&p).unwrap();
        held.write_all(b"first\n").unwrap();
        let mut new = start_over(&p).unwrap();
        held.write_all(b"still the old one\n").unwrap();
        new.write_all(b"second\n").unwrap();
        assert_eq!(read(&old_of(&p)), "first\nstill the old one\n");
        assert_eq!(read(&p), "second\n");
        drop(held);
        let mut third = start_over(&p).unwrap();
        third.write_all(b"third\n").unwrap();
        assert_eq!(read(&old_of(&p)), "second\n", "one kept");
        assert_eq!(read(&p), "third\n");
        drop((new, third));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The sidecar's log (through `pump`): past its cap it starts over at the next write, keeps one
    /// old one, and never loses a byte of what came through the pipe since.
    #[test]
    fn a_capped_log_starts_over_as_it_is_written() {
        let d = dir("capped");
        let p = d.join("sub").join("capri-voice.log");
        let mut c = Capped::open(&p, 10).unwrap();
        c.write_all(b"0123456789").unwrap();
        c.write_all(b"A").unwrap();
        assert_eq!(read(&p), "0123456789A", "at the cap: not yet");
        c.write_all(b"B").unwrap();
        assert_eq!((read(&old_of(&p)).as_str(), read(&p).as_str()), ("0123456789A", "B"));
        pump(&b"xxxxxxxxxxyyyy"[..], &mut c);
        pump(&b"z"[..], &mut c);
        assert_eq!((read(&old_of(&p)).as_str(), read(&p).as_str()), ("Bxxxxxxxxxxyyyy", "z"));
        drop(c);
        // Opened again past its cap: started over before the first write.
        std::fs::write(&p, "a long one from before").unwrap();
        let mut c = Capped::open(&p, 10).unwrap();
        c.write_all(b"new").unwrap();
        assert_eq!((read(&old_of(&p)).as_str(), read(&p).as_str()), ("a long one from before", "new"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A start-over that can't (here: `.old.log` is a folder, so the rename fails) goes on in the
    /// same file, and tries again a cap later.
    #[test]
    fn a_capped_log_that_cant_start_over_goes_on() {
        let d = dir("stuck");
        let p = d.join("capri-voice.log");
        std::fs::create_dir_all(d.join("capri-voice.old.log").join("in the way")).unwrap();
        let mut c = Capped::open(&p, 4).unwrap();
        c.write_all(b"12345").unwrap();
        c.write_all(b"6").unwrap();
        assert_eq!(read(&p), "123456", "failed: the same file");
        assert_eq!(c.at, 9, "tried again a cap later");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A reader that fails, or a pipe that's closed: the copy ends.
    #[test]
    fn the_pump_ends_with_its_pipe() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("gone"))
            }
        }
        let mut out = Vec::new();
        pump(Broken, &mut out);
        assert!(out.is_empty());
        let (mut r, mut w) = std::io::pipe().unwrap();
        let t = std::thread::spawn(move || {
            let mut out = Vec::new();
            pump(&mut r, &mut out);
            out
        });
        w.write_all(b"line\n").unwrap();
        drop(w);
        assert_eq!(t.join().unwrap(), b"line\n");
    }
}
