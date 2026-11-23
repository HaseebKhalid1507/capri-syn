//! Real Line/Settings, not a stand-in of a requested size. No pack, daemon or login needed.
//! Run only inside tools/wlrig (the guard is shared with contract_hypr).

use super::super::{Os, Role, WinId, WinRect};
use super::testkit::{focus_output, hyprctl, panel_outputs, require_rig};
use crate::beside::{self, Panel};
use crate::line::{Ask, Line, Page, Prefs, Row, Snap, Who};
use crate::view::line_height;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, WindowKind, div, point, px, size};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// A body-host stand-in; the tested panel below is the real Line, not a size-only stub.
struct Empty;
impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("output contract")
    }
}

fn layer_on(namespace: &str) -> Option<(String, WinRect)> {
    let layers: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "layers"])).ok()?;
    for (name, monitor) in layers.as_object()? {
        for (level, surfaces) in monitor["levels"].as_object()? {
            for surface in surfaces.as_array()? {
                if surface["namespace"] == namespace {
                    if matches!(namespace, "capri-line" | "capri-reader") {
                        assert_eq!(level, "3", "panels must stay overlays, never tiles");
                    }
                    let n = |key: &str| surface[key].as_i64().map(|v| v as i32);
                    return Some((name.clone(), WinRect::new(n("x")?, n("y")?, n("w")?, n("h")?)));
                }
            }
        }
    }
    None
}

fn panel_size() -> Option<(f32, f32)> {
    layer_on("capri-line").map(|(_, r)| (r.w as f32, r.h as f32))
}

fn inside(r: WinRect, a: WinRect) -> bool {
    r.x >= a.x && r.y >= a.y && r.right() <= a.right() && r.bottom() <= a.bottom()
}

fn assert_output(namespace: &str, name: &str, area: WinRect) {
    let (got, r) = layer_on(namespace).expect("mapped layer");
    assert_eq!(got, name, "{namespace} must follow her body, not keyboard focus");
    assert!(inside(r, area), "{namespace} is clipped at its output edge: {r:?}, usable {area:?}");
}

struct PanelHarness {
    handle: AnyWindowHandle,
    view: Entity<Line>,
    measured: Rc<Cell<f32>>,
    width: Rc<Cell<f32>>,
    managed: Rc<Cell<bool>>,
    adopted: Rc<Cell<Option<(WinId, f32)>>>,
}

fn open_panel(cx: &mut App, os: Rc<Os>, first: Page, draft: String, sent: Vec<String>) -> PanelHarness {
    let measured = Rc::new(Cell::new(0.0f32));
    let width = Rc::new(Cell::new(780.0f32));
    let managed = Rc::new(Cell::new(true));
    let adopted = Rc::new(Cell::new(None));
    let built: Rc<RefCell<Option<Entity<Line>>>> = Rc::default();
    let view_out = built.clone();
    let measured_open = measured.clone();
    let width_open = width.clone();
    let managed_open = managed.clone();
    let adopted_open = adopted.clone();
    let area = os.panel_area().expect("body output's usable area");
    // Deliberately outside the body output: dress must clamp bootstrap and use the body
    // host, not the bounds' centre or focus, to choose the wl_output.
    let bounds = Bounds::new(point(px(1169.0), px(1455.0)), size(px(780.0), px(120.0)));
    let options = os.panel_options(Role::Line, bounds, false, cx).expect("pinned panel options");
    assert!(matches!(options.kind, WindowKind::LayerShell(_)));
    assert!(options.display_id.is_some(), "get_layer_surface must receive a wl_output");
    let handle = cx
        .open_window(options, move |window, cx| {
            let id = os.adopt(Role::Line, window).expect("adopt Line");
            let sf = window.scale_factor();
            adopted_open.set(Some((id, sf)));
            let on_ask = Box::new(move |ask, _: &mut App| {
                if let Ask::Height(h) = ask {
                    measured_open.set(h);
                    if managed_open.get() {
                        follow(&os, id, sf, h, width_open.get())?;
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
                    draft,
                    sent,
                    "Capri".into(),
                    "DejaVu Sans Mono".into(),
                    crate::theme::Pal::SHARD,
                    area.h as f32 / 3.0,
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
        .expect("open Line");
    let view = built.borrow_mut().take().expect("the real Line");
    PanelHarness {
        handle: handle.into(),
        view,
        measured,
        width,
        managed,
        adopted,
    }
}

// Same truncation/clamp helper and beside::place as lineview's Ask::Height -> line_follow.
// Her position and wanted width are fixtures; no avatar pack or full View is needed.
fn follow(os: &Os, id: WinId, sf: f32, measured: f32, width: f32) -> Result<(), String> {
    let d = |v: f32| (v * sf) as i32;
    let area = os.panel_area().expect("body output's usable area");
    let her = WinRect::new(area.x + d(760.0), area.bottom() - d(686.0), d(500.0), d(708.0));
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
    panel_outputs();
    focus_output("HEADLESS-2");
    let (desk, stage) = super::connect().expect("the rig's Hyprland");
    let os = Rc::new(Os::new(desk, Box::new(stage)));
    let finished = Rc::new(Cell::new(false));
    let done = finished.clone();
    gpui_kit::application().run(move |cx: &mut App| {
        // Reopen from scratch after closing the first panel; don't quit with its last window.
        cx.set_quit_mode(gpui_kit::QuitMode::Explicit);
        crate::line::kit_init(cx);
        let bodies: Vec<_> = [0.0, 2560.0]
            .into_iter()
            .map(|x| {
                let bounds = Bounds::new(point(px(x + 760.0), px(914.0)), size(px(500.0), px(708.0)));
                let options = os.panel_options(Role::Body, bounds, false, cx).expect("body output");
                let os = os.clone();
                cx.open_window(options, move |window, cx| {
                    os.adopt(Role::Body, window).expect("adopt body host");
                    cx.new(|_| Empty)
                })
                .expect("body host")
            })
            .collect();
        os.move_now(760, 914).expect("body on first output");
        cx.spawn(async move |cx| {
            // Fresh chat, a page swap in each direction, then Settings opened directly.
            // Both used to remain 120 px tall; chat's field / Settings' bottom rows were clipped.
            for first in [Page::Chat, Page::Settings] {
                cx.update(|_| os.move_now(760, 914)).expect("body back on first output");
                focus_output("HEADLESS-2");
                let PanelHarness {
                    handle,
                    view,
                    measured,
                    width,
                    managed,
                    adopted,
                } = cx.update(|cx| open_panel(cx, os.clone(), first, "a draft in the input field".into(), vec!["previous message".into()]));
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
                    assert_output("capri-line", "WAYLAND-1", os.panel_area().unwrap());
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
                // Window::resize itself cannot reposition a layer. Leave room for the
                // independent larger resize; production Line sizing uses os.place.
                let area = os.panel_area().unwrap();
                cx.update(|_| os.place(id, area.x + 100, area.y + 100, 620, 400)).expect("room for independent resize");
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
                assert_output("capri-line", "WAYLAND-1", os.panel_area().unwrap());
                // Arbitrary off-screen place must clamp both size and bottom edge; this
                // bypasses beside::place to exercise the Hyprland boundary itself.
                cx.update(|_| os.place(id, 1169, 1455, 820, 4000)).expect("clamped place");
                let area = os.panel_area().unwrap();
                let mut clamped = false;
                for _ in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    if layer_on("capri-line").is_some_and(|(_, r)| r.h == area.h && inside(r, area)) {
                        clamped = true;
                        break;
                    }
                }
                assert!(clamped, "Line may never extend outside usable height: {:?}", layer_on("capri-line"));

                // Move her while the Line is open. A new output requires a new surface,
                // not different margins on the original one. Focus remains on the OTHER.
                handle.update(cx, |_, window, cx| view.update(cx, |l, cx| l.show(Page::Settings, window, cx))).unwrap();
                cx.background_executor().timer(Duration::from_millis(200)).await;
                cx.update(|_| os.move_now(3320, 914)).expect("body on second output");
                assert!(os.panel_needs_reopen(id), "an open layer cannot follow via margins");
                focus_output("WAYLAND-1");
                let (draft, sent, page) = handle
                    .update(cx, |_, window, cx| view.update(cx, |l, cx| l.detach_for_output(window, cx)))
                    .expect("detach old Line");
                assert_eq!(draft, "a draft in the input field");
                assert_eq!(sent, ["previous message"]);
                assert_eq!(page, Page::Settings);
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let moved = cx.update(|cx| open_panel(cx, os.clone(), page, draft, sent));
                assert!(!os.panel_needs_reopen(moved.adopted.get().unwrap().0));
                let mut followed = false;
                for _ in 0..100 {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    let a = os.panel_area().unwrap();
                    if layer_on("capri-line").is_some_and(|(name, r)| name == "HEADLESS-2" && r.h >= 300 && inside(r, a)) {
                        followed = true;
                        break;
                    }
                }
                assert!(followed, "recreated Settings must be whole on her new output: {:?}", layer_on("capri-line"));
                assert_output("capri-line", "HEADLESS-2", os.panel_area().unwrap());
                let state = moved.handle.update(cx, |_, _, cx| moved.view.read(cx).state_json(cx)).unwrap();
                assert_eq!(state["draft_chars"], "a draft in the input field".chars().count());
                moved.handle.update(cx, |_, window, _| window.remove_window()).expect("close moved panel");
                cx.background_executor().timer(Duration::from_millis(150)).await;

                // All per-moment roles share the same output-binding path, not just Line.
                for role in [Role::Reader, Role::Chip, Role::Intro] {
                    let area = os.panel_area().unwrap();
                    let h = cx.update(|cx| {
                        let bounds = Bounds::new(point(px((area.x + 100) as f32), px(100.0)), size(px(300.0), px(180.0)));
                        let options = os.panel_options(role, bounds, false, cx).expect("per-moment output");
                        cx.open_window(options, |window, cx| {
                            os.adopt(role, window).unwrap();
                            cx.new(|_| Empty)
                        })
                        .unwrap()
                    });
                    for _ in 0..100 {
                        if layer_on(super::namespace(role)).is_some() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                    }
                    assert_output(super::namespace(role), "HEADLESS-2", area);
                    h.update(cx, |_, window, _| window.remove_window()).unwrap();
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                }
            }
            for body in bodies {
                body.update(cx, |_, window, _| window.remove_window()).unwrap();
            }
            done.set(true);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    assert!(finished.get(), "the panel contract ran to completion");
}
