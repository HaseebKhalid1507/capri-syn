//! Only explicit layer_ctl bounds requests override the configure geometry echo.
//! The actual viewport still follows the compositor's configure, including 0x0 = no change.

pub(super) fn configure_request(
    requested: Option<(u32, u32)>,
    configured: (i32, i32),
) -> (i32, i32) {
    requested.map_or(configured, |(w, h)| (w as i32, h as i32))
}

#[cfg(test)]
mod tests {
    use super::configure_request;

    #[test]
    fn old_configure_does_not_undo_a_measured_size_request() {
        assert_eq!(configure_request(Some((780, 436)), (780, 120)), (780, 436));
        assert_eq!(configure_request(Some((780, 436)), (0, 0)), (780, 436));
        assert_eq!(configure_request(Some((780, 436)), (780, 436)), (780, 436));
    }

    #[test]
    fn latest_request_can_shrink_as_well_as_grow() {
        assert_eq!(configure_request(Some((780, 228)), (780, 436)), (780, 228));
    }

    #[test]
    fn unmanaged_geometry_echo_is_unchanged() {
        assert_eq!(configure_request(None, (780, 120)), (780, 120));
        assert_eq!(configure_request(None, (0, 0)), (0, 0));
    }
}
