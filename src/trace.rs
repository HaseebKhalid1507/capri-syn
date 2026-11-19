//! Diagnostics. `SA_TRACE=1`: a line with the wall clock (ms) for each call on her press, move
//! and focus path, to her log and into a ring of the last [`RING`] that the UI-thread watchdog
//! prints when her UI thread stalls. The heartbeat that watchdog reads: her UI thread beats it
//! (a foreground task, and each frame), so a stalled UI thread stops it.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// How many trace lines the ring keeps.
pub const RING: usize = 50;

static ON: OnceLock<bool> = OnceLock::new();
static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static T0: OnceLock<Instant> = OnceLock::new();
/// Milliseconds since `T0` at the last beat (0: none yet).
static BEAT: AtomicU64 = AtomicU64::new(0);

/// Whether tracing is on (`SA_TRACE`, anything but empty or 0).
pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var("SA_TRACE").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// Milliseconds on a monotonic clock (this process's).
pub fn now_ms() -> u64 {
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// The local wall clock, `HH:MM:SS.mmm` (trace lines line up with his clock and the OS's logs).
pub fn wall_clock() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let unix = d.as_secs() as i64;
    let s = (unix + crate::platform::utc_offset_s(unix) as i64).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}.{:03}", s / 3600, s / 60 % 60, s % 60, d.subsec_millis())
}

/// Her UI thread is alive now.
pub fn beat() {
    BEAT.store(now_ms(), Ordering::Relaxed);
}

/// When her UI thread last beat (`now_ms`), 0 before the first beat.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn last_beat() -> u64 {
    BEAT.load(Ordering::Relaxed)
}

/// Record a trace line (the caller checked `on()`): `HH:MM:SS.mmm [thread] text`, to her log and
/// the ring. The ring's lock is held for a push only.
pub fn record(text: std::fmt::Arguments<'_>) {
    let t = std::thread::current();
    let line = format!("{} [{}] {text}", wall_clock(), t.name().unwrap_or("?"));
    elog!("synaps-avatar: trace {line}");
    let mut g = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if g.len() == RING {
        g.pop_front();
    }
    g.push_back(line);
}

/// The ring, oldest first. `try_lock`: a watchdog never waits on it.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn ring() -> Vec<String> {
    match LINES.try_lock() {
        Ok(g) => g.iter().cloned().collect(),
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner().iter().cloned().collect(),
        Err(std::sync::TryLockError::WouldBlock) => vec!["(the trace ring is busy)".into()],
    }
}

/// A trace line when `SA_TRACE` is on; nothing is formatted otherwise.
macro_rules! trace {
    ($($t:tt)*) => {
        if $crate::trace::on() {
            $crate::trace::record(format_args!($($t)*));
        }
    };
}
