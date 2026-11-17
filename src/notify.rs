//! Desktop notifications through the platform's service (`platform::notify`), sent off the
//! main thread: fire and forget, a failure is logged.

use std::path::PathBuf;

/// Notify: `summary`, `body`, `image` (a file) as the notification's image.
pub fn send(summary: String, body: String, image: Option<PathBuf>) {
    let spawned = std::thread::Builder::new().name("notify".into()).spawn(move || match crate::platform::notify(&summary, &body, image) {
        Ok(id) => elog!("synaps-avatar: notification {id}: {summary}"),
        Err(e) => elog!("synaps-avatar: notification: {e:#}"),
    });
    if let Err(e) = spawned {
        elog!("synaps-avatar: notification: {e}");
    }
}
