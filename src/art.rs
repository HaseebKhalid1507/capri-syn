//! The pack's optional desktop art: `anim/anim.json`, written by tools/make-pack from
//! lachydotmcg/capri-desktop at its pin (our own measured geometry plus the sha256 of every file).
//! Each sprite strip is cut into cells at decode; every cell is a frame like any other,
//! registered on the pack's canvas by its strip's anchor: feet on the ground point, a seat (or
//! a lying figure's surface) on the seat line. Strips are sized against the pack's own body:
//! a standing lachy figure is as tall as the default sprite. A pack without `anim/` loads as before.

use crate::pack::{Anchor, FrameSource, Rect};
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone, Debug)]
struct AnchorDoc {
    name: String,
    x: i32,
    y: i32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct StripDoc {
    file: String,
    #[serde(default)]
    frames: u32,
    #[serde(default)]
    cell: [u32; 2],
    #[serde(default)]
    fps: f32,
    /// A cel sheet: its cells in `[cols, rows]`, row by row (absent: one row of `frames`).
    #[serde(default)]
    grid: Option<[u32; 2]>,
    /// The cells played, in order: keys and in-betweens, a cell may come again (absent: each
    /// cell once).
    #[serde(default)]
    order: Vec<u32>,
    /// The positions in `order` that are keys: thinned (`every` > 1), only they play, each held
    /// until the next (absent: every n-th position).
    #[serde(default)]
    keys: Vec<u32>,
    /// Per position (or per cell) ms, instead of `fps`.
    #[serde(default)]
    ms: Vec<u32>,
    /// A geometry file beside the sheet (relative to the side's root): the fields this entry
    /// leaves out (`cell`, `grid`, `frames`, `fps`, `order`, `keys`, `ms`, `bounds`, `anchor`,
    /// `contact`) come from it.
    #[serde(default)]
    geometry: Option<String>,
    /// An animated WebP: its cells are the file's frames, `cell` its canvas, `ms` its delays.
    #[serde(default)]
    animated: bool,
    /// The outfit (`moods.json` `variants.outfits`) this strip is drawn in; absent or `any`: all.
    #[serde(default)]
    outfit: Option<String>,
    #[serde(default)]
    r#loop: bool,
    family: String,
    #[serde(default)]
    anchor: Option<AnchorDoc>,
    #[serde(default)]
    bounds: Option<[i32; 4]>,
    #[serde(default)]
    mirror_art: Option<String>,
    #[serde(default)]
    contact: Option<ContactDoc>,
    #[serde(default)]
    glitch_cells: Vec<u32>,
}

/// On a window top: the line the figure sits or lies on (cell px), and the columns whose
/// pixels may hang below it (the lower legs). Everything else below the line is cut: the
/// window's front face hides it.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ContactDoc {
    pub line: u32,
    pub legs: [u32; 2],
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct FamilyDoc {
    #[serde(default)]
    body_px: Option<f32>,
    #[serde(default)]
    units_high_vs_stand: Option<f32>,
    #[serde(default)]
    faces: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct FileDoc {
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct SideDoc {
    /// Parsed one by one, after their `geometry` file is merged in.
    strips: BTreeMap<String, serde_json::Value>,
    stills: BTreeMap<String, StillDoc>,
    families: BTreeMap<String, FamilyDoc>,
    files: BTreeMap<String, FileDoc>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct StillDoc {
    file: String,
}

#[derive(Deserialize, Clone, Debug)]
struct MochiFile {
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

/// `mochi`: the local-only images (`make-pack --mochi`): a file list, no geometry.
#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct MochiDoc {
    files: BTreeMap<String, MochiFile>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AnimDoc {
    schema_version: u32,
    #[serde(default)]
    lachy: Option<SideDoc>,
    #[serde(default)]
    mochi: Option<MochiDoc>,
}

/// The cinematic intro's close-ups, in `crate::intro::FRAMES` order: `anim/mochi/intro-<name>.*`.
/// Local only: a pack built without `--mochi` has none, and the intro is just the bolt.
pub const INTRO_KIND: &str = "intro";

/// One strip, resolved: where its cells come from and how they register.
#[derive(Clone, Debug)]
pub struct Strip {
    pub name: String,
    pub path: PathBuf,
    /// The same strip at twice the size (`make-pack --upscale`), if the pack has it.
    pub x2: Option<PathBuf>,
    pub sha256: Option<String>,
    pub frames: u32,
    pub cell: (u32, u32),
    /// Per cell, from the strip's fps.
    pub ms: u32,
    pub looping: bool,
    /// Its family's index in anim.json (a seam is a change of it).
    pub family_id: u16,
    /// The pack's anchor kind: `ground` (feet), `seat` (a seat or a lying figure's surface),
    /// `grip`, `ceiling` (hands on a bar, on the hang pose's grip); cell px.
    pub anchor: (String, i32, i32),
    /// The union of every cell's opaque box, cell px.
    pub bounds: Rect,
    /// Canvas px per cell px.
    pub k: f32,
    /// Her face looks right in the unmirrored cells.
    pub faces_right: bool,
    /// The strip drawn to be mirrored when she faces the other way (`mirrorArt`).
    pub mirror_art: Option<String>,
    /// Below `line` only the `legs` columns stay (cell px; see `ContactDoc`).
    pub contact: Option<ContactDoc>,
    /// An effect drawn as frames (the form bloom): it doesn't size her window (cut to it).
    pub fx: bool,
    /// Cells with a chroma ghost drawn into them (`glitchCells`): a loop she holds (a dance,
    /// a dangle) skips them, so no glitch runs inside it.
    pub glitch_cells: Vec<u32>,
    /// The sheet: cells per row, and how many cells it holds (a plain strip: `frames`, 1 row).
    pub cols: u32,
    pub sheet_cells: u32,
    /// Per position (0..frames), the sheet cell it shows (empty: position = cell).
    pub src: Vec<u32>,
    /// Per position, its ms (empty: `ms` each).
    pub cell_ms: Vec<u32>,
    /// Key positions, ascending (empty: none marked).
    pub keys: Vec<u32>,
    /// An animated WebP: cells are its frames.
    pub animated: bool,
    /// Drawn in this outfit only (None: any outfit).
    pub outfit: Option<String>,
}

impl Strip {
    /// The cells a loop plays: all but the glitched ones (all of them if that's every cell).
    pub fn clean_cells(&self) -> Vec<u32> {
        let v: Vec<u32> = (0..self.frames).filter(|c| !self.glitch_cells.contains(c)).collect();
        if v.is_empty() { (0..self.frames).collect() } else { v }
    }

    /// Frame id of cell `i` (the twin's cells are stored mirrored: `~` marks them).
    pub fn cell_id(&self, i: u32) -> String {
        format!("{}#{i}", self.name)
    }

    /// The sheet cell position `i` shows.
    pub fn sheet_cell(&self, i: u32) -> u32 {
        self.src.get(i as usize).copied().unwrap_or(i)
    }

    /// How long position `i` holds.
    pub fn ms_at(&self, i: u32) -> u32 {
        self.cell_ms.get(i as usize).copied().unwrap_or(self.ms)
    }

    /// The positions played thinned to every `every`-th (a cel sheet with keys: its keys), each
    /// with how long it holds: its own ms, or (thinned) until the next one played.
    pub fn played(&self, every: u32) -> Vec<(u32, u32)> {
        let e = every.max(1);
        if e == 1 {
            return (0..self.frames).map(|i| (i, self.ms_at(i))).collect();
        }
        if self.cell_ms.is_empty() && self.keys.is_empty() {
            return (0..self.frames).step_by(e as usize).map(|i| (i, self.ms * e)).collect();
        }
        let starts: Vec<u32> = if self.keys.is_empty() { (0..self.frames).step_by(e as usize).collect() } else { self.keys.clone() };
        starts
            .iter()
            .enumerate()
            .map(|(j, &s)| {
                let end = starts.get(j + 1).copied().unwrap_or(self.frames);
                (s, (s..end).map(|i| self.ms_at(i)).sum::<u32>().max(1))
            })
            .collect()
    }

    /// Wearable in outfit `outfit` (`moods.json` `variants.outfits`).
    pub fn worn_in(&self, outfit: &str) -> bool {
        self.outfit.as_deref().is_none_or(|o| o == outfit || o.eq_ignore_ascii_case("any"))
    }
}

/// An animated WebP's canvas and per-frame delays (ms; 0 reads as 100, as browsers do), from
/// its RIFF chunks alone: nothing is decoded.
pub fn webp_frames(path: &Path) -> Result<((u32, u32), Vec<u32>)> {
    use std::io::{Read, Seek, SeekFrom};
    let u24 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], 0]);
    let mut f = std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr).with_context(|| format!("{} is not a WebP", path.display()))?;
    if &hdr[0..4] != b"RIFF" || &hdr[8..12] != b"WEBP" {
        bail!("{} is not a WebP", path.display());
    }
    let end = 8 + u64::from(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]));
    let (mut canvas, mut ms) = (None, Vec::new());
    let mut pos = 12u64;
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos))?;
        let mut ch = [0u8; 8];
        if f.read_exact(&mut ch).is_err() {
            break;
        }
        let len = u64::from(u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]));
        match &ch[0..4] {
            b"VP8X" => {
                let mut p = [0u8; 10];
                f.read_exact(&mut p)?;
                canvas = Some((u24(&p[4..7]) + 1, u24(&p[7..10]) + 1));
            }
            b"ANMF" => {
                let mut p = [0u8; 16];
                f.read_exact(&mut p)?;
                let d = u24(&p[12..15]);
                ms.push(if d == 0 { 100 } else { d });
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    match canvas {
        Some(c) if !ms.is_empty() => Ok((c, ms)),
        _ => bail!("{} is not an animated WebP", path.display()),
    }
}

/// A cell of a strip, as a frame source.
#[derive(Clone, Debug)]
pub struct CellSrc {
    /// Index into `Anim::strips`.
    pub strip: usize,
    pub i: u32,
    /// Stored mirrored: a `mirrorArt` twin's cell, shown for the base strip when she faces
    /// the other way.
    pub mirror: bool,
}

/// An effect strip without an anchor (the click burst): drawn over her as solid-colour layers,
/// never one of her frames.
#[derive(Clone, Debug)]
pub struct Effect {
    pub path: PathBuf,
    pub sha256: Option<String>,
    pub frames: u32,
    pub cell: (u32, u32),
    /// Per cell, from the strip's fps.
    pub ms: u32,
    /// Canvas px per cell px.
    pub k: f32,
}

#[derive(Debug, Default)]
pub struct Anim {
    pub strips: Vec<Strip>,
    /// Effect strips by name (`Effect`).
    pub effects: HashMap<String, Effect>,
    pub by_name: HashMap<String, usize>,
    /// The close-ups for the cinematic intro (path, sha256), all of them or none.
    pub intro: Vec<(PathBuf, Option<String>)>,
    /// lachy's stills on disk, by name (`face`, `capri`, `cursor`): (path, sha256).
    pub stills: HashMap<String, (PathBuf, Option<String>)>,
    /// The local-only images besides the intro's (`make-pack --mochi`: the cut-ins, the Line's
    /// faces), by file stem (`cutin-eyes`, `face-calm`): (path, sha256). Only those on disk.
    pub mochi: HashMap<String, (PathBuf, Option<String>)>,
}

/// `mochi.files` other than the intro's, by stem, that are on disk.
fn mochi_files(dir: &Path, m: Option<&MochiDoc>) -> HashMap<String, (PathBuf, Option<String>)> {
    let Some(m) = m else { return HashMap::new() };
    let root = dir.join("anim/mochi");
    m.files
        .iter()
        .filter(|(f, d)| !f.contains('/') && d.kind.as_deref() != Some(INTRO_KIND) && root.join(f).is_file())
        .filter_map(|(f, d)| Some((Path::new(f).file_stem()?.to_str()?.to_string(), (root.join(f), d.sha256.clone()))))
        .collect()
}

/// Every close-up the intro needs, from `mochi.files` (kind `intro`), in order; empty unless all
/// are listed and on disk.
fn intro_files(dir: &Path, m: Option<&MochiDoc>) -> Vec<(PathBuf, Option<String>)> {
    let Some(m) = m else { return Vec::new() };
    let root = dir.join("anim/mochi");
    let mut out = Vec::new();
    for name in crate::intro::FRAMES {
        let stem = format!("intro-{name}.");
        let hit = m.files.iter().find(|(f, d)| f.starts_with(&stem) && !f.contains('/') && d.kind.as_deref() == Some(INTRO_KIND));
        match hit {
            Some((f, d)) if root.join(f).is_file() => out.push((root.join(f), d.sha256.clone())),
            _ => return Vec::new(),
        }
    }
    out
}

/// A strip entry with its `geometry` file merged under it (the entry's own fields win).
fn strip_doc(root: &Path, raw: &serde_json::Value) -> Result<StripDoc> {
    let mut v = raw.clone();
    if let Some(g) = raw.get("geometry").and_then(|g| g.as_str()) {
        let p = root.join(g);
        let text = std::fs::read_to_string(&p).with_context(|| format!("geometry {} is missing", p.display()))?;
        let geo: serde_json::Value = serde_json::from_str(&text).with_context(|| format!("geometry {} is invalid", p.display()))?;
        if let (Some(obj), Some(geo)) = (v.as_object_mut(), geo.as_object()) {
            for (k, x) in geo {
                obj.entry(k.clone()).or_insert_with(|| x.clone());
            }
        }
    }
    serde_json::from_value(v).context("not a strip entry")
}

/// A strip's sheet, resolved: its cells, the positions played and their timing.
struct Sheet {
    cell: (u32, u32),
    cols: u32,
    cells: u32,
    frames: u32,
    ms: u32,
    src: Vec<u32>,
    cell_ms: Vec<u32>,
    keys: Vec<u32>,
}

/// One row of `frames` cells (as ever), a cel sheet's grid, or an animated WebP's frames; then
/// `order` (keys and in-betweens), `keys` and per-position `ms`. None: refused (in `errors`).
fn sheet_of(name: &str, d: &StripDoc, path: &Path, errors: &mut Vec<String>) -> Option<Sheet> {
    let (cell, cols, cells, own_ms) = if d.animated {
        match webp_frames(path) {
            Ok((c, ms)) => (c, ms.len() as u32, ms.len() as u32, ms),
            Err(e) if path.is_file() => {
                errors.push(format!("anim strip `{name}`: {e:#}"));
                return None;
            }
            // Missing: reported with the other strips' missing files.
            Err(_) => return None,
        }
    } else {
        let (cols, rows) = d.grid.map_or((d.frames, 1), |[c, r]| (c, r));
        let cells = if d.grid.is_some() && d.frames == 0 { cols * rows } else { d.frames };
        if d.grid.is_some() && (cols == 0 || cells > cols * rows) {
            errors.push(format!("anim strip `{name}`: {cells} cells in a {cols}x{rows} grid"));
            return None;
        }
        ((d.cell[0], d.cell[1]), cols, cells, Vec::new())
    };
    let (cw, ch) = cell;
    if cells == 0 || cw == 0 || ch == 0 || (d.fps <= 0.0 && d.ms.is_empty() && own_ms.is_empty()) {
        errors.push(format!("anim strip `{name}`: {cells} frames of {cw}x{ch} at {} fps", d.fps));
        return None;
    }
    if let Some(bad) = d.order.iter().find(|&&c| c >= cells) {
        errors.push(format!("anim strip `{name}`: order cell {bad} past its {cells} cells"));
        return None;
    }
    let frames = if d.order.is_empty() { cells } else { d.order.len() as u32 };
    let by_cell = |per_cell: &[u32]| -> Vec<u32> { (0..frames).map(|i| per_cell[d.order.get(i as usize).copied().unwrap_or(i) as usize]).collect() };
    let cell_ms: Vec<u32> = match d.ms.len() {
        0 if own_ms.is_empty() => Vec::new(),
        0 => by_cell(&own_ms),
        n if n == frames as usize => d.ms.clone(),
        n if n == cells as usize => by_cell(&d.ms),
        n => {
            errors.push(format!("anim strip `{name}`: {n} ms for {frames} positions ({cells} cells)"));
            return None;
        }
    };
    let cell_ms: Vec<u32> = cell_ms.into_iter().map(|m| m.max(1)).collect();
    let mut keys = d.keys.clone();
    keys.sort_unstable();
    keys.dedup();
    if let Some(bad) = keys.iter().find(|&&k| k >= frames) {
        errors.push(format!("anim strip `{name}`: key {bad} past its {frames} positions"));
        return None;
    }
    let ms = if cell_ms.is_empty() { (1000.0 / d.fps).round().max(1.0) as u32 } else { (cell_ms.iter().map(|&m| u64::from(m)).sum::<u64>() / u64::from(frames)).max(1) as u32 };
    Some(Sheet { cell, cols: cols.max(1), cells, frames, ms, src: d.order.clone(), cell_ms, keys })
}

impl Anim {
    pub fn strip(&self, name: &str) -> Option<&Strip> {
        self.by_name.get(name).map(|&i| &self.strips[i])
    }

    /// `dir/anim/anim.json`, if the pack has one. `ref_h`: the default sprite's visible height
    /// (canvas px), which a standing lachy figure (`families.stand.bodyPx`) is scaled to.
    pub fn load(dir: &Path, ref_h: i32) -> Result<Option<Anim>> {
        let p = dir.join("anim/anim.json");
        let s = match std::fs::read_to_string(&p) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("pack: cannot read {}", p.display())),
        };
        let doc: AnimDoc = serde_json::from_str(&s).with_context(|| format!("pack: {} is invalid", p.display()))?;
        if doc.schema_version != 1 {
            bail!("pack: anim/anim.json schemaVersion {} (expected 1)", doc.schema_version);
        }
        let intro = intro_files(dir, doc.mochi.as_ref());
        let mochi = mochi_files(dir, doc.mochi.as_ref());
        let Some(side) = doc.lachy else { return Ok(Some(Anim { intro, mochi, ..Anim::default() })) };
        let root = dir.join("anim/lachy");
        let mut errors = Vec::new();
        let stand = side.families.get("stand").and_then(|f| f.body_px).filter(|&b| b > 0.0);
        let mut anim = Anim { intro, mochi, ..Anim::default() };
        for (name, s) in &side.stills {
            let path = root.join(&s.file);
            if path.is_file() {
                anim.stills.insert(name.clone(), (path, side.files.get(&s.file).and_then(|f| f.sha256.clone())));
            }
        }
        for (name, raw) in &side.strips {
            let d = match strip_doc(&root, raw) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("anim strip `{name}`: {e:#}"));
                    continue;
                }
            };
            let d = &d;
            // Effects and overlays have no anchor: not body frames. An effect is kept as such.
            let Some(a) = &d.anchor else {
                let fam = side.families.get(&d.family).cloned().unwrap_or_default();
                if d.family == "fx"
                    && let Some(body) = stand
                    && d.frames > 0
                    && d.cell[0] > 0
                    && d.cell[1] > 0
                    && d.fps > 0.0
                {
                    let path = root.join(&d.file);
                    if path.is_file() {
                        anim.effects.insert(name.clone(), Effect {
                            path,
                            sha256: side.files.get(&d.file).and_then(|f| f.sha256.clone()),
                            frames: d.frames,
                            cell: (d.cell[0], d.cell[1]),
                            ms: (1000.0 / d.fps).round().max(1.0) as u32,
                            k: ref_h as f32 / body * fam.units_high_vs_stand.unwrap_or(1.0),
                        });
                    }
                }
                continue;
            };
            let kind = match a.name.as_str() {
                "feet" => "ground",
                "seat" | "surface" => "seat",
                "grip" => "grip",
                // Hanging from a bar: its grip on the hang pose's (`brain.hangFidgets`).
                "ceiling" => "ceiling",
                other => {
                    errors.push(format!("anim strip `{name}`: anchor `{other}` (feet|seat|surface|grip|ceiling)"));
                    continue;
                }
            };
            let fam = side.families.get(&d.family).cloned().unwrap_or_default();
            let Some(body) = stand else {
                errors.push("anim.json: families.stand.bodyPx is missing".into());
                break;
            };
            let k = ref_h as f32 / body * fam.units_high_vs_stand.unwrap_or(1.0);
            let path = root.join(&d.file);
            if !path.is_file() {
                errors.push(format!("anim strip `{name}`: {} is missing", d.file));
            }
            let Some(sheet) = sheet_of(name, d, &path, &mut errors) else { continue };
            let (cw, ch) = sheet.cell;
            let b = d.bounds.unwrap_or([0, 0, cw as i32, ch as i32]);
            let bounds = Rect { x: b[0], y: b[1], width: b[2], height: b[3] };
            if bounds.x < 0 || bounds.y < 0 || bounds.right() > cw as i32 || bounds.bottom() > ch as i32 || b[2] <= 0 || b[3] <= 0 {
                errors.push(format!("anim strip `{name}`: bounds {b:?} outside its {cw}x{ch} cell"));
            }
            if let Some(c) = d.contact
                && (c.line >= ch || c.legs[0] > c.legs[1] || c.legs[1] > cw)
            {
                errors.push(format!("anim strip `{name}`: contact {c:?} outside its {cw}x{ch} cell"));
            }
            let x2 = root.join("x2").join(&d.file);
            anim.by_name.insert(name.clone(), anim.strips.len());
            anim.strips.push(Strip {
                name: name.clone(),
                path,
                x2: x2.is_file().then_some(x2),
                sha256: side.files.get(&d.file).and_then(|f| f.sha256.clone()),
                frames: sheet.frames,
                cell: (cw, ch),
                ms: sheet.ms,
                looping: d.r#loop,
                family_id: side.families.keys().position(|f| *f == d.family).unwrap_or(usize::MAX) as u16,
                anchor: (kind.into(), a.x, a.y),
                bounds,
                k,
                faces_right: !fam.faces.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("left")),
                mirror_art: d.mirror_art.clone(),
                contact: d.contact,
                fx: d.family == "fx",
                glitch_cells: d.glitch_cells.iter().copied().filter(|&c| c < sheet.frames).collect(),
                cols: sheet.cols,
                sheet_cells: sheet.cells,
                src: sheet.src,
                cell_ms: sheet.cell_ms,
                keys: sheet.keys,
                animated: d.animated,
                outfit: d.outfit.clone(),
            });
        }
        for s in &anim.strips {
            if let Some(m) = &s.mirror_art {
                match anim.strip(m) {
                    None => errors.push(format!("anim strip `{}`: mirrorArt `{m}` is not a strip", s.name)),
                    Some(t) if t.cell != s.cell || t.frames != s.frames => {
                        errors.push(format!("anim strip `{}`: mirrorArt `{m}` has another grid", s.name))
                    }
                    _ => {}
                }
            }
        }
        if !errors.is_empty() {
            bail!("pack {} anim/anim.json failed validation:\n  - {}", dir.display(), errors.join("\n  - "));
        }
        Ok(Some(anim))
    }

    /// The frame sources of strip `si`'s cells (`mirror`: stored mirrored, a twin's).
    pub fn cells(&self, si: usize, mirror: bool) -> Vec<(String, FrameSource)> {
        let s = &self.strips[si];
        let scale = |v: i32| (v as f32 * s.k).round() as i32;
        let anchor = Anchor { name: s.anchor.0.clone(), x: scale(s.anchor.1), y: scale(s.anchor.2) };
        let b = s.bounds;
        let bounds = Rect { x: scale(b.x), y: scale(b.y), width: scale(b.width).max(1), height: scale(b.height).max(1) };
        (0..s.frames)
            .map(|i| {
                let id = if mirror { format!("~{}", s.cell_id(i)) } else { s.cell_id(i) };
                let src = FrameSource {
                    id: id.clone(),
                    path: s.path.clone(),
                    anchor: anchor.clone(),
                    bounds,
                    sha256: None,
                    patch: None,
                    cell: Some(CellSrc { strip: si, i, mirror }),
                };
                (id, src)
            })
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    /// `dir/anim/`: a 3-cell 4×6 standing strip with a twin, a 2-cell 6×4 lying strip.
    pub fn write_anim(dir: &Path) {
        let s = dir.join("anim/lachy/sprites");
        std::fs::create_dir_all(&s).unwrap();
        let strip = |f: &str, w: u32, h: u32, n: u32| {
            let mut im = image::RgbaImage::new(w * n, h);
            for i in 0..n {
                // A 2×(h-2) red body per cell, one px further right each cell.
                for y in 1..h - 1 {
                    for x in 1 + i..3 + i {
                        im.put_pixel(i * w + x.min(w - 1), y, image::Rgba([255, 0, 0, 255]));
                    }
                }
            }
            im.save(s.join(f)).unwrap();
        };
        strip("stand.png", 4, 6, 3);
        strip("stand_flipped.png", 4, 6, 3);
        strip("lie.png", 6, 4, 2);
        strip("perch_in.png", 4, 6, 2);
        strip("click.png", 4, 4, 2);
        strip("hang.png", 4, 6, 2);
        let doc = json!({"schemaVersion": 1, "lachy": {
            "families": {"stand": {"bodyPx": 4, "faces": "right"}, "lie": {"faces": "right"}},
            "strips": {
                "stand": {"file": "sprites/stand.png", "frames": 3, "cell": [4, 6], "fps": 10, "loop": true, "family": "stand",
                          "anchor": {"name": "feet", "x": 2, "y": 5}, "bounds": [1, 1, 3, 4], "mirrorArt": "stand_flipped"},
                "stand_flipped": {"file": "sprites/stand_flipped.png", "frames": 3, "cell": [4, 6], "fps": 10, "loop": true,
                                  "family": "stand", "anchor": {"name": "feet", "x": 2, "y": 5}, "bounds": [1, 1, 3, 4]},
                "perch_in": {"file": "sprites/perch_in.png", "frames": 2, "cell": [4, 6], "fps": 10, "family": "lie",
                             "anchor": {"name": "seat", "x": 2, "y": 4}, "bounds": [1, 1, 3, 4]},
                "lie": {"file": "sprites/lie.png", "frames": 2, "cell": [6, 4], "fps": 4, "loop": true, "family": "lie",
                        "anchor": {"name": "surface", "x": 3, "y": 2}, "bounds": [1, 1, 4, 2],
                        "contact": {"line": 2, "legs": [4, 6]}},
                "click": {"file": "sprites/click.png", "frames": 2, "cell": [4, 4], "fps": 30, "family": "fx"},
                "hang": {"file": "sprites/hang.png", "frames": 2, "cell": [4, 6], "fps": 10, "loop": true, "family": "dangle",
                         "anchor": {"name": "grip", "x": 2, "y": 1}, "bounds": [1, 1, 3, 4]}
            },
            "files": {"sprites/stand.png": {"sha256": "ab"}}}});
        std::fs::write(dir.join("anim/anim.json"), doc.to_string()).unwrap();
    }

    /// An animated WebP of `frames` (lossless), `ms` each: the encoder's still VP8L bitstreams
    /// wrapped in ANMF chunks (synthetic test art: solid shapes only).
    pub fn animated_webp(frames: &[image::RgbaImage], ms: &[u32]) -> Vec<u8> {
        let (w, h) = frames[0].dimensions();
        let u24 = |v: u32| v.to_le_bytes()[..3].to_vec();
        let chunk = |tag: &[u8], data: &[u8]| -> Vec<u8> {
            let mut c = tag.to_vec();
            c.extend((data.len() as u32).to_le_bytes());
            c.extend(data);
            if data.len() & 1 == 1 {
                c.push(0);
            }
            c
        };
        let mut body = chunk(b"VP8X", &[vec![0x12, 0, 0, 0], u24(w - 1), u24(h - 1)].concat());
        body.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
        for (f, &d) in frames.iter().zip(ms) {
            let mut still = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut still).encode(f.as_raw(), w, h, image::ExtendedColorType::Rgba8).unwrap();
            assert_eq!(&still[12..16], b"VP8L", "the simple container");
            let n = u32::from_le_bytes([still[16], still[17], still[18], still[19]]) as usize;
            let mut anmf = [u24(0), u24(0), u24(w - 1), u24(h - 1), u24(d)].concat();
            anmf.push(0x02);
            anmf.extend(chunk(b"VP8L", &still[20..20 + n]));
            body.extend(chunk(b"ANMF", &anmf));
        }
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32 + 4).to_le_bytes());
        out.extend(b"WEBP");
        out.extend(body);
        out
    }

    /// A `w`×`h` cell of one colour with a 2 px column of it (the rest clear): `n` such cells.
    pub fn solid_cells(w: u32, h: u32, colours: &[[u8; 3]]) -> Vec<image::RgbaImage> {
        colours
            .iter()
            .map(|c| {
                let mut im = image::RgbaImage::new(w, h);
                for y in 1..h - 1 {
                    for x in 1..3 {
                        im.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
                    }
                }
                im
            })
            .collect()
    }

    /// Add to `dir/anim/`: a 2x2 cel sheet (`sheet`, 3 cells used, a geometry file, keys and
    /// in-betweens, per-position ms), an animated WebP (`greet`, 3 frames) and a hang strip
    /// (`pullup`, ceiling anchor, drawn for `outfit` if given). Cells: red, green, blue.
    pub fn write_cels(dir: &Path, outfit: Option<&str>) {
        let s = dir.join("anim/lachy/sprites");
        let cells = solid_cells(4, 6, &[[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
        let mut sheet = image::RgbaImage::new(8, 12);
        for (i, c) in cells.iter().enumerate() {
            image::imageops::replace(&mut sheet, c, (i as i64 % 2) * 4, (i as i64 / 2) * 6);
        }
        sheet.save(s.join("sheet.png")).unwrap();
        std::fs::write(s.join("sheet.json"), json!({"cell": [4, 6], "grid": [2, 2], "frames": 3, "anchor": {"name": "feet", "x": 2, "y": 5}, "bounds": [1, 1, 3, 4]}).to_string()).unwrap();
        std::fs::write(s.join("greet.webp"), animated_webp(&cells, &[80, 120, 200])).unwrap();
        cells[0].save(s.join("pullup.png")).unwrap();
        let p = dir.join("anim/anim.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let st = &mut v["lachy"]["strips"];
        st["sheet"] = json!({"file": "sprites/sheet.png", "geometry": "sprites/sheet.json", "family": "stand",
                             "order": [0, 1, 2, 1, 0], "keys": [0, 2], "ms": [100, 50, 300, 50, 100]});
        st["greet"] = json!({"file": "sprites/greet.webp", "animated": true, "family": "stand", "anchor": {"name": "feet", "x": 2, "y": 5}});
        st["pullup"] = json!({"file": "sprites/pullup.png", "frames": 1, "cell": [4, 6], "fps": 10, "family": "hang",
                              "anchor": {"name": "ceiling", "x": 2, "y": 0}});
        if let Some(o) = outfit {
            st["pullup"]["outfit"] = json!(o);
        }
        std::fs::write(&p, v.to_string()).unwrap();
    }

    #[test]
    fn a_cel_sheet_plays_its_order_keys_and_timing() {
        let d = tmp("cels");
        write_anim(&d);
        write_cels(&d, None);
        let a = Anim::load(&d, 8).unwrap().unwrap();
        let s = a.strip("sheet").unwrap();
        // The geometry file filled cell, grid, frames, anchor and bounds.
        assert_eq!((s.cell, s.cols, s.sheet_cells, s.frames), ((4, 6), 2, 3, 5));
        assert_eq!((s.anchor.0.as_str(), s.bounds.width), ("ground", 3));
        assert_eq!((0..5).map(|i| s.sheet_cell(i)).collect::<Vec<_>>(), [0, 1, 2, 1, 0], "keys and in-betweens");
        assert_eq!(s.played(1), [(0, 100), (1, 50), (2, 300), (3, 50), (4, 100)]);
        // Thinned: the keys, each held until the next.
        assert_eq!(s.played(2), [(0, 150), (2, 450)]);
        assert_eq!(s.ms, 120, "the mean, for whatever wants one");
        // A plain strip thins as it always did.
        assert_eq!(a.strip("stand").unwrap().played(2), [(0, 200), (2, 200)]);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn an_animated_webp_strip_takes_its_frames_and_delays_from_the_file() {
        let d = tmp("webp");
        write_anim(&d);
        write_cels(&d, Some("alt"));
        let ((w, h), ms) = webp_frames(&d.join("anim/lachy/sprites/greet.webp")).unwrap();
        assert_eq!(((w, h), ms.as_slice()), ((4, 6), &[80, 120, 200][..]));
        let a = Anim::load(&d, 8).unwrap().unwrap();
        let g = a.strip("greet").unwrap();
        assert!(g.animated);
        assert_eq!((g.cell, g.frames, g.cell_ms.as_slice()), ((4, 6), 3, &[80, 120, 200][..]));
        assert_eq!(g.played(1).iter().map(|p| p.1).sum::<u32>(), 400);
        // A hang strip registers on the hang pose, drawn for one outfit only.
        let p = a.strip("pullup").unwrap();
        assert_eq!(p.anchor.0, "ceiling");
        assert!(p.worn_in("alt") && !p.worn_in("pack") && a.strip("greet").unwrap().worn_in("pack"));
        assert!(webp_frames(&d.join("anim/lachy/sprites/sheet.png")).is_err(), "a PNG isn't one");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_cel_sheet_out_of_its_grid_is_refused() {
        let d = tmp("celbad");
        write_anim(&d);
        write_cels(&d, None);
        let p = d.join("anim/anim.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        v["lachy"]["strips"]["sheet"]["order"] = json!([0, 3]);
        v["lachy"]["strips"]["greet"]["ms"] = json!([1, 2]);
        std::fs::write(&p, v.to_string()).unwrap();
        let e = format!("{:#}", Anim::load(&d, 8).unwrap_err());
        assert!(e.contains("order cell 3 past its 3 cells") && e.contains("2 ms for 3 positions"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sa-art-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_anim_dir_is_no_art() {
        let d = tmp("none");
        assert!(Anim::load(&d, 1050).unwrap().is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn strips_resolve_with_scale_anchor_kind_and_twins() {
        let d = tmp("ok");
        write_anim(&d);
        let a = Anim::load(&d, 8).unwrap().unwrap();
        assert!(a.strip("click").is_none(), "no anchor: not a body strip");
        let e = &a.effects["click"];
        assert_eq!((e.frames, e.cell, e.ms), (2, (4, 4), 33), "an effect: drawn over her, never a frame");
        let s = a.strip("stand").unwrap();
        assert_eq!((s.k, s.ms, s.anchor.0.as_str(), s.looping), (2.0, 100, "ground", true), "bodyPx 4 against 8 canvas px");
        assert_eq!(s.sha256.as_deref(), Some("ab"));
        assert_eq!(s.mirror_art.as_deref(), Some("stand_flipped"));
        let l = a.strip("lie").unwrap();
        assert_eq!((l.anchor.0.as_str(), l.ms), ("seat", 250), "a surface sits on the seat line");
        assert_eq!(l.contact, Some(ContactDoc { line: 2, legs: [4, 6] }));
        assert_eq!(s.contact, None);
        let cells = a.cells(a.by_name["stand"], false);
        assert_eq!(cells.len(), 3);
        let (id, f) = &cells[2];
        assert_eq!(id, "stand#2");
        assert_eq!((f.anchor.x, f.anchor.y, f.bounds.x, f.bounds.width, f.bounds.height), (4, 10, 2, 6, 8));
        assert_eq!(a.cells(a.by_name["stand_flipped"], true)[0].0, "~stand_flipped#0");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_intro_close_ups_are_all_or_nothing() {
        let d = tmp("intro");
        write_anim(&d);
        let p = d.join("anim/anim.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        std::fs::create_dir_all(d.join("anim/mochi")).unwrap();
        let mut files = serde_json::Map::new();
        for n in crate::intro::FRAMES {
            let f = format!("intro-{n}.webp");
            std::fs::write(d.join("anim/mochi").join(&f), b"x").unwrap();
            files.insert(f, json!({"sha256": "00", "kind": "intro"}));
        }
        files.insert("face-calm.png".into(), json!({"sha256": "01", "kind": "face"}));
        v["mochi"] = json!({"files": files});
        std::fs::write(&p, v.to_string()).unwrap();
        let a = Anim::load(&d, 8).unwrap().unwrap();
        assert_eq!(a.intro.len(), 9);
        assert!(a.intro[0].0.ends_with("anim/mochi/intro-grab.webp") && a.intro[8].0.ends_with("intro-happy.webp"));
        assert_eq!(a.intro[0].1.as_deref(), Some("00"));
        // The other images by stem, only those on disk.
        assert!(a.mochi.is_empty(), "face-calm.png isn't on disk");
        std::fs::write(d.join("anim/mochi/face-calm.png"), b"x").unwrap();
        let a = Anim::load(&d, 8).unwrap().unwrap();
        assert_eq!(a.mochi.get("face-calm").map(|m| m.1.as_deref()), Some(Some("01")));
        assert!(a.mochi.keys().all(|k| !k.starts_with("intro-")));
        // One missing on disk: no intro at all (it silently doesn't exist).
        std::fs::remove_file(d.join("anim/mochi/intro-smear.webp")).unwrap();
        assert!(Anim::load(&d, 8).unwrap().unwrap().intro.is_empty());
        // Without lachy's art the intro still loads.
        v.as_object_mut().unwrap().remove("lachy");
        std::fs::write(d.join("anim/mochi/intro-smear.webp"), b"x").unwrap();
        std::fs::write(&p, v.to_string()).unwrap();
        let a = Anim::load(&d, 8).unwrap().unwrap();
        assert!(a.strips.is_empty() && a.intro.len() == 9);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_broken_anim_json_is_refused() {
        let d = tmp("bad");
        write_anim(&d);
        let p = d.join("anim/anim.json");
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        v["lachy"]["strips"]["stand"]["mirrorArt"] = json!("nope");
        v["lachy"]["strips"]["lie"]["bounds"] = json!([0, 0, 9, 9]);
        v["lachy"]["strips"]["stand_flipped"]["file"] = json!("sprites/gone.png");
        v["lachy"]["strips"]["stand"]["contact"] = json!({"line": 6, "legs": [0, 4]});
        std::fs::write(&p, v.to_string()).unwrap();
        let e = format!("{:#}", Anim::load(&d, 8).unwrap_err());
        assert!(e.contains("mirrorArt `nope`") && e.contains("outside its 6x4 cell") && e.contains("gone.png is missing"), "{e}");
        assert!(e.contains("contact ContactDoc { line: 6, legs: [0, 4] } outside its 4x6 cell"), "{e}");
        std::fs::write(&p, json!({"schemaVersion": 2}).to_string()).unwrap();
        assert!(format!("{:#}", Anim::load(&d, 8).unwrap_err()).contains("schemaVersion 2"));
        std::fs::remove_dir_all(&d).ok();
    }
}
