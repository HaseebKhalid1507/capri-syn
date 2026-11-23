//! The Hyprland backend: plain translation. The world outside her comes from Hyprland's IPC
//! (`ipc.rs`: monitors, clients, layers, the cursor, socket2 hints); her own surfaces are GPUI
//! layer-shell windows driven through the vendored GPUI's `layer_ctl` (PATCHES.md PL-1..PL-4).
//! Every rule built on these answers is the core's (`platform/os.rs`); the core polls the perch
//! target and paints effect layers in her window (`Caps`).

mod geometry;
mod ipc;
#[cfg(test)]
mod panel_test;
#[cfg(test)]
mod testkit;
// Compile the vendored pure configure regression tests in the app's normal CI unit suite.
#[cfg(test)]
#[path = "../../../vendor/gpui-pre-linux/src/linux/wayland/layer_size.rs"]
mod layer_size_tests;

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
    disabled: bool,
}

impl HMon {
    fn monitor(&self, primary: bool) -> Monitor {
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        Monitor { primary, x: self.x, y: self.y, w: (self.width as f32 / s).round() as i32, h: (self.height as f32 / s).round() as i32 }
    }

    /// The output less what layers with an exclusive zone reserve (`reserved`).
    fn work_area(&self, primary: bool) -> Monitor {
        let m = self.monitor(primary);
        let g = |i: usize| self.reserved.get(i).copied().unwrap_or(0);
        Monitor { primary, x: m.x + g(0), y: m.y + g(1), w: m.w - g(0) - g(2), h: m.h - g(1) - g(3) }
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

    /// Bottom → top per monitor: tiled clients, floating ones oldest focus first, fullscreen;
    /// the open special workspace above; then other clients' top and overlay layers that
    /// `occludes` (bars, notifications: they cover windows but nobody sits on them).
    fn stack(&self, ids: &mut Ids) -> Vec<Win> {
        let mut cs: Vec<&HClient> = self.clients.iter().filter(|c| self.visible(c) && !ours(&c.class)).collect();
        cs.sort_by_key(|c| (self.special(c), c.fullscreen & 2 != 0, c.floating, -c.focus_history_id));
        let mut out: Vec<Win> = cs.iter().map(|c| Win { rect: WinRect { id: ids.client(&c.address, &c.stable_id), x: c.at[0], y: c.at[1], w: c.size[0], h: c.size[1] }, managed: true }).collect();
        // The work area of the output a layer is on, by the name `hyprctl layers` files it under.
        let work = |name: &str| self.mons.iter().find(|m| m.name == name).map(|m| m.work_area(false).rect());
        let mut ls: Vec<&(String, u8, HLayer)> = self.layers.iter().filter(|(mon, lvl, l)| *lvl >= 2 && !ours(&l.namespace) && occludes(l, work(mon))).collect();
        ls.sort_by_key(|(_, lvl, _)| *lvl);
        out.extend(ls.iter().map(|(_, _, l)| Win { rect: WinRect { id: ids.id(&l.address), x: l.x, y: l.y, w: l.w, h: l.h }, managed: false }));
        out
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
    /// A client's id (and its address noted).
    fn client(&mut self, address: &str, stable: &str) -> WinId {
        let id = self.id(&client_key(address, stable));
        self.addr.insert(id, address.to_string());
        id
    }
}

pub struct HyprDesktop {
    ipc: ipc::Ipc,
    ids: Mutex<Ids>,
    /// The last pointer read and when (`POINTER_EVERY`).
    pointer: Mutex<Option<(Instant, (i32, i32))>>,
    /// Invalidated by socket2 layout events; placement never polls IPC per frame.
    monitors: Arc<Mutex<Option<Vec<HMon>>>>,
}

fn ours(ns_or_class: &str) -> bool {
    ns_or_class.starts_with(NS_PREFIX)
}

/// A layer this thin (px, either way) or thinner draws nothing to sit under: hot-edge triggers,
/// caelestia's 1x1 border-exclusion and quickshell probes. A layer that misses the work area by
/// no more than this on a side still covers it: a fractional scale rounds the sizes.
const SLIVER: i32 = 2;

/// Whether another client's top or overlay layer `l` covers the windows under it. `work` is the
/// work area of its output (logical px; None: the output is unknown). `hyprctl layers` reports
/// geometry only, never what is drawn or where clicks land, so the rule goes by geometry:
/// - A sliver (`SLIVER`) never does.
/// - A layer that contains the whole work area never does. Shells like caelestia (and some
///   quickshell setups; ags and eww mostly make a layer per widget) draw their bar, drawers and
///   popups on one transparent, mostly click-through layer over each output, so counting it hid
///   every window on that output (`edge hidden ... exposed []` for each). By the layer-shell
///   protocol a surface anchored to all four edges gets the whole output (exclusive zone -1) or
///   the output less what the other layers reserve (0): the work area. So containing it tells such
///   a canvas at any size, where a share of the output does not (a 48 px bar and dock leave a
///   canvas 87.5% of a 1366x768 output; a centred panel with a margin all round is 94% of one and
///   is a cover). A bar, dock, side panel (even a half-screen one) or notification never contains
///   it: it is thin one way.
///   The price: an opaque layer this big (a launcher's dimmer, a logout screen) lets her sit on
///   what it hides. `j/layers` doesn't say which layers take the keyboard, which would tell them.
/// - Anything else does.
fn occludes(l: &HLayer, work: Option<WinRect>) -> bool {
    let canvas = work.is_some_and(|a| a.w > 0 && a.h > 0 && l.x <= a.x + SLIVER && l.y <= a.y + SLIVER && l.x + l.w >= a.right() - SLIVER && l.y + l.h >= a.bottom() - SLIVER);
    l.w > SLIVER && l.h > SLIVER && !canvas
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
        let Ok(mut cached) = self.monitors.lock() else { return Vec::new() };
        if cached.is_none() {
            *cached = self.ipc.json::<Vec<HMon>>("monitors").ok().map(|m| m.into_iter().filter(|m| !m.disabled).collect());
        }
        cached.clone().unwrap_or_default()
    }

    fn id(&self, key: &str) -> WinId {
        self.ids.lock().map(|mut i| i.id(key)).unwrap_or(0)
    }

    fn key(&self, w: WinId) -> Option<String> {
        self.ids.lock().ok()?.key(w).map(str::to_string)
    }

    /// A client's id (and its address noted).
    fn client_id(&self, address: &str, stable: &str) -> WinId {
        self.ids.lock().map(|mut i| i.client(address, stable)).unwrap_or(0)
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

    /// Output holding a core logical point; deterministic first output if outside all.
    /// Its name binds both the stage and GPUI to the same wl_output.
    fn output_at(&self, x: f32, y: f32) -> HMon {
        let ms = self.mons();
        let inside = |m: &&HMon| {
            let r = m.monitor(false);
            (r.x as f32..(r.x + r.w) as f32).contains(&x) && (r.y as f32..(r.y + r.h) as f32).contains(&y)
        };
        ms.iter().find(inside).or(ms.first()).cloned().unwrap_or_default()
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
        mons.iter().map(|m| hm.iter().find(|h| h.monitor(false).rect() == m.rect()).map_or_else(|| m.clone(), |h| h.work_area(m.primary))).collect()
    }

    fn refresh_at(&self, x: i32, y: i32) -> Option<f32> {
        self.mons().iter().find(|m| m.monitor(false).contains(x, y)).map(|m| m.refresh_rate)
    }

    /// Every visible window and the layers over them (`Snap::stack`), bottom → top.
    fn windows(&self) -> Vec<Win> {
        let Ok(s) = self.snap() else { return Vec::new() };
        let Ok(mut ids) = self.ids.lock() else { return Vec::new() };
        s.stack(&mut ids)
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
            return Some(self.id(&our_key(ns, ns == namespace(Role::Body), &self.output_at(x, y).name)));
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
        let d = HyprDesktop { ipc: self.ipc.clone(), ids: Mutex::default(), pointer: Mutex::new(None), monitors: Arc::default() };
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

    /// None yet (`linux::tray`: where a StatusNotifierItem goes).
    fn tray(&self, icon: super::TrayIcon, instance: &str, on: Box<dyn Fn(super::TrayEv) + Send>) -> Result<Option<Box<dyn super::Tray>>> {
        super::linux::tray(icon, instance, on)
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
        let monitors = self.monitors.clone();
        std::thread::Builder::new().name("avatar-hypr-events".into()).spawn(move || {
            ipc.events(|name, data| {
                if let Some(h) = event_hint(name, data) {
                    if matches!(h, Hint::Layout) && let Ok(mut cache) = monitors.lock() { *cache = None; }
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

/// One of her surfaces: protocol key, namespace and immutable wl_output name.
/// All backend coordinates are Hyprland logical pixels (the core's sf is 1).
#[derive(Clone)]
struct Surface {
    ptr: usize,
    ns: &'static str,
    output: String,
}

pub struct LayerStage {
    desk: Arc<HyprDesktop>,
    surfaces: RefCell<HashMap<WinId, Surface>>,
    /// Where `dress` put each role (the output it was made on).
    dressed: RefCell<HashMap<&'static str, String>>,
    body: std::cell::Cell<WinId>,
}

/// The id key of one of her surfaces: its namespace (body hosts: and their output).
fn our_key(ns: &str, host: bool, output: &str) -> String {
    if host { format!("ours:{ns}@{output}") } else { format!("ours:{ns}") }
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

    fn monitor_named(&self, name: &str) -> Option<HMon> {
        self.desk.mons().into_iter().find(|m| m.name == name)
    }

    fn margins(&self, s: &Surface, x: i32, y: i32) -> Result<[i32; 4]> {
        let m = self.monitor_named(&s.output).context("surface output disappeared")?;
        Ok(geometry::margins(x, y, m.x, m.y))
    }
}

impl Stage for LayerStage {
    /// Hosts use the bounds' centre; per-moment layers use the active body host's output.
    /// Each layer is anchored top-left and placed
    /// by margins. Her body on Top (Hyprland hides it under a fullscreen window), the Line and
    /// the reader on Overlay (never hidden while they may hold the keyboard), the chip on Top.
    fn dress(&self, role: Role, o: &mut WindowOptions) {
        use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
        let ns = namespace(role);
        let mut b = o.window_bounds.map(|b| b.get_bounds()).unwrap_or_default();
        // Per-moment surfaces belong to her current body host, not the panel's centre
        // (which can lie off-screen during bootstrap) or the compositor's focus.
        let host = (!matches!(role, Role::Body | Role::Fx)).then(|| self.body_surface()).flatten();
        let output = host.and_then(|s| self.monitor_named(&s.output))
            .unwrap_or_else(|| self.desk.output_at(f32::from(b.center().x), f32::from(b.center().y)));
        if role == Role::Line {
            let a = output.work_area(false);
            let d = |v: Pixels| f32::from(v).round() as i32;
            let [x, y, w, h] = geometry::fit_bounds([d(b.origin.x), d(b.origin.y), d(b.size.width), d(b.size.height)], [a.x, a.y, a.w, a.h]);
            b = Bounds::new(point(px(x as f32), px(y as f32)), size(px(w as f32), px(h as f32)));
            o.window_bounds = Some(gpui_kit::WindowBounds::Windowed(b));
        }
        let at = b.origin;
        self.dressed.borrow_mut().insert(ns, output.name.clone());
        let m = |v: Pixels, o: i32| px(f32::from(v) - o as f32);
        o.kind = WindowKind::LayerShell(LayerShellOptions {
            namespace: ns.to_string(),
            layer: if matches!(role, Role::Line | Role::Reader) { Layer::Overlay } else { Layer::Top },
            anchor: Anchor::TOP | Anchor::LEFT,
            exclusive_zone: Some(px(-1.0)),
            margin: Some((m(at.y, output.y), px(0.0), px(0.0), m(at.x, output.x))),
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        });
    }

    fn pin_output(&self, o: &mut WindowOptions, cx: &gpui_kit::App) -> Result<()> {
        let WindowKind::LayerShell(layer) = &o.kind else { bail!("not a Hyprland layer") };
        // GPUI's integer-scale display bounds are NOT Hyprland's fractional-scale
        // logical bounds. Match wl_output v4 names, for hosts and panels alike.
        let name = self.dressed.borrow().get(layer.namespace.as_str()).cloned().context("layer has no output")?;
        let displays = cx.displays();
        // PlatformDisplay has no name accessor; WaylandDisplay's Debug includes its
        // wl_output name (also None before advertised), not just an opaque UUID.
        o.display_id = Some(layer_ctl::display_id_for_name(&name, &displays)
            .with_context(|| format!("wl_output {name:?} not yet advertised; GPUI displays (including names): {displays:?}"))?);
        Ok(())
    }

    fn body_area(&self) -> Option<WinRect> {
        Some(self.monitor_named(&self.body_surface()?.output)?.work_area(false).rect())
    }

    fn surface_monitor(&self, id: WinId) -> Option<Monitor> {
        Some(self.monitor_named(&self.surface(id)?.output)?.monitor(false))
    }

    fn needs_reopen(&self, id: WinId) -> bool {
        self.surface(id).is_some_and(|s| geometry::needs_reopen(&s.output, self.body_surface().as_ref().map(|b| b.output.as_str())))
    }

    fn adopt(&self, role: Role, window: &Window) -> Result<WinId> {
        let ptr = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Wayland(h)) => h.surface.as_ptr() as usize,
            other => bail!("not a Wayland window ({other:?}): GPUI and the Hyprland backend disagree"),
        };
        let ns = namespace(role);
        let output = self.dressed.borrow().get(ns).cloned().context("layer has no output name")?;
        // One id per surface (body hosts share a namespace).
        let id = self.desk.id(&our_key(ns, matches!(role, Role::Body | Role::Fx), &output));
        self.surfaces.borrow_mut().insert(id, Surface { ptr, ns, output });
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
            layer_ctl::set_margin(s.ptr, self.margins(&s, x, y)?, Commit::WithFrame);
        }
        Ok(())
    }

    /// Top layers are above every window already.
    fn raise(&self) -> Result<()> {
        Ok(())
    }

    /// The core measures the Line (Settings is the same surface) after its first layout.
    /// Margins alone leave its buffer at LINE_H0 forever (#145). Only the Line gets live
    /// width/height here; Reader follows and effect-host placement stay margin-only.
    fn place(&self, s: WinId, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        let s = self.surface(s).context("surface closed before place")?;
        let [x, y, w, h] = if s.ns == namespace(Role::Line) {
            let a = self.monitor_named(&s.output).context("Line output disappeared; recreate it on her body host")?.work_area(false);
            geometry::fit_bounds([x, y, w, h], [a.x, a.y, a.w, a.h])
        } else { [x, y, w, h] };
        let margins = self.margins(&s, x, y)?;
        if let Some(size) = geometry::placed_size(s.ns, w, h) {
            anyhow::ensure!(layer_ctl::set_bounds(s.ptr, margins, size), "layer closed before place");
        } else {
            anyhow::ensure!(layer_ctl::set_margin(s.ptr, margins, Commit::WithFrame), "layer closed before place");
        }
        Ok(())
    }

    /// The input region (core logical px → surface logical px); the visible region is all of
    /// her window, always (a compositor blends her alpha).
    fn set_shapes(&self, input: Option<&[ShapeRect]>, _visible: Option<Visible<'_>>) -> Result<()> {
        let (Some(s), Some(input)) = (self.body_surface(), input) else { return Ok(()) };
        // Shape rectangles, like margins, are already logical (core sf=1).
        let rects: Vec<Bounds<Pixels>> = input
            .iter()
            .map(|r| Bounds::new(point(px(r.0 as f32), px(r.1 as f32)), size(px(r.2 as f32), px(r.3 as f32))))
            .collect();
        layer_ctl::set_input_region(s.ptr, Some(&rects));
        Ok(())
    }

    /// The input region of surface `s` (a compositor blends the rest).
    fn shape_of(&self, s: WinId, input: &[ShapeRect], _visible: Visible<'_>) -> Result<()> {
        let s = self.surface(s).context("not one of her surfaces")?;
        // Shape rectangles, like margins, are already logical (core sf=1).
        let rects: Vec<Bounds<Pixels>> = input
            .iter()
            .map(|r| Bounds::new(point(px(r.0 as f32), px(r.1 as f32)), size(px(r.2 as f32), px(r.3 as f32))))
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
    let desk = Arc::new(HyprDesktop { ipc, ids: Mutex::default(), pointer: Mutex::new(None), monitors: Arc::default() });
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
