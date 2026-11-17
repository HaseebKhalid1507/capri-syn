//! The variant pools on screen (`crate::variants` decides, this shows): which pooled state she is
//! in, the pick, the strip's loop for it (talk, music and blinks follow the variant on screen),
//! the glitch over every seam, the in-place shift on a long stay, and the strips' pixels:
//! decoded off the main thread when a pick needs them, dropped least-recently-used past the
//! pack's budget (`variants.cacheMb`). A pack without pools or `anim/` never gets here.

use super::{Avatar, Placement};
use crate::anim::{Mood, Seq};
use crate::brain::RestPose;
use crate::frames::Decoded;
use crate::fx::{Glitch, GlitchKind};
use crate::pack::{Variant, VariantArt};
use crate::variants::{Art, Picker, PoolKey, seam, weighted};
use gpui_kit::Context;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// A strip's cells as decoded together: (strip, the mirrorArt twin's mirrored cells).
type Unit = (usize, bool);

/// Not now (a reply, music, a one-shot): the shift waits this long and asks again.
const SHIFT_RETRY: Duration = Duration::from_secs(20);

/// A strip off screen and out of use this long goes (a dance after the music, a walk's): its
/// pixels and atlas tiles, decoded again when it next plays (~15 ms).
const UNUSED_DROP: Duration = Duration::from_secs(45);

pub(super) struct ArtState {
    picker: Picker,
    /// The variant in effect per pooled state she is in (or is teleporting into).
    picks: HashMap<PoolKey, usize>,
    /// The window the perch pick belongs to: it holds through a sleep on that window. Since
    /// when she has been on it (for `afterMs`).
    perch_of: Option<u32>,
    perch_since: Option<Instant>,
    /// The floor sleep picked last was a strip: it has its own way out (no CapriCore stand-up).
    pub floor_strip: bool,
    shift_at: Option<Instant>,
    next_blink: Option<Instant>,
    /// The rest after each pass of the base loop (`holdMs`), drawn per pick.
    hold_ms: u32,
    /// Decoded units and when they were last on screen; decodes in flight; failed ones.
    ready: HashMap<Unit, Instant>,
    pending: HashSet<Unit>,
    failed: HashSet<Unit>,
    /// Asked for from `&self` code (the layer), fetched at the next schedule.
    want: RefCell<Vec<Unit>>,
    /// The next check for strips out of use (`UNUSED_DROP`).
    expire_at: Option<Instant>,
    /// The art family on screen last schedule: a change is a seam. And whether it was a
    /// motion's drawing (a walk, a dance, a dangle): a seam into or out of one is light.
    seam_last: Option<Art>,
    seam_motion: bool,
    /// A lying variant fits on its window top (per strip): asked once per relayer, like the
    /// pack's own lying frame.
    pub fits: RefCell<HashMap<usize, bool>>,
    pub seams: u32,
    /// A swap in place holds the drawing before it until then (`relayer_cut_ahead`): no tag on
    /// those frames (it would jump before her, and its pixels showed in her new outline).
    pub cut_hold: Option<Instant>,
    /// Until then a change of art family is a swap (one light glitch on the cut frame, the tag
    /// whole): falling asleep, waking, the greeting and the stretch after it (r38: a burst
    /// there read as a rendering failure).
    pub quiet_seams: Option<Instant>,
}

impl ArtState {
    pub fn new(seed: u64) -> ArtState {
        ArtState {
            picker: Picker::new(seed),
            picks: HashMap::new(),
            perch_of: None,
            perch_since: None,
            floor_strip: false,
            shift_at: None,
            next_blink: None,
            hold_ms: 0,
            ready: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            want: RefCell::new(Vec::new()),
            expire_at: None,
            seam_last: None,
            seam_motion: false,
            fits: RefCell::new(HashMap::new()),
            seams: 0,
            cut_hold: None,
            quiet_seams: None,
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        [self.shift_at, self.next_blink, self.expire_at].into_iter().flatten().min()
    }
}

impl Avatar {
    // ---- which pools ------------------------------------------------------------------------

    /// Where she is, or where the teleport in flight lands (picked early, decoded during the flight).
    fn art_place(&self) -> Placement {
        self.teleport.as_ref().map_or(self.placement, |t| t.place)
    }

    fn floor_asleep(&self) -> bool {
        let rest = self.brain.as_ref().and_then(|h| h.rest);
        rest == Some(RestPose::Nap) || (self.mood == Mood::Sleepy && rest.is_none())
    }

    fn art_variant(&self, k: PoolKey) -> Option<&Variant> {
        self.pack.pools.get(&k)?.variants.get(*self.art.picks.get(&k)?)
    }

    /// The strip variant of `k` she shows now (its placement is hers, not a teleport's target).
    fn art_on(&self, k: PoolKey) -> Option<&VariantArt> {
        let here = match k {
            PoolKey::Perch | PoolKey::SleepWindow => self.sitting(),
            PoolKey::SleepFloor => self.placement == Placement::Ground,
            PoolKey::Stand => self.placement == Placement::Ground && self.standing(),
        };
        if !here {
            return None;
        }
        let v = self.art_variant(k)?;
        let a = v.art.as_ref()?;
        if !v.lying {
            return Some(a);
        }
        let fits = *self.art.fits.borrow_mut().entry(a.base).or_insert_with(|| self.art_fits(a.base, self.placement));
        fits.then_some(a)
    }

    /// The strip variant on screen, if any (the sleep pools over the perch).
    pub(super) fn art_strip(&self) -> Option<&VariantArt> {
        let sleeping_window = self.art.picks.contains_key(&PoolKey::SleepWindow);
        let sw = self.art_on(PoolKey::SleepWindow);
        // Asleep on a window with the pack's own sleep: the perch variant shows only if that
        // sleep isn't lying (it sits asleep where the lying frame doesn't fit).
        if sleeping_window && sw.is_none() && self.perch_sleep_frame(Mood::Sleepy).is_some() {
            return None;
        }
        sw.or_else(|| self.art_on(PoolKey::SleepFloor)).or_else(|| self.art_on(PoolKey::Perch)).or_else(|| self.art_on(PoolKey::Stand))
    }

    /// The strip variant on screen is `k`'s.
    pub(super) fn art_strip_is(&self, k: PoolKey) -> bool {
        let Some(on) = self.art_strip() else { return false };
        self.art_variant(k).and_then(|v| v.art.as_ref()).is_some_and(|a| a.base == on.base)
    }

    /// Pools in or out as she moves, sleeps and wakes; a pick on the way in. True if a pick changed.
    pub(super) fn art_sync(&mut self, now: Instant) -> bool {
        if self.pack.pools.is_empty() {
            return false;
        }
        let place = self.art_place();
        // Walking along a window top she keeps its pick: she sits back in the same drawing.
        let on = match (place, self.brain_walk_place()) {
            (Placement::Perch { win, sit: true, .. }, _) | (_, Some(Placement::Perch { win, sit: true, .. })) => Some(win),
            _ => None,
        };
        let sleepy = self.arrive.deep || self.mood == Mood::Sleepy;
        let idle = self.mood == Mood::Idle && self.music.is_none() && self.brain.as_ref().is_none_or(|h| h.rest.is_none());
        let want = [
            (PoolKey::Stand, place == Placement::Ground && idle && !self.floor_asleep()),
            (PoolKey::Perch, on.is_some()),
            (PoolKey::SleepWindow, on.is_some() && sleepy),
            (PoolKey::SleepFloor, place == Placement::Ground && self.floor_asleep()),
        ];
        let mut changed = false;
        if on != self.art.perch_of {
            self.art.perch_of = on;
            self.art.perch_since = on.map(|_| now);
            changed |= self.art_drop(PoolKey::Perch);
        }
        for (k, active) in want {
            if !self.pack.pools.contains_key(&k) {
                continue;
            }
            match (active, self.art.picks.contains_key(&k)) {
                (true, false) => {
                    let eligible: Vec<bool> = self.pack.pools[&k].variants.iter().map(|v| self.art_eligible(v, place, now)).collect();
                    let weights: Vec<f32> = self.pack.pools[&k].variants.iter().map(|v| v.weight).collect();
                    let own: Vec<bool> = self.pack.pools[&k].variants.iter().map(|v| v.art.is_none()).collect();
                    // Asleep where she was lounging: the same lying pose, asleep (its weight
                    // may be 0: it only ever follows the lounge).
                    let follow = (k == PoolKey::SleepWindow)
                        .then(|| self.art.picks.get(&PoolKey::Perch).map(|&i| &self.pack.pools[&PoolKey::Perch].variants[i]))
                        .flatten()
                        .filter(|v| v.lying)
                        .and_then(|v| self.pack.pools[&k].by_id(&v.id))
                        .filter(|&i| eligible[i]);
                    if let Some(i) = follow {
                        self.art.picker.force(k, i);
                    }
                    if let Some(i) = follow.or_else(|| self.art.picker.pick(k, &weights, &eligible, &own)) {
                        self.art_set(k, i);
                        changed = true;
                    }
                }
                (false, true) => changed |= self.art_drop(k),
                _ => {}
            }
        }
        if changed {
            self.art_after_pick(now);
        }
        changed
    }

    fn art_set(&mut self, k: PoolKey, i: usize) {
        self.art.picks.insert(k, i);
        if k == PoolKey::SleepFloor {
            self.art.floor_strip = self.pack.pools[&k].variants[i].art.is_some();
        }
    }

    fn art_drop(&mut self, k: PoolKey) -> bool {
        self.art.picks.remove(&k).is_some()
    }

    /// After a pick: the next shift and blink, and the strips it needs.
    fn art_after_pick(&mut self, now: Instant) {
        let top = [PoolKey::SleepWindow, PoolKey::SleepFloor, PoolKey::Perch, PoolKey::Stand].into_iter().find(|k| self.art.picks.contains_key(k));
        let range = top.and_then(|k| self.pack.pools[&k].shift_ms).filter(|_| top.is_some_and(|k| self.pack.pools[&k].variants.len() > 1));
        // A beat with its own length (the lounge): that long, then on.
        let stay = top.and_then(|k| self.pack.pools[&k].variants[self.art.picks[&k]].stay_ms);
        self.art.shift_at = stay.or(range).map(|r| now + self.art.picker.shift_in(r));
        let blink = self.art.picks.iter().filter_map(|(&k, &i)| self.pack.pools[&k].variants[i].art.as_ref()).find(|a| !a.blinks.is_empty()).map(|a| a.blink_ms);
        self.art.next_blink = blink.map(|r| now + self.art.picker.shift_in(r));
        let hold = self.art_strip().and_then(|a| a.hold_ms);
        self.art.hold_ms = hold.map_or(0, |r| self.art.picker.shift_in(r).as_millis() as u32);
        let arts: Vec<(PoolKey, VariantArt)> = self.art.picks.iter().filter_map(|(&k, &i)| Some((k, self.pack.pools[&k].variants[i].art.clone()?))).collect();
        for (k, a) in arts {
            self.art_need(a.base);
            for (b, _) in &a.blinks {
                self.art_need(*b);
            }
            // Its family's drawn ways in and out, in by the time they play.
            let keys: &[&str] = match k {
                PoolKey::Stand => &["floorArrive", "floorLeave", "form"],
                PoolKey::Perch => &["perchLeave"],
                PoolKey::SleepFloor => &["wake"],
                PoolKey::SleepWindow => &[],
            };
            for key in keys {
                self.art_want(key);
            }
        }
    }

    /// A lying variant only where the whole figure is on the window top's exposed stretch; one
    /// with `afterMs` only that long into her stay on this window.
    fn art_eligible(&self, v: &Variant, place: Placement, now: Instant) -> bool {
        let long = v.after_ms.is_none_or(|ms| self.art.perch_since.is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_millis(ms)));
        let band = v.energy.is_empty() || v.energy.contains(&self.band());
        long && band && match &v.art {
            Some(a) if v.lying => self.art_fits(a.base, place),
            _ => true,
        }
    }

    /// Strip `si`'s figure (its registered bounds) lies on the exposed top at the seat of `place`.
    fn art_fits(&self, si: usize, place: Placement) -> bool {
        let Placement::Perch { win, dx, .. } = place else { return false };
        let Some(r) = self.os.toplevel_rect(win) else { return false };
        let (a, b) = self.art_span(si);
        let x = r.x + dx;
        let (spans, _) = self.exposed_seats(&r, true);
        spans.iter().any(|&(lo, hi)| lo <= x + a && x + b < hi)
    }

    /// Strip `si`'s x extent about the seat (device px), as she faces now.
    fn art_span(&self, si: usize) -> (i32, i32) {
        let Some(fi) = self.pack.strip_first.get(si).copied().flatten() else { return (0, 0) };
        let f = &self.pack.frames[fi];
        let (dx, _) = self.pack.offset(f);
        let l = &self.layout;
        let (x0, _) = l.geo.to_device(f.bounds.x + dx, 0);
        let (x1, _) = l.geo.to_device(f.bounds.right() + dx, 0);
        let g = l.ground_x - l.avatar_x;
        let (lo, hi) = (x0.floor() as i32 - g, x1.ceil() as i32 - g);
        if self.mirrored(fi) { (-hi, -lo) } else { (lo, hi) }
    }

    // ---- what shows -------------------------------------------------------------------------

    /// The strip unit for `si` as she faces now: its twin's mirrored cells if it has them.
    fn art_unit(&self, si: usize) -> Unit {
        let Some(fi) = self.pack.strip_first.get(si).copied().flatten() else { return (si, false) };
        (si, self.mirrored(fi) && self.pack.mirror_first.get(si).copied().flatten().is_some())
    }

    /// Ask for `si`'s cells (as she faces now); fetched at the next schedule.
    pub(super) fn art_need(&self, si: usize) {
        let u = self.art_unit(si);
        if !self.art.ready.contains_key(&u) && !self.art.pending.contains(&u) && !self.art.failed.contains(&u) {
            self.art.want.borrow_mut().push(u);
        }
    }

    /// Strip `si` can show: the cells as she faces now (asked for if not), or the other set
    /// (mirrored on the fly) meanwhile.
    pub(super) fn art_ready(&self, si: usize) -> bool {
        let u = self.art_unit(si);
        if self.art.ready.contains_key(&u) {
            return true;
        }
        self.art_need(si);
        // The other set (mirrored on the fly) stands in meanwhile.
        self.art.ready.contains_key(&(si, !u.1))
    }

    /// The layer for mood `m` from the strip variant on screen: the music, talk or base loop.
    /// None: the pack's own frames (no pool here, its own variant, or the cells aren't in yet).
    pub(super) fn art_layer(&self, m: Mood) -> Option<(usize, Option<Seq>)> {
        let a = self.art_strip()?;
        let si = match (self.music.is_some(), m) {
            (true, _) => a.music?,
            (false, Mood::Talking) => a.talk.filter(|&t| self.art_ready(t)).unwrap_or(a.base),
            _ => a.base,
        };
        if !self.art_ready(si) {
            return None;
        }
        let mut seq = self.pack.strip_seq(si, a.every);
        // A loop she holds skips the cells with a chroma ghost drawn in (`glitchCells`: the
        // sleep's green scan lines).
        if let (Some(an), Some(Some(first))) = (&self.pack.anim, self.pack.strip_first.get(si)) {
            let skip = &an.strips[si].glitch_cells;
            if seq.iter().any(|(fi, _)| !skip.contains(&((fi - first) as u32))) {
                seq.retain(|(fi, _)| !skip.contains(&((fi - first) as u32)));
            }
        }
        let base = seq.first()?.0;
        // At rest (the base loop): a pass, then a rest on its first cell (blinks come back to it).
        if si == a.base && self.art.hold_ms > 0 {
            seq.push((base, self.art.hold_ms));
        }
        Some((base, Some(seq)))
    }

    /// The layer for mood `m` is a strip's base loop with a rest at its end.
    pub(super) fn art_rests(&self, m: Mood) -> bool {
        self.art.hold_ms > 0 && self.art_layer(m).is_some_and(|(b, _)| self.art_strip().is_some_and(|a| self.pack.strip_first.get(a.base).copied().flatten() == Some(b)))
    }

    /// The music strip on screen (its first cell): notes and the music pose key off it.
    pub(super) fn art_music_frame(&self) -> Option<usize> {
        self.music.as_ref()?;
        let si = self.art_strip()?.music?;
        self.art_ready(si).then(|| self.pack.strip_first.get(si).copied().flatten()).flatten()
    }

    /// A strip's frame shows (or is about to): it breathes on its own, no procedural pose.
    pub(super) fn art_shows_strip(&self) -> bool {
        self.pack.frames.get(self.shown_frame()).is_some_and(|f| f.cell.is_some())
    }

    /// A click on a strip variant: one of its blinks (the pack's perch reactions are another
    /// drawing). None: not on a strip variant.
    pub(super) fn art_click(&mut self) -> Option<Seq> {
        let a = self.art_strip()?.clone();
        Some(self.art_blink_seq(&a).unwrap_or_default())
    }

    fn art_blink_seq(&mut self, a: &VariantArt) -> Option<Seq> {
        let w: Vec<f32> = a.blinks.iter().map(|b| if self.art_ready(b.0) { b.1 } else { 0.0 }).collect();
        let i = weighted(self.art.picker.rng(), &w)?;
        Some(self.pack.strip_seq(a.blinks[i].0, a.every))
    }

    // ---- time -------------------------------------------------------------------------------

    /// Per tick: blinks over a strip loop, the in-place shift. True: redraw.
    /// Her energy's band changed: a pick it no longer allows (a lively drawing at night)
    /// shifts at the next calm moment.
    pub(super) fn art_band_changed(&mut self, now: Instant) {
        let place = self.placement;
        let stale = self.art.picks.iter().any(|(k, &i)| self.pack.pools.get(k).and_then(|p| p.variants.get(i)).is_some_and(|v| !self.art_eligible(v, place, now)));
        if stale {
            self.art.shift_at = Some(now);
        }
    }

    pub(super) fn art_tick(&mut self, now: Instant) -> bool {
        let mut dirty = false;
        if let Some(at) = self.art.next_blink
            && at <= now
        {
            let a = self.art_strip().cloned();
            let quiet = !self.anim.busy() && self.music.is_none() && self.mood != Mood::Talking && !self.asleep();
            // A blink comes from her rest, never in the middle of a sway.
            if quiet && a.as_ref().is_some_and(|a| a.hold_ms.is_some()) && !self.anim.at_rest() {
                self.art.next_blink = Some(now + Duration::from_millis(250));
            } else {
                if let (Some(a), true) = (&a, quiet)
                    && let Some(seq) = self.art_blink_seq(a)
                    && !seq.is_empty()
                {
                    self.anim.play(seq, now);
                    dirty = true;
                }
                let range = a.map_or([3000, 6000], |a| a.blink_ms);
                self.art.next_blink = Some(now + self.art.picker.shift_in(range));
            }
        }
        if let Some(at) = self.art.shift_at
            && at <= now
        {
            let calm = matches!(self.mood, Mood::Idle | Mood::Sleepy)
                && !self.anim.busy()
                && self.music.is_none()
                && self.teleport.is_none()
                && self.rag.is_none()
                && self.glitch.is_none();
            if calm {
                dirty |= self.art_shift(now);
            } else {
                self.art.shift_at = Some(now + SHIFT_RETRY);
            }
        }
        dirty
    }

    /// A long stay: the top pool re-picks (never the same), on the same spot.
    fn art_shift(&mut self, now: Instant) -> bool {
        let Some(k) = [PoolKey::SleepWindow, PoolKey::SleepFloor, PoolKey::Perch, PoolKey::Stand].into_iter().find(|k| self.art.picks.contains_key(k)) else {
            self.art.shift_at = None;
            return false;
        };
        let place = self.placement;
        let eligible: Vec<bool> = self.pack.pools[&k].variants.iter().map(|v| self.art_eligible(v, place, now)).collect();
        let weights: Vec<f32> = self.pack.pools[&k].variants.iter().map(|v| v.weight).collect();
        let before = self.art.picks.get(&k).copied();
        let stays: Vec<bool> = self.pack.pools[&k].variants.iter().map(|v| self.pack.pools[&k].shift_stays && v.art.is_none()).collect();
        let picked = self.art.picker.pick(k, &weights, &eligible, &stays);
        if let Some(i) = picked {
            self.art_set(k, i);
        }
        self.art_after_pick(now);
        if picked.is_some() && picked != before {
            self.relayer_cut_ahead(now);
            return true;
        }
        false
    }

    /// Relayer for a pose change in place, its cut a frame late: the drawing on screen holds one
    /// more frame while the new one goes underneath (the look-ahead), so her new outline never
    /// shows the window's pixels before they are drawn (r36: a shift into a lying pose had a
    /// shredded first frame, the old sit's pixels in the new outline).
    fn relayer_cut_ahead(&mut self, now: Instant) {
        let before = self.anim.frame();
        self.relayer(now);
        let after = self.anim.frame();
        if after != before && !self.anim.busy() && !self.hidden {
            self.anim.play(vec![(before, super::FRAME_FX.as_millis() as u32)], now);
            self.art.cut_hold = Some(now + super::FRAME_FX);
            // Its seam on the cut: the frames before it lay its one step underneath, the cut
            // frame shows it (r36: the burst after the cut shredded her leg, threw bars, tore
            // the tag).
            if seam(self.pack.art_of(before), self.pack.art_of(after)) && self.loaded && self.glitch.is_none() && self.teleport.is_none() && self.rag.is_none() {
                self.glitch = Some(Glitch::swap(now + super::FRAME_FX, self.rng.next()));
                self.art.seams += 1;
            }
        }
    }

    /// A drawn arrival or wake ended in the stand pool's bridge drawing: she stands in it a few
    /// seconds before the next shift (a glitch over to her pose then). False: no stand pool
    /// here now (another mood: the layer's cut is the seam).
    pub(super) fn art_bridge(&mut self, from: Instant, now: Instant) -> bool {
        let k = PoolKey::Stand;
        let Some((i, range)) = self.pack.pools.get(&k).and_then(|p| Some((p.bridge?, p.bridge_ms))) else { return false };
        self.art_sync(now);
        if !self.art.picks.contains_key(&k) {
            return false;
        }
        self.art.picker.force(k, i);
        self.art_set(k, i);
        self.art_after_pick(now);
        self.art.shift_at = Some(from + self.art.picker.shift_in(range));
        true
    }

    /// A drawn transition `key` (`moods.json transitions`) whose cells are in (as she faces):
    /// its frames. Asked for if not.
    pub(super) fn art_transition(&self, key: &str) -> Option<Seq> {
        let si = *self.pack.transitions.get(key)?;
        self.art_ready(si).then(|| self.pack.strip_seq(si, 1))
    }

    /// Transition `key` without its drawn glitch cells (`glitchCells`): the wake's own flash
    /// and RGB slices read as a rendering failure (r38/r39). What's left holds at least 300 ms
    /// (one clean cell isn't a blink).
    pub(super) fn art_transition_clean(&self, key: &str) -> Option<Seq> {
        let si = *self.pack.transitions.get(key)?;
        let skip = self.pack.anim.as_ref().map(|a| a.strips[si].glitch_cells.clone()).unwrap_or_default();
        let seq = self.art_transition(key)?;
        let first = self.pack.strip_first.get(si).copied().flatten()?;
        let mut seq: Seq = seq.into_iter().filter(|(fi, _)| !skip.contains(&(fi.saturating_sub(first) as u32))).collect();
        let ms: u32 = seq.iter().map(|s| s.1).sum();
        if let Some(last) = seq.last_mut() {
            last.1 += 300u32.saturating_sub(ms);
        }
        Some(seq)
    }

    /// Ask for transition `key`'s cells now (they decode in ~10-30 ms).
    pub(super) fn art_want(&self, key: &str) {
        if let Some(&si) = self.pack.transitions.get(key) {
            self.art_need(si);
        }
    }

    /// Dev: `art <pool> <variant id|next>`.
    pub(super) fn art_force(&mut self, pool: &str, id: &str, now: Instant) -> String {
        let Some(k) = PoolKey::parse(pool) else { return format!("error: pool `{pool}` (stand|perch|sleepFloor|sleepWindow)") };
        let Some(p) = self.pack.pools.get(&k) else { return format!("error: this pack has no `{pool}` pool") };
        if !self.art.picks.contains_key(&k) {
            return format!("error: not in the `{pool}` state now");
        }
        if id == "next" {
            self.art.shift_at = Some(now);
            return if self.art_shift(now) { "ok".into() } else { "error: no other eligible variant".into() };
        }
        let Some(i) = p.by_id(id) else { return format!("error: no variant `{id}` in `{pool}`") };
        self.art.picker.force(k, i);
        self.art_set(k, i);
        self.art_after_pick(now);
        self.relayer_cut_ahead(now);
        "ok".into()
    }

    pub(super) fn art_json(&self) -> serde_json::Value {
        let picks: serde_json::Map<String, serde_json::Value> =
            self.art.picks.iter().map(|(k, &i)| (k.key().to_string(), self.pack.pools[k].variants[i].id.clone().into())).collect();
        let name = |u: &Unit| {
            let n = self.pack.anim.as_ref().map_or("?", |a| a.strips[u.0].name.as_str());
            if u.1 { format!("{n} (mirrorArt)") } else { n.to_string() }
        };
        let mut ready: Vec<String> = self.art.ready.keys().map(name).collect();
        ready.sort();
        serde_json::json!({
            "picks": picks,
            "strip": self.art_shows_strip(),
            "ready": ready,
            "pending": self.art.pending.len(),
            "bytes": self.art_bytes(),
            "seams": self.art.seams,
            "shift_in_ms": self.art.shift_at.map(|t| t.saturating_duration_since(Instant::now()).as_millis() as u64),
        })
    }

    // ---- the seam -----------------------------------------------------------------------------

    /// Every schedule: the art family on screen changed (CapriCore ↔ lachy, or one lachy drawing
    /// to another): a glitch over the cut, unless something else already covers it.
    pub(super) fn art_seam(&mut self, now: Instant) {
        if self.pack.anim.is_none() {
            return;
        }
        let shown = self.shown_frame();
        // Turned on a strip: its cells for this way round (the other set stands in until then).
        if let Some(c) = self.pack.frames.get(shown).and_then(|f| f.cell.as_ref())
            && !c.mirror
        {
            self.art_ready(c.strip);
        }
        let art = self.pack.art_of(shown);
        let prev = self.art.seam_last.replace(art);
        let motion = self.motion_cell(shown);
        let was_motion = std::mem::replace(&mut self.art.seam_motion, motion);
        if let Some(prev) = prev
            && seam(prev, art)
            && self.loaded
            && !self.hidden
            && self.glitch.is_none()
            && self.teleport.is_none()
            && self.rag.is_none()
        {
            // A cover for the body's cut: the tag stays whole. In or out of a motion, a light
            // one (r32: the full burst read as a teleport, smeared her hair); around her sleep,
            // the swap's (one light step on the cut frame).
            if self.asleep() || self.art.quiet_seams.is_some_and(|t| now < t) {
                self.glitch = Some(Glitch::swap(now, self.rng.next()));
            } else {
                let kind = if motion || was_motion { GlitchKind::Seam } else { GlitchKind::Burst };
                let mut g = Glitch::new(kind, now, self.rng.next());
                g.tears_tag = false;
                self.glitch = Some(g);
            }
            self.art.seams += 1;
        }
    }

    // ---- pixels -------------------------------------------------------------------------------

    fn unit_frames(&self, u: Unit) -> Vec<usize> {
        self.pack.strip_cells(u.0, u.1)
    }

    fn art_bytes(&self) -> usize {
        self.art.ready.keys().flat_map(|&u| self.unit_frames(u)).filter_map(|i| self.frames[i].as_ref()).map(|f| f.bytes()).sum()
    }

    /// From `schedule`: decode what the layer asked for, off the main thread.
    pub(super) fn art_fetch(&mut self, cx: &mut Context<Self>) {
        if self.art.expire_at.is_some_and(|t| t <= Instant::now()) {
            self.art_evict(cx);
        }
        let want: Vec<Unit> = self.art.want.borrow_mut().drain(..).collect();
        for u in want {
            if self.art.ready.contains_key(&u) || !self.art.pending.insert(u) {
                continue;
            }
            let idx = self.unit_frames(u);
            let (p, geo) = (self.pack.clone(), self.layout.geo);
            let which = idx.clone();
            cx.spawn(async move |this, cx| {
                let t0 = Instant::now();
                let r = crate::off_thread(move || crate::frames::decode(&p, &geo, &which, false, 2)).await;
                this.update(cx, |v, cx| v.art_loaded(u, idx, r, t0, cx)).ok();
            })
            .detach();
        }
    }

    fn art_loaded(&mut self, u: Unit, idx: Vec<usize>, r: anyhow::Result<Decoded>, t0: Instant, cx: &mut Context<Self>) {
        self.art.pending.remove(&u);
        let name = self.pack.anim.as_ref().map_or(String::new(), |a| a.strips[u.0].name.clone());
        match r {
            Err(e) => {
                elog!("synaps-avatar: art: strip `{name}` unavailable: {e:#}");
                self.art.failed.insert(u);
            }
            Ok(d) => {
                let n = d.frames.len();
                for (i, f) in idx.into_iter().zip(d.frames) {
                    self.frames[i] = Some(f);
                }
                let now = Instant::now();
                self.art.ready.insert(u, now);
                // The other way round's cells stood in, mirrored on the fly: not needed now.
                for i in self.unit_frames((u.0, !u.1)) {
                    if let Some(m) = self.frames[i].as_mut().and_then(|f| f.forget_mirror()) {
                        cx.drop_image(m, None);
                    }
                }
                let bytes = self.art_evict(cx);
                elog!(
                    "synaps-avatar: art: strip `{name}`{} in: {n} cells, decoded in {} ms ({} ms after asked), art {:.1} MB",
                    if u.1 { " (mirrorArt)" } else { "" },
                    d.decode_ms,
                    t0.elapsed().as_millis(),
                    bytes as f64 / 1e6
                );
                // What the layer wanted is in: show it, cut a frame late like any swap in place
                // (r37: a lounge's strip came in after its pick, and its cut got the full burst).
                if self.loaded && self.layer(self.mood).0 != self.anim.base() {
                    self.relayer_cut_ahead(now);
                }
            }
        }
        cx.notify();
        self.schedule(cx);
    }

    /// Past the budget: drop the least recently shown strips that aren't in use (their atlas
    /// tiles too). The bytes held after.
    fn art_evict(&mut self, cx: &mut Context<Self>) -> usize {
        let now = Instant::now();
        let mut in_use: HashSet<Unit> = HashSet::new();
        let arts: Vec<VariantArt> = self.art.picks.iter().filter_map(|(&k, &i)| self.pack.pools[&k].variants[i].art.clone()).collect();
        let (talking, music) = (self.mood == Mood::Talking, self.music.is_some());
        for a in &arts {
            let now_on = a.talk.filter(|_| talking).into_iter().chain(a.music.filter(|_| music));
            for si in std::iter::once(a.base).chain(now_on).chain(a.blinks.iter().map(|b| b.0)) {
                in_use.insert(self.art_unit(si));
            }
        }
        // A motion's strip while it plays (a walk, a dance, a dangle).
        let shown = self.shown_frame();
        if let Some(c) = self.pack.frames.get(shown).and_then(|f| f.cell.as_ref()) {
            in_use.insert((c.strip, c.mirror));
            in_use.insert(self.art_unit(c.strip));
        }
        // Asleep on a floor strip: its drawn wake stays ready (r39: dropped after 45 s, the
        // wake had no stand-up at all).
        if self.art.floor_strip
            && self.asleep()
            && let Some(&si) = self.pack.transitions.get("wake")
        {
            in_use.insert(self.art_unit(si));
        }
        let m = &self.pack.motions;
        let motions = [(m.walk, self.brain_walking()), (m.dance, self.dance_strip().is_some()), (m.dangle, self.rag.is_some())];
        for (si, on) in motions {
            if let (Some(si), true) = (si, on) {
                in_use.insert(self.art_unit(si));
            }
        }
        for u in &in_use {
            if let Some(t) = self.art.ready.get_mut(u) {
                *t = now;
            }
        }
        let mut bytes = self.art_bytes();
        let budget = self.pack.cache_bytes;
        let mut dropped = false;
        loop {
            let lru = self.art.ready.iter().filter(|(u, _)| !in_use.contains(u)).min_by_key(|(_, t)| **t).map(|(&u, &t)| (u, t));
            let Some((u, _)) = lru.filter(|&(_, t)| bytes > budget || now.saturating_duration_since(t) >= UNUSED_DROP) else {
                // The next one to go out of use, or (all in use) a look again later.
                self.art.expire_at = (!self.art.ready.is_empty()).then(|| lru.map_or(now, |(_, t)| t) + UNUSED_DROP);
                break;
            };
            dropped = true;
            self.art.ready.remove(&u);
            for i in self.unit_frames(u) {
                if let Some(f) = self.frames[i].take() {
                    bytes = bytes.saturating_sub(f.bytes());
                    if let Some(m) = f.mirror_built() {
                        cx.drop_image(m.image.clone(), None);
                    }
                    cx.drop_image(f.image.clone(), None);
                }
            }
        }
        if dropped {
            crate::platform::trim_heap();
        }
        bytes
    }
}
