//! She starts the voice sidecar on Windows (on Linux a systemd user unit runs it; she starts
//! nothing there). The bundle (`packaging/windows/voice-bundle.ps1`, put beside her by the voice
//! installer) runs as her child inside a job object that kills it when the job's last handle
//! closes: it dies with her, even when she is killed. It follows `[voice] enabled`. A sidecar that
//! exits is started again after 1, 2, 4 … 60 s (back to 1 s after a run of a minute). Its stdout
//! and stderr go to `%LOCALAPPDATA%\capri-voice\capri-voice.log` (started over past 4 MB).
//!
//! `CAPRI_VOICE_BUNDLE` names another bundle folder; `CAPRI_VOICE_SPAWN=0` leaves the sidecar to
//! him (he runs it himself). Never under test (`SA_BRAIN_SYNTH`).

use crate::platform::BaseDir;
use super::dirs::{base_dir, starts_over};
use ::windows::Win32::Foundation::HANDLE;
use ::windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use ::windows::core::PCWSTR;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, Once};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// How often the supervisor looks (an exit, `[voice] enabled` turned off).
const LOOK: Duration = Duration::from_millis(250);
/// A run this long counts as a good one: the next exit waits 1 s again.
const GOOD_RUN: Duration = Duration::from_secs(60);
const MAX_WAIT: Duration = Duration::from_secs(60);

static WANT: AtomicBool = AtomicBool::new(false);
/// His microphone (`[voice] mic`) as she runs with it: her last good settings, never the file
/// read again (a typo elsewhere in it must not swap his mic for the default; the wake word keeps
/// the mic open).
static MIC: Mutex<String> = Mutex::new(String::new());
static STARTED: Once = Once::new();

/// Whether the sidecar should run (`[voice] enabled`) and on which microphone (`[voice] mic`;
/// empty: Windows' default), called every tick: the first `true` starts the supervisor; it
/// starts, restarts and stops the sidecar from then on. The mic is read at each start.
pub fn voice_sidecar(on: bool, mic: &str) {
    {
        let mut m = MIC.lock().unwrap_or_else(|p| p.into_inner());
        if *m != mic {
            mic.clone_into(&mut *m);
        }
    }
    WANT.store(on, Ordering::Relaxed);
    if !on || std::env::var_os("SA_BRAIN_SYNTH").is_some() || std::env::var("CAPRI_VOICE_SPAWN").is_ok_and(|v| v == "0") {
        return;
    }
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new().name("voice-sidecar".into()).spawn(|| {
            let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
            let Some(dir) = find_bundle(std::env::var_os("CAPRI_VOICE_BUNDLE").map(PathBuf::from), exe_dir.as_deref()) else {
                elog!(
                    "synaps-avatar: voice: no sidecar bundle beside her ({}\\voice) or at CAPRI_VOICE_BUNDLE: install voice, or run capri-voice yourself",
                    exe_dir.as_deref().map(|d| d.display().to_string()).unwrap_or_default()
                );
                return;
            };
            let job = match Job::new() {
                Ok(j) => j,
                Err(e) => {
                    elog!("synaps-avatar: voice: no job object ({e}): the sidecar is not started (it could outlive her)");
                    return;
                }
            };
            let log = base_dir(BaseDir::State).unwrap_or_else(std::env::temp_dir).join("capri-voice").join("capri-voice.log");
            elog!("synaps-avatar: voice: the sidecar runs from {}, its log is {}", dir.display(), log.display());
            let mut sup = Supervisor::default();
            loop {
                let mut once = || start(&dir, &log, &job);
                sup.step(WANT.load(Ordering::Relaxed), Instant::now(), &mut once);
                std::thread::sleep(LOOK);
            }
        });
        if let Err(e) = spawned {
            elog!("synaps-avatar: voice: no supervisor thread ({e})");
        }
    });
}

/// The bundle folder: `$CAPRI_VOICE_BUNDLE`, else `voice` beside her exe (where the voice
/// installer puts it). One holds `python\python.exe` and `capri-voice`.
fn find_bundle(env: Option<PathBuf>, exe_dir: Option<&Path>) -> Option<PathBuf> {
    env.into_iter()
        .chain(exe_dir.map(|d| d.join("voice")))
        .find(|d| d.join("python").join("python.exe").is_file() && d.join("capri-voice").is_file())
}

/// The wait before the next start after an exit: 1 s doubling to a minute; 1 s again after a
/// good run.
fn next_wait(prev: Duration, ran: Duration) -> Duration {
    if ran >= GOOD_RUN { Duration::from_secs(1) } else { (prev * 2).clamp(Duration::from_secs(1), MAX_WAIT) }
}

/// A job object that kills what it holds when its last handle closes (hers: at her exit, however
/// she ends). Never closed while she runs.
struct Job(HANDLE);

impl Job {
    fn new() -> io::Result<Job> {
        let h = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(io::Error::other)?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        unsafe { SetInformationJobObject(h, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size) }.map_err(io::Error::other)?;
        Ok(Job(h))
    }

    fn assign(&self, c: &Child) -> io::Result<()> {
        unsafe { AssignProcessToJobObject(self.0, HANDLE(c.as_raw_handle())) }.map_err(io::Error::other)
    }
}

/// Its log, appended (a new one past 4 MB, the old kept as `capri-voice.old.log`).
fn open_log(path: &Path) -> io::Result<std::fs::File> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    if starts_over(std::fs::metadata(path).map(|m| m.len()).ok()) {
        std::fs::rename(path, path.with_extension("old.log")).ok();
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

/// Her models' folder (`$CAPRI_VOICE_MODELS`, else `%LOCALAPPDATA%\capri-voice\models`, as the
/// sidecar reads it): ready when Kokoro is there (without it the sidecar can't start; the model
/// fetch, `voice-models.ps1`, puts it there).
fn models_ready() -> Result<(), PathBuf> {
    let d = std::env::var_os("CAPRI_VOICE_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("capri-voice").join("models"));
    if ["kokoro-v1.0.int8.onnx", "voices-v1.0.bin"].iter().all(|f| d.join(f).is_file()) { Ok(()) } else { Err(d) }
}

/// One start: the bundle's interpreter on `capri-voice`, no console window, in the job.
fn start(dir: &Path, log: &Path, job: &Job) -> io::Result<Child> {
    if let Err(d) = models_ready() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no models in {} yet: run voice-models.ps1 (the voice installer does)", d.display())));
    }
    let out = open_log(log)?;
    let err = out.try_clone()?;
    let mic = MIC.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut c = command(dir, &mic);
    c.stdout(out).stderr(err);
    launch(&mut c, job)
}

/// The sidecar's command: the bundle's interpreter on `capri-voice`, no console window. `mic`
/// (`[voice] mic`): its capture device, by name; empty: Windows' default, and a
/// `CAPRI_VOICE_SOURCE` she inherited is not passed on.
fn command(dir: &Path, mic: &str) -> Command {
    let mut c = Command::new(dir.join("python").join("python.exe"));
    c.arg(dir.join("capri-voice"))
        .current_dir(dir)
        // Her log lines carry "·" and the like, whatever his code page.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    if mic.is_empty() {
        c.env_remove("CAPRI_VOICE_SOURCE");
    } else {
        c.env("CAPRI_VOICE_SOURCE", mic);
    }
    c
}

/// Spawn `c` and put it in the job; a child that can't be put there is killed (it would outlive
/// her).
fn launch(c: &mut Command, job: &Job) -> io::Result<Child> {
    let mut child = c.spawn()?;
    if let Err(e) = job.assign(&child) {
        child.kill().ok();
        child.wait().ok();
        return Err(e);
    }
    Ok(child)
}

/// The supervisor's state: the running sidecar, when it started, the wait before the next start.
#[derive(Default)]
struct Supervisor {
    child: Option<(Child, Instant)>,
    wait: Duration,
    next_at: Option<Instant>,
    starts: u32,
}

impl Supervisor {
    /// One look: start it if wanted and due, stop it if not wanted, note an exit.
    fn step(&mut self, want: bool, now: Instant, start: &mut dyn FnMut() -> io::Result<Child>) {
        if let Some((c, t0)) = &mut self.child {
            if !want {
                c.kill().ok();
                c.wait().ok();
                elog!("synaps-avatar: voice: sidecar stopped (voice off)");
                self.child = None;
                self.wait = Duration::ZERO;
                self.next_at = None;
                return;
            }
            match c.try_wait() {
                Ok(None) => return,
                Ok(Some(st)) => {
                    let ran = now.saturating_duration_since(*t0);
                    self.wait = next_wait(self.wait, ran);
                    elog!("synaps-avatar: voice: sidecar exited ({st}) after {} s; again in {} s", ran.as_secs(), self.wait.as_secs());
                }
                Err(e) => {
                    self.wait = next_wait(self.wait, Duration::ZERO);
                    elog!("synaps-avatar: voice: sidecar: {e}; again in {} s", self.wait.as_secs());
                }
            }
            self.child = None;
            self.next_at = Some(now + self.wait);
            return;
        }
        if !want || self.next_at.is_some_and(|t| now < t) {
            return;
        }
        match start() {
            Ok(c) => {
                self.starts += 1;
                elog!("synaps-avatar: voice: sidecar started (pid {}, start {})", c.id(), self.starts);
                self.child = Some((c, now));
            }
            Err(e) => {
                self.wait = next_wait(self.wait, Duration::ZERO);
                self.next_at = Some(now + self.wait);
                elog!("synaps-avatar: voice: the sidecar didn't start ({e}); again in {} s", self.wait.as_secs());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_back_off_and_a_good_run_resets_them() {
        let mut w = Duration::ZERO;
        let mut seen = Vec::new();
        for _ in 0..8 {
            w = next_wait(w, Duration::from_secs(2));
            seen.push(w.as_secs());
        }
        assert_eq!(seen, [1, 2, 4, 8, 16, 32, 60, 60]);
        assert_eq!(next_wait(Duration::from_secs(60), GOOD_RUN), Duration::from_secs(1));
    }

    #[test]
    fn the_bundle_is_found_where_the_installer_puts_it() {
        let d = std::env::temp_dir().join(format!("sa-bundle-{}", std::process::id()));
        let b = d.join("voice");
        std::fs::create_dir_all(b.join("python")).unwrap();
        assert_eq!(find_bundle(None, Some(&d)), None, "an empty folder is no bundle");
        std::fs::write(b.join("python").join("python.exe"), b"").unwrap();
        std::fs::write(b.join("capri-voice"), b"").unwrap();
        assert_eq!(find_bundle(None, Some(&d)), Some(b.clone()));
        assert_eq!(find_bundle(Some(d.join("nothing")), Some(&d)), Some(b.clone()), "a wrong CAPRI_VOICE_BUNDLE falls through");
        assert_eq!(find_bundle(Some(b.clone()), None), Some(b));
        std::fs::remove_dir_all(&d).ok();
    }

    /// His mic reaches the sidecar as she runs with it (never the file read again); none:
    /// Windows' default, and a CAPRI_VOICE_SOURCE she inherited is dropped, not passed on.
    #[test]
    fn the_sidecar_gets_his_mic_or_none() {
        let source = |c: &Command| {
            c.get_envs().find(|(k, _)| k.to_str() == Some("CAPRI_VOICE_SOURCE")).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        let d = Path::new(r"C:\voice");
        assert_eq!(source(&command(d, "Microphone (USB Audio)")), Some(Some("Microphone (USB Audio)".into())));
        assert_eq!(source(&command(d, "")), Some(None), "removed, not inherited");
        voice_sidecar(false, "Microphone (USB Audio)");
        assert_eq!(*MIC.lock().unwrap(), "Microphone (USB Audio)");
        voice_sidecar(false, "");
        assert_eq!(*MIC.lock().unwrap(), "");
    }

    /// The job kills its child when its last handle closes: she is that handle.
    #[test]
    fn the_sidecar_dies_with_her() {
        let job = Job::new().unwrap();
        let mut c = launch(Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(Stdio::null()), &job).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(c.try_wait().unwrap().is_none(), "it runs");
        let t0 = Instant::now();
        unsafe { ::windows::Win32::Foundation::CloseHandle(job.0) }.unwrap();
        c.wait().unwrap();
        assert!(t0.elapsed() < Duration::from_secs(5), "killed with the job, not after its 30 s ({:?})", t0.elapsed());
    }

    /// Exits are started again after the backoff; voice off stops it.
    #[test]
    fn the_supervisor_restarts_and_stops() {
        let job = Job::new().unwrap();
        let mut sup = Supervisor::default();
        let mut quick = || launch(Command::new("cmd").args(["/c", "exit 3"]), &job);
        let t0 = Instant::now();
        sup.step(true, t0, &mut quick);
        assert_eq!(sup.starts, 1);
        // Its exit is seen (real time: the process ends), and the next start waits 1 s.
        let seen = Instant::now();
        while sup.child.is_some() && seen.elapsed() < Duration::from_secs(10) {
            sup.step(true, t0, &mut quick);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(sup.child.is_none(), "the exit was seen");
        assert_eq!(sup.next_at, Some(t0 + Duration::from_secs(1)));
        sup.step(true, t0 + Duration::from_millis(500), &mut quick);
        assert_eq!(sup.starts, 1, "not before the wait");
        sup.step(true, t0 + Duration::from_millis(1100), &mut quick);
        assert_eq!(sup.starts, 2, "again after it");
        // Voice off: a running one is killed; nothing starts while off.
        let mut long = || launch(Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(Stdio::null()), &job);
        let mut sup = Supervisor::default();
        sup.step(true, t0, &mut long);
        assert!(sup.child.is_some());
        sup.step(false, t0, &mut long);
        assert!(sup.child.is_none());
        sup.step(false, t0 + Duration::from_secs(5), &mut long);
        assert_eq!(sup.starts, 1);
    }
}
