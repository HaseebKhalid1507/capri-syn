//! Stage::place uses device pixels; layer-shell set_size uses positive logical pixels.
//! Zero means "let the compositor choose", not an empty panel (and requires opposing anchors).

pub(super) fn layer_size(width: i32, height: i32, scale: f32) -> (u32, u32) {
    let logical = |v: i32| (v as f32 / scale - 1e-3).ceil().max(1.0) as u32;
    (logical(width), logical(height))
}

/// Only the Line opts into live dimensions. Reader follows and effect-host placement
/// retain margin-only behavior; effects already size themselves through Window::resize.
pub(super) fn placed_size(namespace: &str, width: i32, height: i32, scale: f32) -> Option<(u32, u32)> {
    (namespace == "capri-line").then(|| layer_size(width, height, scale))
}

#[cfg(test)]
mod tests {
    use super::{layer_size, placed_size};

    #[test]
    fn measured_panel_replaces_the_bootstrap_height() {
        assert_eq!(layer_size(780, 120, 1.0), (780, 120));
        // Chat grows after layout, Settings grows again; neither is stuck at the first request.
        assert_eq!(layer_size(780, 228, 1.0), (780, 228));
        assert_eq!(layer_size(780, 436, 1.0), (780, 436));
        assert_eq!(layer_size(780, 228, 1.0), (780, 228));
    }

    #[test]
    fn device_pixels_are_scaled_once_and_never_round_below_the_content() {
        assert_eq!(layer_size(1560, 872, 2.0), (780, 436));
        assert_eq!(layer_size(1170, 654, 1.5), (780, 436));
        assert_eq!(layer_size(975, 546, 1.25), (780, 437));
        assert_eq!(layer_size(1, 1, 2.0), (1, 1));
    }

    #[test]
    fn float_noise_does_not_add_a_logical_pixel() {
        // The output scale can be just below the exact ratio (fractional-scale/float noise).
        assert_eq!(layer_size(1170, 654, 1.5 - f32::EPSILON), (780, 436));
        // A real fraction still rounds up.
        assert_eq!(layer_size(1171, 655, 1.5), (781, 437));
    }

    #[test]
    fn only_the_line_gets_live_dimensions_in_place() {
        assert_eq!(placed_size("capri-line", 620, 436, 1.0), Some((620, 436)));
        for namespace in ["capri-reader", "capri-fx", "capri-body", "capri-chip", "capri-intro"] {
            assert_eq!(placed_size(namespace, 620, 436, 1.0), None, "{namespace}");
        }
    }

    #[test]
    fn never_asks_the_compositor_to_choose_a_dimension() {
        assert_eq!(layer_size(0, 0, 1.0), (1, 1));
        assert_eq!(layer_size(-40, -90, 1.5), (1, 1));
    }
}
