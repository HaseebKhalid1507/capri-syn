//! The measured Line height -> device height path, shared by line_follow and its rig test.
//! Keep the original truncate-before-clamp behavior on every backend.

pub(crate) fn device_height(measured: f32, scale: f32, area_height: i32) -> i32 {
    ((measured * scale) as i32).min(area_height - (36.0 * scale) as i32).max((60.0 * scale) as i32)
}

#[cfg(test)]
mod tests {
    use super::device_height;

    #[test]
    fn truncates_in_device_pixels_before_clamping() {
        assert_eq!(device_height(228.9, 1.0, 1600), 228);
        assert_eq!(device_height(228.9, 1.25, 2000), 286);
        assert_eq!(device_height(436.75, 1.5, 2400), 655);
    }

    #[test]
    fn keeps_the_original_work_area_and_minimum_height_limits() {
        assert_eq!(device_height(2000.0, 1.0, 1600), 1564);
        assert_eq!(device_height(2000.0, 1.5, 2400), 2346);
        assert_eq!(device_height(0.0, 1.25, 2000), 75);
        assert_eq!(device_height(2000.0, 1.0, 50), 60);
    }
}
