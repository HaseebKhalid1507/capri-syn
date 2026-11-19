//! What she shows the desktop as, from one place: her app's name, her tray tooltip, and her icon
//! (the tray's and her windows'). Her icon, by one rule (`icon`): his own beside settings.toml,
//! else her pack's (`moods.json` `icon`; not on Windows, where her icon is the app's, as on her
//! shortcut and in Apps & features), else the capri-syn icon (assets/icon, the installer's too).

use crate::platform::{ICON_FILE, TrayIcon};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Her app's name in the tray.
pub const NAME: &str = "capri-syn";

/// The capri-syn icon, 256 px, for what wants pixels (her windows' `_NET_WM_ICON`, a tray drawn
/// from them). On Windows her exe also carries assets/icon/capri-syn.ico as its icon resource
/// (build.rs), and her tray takes its own size from there.
const OWN: &[u8] = include_bytes!("../assets/icon/capri-syn-256.png");

/// An icon file bigger than this a side, or one whose decode wants more than `MAX_ALLOC`, isn't
/// read (logged; the next in line is used).
const MAX_SIDE: u32 = 1024;
const MAX_ALLOC: u64 = 64 << 20;
/// Her icon's picture is kept at most this big a side (a tray wants 16 to 32 px, a window 48).
pub const KEEP_SIDE: u32 = 256;

/// A pack's icon counts where her windows are hers to mark (Linux: `_NET_WM_ICON`). On Windows
/// her icon is the app's: her exe's, her shortcut's, her tray's.
const PACK_ICON: bool = cfg!(not(windows));

/// Her tray tooltip for her status (her tag's word: ONLINE, SLEEPING, NO BRAIN YET,
/// OFFLINE...). Hidden (HIDE), it says so: the tray is the way back.
pub fn tooltip(status: &str, hidden: bool) -> String {
    match if hidden { "HIDDEN" } else { status.trim() } {
        "" => NAME.to_string(),
        s => format!("{NAME} · {s}"),
    }
}

/// Where her icon came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    His,
    Pack,
    App,
}

/// Her icon: its picture (straight-alpha RGBA, at most `KEEP_SIDE` a side; the tray and her
/// windows scale it), and the file it was read from.
pub struct Icon {
    pub img: RgbaImage,
    /// His file or her pack's; None: the capri-syn icon.
    pub file: Option<PathBuf>,
    pub origin: Origin,
    stamp: Stamp,
}

/// Each place her icon may come from, with its file's length and time where there is one. The
/// same stamp: the same icon (nothing is read again).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Stamp(Vec<(PathBuf, Option<(u64, SystemTime)>)>);

impl Stamp {
    fn of(places: &[&Path]) -> Stamp {
        let at = |p: &Path| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        Stamp(places.iter().map(|p| (p.to_path_buf(), at(p))).collect())
    }
}

/// Where his own icon goes: `ICON_FILE` beside settings.toml (`%APPDATA%\synaps-avatar\icon.ico`,
/// `$XDG_CONFIG_HOME/synaps-avatar/icon.png`).
pub fn icon_file() -> PathBuf {
    crate::settings::Settings::path().with_file_name(ICON_FILE)
}

/// Her pack's icon (`moods.json` `icon`, a file in the pack), if it names one.
pub fn pack_icon(pack: &crate::pack::Pack) -> Option<PathBuf> {
    pack.behaviour.icon.as_ref().map(|f| pack.dir.join(f))
}

/// Her icon by the one rule. Read only when a place it may come from changed (path, length,
/// time): at start and on `settings reload` it costs a look at those files, and the same `Arc`
/// comes back while nothing changed.
pub fn icon(pack: Option<&Path>) -> Arc<Icon> {
    cached(&icon_file(), pack)
}

/// The icon last read.
static LAST: Mutex<Option<Arc<Icon>>> = Mutex::new(None);

fn cached(his: &Path, pack: Option<&Path>) -> Arc<Icon> {
    let places: Vec<&Path> = std::iter::once(his).chain(pack).collect();
    let stamp = Stamp::of(&places);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = last.as_ref().filter(|i| i.stamp == stamp) {
        return i.clone();
    }
    let i = Arc::new(read(his, pack, stamp));
    *last = Some(i.clone());
    i
}

/// `icon`, read: his, else her pack's (where it counts), else the capri-syn icon. Each one
/// passed over is logged with why.
fn read(his: &Path, pack: Option<&Path>, stamp: Stamp) -> Icon {
    let app = |stamp| Icon { img: own(), file: None, origin: Origin::App, stamp };
    if his.is_file() {
        match load(his) {
            Ok(img) => {
                if let Some(p) = pack {
                    elog!("synaps-avatar: icon: her pack's {} skipped (his own {} wins)", p.display(), his.display());
                }
                return Icon { img, file: Some(his.to_path_buf()), origin: Origin::His, stamp };
            }
            Err(e) => elog!("synaps-avatar: icon {}: {e} (skipped)", his.display()),
        }
    }
    let Some(p) = pack else { return app(stamp) };
    if !PACK_ICON {
        elog!("synaps-avatar: icon: her pack's {} skipped (here her icon is the app's: the capri-syn icon, or his own {ICON_FILE})", p.display());
        return app(stamp);
    }
    match load(p) {
        Ok(img) => {
            elog!("synaps-avatar: icon: her pack's {}", p.display());
            Icon { img, file: Some(p.to_path_buf()), origin: Origin::Pack, stamp }
        }
        Err(e) => {
            elog!("synaps-avatar: icon: her pack's {}: {e} (skipped; the capri-syn icon)", p.display());
            app(stamp)
        }
    }
}

/// An icon file, by what it holds (a PNG called icon.ico reads too), within `MAX_SIDE` and
/// `MAX_ALLOC`, kept at most `KEEP_SIDE` a side.
fn load(file: &Path) -> image::ImageResult<RgbaImage> {
    let mut r = image::ImageReader::open(file)?.with_guessed_format()?;
    let mut l = image::Limits::default();
    l.max_image_width = Some(MAX_SIDE);
    l.max_image_height = Some(MAX_SIDE);
    l.max_alloc = Some(MAX_ALLOC);
    r.limits(l);
    Ok(fit(r.decode()?.into_rgba8()))
}

/// At most `KEEP_SIDE` a side, its shape kept.
fn fit(img: RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let side = w.max(h);
    if side <= KEEP_SIDE {
        return img;
    }
    let to = |n: u32| ((u64::from(n) * u64::from(KEEP_SIDE)).div_ceil(u64::from(side)) as u32).max(1);
    image::imageops::resize(&img, to(w), to(h), image::imageops::FilterType::Lanczos3)
}

/// The capri-syn icon, decoded.
pub fn own() -> RgbaImage {
    image::load_from_memory_with_format(OWN, image::ImageFormat::Png).map(|i| i.into_rgba8()).unwrap_or_else(|_| RgbaImage::new(1, 1))
}

impl Icon {
    /// Where it came from, for her log.
    pub fn source(&self) -> String {
        match (self.origin, &self.file) {
            (Origin::His, Some(f)) => format!("his {}", f.display()),
            (Origin::Pack, Some(f)) => format!("her pack's {}", f.display()),
            _ => "the capri-syn icon".into(),
        }
    }

    /// For the desktop's tray; its `stamp` is this icon's (the same: nothing to tell).
    pub fn tray(&self) -> TrayIcon {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        self.stamp.hash(&mut h);
        TrayIcon { file: self.file.clone(), stamp: h.finish(), w: self.img.width(), h: self.img.height(), rgba: self.img.as_raw().clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_is_her_name_and_her_status() {
        assert_eq!(tooltip("ONLINE", false), "capri-syn · ONLINE");
        assert_eq!(tooltip("SLEEPING", false), "capri-syn · SLEEPING");
        assert_eq!(tooltip("NO BRAIN YET", false), "capri-syn · NO BRAIN YET");
        assert_eq!(tooltip("OFFLINE", false), "capri-syn · OFFLINE");
        assert_eq!(tooltip(" THINKING ", false), "capri-syn · THINKING");
        assert_eq!(tooltip("", false), "capri-syn", "no status yet: her name alone");
        assert_eq!(tooltip("ONLINE", true), "capri-syn · HIDDEN", "hidden, her tag's word isn't what he sees");
    }

    #[test]
    fn her_moods_read_in_the_tray_as_on_her_tag() {
        use crate::anim::Mood;
        assert_eq!(tooltip(Mood::Idle.default_status(), false), "capri-syn · ONLINE");
        assert_eq!(tooltip(Mood::Sleepy.default_status(), false), "capri-syn · SLEEPING");
        assert_eq!(tooltip(Mood::Away.default_status(), false), "capri-syn · OFFLINE");
        assert_eq!(tooltip(Mood::NeedsYou.default_status(), false), "capri-syn · NEEDS YOU");
    }

    /// assets/icon: her 256 px picture reads, and the .ico her exe carries (build.rs) has every
    /// size a desktop asks for, 16 to 256, the tray's 16/20/24 among them.
    #[test]
    fn the_capri_syn_icon_is_whole() {
        let m = own();
        assert_eq!(m.dimensions(), (256, 256));
        assert_ne!(m.get_pixel(128, 128).0[3], 0, "not blank");
        let ico = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/capri-syn.ico")).unwrap();
        let u16_at = |i: usize| u16::from_le_bytes([ico[i], ico[i + 1]]);
        assert_eq!((u16_at(0), u16_at(2)), (0, 1), "an icon");
        // The directory's width byte: 0 is 256.
        let sizes: Vec<u32> = (0..usize::from(u16_at(4)))
            .map(|k| match ico[6 + 16 * k] {
                0 => 256,
                w => u32::from(w),
            })
            .collect();
        assert_eq!(sizes, [16, 20, 24, 32, 40, 48, 64, 128, 256]);
        assert!(image::load_from_memory(&ico).is_ok(), "it reads");
    }

    /// Beside settings.toml, in the OS's icon format.
    #[test]
    fn his_icon_goes_beside_settings() {
        let f = icon_file();
        assert_eq!(f.parent(), crate::settings::Settings::path().parent());
        assert_eq!(f.file_name().and_then(|n| n.to_str()), Some(if cfg!(windows) { "icon.ico" } else { "icon.png" }));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("capri-icon-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn read_now(his: &Path, pack: Option<&Path>) -> Icon {
        let places: Vec<&Path> = std::iter::once(his).chain(pack).collect();
        read(his, pack, Stamp::of(&places))
    }

    /// His own wins; then her pack's (not on Windows); none, or one that doesn't read, is the
    /// capri-syn icon.
    #[test]
    fn his_icon_then_her_packs_then_the_app_icon() {
        let d = scratch("order");
        let his = d.join(ICON_FILE);
        let pack = d.join("pack-icon.png");
        let i = read_now(&his, None);
        assert!(i.file.is_none() && i.origin == Origin::App && i.img == own());

        let blue = RgbaImage::from_pixel(48, 48, image::Rgba([0x20, 0x80, 0xff, 0xff]));
        let green = RgbaImage::from_pixel(40, 40, image::Rgba([0x20, 0xff, 0x40, 0xff]));
        green.save(&pack).unwrap();
        let i = read_now(&his, Some(&pack));
        if PACK_ICON {
            assert_eq!((i.origin, i.file.as_deref(), i.img.dimensions()), (Origin::Pack, Some(pack.as_path()), (40, 40)));
        } else {
            assert_eq!((i.origin, i.file.as_deref()), (Origin::App, None), "Windows: the app's icon, the pack's skipped");
        }

        // Saved as its name says (icon.ico: an ICO; icon.png: a PNG).
        blue.save(&his).unwrap();
        let i = read_now(&his, Some(&pack));
        assert_eq!((i.origin, i.file.as_deref(), i.img.dimensions(), i.img.get_pixel(5, 5).0), (Origin::His, Some(his.as_path()), (48, 48), [0x20, 0x80, 0xff, 0xff]));

        // A PNG under the other name reads too; garbage is passed over, logged.
        blue.save_with_format(&his, image::ImageFormat::Png).unwrap();
        assert_eq!(read_now(&his, None).img.dimensions(), (48, 48));
        std::fs::write(&his, b"not an icon").unwrap();
        let i = read_now(&his, None);
        assert!(i.file.is_none() && i.img == own());
        std::fs::write(&pack, b"not an icon either").unwrap();
        assert_eq!(read_now(&his, Some(&pack)).origin, Origin::App);
        std::fs::remove_dir_all(d).ok();
    }

    /// A huge file isn't decoded; a big one is kept at `KEEP_SIDE`, its shape kept.
    #[test]
    fn his_icon_is_bounded() {
        let d = scratch("bounds");
        let his = d.join(ICON_FILE);
        let wide = RgbaImage::from_pixel(600, 300, image::Rgba([0xff, 0x14, 0x38, 0xff]));
        wide.save_with_format(&his, image::ImageFormat::Png).unwrap();
        let i = read_now(&his, None);
        assert_eq!((i.origin, i.img.dimensions()), (Origin::His, (256, 128)));
        let huge = RgbaImage::new(MAX_SIDE + 1, 8);
        huge.save_with_format(&his, image::ImageFormat::Png).unwrap();
        let i = read_now(&his, None);
        assert_eq!(i.origin, Origin::App, "over the limit: the capri-syn icon");
        std::fs::remove_dir_all(d).ok();
    }

    /// Read again only when a file it may come from changed: the same `Arc` (and the same tray
    /// stamp) until then.
    #[test]
    fn her_icon_is_read_again_only_when_it_changed() {
        let d = scratch("stamp");
        let his = d.join(ICON_FILE);
        let a = cached(&his, None);
        let b = cached(&his, None);
        assert!(Arc::ptr_eq(&a, &b) && a.tray().stamp == b.tray().stamp);
        RgbaImage::from_pixel(32, 32, image::Rgba([1, 2, 3, 255])).save_with_format(&his, image::ImageFormat::Png).unwrap();
        let c = cached(&his, None);
        assert!(!Arc::ptr_eq(&a, &c) && c.origin == Origin::His && c.tray().stamp != a.tray().stamp);
        assert!(Arc::ptr_eq(&c, &cached(&his, None)));
        // Another length: read again.
        RgbaImage::from_pixel(33, 33, image::Rgba([1, 2, 3, 255])).save_with_format(&his, image::ImageFormat::Png).unwrap();
        assert_eq!(cached(&his, None).img.dimensions(), (33, 33));
        std::fs::remove_file(&his).unwrap();
        assert_eq!(cached(&his, None).origin, Origin::App);
        std::fs::remove_dir_all(d).ok();
    }
}
