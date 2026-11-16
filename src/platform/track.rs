//! The drag poller's pointer track, and where the pointer was at a frame's time (core).
//!
//! A held window follows the pointer once a frame. Taking the newest sample made the steps
//! uneven whenever input events and frames beat against each other: 120 Hz motion into 60 Hz
//! frames put two moves in most frames, one or three in some (9 → 11 → 5 → 11 → 5 px in the
//! r25 strip). Resampled at a fixed latency before the frame (Android's input resampling), a
//! steady drag moves the same distance every frame.

use std::collections::VecDeque;
use std::time::Instant;

/// Resample this long before the frame: a steady drag's frame time then falls between two
/// samples (or just past the newest), never far ahead of what was seen.
const LATENCY: f64 = 0.005;
/// Samples closer than this give no direction (one read twice).
const MIN_DELTA: f64 = 0.002;
/// Samples further apart than this: the pointer rested between them (held, not glided).
const MAX_DELTA: f64 = 0.020;
/// Past the newest sample, along the last two, at most this far (and half their gap).
const MAX_PREDICTION: f64 = 0.008;
/// Changes kept (a 2 ms poller: the last ~30 ms of a steady drag).
const KEEP: usize = 16;

/// Each pointer change the poller saw (root px), with when it first saw it.
#[derive(Default)]
pub(super) struct Track {
    changes: VecDeque<(Instant, (i32, i32))>,
    /// Bumped on every change (the view asks for a frame when it moved since the last).
    seq: u64,
}

impl Track {
    pub(super) fn push(&mut self, at: Instant, p: (i32, i32)) {
        if self.changes.back().is_some_and(|c| c.1 == p) {
            return;
        }
        if self.changes.len() == KEEP {
            self.changes.pop_front();
        }
        self.changes.push_back((at, p));
        self.seq += 1;
    }

    pub(super) fn newest(&self) -> Option<(Instant, (i32, i32))> {
        self.changes.back().copied()
    }

    pub(super) fn seq(&self) -> u64 {
        self.seq
    }

    /// Where the pointer was at `at`, resampled (None before the first sample).
    pub(super) fn at(&self, at: Instant) -> Option<(f32, f32)> {
        let base = self.changes.front()?.0;
        let s = |t: Instant| t.checked_duration_since(base).map_or(0.0, |d| d.as_secs_f64());
        let v: Vec<(f64, (f32, f32))> = self.changes.iter().map(|&(t, p)| (s(t), (p.0 as f32, p.1 as f32))).collect();
        let at = at.checked_duration_since(base).map_or(0.0, |d| d.as_secs_f64());
        resample(&v, at)
    }
}

/// The pointer at `at - LATENCY` from timed samples (seconds, oldest first): on the segment
/// around then; past the newest, along the last two (`MAX_PREDICTION`, half their gap); held
/// across a gap longer than `MAX_DELTA` (it rested) and once the newest is that old (it
/// stopped: no overshoot).
pub(super) fn resample(v: &[(f64, (f32, f32))], at: f64) -> Option<(f32, f32)> {
    let &(tn, pn) = v.last()?;
    let s = at - LATENCY;
    let lerp = |a: (f32, f32), b: (f32, f32), k: f64| (a.0 + (b.0 - a.0) * k as f32, a.1 + (b.1 - a.1) * k as f32);
    if s >= tn {
        if let [.., (tp, pp), _] = v {
            let d = tn - tp;
            if (MIN_DELTA..=MAX_DELTA).contains(&d) && at - tn <= MAX_DELTA {
                let ahead = (s - tn).min(d / 2.0).min(MAX_PREDICTION);
                return Some(lerp(pn, *pp, -ahead / d));
            }
        }
        return Some(pn);
    }
    let i = v.iter().rposition(|c| c.0 <= s)?;
    let ((ta, pa), (tb, pb)) = (v[i], v[i + 1]);
    if tb - ta > MAX_DELTA {
        return Some(pa);
    }
    Some(lerp(pa, pb, (s - ta) / (tb - ta)))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Samples = Vec<(f64, (f32, f32))>;

    /// A steady 600 px/s drag from a 125 Hz mouse (±1 ms of read jitter), shown at 60 Hz:
    /// the two rates drift, so a frame now and then sees three samples instead of two.
    fn steady() -> (Samples, Vec<f64>) {
        let jitter = [0.0, 0.001, -0.0008, 0.0005];
        let v = (0..80).map(|k| (k as f64 / 125.0 + jitter[k % 4], (0.0, (600.0 * k as f64 / 125.0) as f32))).collect();
        let frames = (2..34).map(|f| f as f64 / 60.0 + 0.0031).collect();
        (v, frames)
    }

    fn steps(v: &[(f64, (f32, f32))], frames: &[f64], newest_only: bool) -> Vec<f32> {
        let pos: Vec<f32> = frames
            .iter()
            .map(|&f| {
                let seen: Vec<_> = v.iter().copied().filter(|c| c.0 <= f).collect();
                if newest_only { seen.last().unwrap().1.1 } else { resample(&seen, f).unwrap().1 }
            })
            .collect();
        pos.windows(2).map(|w| w[1] - w[0]).collect()
    }

    #[test]
    fn a_steady_drag_moves_the_same_every_frame() {
        let (v, frames) = steady();
        let raw = steps(&v, &frames, true);
        let (lo, hi) = raw.iter().fold((f32::MAX, f32::MIN), |(a, b), &s| (a.min(s), b.max(s)));
        assert!(hi - lo >= 4.0, "the newest sample alone beats against the frames: {raw:?}");
        for s in steps(&v, &frames, false) {
            assert!((s - 10.0).abs() < 1.5, "resampled steps stay near 10 px (the read jitter): {s}");
        }
    }

    #[test]
    fn a_stop_holds_and_a_rest_does_not_glide() {
        let v = [(0.0, (0.0, 0.0)), (0.0083, (0.0, 5.0)), (0.0167, (0.0, 10.0))];
        assert_eq!(resample(&v, 0.05), Some((0.0, 10.0)), "stopped: where it stopped");
        let ahead = resample(&v, 0.0167 + 0.008).unwrap().1;
        assert!(ahead > 10.0 && ahead <= 10.0 + 5.0 / 2.0, "just past the newest: a little ahead, at most half a step: {ahead}");
        let v = [(0.0, (0.0, 0.0)), (0.5, (0.0, 24.0))];
        assert_eq!(resample(&v, 0.499), Some((0.0, 0.0)), "at rest before a jump: no glide");
        assert_eq!(resample(&v, 0.506), Some((0.0, 24.0)), "the jump, once its latency has passed");
        assert_eq!(resample(&[], 1.0), None);
    }

    #[test]
    fn the_track_keeps_changes_only() {
        let t0 = Instant::now();
        let mut tr = Track::default();
        for (i, p) in [(0, 0), (0, 0), (0, 5), (0, 5), (0, 10)].into_iter().enumerate() {
            tr.push(t0 + std::time::Duration::from_millis(4 * i as u64), p);
        }
        assert_eq!((tr.changes.len(), tr.seq()), (3, 3));
        assert_eq!(tr.newest().map(|n| n.1), Some((0, 10)));
    }
}
