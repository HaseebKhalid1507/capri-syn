//! Where she goes: a brain `Goal` + the world → a `Spot`, and the hard rules (`guard`). Pure:
//! the view snapshots the world (stack, focus, work areas, cursor) at decision time.
//!
//! Units: root device px. Body boxes come from the frames' masks, relative to the pose's
//! anchor (feet, seat, grip), so a spot's `body` is exactly what will cover the screen.

use crate::anim::Rng;
use crate::brain::{Approach, Goal, Why};
use crate::edges::{self, WinRect};

/// The pointer is never covered: the body box clears it by this (logical px).
pub const CURSOR_CLEAR: f32 = 64.0;
/// A passive teleport goes at least this far (logical px); closer spots are skipped.
pub const MIN_TELEPORT: f32 = 300.0;
/// `Passive { far }`: this far from the cursor at least (logical px).
pub const FAR_PX: f32 = 900.0;
/// Floating spots clear every window by this (logical px).
pub const FLOAT_CLEAR: f32 = 24.0;
/// Peek only when the cursor is this far from her (logical px).
pub const PEEK_CURSOR: f32 = 600.0;
/// Float beside: within this of the focused window's side (logical px).
pub const BESIDE_MAX: f32 = 200.0;

/// Body boxes (relative to each pose's anchor point) of every pose she can be placed in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Boxes {
    /// Standing (feet).
    pub stand: WinRect,
    /// On an edge (seat).
    pub sit: WinRect,
    /// Sitting on the floor (feet).
    pub floor_sit: WinRect,
    /// Hanging by the hands (grip) / by the feet (grip).
    pub hang: Option<WinRect>,
    pub hang_ud: Option<WinRect>,
    /// Gripping a vertical edge on her left (body to the right) / on her right.
    pub cling_l: Option<WinRect>,
    pub cling_r: Option<WinRect>,
    /// Crouched (feet).
    pub peek: Option<WinRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotKind {
    /// Sitting on a window's top edge (passive).
    Edge { win: u32 },
    Floor,
    FloorSit,
    Float,
    Hang { upside_down: bool },
    /// `left`: the edge is on her left (the window's left side).
    Cling { left: bool },
    SitEdge { win: u32 },
    Peek,
    /// The floor beside his window (or the next monitor's).
    Beside,
    FloatBeside,
    InPlace,
}

impl SpotKind {
    pub fn key(&self) -> &'static str {
        match self {
            SpotKind::Edge { .. } => "edge",
            SpotKind::Floor => "floor",
            SpotKind::FloorSit => "floor_sit",
            SpotKind::Float => "float",
            SpotKind::Hang { upside_down: false } => "hang",
            SpotKind::Hang { upside_down: true } => "hang_upside_down",
            SpotKind::Cling { .. } => "cling",
            SpotKind::SitEdge { .. } => "sit_edge",
            SpotKind::Peek => "peek",
            SpotKind::Beside => "beside",
            SpotKind::FloatBeside => "float_beside",
            SpotKind::InPlace => "in_place",
        }
    }
    /// The passive kind family (the last one isn't repeated).
    fn family(&self) -> u8 {
        match self {
            SpotKind::Edge { .. } => 0,
            SpotKind::Floor => 1,
            SpotKind::FloorSit => 2,
            SpotKind::Float => 3,
            _ => 9,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub kind: SpotKind,
    /// Root px of the pose's anchor: feet, seat or grip.
    pub anchor: (i32, i32),
    /// Root px body box at that anchor.
    pub body: WinRect,
    /// Index into `World::areas`.
    pub area: usize,
    pub approach: Option<Approach>,
    /// Share of the focused window covered, %.
    pub cover_pct: f64,
}

impl Spot {
    pub fn covers_focus(&self) -> bool {
        self.cover_pct > 0.0
    }
    pub fn win(&self) -> Option<u32> {
        match self.kind {
            SpotKind::Edge { win } | SpotKind::SitEdge { win } => Some(win),
            _ => None,
        }
    }
}

/// The world at decision time.
#[derive(Clone, Debug)]
pub struct World<'a> {
    /// Bottom → top, every viewable window (occluders too).
    pub stack: &'a [WinRect],
    /// Windows she may use (sit on, keep clear of): big, managed, inside the test arena.
    pub eligible: &'a [bool],
    pub focused: Option<WinRect>,
    /// Work areas (monitor minus docks) and their monitors, same order.
    pub areas: &'a [WinRect],
    pub monitors: &'a [WinRect],
    /// A fullscreen window on that monitor.
    pub fullscreen: &'a [bool],
    pub cursor: (i32, i32),
    /// Where she is now.
    pub her: Spot,
    pub boxes: Boxes,
    /// Edge contact extent relative to the seat, and EDGE_MARGIN, device px.
    pub contact: (i32, i32),
    pub margin: i32,
    /// Feet stand this far above an area's bottom (foot pad + ground room).
    pub floor_pad: i32,
    pub sf: f32,
    pub cover_max_pct: f64,
    /// Windows she sat on lately (not again).
    pub last_wins: &'a [u32],
    pub last_kind: Option<SpotKind>,
    /// Summons: room the HUD needs above her body (prompt panel, reply).
    pub hud_h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Fullscreen,
    Cursor,
    FocusedInterior,
    /// Covers more than `cover_max_pct` of his window, or not at its far corner.
    Cover,
    Offscreen,
    /// No window, no pose, no room: nothing to try.
    NoSpot,
    /// The HUD (prompt) wouldn't fit above her.
    NoHudRoom,
}

impl World<'_> {
    fn d(&self, logical: f32) -> i32 {
        (logical * self.sf).round() as i32
    }

    pub fn area_of(&self, x: i32, y: i32) -> usize {
        self.areas
            .iter()
            .position(|a| a.contains(x, y))
            .or_else(|| self.monitors.iter().position(|m| m.contains(x, y)))
            .unwrap_or_else(|| (0..self.areas.len()).min_by_key(|&i| dist2(&self.areas[i], x, y)).unwrap_or(0))
    }

    fn focused_fullscreen(&self) -> bool {
        self.focused.is_some_and(|f| self.monitors.iter().any(|m| m.x == f.x && m.y == f.y && m.w == f.w && m.h == f.h))
    }

    fn floor_y(&self, area: usize) -> i32 {
        self.areas[area].bottom() - self.floor_pad
    }

    fn spot(&self, kind: SpotKind, anchor: (i32, i32), rel: WinRect, approach: Option<Approach>) -> Spot {
        let body = WinRect::new(anchor.0 + rel.x, anchor.1 + rel.y, rel.w, rel.h);
        let area = self.area_of(anchor.0, anchor.1.max(body.y));
        let cover_pct = self
            .focused
            .and_then(|f| f.intersect(&body).map(|o| 100.0 * area_of(&o) as f64 / area_of(&f).max(1) as f64))
            .unwrap_or(0.0);
        Spot { kind, anchor, body, area, approach, cover_pct }
    }
}

fn area_of(r: &WinRect) -> i64 {
    r.w.max(0) as i64 * r.h.max(0) as i64
}

fn dist2(r: &WinRect, x: i32, y: i32) -> i64 {
    let dx = (r.x - x).max(0).max(x - (r.right() - 1)) as i64;
    let dy = (r.y - y).max(0).max(y - (r.bottom() - 1)) as i64;
    dx * dx + dy * dy
}

/// The corner of `f` farthest from the cursor: (right side?, bottom side?).
fn far_corner(f: &WinRect, c: (i32, i32)) -> (bool, bool) {
    let (cx, cy) = f.center();
    (c.0 < cx, c.1 < cy)
}

/// The hard rules of §4.3 (+ the §14.1 cover exception) for a spot.
pub fn guard(w: &World, s: &Spot) -> Result<(), Block> {
    // 1. No move while a fullscreen window is on her monitor or the target's, or focused.
    let fs = |i: usize| w.fullscreen.get(i).copied().unwrap_or(false);
    if fs(s.area) || fs(w.her.area) || w.focused_fullscreen() {
        return Err(Block::Fullscreen);
    }
    // On a monitor (hanging hands may reach into the bar; the rest stays in the work area).
    let Some(m) = w.monitors.get(s.area) else { return Err(Block::Offscreen) };
    let a = w.areas[s.area];
    let hanging = matches!(s.kind, SpotKind::Hang { .. } | SpotKind::Cling { .. });
    let top = if hanging { m.y } else { a.y };
    if s.body.x < m.x || s.body.right() > m.right() || s.body.y < top || s.body.bottom() > a.bottom() + w.floor_pad + 2 {
        return Err(Block::Offscreen);
    }
    // 2. The cursor is never covered.
    if s.kind != SpotKind::InPlace && s.body.grow(w.d(CURSOR_CLEAR)).contains(w.cursor.0, w.cursor.1) {
        return Err(Block::Cursor);
    }
    // 3. His window's interior is never covered, with two exceptions.
    if let Some(f) = w.focused
        && let Some(o) = f.intersect(&s.body)
        && s.kind != SpotKind::InPlace
    {
        match (s.kind, s.approach) {
            // Sitting on its top edge: the hanging legs overlap the top strip.
            (SpotKind::SitEdge { win }, _) if win == f.id => {}
            (_, Some(ap)) if ap.covers() => {
                if s.cover_pct > w.cover_max_pct {
                    return Err(Block::Cover);
                }
                // Only at the corner farther from the cursor.
                let (right, bottom) = far_corner(&f, w.cursor);
                let (ox, oy) = o.center();
                let (fx, fy) = f.center();
                let side_ok = if right { ox >= fx } else { ox <= fx };
                let row_ok = match s.kind {
                    SpotKind::Peek => oy >= fy && bottom,
                    SpotKind::Cling { .. } => true,
                    _ => oy <= fy,
                };
                if !side_ok || !row_ok {
                    return Err(Block::Cover);
                }
            }
            _ => return Err(Block::FocusedInterior),
        }
    }
    Ok(())
}

/// Resolve a goal to a spot: the passive kinds, or the approaches in order then the beside
/// chain. Err: why each try failed (for the decision log).
pub fn resolve(goal: &Goal, w: &World, rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    match goal {
        // Far (after a sulk or a fling) if there is such a spot, else just away.
        Goal::Passive { far: true } => passive(w, true, false, rng).or_else(|mut why| {
            passive(w, false, false, rng).map_err(|more| {
                why.extend(more);
                why
            })
        }),
        Goal::Passive { far: false } => passive(w, false, false, rng),
        Goal::Leave => passive(w, false, true, rng),
        Goal::Active { why, approaches } => active(w, *why, approaches, rng),
    }
}

// ---- passive -------------------------------------------------------------------------

fn passive(w: &World, far: bool, leaving: bool, rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    let mut why = Vec::new();
    let kinds: [(u8, f64); 4] = [(0, 0.40), (1, 0.25), (2, 0.20), (3, 0.15)];
    let last = w.last_kind.map(|k| k.family());
    let mut pool: Vec<(u8, f64)> = kinds.iter().copied().filter(|k| Some(k.0) != last).collect();
    if leaving {
        // Off his window to stand on the floor (a sulk or the satisfied beat plays there).
        pool = vec![(1, 1.0)];
    }
    let min_move = if leaving { w.d(80.0) } else { w.d(MIN_TELEPORT) };
    let her_win = w.her.win();
    let ok = |s: &Spot| -> Result<(), Block> {
        guard(w, s)?;
        if s.covers_focus() {
            return Err(Block::FocusedInterior);
        }
        let (dx, dy) = ((s.anchor.0 - w.her.anchor.0) as i64, (s.anchor.1 - w.her.anchor.1) as i64);
        if dx * dx + dy * dy < (min_move as i64).pow(2) {
            return Err(Block::NoSpot);
        }
        if far {
            let d = (dist2(&s.body, w.cursor.0, w.cursor.1) as f64).sqrt();
            if d < w.d(FAR_PX) as f64 || (her_win.is_some() && s.win() == her_win) {
                return Err(Block::Cursor);
            }
        }
        Ok(())
    };
    while !pool.is_empty() {
        let total: f64 = pool.iter().map(|p| p.1).sum();
        let mut r = rng.range(0.0, 1.0) as f64 * total;
        let mut k = pool.len() - 1;
        for (i, p) in pool.iter().enumerate() {
            if r < p.1 {
                k = i;
                break;
            }
            r -= p.1;
        }
        let kind = pool.remove(k).0;
        let cands = match kind {
            0 => edge_spots(w, rng),
            1 | 2 => floor_spots(w, kind == 2, rng),
            _ => float_spots(w, rng),
        };
        let mut last_block = Block::NoSpot;
        for s in cands {
            match ok(&s) {
                Ok(()) => return Ok(s),
                Err(b) => last_block = b,
            }
        }
        why.push((["edge", "floor", "floor_sit", "float"][kind as usize].to_string(), last_block));
        if leaving && pool.is_empty() && why.len() == 1 {
            pool = vec![(0, 0.5), (3, 0.5)];
        } else if pool.is_empty()
            && !leaving
            && let Some(l) = last.filter(|_| !why.iter().any(|w| w.0 == "last"))
        {
            // Nothing else fits: the same kind as last time beats not moving at all.
            why.push(("last".into(), Block::NoSpot));
            pool = vec![(l, 1.0)];
        }
    }
    why.retain(|w| w.0 != "last");
    Err(why)
}

/// Sit spots on exposed edges of eligible windows other than the focused one and the last two.
fn edge_spots(w: &World, rng: &mut Rng) -> Vec<Spot> {
    let mut out = Vec::new();
    let mut plats = edges::platforms(w.stack, w.eligible, w.contact, w.margin);
    plats.retain(|p| Some(p.win) != w.focused.map(|f| f.id) && !w.last_wins.contains(&p.win));
    // Shuffle.
    for i in (1..plats.len()).rev() {
        plats.swap(i, rng.pick(i + 1));
    }
    for p in plats {
        for _ in 0..3 {
            let x = p.seat_x.0 + rng.pick((p.seat_x.1 - p.seat_x.0 + 1).max(1) as usize) as i32;
            out.push(w.spot(SpotKind::Edge { win: p.win }, (x, p.y), w.boxes.sit, None));
        }
    }
    out
}

fn floor_spots(w: &World, sit: bool, rng: &mut Rng) -> Vec<Spot> {
    let mut out = Vec::new();
    let rel = if sit { w.boxes.floor_sit } else { w.boxes.stand };
    for _ in 0..16 {
        // Her monitor 70 %, another 30 %.
        let i = if w.areas.len() > 1 && rng.range(0.0, 1.0) >= 0.7 {
            let others: Vec<usize> = (0..w.areas.len()).filter(|&i| i != w.her.area).collect();
            others[rng.pick(others.len())]
        } else {
            w.her.area.min(w.areas.len().saturating_sub(1))
        };
        let a = w.areas[i];
        let x = a.x + (a.w as f32 * rng.range(0.1, 0.9)) as i32;
        out.push(w.spot(if sit { SpotKind::FloorSit } else { SpotKind::Floor }, (x, w.floor_y(i)), rel, None));
    }
    out
}

/// Empty air: 24 seeded samples, clear of every eligible window (24 px) and the cursor.
fn float_spots(w: &World, rng: &mut Rng) -> Vec<Spot> {
    let clear = w.d(FLOAT_CLEAR);
    let mut out = Vec::new();
    for _ in 0..24 {
        let i = w.her.area.min(w.areas.len().saturating_sub(1));
        let i = if w.areas.len() > 1 && rng.range(0.0, 1.0) >= 0.7 { rng.pick(w.areas.len()) } else { i };
        let a = w.areas[i];
        let x = a.x + (a.w as f32 * rng.range(0.1, 0.9)) as i32;
        let body_top = a.y + (a.h as f32 * rng.range(0.15, 0.70)) as i32;
        let anchor = (x, body_top - w.boxes.stand.y);
        let s = w.spot(SpotKind::Float, anchor, w.boxes.stand, None);
        let g = s.body.grow(clear);
        let free = w.stack.iter().zip(w.eligible).all(|(r, &e)| !e || r.intersect(&g).is_none());
        if free && s.body.bottom() < w.floor_y(i) - w.d(40.0) {
            out.push(s);
        }
    }
    out
}

// ---- active -----------------------------------------------------------------------------

fn active(w: &World, why: Why, approaches: &[Approach], rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    let mut tried = Vec::new();
    let Some(f) = w.focused else {
        tried.push(("focus".into(), Block::NoSpot));
        return in_place(w, why).ok_or(tried);
    };
    let summon = matches!(why, Why::Summon(_));
    let hud_ok = |s: &Spot| !summon || s.body.y - w.hud_h >= w.areas[s.area].y;
    for &ap in approaches {
        if ap == Approach::Beside {
            continue;
        }
        let cands = match ap {
            Approach::Hang => hang_spots(w, &f, false),
            Approach::HangUpsideDown => hang_spots(w, &f, true),
            Approach::Cling => cling_spots(w, &f),
            Approach::SitEdge => sit_edge_spots(w, &f),
            Approach::Peek => peek_spots(w, &f),
            Approach::Beside => Vec::new(),
        };
        let mut block = if cands.is_empty() { Block::NoSpot } else { Block::Offscreen };
        let mut found = None;
        for s in cands {
            if !hud_ok(&s) {
                block = Block::NoHudRoom;
                continue;
            }
            match guard(w, &s) {
                Ok(()) => {
                    found = Some(s);
                    break;
                }
                Err(b) => block = b,
            }
        }
        match found {
            Some(s) => return Ok(s),
            None => tried.push((ap.key().into(), block)),
        }
    }
    // Beside: the floor next to it, the next monitor, floating beside, in place.
    let mut block = Block::NoSpot;
    for s in beside_spots(w, &f).into_iter().chain(float_beside_spots(w, &f, rng)) {
        if !hud_ok(&s) {
            block = Block::NoHudRoom;
            continue;
        }
        match guard(w, &s) {
            Ok(()) => return Ok(s),
            Err(b) => block = b,
        }
    }
    tried.push(("beside".into(), block));
    in_place(w, why).ok_or(tried)
}

/// She stays and bids where she stands, if she is on his monitor.
fn in_place(w: &World, _why: Why) -> Option<Spot> {
    let f = w.focused?;
    let mon = w.area_of(f.center().0, f.center().1);
    (w.her.area == mon).then_some(Spot { kind: SpotKind::InPlace, approach: Some(Approach::Beside), ..w.her })
}

fn hang_spots(w: &World, f: &WinRect, upside_down: bool) -> Vec<Spot> {
    let Some(rel) = (if upside_down { w.boxes.hang_ud } else { w.boxes.hang }) else { return Vec::new() };
    let i = w.area_of(f.center().0, f.y.max(w.areas[w.area_of(f.x, f.y)].y));
    let line = w.areas[i].y;
    let (right, _) = far_corner(f, w.cursor);
    let inset = w.d(16.0);
    let mut out = Vec::new();
    // Over the far corner, then shifted outward so less of his window is covered.
    for shift in [0, rel.w / 4, rel.w / 2, rel.w * 3 / 4] {
        let x = if right { f.right() - inset + shift - (rel.x + rel.w) } else { f.x + inset - shift - rel.x };
        out.push(w.spot(SpotKind::Hang { upside_down }, (x, line), rel, Some(if upside_down { Approach::HangUpsideDown } else { Approach::Hang })));
    }
    out
}

fn cling_spots(w: &World, f: &WinRect) -> Vec<Spot> {
    let (right, _) = far_corner(f, w.cursor);
    // The window's right edge is on her right (pose-dock-right), the left one on her left.
    let Some(rel) = (if right { w.boxes.cling_r } else { w.boxes.cling_l }) else { return Vec::new() };
    let x = if right { f.right() } else { f.x };
    [0.12, 0.2, 0.3]
        .iter()
        .map(|k| w.spot(SpotKind::Cling { left: !right }, (x, f.y + (f.h as f32 * k) as i32), rel, Some(Approach::Cling)))
        .collect()
}

fn sit_edge_spots(w: &World, f: &WinRect) -> Vec<Spot> {
    let Some(i) = w.stack.iter().position(|r| r.id == f.id) else { return Vec::new() };
    let mut xs: Vec<i32> = Vec::new();
    for (lo, hi) in edges::seat_ranges(w.stack, i, w.contact, w.margin) {
        // Nearest the cursor's x, and both ends.
        xs.extend([w.cursor.0.clamp(lo, hi), lo, hi, (lo + hi) / 2]);
    }
    xs.sort_by_key(|&x| (x - w.cursor.0).abs());
    xs.dedup();
    let area = w.areas[w.area_of(f.center().0, f.y)];
    xs.into_iter()
        .map(|x| w.spot(SpotKind::SitEdge { win: f.id }, (x, f.y), w.boxes.sit, Some(Approach::SitEdge)))
        // Head room under the bar.
        .filter(|s| s.body.y >= area.y && f.y > area.y)
        .collect()
}

fn peek_spots(w: &World, f: &WinRect) -> Vec<Spot> {
    let Some(rel) = w.boxes.peek else { return Vec::new() };
    let (right, bottom) = far_corner(f, w.cursor);
    if !bottom {
        return Vec::new();
    }
    let i = w.area_of(f.center().0, f.bottom() - 1);
    let fy = w.floor_y(i);
    let m = w.monitors[i];
    let mut out = Vec::new();
    for k in [0.35f32, 0.25, 0.5, 0.15] {
        let inside = (rel.w as f32 * k) as i32;
        let mut x = if right { f.right() - inside - rel.x } else { f.x + inside - (rel.x + rel.w) };
        // Stay on the monitor (a tile at the screen edge: she peeks in a little more).
        x = x.clamp(m.x - rel.x, m.right() - (rel.x + rel.w));
        let s = w.spot(SpotKind::Peek, (x, fy), rel, Some(Approach::Peek));
        let d = (dist2(&s.body, w.cursor.0, w.cursor.1) as f64).sqrt();
        if d >= w.d(PEEK_CURSOR) as f64 && s.body.intersect(f).is_some() {
            out.push(s);
        }
    }
    out
}

/// The floor just outside his window (the side nearer the cursor first); a window as wide as
/// its monitor: the next monitor's floor, by the shared edge.
fn beside_spots(w: &World, f: &WinRect) -> Vec<Spot> {
    let rel = w.boxes.stand;
    let i = w.area_of(f.center().0, f.center().1);
    let a = w.areas[i];
    let gap = w.d(10.0);
    let mut out = Vec::new();
    let left_first = w.cursor.0 < f.center().0;
    let sides = if left_first { [true, false] } else { [false, true] };
    for left in sides {
        let x = if left { f.x - gap - (rel.x + rel.w) } else { f.right() + gap - rel.x };
        out.push(w.spot(SpotKind::Beside, (x, w.floor_y(i)), rel, Some(Approach::Beside)));
    }
    // Spans its monitor's width: the adjacent monitor, next to the seam.
    if f.w >= a.w - w.d(48.0) {
        for (j, b) in w.areas.iter().enumerate() {
            if j == i {
                continue;
            }
            let seam_right = b.x == w.monitors[i].right();
            let seam_left = b.right() == w.monitors[i].x;
            if seam_right {
                out.push(w.spot(SpotKind::Beside, (b.x + w.d(24.0) - rel.x, w.floor_y(j)), rel, Some(Approach::Beside)));
            } else if seam_left {
                out.push(w.spot(SpotKind::Beside, (b.right() - w.d(24.0) - (rel.x + rel.w), w.floor_y(j)), rel, Some(Approach::Beside)));
            }
        }
    }
    out
}

fn float_beside_spots(w: &World, f: &WinRect, rng: &mut Rng) -> Vec<Spot> {
    let rel = w.boxes.stand;
    let clear = w.d(FLOAT_CLEAR);
    let mut out = Vec::new();
    for _ in 0..12 {
        let gap = clear + rng.pick((w.d(BESIDE_MAX) - clear).max(1) as usize) as i32;
        let left = rng.pick(2) == 0;
        let x = if left { f.x - gap - (rel.x + rel.w) } else { f.right() + gap - rel.x };
        let top = f.y + (f.h as f32 * rng.range(0.0, 0.5)) as i32;
        let s = w.spot(SpotKind::FloatBeside, (x, top - rel.y), rel, Some(Approach::Beside));
        let g = s.body.grow(clear);
        if w.stack.iter().zip(w.eligible).all(|(r, &e)| !e || r.intersect(&g).is_none()) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::SummonKind;

    fn r(id: u32, x: i32, y: i32, w: i32, h: i32) -> WinRect {
        WinRect { id, x, y, w, h }
    }

    // bella-like: two monitors, a 29 px bar on each; sf 1.
    const MONS: [WinRect; 2] = [WinRect { id: 0, x: 0, y: 0, w: 2560, h: 1600 }, WinRect { id: 1, x: 2560, y: 0, w: 2560, h: 1440 }];
    const AREAS: [WinRect; 2] = [WinRect { id: 0, x: 0, y: 29, w: 2560, h: 1571 }, WinRect { id: 1, x: 2560, y: 29, w: 2560, h: 1411 }];

    fn boxes() -> Boxes {
        Boxes {
            stand: r(0, -120, -370, 240, 375),
            sit: r(0, -140, -250, 300, 380),
            floor_sit: r(0, -150, -240, 300, 245),
            hang: Some(r(0, -150, -16, 300, 372)),
            hang_ud: Some(r(0, -150, -10, 300, 320)),
            cling_l: Some(r(0, -10, -130, 300, 372)),
            cling_r: Some(r(0, -290, -130, 300, 372)),
            peek: Some(r(0, -130, -260, 260, 265)),
        }
    }

    struct Fixture {
        stack: Vec<WinRect>,
        eligible: Vec<bool>,
        focused: Option<WinRect>,
        cursor: (i32, i32),
        fs: [bool; 2],
        her: (i32, i32),
    }

    impl Fixture {
        fn world(&self) -> World<'_> {
            let b = boxes();
            let her = Spot {
                kind: SpotKind::Floor,
                anchor: self.her,
                body: b.stand.offset(self.her.0, self.her.1),
                area: if self.her.0 >= 2560 { 1 } else { 0 },
                approach: None,
                cover_pct: 0.0,
            };
            World {
                stack: &self.stack,
                eligible: &self.eligible,
                focused: self.focused,
                areas: &AREAS,
                monitors: &MONS,
                fullscreen: &self.fs,
                cursor: self.cursor,
                her,
                boxes: b,
                contact: (-110, 130),
                margin: 65,
                floor_pad: 12,
                sf: 1.0,
                cover_max_pct: 12.0,
                last_wins: &[],
                last_kind: None,
                hud_h: 0,
            }
        }
    }

    trait Off {
        fn offset(&self, dx: i32, dy: i32) -> WinRect;
    }
    impl Off for WinRect {
        fn offset(&self, dx: i32, dy: i32) -> WinRect {
            WinRect { x: self.x + dx, y: self.y + dy, ..*self }
        }
    }

    /// An i3 desk: two tiles on monitor 0 right under the bar (gaps 12), one window on 1.
    fn tiled() -> Fixture {
        let a = r(1, 12, 41, 1264, 1547);
        let b = r(2, 1284, 41, 1264, 1547);
        let c = r(3, 2700, 300, 1200, 800);
        Fixture { stack: vec![a, b, c], eligible: vec![true; 3], focused: Some(a), cursor: (900, 900), fs: [false; 2], her: (2000, 1588) }
    }

    fn bid(ap: &[Approach]) -> Goal {
        let mut v = ap.to_vec();
        v.push(Approach::Beside);
        Goal::Active { why: Why::Attention, approaches: v }
    }

    #[test]
    fn passive_never_focused() {
        let fx = tiled();
        let w = fx.world();
        let mut rng = Rng::new(1);
        for _ in 0..300 {
            let s = resolve(&Goal::Passive { far: false }, &w, &mut rng).unwrap();
            assert_ne!(s.win(), Some(1), "never on the focused window");
            assert!(s.body.intersect(&fx.focused.unwrap()).is_none(), "{s:?}");
        }
    }

    #[test]
    fn never_covered_edge() {
        // Window 3's edge is fully covered by 4 above it: no edge spot on 3.
        let mut fx = tiled();
        fx.stack.push(r(4, 2650, 250, 1300, 120));
        fx.eligible.push(false);
        let w = fx.world();
        let mut rng = Rng::new(2);
        for _ in 0..300 {
            if let Ok(s) = resolve(&Goal::Passive { far: false }, &w, &mut rng) {
                assert_ne!(s.win(), Some(3));
            }
        }
    }

    #[test]
    fn cursor_clearance() {
        let mut fx = tiled();
        let mut rng = Rng::new(3);
        for k in 0..200 {
            fx.cursor = (rng.pick(5120) as i32, rng.pick(1600) as i32);
            let w = fx.world();
            for g in [Goal::Passive { far: false }, bid(&[Approach::Hang, Approach::Cling, Approach::Peek, Approach::SitEdge])] {
                if let Ok(s) = resolve(&g, &w, &mut rng)
                    && s.kind != SpotKind::InPlace
                {
                    assert!(!s.body.grow(64).contains(fx.cursor.0, fx.cursor.1), "#{k} {s:?} covers {:?}", fx.cursor);
                }
            }
        }
    }

    #[test]
    fn focused_interior_untouched() {
        let fx = tiled();
        let w = fx.world();
        let mut rng = Rng::new(4);
        for _ in 0..100 {
            let s = resolve(&bid(&[]), &w, &mut rng).unwrap();
            assert!(s.kind == SpotKind::InPlace || s.body.intersect(&fx.focused.unwrap()).is_none(), "{s:?}");
        }
        // A spot over his window is refused by the guard unless it's an exception.
        let over = w.spot(SpotKind::Floor, (600, 1588), boxes().stand, None);
        assert_eq!(guard(&w, &over), Err(Block::FocusedInterior));
    }

    #[test]
    fn hang_cover_rules() {
        let fx = tiled();
        let w = fx.world();
        let mut rng = Rng::new(5);
        let s = resolve(&bid(&[Approach::Hang]), &w, &mut rng).unwrap();
        assert_eq!(s.kind, SpotKind::Hang { upside_down: false });
        assert_eq!(s.anchor.1, 29, "the hang line is the bar's bottom edge (the dock strut)");
        assert!(s.cover_pct <= 12.0 && s.cover_pct > 0.0, "{}", s.cover_pct);
        // Cursor at (900, 900) is right of centre: the far corner is the top left.
        assert!(s.body.center().0 < fx.focused.unwrap().center().0);
        // A small window: 12 % is too little for her even shifted out; next approach.
        let mut small = tiled();
        small.stack[0] = r(1, 12, 41, 700, 400);
        small.focused = Some(small.stack[0]);
        small.cursor = (600, 380);
        let w = small.world();
        let s = resolve(&bid(&[Approach::Hang, Approach::Cling]), &w, &mut rng);
        if let Ok(s) = s {
            assert!(s.cover_pct <= 12.0);
        }
        // A spot over the near corner is refused.
        let fx = tiled();
        let w = fx.world();
        let near = w.spot(SpotKind::Hang { upside_down: false }, (1100, 29), boxes().hang.unwrap(), Some(Approach::Hang));
        assert!(near.cover_pct < 12.0);
        assert_eq!(guard(&w, &near), Err(Block::Cover));
    }

    #[test]
    fn focused_edge_legs_exception() {
        // A floating window with head room: sit_edge works, legs over its top strip allowed.
        let mut fx = tiled();
        let f = r(5, 300, 700, 1200, 600);
        fx.stack.push(f);
        fx.eligible.push(true);
        fx.focused = Some(f);
        fx.cursor = (400, 1250);
        let w = fx.world();
        let s = resolve(&bid(&[Approach::SitEdge]), &w, &mut Rng::new(6)).unwrap();
        assert_eq!(s.kind, SpotKind::SitEdge { win: 5 });
        assert!(s.body.intersect(&f).is_some(), "legs hang over the top strip");
        assert!(guard(&w, &s).is_ok());
        // The seat is kept clear of the cursor.
        assert!(!s.body.grow(64).contains(fx.cursor.0, fx.cursor.1));
    }

    #[test]
    fn fallback_chain() {
        let mut rng = Rng::new(7);
        // An i3 tile at the top: no head room for sit_edge -> beside (the next tile's floor? no: his
        // side has the other tile -> the far side is the screen edge) -> in place on his monitor.
        let fx = tiled();
        let w = fx.world();
        let s = resolve(&bid(&[Approach::SitEdge]), &w, &mut rng);
        let s = s.unwrap();
        assert_ne!(s.kind, SpotKind::SitEdge { win: 1 });
        // A floating window with room: sit_edge.
        let mut fx2 = tiled();
        let f = r(5, 300, 700, 1200, 600);
        fx2.stack.push(f);
        fx2.eligible.push(true);
        fx2.focused = Some(f);
        fx2.cursor = (1400, 1250);
        assert_eq!(resolve(&bid(&[Approach::SitEdge]), &fx2.world(), &mut rng).unwrap().kind, SpotKind::SitEdge { win: 5 });
        // Maximized on monitor 1 alone: beside goes to monitor 0's floor at the seam... monitor 0
        // is left of monitor 1.
        let mut fx3 = tiled();
        let m = r(6, 2572, 41, 2536, 1387);
        fx3.stack = vec![m];
        fx3.eligible = vec![true];
        fx3.focused = Some(m);
        fx3.cursor = (3000, 700);
        fx3.her = (1000, 1588);
        let s = resolve(&bid(&[]), &fx3.world(), &mut rng).unwrap();
        assert_eq!(s.kind, SpotKind::Beside);
        assert!(s.body.right() <= 2560 && s.body.right() > 2400, "at the seam: {s:?}");
        // Single monitor, maximized, she is on it: in place. Not on it: no spot.
        let mut fx4 = fx3.clone_one();
        fx4.her = (3500, 1428);
        let w4 = World { areas: &AREAS[1..], monitors: &MONS[1..], ..fx4.world() };
        let w4 = World { her: Spot { area: 0, ..w4.her }, ..w4 };
        let s = resolve(&bid(&[]), &w4, &mut rng).unwrap();
        assert_eq!(s.kind, SpotKind::InPlace);
    }

    impl Fixture {
        fn clone_one(&self) -> Fixture {
            Fixture { stack: self.stack.clone(), eligible: self.eligible.clone(), focused: self.focused, cursor: self.cursor, fs: self.fs, her: self.her }
        }
    }

    #[test]
    fn float_samples_clear_windows() {
        let mut fx = tiled();
        fx.stack = vec![r(1, 100, 400, 600, 400)];
        fx.eligible = vec![true];
        fx.focused = Some(fx.stack[0]);
        let w = fx.world();
        let mut rng = Rng::new(8);
        for s in float_spots(&w, &mut rng) {
            assert!(s.body.grow(24).intersect(&fx.stack[0]).is_none());
        }
    }

    #[test]
    fn guard_rejects_fullscreen_target_monitor() {
        let mut fx = tiled();
        fx.fs = [false, true];
        let w = fx.world();
        let s = w.spot(SpotKind::Floor, (3000, 1428), boxes().stand, None);
        assert_eq!(guard(&w, &s), Err(Block::Fullscreen));
        // Her own monitor fullscreen: nothing moves anywhere.
        fx.fs = [true, false];
        fx.her = (2000, 1588);
        let w = fx.world();
        let s = w.spot(SpotKind::Floor, (1800, 1588), boxes().stand, None);
        assert_eq!(guard(&w, &s), Err(Block::Fullscreen));
    }

    #[test]
    fn summons_need_hud_room() {
        let fx = tiled();
        let mut w = fx.world();
        w.hud_h = 220;
        let g = Goal::Active { why: Why::Summon(SummonKind::Done), approaches: vec![Approach::Hang, Approach::Beside] };
        let s = resolve(&g, &w, &mut Rng::new(9)).unwrap();
        assert_ne!(s.kind, SpotKind::Hang { upside_down: false }, "no room for the panel above a hang");
        assert!(s.body.y - 220 >= 29 || s.kind == SpotKind::InPlace);
    }

    #[test]
    fn cling_side_far_from_cursor() {
        let fx = tiled();
        let w = fx.world();
        let s = resolve(&bid(&[Approach::Cling]), &w, &mut Rng::new(10)).unwrap();
        // Cursor right of centre: the left edge, gripped on her left.
        assert_eq!(s.kind, SpotKind::Cling { left: true });
        assert_eq!(s.anchor.0, 12);
        assert!(s.cover_pct <= 12.0);
    }
}
