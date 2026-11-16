//! Variant pools (Haseeb, 2026-11-16: "mix, don't replace"). A state with more than one art
//! option keeps a weighted pool: a window perch (CapriCore's sit, lachy's perch, lachy's laydown),
//! sleep on the floor, sleep on a window. Entering the state picks one: weighted, never the last
//! pick again, from a seeded rng (tests stay deterministic). A long stay shifts it in place now
//! and then. Every switch between CapriCore and lachy art (or between two lachy drawings without
//! a drawn transition) goes through a glitch: `seam`. Pure: no OS, no view, no pixels.

use crate::anim::Rng;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// The states with a pool.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PoolKey {
    /// Sitting on a window top.
    Perch,
    /// Asleep on the floor.
    SleepFloor,
    /// Asleep on a window top.
    SleepWindow,
}

impl PoolKey {
    pub const ALL: [PoolKey; 3] = [PoolKey::Perch, PoolKey::SleepFloor, PoolKey::SleepWindow];

    pub fn key(self) -> &'static str {
        match self {
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
    /// Play every n-th cell of the strips (each held n times as long): halves frames and redraws.
    #[serde(default)]
    pub every: Option<u32>,
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

/// The picks, per pool: weighted, eligible only, never the last pick twice running (unless it
/// is the only one left).
pub struct Picker {
    rng: Rng,
    last: [Option<usize>; 3],
}

impl Picker {
    pub fn new(seed: u64) -> Picker {
        Picker { rng: Rng::new(seed), last: [None; 3] }
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    #[cfg(test)]
    pub fn last(&self, k: PoolKey) -> Option<usize> {
        self.last[k.slot()]
    }

    /// Pick a variant of pool `k`: `weights[i]`, `eligible[i]`.
    pub fn pick(&mut self, k: PoolKey, weights: &[f32], eligible: &[bool]) -> Option<usize> {
        let ok = |i: usize| eligible.get(i).copied().unwrap_or(false) && weights[i] > 0.0;
        let last = self.last[k.slot()];
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
        let mut n = [0u32; 3];
        let mut prev = None;
        for _ in 0..3000 {
            let i = p.pick(PoolKey::Perch, &w, &all).unwrap();
            assert_ne!(Some(i), prev, "no repeat");
            prev = Some(i);
            n[i] += 1;
        }
        // Without repeats the heavy one can't exceed every other pick; it still leads.
        assert!(n[0] > n[1] && n[0] > n[2] && n[0] <= 1500, "{n:?}");
        // Ineligible never wins; the only one left may repeat.
        let only = [false, true, false];
        assert_eq!(p.pick(PoolKey::Perch, &w, &only), Some(1));
        assert_eq!(p.pick(PoolKey::Perch, &w, &only), Some(1));
        assert_eq!(p.pick(PoolKey::Perch, &w, &[false; 3]), None);
        assert_eq!(p.pick(PoolKey::Perch, &[0.0, 0.0, 0.0], &all), None);
    }

    #[test]
    fn two_variants_alternate_and_pools_keep_their_own_last() {
        let mut p = Picker::new(3);
        let seq: Vec<usize> = (0..6).map(|_| p.pick(PoolKey::SleepFloor, &[1.0, 1.0], &[true, true]).unwrap()).collect();
        assert!(seq.windows(2).all(|w| w[0] != w[1]), "{seq:?}");
        let f = p.last(PoolKey::SleepFloor);
        p.pick(PoolKey::Perch, &[1.0, 1.0], &[true, true]);
        assert_eq!(p.last(PoolKey::SleepFloor), f);
        p.force(PoolKey::SleepWindow, 1);
        assert_eq!(p.pick(PoolKey::SleepWindow, &[1.0, 1.0], &[true, true]), Some(0));
    }

    #[test]
    fn seeded_picks_repeat() {
        let run = |s| {
            let mut p = Picker::new(s);
            (0..20).map(|_| p.pick(PoolKey::Perch, &[2.0, 1.0, 1.0], &[true; 3]).unwrap()).collect::<Vec<_>>()
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
    }
}
