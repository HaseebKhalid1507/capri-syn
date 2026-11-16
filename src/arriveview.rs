//! CapriCore's extra art in her behaviour (PROPOSAL-assets phase B): the arrival sequences
//! (launch, $mod+c, some teleports, a closed perch's rescue), the deep nap on her side (on the
//! floor or along a window top), the rare air kiss, and her pack's icon on her windows. The
//! frames and timings are the pack's (`moods.json` `arrive`, `kiss`, `brain.nap*`); this is
//! behaviour only.

use super::{Avatar, Leg, Placement, Teleport};
use crate::anim::Seq;
use crate::fx::{Bolt, Glitch, GlitchKind};
use crate::pack::Step;
use std::time::{Duration, Instant};

/// How far along a window top she shifts to lie down on it (logical px).
const LIE_SLIDE: f32 = 260.0;
/// Woken lying on a window top: she sits up there this long before any move.
pub(super) const SIT_UP: Duration = Duration::from_millis(600);

/// Which arrival sequence a teleport lands with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArriveKind {
    Launch,
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
}

impl Avatar {
    fn arrive_steps(&self, k: ArriveKind) -> Vec<Step> {
        let Some(a) = &self.pack.behaviour.arrive else { return Vec::new() };
        match k {
            ArriveKind::Launch => a.launch.clone(),
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

    /// She reappears from a teleport: its arrival sequence, if it has one.
    pub(super) fn arrive_appeared(&mut self, now: Instant) {
        let Some(seq) = self.arrive.on_appear.take() else { return };
        let ms: u64 = seq.iter().map(|s| s.1 as u64).sum();
        self.anim.play(seq, now);
        self.arrive.until = Some(now + Duration::from_millis(ms));
    }

    /// A new teleport, or one that ended before she appeared: nothing pending.
    pub(super) fn arrive_reset(&mut self) {
        self.arrive.on_appear = None;
        self.arrive.launching = false;
    }

    /// An arrival sequence is playing (a relayer keeps it).
    pub(super) fn arriving(&self, now: Instant) -> bool {
        self.arrive.until.is_some_and(|t| t > now) && self.anim.busy()
    }

    /// The frames are in: the launch intro (a plain red bolt from the top of her monitor down
    /// to her, then the intro frames), standing on the floor only. False: no intro.
    pub(super) fn launch_intro(&mut self, now: Instant) -> bool {
        if !std::mem::take(&mut self.arrive.launch_wait) {
            return false;
        }
        let standing = self.placement == Placement::Ground && self.parked.is_none();
        let seq = self.pack.flatten(&self.arrive_steps(ArriveKind::Launch));
        if !standing || seq.is_empty() {
            return false;
        }
        let to = self.center_root(self.origin);
        let (gx, gy) = self.ground();
        let top = self.os.area_at(gx, gy - 1).y as f32;
        let seed = self.rng.next();
        let bolt = Bolt::new((to.0, top), to, seed, 220.0 * self.layout.sf);
        self.hidden = true;
        self.glitch = None;
        self.teleport = Some(Teleport { seed, leg: Leg::Bolt(now), to: self.origin, place: self.placement, bolt: Some(bolt), moved: true, no_bolt: false, quiet: false });
        elog!("synaps-avatar: arrival Launch: {} frames after the bolt", seq.len());
        self.arrive.on_appear = Some(seq);
        self.arrive.launching = true;
        true
    }

    /// The first frame is in: with a launch intro she stays hidden until the bolt.
    pub(super) fn launch_hold(&mut self) -> bool {
        let has = self.pack.behaviour.arrive.as_ref().is_some_and(|a| !a.launch.is_empty());
        if has && self.placement == Placement::Ground {
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
        // A strip's floor sleep has its own way out (the seam's glitch): no CapriCore stand-up.
        if std::mem::take(&mut self.art.floor_strip) {
            return;
        }
        let wake = self.pack.flatten(&self.pack.behaviour.brain.nap_wake.clone());
        if wake.is_empty() {
            return;
        }
        let ms: u64 = wake.iter().map(|s| s.1 as u64).sum();
        self.anim.play(wake, now);
        self.waking_until = Some(now + Duration::from_millis(ms));
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
            self.glitch = Some(Glitch::soft(GlitchKind::Burst, now, self.rng.next()));
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
