//! The desktop art's motions on screen (B2c; `crate::motion` holds the pure parts): the dance
//! (music's layer on the floor when it has energy), the dangle's pick and cells (the ragdoll's
//! head carry, `ragview`), and the click burst (effect layers under the pointer). The walk is a
//! brain move (`brainview`). Each is rate-capped and falls back to what she did before.

use super::*;
use crate::motion::{BurstCell, Gap};

/// The click burst's colour layers (red, green, the ring's light inner edge), over everything.
const CLICK_LAYER: usize = GLINT_LAYER + 2 * GLINTS_MAX;
const CLICK_LAYERS: usize = 3;

/// The click burst, decoded after the frames: its cells, its centre (cell px at her size) and
/// the time per cell.
pub(super) struct Burst {
    cells: Vec<BurstCell>,
    centre: (i32, i32),
    ms: u32,
}

#[derive(Default)]
pub(super) struct MotionState {
    pub walk_gap: Gap,
    dance_gap: Gap,
    dangle_gap: Gap,
    click_gap: Gap,
    /// The dance the layer shows (a change relayers).
    dancing: bool,
    burst: Option<Rc<Burst>>,
    /// A burst playing: its start and centre (root px), the cell drawn last.
    click: Option<(Instant, (i32, i32))>,
    click_cell: Option<usize>,
    pub walks: u32,
    /// Her lean before and after a walk.
    pub lean: Option<crate::motion::Lean>,
    dances: u32,
    dangles: u32,
    clicks: u32,
}

impl Avatar {
    /// Frame `i` is a cell of a motion's strip (a walk, a dance, a dangle).
    pub(super) fn motion_cell(&self, i: usize) -> bool {
        let m = &self.pack.motions;
        self.pack.frames.get(i).and_then(|f| f.cell.as_ref()).is_some_and(|c| [m.walk, m.dance, m.dangle].contains(&Some(c.strip)))
    }

    // ---- the dance -----------------------------------------------------------------------------

    /// Music started (or its tempo came in): with energy, and not too soon after the last one,
    /// she dances a while (once per session).
    pub(super) fn dance_decide(&mut self, now: Instant) {
        let Some(spec) = self.pack.behaviour.motions.dance.clone() else { return };
        if self.pack.motions.dance.is_none() {
            return;
        }
        let roll = self.rng.range(0.0, 1.0);
        let len = self.rng.range(spec.ms[0] as f32, spec.ms[1].max(spec.ms[0]) as f32) as u64;
        let ready = self.motion.dance_gap.ready(now, Duration::from_millis(spec.gap_ms));
        let Some(m) = &mut self.music else { return };
        if m.dance_rolled {
            return;
        }
        m.dance_rolled = true;
        if ready && crate::motion::dances(m.bpm, spec.min_bpm, spec.chance, roll) {
            m.dance_until = Some(now + Duration::from_millis(len));
            self.motion.dance_gap.mark(now);
            self.motion.dances += 1;
            // The music's own seam stands for her setting off.
        }
    }

    /// The dance strip, when she dances now: on the floor on her feet, idle, nothing open, no
    /// DND, its cells in (asked for if not).
    pub(super) fn dance_strip(&self) -> Option<usize> {
        let until = self.music.as_ref()?.dance_until?;
        let si = self.pack.motions.dance?;
        let ok = until > Instant::now()
            && self.placement == Placement::Ground
            && self.standing()
            && self.mood == Mood::Idle
            && !self.panel_up()
            && self.line.is_none()
            && !self.dnd_on();
        (ok && self.art_ready(si)).then_some(si)
    }

    /// The dance as a layer: its cells round and round, two beats a loop when the tempo is known.
    pub(super) fn dance_layer(&self) -> Option<(usize, Option<Seq>)> {
        let si = self.dance_strip()?;
        let first = self.pack.strip_first.get(si).copied().flatten()?;
        let s = &self.pack.anim.as_ref()?.strips[si];
        // Its clean cells only: two of lachy's have a chroma ghost drawn in (a flash on the beat).
        let cells = s.clean_cells();
        let ms = crate::motion::dance_ms(self.music.as_ref().and_then(|m| m.bpm), s.ms, cells.len() as u32);
        Some((first, Some(cells.iter().map(|&c| (first + c as usize, ms)).collect())))
    }

    /// The dance's cell on screen (for the notes' emitters), or its first.
    pub(super) fn dance_frame(&self) -> Option<usize> {
        let si = self.dance_strip()?;
        let shown = self.shown_frame();
        let on = self.pack.frames.get(shown).and_then(|f| f.cell.as_ref()).is_some_and(|c| c.strip == si);
        if on { Some(shown) } else { self.pack.strip_first.get(si).copied().flatten() }
    }

    /// Per tick: the dance comes in or goes (its time is up, she was moved, a panel opened):
    /// a relayer, the seam's glitch covers the cut. Waits for a one-shot on screen.
    pub(super) fn dance_tick(&mut self, now: Instant) -> bool {
        let leaning = self.motion.lean.is_some_and(|l| l.px(now, self.layout.sf).is_some());
        if !leaning {
            self.motion.lean = None;
        }
        leaning | self.dance_tick_inner(now)
    }

    fn dance_tick_inner(&mut self, now: Instant) -> bool {
        if let Some(m) = &mut self.music
            && m.dance_until.is_some_and(|u| u <= now)
        {
            m.dance_until = None;
        }
        let want = self.dance_strip().is_some();
        if want == self.motion.dancing || !self.loaded || self.anim.busy() {
            return false;
        }
        self.motion.dancing = want;
        self.relayer(now);
        true
    }

    /// When the dance ends on its own.
    pub(super) fn dance_deadline(&self) -> Option<Instant> {
        self.music.as_ref()?.dance_until
    }

    // ---- the dangle ----------------------------------------------------------------------------

    /// Picked up by the head: dangle this time? The rate cap and the roll. Some: the rag time
    /// it comes in at (once its cells are in: asked for at the press, and here).
    pub(super) fn dangle_pick(&mut self, now: Instant) -> Option<f32> {
        let spec = self.pack.behaviour.motions.dangle.clone()?;
        let si = self.pack.motions.dangle?;
        self.art_ready(si);
        if !self.motion.dangle_gap.ready(now, Duration::from_millis(spec.gap_ms)) {
            return None;
        }
        if self.rng.range(0.0, 1.0) >= spec.chance {
            return None;
        }
        self.motion.dangle_gap.mark(now);
        self.motion.dangles += 1;
        Some(spec.after_ms as f32 / 1000.0)
    }

    /// A press on her: the dangle's cells are asked for now, so a carry can use them.
    pub(super) fn dangle_want(&self) {
        if let Some(si) = self.pack.motions.dangle {
            self.art_ready(si);
        }
    }

    /// The dangle's cells are in.
    pub(super) fn dangle_ready(&self) -> bool {
        self.pack.motions.dangle.is_some_and(|si| self.art_ready(si))
    }

    /// The dangle's cell `secs` into it.
    pub(super) fn dangle_cell(&self, secs: f32) -> usize {
        let Some(si) = self.pack.motions.dangle else { return self.anim.frame() };
        let (Some(first), Some(s)) = (self.pack.strip_first.get(si).copied().flatten(), self.pack.anim.as_ref().map(|a| &a.strips[si])) else {
            return self.anim.frame();
        };
        // Its clean cells, round and round (six have a chroma ghost drawn in).
        let cells = s.clean_cells();
        first + cells[(secs.max(0.0) * 1000.0 / s.ms.max(1) as f32) as usize % cells.len()] as usize
    }

    // ---- the click burst -----------------------------------------------------------------------

    /// The burst's cells at her size, off the main thread (once, after the frames).
    pub(super) fn burst_decode(&mut self, cx: &mut Context<Self>) {
        let Some(e) = self.pack.motions.click.as_ref().and_then(|n| self.pack.anim.as_ref()?.effects.get(n)).cloned() else { return };
        let scale = e.k * self.layout.geo.scale;
        cx.spawn(async move |this, cx| {
            let r = crate::off_thread(move || -> anyhow::Result<Burst> {
                let bytes = std::fs::read(&e.path)?;
                if let Some(want) = &e.sha256 {
                    let got = crate::frames::sha_hex(&bytes);
                    anyhow::ensure!(got.eq_ignore_ascii_case(want), "{}: sha256 mismatch", e.path.display());
                }
                let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)?.into_rgba8();
                anyhow::ensure!(img.width() == e.cell.0 * e.frames && img.height() == e.cell.1, "{}: not {} cells of {:?}", e.path.display(), e.frames, e.cell);
                let (cells, centre) = crate::motion::burst_cells(&img, e.frames, e.cell, scale);
                Ok(Burst { cells, centre, ms: e.ms })
            })
            .await;
            this.update(cx, |v, _| match r {
                Ok(b) => v.motion.burst = Some(Rc::new(b)),
                Err(e) => elog!("synaps-avatar: click burst unavailable: {e:#}"),
            })
            .ok();
        })
        .detach();
    }

    /// A click on her at `at` (window logical px): the burst there (one at a time, capped).
    pub(super) fn burst_start(&mut self, at: Point<Pixels>, now: Instant) {
        let Some(spec) = &self.pack.behaviour.motions.click else { return };
        if self.motion.burst.is_none() || self.hidden || !self.motion.click_gap.ready(now, Duration::from_millis(spec.gap_ms)) {
            return;
        }
        let sf = self.layout.sf;
        let root = (self.applied.0 + (f32::from(at.x) * sf).round() as i32, self.applied.1 + (f32::from(at.y) * sf).round() as i32);
        self.motion.click_gap.mark(now);
        self.motion.click = Some((now, root));
        self.motion.click_cell = None;
        self.motion.clicks += 1;
    }

    /// Per tick: the burst's cell for now on its layers; gone when it's over (or she is).
    pub(super) fn burst_tick(&mut self, now: Instant) {
        let Some((t0, root)) = self.motion.click else { return };
        let Some(b) = self.motion.burst.clone() else { return };
        let i = (now.saturating_duration_since(t0).as_millis() / b.ms.max(1) as u128) as usize;
        if i >= b.cells.len() || self.hidden || self.teleport.is_some() || self.quitting {
            for j in 0..CLICK_LAYERS {
                self.os.fx_unmap(CLICK_LAYER + j);
            }
            self.os.flush();
            self.motion.click = None;
            self.motion.click_cell = None;
            return;
        }
        if self.motion.click_cell == Some(i) {
            return;
        }
        self.motion.click_cell = Some(i);
        let c = &b.cells[i];
        let o = (root.0 - b.centre.0, root.1 - b.centre.1);
        // On her only (r32: specks flew off her body): cut to the shape she shows.
        let l = self.layout;
        let body = self.frame_or_default(self.shown_frame()).map(|f| f.rects.clone()).unwrap_or_default();
        let at = (self.applied.0 + l.avatar_x - o.0, self.applied.1 + l.avatar_y - o.1);
        for j in 0..CLICK_LAYERS {
            let rows = c
                .layers
                .get(j)
                .map(|(rgb, whole, specks)| (*rgb, whole.iter().copied().chain(crate::motion::clip_to(specks, &body, at)).collect::<Vec<_>>()))
                .filter(|(_, r)| !r.is_empty());
            match rows {
                Some((rgb, rows)) => {
                    self.os.fx_set(CLICK_LAYER + j, rgb, o, (c.w, c.h), &rows).ok();
                }
                None => self.os.fx_unmap(CLICK_LAYER + j),
            }
        }
        self.os.flush();
    }

    /// The burst's next cell.
    pub(super) fn burst_deadline(&self) -> Option<Instant> {
        let (t0, _) = self.motion.click?;
        let ms = self.motion.burst.as_ref()?.ms.max(1) as u64;
        let i = self.motion.click_cell.map_or(0, |c| c as u64 + 1);
        Some(t0 + Duration::from_millis(i * ms))
    }

    pub(super) fn motion_json(&self) -> serde_json::Value {
        serde_json::json!({
            "walks": self.motion.walks,
            "walking": self.brain_walking(),
            "dances": self.motion.dances,
            "dancing": self.motion.dancing,
            "danceLeftMs": self.music.as_ref().and_then(|m| m.dance_until).map(|u| u.saturating_duration_since(Instant::now()).as_millis() as u64),
            "dangles": self.motion.dangles,
            "clicks": self.motion.clicks,
            "burst": self.motion.burst.is_some(),
        })
    }
}
