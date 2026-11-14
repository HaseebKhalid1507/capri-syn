//! Which way she faces. The art is drawn facing one way (the pack's `facing`, right by
//! default); mirrored, she faces the other. Pure: the hysteresis that turns her on a drag,
//! and where she should look on her own.

/// A drag turns her once the pointer's horizontal velocity (logical px/s, the same fit the
/// drag follow uses) has pointed the other way past this for `TURN_HOLD`.
pub const TURN_V: f32 = 200.0;
pub const TURN_HOLD: f32 = 0.11;
/// Never two drag turns closer than this (a shake doesn't strobe her).
pub const TURN_GAP: f32 = 0.30;
/// Context turns ignore targets closer than this (logical px): straight ahead is no turn.
pub const DEAD_PX: f32 = 40.0;
/// Near a monitor edge (this share of its width) she faces into the screen.
pub const EDGE_SHARE: f32 = 0.2;
/// Idle look-arounds: never within this of another turn (s).
pub const LOOK_QUIET_S: f64 = 20.0;

/// Drag facing with hysteresis.
#[derive(Default, Clone, Debug)]
pub struct Turner {
    /// The way the velocity has pointed (past `TURN_V`), since when.
    pending: Option<(bool, f32)>,
    last: Option<f32>,
}

impl Turner {
    /// `vx` logical px/s at `t` s; `right`: facing right now. `Some(new)` when she turns.
    pub fn feed(&mut self, t: f32, vx: f32, right: bool) -> Option<bool> {
        let want = if vx > TURN_V {
            Some(true)
        } else if vx < -TURN_V {
            Some(false)
        } else {
            None
        };
        match want {
            Some(w) if w != right => {
                let since = match self.pending {
                    Some((d, s)) if d == w => s,
                    _ => {
                        self.pending = Some((w, t));
                        t
                    }
                };
                if t - since >= TURN_HOLD && self.last.is_none_or(|l| t - l >= TURN_GAP) {
                    self.pending = None;
                    self.last = Some(t);
                    return Some(w);
                }
                None
            }
            _ => {
                self.pending = None;
                None
            }
        }
    }
}

/// Face `target_x` from `x` (root px), unless it is within `dead` px.
pub fn toward(x: i32, target_x: i32, dead: i32) -> Option<bool> {
    ((target_x - x).abs() > dead).then_some(target_x > x)
}

/// Near a monitor edge (`x` in the outer `EDGE_SHARE` of `[mx, mx + mw)`): into the screen.
pub fn inward(x: i32, mx: i32, mw: i32) -> Option<bool> {
    let edge = (mw as f32 * EDGE_SHARE) as i32;
    if x < mx + edge {
        Some(true)
    } else if x >= mx + mw - edge {
        Some(false)
    } else {
        None
    }
}

/// Seconds to the next idle look-around: uniform in `every_min` minutes (`u` in 0..1).
pub fn look_gap(every_min: (f32, f32), u: f32) -> f64 {
    let (lo, hi) = (every_min.0.max(0.05), every_min.1.max(every_min.0.max(0.05)));
    ((lo + (hi - lo) * u.clamp(0.0, 1.0)) * 60.0) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ragdoll::Pointer;

    /// Drive a Turner from pointer samples every 8 ms through the drag's own velocity fit.
    fn run(path: impl Fn(f32) -> f32, secs: f32, mut right: bool) -> Vec<(f32, bool)> {
        let mut p = Pointer::default();
        let mut tr = Turner::default();
        let mut turns = Vec::new();
        let mut t = 0.0;
        while t <= secs {
            p.push(t, path(t), 300.0);
            if let Some(r) = tr.feed(t, p.velocity(t).0, right) {
                right = r;
                turns.push((t, r));
            }
            t += 0.008;
        }
        turns
    }

    #[test]
    fn a_steady_drag_turns_once_after_the_hold() {
        let turns = run(|t| 500.0 - 400.0 * t, 1.0, true);
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert!(!turns[0].1);
        assert!(turns[0].0 >= TURN_HOLD && turns[0].0 < 0.3, "{turns:?}");
    }

    #[test]
    fn right_then_left_turns_twice() {
        let path = |t: f32| if t < 0.6 { 400.0 + 500.0 * t } else { 700.0 - 500.0 * (t - 0.6) };
        let turns = run(path, 1.2, false);
        assert_eq!(turns.iter().map(|t| t.1).collect::<Vec<_>>(), vec![true, false], "{turns:?}");
    }

    #[test]
    fn jitter_never_turns_her() {
        // Small back-and-forth wiggles: up to ±20 px at 2-6 Hz.
        for (amp, hz) in [(4.0, 6.0), (8.0, 5.0), (12.0, 4.0), (20.0, 3.0), (20.0, 2.0)] {
            let turns = run(move |t: f32| 500.0 + amp * (std::f32::consts::TAU * hz * t).sin(), 3.0, true);
            assert!(turns.is_empty(), "±{amp} px at {hz} Hz turned her: {turns:?}");
        }
    }

    #[test]
    fn a_shake_does_not_strobe() {
        // Big fast reversals: she may follow, but never more than once per TURN_GAP.
        let turns = run(|t: f32| 500.0 + 80.0 * (std::f32::consts::TAU * 3.0 * t).sin(), 3.0, true);
        for w in turns.windows(2) {
            assert!(w[1].0 - w[0].0 >= TURN_GAP - 1e-4, "{turns:?}");
        }
    }

    #[test]
    fn context_helpers() {
        assert_eq!(toward(100, 400, 40), Some(true));
        assert_eq!(toward(400, 100, 40), Some(false));
        assert_eq!(toward(400, 420, 40), None);
        assert_eq!(inward(50, 0, 1000), Some(true));
        assert_eq!(inward(950, 0, 1000), Some(false));
        assert_eq!(inward(500, 0, 1000), None);
        assert_eq!(look_gap((1.0, 4.0), 0.0), 60.0);
        assert_eq!(look_gap((1.0, 4.0), 1.0), 240.0);
    }
}
