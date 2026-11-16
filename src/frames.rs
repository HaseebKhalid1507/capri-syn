//! Frame decode → anchor registration → crop to the shared canvas rect → scale to display
//! size → BGRA render image + hit mask. Only the scaled pixels are kept.

use crate::pack::{FrameSource, Pack, Rect};
use anyhow::{Context as _, Result, bail};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use gpui_kit::RenderImage;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Alpha at or above this counts as "body" for click-through and hit tests.
pub const HIT_ALPHA: u8 = 96;

pub struct Mask {
    pub w: u32,
    pub h: u32,
    bits: Vec<u64>,
}

impl Mask {
    fn new(w: u32, h: u32) -> Self {
        Mask { w, h, bits: vec![0; (w as usize * h as usize).div_ceil(64)] }
    }
    fn set(&mut self, x: u32, y: u32) {
        let i = y as usize * self.w as usize + x as usize;
        self.bits[i / 64] |= 1 << (i % 64);
    }
    pub fn get(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as u32 >= self.w || y as u32 >= self.h {
            return false;
        }
        let i = y as usize * self.w as usize + x as usize;
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }
    /// Run-length rows merged into bands of identical rows: the X input/bounding shape.
    pub fn rects(&self) -> Vec<(i16, i16, u16, u16)> {
        let mut out: Vec<(i16, i16, u16, u16)> = Vec::new();
        let mut prev: Vec<(u32, u32)> = Vec::new();
        let mut open: Vec<usize> = Vec::new(); // indices into `out` for prev row's runs
        for y in 0..self.h {
            let mut runs = Vec::new();
            let mut x = 0;
            while x < self.w {
                if self.get(x as i32, y as i32) {
                    let s = x;
                    while x < self.w && self.get(x as i32, y as i32) {
                        x += 1;
                    }
                    runs.push((s, x - s));
                } else {
                    x += 1;
                }
            }
            if runs == prev && !runs.is_empty() {
                for &i in &open {
                    out[i].3 += 1;
                }
            } else {
                open.clear();
                for &(s, w) in &runs {
                    open.push(out.len());
                    out.push((s as i16, y as i16, w as u16, 1));
                }
                prev = runs;
            }
        }
        out
    }
}

pub struct Frame {
    pub id: String,
    /// The pixels, `at` (device px) inside the avatar rect: the whole rect for the pack's
    /// frames, the opaque box for a strip's cells (they would be mostly empty).
    pub image: Arc<RenderImage>,
    pub at: (i32, i32),
    pub mask: Mask,
    pub rects: Arc<Vec<(i16, i16, u16, u16)>>,
    /// Device-pixel y (inside the avatar rect) above which a click is a head click.
    pub head_line: i32,
    /// Device-px top of the frame's visible bounds inside the avatar rect.
    pub top: i32,
    /// Device-px lowest opaque row + 1 (the ragdoll's grip zones run top..bottom).
    pub bottom: i32,
    /// Device-px x of the head: mask centroid over the top 30 % of the visible bounds.
    pub head_x: i32,
    /// Points well inside the upper part of the figure (hair), for glints.
    pub glints: Vec<(i16, i16)>,
    /// Topmost opaque row per column (i16::MAX where the column is empty).
    pub profile: Arc<Vec<i16>>,
    /// Device-px rows of the face (brows to mouth): glitches keep it readable.
    pub face: (i32, i32),
    /// This frame mirrored left-right, built on first use (`mirrored`).
    mirror: std::sync::OnceLock<Box<Frame>>,
}

impl Frame {
    /// This frame mirrored about the avatar rect's centre, which is the canonical anchor
    /// (the crop is symmetric about it): her feet stay put. Image, hit mask, shape rects,
    /// head, glints and profile all mirror together; built once, then cached, so a turn
    /// costs no more than any frame change.
    pub fn mirrored(&self) -> &Frame {
        self.mirror.get_or_init(|| Box::new(self.build_mirror()))
    }

    /// The mirror, if it has been built (its pixels have to be released with the frame's).
    pub fn mirror_built(&self) -> Option<&Frame> {
        self.mirror.get().map(|b| &**b)
    }

    /// The image's size, device px.
    pub fn image_size(&self) -> (i32, i32) {
        let s = self.image.size(0);
        (i32::from(s.width), i32::from(s.height))
    }

    /// Pixel bytes held (the image and a built mirror).
    pub fn bytes(&self) -> usize {
        let (w, h) = self.image_size();
        let own = (w * h * 4) as usize;
        own + self.mirror.get().map_or(0, |_| own)
    }

    fn build_mirror(&self) -> Frame {
        let (w, h) = (self.mask.w, self.mask.h);
        let mut mask = Mask::new(w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if self.mask.get(x, y) {
                    mask.set(w - 1 - x as u32, y as u32);
                }
            }
        }
        let src = self.image.as_bytes(0).unwrap_or(&[]);
        let (iw, ih) = self.image_size();
        let mut px = vec![0u8; src.len()];
        let row = iw.max(1) as usize * 4;
        for (d, s) in px.chunks_exact_mut(row).zip(src.chunks_exact(row)) {
            for (dp, sp) in d.as_chunks_mut::<4>().0.iter_mut().zip(s.as_chunks::<4>().0.iter().rev()) {
                *dp = *sp;
            }
        }
        let rgba = image::RgbaImage::from_raw(iw as u32, ih as u32, px).unwrap_or_else(|| image::RgbaImage::new(iw as u32, ih as u32));
        let mx = |x: i32| w as i32 - 1 - x;
        Frame {
            id: self.id.clone(),
            image: Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])),
            at: (w as i32 - (self.at.0 + iw), self.at.1),
            rects: Arc::new(mask.rects()),
            mask,
            head_line: self.head_line,
            top: self.top,
            bottom: self.bottom,
            head_x: mx(self.head_x),
            glints: self.glints.iter().map(|&(x, y)| (mx(x as i32) as i16, y)).collect(),
            profile: Arc::new(self.profile.iter().rev().copied().collect()),
            face: self.face,
            mirror: std::sync::OnceLock::new(),
        }
    }
}

/// Topmost opaque row per column.
fn top_profile(mask: &Mask) -> Vec<i16> {
    (0..mask.w as i32)
        .map(|x| (0..mask.h as i32).find(|&y| mask.get(x, y)).map_or(i16::MAX, |y| y as i16))
        .collect()
}

/// Rows of the face. Capri-style eyes are saturated blue-teal disks; their rows give the
/// band (brows a little above, the mouth below). Without them: a fixed slice under the top.
fn face_band(px: &[u8], w: u32, top: i32, height: i32) -> (i32, i32) {
    let mut rows = Vec::new();
    let lim = (top + (height as f32 * 0.65) as i32).min((px.len() / 4 / w as usize) as i32);
    for y in top.max(0)..lim {
        for x in 0..w as i32 {
            let i = (y as usize * w as usize + x as usize) * 4;
            let (r, g, b, a) = (px[i], px[i + 1], px[i + 2], px[i + 3]);
            if a > 200 && b > 190 && r < 100 && g > 50 && g < 200 {
                rows.push(y);
            }
        }
    }
    if rows.len() >= 6 {
        rows.sort_unstable();
        let (y0, y1) = (rows[rows.len() / 10], rows[rows.len() * 9 / 10]);
        let eye = (y1 - y0).max(6);
        return (y0 - eye, y1 + eye * 2);
    }
    (top + (height as f32 * 0.17) as i32, top + (height as f32 * 0.37) as i32)
}

/// A clean 1 px rim of `rgb` along the outline of the dark parts of the figure, from row
/// `from` down. Rim pixels are the transparent pixels 4-adjacent to the 1-bit shape that
/// have a dark figure pixel within 2 px, so the line is continuous along dark stretches
/// (no speckle from anti-aliased edges) and stays off coloured parts. Drawn into
/// transparent pixels only: behind the art, never over it. RGBA straight alpha.
fn add_rim(px: &mut [u8], w: u32, h: u32, from: i32, rgb: u32) {
    add_rim_rows(px, w, h, from, h as i32, rgb);
}

/// `add_rim` over rows `from..to` only.
fn add_rim_rows(px: &mut [u8], w: u32, h: u32, from: i32, to: i32, rgb: u32) {
    let (w, h) = (w as i32, h as i32);
    let at = |x: i32, y: i32| (y * w + x) as usize * 4;
    let inside = |px: &[u8], x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && px[at(x, y) + 3] >= HIT_ALPHA;
    let dark = |px: &[u8], x: i32, y: i32| {
        if !inside(px, x, y) {
            return false;
        }
        let i = at(x, y);
        (px[i] as u32 * 3 + px[i + 1] as u32 * 6 + px[i + 2] as u32) < 10 * 80
    };
    let mut set = Vec::new();
    for y in from.max(0)..to.min(h) {
        for x in 0..w {
            if inside(px, x, y) {
                continue;
            }
            if !(inside(px, x - 1, y) || inside(px, x + 1, y) || inside(px, x, y - 1) || inside(px, x, y + 1)) {
                continue;
            }
            if (-2..=2).any(|dy| (-2..=2).any(|dx| dark(px, x + dx, y + dy))) {
                set.push(at(x, y));
            }
        }
    }
    for i in set {
        px[i] = (rgb >> 16) as u8;
        px[i + 1] = (rgb >> 8) as u8;
        px[i + 2] = rgb as u8;
        px[i + 3] = 255;
    }
}

/// Head x and glint candidates from the hit mask (pack-agnostic: no colour rules).
fn head_and_glints(mask: &Mask, top: i32, height: i32) -> (i32, Vec<(i16, i16)>) {
    let band = (height as f32 * 0.3) as i32;
    let (mut sum, mut n) = (0i64, 0i64);
    for y in top.max(0)..(top + band).min(mask.h as i32) {
        for x in 0..mask.w as i32 {
            if mask.get(x, y) {
                sum += x as i64;
                n += 1;
            }
        }
    }
    let head_x = if n > 0 { (sum / n) as i32 } else { mask.w as i32 / 2 };
    // Candidates: pixels in the top 40 % whose 9x9 neighbourhood is all body, on a grid.
    let r = 4;
    let mut cands = Vec::new();
    let y1 = (top + (height as f32 * 0.4) as i32).min(mask.h as i32 - r);
    for y in ((top + r).max(r)..y1).step_by(3) {
        for x in (r..mask.w as i32 - r).step_by(3) {
            if [(-r, -r), (r, -r), (-r, r), (r, r), (0, 0), (0, -r), (0, r), (-r, 0), (r, 0)].iter().all(|&(dx, dy)| mask.get(x + dx, y + dy)) {
                cands.push((x as i16, y as i16));
            }
        }
    }
    // A fixed spread of 48 of them (deterministic LCG).
    let mut seed = 0x2545_f491u32 ^ (cands.len() as u32);
    let mut glints = Vec::new();
    for _ in 0..cands.len().min(48) {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        glints.push(cands[(seed >> 8) as usize % cands.len()]);
    }
    (head_x, glints)
}

/// Her face mask inside `rect` of an RGBA canvas: the bright green of her face, its gaps
/// (eyes, brows) of up to `FACE_GAP` px closed along rows then columns, grown by
/// `FACE_GROW` px. Row-major over `rect`.
fn face_mask(px: &[u8], w: u32, rect: Rect) -> Vec<bool> {
    const FACE_GAP: usize = 24;
    let (rw, rh) = (rect.width.max(0) as usize, rect.height.max(0) as usize);
    let mut m = vec![false; rw * rh];
    for y in 0..rh {
        for x in 0..rw {
            let (cx, cy) = (rect.x + x as i32, rect.y + y as i32);
            if cx < 0 || cy < 0 || cx >= w as i32 {
                continue;
            }
            let i = (cy as usize * w as usize + cx as usize) * 4;
            let Some(p) = px.get(i..i + 4) else { continue };
            m[y * rw + x] = p[1] > 170 && p[0] < 140 && p[2] < 170 && p[3] > 200;
        }
    }
    // Close the gaps between face pixels: rows, then columns (each pass on the last's result).
    let close = |m: &mut [bool], n: usize, len: usize, at: &dyn Fn(usize, usize) -> usize| {
        for line in 0..n {
            let mut last: Option<usize> = None;
            for k in 0..len {
                if m[at(line, k)] {
                    if let Some(p) = last
                        && k - p > 1
                        && k - p <= FACE_GAP
                    {
                        for j in p + 1..k {
                            m[at(line, j)] = true;
                        }
                    }
                    last = Some(k);
                }
            }
        }
    };
    close(&mut m, rh, rw, &|y, x| y * rw + x);
    close(&mut m, rw, rh, &|x, y| y * rw + x);
    let mut out = m.clone();
    for y in 0..rh {
        for x in 0..rw {
            if !m[y * rw + x] {
                continue;
            }
            for yy in y.saturating_sub(FACE_GROW)..(y + FACE_GROW + 1).min(rh) {
                for xx in x.saturating_sub(FACE_GROW)..(x + FACE_GROW + 1).min(rw) {
                    out[yy * rw + xx] = true;
                }
            }
        }
    }
    out
}

const FACE_GROW: usize = 3;

/// Paint the face of `src` (inside `rect`) onto `dst`, moved by `d`: e.g. the headpat's
/// closed happy eyes onto the perch frame. Both are canvases `w`×`h`.
pub fn face_patch(dst: &mut [u8], src: &[u8], w: u32, h: u32, rect: Rect, d: (i32, i32)) {
    let m = face_mask(src, w, rect);
    let rw = rect.width.max(0) as usize;
    for (k, _) in m.iter().enumerate().filter(|x| *x.1) {
        let (sx, sy) = (rect.x + (k % rw) as i32, rect.y + (k / rw) as i32);
        let (tx, ty) = (sx + d.0, sy + d.1);
        if sx < 0 || sy < 0 || tx < 0 || ty < 0 || sx >= w as i32 || sy >= h as i32 || tx >= w as i32 || ty >= h as i32 {
            continue;
        }
        let (si, ti) = ((sy as usize * w as usize + sx as usize) * 4, (ty as usize * w as usize + tx as usize) * 4);
        dst[ti..ti + 4].copy_from_slice(&src[si..si + 4]);
    }
}

/// Where an image sits (`at`, its size) when the whole `aw`×`ah` avatar rect is drawn at `r`
/// (x, y, w, h; any scale): the pack's frames fill it, a strip's cell is its opaque box.
pub fn place_in(at: ((i32, i32), (i32, i32)), r: (f32, f32, f32, f32), aw: i32, ah: i32) -> (f32, f32, f32, f32) {
    let ((x, y), (w, h)) = at;
    if (x, y, w, h) == (0, 0, aw, ah) {
        return r;
    }
    let (kx, ky) = (r.2 / aw.max(1) as f32, r.3 / ah.max(1) as f32);
    (r.0 + x as f32 * kx, r.1 + y as f32 * ky, w as f32 * kx, h as f32 * ky)
}

/// Display geometry shared by all frames.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// Canvas rect every frame is cropped to.
    pub crop: Rect,
    /// Canvas px → device px.
    pub scale: f32,
    pub width: u32,
    pub height: u32,
}

impl Geometry {
    /// `body_px`: device-pixel height of the default sprite's visible bounds.
    pub fn new(pack: &Pack, body_px: f32) -> Geometry {
        let crop = pack.crop_rect();
        let reference = pack.frame(&pack.catalog.default_sprite).map(|i| pack.frames[i].bounds.height).unwrap_or(1000);
        let scale = body_px / reference as f32;
        Geometry {
            crop,
            scale,
            width: (crop.width as f32 * scale).round() as u32,
            height: (crop.height as f32 * scale).round() as u32,
        }
    }

    /// Device px of a canonical-canvas point inside the avatar rect.
    pub fn to_device(self, x: i32, y: i32) -> (f32, f32) {
        ((x - self.crop.x) as f32 * self.scale, (y - self.crop.y) as f32 * self.scale)
    }
}

pub struct Decoded {
    pub frames: Vec<Frame>,
    pub decode_ms: u128,
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Strips decoded once per `decode` call, shared by their cells (and its threads).
type Strips = std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Arc<std::sync::OnceLock<Result<Arc<image::RgbaImage>, String>>>>>;

fn strip_image(strips: &Strips, path: &std::path::Path, sha: Option<&str>, verify: bool) -> Result<Arc<image::RgbaImage>> {
    let slot = strips.lock().unwrap().entry(path.to_owned()).or_default().clone();
    slot.get_or_init(|| {
        let load = || -> Result<Arc<image::RgbaImage>> {
            let bytes = std::fs::read(path).with_context(|| format!("strip {}: cannot read", path.display()))?;
            if verify && let Some(want) = sha {
                let got = sha_hex(&bytes);
                if !got.eq_ignore_ascii_case(want) {
                    bail!("strip {}: sha256 mismatch (anim.json {want}, file {got})", path.display());
                }
            }
            let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .with_context(|| format!("strip {}: not a valid PNG", path.display()))?;
            Ok(Arc::new(img.into_rgba8()))
        };
        load().map_err(|e| format!("{e:#}"))
    })
    .clone()
    .map_err(|e| anyhow::anyhow!(e))
}

/// A strip's cell, scaled to device px and laid into an avatar-rect buffer at its registration:
/// cell px (0, 0) is canvas (dx, dy) of `pack.offset` (the cell's anchor on the canonical point
/// or the seat line). The x2 strip (`make-pack --upscale`) serves when the cell would be enlarged.
fn cell_pixels(pack: &Pack, f: &FrameSource, c: &crate::art::CellSrc, geo: &Geometry, verify: bool, resizer: &mut Resizer, strips: &Strips) -> Result<Vec<u8>> {
    let a = pack.anim.as_ref().context("a strip cell without anim/")?;
    let st = &a.strips[c.strip];
    let dev = geo.scale * st.k;
    let (path, n, sha) = match &st.x2 {
        Some(p) if dev > 1.0 => (p, 2u32, None),
        _ => (&st.path, 1, st.sha256.as_deref()),
    };
    let img = strip_image(strips, path, sha, verify)?;
    let (cw, ch) = (st.cell.0 * n, st.cell.1 * n);
    if img.width() != cw * st.frames || img.height() != ch {
        bail!("strip `{}`: {} is {}x{}, anim.json says {} cells of {cw}x{ch}", st.name, path.display(), img.width(), img.height(), st.frames);
    }
    let mut cell = vec![0u8; (cw * ch * 4) as usize];
    let raw = img.as_raw();
    let row = cw as usize * 4;
    for y in 0..ch as usize {
        let s0 = (y * img.width() as usize + (c.i * cw) as usize) * 4;
        cell[y * row..(y + 1) * row].copy_from_slice(&raw[s0..s0 + row]);
    }
    if let Some(k) = st.contact {
        cut_below(&mut cell, cw, k.line * n, [k.legs[0] * n, k.legs[1] * n]);
    }
    let k = dev / n as f32;
    let (rw, rh) = (((cw as f32 * k).round() as u32).max(1), ((ch as f32 * k).round() as u32).max(1));
    let src = Image::from_vec_u8(cw, ch, cell, PixelType::U8x4)?;
    let mut dst = Image::new(rw, rh, PixelType::U8x4);
    resizer.resize(&src, &mut dst, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)))?;
    let small = dst.into_vec();
    let (dx, dy) = pack.offset(f);
    let (tx, ty) = geo.to_device(dx, dy);
    let (tx, ty) = (tx.round() as i32, ty.round() as i32);
    let (w, h) = (geo.width as i32, geo.height as i32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..rh as i32 {
        let yy = ty + y;
        if yy < 0 || yy >= h {
            continue;
        }
        let (x0, x1) = ((-tx).max(0), (w - tx).min(rw as i32));
        if x0 >= x1 {
            continue;
        }
        let s0 = ((y * rw as i32 + x0) * 4) as usize;
        let d0 = ((yy * w + tx + x0) * 4) as usize;
        let len = ((x1 - x0) * 4) as usize;
        px[d0..d0 + len].copy_from_slice(&small[s0..s0 + len]);
    }
    Ok(px)
}

/// On a window top, the window's front face hides what is below the contact line, except the
/// columns `legs` (the lower legs hanging over its edge): cleared, straight RGBA.
fn cut_below(cell: &mut [u8], w: u32, line: u32, legs: [u32; 2]) {
    for (i, p) in cell.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if y > line && !(legs[0]..legs[1]).contains(&x) {
            *p = [0; 4];
        }
    }
}

fn decode_one(pack: &Pack, f: &FrameSource, geo: &Geometry, verify: bool, resizer: &mut Resizer, strips: &Strips) -> Result<Frame> {
    let px = match &f.cell {
        Some(c) => cell_pixels(pack, f, c, geo, verify, resizer, strips)?,
        None => canvas_pixels(pack, f, geo, verify, resizer)?,
    };
    let frame = finish(pack, f, geo, px)?;
    Ok(if f.cell.as_ref().is_some_and(|c| c.mirror) { frame.build_mirror() } else { frame })
}

/// A catalog sprite or sidecar extra: registered into the shared crop rect, scaled to device px.
fn canvas_pixels(pack: &Pack, f: &FrameSource, geo: &Geometry, verify: bool, resizer: &mut Resizer) -> Result<Vec<u8>> {
    let bytes = std::fs::read(&f.path).with_context(|| format!("frame `{}`: cannot read {}", f.id, f.path.display()))?;
    if verify
        && let Some(want) = &f.sha256
    {
        let got = sha_hex(&bytes);
        if !got.eq_ignore_ascii_case(want) {
            bail!("frame `{}`: sha256 mismatch for {} (catalog {want}, file {got})", f.id, f.path.display());
        }
    }
    let mut img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .with_context(|| format!("frame `{}`: {} is not a valid PNG", f.id, f.path.display()))?
        .into_rgba8();
    if let Some((path, rect, pdx, pdy, sha)) = &f.patch {
        let pb = std::fs::read(path).with_context(|| format!("frame `{}`: cannot read its patch {}", f.id, path.display()))?;
        if verify
            && let Some(want) = sha
            && !sha_hex(&pb).eq_ignore_ascii_case(want)
        {
            bail!("frame `{}`: sha256 mismatch for its patch {}", f.id, path.display());
        }
        let src = image::load_from_memory_with_format(&pb, image::ImageFormat::Png)
            .with_context(|| format!("frame `{}`: patch {} is not a valid PNG", f.id, path.display()))?
            .into_rgba8();
        if src.dimensions() != img.dimensions() {
            bail!("frame `{}`: patch {} is not the frame's size", f.id, path.display());
        }
        let (w, h) = img.dimensions();
        face_patch(img.as_mut(), src.as_raw(), w, h, *rect, (*pdx, *pdy));
    }
    let (iw, ih) = img.dimensions();
    let canvas = &pack.catalog.canvas;
    if iw != canvas.width || ih != canvas.height {
        bail!("frame `{}`: {} is {iw}x{ih}, catalog canvas is {}x{}", f.id, f.path.display(), canvas.width, canvas.height);
    }

    // Copy the registered frame into the shared crop rect (transparent padding).
    let (dx, dy) = pack.offset(f);
    let crop = geo.crop;
    let (cw, ch) = (crop.width as usize, crop.height as usize);
    let mut buf = vec![0u8; cw * ch * 4];
    let src = img.as_raw();
    for row in 0..ch {
        let sy = crop.y + row as i32 - dy;
        if sy < 0 || sy >= ih as i32 {
            continue;
        }
        let sx0 = crop.x - dx;
        let x_from = (-sx0).max(0) as usize;
        let x_to = (iw as i32 - sx0).min(cw as i32).max(0) as usize;
        if x_from >= x_to {
            continue;
        }
        let s = (sy as usize * iw as usize + (sx0 + x_from as i32) as usize) * 4;
        let d = (row * cw + x_from) * 4;
        let n = (x_to - x_from) * 4;
        buf[d..d + n].copy_from_slice(&src[s..s + n]);
    }
    drop(img);

    let src = Image::from_vec_u8(cw as u32, ch as u32, buf, PixelType::U8x4)?;
    let mut dst = Image::new(geo.width, geo.height, PixelType::U8x4);
    // Lanczos3 on premultiplied alpha (fast_image_resize premultiplies for U8x4).
    resizer.resize(&src, &mut dst, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)))?;
    Ok(dst.into_vec())
}

/// Rims, hit mask, head, glints, profile, face band of an avatar-rect RGBA buffer.
fn finish(pack: &Pack, f: &FrameSource, geo: &Geometry, mut px: Vec<u8>) -> Result<Frame> {
    let (dx, dy) = pack.offset(f);
    if pack.perch_frames.contains(&f.id)
        && let Some(rim) = pack.behaviour.perch.as_ref().and_then(|p| u32::from_str_radix(p.rim.trim_start_matches('#'), 16).ok())
    {
        // The limbs over the window: from just above the seat line down.
        let (_, seat) = geo.to_device(0, f.anchor.y + dy);
        add_rim(&mut px, geo.width, geo.height, seat.round() as i32 - 8, rim);
    }
    let vb = f.bounds.offset(dx, dy);
    let (_, top) = geo.to_device(0, vb.y);
    if pack.behaviour.brain.poses.get("hang") == Some(&f.id)
        && let Some(rim) = pack.behaviour.perch.as_ref().and_then(|p| u32::from_str_radix(p.rim.trim_start_matches('#'), 16).ok())
    {
        // The hands over the bar (dark claws on a dark bar): a 2 px light rim from the top
        // to a little under the grip, so they read as gripping the edge.
        let (_, grip) = geo.to_device(0, f.anchor.y + dy);
        let to = (grip + 16.0 * geo.scale).round() as i32;
        for _ in 0..2 {
            add_rim_rows(&mut px, geo.width, geo.height, top.round() as i32 - 4, to, rim);
        }
    }
    let face = face_band(&px, geo.width, top.round() as i32, (vb.height as f32 * geo.scale) as i32);

    let mut mask = Mask::new(geo.width, geo.height);
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if p[3] >= HIT_ALPHA {
            mask.set(i as u32 % geo.width, i as u32 / geo.width);
        }
        p.swap(0, 2); // RGBA → BGRA (GPUI's sprite atlas format), straight alpha.
    }
    let rects = Arc::new(mask.rects());
    let head_fraction = pack.behaviour.click.head_fraction;
    let head_line = (top + vb.height as f32 * geo.scale * head_fraction).round() as i32;
    // A strip's cell keeps only its opaque box (most of the rect is empty).
    let (at, rgba) = if f.cell.is_some() { crop_opaque(px, geo.width, geo.height) } else { ((0, 0), image::RgbaImage::from_raw(geo.width, geo.height, px).expect("buffer size")) };
    let (head_x, glints) = head_and_glints(&mask, top.round() as i32, (vb.height as f32 * geo.scale) as i32);
    let profile = Arc::new(top_profile(&mask));
    let bottom = (0..mask.h as i32).rev().find(|&y| (0..mask.w as i32).any(|x| mask.get(x, y))).map_or(top.round() as i32 + 1, |y| y + 1);
    Ok(Frame {
        bottom,
        profile,
        face,
        id: f.id.clone(),
        image: Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])),
        at,
        mask,
        rects,
        head_line,
        top: top.round() as i32,
        head_x,
        glints,
        mirror: std::sync::OnceLock::new(),
    })
}

/// The opaque box of a `w`×`h` buffer: its top-left and pixels (1×1 transparent if empty).
fn crop_opaque(px: Vec<u8>, w: u32, h: u32) -> ((i32, i32), image::RgbaImage) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
        if p[3] > 0 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    if x0 >= x1 || y0 >= y1 {
        return ((0, 0), image::RgbaImage::new(1, 1));
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..y1 {
        let s0 = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&px[s0..s0 + (cw * 4) as usize]);
    }
    ((x0 as i32, y0 as i32), image::RgbaImage::from_raw(cw, ch, out).expect("buffer size"))
}

/// Decode the frames at `which` (indices into `pack.frames`) on up to `threads` threads.
pub fn decode(pack: &Pack, geo: &Geometry, which: &[usize], verify: bool, threads: usize) -> Result<Decoded> {
    let t0 = std::time::Instant::now();
    let next = AtomicUsize::new(0);
    let slots: std::sync::Mutex<Vec<Option<Result<Frame>>>> =
        std::sync::Mutex::new((0..which.len()).map(|_| None).collect());
    let strips = Strips::default();
    std::thread::scope(|s| {
        for _ in 0..threads.max(1).min(which.len().max(1)) {
            s.spawn(|| {
                let mut resizer = Resizer::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= which.len() {
                        break;
                    }
                    let r = decode_one(pack, &pack.frames[which[i]], geo, verify, &mut resizer, &strips);
                    slots.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    let mut frames = Vec::with_capacity(which.len());
    for r in slots.into_inner().unwrap() {
        frames.push(r.expect("every slot decoded")?);
    }
    Ok(Decoded { frames, decode_ms: t0.elapsed().as_millis() })
}

/// One pass over the docking poses' hashes (they are not decoded in phase 1).
pub fn verify_poses(pack: &Pack) -> Result<()> {
    for p in &pack.poses {
        if let Some(want) = &p.sha256 {
            let bytes = std::fs::read(&p.path).with_context(|| format!("pose `{}`: cannot read", p.id))?;
            let got = sha_hex(&bytes);
            if !got.eq_ignore_ascii_case(want) {
                bail!("pose `{}`: sha256 mismatch for {}", p.id, p.path.display());
            }
        }
    }
    Ok(())
}

/// A music note as two 1-bit masks (dark outline, colour fill) per rotation: shown as
/// two shaped solid-colour X windows, like the hearts, so moving notes never expose stale
/// pixels of the avatar window.
pub struct NoteMasks {
    pub size: u32,
    pub fill_rgb: u32,
    pub outline_rgb: u32,
    /// Per `NOTE_ANGLES` entry: (outline, fill).
    pub variants: Vec<(Mask, Mask)>,
}

pub const NOTE_ANGLES: [f32; 7] = [-18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0];

/// Rotate a square RGBA image about its centre (bilinear, premultiplied while sampling).
fn rotate_rgba(src: &[u8], n: u32, deg: f32) -> Vec<u8> {
    let (s, c) = deg.to_radians().sin_cos();
    let n_i = n as i32;
    let mid = (n as f32 - 1.0) / 2.0;
    let mut out = vec![0u8; src.len()];
    let at = |x: i32, y: i32| -> [f32; 4] {
        if x < 0 || y < 0 || x >= n_i || y >= n_i {
            return [0.0; 4];
        }
        let i = (y as usize * n as usize + x as usize) * 4;
        let a = src[i + 3] as f32 / 255.0;
        [src[i] as f32 * a, src[i + 1] as f32 * a, src[i + 2] as f32 * a, src[i + 3] as f32]
    };
    for y in 0..n_i {
        for x in 0..n_i {
            let (dx, dy) = (x as f32 - mid, y as f32 - mid);
            let sx = c * dx + s * dy + mid;
            let sy = -s * dx + c * dy + mid;
            let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let (p00, p10, p01, p11) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            let mut v = [0.0f32; 4];
            for k in 0..4 {
                v[k] = (p00[k] * (1.0 - fx) + p10[k] * fx) * (1.0 - fy) + (p01[k] * (1.0 - fx) + p11[k] * fx) * fy;
            }
            let i = (y as usize * n as usize + x as usize) * 4;
            if v[3] > 0.5 {
                let a = v[3] / 255.0;
                out[i] = (v[0] / a).round().min(255.0) as u8;
                out[i + 1] = (v[1] / a).round().min(255.0) as u8;
                out[i + 2] = (v[2] / a).round().min(255.0) as u8;
                out[i + 3] = v[3].round().min(255.0) as u8;
            }
        }
    }
    out
}

/// Decode note images to `size` device px squares, rotated at every `NOTE_ANGLES` step.
pub fn decode_notes(pack: &Pack, files: &[String], size: u32) -> Result<Vec<NoteMasks>> {
    let mut resizer = Resizer::new();
    let mut out = Vec::new();
    let is_dark = |p: &[u8]| (p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) < 10 * 60;
    for rel in files {
        let path = pack.dir.join(rel);
        let bytes = std::fs::read(&path).with_context(|| format!("note {}: cannot read", path.display()))?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .with_context(|| format!("note {}: not a PNG", path.display()))?
            .into_rgba8();
        let (w, h) = img.dimensions();
        if w != h {
            bail!("note {}: {w}x{h} is not square", path.display());
        }
        // Fill colour: the mean of the bright opaque pixels.
        let (mut sum, mut n) = ([0u64; 3], 0u64);
        for p in img.as_raw().as_chunks::<4>().0.iter().filter(|p| p[3] > 200 && !is_dark(*p)) {
            for k in 0..3 {
                sum[k] += p[k] as u64;
            }
            n += 1;
        }
        let n = n.max(1);
        let fill_rgb = ((sum[0] / n) << 16 | (sum[1] / n) << 8 | (sum[2] / n)) as u32;
        let mut variants = Vec::new();
        for &deg in &NOTE_ANGLES {
            let rot = if deg == 0.0 { img.as_raw().clone() } else { rotate_rgba(img.as_raw(), w, deg) };
            let src = Image::from_vec_u8(w, w, rot, PixelType::U8x4)?;
            let mut dst = Image::new(size, size, PixelType::U8x4);
            resizer.resize(&src, &mut dst, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)))?;
            let px = dst.into_vec();
            let (mut outline, mut fill) = (Mask::new(size, size), Mask::new(size, size));
            for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (i as u32 % size, i as u32 / size);
                if p[3] >= 110 {
                    outline.set(x, y);
                    if p[3] >= 200 && !is_dark(p) {
                        fill.set(x, y);
                    }
                }
            }
            variants.push((outline, fill));
        }
        out.push(NoteMasks { size, fill_rgb, outline_rgb: 0x0b0420, variants });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_cell_decodes_to_its_opaque_box_and_its_twin_mirrored() {
        crate::pack::tests::pack_in("cells", crate::pack::tests::pooled(), true, |p| {
            let p = p.unwrap();
            let geo = Geometry::new(&p, 60.0);
            let a = p.anim.as_ref().unwrap();
            let s1 = p.strip_first[a.by_name["stand"]].unwrap() + 1;
            let t1 = p.twin(s1).unwrap();
            let n = p.index["neutral"];
            let d = decode(&p, &geo, &[n, s1, t1], false, 2).unwrap();
            // Verified against anim.json's sha256 (the fixture's is wrong on purpose).
            let e = format!("{:#}", decode(&p, &geo, &[s1], true, 1).err().unwrap());
            assert!(e.contains("sha256 mismatch"), "{e}");
            let (full, cell, twin) = (&d.frames[0], &d.frames[1], &d.frames[2]);
            assert_eq!((full.at, full.image_size()), ((0, 0), (geo.width as i32, geo.height as i32)), "the pack's frames fill the rect");
            let (w, h) = cell.image_size();
            assert!(w < geo.width as i32 && h < geo.height as i32, "{w}x{h} of {}x{}", geo.width, geo.height);
            // The opaque box is the mask's box.
            let cols: Vec<i32> = (0..cell.mask.w as i32).filter(|&x| (0..cell.mask.h as i32).any(|y| cell.mask.get(x, y))).collect();
            assert!(cell.at.0 <= cols[0] && cols[cols.len() - 1] < cell.at.0 + w);
            // Feet on the ground point: the lowest opaque row sits just above the canonical y.
            let (_, gy) = geo.to_device(p.catalog.canonical_anchor.x, p.catalog.canonical_anchor.y);
            assert!((cell.bottom as f32 - gy).abs() <= 2.0 * geo.scale, "bottom {} ground {gy}", cell.bottom);
            // The twin strip is pixel-identical here: its cell, stored mirrored, is the cell mirrored.
            let m = cell.mirrored();
            assert_eq!((twin.at, twin.image_size()), (m.at, m.image_size()));
            assert_eq!(twin.rects, m.rects);
            assert_eq!(twin.image.as_bytes(0), m.image.as_bytes(0));
            assert!(cell.mirror_built().is_some() && cell.bytes() == 2 * (w * h * 4) as usize);
            // Mirroring the box moves it across the anchor's vertical.
            assert_eq!(m.at.0, geo.width as i32 - (cell.at.0 + w));
        });
    }

    #[test]
    fn below_the_contact_line_only_the_legs_stay() {
        // 4x3 cell, all opaque; line at row 0, legs in columns 2..4.
        let mut cell: Vec<u8> = (0..12).flat_map(|_| [9, 9, 9, 255]).collect();
        cut_below(&mut cell, 4, 0, [2, 4]);
        let alpha: Vec<u8> = cell.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        assert_eq!(alpha, [255, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn a_strip_of_the_wrong_size_is_refused() {
        crate::pack::tests::pack_in("cellsize", crate::pack::tests::pooled(), true, |p| {
            let p = p.unwrap();
            let a = p.anim.as_ref().unwrap();
            image::RgbaImage::new(5, 6).save(&a.strips[a.by_name["lie"]].path).unwrap();
            let geo = Geometry::new(&p, 60.0);
            let l0 = p.strip_first[a.by_name["lie"]].unwrap();
            let e = format!("{:#}", decode(&p, &geo, &[l0], false, 1).err().unwrap());
            assert!(e.contains("anim.json says 2 cells of 6x4"), "{e}");
        });
    }

    #[test]
    fn place_in_maps_a_box_through_any_scale() {
        assert_eq!(place_in(((0, 0), (10, 20)), (5.0, 6.0, 10.0, 20.0), 10, 20), (5.0, 6.0, 10.0, 20.0));
        assert_eq!(place_in(((2, 4), (3, 5)), (0.0, 0.0, 20.0, 40.0), 10, 20), (4.0, 8.0, 6.0, 10.0));
    }

    #[test]
    fn face_patch_moves_the_green_face_with_its_eyes_and_nothing_else() {
        let (w, h) = (40u32, 30u32);
        let px = |c: [u8; 4]| -> Vec<u8> { (0..w * h).flat_map(|_| c).collect() };
        let red = [220, 10, 40, 255];
        // The source: a green face (10..30 × 8..18) with a dark eye slit inside, on red hair.
        let mut src = px(red);
        for y in 8..18 {
            for x in 10..30 {
                let eye = (14..18).contains(&x) && (11..13).contains(&y);
                let i = ((y * w + x) * 4) as usize;
                src[i..i + 4].copy_from_slice(&if eye { [5, 5, 5, 255] } else { [40, 240, 90, 255] });
            }
        }
        let mut dst = px([1, 2, 3, 255]);
        face_patch(&mut dst, &src, w, h, Rect { x: 5, y: 4, width: 30, height: 20 }, (-2, 1));
        let at = |b: &[u8], x: u32, y: u32| b[((y * w + x) * 4) as usize..][..4].to_vec();
        assert_eq!(at(&dst, 14, 12), vec![5, 5, 5, 255], "the eye came along (gap closed)");
        assert_eq!(at(&dst, 10, 10), vec![40, 240, 90, 255], "the face, moved by (-2, 1)");
        assert_eq!(at(&dst, 38, 28), vec![1, 2, 3, 255], "far outside: untouched");
        assert_eq!(at(&dst, 2, 2), vec![1, 2, 3, 255], "the hair isn't copied");
        // Grown by FACE_GROW px around the face, no more.
        assert_ne!(at(&dst, 10 - 2 - FACE_GROW as u32, 12 + 1), vec![1, 2, 3, 255]);
        assert_eq!(at(&dst, 10 - 2 - FACE_GROW as u32 - 1, 12 + 1), vec![1, 2, 3, 255]);
    }
}
