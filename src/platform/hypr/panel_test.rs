//! Real Line/Settings, not a stand-in of a requested size. No pack, daemon or login needed.
//! Run only inside tools/wlrig (the guard is shared with contract_hypr).

use super::super::{Os, Role};
use super::testkit::{hyprctl, require_rig};
use crate::line::{Ask, Line, Page, Prefs, Row, Snap, Who};
use gpui_kit::{App, AppContext as _, Bounds, Entity, WindowKind, point, px, size};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// Hyprland reports layer dimensions in logical pixels, as does GPUI's viewport.
fn panel_height() -> Option<f32> {
    let layers: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "layers"])).ok()?;
    for monitor in layers.as_object()?.values() {
        for (level, surfaces) in monitor["levels"].as_object()? {
            for surface in surfaces.as_array()? {
                if surface["namespace"] == "capri-line" {
                    assert_eq!(level, "3", "the Line must stay an overlay, never a tile");
                    return surface["h"].as_f64().map(|h| h as f32);
                }
            }
        }
    }
    None
}

#[test]
#[ignore = "needs tools/wlrig/run.sh env SA_CONTRACT_HYPR=1 <test binary> --ignored --test-threads 1 contract_hypr_panels"]
fn contract_hypr_panels() {
    require_rig();
    let (desk, stage) = super::connect().expect("the rig's Hyprland");
    let os = Rc::new(Os::new(desk, Box::new(stage)));
    let finished = Rc::new(Cell::new(false));
    let done = finished.clone();
    gpui_kit::application().run(move |cx: &mut App| {
        // Reopen from scratch after closing the first panel; don't quit with its last window.
        cx.set_quit_mode(gpui_kit::QuitMode::Explicit);
        crate::line::kit_init(cx);
        cx.spawn(async move |cx| {
            // Fresh chat, a page swap in each direction, then Settings opened directly.
            // Both used to remain 120 px tall; chat's field / Settings' bottom rows were clipped.
            for first in [Page::Chat, Page::Settings] {
                let measured = Rc::new(Cell::new(0.0f32));
                let built: Rc<RefCell<Option<Entity<Line>>>> = Rc::default();
                let view_out = built.clone();
                let os_open = os.clone();
                let measured_open = measured.clone();
                let handle = cx.update(move |cx| {
                    let bounds = Bounds::new(point(px(100.0), px(100.0)), size(px(780.0), px(120.0)));
                    let options = os_open.window_options(Role::Line, bounds, false);
                    assert!(matches!(options.kind, WindowKind::LayerShell(_)));
                    cx.open_window(options, move |window, cx| {
                        let id = os_open.adopt(Role::Line, window).expect("adopt Line");
                        let sf = window.scale_factor();
                        let on_ask = Box::new(move |ask, _: &mut App| {
                            if let Ask::Height(h) = ask {
                                measured_open.set(h);
                                // The same measured-height -> Stage::place path as line_follow.
                                // No Window::resize: the configure must resize the real GPUI window.
                                os_open
                                    .place(id, (100.0 * sf) as i32, (100.0 * sf) as i32, (780.0 * sf) as i32, (h * sf).ceil() as i32)
                                    .map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        });
                        let rows = (0..4)
                            .map(|i| Row {
                                who: if i % 2 == 0 { Who::You } else { Who::Her },
                                text: format!("Panel height regression, row {i}"),
                                detail: String::new(),
                                streaming: false,
                            })
                            .collect();
                        let snap = Snap {
                            rows,
                            status: "ONLINE".into(),
                            prefs: Some(Prefs {
                                brain: true,
                                voice: true,
                                wake: true,
                                about: "CAPRI contract".into(),
                                ..Prefs::default()
                            }),
                            ..Snap::default()
                        };
                        let view = cx.new(|cx| {
                            Line::new(
                                snap,
                                "a draft in the input field".into(),
                                Vec::new(),
                                "Capri".into(),
                                "DejaVu Sans Mono".into(),
                                crate::theme::Pal::SHARD,
                                400.0,
                                None,
                                first,
                                on_ask,
                                window,
                                cx,
                            )
                        });
                        *view_out.borrow_mut() = Some(view.clone());
                        cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
                    })
                    .expect("open Line")
                });
                let view = built.borrow_mut().take().expect("the real Line");
                for page in [first, Page::Settings, Page::Chat] {
                    handle
                        .update(cx, |_, window, cx| view.update(cx, |line, cx| line.show(page, window, cx)))
                        .expect("page swap");
                    let minimum = if page.settings() { 300.0 } else { 180.0 };
                    let mut settled = false;
                    let mut last = String::new();
                    // Keep pumping Wayland: blocking HDriver::until would starve configure events.
                    for _ in 0..100 {
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                        let (viewport, state) = handle
                            .update(cx, |_, window, cx| (f32::from(window.viewport_size().height), view.read(cx).state_json(cx)))
                            .expect("read viewport");
                        let layer = panel_height().unwrap_or(0.0);
                        let content = measured.get();
                        // spot = [x, distance from viewport bottom, w, h]. The cog shares the
                        // field row; Settings' CHAT button is below every Settings row.
                        let spot = state["spot"].as_array();
                        let visible = spot.is_some_and(|s| {
                            let bottom = s[1].as_f64().unwrap_or(-1.0) as f32;
                            let height = s[3].as_f64().unwrap_or(0.0) as f32;
                            bottom >= 0.0 && height > 0.0 && bottom + height <= viewport
                        });
                        last = format!("page={} layer={layer} viewport={viewport} measured={content} spot={:?}", page.key(), state["spot"]);
                        if content >= minimum && layer >= minimum && viewport >= minimum && (layer - content).abs() <= 2.0 && (viewport - layer).abs() <= 1.0 && visible {
                            settled = true;
                            break;
                        }
                    }
                    assert!(settled, "panel did not grow to its content: {last}");
                    eprintln!("contract_hypr_panels: {last}");
                    let clients: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "clients"])).expect("clients");
                    assert!(
                        !clients.as_array().unwrap().iter().any(|c| c["class"] == "synaps-avatar" || c["class"] == "capri-line"),
                        "panel must not become an xdg toplevel/tile"
                    );
                    // A later frame/configure must not undo the request.
                    cx.background_executor().timer(Duration::from_millis(150)).await;
                    assert!(panel_height().unwrap_or(0.0) >= minimum);
                }
                handle.update(cx, |_, window, _| window.remove_window()).expect("close panel");
                cx.background_executor().timer(Duration::from_millis(100)).await;
            }
            done.set(true);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    assert!(finished.get(), "the panel contract ran to completion");
}
