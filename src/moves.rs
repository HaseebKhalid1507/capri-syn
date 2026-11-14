//! Short moves without walk art (BRIEF-brain §6): hop-scoot along the floor, and the float
//! drift path. Pure; logical px and ms, the view scales: the `hop` arrival and passive
//! scoots, and float-to-float drifts.

use crate::anim::Rng;

pub const HOP_DX: (f32, f32) = (40.0, 90.0);
pub const HOP_H: f32 = 28.0;
pub const HOP_CROUCH_MS: u32 = 120;
pub const HOP_AIR_MS: u32 = 260;
pub const HOP_LAND_MS: u32 = 180;
pub const SCOOT_MAX: f32 = 520.0;
pub const SCOOT_HOPS_MAX: usize = 6;
/// The `hop` arrival is only for spots this close (logical px).
pub const HOP_ARRIVAL_MAX: f32 = 300.0;
/// A passive teleport needs at least this (logical px): closer spots are scooted.
pub const MIN_TELEPORT: f32 = 300.0;
pub const DRIFT_MAX_SPEED: f32 = 90.0;
pub const DRIFT_BURST_S: (f32, f32) = (1.5, 4.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hop {
    pub from_x: f32,
    pub to_x: f32,
}

impl Hop {
    pub const MS: u32 = HOP_CROUCH_MS + HOP_AIR_MS + HOP_LAND_MS;

    /// Window offset from `from_x` at `ms` into the hop: (dx, dy up = negative). Moves only in
    /// the air; crouch and landing stay put (the frames and the squash play there).
    pub fn offset(&self, ms: u32) -> (f32, f32) {
        let dx = self.to_x - self.from_x;
        if ms < HOP_CROUCH_MS {
            return (0.0, 0.0);
        }
        let a = ms - HOP_CROUCH_MS;
        if a >= HOP_AIR_MS {
            return (dx, 0.0);
        }
        let u = a as f32 / HOP_AIR_MS as f32;
        (dx * u, -4.0 * HOP_H * u * (1.0 - u))
    }
}

/// Hops from `from_x` to `to_x` (at most `SCOOT_MAX` away, else None: teleport instead). Each
/// hop covers `HOP_DX`; the last one lands exactly on `to_x`.
pub fn plan_scoot(from_x: f32, to_x: f32, rng: &mut Rng) -> Option<Vec<Hop>> {
    let d = to_x - from_x;
    if !d.is_finite() || d.abs() > SCOOT_MAX {
        return None;
    }
    if d.abs() < HOP_DX.0 * 0.5 {
        return Some(Vec::new());
    }
    // As many hops as keep each in HOP_DX (about 65 px), at most SCOOT_HOPS_MAX.
    let lo = (d.abs() / HOP_DX.1).ceil().max(1.0) as usize;
    let hi = ((d.abs() / HOP_DX.0).floor() as usize).max(lo);
    let n = ((d.abs() / 65.0).ceil() as usize).clamp(lo, hi).min(SCOOT_HOPS_MAX);
    let base = d.abs() / n as f32;
    // A little jitter, kept inside HOP_DX, summing to |d|.
    let mut steps: Vec<f32> = (0..n).map(|_| base * rng.range(0.9, 1.1)).collect();
    let sum: f32 = steps.iter().sum();
    steps.iter_mut().for_each(|s| *s *= d.abs() / sum);
    if n > 1 && steps.iter().any(|&s| !(HOP_DX.0..=HOP_DX.1).contains(&s)) {
        steps = vec![base; n];
    }
    let mut hops = Vec::with_capacity(n);
    let mut x = from_x;
    for (i, s) in steps.iter().enumerate() {
        let nx = if i + 1 == n { to_x } else { x + s * d.signum() };
        hops.push(Hop { from_x: x, to_x: nx });
        x = nx;
    }
    Some(hops)
}

/// The scoot cut short at the first hop whose landing breaks a rule (`ok(x)` = false): that
/// hop's start becomes the spot.
pub fn scoot_until(hops: &[Hop], ok: impl Fn(f32) -> bool) -> Vec<Hop> {
    hops.iter().take_while(|h| ok(h.to_x)).copied().collect()
}

/// A drift leg: cubic ease in/out between two points, slow enough that its peak speed stays
/// under `DRIFT_MAX_SPEED` (a smoothstep peaks at 1.5x the mean speed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drift {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub secs: f32,
}

impl Drift {
    pub fn new(from: (f32, f32), to: (f32, f32), rng: &mut Rng) -> Drift {
        let d = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
        let min = 1.5 * d / DRIFT_MAX_SPEED;
        let secs = rng.range(DRIFT_BURST_S.0, DRIFT_BURST_S.1).max(min);
        Drift { from, to, secs }
    }

    pub fn at(&self, t: f32) -> (f32, f32) {
        let u = (t / self.secs).clamp(0.0, 1.0);
        let e = u * u * (3.0 - 2.0 * u);
        (self.from.0 + (self.to.0 - self.from.0) * e, self.from.1 + (self.to.1 - self.from.1) * e)
    }

    pub fn done(&self, t: f32) -> bool {
        t >= self.secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoot_hops_within_bounds() {
        let mut rng = Rng::new(1);
        for k in 0..500 {
            let from = rng.range(0.0, 2000.0);
            let d = rng.range(-SCOOT_MAX, SCOOT_MAX);
            let hops = plan_scoot(from, from + d, &mut rng).unwrap();
            assert!(hops.len() <= SCOOT_HOPS_MAX, "#{k}");
            if d.abs() >= HOP_DX.0 * 0.5 {
                assert_eq!(hops.last().unwrap().to_x, from + d);
                for h in &hops {
                    let s = (h.to_x - h.from_x).abs();
                    assert!(s <= HOP_DX.1 + 0.5 && (s >= HOP_DX.0 - 0.5 || s >= d.abs() - 0.5 || hops.len() == 1), "#{k} step {s} of {d}");
                    assert_eq!((h.to_x - h.from_x).signum(), d.signum());
                }
                for w in hops.windows(2) {
                    assert_eq!(w[0].to_x, w[1].from_x);
                }
            }
            // The arc: off the ground only in the air, never higher than HOP_H.
            for h in &hops {
                assert_eq!(h.offset(0), (0.0, 0.0));
                assert_eq!(h.offset(Hop::MS), (h.to_x - h.from_x, 0.0));
                for ms in (0..Hop::MS).step_by(10) {
                    let (_, dy) = h.offset(ms);
                    assert!((-HOP_H - 0.01..=0.0).contains(&dy));
                }
            }
        }
        assert!(plan_scoot(0.0, SCOOT_MAX + 1.0, &mut rng).is_none());
        assert!(plan_scoot(0.0, f32::NAN, &mut rng).is_none());
    }

    #[test]
    fn scoot_stops_at_guard() {
        let mut rng = Rng::new(2);
        let hops = plan_scoot(0.0, 400.0, &mut rng).unwrap();
        let cut = scoot_until(&hops, |x| x < 250.0);
        assert!(!cut.is_empty() && cut.len() < hops.len());
        assert!(cut.iter().all(|h| h.to_x < 250.0));
        assert_eq!(cut.last().unwrap().to_x, hops[cut.len() - 1].to_x);
        assert!(scoot_until(&hops, |_| false).is_empty());
    }

    #[test]
    fn drift_speed_cap() {
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let from = (rng.range(0.0, 2000.0), rng.range(0.0, 1000.0));
            let to = (from.0 + rng.range(-400.0, 400.0), from.1 + rng.range(-200.0, 200.0));
            let d = Drift::new(from, to, &mut rng);
            let dt = 1.0 / 240.0;
            let mut t = 0.0;
            let mut p = d.at(0.0);
            while !d.done(t) {
                t += dt;
                let q = d.at(t);
                let v = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt() / dt;
                assert!(v <= DRIFT_MAX_SPEED + 0.5, "{v}");
                p = q;
            }
            assert!((p.0 - to.0).abs() < 1e-3 && (p.1 - to.1).abs() < 1e-3);
        }
    }
}
