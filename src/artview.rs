//! The variant pools on screen (`crate::variants` decides, this shows): which pooled state she is
//! in, the pick, the strip's loop for it (talk, music and blinks follow the variant on screen),
//! the glitch over every seam, the in-place shift on a long stay, and the strips' pixels:
//! decoded off the main thread when a pick needs them, dropped least-recently-used past the
//! pack's budget (`variants.cacheMb`). A pack without pools or `anim/` never gets here.

use super::{Avatar, Placement};
use crate::anim::{Looped, Mood, Seq};
use crate::brain::RestPose;
use crate::frames::Decoded;
use crate::fx::{Glitch, Transition};
use crate::pack::{Step, Variant, VariantArt};
use crate::variants::{Art, Picker, PoolKey, seam, weighted};
use gpui_kit::Context;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// A strip's cells as decoded together: (strip, the mirrorArt twin's mirrored cells).
type Unit = (usize, bool);

/// Perched with music: one of the seat's looped music sequences.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(super) enum Tune {
    /// `music.windowLoop`.
    Window,
    /// A perch variant's `music` strip, at its variant's thinning (`every`).
    Strip(usize, u32),
}

/// One of the loops a held state turns among (`held_tick`): a pool's variant (perched, standing
/// idle), or one of perched music's sequences.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Held {
    Pool(PoolKey, usize),
    Tune(Tune),
}

impl Held {
    /// Its identity for the no-repeat rule (`life::Beats`).
    fn key(self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }
}

/// Where she is held (sitting, on her feet on the ground, music on, the window, the outfit she
/// wears): a change starts its clock over, from the seat's own music.
type HeldOf = (bool, bool, bool, Option<u32>, usize);

/// The rotation's music sequence as it may show: only perched with music on, and never a strip
/// the outfit she wears doesn't draw (an outfit swap on the seat).
fn tune_shown(tune: Option<Tune>, perched_music: bool, wears: impl Fn(usize) -> bool) -> Option<Tune> {
    tune.filter(|t| perched_music && !matches!(*t, Tune::Strip(si, _) if !wears(si)))
}

/// A lounge with its own length (`stayMs`) stays it out (the long-stay shift ends it): the
/// rotation never turns away from it.
fn held_stays(cur: Held, stay: impl Fn(PoolKey, usize) -> bool) -> bool {
    matches!(cur, Held::Pool(k, j) if stay(k, j))
}

/// A turn's short beat: when the restless clock asks for one, never with music on (her idle
/// beats would knock her out of the music mid-song).
fn tangent_wanted(tangent: bool, music: bool) -> bool {
    tangent && !music
}

/// The long-stay shift after a turn to a pool's variant: its own clock (`kept`) goes on, unless
/// the new one has its own length (`stay`) or there was none; then the pick's (`picked`).
fn shift_after_turn(kept: Option<Instant>, picked: Option<Instant>, stay: bool) -> Option<Instant> {
    if stay || kept.is_none() { picked } else { kept }
}

/// How often, at most, what holds a still layer is looked at while a shift waits (a loop: once
/// a pass).
const HELD_LOOK: Duration = Duration::from_millis(250);

/// Not now (a reply, music, a one-shot): the shift waits this long and asks again.
const SHIFT_RETRY: Duration = Duration::from_secs(20);

/// A strip off screen and out of use this long goes (a dance after the music, a walk's): its
/// pixels and atlas tiles, decoded again when it next plays (~15 ms).
const UNUSED_DROP: Duration = Duration::from_secs(45);

/// She changes outfit (`variants.outfits`) about this often, ms [min, max]; only where it can't
/// be seen: in the bolt of a teleport, or waking (she wakes in it).
const OUTFIT_EVERY: [u64; 2] = [45 * 60_000, 120 * 60_000];
/// A look decoded and not put on by then (no bolt, no wake): dropped, tried again later.
const OUTFIT_KEEP: Duration = Duration::from_secs(20 * 60);

/// A decoded outfit waiting for an unseen moment: the outfit, its frames by index, when it came in.
type OutfitReady = (usize, Vec<(usize, crate::frames::Frame)>, Instant);

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
    /// Strips standing in as clips that are about to play (a greeting while she sleeps, the
    /// restless idle's next beat): asked for ahead and kept while wanted.
    pub soon: HashSet<usize>,
    soon_asleep: bool,
    /// Perched with music: the sequence the held rotation put on (None: the seat's own).
    tune: Option<Tune>,
    /// Her held state's loops, never the same twice running (`held_tick`); where she is held;
    /// when what holds it was last looked at.
    held_beats: crate::life::Beats,
    held_of: Option<HeldOf>,
    held_seen: Option<Instant>,
    held_pass: Option<u64>,
    /// The next change of outfit; a look decoding; one decoded, waiting for an unseen moment
    /// (outfit, its frames, when it came in); the replaced drawings' images, dropped at the next
    /// schedule.
    outfit_at: Option<Instant>,
    outfit_busy: bool,
    outfit_ready: Option<OutfitReady>,
    to_drop: Vec<std::sync::Arc<gpui_kit::RenderImage>>,
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
            soon: HashSet::new(),
            soon_asleep: false,
            tune: None,
            held_beats: crate::life::Beats::default(),
            held_of: None,
            held_seen: None,
            held_pass: None,
            outfit_at: None,
            outfit_busy: false,
            outfit_ready: None,
            to_drop: Vec::new(),
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
        [self.shift_at, self.next_blink, self.expire_at, self.outfit_at].into_iter().flatten().min()
    }
}

/// The frames a change from outfit `cur` to `o` repaints: those `o` draws, and those `cur` drew
/// (back to the pack's own). Every other frame is the pack's own in both: not decoded again.
pub(super) fn outfit_frames(pack: &crate::pack::Pack, cur: usize, o: usize) -> Vec<usize> {
    let drawn = |i: usize| pack.outfits.get(i).map(|x| x.frames.keys().copied());
    let mut v: Vec<usize> = drawn(cur).into_iter().flatten().chain(drawn(o).into_iter().flatten()).collect();
    v.sort_unstable();
    v.dedup();
    v
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
        // A strip drawn for another outfit isn't hers to show now.
        let worn = v.art.as_ref().is_none_or(|a| self.pack.strip_wears(a.base));
        long && band && worn && match &v.art {
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

    /// The layer for mood `m` from the strip variant on screen: the music, talk or base loop
    /// (perched with music, the held rotation's strip). None: the pack's own frames (no pool
    /// here, its own variant, the rotation on `music.windowLoop`, or the cells aren't in yet).
    pub(super) fn art_layer(&self, m: Mood) -> Option<(usize, Option<Looped>)> {
        let tune = self.perch_tune();
        if let Some(Tune::Strip(si, every)) = tune {
            return self.art_loop(si, every, None);
        }
        let a = self.art_strip()?;
        let si = match (self.music.is_some(), m) {
            (true, _) if tune == Some(Tune::Window) => return None,
            (true, _) => a.music?,
            (false, Mood::Talking) => a.talk.filter(|&t| self.art_ready(t)).unwrap_or(a.base),
            _ => a.base,
        };
        // At rest (the base loop): a pass, then a rest on its first cell (blinks come back to it).
        let rest = (si == a.base && self.art.hold_ms > 0).then_some(self.art.hold_ms);
        self.art_loop(si, a.every, rest)
    }

    /// Strip `si` looped at thinning `every` (its cells in): its entry, loop and exit
    /// (`entry`/`exit` in anim.json), a rest of `rest` ms after each pass of the loop.
    fn art_loop(&self, si: usize, every: u32, rest: Option<u32>) -> Option<(usize, Option<Looped>)> {
        if !self.art_ready(si) {
            return None;
        }
        let (an, first) = (self.pack.anim.as_ref()?, self.pack.strip_first.get(si).copied().flatten()?);
        let s = &an.strips[si];
        let mut played = s.played(every);
        // A loop she holds skips the cells with a chroma ghost drawn in (`glitchCells`: the
        // sleep's green scan lines).
        if played.iter().any(|(c, _)| !s.glitch_cells.contains(c)) {
            played.retain(|(c, _)| !s.glitch_cells.contains(c));
        }
        let mut r = s.ranged(played).map(|&(c, ms)| (first + c as usize, ms));
        if r.body.is_empty() {
            r = Looped::whole([r.entry, r.exit].concat());
        }
        let base = r.first()?.0;
        if let Some(ms) = rest {
            r.body.push((r.body[0].0, ms));
        }
        Some((base, Some(r)))
    }

    /// The layer for mood `m` is a strip's base loop with a rest at its end.
    pub(super) fn art_rests(&self, m: Mood) -> bool {
        self.art.hold_ms > 0 && self.art_layer(m).is_some_and(|(b, _)| self.art_strip().is_some_and(|a| self.pack.strip_first.get(a.base).copied().flatten() == Some(b)))
    }

    /// The music strip on screen (its first cell): notes and the music pose key off it.
    pub(super) fn art_music_frame(&self) -> Option<usize> {
        self.music.as_ref()?;
        let si = match self.perch_tune() {
            Some(Tune::Strip(si, _)) => si,
            Some(Tune::Window) => return None,
            None => self.art_strip()?.music?,
        };
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

    /// `Pack::flatten`, with every strip cell in it decoded: steps naming a strip whose cells
    /// aren't in yet ask for them and play as nothing this time (the caller's fallback, as when
    /// a pack leaves them out), so no frame that isn't there ever shows.
    pub(super) fn flat(&self, steps: &[Step]) -> Seq {
        let seq = self.pack.flatten(steps);
        if self.seq_ready(&seq) { seq } else { Vec::new() }
    }

    /// The strips these frames are cells of are in (each asked for: they decode together).
    pub(super) fn seq_ready(&self, seq: &[(usize, u32)]) -> bool {
        let mut strips: Vec<usize> = Vec::new();
        for c in seq.iter().filter_map(|&(fi, _)| self.pack.frames.get(fi).and_then(|f| f.cell.as_ref())) {
            if !c.mirror && !strips.contains(&c.strip) {
                strips.push(c.strip);
            }
        }
        // Each asked for (not just the first missing one): they decode together.
        let mut ready = true;
        for &si in &strips {
            ready &= self.art_ready(si);
        }
        ready
    }

    /// The step strips these steps name (`Pack::step_strips`).
    pub(super) fn strips_of(&self, steps: &[Step]) -> Vec<usize> {
        let Some(a) = &self.pack.anim else { return Vec::new() };
        steps
            .iter()
            .flat_map(Step::leaves)
            .filter_map(|s| match s {
                Step::Clip(c) => a.by_name.get(c).copied().filter(|si| self.pack.step_strips.contains(si)),
                Step::Hold { .. } | Step::Repeat(_) => None,
            })
            .collect()
    }

    /// These steps are about to play: their strips are asked for now and kept until `art_soon`
    /// changes again (empty: nothing wanted ahead).
    pub(super) fn art_soon(&mut self, steps: &[Step]) {
        let want: HashSet<usize> = self.strips_of(steps).into_iter().collect();
        for &si in &want {
            self.art_need(si);
        }
        self.art.soon = want;
    }

    // ---- outfits ------------------------------------------------------------------------------

    /// The frames are in: the first change of outfit is due a while from now.
    pub(super) fn outfit_start(&mut self, now: Instant) {
        if self.pack.outfits.len() > 1 {
            self.art.outfit_at = Some(now + self.art.picker.shift_in(OUTFIT_EVERY));
        }
    }

    /// From `schedule`: a change due decodes the other look (weighted, never the one she wears)
    /// off the main thread; one kept too long without an unseen moment goes.
    fn outfit_fetch(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.art.outfit_ready.as_ref().is_some_and(|r| now.saturating_duration_since(r.2) >= OUTFIT_KEEP) {
            self.art.outfit_ready = None;
            self.art.outfit_at = Some(now + self.art.picker.shift_in(OUTFIT_EVERY));
        }
        if !self.loaded || self.art.outfit_busy || self.art.outfit_ready.is_some() || !self.art.outfit_at.is_some_and(|t| t <= now) {
            return;
        }
        self.art.outfit_at = None;
        let cur = self.pack.wearing();
        let w: Vec<f32> = self.pack.outfits.iter().enumerate().map(|(i, o)| if i == cur { 0.0 } else { o.weight }).collect();
        let Some(o) = weighted(self.art.picker.rng(), &w) else { return };
        // Only the frames that differ: the ones it draws, and the ones the look she wears drew
        // (back to the pack's own). Every other frame is the pack's in both.
        let which = outfit_frames(&self.pack, cur, o);
        let (p, geo) = (self.pack.clone(), self.layout.geo);
        self.art.outfit_busy = true;
        cx.spawn(async move |this, cx| {
            let w2 = which.clone();
            let r = crate::off_thread(move || crate::frames::decode_dressed(&p, &geo, &w2, o, false, 2)).await;
            this.update(cx, |v, cx| v.outfit_loaded(o, which, r, cx)).ok();
        })
        .detach();
    }

    fn outfit_loaded(&mut self, o: usize, idx: Vec<usize>, r: anyhow::Result<Decoded>, cx: &mut Context<Self>) {
        self.art.outfit_busy = false;
        let now = Instant::now();
        match r {
            Err(e) => {
                elog!("synaps-avatar: outfit `{}` unavailable: {e:#}", self.pack.outfits.get(o).map_or("", |x| x.id.as_str()));
                self.art.outfit_at = Some(now + self.art.picker.shift_in(OUTFIT_EVERY));
            }
            Ok(d) => {
                elog!("synaps-avatar: outfit `{}` decoded ({} frames, {} ms): on at her next bolt or wake", self.pack.outfits.get(o).map_or("", |x| x.id.as_str()), d.frames.len(), d.decode_ms);
                self.art.outfit_ready = Some((o, idx.into_iter().zip(d.frames).collect(), now));
                // It counts in the budget: strips out of use make room for it.
                self.art_evict(cx);
            }
        }
        self.schedule(cx);
    }

    /// The decoded look waiting for an unseen moment: its bytes (inside `variants.cacheMb`).
    fn outfit_bytes(&self) -> usize {
        self.art.outfit_ready.as_ref().map_or(0, |r| r.1.iter().map(|(_, f)| f.bytes()).sum())
    }

    /// She wakes (out of Sleepy, the brain's nap ends): the decoded look goes on under the wake's
    /// cut, the same light glitch the sleep cut gets (`Transition::Sleep`), so the change itself
    /// is never seen. Not while he carries her or mid-vanish (the glitch can't cover it there:
    /// the next bolt or wake does it), and not before her frames are in. Call before the wake's
    /// frames are picked, so they are the new look's. The sleep strip on screen keeps its pick:
    /// it leaves with the wake.
    pub(super) fn outfit_wake(&mut self, now: Instant) {
        if self.art.outfit_ready.is_none() || !self.loaded || self.rag.is_some() || (self.teleport.is_some() && !self.hidden) {
            return;
        }
        let unseen = self.hidden;
        if !self.outfit_swap_keeping("wake", true, now) || unseen {
            return;
        }
        self.art.quiet_seams = Some(now + super::QUIET_SEAMS);
        // Its own glitch, unless one starts this very instant: one still to come (or long
        // under way) would leave the swap uncovered.
        if self.glitch.is_none_or(|g| g.t0 != now) {
            self.glitch = Glitch::for_cut(Transition::Sleep, now, self.rng.next());
        }
    }

    /// Where it can't be seen (the bolt of a teleport, hidden): the decoded look goes on, every
    /// frame at once, never mid-animation. Not into a pose that look doesn't draw.
    pub(super) fn outfit_swap(&mut self, why: &str, now: Instant) {
        self.outfit_swap_keeping(why, false, now);
    }

    /// `outfit_swap`; `keep_sleep`: the sleep pools' picks stay (on screen as she wakes, they go
    /// with the wake), the others drawn for the old look are picked again. True: it went on.
    fn outfit_swap_keeping(&mut self, why: &str, keep_sleep: bool, now: Instant) -> bool {
        let Some((o, _, _)) = self.art.outfit_ready.as_ref() else { return false };
        let o = *o;
        let target = self.teleport.as_ref().map_or(self.placement, |t| t.place);
        if let Placement::Pose { frame } = target
            && !self.pack.draws(o, frame)
        {
            return false;
        }
        let Some((_, frames, _)) = self.art.outfit_ready.take() else { return false };
        self.pack.wear(o);
        for (i, f) in frames {
            if let Some(old) = self.frames[i].replace(f) {
                if let Some(m) = old.mirror_built() {
                    self.art.to_drop.push(m.image.clone());
                }
                self.art.to_drop.push(old.image.clone());
            }
        }
        // Picks drawn for the other look are dropped (still unseen) and picked again.
        let stale: Vec<PoolKey> = self
            .art
            .picks
            .iter()
            .filter(|&(k, _)| !(keep_sleep && matches!(k, PoolKey::SleepFloor | PoolKey::SleepWindow)))
            .filter(|&(k, &i)| self.pack.pools[k].variants[i].art.as_ref().is_some_and(|a| !self.pack.strip_wears(a.base)))
            .map(|(&k, _)| k)
            .collect();
        for k in stale {
            self.art_drop(k);
        }
        self.art_sync(now);
        self.contact = [self.measure_contact(true), self.measure_contact(false)];
        self.brain_rebox();
        crate::state::save_outfit(self.pack.outfit_id().to_string());
        self.art.outfit_at = Some(now + self.art.picker.shift_in(OUTFIT_EVERY));
        elog!("synaps-avatar: outfit `{}` on ({why})", self.pack.outfit_id());
        true
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
        // Asleep: the strips of her waking greeting stay ready (she wakes into it).
        let asleep = self.asleep();
        if asleep != self.art.soon_asleep && !self.pack.step_strips.is_empty() {
            self.art.soon_asleep = asleep;
            if asleep {
                let b = &self.pack.behaviour.brain;
                let steps: Vec<Step> = b.greet.iter().chain(&b.greet_morning).chain(&b.nap_wake_greet).chain(&b.nap_wake).cloned().collect();
                self.art_soon(&steps);
            } else {
                self.art.soon.clear();
            }
        }
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
        dirty |= self.held_tick(now);
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

    // ---- held states ------------------------------------------------------------------------

    /// The held rotation's music sequence, while she sits with music on (None: the seat's own).
    pub(super) fn perch_tune(&self) -> Option<Tune> {
        tune_shown(self.art.tune, self.music.is_some() && self.sitting(), |si| self.pack.strip_wears(si))
    }

    /// Every looped music sequence for this seat that can play now (worn, its cells in; asked
    /// for if not), weighted: `music.windowLoop` and each eligible perch variant's `music` strip.
    fn tunes(&self, now: Instant) -> Vec<(Tune, f32)> {
        let mut v: Vec<(Tune, f32)> = Vec::new();
        if self.window_loop().is_some() {
            v.push((Tune::Window, 1.0));
        }
        if let Some(p) = self.pack.pools.get(&PoolKey::Perch) {
            for x in &p.variants {
                let Some(a) = &x.art else { continue };
                let Some(si) = a.music else { continue };
                if !v.iter().any(|o| matches!(o.0, Tune::Strip(s, _) if s == si)) && self.pack.strip_wears(si) && self.art_eligible(x, self.placement, now) && self.art_ready(si) {
                    v.push((Tune::Strip(si, a.every), x.weight.max(0.01)));
                }
            }
        }
        v
    }

    /// The music sequence on screen now: the rotation's, else the seat's own (the perch
    /// variant's strip, its cells in; else `music.windowLoop`).
    fn tune_on(&self) -> Option<Tune> {
        if let Some(t) = self.perch_tune() {
            return Some(t);
        }
        match self.art_strip().and_then(|a| Some((a.music?, a.every))) {
            Some((si, every)) if self.art_ready(si) => Some(Tune::Strip(si, every)),
            _ => self.window_loop().map(|_| Tune::Window),
        }
    }

    /// The pool her held state turns in: perched, the perch's; on her feet on the ground, the
    /// stand pool (each only while it's in, as `art_sync` keeps it).
    fn held_pool(&self) -> Option<PoolKey> {
        let k = if self.sitting() {
            PoolKey::Perch
        } else if self.placement == Placement::Ground && self.standing() {
            PoolKey::Stand
        } else {
            return None;
        };
        self.art.picks.contains_key(&k).then_some(k)
    }

    /// The loops her held state can turn to now, weighted: perched with music, the seat's music
    /// sequences; perched or standing idle, its pool's variants (eligible, weight > 0, their
    /// cells in; asked for if not). Music on the floor, hanging, a floor sit: one loop, as before.
    fn held_options(&self, now: Instant) -> Vec<(Held, f32)> {
        if self.music.is_some() {
            return if self.sitting() { self.tunes(now).into_iter().map(|(t, w)| (Held::Tune(t), w)).collect() } else { Vec::new() };
        }
        let Some(k) = self.held_pool() else { return Vec::new() };
        let mut v = Vec::new();
        for (i, x) in self.pack.pools[&k].variants.iter().enumerate() {
            let ready = x.art.as_ref().is_none_or(|a| self.art_ready(a.base));
            if x.weight > 0.0 && self.art_eligible(x, self.placement, now) && ready {
                v.push((Held::Pool(k, i), x.weight));
            }
        }
        v
    }

    /// The loop her held state shows now.
    fn held_on(&self) -> Option<Held> {
        if self.music.is_some() {
            return self.sitting().then(|| self.tune_on()).flatten().map(Held::Tune);
        }
        let k = self.held_pool()?;
        Some(Held::Pool(k, self.art.picks[&k]))
    }

    /// What holds her loop (`life::shift_quiet`): asleep; a holder, the voice, a reaction that
    /// follows the pointer, her session's turn (hyperfocus: one steady loop); hidden, a fullscreen
    /// window, DND; a move. Music is the state here, not a move.
    fn held_quiet(&self) -> crate::life::Quiet {
        crate::life::shift_quiet(self.still_now(|b| b != "music", self.dnd_on()))
    }

    /// Held a while in one state (perched, music on a window top, standing idle), she turns to
    /// another of its loops on the restless idle's clock (`life::Restless::shift_poll`: about
    /// every 8-25 s), picked like her beats (`life::Beats`: weighted, never the same twice
    /// running), swapped only as a pass of the loop on screen ends: its exit, now and then a
    /// short beat of hers, the next one's entry, then its loop, under the cut's glitch. Never a
    /// move. One loop here: nothing changes.
    fn held_tick(&mut self, now: Instant) -> bool {
        let rt = self.restless_secs(now);
        let of = (self.sitting(), self.placement == Placement::Ground && self.standing(), self.music.is_some(), self.art.perch_of, self.pack.wearing());
        let mut dirty = false;
        if self.art.held_of != Some(of) {
            self.art.held_of = Some(of);
            self.restless.shift_reset();
            // Back to the seat's own loop: its layer now, not whenever one comes.
            if self.art.tune.take().is_some() {
                self.relayer(now);
                dirty = true;
            }
        }
        // Looked at only at a boundary (once a pass of the loop, on the step its wrap put it on;
        // a still layer with nothing over it, at most every `HELD_LOOK`) once a shift is due.
        if !self.restless.shift_wants(rt) || !self.anim.at_boundary() {
            return dirty;
        }
        let pass = self.anim.wrapped().then(|| self.anim.passes());
        let seen = match pass {
            Some(p) => self.art.held_pass == Some(p),
            None => self.art.held_seen.is_some_and(|t| now.saturating_duration_since(t) < HELD_LOOK),
        };
        if seen {
            return dirty;
        }
        (self.art.held_seen, self.art.held_pass) = (Some(now), pass);
        let Some(cur) = self.held_on() else { return dirty };
        // A lounge with its own length stays it out: no options gathered, nothing decoded; its
        // clock goes a span on, as for one loop (no turn the moment the long-stay shift ends it).
        if held_stays(cur, |k, j| self.pack.pools[&k].variants[j].stay_ms.is_some()) {
            self.restless.shift_poll(rt, crate::life::Quiet::No, 1);
            return dirty;
        }
        let calm = self.loaded && self.mood == Mood::Idle && self.rag.is_none() && self.teleport.is_none() && self.glitch.is_none() && !self.posed();
        let q = if calm { self.held_quiet() } else { crate::life::Quiet::Off };
        let opts = if q == crate::life::Quiet::No { self.held_options(now) } else { Vec::new() };
        if !self.restless.shift_poll(rt, q, opts.len()) {
            return dirty;
        }
        let keys: Vec<u64> = opts.iter().map(|o| o.0.key()).collect();
        let weights: Vec<f32> = opts.iter().map(|o| o.1).collect();
        self.art.held_beats.mark(cur.key());
        let picked = self.art.held_beats.pick_weighted(&keys, &weights, &mut self.rng);
        let tangent = self.restless.shifted(rt);
        let Some(i) = picked.filter(|&i| keys[i] != cur.key()) else { return dirty };
        // The short beat first (one of her beats here, picked like any), before she settles.
        let beat = if tangent_wanted(tangent, self.music.is_some()) { self.held_tangent() } else { Vec::new() };
        match opts[i].0 {
            Held::Pool(k, j) => {
                self.art.picker.force(k, j);
                self.art_set(k, j);
                // The long-stay shift (`shiftMs`) keeps its own clock; one with its own length
                // (`stayMs`) starts that.
                let kept = self.art.shift_at;
                self.art_after_pick(now);
                self.art.shift_at = shift_after_turn(kept, self.art.shift_at, self.pack.pools[&k].variants[j].stay_ms.is_some());
            }
            Held::Tune(t) => self.art.tune = Some(t),
        }
        self.relayer_cut_ahead(now);
        if !beat.is_empty() {
            self.anim.then(beat, now);
        }
        true
    }

    /// A short beat of hers between two held loops: one of her idle beats here (never the one
    /// just played), if she has any.
    fn held_tangent(&mut self) -> Seq {
        let n = self.activities().len();
        if n == 0 {
            return Vec::new();
        }
        let Some(j) = self.activity_pick(n, 0) else { return Vec::new() };
        let steps = self.activities().get(j).map(|s| s.to_vec()).unwrap_or_default();
        self.flat(&self.resolve_steps(&steps))
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
                self.glitch = Glitch::for_cut(Transition::Pose, now + super::FRAME_FX, self.rng.next());
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
        (self.pack.strip_wears(si) && self.art_ready(si)).then(|| self.pack.strip_seq(si, 1))
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
            // A cover for the body's cut: the swap's light seam, one step on the cut frame, the
            // tag whole (r42: the burst tore her legs in a wiggle and a pounce).
            let t = if motion || was_motion {
                Transition::Motion
            } else if self.asleep() || self.art.quiet_seams.is_some_and(|t| now < t) {
                Transition::Sleep
            } else {
                Transition::Pose
            };
            self.glitch = Glitch::for_cut(t, now, self.rng.next());
            self.art.seams += 1;
        }
    }

    // ---- pixels -------------------------------------------------------------------------------

    fn unit_frames(&self, u: Unit) -> Vec<usize> {
        self.pack.strip_cells(u.0, u.1)
    }

    /// The strips' bytes, plus a decoded look waiting to go on (it counts in `variants.cacheMb`
    /// too: the strips make room for it).
    fn art_bytes(&self) -> usize {
        let strips: usize = self.art.ready.keys().flat_map(|&u| self.unit_frames(u)).filter_map(|i| self.frames[i].as_ref()).map(|f| f.bytes()).sum();
        strips + self.outfit_bytes()
    }

    /// From `schedule`: decode what the layer asked for, off the main thread.
    pub(super) fn art_fetch(&mut self, cx: &mut Context<Self>) {
        for img in std::mem::take(&mut self.art.to_drop) {
            cx.drop_image(img, None);
        }
        self.outfit_fetch(cx);
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
        // Perched with music: every sequence the rotation may turn to stays in.
        if self.music.is_some() && self.sitting() {
            for (t, _) in self.tunes(now) {
                if let Tune::Strip(si, _) = t {
                    in_use.insert(self.art_unit(si));
                }
            }
            if let Some(m) = self.music_spec() {
                for (fi, _) in self.pack.flatten(&m.window_loop.as_steps()) {
                    if let Some(c) = self.pack.frames.get(fi).and_then(|f| f.cell.as_ref()) {
                        in_use.insert(self.art_unit(c.strip));
                    }
                }
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
        // Wanted ahead (a greeting while she sleeps, the restless idle's next beat).
        for &si in &self.art.soon {
            in_use.insert(self.art_unit(si));
        }
        // The glance's strips, once in: glances come further apart than `UNUSED_DROP`.
        for si in self.strips_of(&self.pack.behaviour.brain.glance) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_rotation_s_rules_hold() {
        // perch_tune: a strip the outfit doesn't draw never shows; the window loop always may.
        let worn = |si: usize| si != 7;
        assert_eq!(tune_shown(Some(Tune::Strip(3, 1)), true, worn), Some(Tune::Strip(3, 1)));
        assert_eq!(tune_shown(Some(Tune::Strip(7, 1)), true, worn), None, "the old outfit's music strip");
        assert_eq!(tune_shown(Some(Tune::Window), true, worn), Some(Tune::Window));
        assert_eq!(tune_shown(Some(Tune::Strip(3, 1)), false, worn), None, "not perched with music");
        // No short beat with music on.
        assert!(tangent_wanted(true, false));
        assert!(!tangent_wanted(true, true), "music: no idle beat between its loops");
        assert!(!tangent_wanted(false, false));
        // A stayMs lounge is never turned away from; the rest are; a tune isn't a pool's.
        let lounge = |k: PoolKey, j: usize| k == PoolKey::Perch && j == 1;
        assert!(held_stays(Held::Pool(PoolKey::Perch, 1), lounge));
        assert!(!held_stays(Held::Pool(PoolKey::Perch, 0), lounge));
        assert!(!held_stays(Held::Tune(Tune::Window), lounge));
        // The long-stay shift keeps its clock through a turn; a stay starts its own.
        let t = Instant::now();
        let (kept, picked) = (Some(t + Duration::from_secs(200)), Some(t + Duration::from_secs(20)));
        assert_eq!(shift_after_turn(kept, picked, false), kept, "shiftMs runs on");
        assert_eq!(shift_after_turn(kept, picked, true), picked, "into a lounge: its stayMs");
        assert_eq!(shift_after_turn(None, picked, false), picked, "none running: the pick's");
    }

    #[test]
    fn a_change_of_outfit_decodes_only_the_frames_that_differ() {
        // The alt look draws `neutral` and `sit`; the pack has `wink` too, drawn only by its own.
        let moods = serde_json::json!({"perch": {"base": "sit"}, "extraFrames": [{"id": "wink", "file": "l.png", "like": "neutral"}],
            "variants": {"outfits": [{"id": "main"}, {"id": "alt", "sprites": "outfits/alt/sprites.json"}]}});
        crate::pack::tests::pack_setup("outfit-frames", moods, |d| crate::pack::tests::write_outfit(d, &["neutral", "sit"]), |p| {
            let p = p.unwrap();
            let mut drawn = vec![p.index["neutral"], p.index["sit"]];
            drawn.sort_unstable();
            assert_eq!(outfit_frames(&p, 0, 1), drawn, "into the alt look: what it draws, not the wink");
            assert_eq!(outfit_frames(&p, 1, 0), drawn, "back: what it drew, as the pack's own");
            assert!(outfit_frames(&p, 0, 0).is_empty());
        });
    }
}
