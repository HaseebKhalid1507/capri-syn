//! Device-pixel shape helpers: scanline-fill polygons into X shape rectangles, so the
//! bounding shape (no compositor) follows rounded corners, the bubble tail and hearts.

pub type ShapeRect = (i16, i16, u16, u16);

/// Scanline fill (even-odd) of a polygon in device px.
pub fn poly_rows(pts: &[(f32, f32)], out: &mut Vec<ShapeRect>) {
    if pts.len() < 3 {
        return;
    }
    let ymin = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i32;
    let ymax = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i32;
    let mut xs: Vec<f32> = Vec::new();
    for y in ymin..ymax {
        let yc = y as f32 + 0.5;
        xs.clear();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.1 <= yc && yc < b.1) || (b.1 <= yc && yc < a.1) {
                xs.push(a.0 + (yc - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let (x0, x1) = (pair[0].round() as i32, pair[1].round() as i32);
            if x1 > x0 {
                out.push((x0 as i16, y as i16, (x1 - x0) as u16, 1));
            }
        }
    }
}

/// Classic parametric heart, `size` px wide, centred on (cx, cy).
pub fn heart(cx: f32, cy: f32, size: f32) -> Vec<(f32, f32)> {
    let k = size / 32.0;
    (0..40)
        .map(|i| {
            let t = i as f32 / 40.0 * std::f32::consts::TAU;
            let x = 16.0 * t.sin().powi(3);
            let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (cx + x * k, cy - y * k - 1.5 * k)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_fills() {
        let mut out = Vec::new();
        poly_rows(&[(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)], &mut out);
        assert_eq!(out, vec![(0, 0, 10, 1), (0, 1, 10, 1), (0, 2, 10, 1), (0, 3, 10, 1)]);
    }
}

/// Vertically adjacent rects merged while each edge stays within `tol` px across the rows
/// merged; a merged rect is their intersection, so the result never covers more than the
/// input (inward only: no region that wasn't painted).
pub fn merge_tol(rects: &[ShapeRect], tol: i32) -> Vec<ShapeRect> {
    #[derive(Clone, Copy)]
    struct Open {
        y0: i32,
        y1: i32,
        lo0: i32,
        hi0: i32,
        lo1: i32,
        hi1: i32,
    }
    let mut v: Vec<(i32, i32, i32, i32)> = rects.iter().map(|r| (r.0 as i32, r.1 as i32, r.0 as i32 + r.2 as i32, r.1 as i32 + r.3 as i32)).collect();
    v.sort_unstable_by_key(|r| (r.1, r.0));
    let mut open: Vec<Open> = Vec::new();
    let mut out: Vec<ShapeRect> = Vec::new();
    let close = |o: &Open, out: &mut Vec<ShapeRect>| {
        if o.lo1 > o.hi0 && o.y1 > o.y0 {
            out.push((o.hi0 as i16, o.y0 as i16, (o.lo1 - o.hi0) as u16, (o.y1 - o.y0) as u16));
        }
    };
    let mut i = 0;
    while i < v.len() {
        let y = v[i].1;
        let mut j = i;
        while j < v.len() && v[j].1 == y {
            j += 1;
        }
        // Opens that end above this row can't grow any more.
        let (keep, done): (Vec<Open>, Vec<Open>) = open.into_iter().partition(|o| o.y1 == y);
        for o in &done {
            close(o, &mut out);
        }
        let mut avail = keep;
        let mut next: Vec<Open> = Vec::new();
        for &(x0, _, x1, y1) in &v[i..j] {
            let k = avail.iter().position(|o| {
                o.lo0.min(x0) + tol >= o.hi0.max(x0) && o.lo1.min(x1) + tol >= o.hi1.max(x1) && x1.min(o.lo1) > x0.max(o.hi0)
            });
            match k {
                Some(k) => {
                    let mut o = avail.swap_remove(k);
                    o.y1 = y1;
                    o.lo0 = o.lo0.min(x0);
                    o.hi0 = o.hi0.max(x0);
                    o.lo1 = o.lo1.min(x1);
                    o.hi1 = o.hi1.max(x1);
                    next.push(o);
                }
                None => next.push(Open { y0: y, y1, lo0: x0, hi0: x0, lo1: x1, hi1: x1 }),
            }
        }
        for o in &avail {
            close(o, &mut out);
        }
        open = next;
        i = j;
    }
    for o in &open {
        close(o, &mut out);
    }
    out
}

/// Whether (x, y) is inside any of the rects.
#[cfg(test)]
pub fn hit(rects: &[ShapeRect], x: i32, y: i32) -> bool {
    rects.iter().any(|&(rx, ry, rw, rh)| x >= i32::from(rx) && y >= i32::from(ry) && x < i32::from(rx) + i32::from(rw) && y < i32::from(ry) + i32::from(rh))
}

/// One rect around all of `rects`.
pub fn bbox(rects: &[ShapeRect]) -> Option<ShapeRect> {
    let x0 = rects.iter().map(|r| r.0 as i32).min()?;
    let y0 = rects.iter().map(|r| r.1 as i32).min()?;
    let x1 = rects.iter().map(|r| r.0 as i32 + r.2 as i32).max()?;
    let y1 = rects.iter().map(|r| r.1 as i32 + r.3 as i32).max()?;
    Some((x0 as i16, y0 as i16, (x1 - x0) as u16, (y1 - y0) as u16))
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn covered(r: &[ShapeRect]) -> std::collections::HashSet<(i32, i32)> {
        r.iter().flat_map(|&(x, y, w, h)| (y as i32..y as i32 + h as i32).flat_map(move |yy| (x as i32..x as i32 + w as i32).map(move |xx| (xx, yy)))).collect()
    }

    #[test]
    fn merge_tol_is_inward_and_fewer() {
        // A wobbly edge: 40 one-px rows, edges jittering by 1 px, two runs per row.
        let mut rows = Vec::new();
        for y in 0..40i16 {
            let j = (y % 3) - 1;
            rows.push((10 + j, y, (30 - j) as u16, 1));
            rows.push((60 - j, y, (20 + j) as u16, 1));
        }
        let m = merge_tol(&rows, 2);
        assert!(m.len() * 10 <= rows.len(), "{} -> {}", rows.len(), m.len());
        let (a, b) = (covered(&rows), covered(&m));
        assert!(b.is_subset(&a), "never more than the input");
        assert!(b.len() * 100 >= a.len() * 85, "eroded at most a little: {} of {}", b.len(), a.len());
        assert_eq!(bbox(&rows), Some((9, 0, 71, 40)));
    }
}
