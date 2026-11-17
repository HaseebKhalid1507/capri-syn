//! CapriCore's extra art in her behaviour (PROPOSAL-assets phase B): the arrival sequences
//! (launch, $mod+c, some teleports, a closed perch's rescue), the deep nap on her side (on the
//! floor or along a window top), the rare air kiss, and her pack's icon on her windows. The
//! frames and timings are the pack's (`moods.json` `arrive`, `kiss`, `brain.nap*`); this is
//! behaviour only.

use super::{Avatar, Leg, Placement, Teleport};
use crate::anim::Seq;
use crate::fx::{Bolt, Glitch, GlitchKind, Transition};
use crate::pack::Step;
use crate::variants::PoolKey;
use std::time::{Duration, Instant};

/// How far along a window top she shifts to lie down on it (logical px).
const LIE_SLIDE: f32 = 260.0;
/// Woken lying on a window top: she sits up there this long before any move.
pub(super) const SIT_UP: Duration = Duration::from_millis(600);

/// Which arrival sequence a teleport lands with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArriveKind {
    /// A passive or bid teleport (its share is `arrive.teleportChance`, never twice running).
    Teleport,
    /// Her perch closed: the rescue teleport onto another window's top (LOCKED #18).
    Rescue,
}

#[derive(Default)]
pub(super) struct ArriveState {
    /// Played as she appears from the teleport in flight.
    on_appear: Option<Seq>,
    /// An arrival sequence is playing until then: a relayer doesn't cut it.
    pub until: Option<Instant>,
    /// A drawn arrival shows as the red land silhouette until then.
    pub red_until: Option<Instant>,
    /// Its contact squash ends there (the cut to the drawing: no step at the cut).
    pub squash_end: Option<Instant>,
    /// A drawn arrival forms her until then (its first cells are the forming): the HUD waits.
    pub formed: Option<Instant>,
    /// The last passive/bid teleport landed with the sequence.
    last_variant: bool,
    /// The pack has a launch intro: hidden until the frames are in, then the bolt.
    pub launch_wait: bool,
    /// The launch intro's bolt is in flight: an appearance, not a move (the brain isn't held).
    pub launching: bool,
    /// Napping since (the brain's `napping`), and asleep on her side.
    nap_since: Option<Instant>,
    pub deep: bool,
    /// He put her down while she slept: no slide along the top when the nap deepens.
    placed: bool,
    /// The last air kiss, and its hearts' start.
    kissed: Option<Instant>,
    hearts_at: Option<Instant>,
    /// The sit's legs unfold over the edge from then (`UNFOLD_MS`): the in-between from the
    /// crouch (r31: the crouch popped into the sit).
    pub unfold: Option<Instant>,
    /// A drawn touchdown is painted a frame before it shows (outside her shape).
    pub prepaint: bool,
}

impl ArriveState {
    /// An arrival sequence waits for her to appear.
    pub(super) fn pending(&self) -> bool {
        self.on_appear.is_some()
    }
}

/// The legs unfolding below the edge as the sit comes in.
pub(super) const UNFOLD_MS: u64 = 170;

/// The sit drawn with its legs `d` (0..1) in: everything above the seat line as it is; below
/// it the legs' rows come in where they are drawn (2 px rows, the ones near the seat first,
/// the rest scattered toward her feet: `seed`), never a straight cut across the shins. A row
/// in stays in and nothing moves, so her shape only grows (r35: legs that slid down changed
/// her shape every frame, and each change showed the window under her repainting, a dark
/// streak under her feet).
pub(super) fn unfold_bands(top: i32, seat: i32, bottom: i32, d: f32, seed: u64) -> Vec<(i32, i32, i32, i32)> {
    let mut out = vec![(top, seat, 0, 0)];
    let legs = (bottom - seat).max(0);
    let d = d.clamp(0.0, 1.0);
    let (mut y, mut k) = (seat, 0u64);
    while y < bottom {
        let y1 = (y + UNFOLD_ROW).min(bottom);
        let mut x = seed ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 29;
        let r = (x >> 40) as f32 / (1u64 << 24) as f32;
        // Its turn: down the legs, give or take.
        let at = 0.85 * (y - seat) as f32 / legs.max(1) as f32 + 0.15 * r;
        if at <= d || d >= 1.0 {
            match out.last_mut() {
                Some(l) if l.1 == y && l.2 == 0 && l.3 == 0 => l.1 = y1,
                _ => out.push((y, y1, 0, 0)),
            }
        }
        y = y1;
        k += 1;
    }
    out
}

/// The legs' rows (device px).
const UNFOLD_ROW: i32 = 2;
/// Their order (the same every landing).
pub(super) const UNFOLD_SEED: u64 = 0x5eed_1e95;

/// How far down the legs already are on the sit's first frame (r35: on the cut frame they were
/// cut flat at the edge, nothing hanging over).
const UNFOLD_FROM: f32 = 0.4;

/// How far down the sit's legs show `ms` into the unfold, `UNFOLD_FROM`..1 (eased out: they
/// drop, then settle).
pub(super) fn unfold_depth(ms: u64) -> f32 {
    let u = (ms as f32 / UNFOLD_MS as f32).clamp(0.0, 1.0);
    UNFOLD_FROM + (1.0 - UNFOLD_FROM) * (1.0 - (1.0 - u) * (1.0 - u))
}

impl Avatar {
    fn arrive_steps(&self, k: ArriveKind) -> Vec<Step> {
        let Some(a) = &self.pack.behaviour.arrive else { return Vec::new() };
        match k {
            ArriveKind::Teleport | ArriveKind::Rescue if self.sitting_at(self.teleport_place()) => a.perch.clone(),
            ArriveKind::Teleport | ArriveKind::Rescue if self.teleport_place() == Some(Placement::Ground) => a.teleport.clone(),
            _ => Vec::new(),
        }
    }

    fn teleport_place(&self) -> Option<Placement> {
        self.teleport.as_ref().map(|t| t.place)
    }

    fn sitting_at(&self, p: Option<Placement>) -> bool {
        matches!(p, Some(Placement::Perch { sit: true, .. }))
    }

    /// The teleport just started lands with the `k` sequence (passive/bid ones only some of
    /// the time). Call right after `start_teleport*`.
    pub(super) fn arrive_with(&mut self, k: ArriveKind) {
        if self.teleport.is_none() {
            return;
        }
        if k == ArriveKind::Teleport {
            let p = self.pack.behaviour.arrive.as_ref().map_or(0.0, |a| a.teleport_chance);
            let go = variant(self.arrive.last_variant, self.rng.range(0.0, 1.0), p);
            self.arrive.last_variant = go;
            if !go {
                return;
            }
        }
        let seq = self.pack.flatten(&self.arrive_steps(k));
        if !seq.is_empty() {
            elog!("synaps-avatar: arrival {k:?}: {} frames", seq.len());
        }
        self.arrive.on_appear = (!seq.is_empty()).then_some(seq);
    }

    /// She reappears from a teleport (the landing glitch starts now): its arrival sequence, if
    /// it has one. On a window top it is the pack's touchdown (`arrive.perch`), then a dip at
    /// contact (`perchDipMs`: the squash, its trough where the crouch ends: no double image),
    /// and the sit's legs unfold over the edge from there (`UNFOLD_MS`). Before a drawn pose
    /// only the touchdown's first frame plays, cut inside the landing glitch, so the change of
    /// art happens before she is solid; the dip is then the settle at the glitch's end.
    pub(super) fn arrive_appeared(&mut self, now: Instant) {
        let own = self.arrive.on_appear.take();
        if let Some(seq) = self.drawn_floor_arrival() {
            // The drawing is the materialisation: no landing glitch over it. She then stands in
            // the drawing it ends in a few seconds (the stand pool's bridge).
            let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
            self.glitch = None;
            self.arrive.red_until = Some(now + DRAWN_RED);
            if self.art_bridge(now + Duration::from_millis(ms), now) {
                self.relayer(now);
            }
            self.anim.play(seq, now);
            self.arrive.until = Some(now + Duration::from_millis(ms));
            self.arrive.formed = Some(now + Duration::from_millis(ms));
            return;
        }
        let Some(own) = own else { return };
        let land_ms = self.glitch.filter(|g| g.kind == GlitchKind::Land).map_or(0.0, |g| g.ms);
        let dip_ms = self.pack.behaviour.arrive.as_ref().map_or(0, |a| a.perch_dip_ms);
        let drawn = self.sitting() && {
            self.art_sync(now);
            self.art_strip().is_some()
        };
        // The drawn pose itself as the red silhouette (its own outline, legs over the edge),
        // then the drawing: the art never changes shape at the cut (r34: the crouch's
        // silhouette jumped into the sit, and the strip in between floated off the edge).
        // A quiet teleport's soft landing has no red: the drawing at once.
        if drawn && self.drawn_ready() {
            let quiet = self.teleport.as_ref().is_some_and(|t| t.quiet);
            let red = if quiet { Duration::ZERO } else { DRAWN_PERCH_RED };
            self.arrive.red_until = Some(now + red);
            self.arrive.until = Some(now + red);
            if dip_ms > 0 && !red.is_zero() {
                // Contact: the silhouette itself settles (a squash about the seat), not a
                // cut-out held still (r35), easing into rest on the cut: the drawing comes in at
                // the silhouette's own height (r36: still rising at the cut, she hopped up 3 px).
                self.squash = Some(now);
                self.arrive.squash_end = Some(now + red);
            }
            return;
        }
        let seq = match perch_touchdown(self.sitting(), drawn, land_ms) {
            Touchdown::Whole => own,
            Touchdown::Cut(ms) => own.first().map(|&(f, _)| vec![(f, ms)]).unwrap_or_default(),
        };
        if seq.is_empty() {
            return;
        }
        let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
        if self.sitting() && dip_ms > 0 {
            // A squash scheduled ahead (`fx::squash` holds 1.0 until it starts).
            let contact = (ms as f32).max(land_ms) as u64;
            self.squash = Some(now + Duration::from_millis(contact.saturating_sub(dip_ms as u64)));
            if !drawn {
                self.arrive.unfold = Some(now + Duration::from_millis(ms));
            }
        }
        self.anim.play(seq, now);
        self.arrive.until = Some(now + Duration::from_millis(ms));
    }

    /// A loud teleport onto the floor where her stand pick is lachy's: its drawn arrival
    /// (`floorArrive`), now and then from the `form` bloom (`formChance`). None: the pack's own.
    fn drawn_floor_arrival(&mut self) -> Option<Seq> {
        let loud = self.teleport.as_ref().is_some_and(|t| !t.quiet);
        if !loud || self.arrive.launching || self.placement != Placement::Ground || !self.standing() || !self.art_strip_is(PoolKey::Stand) {
            return None;
        }
        let mut seq = self.art_transition("floorArrive")?;
        if self.rng.range(0.0, 1.0) < self.pack.behaviour.form_chance
            && let Some(mut form) = self.art_transition("form")
        {
            form.append(&mut seq);
            seq = form;
        }
        Some(seq)
    }

    /// Touching down onto a window top in a drawn pose whose cell is in: she can form as it.
    pub(super) fn drawn_touchdown(&mut self, now: Instant) -> bool {
        self.sitting() && self.arrive.pending() && {
            self.art_sync(now);
            self.art_strip().is_some()
        } && self.drawn_ready()
    }

    /// The frame she shows now is a strip's cell, decoded.
    fn drawn_ready(&self) -> bool {
        let i = self.anim.frame();
        self.pack.frames.get(i).is_some_and(|f| f.cell.is_some()) && self.fr(i).is_some()
    }

    /// A new teleport, or one that ended before she appeared: nothing pending.
    pub(super) fn arrive_reset(&mut self) {
        self.arrive.on_appear = None;
        self.arrive.formed = None;
        self.arrive.prepaint = false;
        self.arrive.launching = false;
    }

    /// An arrival sequence is playing (a relayer keeps it).
    pub(super) fn arriving(&self, now: Instant) -> bool {
        self.arrive.until.is_some_and(|t| t > now) && self.anim.busy()
    }

    /// The frames are in: the launch (`introview`: the close-up if it plays, then the bolt).
    /// False: no launch sequence, she just appears.
    pub(super) fn launch_intro(&mut self, now: Instant, cx: &mut gpui_kit::Context<Self>) -> bool {
        if !std::mem::take(&mut self.arrive.launch_wait) || self.parked.is_some() {
            return false;
        }
        if !self.intro.cfg.cinematic {
            return self.launch_bolt((self.origin, self.placement), false, now);
        }
        self.launch_cinematic(now, cx)
    }

    /// The launch bolt: a plain red bolt from the top of the monitor down to `to`, then the
    /// launch frames on the floor (after the close-up, `greeted`: just the landing,
    /// `arrive.cinematic`), or the arrival matching her perch pick on a window top
    /// (`arrive_appeared`). False: nothing to play (an old pack on a window top).
    pub(super) fn launch_bolt(&mut self, (to, place): ((i32, i32), Placement), greeted: bool, now: Instant) -> bool {
        let perch = self.sitting_at(Some(place));
        let steps = match &self.pack.behaviour.arrive {
            Some(a) if perch => a.perch.clone(),
            Some(a) if place == Placement::Ground && greeted && !a.cinematic.is_empty() => a.cinematic.clone(),
            Some(a) if place == Placement::Ground => a.launch.clone(),
            _ => Vec::new(),
        };
        let seq = self.pack.flatten(&steps);
        if seq.is_empty() {
            return false;
        }
        let at = self.center_root(to);
        let (gx, gy) = self.ground();
        let top = self.os.area_at(at.0 as i32, at.1 as i32).y.min(self.os.area_at(gx, gy - 1).y) as f32;
        let seed = self.rng.next();
        let bolt = Bolt::new((at.0, top), at, seed, 220.0 * self.layout.sf);
        self.hidden = true;
        self.glitch = None;
        let moved = to == self.origin && place == self.placement;
        self.teleport = Some(Teleport { seed, leg: Leg::Bolt(now), to, place, bolt: Some(bolt), moved, no_bolt: false, quiet: false, leave_until: None });
        elog!("synaps-avatar: arrival Launch: {} frames after the bolt", seq.len());
        self.arrive.on_appear = Some(seq);
        self.arrive.launching = true;
        true
    }

    /// The first frame is in: with a launch sequence she stays hidden until the bolt.
    pub(super) fn launch_hold(&mut self) -> bool {
        let has = self.pack.behaviour.arrive.as_ref().is_some_and(|a| !a.launch.is_empty());
        if (has && self.placement == Placement::Ground) || self.intro.cfg.cinematic {
            self.arrive.launch_wait = true;
            self.hidden = true;
        }
        self.arrive.launch_wait
    }

    // ---- asleep ------------------------------------------------------------------------------

    /// Asleep: the session's Sleepy, or the brain's away nap. Nothing moves her on her own
    /// (the brain's `asleep`); he can still carry her, and she sleeps where he puts her.
    pub(super) fn asleep(&self) -> bool {
        self.mood == crate::anim::Mood::Sleepy || self.brain.as_ref().is_some_and(|h| h.brain.mode_key() == "napping")
    }

    /// He woke her (the field, the Line, $mod+c, talking; a summon): out of Sleepy, the
    /// stand-up first (`set_mood` plays Sleepy's exit and holds moves until it ends). The
    /// session's quiet clock starts again. Compacting keeps her Sleepy (the session's own).
    pub(super) fn wake_up(&mut self, why: &str, now: Instant) {
        use crate::anim::Mood;
        if self.mood != Mood::Sleepy {
            return;
        }
        if let Some(s) = &mut self.session {
            s.link.wake(now);
        }
        self.sync_body(now);
        let held = self.session.as_ref().and_then(|s| s.shown.as_ref()).is_some_and(|b| b.mood == Mood::Sleepy);
        if self.mood == Mood::Sleepy && !held {
            // A mood set by hand (`mood sleepy`, no session): Idle.
            self.set_mood(Mood::Idle, now);
        }
        if self.mood != Mood::Sleepy {
            elog!("synaps-avatar: woken ({why}) at {} ms", self.t_start.elapsed().as_millis());
        }
    }

    /// The stand-up is still playing.
    pub(super) fn waking(&self, now: Instant) -> bool {
        self.waking_until.is_some_and(|w| w > now)
    }

    /// Landed from a drop or a throw: asleep, she stays there (no slide when the nap deepens).
    pub(super) fn arrive_placed(&mut self) {
        if self.asleep() {
            self.arrive.placed = true;
        }
    }

    /// The nap's stand-up (on the floor), moves held until it ends.
    pub(super) fn play_nap_wake(&mut self, now: Instant) {
        self.play_nap_wake_for(false, now);
    }

    /// `greet`: a greeting follows, so the quicker get-up (`napWakeGreet`) if the pack has one
    /// (r40: she sat low ~1.8 s before the wave).
    pub(super) fn play_nap_wake_for(&mut self, greet: bool, now: Instant) {
        // A strip's floor sleep has its own way out: its drawn wake (then she stands in the
        // drawing it ends in a few seconds), else the seam's glitch. No CapriCore stand-up.
        if std::mem::take(&mut self.art.floor_strip) {
            self.play_drawn_wake(now);
            return;
        }
        let b = &self.pack.behaviour.brain;
        let steps = if greet && !b.nap_wake_greet.is_empty() { b.nap_wake_greet.clone() } else { b.nap_wake.clone() };
        let wake = self.pack.flatten(&steps);
        if wake.is_empty() {
            return;
        }
        let ms: u64 = wake.iter().map(|s| s.1 as u64).sum();
        self.anim.play(wake, now);
        self.waking_until = Some(now + Duration::from_millis(ms));
    }

    /// Up after a long nap on the floor (BRIEF-alive S9): the pack's stretch inside the
    /// stand-up, before anything moves her (and before a greeting); at most 4 an hour.
    pub(super) fn stretch_due(&self, napped_s: f64, now: Instant) -> bool {
        self.standing() && self.stretch_earned(napped_s, now)
    }

    /// The nap was long enough, on the floor, and she has stretched under 4 times this hour.
    fn stretch_earned(&self, napped_s: f64, now: Instant) -> bool {
        let b = &self.pack.behaviour.brain;
        let Some(after) = b.stretch_after_ms else { return false };
        if b.stretch.is_empty() || napped_s * 1000.0 < after as f64 || self.placement != Placement::Ground {
            return false;
        }
        let Some(h) = &self.brain else { return false };
        let t = h.bnow(now);
        h.stretches.iter().filter(|&&s| t - s < 3600.0).count() < 4
    }

    /// Out of a nap now, a stretch would follow (the nap's length so far).
    pub(super) fn stretch_pending(&self, now: Instant) -> bool {
        let Some(h) = &self.brain else { return false };
        h.nap_since.is_some_and(|since| self.stretch_earned(h.bnow(now) - since, now))
    }

    pub(super) fn stretch_after_nap(&mut self, napped_s: f64, now: Instant) {
        let steps = self.pack.behaviour.brain.stretch.clone();
        let Some(h) = &mut self.brain else { return };
        let t = h.bnow(now);
        h.stretches.retain(|&s| t - s < 3600.0);
        h.stretches.push(t);
        h.write(serde_json::json!({ "t": t - h.brain.start(), "k": "stretch", "napped_s": napped_s.round() }));
        let seq = self.pack.flatten(&steps);
        let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
        let from = if self.anim.busy() { self.waking_until.filter(|&w| w > now).unwrap_or(now) } else { now };
        if self.anim.busy() {
            self.anim.then(seq, now);
        } else {
            self.anim.play(seq, now);
        }
        let until = from + Duration::from_millis(ms);
        self.waking_until = Some(self.waking_until.map_or(until, |w| w.max(until)));
        self.art.quiet_seams = Some(self.art.quiet_seams.map_or(until, |q| q.max(until)) + Duration::from_millis(500));
    }

    /// Up from a lachy floor sleep: `wake` (a white flash, a glitch, standing), if it's in.
    pub(super) fn play_drawn_wake(&mut self, now: Instant) -> bool {
        if self.placement != Placement::Ground {
            return false;
        }
        let Some(seq) = self.art_transition_clean("wake") else { return false };
        let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
        let until = now + Duration::from_millis(ms);
        if self.art_bridge(until, now) {
            self.relayer(now);
        }
        self.anim.play(seq, now);
        self.waking_until = Some(until);
        true
    }

    // ---- the deep nap -----------------------------------------------------------------------

    fn nap_deep_after(&self) -> Option<Duration> {
        let b = &self.pack.behaviour.brain;
        b.nap_deep.as_ref().or(b.nap_perch.as_ref())?;
        Some(Duration::from_millis(b.nap_deep_after_ms.unwrap_or(60_000)))
    }

    /// When the nap turns into sleep on her side.
    pub(super) fn nap_deadline(&self) -> Option<Instant> {
        let since = self.arrive.nap_since?;
        (!self.arrive.deep).then(|| since + self.nap_deep_after().unwrap_or(Duration::MAX / 4))
    }

    /// Per tick, after the brain ran: the brain's nap begins, deepens, ends. True: redraw.
    pub(super) fn step_nap(&mut self, now: Instant) -> bool {
        let napping = self.brain.as_ref().is_some_and(|h| h.brain.mode_key() == "napping");
        match (napping, self.arrive.nap_since) {
            (true, None) => {
                self.arrive.nap_since = Some(now);
                false
            }
            (false, Some(_)) => {
                // (On the floor the nap's pose ended with the brain's Pose(None): the
                // stand-up played there, before any move in the same decision.)
                self.arrive.nap_since = None;
                self.arrive.placed = false;
                if std::mem::take(&mut self.arrive.deep) {
                    let lying = self.perch_sleep_frame(crate::anim::Mood::Sleepy).is_some();
                    self.relayer(now);
                    if self.standing() {
                        self.play_nap_wake(now);
                    } else if lying {
                        self.waking_until = Some(now + SIT_UP);
                    }
                }
                true
            }
            (true, Some(t0)) if !self.arrive.deep && self.nap_deep_after().is_some_and(|d| now >= t0 + d) => {
                self.arrive.deep = true;
                if !self.arrive.placed {
                    self.slide_to_lie();
                }
                self.relayer(now);
                true
            }
            _ => false,
        }
    }

    /// The lying frame on a window top: its index and its x extent relative to the seat.
    fn lying_span(&self) -> Option<(usize, i32, i32)> {
        let i = *self.pack.index.get(self.pack.behaviour.brain.nap_perch.as_ref()?)?;
        let f = self.fr(i)?;
        let cols: Vec<i32> = (0..f.mask.w as i32).filter(|&x| (0..f.mask.h as i32).any(|y| f.mask.get(x, y))).collect();
        let (x0, x1) = (*cols.first()?, *cols.last()?);
        // The seat is at origin + ground_x (perch_origin).
        let (l, g) = (self.layout.avatar_x, self.layout.ground_x);
        Some((i, l + x0 - g, l + x1 - g))
    }

    /// Asleep on a window top: the seat-registered lying frame, if it lies on the exposed
    /// top (else she sleeps sitting).
    pub(super) fn perch_sleep_frame(&self, m: crate::anim::Mood) -> Option<usize> {
        let sleepy = self.arrive.deep || m == crate::anim::Mood::Sleepy;
        if !sleepy || !self.sitting() {
            return None;
        }
        let (i, a, b) = self.lying_span()?;
        let Placement::Perch { win, dx, .. } = self.placement else { return None };
        let r = self.os.toplevel_rect(win)?;
        let x = r.x + dx;
        let (spans, _) = self.exposed_seats(&r, true);
        spans.iter().any(|&(lo, hi)| lo <= x + a && x + b < hi).then_some(i)
    }

    /// Falling asleep on a window top where the lying frame doesn't fit: along the top (the
    /// soft glitch covers it), by at most `LIE_SLIDE`, if the exposed stretch is long enough.
    fn slide_to_lie(&mut self) {
        if !self.sitting() || self.perch_sleep_frame(crate::anim::Mood::Sleepy).is_some() {
            return;
        }
        let Some((_, a, b)) = self.lying_span() else { return };
        let Placement::Perch { win, sit, dx } = self.placement else { return };
        let Some(r) = self.os.toplevel_rect(win) else { return };
        let x = r.x + dx;
        let (spans, _) = self.exposed_seats(&r, sit);
        let fits: Vec<(i32, i32)> = spans.iter().filter(|(lo, hi)| hi - lo > b - a).map(|&(lo, hi)| (lo - a, hi - 1 - b)).collect();
        let Some(nx) = super::nearest_in(&fits, x) else { return };
        if (nx - x).abs() > (LIE_SLIDE * self.layout.sf) as i32 {
            return;
        }
        let o = self.perch_origin(&r, sit, nx - r.x);
        if self.perch_fits(&r, o, sit) {
            self.placement = Placement::Perch { win, sit, dx: nx - r.x };
            self.set_origin(o);
        }
    }

    /// The nap's frame on the floor: on her side once it's deep.
    pub(super) fn nap_frame(&self) -> Option<usize> {
        let b = &self.pack.behaviour.brain;
        let id = if self.arrive.deep { b.nap_deep.as_ref().or(b.nap.as_ref()) } else { b.nap.as_ref() };
        self.pack.index.get(id?).copied()
    }

    /// The frame shown changed: into or out of lying on her side, a soft glitch covers the cut.
    pub(super) fn lying_cut(&mut self, before: usize, now: Instant) {
        let b = &self.pack.behaviour.brain;
        let lying: Vec<usize> = b.nap_deep.iter().chain(b.nap_perch.iter()).filter_map(|id| self.pack.index.get(id).copied()).collect();
        let after = self.anim.frame();
        if lying.contains(&before) != lying.contains(&after) && self.glitch.is_none() && !self.hidden {
            self.glitch = Glitch::for_cut(Transition::Pose, now, self.rng.next());
        }
    }

    // ---- the air kiss -------------------------------------------------------------------------

    /// Rarely, after a headpat: the air kiss, hearts on its release. Standing only, at most
    /// once in `gapMs`. True if it plays (its steps come after `after_ms` of what's playing).
    pub(super) fn maybe_kiss(&mut self, after_ms: u64, now: Instant) -> Option<Vec<Step>> {
        let k = self.pack.behaviour.kiss.clone()?;
        if k.steps.is_empty() || !self.standing() {
            return None;
        }
        let roll = self.rng.range(0.0, 1.0);
        if !rare(self.arrive.kissed, now, Duration::from_millis(k.gap_ms), roll, k.chance) {
            return None;
        }
        self.arrive.kissed = Some(now);
        self.arrive.hearts_at = Some(now + Duration::from_millis(after_ms + k.hearts_at_ms));
        Some(k.steps)
    }

    pub(super) fn kiss_deadline(&self) -> Option<Instant> {
        self.arrive.hearts_at
    }

    pub(super) fn step_kiss(&mut self, now: Instant) -> bool {
        if self.arrive.hearts_at.is_some_and(|t| t <= now) {
            self.arrive.hearts_at = None;
            self.hearts = Some((now, self.head_point()));
            return true;
        }
        false
    }

    // ---- the icon -----------------------------------------------------------------------------

    /// Her pack's icon (`moods.json` `icon`) on window `xid` as `_NET_WM_ICON` (48 and 32 px).
    pub(super) fn set_icon_on(&self, xid: u32) {
        let Some(f) = &self.pack.behaviour.icon else { return };
        let path = self.pack.dir.join(f);
        let img = match image::open(&path) {
            Ok(i) => i.into_rgba8(),
            Err(e) => {
                elog!("synaps-avatar: icon {}: {e}", path.display());
                return;
            }
        };
        let mut data: Vec<u32> = Vec::new();
        for n in [48u32, 32] {
            let s = image::imageops::resize(&img, n, n, image::imageops::FilterType::Lanczos3);
            data.extend([n, n]);
            data.extend(s.pixels().map(|p| (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32));
        }
        if let Err(e) = self.os.set_icon(xid, &data) {
            elog!("synaps-avatar: icon on 0x{xid:x}: {e:#}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Touchdown {
    /// The whole sequence.
    Whole,
    /// Its first frame only, this long: the cut into drawn art falls inside the landing glitch.
    Cut(u32),
}

/// A drawn arrival's first frames are the plain red land silhouette this long.
const DRAWN_RED: Duration = Duration::from_millis(70);

/// A touchdown into a drawn perch: its own red silhouette this long, from the bolt's arrival.
const DRAWN_PERCH_RED: Duration = Duration::from_millis(130);

/// Where in the landing glitch (`land_ms` long) the touchdown cuts to a drawn pose: while she is
/// still the red silhouette (`Glitch::tint`, its first 30 %).
const CUT_IN_LAND: f32 = 0.25;

/// The touchdown for a landing: sitting on a window top in a drawn pose, the first frame cut
/// inside the landing glitch; else all of it.
fn perch_touchdown(sitting: bool, drawn: bool, land_ms: f32) -> Touchdown {
    if sitting && drawn {
        Touchdown::Cut((land_ms * CUT_IN_LAND).round().max(1.0) as u32)
    } else {
        Touchdown::Whole
    }
}

/// A passive/bid teleport lands with the arrival sequence: chance `p`, never twice running
/// (so about p / (1 + p) of them: 0.5 gives 1 in 3).
fn variant(last: bool, roll: f32, p: f32) -> bool {
    !last && roll < p
}

/// A rare beat: chance `p`, and never within `gap` of the last one.
fn rare(last: Option<Instant>, now: Instant, gap: Duration, roll: f32, p: f32) -> bool {
    last.is_none_or(|t| now >= t + gap) && roll < p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Rng;

    #[test]
    fn the_legs_come_in_row_by_row_where_they_are_drawn() {
        // Seat at 100, feet at 160.
        let rows = |d: f32| -> Vec<i32> { unfold_bands(0, 100, 160, d, 9).iter().flat_map(|b| b.0..b.1).filter(|&y| y >= 100).collect() };
        assert!(rows(0.0).len() <= 4, "folded: (almost) nothing below the seat");
        let half = rows(0.5);
        assert!(half.len() > 20 && half.len() < 45, "{}", half.len());
        assert!(half.contains(&100) && half.contains(&101), "the rows at the seat first");
        assert_eq!(rows(1.0).len(), 60, "all of them");
        // Never moved, and a row in stays in: her shape only grows.
        assert!(unfold_bands(0, 100, 160, 0.5, 9).iter().all(|b| b.2 == 0 && b.3 == 0));
        let mut last = Vec::new();
        for k in 0..=20 {
            let now = rows(k as f32 / 20.0);
            assert!(last.iter().all(|y| now.contains(y)));
            last = now;
        }
    }

    #[test]
    fn the_sits_legs_unfold_top_down_and_settle() {
        assert_eq!(unfold_depth(0), UNFOLD_FROM, "something hangs over the edge on the first frame");
        assert_eq!(unfold_depth(UNFOLD_MS), 1.0);
        let d: Vec<f32> = (0..=UNFOLD_MS).step_by(10).map(unfold_depth).collect();
        assert!(d.windows(2).all(|w| w[1] >= w[0]), "only ever further down");
        assert!(unfold_depth(UNFOLD_MS / 2) > 0.6, "most of the drop early, then the settle");
        const { assert!(UNFOLD_MS <= 200, "an in-between, not a slow reveal") };
    }

    #[test]
    fn a_drawn_perch_pose_is_cut_to_inside_the_landing_glitch() {
        assert_eq!(perch_touchdown(true, false, 330.0), Touchdown::Whole, "her own pose: the whole touchdown");
        assert_eq!(perch_touchdown(false, true, 330.0), Touchdown::Whole, "on the floor");
        let Touchdown::Cut(ms) = perch_touchdown(true, true, 330.0) else { panic!() };
        assert!(ms > 40 && (ms as f32) < 330.0 * 0.3, "{ms}: while she is still red");
        assert_eq!(perch_touchdown(true, true, 70.0), Touchdown::Cut(18), "a quiet landing's short glitch");
        assert_eq!(perch_touchdown(true, true, 0.0), Touchdown::Cut(1), "no glitch: at once");
    }

    #[test]
    fn teleport_variant_one_in_three_never_twice_running() {
        let mut rng = Rng::new(7);
        let (mut last, mut n) = (false, 0);
        for _ in 0..30_000 {
            let go = variant(last, rng.range(0.0, 1.0), 0.5);
            assert!(!(go && last), "twice running");
            n += go as u32;
            last = go;
        }
        let share = n as f64 / 30_000.0;
        assert!((0.31..0.36).contains(&share), "{share}");
        assert!(!variant(false, 0.0, 0.0), "chance 0: never");
    }

    #[test]
    fn the_kiss_is_rare_and_spaced() {
        let t0 = Instant::now();
        let gap = Duration::from_secs(600);
        assert!(rare(None, t0, gap, 0.05, 0.1));
        assert!(!rare(None, t0, gap, 0.2, 0.1), "the roll misses");
        assert!(!rare(Some(t0), t0 + Duration::from_secs(599), gap, 0.0, 0.1), "within the gap");
        assert!(rare(Some(t0), t0 + gap, gap, 0.0, 0.1));
        // Headpats every 20 s for 2 h: at most one kiss per 10 min, and some.
        let mut rng = Rng::new(3);
        let (mut last, mut kisses) = (None, Vec::new());
        for i in 0..360u64 {
            let now = t0 + Duration::from_secs(20 * i);
            if rare(last, now, gap, rng.range(0.0, 1.0), 0.1) {
                last = Some(now);
                kisses.push(now);
            }
        }
        assert!(kisses.windows(2).all(|w| w[1] - w[0] >= gap) && (3..=12).contains(&kisses.len()), "{}", kisses.len());
    }
}
