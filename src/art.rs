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
    frames: u32,
    cell: [u32; 2],
    fps: f32,
    #[serde(default)]
    r#loop: bool,
    family: String,
    #[serde(default)]
    anchor: Option<AnchorDoc>,
    #[serde(default)]
    bounds: Option<[i32; 4]>,
    #[serde(default)]
    mirror_art: Option<String>,
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
    strips: BTreeMap<String, StripDoc>,
    families: BTreeMap<String, FamilyDoc>,
    files: BTreeMap<String, FileDoc>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AnimDoc {
    schema_version: u32,
    #[serde(default)]
    lachy: Option<SideDoc>,
}

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
    /// `grip`; cell px.
    pub anchor: (String, i32, i32),
    /// The union of every cell's opaque box, cell px.
    pub bounds: Rect,
    /// Canvas px per cell px.
    pub k: f32,
    /// Her face looks right in the unmirrored cells.
    pub faces_right: bool,
    /// The strip drawn to be mirrored when she faces the other way (`mirrorArt`).
    pub mirror_art: Option<String>,
}

impl Strip {
    /// Frame id of cell `i` (the twin's cells are stored mirrored: `~` marks them).
    pub fn cell_id(&self, i: u32) -> String {
        format!("{}#{i}", self.name)
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

#[derive(Debug, Default)]
pub struct Anim {
    pub strips: Vec<Strip>,
    pub by_name: HashMap<String, usize>,
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
        let Some(side) = doc.lachy else { return Ok(Some(Anim::default())) };
        let root = dir.join("anim/lachy");
        let mut errors = Vec::new();
        let stand = side.families.get("stand").and_then(|f| f.body_px).filter(|&b| b > 0.0);
        let mut anim = Anim::default();
        for (name, d) in &side.strips {
            // Effects and overlays have no anchor: not body frames (later phases).
            let Some(a) = &d.anchor else { continue };
            let kind = match a.name.as_str() {
                "feet" => "ground",
                "seat" | "surface" => "seat",
                "grip" => "grip",
                other => {
                    errors.push(format!("anim strip `{name}`: anchor `{other}` (feet|seat|surface|grip)"));
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
            let (cw, ch) = (d.cell[0], d.cell[1]);
            if d.frames == 0 || cw == 0 || ch == 0 || d.fps <= 0.0 {
                errors.push(format!("anim strip `{name}`: {} frames of {cw}x{ch} at {} fps", d.frames, d.fps));
                continue;
            }
            let b = d.bounds.unwrap_or([0, 0, cw as i32, ch as i32]);
            let bounds = Rect { x: b[0], y: b[1], width: b[2], height: b[3] };
            if bounds.x < 0 || bounds.y < 0 || bounds.right() > cw as i32 || bounds.bottom() > ch as i32 || b[2] <= 0 || b[3] <= 0 {
                errors.push(format!("anim strip `{name}`: bounds {b:?} outside its {cw}x{ch} cell"));
            }
            let x2 = root.join("x2").join(&d.file);
            anim.by_name.insert(name.clone(), anim.strips.len());
            anim.strips.push(Strip {
                name: name.clone(),
                path,
                x2: x2.is_file().then_some(x2),
                sha256: side.files.get(&d.file).and_then(|f| f.sha256.clone()),
                frames: d.frames,
                cell: (cw, ch),
                ms: (1000.0 / d.fps).round().max(1.0) as u32,
                looping: d.r#loop,
                family_id: side.families.keys().position(|f| *f == d.family).unwrap_or(usize::MAX) as u16,
                anchor: (kind.into(), a.x, a.y),
                bounds,
                k,
                faces_right: !fam.faces.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("left")),
                mirror_art: d.mirror_art.clone(),
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
        let doc = json!({"schemaVersion": 1, "lachy": {
            "families": {"stand": {"bodyPx": 4, "faces": "right"}, "lie": {"faces": "right"}},
            "strips": {
                "stand": {"file": "sprites/stand.png", "frames": 3, "cell": [4, 6], "fps": 10, "loop": true, "family": "stand",
                          "anchor": {"name": "feet", "x": 2, "y": 5}, "bounds": [1, 1, 3, 4], "mirrorArt": "stand_flipped"},
                "stand_flipped": {"file": "sprites/stand_flipped.png", "frames": 3, "cell": [4, 6], "fps": 10, "loop": true,
                                  "family": "stand", "anchor": {"name": "feet", "x": 2, "y": 5}, "bounds": [1, 1, 3, 4]},
                "lie": {"file": "sprites/lie.png", "frames": 2, "cell": [6, 4], "fps": 4, "loop": true, "family": "lie",
                        "anchor": {"name": "surface", "x": 3, "y": 2}, "bounds": [1, 1, 4, 2]},
                "click": {"file": "sprites/click.png", "frames": 2, "cell": [4, 4], "fps": 30, "family": "fx"}
            },
            "files": {"sprites/stand.png": {"sha256": "ab"}}}});
        std::fs::write(dir.join("anim/anim.json"), doc.to_string()).unwrap();
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
        let s = a.strip("stand").unwrap();
        assert_eq!((s.k, s.ms, s.anchor.0.as_str(), s.looping), (2.0, 100, "ground", true), "bodyPx 4 against 8 canvas px");
        assert_eq!(s.sha256.as_deref(), Some("ab"));
        assert_eq!(s.mirror_art.as_deref(), Some("stand_flipped"));
        let l = a.strip("lie").unwrap();
        assert_eq!((l.anchor.0.as_str(), l.ms), ("seat", 250), "a surface sits on the seat line");
        let cells = a.cells(a.by_name["stand"], false);
        assert_eq!(cells.len(), 3);
        let (id, f) = &cells[2];
        assert_eq!(id, "stand#2");
        assert_eq!((f.anchor.x, f.anchor.y, f.bounds.x, f.bounds.width, f.bounds.height), (4, 10, 2, 6, 8));
        assert_eq!(a.cells(a.by_name["stand_flipped"], true)[0].0, "~stand_flipped#0");
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
        std::fs::write(&p, v.to_string()).unwrap();
        let e = format!("{:#}", Anim::load(&d, 8).unwrap_err());
        assert!(e.contains("mirrorArt `nope`") && e.contains("outside its 6x4 cell") && e.contains("gone.png is missing"), "{e}");
        std::fs::write(&p, json!({"schemaVersion": 2}).to_string()).unwrap();
        assert!(format!("{:#}", Anim::load(&d, 8).unwrap_err()).contains("schemaVersion 2"));
        std::fs::remove_dir_all(&d).ok();
    }
}
