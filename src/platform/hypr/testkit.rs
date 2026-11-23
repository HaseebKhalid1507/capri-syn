//! The contract suite on Hyprland, in the rig only (`tools/wlrig`: a bubblewrap sandbox, headless
//! sway, nested Hyprland; never his session). Its own GPUI app opens her body's layer surface (a
//! stand-in that paints only the core's in-scene effect layers) and runs the suite on its event
//! loop; `tools/wlrig/wlworld.py serve` plays the world on a Wayland connection of its own
//! (windows, panels, popups, the virtual pointer, the pointer probe), `hyprctl` moves things.
//!   tools/wlrig/run.sh env SA_CONTRACT_HYPR=1 <test binary> --ignored --test-threads 1 contract_hypr

use super::super::contract::{self, Driver};
use super::super::{Os, Role, ShapeRect, WinId, WinRect};
use super::HyprDesktop;
use gpui_kit::{App, AppContext as _, AsyncApp, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _, WeakEntity, Window, canvas, div, point, px, size};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Behind her body: the world's `under` layer (pointer probes, a plain backdrop for pixels).
const UNDER: u32 = 0x203040;
/// Where her body starts (clear of the windows the scenarios open).
const BODY_AT: (i32, i32) = (1800, 200);
const BODY_SIZE: (i32, i32) = (300, 400);

/// Her stand-in body: nothing of its own, the core's in-scene effect layers.
struct StandIn {
    os: Rc<Os>,
}

impl Render for StandIn {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let os = self.os.clone();
        div().size_full().child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    os.paint_fx(window, true, 1.0);
                    os.paint_fx(window, false, 1.0);
                },
            )
            .size_full(),
        )
    }
}

struct World {
    _child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

impl World {
    fn start() -> World {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/wlrig/wlworld.py");
        let mut child = Command::new("python3").args([script, "serve"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("wlworld.py");
        let stdin = child.stdin.take().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut l = String::new();
        out.read_line(&mut l).unwrap();
        assert_eq!(l.trim(), "ready", "wlworld.py");
        World { _child: child, stdin, out }
    }

    fn cmd(&mut self, line: &str) -> String {
        writeln!(self.stdin, "{line}").unwrap();
        let mut l = String::new();
        self.out.read_line(&mut l).unwrap();
        let l = l.trim().to_string();
        assert!(!l.starts_with("err"), "wlworld `{line}`: {l}");
        l
    }
}

fn hyprctl(args: &[&str]) -> String {
    let o = Command::new("hyprctl").args(args).output().expect("hyprctl");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn dispatch(code: &str) {
    let r = hyprctl(&["dispatch", code]);
    assert_eq!(r, "ok", "dispatch {code}");
}

struct HDriver {
    world: World,
    os: Rc<Os>,
    fx: Vec<crate::host::FxHost>,
    desk: Arc<HyprDesktop>,
    body_ptr: usize,
    body: WinId,
    view: WeakEntity<StandIn>,
    cx: AsyncApp,
    /// Our windows: id → (wlworld name, a toplevel).
    names: HashMap<WinId, (String, bool)>,
    n: u32,
}

impl HDriver {
    fn name(&mut self) -> String {
        self.n += 1;
        format!("ct{}", self.n)
    }

    /// Poll until `f` gives something (1 s).
    fn until<T>(&self, mut f: impl FnMut() -> Option<T>) -> Option<T> {
        let end = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(v) = f() {
                return Some(v);
            }
            if Instant::now() > end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn layer_addr(&self, ns: &str) -> Option<(String, WinRect)> {
        let v: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "layers"])).ok()?;
        for mon in v.as_object()?.values() {
            for ls in mon["levels"].as_object()?.values() {
                for l in ls.as_array()? {
                    if l["namespace"] == ns {
                        let g = |k: &str| l[k].as_i64().unwrap_or(0) as i32;
                        return Some((l["address"].as_str()?.to_string(), WinRect::new(g("x"), g("y"), g("w"), g("h"))));
                    }
                }
            }
        }
        None
    }

    /// The window of this class, by the backend's id.
    fn client_id(&self, class: &str) -> Option<WinId> {
        let v: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "clients"])).ok()?;
        v.as_array()?.iter().find(|c| c["class"] == class).and_then(|c| self.desk.value_id(c))
    }

    fn sel(&self, w: WinId) -> String {
        format!("window = \"class:^{}$\"", self.names[&w].0)
    }

    /// The workspace HEADLESS-2 shows.
    fn other_ws(&self) -> Option<i64> {
        let v: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "monitors"])).ok()?;
        v.as_array()?.iter().find(|m| m["name"] == "HEADLESS-2")?["activeWorkspace"]["id"].as_i64()
    }

    fn outputs(&self) -> usize {
        serde_json::from_str::<serde_json::Value>(&hyprctl(&["-j", "monitors"])).ok().and_then(|v| v.as_array().map(Vec::len)).unwrap_or(0)
    }

    /// The screen at `r`, as RGB rows.
    fn grab(r: WinRect) -> Vec<u8> {
        let o = Command::new("grim").args(["-g", &format!("{},{} {}x{}", r.x, r.y, r.w, r.h), "-t", "ppm", "-"]).output().expect("grim");
        let b = o.stdout;
        // P6\nW H\n255\n
        let mut nl = 0;
        let start = b.iter().position(|&c| {
            nl += usize::from(c == b'\n');
            nl == 3
        });
        b[start.map_or(0, |s| s + 1)..].to_vec()
    }
}

impl Driver for HDriver {
    fn open(&mut self, r: WinRect, managed: bool) -> WinId {
        let name = self.name();
        let id = if managed {
            let rule = format!("hl.window_rule({{ name = \"{name}\", match = {{ class = \"^{name}$\" }}, float = true, move = \"{} {}\", size = \"{} {}\" }})", r.x, r.y, r.w, r.h);
            assert_eq!(hyprctl(&["eval", &rule]), "ok");
            self.world.cmd(&format!("toplevel {name} {} {}", r.w, r.h));
            self.until(|| self.client_id(&name)).expect("the window mapped")
        } else {
            self.world.cmd(&format!("layer {name} {} {} {} {} top", r.x, r.y, r.w, r.h));
            let (a, _) = self.until(|| self.layer_addr(&name)).expect("the popup mapped");
            self.desk.id(&a)
        };
        self.names.insert(id, (name, managed));
        id
    }
    fn configure(&mut self, w: WinId, r: WinRect) {
        let (name, managed) = self.names[&w].clone();
        if managed {
            // Resizing keeps the centre: size first, then the place.
            dispatch(&format!("hl.dsp.window.resize({{ x = {}, y = {}, {} }})", r.w, r.h, self.sel(w)));
            dispatch(&format!("hl.dsp.window.move({{ x = {}, y = {}, {} }})", r.x, r.y, self.sel(w)));
        } else {
            self.world.cmd(&format!("move {name} {} {} {} {}", r.x, r.y, r.w, r.h));
        }
    }
    /// Its workspace leaves the screen (a workspace it isn't shown on).
    fn hide(&mut self, w: WinId) {
        dispatch(&format!("hl.dsp.window.move({{ workspace = 9, follow = false, {} }})", self.sel(w)));
    }
    fn show(&mut self, w: WinId) {
        dispatch(&format!("hl.dsp.window.move({{ workspace = 1, follow = false, {} }})", self.sel(w)));
    }
    fn close(&mut self, w: WinId) {
        let (name, _) = self.names.remove(&w).expect("ours");
        self.world.cmd(&format!("destroy {name}"));
    }
    fn fullscreen(&mut self, w: WinId, on: bool) {
        dispatch(&format!("hl.dsp.window.fullscreen({{ mode = 'fullscreen', action = '{}', {} }})", if on { "set" } else { "unset" }, self.sel(w)));
    }
    /// The second output (the rig's HEADLESS-2, right of WAYLAND-1), or back to one.
    fn split_monitors(&mut self, on: bool) -> bool {
        if on != (self.outputs() == 2) {
            hyprctl(if on { &["output", "create", "headless", "HEADLESS-2"][..] } else { &["output", "remove", "HEADLESS-2"][..] });
        }
        self.until(|| (self.outputs() == if on { 2 } else { 1 }).then_some(())).is_some()
    }
    /// A panel: a top-edge layer with an exclusive zone, on the first output.
    fn dock_top(&mut self, h: i32) -> WinId {
        let name = self.name();
        self.world.cmd(&format!("layer {name} 0 0 0 {h} top {h}"));
        let (a, _) = self.until(|| self.layer_addr(&name)).expect("the panel mapped");
        let id = self.desk.id(&a);
        self.names.insert(id, (name, false));
        id
    }
    fn warp(&mut self, x: i32, y: i32) {
        self.world.cmd(&format!("warp {x} {y}"));
    }
    fn body_rect(&mut self) -> Option<WinRect> {
        self.layer_addr("capri-body").map(|(_, r)| WinRect { id: self.body, ..r })
    }
    /// Her window's pixels and her effect hosts': one extent per colour that isn't the backdrop.
    fn fx_rects(&mut self) -> Vec<WinRect> {
        let Some(b) = self.body_rect() else { return Vec::new() };
        let b = match self.os.fx_staged_box() {
            Some(f) => {
                let (x0, y0) = (b.x.min(f.x), b.y.min(f.y));
                WinRect::new(x0, y0, b.right().max(f.right()) - x0, b.bottom().max(f.bottom()) - y0)
            }
            None => b,
        };
        let px = Self::grab(b);
        let mut ext: HashMap<u32, (i32, i32, i32, i32)> = HashMap::new();
        for (i, p) in px.as_chunks::<3>().0.iter().enumerate() {
            // The backdrop comes back a step off its value (the compositor's colour path).
            let off = |i: usize, v: u8| (i32::from(v) - ((UNDER >> (16 - 8 * i)) & 0xff) as i32).abs();
            if (0..3).all(|i| off(i, p[i]) <= 2) {
                continue;
            }
            let c = u32::from_be_bytes([0, p[0], p[1], p[2]]);
            let (x, y) = (b.x + (i as i32 % b.w), b.y + (i as i32 / b.w));
            let e = ext.entry(c).or_insert((x, y, x, y));
            *e = (e.0.min(x), e.1.min(y), e.2.max(x), e.3.max(y));
        }
        ext.values().map(|&(x0, y0, x1, y1)| WinRect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)).collect()
    }
    /// The world's backdrop surface loses the pointer to her where her input region is.
    fn hits(&mut self, x: i32, y: i32) -> bool {
        self.world.cmd(&format!("probe {x} {y}")) == "other"
    }
    fn visible_region(&mut self) -> Option<Vec<ShapeRect>> {
        None
    }
    /// Commit her pending surface state, let a frame be drawn, let the compositor catch up.
    fn to_other_space(&mut self, w: WinId) -> bool {
        let Some(ws) = self.other_ws() else { return false };
        dispatch(&format!("hl.dsp.window.move({{ workspace = {ws}, follow = false, {} }})", self.sel(w)));
        true
    }
    /// HEADLESS-2 to another workspace, the focus staying where it is.
    fn flip_other_space(&mut self) -> bool {
        let Some(ws) = self.other_ws() else { return false };
        let to = if ws == 7 { 2 } else { 7 };
        hyprctl(&["eval", &format!("hl.get_monitor(\"HEADLESS-2\"):set_workspace({to})")]) == "ok"
    }
    async fn settle(&mut self) {
        gpui_linux::layer_ctl::commit_pending(self.body_ptr);
        self.view.update(&mut self.cx, |_, cx| cx.notify()).ok();
        let (os, fx) = (self.os.clone(), &mut self.fx);
        self.cx.update(|cx| crate::host::fx_hosts_sync(cx, &os, fx));
        self.cx.background_executor().timer(Duration::from_millis(150)).await;
        self.world.cmd("hypr version");
    }
}

#[test]
#[ignore = "needs the Wayland rig: tools/wlrig/run.sh env SA_CONTRACT_HYPR=1 <test binary> --ignored contract_hypr"]
fn contract_hypr() {
    // Inside the rig's sandbox only: its runtime dir, its Hyprland.
    let rig = std::env::var("RIG").unwrap_or_default();
    assert!(std::env::var("SA_CONTRACT_HYPR").is_ok_and(|v| v == "1") && !rig.is_empty(), "refusing: run inside tools/wlrig (run.sh) with SA_CONTRACT_HYPR=1");
    // The rig's runtime dir (its `rig.ready`), one Hyprland in it. (`hyprctl instances` is empty
    // here: the compositor's pid is in another namespace.)
    let run = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_default());
    assert!(run.join("rig.ready").exists(), "refusing: not the rig's runtime dir");
    assert_eq!(std::fs::read_dir(run.join("hypr")).map(|d| d.count()).unwrap_or(0), 1, "exactly the rig's Hyprland");
    let (desk, stage) = super::connect().expect("the rig's Hyprland");
    let os = Rc::new(Os::new(desk.clone(), Box::new(stage)));
    let result: Rc<RefCell<Option<anyhow::Result<Vec<&'static str>>>>> = Rc::default();
    let res = result.clone();
    gpui_kit::application().run(move |cx: &mut App| {
        let bounds = Bounds::new(point(px(BODY_AT.0 as f32), px(BODY_AT.1 as f32)), size(px(BODY_SIZE.0 as f32), px(BODY_SIZE.1 as f32)));
        let options = os.window_options(Role::Body, bounds, false);
        let os2 = os.clone();
        let ptr = Rc::new(std::cell::Cell::new(0usize));
        let ptr2 = ptr.clone();
        let win = cx
            .open_window(options, |window, cx| {
                use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
                if let Ok(RawWindowHandle::Wayland(h)) = window.window_handle().map(|h| h.as_raw()) {
                    ptr2.set(h.surface.as_ptr() as usize);
                }
                os2.adopt(Role::Body, window).expect("adopt");
                os2.move_to(BODY_AT.0, BODY_AT.1).ok();
                cx.new(|_| StandIn { os: os2.clone() })
            })
            .expect("her layer surface");
        let view = win.update(cx, |_, _, cx| cx.entity().downgrade()).expect("the stand-in");
        let fx = crate::host::open_fx_hosts(cx, &os, 1.0);
        let os = os.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            let mut world = World::start();
            world.cmd("under");
            let mut d = HDriver { world, os: os.clone(), fx, desk, body_ptr: ptr.get(), body: os.body(), view, cx: cx.clone(), names: HashMap::new(), n: 0 };
            let two = d.outputs() == 2;
            let r = contract::run(&os, &mut d).await;
            d.split_monitors(two);
            *res.borrow_mut() = Some(r);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let r = result.borrow_mut().take().expect("the suite ran");
    let passed = r.unwrap();
    eprintln!("contract_hypr: {} scenarios passed: {}", passed.len(), passed.join(", "));
    assert_eq!(passed.len(), 15);
}

/// What the stack is made of, from snapshots of a live two-output caelestia session (no Hyprland, no
/// display: plain `cargo test`).
/// `hyprctl layers` says where a layer is, never what it draws, so these pin which geometry
/// counts as a window's cover (`occludes`: a layer that contains its output's work area is the
/// shell's canvas, none) and that the perch rules then find the edges open.
mod shell_overlay {
    use super::super::{HClient, HLayer, HMon, Ids, Snap, WsRef, occludes};
    use crate::edges::{WinRect, exposed_spans, seat_ranges};

    /// The contact extent of her sitting (the live error's: -32..49 about the seat) and
    /// `view.rs`'s `EDGE_MARGIN`.
    const CONTACT: (i32, i32) = (-32, 49);
    const MARGIN: i32 = 60;

    /// `hyprctl monitors` of that session: eDP-1 2560x1600 at the origin, DP-2 2560x1440 to its right,
    /// both at scale 1 with caelestia's reserved edges (left, top, right, bottom).
    fn outputs() -> Vec<HMon> {
        let mon = |id: i64, name: &str, x: i32, h: i32, ws: i64| HMon {
            id,
            name: name.into(),
            x,
            y: 0,
            width: 2560,
            height: h,
            scale: 1.0,
            reserved: vec![60, 10, 10, 10],
            active_workspace: WsRef { id: ws },
            ..Default::default()
        };
        vec![mon(1, "eDP-1", 0, 1600, 1), mon(0, "DP-2", 2560, 1440, 2)]
    }

    /// A mapped client on monitor `mon`'s workspace `ws`.
    fn client(address: &str, mon: i64, ws: i64, floating: bool, at: [i32; 2], size: [i32; 2]) -> HClient {
        HClient { address: address.into(), stable_id: format!("s{address}"), mapped: true, at, size, workspace: WsRef { id: ws }, floating, monitor: mon, class: "kitty".into(), ..Default::default() }
    }

    /// One tiled window per output, gaps and the reserved edges kept clear.
    fn tiled() -> Vec<HClient> {
        vec![client("0x16", 1, 1, false, [70, 10], [2480, 1580]), client("0x8", 0, 2, false, [2630, 10], [2420, 1420])]
    }

    fn layer(mon: &str, level: u8, ns: &str, (x, y, w, h): (i32, i32, i32, i32)) -> (String, u8, HLayer) {
        (mon.into(), level, HLayer { address: format!("{mon}:{ns}:{x},{y}"), x, y, w, h, namespace: ns.into() })
    }

    /// Every top layer on both outputs as `hyprctl layers` listed them with caelestia up (hers
    /// among them): a drawers layer over each whole output, 1x1 probes, her own.
    fn caelestia() -> Vec<(String, u8, HLayer)> {
        let probes = |mon: &str, at: [(i32, i32); 4]| at.map(|(x, y)| layer(mon, 2, "caelestia-border-exclusion", (x, y, 1, 1)));
        let mut ls = vec![layer("eDP-1", 2, "caelestia-drawers", (0, 0, 2560, 1600)), layer("DP-2", 2, "caelestia-drawers", (2560, 0, 2560, 1440))];
        ls.extend(probes("eDP-1", [(0, 800), (1310, 0), (2559, 805), (1305, 1599)]));
        ls.extend(probes("DP-2", [(2560, 720), (3870, 0), (5119, 725), (3865, 1439)]));
        ls.push(layer("DP-2", 2, "quickshell", (3865, 720, 1, 1)));
        ls.push(layer("DP-2", 2, "capri-body", (2560, 0, 500, 708)));
        ls
    }

    /// x intervals, root px.
    type Spans = Vec<(i32, i32)>;

    /// The exposed spans of the top edge of the window whose left edge is at `x`, and the seat
    /// ranges on them: what `Avatar::perch` finds.
    fn edge(s: &Snap, x: i32) -> (Spans, Spans) {
        let stack = s.stack(&mut Ids::default());
        let rects: Vec<WinRect> = stack.iter().map(|w| w.rect).collect();
        let i = stack.iter().position(|w| w.managed && w.rect.x == x).expect("the window is in the stack");
        (exposed_spans(&rects, i), seat_ranges(&rects, i, CONTACT, MARGIN))
    }

    fn occluders(s: &Snap) -> usize {
        s.stack(&mut Ids::default()).iter().filter(|w| !w.managed).count()
    }

    #[test]
    fn a_shells_full_output_layer_hides_no_window() {
        // The live bug: `perch sit` and `perch stand` both ended in `edge hidden ... exposed []`.
        let s = Snap { mons: outputs(), clients: tiled(), layers: caelestia() };
        assert_eq!(occluders(&s), 0, "none of caelestia's (nor hers) is a cover");
        for (x, w) in [(70, 2480), (2630, 2420)] {
            let (spans, seats) = edge(&s, x);
            assert_eq!(spans, vec![(x, x + w)], "the window at x {x}: its whole top edge is open");
            assert_eq!(seats, vec![(x + MARGIN + 32, x + w - MARGIN - 49)], "the window at x {x}: she can sit on it");
        }
    }

    #[test]
    fn a_canvas_is_measured_in_logical_px() {
        // 3840x2400 at 1.5 is 2560x1600 to the layer shell, and `reserved` is in its px too: a
        // canvas over the output, or one over what is left of it, covers the work area.
        let mon = HMon { id: 0, name: "DP-1".into(), width: 3840, height: 2400, scale: 1.5, reserved: vec![60, 10, 10, 10], active_workspace: WsRef { id: 1 }, ..Default::default() };
        assert_eq!(mon.work_area(false).rect(), WinRect::new(60, 10, 2490, 1580));
        for canvas in [(0, 0, 2560, 1600), (60, 10, 2490, 1580)] {
            let s = Snap { mons: vec![mon.clone()], clients: vec![client("0x1", 0, 1, false, [70, 10], [2480, 1580])], layers: vec![layer("DP-1", 2, "ags", canvas)] };
            assert_eq!(occluders(&s), 0, "{canvas:?}");
            assert_eq!(edge(&s, 70).0, vec![(70, 2550)]);
        }
    }

    #[test]
    fn slivers_hide_nothing() {
        // On the top edge of the window on eDP-1 (y 10): a 1x1 probe, a hot edge along it and
        // another across it, and a 2x2 over the other output's window.
        let layers = vec![
            layer("eDP-1", 2, "probe", (500, 10, 1, 1)),
            layer("eDP-1", 3, "hot-edge", (0, 9, 2560, 2)),
            layer("eDP-1", 3, "hot-edge", (1000, 0, 2, 1600)),
            layer("DP-2", 2, "probe", (3000, 10, 2, 2)),
        ];
        let s = Snap { mons: outputs(), clients: tiled(), layers };
        assert_eq!(occluders(&s), 0);
        assert_eq!(edge(&s, 70).0, vec![(70, 2550)]);
        assert_eq!(edge(&s, 2630).0, vec![(2630, 5050)]);
    }

    #[test]
    fn a_bar_and_a_popup_still_cover_beside_a_shells_canvas() {
        let mut layers = caelestia();
        // A bar over the top of eDP-1 and a notification over DP-2's window, a level up.
        layers.push(layer("eDP-1", 2, "waybar", (0, 0, 2560, 32)));
        layers.push(layer("DP-2", 3, "swaync", (4600, 30, 400, 120)));
        // A floating window with its top edge under the bar; a tiled one under the popup.
        let clients = vec![client("0x20", 1, 1, true, [200, 16], [1200, 800]), client("0x8", 0, 2, false, [2630, 40], [2420, 1390])];
        let s = Snap { mons: outputs(), clients, layers };
        assert_eq!(occluders(&s), 2, "the bar and the popup, not the canvases");
        let (spans, seats) = edge(&s, 200);
        assert!(spans.is_empty() && seats.is_empty(), "the bar covers the whole edge: {spans:?}");
        let (spans, seats) = edge(&s, 2630);
        assert_eq!(spans, vec![(2630, 4600), (5000, 5050)], "the popup covers its stretch of the edge");
        assert!(!seats.is_empty(), "the rest of it is open");
    }

    #[test]
    fn a_canvas_is_judged_by_its_own_output() {
        // A 4K output with a 1080p one to its right: each one's canvas is the other's cover, the
        // one that fits is told by the name `hyprctl layers` files it under (not the first output,
        // the biggest, or the one the list ends with), whatever order the outputs come in.
        let mon = |id: i64, name: &str, x: i32, (w, h): (i32, i32)| HMon { id, name: name.into(), x, width: w, height: h, scale: 1.0, active_workspace: WsRef { id: id + 1 }, ..Default::default() };
        let (dp, hdmi) = (mon(0, "DP-1", 0, (3840, 2160)), mon(1, "HDMI-A-1", 3840, (1920, 1080)));
        for mons in [vec![dp.clone(), hdmi.clone()], vec![hdmi, dp]] {
            for (filed_under, canvas, covers) in [("DP-1", (0, 0, 3840, 2160), 0), ("HDMI-A-1", (3840, 0, 1920, 1080), 0), ("DP-1", (3840, 0, 1920, 1080), 1), ("HDMI-A-1", (0, 0, 3840, 2160), 1)] {
                let s = Snap { mons: mons.clone(), clients: Vec::new(), layers: vec![layer(filed_under, 2, "shell", canvas)] };
                assert_eq!(occluders(&s), covers, "{canvas:?} filed under {filed_under}");
            }
        }
        // And one that names no output is no canvas.
        let s = Snap { mons: vec![mon(0, "DP-1", 0, (3840, 2160))], clients: Vec::new(), layers: vec![layer("eDP-1", 2, "shell", (0, 0, 3840, 2160))] };
        assert_eq!(occluders(&s), 1);
    }

    #[test]
    fn a_canvas_beside_a_bar_and_a_dock_on_a_small_screen() {
        // 1366x768 with a 48 px bar and a 48 px dock: the canvas is what they leave, 87.5% of
        // the output tall. Only the bar and the dock cover; the window between them has its edge.
        let mon = HMon { id: 0, name: "LVDS-1".into(), width: 1366, height: 768, scale: 1.0, reserved: vec![0, 48, 0, 48], active_workspace: WsRef { id: 1 }, ..Default::default() };
        let layers = vec![layer("LVDS-1", 2, "shell", (0, 48, 1366, 672)), layer("LVDS-1", 2, "bar", (0, 0, 1366, 48)), layer("LVDS-1", 2, "dock", (0, 720, 1366, 48))];
        let s = Snap { mons: vec![mon], clients: vec![client("0x1", 0, 1, false, [8, 56], [1350, 656])], layers };
        let covers: Vec<_> = s.stack(&mut Ids::default()).into_iter().filter(|w| !w.managed).map(|w| (w.rect.y, w.rect.h)).collect();
        assert_eq!(covers, vec![(0, 48), (720, 48)], "the bar and the dock");
        let (spans, seats) = edge(&s, 8);
        assert_eq!(spans, vec![(8, 1358)]);
        assert!(!seats.is_empty());
    }

    #[test]
    fn a_centred_panel_is_a_cover() {
        // A panel 94% of the output both ways, centred (an opaque launcher or settings box):
        // near the whole output, but not its work area, so it hides the window under it.
        let mon = HMon { id: 0, name: "eDP-1".into(), width: 2560, height: 1600, scale: 1.0, reserved: vec![60, 10, 10, 10], active_workspace: WsRef { id: 1 }, ..Default::default() };
        let clients = vec![client("0x1", 0, 1, true, [200, 100], [800, 600])];
        let open = Snap { mons: vec![mon.clone()], clients: clients.clone(), layers: Vec::new() };
        assert_eq!(edge(&open, 200).0, vec![(200, 1000)]);
        let s = Snap { mons: vec![mon], clients, layers: vec![layer("eDP-1", 3, "launcher", (77, 48, 2406, 1504))] };
        assert_eq!(occluders(&s), 1);
        assert!(edge(&s, 200).0.is_empty(), "the panel covers it");
    }

    #[test]
    fn the_rule() {
        let l = |(x, y, w, h): (i32, i32, i32, i32)| HLayer { x, y, w, h, ..Default::default() };
        // eDP-1 with caelestia's reserved edges: 2560x1600 less 70 and 20 px.
        let work = Some(WinRect::new(60, 10, 2490, 1580));
        // The whole output, what is left of it, and a layer that bleeds past it: the canvas.
        assert!(!occludes(&l((0, 0, 2560, 1600)), work));
        assert!(!occludes(&l((60, 10, 2490, 1580)), work));
        assert!(!occludes(&l((-20, -20, 2600, 1640)), work));
        // Rounding (SLIVER) is allowed on each side; a px more is a cover.
        assert!(!occludes(&l((62, 12, 2486, 1576)), work));
        assert!(occludes(&l((63, 10, 2487, 1580)), work), "left");
        assert!(occludes(&l((60, 13, 2490, 1577)), work), "top");
        assert!(occludes(&l((60, 10, 2487, 1580)), work), "right");
        assert!(occludes(&l((60, 10, 2490, 1577)), work), "bottom");
        // A bar, a dock, a side panel, a half-screen sidebar, a popup, a centred panel: covers.
        assert!(occludes(&l((0, 0, 2560, 32)), work));
        assert!(occludes(&l((0, 1536, 2560, 64)), work));
        assert!(occludes(&l((0, 0, 64, 1600)), work));
        assert!(occludes(&l((1280, 0, 1280, 1600)), work));
        assert!(occludes(&l((4600, 30, 400, 120)), work));
        assert!(occludes(&l((77, 48, 2406, 1504)), work));
        // Slivers, whatever the output.
        for sliver in [(0, 0, 1, 1), (0, 0, 2560, 2), (0, 0, 2, 1600), (700, 9, 2, 2)] {
            assert!(!occludes(&l(sliver), work), "{sliver:?}");
            assert!(!occludes(&l(sliver), None), "{sliver:?}");
        }
        assert!(occludes(&l((0, 0, 3, 3)), work));
        // An output she doesn't know of, or one with nothing left: no canvas to tell, only slivers go.
        assert!(occludes(&l((0, 0, 2560, 1600)), None));
        assert!(occludes(&l((0, 0, 2560, 1600)), Some(WinRect::new(0, 0, 0, 0))));
    }
}
