//! Hyprland root coordinates AND the core's sf=1 dimensions are logical pixels.
//! GPUI alone scales the drawable buffer. Dividing by the output scale here shrinks it twice.

pub(super) fn layer_size(width: i32, height: i32) -> (u32, u32) {
    (width.max(1) as u32, height.max(1) as u32)
}

/// Root logical position to output-relative logical layer margins, at any buffer scale.
pub(super) fn margins(x: i32, y: i32, output_x: i32, output_y: i32) -> [i32; 4] {
    [y - output_y, 0, 0, x - output_x]
}

/// Only the Line opts into live dimensions. Reader follows and effect-host placement
/// retain margin-only behavior; effects already size themselves through Window::resize.
pub(super) fn placed_size(namespace: &str, width: i32, height: i32) -> Option<(u32, u32)> {
    (namespace == "capri-line").then(|| layer_size(width, height))
}

/// Fit all four edges inside the output's usable rectangle, including a panel taller
/// than a newly selected output. Values are in the backend's root coordinate space.
pub(super) fn fit_bounds(bounds: [i32; 4], area: [i32; 4]) -> [i32; 4] {
    let [x, y, w, h] = bounds;
    let [ax, ay, aw, ah] = area;
    let (w, h) = (w.clamp(1, aw.max(1)), h.clamp(1, ah.max(1)));
    [x.clamp(ax, ax + aw.max(1) - w), y.clamp(ay, ay + ah.max(1) - h), w, h]
}

/// Immutable layer output differs from the active body host: moving margins cannot fix it.
pub(super) fn needs_reopen(surface: &str, body: Option<&str>) -> bool {
    body.is_some_and(|body| surface != body)
}

#[cfg(test)]
mod tests {
    use super::{fit_bounds, layer_size, margins, needs_reopen, placed_size};

    #[test]
    fn live_report_fits_the_body_output_not_the_focused_output() {
        assert_eq!(fit_bounds([1169, 946, 820, 669], [0, 0, 2560, 1600]), [1169, 931, 820, 669]);
        // Reserved bars, a shorter second output, then an output shorter than the content.
        assert_eq!(fit_bounds([3729, 946, 820, 669], [2560, 32, 2560, 1408]), [3729, 771, 820, 669]);
        assert_eq!(fit_bounds([3729, 946, 820, 2000], [2560, 32, 2560, 1408]), [3729, 32, 820, 1408]);
        assert_eq!(fit_bounds([-500, -500, 800, 900], [-2560, -1440, 2560, 1400]), [-800, -940, 800, 900]);
    }

    #[test]
    fn output_switch_requires_recreation_not_new_margins() {
        assert!(!needs_reopen("eDP-1", Some("eDP-1")));
        assert!(needs_reopen("eDP-1", Some("DP-2")));
        assert!(needs_reopen("DP-2", Some("eDP-1")));
        assert!(!needs_reopen("eDP-1", None));
    }

    #[test]
    fn measured_panel_replaces_the_bootstrap_height() {
        assert_eq!(layer_size(780, 120), (780, 120));
        // Chat grows after layout, Settings grows again; neither is stuck at the first request.
        assert_eq!(layer_size(780, 228), (780, 228));
        assert_eq!(layer_size(780, 436), (780, 436));
        assert_eq!(layer_size(780, 228), (780, 228));
    }

    #[test]
    fn fractional_output_does_not_scale_logical_requests_again() {
        // Same 780x436 layout on 100%, 125%, 150%, 200%. Only GPUI's buffer scales.
        assert_eq!(layer_size(780, 436), (780, 436));
        // 2560x1600 @1.25: Hyprland logical bounds, NOT GPUI's 1280x800.
        assert_eq!(fit_bounds([1500, 900, 780, 436], [0, 0, 2048, 1280]), [1268, 844, 780, 436]);
    }

    #[test]
    fn margins_use_logical_origins_even_on_fractional_or_negative_outputs() {
        assert_eq!(margins(6620, 750, 5120, 0), [750, 0, 0, 1500]);
        assert_eq!(margins(-1800, -900, -2048, -1280), [380, 0, 0, 248]);
    }

    #[test]
    fn only_the_line_gets_live_dimensions_in_place() {
        assert_eq!(placed_size("capri-line", 620, 436), Some((620, 436)));
        for namespace in ["capri-reader", "capri-fx", "capri-body", "capri-chip", "capri-intro"] {
            assert_eq!(placed_size(namespace, 620, 436), None, "{namespace}");
        }
    }

    #[test]
    fn never_asks_the_compositor_to_choose_a_dimension() {
        assert_eq!(layer_size(0, 0), (1, 1));
        assert_eq!(layer_size(-40, -90), (1, 1));
    }
}
