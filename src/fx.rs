//! Effects as pure functions of time: glitch bursts (RGB split, slice displacement,
//! scanlines), the pat squash, the breathing bob, the attention hop, hair glints and the
//! teleport bolt's geometry. The view samples them while they run and redraws only then.
//! Some looks are modelled on what CapriCore's effects do on screen, and the palette below is
//! its colours; none of this code is copied or translated from CapriCore's source. The bolt and
//! the music notes were rewritten clean-room, from a behaviour spec alone, by someone who never
//! read their earlier, CapriCore-derived versions.

use crate::anim::Rng;
use crate::frames::NOTE_ANGLES;
use crate::shape::{self, ShapeRect};
use std::f32::consts::PI;
use std::time::{Duration, Instant};

pub const CYAN: u32 = 0x00ffdd;
pub const PINK: u32 = 0xff1769;
pub const VIOLET: u32 = 0xa668ff;
pub const YELLOW: u32 = 0xedff43;
pub const BOLT_RED: u32 = 0xff003d;

fn ms(since: Duration) -> f32 {
    since.as_secs_f32() * 1000.0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlitchKind {
    /// Minor mood change / periodic interference.
    Burst,
    /// A big mood change (into or out of an attention state, waking up).
    Big,
    /// Coming online: bands fill in through a long stutter.
    Appear,
    /// Teleport landing: a red silhouette reassembles, then a short settle.
    Land,
    /// Dematerialise: bands drop out while the interference rises.
    Vanish,
    /// The cut in or out of a motion's drawing (a walk, a dance, a dangle): ~130 ms, a few
    /// small shifts inside the body, no colour split, bars or cells, the tag left whole.
    Seam,
}

/// Glitch envelopes: `(start, strength)` steps over 0..1. Build-up (small offsets), a
/// hard peak (the biggest offsets, the thickest bars), then smaller aftershocks with clean
/// gaps between them, then settled. The pattern re-rolls every frame throughout.
const STUTTER_MINOR: [(f32, f32); 6] = [(0.0, 0.32), (0.18, 0.9), (0.52, 0.0), (0.64, 0.5), (0.75, 0.0), (0.85, 0.0)];
const STUTTER_BIG: [(f32, f32); 9] =
    [(0.0, 0.3), (0.1, 0.5), (0.2, 1.0), (0.62, 0.0), (0.68, 0.6), (0.76, 0.0), (0.84, 0.35), (0.89, 0.0), (0.95, 0.0)];
const STUTTER_APPEAR: [(f32, f32); 6] = [(0.0, 1.0), (0.6, 0.0), (0.68, 0.65), (0.74, 0.0), (0.83, 0.45), (0.89, 0.0)];

fn stutter(table: &[(f32, f32)], u: f32, seed: u64) -> f32 {
    // Steps move ±0.01 and levels ±12 % per glitch, so no two bursts match.
    let mut rng = Rng::new(seed ^ 0x5157_7e7e);
    let mut level = 0.0;
    for (i, &(at, s)) in table.iter().enumerate() {
        let at = if i == 0 { 0.0 } else { at + rng.range(-0.01, 0.01) };
        let s = (s * rng.range(0.88, 1.12)).min(1.0);
        if u < at {
            break;
        }
        level = s;
    }
    let last = table[table.len() - 1].0;
    if u >= last {
        level *= ((1.0 - u) / (1.0 - last)).clamp(0.0, 1.0);
    }
    level
}

/// Every kind of cut on her body, for `Glitch::for_cut`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Gone here, back there (the teleport legs, `Vanish` and `Land`, are its cover).
    #[allow(dead_code)]
    Teleport,
    /// Into or out of a summon's attention mood (NEEDS YOU).
    Summon,
    /// A bid in place: she flags him down where she is.
    Bid,
    /// One drawing to another in place (CapriCore <-> lachy, a crouch, a sit, a point).
    Pose,
    /// Into or out of a motion's drawing (a walk, a dance, a leap, a dangle).
    Motion,
    /// Into or out of her sleep, and the wake's family changes.
    Sleep,
    /// A perch <-> laydown swap, a drop onto a window.
    Perch,
    /// Any other mood change (the tag's status word changes; the body only seams).
    Mood,
    /// Music on or off.
    Music,
    /// A startle's jolt and an emote's fx beat.
    Jolt,
}

#[cfg(test)]
impl Transition {
    pub const ALL: [Transition; 10] = [
        Transition::Teleport,
        Transition::Summon,
        Transition::Bid,
        Transition::Pose,
        Transition::Motion,
        Transition::Sleep,
        Transition::Perch,
        Transition::Mood,
        Transition::Music,
        Transition::Jolt,
    ];
}

#[derive(Clone, Copy, Debug)]
pub struct Glitch {
    pub t0: Instant,
    pub kind: GlitchKind,
    pub ms: f32,
    seed: u64,
    /// The quiet passive-teleport legs: short, small offsets, little fringing.
    pub soft: bool,
    /// The status tag tears with the body (a mood's burst); an art seam's cover leaves it be.
    pub tears_tag: bool,
}

impl Glitch {
    pub fn new(kind: GlitchKind, now: Instant, seed: u64) -> Glitch {
        let ms = match kind {
            GlitchKind::Burst => 480.0,
            GlitchKind::Big => 850.0,
            GlitchKind::Appear => 900.0,
            GlitchKind::Land => 330.0,
            GlitchKind::Vanish => 300.0,
            GlitchKind::Seam => SEAM_MS,
        };
        Glitch { t0: now, kind, ms, seed, soft: false, tears_tag: kind != GlitchKind::Seam }
    }

    /// A pose swap in place (perch <-> laydown) cut at `at`: a light seam whose one step is the
    /// cut frame itself (the frames before it stay clean and lay it underneath; r36: a burst after
    /// the cut tore her hanging leg and threw bars), smaller shifts, the tag left whole.
    pub fn swap(at: Instant, seed: u64) -> Glitch {
        Glitch { t0: at, kind: GlitchKind::Seam, ms: SWAP_MS, seed, soft: true, tears_tag: false }
    }

    /// The glitch a cut gets (r42: the heavy glitch is for teleports and summons only; every
    /// other pose or family change gets the swap's light seam, the tag whole). `None`: the
    /// teleport's own legs (`Vanish`, `Land`) cover it.
    pub fn for_cut(t: Transition, at: Instant, seed: u64) -> Option<Glitch> {
        match t {
            Transition::Teleport => None,
            Transition::Summon => Some(Glitch::new(GlitchKind::Big, at, seed)),
            Transition::Bid => Some(Glitch::new(GlitchKind::Burst, at, seed)),
            _ => Some(Glitch::swap(at, seed)),
        }
    }

    /// A quiet teleport leg (passive moves): a shorter dissolve, and a ~150 ms rebuild
    /// with little RGB fringing.
    pub fn soft(kind: GlitchKind, now: Instant, seed: u64) -> Glitch {
        let ms = match kind {
            GlitchKind::Land => 70.0,
            GlitchKind::Vanish => 200.0,
            _ => Glitch::new(kind, now, seed).ms,
        };
        Glitch { t0: now, kind, ms, seed, soft: true, tears_tag: true }
    }

    pub fn done(&self, now: Instant) -> bool {
        ms(now.saturating_duration_since(self.t0)) >= self.ms
    }

    fn u(&self, now: Instant) -> f32 {
        (ms(now.saturating_duration_since(self.t0)) / self.ms).clamp(0.0, 1.0)
    }

    /// Interference 0..1.
    pub fn strength(&self, now: Instant) -> f32 {
        let u = self.u(now);
        if self.soft && self.kind == GlitchKind::Land {
            // Reassembled at once, then a short settle (2 frames) that registers the landing.
            return if u < 0.5 { 0.6 } else { 0.0 };
        }
        match self.kind {
            GlitchKind::Burst => stutter(&STUTTER_MINOR, u, self.seed),
            GlitchKind::Big => stutter(&STUTTER_BIG, u, self.seed),
            GlitchKind::Appear => stutter(&STUTTER_APPEAR, u, self.seed),
            // Reassembled by ~45 %, then a short settle.
            GlitchKind::Land => {
                if u < 0.45 {
                    1.0
                } else {
                    0.6 * (1.0 - (u - 0.45) / 0.55)
                }
            }
            GlitchKind::Vanish => {
                if u < 0.15 {
                    0.6
                } else {
                    1.0
                }
            }
            // Two steps, then settled: the cut is on its first frame. A swap's: one, smaller
            // (a limb never comes off).
            GlitchKind::Seam if self.soft => {
                if u < 0.6 {
                    0.6
                } else {
                    0.0
                }
            }
            GlitchKind::Seam => {
                if u < 0.5 {
                    1.0
                } else if u < 0.8 {
                    0.45
                } else {
                    0.0
                }
            }
        }
    }

    /// Fraction of horizontal bands drawn (teleport legs only; a glitch keeps them all).
    pub fn coverage(&self, now: Instant) -> f32 {
        let u = self.u(now);
        match self.kind {
            GlitchKind::Burst | GlitchKind::Big | GlitchKind::Appear | GlitchKind::Seam => 1.0,
            // Starts a quarter in, so the body never blinks out between bolt and arrival.
            GlitchKind::Land if self.soft => 1.0,
            GlitchKind::Land => 0.25 + 0.75 * (u / 0.4).min(1.0),
            GlitchKind::Vanish => 1.0 - 0.8 * u,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// A quiet exit squeezes her, in one piece, into a sliver (no bands): her width at `now`.
    pub fn squeeze_sx(&self, now: Instant) -> Option<f32> {
        if !(self.soft && self.kind == GlitchKind::Vanish) {
            return None;
        }
        let u = self.u(now);
        Some(1.0 - 0.94 * u * u * (3.0 - 2.0 * u))
    }

    /// Fraction of bands drawn as a single colour channel instead of the image: coming
    /// online, the figure builds up from channel ghosts to full colour in ~400 ms, never
    /// from nothing.
    pub fn solid(&self, now: Instant) -> f32 {
        match self.kind {
            GlitchKind::Appear => 0.7 * (1.0 - ms(now.saturating_duration_since(self.t0)) / 400.0).max(0.0),
            _ => 0.0,
        }
    }

    /// The face stays readable in every frame, except while dematerialising.
    pub fn keeps_face(&self) -> bool {
        self.kind != GlitchKind::Vanish
    }

    /// Teleport legs turn the figure into a red silhouette: late in the dissolve, early
    /// in the landing.
    pub fn tint(&self, now: Instant) -> Option<u32> {
        let u = self.u(now);
        match self.kind {
            GlitchKind::Vanish if u > 0.45 => Some(BOLT_RED),
            GlitchKind::Land if self.soft => None,
            GlitchKind::Land if u < 0.3 => Some(BOLT_RED),
            _ => None,
        }
    }

    /// The pattern for drawn frame `step`: a new one every frame, like a bad signal.
    pub fn step_seed(&self, step: u64) -> u64 {
        self.seed ^ step.wrapping_add(1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }
}

/// One glitch step's inputs. Device px, avatar-local.
#[derive(Clone, Copy, Debug)]
pub struct GlitchStep {
    pub strength: f32,
    pub coverage: f32,
    pub seed: u64,
    /// Fixes which bands appear first (per glitch).
    pub order: u64,
    /// Rows to band: from `y0` (negative reaches the HUD above the avatar rect) to `h`.
    pub y0: i32,
    pub h: i32,
    /// The frame's visible box (x, y, w, h).
    pub body: (i32, i32, i32, i32),
    /// Face rows: small offsets only, never dropped, no split, lighter scanlines.
    pub face: Option<(i32, i32)>,
    /// Fraction of bands drawn as a solid colour channel (see `Glitch::solid`).
    pub solid: f32,
    /// The status tag's rows: one cut falls inside them, so the plate tears in two.
    pub tag: Option<(i32, i32)>,
    /// Device px per logical px (band heights, split widths).
    pub sf: f32,
    /// A teleport landing: bands come back top-down in one piece (no loose chunks), no
    /// chroma bars or corruption cells, lighter scanlines.
    pub calm: bool,
    /// Bands reaching this row and below stay where they are, with no split (a landing on a
    /// window top: her legs at the edge never tear across it).
    pub still_from: Option<i32>,
    /// A motion's seam (`GlitchKind::Seam`): shifts of a few px, no split, bars, cells or
    /// scanlines, a few tall bands.
    pub light: bool,
}

/// A motion seam's length.
pub const SEAM_MS: f32 = 130.0;
/// A pose swap's seam (a lounge in or out): one step, on the cut frame, then clean.
pub const SWAP_MS: f32 = 30.0;

/// How the body is drawn this frame. Device px, relative to the avatar rect.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyPlan {
    /// Squash about `pivot`.
    pub sx: f32,
    pub sy: f32,
    pub pivot: (f32, f32),
    /// Horizontal bands `(y0, y1, dx, dy)` that are drawn: source rows y0..y1, moved by
    /// (dx, dy). `None` = the whole body, unshifted.
    pub slices: Option<Vec<(i32, i32, i32, i32)>>,
    /// Per band (parallel to `slices`; empty = none): the red/blue split, device px. The
    /// band is drawn again `split` px to the left in red and to the right in blue, under it.
    pub split: Vec<i32>,
    /// Per band (parallel to `slices`; empty = none): draw the band as this solid colour.
    pub solid: Vec<Option<u32>>,
    /// Chroma bars and corruption cells `(rect, rgb)`, drawn over the body and clipped to
    /// it (`bar_rects`), so none poke out past the outline.
    pub bars: Vec<(ShapeRect, u32)>,
    /// Scanline darkness 0..1 (half in the face rows).
    pub scan: f32,
    pub face: Option<(i32, i32)>,
    /// Draw the body as a solid silhouette of this colour (teleport dissolve/land).
    pub tint: Option<u32>,
    /// A hanging leg kick over `slices`.
    pub leg: Option<LegKick>,
}

pub const SPLIT_RED: u32 = 0xff2448;
pub const SPLIT_BLUE: u32 = 0x2e7bff;

impl LegKick {
    /// Extra dx for a run `x..x+w` in band `i`: the kicking side's columns only.
    pub fn extra(&self, i: usize, x: i32, w: i32) -> i32 {
        let d = self.dx.get(i).copied().unwrap_or(0);
        if d != 0 && ((x + w / 2 >= self.split) == self.right) { d } else { 0 }
    }
}

impl BodyPlan {
    pub fn plain() -> BodyPlan {
        BodyPlan {
            sx: 1.0,
            sy: 1.0,
            pivot: (0.0, 0.0),
            slices: None,
            split: Vec::new(),
            solid: Vec::new(),
            bars: Vec::new(),
            scan: 0.0,
            face: None,
            tint: None,
            leg: None,
        }
    }

    pub fn is_plain(&self) -> bool {
        self.sx == 1.0 && self.sy == 1.0 && self.slices.is_none() && self.bars.is_empty()
    }

    pub fn has_ghosts(&self) -> bool {
        self.split.iter().any(|&d| d != 0)
    }

    /// Glitch layout for one step (a new random pattern every step). The whole silhouette
    /// stays drawn: 6–14 bands of random height across the figure, each moved sideways (up
    /// to 10–40 px at the peak, a few px in the build-up), each with its own red/blue split
    /// (4–8 px at the peak); chroma bars 4–10 px tall and corruption cells, clipped to the
    /// figure; faint scanlines. The face rows are one band with at most a few px of shift
    /// and no split. The status tag is cut inside, so it tears with the body.
    pub fn glitch(&mut self, g: GlitchStep) {
        let mut rng = Rng::new(g.seed | 1);
        let s = g.strength;
        // A calm landing tears half as far (no loose slivers flying off).
        let amp = (s * 1.15).min(1.0) * if g.calm { 0.5 } else { 1.0 };
        let (bx, by, bw, bh) = g.body;
        let top = by.clamp(g.y0, g.h);
        // Band edges: the tag (or the HUD) cut once, n over the figure, the face's edges.
        let mut cuts = vec![g.y0, top, g.h];
        match g.tag {
            Some((t0, t1)) if t1 - t0 > 6 && t0 >= g.y0 && t1 <= top => cuts.push(t0 + (t1 - t0) * rng.range(35.0, 65.0) as i32 / 100),
            _ if top - g.y0 > 8 => cuts.push(g.y0 + (top - g.y0) * rng.range(30.0, 70.0) as i32 / 100),
            _ => {}
        }
        // Some steps tear into a few tall blocks, others into many thin strips (a seam: a few
        // tall blocks only).
        let n = if g.light { 3 + rng.pick(3) } else { 6 + rng.pick(9) };
        let min_h = (if g.light { rng.range(24.0, 40.0) } else { rng.range(6.0, 14.0) } * g.sf) as i32;
        for _ in 1..n {
            cuts.push(rng.range(top as f32, g.h as f32) as i32);
        }
        if let Some((f0, f1)) = g.face {
            cuts.retain(|&c| c <= f0 || c >= f1);
            cuts.push(f0.clamp(g.y0, g.h));
            cuts.push(f1.clamp(g.y0, g.h));
        }
        cuts.sort_unstable();
        cuts.dedup();
        // Merge slivers into their neighbour (never across the face's edges).
        let is_face_edge = |c: i32| g.face.is_some_and(|(f0, f1)| c == f0 || c == f1);
        let mut merged: Vec<i32> = Vec::new();
        for c in cuts {
            match merged.last() {
                Some(&p) if c - p < min_h && !is_face_edge(c) && c != g.h && c != top => {}
                _ => merged.push(c),
            }
        }
        let mut slices = Vec::new();
        let mut split = Vec::new();
        let mut solid = Vec::new();
        let mut sign = if rng.range(0.0, 1.0) < 0.5 { -1.0 } else { 1.0 };
        // At the peak the shifts reach further (the floor rises with the strength).
        let lo = 10.0 + 8.0 * amp;
        for (k, w) in merged.windows(2).enumerate() {
            let (y0, y1) = (w[0], w[1]);
            if y1 <= y0 {
                continue;
            }
            let face = g.face.is_some_and(|(f0, f1)| y0 >= f0 && y1 <= f1);
            // Mostly alternating directions, so neighbouring bands tear apart.
            if rng.range(0.0, 1.0) < 0.75 {
                sign = -sign;
            }
            let still = g.still_from.is_some_and(|s| y1 > s);
            let (dx, d) = if still {
                (0, 0)
            } else if g.light {
                ((sign * rng.range(1.5, 5.0) * g.sf * amp).round() as i32, 0)
            } else if face {
                ((sign * rng.range(0.0, 3.0) * g.sf * amp).round() as i32, 0)
            } else if rng.range(0.0, 1.0) < 0.85 {
                ((sign * rng.range(lo, 40.0) * g.sf * amp).round() as i32, (rng.range(4.0, 8.0) * g.sf * amp).round() as i32)
            } else {
                (0, (rng.range(4.0, 8.0) * g.sf * amp).round() as i32)
            };
            // Teleport legs drop bands (coverage); a glitch never does.
            let keep = g.coverage >= 1.0
                || face
                || if g.calm { (y0 - top) as f32 <= g.coverage * bh.max(1) as f32 } else { hash01(y0.div_euclid(12) as u32, g.order as u32) < g.coverage };
            if !keep {
                continue;
            }
            let channel = (!face && !still && g.solid > 0.0 && hash01(k as u32 + 17, g.order as u32 ^ 0x5a5a) < g.solid)
                .then_some(if k % 2 == 0 { SPLIT_RED } else { SPLIT_BLUE });
            slices.push((y0, y1, dx, 0));
            split.push(d);
            solid.push(channel);
        }
        self.slices = Some(slices);
        self.split = split;
        self.solid = solid;
        self.face = g.face;
        let in_face = |y0: i32, y1: i32| g.face.is_some_and(|(f0, f1)| y1 > f0 && y0 < f1);
        let palette = [YELLOW, VIOLET, CYAN, PINK];
        // Chroma bars: 1 in the build-up and the aftershocks, 1–3 at the peak; 4–10 px tall,
        // thicker with the strength; anywhere from below the face to the knees.
        if s > 0.2 && !g.calm && !g.light {
            let bars = if s > 0.8 { 1 + rng.pick(3) } else { 1 };
            let t0 = g.face.map_or(by + bh / 5, |f| f.1 + (4.0 * g.sf) as i32);
            let t1 = (by + bh * 4 / 5).max(t0 + 1);
            let first = rng.pick(palette.len());
            for i in 0..bars {
                let hh = (4.0 + 6.0 * amp * rng.range(0.55, 1.0)).round().clamp(4.0, 10.0) as i32;
                let y = rng.range(t0 as f32, (t1 - hh) as f32) as i32;
                let x0 = bx + (rng.range(-0.1, 0.4) * bw as f32) as i32;
                let w = (rng.range(0.35, 1.0) * bw as f32) as u16;
                self.bars.push(((x0 as i16, y as i16, w, hh as u16), palette[(first + i) % palette.len()]));
            }
        }
        // Corruption cells, 2–4 px tall (never on the face).
        for i in 0..if g.calm || g.light { 0 } else { (s * 7.0) as usize } {
            let cw = (rng.range(8.0, 30.0) * g.sf) as u16;
            let ch = rng.range(2.0, 4.99) as u16;
            let x = bx + (rng.range(0.0, 1.0) * bw as f32) as i32;
            let y = by + (rng.range(0.0, 1.0) * bh as f32) as i32;
            if in_face(y, y + ch as i32) {
                continue;
            }
            self.bars.push(((x as i16, y as i16, cw, ch), palette[(i + 1) % palette.len()]));
        }
        self.scan = if g.light {
            0.0
        } else if g.calm {
            0.14 * s
        } else {
            0.32 * s
        };
    }

    /// Squash/stretch a rect about the pivot without opening gaps between bands.
    fn scale_rect(&self, r: ShapeRect) -> ShapeRect {
        if self.sx == 1.0 && self.sy == 1.0 {
            return r;
        }
        let (px, py) = self.pivot;
        let tx = |x: f32| (px + (x - px) * self.sx).round() as i32;
        let ty = |y: f32| (py + (y - py) * self.sy).round() as i32;
        let (x0, x1) = (tx(r.0 as f32), tx(r.0 as f32 + r.2 as f32));
        let (y0, y1) = (ty(r.1 as f32), ty(r.1 as f32 + r.3 as f32));
        (x0 as i16, y0 as i16, (x1 - x0).max(1) as u16, (y1 - y0).max(0) as u16)
    }

    /// The image rect after the squash: (x, y, w, h), device px.
    pub fn image_rect(&self, w: i32, h: i32) -> (f32, f32, f32, f32) {
        let (px, py) = self.pivot;
        let x0 = px + (0.0 - px) * self.sx;
        let y0 = py + (0.0 - py) * self.sy;
        (x0, y0, w as f32 * self.sx, h as f32 * self.sy)
    }

    /// The squashed body rects (no slicing).
    pub fn scaled(&self, rects: &[ShapeRect]) -> Vec<ShapeRect> {
        rects.iter().map(|&r| self.scale_rect(r)).filter(|r| r.3 > 0).collect()
    }

    /// `scaled` cut to the drawn bands and shifted with them (plus `dx`, `dy`). `only`:
    /// just these bands (by index).
    fn banded(&self, scaled: &[ShapeRect], dx: i32, dy: i32, only: Option<&dyn Fn(usize) -> bool>, out: &mut Vec<ShapeRect>) {
        let mv = |x: i16, y: i32, w: u16, h: i32, sx: i32, sy: i32| ((x as i32 + sx + dx) as i16, (y + sy + dy) as i16, w, h as u16);
        match &self.slices {
            None => out.extend(scaled.iter().map(|&(x, y, w, h)| mv(x, y as i32, w, h as i32, 0, 0))),
            Some(bands) => {
                for &(x, y, w, h) in scaled {
                    let (ry0, ry1) = (y as i32, y as i32 + h as i32);
                    for (i, &(b0, b1, sx, sy)) in bands.iter().enumerate() {
                        if only.is_some_and(|f| !f(i)) {
                            continue;
                        }
                        let (y0, y1) = (ry0.max(b0), ry1.min(b1));
                        if y1 > y0 {
                            let kick = self.leg.as_ref().map_or(0, |l| l.extra(i, x as i32, w as i32));
                            out.push(mv(x, y0, w, y1 - y0, sx + kick, sy));
                        }
                    }
                }
            }
        }
    }

    /// The body as drawn (squashed, banded, shifted), no ghosts or cells.
    pub fn body_rects(&self, rects: &[ShapeRect]) -> Vec<ShapeRect> {
        let mut out = Vec::new();
        self.banded(&self.scaled(rects), 0, 0, None, &mut out);
        out
    }

    /// Bands drawn as a solid colour: `(rect, rgb)`.
    pub fn solid_rects(&self, rects: &[ShapeRect]) -> Vec<(ShapeRect, u32)> {
        let scaled = self.scaled(rects);
        let mut out = Vec::new();
        let mut tmp = Vec::new();
        for (i, c) in self.solid.iter().enumerate() {
            let Some(c) = *c else { continue };
            tmp.clear();
            self.banded(&scaled, 0, 0, Some(&|j| j == i), &mut tmp);
            out.extend(tmp.iter().map(|&r| (r, c)));
        }
        out
    }

    /// The red/blue split silhouettes, band by band: `(rect, rgb)`.
    pub fn ghost_rects(&self, rects: &[ShapeRect]) -> Vec<(ShapeRect, u32)> {
        let scaled = self.scaled(rects);
        let mut out = Vec::new();
        let mut tmp = Vec::new();
        for (i, &d) in self.split.iter().enumerate() {
            if d == 0 {
                continue;
            }
            for (off, c) in [(-d, SPLIT_RED), (d, SPLIT_BLUE)] {
                tmp.clear();
                self.banded(&scaled, off, 0, Some(&|j| j == i), &mut tmp);
                out.extend(tmp.iter().map(|&r| (r, c)));
            }
        }
        out
    }

    /// Everything this plan draws, as shape rects (avatar-local).
    pub fn shape(&self, rects: &[ShapeRect]) -> Vec<ShapeRect> {
        let scaled = self.scaled(rects);
        let mut out = Vec::with_capacity(scaled.len() * 3);
        self.banded(&scaled, 0, 0, None, &mut out);
        out.extend(self.ghost_rects(rects).into_iter().map(|g| g.0));
        out
    }

    /// The bars and cells clipped to the body as drawn (squashed, banded, shifted).
    pub fn bar_rects(&self, rects: &[ShapeRect]) -> Vec<(ShapeRect, u32)> {
        if self.bars.is_empty() {
            return Vec::new();
        }
        let body = self.body_rects(rects);
        let mut out = Vec::new();
        for &((bx, by, bw, bh), c) in &self.bars {
            let (bx0, by0, bx1, by1) = (bx as i32, by as i32, bx as i32 + bw as i32, by as i32 + bh as i32);
            for &(x, y, w, h) in &body {
                let (x0, y0) = (bx0.max(x as i32), by0.max(y as i32));
                let (x1, y1) = (bx1.min(x as i32 + w as i32), by1.min(y as i32 + h as i32));
                if x1 > x0 && y1 > y0 {
                    out.push(((x0 as i16, y0 as i16, (x1 - x0) as u16, (y1 - y0) as u16), c));
                }
            }
        }
        out
    }
}

fn hash01(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The pat squash: a quick 7 % vertical squash, then a damped spring back.
pub const SQUASH_MS: f32 = 560.0;
pub fn squash(age: Duration) -> Option<(f32, f32)> {
    let t = ms(age);
    if t >= SQUASH_MS {
        return None;
    }
    let sy = if t < 80.0 {
        let u = t / 80.0;
        1.0 - 0.07 * (1.0 - (1.0 - u) * (1.0 - u))
    } else {
        let t = t - 80.0;
        1.0 - 0.07 * (-t / 120.0).exp() * (2.0 * PI * t / 280.0).cos()
    };
    // Never past the rest silhouette (no stretch, no widening): without a compositor the
    // shape may only grow back into pixels the view has painted (see the body canvas).
    Some((1.0, sy.min(1.0)))
}

/// A contact settle that comes to rest exactly at `total` (a cut there has no step): down 7 %
/// over its first 30 %, then eased back up, slowing into rest (r36: a cut in the middle of the
/// squash's rise hopped her up 3 px; one at rest after a still hold read as a pasted cut-out).
pub fn settle(age: Duration, total: Duration) -> Option<(f32, f32)> {
    let (t, n) = (ms(age), ms(total).max(1.0));
    if t >= n {
        return None;
    }
    let u = t / n;
    let d = if u < 0.3 {
        let k = u / 0.3;
        1.0 - (1.0 - k) * (1.0 - k)
    } else {
        (1.0 - (u - 0.3) / 0.7).powi(3)
    };
    Some((1.0, 1.0 - 0.07 * d))
}

/// Idle and music body motion, device px: `breath` lifts everything above the waist,
/// `nod` drops the head, `sway` shears the top of the figure sideways (full at the top
/// row, nothing at the shear line).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pose {
    pub breath: i32,
    pub nod: i32,
    pub sway: i32,
    /// Hanging (from the bar, a window side): rows below the grip shear this far at the
    /// bottom, the grip stays put. Overrides the others.
    pub swing: i32,
    /// Hanging: one shin kicks this far (at the foot) from the knee, on top of the swing.
    pub kick: i32,
    /// The kicking leg is the one right of `PoseRows::split` on screen.
    pub kick_right: bool,
    /// Her upper body bent toward the pointer (r43), device px at the neck, + = right: the torso
    /// curves from the waist (nothing at the hips, most at the neck), the head and hair go along
    /// as one piece (no slant), the legs stay planted.
    pub bend: i32,
    /// The bend a moment ago (`bend` lagged): the hair above her brow trails toward it, more the
    /// higher it is, and catches up once the bend holds.
    pub bend_hair: i32,
}

/// Rows (avatar-local) a pose acts on.
#[derive(Clone, Copy, Debug)]
pub struct PoseRows {
    /// First row to band (negative reaches the HUD area; nothing to draw there).
    pub y0: i32,
    pub h: i32,
    /// Topmost opaque row.
    pub top: i32,
    /// Rows above this shear with `sway`.
    pub shear_end: i32,
    /// Rows above this nod.
    pub neck: i32,
    /// Her brow: the hair above it trails a bend (`Pose::bend_hair`).
    pub brow: i32,
    /// Rows above this breathe.
    pub waist: i32,
    /// The grip row of a hanging pose (`swing`).
    pub grip: i32,
    /// The knee row of a hanging pose (`kick`; at or past `h`: no kick).
    pub knee: i32,
    /// The column between the legs (`kick`).
    pub split: i32,
}

/// One leg's kick over `pose_bands` (parallel to them): extra dx per band for the columns
/// on the kicking side of `split`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegKick {
    pub split: i32,
    pub right: bool,
    pub dx: Vec<i32>,
}

pub fn pose_leg(p: Pose, r: PoseRows, bands: &[(i32, i32, i32, i32)]) -> Option<LegKick> {
    if p.kick == 0 || r.knee >= r.h {
        return None;
    }
    let shin = (r.h - r.knee).max(1) as f32;
    let dx = bands.iter().map(|&(y0, _, _, _)| if y0 >= r.knee { (p.kick as f32 * (y0 - r.knee) as f32 / shin).round() as i32 } else { 0 }).collect();
    Some(LegKick { split: r.split, right: p.kick_right, dx })
}

/// A bend's curve up the torso (`Pose::bend`): the share of the neck's shift at a height
/// over the waist is that height's fraction to this power (the hips barely move; r44: softer
/// than 1.8, which put a kink under the neck at twice the bend).
const BEND_CURVE: f32 = 1.4;

/// Bands `(y0, y1, dx, dy)` in paint order (legs, torso, head) for a pose. The waist rows
/// repeat over the breath height, so nothing opens up between torso and legs.
pub fn pose_bands(p: Pose, r: PoseRows) -> Vec<(i32, i32, i32, i32)> {
    if p.swing != 0 || p.kick != 0 {
        // A pendulum from the grip: 0 at the grip row, `swing` at the bottom. Below the knee
        // the bands stay 2 px (one leg kicks band by band: `pose_leg`).
        let grip = r.grip.clamp(r.y0, r.h - 1);
        let span = (r.h - grip).max(1) as f32;
        let knee = if p.kick != 0 { r.knee.clamp(grip + 1, r.h) } else { r.h };
        let mut out = vec![(r.y0, grip, 0, 0)];
        let mut y = grip;
        while y < r.h {
            let y1 = (y + 2).min(r.h);
            let dx = (p.swing as f32 * (y - grip) as f32 / span).round() as i32;
            match out.last_mut() {
                Some(last) if last.1 == y && last.2 == dx && y < knee => last.1 = y1,
                _ => out.push((y, y1, dx, 0)),
            }
            y = y1;
        }
        return out;
    }
    let b = p.breath.max(0);
    let waist = r.waist.clamp(r.y0 + 1, r.h);
    let neck = r.neck.clamp(r.y0, waist);
    let shear_end = r.shear_end.clamp(r.y0, waist);
    let span = (shear_end - r.top).max(1) as f32;
    let torso = (waist - neck).max(1) as f32;
    let bent = p.bend != 0 || p.bend_hair != 0;
    let lag = p.bend_hair - p.bend;
    let brow = r.brow.clamp(r.y0, neck);
    let crown = (brow - r.top).max(1) as f32;
    let shear = |y: i32| {
        let f = ((shear_end - y) as f32 / span).clamp(0.0, 1.0);
        let bend = p.bend as f32 * ((waist - y) as f32 / torso).clamp(0.0, 1.0).powf(BEND_CURVE);
        let hair = lag as f32 * ((brow - y) as f32 / crown).clamp(0.0, 1.0);
        (p.sway as f32 * f.powf(1.2) + bend + hair).round() as i32
    };
    // Legs first, then the torso, then the head on top (a nod overlaps the collar).
    let mut out = vec![(waist - b, r.h, 0, 0)];
    for (from, to, dy) in [(neck, waist, -b), (r.y0, neck, p.nod - b)] {
        let first = out.len();
        let mut y = from;
        while y < to {
            let (y1, dx) = if bent && y >= neck {
                // The torso curves, a row a band (r44: 2-row steps of 2 px serrated her coat's
                // slanted edges).
                ((y + 1).min(to), shear(y))
            } else if lag != 0 && y < brow {
                // The hair above her brow trails, 2 rows a band.
                ((y + 2).min(brow).min(to), shear(y))
            } else if p.sway != 0 && y < shear_end {
                ((y + 2).min(shear_end).min(to), shear(y))
            } else {
                // The head (bent: all of it at the neck's shift).
                (to, shear(y))
            };
            let n = out.len();
            match out.last_mut() {
                Some(last) if n > first && last.1 == y && last.2 == dx && last.3 == dy => last.1 = y1,
                _ => out.push((y, y1, dx, dy)),
            }
            y = y1;
        }
    }
    out
}

/// Bands (src y0, y1, dx, dy) → the same bands over the image scaled by (sx, sy) about
/// `pivot`. Chain bands tile in destination rows (y0 + dy), so those are what get scaled
/// (rounded once each, shared by neighbours: still tiling); the source window starts at the
/// scaled y0.
pub fn scale_bands(b: &[(i32, i32, i32, i32)], pivot: (f32, f32), sx: f32, sy: f32) -> Vec<(i32, i32, i32, i32)> {
    let row = |y: i32| (pivot.1 + (y as f32 - pivot.1) * sy).round() as i32;
    b.iter()
        .map(|&(y0, y1, dx, dy)| {
            let (d0, d1) = (row(y0 + dy), row(y1 + dy));
            let a = row(y0);
            (a, a + (d1 - d0).max(0), (dx as f32 * sx).round() as i32, d0 - a)
        })
        .collect()
}

/// One floating note, in the music frame's canvas px.
#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub sprite: usize,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: f32,
    pub deg: f32,
}

/// The most notes the view draws (`NOTES_MAX`, view.rs): past it the oldest go, not the newest.
const NOTES_DRAWN: usize = 7;
/// The note size the view decodes its sprites for, canvas px (`view::note_px`): never bigger.
const NOTE_PX: f32 = 120.0;
/// A note's life, seconds, each note within ±`NOTE_LIFE_VAR` of it: about five alive at once at
/// 110 bpm, two at 40. Past ~120 bpm that many beats would hold more than `NOTES_DRAWN`, so each
/// life is cut to under `NOTES_DRAWN` beats (`music_notes`): faster notes, never more than the cap.
const NOTE_LIFE: f32 = 2.9;
const NOTE_LIFE_VAR: f32 = 0.15;
/// How much of a beat the shortest cut life leaves between the oldest note's end and the next
/// birth: the oldest has faded out before a newer one would push it off the list.
const NOTE_BEAT_SPARE: f64 = 0.25;
/// Each note's own rolls (`hash01` salts on its birth beat), one per property, so no two of them
/// move together.
const ROLL_LIFE: u32 = 1;
const ROLL_OUT: u32 = 2;
const ROLL_UP: u32 = 3;
const ROLL_SIZE: u32 = 4;
const ROLL_TILT: u32 = 5;
const ROLL_PEEL: u32 = 6;
const ROLL_RISE: u32 = 7;
/// How far out and how far up a note drifts over its life, canvas px (each note somewhere in these
/// ranges): from beside her headphones to well clear of her head, out to the side and above.
const NOTE_OUT: (f32, f32) = (120.0, 230.0);
const NOTE_UP: (f32, f32) = (300.0, 430.0);
/// Its full size, canvas px; it pops in at `NOTE_POP` of that and is grown `NOTE_GROW` into its life.
const NOTE_SIZE: (f32, f32) = (70.0, 92.0);
const NOTE_POP: f32 = 0.3;
const NOTE_GROW: f32 = 0.12;
/// It fades in over the first `NOTE_FADE_IN` of its life and out, evenly, over the last
/// `NOTE_FADE_OUT` (the view steps alpha in four, then pops it under a floor: no long faint tail).
const NOTE_FADE_IN: f32 = 0.07;
const NOTE_FADE_OUT: f32 = 0.32;
/// Its own tilt (± degrees), and the one slow lean into its travel that swells to `NOTE_LEAN` at
/// mid-life and relaxes (the view snaps to `NOTE_ANGLES`, 6° apart: this steps through two or three).
const NOTE_TILT: f32 = 4.5;
const NOTE_LEAN: f32 = 13.0;

/// The notes alive `age_s` seconds into a music session at one beat per `beat_s` seconds,
/// launched from `emitters` (the music frame's canvas px), oldest first.
///
/// One note is born on every beat (the head-nod's grid: beat `k` at `k * beat_s`), from the
/// emitters in turn, so with Capri's two they alternate left, right, left. Each note is a pure
/// function of its birth beat and its age, so this rebuilds the field from scratch on every call
/// and only visits the births that can still be alive. A tempo change moves the grid, and the
/// notes alive then jump once.
pub fn music_notes(age_s: f32, beat_s: f32, emitters: &[[i32; 2]]) -> Vec<Note> {
    if emitters.is_empty() || !age_s.is_finite() || !beat_s.is_finite() || beat_s <= 0.0 || age_s < 0.0 {
        return Vec::new();
    }
    // f64: hours in, `age_s` is a big f32, and each note's age is a small difference of two.
    let (age, beat) = (f64::from(age_s), f64::from(beat_s));
    let mid = emitters.iter().map(|e| f64::from(e[0])).sum::<f64>() / emitters.len() as f64;
    // No life reaches `NOTES_DRAWN` beats: at most that many births are ever alive at once, so
    // the cap below never drops a note still on screen (at a fast tempo, a fully lit note would
    // pop off the list). Also bounds this loop for any positive `beat`.
    let cap = (NOTES_DRAWN as f64 - NOTE_BEAT_SPARE) * beat;
    let longest = f64::from(NOTE_LIFE * (1.0 + NOTE_LIFE_VAR)).min(cap);
    // Newest first, so the cap drops the oldest; then turned around.
    let mut out = Vec::with_capacity(NOTES_DRAWN);
    let mut k = (age / beat).floor() as u64;
    loop {
        let t = age - k as f64 * beat;
        if t > longest || out.len() == NOTES_DRAWN {
            break;
        }
        out.extend(note(k, t, emitters, mid, cap));
        match k.checked_sub(1) {
            Some(j) => k = j,
            None => break,
        }
    }
    out.reverse();
    out
}

/// Note `k` (born on beat `k`) `t` seconds into its life of at most `cap` seconds, or `None`
/// once it has gone.
fn note(k: u64, t: f64, emitters: &[[i32; 2]], mid: f64, cap: f64) -> Option<Note> {
    let id = k as u32;
    let roll = |salt: u32| hash01(id, salt);
    let pick = |(lo, hi): (f32, f32), salt: u32| lo + (hi - lo) * roll(salt);
    let life = f64::from(NOTE_LIFE * (1.0 + NOTE_LIFE_VAR * (2.0 * roll(ROLL_LIFE) - 1.0))).min(cap);
    if t > life {
        return None;
    }
    // A hair under zero when the beat's multiple rounds past `age`: born just now.
    let s = (t / life).max(0.0) as f32;
    let e = emitters[(k % emitters.len() as u64) as usize];
    // Outward: away from the other emitters. One alone (or in line with the rest): every other
    // note goes left.
    let off = f64::from(e[0]) - mid;
    let side = if off.abs() > 0.5 {
        off.signum() as f32
    } else if k.is_multiple_of(2) {
        -1.0
    } else {
        1.0
    };
    // It peels off sideways first (eased out), then the rise takes over (eased in), so the path
    // bends from the side to straight up. Never down: `up` only grows.
    let out = pick(NOTE_OUT, ROLL_OUT) * (1.0 - (1.0 - s).powf(1.7 + 0.8 * roll(ROLL_PEEL)));
    let up = pick(NOTE_UP, ROLL_UP) * (0.2 * s + 0.8 * s.powf(1.45 + 0.5 * roll(ROLL_RISE)));
    let grow = (s / NOTE_GROW).min(1.0);
    let size = (pick(NOTE_SIZE, ROLL_SIZE) * (NOTE_POP + (1.0 - NOTE_POP) * (1.0 - (1.0 - grow).powi(3)))).min(NOTE_PX);
    let alpha = (s / NOTE_FADE_IN).min((1.0 - s) / NOTE_FADE_OUT).clamp(0.0, 1.0);
    // Positive is clockwise: a note going right tips its top right, one going left tips it left.
    let edge = NOTE_ANGLES[NOTE_ANGLES.len() - 1];
    let deg = (NOTE_TILT * (2.0 * roll(ROLL_TILT) - 1.0) + side * NOTE_LEAN * (PI * s).sin()).clamp(-edge, edge);
    // The sprites cycle, with a skip every fourth birth: neighbours always differ, and each side
    // shows every sprite in turn instead of the same two.
    Some(Note { sprite: k.wrapping_add(k / 4) as usize, x: e[0] as f32 + side * out, y: e[1] as f32 - up, size, alpha, deg })
}

/// Breathing bob, device px upward: 0..=px on a cosine.
pub fn bob(elapsed: Duration, px: i32, period_ms: u64) -> i32 {
    if px <= 0 || period_ms == 0 {
        return 0;
    }
    let ph = ms(elapsed) / period_ms as f32 * 2.0 * PI;
    (px as f32 * (0.5 - 0.5 * ph.cos())).round() as i32
}

/// The attention hop: up and down once, device px upward.
pub const HOP_MS: f32 = 440.0;
/// Where in it the big beat lands (the little one after it is a rebound).
pub const HOP_LAND_AT: f32 = 0.68;
pub fn hop(age: Duration, height: f32) -> Option<i32> {
    let t = ms(age);
    if t >= HOP_MS {
        return None;
    }
    let u = t / HOP_MS;
    // Two beats: a full hop, then a little one.
    let y = if u < HOP_LAND_AT {
        let v = u / HOP_LAND_AT;
        4.0 * v * (1.0 - v)
    } else {
        let v = (u - HOP_LAND_AT) / (1.0 - HOP_LAND_AT);
        0.3 * 4.0 * v * (1.0 - v)
    };
    Some((y * height).round() as i32)
}

/// A glint on the hair: grows and shrinks once.
#[derive(Clone, Copy, Debug)]
pub struct Glint {
    pub t0: Instant,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub spin: f32,
    /// Travel over its life (device px), centred on (x, y).
    pub drift: (f32, f32),
    /// Its life has started on screen (it swells from nothing there, never pops in grown).
    pub live: bool,
}

/// Samples per glint life: ticks land at the centres of `GLINT_STEPS` equal slices, plus one
/// at its end that takes it away: it swells over 4 frames and fades over 4 (sin(pi u):
/// 0.22, 0.62, 0.90, 1.0, 0.90, 0.62, 0.22, gone).
pub const GLINT_STEPS: u32 = 7;

impl Glint {
    /// Life fraction 0..1, or None before it starts or once finished.
    fn u(&self, now: Instant, life_ms: u64) -> Option<f32> {
        if now < self.t0 {
            return Some(0.0);
        }
        let u = ms(now.saturating_duration_since(self.t0)) / life_ms as f32;
        (u < 1.0).then_some(u)
    }

    /// (scale 0..1) or None when finished: one eased curve drives size, halo and colour.
    pub fn scale(&self, now: Instant, life_ms: u64) -> Option<f32> {
        self.u(now, life_ms).map(|u| (PI * u).sin())
    }

    /// Where it is now, relative to (x, y): it drifts along its slant.
    pub fn offset(&self, now: Instant, life_ms: u64) -> (f32, f32) {
        let u = self.u(now, life_ms).unwrap_or(1.0) - 0.5;
        (self.drift.0 * u, self.drift.1 * u)
    }

    /// Every point a full-grown star reaches over its life (its arm tips at both ends of its
    /// drift, and the drift's ends): all of them on the hair, or it reads as a stray speck.
    pub fn reach(&self) -> [(f32, f32); 10] {
        let mut out = [(self.x, self.y); 10];
        let (hx, hy) = (self.drift.0 / 2.0, self.drift.1 / 2.0);
        out[0] = (self.x - hx, self.y - hy);
        out[1] = (self.x + hx, self.y + hy);
        for k in 0..4 {
            let a = self.spin + k as f32 * PI / 2.0;
            let (dx, dy) = (a.cos() * self.size, a.sin() * self.size);
            out[2 + 2 * k] = (self.x - hx + dx, self.y - hy + dy);
            out[3 + 2 * k] = (self.x + hx + dx, self.y + hy + dy);
        }
        out
    }

    /// Its next sample instant after `now` (slice centres, then its end), if it isn't done.
    pub fn next_sample(&self, now: Instant, life_ms: u64) -> Option<Instant> {
        let life = Duration::from_millis(life_ms);
        let step = life / GLINT_STEPS;
        (0..GLINT_STEPS).map(|k| self.t0 + step * k + step / 2).chain([self.t0 + life]).find(|&t| t > now)
    }
}

/// A glint's outline: a solid core `core` px out from its centre each way, and four arms `r`
/// long, `base` px half-wide where they leave the core, tapering to a point.
pub fn glint_star(cx: f32, cy: f32, r: f32, core: f32, base: f32, spin: f32) -> Vec<(f32, f32)> {
    let r = r.max(core + 0.5);
    let mut pts = Vec::with_capacity(16);
    for k in 0..4 {
        let a = spin + k as f32 * PI / 2.0;
        let (ux, uy) = (a.cos(), a.sin());
        let (nx, ny) = (-uy, ux);
        pts.push((cx + ux * core - nx * base, cy + uy * core - ny * base));
        pts.push((cx + ux * r, cy + uy * r));
        pts.push((cx + ux * core + nx * base, cy + uy * core + ny * base));
        let b = a + PI / 4.0;
        pts.push((cx + b.cos() * core * std::f32::consts::SQRT_2, cy + b.sin() * core * std::f32::consts::SQRT_2));
    }
    pts
}

// ---- teleport bolt ------------------------------------------------------------------

pub const BOLT_MS: f32 = 420.0;
/// The body lands (window moves, still hidden) when the tip reaches the target…
pub const BOLT_ARRIVE: f32 = 0.4;
/// …the landing glitch starts while the trail is still visible…
pub const BOLT_LAND: f32 = 0.5;
/// …and the trail drains into her for a few more frames (no empty frame in between).
pub const BOLT_END: f32 = 0.66;
/// The travelling line: one flat layer in the bar's bolt red. Plain red only (LOCKED #1:
/// no glow, no core, no outline, no colour split; legibility may change length/thickness).
pub const BOLT_LINE: u32 = 0xff1738;

/// The strike's clock, fractions of `BOLT_MS` (`Bolt::span`). The tip leaves with a small head
/// start (lit on the very first frame), shoots out and eases into the target at `HEAD_FLIGHT`:
/// four 30 fps frames in flight, home well before `BOLT_ARRIVE`.
const HEAD_START: f32 = 0.06;
const HEAD_FLIGHT: f32 = 0.3;
const HEAD_EASE: f32 = 2.2;
/// The tail sets off just after the tip, slowly at first (a third of the strike is still lit at
/// `BOLT_LAND`), then gathers speed and drains into her by `TAIL_HOME`, just before `BOLT_END`:
/// a speck on the last frame, nothing left for the unmap to cut off.
const TAIL_START: f32 = 0.05;
const TAIL_HOME: f32 = 0.64;
const TAIL_EASE: f32 = 1.8;

/// The ribbon's half-width where it's thickest: `RIB` of the figure's size, within
/// `[RIB_MIN, RIB_MAX]` px (readable on a small figure, no slab on a big one), and at most
/// `RIB_OF_LEN` of a short hop's length.
const RIB: f32 = 0.032;
const RIB_MIN: f32 = 3.0;
const RIB_MAX: f32 = 11.0;
const RIB_OF_LEN: f32 = 0.06;
/// How far each edge sits from the centre line at each route point, a share of the half-width,
/// the two sides rolled apart: chipped, crystalline edges rather than a tube.
const CHIP: (f32, f32) = (0.35, 1.35);
/// Main corners: about one per `CORNER_EVERY` of the figure's size along the strike, 2 to
/// `CORNERS_MAX` of them, never closer than `CORNER_GAP` half-widths.
const CORNER_EVERY: f32 = 0.7;
const CORNERS_MAX: usize = 7;
const CORNER_GAP: f32 = 6.0;
/// Their swing off the straight line at its widest (mid-strike): `SWING` of the strike's length,
/// within `[SWING_MIN, SWING_MAX]` of the figure's size (a hop still reads as jagged, a strike
/// across monitors doesn't flail), and at most `SWING_OF_LEN` of the length.
const SWING: f32 = 0.17;
const SWING_MIN: f32 = 0.08;
const SWING_MAX: f32 = 0.4;
const SWING_OF_LEN: f32 = 0.35;
/// No leg between main corners leans more than 45° off the strike's line, so a main corner turns
/// at most 90°. The jags laid on the legs afterwards kink them further (turns to about 120°), so
/// the outline's own guards are `unfold` and `untangled`, not this.
const LEG_SLOPE: f32 = 1.0;
/// The small jags on the legs between corners: a kink of this share of the leg, at most `JAG_MAX`
/// of the figure's size.
const JAG: (f32, f32) = (0.07, 0.13);
const JAG_MAX: f32 = 0.12;
/// The route is resampled every `ROUTE_STEP` half-widths (or a `ROUTE_PIECES`th of a long strike),
/// so the edges chip along straight legs too, in tens of points.
const ROUTE_STEP: f32 = 2.6;
const ROUTE_PIECES: f32 = 40.0;
/// The width along the lit piece: thickest `LEAD` of the way back from the tip (at most
/// `LEAD_MAX` of the figure's size), almost a point at the tip (`TIP_W`) and at the tail (`TAIL_W`).
const LEAD: f32 = 0.25;
const LEAD_MAX: f32 = 0.35;
const TIP_W: f32 = 0.12;
const TAIL_W: f32 = 0.08;
/// A lit piece shorter than `THIN_BELOW` half-widths is thinner, down to `THIN_MIN` (the launch's
/// first spark, the end of the drain).
const THIN_BELOW: f32 = 8.0;
const THIN_MIN: f32 = 0.3;
/// The streak's half-width the box leaves room for, of the figure's size (the view's is far less).
const STREAK_ROOM: f32 = 0.05;
/// The figure size, px, a `scale` that isn't a number stands for (the view's at sf 1).
const SCALE_FALLBACK: f32 = 220.0;
/// Shorter than this (px), a strike draws nothing.
const BOLT_MIN_LEN: f32 = 1.0;
/// Coordinates past this aren't on any desktop: such a strike draws nothing.
const ROOT_MAX: f32 = 1.0e6;
/// The box's largest side, px (shape rects are `i16` from its origin): a strike that needs more
/// draws nothing.
const BOX_MAX: i64 = 32_000;

/// One point of a strike's route.
#[derive(Clone, Copy, Debug)]
struct Knot {
    p: (f32, f32),
    /// Px along the straight line from `a` (never decreasing along the route).
    along: f32,
    /// The left and right edges' share of the half-width here.
    chip: (f32, f32),
}

/// A cross-section of the lit ribbon: a centre-line point and the half-width either side.
#[derive(Clone, Copy, Debug)]
struct Rib {
    p: (f32, f32),
    left: f32,
    right: f32,
}

/// A jagged strike from `a` to `b` (root px): a plain red ribbon whose tip races from the
/// origin to the target with the trail behind it. The route is laid when the strike is made
/// (corners alternating sides, uneven swings, small jags between); each frame lights the stretch
/// of it between `span`'s tail and head.
pub struct Bolt {
    a: (f32, f32),
    /// Unit vector a → b (straight down when there's no length).
    dir: (f32, f32),
    len: f32,
    /// `a` to `b`, exact at both ends, monotone along `dir`; empty when the strike draws nothing.
    route: Vec<Knot>,
    /// The ribbon's half-width at its thickest, px.
    half: f32,
    /// The widest half-width a streak may take and still fit the box, px.
    room: f32,
    /// The longest the tip's taper gets, px.
    lead_max: f32,
    bbox: (i32, i32, i32, i32),
}

impl Bolt {
    /// `scale`: figure size in px (the bolt's thickness follows it).
    pub fn new(a: (f32, f32), b: (f32, f32), seed: u64, scale: f32) -> Bolt {
        let scale = if scale.is_finite() { scale.clamp(20.0, 2000.0) } else { SCALE_FALLBACK };
        let sane = [a.0, a.1, b.0, b.1].iter().all(|v| v.abs() <= ROOT_MAX);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = if sane { dx.hypot(dy) } else { 0.0 };
        let dir = if len > 1e-3 { (dx / len, dy / len) } else { (0.0, 1.0) };
        let half = (RIB * scale).clamp(RIB_MIN, RIB_MAX).min((RIB_OF_LEN * len).max(1.5));
        let mut bolt =
            Bolt { a, dir, len, route: Vec::new(), half, room: STREAK_ROOM * scale, lead_max: LEAD_MAX * scale, bbox: (0, 0, 1, 1) };
        if !sane {
            return bolt;
        }
        let mut rng = Rng::new(stir(seed));
        let flip = |rng: &mut Rng| if rng.pick(2) == 0 { 1.0 } else { -1.0 };

        // The main corners as (along, off): px along the straight line, px off it (+ = left of
        // travel). Evenly spread with a jitter, sides alternating from a random first one.
        let gap = CORNER_GAP * half;
        let mut knots = vec![(0.0f32, 0.0f32)];
        if len >= 2.0 * gap {
            let n = ((len / (CORNER_EVERY * scale)).round() as usize + 1).clamp(2, CORNERS_MAX).min(((len / gap) as usize).saturating_sub(1));
            let swing = (SWING * len).clamp(SWING_MIN * scale, SWING_MAX * scale).min(SWING_OF_LEN * len);
            let mut side = flip(&mut rng);
            let (mut at, mut last) = (0.0f32, None::<f32>);
            for i in 1..=n {
                let t = ((i as f32 + rng.range(-0.38, 0.38)) / (n + 1) as f32 * len).max(at + gap);
                if t > len - gap {
                    break;
                }
                at = t;
                // Uneven: a big swing never sits next to one of about its size.
                let mut r = rng.range(0.15, 1.0);
                if last.is_some_and(|q| (r - q).abs() < 0.3) {
                    r = if r < 0.55 { r + 0.45 } else { r - 0.45 };
                }
                last = Some(r);
                // Gentlest near both ends (it leaves and lands almost head-on), widest mid-strike.
                let env = (PI * t / len).sin().max(0.0).powf(0.8);
                knots.push((t, side * swing * env * r));
                side = -side;
            }
        }
        knots.push((len, 0.0));
        // Legs at most 45° off the line: shrink the corners a too-steep leg joins (a few passes
        // settle it; the ends stay on the line).
        for _ in 0..4 {
            for i in 0..knots.len() - 1 {
                let ((x0, o0), (x1, o1)) = (knots[i], knots[i + 1]);
                let (rise, room) = ((o1 - o0).abs(), LEG_SLOPE * (x1 - x0));
                if rise > room {
                    let k = room / rise;
                    if i > 0 {
                        knots[i].1 = o0 * k;
                    }
                    if i + 2 < knots.len() {
                        knots[i + 1].1 = o1 * k;
                    }
                }
            }
        }
        let nrm = (-dir.1, dir.0);
        let mut main: Vec<(f32, f32)> =
            knots.iter().map(|&(t, o)| (a.0 + dir.0 * t + nrm.0 * o, a.1 + dir.1 * t + nrm.1 * o)).collect();
        main[0] = a;
        if let Some(p) = main.last_mut() {
            *p = b;
        }

        // Small jags on the legs: one kink on a mid-size leg, two on a long one.
        let mut path = vec![a];
        for leg in main.windows(2) {
            let (p, q) = (leg[0], leg[1]);
            let (ex, ey) = (q.0 - p.0, q.1 - p.1);
            let long = ex.hypot(ey);
            let cuts: &[(f32, f32)] = if long >= 7.0 * gap {
                &[(0.2, 0.36), (0.64, 0.8)]
            } else if long >= 2.5 * gap {
                &[(0.35, 0.65)]
            } else {
                &[]
            };
            for &(lo, hi) in cuts {
                let f = rng.range(lo, hi);
                let o = (rng.range(JAG.0, JAG.1) * long).min(JAG_MAX * scale) * flip(&mut rng);
                path.push((p.0 + ex * f - ey / long * o, p.1 + ey * f + ex / long * o));
            }
            path.push(q);
        }

        // Resampled, each point with its own chip either side.
        let step = (ROUTE_STEP * half).max(len / ROUTE_PIECES);
        let mut pts = vec![a];
        for seg in path.windows(2) {
            let (p, q) = (seg[0], seg[1]);
            let m = ((q.0 - p.0).hypot(q.1 - p.1) / step).ceil().max(1.0) as usize;
            for j in 1..m {
                let f = j as f32 / m as f32;
                pts.push((p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f));
            }
            pts.push(q);
        }
        let last = pts.len() - 1;
        let mut prev = 0.0f32;
        bolt.route = pts
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let along = if i == last { len } else { ((p.0 - a.0) * dir.0 + (p.1 - a.1) * dir.1).clamp(prev, len) };
                prev = along;
                Knot { p, along, chip: (rng.range(CHIP.0, CHIP.1), rng.range(CHIP.0, CHIP.1)) }
            })
            .collect();

        // The box: the route's, padded by the most an edge can stand off it (the widest chip at
        // a miter's longest) or the streak's room.
        let pad = (2.0 * CHIP.1 * half).max(bolt.room) + 2.0;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for k in &bolt.route {
            (x0, y0, x1, y1) = (x0.min(k.p.0), y0.min(k.p.1), x1.max(k.p.0), y1.max(k.p.1));
        }
        let (bx, by) = ((x0 - pad).floor() as i64, (y0 - pad).floor() as i64);
        let (bw, bh) = ((x1 + pad).ceil() as i64 - bx + 1, (y1 + pad).ceil() as i64 - by + 1);
        if bw > BOX_MAX || bh > BOX_MAX {
            bolt.route.clear();
            return bolt;
        }
        bolt.bbox = (bx as i32, by as i32, bw as i32, bh as i32);
        bolt
    }

    /// Root-px box `(x, y, w, h)` holding every polygon `frame` and `streak` can return.
    pub fn bbox(&self) -> (i32, i32, i32, i32) {
        self.bbox
    }

    /// Head and tail progress along the strike (each 0..1) at time `u` (0..1 of `BOLT_MS`):
    /// fractions of the straight line from `a` to `b`, for `frame` and `streak` alike. The head
    /// shoots out and settles into the target; the tail follows, slow then fast, and is home just
    /// before `BOLT_END`. From `BOLT_END` on, both are home (nothing lit).
    pub fn span(u: f32) -> (f32, f32) {
        if u.is_nan() || u >= BOLT_END {
            return (1.0, 1.0);
        }
        let u = u.max(0.0);
        let s = (u / HEAD_FLIGHT).min(1.0);
        let head = 1.0 - (1.0 - HEAD_START) * (1.0 - s).powf(HEAD_EASE);
        let s = ((u - TAIL_START) / (TAIL_HOME - TAIL_START)).clamp(0.0, 1.0);
        (head, s.powf(TAIL_EASE).min(head))
    }

    /// The quiet (passive) strike at time `u`: one thin straight shard, `len` px long and `w` px
    /// wide. Root px, empty before/after.
    pub fn streak(&self, u: f32, len: f32, w: f32) -> Vec<(f32, f32)> {
        if self.route.len() < 2 || self.len < BOLT_MIN_LEN || !len.is_finite() || !w.is_finite() {
            return Vec::new();
        }
        let (head, tail) = Bolt::span(u);
        let long = ((head - tail) * self.len).min(len);
        let half = (0.5 * w).min(self.room).min(0.2 * long);
        if long < 0.5 || half <= 0.0 {
            return Vec::new();
        }
        // On the straight line: a point at the head, widest a little behind it, a point at the back.
        let d = self.dir;
        let tip = (self.a.0 + d.0 * head * self.len, self.a.1 + d.1 * head * self.len);
        let at = |back: f32, off: f32| (tip.0 - d.0 * back - d.1 * off, tip.1 - d.1 * back + d.0 * off);
        vec![tip, at(0.22 * long, half), at(long, 0.0), at(0.22 * long, -half)]
    }

    /// The bolt at time `u` (0..1 of `BOLT_MS`) as one simple polygon, root px (empty
    /// before/after the strike): the lit piece's `outline`, never crossing itself (`untangled`).
    pub fn frame(&self, u: f32) -> Vec<(f32, f32)> {
        untangled(self.ribs(u), self.dir)
    }

    /// The lit piece at `u` as cross-sections, tail (a point) to tip (a point): the route's own
    /// points in between, plus the shoulder where it's thickest.
    fn ribs(&self, u: f32) -> Vec<Rib> {
        let (head, tail) = Bolt::span(u);
        let (h, t) = (head * self.len, tail * self.len);
        let piece = h - t;
        if self.route.len() < 2 || self.len < BOLT_MIN_LEN || piece < 0.5 {
            return Vec::new();
        }
        let lead = (LEAD * piece).min(self.lead_max);
        let peak = self.half * (piece / (THIN_BELOW * self.half)).clamp(THIN_MIN, 1.0);
        let mut at: Vec<f32> = self.route.iter().map(|k| k.along).filter(|&s| s > t + 0.5 && s < h - 0.5).collect();
        let shoulder = h - lead;
        if shoulder > t + 0.5 && shoulder < h - 0.5 {
            at.insert(at.partition_point(|&s| s < shoulder), shoulder);
        }
        let mut ribs = Vec::with_capacity(at.len() + 2);
        ribs.push(Rib { p: self.point(t).0, left: 0.0, right: 0.0 });
        for s in at {
            let (p, chip) = self.point(s);
            // Rising fast from the tip to the shoulder, then thinning steadily back to the tail.
            let back = h - s;
            let w = if back < lead {
                TIP_W + (1.0 - TIP_W) * (back / lead).powf(0.7)
            } else {
                TAIL_W + (1.0 - TAIL_W) * (1.0 - (back - lead) / (piece - lead).max(1e-3)).clamp(0.0, 1.0).powf(0.6)
            };
            ribs.push(Rib { p, left: peak * w * chip.0, right: peak * w * chip.1 });
        }
        ribs.push(Rib { p: self.point(h).0, left: 0.0, right: 0.0 });
        unfold(&mut ribs);
        ribs
    }

    /// The route's point `s` px along the strike, and the chip either side there.
    fn point(&self, s: f32) -> ((f32, f32), (f32, f32)) {
        let r = &self.route;
        let i = r.partition_point(|k| k.along <= s).clamp(1, r.len() - 1);
        let (k0, k1) = (r[i - 1], r[i]);
        let f = ((s - k0.along) / (k1.along - k0.along).max(1e-6)).clamp(0.0, 1.0);
        let mix = |x: f32, y: f32| if f >= 1.0 { y } else { x + (y - x) * f };
        ((mix(k0.p.0, k1.p.0), mix(k0.p.1, k1.p.1)), (mix(k0.chip.0, k1.chip.0), mix(k0.chip.1, k1.chip.1)))
    }
}

/// Every bit of `seed` into every bit of the result (a splitmix64 finalizer): `Rng::new` drops
/// bit 0, so two seeds one apart would otherwise lay the same strike.
fn stir(seed: u64) -> u64 {
    let mut x = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// `v` at length 1, or `or` when it has (next to) none.
fn unit(v: (f32, f32), or: (f32, f32)) -> (f32, f32) {
    let l = v.0.hypot(v.1);
    if l > 1e-6 { (v.0 / l, v.1 / l) } else { or }
}

/// How sharply the centre line turns at `b` (radians, 0 = straight on).
fn bend(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
    (u.0 * v.1 - u.1 * v.0).abs().atan2(u.0 * v.0 + u.1 * v.1)
}

/// How sharply the ribbon turns at cross-section `i` (0 at either end).
fn turn(r: &[Rib], i: usize) -> f32 {
    if i == 0 || i + 1 >= r.len() { 0.0 } else { bend(r[i - 1].p, r[i].p, r[i + 1].p) }
}

/// The lit piece's outline: its left edge from the tail's point to the tip's, then its right
/// edge back, mitred at every bend (a miter at most 2× the width, which keeps it in the box).
/// Empty under three cross-sections.
fn outline(ribs: &[Rib], dir: (f32, f32)) -> Vec<(f32, f32)> {
    if ribs.len() < 3 {
        return Vec::new();
    }
    let across = |p: (f32, f32), q: (f32, f32)| {
        let e = unit((q.0 - p.0, q.1 - p.1), dir);
        (-e.1, e.0)
    };
    let (mut left, mut right) = (Vec::with_capacity(ribs.len()), Vec::with_capacity(ribs.len()));
    for w in ribs.windows(3) {
        let (r, n0, n1) = (w[1], across(w[0].p, w[1].p), across(w[1].p, w[2].p));
        let m = unit((n0.0 + n1.0, n0.1 + n1.1), n0);
        let c = (m.0 * n0.0 + m.1 * n0.1).max(0.5);
        left.push((r.p.0 + m.0 * r.left / c, r.p.1 + m.1 * r.left / c));
        right.push((r.p.0 - m.0 * r.right / c, r.p.1 - m.1 * r.right / c));
    }
    let mut poly = Vec::with_capacity(2 * ribs.len());
    poly.push(ribs[0].p);
    poly.extend(left);
    poly.push(ribs[ribs.len() - 1].p);
    poly.extend(right.into_iter().rev());
    poly
}

/// The first two edges of the closed polygon `poly` that cross (each by the index of its first
/// point), or `None` when it's one simple loop. Edges sharing a point don't count, nor does a
/// touch.
fn first_crossing(poly: &[(f32, f32)]) -> Option<(usize, usize)> {
    let n = poly.len();
    let side = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let apart = |p: (f32, f32), q: (f32, f32), r: (f32, f32), s: (f32, f32)| {
        p.0.max(q.0) < r.0.min(s.0) || r.0.max(s.0) < p.0.min(q.0) || p.1.max(q.1) < r.1.min(s.1) || r.1.max(s.1) < p.1.min(q.1)
    };
    (0..n).find_map(|i| {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        (i + 2..n).filter(|&j| i > 0 || j + 1 < n).find_map(|j| {
            let (r, s) = (poly[j], poly[(j + 1) % n]);
            let cross = !apart(p, q, r, s) && side(r, s, p) * side(r, s, q) < 0.0 && side(p, q, r) * side(p, q, s) < 0.0;
            cross.then_some((i, j))
        })
    })
}

/// `outline`, made sure to be one simple loop (LOCKED #1: under `bolt_rects`' even-odd fill an
/// overlap is a hole in the line). `unfold` has already spaced the cross-sections so the edges
/// don't fold at a bend; should two edges cross all the same, the straightest cross-section at
/// either of them goes and the outline is drawn again. With a single one left and still
/// crossing, `kite` stands in.
fn untangled(mut ribs: Vec<Rib>, dir: (f32, f32)) -> Vec<(f32, f32)> {
    loop {
        let poly = outline(&ribs, dir);
        let Some((e, f)) = first_crossing(&poly) else { return poly };
        // `poly` is ribs[0], the left of 1..n-1, ribs[n - 1], the right of n-2 down to 1.
        let n = ribs.len();
        let rib = |j: usize| if j < n { j } else { 2 * n - 2 - j };
        let at = [e, e + 1, f, f + 1].map(|j| rib(j % poly.len()));
        let gone = at.into_iter().filter(|&i| i > 0 && i + 1 < n).min_by(|&x, &y| turn(&ribs, x).total_cmp(&turn(&ribs, y)));
        match gone {
            Some(i) if n > 3 => {
                ribs.remove(i);
            }
            _ => return kite(&ribs),
        }
    }
}

/// The lit piece as a slim kite on the straight chord from its tail's point to its tip's, as
/// wide as its widest cross-section and widest near the tip: convex, so it can't cross itself.
/// `untangled`'s last resort (never seen in a strike).
fn kite(ribs: &[Rib]) -> Vec<(f32, f32)> {
    let (Some(t), Some(h)) = (ribs.first(), ribs.last()) else { return Vec::new() };
    let (dx, dy) = (h.p.0 - t.p.0, h.p.1 - t.p.1);
    let long = dx.hypot(dy);
    if !long.is_finite() || long < 0.5 {
        return Vec::new();
    }
    let half = ribs.iter().fold(0.0f32, |w, r| w.max(r.left).max(r.right)).min(0.2 * long).max(0.25);
    let (nx, ny) = (-dy / long * half, dx / long * half);
    let (mx, my) = (t.p.0 + 0.7 * dx, t.p.1 + 0.7 * dy);
    vec![t.p, (mx + nx, my + ny), h.p, (mx - nx, my - ny)]
}

/// Thins out cross-sections until every stretch between two is longer than both ends' inner
/// miters reach back along it, each miter sized by the widest cross-section at its bend (the
/// chips let a neighbour be ~4× the bend's own width, and a wide neighbour's inner corner pokes
/// back past a thin bend's miter). So the edges don't fold at a bend and the jagged shape stays
/// as laid; `untangled` only has freaks left to catch. The straighter of the two goes, never an
/// end; real corners stay.
fn unfold(ribs: &mut Vec<Rib>) {
    // The inner miter point, projected back onto a leg (`outline` caps the miter at 2×).
    fn reach(r: &[Rib], i: usize) -> f32 {
        let (s, c) = (0.5 * turn(r, i)).sin_cos();
        let w = r[i.saturating_sub(1)..r.len().min(i + 2)].iter().fold(0.0f32, |w, q| w.max(q.left).max(q.right));
        w * s / c.max(0.5)
    }
    fn pinch(r: &[Rib]) -> Option<usize> {
        (0..r.len().saturating_sub(1)).find_map(|i| {
            let gap = (r[i + 1].p.0 - r[i].p.0).hypot(r[i + 1].p.1 - r[i].p.1);
            if gap >= reach(r, i) + reach(r, i + 1) + 0.5 {
                return None;
            }
            [i, i + 1].into_iter().filter(|&j| j > 0 && j + 1 < r.len()).min_by(|&x, &y| turn(r, x).total_cmp(&turn(r, y)))
        })
    }
    while let Some(j) = pinch(ribs) {
        ribs.remove(j);
    }
}

/// The bolt polygon (root px) → shape rows relative to `origin`, run edges snapped to a
/// 2 px grid with a little per-row jitter, so the outline stair-steps like Capri's bolt.
pub fn bolt_rects(poly: &[(f32, f32)], origin: (i32, i32)) -> Vec<ShapeRect> {
    const STEP: i32 = 2;
    let mut rows = Vec::new();
    let local: Vec<(f32, f32)> = poly.iter().map(|&(x, y)| (x - origin.0 as f32, y - origin.1 as f32)).collect();
    shape::poly_rows(&local, &mut rows);
    rows.into_iter()
        .filter_map(|(x, y, w, h)| {
            let jitter = (hash01(y as u32, x as u32 / STEP as u32) * STEP as f32) as i32;
            let x0 = (x as i32 + jitter / 2).div_euclid(STEP) * STEP;
            let x1 = (x as i32 + w as i32 + jitter / 2).div_euclid(STEP) * STEP;
            (x1 > x0).then_some((x0 as i16, y, (x1 - x0) as u16, h))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_glint_has_a_solid_core_and_arms_that_taper_to_points() {
        let mut rows = Vec::new();
        crate::shape::poly_rows(&super::glint_star(10.5, 10.5, 9.0, 1.5, 1.6, 0.0), &mut rows);
        let on = |x: i32, y: i32| rows.iter().any(|r| r.1 as i32 == y && (r.0 as i32..r.0 as i32 + r.2 as i32).contains(&x));
        assert!((9..12).all(|y| (9..12).all(|x| on(x, y))), "a 3x3 core");
        // The arm up: 3 px across where it leaves the core, 1 px at its point, nothing past it.
        let across = |y: i32| (0..21).filter(|&x| on(x, y)).count();
        assert_eq!((across(8), across(6), across(3), across(1)), (3, 3, 1, 0));
    }

    #[test]
    fn a_settle_dips_and_comes_to_rest_at_its_end_without_a_step() {
        let ms = std::time::Duration::from_millis;
        let sy = |t: u64| super::settle(ms(t), ms(130)).map_or(1.0, |v| v.1);
        assert!(sy(39) < 0.931, "{}", sy(39));
        // Its last frame (16 ms before the end) is within 0.1 % of rest: under a px on her.
        assert!(1.0 - sy(114) < 0.001, "{}", sy(114));
        assert!((1..130).all(|t| sy(t) >= sy(t - 1) || t <= 39), "never back down after the trough");
        assert_eq!(super::settle(ms(130), ms(130)), None);
    }

    #[test]
    fn a_swap_seam_has_one_small_step_and_leaves_the_tag_whole() {
        let t0 = std::time::Instant::now();
        let g = super::Glitch::swap(t0, 7);
        assert!(!g.tears_tag);
        let at = |ms: u64| g.strength(t0 + std::time::Duration::from_millis(ms));
        assert_eq!((at(0), at(16), at(20)), (0.6, 0.6, 0.0), "one step: the cut frame's");
        assert!(g.done(t0 + std::time::Duration::from_millis(31)));
    }

    use super::*;

    #[test]
    fn every_cut_but_a_teleport_or_summon_gets_the_light_seam() {
        let t0 = std::time::Instant::now();
        for t in Transition::ALL {
            let g = Glitch::for_cut(t, t0, 9);
            match t {
                Transition::Teleport => assert!(g.is_none(), "the teleport legs cover it"),
                Transition::Summon => assert_eq!(g.map(|g| g.kind), Some(GlitchKind::Big)),
                Transition::Bid => assert_eq!(g.map(|g| g.kind), Some(GlitchKind::Burst)),
                _ => {
                    let g = g.unwrap();
                    assert!(!g.tears_tag, "{t:?}: the tag whole");
                    assert_eq!((g.kind, g.ms), (GlitchKind::Seam, SWAP_MS), "{t:?}: the swap's seam");
                    // One light frame at most (~60 Hz), then clean.
                    let lit: Vec<f32> = (0..400).map(|ms| g.strength(t0 + std::time::Duration::from_millis(ms))).filter(|&v| v > 0.0).collect();
                    assert!(lit.len() <= 20 && lit.iter().all(|&v| v <= 0.6), "{t:?}: lit {} ms", lit.len());
                }
            }
        }
    }

    #[test]
    fn scaled_bands_tile_and_cover() {
        // Destination-tiling bands with uneven source windows (a chain mid-swing).
        let b = vec![(10, 30, 0, 0), (31, 35, 3, -1), (33, 38, 5, 1), (40, 90, 8, -1)];
        for w in b.windows(2) {
            assert_eq!(w[0].1 + w[0].3, w[1].0 + w[1].3);
        }
        for &(sx, sy) in &[(1.0, 1.12), (0.9, 0.78), (1.0, 0.08), (1.05, 1.05), (1.0, 1.0)] {
            let s = scale_bands(&b, (50.0, 89.0), sx, sy);
            for w in s.windows(2) {
                assert_eq!(w[0].1 + w[0].3, w[1].0 + w[1].3, "destinations tile at {sx},{sy}");
            }
            let last = s.last().unwrap();
            assert_eq!(last.1 + last.3, (89.0 + (89.0 - 89.0) * sy) as i32, "the pivot row stays");
        }
        assert_eq!(scale_bands(&b, (50.0, 89.0), 1.0, 1.0), b, "identity");
    }

    #[test]
    fn squash_springs_back() {
        let (_, sy) = squash(Duration::from_millis(80)).unwrap();
        assert!((sy - 0.93).abs() < 0.01);
        assert!(squash(Duration::from_millis(600)).is_none());
    }

    #[test]
    fn bob_range() {
        let v: Vec<i32> = (0..36).map(|i| bob(Duration::from_millis(i * 100), 2, 3_600)).collect();
        assert_eq!(*v.iter().min().unwrap(), 0);
        assert_eq!(*v.iter().max().unwrap(), 2);
    }

    #[test]
    fn quiet_exit_is_one_piece() {
        let t = Instant::now();
        let g = Glitch::soft(GlitchKind::Vanish, t, 7);
        assert_eq!(g.ms, 200.0);
        assert_eq!(g.squeeze_sx(t), Some(1.0));
        let mid = g.squeeze_sx(t + Duration::from_millis(100)).unwrap();
        assert!(mid > 0.4 && mid < 0.6, "{mid}");
        assert!((g.squeeze_sx(t + Duration::from_millis(200)).unwrap() - 0.06).abs() < 1e-4);
        assert_eq!(Glitch::new(GlitchKind::Vanish, t, 7).squeeze_sx(t), None);
    }

    #[test]
    fn glitch_vanish_thins_to_red() {
        let t = Instant::now();
        let g = Glitch::new(GlitchKind::Vanish, t, 7);
        let step = |at: Instant| GlitchStep {
            strength: g.strength(at),
            coverage: g.coverage(at),
            seed: g.step_seed(at.duration_since(t).as_millis() as u64 / 33),
            order: g.seed(),
            y0: 0,
            h: 400,
            body: (0, 0, 300, 400),
            face: None,
            solid: 0.0,
            tag: None,
            sf: 1.0,
            calm: false,
            light: false,
            still_from: None,
        };
        let mut plan = BodyPlan::plain();
        plan.glitch(step(t));
        let full: i32 = plan.slices.unwrap().iter().map(|b| b.1 - b.0).sum();
        let late = t + Duration::from_millis(290);
        let mut plan = BodyPlan::plain();
        plan.glitch(step(late));
        let thin: i32 = plan.slices.unwrap().iter().map(|b| b.1 - b.0).sum();
        assert!(thin * 2 < full, "{thin} vs {full}");
        assert_eq!(g.tint(late), Some(BOLT_RED));
        assert_eq!(g.tint(t), None);
    }

    #[test]
    fn glitch_keeps_the_face() {
        let t = Instant::now();
        for kind in [GlitchKind::Big, GlitchKind::Appear, GlitchKind::Burst] {
            let g = Glitch::new(kind, t, 11);
            for ms in (0..950).step_by(25) {
                let at = t + Duration::from_millis(ms);
                let mut plan = BodyPlan::plain();
                plan.glitch(GlitchStep {
                    strength: g.strength(at),
                    coverage: g.coverage(at),
                    seed: g.step_seed(at.duration_since(t).as_millis() as u64 / 33),
                    order: g.seed(),
                    y0: -80,
                    h: 400,
                    body: (20, 10, 300, 390),
                    face: Some((90, 140)),
                    solid: g.solid(at),
                    tag: Some((-40, -14)),
                    sf: 1.0,
                    calm: false,
                    light: false,
                    still_from: None,
                });
                let bands = plan.slices.clone().unwrap();
                // Every row from the HUD to the feet is in exactly one band: nothing dropped.
                for y in -80..400 {
                    assert_eq!(bands.iter().filter(|b| y >= b.0 && y < b.1).count(), 1, "{kind:?} {ms} row {y}");
                }
                // The tag (rows -40..-14) is cut inside.
                assert!(bands.iter().any(|b| b.0 > -40 && b.0 < -14), "{kind:?} {ms}: tag not torn");
                let face: Vec<usize> = (0..bands.len()).filter(|&i| bands[i].1 > 90 && bands[i].0 < 140).collect();
                assert_eq!(face.iter().map(|&i| bands[i].1 - bands[i].0).sum::<i32>(), 50, "{kind:?} {ms}");
                assert!(face.iter().all(|&i| bands[i].2.abs() <= 3 && bands[i].0 >= 90 && bands[i].1 <= 140 && plan.split[i] == 0));
                assert!(face.iter().all(|&i| plan.solid[i].is_none()));
                if g.strength(at) >= 0.8 {
                    let body = bands.iter().filter(|b| b.0 >= 10).count();
                    assert!((6..=16).contains(&body), "{kind:?} {ms}: {body} bands");
                    let shifted: Vec<i32> = bands.iter().enumerate().filter(|(i, b)| b.0 >= 10 && !face.contains(i) && b.2 != 0).map(|b| b.1.2.abs()).collect();
                    assert!(shifted.iter().all(|&d| (10..=40).contains(&d)), "{shifted:?}");
                    assert!(plan.split.iter().enumerate().filter(|(i, _)| !face.contains(i)).all(|(_, &d)| (4..=8).contains(&d)));
                }
            }
        }
    }

    #[test]
    fn glitch_builds_peaks_and_echoes() {
        let t = Instant::now();
        for (kind, aftershocks) in [(GlitchKind::Big, 2), (GlitchKind::Burst, 1)] {
            let g = Glitch::new(kind, t, 5);
            let v: Vec<f32> = (0..g.ms as u64 / 10).map(|i| g.strength(t + Duration::from_millis(i * 10))).collect();
            // Build-up below the peak, then a hard peak.
            let peak = v.iter().cloned().fold(0.0, f32::max);
            assert!(peak >= 0.8, "{kind:?} {peak}");
            let first_peak = v.iter().position(|&x| x >= peak * 0.95).unwrap();
            assert!(first_peak >= 6 && v[..first_peak].iter().all(|&x| x > 0.15 && x < peak * 0.7), "{kind:?} build-up {:?}", &v[..first_peak]);
            // Main hit (build-up + peak) 0.4–0.6 s for the big one.
            let hit = v.iter().take_while(|&&x| x > 0.05).count();
            if kind == GlitchKind::Big {
                assert!((40..=60).contains(&hit), "{hit}");
            }
            // Then `aftershocks` echoes, each smaller than the peak, with clean gaps.
            let mut echoes = Vec::new();
            let mut i = hit;
            while i < v.len() {
                if v[i] > 0.05 {
                    let j = (i..v.len()).find(|&j| v[j] <= 0.05).unwrap_or(v.len());
                    echoes.push(v[i..j].iter().cloned().fold(0.0, f32::max));
                    i = j;
                } else {
                    i += 1;
                }
            }
            assert_eq!(echoes.len(), aftershocks, "{kind:?} {echoes:?}");
            assert!(echoes.iter().all(|&e| e < peak * 0.75), "{echoes:?}");
            assert!(echoes.windows(2).all(|w| w[1] < w[0]), "{echoes:?}");
        }
    }

    #[test]
    fn a_perch_landing_never_tears_her_legs_across_the_edge() {
        let t = Instant::now();
        let g = Glitch::new(GlitchKind::Land, t, 5);
        for f in 0..10u64 {
            let at = t + Duration::from_millis(f * 33);
            let mut plan = BodyPlan::plain();
            plan.glitch(GlitchStep {
                strength: g.strength(at),
                coverage: g.coverage(at),
                seed: g.step_seed(f),
                order: g.seed(),
                y0: -40,
                h: 300,
                body: (50, 0, 200, 300),
                face: Some((40, 70)),
                solid: 0.3,
                tag: None,
                sf: 1.0,
                calm: true,
                light: false,
                still_from: Some(200),
            });
            let bands = plan.slices.clone().unwrap();
            for (i, &(_, y1, dx, _)) in bands.iter().enumerate() {
                if y1 > 200 {
                    assert_eq!((dx, plan.split[i], plan.solid[i]), (0, 0, None), "frame {f}: a band at the edge moved");
                }
            }
        }
    }

    #[test]
    fn glitch_pattern_changes_every_frame_and_bars_stay_inside() {
        let t = Instant::now();
        let g = Glitch::new(GlitchKind::Big, t, 9);
        // A 200 x 300 body: rows 0..300, x 50..250.
        let body: Vec<ShapeRect> = (0..300).map(|y| (50, y as i16, 200, 1)).collect();
        let mut prev: Option<Vec<(i32, i32, i32, i32)>> = None;
        for f in 0..20u64 {
            let at = t + Duration::from_millis(f * 34);
            let mut plan = BodyPlan::plain();
            plan.glitch(GlitchStep {
                strength: g.strength(at),
                coverage: 1.0,
                seed: g.step_seed(at.duration_since(t).as_millis() as u64 / 33),
                order: g.seed(),
                y0: -40,
                h: 300,
                body: (50, 0, 200, 300),
                face: Some((40, 70)),
                solid: 0.0,
                tag: None,
                sf: 1.0,
                calm: false,
                light: false,
                still_from: None,
            });
            let bands = plan.slices.clone().unwrap();
            if g.strength(at) > 0.05 {
                if let Some(p) = &prev {
                    assert_ne!(p, &bands, "frame {f}: same pattern as the last");
                }
                prev = Some(bands);
            }
            let drawn = plan.body_rects(&body);
            for (r, _) in plan.bar_rects(&body) {
                // Every bar pixel row-run lies inside some drawn body run.
                assert!(drawn.iter().any(|d| r.0 >= d.0 && r.0 as i32 + r.2 as i32 <= d.0 as i32 + d.2 as i32 && r.1 >= d.1 && r.1 as i32 + r.3 as i32 <= d.1 as i32 + d.3 as i32), "{r:?}");
            }
            for &((_, _, _, h), _) in plan.bars.iter().filter(|b| b.0.3 >= 4) {
                assert!((4..=10).contains(&h), "bar height {h}");
            }
        }
    }

    #[test]
    fn pose_bands_cover_without_gaps() {
        let r = PoseRows { y0: -50, h: 400, top: 10, shear_end: 80, neck: 120, brow: 80, waist: 220, grip: 0, knee: 400, split: 100 };
        let bands = pose_bands(Pose { breath: 2, nod: 0, sway: 3, ..Pose::default() }, r);
        // Every destination row from the top to the bottom is painted by some band.
        for y in 10..400 {
            assert!(bands.iter().any(|&(a, b, _, dy)| y >= a + dy && y < b + dy), "row {y} {bands:?}");
        }
        assert_eq!(bands.iter().filter(|b| b.2 == 3).count(), 1);
        assert!(bands.iter().all(|b| b.2.abs() <= 3));
        // The legs never move.
        assert!(bands.iter().filter(|b| b.1 > 220).all(|b| b.2 == 0 && b.3 == 0));
        // Music: the shear reaches down to the waist (the upper body tilts), head nods.
        let music = pose_bands(Pose { breath: 0, nod: 4, sway: 4, ..Pose::default() }, PoseRows { shear_end: 220, ..r });
        for y in 10..400 {
            assert!(music.iter().any(|&(a, b, _, dy)| y >= a + dy && y < b + dy), "row {y}");
        }
        assert!(music.iter().any(|b| b.0 >= 120 && b.1 <= 220 && b.2 != 0));
        let still = pose_bands(Pose::default(), r);
        assert!(still.iter().all(|b| b.2 == 0 && b.3 == 0));
        // Bent 20 px right (r43, a linger): the legs stay planted, the torso curves (little at
        // the hips, most at the neck), the head and hair move as one piece (no slant), no gaps.
        let bent = pose_bands(Pose { bend: 20, bend_hair: 20, ..Pose::default() }, r);
        for y in 10..400 {
            assert!(bent.iter().any(|&(a, b, _, dy)| y >= a + dy && y < b + dy), "row {y}");
        }
        assert!(bent.iter().filter(|b| b.0 >= r.waist).all(|b| b.2 == 0), "feet planted");
        let at = |y: i32| bent.iter().find(|b| y >= b.0 && y < b.1).unwrap().2;
        assert!((r.y0..r.neck).all(|y| at(y) == 20), "the head in one piece: {bent:?}");
        let mid = (r.neck + r.waist) / 2;
        assert!(at(mid) > 0 && at(mid) < 10 && at(r.waist - 4) <= 1, "a curve, not a slant: {bent:?}");
        let left = pose_bands(Pose { bend: -12, bend_hair: -12, ..Pose::default() }, r);
        assert_eq!(left.iter().find(|b| 10 >= b.0 && 10 < b.1).unwrap().2, -12, "the other way");
        // Easing in (r44): the hair above the brow trails, more the higher, the face doesn't.
        let easing = pose_bands(Pose { bend: 20, bend_hair: 12, ..Pose::default() }, r);
        for y in 10..400 {
            assert!(easing.iter().any(|&(a, b, _, dy)| y >= a + dy && y < b + dy), "row {y}");
        }
        let at = |y: i32| easing.iter().find(|b| y >= b.0 && y < b.1).unwrap().2;
        assert!((r.brow..r.neck).all(|y| at(y) == 20), "the face in one piece: {easing:?}");
        assert!(at(r.top) == 12 && at(45) > 12 && at(45) < 20, "the crown trails: {easing:?}");
        assert!((r.top..r.brow).all(|y| at(y) <= at(y + 1)), "no step back: {easing:?}");
        // Hanging: the grip row and above stay, the bottom swings the full amount, no gaps.
        let hang = pose_bands(Pose { swing: -9, ..Pose::default() }, PoseRows { grip: 30, ..r });
        for y in -50..400 {
            assert_eq!(hang.iter().filter(|&&(a, b, _, _)| y >= a && y < b).count(), 1, "row {y}");
        }
        assert!(hang.iter().filter(|b| b.1 <= 31).all(|b| b.2 == 0));
        assert_eq!(hang.last().unwrap().2, -9);
        // One leg kicks from the knee: the bands are the swing's, the kick is per band and
        // only right of the split.
        let p = Pose { swing: 4, kick: 10, kick_right: true, ..Pose::default() };
        let rows = PoseRows { grip: 30, knee: 300, ..r };
        let kick = pose_bands(p, rows);
        for y in -50..400 {
            assert_eq!(kick.iter().filter(|&&(a, b, _, _)| y >= a && y < b).count(), 1, "row {y}");
        }
        assert_eq!(kick.last().unwrap().2, 4);
        let leg = pose_leg(p, rows, &kick).unwrap();
        assert_eq!(leg.dx.len(), kick.len());
        let last = kick.len() - 1;
        assert!(leg.dx[last] >= 9);
        assert!(kick.iter().zip(&leg.dx).all(|(b, &d)| b.0 >= 300 || d == 0), "nothing above the knee kicks");
        assert_eq!(leg.extra(last, 120, 20), leg.dx[last]);
        assert_eq!(leg.extra(last, 60, 20), 0, "the other leg stays");
        assert!(pose_leg(Pose { kick: 0, ..p }, rows, &kick).is_none());
    }
}

#[cfg(test)]
mod glint_tests {
    use super::*;

    #[test]
    fn glint_swells_and_fades_symmetrically_then_ends_on_a_wake() {
        let t0 = Instant::now();
        let g = Glint { t0, x: 0.0, y: 0.0, size: 10.0, spin: 0.0, drift: (8.0, 0.0), live: true };
        let life = 560;
        let mut at = t0;
        let mut scales = Vec::new();
        while let Some(t) = g.next_sample(at, life) {
            scales.push(g.scale(t, life));
            at = t;
        }
        assert_eq!(scales.len(), GLINT_STEPS as usize + 1);
        assert_eq!(scales.last(), Some(&None), "the last wake is its end: nothing left drawn");
        let s: Vec<f32> = scales.iter().flatten().copied().collect();
        assert!(s.windows(2).take(3).all(|w| w[1] > w[0]), "4 frames up: {s:?}");
        assert!(s.windows(2).skip(3).all(|w| w[1] < w[0]), "4 frames down: {s:?}");
        assert!(s[0] < 0.3 && *s.last().unwrap() < 0.3);
        assert!((g.offset(t0, life).0 + 4.0).abs() < 1e-3 && g.offset(t0 + Duration::from_millis(280), life).0.abs() < 1e-3);
    }
}

#[cfg(test)]
mod bolt_tests {
    use super::*;

    /// One 30 fps frame (`FRAME_FX`, the view's effect rate) on the strike's clock.
    const FRAME_30: f32 = 1000.0 / 30.0 / BOLT_MS;
    /// The view's quiet streak at sf 1: `len` and `w` (view.rs, `step_teleport`).
    const STREAK: (f32, f32) = (90.0, 6.0);
    /// Diagonal, the launch (straight down), across two monitors, very long, a short hop,
    /// sideways back left, sub-pixel, none at all.
    const STRIKES: [((f32, f32), (f32, f32)); 8] = [
        ((120.0, 840.0), (930.0, 260.0)),
        ((640.0, 0.0), (640.0, 560.0)),
        ((-1700.0, 300.0), (1500.0, 340.0)),
        ((-2400.0, -900.0), (5200.0, 1400.0)),
        ((900.0, 500.0), (948.0, 506.0)),
        ((1500.0, 900.0), (200.0, 950.0)),
        ((300.0, 300.0), (300.4, 300.2)),
        ((300.0, 300.0), (300.0, 300.0)),
    ];

    fn filled(poly: &[(f32, f32)]) -> Vec<ShapeRect> {
        let mut rows = Vec::new();
        shape::poly_rows(poly, &mut rows);
        rows
    }

    /// `shape::hit` for `poly_rows`' output, which comes row by row from the top: a binary search.
    fn lit(rows: &[ShapeRect], x: i32, y: i32) -> bool {
        let from = rows.partition_point(|r| i32::from(r.1) < y);
        rows[from..].iter().take_while(|r| i32::from(r.1) == y).any(|r| (i32::from(r.0)..i32::from(r.0) + i32::from(r.2)).contains(&x))
    }

    #[test]
    fn bolt_reaches_both_ends() {
        let (a, b) = STRIKES[0];
        let bolt = Bolt::new(a, b, 0x5eed, 220.0);
        // The centre line starts exactly at `a` and ends exactly at `b`.
        assert_eq!(bolt.route.first().map(|k| k.p), Some(a));
        assert_eq!(bolt.route.last().map(|k| k.p), Some(b));
        assert_eq!((bolt.point(0.0).0, bolt.point(bolt.len).0), (a, b));
        // Lit mid-flight, gone at the end, and the same `u` draws the same polygon (no flicker).
        assert!(!bolt.frame(0.25).is_empty());
        assert!(bolt.frame(1.0).is_empty());
        assert_eq!(bolt.frame(0.25), bolt.frame(0.25));
        assert_eq!(bolt.frame(0.47), bolt.frame(0.47));
        // The tip travels: short of the target on a few 30 fps frames, not one, not a crawl.
        let short = (0..30).filter(|&k| Bolt::span(k as f32 * FRAME_30).0 < 1.0).count();
        assert!((3..=5).contains(&short), "{short} frames in flight");
        // Home by the arrival; at the landing, a good stretch of trail behind it; drained by the end.
        assert_eq!(Bolt::span(BOLT_ARRIVE).0, 1.0);
        let (head, tail) = Bolt::span(BOLT_LAND);
        assert_eq!(head, 1.0);
        assert!(head - tail > 0.25, "trail at the landing: {}", head - tail);
        assert_eq!(Bolt::span(BOLT_END), (1.0, 1.0));
    }

    #[test]
    fn span_only_moves_forward_and_fits_the_view_timeline() {
        let mut was = (0.0, 0.0);
        for k in 0..=1500 {
            let u = k as f32 / 1000.0;
            let (h, t) = Bolt::span(u);
            assert!((0.0..=1.0).contains(&t) && t <= h && h <= 1.0, "{u}: {h} {t}");
            assert!(h >= was.0 && t >= was.1, "{u}: back from {was:?} to {:?}", (h, t));
            was = (h, t);
        }
        let (h, t) = Bolt::span(0.0);
        assert!(h - t > 0.03, "lit from the first frame");
        let (h, t) = Bolt::span(BOLT_ARRIVE);
        assert!(h - t > 0.5, "the tail well back at touchdown: {}", h - t);
        // The last 30 fps frame before the unmap is a speck at most, nothing to cut off.
        let last = ((BOLT_END / FRAME_30).ceil() - 1.0) * FRAME_30;
        let (h, t) = Bolt::span(last);
        assert!(h - t < 0.03, "{last}: {}", h - t);
        for u in [f32::NAN, f32::INFINITY, 7.0, 1.0] {
            assert_eq!(Bolt::span(u), (1.0, 1.0));
        }
        assert_eq!(Bolt::span(f32::NEG_INFINITY), Bolt::span(0.0));
    }

    #[test]
    fn something_is_on_screen_from_the_first_frame_to_the_drain() {
        // The launch: straight down from the top of the work area to her centre. Lit one frame in
        // and within the intro's 60 ms hold (`BOLT_OVER_HOLD`).
        let launch = Bolt::new(STRIKES[1].0, STRIKES[1].1, 4, 220.0);
        for u in [FRAME_30, 60.0 / BOLT_MS] {
            assert!(filled(&launch.frame(u)).len() > 20, "{u}");
        }
        // Never an empty frame mid-strike (at the view's 60 Hz), on any strike that has length.
        for &(a, b) in &STRIKES[..6] {
            let bolt = Bolt::new(a, b, 11, 220.0);
            let mut u = 0.0;
            while u < 0.6 {
                assert!(!bolt.frame(u).is_empty(), "{a:?} {b:?} at {u}");
                u += 0.016;
            }
        }
    }

    #[test]
    fn every_polygon_stays_inside_the_box() {
        for &(a, b) in &STRIKES {
            for seed in [1, 7, 0xdead_beef, u64::MAX] {
                for scale in [110.0, 220.0, 440.0] {
                    let bolt = Bolt::new(a, b, seed, scale);
                    let (x, y, w, h) = bolt.bbox();
                    assert!(w > 0 && h > 0 && w < 32_767 && h < 32_767, "{a:?} {b:?}: {w}x{h}");
                    let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
                    let sf = scale / 220.0;
                    let polys = |u: f32| [bolt.frame(u), bolt.streak(u, STREAK.0 * sf, STREAK.1 * sf)];
                    for k in 0..=100 {
                        let u = k as f32 / 100.0;
                        for poly in polys(u) {
                            for &(px, py) in &poly {
                                assert!(px >= x0 && py >= y0 && px <= x1 && py <= y1, "{a:?} {b:?} seed {seed} u {u}: ({px}, {py}) outside {:?}", bolt.bbox());
                            }
                        }
                    }
                    // And through the view's own path: every row inside the box's window.
                    for k in (0..=100).step_by(10) {
                        for poly in polys(k as f32 / 100.0) {
                            for r in bolt_rects(&poly, (x, y)) {
                                assert!(r.0 >= 0 && r.1 >= 0 && i32::from(r.0) + i32::from(r.2) <= w && i32::from(r.1) + i32::from(r.3) <= h, "{a:?} {b:?}: {r:?} in {w}x{h}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn no_strike_makes_nan_or_panics() {
        let (nan, inf) = (f32::NAN, f32::INFINITY);
        let ends = [(0.0, 0.0), (0.3, 0.1), (900.0, 640.0), (-2500.0, 1200.0), (-40_000.0, 25_000.0), (nan, 3.0), (inf, -inf), (2.0e6, 0.0), (1.0e30, 1.0e30)];
        for &a in &ends {
            for &b in &ends {
                for seed in [0, 0x5eed, u64::MAX] {
                    for scale in [0.0, -5.0, 220.0, 1.0e9, nan] {
                        let bolt = Bolt::new(a, b, seed, scale);
                        let (_, _, w, h) = bolt.bbox();
                        assert!((1..32_767).contains(&w) && (1..32_767).contains(&h), "{a:?} {b:?}: {w}x{h}");
                        for u in [-1.0, 0.0, 1.0e-7, 0.05, 0.3, 0.5, 0.63, 0.66, 1.0, 1.0e9, nan, inf, -inf] {
                            for poly in [bolt.frame(u), bolt.streak(u, 90.0, 6.0), bolt.streak(u, nan, inf), bolt.streak(u, -3.0, 1.0e9)] {
                                assert!(poly.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "{a:?} {b:?} {u}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn each_seed_strikes_its_own_shape() {
        let (a, b) = STRIKES[0];
        let polys: Vec<Vec<(f32, f32)>> = (0..12).map(|s| Bolt::new(a, b, s, 220.0).frame(0.25)).collect();
        for (i, p) in polys.iter().enumerate() {
            assert!(polys[i + 1..].iter().all(|q| q != p), "seed {i} repeats");
        }
        // The first big swing goes either way: no house shape.
        let first_left = |seed: u64| {
            let bolt = Bolt::new(a, b, seed, 220.0);
            let n = (-bolt.dir.1, bolt.dir.0);
            let off = |k: &Knot| (k.p.0 - a.0) * n.0 + (k.p.1 - a.1) * n.1;
            let early = bolt.route.iter().filter(|k| k.along < 0.3 * bolt.len);
            early.map(off).fold(0.0f32, |m, o| if o.abs() > m.abs() { o } else { m }) > 0.0
        };
        let left = (0..32).filter(|&s| first_left(s)).count();
        assert!((6..=26).contains(&left), "{left} of 32 swing left first");
    }

    #[test]
    fn the_route_zigzags_with_uneven_swings_gentle_at_the_ends() {
        let (a, b) = STRIKES[0];
        let mut lopsided = 0;
        for seed in 0..16 {
            let bolt = Bolt::new(a, b, seed, 220.0);
            let n = (-bolt.dir.1, bolt.dir.0);
            let offs: Vec<(f32, f32)> = bolt.route.iter().map(|k| (k.along / bolt.len, (k.p.0 - a.0) * n.0 + (k.p.1 - a.1) * n.1)).collect();
            // Corners on both sides of the line, uneven.
            let (lo, hi) = offs.iter().fold((0.0f32, 0.0f32), |(lo, hi), &(_, o)| (lo.min(o), hi.max(o)));
            assert!(lo < -8.0 && hi > 8.0, "seed {seed}: {lo}..{hi}");
            lopsided += usize::from((hi + lo).abs() > 3.0);
            // Close to the line near both ends; never more than the swing's cap.
            let cap = SWING_MAX * 220.0 + JAG_MAX * 220.0;
            for &(f, o) in &offs {
                let near_end = f.min(1.0 - f);
                assert!(o.abs() <= cap && (near_end > 0.04 || o.abs() < 40.0), "seed {seed}: {o} at {f}");
            }
            // Bounded: tens of points, not thousands.
            assert!(bolt.route.len() < 120, "{}", bolt.route.len());
        }
        assert!(lopsided >= 10, "swings mirror each other on {} of 16 seeds", 16 - lopsided);
        let long = Bolt::new(STRIKES[3].0, STRIKES[3].1, 3, 220.0);
        assert!(long.route.len() < 120 && long.frame(0.25).len() < 260, "{} {}", long.route.len(), long.frame(0.25).len());
    }

    #[test]
    fn the_ribbon_is_one_solid_piece() {
        // LOCKED #1 against the even-odd fill: wherever the ribbon is a few px wide, every point
        // of its centre line between tail and tip is filled (a fold would read as a hole).
        for &(a, b) in STRIKES[..6].iter().filter(|s| (s.1.0 - s.0.0).abs() < 4000.0) {
            for seed in 0..24u64 {
                let bolt = Bolt::new(a, b, seed.wrapping_mul(0x9e37_79b9_7f4a_7c15), 220.0);
                for k in 0..40 {
                    let u = k as f32 * 0.016;
                    let rows = filled(&bolt.frame(u));
                    for pair in bolt.ribs(u).windows(2) {
                        let (r0, r1) = (pair[0], pair[1]);
                        if r0.left.min(r0.right).min(r1.left).min(r1.right) < 2.0 {
                            continue;
                        }
                        for j in 0..6 {
                            let f = j as f32 / 6.0;
                            let p = (r0.p.0 + (r1.p.0 - r0.p.0) * f, r0.p.1 + (r1.p.1 - r0.p.1) * f);
                            assert!(lit(&rows, p.0.floor() as i32, p.1.floor() as i32), "{a:?} {b:?} seed {seed} u {u}: a hole at {p:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_outline_never_crosses_itself() {
        // Stronger than the holes test: one simple loop, at every scale. And `unfold` alone
        // gets there, so the strike is the shape as laid, not `untangled`'s repair of it.
        for &(a, b) in &STRIKES[..6] {
            for seed in 0..24u64 {
                for scale in [110.0, 220.0, 440.0] {
                    let bolt = Bolt::new(a, b, seed.wrapping_mul(0x2545_f491_4f6c_dd1d), scale);
                    for k in 0..34 {
                        let u = k as f32 * 0.02;
                        let laid = outline(&bolt.ribs(u), bolt.dir);
                        assert_eq!(first_crossing(&laid), None, "{a:?} {b:?} seed {seed} scale {scale} u {u}");
                        assert_eq!(bolt.frame(u), laid, "{a:?} {b:?} seed {seed} scale {scale} u {u}: repaired");
                    }
                }
            }
        }
    }

    fn rib(x: f32, y: f32, left: f32, right: f32) -> Rib {
        Rib { p: (x, y), left, right }
    }

    /// A lit piece from a fuzzed strike (scale 440, u 0.064): a 100° bend whose own cross-section
    /// is thin between wider ones. Sized by the bend's width alone, the next cross-section's
    /// inner corner poked 0.55 px past the incoming leg's inner edge.
    fn thin_bend() -> Vec<Rib> {
        vec![
            rib(-1961.0, 805.0, 0.0, 0.0),
            rib(-1935.8, 797.6, 5.6, 7.15),
            rib(-1910.8, 790.2, 9.83, 3.62),
            rib(-1885.8, 782.8, 10.52, 7.85),
            rib(-1860.8, 775.5, 5.37, 4.83),
            rib(-1860.0, 782.5, 8.16, 5.41),
            rib(-1858.1, 799.4, 13.81, 6.28),
            rib(-1855.4, 823.4, 5.91, 4.24),
            rib(-1852.7, 847.5, 0.0, 0.0),
        ]
    }

    #[test]
    fn a_wide_neighbour_never_folds_over_a_thin_bend() {
        let dir = (0.88, 0.47);
        assert!(first_crossing(&outline(&thin_bend(), dir)).is_some(), "the case still crosses unthinned");
        let mut ribs = thin_bend();
        unfold(&mut ribs);
        let poly = outline(&ribs, dir);
        assert!(ribs.len() >= 6 && first_crossing(&poly).is_none(), "{poly:?}");
        // And the net alone untangles it, keeping the bend.
        let poly = untangled(thin_bend(), dir);
        assert!(poly.len() >= 8 && first_crossing(&poly).is_none(), "{poly:?}");
    }

    #[test]
    fn whatever_goes_in_untangled_returns_one_simple_loop() {
        // Hairpins, doubling back, wild widths: no input `unfold` would ever pass on, and still
        // one simple loop out, on the chord from the tail's point to the tip's.
        let mut rng = Rng::new(0x7a46_01e5);
        for case in 0..3000 {
            let n = 3 + rng.pick(10);
            let mut ribs: Vec<Rib> = (0..n).map(|_| rib(rng.range(0.0, 60.0), rng.range(0.0, 60.0), rng.range(0.0, 30.0), rng.range(0.0, 30.0))).collect();
            (ribs[0].left, ribs[0].right, ribs[n - 1].left, ribs[n - 1].right) = (0.0, 0.0, 0.0, 0.0);
            let (t, h) = (ribs[0].p, ribs[n - 1].p);
            let poly = untangled(ribs, (0.0, 1.0));
            assert_eq!(first_crossing(&poly), None, "case {case}: {poly:?}");
            assert!(poly.iter().all(|p| p.0.is_finite() && p.1.is_finite()), "case {case}");
            if (h.0 - t.0).hypot(h.1 - t.1) >= 0.5 {
                assert!(poly.len() >= 4 && poly[0] == t, "case {case}: {poly:?}");
            }
        }
    }

    #[test]
    fn the_ribbon_is_thickest_behind_the_tip_and_follows_the_figure() {
        let (a, b) = STRIKES[0];
        for seed in 0..8 {
            for u in [0.12, 0.2, 0.3, 0.45] {
                // Tail first, tip last: the half nearer the tip is the thicker, both ends points.
                let ribs = Bolt::new(a, b, seed, 220.0).ribs(u);
                let n = ribs.len();
                let mean = |rs: &[Rib]| rs.iter().map(|r| r.left + r.right).sum::<f32>() / rs.len() as f32;
                assert!(mean(&ribs[n / 2..]) > mean(&ribs[..n / 2]), "seed {seed} u {u}");
                assert!(ribs[0].left + ribs[0].right == 0.0 && ribs[n - 1].left + ribs[n - 1].right == 0.0);
            }
        }
        // Thicker on a bigger figure, within limits.
        let half = |scale: f32| Bolt::new(a, b, 5, scale).half;
        assert!(RIB_MIN <= half(40.0) && half(110.0) < half(220.0) && half(220.0) < half(880.0) && half(880.0) <= RIB_MAX);
        let widest = Bolt::new(a, b, 5, 880.0).ribs(0.2).iter().map(|r| r.left.max(r.right)).fold(0.0f32, f32::max);
        assert!(widest <= RIB_MAX * CHIP.1, "{widest}");
    }

    #[test]
    fn the_quiet_streak_is_a_thin_straight_shard() {
        let (a, b) = ((200.0, 600.0), (1100.0, 380.0));
        let bolt = Bolt::new(a, b, 9, 220.0);
        let (len, w) = STREAK;
        let d = bolt.dir;
        for k in 0..=70 {
            let u = k as f32 / 100.0;
            let poly = bolt.streak(u, len, w);
            if u < 0.6 {
                assert!(poly.len() >= 3, "{u}: lit");
            }
            let along: Vec<f32> = poly.iter().map(|p| (p.0 - a.0) * d.0 + (p.1 - a.1) * d.1).collect();
            let across = poly.iter().map(|p| (p.1 - a.1) * d.0 - (p.0 - a.0) * d.1);
            let (lo, hi) = along.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
            assert!(poly.is_empty() || (hi - lo <= len + 0.01 && lo >= -0.01 && hi <= bolt.len + 0.01), "{u}: {lo}..{hi}");
            for c in across {
                assert!(c.abs() <= 0.5 * w + 0.01, "{u}: {c} off the line");
            }
            // Its point leads, at `span`'s head.
            if let Some(&tip) = along.first() {
                assert!((tip - Bolt::span(u).0 * bolt.len).abs() < 0.05 && tip >= hi - 0.01, "{u}");
            }
        }
        assert!(bolt.streak(1.0, len, w).is_empty());
        assert!(bolt.streak(0.3, 0.0, w).is_empty() && bolt.streak(0.3, len, 0.0).is_empty());
    }
}

#[cfg(test)]
mod note_tests {
    use super::*;

    /// The view draws the first seven (view.rs, `NOTES_MAX`).
    const VIEW_NOTES_MAX: usize = 7;
    /// Capri's default tempo (moods.json `music.bpm` 110).
    const BEAT_110: f32 = 60.0 / 110.0;
    /// Two emitters a head-width apart, the right one a little higher (Capri's music-window).
    const EMITTERS: [[i32; 2]; 2] = [[533, 402], [764, 365]];

    fn sprite_of(k: u64) -> usize {
        (k + k / 4) as usize
    }

    #[test]
    fn notes_rise_and_fade() {
        assert!(music_notes(0.001, BEAT_110, &EMITTERS).len() <= 1, "one note at most in the first instant");
        let lower = EMITTERS.iter().map(|e| e[1]).max().unwrap_or(0) as f32;
        let edge = NOTE_ANGLES[NOTE_ANGLES.len() - 1] + 0.01;
        let mut alive = 0;
        for i in 0..400 {
            let age = 20.0 + i as f32 * 0.025;
            let notes = music_notes(age, BEAT_110, &EMITTERS);
            assert!((3..=VIEW_NOTES_MAX).contains(&notes.len()), "{age}: {} alive", notes.len());
            alive += notes.len();
            for n in &notes {
                assert!(n.y <= lower, "{age}: {n:?} below the lower emitter");
                assert!((0.0..=1.0).contains(&n.alpha), "{n:?}");
                assert!(n.deg.abs() <= edge, "{n:?}");
                assert!(n.size > 0.0 && n.size <= NOTE_PX, "{n:?}");
            }
        }
        assert!(alive >= 4 * 400, "a handful on average: {}", alive as f32 / 400.0);
    }

    #[test]
    fn notes_are_a_pure_function_and_move_smoothly() {
        for age in [0.3f32, 2.0, 7.77, 61.0, 900.5] {
            let now = music_notes(age, BEAT_110, &EMITTERS);
            assert_eq!(format!("{now:?}"), format!("{:?}", music_notes(age, BEAT_110, &EMITTERS)));
            let later = music_notes(age + 0.004, BEAT_110, &EMITTERS);
            let mut kept = 0;
            for n in &now {
                let Some(m) = later.iter().find(|m| m.sprite == n.sprite) else { continue };
                kept += 1;
                assert!((m.x - n.x).hypot(m.y - n.y) < 2.0, "{age}: moved {n:?} -> {m:?}");
                assert!((m.size - n.size).abs() < 4.0 && (m.alpha - n.alpha).abs() < 0.05 && (m.deg - n.deg).abs() < 0.5, "{n:?} -> {m:?}");
            }
            assert!(kept + 1 >= now.len(), "{age}: {kept} of {} kept", now.len());
        }
    }

    #[test]
    fn the_fastest_tempo_keeps_the_newest_few_oldest_first() {
        let beat = 60.0 / 240.0;
        for i in 0..120 {
            let age = 30.0 + i as f32 * 0.037;
            let notes = music_notes(age, beat, &EMITTERS);
            assert!((6..=VIEW_NOTES_MAX).contains(&notes.len()), "{age}: {}", notes.len());
            let newest = (f64::from(age) / f64::from(beat)).floor() as u64;
            assert_eq!(notes.last().map(|n| n.sprite), Some(sprite_of(newest)), "{age}: the newest last");
            assert!(notes.windows(2).all(|w| w[0].sprite < w[1].sprite), "{age}: oldest first");
        }
        // The slowest: fewer, never none.
        let slow: Vec<usize> = (0..100).map(|i| music_notes(30.0 + i as f32 * 0.1, 1.5, &EMITTERS).len()).collect();
        assert!(slow.iter().all(|n| (1..=VIEW_NOTES_MAX).contains(n)), "{slow:?}");
    }

    #[test]
    fn the_cap_only_ever_drops_a_faded_note() {
        // Whatever leaves the list between two calls 5 ms apart is under the view's alpha floor
        // (0.14, `update_notes`) already: none pops while drawn, at any tempo.
        for bpm in [60.0f32, 110.0, 130.0, 150.0, 180.0, 240.0] {
            let beat = 60.0 / bpm;
            let mut was = music_notes(30.0, beat, &EMITTERS);
            for i in 1..2000 {
                let age = 30.0 + i as f32 * 0.005;
                let now = music_notes(age, beat, &EMITTERS);
                for n in was.iter().filter(|n| now.iter().all(|m| m.sprite != n.sprite)) {
                    assert!(n.alpha < 0.14, "{bpm} bpm at {age}: {n:?} dropped while drawn");
                }
                assert!(now.len() <= VIEW_NOTES_MAX, "{bpm} bpm at {age}: {}", now.len());
                was = now;
            }
        }
        // And at the fastest, each note still gets well clear before it goes.
        let beat = 0.25;
        let rise = (0..400).filter_map(|i| {
            let notes = music_notes(30.0 + i as f32 * 0.01, beat, &EMITTERS);
            notes.first().map(|n| EMITTERS[0][1].min(EMITTERS[1][1]) as f32 - n.y)
        });
        assert!(rise.fold(0.0f32, f32::max) > 250.0, "cut halfway up");
    }

    #[test]
    fn nonsense_gives_no_notes() {
        let (nan, inf) = (f32::NAN, f32::INFINITY);
        for (age, beat) in [(5.0, 0.0), (5.0, -0.5), (nan, 0.5), (5.0, nan), (inf, 0.5), (5.0, inf), (-1.0, 0.5), (-inf, 0.5)] {
            assert!(music_notes(age, beat, &EMITTERS).is_empty(), "{age} {beat}");
        }
        assert!(music_notes(5.0, 0.5, &[]).is_empty());
        // Silly but finite: bounded, and nothing out of range.
        for (age, beat) in [(5.0, 1.0e-30), (16.0, 1.0e-18), (3.0e38, 1.0e-30), (1.0e30, 0.5)] {
            let notes = music_notes(age, beat, &EMITTERS);
            assert!(notes.len() <= VIEW_NOTES_MAX && notes.iter().all(|n| n.x.is_finite() && n.y.is_finite() && n.size.is_finite()), "{age} {beat}");
        }
    }

    #[test]
    fn births_alternate_sides_and_drift_outward() {
        for k in 0..12u64 {
            // Note `k` is the newest, 0.4 s old.
            let notes = music_notes(k as f32 * BEAT_110 + 0.4, BEAT_110, &EMITTERS);
            let Some(n) = notes.last().copied() else { panic!("note {k}: none") };
            assert_eq!(n.sprite, sprite_of(k));
            let (from, out) = if k.is_multiple_of(2) { (EMITTERS[0], -1.0) } else { (EMITTERS[1], 1.0) };
            assert!((n.x - from[0] as f32) * out > 10.0, "note {k} at {} from {from:?}: not outward", n.x);
            assert!(n.y < from[1] as f32, "note {k}: not rising");
        }
        // One emitter: they still take turns, left then right.
        let one = [[400, 300]];
        for k in 0..6u64 {
            let Some(n) = music_notes(k as f32 * BEAT_110 + 0.4, BEAT_110, &one).last().copied() else { panic!("note {k}: none") };
            assert!(if k.is_multiple_of(2) { n.x < 390.0 } else { n.x > 410.0 }, "note {k}: {}", n.x);
        }
        // Three: no panic, all used.
        let three = [[100, 300], [300, 280], [500, 300]];
        assert!(!music_notes(9.0, BEAT_110, &three).is_empty());
    }

    #[test]
    fn each_note_leans_into_its_travel_through_a_few_angle_steps() {
        let nearest = |deg: f32| NOTE_ANGLES.iter().enumerate().min_by(|a, b| (a.1 - deg).abs().total_cmp(&(b.1 - deg).abs())).map_or(3, |p| p.0);
        for k in 0..16u64 {
            let (mut steps, mut lean) = (std::collections::BTreeSet::new(), 0.0);
            for j in 0..130 {
                let age = k as f32 * BEAT_110 + j as f32 * 0.025;
                if let Some(n) = music_notes(age, BEAT_110, &EMITTERS).iter().find(|n| n.sprite == sprite_of(k))
                    && n.alpha >= 0.14
                {
                    steps.insert(nearest(n.deg));
                    lean += n.deg;
                }
            }
            assert!(steps.len() >= 3, "note {k}: {steps:?}");
            // Left-going notes tip left (counter-clockwise), right-going ones right.
            assert!(if k.is_multiple_of(2) { lean < 0.0 } else { lean > 0.0 }, "note {k}: {lean}");
        }
    }

    #[test]
    fn hours_into_a_session_it_still_holds() {
        for hours in [1.0f32, 10.0, 100.0] {
            let age = hours * 3600.0;
            let notes = music_notes(age, BEAT_110, &EMITTERS);
            assert!((3..=VIEW_NOTES_MAX).contains(&notes.len()), "{hours} h: {}", notes.len());
            assert!(notes.iter().all(|n| n.x.is_finite() && n.y <= 402.0 && n.size > 0.0 && n.size <= NOTE_PX), "{hours} h: {notes:?}");
            assert!(notes.windows(2).all(|w| w[0].sprite < w[1].sprite), "{hours} h");
            // The next representable age: the same notes, barely moved.
            let next = music_notes(f32::from_bits(age.to_bits() + 1), BEAT_110, &EMITTERS);
            for n in &notes {
                if let Some(m) = next.iter().find(|m| m.sprite == n.sprite) {
                    assert!((m.y - n.y).abs() < 12.0, "{hours} h: {n:?} -> {m:?}");
                }
            }
        }
    }
}

/// Contact sheets of the bolt and the notes, for tuning them by eye:
/// `FX_SHEETS=dir cargo test --bin synaps-avatar fx::sheets -- --ignored` (else the temp dir).
/// The bolt is painted through `bolt_rects`, the view's own path; the notes through the view's
/// quantisation in `update_notes` (the alpha floor and four shrink-and-dim steps, the size cap,
/// the nearest pre-rotated angle), as stand-in glyphs in each sprite's colour (the art isn't in git).
#[cfg(test)]
mod sheets {
    use super::*;
    use image::{Rgb, RgbImage};

    const BG: Rgb<u8> = Rgb([16, 18, 26]);
    const GAP: Rgb<u8> = Rgb([64, 64, 72]);
    const INK: Rgb<u8> = Rgb([150, 150, 165]);
    /// view.rs `update_notes`, `NOTE_SCALE`, `NOTE_TINT`.
    const NOTE_FLOOR: f32 = 0.14;
    const NOTE_SCALE: f32 = 1.15;
    const NOTE_TINT: u32 = 0x3a4656;
    /// Capri's four note sprites' colours: cyan, lime, pink, violet.
    const SPRITES: [u32; 4] = [0x00ffdd, 0x9cff3c, 0xff1769, 0xa668ff];
    /// 3×5 digits, three bits a row from the top, the high bit leftmost.
    const DIGITS: [u16; 10] = [0x7b6f, 0x2c97, 0x73e7, 0x73cf, 0x5bc9, 0x79cf, 0x79ef, 0x7249, 0x7bef, 0x7bcf];

    fn rgb(c: u32) -> Rgb<u8> {
        Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
    }

    fn put(im: &mut RgbImage, x: i64, y: i64, c: Rgb<u8>) {
        if x >= 0 && y >= 0 && x < i64::from(im.width()) && y < i64::from(im.height()) {
            im.put_pixel(x as u32, y as u32, c);
        }
    }

    /// `n` in 3×5 digits, `px` px a dot, its top-left at (x, y).
    fn label(im: &mut RgbImage, x: i64, y: i64, n: u32, px: i64) {
        for (i, ch) in n.to_string().bytes().enumerate() {
            let g = DIGITS[usize::from(ch - b'0')];
            for bit in 0..15i64 {
                if (g >> (14 - bit)) & 1 == 1 {
                    let (cx, cy) = (x + (i as i64 * 4 + bit % 3) * px, y + bit / 3 * px);
                    for d in 0..px * px {
                        put(im, cx + d % px, cy + d / px, INK);
                    }
                }
            }
        }
    }

    fn ring(im: &mut RgbImage, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgb<u8>) {
        for i in 0..720 {
            let a = i as f32 / 720.0 * 2.0 * PI;
            put(im, (cx + rx * a.cos()).round() as i64, (cy + ry * a.sin()).round() as i64, c);
        }
    }

    /// Cells left to right, `cols` a row, each `zoom`× (nearest), 4 px apart.
    fn grid(cells: &[RgbImage], cols: usize, zoom: u32) -> RgbImage {
        let w = cells.iter().map(RgbImage::width).max().unwrap_or(1) * zoom + 4;
        let h = cells.iter().map(RgbImage::height).max().unwrap_or(1) * zoom + 4;
        let rows = cells.len().div_ceil(cols) as u32;
        let mut out = RgbImage::from_pixel(cols as u32 * w, rows * h, GAP);
        for (i, c) in cells.iter().enumerate() {
            let (ox, oy) = ((i % cols) as u32 * w, (i / cols) as u32 * h);
            for (x, y, p) in c.enumerate_pixels() {
                for d in 0..zoom * zoom {
                    out.put_pixel(ox + x * zoom + d % zoom, oy + y * zoom + d / zoom, *p);
                }
            }
        }
        out
    }

    /// `im` at 1/`k`: a block takes any lit pixel in it, so thin lines survive.
    fn shrink(im: &RgbImage, k: u32) -> RgbImage {
        let mut out = RgbImage::from_pixel(im.width().div_ceil(k), im.height().div_ceil(k), BG);
        for (x, y, p) in im.enumerate_pixels() {
            if *p != BG {
                out.put_pixel(x / k, y / k, *p);
            }
        }
        out
    }

    fn save(im: &RgbImage, name: &str) {
        let dir = std::env::var_os("FX_SHEETS").map_or_else(|| std::env::temp_dir().join("fx-sheets"), std::path::PathBuf::from);
        std::fs::create_dir_all(&dir).expect("sheet dir");
        im.save(dir.join(name)).expect("sheet png");
        eprintln!("{}: {}x{}", dir.join(name).display(), im.width(), im.height());
    }

    /// `bolt` at `u` as the view paints it: `bolt_rects` in `BOLT_LINE` over its box; where she
    /// was (cyan) and where she lands (yellow) ringed, for the sheet only.
    fn bolt_cell(bolt: &Bolt, u: f32, quiet: bool, sf: f32) -> RgbImage {
        let (bx, by, bw, bh) = bolt.bbox();
        let mut im = RgbImage::from_pixel(bw as u32, bh as u32, BG);
        let b = bolt.route.last().map_or(bolt.a, |k| k.p);
        for (p, c) in [(bolt.a, CYAN), (b, YELLOW)] {
            ring(&mut im, p.0 - bx as f32, p.1 - by as f32, 4.0, 4.0, rgb(c));
        }
        let poly = if quiet { bolt.streak(u, 90.0 * sf, 6.0 * sf) } else { bolt.frame(u) };
        for (x, y, w, h) in bolt_rects(&poly, (bx, by)) {
            for d in 0..i64::from(w) * i64::from(h) {
                put(&mut im, i64::from(x) + d % i64::from(w), i64::from(y) + d / i64::from(w), rgb(BOLT_LINE));
            }
        }
        im
    }

    /// One strike across its timeline at 30 fps (0 to 300 ms), each cell labelled in ms.
    fn strike(name: &str, bolt: &Bolt, quiet: bool, cols: usize, zoom: u32, by: u32) {
        let cells: Vec<RgbImage> = (0..10)
            .map(|k| {
                let u = k as f32 * 1000.0 / 30.0 / BOLT_MS;
                let mut c = shrink(&bolt_cell(bolt, u, quiet, 1.0), by);
                label(&mut c, 3, 3, (u * BOLT_MS).round() as u32, 2);
                c
            })
            .collect();
        save(&grid(&cells, cols, zoom), name);
    }

    /// The view's four alpha steps (`update_notes`).
    fn step(alpha: f32) -> f32 {
        if alpha > 0.8 {
            1.0
        } else if alpha > 0.55 {
            0.72
        } else if alpha > 0.32 {
            0.48
        } else {
            0.28
        }
    }

    fn mix(a: u32, b: u32, k: f32) -> Rgb<u8> {
        let ch = |sh: u32| {
            let (x, y) = (((a >> sh) & 0xff) as f32, ((b >> sh) & 0xff) as f32);
            (x + (y - x) * k).round() as u8
        };
        Rgb([ch(16), ch(8), ch(0)])
    }

    /// A stand-in eighth note `size` px square centred at (cx, cy), turned `deg` clockwise.
    fn glyph(im: &mut RgbImage, cx: f32, cy: f32, size: f32, deg: f32, c: Rgb<u8>) {
        let (s, co) = deg.to_radians().sin_cos();
        let r = size.ceil() as i64;
        for d in 0..(2 * r + 1) * (2 * r + 1) {
            let (x, y) = ((d % (2 * r + 1) - r) as f32, (d / (2 * r + 1) - r) as f32);
            // Back into the glyph's own frame (y down, so clockwise is the plain rotation).
            let (gx, gy) = ((co * x + s * y) / size, (co * y - s * x) / size);
            let head = ((gx + 0.12) / 0.21).powi(2) + ((gy - 0.27) / 0.16).powi(2) < 1.0;
            let stem = (0.05..0.14).contains(&gx) && (-0.45..0.27).contains(&gy);
            let flag = (0.12..0.36).contains(&gx) && gy > -0.45 && gy < -0.3 + 0.5 * (gx - 0.12);
            if head || stem || flag {
                put(im, (cx + x).round() as i64, (cy + y).round() as i64, c);
            }
        }
    }

    /// The notes at `age` around her head (a ring between the emitters, for the sheet only), at
    /// `geo` device px per canvas px, as the view quantises them; labelled in centiseconds.
    /// The canvas span a notes cell shows, `(x0, x1)` and `(y0, y1)`: room out to both sides
    /// and well above the emitters.
    fn crop(em: &[[i32; 2]]) -> ((i32, i32), (i32, i32)) {
        let xs = (em.iter().map(|e| e[0]).min().unwrap_or(0) - 420, em.iter().map(|e| e[0]).max().unwrap_or(0) + 420);
        let ys = (em.iter().map(|e| e[1]).min().unwrap_or(0) - 560, em.iter().map(|e| e[1]).max().unwrap_or(0) + 200);
        (xs, ys)
    }

    fn notes_cell(age: f32, beat: f32, em: &[[i32; 2]], geo: f32) -> RgbImage {
        let (xs, ys) = crop(em);
        let to = |x: f32, y: f32| ((x - xs.0 as f32) * geo, (y - ys.0 as f32) * geo);
        let mut im = RgbImage::from_pixel(((xs.1 - xs.0) as f32 * geo) as u32, ((ys.1 - ys.0) as f32 * geo) as u32, BG);
        let (l, r) = (em[0], em[em.len() - 1]);
        let (hx, hy) = to((l[0] + r[0]) as f32 / 2.0, (l[1] + r[1]) as f32 / 2.0);
        let rx = (r[0] - l[0]).abs() as f32 / 2.0 * geo;
        ring(&mut im, hx, hy - 0.1 * rx, rx, 1.15 * rx, rgb(0x3a3a50));
        for e in em {
            let (x, y) = to(e[0] as f32, e[1] as f32);
            ring(&mut im, x, y, 3.0, 3.0, INK);
        }
        let cap = (NOTE_PX * geo * NOTE_SCALE).ceil();
        for n in music_notes(age, beat, em).iter().take(NOTES_DRAWN) {
            if n.alpha < NOTE_FLOOR {
                continue;
            }
            let k = step(n.alpha);
            let size = (n.size * geo * NOTE_SCALE * (0.55 + 0.45 * k)).round().clamp(4.0, cap);
            let deg = NOTE_ANGLES.iter().copied().min_by(|a, b| (a - n.deg).abs().total_cmp(&(b - n.deg).abs())).unwrap_or(0.0);
            let (x, y) = to(n.x, n.y);
            glyph(&mut im, x, y, size, deg, mix(NOTE_TINT, SPRITES[n.sprite % SPRITES.len()], k));
        }
        label(&mut im, 3, 3, (age * 100.0).round() as u32, 2);
        im
    }

    fn notes(name: &str, beat: f32, em: &[[i32; 2]], from: f32, every: f32, n: usize) {
        let cells: Vec<RgbImage> = (0..n).map(|i| notes_cell(from + i as f32 * every, beat, em, 0.4)).collect();
        save(&grid(&cells, 6, 1), name);
    }

    #[test]
    #[ignore = "writes PNG contact sheets for tuning by eye (FX_SHEETS=dir)"]
    fn contact_sheets() {
        // The bolt over its timeline: diagonal, the launch, a short hop, a mid hop zoomed in, across
        // two monitors (at half size), and the quiet streak.
        strike("bolt-diagonal.png", &Bolt::new((120.0, 840.0), (930.0, 260.0), 0x5eed, 220.0), false, 5, 1, 1);
        strike("bolt-launch.png", &Bolt::new((640.0, 0.0), (640.0, 560.0), 4, 220.0), false, 10, 1, 1);
        strike("bolt-hop48.png", &Bolt::new((900.0, 500.0), (948.0, 506.0), 21, 220.0), false, 5, 4, 1);
        strike("bolt-hop300.png", &Bolt::new((100.0, 300.0), (380.0, 200.0), 77, 220.0), false, 5, 2, 1);
        strike("bolt-monitors.png", &Bolt::new((-1700.0, 300.0), (1500.0, 700.0), 9, 220.0), false, 2, 1, 2);
        strike("bolt-quiet.png", &Bolt::new((120.0, 840.0), (930.0, 260.0), 0x5eed, 220.0), true, 5, 1, 1);
        // Twelve seeds of one strike, mostly lit (no house shape), and the same strike on a
        // small, the usual and a 4K figure.
        let seeds: Vec<RgbImage> = (0..12).map(|s| bolt_cell(&Bolt::new((60.0, 400.0), (700.0, 120.0), s * 7919 + 1, 220.0), 0.24, false, 1.0)).collect();
        save(&grid(&seeds, 4, 1), "bolt-seeds.png");
        let scales: Vec<RgbImage> = [110.0, 220.0, 440.0].iter().map(|&sc| bolt_cell(&Bolt::new((60.0, 400.0), (700.0, 120.0), 3, sc), 0.24, false, sc / 220.0)).collect();
        save(&grid(&scales, 3, 1), "bolt-scales.png");

        // The notes: the first seconds at Capri's 110 bpm, then steady state at 40, 110 and 240.
        let window = [[533, 402], [764, 365]];
        notes("notes-110-start.png", 60.0 / 110.0, &window, 0.0, 0.2, 24);
        notes("notes-110.png", 60.0 / 110.0, &window, 30.0, 0.1, 18);
        notes("notes-40.png", 1.5, &window, 30.0, 0.25, 12);
        notes("notes-240.png", 0.25, &window, 30.0, 0.05, 12);
        notes("notes-floor-1h.png", 60.0 / 110.0, &[[522, 642], [775, 591]], 3600.0, 0.1, 12);
        // Every note's centre over ten seconds, in its sprite's colour.
        let mut im = notes_cell(-1.0, 60.0 / 110.0, &window, 0.4);
        let (xs, ys) = crop(&window);
        for i in 0..600 {
            for n in music_notes(20.0 + i as f32 / 60.0, 60.0 / 110.0, &window) {
                let (x, y) = ((n.x - xs.0 as f32) * 0.4, (n.y - ys.0 as f32) * 0.4);
                put(&mut im, x.round() as i64, y.round() as i64, rgb(SPRITES[n.sprite % SPRITES.len()]));
            }
        }
        save(&im, "notes-paths.png");
    }
}
