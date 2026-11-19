//! What she shows the desktop as, from one place: her app's name, her tray tooltip, and her icon
//! (the tray's and her windows'). Her icon is the capri-syn icon (assets/icon, the installer's
//! too), unless he puts his own beside settings.toml (`icon`: the one rule, for whatever else
//! wants her icon later).

use crate::platform::{ICON_FILE, TrayIcon};
use image::RgbaImage;
use std::path::{Path, PathBuf};

/// Her app's name in the tray.
pub const NAME: &str = "capri-syn";

/// The capri-syn icon, 256 px, for what wants pixels (her windows' `_NET_WM_ICON`, a tray drawn
/// from them). On Windows her exe also carries assets/icon/capri-syn.ico as its icon resource
/// (build.rs), and her tray takes its own size from there.
const OWN: &[u8] = include_bytes!("../assets/icon/capri-syn-256.png");

/// Her tray tooltip for her status (her tag's word: ONLINE, SLEEPING, NO BRAIN YET,
/// OFFLINE...).
pub fn tooltip(status: &str) -> String {
    match status.trim() {
        "" => NAME.to_string(),
        s => format!("{NAME} · {s}"),
    }
}

/// Her icon, straight-alpha RGBA as its source has it (the tray and her windows scale it), and
/// his file when it is his.
pub struct Icon {
    pub img: RgbaImage,
    pub file: Option<PathBuf>,
}

/// Where his own icon goes: `ICON_FILE` beside settings.toml (`%APPDATA%\synaps-avatar\icon.ico`,
/// `$XDG_CONFIG_HOME/synaps-avatar/icon.png`).
pub fn icon_file() -> PathBuf {
    crate::settings::Settings::path().with_file_name(ICON_FILE)
}

/// Her icon, by the one rule: his own (`icon_file`) if it is there and reads, else the capri-syn
/// icon (`own`). Read when asked: at start and on `settings reload`.
pub fn icon() -> Icon {
    icon_from(&icon_file())
}

/// `icon` with his own at `file`.
fn icon_from(file: &Path) -> Icon {
    if file.is_file() {
        // By what it holds, not its name (a PNG called icon.ico reads too).
        match image::ImageReader::open(file).and_then(|r| r.with_guessed_format()).map_err(image::ImageError::IoError).and_then(|r| r.decode()) {
            Ok(i) => return Icon { img: i.into_rgba8(), file: Some(file.to_path_buf()) },
            Err(e) => elog!("synaps-avatar: icon {}: {e} (the capri-syn icon instead)", file.display()),
        }
    }
    Icon { img: own(), file: None }
}

/// The capri-syn icon, decoded.
pub fn own() -> RgbaImage {
    image::load_from_memory_with_format(OWN, image::ImageFormat::Png).map(|i| i.into_rgba8()).unwrap_or_else(|_| RgbaImage::new(1, 1))
}

impl Icon {
    /// Where it came from, for her log: his file, or hers.
    pub fn source(&self) -> String {
        self.file.as_ref().map_or_else(|| "the capri-syn icon".to_string(), |p| p.display().to_string())
    }

    /// For the desktop's tray.
    pub fn tray(&self) -> TrayIcon {
        TrayIcon { file: self.file.clone(), w: self.img.width(), h: self.img.height(), rgba: self.img.as_raw().clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_is_her_name_and_her_status() {
        assert_eq!(tooltip("ONLINE"), "capri-syn · ONLINE");
        assert_eq!(tooltip("SLEEPING"), "capri-syn · SLEEPING");
        assert_eq!(tooltip("NO BRAIN YET"), "capri-syn · NO BRAIN YET");
        assert_eq!(tooltip("OFFLINE"), "capri-syn · OFFLINE");
        assert_eq!(tooltip(" THINKING "), "capri-syn · THINKING");
        assert_eq!(tooltip(""), "capri-syn", "no status yet: her name alone");
    }

    #[test]
    fn her_moods_read_in_the_tray_as_on_her_tag() {
        use crate::anim::Mood;
        assert_eq!(tooltip(Mood::Idle.default_status()), "capri-syn · ONLINE");
        assert_eq!(tooltip(Mood::Sleepy.default_status()), "capri-syn · SLEEPING");
        assert_eq!(tooltip(Mood::Away.default_status()), "capri-syn · OFFLINE");
        assert_eq!(tooltip(Mood::NeedsYou.default_status()), "capri-syn · NEEDS YOU");
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

    /// His own icon wins; none, or one that doesn't read, is the capri-syn icon.
    #[test]
    fn his_icon_wins_where_it_reads() {
        let d = std::env::temp_dir().join(format!("capri-icon-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join(ICON_FILE);
        std::fs::remove_file(&file).ok();
        let i = icon_from(&file);
        assert!(i.file.is_none() && i.img == own());

        let art = RgbaImage::from_pixel(48, 48, image::Rgba([0x20, 0x80, 0xff, 0xff]));
        // Saved as its name says (icon.ico: an ICO; icon.png: a PNG).
        art.save(&file).unwrap();
        let i = icon_from(&file);
        assert_eq!((i.file.as_deref(), i.img.dimensions(), i.img.get_pixel(5, 5).0), (Some(file.as_path()), (48, 48), [0x20, 0x80, 0xff, 0xff]));

        // A PNG under the other name reads too; garbage is hers, logged.
        art.save_with_format(&file, image::ImageFormat::Png).unwrap();
        assert_eq!(icon_from(&file).img.dimensions(), (48, 48));
        std::fs::write(&file, b"not an icon").unwrap();
        let i = icon_from(&file);
        assert!(i.file.is_none() && i.img == own());
        std::fs::remove_dir_all(d).ok();
    }
}
