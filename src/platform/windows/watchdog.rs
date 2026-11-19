//! Her UI-thread watchdog (diagnostics). Her UI thread beats `trace::beat` (a foreground task,
//! each 100 ms); when it hasn't for [`STALL_MS`], her log gets `STALL <ms>` with the trace ring
//! (`SA_TRACE=1` fills it), and a full-memory minidump goes to
//! `%LOCALAPPDATA%\synaps-avatar\dumps\` (at most [`MAX_DUMPS`] a run, [`KEEP`] kept). The dump
//! holds her memory (a token among it): it stays on his machine. `SA_WATCHDOG=0`: none.

use super::dirs::base_dir;
use crate::platform::BaseDir;
use crate::trace;
use ::windows::Win32::Foundation::HANDLE;
use ::windows::Win32::System::Diagnostics::Debug::{
    MINIDUMP_TYPE, MiniDumpWithFullMemory, MiniDumpWithFullMemoryInfo, MiniDumpWithHandleData, MiniDumpWithThreadInfo, MiniDumpWithUnloadedModules,
    MiniDumpWriteDump,
};
use ::windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
use std::os::windows::io::AsRawHandle as _;
use std::path::PathBuf;
use std::time::Duration;

/// A UI thread out this long is stalled.
pub const STALL_MS: u64 = 1500;
/// A stall that lasts is logged again at these ages.
const AGAIN_MS: [u64; 3] = [5_000, 15_000, 60_000];
const LOOK: Duration = Duration::from_millis(250);
const MAX_DUMPS: usize = 3;
/// Dumps kept in the folder (older ones go first).
const KEEP: usize = 6;

/// Start the watchdog thread (once).
pub fn start_watchdog() {
    if std::env::var("SA_WATCHDOG").is_ok_and(|v| v == "0") {
        return;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new().name("ui-watchdog".into()).spawn(watch);
        if let Err(e) = spawned {
            elog!("synaps-avatar: no UI watchdog: {e}");
        }
    });
}

fn watch() {
    let (mut dumps, mut last_look) = (0, trace::now_ms());
    // The beat the stall began after, and how many of `AGAIN_MS` were logged.
    let mut stall: Option<(u64, usize)> = None;
    loop {
        std::thread::sleep(LOOK);
        let now = trace::now_ms();
        let gap = now - std::mem::replace(&mut last_look, now);
        let beat = trace::last_beat();
        if beat == 0 {
            continue;
        }
        // This thread was out too (the machine slept): her UI thread gets a look to beat first.
        if gap > 4 * LOOK.as_millis() as u64 {
            continue;
        }
        let age = now.saturating_sub(beat);
        match stall {
            Some((b, _)) if beat != b => {
                elog!("synaps-avatar: STALL over: her UI thread was out {} ms ({})", beat.saturating_sub(b), trace::wall_clock());
                stall = None;
            }
            Some((b, n)) => {
                if n < AGAIN_MS.len() && age >= AGAIN_MS[n] {
                    elog!("synaps-avatar: STALL {age} ms, still ({})", trace::wall_clock());
                    stall = Some((b, n + 1));
                }
            }
            None if age >= STALL_MS => {
                stall = Some((beat, 0));
                elog!("synaps-avatar: STALL {age} ms: her UI thread hasn't run since then ({}); the last trace lines:", trace::wall_clock());
                let ring = trace::ring();
                if ring.is_empty() {
                    elog!("synaps-avatar:   (none: SA_TRACE=1 records them)");
                }
                for l in ring {
                    elog!("synaps-avatar:   | {l}");
                }
                if dumps < MAX_DUMPS {
                    dumps += 1;
                    let t0 = std::time::Instant::now();
                    match write_dump() {
                        Ok(p) => elog!("synaps-avatar: STALL dump {} ({} ms)", p.display(), t0.elapsed().as_millis()),
                        Err(e) => elog!("synaps-avatar: STALL dump failed: {e:#}"),
                    }
                }
            }
            None => {}
        }
    }
}

fn write_dump() -> anyhow::Result<PathBuf> {
    let dir = base_dir(BaseDir::State).ok_or_else(|| anyhow::anyhow!("no %LOCALAPPDATA%"))?.join("synaps-avatar").join("dumps");
    std::fs::create_dir_all(&dir)?;
    prune(&dir);
    let stamp = trace::wall_clock().replace([':', '.'], "");
    let path = dir.join(format!("synaps-avatar-{}-{stamp}.dmp", std::process::id()));
    let f = std::fs::File::create(&path)?;
    let kind = MiniDumpWithFullMemory.0 | MiniDumpWithHandleData.0 | MiniDumpWithThreadInfo.0 | MiniDumpWithFullMemoryInfo.0 | MiniDumpWithUnloadedModules.0;
    unsafe { MiniDumpWriteDump(GetCurrentProcess(), GetCurrentProcessId(), HANDLE(f.as_raw_handle()), MINIDUMP_TYPE(kind), None, None, None)? };
    Ok(path)
}

/// Room for one more: the oldest dumps go, `KEEP - 1` stay.
fn prune(dir: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut dumps: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "dmp"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dumps.sort();
    while dumps.len() >= KEEP {
        let (_, p) = dumps.remove(0);
        std::fs::remove_file(p).ok();
    }
}
