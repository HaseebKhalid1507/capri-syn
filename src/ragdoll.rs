//! Ragdoll physics (BRIEF-ragdoll §1): the verlet band chain she dangles by, the ballistic
//! window flight with bounces, the work-area arena, the pointer estimators and the squash
//! spring. Pure: no gpui, no x11rb, no `Instant`; seconds in, device px.
//!
//! The view wiring is ragview.rs.

use crate::edges::{Platform, WinRect};

pub const H: f32 = 1.0 / 240.0;
pub const MAX_FRAME_DT: f32 = 0.1;
pub const MAX_SUBSTEPS: u32 = 24;
pub const REST_OFFSET: f32 = 0.75;
pub const REST_SPEED: f32 = 6.0;
pub const REST_HOLD: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub links_down: usize,
    pub links_up: usize,
    pub gravity: f32,
    pub damping: f32,
    pub damping_near_rest: f32,
    pub iterations: usize,
    pub bend_stiff: f32,
    pub up_stiff: f32,
    pub max_swing_deg: f32,
    pub max_swing_up_deg: f32,
    pub max_bend_deg: f32,
    pub stretch: (f32, f32),
    pub stretch_per_accel: f32,
    pub stretch_omega: f32,
    pub stretch_zeta: f32,
    /// Lateral limits of the chain relative to the grip.
    pub room: (f32, f32),
    /// Share of the anchor's motion every node takes directly (0 = pure physics): the lag
    /// that bends her when she's dragged or shaken shrinks by this much.
    pub follow: f32,
    /// Damping of each node's velocity relative to its parent (1/s): kills the fast leg flap
    /// (the chain's bending mode) without slowing the whole-body swing.
    pub bend_damping: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            links_down: 8,
            links_up: 0,
            gravity: 5200.0,
            damping: 1.9,
            damping_near_rest: 7.0,
            iterations: 4,
            bend_stiff: 0.08,
            up_stiff: 0.35,
            max_swing_deg: 32.0,
            max_swing_up_deg: 18.0,
            max_bend_deg: 14.0,
            stretch: (0.92, 1.08),
            stretch_per_accel: 3.0e-6,
            stretch_omega: std::f32::consts::TAU * 3.2,
            stretch_zeta: 0.32,
            room: (-1.0e5, 1.0e5),
            follow: 0.0,
            bend_damping: 0.0,
        }
    }
}

/// Bending damping at strength 0 (1/s); half of it at the 0.5 default.
pub const BEND_DAMPING: f32 = 120.0;

/// `[ragdoll] strength` → the physics (0 = rigid, 1 = the full-strength defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strength(pub f32);

impl Strength {
    pub fn k(self) -> f32 {
        fin(self.0).clamp(0.0, 1.0)
    }
    /// Damping multiplier: x1 at full strength, x2 at 0.5 (settles about twice as fast).
    pub fn damp(self) -> f32 {
        1.0 + 2.0 * (1.0 - self.k())
    }
    /// The chain params: bend limits, stretch and drag/shake lag scale with k; the damping
    /// (whole-body and bending) grows as k drops. At 1 these are exactly `Params::default()`.
    pub fn params(self) -> Params {
        let (k, d) = (self.k(), self.damp());
        let p = Params::default();
        Params {
            damping: p.damping * d,
            damping_near_rest: p.damping_near_rest * d,
            max_swing_deg: p.max_swing_deg * k,
            max_swing_up_deg: p.max_swing_up_deg * k,
            max_bend_deg: p.max_bend_deg * k,
            stretch: (1.0 - (1.0 - p.stretch.0) * k, 1.0 + (p.stretch.1 - 1.0) * k),
            stretch_per_accel: p.stretch_per_accel * k,
            stretch_zeta: (p.stretch_zeta * d).min(1.0),
            follow: 1.0 - k,
            bend_damping: BEND_DAMPING * (1.0 - k),
            ..p
        }
    }
}

impl Params {
    /// Logical → device px.
    pub fn scaled(&self, sf: f32) -> Params {
        Params {
            gravity: self.gravity * sf,
            stretch_per_accel: self.stretch_per_accel / sf,
            room: (self.room.0 * sf, self.room.1 * sf),
            ..*self
        }
    }
}

fn fin(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn sane_dt(dt: f32) -> f32 {
    if dt.is_nan() || dt <= 0.0 { 0.0 } else { dt.min(MAX_FRAME_DT) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Node {
    p: (f32, f32),
    prev: (f32, f32),
}

/// One chain hanging (and, for the torso grip, standing) from the grip point.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    p: Params,
    /// Rest offsets (rows) from the grip: down positive and increasing, up negative and decreasing.
    rows_down: Vec<f32>,
    rows_up: Vec<f32>,
    down: Vec<Node>,
    up: Vec<Node>,
    anchor: (f32, f32),
    accum: f32,
    gravity: f32,
    s: f32,
    s_vel: f32,
    a_est: f32,
    v_prev: Option<f32>,
    still: f32,
    asleep: bool,
    /// Per down node: lateral limits relative to the grip (else `p.room`).
    rooms: Vec<(f32, f32)>,
}

impl Chain {
    /// `rows_up` / `rows_down`: the rest rows of the nodes relative to the grip (grip = 0).
    pub fn new(p: &Params, rows_up: &[i32], rows_down: &[i32], anchor: (f32, f32)) -> Chain {
        let mk = |r: &[i32]| -> Vec<Node> {
            r.iter().map(|&y| Node { p: (anchor.0, anchor.1 + y as f32), prev: (anchor.0, anchor.1 + y as f32) }).collect()
        };
        Chain {
            p: *p,
            rows_down: rows_down.iter().map(|&r| r as f32).collect(),
            rows_up: rows_up.iter().map(|&r| r as f32).collect(),
            down: mk(rows_down),
            up: mk(rows_up),
            anchor,
            accum: 0.0,
            gravity: p.gravity,
            s: 1.0,
            s_vel: 0.0,
            a_est: 0.0,
            v_prev: None,
            still: 0.0,
            asleep: false,
            rooms: Vec::new(),
        }
    }

    /// Per-node lateral limits (the window's room for the rows around each down node).
    pub fn set_rooms(&mut self, rooms: Vec<(f32, f32)>) {
        self.rooms = rooms.into_iter().map(|(a, b)| (fin(a).min(0.0), fin(b).max(0.0))).collect();
    }

    /// Evenly spaced rest rows for `links` nodes over `len` px (down) or `-len` (up).
    pub fn rows(links: usize, len: i32, up: bool) -> Vec<i32> {
        (1..=links).map(|i| ((len as f32 * i as f32 / links as f32).round() as i32) * if up { -1 } else { 1 }).collect()
    }

    pub fn set_gravity(&mut self, g: f32) {
        self.gravity = fin(g);
    }

    pub fn asleep(&self) -> bool {
        self.asleep
    }

    pub fn stretch(&self) -> f32 {
        self.s
    }

    /// Velocity added to the nodes, growing with the distance from the grip.
    pub fn kick(&mut self, impulse: (f32, f32)) {
        let (ix, iy) = (fin(impulse.0), fin(impulse.1));
        let n = self.down.len().max(1) as f32;
        for (i, nd) in self.down.iter_mut().enumerate() {
            let k = (i + 1) as f32 / n;
            nd.prev.0 -= ix * H * k;
            nd.prev.1 -= iy * H * k;
        }
        self.asleep = false;
        self.still = 0.0;
    }

    /// The last down node minus its rest position.
    pub fn feet_offset(&self) -> (f32, f32) {
        match (self.down.last(), self.rows_down.last()) {
            (Some(n), Some(&r)) => (n.p.0 - self.anchor.0, n.p.1 - (self.anchor.1 + r)),
            _ => (0.0, 0.0),
        }
    }

    pub fn feet_speed(&self) -> f32 {
        self.down.last().map_or(0.0, |n| ((n.p.0 - n.prev.0).powi(2) + (n.p.1 - n.prev.1).powi(2)).sqrt() / H)
    }

    fn offsets(&self) -> (f32, f32) {
        let mut off = 0.0f32;
        let mut spd = 0.0f32;
        for (nodes, rows) in [(&self.down, &self.rows_down), (&self.up, &self.rows_up)] {
            for (n, r) in nodes.iter().zip(rows.iter()) {
                off = off.max(((n.p.0 - self.anchor.0).powi(2) + (n.p.1 - self.anchor.1 - r).powi(2)).sqrt());
                spd = spd.max(((n.p.0 - n.prev.0).powi(2) + (n.p.1 - n.prev.1).powi(2)).sqrt() / H);
            }
        }
        (off, spd)
    }

    /// Advance by `dt`, the anchor moving linearly to `anchor` across the substeps.
    pub fn step(&mut self, anchor: (f32, f32), dt: f32) {
        let dt = sane_dt(dt);
        let target = if anchor.0.is_finite() && anchor.1.is_finite() { anchor } else { self.anchor };
        self.accum += dt;
        let n = ((self.accum + 1e-6) / H).floor().min(MAX_SUBSTEPS as f32) as u32;
        if n == 0 {
            return;
        }
        self.accum = (self.accum - n as f32 * H).clamp(0.0, H);
        let from = self.anchor;
        for k in 1..=n {
            let u = k as f32 / n as f32;
            let a = (from.0 + (target.0 - from.0) * u, from.1 + (target.1 - from.1) * u);
            self.substep(a);
        }
    }

    fn substep(&mut self, a: (f32, f32)) {
        let prev_anchor = self.anchor;
        self.anchor = a;
        if self.p.follow > 0.0 {
            let d = ((a.0 - prev_anchor.0) * self.p.follow, (a.1 - prev_anchor.1) * self.p.follow);
            for nd in self.down.iter_mut().chain(self.up.iter_mut()) {
                nd.p = (nd.p.0 + d.0, nd.p.1 + d.1);
                nd.prev = (nd.prev.0 + d.0, nd.prev.1 + d.1);
            }
        }
        // Stretch: driven by the anchor's vertical acceleration.
        let v = (a.1 - prev_anchor.1) / H;
        let acc = self.v_prev.map_or(0.0, |vp| (v - vp) / H).clamp(-60_000.0, 60_000.0);
        self.v_prev = Some(v);
        let alpha = 1.0 - (-H / 0.03f32).exp();
        self.a_est += (acc - self.a_est) * alpha;
        let (w, z) = (self.p.stretch_omega, self.p.stretch_zeta);
        let force = -w * w * (self.s - 1.0) - 2.0 * z * w * self.s_vel + w * w * self.p.stretch_per_accel * (-self.a_est);
        self.s_vel = fin(self.s_vel + force * H);
        self.s = fin(self.s + self.s_vel * H).clamp(self.p.stretch.0, self.p.stretch.1);
        if self.s <= self.p.stretch.0 || self.s >= self.p.stretch.1 {
            self.s_vel *= 0.5;
        }

        let (off, spd) = self.offsets();
        let damp = if off < 3.0 && spd < 40.0 { self.p.damping_near_rest } else { self.p.damping };
        let keep = (-damp * H).exp();
        let g = self.gravity * H * H;
        if self.p.bend_damping > 0.0 {
            // Each segment's spin relative to the segment above it decays (the leg flap); a
            // whole-body swing (every segment spinning together) is left alone.
            let f = 1.0 - (-self.p.bend_damping * H).exp();
            let va = (a.0 - prev_anchor.0, a.1 - prev_anchor.1);
            for nodes in [&mut self.down, &mut self.up] {
                let (mut pp, mut pv) = (a, va);
                let mut prev_w: Option<f32> = None;
                for nd in nodes.iter_mut() {
                    let d = (nd.p.0 - pp.0, nd.p.1 - pp.1);
                    let dd = (d.0 * d.0 + d.1 * d.1).max(1e-6);
                    let v = (nd.p.0 - nd.prev.0, nd.p.1 - nd.prev.1);
                    let rel = (v.0 - pv.0, v.1 - pv.1);
                    let w = (d.0 * rel.1 - d.1 * rel.0) / dd;
                    let radial = (rel.0 * d.0 + rel.1 * d.1) / dd;
                    let w2 = prev_w.map_or(w, |pw| w - (w - pw) * f);
                    let nv = (pv.0 + radial * d.0 - w2 * d.1, pv.1 + radial * d.1 + w2 * d.0);
                    nd.prev = (nd.p.0 - nv.0, nd.p.1 - nv.1);
                    prev_w = Some(w2);
                    pp = nd.p;
                    pv = nv;
                }
            }
        }
        for nd in self.down.iter_mut().chain(self.up.iter_mut()) {
            let vx = (nd.p.0 - nd.prev.0) * keep;
            let vy = (nd.p.1 - nd.prev.1) * keep;
            nd.prev = nd.p;
            nd.p = (nd.p.0 + vx, nd.p.1 + vy + g);
        }
        let s = self.s;
        for _ in 0..self.p.iterations {
            relax(&mut self.down, &self.rows_down, a, s);
            relax(&mut self.up, &self.rows_up, a, s);
        }
        // Muscle: bend pull toward the parent's direction (down), straight up (up).
        pull(&mut self.down, &self.rows_down, a, s, self.p.bend_stiff, false);
        pull(&mut self.up, &self.rows_up, a, s, self.p.up_stiff, true);
        let sw = self.p.max_swing_deg.to_radians();
        let swu = self.p.max_swing_up_deg.to_radians();
        let bd = self.p.max_bend_deg.to_radians();
        clamp_angles(&mut self.down, &self.rows_down, a, s, sw, bd, false);
        clamp_angles(&mut self.up, &self.rows_up, a, s, swu, bd, true);
        let room = self.p.room;
        for (i, nd) in self.down.iter_mut().enumerate().chain(self.up.iter_mut().map(|n| (usize::MAX, n))) {
            let room = self.rooms.get(i).copied().unwrap_or(room);
            nd.p.0 = nd.p.0.clamp(a.0 + room.0, a.0 + room.1);
            if !(nd.p.0.is_finite() && nd.p.1.is_finite()) {
                nd.p = a;
            }
            if !(nd.prev.0.is_finite() && nd.prev.1.is_finite()) {
                nd.prev = nd.p;
            }
        }
        let (off, spd) = self.offsets();
        if off < REST_OFFSET && spd < REST_SPEED {
            self.still += H;
        } else {
            self.still = 0.0;
        }
        self.asleep = self.still >= REST_HOLD;
    }

    /// Source row (relative to the grip) → dest (dx, dy) relative to the grip.
    fn map(&self, r: f32) -> (f32, f32) {
        let (nodes, rows) = if r >= 0.0 { (&self.down, &self.rows_down) } else { (&self.up, &self.rows_up) };
        let mut prev_r = 0.0;
        let mut prev_p = (0.0, 0.0);
        let mut dir = (0.0, if r >= 0.0 { self.s } else { -self.s });
        for (n, &nr) in nodes.iter().zip(rows.iter()) {
            let p = (n.p.0 - self.anchor.0, n.p.1 - self.anchor.1);
            let span = nr - prev_r;
            if span.abs() > 1e-3 {
                dir = ((p.0 - prev_p.0) / span.abs(), (p.1 - prev_p.1) / span.abs());
            }
            if (r >= 0.0 && r <= nr) || (r < 0.0 && r >= nr) {
                let t = if span.abs() > 1e-3 { (r - prev_r) / span } else { 1.0 };
                return (prev_p.0 + (p.0 - prev_p.0) * t, prev_p.1 + (p.1 - prev_p.1) * t);
            }
            prev_r = nr;
            prev_p = p;
        }
        // Past the last node: continue along the last segment.
        let d = (r - prev_r).abs();
        (prev_p.0 + dir.0 * d, prev_p.1 + dir.1 * d)
    }

    /// Bands that tile the deformed frame: (src y0, src y1, dx, dy), drawn at rows y0+dy..y1+dy.
    pub fn bands(&self, band_px: i32, src_top: i32, src_bottom: i32, grip_row: i32) -> Vec<(i32, i32, i32, i32)> {
        let bp = band_px.max(2);
        if src_bottom <= src_top {
            return Vec::new();
        }
        let grip = grip_row.clamp(src_top, src_bottom - 1);
        let g0 = (grip - bp / 2).max(src_top);
        let g1 = (g0 + bp).min(src_bottom);
        let dest = |s: i32| -> (i32, i32) {
            let (dx, dy) = self.map((s - grip) as f32);
            let (dx, dy) = (fin(dx), fin(dy));
            ((dx.round() as i32).clamp(-100_000, 100_000), (grip as f32 + dy).round().clamp(-1.0e6, 1.0e6) as i32)
        };
        // Boundaries outward from the grip band; dest rows forced monotone.
        let mut down: Vec<(i32, i32, i32)> = vec![(g1, g1, 0)];
        let mut s = g1;
        while s < src_bottom {
            let ns = (s + bp).min(src_bottom);
            let (dx, d) = dest(ns);
            let last = down.last().unwrap().1;
            down.push((ns, d.max(last), dx));
            s = ns;
        }
        let mut up: Vec<(i32, i32, i32)> = vec![(g0, g0, 0)];
        let mut s = g0;
        while s > src_top {
            let ns = (s - bp).max(src_top);
            let (dx, d) = dest(ns);
            let last = up.last().unwrap().1;
            up.push((ns, d.min(last), dx));
            s = ns;
        }
        let mut out = Vec::new();
        let band = |s0: i32, d0: i32, d1: i32, dx: i32| -> Option<(i32, i32, i32, i32)> {
            let h = d1 - d0;
            if h <= 0 {
                return None;
            }
            let h = h.min(src_bottom - src_top);
            let y0 = s0.clamp(src_top, src_bottom - h);
            Some((y0, y0 + h, dx, d0 - y0))
        };
        // Up bands, top first.
        for w in up.windows(2).rev() {
            let (s_lo, d_lo, dx) = (w[1].0, w[1].1, w[1].2);
            if let Some(b) = band(s_lo, d_lo, w[0].1, dx) {
                out.push(b);
            }
        }
        out.push((g0, g1, 0, 0));
        for w in down.windows(2) {
            let dx = if w[0].0 == g1 { dest(g1).0 } else { w[0].2 };
            if let Some(b) = band(w[0].0, w[0].1, w[1].1, dx) {
                out.push(b);
            }
        }
        out
    }

    /// Dest row (relative to the frame) → the source row under it: catching her mid-flight.
    pub fn source_of(&self, dest: (i32, i32), bands: &[(i32, i32, i32, i32)]) -> Option<(i32, i32)> {
        bands.iter().find(|b| dest.1 >= b.0 + b.3 && dest.1 < b.1 + b.3).map(|b| (dest.0 - b.2, dest.1 - b.3))
    }

    #[cfg(test)]
    pub fn segment_angles(&self) -> (Vec<f32>, Vec<f32>) {
        let ang = |nodes: &[Node], up: bool| -> Vec<f32> {
            let mut prev = self.anchor;
            nodes
                .iter()
                .map(|n| {
                    let (dx, dy) = (n.p.0 - prev.0, n.p.1 - prev.1);
                    prev = n.p;
                    if up { dx.atan2(-dy) } else { dx.atan2(dy) }
                })
                .collect()
        };
        (ang(&self.down, false), ang(&self.up, true))
    }
}

fn seg_len(rows: &[f32], i: usize, s: f32) -> f32 {
    let prev = if i == 0 { 0.0 } else { rows[i - 1] };
    (rows[i] - prev).abs() * s
}

fn relax(nodes: &mut [Node], rows: &[f32], a: (f32, f32), s: f32) {
    for i in 0..nodes.len() {
        let parent = if i == 0 { a } else { nodes[i - 1].p };
        let l = seg_len(rows, i, s);
        let (dx, dy) = (nodes[i].p.0 - parent.0, nodes[i].p.1 - parent.1);
        let d = (dx * dx + dy * dy).sqrt().max(1e-6);
        let diff = (d - l) / d;
        if i == 0 {
            nodes[i].p.0 -= dx * diff;
            nodes[i].p.1 -= dy * diff;
        } else {
            nodes[i].p.0 -= dx * diff * 0.5;
            nodes[i].p.1 -= dy * diff * 0.5;
            nodes[i - 1].p.0 += dx * diff * 0.5;
            nodes[i - 1].p.1 += dy * diff * 0.5;
        }
    }
}

fn pull(nodes: &mut [Node], rows: &[f32], a: (f32, f32), s: f32, k: f32, up: bool) {
    let sign = if up { -1.0 } else { 1.0 };
    for i in 0..nodes.len() {
        let parent = if i == 0 { a } else { nodes[i - 1].p };
        let dir = if up || i == 0 {
            (0.0, sign)
        } else {
            let gp = if i == 1 { a } else { nodes[i - 2].p };
            let (dx, dy) = (parent.0 - gp.0, parent.1 - gp.1);
            let d = (dx * dx + dy * dy).sqrt().max(1e-6);
            (dx / d, dy / d)
        };
        let l = seg_len(rows, i, s);
        let t = (parent.0 + dir.0 * l, parent.1 + dir.1 * l);
        nodes[i].p.0 += (t.0 - nodes[i].p.0) * k;
        nodes[i].p.1 += (t.1 - nodes[i].p.1) * k;
    }
}

fn clamp_angles(nodes: &mut [Node], rows: &[f32], a: (f32, f32), s: f32, swing: f32, bend: f32, up: bool) {
    let mut prev_ang = 0.0f32;
    for i in 0..nodes.len() {
        let parent = if i == 0 { a } else { nodes[i - 1].p };
        let (dx, dy) = (nodes[i].p.0 - parent.0, nodes[i].p.1 - parent.1);
        let mut ang = if up { dx.atan2(-dy) } else { dx.atan2(dy) };
        if !ang.is_finite() {
            ang = 0.0;
        }
        if i > 0 {
            ang = ang.clamp(prev_ang - bend, prev_ang + bend);
        }
        ang = ang.clamp(-swing, swing);
        let l = seg_len(rows, i, s);
        let (sx, sy) = (ang.sin() * l, ang.cos() * l * if up { -1.0 } else { 1.0 });
        nodes[i].p = (parent.0 + sx, parent.1 + sy);
        prev_ang = ang;
    }
}

// ---- squash spring -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquashAnim {
    pub s: f32,
    pub v: f32,
    accum: f32,
    z: f32,
}

/// The landing squash for an impact speed (logical px/s), depth scaled by `strength`.
pub fn squash_spring(impact: f32, strength: Strength) -> SquashAnim {
    let k = strength.k();
    let depth = (0.05 + fin(impact).abs() / 2600.0 * 0.17).clamp(0.05, 0.22) * k;
    SquashAnim { s: 1.0 - depth, v: 0.0, accum: 0.0, z: (SquashAnim::Z * strength.damp()).min(1.0) }
}

impl SquashAnim {
    const W: f32 = std::f32::consts::TAU * 4.5;
    const Z: f32 = 0.38;

    pub fn step(&mut self, dt: f32) {
        self.accum += sane_dt(dt);
        while self.accum >= H {
            self.accum -= H;
            let f = -Self::W * Self::W * (self.s - 1.0) - 2.0 * self.z * Self::W * self.v;
            self.v += f * H;
            self.s += self.v * H;
        }
    }
    /// (sx, sy) about the pivot.
    pub fn scale(&self) -> (f32, f32) {
        (1.0 + 0.55 * (1.0 - self.s), self.s)
    }
    pub fn done(&self) -> bool {
        (self.s - 1.0).abs() < 0.004 && self.v.abs() < 0.05
    }
}

// ---- pointer estimators -------------------------------------------------------------------

pub const V_FLING: f32 = 900.0;
pub const V_MAX: f32 = 4200.0;
pub const VEL_WINDOW: f32 = 0.070;
pub const SHAKE_WINDOW: f32 = 0.8;
pub const SHAKE_PEAK: f32 = 700.0;

/// Pointer samples → release velocity (least squares over the last `VEL_WINDOW`), speed and
/// the shake detector's reversals.
#[derive(Clone, Debug, Default)]
pub struct Pointer {
    samples: std::collections::VecDeque<(f32, f32, f32)>,
    lobe: [(f32, f32); 2],
    reversals: std::collections::VecDeque<f32>,
}

impl Pointer {
    pub fn push(&mut self, t: f32, x: f32, y: f32) {
        if !(t.is_finite() && x.is_finite() && y.is_finite()) {
            return;
        }
        if let Some(&(lt, ..)) = self.samples.back()
            && t < lt
        {
            return;
        }
        self.samples.push_back((t, x, y));
        while self.samples.front().is_some_and(|s| s.0 < t - 1.0) {
            self.samples.pop_front();
        }
        // Shake: per-axis sign changes of a short-window velocity, each lobe peaking >= 700 px/s.
        let v = self.fit(t, 0.03);
        for (ax, &va) in [v.0, v.1].iter().enumerate() {
            let (sign, peak) = self.lobe[ax];
            let sg = if va > 50.0 { 1.0 } else if va < -50.0 { -1.0 } else { 0.0 };
            if sg != 0.0 && sign != 0.0 && sg != sign {
                if peak >= SHAKE_PEAK {
                    self.reversals.push_back(t);
                }
                self.lobe[ax] = (sg, va.abs());
            } else if sg != 0.0 {
                self.lobe[ax] = (sg, peak.max(va.abs()));
            }
        }
        while self.reversals.front().is_some_and(|&r| r < t - SHAKE_WINDOW) {
            self.reversals.pop_front();
        }
    }

    /// Least-squares velocity over `[t - window, t]`, the pointer resting at its last sample
    /// at `t`. Fewer than two samples there: 0.
    fn fit(&self, t: f32, window: f32) -> (f32, f32) {
        let mut pts: Vec<(f32, f32, f32)> = self.samples.iter().copied().filter(|s| s.0 >= t - window - 1e-6).collect();
        if let Some(&(lt, lx, ly)) = self.samples.back()
            && lt < t - 1e-6
        {
            pts.push((t, lx, ly));
        }
        if pts.len() < 2 {
            return (0.0, 0.0);
        }
        let n = pts.len() as f32;
        let mt = pts.iter().map(|p| p.0).sum::<f32>() / n;
        let mx = pts.iter().map(|p| p.1).sum::<f32>() / n;
        let my = pts.iter().map(|p| p.2).sum::<f32>() / n;
        let stt: f32 = pts.iter().map(|p| (p.0 - mt).powi(2)).sum();
        if stt < 1e-9 {
            return (0.0, 0.0);
        }
        let sx: f32 = pts.iter().map(|p| (p.0 - mt) * (p.1 - mx)).sum();
        let sy: f32 = pts.iter().map(|p| (p.0 - mt) * (p.2 - my)).sum();
        (sx / stt, sy / stt)
    }

    pub fn last(&self) -> Option<(f32, f32)> {
        self.samples.back().map(|s| (s.1, s.2))
    }

    /// The release velocity at `t`, clamped to `V_MAX`.
    pub fn velocity(&self, t: f32) -> (f32, f32) {
        let (vx, vy) = self.fit(t, VEL_WINDOW);
        let sp = (vx * vx + vy * vy).sqrt();
        if sp > V_MAX { (vx * V_MAX / sp, vy * V_MAX / sp) } else { (vx, vy) }
    }

    pub fn speed(&self, t: f32) -> f32 {
        let (vx, vy) = self.velocity(t);
        (vx * vx + vy * vy).sqrt()
    }

    /// Reversals in the last `SHAKE_WINDOW`, and seconds since the last one.
    pub fn reversals(&self, t: f32) -> (usize, Option<f32>) {
        (self.reversals.iter().filter(|&&r| r >= t - SHAKE_WINDOW).count(), self.reversals.back().map(|&r| t - r))
    }
}

// ---- flight -----------------------------------------------------------------------------

pub const G_FLIGHT: f32 = 4600.0;
/// Horizontal only: vertical drag would bend the bounce apex ratio off E_FLOOR² (test 7).
pub const AIR_DRAG: f32 = 0.25;
pub const E_FLOOR: f32 = 0.36;
pub const E_WALL: f32 = 0.55;
pub const E_CEIL: f32 = 0.40;
pub const FLOOR_FRICTION: f32 = 0.72;
pub const LAND_VY: f32 = 300.0;
pub const MAX_BOUNCES: u32 = 3;
pub const HARD_VY: f32 = 1900.0;
pub const HARD_WALL: f32 = 2600.0;
pub const LAND_SLIDE: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Walls {
    pub left: Option<i32>,
    pub right: Option<i32>,
    pub top: Option<i32>,
    pub floor: Option<i32>,
}

/// The work areas she flies in (root device px); the floor sits `floor_pad` above an area's bottom.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Arena {
    pub areas: Vec<WinRect>,
    pub floor_pad: i32,
}

impl Arena {
    fn inside(&self, x: i32, y: i32) -> bool {
        self.areas.iter().any(|a| a.contains(x, y))
    }

    /// The walls around a body box (x0, y0, x1, y1): a side is open where a neighbouring area
    /// holds the point just past it at the feet row; no floor/ceiling between stacked areas.
    pub fn walls_at(&self, body: (i32, i32, i32, i32)) -> Walls {
        let (x0, y0, x1, y1) = body;
        let cx = (x0 + x1) / 2;
        let feet = y1 - 1;
        let dist = |a: &&WinRect| {
            let dx = (a.x - cx).max(0).max(cx - (a.right() - 1)) as i64;
            let dy = (a.y - feet).max(0).max(feet - (a.bottom() - 1)) as i64;
            dx * dx + dy * dy
        };
        let _ = y0;
        let Some(a) = self.areas.iter().find(|a| a.contains(cx, feet)).or_else(|| self.areas.iter().min_by_key(dist)) else {
            return Walls::default();
        };
        let left = (!self.inside(a.x - 1, feet)).then_some(a.x);
        let right = (!self.inside(a.right(), feet)).then_some(a.right());
        let top = (!self.inside(cx, a.y - 1)).then_some(a.y);
        let floor = (!self.inside(cx, a.bottom())).then_some(a.bottom() - self.floor_pad);
        Walls { left, right, top, floor }
    }
}

/// Body extents relative to the flight position (the feet), device px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyExtents {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Contact {
    None,
    /// `normal` points away from the surface; `speed` along it before the bounce.
    Bounce { normal: (f32, f32), speed: f32 },
    Floor { speed: f32 },
    Perch { win: u32, x: f32, speed: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    pub pos: (f32, f32),
    pub vel: (f32, f32),
    pub bounces: u32,
    pub landed: bool,
    accum: f32,
}

impl Flight {
    pub fn new(pos: (f32, f32), vel: (f32, f32)) -> Flight {
        let sp = (vel.0 * vel.0 + vel.1 * vel.1).sqrt();
        let vel = if sp > V_MAX { (vel.0 * V_MAX / sp, vel.1 * V_MAX / sp) } else { (fin(vel.0), fin(vel.1)) };
        Flight { pos, vel, bounces: 0, landed: false, accum: 0.0 }
    }

    /// A slow release: half the sideways speed, never upward.
    pub fn drop(pos: (f32, f32), vel: (f32, f32)) -> Flight {
        Flight::new(pos, (0.5 * fin(vel.0), fin(vel.1).max(0.0)))
    }

    /// Advance; the first contact of this step is returned (Floor/Perch: landed, stop).
    pub fn step(&mut self, dt: f32, arena: &Arena, plats: &[Platform], b: BodyExtents) -> Contact {
        if self.landed {
            return Contact::None;
        }
        self.accum += sane_dt(dt);
        let mut out = Contact::None;
        while self.accum >= H - 1e-7 && !self.landed {
            self.accum -= H;
            let c = self.substep(arena, plats, b);
            if c != Contact::None && out == Contact::None {
                out = c;
            }
            if matches!(c, Contact::Floor { .. } | Contact::Perch { .. }) {
                out = c;
            }
        }
        if self.landed {
            self.accum = 0.0;
        }
        out
    }

    fn substep(&mut self, arena: &Arena, plats: &[Platform], b: BodyExtents) -> Contact {
        let (y0, v0) = (self.pos.1, self.vel.1);
        // Exact for constant gravity (no semi-implicit apex bias).
        self.pos.1 += v0 * H + 0.5 * G_FLIGHT * H * H;
        self.vel.1 += G_FLIGHT * H;
        self.vel.0 *= (-AIR_DRAG * H).exp();
        self.pos.0 += self.vel.0 * H;
        let prev_feet = y0;
        let body = (
            (self.pos.0 + b.left).round() as i32,
            (self.pos.1 + b.top).round() as i32,
            (self.pos.0 + b.right).round() as i32,
            (self.pos.1 + b.bottom).round() as i32,
        );
        // Perches: crossing the edge line going down, the seat over the exposed stretch.
        if self.vel.1 > 0.0 {
            for p in plats {
                let y = p.y as f32;
                if prev_feet < y && self.pos.1 >= y {
                    let (lo, hi) = (p.seat_x.0 as f32, p.seat_x.1 as f32);
                    if self.pos.0 >= lo - LAND_SLIDE && self.pos.0 <= hi + LAND_SLIDE {
                        let x = self.pos.0.clamp(lo, hi);
                        let speed = self.vel.1;
                        self.pos = (x, y);
                        self.vel = (0.0, 0.0);
                        self.landed = true;
                        return Contact::Perch { win: p.win, x, speed };
                    }
                }
            }
        }
        let w = arena.walls_at(body);
        if let Some(f) = w.floor {
            let fy = f as f32 - b.bottom;
            if self.pos.1 >= fy && self.vel.1 > 0.0 {
                // The moment of impact inside this substep, then the rest of it after the bounce.
                let g = G_FLIGHT;
                let disc = (v0 * v0 + 2.0 * g * (fy - y0).max(0.0)).max(0.0);
                let tau = ((-v0 + disc.sqrt()) / g).clamp(0.0, H);
                let speed = v0 + g * tau;
                self.bounces += 1;
                let up = speed * E_FLOOR;
                if up < LAND_VY || self.bounces > MAX_BOUNCES {
                    self.pos.1 = fy;
                    self.vel = (0.0, 0.0);
                    self.landed = true;
                    return Contact::Floor { speed };
                }
                let rest = H - tau;
                self.pos.1 = (fy - up * rest + 0.5 * g * rest * rest).min(fy);
                self.vel = (self.vel.0 * FLOOR_FRICTION, -up + g * rest);
                return Contact::Bounce { normal: (0.0, -1.0), speed };
            }
        }
        if let Some(t) = w.top {
            let ty = t as f32 - b.top;
            if self.pos.1 < ty && self.vel.1 < 0.0 {
                let speed = -self.vel.1;
                self.pos.1 = ty;
                self.vel.1 = speed * E_CEIL;
                return Contact::Bounce { normal: (0.0, 1.0), speed };
            }
        }
        if let Some(l) = w.left {
            let lx = l as f32 - b.left;
            if self.pos.0 < lx && self.vel.0 < 0.0 {
                let speed = -self.vel.0;
                self.pos.0 = lx;
                self.vel.0 = speed * E_WALL;
                return Contact::Bounce { normal: (1.0, 0.0), speed };
            }
        }
        if let Some(r) = w.right {
            let rx = r as f32 - b.right;
            if self.pos.0 > rx && self.vel.0 > 0.0 {
                let speed = self.vel.0;
                self.pos.0 = rx;
                self.vel.0 = -speed * E_WALL;
                return Contact::Bounce { normal: (-1.0, 0.0), speed };
            }
        }
        Contact::None
    }

    /// Flight stretch on the dominant axis: 1 + |v| / 12000, at most 1.10, scaled by strength.
    pub fn stretch(&self, strength: Strength) -> f32 {
        1.0 + ((self.vel.0 * self.vel.0 + self.vel.1 * self.vel.1).sqrt() / 12_000.0).clamp(0.0, 0.10) * strength.k()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feet x relative to the hand, 240 Hz, until asleep (or 30 s): a kick at rest, then a
    /// 260 px drag in 0.3 s that stops.
    fn traces(p: &Params) -> (Vec<f32>, Vec<f32>) {
        let mut c = Chain::new(p, &[], &Chain::rows(p.links_down, 290, false), (500.0, 100.0));
        c.kick((120.0, 0.0));
        let mut kick = Vec::new();
        while !c.asleep() && kick.len() < 240 * 30 {
            c.step((500.0, 100.0), H);
            kick.push(c.feet_offset().0);
        }
        let mut c = Chain::new(p, &[], &Chain::rows(p.links_down, 290, false), (500.0, 100.0));
        let mut drag = Vec::new();
        for i in 0..240 * 6 {
            let t = i as f32 * H;
            c.step((500.0 + 260.0 * (t / 0.3).min(1.0), 100.0), H);
            drag.push(c.feet_offset().0);
        }
        (kick, drag)
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0].signum() != w[1].signum() && w[0].abs().max(w[1].abs()) > 1.0).count()
    }

    #[test]
    fn strength_halves_the_bend_and_settles_faster() {
        let full = Strength(1.0).params();
        let half = Strength(0.5).params();
        assert_eq!(full, Params::default(), "1 = the old values");
        assert_eq!(half.max_swing_deg, 16.0);
        assert_eq!(half.max_bend_deg, 7.0);
        assert!((half.stretch.0 - 0.96).abs() < 1e-6 && (half.stretch.1 - 1.04).abs() < 1e-6, "±4 %");
        let ((k1, d1), (k2, d2)) = (traces(&full), traces(&half));
        let secs = |x: &[f32]| x.len() as f32 * H;
        // After the drag stops (0.3 s): when the feet are last more than 2 px off.
        let rest = |x: &[f32]| x.iter().rposition(|v| v.abs() > 2.0).map_or(0.0, |i| i as f32 * H) - 0.3;
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let (st1, st2) = (secs(&k1), secs(&k2));
        let (r1, r2) = (rest(&d1), rest(&d2));
        let (c1, c2) = (crossings(&k1), crossings(&k2));
        let (p1, p2) = (peak(&d1), peak(&d2));
        assert!(st2 <= 0.5 * st1 && r2 <= 0.5 * r1, "settles >= 2x faster: kick {st1:.2} → {st2:.2} s, drag {r1:.2} → {r2:.2} s");
        assert!(c2 * 3 <= c1, "overshoots less: {c1} → {c2} crossings");
        assert!(p2 <= 0.6 * p1, "about half the swing: {p1:.0} → {p2:.0} px");
        // A shake (±150 px at 5 Hz): the feet lag the hand about half as much.
        let lag = |p: &Params| {
            let mut c = Chain::new(p, &[], &Chain::rows(p.links_down, 290, false), (500.0, 100.0));
            let mut m = 0.0f32;
            for i in 0..72 {
                let t = i as f32 / 60.0;
                c.step((500.0 + 150.0 * (t * std::f32::consts::TAU * 5.0).sin(), 100.0), 1.0 / 60.0);
                m = m.max(c.feet_offset().0.abs());
            }
            m
        };
        let (l1, l2) = (lag(&full), lag(&half));
        assert!(l2 <= 0.6 * l1, "a calmer shake: {l1:.0} → {l2:.0} px");
        eprintln!(
            "strength 1 → 0.5: drag swing {p1:.0} → {p2:.0} px; at rest after the stop {r1:.2} → {r2:.2} s; kick settle {st1:.2} → {st2:.2} s, \
             {c1} → {c2} crossings; shake lag {l1:.0} → {l2:.0} px"
        );
    }

    #[test]
    fn strength_zero_is_rigid() {
        let p = Strength(0.0).params();
        assert_eq!((p.max_swing_deg, p.max_bend_deg, p.follow), (0.0, 0.0, 1.0));
        let mut c = Chain::new(&p, &[], &Chain::rows(p.links_down, 290, false), (500.0, 100.0));
        for i in 0..60 {
            c.step((500.0 + 8.0 * i as f32, 100.0), 1.0 / 60.0);
        }
        assert!(c.feet_offset().0.abs() < 1.0, "the feet stay under the hand: {:?}", c.feet_offset());
    }

    fn chain(p: &Params) -> Chain {
        Chain::new(p, &[], &Chain::rows(p.links_down, 290, false), (500.0, 100.0))
    }

    fn tilt(c: &mut Chain, deg: f32) {
        let a = c.anchor;
        let (s, co) = deg.to_radians().sin_cos();
        for (n, &r) in c.down.iter_mut().zip(c.rows_down.iter()) {
            n.p = (a.0 + r * s, a.1 + r * co);
            n.prev = n.p;
        }
    }

    #[test]
    fn decays_to_rest() {
        let p = Params::default();
        let mut c = chain(&p);
        tilt(&mut c, 30.0);
        let mut windows = Vec::new();
        let mut wmax = 0.0f32;
        for k in 1..=(6 * 60) {
            c.step((500.0, 100.0), 1.0 / 60.0);
            wmax = wmax.max(c.offsets().0);
            if k % 30 == 0 {
                windows.push(wmax);
                wmax = 0.0;
            }
        }
        // Monotone envelope: no half-second window swings wider than the second before it.
        for i in 2..windows.len() {
            assert!(windows[i] <= windows[i - 1].max(windows[i - 2]) + 0.05, "{windows:?}");
        }
        let (off, _) = c.offsets();
        assert!(off < 0.5, "offset {off}");
        assert!(c.asleep());
    }

    #[test]
    fn no_nan_any_dt() {
        let p = Params { links_up: 4, ..Params::default() };
        let mut c = Chain::new(&p, &Chain::rows(4, 80, true), &Chain::rows(8, 290, false), (0.0, 0.0));
        let dts = [f32::NAN, -1.0, 0.0, 1e-9, 1e-4, H, 1.0 / 60.0, 0.1, 1.0, 1e6];
        let jumps = [(0.0, 0.0), (1e6, 0.0), (-1e6, 1e6), (f32::NAN, 3.0), (0.0, f32::INFINITY), (5.0, -1e6)];
        for (i, &dt) in dts.iter().enumerate() {
            for &j in &jumps {
                c.step(j, dt);
                c.kick((f32::NAN, 1e9));
                let ok = c.down.iter().chain(c.up.iter()).all(|n| n.p.0.is_finite() && n.p.1.is_finite());
                assert!(ok && c.s.is_finite(), "dt #{i}");
                for b in c.bands(4, 0, 390, 120) {
                    assert!(b.0 >= 0 && b.1 <= 390);
                }
            }
        }
    }

    #[test]
    fn angle_clamp() {
        let p = Params::default();
        let mut c = chain(&p);
        let mut x = 500.0;
        let check = |c: &Chain| {
            let (down, _) = c.segment_angles();
            let mut prev = 0.0f32;
            for (i, a) in down.iter().enumerate() {
                assert!(a.abs().to_degrees() <= p.max_swing_deg + 0.5, "swing {}", a.to_degrees());
                if i > 0 {
                    assert!((a - prev).abs().to_degrees() <= p.max_bend_deg + 0.5, "bend {}", (a - prev).to_degrees());
                }
                prev = *a;
            }
            assert!(c.s >= p.stretch.0 && c.s <= p.stretch.1);
        };
        for dir in [1.0, -1.0] {
            for _ in 0..18 {
                x += dir * 50_000.0 / 60.0;
                c.step((x, 100.0 + dir * 30.0), 1.0 / 60.0);
                check(&c);
            }
        }
    }

    #[test]
    fn deterministic() {
        let p = Params::default();
        let feed = |c: &mut Chain| {
            for k in 0..300 {
                let t = k as f32 / 60.0;
                c.step((500.0 + 300.0 * (t * 3.0).sin(), 100.0 + 80.0 * (t * 5.0).cos()), 1.0 / 60.0);
            }
        };
        let (mut a, mut b) = (chain(&p), chain(&p));
        feed(&mut a);
        feed(&mut b);
        let bits = |c: &Chain| c.down.iter().flat_map(|n| [n.p.0.to_bits(), n.p.1.to_bits(), n.prev.0.to_bits(), n.prev.1.to_bits()]).collect::<Vec<_>>();
        assert_eq!(bits(&a), bits(&b));
        assert_eq!(a.s.to_bits(), b.s.to_bits());
    }

    #[test]
    fn chunking_tolerance() {
        let p = Params::default();
        let (mut a, mut b) = (chain(&p), chain(&p));
        tilt(&mut a, 20.0);
        tilt(&mut b, 20.0);
        a.step((530.0, 100.0), 1.0 / 20.0);
        for k in 1..=3 {
            b.step((500.0 + 10.0 * k as f32, 100.0), 1.0 / 60.0);
        }
        for (x, y) in a.down.iter().zip(b.down.iter()) {
            let d = ((x.p.0 - y.p.0).powi(2) + (x.p.1 - y.p.1).powi(2)).sqrt();
            assert!(d < 0.5, "{d}");
        }
    }

    #[test]
    fn bands_tile() {
        let p = Params { links_up: 4, ..Params::default() };
        let mut rng = crate::anim::Rng::new(5);
        for k in 0..200 {
            let grip = 40 + rng.pick(300) as i32;
            let (top, bottom) = (6, 386);
            let mut c = Chain::new(&p, &Chain::rows(4, grip - top, true), &Chain::rows(8, bottom - grip, false), (0.0, 0.0));
            for _ in 0..rng.pick(40) {
                c.step((rng.range(-300.0, 300.0), rng.range(-200.0, 200.0)), rng.range(0.0, 0.05));
            }
            let bands = c.bands(4, top, bottom, grip);
            assert!(!bands.is_empty());
            let gb = bands.iter().find(|b| b.0 <= grip && grip < b.1).expect("grip band");
            assert_eq!((gb.2, gb.3), (0, 0), "#{k}");
            for w in bands.windows(2) {
                assert_eq!(w[0].1 + w[0].3, w[1].0 + w[1].3, "#{k}: dest bands contiguous {bands:?}");
            }
            for b in &bands {
                assert!(b.0 >= top && b.1 <= bottom && b.1 > b.0, "#{k} {b:?}");
            }
            // Covers the deformed span from the top row's dest to the bottom row's.
            let first = bands[0];
            let last = bands[bands.len() - 1];
            assert!(first.0 + first.3 <= grip && last.1 + last.3 > grip);
            let src = c.source_of((10, grip + 3), &bands).unwrap();
            assert_eq!(src.1, grip + 3, "the grip band maps 1:1");
        }
    }

    #[test]
    fn release_velocity() {
        let mut p = Pointer::default();
        for k in 0..30 {
            let t = k as f32 * 0.008;
            p.push(t, 1500.0 * t, 100.0);
        }
        let v = p.speed(29.0 * 0.008);
        assert!((v - 1500.0).abs() < 45.0, "{v}");
        let mut p = Pointer::default();
        for k in 0..30 {
            let t = k as f32 * 0.008;
            p.push(t, 1500.0 * t, 100.0);
        }
        let v = p.speed(29.0 * 0.008 + 0.080);
        assert!(v < 50.0, "{v}");
    }

    fn arena1() -> Arena {
        Arena { areas: vec![WinRect::new(0, 29, 2560, 1571)], floor_pad: 0 }
    }

    const B: BodyExtents = BodyExtents { left: -100.0, right: 100.0, top: -350.0, bottom: 0.0 };

    #[test]
    fn flight_bounce() {
        let a = arena1();
        let floor = 1600.0;
        let mut f = Flight::new((1000.0, floor - 600.0), (0.0, 0.0));
        let mut apexes = vec![600.0f32];
        let mut cur_apex = f32::MAX;
        let mut lowest = 0.0f32;
        let mut contacts = Vec::new();
        for _ in 0..2000 {
            let c = f.step(1.0 / 240.0, &a, &[], B);
            cur_apex = cur_apex.min(f.pos.1);
            lowest = lowest.max(f.pos.1);
            if let Contact::Bounce { .. } = c {
                if cur_apex < floor - 0.5 && !apexes.is_empty() && !contacts.is_empty() {
                    apexes.push(floor - cur_apex);
                }
                contacts.push(c);
                cur_apex = f32::MAX;
            }
            if f.landed {
                if !contacts.is_empty() && cur_apex < floor - 0.5 {
                    apexes.push(floor - cur_apex);
                }
                break;
            }
        }
        assert!(f.landed && f.bounces <= MAX_BOUNCES + 1);
        assert!(lowest <= floor + 1e-3, "never below the floor");
        assert!(apexes.len() >= 3, "{apexes:?}");
        for w in apexes.windows(2) {
            let r = w[1] / w[0];
            assert!((r / (E_FLOOR * E_FLOOR) - 1.0).abs() < 0.05, "ratio {r} {apexes:?}");
        }
    }

    #[test]
    fn arena_seams() {
        // Side by side, unequal heights.
        let ar = Arena { areas: vec![WinRect::new(0, 29, 2560, 1571), WinRect::new(2560, 29, 2560, 1411)], floor_pad: 0 };
        let w = ar.walls_at((2400, 1000, 2550, 1400));
        assert_eq!(w.right, None, "open seam where her feet fit");
        let w = ar.walls_at((2400, 1200, 2550, 1500));
        assert_eq!(w.right, Some(2560), "below the shorter floor: a wall");
        let w = ar.walls_at((2600, 1000, 2700, 1400));
        assert_eq!((w.left, w.floor), (None, Some(1440)));
        // Stacked: no floor between them, no ceiling under.
        let st = Arena { areas: vec![WinRect::new(0, 0, 1920, 1080), WinRect::new(0, 1080, 1920, 1080)], floor_pad: 0 };
        assert_eq!(st.walls_at((900, 700, 1000, 1000)).floor, None);
        assert_eq!(st.walls_at((900, 1200, 1000, 1500)).top, None);
        assert_eq!(st.walls_at((900, 1200, 1000, 1500)).floor, Some(2160));
    }

    #[test]
    fn flight_platforms() {
        let a = arena1();
        let plats = [Platform { win: 7, y: 800, seat_x: (900, 1100) }];
        let mut f = Flight::new((1110.0, 500.0), (0.0, 0.0));
        let mut c = Contact::None;
        for _ in 0..600 {
            c = f.step(1.0 / 60.0, &a, &plats, B);
            if f.landed {
                break;
            }
        }
        assert!(matches!(c, Contact::Perch { win: 7, x, .. } if x == 1100.0), "{c:?}");
        // Outside the slide: passes by to the floor.
        let mut f = Flight::new((1200.0, 500.0), (0.0, 0.0));
        let mut c = Contact::None;
        for _ in 0..600 {
            let k = f.step(1.0 / 60.0, &a, &plats, B);
            if k != Contact::None {
                c = k;
            }
            if f.landed {
                break;
            }
        }
        assert!(matches!(c, Contact::Floor { .. }), "{c:?}");
        // Going up through it: passes.
        let mut f = Flight::new((1000.0, 1000.0), (0.0, -2500.0));
        for _ in 0..20 {
            let k = f.step(1.0 / 60.0, &a, &plats, B);
            assert!(!matches!(k, Contact::Perch { .. }));
        }
        assert!(f.pos.1 < 800.0);
    }

    #[test]
    fn squash_settles() {
        let mut s = squash_spring(1500.0, Strength(1.0));
        assert!(s.scale().1 < 0.92 && s.scale().0 > 1.0);
        let mut t = 0.0;
        while !s.done() && t < 2.0 {
            s.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        assert!((0.3..0.7).contains(&t), "{t}");
        assert_eq!(squash_spring(f32::NAN, Strength(1.0)).scale().1, 0.95);
        assert_eq!(squash_spring(1e9, Strength(1.0)).scale().1, 0.78);
        assert!((squash_spring(1e9, Strength(0.5)).scale().1 - 0.89).abs() < 1e-6, "half the squash at 0.5");
    }
}
