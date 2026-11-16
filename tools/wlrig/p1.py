#!/usr/bin/env python3
"""p1.py BIN PACK: P1 acceptance in the rig (BRIEF-wayland §5 P1): she appears (first frame, her layer on Top,
no client of hers), idles (CPU over 3 × 60 s, view ticks), clicks through (follow_mouse = 0), drags (400 px in
40 steps at 120 Hz). Inside the sandbox only (run.sh python3 p1.py ...). One line per check."""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from rigapp import RigApp  # noqa: E402
from shoot import ctl  # noqa: E402
from wlworld import hyprctl  # noqa: E402

BIN, PACK = sys.argv[1], sys.argv[2]
IDLE_S = int(os.environ.get("IDLE_S", "60"))
LOG = os.path.join(os.environ["RIG"], "out", "p1-app.log")
fails = 0


def line(name, ok, detail):
    global fails
    fails += not ok
    print(f"{name} {'PASS' if ok else 'FAIL'} {detail}", flush=True)


def state():
    return json.loads(ctl("state"))


def ours():
    out = []
    for mon in json.loads(hyprctl("-j", "layers")).values():
        for lvl, ls in mon["levels"].items():
            out += [(l["namespace"], int(lvl), l) for l in ls if l["namespace"].startswith("capri-")]
    return out


def cpu_ticks(pid):
    f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return int(f[11]) + int(f[12])


hyprctl("eval", "hl.config({ input = { follow_mouse = 0 } })")
with RigApp(BIN, PACK, LOG) as r:
    app, w = r.app, r.world
    first = r.first_frame()
    ms = int(first.split()[2]) if first else None
    line("P1.2a", ms is not None and ms < 1000, f"first frame: {first}")
    ls = ours()
    line("P1.2b", any(ns == "capri-body" and lvl == 2 for ns, lvl, _ in ls), f"her layers: {[(ns, lvl) for ns, lvl, _ in ls]}")
    cl = [c["class"] for c in json.loads(hyprctl("-j", "clients"))]
    line("P1.2c", not any("capri" in c or "synaps" in c for c in cl), f"clients: {cl}")
    st = state()
    print("state:", json.dumps({k: st.get(k) for k in ("placement", "ground", "window", "avatar", "monitor")}), flush=True)

    # Idle: 3 × IDLE_S, her CPU and view ticks (Hyprland's pid is outside this namespace: hyprcpu.sh on the
    # host samples it; the windows' wall-clock bounds are printed for it).
    for i in range(3):
        a0, c0, t = cpu_ticks(app.pid), state()["counters"], time.time()
        time.sleep(IDLE_S)
        a1, c1, t1 = cpu_ticks(app.pid), state()["counters"], time.time()
        per_min = lambda k: (c1[k] - c0[k]) * 60 / (t1 - t)
        print(f"P1.3 idle {i + 1}: her CPU {(a1 - a0) / (t1 - t):.2f} %  view ticks/min {per_min('ticks'):.0f}  renders/min {per_min('renders'):.0f}  window {t:.2f} {t1:.2f}  load {open('/proc/loadavg').read().split()[0]}", flush=True)

    # Click-through: a window under her transparent pixels; a click there activates it, a click on her doesn't.
    st = state()
    win, av = st["window"], st["avatar"]
    hyprctl("eval", f'hl.window_rule({{ name = "p1-under", match = {{ class = "^p1-under$" }}, float = true, move = "{win["x"] - 100} {win["y"] - 100}", size = "{win["w"] + 200} {win["h"] + 200}" }})')
    hyprctl("eval", 'hl.window_rule({ name = "p1-other", match = { class = "^p1-other$" }, float = true, move = "100 100", size = "300 200" })')
    w.toplevel("p1-under", win["w"] + 200, win["h"] + 200)
    w.toplevel("p1-other", 300, 200)
    time.sleep(0.6)
    w.warp(250, 200); w.button(True); w.button(False); time.sleep(0.3)
    act = lambda: json.loads(hyprctl("-j", "activewindow") or "{}").get("class")
    before = act()
    # A transparent pixel inside her window: its top-left corner.
    w.warp(win["x"] + 3, win["y"] + 3); w.button(True); w.button(False); time.sleep(0.3)
    line("P1.4a", act() == "p1-under", f"click on a transparent pixel of her window: activewindow {before} -> {act()}")
    w.warp(250, 200); w.button(True); w.button(False); time.sleep(0.3)
    cx, cy = av["x"] + av["w"] // 2, av["y"] + av["h"] // 2
    w.warp(cx, cy); w.button(True); w.button(False); time.sleep(0.4)
    line("P1.4b", act() == "p1-other", f"click on her body: activewindow stays p1-other: {act()}; state mood {state().get('mood')}")
    w.destroy("p1-under"); w.destroy("p1-other"); time.sleep(0.3)

    # Drag: 400 px right in 40 steps at 120 Hz, from her torso.
    st = state()
    g0, av = st["ground"], st["avatar"]
    sx, sy = av["x"] + av["w"] // 2, av["y"] + av["h"] // 2
    w.warp(sx, sy); time.sleep(0.2); w.button(True)
    for k in range(1, 41):
        w.warp(sx + 10 * k, sy); time.sleep(1 / 120)
    time.sleep(0.15)
    held = state()
    w.button(False)
    time.sleep(2.5)
    st = state()
    line("P1.5", held.get("rag") is not None and abs(st["ground"][0] - (g0[0] + 400)) <= 60,
         f"ground {g0} -> {st['ground']} (pointer +400); while held: rag {json.dumps(held.get('rag'))[:120]}")
    print("app log tail:", [l.strip() for l in open(LOG)][-6:], flush=True)
print(f"P1 done, {fails} failed", flush=True)
