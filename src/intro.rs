//! The cinematic intro (PROPOSAL-desktop-art §5): her close-up over the desktop at launch, then
//! the bolt. Our own beat sheet over the pack's local close-ups (`anim/mochi/intro-*`, from
//! `make-pack --mochi`: local only, never in this repo). Pure: times, frames, effects, where it
//! goes, whether it plays. No OS, no pixels.

use crate::fx::GlitchKind;

/// The close-ups, in the order `art::Anim::intro` holds them.
pub const FRAMES: [&str; 9] = ["grab", "smear", "wave1", "wave2", "wave3", "mouth1", "mouth2", "mouth3", "happy"];
const GRAB: usize = 0;
const SMEAR: usize = 1;
const WAVE: [usize; 5] = [2, 3, 4, 3, 4];
const MOUTH: [(usize, u64); 4] = [(5, 90), (6, 130), (7, 130), (5, 200)];
const HAPPY: usize = 8;

/// Beat starts, ms. The bolt starts `HANDOFF_MS` before the end (no empty frame between them);
/// with it (~0.2 s to contact) and her one landing (`arrive.cinematic`) she is idle on her spot
/// ~2.9 s after the close-up's first frame.
///
/// In: the close-up is where it rests from its first frame (its shape never moves or grows in
/// the entrance: nothing shows that it hasn't drawn), under a red tint and scanlines that settle
/// off it in `SETTLE_MS`; her face reads by the third frame. Then the impact's squash.
pub const SETTLE_MS: u64 = 150;
pub const IMPACT_AT: u64 = SETTLE_MS;
const SCALE_AT: u64 = IMPACT_AT;
const SCALE_MS: u64 = 120;
pub const GRAB_AT: u64 = 250;
const JOLT_MS: u64 = 240;
pub const WHIP_AT: u64 = 530;
pub const WAVE_AT: u64 = 610;
const WAVE_MS: u64 = 140;
const BURST_MS: u64 = 280;
pub const HI_AT: u64 = 1310;
pub const HAPPY_AT: u64 = 1860;
pub const OUT_AT: u64 = 2260;
pub const OUT_MS: u64 = 260;
/// The bolt takes over.
pub const END: u64 = OUT_AT + OUT_MS;
/// The bolt starts this long before the close-up's end: it holds its last rows over the bolt.
pub const HANDOFF_MS: u64 = 80;

/// The close-up's height, of her monitor's.
pub const HEIGHT_OF_MONITOR: f32 = 0.62;
/// The out beat sinks it back toward the bottom edge this much of its height.
const SINK: f32 = 0.10;
/// The impact's squash (its height; never wider or taller than at rest: a shape that grows
/// shows what the window hasn't drawn yet), and the grab's jolt (logical px).
const IMPACT_SQUASH: f32 = 0.95;
const JOLT_PX: f32 = 2.0;
/// Room around the image for the glitch's shifted bands and the jolt (logical px).
pub const MARGIN: f32 = 48.0;

/// A glitch running over the close-up: its kind, start and length (ms into the intro).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlitchCue {
    pub kind: GlitchKind,
    pub at: u64,
    pub ms: u64,
}

/// What shows at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    /// Index into `FRAMES`.
    pub frame: usize,
    pub glitch: Option<GlitchCue>,
    /// The entrance, 0..1: a red tint and scanlines over the close-up's own pixels, settling off.
    pub settle: Option<f32>,
    /// Vertical scale about the bottom centre (the impact's squash).
    pub scale: f32,
    /// Logical px.
    pub jolt: (f32, f32),
    /// Fraction of its height still below its resting place.
    pub rise: f32,
    /// The "hi" beat: her tag shows it.
    pub hi: bool,
    /// The out beat, 0..1: rows of the close-up drop out where they are (`out_bands`) as it
    /// sinks; nothing is drawn outside its own pixels.
    pub out: Option<f32>,
}

/// One play of the intro. `burst`: which of the five wave frames the Burst glitch hits.
#[derive(Clone, Copy, Debug)]
pub struct Sheet {
    burst: usize,
    skip: Option<u64>,
}

impl Sheet {
    pub fn new(roll: u64) -> Sheet {
        Sheet { burst: (roll % WAVE.len() as u64) as usize, skip: None }
    }

    /// Click or Esc at `t`: straight to the out beat from what shows now.
    pub fn skip(&mut self, t: u64) {
        if self.skip.is_none() && t < OUT_AT {
            self.skip = Some(t);
        }
    }

    pub fn skipped(&self) -> bool {
        self.skip.is_some()
    }

    /// When it ends (its window goes).
    pub fn end(&self) -> u64 {
        self.skip.map_or(END, |s| s + OUT_MS)
    }

    /// When the bolt starts (just before the end).
    pub fn handoff(&self) -> u64 {
        self.end().saturating_sub(HANDOFF_MS)
    }

    pub fn done(&self, t: u64) -> bool {
        t >= self.end()
    }

    /// The next time (ms) what shows changes, at most `end()` (5 ms resolution).
    pub fn next_change(&self, t: u64) -> u64 {
        let c = self.cue(t);
        (t + 1..self.end()).step_by(5).find(|&u| self.cue(u) != c).unwrap_or(self.end())
    }

    pub fn cue(&self, t: u64) -> Cue {
        if let Some(s) = self.skip
            && t >= s
        {
            let shown = Sheet { skip: None, ..*self }.cue(s);
            let u = (t - s) as f32 / OUT_MS as f32;
            return Cue { glitch: None, settle: None, hi: false, scale: 1.0, jolt: (0.0, 0.0), out: Some(u), rise: shown.rise + SINK * u * u, ..shown };
        }
        let mut c = Cue { frame: GRAB, glitch: None, settle: None, scale: 1.0, jolt: (0.0, 0.0), rise: 0.0, hi: false, out: None };
        if t < SETTLE_MS {
            // 20 ms steps: a new strength each frame drawn, no faster.
            c.settle = Some((t / 20 * 20) as f32 / SETTLE_MS as f32);
        } else if t < GRAB_AT {
            // The impact: its squash (below).
        } else if t < WHIP_AT {
            if t < GRAB_AT + JOLT_MS {
                let u = (t - GRAB_AT) as f32 / JOLT_MS as f32;
                let a = JOLT_PX * (1.0 - u);
                let w = u * 2.0 * std::f32::consts::TAU;
                c.jolt = ((a * w.sin()).round(), (a * (w + 1.0).sin()).round());
            }
        } else if t < WAVE_AT {
            c.frame = SMEAR;
        } else if t < HI_AT {
            let k = (((t - WAVE_AT) / WAVE_MS) as usize).min(WAVE.len() - 1);
            c.frame = WAVE[k];
        } else if t < HAPPY_AT {
            c.hi = true;
            let mut at = HI_AT;
            c.frame = MOUTH[MOUTH.len() - 1].0;
            for (f, ms) in MOUTH {
                if t < at + ms {
                    c.frame = f;
                    break;
                }
                at += ms;
            }
        } else {
            c.frame = HAPPY;
            if t >= OUT_AT {
                let u = (t - OUT_AT) as f32 / OUT_MS as f32;
                c.out = Some(u);
                c.rise = SINK * u * u;
            }
        }
        // One Burst, from a wave frame (it may run on into "hi").
        let at = WAVE_AT + self.burst as u64 * WAVE_MS;
        if c.glitch.is_none() && (at..at + BURST_MS).contains(&t) {
            c.glitch = Some(GlitchCue { kind: GlitchKind::Burst, at, ms: BURST_MS });
        }
        // The impact's squash eases back through the start of the grab.
        if (SCALE_AT..SCALE_AT + SCALE_MS).contains(&t) {
            let u = (t - SCALE_AT) as f32 / SCALE_MS as f32;
            c.scale = 1.0 - (1.0 - IMPACT_SQUASH) * (1.0 - u) * (1.0 - u);
        }
        c
    }
}

/// The entrance at `u` (0..1): the red tint's and the scanlines' strength (alpha). Most of it is
/// gone by the third frame (u ~0.27 at 30 fps): her face reads.
pub fn settle(u: f32) -> (f32, f32) {
    let k = (1.0 - u.clamp(0.0, 1.0)).powi(2);
    (0.55 * k * k, 0.5 * k)
}

/// Plays at all: the setting, the pack's close-ups, nothing fullscreen focused, not a relaunch
/// within `min_gap_min` of her last sign of life (`seen`, unix s; a clock that went back counts
/// as long ago), and (`cinematic_when = "daily"`) not played yet today (`played_today`).
pub fn plays(on: bool, has_frames: bool, fullscreen: bool, seen: Option<f64>, now_unix: f64, min_gap_min: f64, played_today: bool) -> bool {
    on && has_frames && !fullscreen && !played_today && seen.is_none_or(|t| now_unix < t || now_unix - t >= min_gap_min * 60.0)
}

/// The out beat's rows at `u` (0..1) for a close-up `h` px tall: `(y0, y1, 0, 0)` bands that
/// are still drawn, each where it was (no sideways shift: nothing outside its own pixels). Rows
/// go in a fixed random order (`seed`), a few at first, all by the end. `band`: a row's height.
pub fn out_bands(h: i32, u: f32, seed: u64, band: i32) -> Vec<(i32, i32, i32, i32)> {
    let band = band.max(1);
    let mut out: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut y = 0;
    let mut k = 0u64;
    // The last row to go stays until the very end (an empty window stops being drawn).
    let mut last = (-1.0f32, (0, 0));
    while y < h {
        let y1 = (y + band).min(h);
        // Its turn to go, 0..1 (eased: slow at first).
        let mut x = seed ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 29;
        let goes = (x >> 40) as f32 / (1u64 << 24) as f32;
        if goes > last.0 {
            last = (goes, (y, y1));
        }
        if goes >= u * u * (3.0 - 2.0 * u) {
            match out.last_mut() {
                Some(l) if l.1 == y => l.1 = y1,
                _ => out.push((y, y1, 0, 0)),
            }
        }
        y = y1;
        k += 1;
    }
    if out.is_empty() && u < 1.0 && last.0 >= 0.0 {
        out.push((last.1.0, last.1.1, 0, 0));
    }
    out
}

/// Where the close-up goes on a monitor (all root/device px): `image` its size, `window` the
/// window around it (the margin left, right and above), `at` the image's top-left in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub image: (i32, i32),
    pub window: (i32, i32, i32, i32),
    pub at: (i32, i32),
}

/// `HEIGHT_OF_MONITOR` of the monitor's height (narrower if the work area is), centred across
/// the work area `area` (x, y, w, h), standing on its bottom edge. `src`: the close-ups' size.
pub fn place(area: (i32, i32, i32, i32), monitor_h: i32, src: (u32, u32), sf: f32) -> Place {
    let (ax, ay, aw, ah) = area;
    let mut h = (monitor_h as f32 * HEIGHT_OF_MONITOR).round().min(ah as f32);
    let mut w = h * src.0 as f32 / src.1.max(1) as f32;
    if w > aw as f32 {
        h *= aw as f32 / w;
        w = aw as f32;
    }
    let (w, h) = (w.round().max(1.0) as i32, h.round().max(1.0) as i32);
    let m = (MARGIN * sf).round() as i32;
    let x = ax + (aw - w) / 2;
    let y = ay + ah - h;
    Place { image: (w, h), window: (x - m, y - m, w + 2 * m, h + m), at: (m, m) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_beats_in_order() {
        let s = Sheet::new(0);
        let f = |t| FRAMES[s.cue(t).frame];
        assert_eq!((f(0), f(200), f(400), f(540)), ("grab", "grab", "grab", "smear"));
        assert_eq!([610, 750, 890, 1030, 1170].map(f), ["wave1", "wave2", "wave3", "wave2", "wave3"]);
        assert_eq!([1310, 1400, 1530, 1660, 1850].map(f), ["mouth1", "mouth2", "mouth3", "mouth1", "mouth1"]);
        assert_eq!((f(1860), f(2200)), ("happy", "happy"));
        assert_eq!(s.end(), 2520);
        assert_eq!(s.handoff(), 2440, "the bolt starts under its last rows");
        assert!(!s.done(2519) && s.done(2520));
        // In: where it rests from the first frame, no glitch, no silhouette: a tint and
        // scanlines settle off it, then the impact's squash eases back.
        for t in (0..SETTLE_MS).step_by(5) {
            let c = s.cue(t);
            assert_eq!((c.glitch, c.rise, c.scale, c.jolt), (None, 0.0, 1.0, (0.0, 0.0)), "{t}");
            assert!(c.settle.is_some());
        }
        assert!(s.cue(SETTLE_MS).settle.is_none());
        let (tint, scan) = settle(0.0);
        assert!(tint > 0.5 && scan >= 0.5);
        let (tint, scan) = settle(66.0 / SETTLE_MS as f32);
        assert!(tint < 0.12 && scan < 0.2, "her face reads by the third frame: {tint} {scan}");
        assert_eq!(settle(1.0), (0.0, 0.0));
        assert!((s.cue(150).scale - 0.95).abs() < 1e-3 && s.cue(269).scale > 0.999 && s.cue(270).scale == 1.0);
        assert!((0..s.end()).all(|t| s.cue(t).scale <= 1.0), "never past its rest silhouette");
        assert_ne!(s.cue(280).jolt, (0.0, 0.0), "the grab jolts");
        assert!(s.cue(280).jolt.0.abs() <= 2.0 && s.cue(510).jolt == (0.0, 0.0));
        assert!(s.cue(1400).hi && !s.cue(1260).hi && !s.cue(1900).hi);
        // Out: no glitch, no tint; it sinks while its rows drop out.
        let o = s.cue(2390);
        assert_eq!((o.glitch, o.settle, o.hi), (None, None, false));
        assert!(o.out.is_some_and(|u| (u - 0.5).abs() < 0.01) && o.rise > 0.0 && o.rise <= SINK);
        assert!(s.cue(2259).out.is_none());
        // Held frames wake once, at the next change; the jolt moves every frame.
        assert_eq!(s.next_change(510), 531);
        assert_eq!(s.next_change(1900), 2261);
        assert!(s.next_change(280) <= 290);
    }

    #[test]
    fn one_burst_on_the_rolled_wave_frame() {
        for roll in 0..5u64 {
            let s = Sheet::new(roll);
            let bursts: Vec<u64> = (0..s.end()).filter(|&t| s.cue(t).glitch.is_some_and(|g| g.kind == GlitchKind::Burst)).collect();
            assert_eq!(bursts.first(), Some(&(610 + roll * 140)));
            assert_eq!(bursts.len(), 280);
            assert!(bursts.iter().all(|&t| t < OUT_AT), "never over the out beat");
        }
    }

    #[test]
    fn a_skip_goes_straight_to_the_out_beat() {
        let mut s = Sheet::new(2);
        s.skip(810);
        assert!(s.skipped());
        assert_eq!((s.end(), s.handoff()), (1070, 990));
        let c = s.cue(940);
        assert_eq!(FRAMES[c.frame], "wave2", "holds what showed at the click");
        assert_eq!((c.glitch, c.hi), (None, false));
        assert!(c.out.is_some_and(|u| (u - 0.5).abs() < 0.01));
        assert!(s.done(1070));
        assert_eq!(s.cue(570).frame, 1, "before the skip, as planned");
        // A second press changes nothing; one in the out beat isn't a skip.
        s.skip(1000);
        assert_eq!(s.end(), 1070);
        let mut late = Sheet::new(0);
        late.skip(2500);
        assert!(!late.skipped() && late.end() == END);
    }

    #[test]
    fn the_out_beat_drops_rows_where_they_are() {
        let h = 893;
        let all = out_bands(h, 0.0, 7, 12);
        assert_eq!(all, vec![(0, h, 0, 0)], "nothing gone yet");
        assert!(out_bands(h, 1.0, 7, 12).is_empty(), "all gone at the end");
        assert_eq!(out_bands(h, 0.999, 7, 12).len(), 1, "one row left until then");
        let mut prev = h;
        for k in 1..10 {
            let b = out_bands(h, k as f32 / 10.0, 7, 12);
            let rows: i32 = b.iter().map(|r| r.1 - r.0).sum();
            assert!(rows <= prev, "rows only go");
            prev = rows;
            assert!(b.iter().all(|&(y0, y1, dx, dy)| 0 <= y0 && y0 < y1 && y1 <= h && dx == 0 && dy == 0), "inside, unshifted");
        }
        let half: i32 = out_bands(h, 0.5, 7, 12).iter().map(|r| r.1 - r.0).sum();
        assert!((h / 4..h * 3 / 4).contains(&half), "{half}");
    }

    #[test]
    fn it_plays_unless_told_or_too_soon() {
        let now = 1_000_000.0;
        assert!(plays(true, true, false, None, now, 10.0, false), "first launch ever");
        assert!(plays(true, true, false, Some(now - 601.0), now, 10.0, false));
        assert!(!plays(true, true, false, Some(now - 120.0), now, 10.0, false), "a restart two minutes later");
        assert!(plays(true, true, false, Some(now + 50.0), now, 10.0, false), "the clock went back");
        assert!(plays(true, true, false, Some(now - 1.0), now, 0.0, false), "gap 0: always");
        assert!(!plays(false, true, false, None, now, 10.0, false), "setting off");
        assert!(!plays(true, false, false, None, now, 10.0, false), "a pack without the close-ups");
        assert!(!plays(true, true, true, None, now, 10.0, false), "fullscreen focused");
        assert!(!plays(true, true, false, Some(now - 7200.0), now, 10.0, true), "daily, and it played today");
    }

    #[test]
    fn it_stands_on_the_work_area_bottom() {
        // bella: 2560x1440 at sf 1.1667, no bottom bar.
        let p = place((2560, 0, 2560, 1440), 1440, (1672, 941), 1.1667);
        assert_eq!(p.image, (1587, 893));
        let m = 56;
        assert_eq!(p.window, (2560 + (2560 - 1587) / 2 - m, 1440 - 893 - m, 1587 + 2 * m, 893 + m));
        assert_eq!(p.at, (m, m));
        assert_eq!(p.window.1 + p.at.1 + p.image.1, 1440, "its bottom on the area's");
        // A narrow monitor: as wide as the area, still standing on its bottom.
        let n = place((0, 30, 1080, 1890), 1920, (1672, 941), 1.0);
        assert_eq!(n.image.0, 1080);
        assert_eq!(n.window.1 + n.at.1 + n.image.1, 1920);
    }
}
