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

/// Which arrival sequence a teleport lands with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArriveKind {
    Launch,
    Summon,
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
    /// Napping since (the brain's `napping`), and asleep on her side.
    nap_since: Option<Instant>,
    pub deep: bool,
    /// The last air kiss, and its hearts' start.
    kissed: Option<Instant>,
    hearts_at: Option<Instant>,
}

impl Avatar {
    fn arrive_steps(&self, k: ArriveKind) -> Vec<Step> {
        let Some(a) = &self.pack.behaviour.arrive else { return Vec::new() };
        match k {
            ArriveKind::Launch => a.launch.clone(),
            ArriveKind::Summon => a.summon.clone(),
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
            let go = !self.arrive.last_variant && self.rng.range(0.0, 1.0) < p;
            self.arrive.last_variant = go;
            if !go {
                return;
            }
        }
        let seq = self.pack.flatten(&self.arrive_steps(k));
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
        let top = self.x11.area_at(gx, gy - 1).y as f32;
        let seed = self.rng.next();
        let bolt = Bolt::new((to.0, top), to, seed, 220.0 * self.layout.sf);
        self.hidden = true;
        self.glitch = None;
        self.teleport = Some(Teleport { seed, leg: Leg::Bolt(now), to: self.origin, place: self.placement, bolt: Some(bolt), moved: true, no_bolt: false, quiet: false });
        self.arrive.on_appear = Some(seq);
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
                self.arrive.nap_since = None;
                if std::mem::take(&mut self.arrive.deep) {
                    self.relayer(now);
                    if self.standing() {
                        let wake = self.pack.flatten(&self.pack.behaviour.brain.nap_wake.clone());
                        if !wake.is_empty() {
                            self.anim.play(wake, now);
                        }
                    }
                }
                true
            }
            (true, Some(t0)) if !self.arrive.deep && self.nap_deep_after().is_some_and(|d| now >= t0 + d) => {
                self.arrive.deep = true;
                self.relayer(now);
                true
            }
            _ => false,
        }
    }

    /// Asleep on a window top: the seat-registered lying frame, if the window's top is long
    /// enough for it (else she sleeps sitting).
    pub(super) fn perch_sleep_frame(&self, m: crate::anim::Mood) -> Option<usize> {
        let sleepy = self.arrive.deep || m == crate::anim::Mood::Sleepy;
        if !sleepy || !self.sitting() {
            return None;
        }
        let i = *self.pack.index.get(self.pack.behaviour.brain.nap_perch.as_ref()?)?;
        let Placement::Perch { win, .. } = self.placement else { return None };
        let r = self.x11.toplevel_rect(win)?;
        let f = self.fr(i)?;
        let cols: Vec<i32> = (0..f.mask.w as i32).filter(|&x| (0..f.mask.h as i32).any(|y| f.mask.get(x, y))).collect();
        let (x0, x1) = (*cols.first()?, *cols.last()?);
        let left = self.origin.0 + self.layout.avatar_x;
        (r.x <= left + x0 && left + x1 < r.x + r.w).then_some(i)
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
        if k.steps.is_empty() || !self.standing() || self.arrive.kissed.is_some_and(|t| now < t + Duration::from_millis(k.gap_ms)) {
            return None;
        }
        if self.rng.range(0.0, 1.0) >= k.chance {
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
        if let Err(e) = self.x11.set_icon(xid, &data) {
            elog!("synaps-avatar: icon on 0x{xid:x}: {e:#}");
        }
    }
}
