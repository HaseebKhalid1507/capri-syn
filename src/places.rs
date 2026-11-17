//! Where she goes: a brain `Goal` + the world → a `Spot`, and the hard rules (`guard`). Pure:
//! the view snapshots the world (stack, focus, work areas, cursor) at decision time.
//!
//! Units: root device px. Body boxes come from the frames' masks, relative to the pose's
//! anchor (feet, seat, grip), so a spot's `body` is exactly what will cover the screen.

use crate::anim::Rng;
use crate::brain::{Approach, Goal, HopKind, Why};
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
/// Capri's hands either side of her hang grip, as a fraction of the body width.
const HANG_HAND: f32 = 0.16;
/// Roams, perches and teleports (the window-close one too) land on the monitor the cursor is
/// on this often, on the other monitor(s) the rest (Haseeb 22:59). A monitor with no valid
/// target hands over to the others.
pub const CURSOR_MONITOR_P: f64 = 0.8;
/// Exploring (he has been idle a while): the cursor's monitor only this often.
pub const EXPLORE_MONITOR_P: f64 = 0.5;
/// The passive kinds (edge, floor, floor sit, float) and their weights: sitting on a window's
/// top edge is her favourite (Haseeb 22:55), and the only kind that may follow itself (on
/// another window).
const PASSIVE_KINDS: [(u8, f64); 4] = [(0, 0.70), (1, 0.10), (2, 0.10), (3, 0.10)];
/// A small hop (micro, startle, dodge) reaches at most this far across (logical px), at least
/// `HOP_MIN`. Down to the floor it drops any height; up onto a window top (the "and back")
/// it leaps at most `HOP_UP_MAX`.
pub const HOP_MAX: f32 = 300.0;
pub const HOP_MIN: f32 = 60.0;
pub const HOP_UP_MAX: f32 = 1400.0;
/// He is typing: a micro-hop ends at least this far from his window and never nearer to it.
pub const TYPING_CLEAR: f32 = 200.0;
/// A dodge ends at least this much farther from the pointer (logical px).
pub const DODGE_GAIN: f32 = 100.0;
/// A pounce ends with her body this close to the pointer (beside it).
pub const POUNCE_NEAR: f32 = 220.0;
/// A pounce's reach: farther than a hop (she may leap past it, "missed!") and shorter (the
/// pointer is within a lean of her when it lingers; her body, hair included, is wide).
pub const POUNCE_MAX: f32 = 450.0;
pub const POUNCE_MIN: f32 = 30.0;
/// A window's title bar (logical px): standing in front of it isn't over its content.
const TITLE_PX: f32 = 32.0;

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
    /// Standing, her whole outline (the hair's reach included; `stand` is the figure's box).
    pub outline: WinRect,
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
    /// Windows she sat on lately: the last one isn't sat on again right away (a roam goes
    /// to another window's top).
    pub last_wins: &'a [u32],
    pub last_kind: Option<SpotKind>,
    /// Summons: room the HUD needs above her body (prompt panel, reply).
    pub hud_h: i32,
    /// Passive spots only in this area (leaving his window: his monitor).
    pub only: Option<usize>,
    /// The cursor's area: roams, perches and teleports go there `CURSOR_MONITOR_P` of the
    /// time (None: `pick_area`'s old 70/30 around her, monitor = "any").
    pub prefer: Option<usize>,
}

/// The monitor he is working on: the focused window's (most of it), else the cursor's.
pub fn follow_area(monitors: &[WinRect], focused: Option<WinRect>, cursor: (i32, i32)) -> Option<usize> {
    let by_focus = focused.and_then(|f| {
        (0..monitors.len())
            .filter_map(|i| monitors[i].intersect(&f).map(|o| (i, area_of(&o))))
            .max_by_key(|&(_, a)| a)
            .map(|(i, _)| i)
    });
    by_focus.or_else(|| monitors.iter().position(|m| m.contains(cursor.0, cursor.1)))
}

/// The windows she may sit on (`eligible`), each with the monitor (area index) its centre is
/// on: what a workspace switch changes. Off every monitor: left out.
pub fn desk_windows(stack: &[WinRect], eligible: &[bool], areas: &[WinRect], monitors: &[WinRect]) -> Vec<(u32, usize)> {
    stack
        .iter()
        .zip(eligible)
        .filter(|(_, e)| **e)
        .filter_map(|(r, _)| {
            let (x, y) = r.center();
            areas.iter().position(|a| a.contains(x, y)).or_else(|| monitors.iter().position(|m| m.contains(x, y))).map(|i| (r.id, i))
        })
        .collect()
}

/// A workspace switch, a beat after it, as the brain hears it. i3 switches one monitor at a
/// time, the one he works on after it (the focus, else the pointer, which i3 warps there).
/// It happened on her monitor if he works there now and its windows aren't the ones from
/// before (`before`: `desk_windows` before the switch, None: not known; `after`: now), or if
/// her perch went with the old workspace (`stranded`). A switch on another monitor, or focus
/// moving over to hers, changed nothing under her.
pub fn workspace_switch(
    monitors: &[WinRect],
    focused: Option<WinRect>,
    cursor: (i32, i32),
    her_area: usize,
    stranded: bool,
    before: Option<&[(u32, usize)]>,
    after: &[(u32, usize)],
) -> crate::brain::Screen {
    let on_hers = |d: &[(u32, usize)]| {
        let mut v: Vec<u32> = d.iter().filter(|w| w.1 == her_area).map(|w| w.0).collect();
        v.sort_unstable();
        v
    };
    let his = follow_area(monitors, focused, cursor);
    let changed = before.is_some_and(|b| on_hers(b) != on_hers(after));
    crate::brain::Screen::Workspace { here: stranded || (changed && his == Some(her_area)), stranded }
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
    /// Not on the monitor he is working on (monitor = "follow").
    OtherMonitor,
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

    /// A tiling WM's floor: his window reaches the bottom of the work area, so standing or
    /// sitting on the floor means overlapping its bottom strip. Allowed for the floor kinds
    /// when the overlap stays in its lower half and within `cover_max_pct` (the cursor is
    /// checked apart).
    fn floor_strip_ok(&self, s: &Spot, f: &WinRect) -> bool {
        let Some(o) = f.intersect(&s.body) else { return true };
        let Some(a) = self.areas.get(s.area) else { return false };
        matches!(s.kind, SpotKind::Floor | SpotKind::FloorSit)
            && s.approach.is_none()
            && f.bottom() >= a.bottom() - self.d(24.0)
            && o.y >= f.y + f.h / 2
            && s.cover_pct <= self.cover_max_pct + 1e-9
    }

    /// Areas the passive kinds draw from: `only`, else hers (70 %) or another (30 %).
    fn pick_area(&self, rng: &mut Rng) -> usize {
        if let Some(i) = self.only.filter(|&i| i < self.areas.len()) {
            return i;
        }
        if self.areas.len() > 1 && rng.range(0.0, 1.0) >= 0.7 {
            let others: Vec<usize> = (0..self.areas.len()).filter(|&i| i != self.her.area).collect();
            others[rng.pick(others.len())]
        } else {
            self.her.area.min(self.areas.len().saturating_sub(1))
        }
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
            // On a tiling WM's floor: its bottom strip.
            (SpotKind::Floor | SpotKind::FloorSit, None) if w.floor_strip_ok(s, &f) => {}
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
    let roam = Roam { far: false, leaving: false, min_move: w.d(MIN_TELEPORT), skip_recent: true, kinds: &PASSIVE_KINDS, retry_last: true };
    match goal {
        // Far (after a sulk or a fling) if there is such a spot, else just away.
        Goal::Passive { far: true } => by_monitor(w, CURSOR_MONITOR_P, rng, |w, rng| {
            passive(w, &Roam { far: true, ..roam }, rng).or_else(|mut why| {
                passive(w, &roam, rng).map_err(|more| {
                    why.extend(more);
                    why
                })
            })
        }),
        Goal::Passive { far: false } => by_monitor(w, CURSOR_MONITOR_P, rng, |w, rng| passive(w, &roam, rng)),
        Goal::Explore => by_monitor(w, EXPLORE_MONITOR_P, rng, |w, rng| passive(w, &roam, rng)),
        Goal::Leave => passive(w, &Roam { leaving: true, min_move: w.d(80.0), ..roam }, rng),
        Goal::Active { why, approaches } => active(w, *why, approaches, rng),
        Goal::Hop { kind, away } => hop(w, *kind, *away, rng),
        Goal::FocusEdge => focus_edge(w),
        // Any other window's top (not his, not the one that closed: it's gone from the stack).
        Goal::Rescue => by_monitor(w, CURSOR_MONITOR_P, rng, |w, rng| passive(w, &Roam { min_move: 0, skip_recent: false, kinds: &[(0, 1.0)], retry_last: false, ..roam }, rng)),
        Goal::Follow => follow(w, &roam, rng),
    }
}

/// He switched workspace on her monitor: on that monitor, a window top of the new workspace
/// (her favourite), else the floor. Already on the floor, the floor is where she is: no move.
fn follow(w: &World, roam: &Roam, rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    let w = World { only: Some(w.her.area.min(w.areas.len().saturating_sub(1))), ..w.clone() };
    let tops = Roam { min_move: 0, skip_recent: false, kinds: &[(0, 1.0)], retry_last: false, ..*roam };
    passive(&w, &tops, rng).or_else(|mut why| {
        if matches!(w.her.kind, SpotKind::Floor | SpotKind::FloorSit | SpotKind::Beside) {
            why.push(("floor".into(), Block::NoSpot));
            return Err(why);
        }
        passive(&w, &Roam { kinds: &[(1, 0.5), (2, 0.5)], ..tops }, rng).map_err(|more| {
            why.extend(more);
            why
        })
    })
}

/// A rescue target exists: another visible window has room on top for her (the window-close
/// teleport, else she falls).
pub fn rescue_target(w: &World) -> Result<(), Vec<(String, Block)>> {
    resolve(&Goal::Rescue, w, &mut Rng::new(0x5eed)).map(|_| ())
}

/// The monitor preference: the cursor's area with probability `p`, else one of the others;
/// one with no valid target hands over to the rest (the others, then the cursor's).
fn by_monitor(w: &World, p: f64, rng: &mut Rng, f: impl Fn(&World, &mut Rng) -> Result<Spot, Vec<(String, Block)>>) -> Result<Spot, Vec<(String, Block)>> {
    let n = w.areas.len();
    let Some(pref) = w.prefer.filter(|&i| i < n && n > 1 && w.only.is_none()) else { return f(w, rng) };
    let mut others: Vec<usize> = (0..n).filter(|&i| i != pref).collect();
    for i in (1..others.len()).rev() {
        others.swap(i, rng.pick(i + 1));
    }
    let order: Vec<usize> = if (rng.range(0.0, 1.0) as f64) < p { std::iter::once(pref).chain(others).collect() } else { others.into_iter().chain([pref]).collect() };
    let mut why = Vec::new();
    for i in order {
        match f(&World { only: Some(i), ..w.clone() }, rng) {
            Ok(s) => return Ok(s),
            Err(e) => why.extend(e),
        }
    }
    Err(why)
}

// ---- passive -------------------------------------------------------------------------

/// How a passive spot is chosen.
#[derive(Clone, Copy)]
struct Roam<'k> {
    /// ≥ `FAR_PX` from the cursor, not her window (after a sulk or a fling).
    far: bool,
    /// Off his window, to the floor (after a visit that covered it).
    leaving: bool,
    /// At least this far from where she is (device px).
    min_move: i32,
    /// Not the windows she sat on last.
    skip_recent: bool,
    kinds: &'k [(u8, f64)],
    /// Nothing else fits: the kind she had last time (roams only).
    retry_last: bool,
}

fn passive(w: &World, o: &Roam, rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    let (far, leaving, min_move) = (o.far, o.leaving, o.min_move);
    let mut why = Vec::new();
    // The kind she just had isn't repeated, except the window top (another window's).
    let last = w.last_kind.map(|k| k.family());
    let mut pool: Vec<(u8, f64)> = o.kinds.iter().copied().filter(|k| k.0 == 0 || Some(k.0) != last).collect();
    if leaving {
        // Off his window to stand on the floor (a sulk or the satisfied beat plays there).
        pool = vec![(1, 1.0)];
    }
    let her_win = w.her.win();
    let ok = |s: &Spot| -> Result<(), Block> {
        if w.only.is_some_and(|i| s.area != i) {
            return Err(Block::OtherMonitor);
        }
        guard(w, s)?;
        if s.covers_focus() && !w.focused.is_some_and(|f| w.floor_strip_ok(s, &f)) {
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
        let mut cands = match kind {
            0 => edge_spots(w, o.skip_recent, rng),
            1 | 2 => floor_spots(w, kind == 2, rng),
            _ => float_spots(w, rng),
        };
        // Every kind by the floor's content score: a perch's legs, a teleport's landing keep
        // off windows' text too (r39: a hop landed her over the editor's text).
        clear_first(w, &mut cands);
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
            && o.retry_last
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

/// Sit spots on exposed edges of eligible windows other than the focused one (and the one she
/// sat on last, and is on: `skip_recent`).
fn edge_spots(w: &World, skip_recent: bool, rng: &mut Rng) -> Vec<Spot> {
    let mut out = Vec::new();
    let mut plats = edges::platforms(w.stack, w.eligible, w.contact, w.margin);
    let recent = |win: u32| skip_recent && (w.last_wins.last() == Some(&win) || w.her.win() == Some(win));
    plats.retain(|p| Some(p.win) != w.focused.map(|f| f.id) && !recent(p.win));
    if let Some(i) = w.only {
        // Platforms on that monitor only (the rest would be refused anyway).
        plats.retain(|p| w.area_of(p.seat_x.0.midpoint(p.seat_x.1), p.y) == i);
    }
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
        // Her monitor 70 %, another 30 % (monitor = "any"); his monitor (follow).
        let i = w.pick_area(rng);
        let a = w.areas[i];
        let x = a.x + (a.w as f32 * rng.range(0.1, 0.9)) as i32;
        out.push(w.spot(if sit { SpotKind::FloorSit } else { SpotKind::Floor }, (x, w.floor_y(i)), rel, None));
    }
    clear_first(w, &mut out);
    out
}

/// How much of `body` stands over windows' content (below their title bars), the middle half
/// of a window's width counted twice: 0 is empty desktop or a title bar; a corner beats the
/// middle of the text.
fn content_cost(w: &World, s: &Spot) -> f64 {
    let title = w.d(TITLE_PX);
    // Her whole outline, the hair's reach included (r40: her hair over the editor's text).
    let (st, o) = (w.boxes.stand, w.boxes.outline);
    let (l, t, r, b) = ((st.x - o.x).max(0), (st.y - o.y).max(0), (o.right() - st.right()).max(0), (o.bottom() - st.bottom()).max(0));
    let body = &WinRect::new(s.body.x - l, s.body.y - t, s.body.w + l + r, s.body.h + t + b);
    // Sitting on a window's top, her legs over that window are the pose (Haseeb's favourite).
    let perch = match s.kind {
        SpotKind::Edge { win } | SpotKind::SitEdge { win } => Some(win),
        _ => None,
    };
    let mut cost = 0;
    for (r, _) in w.stack.iter().zip(w.eligible).filter(|(r, e)| **e && Some(r.id) != perch) {
        let h = (r.h - title).max(0);
        let content = WinRect::new(r.x, r.y + title, r.w, h);
        let middle = WinRect::new(r.x + r.w / 4, r.y + title, r.w / 2, h);
        cost += [content, middle].iter().filter_map(|c| c.intersect(body)).map(|o| area_of(&o)).sum::<i64>();
    }
    cost as f64 / area_of(body).max(1) as f64
}

/// Floor spots clear of windows' content first, then over their margins, the middle of the
/// text last (a preference: on a tiled desk every spot is over something). Ties keep their
/// (random) order.
fn clear_first(w: &World, spots: &mut [Spot]) {
    spots.sort_by_cached_key(|s| (content_cost(w, s) * 5.0).floor() as i64);
}

/// Empty air: 24 seeded samples, clear of every eligible window (24 px) and the cursor.
fn float_spots(w: &World, rng: &mut Rng) -> Vec<Spot> {
    let clear = w.d(FLOAT_CLEAR);
    let mut out = Vec::new();
    for _ in 0..24 {
        let i = w.pick_area(rng);
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

// ---- small hops and reactions ------------------------------------------------------------

/// A small hop (≤ `HOP_MAX` across, `HOP_UP_MAX` up, any drop), on her own monitor, under
/// every passive rule (the guard, never over his window, the cursor clear).
/// Micro: along her edge, to the next window over, down to the floor and back. Startle: off
/// her window (the next one or the floor). Dodge: anywhere close, farther from the pointer.
/// `away`: he is typing, keep away from his window.
fn hop(w: &World, kind: HopKind, away: bool, rng: &mut Rng) -> Result<Spot, Vec<(String, Block)>> {
    if kind == HopKind::Pounce {
        return pounce(w);
    }
    let her = w.her;
    let (lo, hi) = (w.d(HOP_MIN) as i64, w.d(HOP_MAX) as i64);
    let her_win = her.win();
    let on_edge = her_win.is_some();
    let gap = |a: &WinRect, f: &WinRect| -> f64 {
        let dx = (f.x - a.right()).max(a.x - f.right()).max(0) as f64;
        let dy = (f.y - a.bottom()).max(a.y - f.bottom()).max(0) as f64;
        (dx * dx + dy * dy).sqrt()
    };
    let cursor_d = |b: &WinRect| (dist2(b, w.cursor.0, w.cursor.1) as f64).sqrt();
    let ok = |s: &Spot| -> Result<(), Block> {
        if s.area != her.area {
            return Err(Block::OtherMonitor);
        }
        guard(w, s)?;
        if s.covers_focus() && !w.focused.is_some_and(|f| w.floor_strip_ok(s, &f)) {
            return Err(Block::FocusedInterior);
        }
        let (dx, dy) = ((s.anchor.0 - her.anchor.0) as i64, (s.anchor.1 - her.anchor.1) as i64);
        if dx * dx + dy * dy < lo * lo || dx.abs() > hi || -dy > w.d(HOP_UP_MAX) as i64 {
            return Err(Block::NoSpot);
        }
        if away && let Some(f) = w.focused {
            // At least TYPING_CLEAR from his window, and never nearer to it than now.
            let (gs, gh) = (gap(&s.body, &f), gap(&her.body, &f));
            if gs < (w.d(TYPING_CLEAR) as f64).min(gh) - 1.0 {
                return Err(Block::FocusedInterior);
            }
        }
        if kind == HopKind::Dodge && cursor_d(&s.body) < cursor_d(&her.body) + w.d(DODGE_GAIN) as f64 {
            return Err(Block::Cursor);
        }
        Ok(())
    };
    // Families: 0 along her edge, 1 the next window over, 2 the floor (down, or along it).
    let fams: Vec<(u8, f64)> = match (kind, on_edge) {
        (HopKind::Micro, true) => vec![(0, 0.5), (1, 0.35), (2, 0.15)],
        // On the floor: back up onto a window top (her favourite) more often than along.
        (HopKind::Micro, false) => vec![(1, 0.75), (2, 0.25)],
        // After a reaction too, a window top before the floor (Haseeb 22:55).
        (HopKind::Startle, _) => vec![(1, 0.65), (2, 0.35)],
        (HopKind::Dodge, true) => vec![(0, 0.4), (1, 0.4), (2, 0.2)],
        (HopKind::Dodge, false) => vec![(1, 0.65), (2, 0.35)],
        (HopKind::Pounce, _) => unreachable!("pounce() above"),
    };
    let mut pool = fams;
    let mut why = Vec::new();
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
        let fam = pool.remove(k).0;
        let mut cands = match fam {
            0 => along_spots(w, rng),
            1 => near_edge_spots(w, rng),
            _ => near_floor_spots(w, on_edge, rng),
        };
        // A dodge takes the farthest from the pointer; the rest are already shuffled, then
        // sorted by the floor's content score (off windows' text first).
        if kind == HopKind::Dodge {
            cands.sort_by(|a, b| cursor_d(&b.body).total_cmp(&cursor_d(&a.body)));
        } else {
            clear_first(w, &mut cands);
        }
        let mut last = Block::NoSpot;
        for s in cands {
            match ok(&s) {
                Ok(()) => return Ok(s),
                Err(b) => last = b,
            }
        }
        why.push((["along", "next_window", "floor"][fam as usize].to_string(), last));
    }
    Err(why)
}

/// A pounce at the still pointer: a hop toward it (`POUNCE_MIN`..`POUNCE_MAX` across) that ends beside it (her body
/// within `POUNCE_NEAR`), never on it (the guard's `CURSOR_CLEAR`). A window top
/// in reach (any but his: its interior is his) beats the floor; then the nearest the pointer.
fn pounce(w: &World) -> Result<Spot, Vec<(String, Block)>> {
    let her = w.her;
    let (lo, hi) = (w.d(POUNCE_MIN) as i64, w.d(POUNCE_MAX) as i64);
    let c = w.cursor;
    let cursor_d = |b: &WinRect| (dist2(b, c.0, c.1) as f64).sqrt();
    // Every seat in her reach, a step apart: the checks below pick the one beside it.
    let step = w.d(24.0).max(1) as usize;
    let reach = (her.anchor.0 - hi as i32, her.anchor.0 + hi as i32);
    let mut tops = Vec::new();
    for p in edges::platforms(w.stack, w.eligible, w.contact, w.margin) {
        if w.focused.is_some_and(|f| f.id == p.win) {
            continue;
        }
        let (a, b) = (p.seat_x.0.max(reach.0), p.seat_x.1.min(reach.1));
        for x in (a..=b).step_by(step) {
            tops.push(w.spot(SpotKind::Edge { win: p.win }, (x, p.y), w.boxes.sit, None));
        }
    }
    let i = her.area.min(w.areas.len().saturating_sub(1));
    let floor: Vec<Spot> = (reach.0..=reach.1).step_by(step).map(|x| w.spot(SpotKind::Floor, (x, w.floor_y(i)), w.boxes.stand, None)).collect();
    let ok = |s: &Spot| -> Result<(), Block> {
        if s.area != her.area {
            return Err(Block::OtherMonitor);
        }
        guard(w, s)?;
        if s.covers_focus() && !w.focused.is_some_and(|f| w.floor_strip_ok(s, &f)) {
            return Err(Block::FocusedInterior);
        }
        let (dx, dy) = ((s.anchor.0 - her.anchor.0) as i64, (s.anchor.1 - her.anchor.1) as i64);
        if dx * dx + dy * dy < lo * lo || dx.abs() > hi || -dy > w.d(HOP_UP_MAX) as i64 {
            return Err(Block::NoSpot);
        }
        // Toward it, never away (r41: beside the pointer on her far side read as a dodge).
        let toward = (c.0 - her.anchor.0).signum() as i64;
        if toward != 0 && dx.signum() != toward {
            return Err(Block::NoSpot);
        }
        // Beside it (she may overshoot: "missed!").
        if cursor_d(&s.body) > w.d(POUNCE_NEAR) as f64 {
            return Err(Block::Cursor);
        }
        Ok(())
    };
    let mut why = Vec::new();
    for (name, mut cands) in [("pounce_top", tops), ("pounce_floor", floor)] {
        cands.sort_by(|a, b| cursor_d(&a.body).total_cmp(&cursor_d(&b.body)));
        // The nearest one's block says why (the farthest ones are only far).
        let mut first = None;
        for s in cands {
            match ok(&s) {
                Ok(()) => return Ok(s),
                Err(b) => {
                    first.get_or_insert(b);
                }
            }
        }
        why.push((name.to_string(), first.unwrap_or(Block::NoSpot)));
    }
    Err(why)
}

/// Seats along the edge she sits on, HOP_MIN..HOP_MAX either side.
fn along_spots(w: &World, rng: &mut Rng) -> Vec<Spot> {
    let Some(win) = w.her.win() else { return Vec::new() };
    let Some(i) = w.stack.iter().position(|r| r.id == win) else { return Vec::new() };
    let (lo, hi) = (w.d(HOP_MIN), w.d(HOP_MAX));
    let x0 = w.her.anchor.0;
    let mut out = Vec::new();
    for (a, b) in edges::seat_ranges(w.stack, i, w.contact, w.margin) {
        for _ in 0..6 {
            let d = lo + rng.pick((hi - lo).max(1) as usize) as i32;
            for x in [x0 - d, x0 + d] {
                if (a..=b).contains(&x) {
                    out.push(w.spot(SpotKind::Edge { win }, (x, w.stack[i].y), w.boxes.sit, None));
                }
            }
        }
    }
    for i in (1..out.len()).rev() {
        out.swap(i, rng.pick(i + 1));
    }
    out
}

/// Seats on other windows' exposed tops (not hers, not his) near her x.
fn near_edge_spots(w: &World, rng: &mut Rng) -> Vec<Spot> {
    let hi = w.d(HOP_MAX);
    let x0 = w.her.anchor.0;
    let mut plats = edges::platforms(w.stack, w.eligible, w.contact, w.margin);
    plats.retain(|p| Some(p.win) != w.her.win() && Some(p.win) != w.focused.map(|f| f.id));
    for i in (1..plats.len()).rev() {
        plats.swap(i, rng.pick(i + 1));
    }
    let mut out = Vec::new();
    for p in plats {
        let near = x0.clamp(p.seat_x.0, p.seat_x.1);
        for x in [near, near + rng.pick(hi.max(1) as usize) as i32 - hi / 2] {
            out.push(w.spot(SpotKind::Edge { win: p.win }, (x.clamp(p.seat_x.0, p.seat_x.1), p.y), w.boxes.sit, None));
        }
    }
    out
}

/// Her monitor's floor within HOP_MAX of her x (straight down from a window top first).
fn near_floor_spots(w: &World, from_edge: bool, rng: &mut Rng) -> Vec<Spot> {
    let (lo, hi) = (w.d(HOP_MIN), w.d(HOP_MAX));
    let i = w.her.area.min(w.areas.len().saturating_sub(1));
    let x0 = w.her.anchor.0;
    let mut out = Vec::new();
    if from_edge {
        out.push(w.spot(SpotKind::Floor, (x0, w.floor_y(i)), w.boxes.stand, None));
    }
    for _ in 0..10 {
        let d = (if from_edge { 0 } else { lo }) + rng.pick((hi - lo).max(1) as usize) as i32;
        let x = if rng.pick(2) == 0 { x0 - d } else { x0 + d };
        out.push(w.spot(SpotKind::Floor, (x, w.floor_y(i)), w.boxes.stand, None));
    }
    clear_first(w, &mut out);
    out
}

/// The newly focused window's top edge at the end farther from the cursor (legs over its top
/// strip at most `cover_max_pct`, the cursor clear); no room on top: the floor beside it.
fn focus_edge(w: &World) -> Result<Spot, Vec<(String, Block)>> {
    let Some(f) = w.focused else { return Err(vec![("focus".into(), Block::NoSpot)]) };
    let mut why = Vec::new();
    if let Some(i) = w.stack.iter().position(|r| r.id == f.id) {
        let area = w.areas[w.area_of(f.center().0, f.y)];
        let mut xs: Vec<i32> = Vec::new();
        for (lo, hi) in edges::seat_ranges(w.stack, i, w.contact, w.margin) {
            xs.extend([lo, hi, (lo + hi) / 2, lo + (hi - lo) / 4, hi - (hi - lo) / 4]);
        }
        xs.sort_by_key(|&x| -(x - w.cursor.0).abs());
        xs.dedup();
        let mut block = Block::NoSpot;
        for x in xs {
            let s = w.spot(SpotKind::SitEdge { win: f.id }, (x, f.y), w.boxes.sit, None);
            if f.y <= area.y || s.body.y < area.y {
                block = Block::Offscreen;
                continue;
            }
            if s.cover_pct > w.cover_max_pct + 1e-9 {
                block = Block::Cover;
                continue;
            }
            match guard(w, &s) {
                Ok(()) => return Ok(s),
                Err(b) => block = b,
            }
        }
        why.push(("focus_edge".into(), block));
    }
    let mut block = Block::NoSpot;
    for s in beside_spots(w, &f) {
        let s = Spot { approach: None, ..s };
        match guard(w, &s) {
            Ok(()) if !s.covers_focus() => return Ok(s),
            Ok(()) => block = Block::FocusedInterior,
            Err(b) => block = b,
        }
    }
    why.push(("beside".into(), block));
    Err(why)
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
    let ap = Some(if upside_down { Approach::HangUpsideDown } else { Approach::Hang });
    let grip = w.d(10.0);
    if upside_down {
        // By the feet, just inside the far corner: half her hair hangs outside his window.
        let x = if right { f.right() - grip } else { f.x + grip };
        out.push(w.spot(SpotKind::Hang { upside_down }, (x, line), rel, ap));
    } else {
        // Room beside the far corner: hang just outside it, no pixel of her over his window
        // (r7 critics: the hair over the title bar and the first lines).
        let gap = w.d(6.0);
        let x = if right { f.right() + gap - rel.x } else { f.x - gap - (rel.x + rel.w) };
        out.push(w.spot(SpotKind::Hang { upside_down }, (x, line), rel, ap));
        // By the hands, so her hair hangs mostly outside his window (less of the title bar
        // and the first lines covered): the inner hand just over the far corner, else the
        // outer one.
        let hand = (rel.w as f32 * HANG_HAND) as i32;
        for x in if right { [f.right() - grip + hand, f.right() - hand] } else { [f.x + grip - hand, f.x + hand] } {
            out.push(w.spot(SpotKind::Hang { upside_down }, (x, line), rel, ap));
        }
    }
    // Over the far corner, then shifted outward so less of his window is covered.
    for shift in [0, rel.w / 4, rel.w / 2, rel.w * 3 / 4] {
        let x = if right { f.right() - inset + shift - (rel.x + rel.w) } else { f.x + inset - shift - rel.x };
        out.push(w.spot(SpotKind::Hang { upside_down }, (x, line), rel, ap));
    }
    out
}

fn cling_spots(w: &World, f: &WinRect) -> Vec<Spot> {
    let (right, _) = far_corner(f, w.cursor);
    // The hand hooks round the frame's outer edge: fingertips a little past it.
    let hook = (3.0 * w.sf).round() as i32;
    let x = if right { f.right() } else { f.x };
    let mut out = Vec::new();
    // Outside the far edge first, holding it (the left edge on her right: pose-dock-right),
    // so she covers none of it; then inside, the edge on her side.
    let outside = if right { w.boxes.cling_l.map(|r| (r, true)) } else { w.boxes.cling_r.map(|r| (r, false)) };
    let inside = if right { w.boxes.cling_r.map(|r| (r, false)) } else { w.boxes.cling_l.map(|r| (r, true)) };
    for (rel, left) in outside.into_iter().chain(inside) {
        // Outside the edge (holding it from her side), the hand reaches over it by `hook`.
        let out_side = left == right;
        let x = if out_side { x + if right { -hook } else { hook } } else { x };
        for k in [0.12, 0.2, 0.3] {
            out.push(w.spot(SpotKind::Cling { left }, (x, f.y + (f.h as f32 * k) as i32), rel, Some(Approach::Cling)));
        }
    }
    out
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
        let s = w.spot(SpotKind::Beside, (x, w.floor_y(i)), rel, Some(Approach::Beside));
        // Past its monitor's side that floor is in mid-air: the seam spot below has the other's.
        if s.area == i {
            out.push(s);
        }
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
            outline: r(0, -170, -390, 340, 395),
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
                only: None,
                prefer: None,
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
        let f = fx.focused.unwrap();
        for _ in 0..300 {
            let s = resolve(&Goal::Passive { far: false }, &w, &mut rng).unwrap();
            assert_ne!(s.win(), Some(1), "never on the focused window");
            // A tile reaching the floor: only its bottom strip, within cover_max_pct.
            if let Some(o) = s.body.intersect(&f) {
                assert!(matches!(s.kind, SpotKind::Floor | SpotKind::FloorSit), "{s:?}");
                assert!(o.y >= f.y + f.h / 2 && s.cover_pct <= 12.0, "{s:?}");
            }
        }
    }

    #[test]
    fn follow_stays_on_her_monitor() {
        // A workspace switch on her monitor: a window top there (her favourite), whatever the
        // pointer's monitor; on its floor if none; already on the floor, she stays.
        let mut fx = tiled();
        fx.stack.push(r(4, 300, 900, 900, 500));
        fx.eligible.push(true);
        fx.focused = Some(fx.stack[2]);
        fx.cursor = (3000, 700);
        fx.her = (1500, 1588);
        let base = fx.world();
        let perched = Spot { kind: SpotKind::Edge { win: 9 }, anchor: (1500, 700), body: boxes().sit.offset(1500, 700), area: 0, ..base.her };
        for seed in 0..300 {
            let w = World { her: perched, prefer: Some(1), ..base.clone() };
            let s = resolve(&Goal::Follow, &w, &mut Rng::new(seed)).unwrap();
            assert_eq!((s.area, s.kind), (0, SpotKind::Edge { win: 4 }), "seed {seed}");
            let s = resolve(&Goal::Follow, &World { prefer: Some(1), ..base.clone() }, &mut Rng::new(seed)).unwrap();
            assert_eq!((s.area, s.kind), (0, SpotKind::Edge { win: 4 }), "seed {seed}: from the floor");
        }
        // No window top on her monitor (only i3 tiles under the bar): perched, her monitor's
        // floor; on the floor, no move.
        let fx = tiled();
        let base = fx.world();
        for seed in 0..300 {
            let w = World { her: perched, prefer: Some(1), ..base.clone() };
            let s = resolve(&Goal::Follow, &w, &mut Rng::new(seed)).unwrap();
            assert!(s.area == 0 && matches!(s.kind, SpotKind::Floor | SpotKind::FloorSit) && s.anchor.1 == 1588, "seed {seed}: {s:?}");
            assert!(resolve(&Goal::Follow, &World { prefer: Some(1), ..base.clone() }, &mut Rng::new(seed)).is_err(), "seed {seed}");
        }
    }

    #[test]
    fn beside_never_past_its_monitor() {
        // His log: `focus_edge spot=beside 4595,738 -> 2388,1430`. A tile at DP-3's left side
        // (no room on top): the spot left of it is on eDP, and DP-3's floor there is mid-air.
        let tile = r(31, 2560, 29, 2560, 709);
        for cursor in [(3000, 400), (4800, 400)] {
            let fx = Fixture { stack: vec![tile], eligible: vec![true], focused: Some(tile), cursor, fs: [false; 2], her: (1842, 1588) };
            let w = fx.world();
            let s = resolve(&Goal::FocusEdge, &w, &mut Rng::new(1)).unwrap();
            assert_eq!(s.anchor.1, AREAS[s.area].bottom() - 12, "on the floor of the monitor it is on: {s:?}");
            for b in beside_spots(&w, &tile) {
                assert_eq!(b.anchor.1, AREAS[b.area].bottom() - 12, "{b:?}");
            }
        }
    }

    #[test]
    fn follow_area_is_his_monitor() {
        // The focused window's monitor (most of it), else the cursor's.
        assert_eq!(follow_area(&MONS, Some(r(9, 2700, 100, 900, 600)), (100, 100)), Some(1));
        assert_eq!(follow_area(&MONS, Some(r(9, 2400, 100, 900, 600)), (100, 100)), Some(1));
        assert_eq!(follow_area(&MONS, None, (3000, 700)), Some(1));
        assert_eq!(follow_area(&MONS, None, (300, 700)), Some(0));
    }

    #[test]
    fn follow_keeps_passive_hops_on_his_monitor() {
        // `only` (leaving his window: his monitor) holds: she is on monitor 0, he works on
        // monitor 1 (window 3 focused): every passive kind (edge, floor, floor_sit, float)
        // lands on monitor 1.
        let mut fx = tiled();
        fx.focused = Some(fx.stack[2]);
        fx.cursor = (3500, 200);
        let mut w = fx.world();
        w.only = follow_area(w.monitors, w.focused, w.cursor);
        assert_eq!(w.only, Some(1));
        let mut rng = Rng::new(5);
        let mut kinds = std::collections::HashSet::new();
        for _ in 0..300 {
            let s = resolve(&Goal::Passive { far: false }, &w, &mut rng).unwrap();
            assert_eq!(s.area, 1, "{s:?}");
            kinds.insert(s.kind.family());
        }
        assert!(kinds.len() >= 3, "{kinds:?}");
        // "any": monitor 0 shows up too.
        w.only = None;
        let other = (0..300).filter(|_| resolve(&Goal::Passive { far: false }, &w, &mut rng).unwrap().area == 0).count();
        assert!(other > 0);
    }

    #[test]
    fn tiling_floor_strip_is_allowed_but_not_more() {
        let fx = tiled();
        let mut w = fx.world();
        w.only = Some(0);
        let f = fx.focused.unwrap();
        // Standing on the floor under the focused tile: its bottom strip, ~4.6 % of it.
        let s = w.spot(SpotKind::Floor, (600, w.floor_y(0)), w.boxes.stand, None);
        assert!(s.body.intersect(&f).is_some() && s.cover_pct < 12.0);
        assert_eq!(guard(&w, &s), Ok(()));
        // Not floating in it, not with the cursor on her.
        let fl = w.spot(SpotKind::Float, (600, 900), w.boxes.stand, None);
        assert_eq!(guard(&w, &fl), Err(Block::FocusedInterior));
        let mut w2 = fx.world();
        w2.cursor = (600, w.floor_y(0) - 100);
        assert_eq!(guard(&w2, &s), Err(Block::Cursor));
        // A small focused window that reaches the floor: covering > 12 % of it is refused.
        let small = r(7, 500, 1288, 300, 300);
        let fx3 = Fixture { focused: Some(small), stack: vec![small], eligible: vec![true], ..tiled() };
        let w3 = fx3.world();
        let s3 = w3.spot(SpotKind::Floor, (650, w3.floor_y(0)), w3.boxes.stand, None);
        assert!(s3.cover_pct > 12.0);
        assert_eq!(guard(&w3, &s3), Err(Block::FocusedInterior));
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
        // A spot over his window's interior is refused by the guard unless it's an exception
        // (the floor under a tile is one: tiling_floor_strip_is_allowed_but_not_more).
        let over = w.spot(SpotKind::Float, (600, 1000), boxes().stand, None);
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
    fn hang_by_the_hand_at_the_corner() {
        // Focused on the right tile, cursor on its right: the far corner is the top left,
        // and there's room left of it. Her outer hand is over the corner, most of her
        // outside the window.
        let mut fx = tiled();
        fx.focused = Some(fx.stack[1]);
        fx.cursor = (2300, 900);
        let w = fx.world();
        let s = resolve(&bid(&[Approach::Hang]), &w, &mut Rng::new(3)).unwrap();
        let f = fx.stack[1];
        // There's room left of it (over the left tile): fully outside, nothing over his window.
        assert_eq!(s.body.right(), f.x - 6, "{:?}", s.body);
        assert_eq!(s.cover_pct, 0.0);
        // Without the room (the left tile's pixels can't hold her: shrink the monitor), the
        // inner hand just over the corner.
        let mut fx2 = tiled();
        fx2.stack.remove(0);
        fx2.eligible.remove(0);
        fx2.focused = Some(r(2, 200, 41, 1264, 1547));
        fx2.stack[0] = fx2.focused.unwrap();
        fx2.cursor = (1300, 900);
        let w = fx2.world();
        let s = resolve(&bid(&[Approach::Hang]), &w, &mut Rng::new(3)).unwrap();
        let f = fx2.stack[0];
        assert_eq!(s.anchor.0, f.x + 10 - (300.0 * HANG_HAND) as i32, "the inner hand, not the middle, over the corner");
        assert!(s.body.x < f.x && s.cover_pct > 0.0 && s.cover_pct < 4.0, "{:?} cover {}", s.body, s.cover_pct);
        // At the monitor's edge there is no room outside: the old shifts, inside.
        let w = tiled();
        let s = resolve(&bid(&[Approach::Hang]), &w.world(), &mut Rng::new(3)).unwrap();
        assert!(s.body.x >= 0 && s.cover_pct <= 12.0);
    }

    #[test]
    fn cling_outside_first() {
        let mut fx = tiled();
        fx.focused = Some(fx.stack[1]);
        fx.cursor = (2300, 900);
        let w = fx.world();
        let s = resolve(&bid(&[Approach::Cling]), &w, &mut Rng::new(4)).unwrap();
        assert_eq!(s.kind, SpotKind::Cling { left: false }, "the left edge on her right: outside it");
        assert!(s.cover_pct < 1.0 && s.body.x < fx.stack[1].x, "only the hand on his window: {}", s.cover_pct);
        // No room outside (the left tile at the monitor edge): inside, the edge on her left.
        let fx = tiled();
        let w = fx.world();
        let s = resolve(&bid(&[Approach::Cling]), &w, &mut Rng::new(4)).unwrap();
        assert_eq!(s.kind, SpotKind::Cling { left: true });
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

    // ---- lively: the window top is her favourite, the cursor's monitor 80/20 ---------------

    /// Floating windows with head room on both monitors; he works in 12 (monitor 0).
    fn desk() -> Fixture {
        let w = vec![
            r(11, 150, 420, 900, 640),
            r(12, 1300, 300, 1000, 760),
            r(13, 2750, 380, 950, 620),
            r(14, 3900, 560, 950, 520),
            r(15, 700, 1150, 700, 300),
            r(16, 1700, 1180, 700, 300),
            r(17, 3300, 1080, 800, 300),
        ];
        let n = w.len();
        Fixture { stack: w, eligible: vec![true; n], focused: Some(r(12, 1300, 300, 1000, 760)), cursor: (1700, 700), fs: [false; 2], her: (600, 1588) }
    }

    fn her_at(w: &mut World, s: Spot) {
        w.her = s;
    }

    #[test]
    fn passive_roams_favour_window_tops() {
        // Roams chained from wherever the last one landed: >= 60 % on a window's top edge, and
        // edge after edge allowed (never the same window twice running, never his).
        let fx = desk();
        let mut w = fx.world();
        let mut rng = Rng::new(21);
        let (mut edges, n) = (0, 3000);
        let mut last_wins: Vec<u32> = Vec::new();
        let mut prev_win = None;
        for _ in 0..n {
            let lw = last_wins.clone();
            let w2 = World { last_wins: &lw, ..w.clone() };
            let s = resolve(&Goal::Passive { far: false }, &w2, &mut rng).unwrap();
            if let SpotKind::Edge { win } = s.kind {
                edges += 1;
                assert_ne!(Some(win), prev_win, "not the same window twice running");
                assert_ne!(win, 12, "never on his window");
                last_wins.push(win);
                if last_wins.len() > 2 {
                    last_wins.remove(0);
                }
                prev_win = Some(win);
            } else {
                prev_win = None;
            }
            w.last_kind = Some(s.kind);
            her_at(&mut w, s);
        }
        let share = edges as f64 / n as f64;
        assert!(share >= 0.6, "window tops {share:.3}");
    }

    #[test]
    fn cursor_monitor_80_20_over_many_seeds() {
        // Roams, perches and the window-close teleport: the cursor's monitor 80 % (±5), the
        // others 20 %.
        for (cursor, focus, pref) in [((1700, 700), 12u32, 0usize), ((3300, 700), 13, 1)] {
            let mut fx = desk();
            fx.cursor = cursor;
            fx.focused = fx.stack.iter().find(|r| r.id == focus).copied();
            for goal in [Goal::Passive { far: false }, Goal::Rescue] {
                let mut on = 0;
                let n = 2000;
                for seed in 0..n {
                    let mut w = fx.world();
                    w.prefer = Some(pref);
                    let s = resolve(&goal, &w, &mut Rng::new(2 * seed + 1)).unwrap();
                    assert!(guard(&w, &s).is_ok());
                    on += usize::from(s.area == pref);
                }
                let share = on as f64 / n as f64;
                assert!((share - 0.8).abs() <= 0.05, "{goal:?} cursor on {pref}: {share:.3}");
            }
        }
        // Exploring (idle a while): about half each.
        let fx = desk();
        let on = (0..2000).filter(|&seed| {
            let mut w = fx.world();
            w.prefer = Some(0);
            resolve(&Goal::Explore, &w, &mut Rng::new(2 * seed + 1)).unwrap().area == 0
        });
        let share = on.count() as f64 / 2000.0;
        assert!((share - 0.5).abs() <= 0.05, "explore {share:.3}");
    }

    #[test]
    fn cursor_monitor_without_a_target_hands_over() {
        // The cursor's monitor (0) has no window top but his: every rescue goes to monitor 1.
        let mut fx = desk();
        fx.stack.retain(|r| r.id == 12 || r.x >= 2560);
        fx.eligible = vec![true; fx.stack.len()];
        for seed in 0..500 {
            let mut w = fx.world();
            w.prefer = Some(0);
            let s = resolve(&Goal::Rescue, &w, &mut Rng::new(seed)).unwrap();
            assert_eq!(s.area, 1, "seed {seed}");
            assert!(matches!(s.kind, SpotKind::Edge { win } if win != 12));
        }
        // A fullscreen app on the cursor's monitor (she is on the other one): roams there too.
        let mut fx = desk();
        fx.fs = [true, false];
        fx.her = (3300, 1428);
        fx.focused = fx.stack.iter().find(|r| r.id == 13).copied();
        for seed in 0..500 {
            let mut w = fx.world();
            w.prefer = Some(0);
            assert_eq!(resolve(&Goal::Passive { far: false }, &w, &mut Rng::new(seed)).unwrap().area, 1, "seed {seed}");
        }
        // The other monitor empty: the cursor's always (never "no spot" while one exists).
        let mut fx = desk();
        fx.stack.retain(|r| r.x < 2560);
        fx.eligible = vec![true; fx.stack.len()];
        for seed in 0..300 {
            let mut w = fx.world();
            w.prefer = Some(0);
            assert_eq!(resolve(&Goal::Rescue, &w, &mut Rng::new(seed)).unwrap().area, 0);
        }
    }

    #[test]
    fn rescue_needs_another_window_with_room() {
        let fx = desk();
        let w = fx.world();
        assert!(rescue_target(&w).is_ok());
        // Only his window left (and windows without head room): no target, she falls.
        let mut fx = desk();
        fx.stack = vec![r(12, 1300, 300, 1000, 760), r(16, 12, 29, 1200, 1500), r(17, 2572, 29, 2500, 1400)];
        fx.eligible = vec![true; 3];
        let w = fx.world();
        assert!(rescue_target(&w).is_err(), "nothing but his window and tiles under the bar");
        // A window whose top is hidden under another: not a target.
        let mut fx = desk();
        fx.stack = vec![r(12, 1300, 300, 1000, 760), r(18, 2800, 500, 900, 500), r(19, 2700, 400, 1200, 300)];
        fx.eligible = vec![true, true, false];
        let w = fx.world();
        assert!(rescue_target(&w).is_err(), "18's top is under 19 (not eligible)");
    }

    #[test]
    fn micro_hops_stay_small_and_clear() {
        let fx = desk();
        let mut rng = Rng::new(31);
        let base = fx.world();
        // From a seat on window 11 and from the floor; many tries.
        let seat = base.spot(SpotKind::Edge { win: 11 }, (600, 420), base.boxes.sit, None);
        let floor = base.spot(SpotKind::Floor, (900, base.floor_y(0)), base.boxes.stand, None);
        let mut fams = std::collections::HashMap::new();
        for from in [seat, floor] {
            for _ in 0..400 {
                let w = World { her: from, ..base.clone() };
                let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Micro, away: false }, &w, &mut rng) else { continue };
                let (dx, dy) = (s.anchor.0 - from.anchor.0, s.anchor.1 - from.anchor.1);
                assert!(dx.abs() <= 300 && -dy <= 1400 && dx * dx + dy * dy >= 60 * 60, "{from:?} -> {s:?}");
                assert_eq!(s.area, from.area);
                assert!(guard(&w, &s).is_ok() && !s.covers_focus() || w.focused.is_some_and(|f| w.floor_strip_ok(&s, &f)));
                assert_ne!(s.win(), Some(12), "never on his window");
                let fam = match (from.win(), s.win()) {
                    (Some(a), Some(b)) if a == b => "along",
                    (_, Some(_)) => "window",
                    _ => "floor",
                };
                *fams.entry((from.kind.key(), fam)).or_insert(0) += 1;
            }
        }
        for k in [("edge", "along"), ("edge", "window"), ("edge", "floor"), ("floor", "window"), ("floor", "floor")] {
            assert!(fams.get(&k).copied().unwrap_or(0) > 10, "{k:?} in {fams:?}");
        }
        // He is typing: never nearer his window than now, and >= 200 px from it.
        let f = fx.focused.unwrap();
        let gap = |b: &WinRect| {
            let dx = (f.x - b.right()).max(b.x - f.right()).max(0) as f64;
            let dy = (f.y - b.bottom()).max(b.y - f.bottom()).max(0) as f64;
            (dx * dx + dy * dy).sqrt()
        };
        for _ in 0..300 {
            let w = World { her: seat, ..base.clone() };
            if let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Micro, away: true }, &w, &mut rng) {
                assert!(gap(&s.body) >= 200.0f64.min(gap(&seat.body)) - 1.0, "{s:?}");
            }
        }
    }

    #[test]
    fn a_pounce_lands_beside_the_pointer_never_on_it_and_prefers_a_top() {
        let mut fx = desk();
        // Nothing focused (his window's interior is never covered: that rule has its own tests).
        fx.focused = None;
        let base = fx.world();
        // On the floor under window 12 (top at y 300 is out of reach: 1100+ up is fine, 300 px
        // across is the limit): the pointer still near window 16's top (y 1180).
        let her = base.spot(SpotKind::Floor, (1600, base.floor_y(0)), base.boxes.stand, None);
        let (mut tops, mut n) = (0, 0);
        for k in 0..40 {
            let c = (1560 + k * 5, 1130 + (k % 4) * 10);
            let w = World { her, cursor: c, ..base.clone() };
            let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Pounce, away: false }, &w, &mut Rng::new(3)) else { continue };
            n += 1;
            assert!(!s.body.grow(w.d(CURSOR_CLEAR)).contains(c.0, c.1), "never on the pointer: {s:?} {c:?}");
            let d = |b: &WinRect| (dist2(b, c.0, c.1) as f64).sqrt();
            assert!(d(&s.body) <= w.d(POUNCE_NEAR) as f64, "beside it");
            assert!((s.anchor.0 - her.anchor.0).abs() <= w.d(POUNCE_MAX), "a pounce's reach");
            assert!((s.anchor.0 - her.anchor.0).signum() == (c.0 - her.anchor.0).signum() || c.0 == her.anchor.0, "toward it");
            tops += usize::from(s.win().is_some());
        }
        assert!(n >= 30, "{n} pounces of 40");
        // A top in reach beats the floor; the floor where every top seat toward it would cover it.
        assert!(tops * 2 >= n, "{tops} tops of {n}");
        // No top in reach: the floor beside it.
        let w = World { her, cursor: (1350, 1500), ..base.clone() };
        let s = resolve(&Goal::Hop { kind: HopKind::Pounce, away: false }, &w, &mut Rng::new(3)).expect("floor");
        assert!(s.win().is_none() && !s.body.grow(w.d(CURSOR_CLEAR)).contains(1350, 1500));
        assert_eq!((s.anchor.0 - her.anchor.0).signum(), (1350 - her.anchor.0).signum(), "toward it");
        // Lingering just past her hair (the rig's r41 case): she still gets there, short or past it.
        for g in [20, 60, 110, 140] {
            let c = (her.body.right() + g, her.anchor.1 - 100);
            let w = World { her, cursor: c, ..base.clone() };
            let s = resolve(&Goal::Hop { kind: HopKind::Pounce, away: false }, &w, &mut Rng::new(3)).unwrap_or_else(|e| panic!("{g}: {e:?}"));
            assert!(s.anchor.0 > her.anchor.0 && !s.body.grow(w.d(CURSOR_CLEAR)).contains(c.0, c.1), "{g}: {s:?}");
        }
        // Already beside her with nothing farther on its side in reach: no pounce (never a hop away).
        let w = World { her, cursor: (her.anchor.0 + 150, her.anchor.1 - 100), ..base.clone() };
        if let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Pounce, away: false }, &w, &mut Rng::new(3)) {
            assert!(s.anchor.0 > her.anchor.0, "toward it: {s:?}");
        }
    }

    #[test]
    fn content_cost_counts_her_whole_outline_but_not_her_perch() {
        let fx = desk();
        let w = fx.world();
        // Standing just right of window 15 (700..1400, content from y 1150 + title): her body
        // box clears it, her hair's reach doesn't.
        let s = w.spot(SpotKind::Floor, (1400 + 125, w.floor_y(0)), w.boxes.stand, None);
        assert!(s.body.x >= 1400);
        assert!(content_cost(&w, &s) > 0.0, "the hair over its text counts");
        // Sitting on window 11's top: her legs over it are the pose, not a cost.
        let seat = w.spot(SpotKind::Edge { win: 11 }, (600, 420), w.boxes.sit, None);
        assert_eq!(content_cost(&w, &seat), 0.0);
    }

    #[test]
    fn startle_leaves_the_window_and_dodge_gets_away_from_the_pointer() {
        let fx = desk();
        let base = fx.world();
        let seat = base.spot(SpotKind::Edge { win: 11 }, (600, 420), base.boxes.sit, None);
        let mut rng = Rng::new(41);
        for _ in 0..200 {
            let w = World { her: seat, ..base.clone() };
            let s = resolve(&Goal::Hop { kind: HopKind::Startle, away: false }, &w, &mut rng).unwrap();
            assert_ne!(s.win(), Some(11), "off the dragged window");
            let c = (seat.body.right() + 70, seat.body.y + 100);
            let w = World { her: seat, cursor: c, ..base.clone() };
            if let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Dodge, away: false }, &w, &mut rng) {
                let d = |b: &WinRect| (dist2(b, c.0, c.1) as f64).sqrt();
                assert!(d(&s.body) >= d(&seat.body) + 100.0, "{s:?}");
                assert!(!s.body.grow(64).contains(c.0, c.1));
            }
        }
        // With another window's top in reach, the reactions land on a top more often than not
        // (her favourite spot, Haseeb 22:55).
        let mut fx = desk();
        fx.stack.push(r(21, 1050, 520, 220, 400));
        fx.eligible.push(true);
        let base = fx.world();
        let seat = base.spot(SpotKind::Edge { win: 11 }, (900, 420), base.boxes.sit, None);
        let (mut startle_tops, mut dodge_tops, mut dodge_n) = (0, 0, 0);
        for seed in 0..1000u64 {
            let w = World { her: seat, ..base.clone() };
            let s = resolve(&Goal::Hop { kind: HopKind::Startle, away: false }, &w, &mut Rng::new(2 * seed + 1)).unwrap();
            startle_tops += usize::from(s.win().is_some());
            let w = World { her: seat, cursor: (seat.body.x - 60, seat.body.y + 120), ..base.clone() };
            if let Ok(s) = resolve(&Goal::Hop { kind: HopKind::Dodge, away: false }, &w, &mut Rng::new(2 * seed + 1)) {
                dodge_n += 1;
                dodge_tops += usize::from(s.win().is_some());
            }
        }
        assert!(startle_tops > 550, "startle: {startle_tops}/1000 on a window top");
        assert!(dodge_n > 500 && dodge_tops * 10 > dodge_n * 6, "dodge: {dodge_tops}/{dodge_n} on a window top");
    }

    #[test]
    fn focus_edge_far_end_within_cover_max() {
        let mut fx = desk();
        fx.cursor = (1500, 700);
        let w = fx.world();
        let s = resolve(&Goal::FocusEdge, &w, &mut Rng::new(1)).unwrap();
        let f = fx.focused.unwrap();
        assert_eq!(s.kind, SpotKind::SitEdge { win: 12 });
        assert!(s.anchor.0 > f.center().0, "the end farther from the cursor: {s:?}");
        assert!(s.cover_pct <= 12.0 && guard(&w, &s).is_ok());
        // A small window: covering more than cover_max_pct of it is refused (beside instead).
        let mut fx = desk();
        let small = r(20, 1500, 900, 360, 260);
        fx.stack.push(small);
        fx.eligible.push(true);
        fx.focused = Some(small);
        fx.cursor = (1600, 1000);
        let w = fx.world();
        if let Ok(s) = resolve(&Goal::FocusEdge, &w, &mut Rng::new(2)) {
            assert!(s.cover_pct <= 12.0, "{s:?}");
            assert!(s.kind != SpotKind::SitEdge { win: 20 } || s.cover_pct <= 12.0);
        }
        // An i3 tile under the bar (no head room): the floor beside it, never over it.
        let fx = tiled();
        let w = fx.world();
        if let Ok(s) = resolve(&Goal::FocusEdge, &w, &mut Rng::new(3)) {
            assert!(!s.covers_focus() && s.kind == SpotKind::Beside, "{s:?}");
        }
    }

    #[test]
    fn perch_share_long_sim() {
        // Her own clocks for 8 h on the desk (his settings: hang_only), every move resolved
        // here, his focus and pointer moving, a screen event every 1–3 min (focus, pointer
        // swipes and rests, her window dragged): she spends most of the time on a window top,
        // and lands on one most often after a roam and after a reaction too.
        use crate::brain::{Arrived, Brain, BrainEvent, Intent, PlaceKind, Screen, Senses, Tuning};
        use crate::settings::{Attention, BrainSettings};
        let fx = desk();
        let wins: Vec<WinRect> = fx.stack.clone();
        for seed in [1u64, 4, 6] {
            let st = BrainSettings { attention: Attention::HangOnly, ..Default::default() };
            let mut b = Brain::new(seed, st.clone(), Tuning::new(&st), None, 0.0);
            let mut rng = Rng::new(seed ^ 0x77);
            let mut prng = Rng::new(seed ^ 0x99);
            let base = fx.world();
            let mut her = base.her;
            let (mut last_wins, mut last_kind): (Vec<u32>, Option<SpotKind>) = (Vec::new(), None);
            let (mut t, mut since) = (0.0f64, 0.0f64);
            let mut time: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
            let mut goals: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            let (mut focus, mut cursor) = (wins[1], (1700, 700));
            let end = 8.0 * 3600.0;
            let mut next_ev = 90.0;
            let (mut react_n, mut react_tops) = (0usize, 0usize);
            while t < end {
                if prng.range(0.0, 1.0) < 0.02 {
                    focus = wins[prng.pick(wins.len())];
                    cursor = (focus.x + prng.pick(focus.w as usize) as i32, focus.y + prng.pick(focus.h as usize) as i32);
                }
                let placement = match her.kind {
                    SpotKind::Edge { .. } | SpotKind::SitEdge { .. } => PlaceKind::Perch,
                    SpotKind::Float | SpotKind::FloatBeside => PlaceKind::Float,
                    SpotKind::Hang { .. } | SpotKind::Cling { .. } | SpotKind::Peek => PlaceKind::Hang,
                    _ => PlaceKind::Ground,
                };
                let d = b.tick(&Senses { now: t, placement, ..Senses::default() });
                for (_, i) in d.intents {
                    let Intent::Move(p) = i else { continue };
                    let lw = last_wins.clone();
                    let areas_pref = if cursor.0 >= 2560 { 1 } else { 0 };
                    let w = World { her, focused: Some(focus), cursor, last_wins: &lw, last_kind, prefer: Some(areas_pref), ..base.clone() };
                    let res = resolve(&p.goal, &w, &mut rng);
                    *goals.entry(format!("{}->{}", p.why, res.as_ref().map_or("FAIL".to_string(), |s| s.kind.key().to_string()))).or_insert(0) += 1;
                    match res {
                        Ok(s) => {
                            if matches!(p.why, "startle_drag" | "dodge" | "focus") {
                                react_n += 1;
                                react_tops += usize::from(s.win().is_some());
                            }
                            // Protected all along: cover_max_pct, and never on the area he works in
                            // (a visit's approach and the focus hop's seat on his window's top edge
                            // may overlap it within cover_max_pct, far from his pointer; nothing else).
                            assert!(s.cover_pct <= w.cover_max_pct + 1e-9, "seed {seed}: {} covers {:.1} %", p.why, s.cover_pct);
                            if !matches!(p.goal, Goal::Active { .. } | Goal::FocusEdge) {
                                assert!(!s.covers_focus() || w.floor_strip_ok(&s, &focus), "seed {seed}: {} onto his window {s:?}", p.why);
                            }
                            *time.entry(if her.win().is_some() { "top" } else { "other" }).or_insert(0.0) += t - since;
                            since = t;
                            if let SpotKind::Edge { win } = s.kind {
                                last_wins.push(win);
                                if last_wins.len() > 2 {
                                    last_wins.remove(0);
                                }
                            }
                            if matches!(p.goal, Goal::Passive { .. } | Goal::Explore) {
                                last_kind = Some(s.kind);
                            }
                            her = s;
                            let covering = s.approach.is_some_and(|a| a.covers()) && s.covers_focus();
                            b.on_event(BrainEvent::Arrived(Arrived { approach: s.approach.filter(|_| matches!(p.goal, Goal::Active { .. })), arrival: p.arrivals[0], covering, why: p.why }), t);
                        }
                        Err(_) => b.on_event(BrainEvent::MoveFailed { busy: false, why: p.why }, t),
                    }
                }
                let wake = d.wake_at.min(end);
                if next_ev <= wake {
                    t = next_ev;
                    let s = match prng.pick(4) {
                        0 => Screen::Focus,
                        1 => Screen::CursorHover,
                        2 if placement == PlaceKind::Perch => Screen::PerchDragged { speed: 3000.0 },
                        _ => Screen::CursorSwipe { speed: 3000.0 },
                    };
                    b.on_event(BrainEvent::Screen(s), t);
                    next_ev += 60.0 + prng.range(0.0, 120.0) as f64;
                } else {
                    t = wake;
                }
            }
            *time.entry(if her.win().is_some() { "top" } else { "other" }).or_insert(0.0) += t - since;
            let share = time["top"] / (time["top"] + time.get("other").copied().unwrap_or(0.0));
            let roams: usize = goals.iter().filter(|(k, _)| k.starts_with("roam->")).map(|x| x.1).sum();
            let roam_tops = goals.get("roam->edge").copied().unwrap_or(0);
            assert!(roam_tops * 10 >= roams * 7, "seed {seed}: {roam_tops} of {roams} roams on a window top ({goals:?})");
            assert!(share > 0.6, "seed {seed}: on a window top {share:.3} of the time ({goals:?})");
            // After reactions the top wins whenever one is in reach (the test above); on this
            // sparse desk a startled hop off a dragged window often has none within 300 px.
            assert!(react_n >= 20 && react_tops * 20 >= react_n * 7, "seed {seed}: after reactions {react_tops}/{react_n} on a window top ({goals:?})");
        }
    }

    // ---- i3 on bella: a workspace switch changes one monitor ---------------------------------
    // Haseeb: "whenever I change workspaces she tends to go to my other monitor, especially when
    // sitting"; "she bugs out when I change workspaces". His live log: `why=workspace
    // goal=follow spot=edge 1842,818 -> 4595,738` and at once `why=focus goal=focus_edge
    // spot=beside 4595,738 -> 2388,1430`: eDP, DP-3, eDP again, in mid-air at DP-3's floor.

    /// i3 as on bella: eDP-1-1 (monitor 0) has workspace 2, DP-3 (monitor 1) has 1, 3 and 4.
    /// A switch shows the workspace on its own monitor and focuses it; the pointer warps there
    /// when the monitor changes (mouse_warping output). The other monitor keeps what it shows.
    struct I3 {
        wins: Vec<(u8, WinRect)>,
        shown: [u8; 2],
        on: usize,
        cursor: (i32, i32),
    }

    impl I3 {
        fn bella() -> I3 {
            I3 {
                wins: vec![
                    // 2 (eDP): a vertical split, the lower tile's top at 818 (her perch in the log).
                    (2, r(21, 0, 29, 2560, 789)),
                    (2, r(22, 0, 818, 2560, 782)),
                    // 3 (DP-3): a vertical split, the lower tile's top at 738.
                    (3, r(31, 2560, 29, 2560, 709)),
                    (3, r(32, 2560, 738, 2560, 702)),
                    // 4 (DP-3): one tile under the bar (no room on top).
                    (4, r(41, 2560, 29, 2560, 1411)),
                    // 1 (DP-3): a tile on the left half, a floating window with room on top.
                    (1, r(11, 2560, 29, 1280, 1411)),
                    (1, r(12, 4000, 600, 1000, 600)),
                ],
                shown: [2, 4],
                on: 0,
                cursor: (1300, 400),
            }
        }

        fn switch(&mut self, ws: u8) {
            let o = usize::from(ws != 2);
            self.shown[o] = ws;
            if o != self.on {
                self.cursor = MONS[o].center();
            }
            self.on = o;
        }

        fn stack(&self) -> Vec<WinRect> {
            self.wins.iter().filter(|(ws, _)| self.shown.contains(ws)).map(|w| w.1).collect()
        }

        /// The window under the pointer on the focused workspace, else its first.
        fn focused(&self) -> Option<WinRect> {
            let mine: Vec<WinRect> = self.wins.iter().filter(|(ws, _)| *ws == self.shown[self.on]).map(|w| w.1).collect();
            mine.iter().copied().find(|w| w.contains(self.cursor.0, self.cursor.1)).or(mine.first().copied())
        }

        fn desk(&self) -> Vec<(u32, usize)> {
            let st = self.stack();
            desk_windows(&st, &vec![true; st.len()], &AREAS, &MONS)
        }
    }

    /// A move as the test saw it: when, why, from and to (monitor, anchor), the spot kind.
    #[derive(Clone, Copy, Debug)]
    struct Hop {
        t: f64,
        why: &'static str,
        from: (usize, (i32, i32)),
        to: (usize, (i32, i32)),
        kind: SpotKind,
    }

    /// Capri on top of `I3`, as brainview runs her: a switch is looked at a beat later
    /// (`desk_settled`); her perch unmapped by it is `stranded` (`brain_perch_gone`); the focus
    /// change a switch makes is the workspace rule's (no `Screen::Focus`); every move resolves on
    /// that moment's world (`world_buf`, monitor = follow) and lands at once.
    struct Desk {
        i3: I3,
        b: crate::brain::Brain,
        her: Spot,
        asleep: bool,
        t: f64,
        /// `desk_windows` at the last look (before a switch).
        seen: Vec<(u32, usize)>,
        rng: Rng,
        hops: Vec<Hop>,
        /// What happened, in the live log's words (`brain move why=… goal=… spot=… a -> b`).
        log: Vec<String>,
    }

    impl Desk {
        /// She sits on `win`'s top at x, else stands on the floor of x's monitor.
        fn new(seed: u64, i3: I3, win: Option<u32>, x: i32) -> Desk {
            use crate::brain::{Brain, Tuning};
            use crate::settings::{Attention, BrainSettings};
            let st = BrainSettings { attention: Attention::Off, ..Default::default() };
            let mut tu = Tuning::new(&st);
            // Only the screen moves her here (no roams, no micro-hops); every focus change hops.
            (tu.roam_s, tu.micro_s, tu.first_roam_min_s, tu.focus_p) = ((1e7, 1e7), (1e7, 1e7), 1e7, 1.0);
            let b = Brain::new(2 * seed + 1, st, tu, None, 0.0);
            let area = usize::from(x >= 2560);
            let her = match win {
                Some(id) => {
                    let w = i3.wins.iter().find(|w| w.1.id == id).unwrap().1;
                    Spot { kind: SpotKind::Edge { win: id }, anchor: (x, w.y), body: boxes().sit.offset(x, w.y), area, approach: None, cover_pct: 0.0 }
                }
                None => {
                    let y = AREAS[area].bottom() - 12;
                    Spot { kind: SpotKind::Floor, anchor: (x, y), body: boxes().stand.offset(x, y), area, approach: None, cover_pct: 0.0 }
                }
            };
            let seen = i3.desk();
            Desk { i3, b, her, asleep: false, t: 0.0, seen, rng: Rng::new(seed ^ 0x5157), hops: Vec::new(), log: Vec::new() }
        }

        fn senses(&self) -> crate::brain::Senses {
            use crate::brain::PlaceKind;
            let placement = match self.her.kind {
                SpotKind::Edge { .. } | SpotKind::SitEdge { .. } => PlaceKind::Perch,
                SpotKind::Float | SpotKind::FloatBeside => PlaceKind::Float,
                SpotKind::Hang { .. } | SpotKind::Cling { .. } | SpotKind::Peek => PlaceKind::Hang,
                _ => PlaceKind::Ground,
            };
            crate::brain::Senses { now: self.t, placement, asleep: self.asleep, ..Default::default() }
        }

        /// Tick (every 50 ms, as often as the view could) and land what she decides.
        fn run_to(&mut self, until: f64) {
            while self.t < until - 1e-9 {
                self.t = (self.t + 0.05).min(until);
                self.decide();
            }
        }

        fn decide(&mut self) {
            use crate::brain::{Arrived, BrainEvent, Intent, React};
            let d = self.b.tick(&self.senses());
            for (_, i) in d.intents {
                match i {
                    Intent::Move(p) => {
                        let stack = self.i3.stack();
                        let eligible = vec![true; stack.len()];
                        let cursor = self.i3.cursor;
                        let focused = self.i3.focused();
                        // world_buf: monitor = "follow".
                        let cursor_area = AREAS.iter().position(|a| a.contains(cursor.0, cursor.1));
                        let only = if p.goal == Goal::Leave { follow_area(&MONS, focused, cursor) } else { None };
                        let prefer = if matches!(p.goal, Goal::Passive { .. } | Goal::Explore | Goal::Rescue | Goal::Follow) { cursor_area } else { None };
                        let fs = [false; 2];
                        let w = World {
                            stack: &stack,
                            eligible: &eligible,
                            focused,
                            areas: &AREAS,
                            monitors: &MONS,
                            fullscreen: &fs,
                            cursor,
                            her: self.her,
                            boxes: boxes(),
                            contact: (-110, 130),
                            margin: 65,
                            floor_pad: 12,
                            sf: 1.0,
                            cover_max_pct: 12.0,
                            last_wins: &[],
                            last_kind: None,
                            hud_h: 0,
                            only,
                            prefer,
                        };
                        match resolve(&p.goal, &w, &mut self.rng) {
                            Ok(s) => {
                                let mon = ["eDP", "DP-3"];
                                self.log.push(format!(
                                    "t={:6.2} brain move why={} goal={} spot={} {},{} -> {},{}  ({} -> {})",
                                    self.t,
                                    p.why,
                                    match p.goal {
                                        Goal::Follow => "follow",
                                        Goal::FocusEdge => "focus_edge",
                                        Goal::Rescue => "rescue",
                                        _ => "other",
                                    },
                                    s.kind.key(),
                                    self.her.anchor.0,
                                    self.her.anchor.1,
                                    s.anchor.0,
                                    s.anchor.1,
                                    mon[self.her.area],
                                    mon[s.area]
                                ));
                                self.hops.push(Hop { t: self.t, why: p.why, from: (self.her.area, self.her.anchor), to: (s.area, s.anchor), kind: s.kind });
                                self.her = s;
                                let a = Arrived { approach: None, arrival: p.arrivals[0], covering: false, why: p.why };
                                self.b.on_event(BrainEvent::Arrived(a), self.t);
                            }
                            Err(_) => {
                                self.log.push(format!("t={:6.2} brain move why={} found no spot: stays", self.t, p.why));
                                self.b.on_event(BrainEvent::MoveFailed { busy: false, why: p.why }, self.t);
                                if self.perch_gone() {
                                    self.fall();
                                }
                            }
                        }
                    }
                    Intent::React(React::Fall) => {
                        self.log.push(format!("t={:6.2} fall", self.t));
                        self.fall();
                    }
                    _ => {}
                }
            }
        }

        fn perch_gone(&self) -> bool {
            self.her.win().is_some_and(|w| !self.i3.stack().iter().any(|r| r.id == w))
        }

        fn fall(&mut self) {
            let y = AREAS[self.her.area].bottom() - 12;
            self.her = Spot { kind: SpotKind::Floor, anchor: (self.her.anchor.0, y), body: boxes().stand.offset(self.her.anchor.0, y), ..self.her };
        }

        fn feed(&mut self, s: crate::brain::Screen) {
            self.b.on_event(crate::brain::BrainEvent::Screen(s), self.t);
            self.decide();
        }

        /// He focuses another window on the monitor he is on (no switch: across monitors i3
        /// would change the workspace too).
        fn focus(&mut self, id: u32) {
            self.log.push(format!("t={:6.2} he focuses window {id}", self.t));
            let (ws, w) = *self.i3.wins.iter().find(|w| w.1.id == id).unwrap();
            assert_eq!(usize::from(ws != 2), self.i3.on, "a focus change on his monitor");
            self.i3.cursor = w.center();
            self.feed(crate::brain::Screen::Focus);
            self.seen = self.i3.desk();
        }

        /// `$mod+N`: i3 switches; her perch, if it went with it, is reported 300 ms on
        /// (`PERCH_GONE_GRACE`), else the switch is looked at 450 ms on.
        fn switch(&mut self, ws: u8) {
            self.i3.switch(ws);
            self.log.push(format!("t={:6.2} $mod+{ws}: eDP shows {}, DP-3 shows {}, he is on {}", self.t, self.i3.shown[0], self.i3.shown[1], ["eDP", "DP-3"][self.i3.on]));
            let until = self.t + 0.3;
            self.run_to(until);
            let after = self.i3.desk();
            if self.perch_gone() {
                let s = workspace_switch(&MONS, self.i3.focused(), self.i3.cursor, self.her.area, true, Some(&self.seen), &after);
                self.feed(s);
            } else {
                let until = self.t + 0.15;
                self.run_to(until);
                let s = workspace_switch(&MONS, self.i3.focused(), self.i3.cursor, self.her.area, false, Some(&self.seen), &after);
                self.feed(s);
            }
            self.seen = after;
        }

        /// The moves the screen made (a workspace switch, a focus change, a closed perch).
        fn relocations(&self) -> Vec<Hop> {
            self.hops.iter().copied().filter(|h| matches!(h.why, "workspace" | "focus" | "perch_closed")).collect()
        }

        /// Never two of her own moves within 10 s of a relocation (no chain, no ping-pong), and
        /// on the floor only at her monitor's floor (never in mid-air).
        fn assert_no_chain(&self, seed: u64) {
            for (i, h) in self.hops.iter().enumerate() {
                if matches!(h.kind, SpotKind::Floor | SpotKind::FloorSit | SpotKind::Beside) {
                    assert_eq!(h.to.1.1, AREAS[h.to.0].bottom() - 12, "seed {seed}: mid-air {h:?}");
                }
                if matches!(h.why, "workspace" | "focus" | "perch_closed")
                    && let Some(n) = self.hops.get(i + 1)
                {
                    assert!(n.t - h.t >= 10.0 - 1e-6, "seed {seed}: {h:?} then {n:?}");
                }
            }
        }
    }

    #[test]
    fn i3_switch_on_the_other_monitor_leaves_her_be() {
        // She is on eDP (workspace 2): sitting on the lower tile like in his log, or on the
        // floor. He goes to DP-3 and back and around there: eDP never changed, she stays put.
        for seed in 0..60 {
            for (win, x) in [(Some(22), 1437), (Some(22), 1842), (None, 1437)] {
                let mut d = Desk::new(seed, I3::bella(), win, x);
                let start = d.her;
                d.run_to(50.0);
                d.switch(3); // eDP -> DP-3 (DP-3: 4 -> 3)
                d.run_to(80.0);
                d.switch(4); // on DP-3 (3 -> 4)
                d.run_to(110.0);
                d.switch(2); // back to eDP (it showed 2 all along)
                d.run_to(140.0);
                d.switch(1); // DP-3 (4 -> 1)
                d.run_to(170.0);
                assert!(d.hops.is_empty(), "seed {seed} {win:?}: {:?}", d.hops);
                assert_eq!(d.her, start, "seed {seed}");
            }
        }
    }

    #[test]
    fn i3_switch_on_her_monitor_moves_her_on_it() {
        for seed in 0..60 {
            // She sits on workspace 3's lower tile (DP-3), he works there.
            let mut i3 = I3::bella();
            i3.switch(3);
            i3.cursor = (3000, 300);
            let mut d = Desk::new(seed, i3, Some(32), 4595);
            d.run_to(50.0);
            // DP-3: 3 -> 4. Her perch went with 3: she lands on DP-3 at once (4 has no window
            // top: the floor), never on eDP.
            d.switch(4);
            d.run_to(70.0);
            let r = d.relocations();
            assert_eq!(r.len(), 1, "seed {seed}: {r:?}");
            assert!(r[0].why == "workspace" && r[0].t < 51.0 && r[0].to.0 == 1, "seed {seed}: {r:?}");
            // DP-3: 4 -> 1 while she stands on its floor: a window top on 1 there (not his
            // focused tile), in 0.8–2 s.
            d.switch(1);
            d.run_to(100.0);
            let r = d.relocations();
            assert_eq!(r.len(), 2, "seed {seed}: {r:?}");
            assert!(r[1].why == "workspace" && r[1].to.0 == 1 && r[1].kind == SpotKind::Edge { win: 12 }, "seed {seed}: {r:?}");
            assert!((70.8..=72.5).contains(&r[1].t), "seed {seed}: {r:?}");
            d.assert_no_chain(seed);
        }
    }

    #[test]
    fn i3_focus_then_switch_is_one_move_at_most() {
        // His log's pair: a focus change (the hop waits 2–5 s), then a switch to DP-3 within a
        // second. Not two teleports back to back, not over to DP-3 and back, never mid-air.
        for seed in 0..60 {
            let mut d = Desk::new(seed, I3::bella(), Some(22), 1842);
            d.run_to(50.0);
            d.focus(21);
            d.run_to(50.6);
            d.switch(3);
            d.run_to(80.0);
            d.assert_no_chain(seed);
            assert!(d.hops.iter().all(|h| h.from.0 == 0 && h.to.0 == 0), "seed {seed}: she stays on eDP {:?}", d.hops);
            assert!(d.relocations().iter().all(|h| h.why != "workspace"), "seed {seed}: {:?}", d.hops);
        }
    }

    #[test]
    fn i3_replay_his_session() {
        // His evening as a script, through the real decision path; `cargo test i3_replay --
        // --nocapture` prints the move log. She sits on eDP's lower tile (1437,818); he hops
        // between the monitors; then he puts her on DP-3's floor and switches DP-3 under her.
        for seed in 0..60 {
            let mut d = Desk::new(seed, I3::bella(), Some(22), 1437);
            for (t, ws) in [(50.0, 3), (80.0, 2), (95.0, 4), (105.0, 2)] {
                d.run_to(t);
                d.switch(ws);
            }
            d.run_to(110.0);
            d.focus(21);
            d.run_to(110.6);
            d.switch(3);
            d.run_to(140.0);
            d.switch(2);
            d.run_to(160.0);
            let y = AREAS[1].bottom() - 12;
            d.her = Spot { kind: SpotKind::Floor, anchor: (3300, y), body: boxes().stand.offset(3300, y), area: 1, approach: None, cover_pct: 0.0 };
            d.log.push(format!("t={:6.2} he drops her on DP-3's floor at 3300,{y}", d.t));
            for (t, ws) in [(170.0, 4), (190.0, 3), (210.0, 1), (230.0, 2), (245.0, 4)] {
                d.run_to(t);
                d.switch(ws);
            }
            d.run_to(270.0);
            if seed == 0 {
                println!("{}", d.log.join("\n"));
            }
            d.assert_no_chain(seed);
            // On eDP until he moved her: not one move. On DP-3: moves only when DP-3 switched,
            // and only on DP-3.
            assert!(d.hops.iter().all(|h| h.t > 160.0 && h.why == "workspace" && h.from.0 == 1 && h.to.0 == 1), "seed {seed}: {:#?}", d.log);
            // DP-3 to workspace 1 (a free window top there): she goes up on it in 0.8–2 s.
            assert!(d.hops.iter().any(|h| (210.8..212.5).contains(&h.t) && h.kind == SpotKind::Edge { win: 12 }), "seed {seed}: {:#?}", d.log);
            // Back on eDP, then DP-3 to 4 from there: her window top went with 1, she lands on
            // DP-3's floor at once (4 has no free top).
            assert!(d.hops.iter().any(|h| (245.0..245.5).contains(&h.t) && matches!(h.kind, SpotKind::Floor | SpotKind::FloorSit)), "seed {seed}: {:#?}", d.log);
        }
    }

    #[test]
    fn i3_replay_0252_workspace_focus_workspace() {
        // His log, 02:52: `why=workspace 1356,818 -> 4058,1430 teleport`, `why=focus 4058,1430
        // -> 2360,1590 teleport`, `why=workspace 2360,1590 -> 3004,1430 glitch`, back to back.
        let mon = |d: &Desk| d.her.area;
        for seed in 0..60 {
            // As it began: she sits on eDP's lower tile at 1356,818. He goes to DP-3, clicks a
            // window there, comes back to eDP, switches DP-3: seconds apart.
            let mut d = Desk::new(seed, I3::bella(), Some(22), 1356);
            d.run_to(50.0);
            d.switch(3);
            d.run_to(51.6);
            d.focus(31);
            d.run_to(53.0);
            d.switch(2);
            d.run_to(54.0);
            d.switch(4);
            d.run_to(80.0);
            if seed == 0 {
                println!("{}", d.log.join("\n"));
            }
            d.assert_no_chain(seed);
            // No switch moved her (eDP never changed while she was on it); his click on DP-3
            // may take her there once; nothing brings her back: one monitor change at most.
            assert!(d.hops.iter().all(|h| h.why == "focus"), "seed {seed}: {:#?}", d.log);
            assert!(d.hops.len() <= 1, "seed {seed}: {:#?}", d.log);
            // The switch first on her monitor (she is on DP-3's floor, DP-3 goes 4 -> 1): she
            // moves on DP-3; his click, his trip to eDP and DP-3's next switch inside the gap
            // don't move her again (her window top went with 1: she drops to DP-3's floor).
            let mut d = Desk::new(seed, I3::bella(), None, 3300);
            d.run_to(50.0);
            d.switch(1);
            d.run_to(53.0);
            let first = d.hops.clone();
            assert!(first.len() == 1 && first[0].why == "workspace" && first[0].to.0 == 1, "seed {seed}: {:#?}", d.log);
            d.run_to(first[0].t + 1.0);
            d.focus(11);
            d.run_to(first[0].t + 2.5);
            d.switch(2);
            d.run_to(first[0].t + 4.0);
            d.switch(4);
            d.run_to(first[0].t + 9.5);
            assert_eq!(d.hops.len(), 1, "seed {seed}: {:#?}", d.log);
            assert_eq!(mon(&d), 1, "seed {seed}: still on DP-3 {:#?}", d.log);
            // Past the gap, DP-3 switching again moves her once more, on DP-3.
            d.run_to(first[0].t + 15.0);
            d.switch(1);
            d.run_to(first[0].t + 30.0);
            d.assert_no_chain(seed);
            assert!(d.hops.iter().all(|h| h.from.0 == 1 && h.to.0 == 1), "seed {seed}: only on DP-3 {:#?}", d.log);
            if seed == 0 {
                println!("--\n{}", d.log.join("\n"));
            }
        }
    }

    #[test]
    fn i3_asleep_she_stays_put() {
        for seed in 0..60 {
            // On eDP's tile top, asleep: he works around on DP-3.
            let mut d = Desk::new(seed, I3::bella(), Some(22), 1437);
            d.asleep = true;
            let start = d.her;
            for (t, ws) in [(50.0, 3), (80.0, 4), (110.0, 2), (140.0, 1)] {
                d.run_to(t);
                d.switch(ws);
            }
            d.run_to(170.0);
            assert!(d.hops.is_empty() && d.her == start, "seed {seed}: {:?}", d.hops);
            // On DP-3's floor, asleep: DP-3 itself switches (4 -> 1, a window top there).
            let mut i3 = I3::bella();
            i3.switch(4);
            let mut d = Desk::new(seed, i3, None, 3300);
            d.asleep = true;
            let start = d.her;
            d.run_to(50.0);
            d.switch(1);
            d.run_to(80.0);
            assert!(d.hops.is_empty() && d.her == start, "seed {seed}: {:?}", d.hops);
        }
    }

    /// The share of floor picks over windows' content (> 20 % of her body over it), and their
    /// mean cost, from `kinds` floor roams over many seeds. `sorted: false`: the first sample
    /// that passes, as before (r38 desk: she parked over the editor's text).
    fn floor_over_content(fx: &Fixture, sorted: bool) -> (f64, f64) {
        let w = fx.world();
        let roam = Roam { far: false, leaving: false, min_move: 0, skip_recent: false, kinds: &[(1, 1.0)], retry_last: false };
        let (mut over, mut sum, n) = (0, 0.0, 600);
        for seed in 0..n {
            let mut rng = Rng::new(seed);
            let s = if sorted {
                passive(&w, &roam, &mut rng).expect("a floor spot")
            } else {
                let mut cands = Vec::new();
                for _ in 0..16 {
                    let i = w.pick_area(&mut rng);
                    let a = w.areas[i];
                    let x = a.x + (a.w as f32 * rng.range(0.1, 0.9)) as i32;
                    cands.push(w.spot(SpotKind::Floor, (x, w.floor_y(i)), w.boxes.stand, None));
                }
                cands.into_iter().find(|s| guard(&w, s).is_ok()).expect("a floor spot")
            };
            let c = content_cost(&w, &s);
            sum += c;
            if c > 0.2 {
                over += 1;
            }
        }
        (over as f64 / n as f64, sum / n as f64)
    }

    #[test]
    fn floor_spots_keep_off_window_content_when_they_can() {
        // The r38 desk: one editor window over the floor's middle; nothing focused.
        let fx = Fixture {
            stack: vec![r(11, 1100, 760, 1100, 560)],
            eligible: vec![true],
            focused: None,
            cursor: (2550, 40),
            fs: [false; 2],
            her: (600, 1588),
        };
        let (before, cb) = floor_over_content(&fx, false);
        let (after, ca) = floor_over_content(&fx, true);
        eprintln!("desk: over content {:.0} % -> {:.0} %, mean cost {cb:.2} -> {ca:.2}", before * 100.0, after * 100.0);
        assert!(before > 0.15, "the fixture means something: {before}");
        assert!(after < 0.02, "{after}");
        // Tiled (i3): windows over the whole floor. Still a spot (a preference, not a rule),
        // and over margins rather than the middle of the text.
        let tiled = Fixture {
            stack: vec![r(11, 0, 29, 1280, 1571), r(12, 1280, 29, 1280, 1571), r(13, 2560, 29, 2560, 1411)],
            eligible: vec![true; 3],
            focused: None,
            cursor: (2550, 40),
            fs: [false; 2],
            her: (600, 1588),
        };
        let (_, cb) = floor_over_content(&tiled, false);
        let (_, ca) = floor_over_content(&tiled, true);
        eprintln!("tiled: mean cost {cb:.2} -> {ca:.2}");
        assert!(ca < cb - 0.3, "{cb} -> {ca}");
    }
}
