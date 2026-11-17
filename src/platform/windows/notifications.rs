//! Notifications on Windows: toasts. A toast needs an AppUserModelID that Windows knows. She runs
//! unpackaged, so she registers hers under `HKCU\Software\Classes\AppUserModelId\<id>` (a display
//! name; no Start-menu shortcut, no admin), as GPUI's own toasts do. A click on one does
//! nothing yet (that needs a COM activator). docs/WINDOWS.md says what a package provides
//! instead.

use ::windows::Data::Xml::Dom::XmlDocument;
use ::windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use ::windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW};
use ::windows::core::{HSTRING, PCWSTR, w};
use anyhow::{Context as _, Result, anyhow};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// Her AppUserModelID, and the name a toast shows it under (the app name her Linux
/// notifications carry).
const AUMID: &str = "Synaps.Avatar";
const DISPLAY_NAME: &str = "synaps-avatar";

/// Show one now (blocking: the caller's thread). Ok: the toast's id (its tag).
pub fn notify(summary: &str, body: &str, image: Option<PathBuf>) -> Result<u32> {
    static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    REGISTERED.get_or_init(|| register(AUMID, DISPLAY_NAME).map_err(|e| format!("{e:#}"))).clone().map_err(|e| anyhow!("toast: registering {AUMID}: {e}"))?;
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    show(id, summary, body, image.as_deref()).context("toast")?;
    Ok(id)
}

fn show(id: u32, summary: &str, body: &str, image: Option<&Path>) -> ::windows::core::Result<()> {
    // WinRT on this (short-lived) thread.
    let _ = unsafe { ::windows::Win32::System::Com::CoInitializeEx(None, ::windows::Win32::System::Com::COINIT_MULTITHREADED) };
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast_xml(summary, body, image)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(id.to_string()))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
}

/// `HKCU\Software\Classes\AppUserModelId\<aumid>`, `DisplayName` = `name`.
fn register(aumid: &str, name: &str) -> Result<()> {
    let path = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{aumid}"));
    let mut key = HKEY::default();
    unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &path, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None) }.ok()?;
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let bytes: Vec<u8> = wide.iter().flat_map(|c| c.to_le_bytes()).collect();
    let set = unsafe { RegSetValueExW(key, w!("DisplayName"), None, REG_SZ, Some(&bytes)) }.ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    set.context("DisplayName")
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// `C:\a b\c.png` → `file:///C:/a%20b/c.png`.
fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('%', "%25").replace(' ', "%20").replace('#', "%23").replace('\\', "/");
    format!("file:///{}", s.trim_start_matches('/'))
}

/// The toast: her summary and body (generic template), the image (a file) as its logo.
fn toast_xml(summary: &str, body: &str, image: Option<&Path>) -> String {
    let img = image
        .filter(|p| p.is_absolute())
        .map(|p| format!(r#"<image placement="appLogoOverride" src="{}"/>"#, esc(&file_uri(p))))
        .unwrap_or_default();
    format!(r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text>{img}</binding></visual></toast>"#, esc(summary), esc(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_escapes_what_she_says() {
        let x = toast_xml("Capri <3", "\"done\" & 'dusted'", None);
        assert_eq!(
            x,
            r#"<toast><visual><binding template="ToastGeneric"><text>Capri &lt;3</text><text>&quot;done&quot; &amp; &apos;dusted&apos;</text></binding></visual></toast>"#
        );
    }

    #[test]
    fn the_image_is_a_file_uri_logo() {
        let x = toast_xml("s", "b", Some(Path::new(r"C:\Users\me\my face#1.png")));
        assert!(x.contains(r#"<image placement="appLogoOverride" src="file:///C:/Users/me/my%20face%231.png"/>"#), "{x}");
        assert!(!toast_xml("s", "b", Some(Path::new("face.png"))).contains("<image"), "a relative path is no image");
    }
}
