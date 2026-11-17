//! The desktop art's motion strips (B2c, PROPOSAL-desktop-art §6): she walks short moves on the
//! floor and along a window top, dances to music with energy, dangles by her hair when he
//! carries her by the head (A/B with the ragdoll), and a click bursts under the pointer. Pure:
//! the walk's path and timing, the dance decision and tempo, the rate caps, the click burst's
//! colour layers. The view (`motionview.rs`) shows them.

use crate::anim::Seq;
use crate::shape::ShapeRect;
use std::time::{Duration, Instant};

/// A rate cap: at most one per `gap`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gap(Option<Instant>);

impl Gap {
    pub fn ready(&self, now: Instant, gap: Duration) -> bool {
        self.0.is_none_or(|t| now.saturating_duration_since(t) >= gap)
    }

    pub fn mark(&mut self, now: Instant) {
        self.0 = Some(now);
    }
}

/// A walk along a line (the floor, a window top): device px, constant pace, eased over its
/// first and last `EASE_S`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walk {
    pub from_x: f32,
    pub to_x: f32,
    pub secs: f32,
}

const EASE_S: f32 = 0.18;

impl Walk {
    /// `pace`: device px per second.
    pub fn new(from_x: f32, to_x: f32, pace: f32) -> Walk {
        let d = (to_x - from_x).abs();
        // The eases cost EASE_S of travel in all: the same pace in the middle.
        let secs = d / pace.max(1.0) + EASE_S;
        Walk { from_x, to_x, secs }
    }

    /// Where her feet are `t` s in.
    pub fn x_at(&self, t: f32) -> f32 {
        let u = progress(t, self.secs);
        self.from_x + (self.to_x - self.from_x) * u
    }

    pub fn done(&self, t: f32) -> bool {
        t >= self.secs
    }
}

/// 0..1 along a walk of `secs`: linear, its ends eased (no jerk at the start or the stop).
fn progress(t: f32, secs: f32) -> f32 {
    if secs <= 0.0 {
        return 1.0;
    }
    let t = t.clamp(0.0, secs);
    let e = EASE_S.min(secs / 2.0);
    // Speed ramps 0 -> v over e, holds, ramps down over the last e: distance in units of v.
    let total = secs - e;
    let d = if t < e {
        t * t / (2.0 * e)
    } else if t > secs - e {
        let r = secs - t;
        total - r * r / (2.0 * e)
    } else {
        t - e / 2.0
    };
    (d / total).clamp(0.0, 1.0)
}

/// The walk's cells: its strip from `first` (`n` cells, `ms` each) round and round, at least
/// `secs` long (the oneshot under the walk; it is stopped where she arrives).
pub fn walk_seq(first: usize, n: u32, ms: u32, secs: f32) -> Seq {
    let cells = ((secs * 1000.0 / ms.max(1) as f32).ceil() as u32).max(1) + 1;
    (0..cells).map(|k| (first + (k % n.max(1)) as usize, ms)).collect()
}

/// Every `step` px from `from` to `to` (both ends included) passes `ok`.
pub fn path_clear(from: f32, to: f32, step: f32, ok: impl Fn(f32) -> bool) -> bool {
    let d = to - from;
    let n = (d.abs() / step.max(1.0)).ceil().max(1.0) as usize;
    (0..=n).all(|k| ok(from + d * k as f32 / n as f32))
}

/// The music has energy: a tempo at or over `min_bpm`; unknown, a roll under `chance` (most
/// players don't send one).
pub fn dances(bpm: Option<f32>, min_bpm: f32, chance: f32, roll: f32) -> bool {
    match bpm {
        Some(b) => b >= min_bpm,
        None => roll < chance,
    }
}

/// A dance cell's time: the strip's own, or two beats per loop when the tempo is known (kept
/// within 0.75-1.4x of the drawing's pace).
pub fn dance_ms(bpm: Option<f32>, strip_ms: u32, cells: u32) -> u32 {
    let Some(b) = bpm.filter(|b| b.is_finite() && *b > 0.0) else { return strip_ms };
    let per = 2.0 * 60_000.0 / b / cells.max(1) as f32;
    let lo = strip_ms as f32 * 0.75;
    let hi = strip_ms as f32 * 1.4;
    per.clamp(lo, hi).round() as u32
}

/// One cell of the click burst as solid-colour layers (no alpha without a compositor): its
/// size, and per colour the rows it covers (cell-local device px).
#[derive(Clone, Debug, Default)]
pub struct BurstCell {
    pub w: i32,
    pub h: i32,
    pub layers: Vec<(u32, Vec<ShapeRect>)>,
}

/// The click strip (`frames` cells of `cell` px in a row), scaled by `scale`, split into two
/// colours (her red and the green): (cells, the burst's centre in scaled px). A pixel counts
/// from `ALPHA_MIN`; its colour class is whichever of red and green is stronger, drawn in that
/// class's mean colour over the whole strip.
pub fn burst_cells(img: &image::RgbaImage, frames: u32, cell: (u32, u32), scale: f32) -> (Vec<BurstCell>, (i32, i32)) {
    const ALPHA_MIN: u8 = 96;
    let (cw, ch) = cell;
    let (w, h) = (((cw as f32 * scale).round() as i32).max(1), ((ch as f32 * scale).round() as i32).max(1));
    let mut sum = [[0u64; 3]; 2];
    let mut cnt = [0u64; 2];
    let (mut cx, mut cy, mut cn) = (0f64, 0f64, 0f64);
    let class = |p: &image::Rgba<u8>| (p[3] >= ALPHA_MIN).then_some(usize::from(p[1] > p[0]));
    for i in 0..frames {
        for y in 0..ch {
            for x in 0..cw {
                let gx = i * cw + x;
                if gx >= img.width() || y >= img.height() {
                    continue;
                }
                let p = img.get_pixel(gx, y);
                if let Some(c) = class(p) {
                    for k in 0..3 {
                        sum[c][k] += p[k] as u64;
                    }
                    cnt[c] += 1;
                    cx += x as f64;
                    cy += y as f64;
                    cn += 1.0;
                }
            }
        }
    }
    let rgb = |c: usize| {
        let n = cnt[c].max(1);
        ((sum[c][0] / n) as u32) << 16 | ((sum[c][1] / n) as u32) << 8 | (sum[c][2] / n) as u32
    };
    let centre = if cn > 0.0 { (((cx / cn) as f32 * scale).round() as i32, ((cy / cn) as f32 * scale).round() as i32) } else { (w / 2, h / 2) };
    let mut out = Vec::with_capacity(frames as usize);
    for i in 0..frames {
        let mut layers = Vec::new();
        for c in 0..2 {
            let mut rows = Vec::new();
            for y in 0..h {
                let sy = ((y as f32 + 0.5) / scale) as u32;
                let mut x = 0;
                let on = |x: i32| {
                    let sx = ((x as f32 + 0.5) / scale) as u32;
                    let gx = i * cw + sx.min(cw - 1);
                    gx < img.width() && sy < img.height() && class(img.get_pixel(gx, sy.min(ch - 1))) == Some(c)
                };
                while x < w {
                    if on(x) {
                        let x0 = x;
                        while x < w && on(x) {
                            x += 1;
                        }
                        rows.push((x0 as i16, y as i16, (x - x0) as u16, 1u16));
                    } else {
                        x += 1;
                    }
                }
            }
            if !rows.is_empty() {
                layers.push((rgb(c), crate::shape::merge_tol(&rows, 0)));
            }
        }
        out.push(BurstCell { w, h, layers });
    }
    (out, centre)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_walk_eases_in_and_out_at_a_steady_pace_between() {
        let w = Walk::new(100.0, 400.0, 100.0);
        assert!((w.secs - 3.18).abs() < 1e-3);
        assert_eq!(w.x_at(0.0), 100.0);
        assert_eq!(w.x_at(w.secs), 400.0);
        assert_eq!(w.x_at(99.0), 400.0);
        // Monotonic, and the middle at full pace.
        let xs: Vec<f32> = (0..=64).map(|k| w.x_at(w.secs * k as f32 / 64.0)).collect();
        assert!(xs.windows(2).all(|p| p[1] >= p[0] - 1e-3));
        let v = (w.x_at(1.6) - w.x_at(1.5)) / 0.1;
        assert!((v - 100.0).abs() < 1.0, "{v}");
        // The first and last frames move less than a full step (eased).
        assert!(w.x_at(1.0 / 30.0) - 100.0 < 100.0 / 30.0 * 0.5);
        assert!(!w.done(3.0) && w.done(3.18));
        // Leftward too.
        let l = Walk::new(400.0, 100.0, 100.0);
        assert_eq!(l.x_at(l.secs), 100.0);
        assert!(l.x_at(1.0) < 400.0);
    }

    #[test]
    fn the_walk_cells_cycle_and_cover_the_walk() {
        let s = walk_seq(10, 12, 83, 2.0);
        assert!(s.iter().map(|c| c.1).sum::<u32>() >= 2000);
        assert_eq!(s[0].0, 10);
        assert_eq!(s[12].0, 10);
        assert_eq!(s[13].0, 11);
        assert!(s.iter().all(|c| (10..22).contains(&c.0)));
    }

    #[test]
    fn path_checks_both_ends_and_between() {
        assert!(path_clear(0.0, 100.0, 16.0, |x| (0.0..=100.0).contains(&x)));
        assert!(!path_clear(0.0, 100.0, 16.0, |x| !(40.0..60.0).contains(&x)));
        assert!(!path_clear(0.0, 100.0, 16.0, |x| x < 100.0));
        assert!(path_clear(5.0, 5.0, 16.0, |x| x == 5.0));
    }

    #[test]
    fn energy_is_the_tempo_else_a_roll() {
        assert!(dances(Some(128.0), 100.0, 0.0, 0.9));
        assert!(!dances(Some(84.0), 100.0, 1.0, 0.0));
        assert!(dances(None, 100.0, 0.5, 0.2));
        assert!(!dances(None, 100.0, 0.5, 0.7));
        // Two beats a loop, within reach of the drawing's own pace.
        assert_eq!(dance_ms(None, 63, 16), 63);
        assert_eq!(dance_ms(Some(120.0), 63, 16), 63);
        assert_eq!(dance_ms(Some(60.0), 63, 16), 88);
        assert_eq!(dance_ms(Some(240.0), 63, 16), 47);
    }

    #[test]
    fn gaps_cap_the_rate() {
        let t = Instant::now();
        let mut g = Gap::default();
        assert!(g.ready(t, Duration::from_secs(5)));
        g.mark(t);
        assert!(!g.ready(t + Duration::from_secs(4), Duration::from_secs(5)));
        assert!(g.ready(t + Duration::from_secs(5), Duration::from_secs(5)));
    }

    #[test]
    fn the_burst_splits_into_its_colours() {
        // Two 8x8 cells: a red 4x4 block, then a green 2x2 one and a faint pixel.
        let mut img = image::RgbaImage::new(16, 8);
        for y in 2..6 {
            for x in 2..6 {
                img.put_pixel(x, y, image::Rgba([250, 10, 60, 255]));
            }
        }
        for y in 3..5 {
            for x in 11..13 {
                img.put_pixel(x, y, image::Rgba([10, 240, 100, 255]));
            }
        }
        img.put_pixel(15, 7, image::Rgba([255, 0, 0, 20]));
        let (cells, centre) = burst_cells(&img, 2, (8, 8), 2.0);
        assert_eq!(cells.len(), 2);
        assert_eq!((cells[0].w, cells[0].h), (16, 16));
        assert_eq!(cells[0].layers.len(), 1);
        let (rgb, rows) = &cells[0].layers[0];
        assert_eq!(*rgb, 0xfa0a3c);
        let area: u32 = rows.iter().map(|r| r.2 as u32 * r.3 as u32).sum();
        assert_eq!(area, 64, "4x4 at 2x");
        assert_eq!(cells[1].layers.len(), 1);
        assert_eq!(cells[1].layers[0].0, 0x0af064);
        // The centre of everything drawn (cell px 3.5,3.5 and 3.5,3.5 -> about 7,7 scaled).
        assert!((centre.0 - 7).abs() <= 1 && (centre.1 - 7).abs() <= 1, "{centre:?}");
    }
}
