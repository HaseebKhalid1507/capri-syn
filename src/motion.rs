//! The desktop art's motion strips (B2c, PROPOSAL-desktop-art §6): she walks short moves on the
//! floor and along a window top, dances to music with energy, dangles by her hair when he
//! carries her by the head (A/B with the ragdoll), and a click bursts under the pointer. Pure:
//! the walk's path and timing, the dance decision and tempo, the rate caps, the click burst's
//! colour layers. The view (`motionview.rs`) shows them.

use crate::anim::{Ranged, Seq};
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

/// Before a walk she leans back this long (the anticipation); after it her momentum carries
/// her forward and back over `SETTLE_MS`.
pub const ANTICIPATE: Duration = Duration::from_millis(170);
pub const SETTLE_MS: u32 = 260;
/// The lean at its widest, logical px at the top of her hair (her feet stay put).
pub const LEAN_PX: f32 = 4.0;

/// A lean of her upper body toward `right` (or away), out and back over `ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lean {
    pub t0: Instant,
    pub right: bool,
    pub ms: u32,
}

impl Lean {
    /// The shift at the top of her hair now (device px, + = right), in 2 px steps; None once
    /// it's over.
    pub fn px(&self, now: Instant, sf: f32) -> Option<i32> {
        let u = now.saturating_duration_since(self.t0).as_secs_f32() * 1000.0 / self.ms.max(1) as f32;
        if u >= 1.0 {
            return None;
        }
        let v = LEAN_PX * sf * (std::f32::consts::PI * u).sin() * if self.right { 1.0 } else { -1.0 };
        Some(((v / 2.0).round() * 2.0) as i32)
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

/// The walk's cells: its strip from `first`, positions each with its own ms (`Strip::played`),
/// round and round, at least `secs` long, then one more (the oneshot under the walk; it is
/// stopped where she arrives). With a range: its entry once, its loop for as long as fits, and
/// its exit done by the time she arrives, its last cell held through it (a walk too short for
/// both drops the entry).
pub fn walk_seq_timed(first: usize, cells: &Ranged<(u32, u32)>, secs: f32) -> Seq {
    if cells.body.is_empty() {
        return Vec::new();
    }
    let want = (secs.max(0.0) * 1000.0) as u64;
    let at = |&(c, ms): &(u32, u32)| (first + c as usize, ms.max(1));
    let len = |s: &[(u32, u32)]| s.iter().map(|c| u64::from(c.1.max(1))).sum::<u64>();
    let mut out = Vec::new();
    if cells.exit.is_empty() {
        out.extend(cells.entry.iter().map(at));
        let mut t = len(&cells.entry);
        for c in cells.body.iter().cycle() {
            out.push(at(c));
            if t >= want {
                break;
            }
            t += u64::from(c.1.max(1));
        }
        return out;
    }
    let exit = len(&cells.exit);
    let mut t = 0;
    if len(&cells.entry) + exit <= want {
        out.extend(cells.entry.iter().map(at));
        t = len(&cells.entry);
    }
    for c in cells.body.iter().cycle() {
        let ms = u64::from(c.1.max(1));
        if t + ms + exit > want {
            break;
        }
        out.push(at(c));
        t += ms;
    }
    out.extend(cells.exit.iter().map(at));
    // Her last exit cell holds until she's there, and a step past it (stopped on arrival).
    let last_at = t + exit - u64::from(cells.exit[cells.exit.len() - 1].1.max(1));
    if let Some(l) = out.last_mut() {
        l.1 = l.1.max((want.saturating_sub(last_at) + u64::from(l.1)).min(u64::from(u32::MAX)) as u32);
    }
    out
}

/// A dance's cells (position, ms) at the music's tempo: the loop two beats long when the tempo is
/// known, each cell keeping its share of it (kept within 0.75-1.4x of the drawing's pace, as
/// `dance_ms`).
pub fn dance_seq(bpm: Option<f32>, cells: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let n = cells.len().max(1) as u32;
    let total = cells.iter().fold(0u32, |t, c| t.saturating_add(c.1));
    let mean = (total / n).max(1);
    let per = dance_ms(bpm, mean, n);
    cells.iter().map(|&(c, ms)| (c, ((ms as f32 * per as f32 / mean as f32).round() as u32).max(1))).collect()
}

/// The position `ms` into a loop of `cells` (position, ms), round and round.
pub fn cell_at(cells: &[(u32, u32)], ms: u64) -> Option<u32> {
    let total: u64 = cells.iter().map(|c| u64::from(c.1.max(1))).sum();
    if total == 0 {
        return None;
    }
    let mut u = ms % total;
    for &(c, d) in cells {
        let d = u64::from(d.max(1));
        if u < d {
            return Some(c);
        }
        u -= d;
    }
    cells.last().map(|c| c.0)
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
    /// Per colour: the ring and other large pieces (shown whole), and the specks (cut to her
    /// body by the view: r32, specks flew off her at the click).
    pub layers: Vec<(u32, Vec<ShapeRect>, Vec<ShapeRect>)>,
}

/// A piece of the burst smaller than this (cell px) is a speck.
const SPECK_PX: usize = 40;

/// The light inner edge of the burst's ring (its dark red alone vanished over her red body).
pub const BURST_EDGE: u32 = 0xfff2f5;

/// The click strip (`frames` cells of `cell` px in a row), scaled by `scale`, split into its
/// colours: her red, the green, and a light edge on the red's inner side (toward the burst's
/// centre): (cells, the burst's centre in scaled px). A pixel counts from `ALPHA_MIN`; red and
/// green are each class's mean colour over the whole strip.
pub fn burst_cells(img: &image::RgbaImage, frames: u32, cell: (u32, u32), scale: f32) -> (Vec<BurstCell>, (i32, i32)) {
    const ALPHA_MIN: u8 = 96;
    const NONE: u8 = 0;
    const RED: u8 = 1;
    const GREEN: u8 = 2;
    const EDGE: u8 = 3;
    let (cw, ch) = cell;
    let (w, h) = (((cw as f32 * scale).round() as i32).max(1), ((ch as f32 * scale).round() as i32).max(1));
    let px = |i: u32, x: u32, y: u32| {
        let gx = i * cw + x;
        (gx < img.width() && y < img.height()).then(|| img.get_pixel(gx, y))
    };
    let class = |p: Option<&image::Rgba<u8>>| match p {
        Some(p) if p[3] >= ALPHA_MIN => if p[1] > p[0] { GREEN } else { RED },
        _ => NONE,
    };
    let mut sum = [[0u64; 3]; 2];
    let mut cnt = [0u64; 2];
    let (mut cx, mut cy, mut cn) = (0f64, 0f64, 0f64);
    for i in 0..frames {
        for y in 0..ch {
            for x in 0..cw {
                let p = px(i, x, y);
                let c = class(p);
                if c != NONE {
                    let p = p.expect("opaque");
                    let k = (c - 1) as usize;
                    for j in 0..3 {
                        sum[k][j] += p[j] as u64;
                    }
                    cnt[k] += 1;
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
    let (ox, oy) = if cn > 0.0 { ((cx / cn) as f32, (cy / cn) as f32) } else { (cw as f32 / 2.0, ch as f32 / 2.0) };
    let centre = ((ox * scale).round() as i32, (oy * scale).round() as i32);
    let mut out = Vec::with_capacity(frames as usize);
    for i in 0..frames {
        // Cell classes, the red's inner edge marked: 2 px toward the centre is empty.
        let mut map = vec![NONE; (cw * ch) as usize];
        for y in 0..ch {
            for x in 0..cw {
                map[(y * cw + x) as usize] = class(px(i, x, y));
            }
        }
        // Pieces: 8-connected runs of one class; small ones are specks.
        let mut speck = vec![false; map.len()];
        let mut seen = vec![false; map.len()];
        for k0 in 0..map.len() {
            if seen[k0] || map[k0] == NONE {
                continue;
            }
            let c = map[k0];
            let (mut stack, mut piece) = (vec![k0], Vec::new());
            seen[k0] = true;
            while let Some(k) = stack.pop() {
                piece.push(k);
                let (x, y) = ((k as u32 % cw) as i32, (k as u32 / cw) as i32);
                for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= cw as i32 || ny >= ch as i32 {
                        continue;
                    }
                    let n = (ny as u32 * cw + nx as u32) as usize;
                    if !seen[n] && map[n] == c {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            if piece.len() < SPECK_PX {
                piece.into_iter().for_each(|k| speck[k] = true);
            }
        }
        let edge: Vec<bool> = (0..cw * ch)
            .map(|k| {
                let (x, y) = ((k % cw) as f32, (k / cw) as f32);
                if map[k as usize] != RED {
                    return false;
                }
                let (dx, dy) = (ox - x, oy - y);
                let d = (dx * dx + dy * dy).sqrt();
                if d < 3.0 {
                    return false;
                }
                if speck[k as usize] {
                    return false;
                }
                let (sx, sy) = ((x + dx / d).round(), (y + dy / d).round());
                sx < 0.0 || sy < 0.0 || sx >= cw as f32 || sy >= ch as f32 || map[(sy as u32 * cw + sx as u32) as usize] == NONE
            })
            .collect();
        for (k, e) in edge.into_iter().enumerate() {
            if e {
                map[k] = EDGE;
            }
        }
        let mut layers = Vec::new();
        for (c, colour) in [(RED, rgb(0)), (GREEN, rgb(1)), (EDGE, BURST_EDGE)] {
            let mut rows = [Vec::new(), Vec::new()];
            for y in 0..h {
                let sy = (((y as f32 + 0.5) / scale) as u32).min(ch - 1);
                let at = |x: i32| {
                    let sx = (((x as f32 + 0.5) / scale) as u32).min(cw - 1);
                    let k = (sy * cw + sx) as usize;
                    (map[k] == c).then_some(speck[k] as usize)
                };
                let mut x = 0;
                while x < w {
                    match at(x) {
                        Some(s) => {
                            let x0 = x;
                            while x < w && at(x) == Some(s) {
                                x += 1;
                            }
                            rows[s].push((x0 as i16, y as i16, (x - x0) as u16, 1u16));
                        }
                        None => x += 1,
                    }
                }
            }
            if rows.iter().any(|r| !r.is_empty()) {
                layers.push((colour, crate::shape::merge_tol(&rows[0], 0), crate::shape::merge_tol(&rows[1], 0)));
            }
        }
        out.push(BurstCell { w, h, layers });
    }
    (out, centre)
}

/// `rows` (cell-local) cut to her body: `body` (her shape's rects) placed at `at` in the
/// cell's coordinates. Nothing of the burst lands off her.
pub fn clip_to(rows: &[ShapeRect], body: &[ShapeRect], at: (i32, i32)) -> Vec<ShapeRect> {
    let mut out = Vec::new();
    for &(x, y, w, h) in rows {
        let (rx0, ry0, rx1, ry1) = (x as i32, y as i32, x as i32 + w as i32, y as i32 + h as i32);
        for &(bx, by, bw, bh) in body {
            let (bx0, by0) = (bx as i32 + at.0, by as i32 + at.1);
            let (x0, y0, x1, y1) = (rx0.max(bx0), ry0.max(by0), rx1.min(bx0 + bw as i32), ry1.min(by0 + bh as i32));
            if x1 > x0 && y1 > y0 {
                out.push((x0 as i16, y0 as i16, (x1 - x0) as u16, (y1 - y0) as u16));
            }
        }
    }
    out
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
        let s = walk_seq_timed(10, &Ranged::whole((0..12).map(|i| (i, 83)).collect()), 2.0);
        assert!(s.iter().map(|c| c.1).sum::<u32>() >= 2000);
        assert_eq!(s[0].0, 10);
        assert_eq!(s[12].0, 10);
        assert_eq!(s[13].0, 11);
        assert!(s.iter().all(|c| (10..22).contains(&c.0)));
    }

    #[test]
    fn per_cell_timing_carries_through_the_walk_the_dance_and_a_held_loop() {
        // A WebP's delays: 100, 300, 100 ms.
        let cells = [(0, 100), (1, 300), (2, 100)];
        let w = walk_seq_timed(10, &Ranged::whole(cells.to_vec()), 0.9);
        assert_eq!(w, vec![(10, 100), (11, 300), (12, 100), (10, 100), (11, 300), (12, 100)], "round and round, past the walk's end");
        assert!(walk_seq_timed(10, &Ranged::whole(Vec::new()), 1.0).is_empty());
        // A range: in once, the loop as long as the exit still ends by her arrival, then out
        // once, its last cell held through the arrival (she's stopped there, so the exit shows).
        let r = Ranged::by_pos(vec![(0, 50), (1, 100), (2, 100), (3, 70)], 4, 1, 1);
        let w = walk_seq_timed(10, &r, 0.5);
        assert_eq!(w, vec![(10, 50), (11, 100), (12, 100), (11, 100), (13, 220)]);
        let starts: Vec<u32> = w.iter().scan(0, |t, c| { let s = *t; *t += c.1; Some(s) }).collect();
        assert!(starts[4] + 70 <= 500, "the exit is through by the arrival (500 ms)");
        assert!(w.iter().map(|c| c.1).sum::<u32>() > 500, "and held past it");
        // A two-cell exit: both shown before she's there.
        let r = Ranged::by_pos(vec![(0, 50), (1, 100), (2, 60), (3, 60)], 4, 1, 2);
        let w = walk_seq_timed(10, &r, 0.4);
        assert_eq!(w, vec![(10, 50), (11, 100), (11, 100), (12, 60), (13, 150)]);
        // Too short for the entry and the exit: the entry goes.
        let r = Ranged::by_pos(vec![(0, 300), (1, 100), (2, 100)], 3, 1, 1);
        assert_eq!(walk_seq_timed(10, &r, 0.25), vec![(11, 100), (12, 250)]);
        // Uniform cells: as `dance_ms`.
        assert_eq!(dance_seq(Some(60.0), &[(0, 63); 16]), vec![(0, 88); 16]);
        // Uneven: each keeps its share; unknown tempo, the drawing's own.
        assert_eq!(dance_seq(None, &cells), cells.to_vec());
        let d = dance_seq(Some(100.0), &cells);
        assert!(d[1].1.abs_diff(3 * d[0].1) <= 2 && d[0].1 == d[2].1 && d[0].1 > 100, "{d:?}");
        assert_eq!([0, 99, 100, 399, 400, 500].map(|t| cell_at(&cells, t)), [0, 0, 1, 1, 2, 0].map(Some));
        assert_eq!(cell_at(&[], 5), None);
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
    fn a_lean_goes_out_and_back_its_way() {
        let t = Instant::now();
        let back = Lean { t0: t, right: false, ms: 170 };
        assert_eq!(back.px(t, 1.0), Some(0));
        assert_eq!(back.px(t + Duration::from_millis(85), 1.0), Some(-4), "widest at the middle, away from the walk");
        assert_eq!(back.px(t + Duration::from_millis(170), 1.0), None);
        let fwd = Lean { t0: t, right: true, ms: SETTLE_MS };
        assert!((0..SETTLE_MS as u64).step_by(10).all(|m| fwd.px(t + Duration::from_millis(m), 2.0).is_some_and(|v| (0..=8).contains(&v) && v % 2 == 0)));
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
        let (rgb, whole, specks) = &cells[0].layers[0];
        assert_eq!(*rgb, 0xfa0a3c);
        assert!(whole.is_empty(), "16 px: a speck");
        let area: u32 = specks.iter().map(|r| r.2 as u32 * r.3 as u32).sum();
        assert_eq!(area, 64, "4x4 at 2x");
        assert_eq!(cells[1].layers.len(), 1);
        assert_eq!(cells[1].layers[0].0, 0x0af064);
        // The centre of everything drawn (cell px 3.5,3.5 and 3.5,3.5 -> about 7,7 scaled).
        assert!((centre.0 - 7).abs() <= 1 && (centre.1 - 7).abs() <= 1, "{centre:?}");
    }

    #[test]
    fn the_rings_inner_edge_is_light_and_the_burst_stays_on_her() {
        // A red ring (radius 6..9) around the middle of a 24x24 cell.
        let mut img = image::RgbaImage::new(24, 24);
        for y in 0..24 {
            for x in 0..24 {
                let d = ((x as f32 - 11.5).powi(2) + (y as f32 - 11.5).powi(2)).sqrt();
                if (6.0..9.0).contains(&d) {
                    img.put_pixel(x, y, image::Rgba([200, 20, 50, 255]));
                }
            }
        }
        let (cells, _) = burst_cells(&img, 1, (24, 24), 1.0);
        let layers = &cells[0].layers;
        assert_eq!(layers.len(), 2, "red and its edge");
        assert!(layers.iter().all(|l| l.2.is_empty()), "the ring is whole, no specks");
        let (edge_rgb, edge, _) = &layers[1];
        assert_eq!(*edge_rgb, BURST_EDGE);
        for &(x, y, w, _) in edge {
            for x in x..x + w as i16 {
                let d = ((x as f32 + 0.5 - 12.0).powi(2) + (y as f32 + 0.5 - 12.0).powi(2)).sqrt();
                assert!(d < 8.0, "({x},{y}) d {d}: the inner side only");
            }
        }
        // Cut to her: only what overlaps her body survives, in the cell's coordinates.
        let body = [(0, 0, 10, 100)];
        let clipped = clip_to(&[(5, 5, 20, 2), (40, 5, 4, 4)], &body, (8, 0));
        assert_eq!(clipped, vec![(8, 5, 10, 2)]);
    }
}
