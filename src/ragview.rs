//! The ragdoll in the view (BRIEF-ragdoll P0-rest, P2, P3): the pointer feed (the real
//! pointer, or a scripted `avatarctl drag`), the held band chain, release, flight with
//! bounces, landings, and what is hidden, paused or deferred while she is carried.
//!
//! A child module of `view`, like the brain's. The sim (ragdoll.rs, drag.rs) is pure; this
//! file feeds it and turns its state into a `BodyPlan` one frame ahead (`cur`/`next`, §4).

use super::*;
use crate::drag::{DragOp, LandKind, Phase, RagEvent, RagInput, RagMachine, RagSpec, Script, Target, Zone};
use crate::ragdoll::{self, Arena, BodyExtents, Chain, Contact, Flight, Params, Pointer, SquashAnim};

/// Permanent room under the feet for the chain's stretch (logical px, §4).
pub(super) const DRAG_PAD_B: f32 = 28.0;
const FLIP_DOWN: f32 = 0.070;
const FLIP_UP: f32 = 0.090;
/// A drop whose feet are this close to the floor lands at once (device px).
const DROP_NEAR: f32 = 6.0;

pub(super) enum Feed {
    Real,
    Script { s: Script, grab: (f32, f32) },
}

/// A frame and its plan, committed one frame ahead.
#[derive(Clone)]
pub(super) struct Shown {
    pub frame: usize,
    pub plan: BodyPlan,
}

pub(super) struct Rag {
    pub m: RagMachine,
    rigid: bool,
    chain: Chain,
    /// The head and shoulders' lag behind the hand (a shake rocks them).
    wobble: ragdoll::Wobble,
    /// The chain's band offsets, held (no 1 px back-and-forth between ticks).
    hold: ragdoll::BandHold,
    zone: Zone,
    /// The frame the chain was built for and the grip in it (avatar-local device px).
    frame: usize,
    grip: (i32, i32),
    flight: Option<Flight>,
    stretch: (f32, f32),
    squash: Option<(SquashAnim, (f32, f32), bool)>,
    pub feed: Feed,
    ptr: Pointer,
    pub t: f32,
    last: Instant,
    due: Instant,
    pub cur: Option<Shown>,
    pub next: Option<Shown>,
    /// `next` has been painted (underneath) by a render: it may become `cur` (the shape).
    pub painted: bool,
    arena: Arena,
    plats: Vec<crate::edges::Platform>,
    /// (bubble was open, prompt was shown) at the grab.
    restore: (bool, bool),
    mood_at_grab: Mood,
    pub pending_glitch: Option<GlitchKind>,
    music_until: Option<Option<Instant>>,
    flip: Option<(f32, usize)>,
    landed: Option<&'static str>,
    hard: bool,
    vel: (f32, f32),
    /// The rigid drag: pointer offset to the window origin.
    offset: (i32, i32),
    /// SA_RAG_PROF: sim + plan time per tick (µs).
    prof: Option<Vec<u32>>,
}

pub(super) fn lockstep() -> bool {
    std::env::var("SA_RAG_LOCKSTEP").is_ok_and(|v| v == "1")
}

/// Tick period: 1/refresh, or SA_RAG_TICK_MS (slow capture under lockstep).
/// In flight the chain bends at most this share of its limits (a tumble stays readable).
const FLIGHT_CAP: f32 = 0.35;

fn tick_period() -> Duration {
    let ms = std::env::var("SA_RAG_TICK_MS").ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| *v >= 4.0).unwrap_or(1000.0 / 60.0);
    Duration::from_secs_f64(ms / 1000.0)
}

/// The band chain is on unless SA_RAG=0 (today's rigid drag, the gauntlet baseline).
fn rag_enabled() -> bool {
    !std::env::var("SA_RAG").is_ok_and(|v| v == "0")
}

impl Avatar {
    pub(super) fn rag_spec(&self) -> RagSpec {
        let b = &self.pack.behaviour;
        let ids = |steps: &[crate::pack::Step]| -> Vec<crate::drag::Step> {
            self.pack.flatten(steps).into_iter().map(|(i, ms)| crate::drag::Step { frame: self.pack.frames[i].id.clone(), ms }).collect()
        };
        let have = |id: &Option<String>| id.clone().filter(|i| self.pack.index.contains_key(i));
        let hang = b.drag.as_ref().and_then(|d| have(&d.hang)).unwrap_or_else(|| self.pack.catalog.default_sprite.clone());
        let d = b.drag.clone().unwrap_or_default();
        let f = b.fling.clone().unwrap_or_default();
        let l = b.land.clone().unwrap_or_default();
        RagSpec {
            grab: ids(&d.grab),
            hang: hang.clone(),
            still: d.still.iter().filter(|s| self.pack.index.contains_key(&s.frame)).map(|s| (s.after_ms as f32 / 1000.0, s.frame.clone())).collect(),
            swing: ids(&d.swing),
            shake: have(&d.shake).unwrap_or_else(|| hang.clone()),
            dizzy: ids(&d.dizzy),
            ankle: have(&d.ankle),
            zones: d.zones.map_or((self.pack.behaviour.click.head_fraction, 0.62), |z| (z[0], z[1])),
            launch: ids(&f.launch),
            fly: have(&f.fly).unwrap_or_else(|| hang.clone()),
            fall: have(&f.fall).unwrap_or_else(|| hang.clone()),
            bounce: ids(&f.bounce),
            annoyed_after: f.annoyed_after.unwrap_or(3),
            soft: ids(&l.soft),
            hard: ids(&l.hard),
            perch: ids(&l.perch),
            perch_base: b.perch.as_ref().map_or_else(|| hang.clone(), |p| p.base.clone()),
        }
    }

    /// The frame on screen: the rag's while she is carried.
    pub(super) fn shown_frame(&self) -> usize {
        self.rag.as_ref().and_then(|r| r.cur.as_ref().or(r.next.as_ref())).map_or_else(|| self.anim.frame(), |s| s.frame)
    }

    fn frame_id(&self, id: &str) -> usize {
        self.pack.index.get(id).copied().unwrap_or_else(|| self.anim.base())
    }

    pub(super) fn rag_held(&self) -> bool {
        self.rag.as_ref().is_some_and(|r| matches!(r.m.phase(), Phase::Held { .. }))
    }

    /// The chain for `frame` gripped at `grip` (avatar-local), at rest, its anchor at `anchor`.
    fn rag_chain(&self, frame: usize, grip: (i32, i32), zone: Zone, anchor: (f32, f32)) -> Chain {
        let l = &self.layout;
        let mut p = self.rag_strength.params().scaled(l.sf);
        if let Some(n) = std::env::var("SA_RAG_LINKS").ok().and_then(|v| v.parse::<usize>().ok()) {
            p.links_down = n.clamp(1, 16);
        }
        let Some(f) = self.frame_or_default(frame) else { return Chain::new(&p, &[], &[], anchor) };
        let len_down = (f.bottom - grip.1).max(8);
        let down = Chain::rows(p.links_down, len_down, false);
        let up = if zone == Zone::Torso { Chain::rows(4, (grip.1 - f.top).max(8), true) } else { Vec::new() };
        // Per node: the room left and right inside the window for the rows around it.
        let rooms: Vec<(f32, f32)> = down
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let lo = grip.1 + if i == 0 { 0 } else { down[i - 1] };
                let hi = grip.1 + down.get(i + 1).copied().unwrap_or(r);
                let (mut x0, mut x1) = (i32::MAX, i32::MIN);
                for y in lo.max(0)..hi.min(f.mask.h as i32) {
                    if let Some(a) = (0..f.mask.w as i32).find(|&x| f.mask.get(x, y)) {
                        x0 = x0.min(a);
                        x1 = x1.max((0..f.mask.w as i32).rev().find(|&x| f.mask.get(x, y)).unwrap_or(a));
                    }
                }
                if x0 > x1 {
                    return (-1.0e5, 1.0e5);
                }
                ((-(l.avatar_x + x0)) as f32, (l.win_w - l.avatar_x - x1 - 1) as f32)
            })
            .collect();
        let mut c = Chain::new(&p, &up, &down, anchor);
        c.set_rooms(rooms);
        c
    }

    /// Root px of the grip point with the window at `o`.
    fn grip_root(&self, o: (i32, i32), grip: (i32, i32)) -> (f32, f32) {
        ((o.0 + self.layout.avatar_x + grip.0) as f32, (o.1 + self.layout.avatar_y + grip.1) as f32)
    }

    /// She is picked up at `local` (avatar-local device px).
    pub(super) fn rag_grab(&mut self, local: (i32, i32), feed: Feed, now: Instant) {
        // A teleport in its vanish or landing leg is cancelled: she stays (or is) here.
        if let Some(tp) = &self.teleport {
            if matches!(tp.leg, Leg::Vanish) {
                self.hidden = false;
            }
            self.teleport = None;
            self.glitch = None;
            self.fx_hide_bolt();
        }
        self.brain_cancel_move();
        self.hop = None;
        self.next_activity = None;
        self.menu = None;
        // Off a perch or a pose: stand again while carried.
        if let Some(h) = &mut self.brain {
            h.rest = None;
        }
        if self.placement != Placement::Ground {
            self.placement = Placement::Ground;
            self.x11.set_target(0);
            self.relayer(now);
        }
        // The HUD goes down like in a teleport; what was open comes back after the landing.
        let bubble_open = self.bubble.as_ref().is_some_and(|b| !b.collapsed);
        if let Some(b) = &mut self.bubble {
            b.collapsed = true;
        }
        let prompt = self.prompt_ui.as_ref().is_some_and(|p| !p.dismissed);
        if let Some(p) = &mut self.prompt_ui {
            p.dismissed = true;
        }
        if self.input.is_some() {
            self.close_input(true);
        }
        let music_until = self.music.take().map(|m| m.until);
        self.update_notes(now);
        let rigid = !rag_enabled() || self.rag_strength.k() <= 0.0 || std::env::var("SA_REDUCED_MOTION").is_ok_and(|v| v == "1");
        let frame = self.anim.frame();
        let mut m = RagMachine::new(self.rag_spec());
        let (top, bottom) = self.frame_or_default(frame).map_or((0, 1), |f| (f.top, f.bottom.max(f.top + 1)));
        let u = ((local.1 - top) as f32 / (bottom - top) as f32).clamp(0.0, 1.0);
        let zone = m.spec.zone(u);
        let out = m.update(0.0, &RagInput { event: Some(RagEvent::Grab { u }), ..Default::default() });
        let anchor = self.grip_root(self.origin, local);
        let chain = self.rag_chain(frame, local, zone, anchor);
        let p0 = match &feed {
            Feed::Script { grab, .. } => *grab,
            Feed::Real => self.x11.pointer().map_or(anchor, |p| (p.0 as f32, p.1 as f32)),
        };
        let mut ptr = Pointer::default();
        ptr.push(0.0, p0.0, p0.1);
        let mut rag = Rag {
            m,
            rigid,
            chain,
            hold: ragdoll::BandHold::default(),
            wobble: self.rag_wobble(),
            zone,
            frame,
            grip: local,
            flight: None,
            stretch: (1.0, 1.0),
            squash: None,
            feed,
            ptr,
            t: 0.0,
            last: now,
            due: now,
            cur: None,
            painted: false,
            next: None,
            arena: Arena::default(),
            plats: Vec::new(),
            restore: (bubble_open, prompt),
            mood_at_grab: self.mood,
            pending_glitch: None,
            music_until,
            flip: None,
            landed: None,
            hard: false,
            vel: (0.0, 0.0),
            offset: (self.origin.0 - p0.0 as i32, self.origin.1 - p0.1 as i32),
            prof: std::env::var("SA_RAG_PROF").is_ok_and(|v| v == "1").then(Vec::new),
        };
        if !rigid {
            rag.chain.kick((0.0, -400.0 * self.layout.sf));
            if out.flip
                && let Some(a) = rag.m.spec.ankle.clone()
            {
                rag.flip = Some((0.0, self.frame_id(&a)));
            }
        }
        let shown = Shown { frame: self.rag_frame(&rag, out.frame.as_deref()), plan: BodyPlan::plain() };
        rag.next = Some(shown);
        // No X round trips inside her 60 Hz ticks: monitors and work areas as of now, and
        // the real pointer from its own thread.
        self.x11.freeze_layout(true);
        self.x11.poll_pointer(matches!(rag.feed, Feed::Real));
        self.rag = Some(rag);
        self.brain_touch(crate::brain::Touch::DragStart);
        self.rag_plan_next();
    }

    fn rag_frame(&self, r: &Rag, id: Option<&str>) -> usize {
        if r.rigid {
            return if lockstep() { self.anim.base() } else { self.anim.frame() };
        }
        if let Some((t0, to)) = r.flip {
            return if r.t - t0 >= FLIP_DOWN { to } else { r.frame };
        }
        id.map_or(r.frame, |id| self.frame_id(id))
    }

    /// Advance the rag one tick. True: redraw.
    pub(super) fn rag_tick(&mut self, now: Instant) -> bool {
        let Some(r) = &mut self.rag else { return false };
        // The shape follows `cur`, so `cur` must already be on screen: a tick waits for the
        // render that paints the last `next` (two ticks between renders would show a region
        // never painted: stale black). Lockstep stays deterministic (virtual time advances
        // per tick, not per wall clock).
        // The render that paints it is already requested (the tick that set `next` asked
        // for a redraw): nothing to do until it lands (`rag_painted` re-arms the ticker).
        if r.next.is_some() && !r.painted {
            return false;
        }
        let dt = if lockstep() {
            if now + Duration::from_millis(1) < r.due {
                return false;
            }
            r.due = now.max(r.due) + tick_period();
            1.0 / 60.0
        } else {
            let d = now.saturating_duration_since(r.last).as_secs_f32();
            // On the period grid (no drift: `now + period` gives ~58 Hz), never bunched.
            r.due = (r.due + tick_period()).max(now + tick_period() / 2);
            d.min(ragdoll::MAX_FRAME_DT)
        };
        r.last = now;
        r.t += dt;
        // What was committed last tick is drawn now (on top); this tick's state goes under
        // it. Shifted here, not at render: every render in a tick paints the same pair.
        if let Some(n) = r.next.clone() {
            r.cur = Some(n);
        }
        r.painted = false;
        let t = r.t;
        let t0 = Instant::now();
        match r.m.phase() {
            Phase::Held { .. } => self.rag_held_step(now, t, dt),
            Phase::Flight { .. } => self.rag_flight_step(now, t, dt),
            Phase::Land(_) => self.rag_land_step(now, t, dt),
            Phase::Idle => self.rag_end(now),
        }
        self.rag_plan_next();
        if let Some(p) = self.rag.as_mut().and_then(|r| r.prof.as_mut()) {
            p.push(t0.elapsed().as_micros() as u32);
        }
        true
    }

    fn rag_pointer(&self, r: &Rag) -> (f32, f32) {
        match &r.feed {
            Feed::Script { s, grab } => {
                let o = s.at(r.t);
                (grab.0 + o.0, grab.1 + o.1)
            }
            Feed::Real => self.x11.pointer_fast().map_or_else(|_| r.ptr_last(), |p| (p.0 as f32, p.1 as f32)),
        }
    }

    fn rag_held_step(&mut self, now: Instant, t: f32, dt: f32) {
        let Some(r) = &self.rag else { return };
        let p = self.rag_pointer(r);
        let release = matches!(&r.feed, Feed::Script { s, .. } if s.release_at.is_some_and(|at| t + 1e-4 >= at));
        let l = self.layout;
        let Some(r) = &mut self.rag else { return };
        r.ptr.push(t, p.0, p.1);
        if r.rigid {
            let o = (p.0 as i32 + r.offset.0, p.1 as i32 + r.offset.1);
            let _ = dt;
            self.move_to(o, false);
        } else {
            // The flip-card: at the bottom of the fold the frame swaps to the upside-down
            // hang and the grip moves to its feet.
            if let Some((t0, to)) = r.flip
                && t - t0 >= FLIP_DOWN
                && r.frame != to
            {
                let grip = self.pack_anchor_local(to);
                let a = (p.0, p.1);
                let chain = self.rag_chain(to, grip, Zone::Legs, a);
                let Some(r) = &mut self.rag else { return };
                r.frame = to;
                r.grip = grip;
                r.chain = chain;
            }
            let Some(r) = &mut self.rag else { return };
            if r.flip.is_some_and(|(t0, _)| t - t0 >= FLIP_DOWN + FLIP_UP) {
                r.flip = None;
            }
            r.chain.step(p, dt);
            r.wobble.step(p.0, dt);
            let feet = r.chain.feet_offset();
            let (n, since) = r.ptr.reversals(t);
            let input = RagInput {
                event: None,
                cursor_speed: r.ptr.speed(t),
                reversals: n,
                since_reversal: since,
                feet_amp: feet.0.abs() / l.sf,
                feet_speed: r.chain.feet_speed() / l.sf,
            };
            let out = r.m.update(t, &input);
            let fi = self.rag_frame(self.rag.as_ref().expect("rag"), out.frame.as_deref());
            let Some(r) = &mut self.rag else { return };
            // Standing frames share the canonical registration: the grip stays put.
            if r.flip.is_none() && r.zone != Zone::Legs {
                r.frame = fi;
            }
            let o = (p.0.round() as i32 - l.avatar_x - r.grip.0, p.1.round() as i32 - l.avatar_y - r.grip.1);
            self.set_origin(o);
        }
        if release {
            self.rag_release(now);
        }
    }

    /// The shake wobble: ~7 px at the top of the head at strength 0.5 (none when rigid).
    fn rag_wobble(&self) -> ragdoll::Wobble {
        let k = self.rag_strength.k();
        ragdoll::Wobble::new(14.0 * k * self.layout.sf, 0.7 * k)
    }

    /// A frame's anchor, avatar-local device px.
    fn pack_anchor_local(&self, i: usize) -> (i32, i32) {
        let a = self.anchor_px(i);
        (a.0 - self.layout.avatar_x, a.1 - self.layout.avatar_y)
    }

    /// Let go: a fling, a drop, or (rigid) today's release snap.
    pub(super) fn rag_release(&mut self, now: Instant) {
        let Some(r) = &mut self.rag else { return };
        if !matches!(r.m.phase(), Phase::Held { .. }) {
            return;
        }
        if r.rigid {
            let r = self.rag.take().expect("rag");
            self.drop_here(now);
            self.rag_restore_with(r, now, true);
            self.relayer(now);
            return;
        }
        let t = r.t;
        let vel = r.ptr.velocity(t);
        r.vel = vel;
        let speed = (vel.0 * vel.0 + vel.1 * vel.1).sqrt();
        let fling = speed >= ragdoll::V_FLING * self.layout.sf;
        if fling {
            self.brain_feed(crate::brain::BrainEvent::Flung { speed }, now);
            let offended = self.brain.as_ref().is_some_and(|h| h.brain.mode_key() == "sulking");
            if let Some(r) = &mut self.rag
                && offended
            {
                r.m.force_hard(t);
            }
        }
        let l = self.layout;
        let (areas, _) = self.rag_areas();
        let plats = self.rag_platforms();
        let Some(r) = &mut self.rag else { return };
        // The pointer is in device px; the machine's thresholds are logical.
        r.m.update(t, &RagInput { event: Some(RagEvent::Release { vel: (vel.0 / l.sf, vel.1 / l.sf) }), ..Default::default() });
        r.chain.set_gravity(0.0);
        r.arena = Arena { areas, floor_pad: GROUND_ROOM };
        r.plats = plats;
        // From the upside-down hang: she rights herself (a standing flight frame) about the
        // same window, so the ground point is the canonical one again.
        if r.zone == Zone::Legs {
            r.zone = Zone::Torso;
        }
        let feet = ((self.origin.0 + l.ground_x) as f32, (self.origin.1 + l.ground_y) as f32);
        let Some(r) = &mut self.rag else { return };
        r.flight = Some(if fling { Flight::new(feet, vel) } else { Flight::drop(feet, vel) });
        r.chain.set_cap(FLIGHT_CAP);
        if !fling {
            // Already standing on the floor: land at once (soft).
            let ext = self.rag_extents();
            let Some(r) = &mut self.rag else { return };
            let w = r.arena.walls_at(((feet.0 + ext.left) as i32, (feet.1 + ext.top) as i32, (feet.0 + ext.right) as i32, (feet.1 + ext.bottom) as i32));
            if let Some(f) = w.floor
                && (f as f32 - ext.bottom - feet.1).abs() <= DROP_NEAR * l.sf
            {
                self.rag_contact(Contact::Floor { speed: 0.0 }, now);
            }
        }
    }

    fn rag_extents(&self) -> BodyExtents {
        let l = &self.layout;
        let fi = self.shown_frame();
        let (x0, x1, top) = self.frame_or_default(fi).map_or((0, l.geo.width as i32, 0), |f| {
            let cols = f.profile.iter().enumerate().filter(|(_, y)| **y != i16::MAX).map(|(x, _)| x as i32);
            let (a, b) = cols.fold((i32::MAX, i32::MIN), |acc, x| (acc.0.min(x), acc.1.max(x)));
            (a.min(b), b.max(a), f.top)
        });
        BodyExtents {
            left: (l.avatar_x + x0 - l.ground_x) as f32,
            right: (l.avatar_x + x1 - l.ground_x) as f32,
            top: (l.avatar_y + top - l.ground_y) as f32,
            bottom: l.foot_pad as f32,
        }
    }

    /// Exposed edges she can land on (seat line crossing, §2.6), shifted to the feet.
    fn rag_platforms(&self) -> Vec<crate::edges::Platform> {
        let l = &self.layout;
        let Some(seat_y) = l.seat_y else { return Vec::new() };
        let (stack, mut eligible): (Vec<WinRect>, Vec<bool>) = self.x11.stack().into_iter().unzip();
        if let Some(allowed) = self.brain_allowed_windows() {
            for (i, w) in stack.iter().enumerate() {
                eligible[i] &= allowed.contains(&w.id);
            }
        }
        let mut p = crate::edges::platforms(&stack, &eligible, self.contact_or_default(true), self.edge_margin());
        let fits = |pl: &crate::edges::Platform| stack.iter().find(|w| w.id == pl.win).is_some_and(|r| self.perch_fits(r, self.perch_origin(r, true, pl.seat_x.0 - r.x), true));
        p.retain(fits);
        for pl in &mut p {
            pl.y += l.ground_y - seat_y;
        }
        p
    }

    fn rag_flight_step(&mut self, now: Instant, t: f32, dt: f32) {
        let ext = self.rag_extents();
        let l = self.layout;
        let Some(r) = &mut self.rag else { return };
        let Some(f) = &mut r.flight else { return };
        let c = f.step(dt, &r.arena, &r.plats, ext);
        let feet = f.pos;
        let v = f.vel;
        // Flight stretch on the dominant axis, smoothed (40 ms).
        let s = f.stretch(self.rag_strength);
        let want = if v.1.abs() >= v.0.abs() { (1.0, s) } else { (s, 1.0) };
        let k = 1.0 - (-dt / 0.04).exp();
        r.stretch = (r.stretch.0 + (want.0 - r.stretch.0) * k, r.stretch.1 + (want.1 - r.stretch.1) * k);
        let o = (feet.0.round() as i32 - l.ground_x, feet.1.round() as i32 - l.ground_y);
        let anchor = ((o.0 + l.avatar_x + r.grip.0) as f32, (o.1 + l.avatar_y + r.grip.1) as f32);
        r.chain.step(anchor, dt);
        let out = r.m.update(t, &RagInput::default());
        let fi = self.rag_frame(self.rag.as_ref().expect("rag"), out.frame.as_deref());
        if let Some(r) = &mut self.rag {
            r.frame = fi;
        }
        self.set_origin(o);
        if let Some(sq) = self.rag.as_mut().and_then(|r| r.squash.as_mut()) {
            sq.0.step(dt);
            if sq.0.done() {
                self.rag.as_mut().expect("rag").squash = None;
            }
        }
        if c != Contact::None {
            self.rag_contact(c, now);
        }
    }

    fn rag_contact(&mut self, c: Contact, now: Instant) {
        let l = self.layout;
        let Some(r) = &mut self.rag else { return };
        let t = r.t;
        let out = r.m.update(t, &RagInput { event: Some(RagEvent::Contact(scale_contact(c, l.sf))), ..Default::default() });
        let feet_local = ((l.ground_x - l.avatar_x) as f32, (l.ground_y - l.avatar_y) as f32);
        match c {
            Contact::Bounce { normal, speed } => {
                let pivot = if normal.0 > 0.0 {
                    (self.rag_extents().left + (l.ground_x - l.avatar_x) as f32, feet_local.1)
                } else if normal.0 < 0.0 {
                    (self.rag_extents().right + (l.ground_x - l.avatar_x) as f32, feet_local.1)
                } else if normal.1 > 0.0 {
                    (feet_local.0, self.rag_extents().top + (l.ground_y - l.avatar_y) as f32)
                } else {
                    feet_local
                };
                let Some(r) = &mut self.rag else { return };
                r.squash = Some((ragdoll::squash_spring(speed / l.sf, self.rag_strength), pivot, normal.0 != 0.0));
                r.chain.kick((-normal.0 * speed * 0.15, -normal.1 * speed * 0.15));
            }
            Contact::Floor { speed } => {
                let hard = matches!(r.m.phase(), Phase::Land(LandKind::Hard));
                r.hard = hard;
                r.landed = Some("floor");
                r.flight = None;
                r.chain.set_cap(1.0);
                r.squash = Some((ragdoll::squash_spring(speed / l.sf, self.rag_strength), feet_local, false));
                r.chain.set_gravity(0.0);
                self.placement = Placement::Ground;
                self.x11.set_target(0);
                self.home = self.ground();
                state::save_ground(self.ground());
                if hard && out.glitch {
                    let g = self.pack.behaviour.land.as_ref().and_then(|l| l.hard_glitch.clone()).unwrap_or_else(|| "land".into());
                    let kind = match g.as_str() {
                        "burst" => Some(GlitchKind::Burst),
                        "none" => None,
                        _ => Some(GlitchKind::Land),
                    };
                    if let Some(k) = kind {
                        self.glitch = Some(Glitch::new(k, now, self.rng.next()));
                    }
                }
                self.brain_feed(crate::brain::BrainEvent::Landed { hard }, now);
            }
            Contact::Perch { win, x, speed } => {
                r.landed = Some("perch");
                r.flight = None;
                r.chain.set_cap(1.0);
                let seat = l.seat_y.map_or(feet_local, |s| (feet_local.0, (s - l.avatar_y) as f32));
                r.squash = Some((ragdoll::squash_spring(speed / l.sf, self.rag_strength), seat, false));
                if let Some(rect) = self.x11.toplevel_rect(win) {
                    let dx = x.round() as i32 - rect.x;
                    let o = self.perch_origin(&rect, true, dx);
                    self.placement = Placement::Perch { win, sit: l.seat_y.is_some(), dx };
                    self.set_origin(o);
                    self.x11.set_target(win);
                    // Home is the floor point below her.
                    let fl = Self::clamp(&self.x11, &self.layout, (o.0, o.1 + 100_000));
                    self.home = (fl.0 + l.ground_x, fl.1 + l.ground_y);
                }
                self.brain_feed(crate::brain::BrainEvent::Landed { hard: false }, now);
            }
            Contact::None => {}
        }
        if let Some(r) = &mut self.rag
            && r.landed.is_some()
        {
            // No more bending once down: the squash carries the landing.
            r.chain = Chain::new(&Params::default(), &[], &[], (0.0, 0.0));
        }
    }

    fn rag_land_step(&mut self, now: Instant, t: f32, dt: f32) {
        let Some(r) = &mut self.rag else { return };
        let settled = match &mut r.squash {
            Some(sq) => {
                sq.0.step(dt);
                sq.0.done()
            }
            None => true,
        };
        if settled {
            r.squash = None;
        }
        let ev = settled.then_some(RagEvent::Settled);
        let out = r.m.update(t, &RagInput { event: ev, ..Default::default() });
        if out.phase == Phase::Idle {
            self.rag_end(now);
            return;
        }
        let fi = self.rag_frame(self.rag.as_ref().expect("rag"), out.frame.as_deref());
        if let Some(r) = &mut self.rag {
            r.frame = fi;
        }
    }

    /// The landing is over: back to normal life, the HUD and the session's news.
    fn rag_end(&mut self, now: Instant) {
        let Some(r) = self.rag.take() else { return };
        self.relayer(now);
        self.rag_restore_with(r, now, true);
    }

    fn rag_restore_with(&mut self, r: Rag, now: Instant, landed: bool) {
        self.x11.freeze_layout(false);
        self.x11.poll_pointer(false);
        if let Some(mut p) = r.prof.clone().filter(|p| !p.is_empty()) {
            p.sort_unstable();
            let q = |f: f32| p[((p.len() - 1) as f32 * f) as usize];
            eprintln!("rag-prof: {} ticks, sim+plan p50 {} µs, p95 {} µs, max {} µs", p.len(), q(0.5), q(0.95), q(1.0));
        }
        if r.restore.0
            && let Some(b) = &mut self.bubble
        {
            b.collapsed = false;
        }
        if r.restore.1
            && let Some(p) = &mut self.prompt_ui
        {
            p.dismissed = false;
            p.shown_at = now;
        }
        // The session moved on while she was carried: play that mood's entry now.
        if landed && self.mood != r.mood_at_grab {
            let m = self.mood;
            self.set_mood(m, now);
        }
        if let Some(k) = r.pending_glitch
            && self.glitch.is_none()
        {
            self.glitch = Some(Glitch::new(k, now, self.rng.next()));
        }
        if let Some(until) = r.music_until
            && until.is_none_or(|u| u > now)
        {
            self.music_on(now, until).ok();
        }
        // The brain's emotes from while she was carried (a fling's sulk).
        let queued = self.brain.as_mut().map(|h| std::mem::take(h.queued_mut())).unwrap_or_default();
        for e in queued {
            self.play_emote(e, now);
        }
        let spec = self.spec(self.mood).cloned();
        let autonomous = spec.as_ref().map_or(self.mood == Mood::Idle, |s| s.autonomy);
        if autonomous && self.pack.catalog.autonomy.enabled && !self.activities().is_empty() {
            self.next_activity = Some(now + self.autonomy_gap());
        }
    }

    /// Drop the rag at once (a command needs her, a real press beat a script): she stays
    /// where she is, on the floor below if she was in the air.
    pub(super) fn rag_cancel(&mut self, now: Instant) {
        let Some(r) = self.rag.take() else { return };
        if r.landed.is_none() && !r.rigid {
            // In the air: on the floor below (no mid-air parking).
            let o = (self.origin.0, self.origin.1 + 100_000);
            self.move_to(o, true);
            self.placement = Placement::Ground;
            self.home = self.ground();
        }
        self.rag_restore_with(r, now, true);
        self.relayer(now);
    }

    /// A render painted the rag's `next`.
    pub(super) fn rag_painted(&mut self) {
        let Some(r) = &mut self.rag else { return };
        let was = r.painted;
        r.painted = true;
        if !was && r.next.is_some() {
            self.ticker.set(self.next_deadline());
        }
    }

    /// The plan for the sim state now: it becomes `next` (painted underneath this frame).
    fn rag_plan_next(&mut self) {
        // The held bands first (they update the hold: once per tick).
        let held = {
            let Some(r) = &self.rag else { return };
            let frame = if r.rigid { self.rag_frame(r, None) } else { r.frame };
            match self.frame_or_default(frame) {
                Some(f) if !r.rigid && r.landed.is_none() => Some((f.top, f.bottom)),
                _ => None,
            }
        };
        let bp = ((4.0 * self.layout.sf).round() as i32).max(2);
        let slices = match (held, self.rag.as_mut()) {
            (Some((top, bottom)), Some(r)) => {
                let mut b = r.chain.bands_held(&mut r.hold, bp, top, bottom, r.grip.1.clamp(top, bottom - 1));
                if r.flight.is_none() {
                    ragdoll::Wobble::apply(r.wobble.px(), &mut b, top, bottom);
                }
                Some(b)
            }
            _ => None,
        };
        let Some(r) = &self.rag else { return };
        let l = &self.layout;
        let frame = if r.rigid { self.rag_frame(r, None) } else { r.frame };
        let mut plan = BodyPlan::plain();
        if !r.rigid && self.frame_or_default(frame).is_some() {
            if r.landed.is_none() {
                plan.slices = slices;
            }
            if let Some((t0, _)) = r.flip {
                let u = r.t - t0;
                plan.sy = if u < FLIP_DOWN {
                    1.0 - 0.92 * (u / FLIP_DOWN)
                } else {
                    let v = ((u - FLIP_DOWN) / FLIP_UP).clamp(0.0, 1.0);
                    if v < 0.75 { 0.08 + 0.97 * (v / 0.75) } else { 1.05 - 0.05 * ((v - 0.75) / 0.25) }
                };
                plan.pivot = (r.grip.0 as f32, r.grip.1 as f32);
            } else if let Some((sq, pivot, wall)) = &r.squash {
                let (a, b) = sq.scale();
                (plan.sx, plan.sy) = if *wall { (b, a) } else { (a, b) };
                plan.pivot = *pivot;
            } else if r.flight.is_some() {
                (plan.sx, plan.sy) = r.stretch;
                plan.pivot = ((l.ground_x - l.avatar_x) as f32, (l.ground_y - l.avatar_y) as f32);
            }
        }
        // Bands are source rows; a scaled plan draws the image scaled about the pivot, so the
        // bands go to the scaled rows too (else the clip cuts the scaled art: a flat top at
        // a stretch, a gap at a squash).
        if (plan.sx != 1.0 || plan.sy != 1.0)
            && let Some(b) = plan.slices.take()
        {
            plan.slices = Some(fx::scale_bands(&b, plan.pivot, plan.sx, plan.sy));
        }
        if let Ok(path) = std::env::var("SA_RAG_TRACE") {
            // One line per committed state: virtual time, frame, window origin, plan.
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            plan.slices.hash(&mut h);
            plan.sx.to_bits().hash(&mut h);
            plan.sy.to_bits().hash(&mut h);
            let line = format!("{:.4} {} {},{} {:016x}\n", r.t, self.pack.frames[frame].id, self.origin.0, self.origin.1, h.finish());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                use std::io::Write as _;
                f.write_all(line.as_bytes()).ok();
            }
        }
        let shown = Shown { frame, plan };
        if let Some(r) = &mut self.rag {
            r.next = Some(shown);
        }
    }

    pub(super) fn rag_deadline(&self, now: Instant) -> Option<Instant> {
        let r = self.rag.as_ref()?;
        // Waiting for the render that paints `next`: it re-arms the ticker; only a 50 ms
        // watchdog (a hidden window never renders). Before, `due.max(now)` kept the loop
        // spinning between renders: ~73 k view ticks/s in a drag (127 k rigid), ~40 renders/s.
        if r.next.is_some() && !r.painted {
            return Some(now + Duration::from_millis(50));
        }
        let active = match r.m.phase() {
            Phase::Held { .. } => {
                r.rigid
                    || !r.chain.asleep()
                    || r.flip.is_some()
                    || matches!(&r.feed, Feed::Script { s, .. } if r.t <= s.ends_at() + 0.05 || s.release_at.is_some())
            }
            _ => true,
        };
        if active {
            return Some(r.due.max(now));
        }
        // Held still by a still hand, the chain asleep: only the next frame change.
        let at = r.m.next_change(r.t)?;
        Some(now + Duration::from_secs_f32((at - r.t).max(0.0)))
    }

    /// A press on her mid-flight catches her.
    pub(super) fn rag_catch(&mut self, local: (i32, i32), now: Instant) -> bool {
        let Some(r) = &mut self.rag else { return false };
        if !matches!(r.m.phase(), Phase::Flight { .. }) {
            return false;
        }
        let vel = r.flight.map_or((0.0, 0.0), |f| f.vel);
        let frame = r.frame;
        // Where on the undeformed frame the press is (the bands bend her a little in the air).
        let local = r.cur.as_ref().and_then(|c| c.plan.slices.as_ref()).and_then(|b| r.chain.source_of(local, b)).unwrap_or(local);
        let (top, bottom) = self.frame_or_default(frame).map_or((0, 1), |f| (f.top, f.bottom.max(f.top + 1)));
        let u = ((local.1 - top) as f32 / (bottom - top) as f32).clamp(0.0, 1.0);
        let anchor = self.grip_root(self.origin, local);
        let p0 = self.x11.pointer().map_or(anchor, |p| (p.0 as f32, p.1 as f32));
        let Some(r) = &mut self.rag else { return false };
        let zone = r.m.spec.zone(u).min_torso();
        let t = r.t;
        r.m.update(t, &RagInput { event: Some(RagEvent::Catch { u }), ..Default::default() });
        r.flight = None;
        r.chain.set_cap(1.0);
        r.squash = None;
        r.landed = None;
        r.feed = Feed::Real;
        self.x11.poll_pointer(true);
        r.grip = local;
        r.zone = zone;
        r.ptr = Pointer::default();
        r.ptr.push(t, p0.0, p0.1);
        let chain = self.rag_chain(frame, local, zone, anchor);
        if let Some(r) = &mut self.rag {
            r.chain = chain;
            r.chain.kick((vel.0 * 0.2, vel.1 * 0.2));
        }
        let _ = now;
        true
    }

    /// `avatarctl drag …`
    pub(super) fn drag_cmd(&mut self, ops: Vec<DragOp>, now: Instant) -> String {
        let mut n = 0;
        for op in ops {
            n += 1;
            match op {
                DragOp::Seed(s) => self.rng = Rng::new(s),
                DragOp::Grab(at) => {
                    if let Err(e) = self.drag_grab_at(at, Script::default(), now) {
                        return e;
                    }
                }
                DragOp::Wiggle { amp, hz, ms } => match self.rag.as_mut().map(|r| &mut r.feed) {
                    Some(Feed::Script { s, .. }) => s.wiggle(amp, hz, ms),
                    _ => return "error: not held (drag grab first)".into(),
                },
                DragOp::Fling { at, v, lead_ms } => {
                    let s = Script::fling(v, lead_ms as f32 / 1000.0);
                    if let Err(e) = self.drag_grab_at(at, s, now) {
                        return e;
                    }
                }
                DragOp::Shake { at, amp, hz, ms, release } => {
                    let s = Script::shake(amp, hz, ms, release);
                    if let Err(e) = self.drag_grab_at(at, s, now) {
                        return e;
                    }
                }
                DragOp::Path(keys, smooth) => match self.rag.as_mut().map(|r| (&mut r.feed, r.t)) {
                    Some((Feed::Script { s, .. }, _)) => s.path(&keys, smooth),
                    _ => return "error: not held (drag grab first)".into(),
                },
                DragOp::Release => match self.rag.as_mut().map(|r| (&mut r.feed, r.t)) {
                    Some((Feed::Script { s, .. }, t)) => {
                        let end = s.ends_at().max(t);
                        s.release_at = Some(end);
                    }
                    _ => return "error: not held".into(),
                },
                DragOp::Cancel => self.rag_cancel(now),
            }
        }
        let ends = self.rag.as_ref().map_or(0.0, |r| match &r.feed {
            Feed::Script { s, .. } => s.ends_at(),
            Feed::Real => r.t,
        });
        format!("ok drag {n} ends_at={}", (ends * 1000.0).round() as i64)
    }

    /// Window-local device px of an opaque pixel in the middle of a grip zone, nearest the
    /// head's column.
    fn zone_point(&self, z: Zone) -> Option<(i32, i32)> {
        let f = self.frame_or_default(self.shown_frame())?;
        let spec = self.rag_spec();
        let u = match z {
            Zone::Head => spec.zones.0 * 0.5,
            Zone::Torso => (spec.zones.0 + spec.zones.1) * 0.5,
            Zone::Legs => (spec.zones.1 + 1.0) * 0.5,
        };
        let y = f.top + ((f.bottom - f.top) as f32 * u) as i32;
        let x = (0..f.mask.w as i32).filter(|&x| f.mask.get(x, y)).min_by_key(|&x| (x - f.head_x).abs())?;
        Some((x + self.layout.avatar_x, y + self.layout.avatar_y))
    }

    fn drag_grab_at(&mut self, at: Target, s: Script, now: Instant) -> Result<(), String> {
        let w = match at {
            Target::At(x, y) => (x, y),
            Target::Zone(z) => self.zone_point(z).ok_or_else(|| "error: miss".to_string())?,
        };
        if !self.loaded || self.hidden || self.quitting || self.teleport.as_ref().is_some_and(|t| matches!(t.leg, Leg::Bolt(_))) {
            return Err("error: not there".into());
        }
        if self.rag_held() {
            return Err("error: held".into());
        }
        let local = (w.0 - self.layout.avatar_x, w.1 - self.layout.avatar_y);
        let hit = self.frame_or_default(self.shown_frame()).is_some_and(|f| f.mask.get(local.0, local.1));
        if !hit {
            return Err("error: miss".into());
        }
        if self.rag.is_some() {
            // Caught mid-flight or picked up while landing: start over from here.
            self.rag_cancel(now);
        }
        let grab = ((self.applied.0 + w.0) as f32, (self.applied.1 + w.1) as f32);
        self.rag_grab(local, Feed::Script { s, grab }, now);
        Ok(())
    }

    pub(super) fn rag_json(&self) -> serde_json::Value {
        let Some(r) = &self.rag else { return serde_json::Value::Null };
        let (phase, zone) = match r.m.phase() {
            Phase::Idle => ("idle", None),
            Phase::Held { zone, .. } => ("held", Some(zone)),
            Phase::Flight { fling } => (if fling { "fling" } else { "drop" }, None),
            Phase::Land(k) => (
                match k {
                    LandKind::Soft => "land",
                    LandKind::Hard => "land_hard",
                    LandKind::Perch => "land_perch",
                },
                None,
            ),
        };
        let zone = zone.unwrap_or(r.zone);
        let frame = self.pack.frames.get(self.shown_frame()).map(|f| f.id.clone());
        let feet = r.chain.feet_offset();
        let (top, bottom) = self.frame_or_default(r.frame).map_or((0, 1), |f| (f.top, f.bottom.max(f.top + 1)));
        serde_json::json!({
            "phase": phase,
            "rigid": r.rigid,
            "zone": format!("{zone:?}").to_lowercase(),
            "frame": frame,
            "t": (r.t * 1000.0).round() / 1000.0,
            "feet": [feet.0.round(), feet.1.round()],
            "s": (r.chain.stretch() * 1000.0).round() / 1000.0,
            "vel": [r.flight.map_or(r.vel.0, |f| f.vel.0).round(), r.flight.map_or(r.vel.1, |f| f.vel.1).round()],
            "bounces": r.flight.map_or(0, |f| f.bounces),
            "grip": [((r.grip.1 - top) as f32 / (bottom - top) as f32 * 100.0).round() / 100.0, r.grip.0],
            "landed": r.landed,
            "hard": r.hard,
            "asleep": r.chain.asleep(),
        })
    }
}

impl Rag {
    fn ptr_last(&self) -> (f32, f32) {
        self.ptr.last().unwrap_or((0.0, 0.0))
    }
}

trait ZoneExt {
    fn min_torso(self) -> Zone;
}

impl ZoneExt for Zone {
    /// A catch never flips her (the ankle hang starts from a grab).
    fn min_torso(self) -> Zone {
        if self == Zone::Legs { Zone::Torso } else { self }
    }
}

/// The machine's thresholds are logical px/s.
fn scale_contact(c: Contact, sf: f32) -> Contact {
    match c {
        Contact::Bounce { normal, speed } => Contact::Bounce { normal, speed: speed / sf },
        Contact::Floor { speed } => Contact::Floor { speed: speed / sf },
        Contact::Perch { win, x, speed } => Contact::Perch { win, x, speed: speed / sf },
        Contact::None => Contact::None,
    }
}
