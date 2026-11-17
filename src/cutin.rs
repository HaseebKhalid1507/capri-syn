//! Cut-ins: a show still beside her for a big moment (NEEDS YOU, a long turn done, a laugh, a
//! summon, an error), for about a second: its rows torn in (the still itself from the first
//! frame), out row by row. The images are local (`make-pack --mochi`, `anim/mochi/cutin-*`);
//! this is where and when, and the plate they are cut to. No pixels of her are drawn here: the
//! still is cropped, framed and timed.

use crate::theme::{self, Cut};

/// The moments a cut-in marks (`moods.json` `cutins.beats` keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beat {
    NeedsYou,
    Done,
    Laugh,
    Summon,
    Annoyed,
}

impl Beat {
    pub fn key(self) -> &'static str {
        match self {
            Beat::NeedsYou => "needsYou",
            Beat::Done => "done",
            Beat::Laugh => "laugh",
            Beat::Summon => "summon",
            Beat::Annoyed => "annoyed",
        }
    }
}

/// Its entrance (rows torn in), then its exit (rows going), ms.
pub const IN_MS: u64 = 200;
pub const OUT_MS: u64 = 200;
/// The share of its rows on its first frame (r35: a flat red plate first read as a missing
/// texture), and of its exit after which the rest go at once (r35: its last row lingered, a
/// thin line where it had been).
pub const IN_FROM: f32 = 0.35;
pub const OUT_TO: f32 = 0.8;

/// Where it is in its life at `t` ms of `ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Its rows coming in, `u` `IN_FROM`..1.
    In(f32),
    Hold,
    /// Its rows going, `u` 0..`OUT_TO`.
    Out(f32),
    Gone,
}

pub fn phase(t: u64, ms: u64) -> Phase {
    let ms = ms.max(IN_MS + OUT_MS);
    let out = (t + OUT_MS).saturating_sub(ms) as f32 / OUT_MS as f32;
    if t >= ms || out >= OUT_TO {
        Phase::Gone
    } else if t < IN_MS {
        Phase::In(IN_FROM + (1.0 - IN_FROM) * t as f32 / IN_MS as f32)
    } else if t + OUT_MS < ms {
        Phase::Hold
    } else {
        Phase::Out(out)
    }
}

/// The rows torn in at `u` (`IN_FROM`..1): `(y0, y1, dx, 0)` bands in a random order (`seed`),
/// each a little off sideways (`jolt` px at most) until it settles, all of them in place at 1.
pub fn in_bands(h: i32, u: f32, seed: u64, band: i32, jolt: i32) -> Vec<(i32, i32, i32, i32)> {
    let (band, u) = (band.max(1), u.clamp(0.0, 1.0));
    let mut out = Vec::new();
    let (mut y, mut k) = (0, 0u64);
    while y < h {
        let y1 = (y + band).min(h);
        let r = mix(seed ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let at = (r >> 40) as f32 / (1u64 << 24) as f32;
        if at <= u {
            // Settling: off by up to `jolt` when it comes in, in place by the end.
            let left = 1.0 - u;
            let dx = (((r >> 8) & 0xff) as f32 / 127.5 - 1.0) * jolt as f32 * left * 2.0;
            out.push((y, y1, dx.round() as i32, 0));
        }
        y = y1;
        k += 1;
    }
    out
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 29;
    x
}

type R = (i32, i32, i32, i32);

/// Pixels of `a` that `b` covers.
fn overlap(a: R, b: R) -> i64 {
    let w = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let h = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    if w > 0 && h > 0 { w as i64 * h as i64 } else { 0 }
}

/// Its top-left (root px), inside her work area (`area`): beside her figure (`body`: x, y, w,
/// h), its top a little below the top of her head, or a step up or down; else over her head
/// (`over`: nothing of hers is up there). Of those, the one covering the least of the windows
/// (`windows`: r35, it sat on his document with free desktop on her other side); among equals,
/// her roomier side, then nearest her head. None: nowhere fits.
pub fn place(body: R, area: R, size: (i32, i32), gap: i32, windows: &[R], over: bool) -> Option<(i32, i32)> {
    let (bx, by, bw, bh) = body;
    let (ax, ay, aw, ah) = area;
    let (w, h) = size;
    let (left, right) = (bx - gap - w, bx + bw + gap);
    let (room_left, room_right) = (bx - ax, ax + aw - (bx + bw));
    let sides = if room_right >= room_left { [right, left] } else { [left, right] };
    let y_in = |y: i32| y.clamp(ay, (ay + ah - h).max(ay));
    let y0 = by + bh / 8;
    let mut cands: Vec<(i32, i32)> = Vec::new();
    for &x in &sides {
        if x >= ax && x + w <= ax + aw {
            for k in [0, -1, 1, -2, 2] {
                cands.push((x, y_in(y0 + k * h / 2)));
            }
        }
    }
    if over {
        let x = (bx + bw / 2 - w / 2).clamp(ax, (ax + aw - w).max(ax));
        cands.push((x, y_in(by - gap - h)));
    }
    let cost = |&(x, y): &(i32, i32)| windows.iter().map(|&r| overlap((x, y, w, h), r)).sum::<i64>();
    // The first of the least covering (the order is the preference).
    cands.iter().enumerate().min_by_key(|(i, c)| (cost(c), *i)).map(|(_, &c)| c)
}

/// The plate's corner cut (px) for a cut-in `h` px tall.
pub fn cut(h: u32) -> f32 {
    (h as f32 * 0.12).round()
}

/// The still cut to its plate (chamfered top-left and bottom-right, `theme::Cut::Corners`) with
/// a `rim` px rim of `rim_rgb` along the cut edges and the sides: RGBA, outside it transparent.
pub fn frame(img: &mut image::RgbaImage, rim: u32, rim_rgb: u32) {
    let (w, h) = img.dimensions();
    let k = cut(h);
    let outer = theme::outline(Cut::Corners(k), 0.0, 0.0, w as f32, h as f32);
    let r = rim as f32;
    let inner = theme::outline(Cut::Corners((k - r * 0.4).max(0.0)), r, r, w as f32 - r, h as f32 - r);
    // Per row, the span inside (a convex plate: one run a row).
    let spans = |polys: &[Vec<(f32, f32)>]| {
        let mut rows = Vec::new();
        for p in polys {
            crate::shape::poly_rows(p, &mut rows);
        }
        let mut v = vec![(0i32, 0i32); h as usize];
        for r in rows {
            for y in (r.1 as i32).max(0)..(r.1 as i32 + r.3 as i32).min(h as i32) {
                v[y as usize] = (r.0 as i32, r.0 as i32 + r.2 as i32);
            }
        }
        v
    };
    let (out_rows, in_rows) = (spans(&outer), spans(&inner));
    let inside = |rows: &[(i32, i32)], x: u32, y: u32| (rows[y as usize].0..rows[y as usize].1).contains(&(x as i32));
    let c = [(rim_rgb >> 16) as u8, (rim_rgb >> 8) as u8, rim_rgb as u8, 255];
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel_mut(x, y);
            if !inside(&out_rows, x, y) {
                *p = image::Rgba([0, 0, 0, 0]);
            } else if !inside(&in_rows, x, y) {
                *p = image::Rgba(c);
            } else {
                p.0[3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_tears_in_holds_and_goes() {
        assert_eq!(phase(0, 1200), Phase::In(IN_FROM), "rows of the still from the first frame");
        assert!(matches!(phase(IN_MS / 2, 1200), Phase::In(u) if u > IN_FROM && u < 1.0));
        assert_eq!(phase(600, 1200), Phase::Hold);
        assert!(matches!(phase(1200 - OUT_MS / 2, 1200), Phase::Out(u) if (u - 0.5).abs() < 0.01));
        // Its last rows go together (no lone line left behind).
        assert_eq!(phase(1200 - OUT_MS + (OUT_MS as f32 * OUT_TO) as u64, 1200), Phase::Gone);
        assert_eq!(phase(1200, 1200), Phase::Gone);
        // Too short for both: as long as they need.
        assert!(matches!(phase(IN_MS + 10, 100), Phase::Out(_)));
    }

    #[test]
    fn its_rows_come_in_settling_and_all_are_in_at_the_end() {
        let first = in_bands(160, IN_FROM, 7, 8, 6);
        let n = first.iter().map(|b| b.1 - b.0).sum::<i32>();
        assert!(n > 20 && n < 100, "{n} of 160 rows on the first frame");
        let all = in_bands(160, 1.0, 7, 8, 6);
        assert_eq!(all.iter().map(|b| b.1 - b.0).sum::<i32>(), 160);
        assert!(all.iter().all(|b| b.2 == 0 && b.3 == 0), "in place at the end");
        assert!(first.iter().all(|b| b.2.abs() <= 8));
        // Rows only ever come in.
        let mid = in_bands(160, 0.6, 7, 8, 6);
        assert!(first.iter().all(|f| mid.iter().any(|m| m.0 == f.0)));
    }

    #[test]
    fn it_goes_beside_her_on_the_roomier_side_inside_her_area() {
        let area = (0, 0, 2560, 1400);
        // Left of the middle: to her right.
        assert_eq!(place((600, 1000, 300, 360), area, (240, 160), 12, &[], true), Some((912, 1045)));
        // Right of the middle: to her left.
        assert_eq!(place((2000, 1000, 300, 360), area, (240, 160), 12, &[], true), Some((1748, 1045)));
        // Near the right edge with no room there: still to her left.
        assert_eq!(place((2300, 1000, 260, 360), area, (240, 160), 12, &[], true).unwrap().0, 2048);
        // Nowhere beside her: over her head, inside the area; not with a panel up there.
        assert_eq!(place((100, 600, 300, 360), (0, 0, 520, 1400), (240, 160), 12, &[], true), Some((130, 428)));
        assert_eq!(place((100, 600, 300, 360), (0, 0, 520, 1400), (240, 160), 12, &[], false), None);
        // Low on the screen: its bottom stays in the area.
        assert_eq!(place((600, 1300, 300, 360), area, (240, 160), 12, &[], true).unwrap().1, 1240);
    }

    #[test]
    fn it_keeps_off_his_windows_when_there_is_room() {
        let (area, her) = ((0, 0, 2560, 1400), (600, 1000, 300, 360));
        // His document on her roomier side: the other side.
        let doc = (1100, 760, 1100, 560);
        assert_eq!(place(her, area, (240, 160), 12, &[doc], true), Some((348, 1045)));
        // Both sides taken where her head is: a step up, clear of them.
        let left = (0, 900, 590, 400);
        let p = place(her, area, (240, 160), 12, &[doc, left], true).unwrap();
        assert!([doc, left].iter().all(|&r| overlap((p.0, p.1, 240, 160), r) == 0), "{p:?}");
        // No room anywhere: the least covering, still beside her.
        let all = (0, 0, 2560, 1400);
        assert_eq!(place(her, area, (240, 160), 12, &[all], true), Some((912, 1045)));
        // A window over the whole area changes nothing.
        assert_eq!(place(her, area, (240, 160), 12, &[all, doc], true), Some((348, 1045)));
    }

    #[test]
    fn the_plate_is_cut_and_rimmed() {
        let mut img = image::RgbaImage::from_pixel(100, 50, image::Rgba([10, 20, 30, 255]));
        frame(&mut img, 3, 0xff003d);
        assert_eq!(img.get_pixel(0, 0).0[3], 0, "the top-left corner is cut");
        assert_eq!(img.get_pixel(99, 49).0[3], 0, "and the bottom-right");
        assert_eq!(img.get_pixel(99, 0).0, [0xff, 0x00, 0x3d, 255], "a square corner is rim");
        assert_eq!(img.get_pixel(50, 1).0, [0xff, 0x00, 0x3d, 255], "the rim runs along the top");
        assert_eq!(img.get_pixel(50, 25).0, [10, 20, 30, 255], "the still inside, as it is");
    }
}
