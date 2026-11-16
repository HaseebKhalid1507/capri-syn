//! Where a panel opens beside her (the Line, the reader). Pure: device px in, a rect out.
//!
//! She never moves for it (Haseeb: "the big ass chat box should open next to her"): the panel
//! goes on the side of her with more free room on her monitor, its bottom level with hers,
//! wholly inside her work area. Both sides too tight and she is on the floor: above her. Its
//! tail points at her. Wherever she goes while it is open, it is placed again from here.

use crate::edges::WinRect;

/// Which side of her the panel is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
}

/// What to place (device px): the width it wants and the least it works at, its height, and
/// the room it needs above her to go there (at least `h`: it is never squeezed there, the
/// Line's input row is at its bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub w: i32,
    pub min_w: i32,
    pub h: i32,
    pub min_h: i32,
}

/// A panel's place: its rect, its side of her, and where its tail points along the edge
/// facing her (px from the panel's top for `Left`/`Right`, from its left for `Above`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub rect: WinRect,
    pub side: Side,
    pub tail: i32,
}

/// The tail keeps this far from the panel's corners (device px; less on a tiny panel).
const TAIL_PAD: i32 = 24;

/// Place `p` beside `her` (her body box) in `area` (her monitor's work area), `gap` apart.
/// `floor`: she is on the floor (above her is allowed when both sides are tight). `avoid`: a
/// point (the pointer) the panel would rather not cover, if the other side has room too.
pub fn place(area: WinRect, her: WinRect, p: Panel, gap: i32, floor: bool, avoid: Option<(i32, i32)>) -> Place {
    let w = p.w.clamp(1, area.w.max(1));
    let h = p.h.clamp(1, area.h.max(1));
    let min_w = p.min_w.clamp(1, w);
    let room = |s: Side| match s {
        Side::Right => area.right() - her.right() - gap,
        Side::Left => her.x - gap - area.x,
        Side::Above => her.y - gap - area.y,
    };
    let beside = |s: Side| {
        // As wide as it wants, down to its least to stay clear of her; still inside the area.
        let pw = w.min(room(s).max(min_w));
        let x = if s == Side::Right { her.right() + gap } else { her.x - gap - pw };
        let x = x.clamp(area.x, (area.right() - pw).max(area.x));
        let y = (her.bottom() - h).clamp(area.y, (area.bottom() - h).max(area.y));
        WinRect::new(x, y, pw, h)
    };
    let fits = |s: Side| room(s) >= min_w;
    let under = |r: &WinRect| avoid.is_some_and(|c| r.contains(c.0, c.1));
    let (first, second) = if room(Side::Right) >= room(Side::Left) { (Side::Right, Side::Left) } else { (Side::Left, Side::Right) };
    let side = if fits(first) {
        if under(&beside(first)) && fits(second) && !under(&beside(second)) { second } else { first }
    } else if floor && room(Side::Above) >= p.min_h.max(h) {
        Side::Above
    } else {
        first
    };
    let rect = if side == Side::Above {
        let x = (her.center().0 - w / 2).clamp(area.x, (area.right() - w).max(area.x));
        WinRect::new(x, her.y - gap - h, w, h)
    } else {
        beside(side)
    };
    // The tail: the middle of the stretch of edge that faces her, kept off the corners.
    let (lo, hi, len, at) = match side {
        Side::Above => (her.x.max(rect.x), her.right().min(rect.right()), rect.w, rect.x),
        _ => (her.y.max(rect.y), her.bottom().min(rect.bottom()), rect.h, rect.y),
    };
    let mid = if lo < hi { (lo + hi) / 2 } else if side == Side::Above { her.center().0 } else { her.center().1 };
    let pad = TAIL_PAD.min(len / 2);
    Place { rect, side, tail: (mid - at).clamp(pad, (len - pad).max(pad)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// bella's DP-3 work area under a 29 px bar, and her standing / sitting boxes (sf 1).
    const AREA: WinRect = WinRect { id: 0, x: 2560, y: 29, w: 2560, h: 1411 };
    const LINE: Panel = Panel { w: 1170, min_w: 720, h: 600, min_h: 90 };

    fn standing(x: i32) -> WinRect {
        // Feet at the floor (area bottom - 12), 240 x 375.
        WinRect::new(x - 120, 1428 - 370, 240, 375)
    }

    fn inside(r: &WinRect, a: &WinRect) -> bool {
        r.x >= a.x && r.y >= a.y && r.right() <= a.right() && r.bottom() <= a.bottom()
    }

    fn clear(r: &WinRect, her: &WinRect) -> bool {
        r.intersect(her).is_none()
    }

    #[test]
    fn on_the_floor_the_roomier_side_bottom_level_with_her() {
        // Left third of the monitor: more room on her right.
        let her = standing(3100);
        let p = place(AREA, her, LINE, 12, true, None);
        assert_eq!(p.side, Side::Right);
        assert_eq!((p.rect.x, p.rect.w, p.rect.bottom()), (her.right() + 12, 1170, her.bottom()));
        assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her));
        // Right third: her left.
        let her = standing(4600);
        let p = place(AREA, her, LINE, 12, true, None);
        assert_eq!(p.side, Side::Left);
        assert_eq!((p.rect.right(), p.rect.bottom()), (her.x - 12, her.bottom()));
        assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her));
    }

    #[test]
    fn at_a_screen_edge_it_opens_inward() {
        for (x, side) in [(2560 + 130, Side::Right), (5120 - 130, Side::Left)] {
            let her = standing(x);
            let p = place(AREA, her, LINE, 12, true, None);
            assert_eq!(p.side, side, "{x}");
            assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her), "{x}: {p:?}");
        }
    }

    #[test]
    fn on_a_perch_beside_her_seat_inside_the_area() {
        // Sitting on a window top at y 738 (her seat box 300 x 380 around it).
        let her = WinRect::new(4595 - 140, 738 - 250, 300, 380);
        let p = place(AREA, her, LINE, 12, false, None);
        assert_eq!(p.side, Side::Left);
        assert_eq!(p.rect.bottom(), her.bottom());
        assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her));
        // Taller than the room above her bottom: pushed down, still whole, still beside her.
        let tall = Panel { h: 1000, ..LINE };
        let p = place(AREA, her, tall, 12, false, None);
        assert_eq!((p.rect.y, p.rect.h), (AREA.y, 1000));
        assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her));
    }

    #[test]
    fn clinging_high_on_a_window_edge() {
        // Gripping a window's side near the top bar: the panel can't sit higher than the area.
        let her = WinRect::new(3800, 40, 300, 372);
        let p = place(AREA, her, LINE, 12, false, None);
        assert!(inside(&p.rect, &AREA) && clear(&p.rect, &her), "{p:?}");
        assert_eq!(p.rect.y, AREA.y);
    }

    #[test]
    fn tight_sides_narrow_it_down_then_above_her_on_the_floor() {
        // 1300 px of room on her right: the Line narrows to it.
        let area = WinRect::new(0, 29, 1920, 1051);
        let her = WinRect::new(380, 1068 - 370, 240, 375);
        let p = place(area, her, LINE, 12, true, None);
        assert_eq!((p.side, p.rect.w), (Side::Right, 1170));
        let her = WinRect::new(1920 - 1000, 1068 - 370, 240, 375);
        let p = place(area, her, LINE, 12, true, None);
        assert_eq!((p.side, p.rect.x, p.rect.w), (Side::Left, 0, her.x - 12));
        // In the middle of a narrow monitor (neither side has 720 px): above her.
        let area = WinRect::new(0, 29, 1280, 771);
        let her = WinRect::new(520, 788 - 370, 240, 375);
        let short = Panel { h: 300, ..LINE };
        let p = place(area, her, short, 12, true, None);
        assert_eq!(p.side, Side::Above);
        assert_eq!((p.rect.bottom(), p.rect.h), (her.y - 12, 300));
        assert!(inside(&p.rect, &area) && clear(&p.rect, &her), "{p:?}");
        // The same off the floor (a perch): the roomier side, overlapping her if it must.
        let p = place(area, her, short, 12, false, None);
        assert_ne!(p.side, Side::Above);
        assert!(inside(&p.rect, &area), "{p:?}");
        // Not room enough above for all of it (or for what it will grow to): never squeezed
        // there (its input row would be cut off), the roomier side instead.
        for tall in [Panel { h: 600, ..LINE }, Panel { h: 300, min_h: 450, ..LINE }] {
            let p = place(area, her, tall, 12, true, None);
            assert_ne!(p.side, Side::Above, "{tall:?}");
            assert!(inside(&p.rect, &area) && p.rect.h == tall.h, "{p:?}");
        }
    }

    #[test]
    fn the_pointer_tips_it_to_the_other_side_if_that_has_room() {
        let her = standing(3840);
        let first = place(AREA, her, LINE, 12, true, None);
        let c = first.rect.center();
        let p = place(AREA, her, LINE, 12, true, Some(c));
        assert_ne!(p.side, first.side);
        assert!(!p.rect.contains(c.0, c.1) && inside(&p.rect, &AREA));
        // No room on the other side: it stays where it fits.
        let her = standing(2560 + 130);
        let first = place(AREA, her, LINE, 12, true, None);
        let c = first.rect.center();
        assert_eq!(place(AREA, her, LINE, 12, true, Some(c)).side, first.side);
    }

    #[test]
    fn the_tail_points_at_her() {
        let her = standing(3100);
        let p = place(AREA, her, LINE, 12, true, None);
        let y = p.rect.y + p.tail;
        assert!(y >= her.y && y < her.bottom(), "the tail's height is hers: {p:?}");
        let area = WinRect::new(0, 29, 1280, 771);
        let her = WinRect::new(520, 788 - 370, 240, 375);
        let p = place(area, her, Panel { h: 300, ..LINE }, 12, true, None);
        assert_eq!(p.side, Side::Above);
        let x = p.rect.x + p.tail;
        assert!(x >= her.x && x < her.right(), "above: the tail is over her: {p:?}");
    }

    #[test]
    fn always_whole_inside_her_work_area() {
        // Every spot, size and kind: never off her monitor, never clipped.
        let mut seed = 0x2545_f491u64;
        let mut rnd = |n: i32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as i32
        };
        for _ in 0..20_000 {
            let area = WinRect::new(rnd(3000), rnd(60), 600 + rnd(2600), 500 + rnd(1200));
            let (bw, bh) = (120 + rnd(200), 150 + rnd(300));
            let her = WinRect::new(area.x + rnd(area.w - bw), area.y + rnd(area.h - bh), bw, bh);
            let p = Panel { w: 200 + rnd(1600), min_w: 100 + rnd(900), h: 60 + rnd(1500), min_h: 60 };
            let pl = place(area, her, p, rnd(30), rnd(2) == 0, None);
            assert!(inside(&pl.rect, &area), "{area:?} {her:?} {p:?} -> {pl:?}");
            assert!(pl.rect.w >= 1 && pl.rect.h >= 1);
            let len = if pl.side == Side::Above { pl.rect.w } else { pl.rect.h };
            assert!((0..=len).contains(&pl.tail), "{pl:?}");
        }
    }
}
