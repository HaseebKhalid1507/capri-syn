//! Only explicit layer_ctl bounds requests override the configure geometry echo.
//! Configured geometry is already clamped positive before this policy runs.

pub(super) fn configure_request(requested: Option<(u32, u32)>, configured: (i32, i32)) -> (i32, i32) {
    requested.map_or(configured, |(w, h)| (w as i32, h as i32))
}

/// A late configure must not shrink an already-resized drawable while the geometry
/// echo still asks for the newer dimensions (Hyprland would stretch that old buffer).
pub(super) fn configure_buffer(requested: Option<(u32, u32)>, configured: Option<(u32, u32)>) -> Option<(u32, u32)> {
    requested.or(configured)
}

/// A later PlatformWindow::resize supersedes a managed bounds request, but must not
/// opt an unrelated layer/xdg window into the new echo policy.
pub(super) fn resize_request(requested: Option<(u32, u32)>, resized: (i32, i32)) -> Option<(u32, u32)> {
    requested.map(|_| (resized.0 as u32, resized.1 as u32))
}

#[cfg(test)]
mod tests {
    use super::{configure_buffer, configure_request, resize_request};

    #[test]
    fn old_configure_does_not_undo_a_measured_size_request() {
        assert_eq!(configure_request(Some((780, 436)), (780, 120)), (780, 436));
        assert_eq!(configure_request(Some((780, 436)), (780, 436)), (780, 436));
    }

    #[test]
    fn old_configure_cannot_restore_a_stretched_buffer() {
        assert_eq!(configure_buffer(Some((780, 436)), Some((780, 120))), Some((780, 436)));
        assert_eq!(configure_buffer(Some((620, 228)), Some((780, 436))), Some((620, 228)));
        assert_eq!(configure_buffer(None, Some((780, 120))), Some((780, 120)));
    }

    #[test]
    fn latest_request_can_shrink_as_well_as_grow() {
        assert_eq!(configure_request(Some((780, 228)), (780, 436)), (780, 228));
    }

    #[test]
    fn window_resize_replaces_the_last_managed_request() {
        let requested = resize_request(Some((780, 436)), (640, 240));
        assert_eq!(requested, Some((640, 240)));
        assert_eq!(configure_request(requested, (780, 436)), (640, 240));
        assert_eq!(configure_request(requested, (640, 240)), (640, 240));
    }

    #[test]
    fn window_resize_does_not_opt_in_other_surfaces() {
        assert_eq!(resize_request(None, (640, 240)), None);
    }

    #[test]
    fn unmanaged_geometry_echo_is_unchanged() {
        assert_eq!(configure_request(None, (780, 120)), (780, 120));
    }
}
