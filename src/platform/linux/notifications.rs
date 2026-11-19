//! Desktop notifications on Linux: org.freedesktop.Notifications on the session bus.

use std::collections::HashMap;
use std::path::PathBuf;
use zbus::zvariant::Value;

/// Show one now (blocking: the caller's thread). Ok: the notification's id.
pub fn notify(summary: &str, body: &str, image: Option<PathBuf>) -> anyhow::Result<u32> {
    notify_as(summary, body, image, 1)
}

/// `notify` at the spec's `urgency`: 0 low, 1 normal, 2 critical (it stays until he closes it).
pub fn notify_as(summary: &str, body: &str, image: Option<PathBuf>, urgency: u8) -> anyhow::Result<u32> {
    let c = zbus::blocking::Connection::session()?;
    // The spec's image-path: a file:// URI (an absolute path).
    let path = image.filter(|p| p.is_absolute()).map(|p| format!("file://{}", p.to_string_lossy())).unwrap_or_default();
    let mut hints: HashMap<&str, Value> = HashMap::new();
    if !path.is_empty() {
        hints.insert("image-path", Value::from(path.clone()));
    }
    hints.insert("urgency", Value::U8(urgency));
    let actions: Vec<&str> = Vec::new();
    let r = c.call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &("synaps-avatar", 0u32, path.as_str(), summary, body, actions, hints, -1i32),
    )?;
    Ok(r.body().deserialize::<u32>()?)
}
