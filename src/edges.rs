//! Window edges she can sit on, and the work areas she stands in. Pure (no X): the view
//! feeds it `X11::stack()` (bottom → top) and the dock struts.
//!
//! An edge is only usable where nothing stacked above the window hides it: a seat behind
//! another window would look like she sits in the middle of that window.

/// A window's (or any) rect in root px. `id` is the X window (0 for plain rects).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WinRect {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl WinRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> WinRect {
        WinRect { id: 0, x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
    /// Grown by `d` on every side.
    pub fn grow(&self, d: i32) -> WinRect {
        WinRect { id: self.id, x: self.x - d, y: self.y - d, w: self.w + 2 * d, h: self.h + 2 * d }
    }
    /// The overlap, if any.
    pub fn intersect(&self, o: &WinRect) -> Option<WinRect> {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        (x0 < x1 && y0 < y1).then(|| WinRect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// Device px: a window whose bottom/top sits within this of an edge line covers it.
pub const OCCLUDE_PAD_Y: i32 = 2;

/// A stretch of exposed top edge she can land on (flights, passive perches).
#[cfg_attr(not(test), allow(dead_code))] // the ragdoll's flights land on these (R-P3)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub win: u32,
    pub y: i32,
    /// Seat x range (root px, inclusive).
    pub seat_x: (i32, i32),
}

/// Exposed x-intervals [x0, x1) of `stack[i]`'s top edge (row `stack[i].y`). `stack` is
/// bottom → top. A higher window h covers column x iff `h.x <= x < h.right()` and
/// `h.y - PAD <= r.y < h.bottom() + PAD`. Covered intervals are merged, then subtracted.
pub fn exposed_spans(stack: &[WinRect], i: usize) -> Vec<(i32, i32)> {
    let Some(r) = stack.get(i) else { return Vec::new() };
    let mut cov: Vec<(i32, i32)> = stack[i + 1..]
        .iter()
        .filter(|h| h.y - OCCLUDE_PAD_Y <= r.y && r.y < h.bottom() + OCCLUDE_PAD_Y)
        .map(|h| (h.x.max(r.x), h.right().min(r.right())))
        .filter(|(a, b)| a < b)
        .collect();
    cov.sort_unstable();
    let mut out = Vec::new();
    let mut x = r.x;
    for (a, b) in cov {
        if a > x {
            out.push((x, a));
        }
        x = x.max(b);
    }
    if x < r.right() {
        out.push((x, r.right()));
    }
    out
}

/// Seat-x ranges (root px, inclusive) on `stack[i]`: each exposed span [s0, s1) becomes
/// [s0 + margin - lo, s1 - margin - hi], where (lo, hi) = the contact extent relative to the
/// seat and margin = EDGE_MARGIN·sf. So the margin holds at the window's corners and at
/// every occluder's side. Empty ranges are dropped. With no occluder this is the old
/// whole-edge perch range.
pub fn seat_ranges(stack: &[WinRect], i: usize, contact: (i32, i32), margin: i32) -> Vec<(i32, i32)> {
    exposed_spans(stack, i)
        .into_iter()
        .filter_map(|(s0, s1)| {
            let (lo, hi) = (s0 + margin - contact.0, s1 - margin - contact.1);
            (lo <= hi).then_some((lo, hi))
        })
        .collect()
}

/// Release snap. Candidates top → bottom (eligible only). A candidate needs the feet x
/// inside it (20 px in from both sides) and the seat or the feet within `tol` of its top
/// edge, as before, AND the feet x in an EXPOSED span. The seat is then clamped into that
/// span's seat range only (never slid across an occluder). A covered or unfit candidate
/// falls through to the next lower one. `fits(i, seat_x)` is the caller's head-room /
/// fullscreen check. Returns (index, seat x).
#[allow(clippy::too_many_arguments)]
pub fn choose_snap(
    stack: &[WinRect],
    eligible: &[bool],
    seat: (i32, i32),
    feet: (i32, i32),
    tol: i32,
    contact: (i32, i32),
    margin: i32,
    fits: &dyn Fn(usize, i32) -> bool,
) -> Option<(usize, i32)> {
    let gx = feet.0;
    for i in (0..stack.len()).rev() {
        if !eligible.get(i).copied().unwrap_or(false) {
            continue;
        }
        let r = stack[i];
        let inside = gx > r.x + 20 && gx < r.right() - 20;
        let near = (seat.1 - r.y).abs() < tol || (feet.1 - r.y).abs() < tol;
        if !inside || !near {
            continue;
        }
        let Some(&(s0, s1)) = exposed_spans(stack, i).iter().find(|s| s.0 <= gx && gx < s.1) else { continue };
        let (lo, hi) = (s0 + margin - contact.0, s1 - margin - contact.1);
        if lo > hi {
            continue;
        }
        let x = gx.clamp(lo, hi);
        if fits(i, x) {
            return Some((i, x));
        }
    }
    None
}

/// One platform per non-empty seat range of every eligible window.
#[cfg_attr(not(test), allow(dead_code))]
pub fn platforms(stack: &[WinRect], eligible: &[bool], contact: (i32, i32), margin: i32) -> Vec<Platform> {
    (0..stack.len())
        .filter(|&i| eligible.get(i).copied().unwrap_or(false))
        .flat_map(|i| seat_ranges(stack, i, contact, margin).into_iter().map(move |s| (i, s)))
        .map(|(i, seat_x)| Platform { win: stack[i].id, y: stack[i].y, seat_x })
        .collect()
}

// ---- work areas ---------------------------------------------------------------------

/// A dock's reserved space, `_NET_WM_STRUT_PARTIAL` semantics: `top` rows from the top of
/// the ROOT window, for x in `top_x` (inclusive), and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Strut {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
    pub left_y: (i32, i32),
    pub right_y: (i32, i32),
    pub top_x: (i32, i32),
    pub bottom_x: (i32, i32),
}

impl Strut {
    /// From `_NET_WM_STRUT_PARTIAL` (12 values).
    pub fn partial(v: &[u32]) -> Option<Strut> {
        if v.len() < 12 {
            return None;
        }
        let g = |i: usize| v[i].min(i32::MAX as u32) as i32;
        Some(Strut {
            left: g(0),
            right: g(1),
            top: g(2),
            bottom: g(3),
            left_y: (g(4), g(5)),
            right_y: (g(6), g(7)),
            top_x: (g(8), g(9)),
            bottom_x: (g(10), g(11)),
        })
    }

    /// From the older `_NET_WM_STRUT` (4 values): the whole root edge.
    pub fn plain(v: &[u32], root: (i32, i32)) -> Option<Strut> {
        if v.len() < 4 {
            return None;
        }
        let g = |i: usize| v[i].min(i32::MAX as u32) as i32;
        let (w, h) = (root.0 - 1, root.1 - 1);
        Some(Strut { left: g(0), right: g(1), top: g(2), bottom: g(3), left_y: (0, h), right_y: (0, h), top_x: (0, w), bottom_x: (0, w) })
    }

    /// A dock without strut properties: its geometry, if it hugs one edge of a monitor.
    pub fn from_dock(d: &WinRect, monitors: &[WinRect], root: (i32, i32)) -> Option<Strut> {
        let m = monitors.iter().find(|m| m.contains(d.center().0, d.center().1))?;
        let (x1, y1) = (d.right() - 1, d.bottom() - 1);
        let wide = d.w >= d.h;
        if wide && d.y <= m.y + 2 {
            Some(Strut { top: d.bottom(), top_x: (d.x, x1), ..Strut::default() })
        } else if wide && d.bottom() >= m.bottom() - 2 {
            Some(Strut { bottom: root.1 - d.y, bottom_x: (d.x, x1), ..Strut::default() })
        } else if !wide && d.x <= m.x + 2 {
            Some(Strut { left: d.right(), left_y: (d.y, y1), ..Strut::default() })
        } else if !wide && d.right() >= m.right() - 2 {
            Some(Strut { right: root.0 - d.x, right_y: (d.y, y1), ..Strut::default() })
        } else {
            None
        }
    }
}

fn overlaps(a: (i32, i32), lo: i32, hi_excl: i32) -> bool {
    a.0 < hi_excl && a.1 >= lo
}

/// Each monitor minus the struts that reach into it (i3 sets no usable `_NET_WORKAREA`).
/// A strut that would leave a monitor less than half its size is ignored.
pub fn work_areas(monitors: &[WinRect], root: (i32, i32), struts: &[Strut]) -> Vec<WinRect> {
    monitors
        .iter()
        .map(|m| {
            let (mut x0, mut y0, mut x1, mut y1) = (m.x, m.y, m.right(), m.bottom());
            for s in struts {
                if s.top > m.y && overlaps(s.top_x, m.x, m.right()) {
                    y0 = y0.max(s.top);
                }
                let b = root.1 - s.bottom;
                if s.bottom > 0 && b < m.bottom() && overlaps(s.bottom_x, m.x, m.right()) {
                    y1 = y1.min(b);
                }
                if s.left > m.x && overlaps(s.left_y, m.y, m.bottom()) {
                    x0 = x0.max(s.left);
                }
                let r = root.0 - s.right;
                if s.right > 0 && r < m.right() && overlaps(s.right_y, m.y, m.bottom()) {
                    x1 = x1.min(r);
                }
            }
            if (x1 - x0) * 2 < m.w || (y1 - y0) * 2 < m.h {
                return *m;
            }
            WinRect { id: m.id, x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u32, x: i32, y: i32, ww: i32, h: i32) -> WinRect {
        WinRect { id, x, y, w: ww, h }
    }
    const C: (i32, i32) = (-40, 40);
    const M: i32 = 65;

    #[test]
    fn no_occluder_whole_edge() {
        let a = w(1, 100, 500, 800, 400);
        let s = [a];
        assert_eq!(exposed_spans(&s, 0), vec![(100, 900)]);
        // The old perch_range: (m - l, r.w - m - h) relative to the window.
        let old = (M - C.0, a.w - M - C.1);
        assert_eq!(seat_ranges(&s, 0, C, M), vec![(a.x + old.0, a.x + old.1)]);
    }

    #[test]
    fn full_cover() {
        let s = [w(1, 100, 500, 800, 400), w(2, 50, 450, 900, 200)];
        assert!(exposed_spans(&s, 0).is_empty());
        let fits = |_: usize, _: i32| true;
        assert_eq!(choose_snap(&s[..1], &[true], (500, 500), (500, 520), 39, C, M, &fits), Some((0, 500)));
        assert_eq!(choose_snap(&s, &[true, false], (500, 500), (500, 520), 39, C, M, &fits), None);
    }

    #[test]
    fn partial_cover_middle() {
        // B covers A's edge from 40 % to 60 %.
        let a = w(1, 0, 500, 1000, 400);
        let b = w(2, 400, 300, 200, 400);
        let s = [a, b];
        assert_eq!(exposed_spans(&s, 0), vec![(0, 400), (600, 1000)]);
        let r = seat_ranges(&s, 0, C, M);
        assert_eq!(r, vec![(M - C.0, 400 - M - C.1), (600 + M - C.0, 1000 - M - C.1)]);
        let fits = |_: usize, _: i32| true;
        // A seat in the left span near B clamps inside it, the margin kept from B's side.
        assert_eq!(choose_snap(&s, &[true, false], (390, 500), (390, 510), 39, C, M, &fits), Some((0, 400 - M - C.1)));
        // A seat inside B's interval: rejected.
        assert_eq!(choose_snap(&s, &[true, false], (500, 500), (500, 510), 39, C, M, &fits), None);
    }

    #[test]
    fn lower_window_ignored() {
        let a = w(1, 0, 500, 1000, 400);
        let b = w(2, 400, 300, 200, 400);
        assert_eq!(exposed_spans(&[b, a], 1), vec![(0, 1000)]);
    }

    #[test]
    fn pad_rules() {
        let a = w(1, 0, 500, 1000, 400);
        let cov = |b: WinRect| exposed_spans(&[a, b], 0) != vec![(0, 1000)];
        // Bottom row at A.y - 1 (exclusive bottom at A.y) covers; at A.y - 3 it doesn't.
        assert!(cov(w(2, 100, 400, 100, 100)));
        assert!(!cov(w(2, 100, 398, 100, 100)));
        // Top at A.y + 2 covers; at A.y + 3 it doesn't.
        assert!(cov(w(2, 100, 502, 100, 100)));
        assert!(!cov(w(2, 100, 503, 100, 100)));
    }

    #[test]
    fn adjacent_occluders_merge() {
        let a = w(1, 0, 500, 1000, 400);
        let s = [a, w(2, 200, 450, 100, 100), w(3, 300, 450, 150, 100)];
        assert_eq!(exposed_spans(&s, 0), vec![(0, 200), (450, 1000)]);
    }

    #[test]
    fn falls_through_to_lower() {
        let a = w(1, 0, 500, 1000, 400);
        let b = w(2, 200, 505, 600, 300);
        // A small popup over B's edge at the feet x (not eligible, but it occludes).
        let c = w(3, 450, 503, 80, 40);
        let s = [a, b, c];
        assert_eq!(exposed_spans(&s, 0), vec![(0, 1000)], "B's edge is 5 px lower: it doesn't cover A's");
        assert_eq!(exposed_spans(&s, 1), vec![(200, 450), (530, 800)]);
        let fits = |_: usize, _: i32| true;
        // Feet at x 490, near both edges: B (topmost eligible) is covered there by C, so A.
        let r = choose_snap(&s, &[true, true, false], (490, 500), (490, 505), 39, C, M, &fits);
        assert_eq!(r, Some((0, 490)));
        // Without C, B wins.
        let r = choose_snap(&s[..2], &[true, true], (490, 500), (490, 505), 39, C, M, &fits);
        assert_eq!(r, Some((1, 490)));
    }

    #[test]
    fn small_window_occludes_not_eligible() {
        let a = w(1, 0, 500, 1000, 400);
        let popup = w(2, 470, 480, 80, 40);
        let s = [a, popup];
        let fits = |_: usize, _: i32| true;
        assert_eq!(choose_snap(&s, &[true, false], (500, 480), (500, 500), 39, C, M, &fits), None);
        // The popup is never a candidate, even right at its own edge.
        assert_eq!(choose_snap(&s, &[true, false], (500, 470), (500, 482), 39, C, M, &fits), None);
    }

    #[test]
    fn platforms_skip_covered() {
        let a = w(1, 0, 500, 1000, 400);
        let b = w(2, 400, 300, 200, 400);
        let p = platforms(&[a, b], &[true, false], C, M);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|p| p.win == 1 && p.y == 500));
        assert!(p.iter().all(|p| p.seat_x.1 + C.1 + M <= 400 || p.seat_x.0 + C.0 - M >= 600));
    }

    #[test]
    fn work_area_from_top_struts() {
        // bella: two monitors side by side, polybar (29 px) at the top of each.
        let mons = [w(0, 0, 0, 2560, 1600), w(1, 2560, 0, 2560, 1440)];
        let root = (5120, 1600);
        let s1 = Strut::partial(&[0, 0, 29, 0, 0, 0, 0, 0, 2560, 5119, 0, 0]).unwrap();
        let s2 = Strut::partial(&[0, 0, 29, 0, 0, 0, 0, 0, 0, 2559, 0, 0]).unwrap();
        let a = work_areas(&mons, root, &[s1, s2]);
        assert_eq!(a[0], w(0, 0, 29, 2560, 1571));
        assert_eq!(a[1], w(1, 2560, 29, 2560, 1411));
        // Only one bar: the other monitor keeps its top.
        let a = work_areas(&mons, root, &[s1]);
        assert_eq!(a[0].y, 0);
        assert_eq!(a[1].y, 29);
        // A dock with geometry only.
        let d = Strut::from_dock(&w(9, 2560, 0, 2560, 29), &mons, root).unwrap();
        assert_eq!(work_areas(&mons, root, &[d])[1].y, 29);
        // A bottom bar on the shorter monitor (struts count from the root's bottom).
        let b = Strut::partial(&[0, 0, 0, 160 + 30, 0, 0, 0, 0, 0, 0, 2560, 5119]).unwrap();
        assert_eq!(work_areas(&mons, root, &[b])[1].bottom(), 1410);
        assert_eq!(work_areas(&mons, root, &[b])[0].bottom(), 1600);
    }
}
