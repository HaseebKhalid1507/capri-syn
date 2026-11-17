//! Cut-ins: a show still beside her for a big moment (NEEDS YOU, a long turn done, a laugh, a
//! summon, an error), for about a second: in on a red flash and a tear, out row by row. The
//! images are local (`make-pack --mochi`, `anim/mochi/cutin-*`); this is where and when, and the
//! plate they are cut to. No pixels of her are drawn here: the still is cropped, framed and
//! timed.

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

/// The red flash it comes in on, then its tear, then its exit (ms).
pub const RED_MS: u64 = 60;
pub const TEAR_MS: u64 = 200;
pub const OUT_MS: u64 = 220;

/// Where it is in its life at `t` ms of `ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Its plate in plain red.
    Red,
    /// Tearing into place, `u` 0..1.
    Tear(f32),
    Hold,
    /// Its rows going, `u` 0..1.
    Out(f32),
    Gone,
}

pub fn phase(t: u64, ms: u64) -> Phase {
    let ms = ms.max(RED_MS + TEAR_MS + OUT_MS);
    if t >= ms {
        Phase::Gone
    } else if t < RED_MS {
        Phase::Red
    } else if t < RED_MS + TEAR_MS {
        Phase::Tear((t - RED_MS) as f32 / TEAR_MS as f32)
    } else if t + OUT_MS < ms {
        Phase::Hold
    } else {
        Phase::Out((t + OUT_MS - ms) as f32 / OUT_MS as f32)
    }
}

/// Its top-left (root px): beside her figure (`body`: x, y, w, h), its top a little below the
/// top of her head, on the side with more room in her work area (`area`), else the other, else
/// over her head (`over`: nothing of hers is up there); always inside the area. None: nowhere.
pub fn place(body: (i32, i32, i32, i32), area: (i32, i32, i32, i32), size: (i32, i32), gap: i32, over: bool) -> Option<(i32, i32)> {
    let (bx, by, bw, bh) = body;
    let (ax, ay, aw, ah) = area;
    let (w, h) = size;
    let left = bx - gap - w;
    let right = bx + bw + gap;
    let room_left = bx - ax;
    let room_right = ax + aw - (bx + bw);
    let fits = |x: i32| x >= ax && x + w <= ax + aw;
    let y = (by + bh / 8).clamp(ay, (ay + ah - h).max(ay));
    let order = if room_right >= room_left { [right, left] } else { [left, right] };
    if let Some(&x) = order.iter().find(|&&x| fits(x)) {
        return Some((x, y));
    }
    // No room either side: over her head.
    let x = (bx + bw / 2 - w / 2).clamp(ax, (ax + aw - w).max(ax));
    over.then(|| (x, (by - gap - h).clamp(ay, (ay + ah - h).max(ay))))
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
    fn it_flashes_red_tears_in_holds_and_goes() {
        assert_eq!(phase(0, 1200), Phase::Red);
        assert!(matches!(phase(RED_MS + TEAR_MS / 2, 1200), Phase::Tear(u) if (u - 0.5).abs() < 0.01));
        assert_eq!(phase(600, 1200), Phase::Hold);
        assert!(matches!(phase(1200 - OUT_MS / 2, 1200), Phase::Out(u) if (u - 0.5).abs() < 0.01));
        assert_eq!(phase(1200, 1200), Phase::Gone);
        // Too short for all three: as long as they need.
        assert!(matches!(phase(RED_MS + TEAR_MS + 10, 100), Phase::Out(_)));
        assert_eq!(phase(RED_MS + TEAR_MS + OUT_MS, 100), Phase::Gone);
    }

    #[test]
    fn it_goes_beside_her_on_the_roomier_side_inside_her_area() {
        let area = (0, 0, 2560, 1400);
        // Left of the middle: to her right.
        assert_eq!(place((600, 1000, 300, 360), area, (240, 160), 12, true), Some((912, 1045)));
        // Right of the middle: to her left.
        assert_eq!(place((2000, 1000, 300, 360), area, (240, 160), 12, true), Some((1748, 1045)));
        // Near the right edge with no room there: still to her left.
        assert_eq!(place((2300, 1000, 260, 360), area, (240, 160), 12, true).unwrap().0, 2048);
        // Nowhere beside her: over her head, inside the area; not with a panel up there.
        assert_eq!(place((100, 600, 300, 360), (0, 0, 520, 1400), (240, 160), 12, true), Some((130, 428)));
        assert_eq!(place((100, 600, 300, 360), (0, 0, 520, 1400), (240, 160), 12, false), None);
        // Low on the screen: its bottom stays in the area.
        assert_eq!(place((600, 1300, 300, 360), area, (240, 160), 12, true).unwrap().1, 1240);
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
