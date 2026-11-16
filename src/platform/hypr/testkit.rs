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

    fn client_addr(&self, class: &str) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(&hyprctl(&["-j", "clients"])).ok()?;
        v.as_array()?.iter().find(|c| c["class"] == class).and_then(|c| c["address"].as_str().map(str::to_string))
    }

    fn sel(&self, w: WinId) -> String {
        format!("window = \"class:^{}$\"", self.names[&w].0)
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
            let a = self.until(|| self.client_addr(&name)).expect("the window mapped");
            self.desk.id(&a)
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
    /// Her window's pixels: one extent per colour that isn't the backdrop.
    fn fx_rects(&mut self) -> Vec<WinRect> {
        let Some(b) = self.body_rect() else { return Vec::new() };
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
    async fn settle(&mut self) {
        gpui_linux::layer_ctl::commit_pending(self.body_ptr);
        self.view.update(&mut self.cx, |_, cx| cx.notify()).ok();
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
        let os = os.clone();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            let mut world = World::start();
            world.cmd("under");
            let mut d = HDriver { world, desk, body_ptr: ptr.get(), body: os.body(), view, cx: cx.clone(), names: HashMap::new(), n: 0 };
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
    assert_eq!(passed.len(), 9);
}
