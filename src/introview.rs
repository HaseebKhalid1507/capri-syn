//! The cinematic intro on screen (`crate::intro` decides what shows when): the pack's local
//! close-ups decoded off the main thread at the launch's size, her close-up window over the
//! desktop (shaped like the art, input on its pixels only: a click there or Esc anywhere skips
//! to the out beat), then the launch bolt onto the floor or the window she sat on last time.
//! The launch heartbeat (`state::save_seen`) lives here too: it's what tells
//! a relaunch from a greeting.

use super::{Avatar, Placement};
use crate::frames::{HIT_ALPHA, alpha_mask, sha_hex};
use crate::fx::{self, BodyPlan, Glitch, GlitchStep};
use crate::intro::{self, Place, Sheet};
use crate::platform::{Os, Role, WinId};
use crate::shape::ShapeRect;
use crate::state::{self, SavedPerch};
use crate::theme::{self, Cut, Fill, tracked};
use anyhow::{Context as _, Result, bail};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use gpui_kit::*;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Her frames are in but the close-ups aren't yet: the launch waits this long, then goes on
/// without them.
const WAIT_FOR_CLOSE_UPS: Duration = Duration::from_millis(1500);
/// Her last sign of life is written this often.
const HEARTBEAT: Duration = Duration::from_secs(60);
/// The "hi" plate (logical px), and where its centre sits on the close-up (of its size).
const HI_SIZE: (f32, f32) = (128.0, 58.0);
const HI_AT: (f32, f32) = (0.86, 0.12);

/// One close-up, scaled for the launch's monitor.
pub struct CloseUp {
    pub image: Arc<RenderImage>,
    pub rects: Arc<Vec<ShapeRect>>,
    /// The opaque box (x, y, w, h), image px.
    pub body: (i32, i32, i32, i32),
}

/// Decode the close-ups at `size` (sha256-checked when listed), on up to `threads` threads.
pub fn decode(files: &[(PathBuf, Option<String>)], size: (u32, u32), threads: usize) -> Result<Vec<CloseUp>> {
    let one = |(path, sha): &(PathBuf, Option<String>), resizer: &mut Resizer| -> Result<CloseUp> {
        let bytes = std::fs::read(path).with_context(|| format!("intro {}: cannot read", path.display()))?;
        if let Some(want) = sha {
            let got = sha_hex(&bytes);
            if !got.eq_ignore_ascii_case(want) {
                bail!("intro {}: sha256 mismatch (anim.json {want}, file {got})", path.display());
            }
        }
        let img = image::load_from_memory(&bytes).with_context(|| format!("intro {}: not an image", path.display()))?.into_rgba8();
        let (iw, ih) = img.dimensions();
        let src = Image::from_vec_u8(iw, ih, img.into_raw(), PixelType::U8x4)?;
        let mut dst = Image::new(size.0, size.1, PixelType::U8x4);
        resizer.resize(&src, &mut dst, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)))?;
        let mut px = dst.into_vec();
        let mask = alpha_mask(&px, size.0, size.1);
        let rects = mask.rects();
        let body = crate::shape::bbox(&rects).map_or((0, 0, size.0 as i32, size.1 as i32), |b| (b.0 as i32, b.1 as i32, b.2 as i32, b.3 as i32));
        for p in px.as_chunks_mut::<4>().0.iter_mut() {
            // Below the hit alpha nothing shows without a compositor: none with one either,
            // so both look alike (no faint fringe outside the shape).
            if p[3] < HIT_ALPHA {
                *p = [0; 4];
            }
            p.swap(0, 2);
        }
        let rgba = image::RgbaImage::from_raw(size.0, size.1, px).context("buffer size")?;
        Ok(CloseUp { image: Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])), rects: Arc::new(rects), body })
    };
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: std::sync::Mutex<Vec<Option<Result<CloseUp>>>> = std::sync::Mutex::new((0..files.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads.clamp(1, files.len().max(1)) {
            s.spawn(|| {
                let mut resizer = Resizer::new();
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(f) = files.get(i) else { break };
                    let r = one(f, &mut resizer);
                    slots.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    slots.into_inner().unwrap().into_iter().map(|r| r.unwrap_or_else(|| bail!("not decoded"))).collect()
}

/// The close-up's size for a monitor `h` px tall (`intro::place`'s).
fn size_for(monitor_h: i32, area: (i32, i32, i32, i32), src: (u32, u32)) -> (u32, u32) {
    let p = intro::place(area, monitor_h, src, 1.0);
    (p.image.0 as u32, p.image.1 as u32)
}

/// The source size of the first close-up (all share it), from its header.
fn src_size(p: &std::path::Path) -> Option<(u32, u32)> {
    image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.into_dimensions().ok()
}

#[derive(Default)]
pub(super) struct IntroState {
    pub cfg: crate::settings::IntroSettings,
    /// Her last sign of life and her perch then, read at start (before the heartbeat writes).
    seen: Option<f64>,
    /// When the close-up last played (`cinematic_when = "daily"`).
    played: Option<f64>,
    saved_perch: Option<SavedPerch>,
    /// Decoded close-ups, a decode in flight, or failed.
    frames: Option<Arc<Vec<CloseUp>>>,
    decoding: bool,
    failed: bool,
    /// The close-up on screen: its window, view and id.
    window: Option<(AnyWindowHandle, WeakEntity<CloseUpView>, WinId)>,
    /// The launch waits for the close-ups (since).
    waiting: Option<Instant>,
    /// Where the bolt goes once the close-up is over.
    target: Option<((i32, i32), Placement)>,
    /// What the last launch did (state dump): "played", "skipped click", "no: gap" …
    pub last: String,
    plays: u32,
    beat: bool,
}

impl IntroState {
    pub fn new() -> IntroState {
        let cfg = crate::settings::Settings::load().map(|s| s.intro).unwrap_or_default();
        IntroState {
            cfg,
            seen: state::load_seen(),
            played: state::load_intro_played(),
            saved_perch: state::load_perch(),
            last: "none".into(),
            ..IntroState::default()
        }
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The local calendar day of a unix time (year, day of the year).
fn local_day(unix: f64) -> Option<(i32, i32)> {
    let t = unix as libc::time_t;
    // SAFETY: localtime_r only writes the tm we own; a null return is checked.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    (!r.is_null()).then_some((tm.tm_year, tm.tm_yday))
}

/// It played on the local day of `now` already.
fn played_today(played: Option<f64>, now: f64) -> bool {
    played.is_some_and(|p| local_day(p).is_some() && local_day(p) == local_day(now))
}

impl Avatar {
    fn intro_files(&self) -> &[(PathBuf, Option<String>)] {
        self.pack.anim.as_ref().map_or(&[], |a| a.intro.as_slice())
    }

    /// The close-up may play at this launch (it's checked again for fullscreen when it starts).
    fn intro_allowed(&self, why: &mut String) -> bool {
        let c = &self.intro.cfg;
        let now = unix_now();
        let today = c.cinematic_daily && played_today(self.intro.played, now);
        let ok = intro::plays(c.cinematic, !self.intro_files().is_empty(), false, self.intro.seen, now, c.cinematic_min_gap_min, today);
        if !ok {
            *why = if !c.cinematic {
                "no: off".into()
            } else if self.intro_files().is_empty() {
                "no: no close-ups in the pack".into()
            } else if today {
                "no: played today (daily)".into()
            } else {
                "no: relaunch within the gap".into()
            };
        }
        ok
    }

    /// The first frame is in: start decoding the close-ups for the launch's monitor, if it may play.
    pub(super) fn intro_prepare(&mut self, cx: &mut Context<Self>) {
        let mut why = String::new();
        if self.os.active_window().is_some_and(|w| self.os.is_fullscreen(w)) {
            return;
        }
        if !self.intro_allowed(&mut why) {
            self.intro.last = why;
            return;
        }
        let (to, place) = self.launch_target();
        self.intro_decode(to, place, cx);
    }

    fn intro_decode(&mut self, to: (i32, i32), place: Placement, cx: &mut Context<Self>) {
        if self.intro.decoding || self.intro.frames.is_some() || self.intro.failed {
            return;
        }
        let files = self.intro_files().to_vec();
        let Some(src) = files.first().and_then(|f| src_size(&f.0)) else {
            self.intro.failed = true;
            return;
        };
        let (gx, gy) = self.target_point(to, place);
        let m = self.os.monitor_at(gx, gy - 1);
        let a = self.os.area_at(gx, gy - 1);
        let size = size_for(m.h, (a.x, a.y, a.w, a.h), src);
        self.intro.decoding = true;
        cx.spawn(async move |this, cx| {
            let t0 = Instant::now();
            let r = crate::off_thread(move || decode(&files, size, 4)).await;
            this.update(cx, |v, cx| v.intro_decoded(r, t0, cx)).ok();
        })
        .detach();
    }

    fn intro_decoded(&mut self, r: Result<Vec<CloseUp>>, t0: Instant, cx: &mut Context<Self>) {
        self.intro.decoding = false;
        match r {
            Ok(f) => {
                let s = f.first().map(|c| c.image.size(0)).map_or((0, 0), |s| (i32::from(s.width), i32::from(s.height)));
                elog!("synaps-avatar: intro: {} close-ups {}x{} decoded in {} ms", f.len(), s.0, s.1, t0.elapsed().as_millis());
                self.intro.frames = Some(Arc::new(f));
                if self.intro.waiting.take().is_some() {
                    self.intro_open(cx);
                }
            }
            Err(e) => {
                elog!("synaps-avatar: intro: close-ups unavailable: {e:#}");
                self.intro.failed = true;
                self.intro.last = "no: decode failed".into();
                if self.intro.waiting.take().is_some() {
                    self.intro_chain(false, Instant::now(), cx);
                }
            }
        }
    }

    /// Root px of the point she stands or sits on at origin `to`.
    fn target_point(&self, to: (i32, i32), place: Placement) -> (i32, i32) {
        let l = &self.layout;
        let y = match place {
            Placement::Perch { sit: true, .. } => l.seat_y.unwrap_or(l.ground_y),
            _ => l.ground_y,
        };
        (to.0 + l.ground_x, to.1 + y)
    }

    /// Where the launch lands: the window she sat on at her last sign of life, if it's still
    /// there (same size, same place, room on its exposed top) and the cinematic launch is on;
    /// else where she stands.
    pub(super) fn launch_target(&self) -> ((i32, i32), Placement) {
        let here = (self.origin, self.placement);
        let Some(s) = self.intro.saved_perch.filter(|_| self.intro.cfg.cinematic) else { return here };
        let Some(r) = self.os.toplevel_rect(s.win) else { return here };
        if (r.x, r.y, r.w, r.h) != s.rect || self.os.is_fullscreen(s.win) {
            return here;
        }
        let sit = s.sit && self.layout.seat_y.is_some();
        let x = r.x + s.dx;
        let (_, seats) = self.exposed_seats(&r, sit);
        if !seats.iter().any(|&(lo, hi)| lo <= x && x <= hi) {
            return here;
        }
        let o = self.perch_origin(&r, sit, s.dx);
        if !self.perch_fits(&r, o, sit) {
            return here;
        }
        (o, Placement::Perch { win: s.win, sit, dx: s.dx })
    }

    /// The launch (her frames are in): the close-up if it may play, then the bolt. False: no
    /// launch sequence at all (she just appears). Close-ups decoded for nothing are dropped.
    pub(super) fn launch_cinematic(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        let target = self.launch_target();
        if let Placement::Perch { win, .. } = target.1 {
            elog!("synaps-avatar: launch: back onto window 0x{win:x}");
        }
        let mut why = String::new();
        let fullscreen = self.os.active_window().is_some_and(|w| self.os.is_fullscreen(w));
        let may = self.intro_allowed(&mut why) && !self.intro.failed;
        if may && fullscreen {
            why = "no: fullscreen".into();
        }
        if !may || fullscreen {
            if !why.is_empty() {
                self.intro.last = why;
            }
            self.intro.frames = None;
            return self.launch_bolt(target, false, now);
        }
        self.intro.target = Some(target);
        self.hidden = true;
        if self.intro.frames.is_some() {
            self.intro_open(cx);
        } else {
            self.intro_decode(target.0, target.1, cx);
            self.intro.waiting = Some(now);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(WAIT_FOR_CLOSE_UPS).await;
                this.update(cx, |v, cx| {
                    if v.intro.waiting.take().is_some() {
                        elog!("synaps-avatar: intro: the close-ups took too long; launching without them");
                        v.intro.last = "no: close-ups late".into();
                        v.intro_chain(false, Instant::now(), cx);
                    }
                })
                .ok();
            })
            .detach();
        }
        true
    }

    /// Open the close-up's window over her target's monitor.
    fn intro_open(&mut self, cx: &mut Context<Self>) {
        let Some(frames) = self.intro.frames.clone() else { return };
        let (to, place) = self.intro.target.unwrap_or((self.origin, self.placement));
        let (gx, gy) = self.target_point(to, place);
        let m = self.os.monitor_at(gx, gy - 1);
        let a = self.os.area_at(gx, gy - 1);
        let sf = self.layout.sf;
        let size = frames.first().map(|c| c.image.size(0)).map_or((1, 1), |s| (u32::from(s.width), u32::from(s.height)));
        // The decoded size wins (a monitor of another height since: still centred on its bottom).
        let mut p = intro::place((a.x, a.y, a.w, a.h), m.h, size, sf);
        let (dw, dh) = (size.0 as i32 - p.image.0, size.1 as i32 - p.image.1);
        p.image = (size.0 as i32, size.1 as i32);
        p.window = (p.window.0 - dw / 2, p.window.1 - dh, p.window.2 + dw, p.window.3 + dh);
        let (x, y, w, h) = p.window;
        let bounds = Bounds::new(point(px(x as f32 / sf), px(y as f32 / sf)), gpui_kit::size(px(w as f32 / sf), px(h as f32 / sf)));
        let options = self.os.window_options(Role::Intro, bounds, false);
        let me = cx.entity().downgrade();
        let os = self.os.clone();
        let (font, pal) = (self.font.clone(), self.pack.palette());
        let seed = self.rng.next();
        let mut view = None;
        let mut id = 0;
        let opened = cx.open_window(options, |window, cx| {
            id = os.adopt(Role::Intro, window).unwrap_or(0);
            let on_done: OnDone = Box::new(move |cx: &mut App| {
                me.update(cx, |a, cx| a.intro_done(cx)).ok();
            });
            let v = cx.new(|_| CloseUpView::new(os.clone(), frames, p, sf, seed, font, pal, on_done));
            view = Some(v.downgrade());
            v
        });
        match (opened, view) {
            (Ok(handle), Some(v)) => {
                if let Some(c) = v.upgrade() {
                    c.update(cx, |c, _| c.id = id);
                }
                self.intro.window = Some((handle.into(), v, id));
                self.intro.plays += 1;
                self.intro.played = Some(unix_now());
                state::save_intro_played(unix_now());
                let play = self.intro.plays;
                // A close-up that never gets to its end (no frames for it) doesn't hold her launch.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(intro::END + 2500)).await;
                    this.update(cx, |v, cx| {
                        if v.intro.plays == play && v.intro.window.is_some() {
                            elog!("synaps-avatar: intro: the close-up stalled; on with the launch");
                            v.intro_done(cx);
                        }
                    })
                    .ok();
                })
                .detach();
                self.os.arm_escape(true);
                elog!("synaps-avatar: intro: close-up {}x{} at {x},{y} (window {w}x{h})", p.image.0, p.image.1);
            }
            (Err(e), _) => {
                elog!("synaps-avatar: intro: cannot open its window: {e:#}");
                self.intro.last = "no: window".into();
                self.intro_chain(false, Instant::now(), cx);
            }
            _ => self.intro_chain(false, Instant::now(), cx),
        }
    }

    /// Click on the close-up or Esc: to the out beat.
    pub(super) fn intro_skip(&mut self, why: &'static str, cx: &mut Context<Self>) {
        if let Some((_, v, _)) = &self.intro.window {
            v.update(cx, |c, cx| c.skip(why, cx)).ok();
        }
    }

    /// The close-up is over: its window goes (and its pixels with it), the bolt comes.
    fn intro_done(&mut self, cx: &mut Context<Self>) {
        self.os.arm_escape(false);
        if let Some((h, v, _)) = self.intro.window.take() {
            let skipped = v.upgrade().and_then(|c| c.read(cx).skipped);
            self.intro.last = skipped.map_or_else(|| "played".to_string(), |w| format!("skipped {w}"));
            if let Some(f) = &self.intro.frames {
                for c in f.iter() {
                    cx.drop_image(c.image.clone(), None);
                }
            }
            h.update(cx, |_, window, _| window.remove_window()).ok();
        }
        // Played once per launch; a dev replay decodes them again.
        self.intro.frames = None;
        self.intro_chain(true, Instant::now(), cx);
    }

    /// After the close-up (`greeted`: it was on screen) or without it: the launch bolt to the
    /// target.
    fn intro_chain(&mut self, greeted: bool, now: Instant, cx: &mut Context<Self>) {
        let target = self.intro.target.take().unwrap_or((self.origin, self.placement));
        if !self.launch_bolt(target, greeted, now) && self.parked.is_none() {
            self.hidden = false;
            self.glitch = Some(Glitch::new(fx::GlitchKind::Appear, now, self.rng.next()));
        }
        cx.notify();
        self.schedule(cx);
    }

    /// The close-up is on screen or about to be.
    pub(super) fn intro_playing(&self) -> bool {
        self.intro.window.is_some() || self.intro.waiting.is_some()
    }

    /// Dev: `intro` plays the launch again where she is; `intro skip` skips the one playing.
    pub(super) fn intro_command(&mut self, skip: bool, now: Instant, cx: &mut Context<Self>) -> String {
        if skip {
            if self.intro.window.is_none() {
                return "error: no intro playing".into();
            }
            self.intro_skip("ctl", cx);
            return "ok".into();
        }
        if !self.loaded || self.intro_playing() || self.teleport.is_some() || self.rag.is_some() || self.parked.is_some() {
            return "error: not now".into();
        }
        if self.intro_files().is_empty() || !self.intro.cfg.cinematic {
            return "error: no cinematic intro (setting off, or the pack has no close-ups)".into();
        }
        self.intro.failed = false;
        self.intro.target = Some((self.origin, self.placement));
        self.hidden = true;
        self.glitch = None;
        self.intro.waiting = Some(now);
        if self.intro.frames.is_some() {
            self.intro.waiting = None;
            self.intro_open(cx);
        } else {
            self.intro_decode(self.origin, self.placement, cx);
        }
        cx.notify();
        "ok".into()
    }

    pub(super) fn intro_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cinematic": self.intro.cfg.cinematic,
            "closeUps": self.intro_files().len(),
            "playing": self.intro_playing(),
            "last": self.intro.last,
            "plays": self.intro.plays,
            "seenAgoS": self.intro.seen.map(|t| (unix_now() - t).round()),
            "daily": self.intro.cfg.cinematic_daily,
            "playedToday": played_today(self.intro.played, unix_now()),
            "savedPerch": self.intro.saved_perch.map(|p| format!("0x{:x}", p.win)),
        })
    }

    // ---- the heartbeat -----------------------------------------------------------------------

    /// Her last sign of life, and her perch: now, then every minute (and at a quit).
    pub(super) fn heartbeat_start(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.intro.beat, true) {
            return;
        }
        self.save_seen();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(HEARTBEAT).await;
                if this.update(cx, |v, _| v.save_seen()).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn save_seen(&self) {
        let perch = match self.placement {
            Placement::Perch { win, sit, dx } if self.teleport.is_none() => {
                self.os.toplevel_rect(win).map(|r| SavedPerch { win, sit, dx, rect: (r.x, r.y, r.w, r.h) })
            }
            _ => None,
        };
        state::save_seen(unix_now(), perch);
    }
}

pub type OnDone = Box<dyn FnOnce(&mut App)>;

/// The close-up's window: plays `Sheet` from its own clock, shapes itself to what it draws.
pub struct CloseUpView {
    os: Rc<Os>,
    pub id: WinId,
    frames: Arc<Vec<CloseUp>>,
    sheet: Sheet,
    t0: Instant,
    seed: u64,
    sf: f32,
    place: Place,
    font: SharedString,
    pal: theme::Pal,
    step: u64,
    warmed: bool,
    sent: u64,
    on_done: Option<OnDone>,
    /// Skipped by: "click", "esc", "ctl".
    pub skipped: Option<&'static str>,
    /// Frames painted; the wake-up asked for (ms into the intro) while nothing moves.
    paints: u32,
    started: bool,
    timer_for: Option<u64>,
    /// Under a glitch (every frame): frames, and the longest gap between two (ms).
    glitch_paints: (u32, u64, Option<u64>),
}

impl CloseUpView {
    #[allow(clippy::too_many_arguments)]
    fn new(os: Rc<Os>, frames: Arc<Vec<CloseUp>>, place: Place, sf: f32, seed: u64, font: SharedString, pal: theme::Pal, on_done: OnDone) -> Self {
        CloseUpView {
            os,
            id: 0,
            frames,
            sheet: Sheet::new(seed >> 7),
            t0: Instant::now(),
            seed,
            sf,
            place,
            font,
            pal,
            step: 0,
            warmed: false,
            sent: 0,
            on_done: Some(on_done),
            skipped: None,
            paints: 0,
            started: false,
            timer_for: None,
            glitch_paints: (0, 0, None),
        }
    }

    fn t(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.t0).as_millis() as u64
    }

    fn skip(&mut self, why: &'static str, cx: &mut Context<Self>) {
        let t = self.t(Instant::now());
        if !self.sheet.skipped() {
            self.sheet.skip(t);
            if self.sheet.skipped() {
                self.skipped = Some(why);
                elog!("synaps-avatar: intro: skipped ({why}) at {t} ms");
            }
        }
        cx.notify();
    }

    /// The "hi" plate's rect, window device px.
    fn hi_rect(&self, ox: i32, oy: i32) -> ShapeRect {
        let (iw, ih) = self.place.image;
        let (w, h) = ((HI_SIZE.0 * self.sf) as i32, (HI_SIZE.1 * self.sf) as i32);
        let (cx, cy) = (ox + (iw as f32 * HI_AT.0) as i32, oy + (ih as f32 * HI_AT.1) as i32);
        ((cx - w / 2) as i16, (cy - h / 2) as i16, w as u16, h as u16)
    }
}

fn hash_rects(a: &[ShapeRect], b: &[ShapeRect]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    0xffu8.hash(&mut h);
    b.hash(&mut h);
    h.finish()
}

fn moved(r: &[ShapeRect], dx: i32, dy: i32) -> Vec<ShapeRect> {
    r.iter().map(|&(x, y, w, h)| ((x as i32 + dx) as i16, (y as i32 + dy) as i16, w, h)).collect()
}

impl Render for CloseUpView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The clock starts at the first frame (it uploads every close-up: the slow one).
        if !self.started {
            self.started = true;
            self.t0 = Instant::now();
        }
        let now = Instant::now();
        let t = self.t(now);
        if self.sheet.done(t) {
            if let Some(f) = self.on_done.take() {
                self.os.shape_of(self.id, &[], &[]).ok();
                let (n, gap, _) = self.glitch_paints;
                elog!("synaps-avatar: intro: {} frames painted in {t} ms ({n} under a glitch, longest gap {gap} ms)", self.paints);
                cx.defer(f);
            }
            return div().size_full();
        }
        let cue = self.sheet.cue(t);
        // Moving (a glitch re-rolls every frame): every frame. Still: a wake-up at the next change.
        let next = self.sheet.next_change(t);
        if cue.glitch.is_some() || next <= t + 20 {
            window.request_animation_frame();
        } else if self.timer_for != Some(next) {
            self.timer_for = Some(next);
            let wait = Duration::from_millis(next - t + 2);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |v, cx| {
                    v.timer_for = None;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.paints += 1;
        if cue.glitch.is_some() {
            let (n, gap, last) = self.glitch_paints;
            self.glitch_paints = (n + 1, last.map_or(gap, |l| gap.max(t - l)), Some(t));
        } else {
            self.glitch_paints.2 = None;
        }
        let f = &self.frames[cue.frame.min(self.frames.len() - 1)];
        let (iw, ih) = self.place.image;
        let sf = self.sf;
        let mut plan = BodyPlan::plain();
        plan.sx = cue.scale;
        plan.sy = cue.scale;
        plan.pivot = (iw as f32 / 2.0, ih as f32);
        if let Some(gc) = cue.glitch {
            let mut g = Glitch::new(gc.kind, self.t0 + Duration::from_millis(gc.at), self.seed ^ gc.at);
            g.ms = gc.ms as f32;
            self.step += 1;
            plan.glitch(GlitchStep {
                strength: g.strength(now),
                coverage: g.coverage(now),
                seed: g.step_seed(self.step),
                order: g.seed(),
                y0: 0,
                h: ih,
                body: f.body,
                face: None,
                solid: g.solid(now),
                tag: None,
                sf,
                calm: false,
            });
            plan.tint = g.tint(now);
        }
        if cue.red {
            plan.tint = Some(fx::BOLT_RED);
        }
        if let Some(u) = cue.out {
            plan.slices = Some(intro::out_bands(ih, u, self.seed, (10.0 * sf).round() as i32));
        }
        let ox = self.place.at.0 + (cue.jolt.0 * sf).round() as i32;
        let oy = self.place.at.1 + (cue.jolt.1 * sf).round() as i32 + (cue.rise * ih as f32).round() as i32;
        let alpha = self.os.composited();
        let body = if plan.is_plain() && plan.tint.is_none() { f.rects.to_vec() } else { plan.shape(&f.rects) };
        let input = moved(&body, ox, oy);
        let mut visible = input.clone();
        if cue.hi {
            visible.push(self.hi_rect(ox, oy));
        }
        let h = hash_rects(&input, if alpha { &[] } else { &visible });
        // Never an empty shape: GPUI stops drawing a fully obscured X window (and this one
        // would never get to its end).
        if h != self.sent && !input.is_empty() {
            self.sent = h;
            self.os.shape_of(self.id, &input, &visible).ok();
        }
        let ghost = plan.ghost_rects(&f.rects);
        let solid = plan.solid_rects(&f.rects);
        let tint = plan.tint.map(|c| (c, plan.body_rects(&f.rects)));
        let bars = if plan.tint.is_some() { Vec::new() } else { plan.bar_rects(&f.rects) };
        let img = f.image.clone();
        let warm: Vec<Arc<RenderImage>> = if std::mem::replace(&mut self.warmed, true) { Vec::new() } else { self.frames.iter().map(|c| c.image.clone()).collect() };
        let plan = Rc::new(plan);
        let entity = cx.entity();
        let mut root = div().size_full().relative().font_family(self.font.clone()).child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if !warm.is_empty() {
                        // Every close-up into the atlas now, unseen: no upload stall in a later beat.
                        let off = ContentMask { bounds: Bounds::new(point(px(-8.0), px(-8.0)), gpui_kit::size(px(1.0), px(1.0))) };
                        window.with_content_mask(Some(off), |window| {
                            for w in warm {
                                window.paint_image(bounds, bounds, Corners::default(), w, 0, false).ok();
                            }
                        });
                    }
                    let (bx, by) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(bx + (ox as f32 + x) / sf), px(by + (oy as f32 + y) / sf)), gpui_kit::size(px(w / sf), px(h / sf)));
                    let quad = |window: &mut Window, r: &ShapeRect, c: u32| {
                        window.paint_quad(fill(rect(r.0 as f32, r.1 as f32, r.2 as f32, r.3 as f32), rgb(c)));
                    };
                    let full = rect(0.0, 0.0, iw as f32, ih as f32);
                    // Without a compositor the bounding shape lands a frame late: the plain
                    // close-up underneath, so wherever it grows there is art, not black.
                    if !alpha && plan.slices.is_some() {
                        window.paint_image(full, full, Corners::default(), img.clone(), 0, false).ok();
                    }
                    for (r, c) in ghost.iter().chain(&solid) {
                        quad(window, r, *c);
                    }
                    let (ix, iy, sw, sh) = plan.image_rect(iw, ih);
                    match (&tint, &plan.slices) {
                        (Some((c, rs)), _) => {
                            for r in rs {
                                quad(window, r, *c);
                            }
                        }
                        (None, None) => {
                            let r = rect(ix, iy, sw, sh);
                            window.paint_image(r, r, Corners::default(), img.clone(), 0, false).ok();
                        }
                        (None, Some(bands)) => {
                            for (i, &(y0, y1, dx, dy)) in bands.iter().enumerate() {
                                if plan.solid.get(i).is_some_and(Option::is_some) {
                                    continue;
                                }
                                let shifted = rect(ix + dx as f32, iy + dy as f32, sw, sh);
                                let clip = rect(ix + dx as f32, (y0 + dy) as f32, sw, (y1 - y0) as f32);
                                window.paint_image(clip, shifted, Corners::default(), img.clone(), 0, false).ok();
                            }
                        }
                    }
                    for (r, c) in &bars {
                        quad(window, r, *c);
                    }
                    let e = entity.clone();
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            e.update(cx, |v, cx| v.skip("click", cx));
                        }
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        if cue.hi {
            let r = self.hi_rect(ox, oy);
            let pal = self.pal;
            root = root.child(
                div()
                    .absolute()
                    .left(px(r.0 as f32 / sf))
                    .top(px(r.1 as f32 / sf))
                    .w(px(r.2 as f32 / sf))
                    .h(px(r.3 as f32 / sf))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(theme::backdrop(Cut::Slant(9.0), Fill::framed(pal.accent, pal.ring, 1.0)))
                    .child(tracked("HI", 4.0).text_size(px(32.0)).font_weight(FontWeight::BOLD).text_color(rgb(pal.ink))),
            );
        }
        root
    }
}
