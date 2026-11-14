//! Which way she faces. The art is drawn facing one way (the pack's `faces`, right by
//! default; Capri faces left); mirrored, she faces the other. Pure: the hysteresis that
//! turns her on a drag, where she should look on her own, and which art that means.

/// A drag turns her once the pointer's horizontal velocity (logical px/s, the same fit the
/// drag follow uses) has pointed the other way past this for `TURN_HOLD`.
pub const TURN_V: f32 = 200.0;
pub const TURN_HOLD: f32 = 0.11;
/// Never two drag turns closer than this (a shake doesn't strobe her).
pub const TURN_GAP: f32 = 0.30;
/// Context turns ignore targets closer than this (logical px): straight ahead is no turn.
pub const DEAD_PX: f32 = 40.0;
/// Near a monitor edge (this share of its width) she faces into the screen.
pub const EDGE_SHARE: f32 = 0.2;
/// Idle look-arounds: never within this of another turn (s).
pub const LOOK_QUIET_S: f64 = 20.0;

/// The velocity she faces along while dragged and flung: `back` (the default) looks back
/// where she's pulled from, against the motion; forward, along it.
pub fn drag_vx(vx: f32, back: bool) -> f32 {
    if back { -vx } else { vx }
}

/// Drag facing with hysteresis.
#[derive(Default, Clone, Debug)]
pub struct Turner {
    /// The way the velocity has pointed (past `TURN_V`), since when.
    pending: Option<(bool, f32)>,
    last: Option<f32>,
}

impl Turner {
    /// `vx` logical px/s at `t` s; `right`: facing right now. `Some(new)` when she turns.
    pub fn feed(&mut self, t: f32, vx: f32, right: bool) -> Option<bool> {
        let want = if vx > TURN_V {
            Some(true)
        } else if vx < -TURN_V {
            Some(false)
        } else {
            None
        };
        match want {
            Some(w) if w != right => {
                let since = match self.pending {
                    Some((d, s)) if d == w => s,
                    _ => {
                        self.pending = Some((w, t));
                        t
                    }
                };
                if t - since >= TURN_HOLD && self.last.is_none_or(|l| t - l >= TURN_GAP) {
                    self.pending = None;
                    self.last = Some(t);
                    return Some(w);
                }
                None
            }
            _ => {
                self.pending = None;
                None
            }
        }
    }
}

/// A turn's shape over its life `u` in 0..1: (sx, sy) about her feet. A one-frame
/// anticipation first (she dips 4 % and widens 3 %), then the squeeze, |cos| down to
/// `TURN_MIN_SX` edge-on, where the art swaps (`TURN_SWAP_U`), and back out.
pub const TURN_ANTIC: f32 = 0.18;
pub const TURN_MIN_SX: f32 = 0.32;
pub const TURN_SWAP_U: f32 = TURN_ANTIC + (1.0 - TURN_ANTIC) * 0.5;
pub fn turn_shape(u: f32) -> Option<(f32, f32)> {
    use std::f32::consts::PI;
    if !(0.0..1.0).contains(&u) {
        return None;
    }
    if u < TURN_ANTIC {
        let s = (PI * u / TURN_ANTIC).sin();
        return Some((1.0 + 0.03 * s, 1.0 - 0.04 * s));
    }
    let w = (u - TURN_ANTIC) / (1.0 - TURN_ANTIC);
    Some(((PI * w).cos().abs().max(TURN_MIN_SX), 1.0))
}

/// Without a compositor the window's shape and its pixels land apart (the shape at once, the
/// pixels up to two frames later), so a squeeze, whose pixels change every frame, tears. A
/// turn there keeps the pixels still and animates only the shape: a band about her feet
/// closes over the old art, holds edge-on while the new art lands, then opens over it. Per
/// painted frame `k`: (band width as a share of her half-width, the new art yet), None after.
/// Frame 0 is a full-width pre-roll: nothing shows, the new art is drawn underneath from there
/// on (its mirror is built then, not at the swap: that cost delayed the swap's pixels behind
/// its shape, torn legs). The new art swaps in edge-on, where a shape/pixel mismatch is a
/// sliver, and leads the open by two frames. Edge-on two frames (r16: four read as a stall).
pub const BAND: [(f32, bool); 7] = [(1.0, false), (0.72, false), (0.42, false), (0.16, true), (0.16, true), (0.42, true), (0.72, true)];
/// A turn onto art never shown yet this session: its first frames on screen came out torn for
/// two or three frames after the swap (r17, :0, the first turn of 2 in 3 fresh starts, whatever
/// was drawn ahead), so it holds edge-on until that's through (the r16 timing). Once per frame
/// and side.
pub const BAND_FIRST: [(f32, bool); 8] = [(0.72, false), (0.42, false), (0.16, false), (0.16, true), (0.16, true), (0.16, true), (0.42, true), (0.72, true)];
pub fn turn_band(k: u32, first: bool) -> Option<(f32, bool)> {
    if first { BAND_FIRST.get(k as usize).copied() } else { BAND.get(k as usize).copied() }
}

/// Shown mirrored to face `right`, for art that faces right (`art_right`) or left unmirrored.
pub fn mirrored_for(art_right: bool, right: bool) -> bool {
    right != art_right
}

/// She faces right: the art's own way, or the other one mirrored.
pub fn faces_right(art_right: bool, mirrored: bool) -> bool {
    art_right != mirrored
}

/// Face `target_x` from `x` (root px), unless it is within `dead` px.
pub fn toward(x: i32, target_x: i32, dead: i32) -> Option<bool> {
    ((target_x - x).abs() > dead).then_some(target_x > x)
}

/// Near a monitor edge (`x` in the outer `EDGE_SHARE` of `[mx, mx + mw)`): into the screen.
pub fn inward(x: i32, mx: i32, mw: i32) -> Option<bool> {
    let edge = (mw as f32 * EDGE_SHARE) as i32;
    if x < mx + edge {
        Some(true)
    } else if x >= mx + mw - edge {
        Some(false)
    } else {
        None
    }
}

/// Seconds to the next idle look-around: uniform in `every_min` minutes (`u` in 0..1).
pub fn look_gap(every_min: (f32, f32), u: f32) -> f64 {
    let (lo, hi) = (every_min.0.max(0.05), every_min.1.max(every_min.0.max(0.05)));
    ((lo + (hi - lo) * u.clamp(0.0, 1.0)) * 60.0) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ragdoll::Pointer;

    /// Drive a Turner from pointer samples every 8 ms through the drag's own velocity fit.
    fn run(path: impl Fn(f32) -> f32, secs: f32, right: bool) -> Vec<(f32, bool)> {
        run_as(path, secs, right, false)
    }

    /// The same, facing `back` (against the motion) or forward.
    fn run_as(path: impl Fn(f32) -> f32, secs: f32, mut right: bool, back: bool) -> Vec<(f32, bool)> {
        let mut p = Pointer::default();
        let mut tr = Turner::default();
        let mut turns = Vec::new();
        let mut t = 0.0;
        while t <= secs {
            p.push(t, path(t), 300.0);
            if let Some(r) = tr.feed(t, drag_vx(p.velocity(t).0, back), right) {
                right = r;
                turns.push((t, r));
            }
            t += 0.008;
        }
        turns
    }

    /// Capri's art faces left: going right shows her mirrored, going left unmirrored; a
    /// right-facing pack the other way round. Every rule decides a side, then the art.
    #[test]
    fn a_left_facing_pack_mirrors_to_go_right() {
        let left_art = false;
        // A drag to the right, from rest (unmirrored: facing left).
        let turns = run(|t| 300.0 + 500.0 * t, 0.6, faces_right(left_art, false));
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert!(turns[0].1, "turns to face right");
        assert!(mirrored_for(left_art, true), "right = the flipped art");
        // ...then back left: the art as drawn.
        let turns = run(|t| 900.0 - 500.0 * t, 0.6, faces_right(left_art, true));
        assert_eq!(turns.iter().map(|t| t.1).collect::<Vec<_>>(), vec![false]);
        assert!(!mirrored_for(left_art, false));
        // Already facing the way she's dragged: no turn.
        assert!(run(|t| 900.0 - 500.0 * t, 0.6, faces_right(left_art, false)).is_empty());
        // A right-facing pack: the opposite art for the same sides.
        assert!(!mirrored_for(true, true) && mirrored_for(true, false));
        for (art, m) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(mirrored_for(art, faces_right(art, m)), m);
        }
    }

    /// Haseeb's pick (`drag_facing = "back"`): dragged, she looks back where she's pulled from,
    /// and a fling keeps that side, so letting go never flips her mid-air.
    #[test]
    fn dragged_back_she_faces_against_the_motion_and_the_fling_keeps_it() {
        let left_art = false;
        // Dragged right from rest (facing left, as drawn): already looking back, no turn.
        assert!(run_as(|t| 300.0 + 500.0 * t, 0.6, faces_right(left_art, false), true).is_empty());
        // Dragged left from rest: she turns to face right (the flipped art), once.
        let turns = run_as(|t| 900.0 - 500.0 * t, 0.6, faces_right(left_art, false), true);
        assert_eq!(turns.iter().map(|t| t.1).collect::<Vec<_>>(), vec![true], "{turns:?}");
        assert!(mirrored_for(left_art, true));
        // Right then left: left, then right.
        let path = |t: f32| if t < 0.6 { 400.0 + 500.0 * t } else { 700.0 - 500.0 * (t - 0.6) };
        let turns = run_as(path, 1.2, true, true);
        assert_eq!(turns.iter().map(|t| t.1).collect::<Vec<_>>(), vec![false, true], "{turns:?}");
        // Let go moving right at the end of a rightward drag: the fling faces left, the side
        // the drag already showed.
        let (drag, fling) = (run_as(|t| 300.0 + 500.0 * t, 0.6, true, true), drag_vx(500.0, true) > 0.0);
        assert_eq!(drag.last().map(|t| t.1), Some(false));
        assert!(!fling);
        // Forward: along the motion, as before.
        assert_eq!(run_as(|t| 300.0 + 500.0 * t, 0.6, false, false).iter().map(|t| t.1).collect::<Vec<_>>(), vec![true]);
        assert!(drag_vx(500.0, false) > 0.0);
        // Jitter stays still either way.
        for back in [true, false] {
            assert!(run_as(|t: f32| 500.0 + 12.0 * (std::f32::consts::TAU * 4.0 * t).sin(), 3.0, true, back).is_empty());
        }
    }

    #[test]
    fn a_left_facing_pack_looks_at_the_pointer_into_the_screen_and_at_the_window() {
        let left_art = false;
        // His pointer to her right: face right = flipped; to her left: as drawn.
        assert_eq!(toward(500, 900, 40).map(|r| mirrored_for(left_art, r)), Some(true));
        assert_eq!(toward(500, 100, 40).map(|r| mirrored_for(left_art, r)), Some(false));
        // Near the left edge of a monitor she faces into it (right): flipped. Right edge: as drawn.
        assert_eq!(inward(100, 0, 1920).map(|r| mirrored_for(left_art, r)), Some(true));
        assert_eq!(inward(1850, 0, 1920).map(|r| mirrored_for(left_art, r)), Some(false));
        assert_eq!(inward(2000, 1920, 2560).map(|r| mirrored_for(left_art, r)), Some(true));
        // On a window's edge she faces its middle: a window to her right = flipped.
        let (win_x, win_w) = (600, 800);
        assert_eq!(toward(610, win_x + win_w / 2, 40).map(|r| mirrored_for(left_art, r)), Some(true));
        assert_eq!(toward(1390, win_x + win_w / 2, 40).map(|r| mirrored_for(left_art, r)), Some(false));
    }

    #[test]
    fn the_capri_pack_faces_left() {
        let b: crate::pack::Behaviour = serde_json::from_str(include_str!("../packs/capri/moods.json")).expect("moods.json");
        assert!(!b.art_faces_right());
        let d: crate::pack::Behaviour = serde_json::from_str("{}").expect("empty");
        assert!(d.art_faces_right(), "other packs default to right");
        let old: crate::pack::Behaviour = serde_json::from_str(r#"{"facing": "left"}"#).expect("alias");
        assert!(!old.art_faces_right());
    }

    #[test]
    fn the_turn_dips_then_squeezes_never_thinner_than_the_floor_and_swaps_edge_on() {
        assert_eq!(turn_shape(0.0), Some((1.0, 1.0)));
        assert_eq!(turn_shape(1.0), None);
        let (ax, ay) = turn_shape(TURN_ANTIC / 2.0).unwrap();
        assert!(ax > 1.0 && ay < 0.97, "anticipation: {ax} {ay}");
        let at_swap = turn_shape(TURN_SWAP_U).unwrap();
        assert!((at_swap.0 - TURN_MIN_SX).abs() < 1e-4 && at_swap.1 == 1.0, "{at_swap:?}");
        let mut u = 0.0;
        while u < 1.0 {
            let (sx, sy) = turn_shape(u).unwrap();
            assert!((TURN_MIN_SX..=1.031).contains(&sx) && (0.959..=1.0).contains(&sy), "{u}: {sx} {sy}");
            u += 0.01;
        }
        let (ex, _) = turn_shape(0.99).unwrap();
        assert!(ex > 0.99, "ends full width: {ex}");
    }

    #[test]
    fn shape_mode_turn_closes_holds_edge_on_and_opens_once() {
        let first: Vec<(f32, bool)> = (0..).map_while(|k| turn_band(k, true)).collect();
        let fs = first.iter().position(|s| s.1).unwrap();
        assert!(first.iter().rposition(|s| s.0 == first[fs].0).unwrap() - fs + 1 >= 3, "first use: a 3-frame lead: {first:?}");
        let b: Vec<(f32, bool)> = (0..).map_while(|k| turn_band(k, false)).collect();
        let swap = b.iter().position(|s| s.1).unwrap();
        assert_eq!(swap, 3);
        assert_eq!(b[0], (1.0, false), "a full-width pre-roll first");
        assert!(b[..swap].windows(2).all(|w| w[1].0 < w[0].0), "closes: {b:?}");
        let min = b.iter().map(|s| s.0).fold(f32::MAX, f32::min);
        let last_min = b.iter().rposition(|s| s.0 == min).unwrap();
        assert_eq!(b[swap].0, min, "the new art swaps in edge-on: {b:?}");
        assert!(last_min - swap + 1 >= 2, "the new art leads the open by two frames (pixels land late): {b:?}");
        let edge_on = b.iter().filter(|s| s.0 == min).count();
        assert!((1..=2).contains(&edge_on), "edge-on 1-2 frames, not a stall: {b:?}");
        assert!(b[last_min..].windows(2).all(|w| w[1].0 > w[0].0), "one monotonic open: {b:?}");
        assert!(b[swap..].iter().all(|s| s.1) && b[..swap].iter().all(|s| !s.1));
        assert_eq!(turn_band(b.len() as u32, false), None);
        assert_eq!(turn_band(first.len() as u32, true), None);
    }

    #[test]
    fn a_steady_drag_turns_once_after_the_hold() {
        let turns = run(|t| 500.0 - 400.0 * t, 1.0, true);
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert!(!turns[0].1);
        assert!(turns[0].0 >= TURN_HOLD && turns[0].0 < 0.3, "{turns:?}");
    }

    #[test]
    fn right_then_left_turns_twice() {
        let path = |t: f32| if t < 0.6 { 400.0 + 500.0 * t } else { 700.0 - 500.0 * (t - 0.6) };
        let turns = run(path, 1.2, false);
        assert_eq!(turns.iter().map(|t| t.1).collect::<Vec<_>>(), vec![true, false], "{turns:?}");
    }

    #[test]
    fn jitter_never_turns_her() {
        // Small back-and-forth wiggles: up to ±20 px at 2-6 Hz.
        for (amp, hz) in [(4.0, 6.0), (8.0, 5.0), (12.0, 4.0), (20.0, 3.0), (20.0, 2.0)] {
            let turns = run(move |t: f32| 500.0 + amp * (std::f32::consts::TAU * hz * t).sin(), 3.0, true);
            assert!(turns.is_empty(), "±{amp} px at {hz} Hz turned her: {turns:?}");
        }
    }

    #[test]
    fn a_shake_does_not_strobe() {
        // Big fast reversals: she may follow, but never more than once per TURN_GAP.
        let turns = run(|t: f32| 500.0 + 80.0 * (std::f32::consts::TAU * 3.0 * t).sin(), 3.0, true);
        for w in turns.windows(2) {
            assert!(w[1].0 - w[0].0 >= TURN_GAP - 1e-4, "{turns:?}");
        }
    }

    #[test]
    fn context_helpers() {
        assert_eq!(toward(100, 400, 40), Some(true));
        assert_eq!(toward(400, 100, 40), Some(false));
        assert_eq!(toward(400, 420, 40), None);
        assert_eq!(inward(50, 0, 1000), Some(true));
        assert_eq!(inward(950, 0, 1000), Some(false));
        assert_eq!(inward(500, 0, 1000), None);
        assert_eq!(look_gap((1.0, 4.0), 0.0), 60.0);
        assert_eq!(look_gap((1.0, 4.0), 1.0), 240.0);
    }
}
