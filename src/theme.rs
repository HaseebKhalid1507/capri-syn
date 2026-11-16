//! SHARD: Akira red over jet black (Haseeb's pick, 2026-11-16). Every colour the UI paints
//! comes from [`Pal`], and every shape it is cut into from [`outline`]: 26 px chamfers on the
//! panels, 9 px on fields and code, slanted plates, a blade of a tail, mane slivers. Two
//! colours plus bone: bone is the text, and it is also "done" and "danger" (told apart by
//! fill, glyph and shape). Ink on red, never bone on red (bone on her red is 3.4:1). The
//! nameplate's outline carries the state, so it reads in greyscale and on the 1-bit X shape.

use gpui_kit::*;

/// A point, logical px.
pub type Pt = (f32, f32);

/// The tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pal {
    /// Jet black: ink on red fills.
    pub bg: u32,
    /// Every surface, one step up from jet so a panel separates from her black art.
    pub panel: u32,
    /// Folds, inline code.
    pub raise: u32,
    /// The panel frame (2 px).
    pub border: u32,
    /// Her: mane red, turned 10° from Akira's so the UI and her hair read as one red.
    pub accent: u32,
    /// Decorative red (the dimmest trailing sliver).
    pub accent_dim: u32,
    /// Bone: body copy.
    pub text: u32,
    /// Labels, hints, LATER, offline.
    pub dim: u32,
    /// The focus ring and the caret.
    pub focus: u32,
    /// The risk banner, ERROR, QUIT?: a bone plate with ink type.
    pub danger: u32,
    /// DONE: a bone outline.
    pub success: u32,
    /// Type on red and bone fills.
    pub ink: u32,
    /// Hairlines, the fold and code frames.
    pub rule: u32,
    /// The text field's fill.
    pub field: u32,
    /// A code block's fill.
    pub code: u32,
    /// His own words in the Line (a step under bone).
    pub you: u32,
    /// The sleeping plate's edge.
    pub asleep: u32,
    /// The 1 px dark ring around everything that sits on the desktop (the red frame alone is
    /// 2.6:1 on a light desktop; the ring carries the edge there).
    pub ring: u32,
}

impl Pal {
    pub const SHARD: Pal = Pal {
        bg: 0x0a0a0b,
        panel: 0x0e0e10,
        raise: 0x18181b,
        border: 0xff1438,
        accent: 0xff1438,
        accent_dim: 0x6a0b1c,
        text: 0xf4f0ea,
        dim: 0xa8a09b,
        focus: 0xf4f0ea,
        danger: 0xf4f0ea,
        success: 0xf4f0ea,
        ink: 0x0a0a0b,
        rule: 0x3a0e16,
        field: 0x121215,
        code: 0x0b0b0d,
        you: 0xd9d3cc,
        asleep: 0x2e2e33,
        ring: 0x02060a,
    };

    /// SHARD, each token the pack overrides (`tones` in moods.json, by token name) replaced.
    pub fn with(mut self, get: impl Fn(&str) -> Option<u32>) -> Pal {
        for (k, v) in [
            ("bg", &mut self.bg),
            ("panel", &mut self.panel),
            ("raise", &mut self.raise),
            ("border", &mut self.border),
            ("accent", &mut self.accent),
            ("accent-dim", &mut self.accent_dim),
            ("text", &mut self.text),
            ("text-dim", &mut self.dim),
            ("focus", &mut self.focus),
            ("danger", &mut self.danger),
            ("success", &mut self.success),
            ("ink", &mut self.ink),
        ] {
            if let Some(c) = get(k) {
                *v = c;
            }
        }
        self
    }

    /// A state's colour, by its tone key (a mood's `tone`): online, think, busy, speak, music,
    /// alert and mic are her red; done is success, error danger; asleep, offline and a paused
    /// wake word are text-dim.
    pub fn tone(&self, key: &str) -> u32 {
        match key {
            "done" => self.success,
            "error" => self.danger,
            "sleep" | "offline" | "paused" => self.dim,
            _ => self.accent,
        }
    }

    /// A nameplate's paint. `key`: the state's tone key; `tone`: its colour; `invert`: a solid
    /// plate; `blink`: the second half of the 1 Hz beat (NEEDS YOU turns bone, the rec dot
    /// dims).
    pub fn look(&self, plate: Plate, key: &str, tone: u32, invert: bool, blink: bool) -> Look {
        let mut l = Look { plate, fill: self.panel, edge: tone, ink: tone, mark: tone, dash: None, drop: 0.0, trail: false };
        match key {
            // The split bone plate, a red ✕ shard broken off.
            "error" => return Look { fill: self.danger, edge: self.danger, ink: self.ink, mark: self.accent, ..l },
            "offline" | "paused" => l.dash = Some([3.0, 2.5]),
            "sleep" => {
                l.edge = self.asleep;
                l.drop = 5.0;
            }
            "wake" => {
                l.dash = Some([5.0, 3.0]);
                l.ink = self.text;
            }
            "think" => l.trail = true,
            _ => {}
        }
        if invert {
            let mic = key == "mic";
            l.fill = tone;
            l.edge = tone;
            l.ink = self.ink;
            l.mark = if mic { self.text } else { self.ink };
            if blink {
                if mic {
                    l.mark = mix(self.text, tone, 0.85);
                } else {
                    l.fill = self.text;
                    l.edge = self.text;
                }
            }
        } else if plate == Plate::Arrow {
            l.mark = self.text;
        }
        l
    }
}

/// The silhouette of a nameplate: the state, readable without colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Plate {
    /// ▱ present: online, thinking (thinking trails three slivers).
    #[default]
    Slant,
    /// ▷ doing: working, speaking, listening, heard.
    Arrow,
    /// ⫸ swallowtail: NEEDS YOU, the mic live / hands-free.
    Flag,
    /// ▢ settled or away: done, offline, sleeping.
    Square,
    /// The split plate: ERROR.
    Split,
}

impl Plate {
    pub fn parse(s: &str) -> Option<Plate> {
        Some(match s {
            "slant" => Plate::Slant,
            "arrow" => Plate::Arrow,
            "flag" => Plate::Flag,
            "square" => Plate::Square,
            "split" => Plate::Split,
            _ => return None,
        })
    }

    /// Extra room inside the plate, left and right of its content (logical px).
    pub fn pad(self) -> (f32, f32) {
        match self {
            Plate::Slant | Plate::Square => (12.0, 14.0),
            Plate::Arrow => (12.0, 20.0),
            Plate::Flag => (12.0, 22.0),
            Plate::Split => (10.0, 12.0),
        }
    }
}

/// How a nameplate is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub plate: Plate,
    pub fill: u32,
    pub edge: u32,
    /// Its type.
    pub ink: u32,
    /// Its glyph.
    pub mark: u32,
    pub dash: Option<[f32; 2]>,
    /// Sits this much lower (asleep).
    pub drop: f32,
    /// Three slivers trail it (thinking).
    pub trail: bool,
}

/// `a` toward `b` by `k` (0 = a).
pub fn mix(a: u32, b: u32, k: f32) -> u32 {
    let ch = |s: u32| -> u32 {
        let (x, y) = (((a >> s) & 0xff) as f32, ((b >> s) & 0xff) as f32);
        ((x + (y - x) * k.clamp(0.0, 1.0)).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// `rgb` at alpha `a`.
pub fn alpha(rgb: u32, a: f32) -> Rgba {
    rgba((rgb << 8) | (a.clamp(0.0, 1.0) * 255.0).round() as u32)
}

/// WCAG 2.x contrast ratio of two colours.
#[cfg(test)]
pub fn contrast(a: u32, b: u32) -> f32 {
    let lum = |c: u32| {
        let ch = |s: u32| {
            let v = ((c >> s) & 0xff) as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

// ---- shapes -------------------------------------------------------------------------------

/// The ring's width outside a plate's outline.
pub const RING: f32 = 1.5;
/// A panel's big chamfer (top-left, bottom-right) and its small one.
pub const BIG_CUT: f32 = 26.0;
pub const SMALL_CUT: f32 = 7.0;
/// The blade notch's depth in a panel's top edge (its mane slivers stand in it).
pub const NOTCH: f32 = 6.0;

/// A cut a shape can be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cut {
    /// A panel (the Line, the reader): 26 px chamfers top-left and bottom-right, 7 px on the
    /// others, and a blade notch stepped into the top edge near its right end.
    Panel,
    /// Chamfered top-left and bottom-right by k (fields 9, code 9, folds 5, the menu 12, the
    /// mic row 8).
    Corners(f32),
    /// Slanted ▱ by k (keys, buttons, the READ pill, the chip).
    Slant(f32),
    /// A tab: square left, the right edge slanted by k (name plates, the READ blade, a hovered
    /// menu item, the code label, the h1 rule).
    Tab(f32),
    /// Square left, a swallowtail k deep on the right (NEEDS YOU in a header).
    Swallow(f32),
    /// The unread badge: a 9 px cut top-left, the right edge slanted 8.
    Badge,
    /// The nameplate.
    Tag(Plate),
    /// The unread pill: a blade pointing on.
    Unread,
    /// Teeth top and bottom: danger, the only serrated thing in the UI.
    Teeth,
}

/// The polygons of `cut` on the box x0..x1, y0..y1 (a split plate is two). A tag's point or
/// swallowtail and the split plate's dropped shard reach a few px past the box.
pub fn outline(cut: Cut, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec<Pt>> {
    let (w, h) = (x1 - x0, y1 - y0);
    let mid = (y0 + y1) / 2.0;
    let one = |p: Vec<Pt>| vec![p];
    match cut {
        Cut::Panel => {
            let big = BIG_CUT.min(h * 0.2);
            let sm = SMALL_CUT;
            if w < 300.0 {
                return one(corners(x0, y0, x1, y1, big, sm));
            }
            one(vec![
                (x0 + big, y0),
                (x1 - 120.0, y0),
                (x1 - 112.0, y0 + NOTCH),
                (x1 - 40.0, y0 + NOTCH),
                (x1 - 36.0, y0),
                (x1 - sm, y0),
                (x1, y0 + sm),
                (x1, y1 - big),
                (x1 - big, y1),
                (x0 + sm, y1),
                (x0, y1 - sm),
                (x0, y0 + big),
            ])
        }
        Cut::Corners(k) => {
            let k = k.min(h * 0.45).min(w * 0.45);
            one(vec![(x0 + k, y0), (x1, y0), (x1, y1 - k), (x1 - k, y1), (x0, y1), (x0, y0 + k)])
        }
        Cut::Slant(k) => {
            let k = k.min(h * 0.33);
            one(vec![(x0 + k, y0), (x1, y0), (x1 - k, y1), (x0, y1)])
        }
        Cut::Tab(k) => one(vec![(x0, y0), (x1, y0), (x1 - k, y1), (x0, y1)]),
        Cut::Swallow(k) => one(vec![(x0, y0), (x1, y0), (x1 - k, mid), (x1, y1), (x0, y1)]),
        Cut::Badge => one(vec![(x0 + 9.0, y0), (x1, y0), (x1 - 8.0, y1), (x0, y1), (x0, y0 + 9.0)]),
        Cut::Unread => {
            let s = 9.0f32.min(h * 0.38);
            one(vec![(x0 + s, y0), (x1 - 10.0, y0), (x1, mid), (x1 - 10.0, y1), (x0, y1)])
        }
        Cut::Teeth => {
            let (tw, td) = (10.0, 4.0);
            let mut p = Vec::new();
            let mut x = x0;
            while x < x1 - 0.1 {
                p.push((x, y0 + td));
                p.push(((x + tw / 2.0).min(x1), y0));
                x += tw;
            }
            p.push((x1, y0 + td));
            p.push((x1, y1 - td));
            let mut x = x1;
            while x > x0 + 0.1 {
                p.push((x, y1 - td));
                p.push(((x - tw / 2.0).max(x0), y1));
                x -= tw;
            }
            p.push((x0, y1 - td));
            one(p)
        }
        Cut::Tag(plate) => {
            let s = 9.0f32.min(h * 0.38);
            match plate {
                Plate::Slant => one(vec![(x0 + s, y0), (x1, y0), (x1 - s, y1), (x0, y1)]),
                Plate::Arrow => one(vec![(x0 + s, y0), (x1 - 4.0, y0), (x1 + 6.0, mid), (x1 - 4.0, y1), (x0, y1)]),
                Plate::Flag => one(vec![(x0 + s, y0), (x1 + 4.0, y0), (x1 - 5.0, mid), (x1 + 4.0, y1), (x0, y1)]),
                Plate::Square => one(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]),
                Plate::Split => {
                    // The glyph's shard breaks off and drops.
                    let gx = x0 + 25.0;
                    vec![
                        vec![(x0 + s, y0 + 3.0), (gx + 2.0, y0 + 3.0), (gx - 3.0, y1 + 3.0), (x0, y1 + 3.0)],
                        vec![(gx + 5.0, y0), (x1, y0), (x1 - s, y1), (gx, y1)],
                    ]
                }
            }
        }
    }
}

fn corners(x0: f32, y0: f32, x1: f32, y1: f32, big: f32, sm: f32) -> Vec<Pt> {
    vec![(x0 + big, y0), (x1 - sm, y0), (x1, y0 + sm), (x1, y1 - big), (x1 - big, y1), (x0 + sm, y1), (x0, y1 - sm), (x0, y0 + big)]
}

/// The speech bubble: cut 18 top-left and bottom-right, 6 on the others, and a blade of a tail
/// on the bottom edge: a wide root, its straight back toward the bubble's middle, the edge
/// leaning out to `tip` (her head).
pub fn bubble(x0: f32, y0: f32, x1: f32, y1: f32, tail_x: f32, tip: Pt) -> Vec<Pt> {
    let (back, edge) = if tail_x >= (x0 + x1) / 2.0 { (tail_x - 6.0, tail_x + 22.0) } else { (tail_x + 6.0, tail_x - 22.0) };
    let (a, b) = if back < edge { (back, edge) } else { (edge, back) };
    vec![(x0 + 18.0, y0), (x1 - 6.0, y0), (x1, y0 + 6.0), (x1, y1 - 18.0), (x1 - 18.0, y1), (b, y1), tip, (a, y1), (x0 + 6.0, y1), (x0, y1 - 6.0), (x0, y0 + 18.0)]
}

/// Three mane slivers in the box x, y, w × h, leaning like "/" (thick, thinner, thinnest).
pub fn slivers(x: f32, y: f32, w: f32, h: f32) -> Vec<Vec<Pt>> {
    // A 115° gradient's stripes: where each starts and ends along it.
    const STOPS: [(f32, f32); 3] = [(0.30, 0.44), (0.55, 0.62), (0.72, 0.76)];
    let (c, s) = (0.906f32, 0.423f32);
    let len = w * c + h * s;
    STOPS
        .iter()
        .map(|&(a, b)| {
            let (ta, tb) = (a * len, b * len);
            vec![(x + ta / c, y), (x + tb / c, y), (x + (tb - h * s) / c, y + h), (x + (ta - h * s) / c, y + h)]
        })
        .collect()
}

/// The thinking plate's three trailing slivers, right of the plate box (x1, y0..y1): (polygon,
/// brightness 0..=2) with the brightest marching on each `step`.
pub fn trail(x1: f32, y0: f32, y1: f32, step: u64) -> Vec<(Vec<Pt>, u8)> {
    const STOPS: [(f32, f32); 3] = [(0.14, 0.30), (0.44, 0.58), (0.72, 0.84)];
    let (x, y, w, h) = (x1 + 2.0, y0 + 4.0, 24.0, (y1 - y0 - 8.0).max(4.0));
    let (c, s) = (0.906f32, 0.423f32);
    let len = w * c + h * s;
    STOPS
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| {
            let (ta, tb) = (a * len, b * len);
            let p = vec![(x + ta / c, y), (x + tb / c, y), (x + (tb - h * s) / c, y + h), (x + (ta - h * s) / c, y + h)];
            (p, ((i as u64 + 3 - step % 3) % 3) as u8)
        })
        .collect()
}

/// `pts` grown outward by `d` (inward if negative), mitred.
pub fn offset(pts: &[Pt], d: f32) -> Vec<Pt> {
    let n = pts.len();
    if n < 3 || d == 0.0 {
        return pts.to_vec();
    }
    let area: f32 = (0..n).map(|i| pts[i].0 * pts[(i + 1) % n].1 - pts[(i + 1) % n].0 * pts[i].1).sum();
    let sgn = if area >= 0.0 { 1.0 } else { -1.0 };
    let normal = |a: Pt, b: Pt| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        (sgn * dy / l, -sgn * dx / l)
    };
    (0..n)
        .map(|i| {
            let (p, c, nx) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let (n1, n2) = (normal(p, c), normal(c, nx));
            let k = (1.0 + n1.0 * n2.0 + n1.1 * n2.1).max(0.25);
            (c.0 + d * (n1.0 + n2.0) / k, c.1 + d * (n1.1 + n2.1) / k)
        })
        .collect()
}

// ---- painting -----------------------------------------------------------------------------

/// How a shape is painted: its fill, its edge (inside the outline, `width` px, maybe dashed)
/// and its dark ring (outside it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fill {
    pub fill: Option<u32>,
    pub edge: Option<u32>,
    pub width: f32,
    pub dash: Option<[f32; 2]>,
    pub ring: Option<u32>,
}

impl Fill {
    pub fn solid(c: u32) -> Fill {
        Fill { fill: Some(c), edge: None, width: 0.0, dash: None, ring: None }
    }
    pub fn framed(fill: u32, edge: u32, width: f32) -> Fill {
        Fill { fill: Some(fill), edge: Some(edge), width, dash: None, ring: None }
    }
    pub fn line(edge: u32, width: f32) -> Fill {
        Fill { fill: None, edge: Some(edge), width, dash: None, ring: None }
    }
    pub fn ringed(self, ring: u32) -> Fill {
        Fill { ring: Some(ring), ..self }
    }
    pub fn dashed(self, dash: Option<[f32; 2]>) -> Fill {
        Fill { dash, ..self }
    }
}

fn path(pts: &[Pt]) -> Vec<Point<Pixels>> {
    pts.iter().map(|&(x, y)| point(px(x), px(y))).collect()
}

/// Fill one polygon (logical window px).
pub fn fill_poly(window: &mut Window, pts: &[Pt], c: impl Into<Background>) {
    if pts.len() < 3 {
        return;
    }
    let mut pb = PathBuilder::fill();
    pb.add_polygon(&path(pts), true);
    if let Ok(p) = pb.build() {
        window.paint_path(p, c);
    }
}

fn stroke_poly(window: &mut Window, pts: &[Pt], w: f32, dash: Option<[f32; 2]>, c: u32) {
    let mut pb = PathBuilder::stroke(px(w));
    if let Some([a, b]) = dash {
        pb = pb.dash_array(&[px(a), px(b)]);
    }
    pb.add_polygon(&path(pts), true);
    if let Ok(p) = pb.build() {
        window.paint_path(p, rgb(c));
    }
}

/// Paint `polys` with `f`: ring, then edge, then fill.
pub fn paint(window: &mut Window, polys: &[Vec<Pt>], f: Fill) {
    if let Some(r) = f.ring {
        for p in polys {
            fill_poly(window, &offset(p, RING), rgb(r));
        }
    }
    for p in polys {
        match (f.edge, f.dash) {
            // Filled: the edge is the outline, the fill inset by its width (crisp, no seam).
            (Some(e), None) if f.width > 0.0 && f.fill.is_some() => {
                fill_poly(window, p, rgb(e));
                if let Some(c) = f.fill {
                    fill_poly(window, &offset(p, -f.width), rgb(c));
                }
            }
            (Some(e), dash) if f.width > 0.0 => {
                if let Some(c) = f.fill {
                    fill_poly(window, p, rgb(c));
                }
                stroke_poly(window, &offset(p, -f.width / 2.0), f.width, dash, e);
            }
            _ => {
                if let Some(c) = f.fill {
                    fill_poly(window, p, rgb(c));
                }
            }
        }
    }
}

/// A shape behind its parent's content: put it first in a `.relative()` element.
pub fn backdrop(cut: Cut, f: Fill) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let polys = outline(cut, f32::from(b.left()), f32::from(b.top()), f32::from(b.right()), f32::from(b.bottom()));
            paint(window, &polys, f);
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A plate: `cut` painted with `f` behind `content`.
pub fn plate(cut: Cut, f: Fill) -> Div {
    div().relative().flex().items_center().child(backdrop(cut, f))
}

/// HUD lettering with CSS-style letter-spacing (GPUI has none): one box per glyph.
pub fn tracked(s: &str, spacing: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .children(s.chars().map(|c| div().mr(px(spacing)).child(if c == ' ' { "\u{00A0}".to_string() } else { c.to_string() })))
}

/// A label with its level meter (`▮▮▯▯▯`) drawn as bars: lit ones in `fg`, the rest at 28 %.
pub fn metered(s: &str, spacing: f32, fg: u32) -> Div {
    let mut row = div().flex().flex_none().items_center();
    let mut word = String::new();
    let mut bars: Vec<bool> = Vec::new();
    let flush_bars = |row: Div, bars: &mut Vec<bool>| {
        if bars.is_empty() {
            return row;
        }
        let m = div().flex().items_end().gap(px(2.0)).h(px(10.0)).mx(px(3.0)).children(bars.drain(..).map(|on| div().w(px(4.0)).h(px(10.0)).bg(alpha(fg, if on { 1.0 } else { 0.28 }))));
        row.child(m)
    };
    for c in s.chars() {
        if c == '▮' || c == '▯' {
            if !word.is_empty() {
                row = row.child(tracked(&std::mem::take(&mut word), spacing));
            }
            bars.push(c == '▮');
        } else {
            row = flush_bars(row, &mut bars);
            word.push(c);
        }
    }
    row = flush_bars(row, &mut bars);
    if !word.is_empty() {
        row = row.child(tracked(&word, spacing));
    }
    row
}

/// A small status glyph drawn as shapes (crisp at any size, no font fallback). 11 px box;
/// `bg` paints cut-outs. Kinds: dot, dots, gear, play, bang, check, cross (and `shard`: the
/// ✕ broken off, tilted), moon, ring, rec, note, mic, hf, warn.
pub fn glyph(kind: &str, fg: u32, bg: u32) -> impl IntoElement + use<> {
    glyph_at(kind, fg, bg, 11.0)
}

/// [`glyph`], `size` px square.
pub fn glyph_at(kind: &str, fg: u32, bg: u32, size: f32) -> impl IntoElement + use<> {
    let kind = kind.to_string();
    let k = size / 11.0;
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let tilt = kind == "shard";
            let at = move |x: f32, y: f32| -> Pt {
                if tilt {
                    // −12°, about the middle.
                    let (c, s) = (0.978f32, -0.208f32);
                    let (dx, dy) = (x - 5.5, y - 5.5);
                    (ox + k * (5.5 + dx * c - dy * s), oy + k * (5.5 + dx * s + dy * c))
                } else {
                    (ox + k * x, oy + k * y)
                }
            };
            let fill = |window: &mut Window, pts: &[Pt], c: u32| {
                let p: Vec<Pt> = pts.iter().map(|&(x, y)| at(x, y)).collect();
                fill_poly(window, &p, rgb(c));
            };
            let circle = |cx: f32, cy: f32, r: f32| (0..24).map(|i| {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                (cx + r * a.cos(), cy + r * a.sin())
            }).collect::<Vec<_>>();
            let stroke = |window: &mut Window, pts: &[Pt], w: f32, c: u32| {
                let mut pb = PathBuilder::stroke(px(w * k));
                let p: Vec<Pt> = pts.iter().map(|&(x, y)| at(x, y)).collect();
                pb.move_to(point(px(p[0].0), px(p[0].1)));
                for &(x, y) in &p[1..] {
                    pb.line_to(point(px(x), px(y)));
                }
                if let Ok(path) = pb.build() {
                    window.paint_path(path, rgb(c));
                }
            };
            match kind.as_str() {
                "ring" => {
                    fill(window, &circle(5.5, 5.5, 4.6), fg);
                    fill(window, &circle(5.5, 5.5, 2.7), bg);
                }
                // The mic is open: a filled dot.
                "rec" => fill(window, &circle(5.5, 5.5, 4.4), fg),
                // Hands-free: the dot in a ring.
                "hf" => {
                    fill(window, &circle(5.5, 5.5, 5.45), fg);
                    fill(window, &circle(5.5, 5.5, 4.35), bg);
                    fill(window, &circle(5.5, 5.5, 2.6), fg);
                }
                "mic" => {
                    // A capsule, its cradle and stand.
                    let cap: Vec<Pt> = (0..=12)
                        .map(|i| {
                            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                            (5.5 + 1.9 * a.cos(), 2.5 + 1.9 * a.sin())
                        })
                        .chain((0..=12).map(|i| {
                            let a = std::f32::consts::PI * (i as f32 / 12.0);
                            (5.5 + 1.9 * a.cos(), 5.1 + 1.9 * a.sin())
                        }))
                        .collect();
                    fill(window, &cap, fg);
                    let arc: Vec<Pt> = (0..=12)
                        .map(|i| {
                            let a = std::f32::consts::PI * (i as f32 / 12.0);
                            (5.5 + 3.9 * a.cos(), 5.2 + 3.9 * a.sin())
                        })
                        .collect();
                    stroke(window, &arc, 1.3, fg);
                    stroke(window, &[(5.5, 9.1), (5.5, 10.8)], 1.3, fg);
                    stroke(window, &[(3.4, 10.6), (7.6, 10.6)], 1.3, fg);
                }
                "warn" => {
                    fill(window, &[(5.5, 0.4), (10.9, 10.4), (0.1, 10.4)], fg);
                    fill(window, &[(4.75, 3.6), (6.25, 3.6), (6.25, 7.4), (4.75, 7.4)], bg);
                    fill(window, &[(4.75, 8.2), (6.25, 8.2), (6.25, 9.6), (4.75, 9.6)], bg);
                }
                "gear" => {
                    let pts: Vec<Pt> = (0..32)
                        .map(|i| {
                            let a = (i as f32 + 0.5) / 32.0 * std::f32::consts::TAU;
                            let r = if (i / 2) % 2 == 0 { 5.6 } else { 4.1 };
                            (5.5 + r * a.cos(), 5.5 + r * a.sin())
                        })
                        .collect();
                    fill(window, &pts, fg);
                    fill(window, &circle(5.5, 5.5, 1.8), bg);
                }
                "play" => fill(window, &[(2.0, 0.8), (10.0, 5.5), (2.0, 10.2)], fg),
                "check" => stroke(window, &[(1.0, 5.8), (4.3, 9.1), (10.2, 1.8)], 2.2, fg),
                "bang" => {
                    fill(window, &[(4.1, 0.0), (6.9, 0.0), (6.9, 7.2), (4.1, 7.2)], fg);
                    fill(window, &[(4.1, 8.4), (6.9, 8.4), (6.9, 11.0), (4.1, 11.0)], fg);
                }
                "cross" | "shard" => {
                    stroke(window, &[(1.5, 1.5), (9.5, 9.5)], 2.3, fg);
                    stroke(window, &[(9.5, 1.5), (1.5, 9.5)], 2.3, fg);
                }
                "moon" => {
                    fill(window, &circle(5.2, 5.5, 4.8), fg);
                    fill(window, &circle(7.6, 3.9, 3.9), bg);
                }
                "dots" => {
                    for x in [1.6, 5.5, 9.4] {
                        fill(window, &circle(x, 6.0, 1.5), fg);
                    }
                }
                "note" => {
                    fill(window, &circle(3.2, 8.3, 2.4), fg);
                    fill(window, &[(4.4, 1.0), (5.8, 1.0), (5.8, 8.3), (4.4, 8.3)], fg);
                    fill(window, &[(5.8, 1.0), (9.8, 3.2), (9.8, 5.0), (5.8, 2.9)], fg);
                }
                "" => {}
                _ => fill(window, &circle(5.5, 5.5, 3.4), fg),
            }
        },
    )
    .flex_none()
    .w(px(size))
    .h(px(size))
}

#[cfg(test)]
mod tests {
    use super::{Cut, Pal, Plate, Pt, bubble, contrast, offset, outline};

    const P: Pal = Pal::SHARD;

    #[test]
    fn every_text_pair_passes_aa() {
        // (what, fg, bg): every pair the UI sets type in.
        let pairs = [
            ("text on panel", P.text, P.panel),
            ("text-dim on panel", P.dim, P.panel),
            ("accent on panel", P.accent, P.panel),
            ("accent on bg", P.accent, P.bg),
            ("ink on accent", P.ink, P.accent),
            ("ink on danger", P.ink, P.danger),
            ("his words on panel", P.you, P.panel),
            ("text on raise (inline code, folds)", P.text, P.raise),
            ("text on field", P.text, P.field),
            ("code on code fill", P.text, P.code),
            ("text-dim on raise", P.dim, P.raise),
            ("accent on raise (fold lang)", P.accent, P.raise),
            ("accent on field (the › prompt)", P.accent, P.field),
        ];
        for (what, fg, bg) in pairs {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{what}: {c:.2}");
        }
        // Glyphs only (non-text, 3:1): bone and red on each other.
        assert!(contrast(P.text, P.accent) >= 3.0);
        assert!(contrast(P.accent, P.danger) >= 3.0);
    }

    #[test]
    fn theme_md_figures() {
        let r = |x: f32| (x * 100.0).round() / 100.0;
        assert_eq!(r(contrast(P.text, P.panel)), 16.99);
        assert_eq!(r(contrast(P.dim, P.panel)), 7.5);
        assert_eq!(r(contrast(P.accent, P.panel)), 4.97);
        assert_eq!(r(contrast(P.ink, P.accent)), 5.1);
    }

    #[test]
    fn tones_collapse() {
        for k in ["online", "think", "busy", "speak", "music", "alert", "mic", "wake"] {
            assert_eq!(P.tone(k), P.accent, "{k}");
        }
        assert_eq!(P.tone("done"), P.success);
        assert_eq!(P.tone("error"), P.danger);
        assert_eq!(P.tone("sleep"), P.dim);
        assert_eq!(P.tone("offline"), P.dim);
    }

    #[test]
    fn needs_you_flips_bone_and_keeps_ink() {
        let red = P.look(Plate::Flag, "alert", P.accent, true, false);
        let bone = P.look(Plate::Flag, "alert", P.accent, true, true);
        assert_eq!((red.fill, red.ink), (P.accent, P.ink));
        assert_eq!((bone.fill, bone.ink), (P.text, P.ink));
        // Error: the bone plate, ink type, a red shard.
        let e = P.look(Plate::Split, "error", P.danger, true, false);
        assert_eq!((e.fill, e.ink, e.mark), (P.danger, P.ink, P.accent));
        // Never bone type on red.
        for blink in [false, true] {
            for key in ["alert", "mic"] {
                let l = P.look(Plate::Flag, key, P.accent, true, blink);
                assert!(contrast(l.ink, l.fill) >= 4.5, "{key}");
            }
        }
    }

    #[test]
    fn silhouettes_differ() {
        let o = |p| outline(Cut::Tag(p), 0.0, 0.0, 120.0, 25.0);
        let all = [Plate::Slant, Plate::Arrow, Plate::Flag, Plate::Square, Plate::Split].map(o);
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
        assert_eq!(all[4].len(), 2, "the split plate is two pieces");
    }

    #[test]
    fn offset_grows_a_square_evenly() {
        let sq = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let g = offset(&sq, 1.5);
        assert_eq!(g, vec![(-1.5, -1.5), (11.5, -1.5), (11.5, 11.5), (-1.5, 11.5)]);
        let s = offset(&sq, -2.0);
        assert_eq!(s, vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)]);
        // Either winding.
        let rev: Vec<Pt> = sq.iter().rev().copied().collect();
        assert_eq!(offset(&rev, 1.5)[0], (-1.5, 11.5));
    }

    #[test]
    fn teeth_only_on_the_danger_plate() {
        let t = &outline(Cut::Teeth, 0.0, 0.0, 100.0, 40.0)[0];
        // Ten teeth top, ten bottom.
        assert!(t.len() >= 40);
        assert!(t.iter().all(|p| p.1 >= 0.0 && p.1 <= 40.0));
    }

    #[test]
    fn the_tail_leans_out() {
        let right = bubble(0.0, 0.0, 400.0, 100.0, 350.0, (352.0, 116.0));
        let left = bubble(0.0, 0.0, 400.0, 100.0, 50.0, (48.0, 116.0));
        assert!(right.contains(&(344.0, 100.0)) && right.contains(&(372.0, 100.0)));
        assert!(left.contains(&(28.0, 100.0)) && left.contains(&(56.0, 100.0)));
    }
}
