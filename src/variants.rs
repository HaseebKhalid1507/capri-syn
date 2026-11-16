//! Variant pools (Haseeb, 2026-11-16: "mix, don't replace"). A state with more than one art
//! option keeps a weighted pool: standing idle on the floor (CapriCore's, lachy's two), a window
//! perch (CapriCore's sit, lachy's perch, lachy's laydown), sleep on the floor, sleep on a window. Entering the state picks one: weighted, from a seeded
//! rng (tests stay deterministic), never the same strip variant twice running. The pack's own
//! art may follow itself: it leads every pool, and with two variants a strict no-repeat would
//! make any weights 50/50. A long stay shifts it in place now and then (always to another).
//! Every switch between CapriCore and lachy art (or between two lachy drawings without a drawn
//! transition) goes through a glitch: `seam`. Pure: no OS, no view, no pixels.

use crate::anim::Rng;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// The states with a pool.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PoolKey {
    /// Standing idle on the floor (the Idle mood only: the other moods' poses are the pack's).
    Stand,
    /// Sitting on a window top.
    Perch,
    /// Asleep on the floor.
    SleepFloor,
    /// Asleep on a window top.
    SleepWindow,
}

impl PoolKey {
    pub const ALL: [PoolKey; 4] = [PoolKey::Stand, PoolKey::Perch, PoolKey::SleepFloor, PoolKey::SleepWindow];

    pub fn key(self) -> &'static str {
        match self {
            PoolKey::Stand => "stand",
            PoolKey::Perch => "perch",
            PoolKey::SleepFloor => "sleepFloor",
            PoolKey::SleepWindow => "sleepWindow",
        }
    }

    pub fn parse(s: &str) -> Option<PoolKey> {
        PoolKey::ALL.into_iter().find(|k| k.key().eq_ignore_ascii_case(s.trim()))
    }

    fn slot(self) -> usize {
        self as usize
    }
}

/// `moods.json` `variants`.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct VariantsSpec {
    /// Decoded strip pixels kept around (MB); the least recently used strips not on screen go
    /// first.
    pub cache_mb: Option<u32>,
    pub pools: BTreeMap<String, PoolSpec>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolSpec {
    /// A long stay shifts the pose in place after `shiftMs[0]..[1]` ms (absent: never).
    pub shift_ms: Option<[u64; 2]>,
    /// A shift may keep the pack's own art (then nothing changes): over time each variant holds
    /// about its weight's share. Without it a shift always changes the pose.
    pub shift_stays: bool,
    /// After a drawn arrival or wake (which end in this variant's drawing), she stands in it
    /// `bridgeMs` before the next shift.
    pub bridge: Option<String>,
    pub bridge_ms: Option<[u64; 2]>,
    pub variants: Vec<VariantSpec>,
}

/// One option. Without `strip` it is the pack's own art for that state, as it behaves today.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VariantSpec {
    pub id: String,
    #[serde(default = "one")]
    pub weight: f32,
    #[serde(default)]
    pub strip: Option<String>,
    /// While she talks (the Talking mood) on this variant.
    #[serde(default)]
    pub talk: Option<String>,
    /// While music plays on this variant.
    #[serde(default)]
    pub music: Option<String>,
    /// Blinks played over the loop, weighted.
    #[serde(default)]
    pub blinks: Vec<Weighted>,
    #[serde(default)]
    pub blink_ms: Option<[u64; 2]>,
    /// Play every n-th cell of the strips (each held n times as long): fewer cells decoded and
    /// fewer redraws (anime "on twos" / "on threes").
    #[serde(default)]
    pub every: Option<u32>,
    /// After each pass of the base loop she rests on its first cell for `holdMs[0]..[1]` ms
    /// (drawn once per pick): a sway now and then, not a constant one, and no redraws meanwhile.
    #[serde(default)]
    pub hold_ms: Option<[u64; 2]>,
    /// Lies along the window top: only where the whole figure fits on its exposed stretch.
    #[serde(default)]
    pub lying: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Weighted {
    pub strip: String,
    #[serde(default = "one")]
    pub weight: f32,
}

fn one() -> f32 {
    1.0
}

/// A weighted pick among `weights` (non-positive weights never win). None if none can.
pub fn weighted(rng: &mut Rng, weights: &[f32]) -> Option<usize> {
    let total: f32 = weights.iter().filter(|w| **w > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    let mut r = rng.range(0.0, total);
    let mut last = None;
    for (i, &w) in weights.iter().enumerate() {
        if w <= 0.0 {
            continue;
        }
        last = Some(i);
        if r < w {
            return Some(i);
        }
        r -= w;
    }
    last
}

/// The picks, per pool: weighted, eligible only; the last pick again only if it `may_repeat` (or
/// is the only one left).
pub struct Picker {
    rng: Rng,
    last: [Option<usize>; 4],
}

impl Picker {
    pub fn new(seed: u64) -> Picker {
        Picker { rng: Rng::new(seed), last: [None; 4] }
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    #[cfg(test)]
    pub fn last(&self, k: PoolKey) -> Option<usize> {
        self.last[k.slot()]
    }

    /// Pick a variant of pool `k`: `weights[i]`, `eligible[i]`, `may_repeat[i]` (it may follow
    /// itself).
    pub fn pick(&mut self, k: PoolKey, weights: &[f32], eligible: &[bool], may_repeat: &[bool]) -> Option<usize> {
        let ok = |i: usize| eligible.get(i).copied().unwrap_or(false) && weights[i] > 0.0;
        let last = self.last[k.slot()].filter(|&l| !may_repeat.get(l).copied().unwrap_or(false));
        let others = (0..weights.len()).any(|i| ok(i) && Some(i) != last);
        let w: Vec<f32> = (0..weights.len()).map(|i| if ok(i) && (!others || Some(i) != last) { weights[i] } else { 0.0 }).collect();
        let i = weighted(&mut self.rng, &w)?;
        self.last[k.slot()] = Some(i);
        Some(i)
    }

    /// Set the last pick by hand (a forced variant): the next pick avoids it like any other.
    pub fn force(&mut self, k: PoolKey, i: usize) {
        self.last[k.slot()] = Some(i);
    }

    /// The next in-place shift, `range` ms from now.
    pub fn shift_in(&mut self, range: [u64; 2]) -> Duration {
        let (lo, hi) = (range[0].min(range[1]), range[0].max(range[1]));
        Duration::from_millis(lo + (self.rng.range(0.0, 1.0) * (hi - lo) as f32) as u64)
    }
}

/// Which art a frame belongs to, for the seam rule: the pack's own, or a strip family.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Art {
    Pack,
    Family(u16),
}

/// A cut between these two needs a glitch over it: CapriCore ↔ lachy, or one lachy drawing to
/// another (perch ↔ laydown) without a drawn transition. Within one family the strips flow.
pub fn seam(a: Art, b: Art) -> bool {
    a != b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_are_weighted_eligible_and_never_the_last_twice() {
        let mut p = Picker::new(7);
        let w = [2.0, 1.0, 1.0];
        let all = [true; 3];
        let never = [false; 3];
        let mut n = [0u32; 3];
        let mut prev = None;
        for _ in 0..3000 {
            let i = p.pick(PoolKey::Perch, &w, &all, &never).unwrap();
            assert_ne!(Some(i), prev, "no repeat");
            prev = Some(i);
            n[i] += 1;
        }
        // Without repeats the heavy one can't exceed every other pick; it still leads.
        assert!(n[0] > n[1] && n[0] > n[2] && n[0] <= 1500, "{n:?}");
        // Ineligible never wins; the only one left may repeat.
        let only = [false, true, false];
        assert_eq!(p.pick(PoolKey::Perch, &w, &only, &never), Some(1));
        assert_eq!(p.pick(PoolKey::Perch, &w, &only, &never), Some(1));
        assert_eq!(p.pick(PoolKey::Perch, &w, &[false; 3], &never), None);
        assert_eq!(p.pick(PoolKey::Perch, &[0.0, 0.0, 0.0], &all, &never), None);
    }

    #[test]
    fn the_packs_own_art_leads_by_its_weight() {
        // Capri's perch, 4 : 1 : 1, CapriCore (0) may follow itself, the strips never do.
        let mut p = Picker::new(21);
        let (w, all, rep) = ([4.0, 1.0, 1.0], [true; 3], [true, false, false]);
        let mut n = [0u32; 3];
        let mut prev = None;
        for _ in 0..6000 {
            let i = p.pick(PoolKey::Perch, &w, &all, &rep).unwrap();
            assert!(i == 0 || Some(i) != prev, "a strip twice running");
            prev = Some(i);
            n[i] += 1;
        }
        let share = n[0] as f32 / 6000.0;
        // Stationary share: x = 4/6 x + 4/5 (1 - x), x = 0.706.
        assert!((0.67..0.74).contains(&share), "CapriCore {share} of the picks ({n:?})");
        // Two variants, 3 : 1: the weights hold (a strict no-repeat would make it 50/50).
        let mut m = [0u32; 2];
        for _ in 0..4000 {
            m[p.pick(PoolKey::SleepFloor, &[3.0, 1.0], &[true, true], &[true, false]).unwrap()] += 1;
        }
        let share = m[0] as f32 / 4000.0;
        assert!((0.75..0.85).contains(&share), "lie {share} ({m:?})");
    }

    #[test]
    fn two_variants_alternate_and_pools_keep_their_own_last() {
        let mut p = Picker::new(3);
        let never = [false; 2];
        let seq: Vec<usize> = (0..6).map(|_| p.pick(PoolKey::SleepFloor, &[1.0, 1.0], &[true, true], &never).unwrap()).collect();
        assert!(seq.windows(2).all(|w| w[0] != w[1]), "{seq:?}");
        let f = p.last(PoolKey::SleepFloor);
        p.pick(PoolKey::Perch, &[1.0, 1.0], &[true, true], &never);
        assert_eq!(p.last(PoolKey::SleepFloor), f);
        p.force(PoolKey::SleepWindow, 1);
        assert_eq!(p.pick(PoolKey::SleepWindow, &[1.0, 1.0], &[true, true], &never), Some(0));
    }

    #[test]
    fn seeded_picks_repeat() {
        let run = |s| {
            let mut p = Picker::new(s);
            (0..20).map(|_| p.pick(PoolKey::Perch, &[2.0, 1.0, 1.0], &[true; 3], &[true, false, false]).unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(run(11), run(11));
        assert_ne!(run(11), run(12));
    }

    #[test]
    fn shifts_fall_in_range() {
        let mut p = Picker::new(5);
        for _ in 0..200 {
            let d = p.shift_in([1000, 3000]);
            assert!((1000..=3000).contains(&(d.as_millis() as u64)));
        }
        assert_eq!(p.shift_in([500, 500]), Duration::from_millis(500));
    }

    #[test]
    fn weighted_skips_non_positive() {
        let mut r = Rng::new(9);
        for _ in 0..100 {
            assert_eq!(weighted(&mut r, &[0.0, -1.0, 3.0]), Some(2));
        }
        assert_eq!(weighted(&mut r, &[]), None);
    }

    #[test]
    fn the_seam_rule() {
        assert!(seam(Art::Pack, Art::Family(0)));
        assert!(seam(Art::Family(0), Art::Pack));
        assert!(seam(Art::Family(0), Art::Family(1)), "perch to laydown has no drawn in-between yet");
        assert!(!seam(Art::Family(1), Art::Family(1)));
        assert!(!seam(Art::Pack, Art::Pack));
    }

    #[test]
    fn the_stand_pool_keeps_capricore_about_three_quarters_of_the_time() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../packs/capri/moods.json")).unwrap();
        let s: VariantsSpec = serde_json::from_value(v["variants"].clone()).unwrap();
        let p = &s.pools["stand"];
        assert!(p.shift_stays && p.bridge.as_deref() == Some("legacy"));
        let ids: Vec<&str> = p.variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["neutral", "idle", "legacy"], "lachy idle + idle_legacy for neutral/idle only");
        let w: Vec<f32> = p.variants.iter().map(|v| v.weight).collect();
        let own: Vec<bool> = p.variants.iter().map(|v| v.strip.is_none()).collect();
        // Shifts in place, each stint the same length: the share of stints is the share of time.
        let mut pk = Picker::new(11);
        let all = [true; 3];
        let mut n = [0u32; 3];
        let mut prev = None;
        for _ in 0..20_000 {
            let i = pk.pick(PoolKey::Stand, &w, &all, &own).unwrap();
            assert!(!(prev == Some(i) && !own[i]), "a lachy drawing never follows itself");
            prev = Some(i);
            n[i] += 1;
        }
        let cc = n[0] as f32 / 20_000.0;
        assert!((0.72..0.82).contains(&cc), "{cc}");
    }

    #[test]
    fn the_capri_pools_parse() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../packs/capri/moods.json")).unwrap();
        let s: VariantsSpec = serde_json::from_value(v["variants"].clone()).unwrap();
        for k in PoolKey::ALL {
            let p = &s.pools[k.key()];
            assert!(p.variants.iter().any(|v| v.strip.is_none()), "{}: CapriCore stays in the pool", k.key());
            assert!(p.variants.iter().any(|v| v.strip.is_some()), "{}: lachy joins it", k.key());
        }
        let perch: Vec<&str> = s.pools["perch"].variants.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(perch, ["sit", "perch", "laydown"]);
        // Haseeb, 2026-11-16 (after the B1 deploy, "a"): the new art about half the time, perch 1 : 1
        // and floor sleep 1 : 1 (was 4 : 1 and 3 : 1). Laydown (perch and window sleep) ships at
        // weight 0 after losing both blind critic rounds (r23, r24): it stays in the pack and in the
        // pool, off until a weight turns it on, so window sleep stays CapriCore's.
        let w = |k: &str, id: &str| s.pools[k].variants.iter().find(|v| v.id == id).map(|v| v.weight).unwrap();
        for (k, lead) in [("perch", 1.0), ("sleepFloor", 1.0), ("sleepWindow", 3.0)] {
            let own: f32 = s.pools[k].variants.iter().filter(|v| v.strip.is_none()).map(|v| v.weight).sum();
            assert_eq!(own, lead, "{k}");
        }
        assert_eq!((w("perch", "perch"), w("perch", "laydown")), (1.0, 0.0));
        assert_eq!(w("sleepFloor", "doze"), 1.0);
        assert_eq!(w("sleepWindow", "laydown"), 0.0);
    }
}
