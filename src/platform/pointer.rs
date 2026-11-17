//! The pointer watcher's reading of the samples it already takes (BRIEF-alive S1, S2): near
//! her, a fast swipe past, resting on her, lingering still beside her, a quick wiggle. Pure:
//! one call per sample, no reads of its own, so recorded traces test it.

use super::{PointerCfg, PointerEv};
use super::WinRect;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Sense {
    last: Option<(Instant, i32, i32)>,
    was_near: bool,
    hover_since: Option<Instant>,
    hovered: bool,
    /// Still beside her since (where it stopped); reported once per stop.
    still: Option<(Instant, i32, i32)>,
    lingered: bool,
    /// The wiggle's zigzag: the last extreme x, the way it heads (0: not yet), the turns.
    ext: Option<i32>,
    dir: i8,
    turns: VecDeque<Instant>,
    wiggled: Option<Instant>,
}

impl Sense {
    /// One sample at `now`: `her` body box (None or `armed` false: nothing reports). Also says
    /// whether the pointer is far from her (> 700 px: the watcher polls slower).
    pub fn sample(&mut self, cfg: &PointerCfg, now: Instant, x: i32, y: i32, her: Option<WinRect>, armed: bool) -> (Vec<PointerEv>, bool) {
        let mut out = Vec::new();
        let mut far = true;
        match her.filter(|h| armed && h.w > 0) {
            Some(her) => {
                let d = rect_dist(&her, x as f32, y as f32);
                far = d > 700.0;
                let near = d <= cfg.near_px;
                if near && !self.was_near && d > 0.0 {
                    out.push(PointerEv::Near);
                }
                self.was_near = near;
                // Resting on her a while (once per visit of the pointer).
                if d <= 0.0 {
                    let since = *self.hover_since.get_or_insert(now);
                    if !self.hovered && now.duration_since(since) >= cfg.hover {
                        self.hovered = true;
                        out.push(PointerEv::Hover);
                    }
                } else {
                    self.hover_since = None;
                    self.hovered = false;
                }
                // A fast swipe past her: the segment since the last sample comes close, both
                // ends outside her.
                if let Some((lt, lx, ly)) = self.last {
                    let dt = now.duration_since(lt).as_secs_f32();
                    let len = (((x - lx).pow(2) + (y - ly).pow(2)) as f32).sqrt();
                    if dt > 0.0 && dt < 0.4 && len / dt >= cfg.swipe_speed && d > 0.0 && rect_dist(&her, lx as f32, ly as f32) > 0.0 {
                        let seg = seg_rect_dist(&her, (lx as f32, ly as f32), (x as f32, y as f32));
                        if seg <= cfg.swipe_px {
                            out.push(PointerEv::Swipe(len / dt));
                        }
                    }
                }
                // Lingering: beside her (not on her), still (within `still_px`) a while; once
                // per stop.
                if d > 0.0 && d <= cfg.linger_px {
                    match self.still {
                        Some((_, sx, sy)) if ((x - sx).pow(2) + (y - sy).pow(2)) as f32 <= cfg.still_px * cfg.still_px => {}
                        _ => {
                            self.still = Some((now, x, y));
                            self.lingered = false;
                        }
                    }
                    if let Some((since, ..)) = self.still
                        && !self.lingered
                        && now.duration_since(since) >= cfg.linger
                    {
                        self.lingered = true;
                        out.push(PointerEv::Linger);
                    }
                } else {
                    self.still = None;
                    self.lingered = false;
                }
                // A wiggle: `wiggle_turns` reversals of x, each `wiggle_amp` back from the last
                // extreme, within `wiggle_window`, within `wiggle_px` of her (on her too).
                if d <= cfg.wiggle_px {
                    self.zigzag(cfg, now, x);
                    while self.turns.front().is_some_and(|&t| now.duration_since(t) > cfg.wiggle_window) {
                        self.turns.pop_front();
                    }
                    let quiet = self.wiggled.is_none_or(|w| now.duration_since(w) >= cfg.wiggle_window);
                    if self.turns.len() >= cfg.wiggle_turns as usize && quiet {
                        self.wiggled = Some(now);
                        self.turns.clear();
                        out.push(PointerEv::Wiggle);
                    }
                } else {
                    self.ext = None;
                    self.dir = 0;
                    self.turns.clear();
                }
            }
            None => {
                self.was_near = false;
                self.hover_since = None;
                self.still = None;
                self.ext = None;
                self.dir = 0;
                self.turns.clear();
            }
        }
        self.last = Some((now, x, y));
        (out, far)
    }

    fn zigzag(&mut self, cfg: &PointerCfg, now: Instant, x: i32) {
        let amp = cfg.wiggle_amp as i32;
        let Some(e) = self.ext else {
            self.ext = Some(x);
            return;
        };
        match self.dir {
            0 if (x - e).abs() >= amp => {
                self.dir = if x > e { 1 } else { -1 };
                self.ext = Some(x);
            }
            0 => {}
            1 if x > e => self.ext = Some(x),
            1 if e - x >= amp => {
                self.turns.push_back(now);
                self.dir = -1;
                self.ext = Some(x);
            }
            -1 if x < e => self.ext = Some(x),
            -1 if x - e >= amp => {
                self.turns.push_back(now);
                self.dir = 1;
                self.ext = Some(x);
            }
            _ => {}
        }
    }
}

pub fn rect_dist(r: &WinRect, x: f32, y: f32) -> f32 {
    let dx = (r.x as f32 - x).max(x - r.right() as f32).max(0.0);
    let dy = (r.y as f32 - y).max(y - r.bottom() as f32).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Closest distance from the segment a→b to the rect (sampled: 24 points, enough at 60 ms).
pub fn seg_rect_dist(r: &WinRect, a: (f32, f32), b: (f32, f32)) -> f32 {
    (0..=24).map(|k| k as f32 / 24.0).map(|u| rect_dist(r, a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)).fold(f32::MAX, f32::min)
}

/// The watcher's defaults at sf 1 (scaled by the caller).
pub fn cfg(sf: f32) -> PointerCfg {
    PointerCfg {
        near_px: 150.0 * sf,
        swipe_px: 120.0 * sf,
        swipe_speed: 1500.0 * sf,
        hover: Duration::from_millis(1500),
        linger_px: 150.0 * sf,
        still_px: 12.0 * sf,
        linger: Duration::from_millis(2500),
        wiggle_px: 200.0 * sf,
        wiggle_amp: 30.0 * sf,
        wiggle_window: Duration::from_millis(1200),
        wiggle_turns: 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorded-style trace: (ms, x, y) at the watcher's 60 ms near her.
    fn run(trace: &[(u64, i32, i32)]) -> Vec<(u64, PointerEv)> {
        let t0 = Instant::now();
        let her = WinRect::new(600, 300, 100, 200);
        let cfg = cfg(1.0);
        let mut s = Sense::default();
        let mut out = Vec::new();
        for &(ms, x, y) in trace {
            for e in s.sample(&cfg, t0 + Duration::from_millis(ms), x, y, Some(her), true).0 {
                out.push((ms, e));
            }
        }
        out
    }

    fn still(from: u64, to: u64, x: i32, y: i32) -> Vec<(u64, i32, i32)> {
        (from..to).step_by(60).map(|ms| (ms, x, y)).collect()
    }

    #[test]
    fn a_pointer_resting_beside_her_lingers_once() {
        // Comes in, stops 80 px right of her, a 4 px jitter, stays 6 s.
        let mut tr = vec![(0, 1100, 400), (60, 900, 400)];
        tr.extend(still(120, 6000, 780, 400).into_iter().enumerate().map(|(i, (ms, x, y))| (ms, x + (i % 2) as i32 * 4, y)));
        let ev = run(&tr);
        let lingers: Vec<_> = ev.iter().filter(|e| e.1 == PointerEv::Linger).collect();
        assert_eq!(lingers.len(), 1, "{ev:?}");
        assert!((2600..=2800).contains(&lingers[0].0), "after 2.5 s still: {ev:?}");
        // Moving on and stopping again: a second one.
        let mut tr2 = tr.clone();
        tr2.extend(still(6000, 9000, 830, 330));
        assert_eq!(run(&tr2).iter().filter(|e| e.1 == PointerEv::Linger).count(), 2);
    }

    #[test]
    fn no_linger_on_her_far_off_or_drifting() {
        assert!(run(&still(0, 6000, 650, 400)).iter().all(|e| e.1 != PointerEv::Linger), "on her: a hover, not a linger");
        assert!(run(&still(0, 6000, 1000, 400)).iter().all(|e| e.1 != PointerEv::Linger), "300 px off");
        // Drifting slowly (20 px a second) beside her never settles.
        let drift: Vec<_> = (0..100).map(|k| (k * 60, 760 + (k as i32 * 6) / 5, 400)).collect();
        assert!(run(&drift).iter().all(|e| e.1 != PointerEv::Linger));
    }

    #[test]
    fn a_wiggle_beside_her_reports_once_per_burst() {
        // Left-right 80 px at ~3 Hz, 100 px right of her, 1.5 s.
        let tr: Vec<_> = (0..25).map(|k| (k * 60, 800 + if (k / 3) % 2 == 0 { 0 } else { 80 }, 420)).collect();
        let ev = run(&tr);
        let w: Vec<_> = ev.iter().filter(|e| e.1 == PointerEv::Wiggle).collect();
        assert_eq!(w.len(), 1, "{ev:?}");
        assert!(w[0].0 <= 1000, "{ev:?}");
    }

    #[test]
    fn small_jitter_slow_sweeps_and_far_wiggles_are_not_wiggles() {
        // 20 px back and forth: under the amplitude.
        let jitter: Vec<_> = (0..40).map(|k| (k * 60, 800 + if k % 2 == 0 { 0 } else { 20 }, 420)).collect();
        assert!(run(&jitter).iter().all(|e| e.1 != PointerEv::Wiggle));
        // Three reversals over 3 s: too slow.
        let slow: Vec<_> = (0..60).map(|k| (k * 60, 800 + if (k / 12) % 2 == 0 { 0 } else { 100 }, 420)).collect();
        assert!(run(&slow).iter().all(|e| e.1 != PointerEv::Wiggle));
        // 400 px off her.
        let far: Vec<_> = (0..25).map(|k| (k * 60, 1100 + if (k / 3) % 2 == 0 { 0 } else { 80 }, 420)).collect();
        assert!(run(&far).iter().all(|e| e.1 != PointerEv::Wiggle));
    }

    #[test]
    fn disarmed_nothing_reports() {
        let t0 = Instant::now();
        let cfg = cfg(1.0);
        let mut s = Sense::default();
        let her = WinRect::new(600, 300, 100, 200);
        for k in 0..100u64 {
            let x = 800 + if (k / 3) % 2 == 0 { 0 } else { 80 };
            assert!(s.sample(&cfg, t0 + Duration::from_millis(k * 60), x, 420, Some(her), false).0.is_empty());
        }
    }
}
