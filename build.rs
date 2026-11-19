//! Her icon (assets/icon/capri-syn.ico, every size in it) in her Windows exe as its icon
//! resource, group 1: Explorer shows it, and her tray loads its own size from it
//! (`platform::windows::tray`). The .res is written here, as `rc.exe` writes it for
//! `1 ICON "capri-syn.ico"` (no SDK lookup, no crate); link.exe takes a .res as it is.

use std::path::PathBuf;

const ICO: &str = "assets/icon/capri-syn.ico";
/// Her icon's group id (`platform::windows::tray::ICON_ID`).
const GROUP: u16 = 1;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// MOVEABLE | DISCARDABLE, and | PURE for the group, as rc.exe sets them.
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
/// en-US, rc.exe's default.
const LANG: u16 = 0x0409;

fn main() {
    println!("cargo:rerun-if-changed={ICO}");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = std::fs::read(ICO).unwrap_or_else(|e| panic!("{ICO}: {e}"));
    let res = res_of(&ico).unwrap_or_else(|e| panic!("{ICO}: {e}"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("capri-syn-icon.res");
    std::fs::write(&out, res).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Her binary and its unit tests (they check it's there).
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=her icon resource is linked on msvc only: this exe has none");
    }
}

/// The .res for an .ico: an empty first entry, each image as an RT_ICON (ids 1..), then the
/// RT_GROUP_ICON naming them (the .ico's directory with ids for offsets).
fn res_of(ico: &[u8]) -> Result<Vec<u8>, String> {
    let u16_at = |i: usize| ico.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or("cut short");
    let u32_at = |i: usize| ico.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("cut short");
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return Err("not an icon".into());
    }
    let n = u16_at(4)?;
    if n == 0 {
        return Err("no images".into());
    }
    let mut res = Vec::new();
    entry(&mut res, 0, 0, 0, 0, &[]);
    let mut group = Vec::new();
    for v in [0, 1, n] {
        group.extend(v.to_le_bytes());
    }
    for k in 0..n {
        let e = 6 + 16 * usize::from(k);
        let (size, at) = (u32_at(e + 8)?, u32_at(e + 12)? as usize);
        let img = ico.get(at..at + size as usize).ok_or("an image past the end")?;
        let id = k + 1;
        entry(&mut res, RT_ICON, id, ICON_FLAGS, LANG, img);
        // Width, height, colours, reserved; planes and bits (an image of PNG often says 0: rc
        // says 1 and 32); its size; its id.
        let planes = u16_at(e + 4)?.max(1);
        let bits = match u16_at(e + 6)? {
            0 => 32,
            b => b,
        };
        group.extend(&ico[e..e + 4]);
        group.extend(planes.to_le_bytes());
        group.extend(bits.to_le_bytes());
        group.extend(size.to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, GROUP, GROUP_FLAGS, LANG, &group);
    Ok(res)
}

/// One resource: its header (type and name by number), its data, padded to 4 bytes.
fn entry(res: &mut Vec<u8>, ty: u16, name: u16, flags: u16, lang: u16, data: &[u8]) {
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes());
    for v in [0xffff, ty, 0xffff, name] {
        res.extend(v.to_le_bytes());
    }
    // Data version, then flags and language, then version and characteristics.
    res.extend(0u32.to_le_bytes());
    res.extend(flags.to_le_bytes());
    res.extend(lang.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(data);
    res.resize(res.len().next_multiple_of(4), 0);
}
