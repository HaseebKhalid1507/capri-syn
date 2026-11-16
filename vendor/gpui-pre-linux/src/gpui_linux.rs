#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;

pub use linux::{current_platform, linux_platform};
/// synaps-avatar patch (PATCHES.md): runtime layer-shell control (margins, anchor, input
/// region, keyboard interactivity, layer).
#[cfg(feature = "wayland")]
pub use linux::layer_ctl;
