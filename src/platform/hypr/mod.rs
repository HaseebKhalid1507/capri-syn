//! The Hyprland backend: plain translation. The world outside her comes from Hyprland's IPC
//! (`ipc.rs`: monitors, clients, layers, the cursor, socket2 hints); her own surfaces are GPUI
//! layer-shell windows driven through the vendored GPUI's `layer_ctl` (PATCHES.md PL-1..PL-4).
//! Every rule built on these answers is the core's (`platform/os.rs`); the core polls the perch
//! target and paints effect layers in her window (`Caps`).

mod ipc;
#[cfg(test)]
mod testkit;

use super::{Caps, Desktop, FxZ, Hint, Monitor, MoveAnim, Place, PointerRead, Role, ShapeRect, Stage, Visible, Win, WinId, WinRect};
use anyhow::{Context as _, Result, bail};
use gpui_kit::{Bounds, Pixels, Window, WindowKind, WindowOptions, point, px, size};
use gpui_linux::layer_ctl::{self, Commit};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Her surfaces' namespaces (one layer rule matches them all: `^capri-.*$`).
const NS_PREFIX: &str = "capri-";
/// A pointer read at most this often (the core's drag poller asks every 2 ms; each read is a
/// request Hyprland answers synchronously).
const POINTER_EVERY: Duration = Duration::from_millis(4);

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct WsRef {
    id: i64,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct HMon {
    id: i64,
    name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    scale: f32,
    /// left, top, right, bottom (logical px).
    reserved: Vec<i32>,
    #[serde(rename = "refreshRate")]
    refresh_rate: f32,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WsRef,
    #[serde(rename = "specialWorkspace")]
    special_workspace: WsRef,
    focused: bool,
    disabled: bool,
}

impl HMon {
    fn monitor(&self, primary: bool) -> Monitor {
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        Monitor { primary, x: self.x, y: self.y, w: (self.width as f32 / s).round() as i32, h: (self.height as f32 / s).round() as i32 }
    }
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct HClient {
    address: String,
    /// Never reused (addresses are: a new window may get a closed one's).
    #[serde(rename = "stableId")]
    stable_id: String,
    mapped: bool,
    hidden: bool,
    at: [i32; 2],
    size: [i32; 2],
    workspace: WsRef,
    floating: bool,
    monitor: i64,
    pinned: bool,
    /// Mode bits: 1 maximised, 2 fullscreen.
    fullscreen: i64,
    class: String,
    #[serde(rename = "focusHistoryID")]
    focus_history_id: i64,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct HLayer {
    address: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    namespace: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HLevels {
    levels: HashMap<String, Vec<HLayer>>,
}

/// One snapshot of the world (one batch request).
struct Snap {
    mons: Vec<HMon>,
    clients: Vec<HClient>,
    /// (monitor name, level 0..3, layer).
    layers: Vec<(String, u8, HLayer)>,
}

impl Snap {
    /// On an active workspace of its monitor (or its open special one), or pinned.
    fn visible(&self, c: &HClient) -> bool {
        c.mapped
            && !c.hidden
            && (c.pinned
                || self.mons.iter().any(|m| m.id == c.monitor && (m.active_workspace.id == c.workspace.id || (m.special_workspace.id != 0 && m.special_workspace.id == c.workspace.id))))
    }

    fn special(&self, c: &HClient) -> bool {
        self.mons.iter().any(|m| m.special_workspace.id != 0 && m.special_workspace.id == c.workspace.id)
    }
}

/// A client's identity: its stable id (its address where there is none).
fn client_key(address: &str, stable: &str) -> String {
    if stable.is_empty() { address.to_string() } else { format!("w:{stable}") }
}

/// Window identity: Hyprland's stable ids (layers' addresses, her own namespaces) as small ids,
/// never reused; each client's last address (what dispatchers take).
#[derive(Default)]
struct Ids {
    by_key: HashMap<String, WinId>,
    keys: Vec<String>,
    addr: HashMap<WinId, String>,
}

impl Ids {
    fn id(&mut self, key: &str) -> WinId {
        if let Some(&i) = self.by_key.get(key) {
            return i;
        }
        self.keys.push(key.to_string());
        // 0 is "none"; 1 stays free (callers treat ids ≤ 1 as no window).
        let i = self.keys.len() as WinId + 1;
        self.by_key.insert(key.to_string(), i);
        i
    }
    fn key(&self, id: WinId) -> Option<&str> {
        self.keys.get((id as usize).checked_sub(2)?).map(String::as_str)
    }
}

pub struct HyprDesktop {
    ipc: ipc::Ipc,
    ids: Mutex<Ids>,
    /// The last pointer read and when (`POINTER_EVERY`).
    pointer: Mutex<Option<(Instant, (i32, i32))>>,
}

fn ours(ns_or_class: &str) -> bool {
    ns_or_class.starts_with(NS_PREFIX)
}

impl HyprDesktop {
    fn snap(&self) -> Result<Snap> {
        let v = self.ipc.batch(&["j/monitors", "j/clients", "j/layers"])?;
        let mons: Vec<HMon> = serde_json::from_value(v[0].clone())?;
        let clients: Vec<HClient> = serde_json::from_value(v[1].clone())?;
        let levels: HashMap<String, HLevels> = serde_json::from_value(v[2].clone())?;
        let mut layers = Vec::new();
        for (mon, l) in levels {
            for (lvl, ls) in l.levels {
                let lvl = lvl.parse().unwrap_or(0);
                layers.extend(ls.into_iter().map(|x| (mon.clone(), lvl, x)));
            }
        }
        Ok(Snap { mons: mons.into_iter().filter(|m| !m.disabled).collect(), clients, layers })
    }

    fn mons(&self) -> Vec<HMon> {
        self.ipc.json::<Vec<HMon>>("monitors").map(|m| m.into_iter().filter(|m| !m.disabled).collect()).unwrap_or_default()
    }

    fn id(&self, key: &str) -> WinId {
        self.ids.lock().map(|mut i| i.id(key)).unwrap_or(0)
    }

    fn key(&self, w: WinId) -> Option<String> {
        self.ids.lock().ok()?.key(w).map(str::to_string)
    }

    /// A client's id (and its address noted).
    fn client_id(&self, address: &str, stable: &str) -> WinId {
        let Ok(mut ids) = self.ids.lock() else { return 0 };
        let id = ids.id(&client_key(address, stable));
        ids.addr.insert(id, address.to_string());
        id
    }

    /// The client a JSON reply describes (`j/activewindow`).
    fn value_id(&self, v: &serde_json::Value) -> Option<WinId> {
        let a = v.get("address")?.as_str().filter(|a| !a.is_empty())?;
        Some(self.client_id(a, v.get("stableId").and_then(|s| s.as_str()).unwrap_or("")))
    }

    /// Window `w` and the world it is in (monitors and clients: one request).
    fn client(&self, w: WinId) -> Option<(Snap, HClient)> {
        let key = self.key(w)?;
        let v = self.ipc.batch(&["j/monitors", "j/clients"]).ok()?;
        let mons: Vec<HMon> = serde_json::from_value(v[0].clone()).ok()?;
        let clients: Vec<HClient> = serde_json::from_value(v[1].clone()).ok()?;
        let s = Snap { mons: mons.into_iter().filter(|m| !m.disabled).collect(), clients, layers: Vec::new() };
        let c = s.clients.iter().find(|c| client_key(&c.address, &c.stable_id) == key)?.clone();
        self.client_id(&c.address, &c.stable_id);
        Some((s, c))
    }

    /// The origin (root px) and scale of the output holding logical point (x, y) (the
    /// focused one if none does): where a layer surface with those bounds is made (the core
    /// names that output in the window's `display_id`).
    fn output_at(&self, x: f32, y: f32) -> Output {
        let ms = self.mons();
        let inside = |m: &&HMon| {
            let r = m.monitor(false);
            (r.x as f32..(r.x + r.w) as f32).contains(&x) && (r.y as f32..(r.y + r.h) as f32).contains(&y)
        };
        ms.iter().find(inside).or(ms.iter().find(|m| m.focused)).or(ms.first()).map_or(((0, 0), 1.0), |m| ((m.x, m.y), if m.scale > 0.0 { m.scale } else { 1.0 }))
    }

    /// Focus window `w` (an address id).
    fn focus(&self, w: WinId) -> Result<()> {
        let addr = self.ids.lock().ok().and_then(|i| i.addr.get(&w).cloned()).context("unknown window")?;
        let r = self.ipc.request(&format!("dispatch hl.dsp.focus({{ window = \"address:{addr}\" }})"))?;
        if r.trim() != "ok" {
            bail!("hyprland: focus: {r}");
        }
        Ok(())
    }
}

impl Desktop for HyprDesktop {
    fn caps(&self) -> Caps {
        // No geometry events on socket2: the core polls the perch target. Her effects are
        // painted in her window (one surface, one commit with her pixels), so every effect
        // step is a redraw and a present. A layer surface loses the pointer when Hyprland
        // re-checks focus under it (her window moving at a drag's start), button held or not.
        // A window's `at`/`size` is its goal: the compositor slides it there.
        Caps { shape_mode: false, target_events: false, fx_layers: false, costly_redraw: true, implicit_grab: false, goal_geometry: true, layout_events: true, host_per_output: true, keyboard_on_input: false }
    }

    fn root_size(&self) -> (i32, i32) {
        let ms: Vec<Monitor> = self.mons().iter().map(|m| m.monitor(false)).collect();
        (ms.iter().map(|m| m.x + m.w).max().unwrap_or(0), ms.iter().map(|m| m.y + m.h).max().unwrap_or(0))
    }

    /// The outputs (the lowest id is the primary: Hyprland has none of its own).
    fn monitors(&self) -> Vec<Monitor> {
        let ms = self.mons();
        let first = ms.iter().map(|m| m.id).min();
        ms.iter().map(|m| m.monitor(Some(m.id) == first)).collect()
    }

    /// Each monitor minus what layers with an exclusive zone reserve (`reserved`).
    fn work_areas(&self, mons: &[Monitor]) -> Vec<Monitor> {
        let hm = self.mons();
        mons.iter()
            .map(|m| {
                let r = hm.iter().find(|h| h.monitor(false).rect() == m.rect()).map(|h| h.reserved.clone()).unwrap_or_default();
                let g = |i: usize| r.get(i).copied().unwrap_or(0);
                Monitor { primary: m.primary, x: m.x + g(0), y: m.y + g(1), w: m.w - g(0) - g(2), h: m.h - g(1) - g(3) }
            })
            .collect()
    }

    fn refresh_at(&self, x: i32, y: i32) -> Option<f32> {
        self.mons().iter().find(|m| m.monitor(false).contains(x, y)).map(|m| m.refresh_rate)
    }

    /// Bottom → top per monitor: tiled clients, floating ones oldest focus first, fullscreen;
    /// the open special workspace above; then other clients' top and overlay layers (bars,
    /// notifications: they cover windows but nobody sits on them).
    fn windows(&self) -> Vec<Win> {
        let Ok(s) = self.snap() else { return Vec::new() };
        let mut cs: Vec<&HClient> = s.clients.iter().filter(|c| s.visible(c) && !ours(&c.class)).collect();
        cs.sort_by_key(|c| (s.special(c), c.fullscreen & 2 != 0, c.floating, -c.focus_history_id));
        let mut out: Vec<Win> = cs.iter().map(|c| Win { rect: WinRect { id: self.client_id(&c.address, &c.stable_id), x: c.at[0], y: c.at[1], w: c.size[0], h: c.size[1] }, managed: true }).collect();
        let mut ls: Vec<&(String, u8, HLayer)> = s.layers.iter().filter(|(_, lvl, l)| *lvl >= 2 && !ours(&l.namespace) && l.w > 0 && l.h > 0).collect();
        ls.sort_by_key(|(_, lvl, _)| *lvl);
        out.extend(ls.iter().map(|(_, _, l)| Win { rect: WinRect { id: self.id(&l.address), x: l.x, y: l.y, w: l.w, h: l.h }, managed: false }));
        out
    }

    fn active_window(&self) -> Option<WinId> {
        let v: serde_json::Value = self.ipc.json("activewindow").ok()?;
        if v.get("class").and_then(|c| c.as_str()).is_some_and(ours) {
            return None;
        }
        self.value_id(&v)
    }

    /// A mapped top or overlay layer asking for the keyboard exclusively has it (hers or
    /// another's); else the active window.
    fn focused_window(&self) -> Option<WinId> {
        let code = "local r = '' for _, l in ipairs(hl.get_layers()) do if l.mapped and l.keyboard_interactivity == 1 and l.layer >= 2 then r = l.namespace .. ' ' .. l.address .. ' ' .. l.x .. ' ' .. l.y end end return r";
        if let Ok(r) = self.ipc.lua(code)
            && let [ns, addr, x, y] = r.split(' ').collect::<Vec<_>>()[..]
        {
            if !ours(ns) {
                return Some(self.id(addr));
            }
            let (x, y) = (x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0));
            return Some(self.id(&our_key(ns, ns == namespace(Role::Body), self.output_at(x, y).0)));
        }
        let v: serde_json::Value = self.ipc.json("activewindow").ok()?;
        self.value_id(&v)
    }

    fn toplevel_of(&self, w: WinId) -> Option<WinId> {
        self.window_exists(w).then_some(w)
    }

    fn toplevel_rect(&self, w: WinId) -> Option<WinRect> {
        self.toplevel_place(w).map(|p| p.rect)
    }

    /// Its workspace is its space; a floating window moves without events.
    /// `windowsMove` (inheriting from `windows`, then `global`). Read when asked: once per move
    /// seen, nothing at rest. (A layout slid by a window opening starts on it too, measured in
    /// the rig; its last ~5 % overshoot it doesn't have.)
    fn move_anim(&self) -> Option<MoveAnim> {
        let r = self.ipc.batch(&["j/getoption animations:enabled", "j/animations"]).ok()?;
        move_anim(&r[0], &r[1])
    }

    fn toplevel_place(&self, w: WinId) -> Option<Place> {
        let (s, c) = self.client(w)?;
        s.visible(&c).then(|| Place { rect: WinRect { id: w, x: c.at[0], y: c.at[1], w: c.size[0], h: c.size[1] }, space: c.workspace.id, loose: c.floating })
    }

    fn window_exists(&self, w: WinId) -> bool {
        self.client(w).is_some()
    }

    fn is_fullscreen(&self, w: WinId) -> bool {
        self.client(w).is_some_and(|(_, c)| c.fullscreen & 2 != 0)
    }

    fn pointer(&self) -> Result<(i32, i32)> {
        let mut g = self.pointer.lock().map_err(|_| anyhow::anyhow!("pointer lock"))?;
        if let Some((t, p)) = *g
            && t.elapsed() < POINTER_EVERY
        {
            return Ok(p);
        }
        let v: serde_json::Value = self.ipc.json("cursorpos")?;
        let p = (v["x"].as_f64().unwrap_or(0.0).round() as i32, v["y"].as_f64().unwrap_or(0.0).round() as i32);
        *g = Some((Instant::now(), p));
        Ok(p)
    }

    fn pointer_reader(&self) -> Result<PointerRead> {
        let d = HyprDesktop { ipc: self.ipc.clone(), ids: Mutex::default(), pointer: Mutex::new(None) };
        Ok(Box::new(move || d.pointer().ok()))
    }

    /// Not yet (ext-idle-notify on a connection of its own, BRIEF-alive §4.0): the core counts
    /// from the pointer's last move meanwhile.
    fn idle_ms(&self) -> Option<u32> {
        None
    }

    fn watch_session(&self, on: Box<dyn Fn(super::SessionEv) + Send>) -> Result<()> {
        super::linux::watch_logind(on)
    }

    fn char_key_down(&self, _ch: char) -> Result<bool> {
        bail!("hyprland: no global key state")
    }

    fn compositor_running(&self) -> bool {
        true
    }

    /// socket2 events as hints; the core re-reads (and polls the perch target: no event
    /// says a window moved or resized).
    fn watch(&self, hint: Box<dyn Fn(Hint) + Send>) -> Result<()> {
        let ipc = self.ipc.clone();
        std::thread::Builder::new().name("avatar-hypr-events".into()).spawn(move || {
            ipc.events(|name, data| {
                if let Some(h) = event_hint(name, data) {
                    hint(h);
                }
            })
        })?;
        Ok(())
    }

    /// Wayland gives no client the clicks on other windows.
    fn watch_clicks(&self, _on: Box<dyn Fn(i32, i32) + Send>) -> Result<()> {
        Ok(())
    }

    fn arm_clicks(&self, _on: bool) {}
}

/// The window-move animation from `getoption animations:enabled` and `animations` (a pair of
/// lists: the leaves, a leaf not overridden inherits its parent's; the curves).
fn move_anim(enabled: &serde_json::Value, v: &serde_json::Value) -> Option<MoveAnim> {
    if enabled["bool"] == false || enabled["int"].as_i64() == Some(0) {
        return Some(MoveAnim { ms: 0.0, bezier: [0.0, 0.0, 1.0, 1.0] });
    }
    let (leaves, curves) = (v.get(0)?.as_array()?, v.get(1)?.as_array()?);
    let leaf = ["windowsMove", "windows", "global"].iter().find_map(|n| leaves.iter().find(|l| l["name"] == *n && l["overridden"] == true))?;
    if leaf["enabled"] == false {
        return Some(MoveAnim { ms: 0.0, bezier: [0.0, 0.0, 1.0, 1.0] });
    }
    let ms = leaf["speed"].as_f64()? as f32 * 100.0;
    let name = leaf["bezier"].as_str().unwrap_or("default");
    let bezier = curves.iter().find(|c| c["name"] == name).map_or([0.0, 0.0, 1.0, 1.0], |c| {
        let f = |k: &str| c[k].as_f64().unwrap_or(0.0) as f32;
        [f("X0"), f("Y0"), f("X1"), f("Y1")]
    });
    Some(MoveAnim { ms, bezier })
}

/// What a socket2 event tells the core (names: the wiki's IPC page; `workspacev2` fires for
/// the focused monitor only, so the core resolves workspaces from a fresh read).
fn event_hint(name: &str, data: &str) -> Option<Hint> {
    Some(match name {
        // Her own layers come and go (the Line, the reader): nothing outside changed.
        "openlayer" | "closelayer" if ours(data) => return None,
        "activewindowv2" => Hint::Focus,
        "workspacev2" | "focusedmonv2" | "moveworkspacev2" | "activespecialv2" => Hint::Desktop,
        "openwindow" | "closewindow" | "movewindowv2" | "changefloatingmode" | "pin" | "minimized" | "togglegroup" | "moveintogroup" | "moveoutofgroup" | "fullscreen" => Hint::Changed,
        // Reserved areas change only with layers (their exclusive zones) and the config.
        "monitoraddedv2" | "monitorremovedv2" | "configreloaded" | "openlayer" | "closelayer" | "(connected)" => Hint::Layout,
        _ => return None,
    })
}

/// An output's origin (root px) and scale.
type Output = ((i32, i32), f32);

/// One of her surfaces: its `wl_surface` (the `layer_ctl` key), its namespace, the origin and
/// scale of the output it is on.
#[derive(Clone)]
struct Surface {
    ptr: usize,
    ns: &'static str,
    output: (i32, i32),
    scale: f32,
}

pub struct LayerStage {
    desk: Arc<HyprDesktop>,
    surfaces: RefCell<HashMap<WinId, Surface>>,
    /// Where `dress` put each role (the output it was made on).
    dressed: RefCell<HashMap<&'static str, Output>>,
    body: std::cell::Cell<WinId>,
}

/// The id key of one of her surfaces: its namespace (body hosts: and their output).
fn our_key(ns: &str, host: bool, output: (i32, i32)) -> String {
    if host { format!("ours:{ns}@{},{}", output.0, output.1) } else { format!("ours:{ns}") }
}

fn namespace(role: Role) -> &'static str {
    match role {
        Role::Body => "capri-body",
        Role::Reader => "capri-reader",
        Role::Chip => "capri-chip",
        Role::Line => "capri-line",
        Role::Fx => "capri-fx",
        Role::Intro => "capri-intro",
    }
}

impl LayerStage {
    fn surface(&self, s: WinId) -> Option<Surface> {
        self.surfaces.borrow().get(&s).cloned()
    }

    fn body_surface(&self) -> Option<Surface> {
        self.surface(self.body.get())
    }

    fn margins(s: &Surface, x: i32, y: i32) -> [i32; 4] {
        let l = |v: i32, o: i32| ((v - o) as f32 / s.scale).round() as i32;
        [l(y, s.output.1), 0, 0, l(x, s.output.0)]
    }
}

impl Stage for LayerStage {
    /// A layer surface on the output under its bounds' centre, anchored top-left and placed
    /// by margins. Her body on Top (Hyprland hides it under a fullscreen window), the Line and
    /// the reader on Overlay (never hidden while they may hold the keyboard), the chip on Top.
    fn dress(&self, role: Role, o: &mut WindowOptions) {
        use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
        let ns = namespace(role);
        let b = o.window_bounds.map(|b| b.get_bounds()).unwrap_or_default();
        let at = b.origin;
        let (output, scale) = self.desk.output_at(f32::from(b.center().x), f32::from(b.center().y));
        self.dressed.borrow_mut().insert(ns, (output, scale));
        let m = |v: Pixels, o: i32| px(f32::from(v) - o as f32 / scale);
        o.kind = WindowKind::LayerShell(LayerShellOptions {
            namespace: ns.to_string(),
            layer: if matches!(role, Role::Line | Role::Reader) { Layer::Overlay } else { Layer::Top },
            anchor: Anchor::TOP | Anchor::LEFT,
            exclusive_zone: Some(px(-1.0)),
            margin: Some((m(at.y, output.1), px(0.0), px(0.0), m(at.x, output.0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        });
    }

    fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        let ptr = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Wayland(h)) => h.surface.as_ptr() as usize,
            other => bail!("not a Wayland window ({other:?}): GPUI and the Hyprland backend disagree"),
        };
        let ns = namespace(role);
        let (output, scale) = self.dressed.borrow().get(ns).copied().unwrap_or(((0, 0), 1.0));
        // One id per surface (body hosts share a namespace).
        let id = self.desk.id(&our_key(ns, matches!(role, Role::Body | Role::Fx), output));
        self.surfaces.borrow_mut().insert(id, Surface { ptr, ns, output, scale });
        if role == Role::Fx {
            // Effects only: every click goes through.
            layer_ctl::set_input_region(ptr, Some(&[]));
        }
        if role == Role::Body {
            if self.body.get() == 0 {
                self.body.set(id);
            } else {
                // A dormant host: nothing of hers, no input.
                layer_ctl::set_input_region(ptr, Some(&[]));
            }
        }
        Ok(id)
    }

    fn body(&self) -> WinId {
        self.body.get()
    }

    /// The host she left takes no input (its pixels go with the frame it draws empty).
    fn set_body(&self, s: WinId) {
        if let Some(old) = self.body_surface() {
            layer_ctl::set_input_region(old.ptr, Some(&[]));
        }
        self.body.set(s);
    }

    /// Margins, committed with the next frame (or alone if none is coming): her pixels and
    /// her place change in one commit.
    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        if let Some(s) = self.body_surface() {
            layer_ctl::set_margin(s.ptr, Self::margins(&s, x, y), Commit::WithFrame);
        }
        Ok(())
    }

    /// Top layers are above every window already.
    fn raise(&self) -> Result<()> {
        Ok(())
    }

    /// The place of one of her other surfaces (its size follows its window; P3).
    fn place(&self, s: WinId, x: i32, y: i32, _w: i32, _h: i32) -> Result<()> {
        if let Some(s) = self.surface(s) {
            layer_ctl::set_margin(s.ptr, Self::margins(&s, x, y), Commit::WithFrame);
        }
        Ok(())
    }

    /// The input region (device px → the surface's logical px); the visible region is all of
    /// her window, always (a compositor blends her alpha).
    fn set_shapes(&self, input: Option<&[ShapeRect]>, _visible: Option<Visible<'_>>) -> Result<()> {
        let (Some(s), Some(input)) = (self.body_surface(), input) else { return Ok(()) };
        let k = 1.0 / s.scale;
        let rects: Vec<Bounds<Pixels>> = input
            .iter()
            .map(|r| Bounds::new(point(px(r.0 as f32 * k), px(r.1 as f32 * k)), size(px(r.2 as f32 * k), px(r.3 as f32 * k))))
            .collect();
        layer_ctl::set_input_region(s.ptr, Some(&rects));
        Ok(())
    }

    /// The input region of surface `s` (a compositor blends the rest).
    fn shape_of(&self, s: WinId, input: &[ShapeRect], _visible: Visible<'_>) -> Result<()> {
        let s = self.surface(s).context("not one of her surfaces")?;
        let k = 1.0 / s.scale;
        let rects: Vec<Bounds<Pixels>> = input
            .iter()
            .map(|r| Bounds::new(point(px(r.0 as f32 * k), px(r.1 as f32 * k)), size(px(r.2 as f32 * k), px(r.3 as f32 * k))))
            .collect();
        layer_ctl::set_input_region(s.ptr, Some(&rects));
        Ok(())
    }

    fn set_icon(&self, _s: WinId, _data: &[u32]) -> Result<()> {
        Ok(())
    }

    // Effect layers are the core's display list here (`Caps::fx_layers` off): never called.
    fn fx_show(&self, _layer: usize, _rgb: u32, _origin: (i32, i32), _size: (i32, i32), _rects: Option<&[ShapeRect]>, _z: FxZ) -> Result<()> {
        Ok(())
    }
    fn fx_hide(&self, _layer: usize) {}
    fn fx_hide_all(&self) {}
    fn flush(&self) {}

    /// Exclusive keyboard interactivity: Hyprland gives her surface the keyboard in that commit.
    fn grab_keyboard(&self, s: WinId) -> Result<()> {
        let s = self.surface(s).context("not one of her surfaces")?;
        if !layer_ctl::set_keyboard_interactivity(s.ptr, layer_ctl::KeyboardInteractivity::Exclusive) {
            bail!("{}: not a layer surface", s.ns);
        }
        Ok(())
    }

    /// No keyboard for her surface; the keyboard goes to `to` when given (else Hyprland picks).
    fn release_keyboard(&self, s: WinId, to: Option<WinId>) -> Result<()> {
        let s = self.surface(s).context("not one of her surfaces")?;
        layer_ctl::set_keyboard_interactivity(s.ptr, layer_ctl::KeyboardInteractivity::None);
        if let Some(w) = to {
            self.desk.focus(w)?;
        }
        Ok(())
    }
}

/// The running Hyprland (its request socket answers), and a stage for her layer surfaces.
pub fn connect() -> Result<(Arc<HyprDesktop>, LayerStage)> {
    let ipc = ipc::Ipc::from_env().context("hyprland")?;
    let desk = Arc::new(HyprDesktop { ipc, ids: Mutex::default(), pointer: Mutex::new(None) });
    let stage = LayerStage { desk: desk.clone(), surfaces: RefCell::default(), dressed: RefCell::default(), body: std::cell::Cell::new(0) };
    Ok((desk, stage))
}

#[cfg(test)]
mod tests {
    use super::move_anim;
    use serde_json::json;

    #[test]
    fn the_move_animation_inherits_and_takes_its_curve() {
        let leaves = |move_over: bool| {
            json!([[
                { "name": "global", "overridden": true, "bezier": "default", "enabled": true, "speed": 8.0 },
                { "name": "windows", "overridden": true, "bezier": "overshot", "enabled": true, "speed": 6.0 },
                { "name": "windowsMove", "overridden": move_over, "bezier": if move_over { "wind" } else { "" }, "enabled": true, "speed": if move_over { 5.0 } else { 0.0 } }
            ], [
                { "name": "wind", "X0": 0.05, "Y0": 0.9, "X1": 0.1, "Y1": 1.05 },
                { "name": "overshot", "X0": 0.13, "Y0": 0.99, "X1": 0.29, "Y1": 1.1 }
            ]])
        };
        let on = json!({ "bool": true });
        let a = move_anim(&on, &leaves(true)).unwrap();
        assert_eq!((a.ms, a.bezier), (500.0, [0.05, 0.9, 0.1, 1.05]));
        // windowsMove not set: windows' (the leaf's parent).
        let a = move_anim(&on, &leaves(false)).unwrap();
        assert_eq!((a.ms, a.bezier), (600.0, [0.13, 0.99, 0.29, 1.1]));
        // Animations off: it jumps.
        assert_eq!(move_anim(&json!({ "int": 0 }), &leaves(true)).unwrap().ms, 0.0);
        assert_eq!(move_anim(&json!({ "option": "animations:enabled", "bool": false, "set": true }), &leaves(true)).unwrap().ms, 0.0);
        assert_eq!(move_anim(&json!({ "option": "animations:enabled", "bool": true, "set": true }), &leaves(true)).unwrap().ms, 500.0);
        // The curve: there at the end, early most of the way (wind), past it on the way (overshoot).
        let a = move_anim(&on, &leaves(true)).unwrap();
        assert!(a.progress(0.0).abs() < 1e-3 && (a.progress(1.0) - 1.0).abs() < 1e-3);
        assert!(a.progress(0.2) > 0.6, "{}", a.progress(0.2));
        assert!((0..=100).any(|i| a.progress(i as f32 / 100.0) > 1.0));
    }
}
