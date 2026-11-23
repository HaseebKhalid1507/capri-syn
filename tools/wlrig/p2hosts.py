#!/usr/bin/env python3
"""p2hosts.py BIN PACK: her body hosts across outputs in the rig (BRIEF-wayland §2.3, §5 P2): one capri-body layer per
output; a teleport to HEADLESS-2 shows her there (and nothing of her is left on WAYLAND-1); back; HEADLESS-2 removed
while she's on it: she comes down on WAYLAND-1 and its host is gone; added again: a dormant host for it. A drag from
WAYLAND-1 let go over HEADLESS-2, with no window focused and with one: held to the release (her window follows the
pointer), let go there, the keyboard where it was. `drop` while his press holds her: let go. Her Line summoned
mid-drag: held on, the Line takes the keyboard at the release.
Inside the sandbox only (run.sh python3 p2hosts.py ...). One line per check."""
import json, os, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from rigapp import RigApp  # noqa: E402
from shoot import ctl  # noqa: E402
from wlworld import hyprctl  # noqa: E402

BIN, PACK = sys.argv[1], sys.argv[2]
LOG = os.path.join(os.environ["RIG"], "out", "p2hosts-app.log")
fails = 0


def line(name, ok, detail):
    global fails
    fails += not ok
    print(f"{name} {'PASS' if ok else 'FAIL'} {detail}", flush=True)


def state():
    return json.loads(ctl("state"))


def mons():
    return {m["name"]: m for m in json.loads(hyprctl("-j", "monitors"))}


def bodies():
    """capri-body layers per output: (monitor, x, y, w, h)."""
    out = []
    for mon, v in json.loads(hyprctl("-j", "layers")).items():
        for ls in v["levels"].values():
            out += [(mon, l["x"], l["y"], l["w"], l["h"]) for l in ls if l["namespace"] == "capri-body"]
    return sorted(out)


def body_ki():
    """Her body hosts' keyboard interactivity (0 none, 1 exclusive, 2 on demand), one digit each."""
    return hyprctl("repl", "local r = '' for _, l in ipairs(hl.get_layers()) do if l.namespace == 'capri-body' then "
                           "r = r .. l.keyboard_interactivity end end return r").strip()


def line_ki():
    """Her Line's keyboard interactivity as Hyprland has it (0 none, 1 exclusive); '' with no Line mapped."""
    return hyprctl("repl", "local r = '' for _, l in ipairs(hl.get_layers()) do if l.namespace == 'capri-line' and l.mapped "
                           "then r = r .. l.keyboard_interactivity end end return r").strip()


def grab(x, y, w, h):
    """RGB bytes of a screen rect."""
    b = subprocess.run(["grim", "-g", f"{x},{y} {w}x{h}", "-t", "ppm", "-"], capture_output=True).stdout
    nl = 0
    for i, c in enumerate(b):
        nl += c == 10
        if nl == 3:
            return b[i + 1:]
    return b


def changed(a, b):
    return sum(1 for i in range(0, min(len(a), len(b)), 3) if abs(a[i] - b[i]) + abs(a[i + 1] - b[i + 1]) + abs(a[i + 2] - b[i + 2]) > 24)


def until(f, s=5.0, dt=0.05):
    end = time.time() + s
    while time.time() < end:
        v = f()
        if v:
            return v
        time.sleep(dt)
    return None


hyprctl("output", "create", "headless", "HEADLESS-2")
until(lambda: len(mons()) == 2)
time.sleep(0.5)
with RigApp(BIN, PACK, LOG) as app:
    ms = mons()
    m2 = ms["HEADLESS-2"]
    b = bodies()
    line("hosts-one-per-output", sorted(x[0] for x in b) == ["HEADLESS-2", "WAYLAND-1"], f"{b}")
    st = state()
    win = st["window"]
    # Teleport onto HEADLESS-2.
    gx, gy = m2["x"] + 600 + st["ground"][0] - win["x"], m2["y"] + m2["height"]
    ctl(f"pos {gx} {gy}")
    until(lambda: not state().get("teleporting") and state()["window"]["x"] >= m2["x"], 6)
    time.sleep(1.0)
    st = state()
    w2 = st["window"]
    on2 = grab(w2["x"], w2["y"], w2["w"], w2["h"])
    old1 = grab(win["x"], win["y"], win["w"], win["h"])
    # Her pixels on HEADLESS-2: compare with the same rect's backdrop (her host's pixels hidden by a brief `hide`).
    ctl("hide")
    time.sleep(0.6)
    bare2 = grab(w2["x"], w2["y"], w2["w"], w2["h"])
    bare1 = grab(win["x"], win["y"], win["w"], win["h"])
    ctl("show")
    time.sleep(0.6)
    px2 = changed(on2, bare2)
    left1 = changed(old1, bare1)
    line("teleport-to-HEADLESS-2", w2["x"] >= m2["x"] and px2 > 2000 and left1 < 50,
         f"her window {w2}; her pixels on HEADLESS-2 {px2}, left on WAYLAND-1 {left1}; layers {bodies()}")
    # Back to WAYLAND-1.
    ctl(f"pos 900 {ms['WAYLAND-1']['height']}")
    until(lambda: not state().get("teleporting") and state()["window"]["x"] < m2["x"], 6)
    time.sleep(1.0)
    st = state()
    line("teleport-back", st["window"]["x"] + st["window"]["w"] // 2 < m2["x"], f"her window {st['window']}")
    # Onto HEADLESS-2 again, then remove it under her.
    ctl(f"pos {gx} {gy}")
    until(lambda: not state().get("teleporting") and state()["window"]["x"] >= m2["x"], 6)
    time.sleep(1.0)
    hyprctl("output", "remove", "HEADLESS-2")
    time.sleep(2.5)
    st = state()
    m1 = mons()["WAYLAND-1"]
    on1 = st["window"]["x"] + st["window"]["w"] // 2 < m1["x"] + m1["width"]
    line("hotplug-removed", on1 and [x[0] for x in bodies()] == ["WAYLAND-1"], f"her window {st['window']} {st['placement']}; layers {bodies()}")
    hyprctl("output", "create", "headless", "HEADLESS-2")
    until(lambda: len(mons()) == 2)
    time.sleep(2.0)
    b = bodies()
    line("hotplug-added", sorted(x[0] for x in b) == ["HEADLESS-2", "WAYLAND-1"], f"{b}")
    # A drag across outputs: pressed on her in her WAYLAND-1 host, let go over HEADLESS-2. Hyprland keeps the pointer
    # on the surface pressed to the release only while some surface holds the keyboard (InputManager::mouseMoveUnified);
    # leaving it releases the button on the way out (CWLPointerResource::sendLeave). With a window focused it holds:
    # the release comes to her WAYLAND-1 host, no longer showing her, which hands it to her. With nothing focused her
    # host asks for the keyboard on demand while the press lasts (Stage::keep_pointer), so the same. Either way she's
    # held to the release, following the pointer onto HEADLESS-2 (not let go at the seam), then let go there, and the
    # keyboard is where it was: his window's, or no host of hers asking for it.
    w = app.world
    for focused in (False, True):
        ms = mons()
        m1, m2 = ms["WAYLAND-1"], ms["HEADLESS-2"]
        if focused:
            hyprctl("eval", 'hl.window_rule({ name = "p2-focus", match = { class = "^p2-focus$" }, float = true, move = "100 100", size = "300 200" })')
            w.toplevel("p2-focus", 300, 200)
            time.sleep(0.6)
        active = json.loads(hyprctl("-j", "activewindow") or "{}").get("class")
        ctl(f"pos 900 {m1['height']}")
        until(lambda: not state().get("teleporting") and state().get("rag") is None and state()["window"]["x"] < m2["x"], 6)
        time.sleep(1.0)
        st = state()
        xid0, av, win0 = st["xid"], st["avatar"], st["window"]
        sx, sy = av["x"] + av["w"] // 2, av["y"] + av["h"] // 2
        tx = m2["x"] + 600
        w.warp(sx, sy); time.sleep(0.2); w.button(True)
        for k in range(1, 61):
            w.warp(round(sx + (tx - sx) * k / 60), sy); time.sleep(1 / 120)
        time.sleep(0.3)
        held = state()
        ki_held = body_ki()
        w.button(False)
        let_go = until(lambda: (state().get("rag") or {}).get("phase") != "held", 2)
        until(lambda: state().get("rag") is None, 6)
        st = state()
        ki_after = body_ki()
        active_after = json.loads(hyprctl("-j", "activewindow") or "{}").get("class")
        on2 = st["window"]["x"] + st["window"]["w"] // 2 >= m2["x"]
        crossed = (held.get("rag") or {}).get("phase") == "held" and held["xid"] != xid0
        # "Following nothing": held, her window moved with the pointer (as far as it went, within her dangle).
        follows = abs((held["window"]["x"] - win0["x"]) - (tx - sx)) <= 60
        asked = "2" in ki_held
        kbd_ok = (active_after == "p2-focus" and not asked) if focused else (active is None and asked and "2" not in ki_after)
        line(f"drop-across-outputs-{'focused' if focused else 'unfocused'}",
             (active == "p2-focus") == focused and crossed and follows and bool(let_go) and on2 and kbd_ok,
             f"activewindow {active} -> {active_after}; body hosts' keyboard asks held {ki_held!r}, after {ki_after!r}; "
             f"held: host {xid0:#x} -> {held['xid']:#x}, window x {win0['x']} -> {held['window']['x']} (pointer {sx} -> {tx}), "
             f"rag {json.dumps(held.get('rag'))[:80]}; after the release: rag {json.dumps(st.get('rag'))[:60]}, her window {st['window']}")
    # `drop` while his press holds her lets her go; the button's release then finds no press.
    ctl(f"pos 900 {m1['height']}")
    until(lambda: not state().get("teleporting") and state().get("rag") is None, 6)
    time.sleep(1.0)
    av = state()["avatar"]
    sx, sy = av["x"] + av["w"] // 2, av["y"] + av["h"] // 2
    w.warp(sx, sy); time.sleep(0.2); w.button(True)
    for k in range(1, 11):
        w.warp(sx + 8 * k, sy); time.sleep(1 / 60)
    time.sleep(0.2)
    phase = (state().get("rag") or {}).get("phase")
    r = ctl("drop")
    let_go = until(lambda: (state().get("rag") or {}).get("phase") != "held", 2)
    w.button(False)
    line("drop-his-press", phase == "held" and r.strip() == "ok released" and bool(let_go), f"held {phase}; drop -> {r.strip()!r}")
    # Her Line summoned while he holds her (the talk key, Mod+C). Hyprland releases every button when a layer takes the
    # keyboard (CLayerSurface::takeKeyboardFocus): she was let go mid-drag. The grab waits for his release
    # (Os::grab_keyboard): she's held on, her Line asking for no keyboard meanwhile, and it has it after.
    ctl(f"pos 900 {m1['height']}")
    until(lambda: not state().get("teleporting") and state().get("rag") is None, 6)
    time.sleep(1.0)
    av = state()["avatar"]
    sx, sy = av["x"] + av["w"] // 2, av["y"] + av["h"] // 2
    w.warp(sx, sy); time.sleep(0.2); w.button(True)
    for k in range(1, 11):
        w.warp(sx + 8 * k, sy); time.sleep(1 / 60)
    summoned = ctl("talk press").strip()
    ctl("talk release")
    time.sleep(0.8)
    for k in range(11, 21):
        w.warp(sx + 8 * k, sy); time.sleep(1 / 60)
    time.sleep(0.2)
    phase = (state().get("rag") or {}).get("phase")
    ki_held = line_ki()
    w.button(False)
    until(lambda: (state().get("rag") or {}).get("phase") != "held", 2)
    ki_after = until(lambda: line_ki() == "1" and "1", 2)
    line("line-summoned-mid-drag", phase == "held" and ki_held == "0" and ki_after == "1",
         f"talk press -> {summoned!r}; while held: rag {phase}, her Line's keyboard asks {ki_held!r}; after the release {line_ki()!r}")
    ctl("line close")
    time.sleep(0.5)
    # No check of a press on her while his press holds her (Avatar::mouse_down, rule 3): real input can't make one here.
    # Hyprland drops a press of a button the client's wl_pointer still has down (CWLPointerResource::sendButton), and
    # every way it comes up there sends her its release first (sendLeave); GPUI has one wl_pointer. press_end_tests.
    w.destroy("p2-focus")
    time.sleep(0.3)
    log = open(LOG).read()
    line("no-errors", "panicked" not in log, "app log clean of panics")
hyprctl("output", "remove", "HEADLESS-2")
print(f"p2hosts: {fails} failed", flush=True)
sys.exit(1 if fails else 0)
