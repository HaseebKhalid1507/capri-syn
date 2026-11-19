//! SHARD: Akira red over jet black (Haseeb's pick, 2026-11-16). Every colour the UI paints
//! comes from [`Pal`], and every shape it is cut into from [`outline`]: 26 px chamfers on the
//! panels, 9 px on fields and code, slanted plates, a blade of a tail, mane slivers. Two
//! colours plus bone: bone is the text, the approval's danger plate and NEEDS YOU (the only
//! bone nameplate). Ink on red, never bone on red (bone on her red is 3.4:1). A nameplate's
//! state is its fill, its outline and its motion together, so it reads in greyscale and in
//! the corner of an eye (r21 critics: outline alone was not enough).

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
    /// The big panels' frame (the Line, the reader): 1 px, a dark red that sits back so her
    /// red (name plates, the field, her dot) stands out on it.
    pub border: u32,
    /// Her: mane red, turned 10° from Akira's so the UI and her hair read as one red.
    pub accent: u32,
    /// Decorative red (the dimmest trailing sliver, the selection).
    pub accent_dim: u32,
    /// Red type on bone (NEEDS YOU): her red is 3.4:1 there.
    pub deep: u32,
    /// Bone: body copy.
    pub text: u32,
    /// Labels, hints, LATER, offline.
    pub dim: u32,
    /// The focus ring and the caret.
    pub focus: u32,
    /// The risk banner, ERROR, QUIT?: a bone plate with ink type.
    pub danger: u32,
    /// DONE: a hollow red plate with a big tick.
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
    /// The faint bar behind his turns in the Line.
    pub you_bar: u32,
    /// The sleeping plate's fill.
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
        border: 0x8a0f24,
        accent: 0xff1438,
        accent_dim: 0x6a0b1c,
        deep: 0xb80e2a,
        text: 0xf4f0ea,
        dim: 0xa8a09b,
        focus: 0xf4f0ea,
        danger: 0xf4f0ea,
        success: 0xff1438,
        ink: 0x0a0a0b,
        rule: 0x3a0e16,
        field: 0x121215,
        code: 0x0b0b0d,
        you: 0xd9d3cc,
        you_bar: 0x18171a,
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
            ("deep", &mut self.deep),
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
    /// alert, error, done and mic are her red (fill and shape tell them apart); asleep, offline
    /// and a paused wake word are text-dim.
    pub fn tone(&self, key: &str) -> u32 {
        match key {
            "done" => self.success,
            "sleep" | "offline" | "paused" => self.dim,
            _ => self.accent,
        }
    }

    /// A nameplate's paint: the state is its fill, outline and motion together. `key`: the
    /// state's tone key; `tone`: its colour; `invert`: a solid plate (a pack's own states);
    /// `blink`: the second half of the 1 Hz beat.
    ///
    /// | state | fill | outline | motion |
    /// |---|---|---|---|
    /// | online | hollow | slant | |
    /// | thinking | hollow | slant | three slivers step behind it |
    /// | working, heard | half: her name on red | arrow | a light sweep crosses the red |
    /// | speaking | solid red | arrow | |
    /// | mic live | solid red | swallowtail | its mic pulses in a bone disc |
    /// | NEEDS YOU | bone, deep-red type | swallowtail | flips bone / black at 1 Hz |
    /// | done | hollow | square, a big tick through its corner | |
    /// | error | black, red type | toothed, its ✕ shard dropped | |
    /// | offline | hollow, grey | square, dashed | |
    /// | sleeping | grey, bone type, text-dim outline | square, 5 px lower | |
    pub fn look(&self, plate: Plate, key: &str, tone: u32, invert: bool, blink: bool) -> Look {
        let mut l = Look { plate, fill: self.panel, edge: tone, ink: tone, mark: tone, dash: None, drop: 0.0, trail: false, half: false, halo: None };
        match key {
            "error" => return Look { fill: self.bg, edge: self.accent, ink: self.accent, mark: self.accent, ..l },
            // The only bone plate; its other beat is black with a bone edge.
            "alert" => {
                return if blink {
                    Look { fill: self.bg, edge: self.text, ink: self.text, mark: self.text, ..l }
                } else {
                    Look { fill: self.text, edge: self.text, ink: self.deep, mark: self.deep, ..l }
                };
            }
            // The mic pulses: a deep-red mic in a bone disc, then an ink one on the red.
            "mic" if invert => {
                let (mark, halo) = if blink { (self.ink, None) } else { (self.deep, Some(self.text)) };
                return Look { fill: tone, edge: tone, ink: self.ink, mark, halo, ..l };
            }
            "speak" => return Look { fill: tone, edge: tone, ink: self.ink, mark: self.ink, ..l },
            "busy" | "heard" => {
                l.half = true;
                l.mark = self.ink;
            }
            "offline" | "paused" => l.dash = Some([3.0, 2.5]),
            // Asleep: a grey plate, its outline text-dim, its type bone (r38: dim on grey read
            // as a smudge next to ONLINE).
            "sleep" => {
                l.fill = self.asleep;
                l.edge = self.dim;
                l.ink = self.text;
                l.mark = self.text;
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
            l.fill = tone;
            l.edge = tone;
            l.ink = self.ink;
            l.mark = self.ink;
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
    /// ▢ away: offline, sleeping.
    Square,
    /// ☑ done: square, a big tick through its top-left corner.
    Check,
    /// The split, toothed plate: ERROR.
    Split,
}

impl Plate {
    pub fn parse(s: &str) -> Option<Plate> {
        Some(match s {
            "slant" => Plate::Slant,
            "arrow" => Plate::Arrow,
            "flag" => Plate::Flag,
            "square" => Plate::Square,
            "check" => Plate::Check,
            "split" => Plate::Split,
            _ => return None,
        })
    }

    /// Extra room inside the plate, left and right of its content (logical px).
    pub fn pad(self) -> (f32, f32) {
        match self {
            Plate::Slant | Plate::Square => (12.0, 14.0),
            Plate::Check => (16.0, 14.0),
            Plate::Arrow => (12.0, 20.0),
            Plate::Flag => (12.0, 22.0),
            Plate::Split => (10.0, 14.0),
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
    /// Its name on a red block, the rest hollow (working); a light sweep crosses the block.
    pub half: bool,
    /// A disc behind its glyph (the live mic's pulse).
    pub halo: Option<u32>,
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
            let mut p = teeth(x0, x1, y0, 10.0, 4.0);
            p.extend(teeth(x1, x0, y1, 10.0, 4.0));
            one(p)
        }
        Cut::Tag(plate) => {
            let s = 9.0f32.min(h * 0.38);
            match plate {
                Plate::Slant => one(vec![(x0 + s, y0), (x1, y0), (x1 - s, y1), (x0, y1)]),
                Plate::Arrow => one(vec![(x0 + s, y0), (x1 - 4.0, y0), (x1 + 6.0, mid), (x1 - 4.0, y1), (x0, y1)]),
                Plate::Flag => one(vec![(x0 + s, y0), (x1 + 4.0, y0), (x1 - 5.0, mid), (x1 + 4.0, y1), (x0, y1)]),
                Plate::Square | Plate::Check => one(vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]),
                Plate::Split => {
                    // The glyph's shard breaks off and drops; the rest wears hazard teeth top
                    // and bottom, pointing out (danger's serration, in her red; the plate
                    // inside them is `tag_inner`).
                    let gx = x0 + 25.0;
                    let mut body = teeth(gx + 6.0, x1, y0 - SPLIT_TD, SPLIT_TOOTH, SPLIT_TD);
                    body.extend(teeth(x1 - s, gx + 1.0, y1 + SPLIT_TD, SPLIT_TOOTH, SPLIT_TD));
                    vec![vec![(x0 + s, y0 + 3.0), (gx + 2.0, y0 + 3.0), (gx - 3.0, y1 + 3.0), (x0, y1 + 3.0)], body]
                }
            }
        }
    }
}

/// ERROR's hazard teeth: width and height (outside the plate box).
const SPLIT_TOOTH: f32 = 8.0;
const SPLIT_TD: f32 = 3.5;

/// Where a plate's fill goes when it is not simply its outline inset by the edge: ERROR's
/// teeth stay solid (red crests above and below), its black is the plain plate inside them.
pub fn tag_inner(plate: Plate, x0: f32, y0: f32, x1: f32, y1: f32, edge: f32) -> Option<Vec<Vec<Pt>>> {
    if plate != Plate::Split {
        return None;
    }
    let s = 9.0f32.min((y1 - y0) * 0.38);
    let gx = x0 + 25.0;
    let shard = offset(&[(x0 + s, y0 + 3.0), (gx + 2.0, y0 + 3.0), (gx - 3.0, y1 + 3.0), (x0, y1 + 3.0)], -edge);
    let (t, b) = (y0 + edge, y1 - edge);
    // The body's slanted sides (as `outline` cuts them), each moved in by the edge.
    let f = |y: f32| (y - y0) / (y1 - y0);
    let (left, right) = (|y: f32| gx + 6.0 - 5.0 * f(y) + edge, |y: f32| x1 - s * f(y) - edge);
    let body = vec![(left(t), t), (right(t), t), (right(b), b), (left(b), b)];
    Some(vec![shard, body])
}

/// A serrated edge from xa to xb (either way), tips on y: valleys `td` toward the plate (down
/// walking right, up walking left), a tip every `tw` or so.
fn teeth(xa: f32, xb: f32, y: f32, tw: f32, td: f32) -> Vec<Pt> {
    let dir = if xb >= xa { 1.0 } else { -1.0 };
    // Tips point out: up on a top edge (walked left to right), down on a bottom one.
    let inside = y + dir * td;
    let n = ((xb - xa).abs() / tw).round().max(1.0) as usize;
    let step = (xb - xa) / n as f32;
    let mut p = Vec::with_capacity(2 * n + 1);
    for i in 0..n {
        let x = xa + step * i as f32;
        p.push((x, inside));
        p.push((x + step / 2.0, y));
    }
    p.push((xb, inside));
    p
}

/// Solid marks a nameplate carries outside its outline, in its mark colour: DONE's big tick
/// (it breaks out through the top edge). Part of the X shape.
pub fn tag_marks(plate: Plate, x0: f32, y0: f32, _x1: f32, y1: f32) -> Vec<Vec<Pt>> {
    match plate {
        Plate::Check => vec![stroke_poly_pts(&[(x0 + 6.0, y0 + 12.5), (x0 + 12.5, y1 - 5.0), (x0 + 27.0, y0 - 6.0)], 1.9)],
        _ => Vec::new(),
    }
}

/// A polyline as a filled outline `half` px each side (mitred joints, square ends).
fn stroke_poly_pts(line: &[Pt], half: f32) -> Vec<Pt> {
    let n = line.len();
    let normal = |a: Pt, b: Pt| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        (-dy / l, dx / l)
    };
    let side = |sgn: f32| -> Vec<Pt> {
        (0..n)
            .map(|i| {
                let (p, c, nx) = (line[i.saturating_sub(1)], line[i], line[(i + 1).min(n - 1)]);
                let n1 = if i == 0 { normal(c, nx) } else { normal(p, c) };
                let n2 = if i + 1 == n { n1 } else { normal(c, nx) };
                let k = (1.0 + n1.0 * n2.0 + n1.1 * n2.1).max(0.25);
                (c.0 + sgn * half * (n1.0 + n2.0) / k, c.1 + sgn * half * (n1.1 + n2.1) / k)
            })
            .collect()
    };
    let mut out = side(1.0);
    out.extend(side(-1.0).into_iter().rev());
    out
}

/// WORKING's red block (its name), a slant on the box, and the light sweep's piece of it at
/// `phase` (0..1 across, `None` resting).
pub fn half_block(x0: f32, y0: f32, x1: f32, y1: f32, phase: Option<f32>) -> (Vec<Pt>, Option<Vec<Pt>>) {
    let s = 9.0f32.min((y1 - y0) * 0.38);
    let block = vec![(x0 + s, y0), (x1, y0), (x1 - s, y1), (x0, y1)];
    // In slant coordinates (u = x at the bottom edge) the block is x0..x1 - s.
    let sweep = phase.and_then(|ph| {
        let w = 9.0;
        let u = x0 - w + ph * (x1 - s - x0 + w);
        let (a, b) = (u.max(x0), (u + w).min(x1 - s));
        (b > a).then(|| vec![(a + s, y0), (b + s, y0), (b, y1), (a, y1)])
    });
    (block, sweep)
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

/// One outline easing into another (`k` 0..1). Both are cast from their common box's centre
/// along the same rays (at every corner of either, plus an even spread), so it is exactly `a`
/// at 0 and exactly `b` at 1, no corner rounded off, and an edge the two share (a plate's top
/// and bottom) stays straight on the way. Collinear and coincident points are dropped, so its
/// edge and fill (an inset of it) stay clean.
pub fn morph(a: &[Pt], b: &[Pt], k: f32) -> Vec<Pt> {
    if k <= 0.0 || b.len() < 3 {
        return a.to_vec();
    }
    if k >= 1.0 || a.len() < 3 {
        return b.to_vec();
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in a.iter().chain(b) {
        (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
    }
    let c = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let angle = |p: &Pt| (p.1 - c.1).atan2(p.0 - c.0);
    let tau = std::f32::consts::TAU;
    // (angle, a corner's): a corner's ray wins over an even one next to it.
    let mut th: Vec<(f32, bool)> = a.iter().chain(b).map(|p| (angle(p), true)).chain((0..64).map(|i| (-std::f32::consts::PI + tau * i as f32 / 64.0, false))).collect();
    th.sort_by(|x, y| x.0.total_cmp(&y.0).then(y.1.cmp(&x.1)));
    th.dedup_by(|x, y| (x.0 - y.0).abs() < 1e-5);
    let pts: Vec<(Pt, bool)> = th
        .iter()
        .filter_map(|&(t, corner)| {
            let d = (t.cos(), t.sin());
            let (p, q) = (cast(a, c, d)?, cast(b, c, d)?);
            Some(((p.0 + (q.0 - p.0) * k, p.1 + (q.1 - p.1) * k), corner))
        })
        .collect();
    simplify(pts)
}

/// Where a ray from `c` along `d` leaves the polygon (its farthest crossing).
fn cast(p: &[Pt], c: Pt, d: Pt) -> Option<Pt> {
    let mut best: Option<f32> = None;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        let e = (b.0 - a.0, b.1 - a.1);
        let den = d.0 * e.1 - d.1 * e.0;
        if den.abs() < 1e-9 {
            continue;
        }
        let w = (a.0 - c.0, a.1 - c.1);
        let t = (w.0 * e.1 - w.1 * e.0) / den;
        let u = (w.0 * d.1 - w.1 * d.0) / den;
        if t > 0.0 && (-1e-4..=1.0 + 1e-4).contains(&u) {
            best = Some(best.map_or(t, |b: f32| b.max(t)));
        }
    }
    best.map(|t| (c.0 + d.0 * t, c.1 + d.1 * t))
}

/// Without points closer than a quarter pixel to the last kept, or on the line between their
/// neighbours.
fn simplify(p: Vec<(Pt, bool)>) -> Vec<Pt> {
    let near = |a: Pt, b: Pt| (a.0 - b.0).hypot(a.1 - b.1) <= 0.25;
    let mut w: Vec<(Pt, bool)> = Vec::with_capacity(p.len());
    for q in p {
        match w.last_mut() {
            // Two on top of each other: a corner's is kept.
            Some(l) if near(l.0, q.0) => {
                if q.1 && !l.1 {
                    *l = q;
                }
            }
            _ => w.push(q),
        }
    }
    while w.len() > 3 && near(w[0].0, w[w.len() - 1].0) {
        let l = w.pop().expect("more than three");
        if l.1 && !w[0].1 {
            w[0] = l;
        }
    }
    let mut v: Vec<Pt> = w.into_iter().map(|q| q.0).collect();
    let mut changed = true;
    while changed && v.len() > 3 {
        changed = false;
        let n = v.len();
        for i in 0..n {
            let (a, b, c) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
            let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            let len = (c.0 - a.0).hypot(c.1 - a.1).max(1e-6);
            if (cross / len).abs() < 0.02 {
                v.remove(i);
                changed = true;
                break;
            }
        }
    }
    v
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
/// `bg` paints cut-outs. Kinds: dot, dots, gear, play (and back), bang, check, cross (and `shard`: the
/// ✕ broken off, tilted), moon, ring, rec, disc, note, mic, hf, warn.
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
                "disc" => fill(window, &circle(5.5, 5.5, 5.5), fg),
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
                // A padlock: the shackle's arch over a body, a keyhole in it.
                "lock" => {
                    let arch: Vec<Pt> = std::iter::once((3.0, 5.4))
                        .chain((0..=12).map(|i| {
                            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                            (5.5 + 2.5 * a.cos(), 3.6 + 2.5 * a.sin())
                        }))
                        .chain(std::iter::once((8.0, 5.4)))
                        .collect();
                    stroke(window, &arch, 1.4, fg);
                    fill(window, &[(1.4, 5.0), (9.6, 5.0), (9.6, 10.8), (1.4, 10.8)], fg);
                    fill(window, &[(4.9, 6.9), (6.1, 6.9), (6.1, 9.0), (4.9, 9.0)], bg);
                }
                "play" => fill(window, &[(2.0, 0.8), (10.0, 5.5), (2.0, 10.2)], fg),
                "back" => fill(window, &[(9.0, 0.8), (1.0, 5.5), (9.0, 10.2)], fg),
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
    #[test]
    fn a_morph_is_exact_at_its_ends_and_keeps_every_corner() {
        let arrow = [(9.0, 0.0), (196.0, 0.0), (206.0, 12.5), (196.0, 25.0), (0.0, 25.0)];
        let square = [(40.0, 0.0), (160.0, 0.0), (160.0, 25.0), (40.0, 25.0)];
        let has = |v: &[Pt], p: Pt| v.iter().any(|q| (q.0 - p.0).abs() < 1e-3 && (q.1 - p.1).abs() < 1e-3);
        let m0 = morph(&arrow, &square, 1e-6);
        assert!(arrow.iter().all(|&p| has(&m0, p)), "every corner of the start");
        let m1 = morph(&arrow, &square, 1.0 - 1e-6);
        assert!(square.iter().all(|&p| has(&m1, p)), "every corner of the end: {m1:?}");
        let mid = morph(&arrow, &square, 0.5);
        let (x0, x1) = mid.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
        assert!(x0 > 0.0 && x0 < 40.0 && x1 > 160.0 && x1 < 206.0, "between the two: {x0}..{x1}");
        assert!(mid.iter().all(|p| p.1 >= -1e-3 && p.1 <= 25.0 + 1e-3));
        // Their shared top and bottom stay straight lines: no notch, nothing under the plate.
        assert!(mid.iter().all(|p| !(p.0 > 45.0 && p.0 < 155.0) || p.1.abs() < 1e-3 || (p.1 - 25.0).abs() < 1e-3), "{mid:?}");
        // Clean enough to inset: no two points on top of each other, no point on a straight run.
        assert!(mid.len() <= 40, "{} points", mid.len());
        let inset = offset(&mid, -1.5);
        assert!(inset.iter().all(|p| p.1 > 1.0 && p.1 < 24.0), "an inset that stays inside: {inset:?}");
    }

    use super::{Cut, Pal, Plate, Pt, bubble, contrast, half_block, morph, offset, outline, tag_marks};

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
            ("deep on bone (NEEDS YOU)", P.deep, P.text),
            ("bone on black (NEEDS YOU's other beat)", P.text, P.bg),
            ("bone on the sleeping plate", P.text, P.asleep),
            ("the sleeping plate's outline on panel", P.dim, P.panel),
            ("ink on bone (READ)", P.ink, P.text),
            ("his words on his bar", P.you, P.you_bar),
            ("ink on WORKING's sweep", P.ink, super::mix(P.accent, P.text, 0.45)),
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
        for k in ["online", "think", "busy", "speak", "music", "alert", "mic", "wake", "error", "done"] {
            assert_eq!(P.tone(k), P.accent, "{k}");
        }
        assert_eq!(P.tone("sleep"), P.dim);
        assert_eq!(P.tone("offline"), P.dim);
    }

    /// Every state's (plate, key, invert) as the pack and the voice set them.
    const STATES: [(&str, Plate, &str, bool); 13] = [
        ("online", Plate::Slant, "online", false),
        ("thinking", Plate::Slant, "think", false),
        ("working", Plate::Arrow, "busy", false),
        ("heard", Plate::Arrow, "heard", false),
        ("speaking", Plate::Arrow, "speak", false),
        ("music", Plate::Arrow, "music", false),
        ("mic live", Plate::Flag, "mic", true),
        ("needs you", Plate::Flag, "alert", true),
        ("done", Plate::Check, "done", false),
        ("error", Plate::Split, "error", true),
        ("offline", Plate::Square, "offline", false),
        ("sleeping", Plate::Square, "sleep", false),
        ("wake word", Plate::Slant, "wake", false),
    ];

    #[test]
    fn needs_you_is_the_only_bone_plate() {
        let bone = P.look(Plate::Flag, "alert", P.accent, true, false);
        let black = P.look(Plate::Flag, "alert", P.accent, true, true);
        assert_eq!((bone.fill, bone.ink), (P.text, P.deep));
        assert_eq!((black.fill, black.edge, black.ink), (P.bg, P.text, P.text));
        for (what, plate, key, invert) in STATES {
            for blink in [false, true] {
                let l = P.look(plate, key, P.tone(key), invert, blink);
                assert!(contrast(l.ink, l.fill) >= 4.5, "{what} {blink}: {:.2}", contrast(l.ink, l.fill));
                if key != "alert" {
                    assert!(l.fill != P.text && l.edge != P.text, "{what} uses bone");
                }
            }
        }
        // Error: black, red type and edge (r21: not bone).
        let e = P.look(Plate::Split, "error", P.accent, true, false);
        assert_eq!((e.fill, e.edge, e.ink), (P.bg, P.accent, P.accent));
    }

    #[test]
    fn states_differ_by_fill_and_shape_in_greyscale() {
        // Luma of a plate's fill, its edge, and whether it carries a mark or moves.
        let luma = |c: u32| {
            let (r, g, b) = (((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32);
            (0.299 * r + 0.587 * g + 0.114 * b).round() as i32
        };
        let sig: Vec<(&str, String)> = STATES
            .iter()
            .filter(|s| s.2 != "heard" && s.2 != "wake" && s.2 != "music")
            .map(|&(what, plate, key, invert)| {
                let l = P.look(plate, key, P.tone(key), invert, false);
                let o = outline(Cut::Tag(plate), 0.0, 0.0, 120.0, 25.0);
                let marks = tag_marks(plate, 0.0, 0.0, 120.0, 25.0).len();
                (what, format!("{:?} fill{} edge{} dash{} half{} trail{} marks{} drop{}", o, luma(l.fill) / 24, luma(l.edge) / 24, l.dash.is_some(), l.half, l.trail, marks, l.drop))
            })
            .collect();
        for i in 0..sig.len() {
            for j in i + 1..sig.len() {
                assert_ne!(sig[i].1, sig[j].1, "{} and {} look alike in grey", sig[i].0, sig[j].0);
            }
        }
    }

    #[test]
    fn the_sweep_stays_on_the_block() {
        let (blk, _) = half_block(10.0, 0.0, 80.0, 25.0, None);
        for ph in [0.0, 0.3, 0.7, 1.0] {
            if let (_, Some(sw)) = half_block(10.0, 0.0, 80.0, 25.0, Some(ph)) {
                for (x, y) in sw {
                    // Inside the parallelogram: left of the right edge, right of the left one.
                    let lean = 9.0 * (25.0 - y) / 25.0;
                    assert!(x >= 10.0 + lean - 0.01 && x <= 80.0 - 9.0 + lean + 0.01, "{ph}: {x},{y} vs {blk:?}");
                }
            }
        }
    }

    #[test]
    fn silhouettes_differ() {
        let o = |p| (outline(Cut::Tag(p), 0.0, 0.0, 120.0, 25.0), tag_marks(p, 0.0, 0.0, 120.0, 25.0));
        let all = [Plate::Slant, Plate::Arrow, Plate::Flag, Plate::Square, Plate::Check, Plate::Split].map(o);
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
        assert_eq!(all[5].0.len(), 2, "the split plate is two pieces");
        assert!(all[4].1[0].iter().any(|p| p.1 < -4.0), "the tick breaks out of the top edge");
        assert!(all[5].0[1].len() > 20, "the error plate is toothed");
        assert!(all[5].0[1].iter().any(|p| p.1 < -3.0) && all[5].0[1].iter().any(|p| p.1 > 28.0), "its teeth point out");
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
