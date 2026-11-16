#!/usr/bin/env python3
"""p2hosts.py BIN PACK: her body hosts across outputs in the rig (BRIEF-wayland §2.3, §5 P2): one capri-body layer per
output; a teleport to HEADLESS-2 shows her there (and nothing of her is left on WAYLAND-1); back; HEADLESS-2 removed
while she's on it: she comes down on WAYLAND-1 and its host is gone; added again: a dormant host for it.
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
    log = open(LOG).read()
    line("no-errors", "panicked" not in log, "app log clean of panics")
hyprctl("output", "remove", "HEADLESS-2")
print(f"p2hosts: {fails} failed", flush=True)
sys.exit(1 if fails else 0)
