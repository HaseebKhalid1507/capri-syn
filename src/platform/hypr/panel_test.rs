//! Real Line/Settings, not a stand-in of a requested size. No pack, daemon or login needed.
//! Run only inside tools/wlrig (the guard is shared with contract_hypr).

use super::super::{Os, Role, WinId, WinRect};
use super::testkit::{hyprctl, require_rig};
use crate::beside::{self, Panel};
use crate::line::{Ask, Line, Page, Prefs, Row, Snap, Who};
use crate::view::line_height;
use gpui_kit::{App, AppContext as _, Bounds, Entity, WindowKind, point, px, size};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// Hyprland reports layer dimensions in logical pixels, as does GPUI's viewport.
fn panel_size() -> Option<(f32, f32)> {
    let layers: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "layers"])).ok()?;
    for monitor in layers.as_object()?.values() {
        for (level, surfaces) in monitor["levels"].as_object()? {
            for surface in surfaces.as_array()? {
                if surface["namespace"] == "capri-line" {
                    assert_eq!(level, "3", "the Line must stay an overlay, never a tile");
                    return Some((surface["w"].as_f64()? as f32, surface["h"].as_f64()? as f32));
                }
            }
        }
    }
    None
}

// Same truncation/clamp helper and beside::place as lineview's Ask::Height -> line_follow.
// Her position and wanted width are fixtures; no avatar pack or full View is needed.
fn follow(os: &Os, id: WinId, sf: f32, measured: f32, width: f32) -> Result<(), String> {
    let d = |v: f32| (v * sf) as i32;
    let area = os.area_at(d(100.0), d(100.0)).rect();
    let her = WinRect::new(area.x + d(900.0), area.y + d(900.0), d(300.0), d(400.0));
    let h = line_height::device_height(measured, sf, area.h);
    let panel = Panel {
        w: d(width),
        min_w: d(480.0),
        h,
        min_h: h.max(area.h / 3 + d(90.0)),
    };
    let r = beside::place(area, her, panel, d(12.0), false, None).rect;
    os.place(id, r.x, r.y, r.w, r.h).map_err(|e| e.to_string())
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
                let width = Rc::new(Cell::new(780.0f32));
                let managed = Rc::new(Cell::new(true));
                let adopted = Rc::new(Cell::new(None));
                let built: Rc<RefCell<Option<Entity<Line>>>> = Rc::default();
                let view_out = built.clone();
                let os_open = os.clone();
                let measured_open = measured.clone();
                let width_open = width.clone();
                let managed_open = managed.clone();
                let adopted_open = adopted.clone();
                let handle = cx.update(move |cx| {
                    let bounds = Bounds::new(point(px(100.0), px(100.0)), size(px(780.0), px(120.0)));
                    let options = os_open.window_options(Role::Line, bounds, false);
                    assert!(matches!(options.kind, WindowKind::LayerShell(_)));
                    cx.open_window(options, move |window, cx| {
                        let id = os_open.adopt(Role::Line, window).expect("adopt Line");
                        let sf = window.scale_factor();
                        adopted_open.set(Some((id, sf)));
                        let on_ask = Box::new(move |ask, _: &mut App| {
                            if let Ask::Height(h) = ask {
                                measured_open.set(h);
                                if managed_open.get() {
                                    follow(&os_open, id, sf, h, width_open.get())?;
                                }
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
                        let layer = panel_size().map_or(0.0, |s| s.1);
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
                    assert!(panel_size().map_or(0.0, |s| s.1) >= minimum);
                }
                // Following her can narrow the Line too; its width used to freeze at open.
                let (id, sf) = adopted.get().expect("adopted Line and scale");
                width.set(620.0);
                cx.update(|_| follow(&os, id, sf, measured.get(), width.get())).expect("follow at a new width");
                let mut width_settled = false;
                for _ in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    let viewport = handle.update(cx, |_, window, _| f32::from(window.viewport_size().width)).expect("viewport width");
                    let layer = panel_size().unwrap_or_default();
                    if (viewport - 620.0).abs() <= 1.0 && (layer.0 - viewport).abs() <= 1.0 {
                        width_settled = true;
                        break;
                    }
                }
                assert!(width_settled, "Line width must follow place, not stay at 780");
                eprintln!("contract_hypr_panels: live width=620");
                // Stop the fixture's measured-height callback before exercising the independent
                // Window::resize API on a surface previously managed by set_bounds.
                managed.set(false);
                handle.update(cx, |_, window, _| window.resize(size(px(640.0), px(700.0)))).expect("explicit resize");
                let mut resize_settled = false;
                for _ in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    let viewport = handle.update(cx, |_, window, _| window.viewport_size()).expect("resized viewport");
                    let layer = panel_size().unwrap_or_default();
                    if (layer.0 - 640.0).abs() <= 1.0
                        && (layer.1 - 700.0).abs() <= 1.0
                        && (f32::from(viewport.width) - layer.0).abs() <= 1.0
                        && (f32::from(viewport.height) - layer.1).abs() <= 1.0
                    {
                        resize_settled = true;
                        break;
                    }
                }
                assert!(resize_settled, "Window::resize must supersede the remembered bounds: {:?}", panel_size());
                cx.background_executor().timer(Duration::from_millis(150)).await;
                assert_eq!(panel_size(), Some((640.0, 700.0)), "a late configure must not restore the old size");
                eprintln!("contract_hypr_panels: subsequent Window::resize=640x700");
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
