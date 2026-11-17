//! Desktop notifications (org.freedesktop.Notifications on the session bus), sent off the main
//! thread: fire and forget, a failure is logged.

use std::collections::HashMap;
use std::path::PathBuf;
use zbus::zvariant::Value;

/// Notify: `summary`, `body`, `image` (a file) as the notification's image.
pub fn send(summary: String, body: String, image: Option<PathBuf>) {
    let spawned = std::thread::Builder::new().name("notify".into()).spawn(move || match send_now(&summary, &body, image) {
        Ok(id) => elog!("synaps-avatar: notification {id}: {summary}"),
        Err(e) => elog!("synaps-avatar: notification: {e:#}"),
    });
    if let Err(e) = spawned {
        elog!("synaps-avatar: notification: {e}");
    }
}

fn send_now(summary: &str, body: &str, image: Option<PathBuf>) -> anyhow::Result<u32> {
    let c = zbus::blocking::Connection::session()?;
    // The spec's image-path: a file:// URI (an absolute path).
    let path = image.filter(|p| p.is_absolute()).map(|p| format!("file://{}", p.to_string_lossy())).unwrap_or_default();
    let mut hints: HashMap<&str, Value> = HashMap::new();
    if !path.is_empty() {
        hints.insert("image-path", Value::from(path.clone()));
    }
    hints.insert("urgency", Value::U8(1));
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
