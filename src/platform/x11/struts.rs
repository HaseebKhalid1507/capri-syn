//! Work areas from X11 struts (`_NET_WM_STRUT(_PARTIAL)`, docks): what panels reserve along the
//! root's edges, taken off each monitor.

use crate::edges::WinRect;

/// Space a panel reserves along the root's edges: `top` rows from the top of the ROOT window,
/// for x in `top_x` (inclusive), and so on (a backend translates its own panels into these).
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

fn overlaps(a: (i32, i32), lo: i32, hi_excl: i32) -> bool {
    a.0 < hi_excl && a.1 >= lo
}

/// Each monitor minus the struts that reach into it.
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
