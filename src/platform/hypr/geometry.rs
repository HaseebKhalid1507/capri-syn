//! Stage::place uses device pixels; layer-shell set_size uses positive logical pixels.
//! Zero means "let the compositor choose", not an empty panel (and requires opposing anchors).

pub(super) fn layer_size(width: i32, height: i32, scale: f32) -> (u32, u32) {
    let logical = |v: i32| (v as f32 / scale).ceil().max(1.0) as u32;
    (logical(width), logical(height))
}

#[cfg(test)]
mod tests {
    use super::layer_size;

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
    fn never_asks_the_compositor_to_choose_a_dimension() {
        assert_eq!(layer_size(0, 0, 1.0), (1, 1));
        assert_eq!(layer_size(-40, -90, 1.5), (1, 1));
    }
}
