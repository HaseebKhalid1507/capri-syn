#!/usr/bin/env python3
"""p2.py BIN PACK: P2 perch acceptance in the rig (BRIEF-wayland §5 P2): follow a floating window dragged in steps,
glide with a tiled one that slides, the workspace rules (her monitor's switch: falls in place there; back within the
grace: stays; the other monitor's switch: nothing), close, the target sent to the other monitor's workspace, and IPC
requests at rest. Inside the sandbox only (run.sh python3 p2.py ...). One line per check."""
import json, os, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from rigapp import RigApp  # noqa: E402
from shoot import ctl  # noqa: E402
from wlworld import hyprctl  # noqa: E402

BIN, PACK = sys.argv[1], sys.argv[2]
LOG = os.path.join(os.environ["RIG"], "out", "p2-app.log")
REST_S = int(os.environ.get("REST_S", "60"))
fails = 0


def line(name, ok, detail):
    global fails
    fails += not ok
    print(f"{name} {'PASS' if ok else 'FAIL'} {detail}", flush=True)


def state():
    return json.loads(ctl("state"))


def dsp(code):
    r = hyprctl("dispatch", code).strip()
    assert r == "ok", f"dispatch {code}: {r}"


def ev(code):
    r = hyprctl("eval", code).strip()
    assert r == "ok", f"eval {code}: {r}"


def client(cls):
    return next((c for c in json.loads(hyprctl("-j", "clients")) if c["class"] == cls), None)


def mons():
    return {m["name"]: m for m in json.loads(hyprctl("-j", "monitors"))}


def until(f, s=4.0, dt=0.02):
    end = time.time() + s
    while time.time() < end:
        v = f()
        if v:
            return v
        time.sleep(dt)
    return None


def perched(st=None):
    st = st or state()
    return st["placement"]["kind"] in ("sit", "stand") and not st.get("teleporting")


def settled_perch(s=6.0):
    return until(lambda: perched() and state(), s, 0.05)


def ipc_count():
    """Requests other than the pointer watcher's (`Os::watch_pointer` polls the pointer on every backend)."""
    return sum(1 for l in open(LOG) if l.startswith("ipc-request") and "j/cursorpos" not in l)


def ptr_count():
    return sum(1 for l in open(LOG) if l.startswith("ipc-request j/cursorpos"))


def open_win(world, name, rect=None, rgb=0x8a8f99):
    """A toplevel: floating at rect (x, y, w, h), or tiled."""
    if rect:
        x, y, w, h = rect
        ev(f'hl.window_rule({{ name = "{name}", match = {{ class = "^{name}$" }}, float = true, move = "{x} {y}", size = "{w} {h}" }})')
        world.toplevel(name, w, h, rgb)
    else:
        world.toplevel(name, 800, 600, rgb)
    c = until(lambda: client(name))
    assert c, f"{name} mapped"
    time.sleep(0.6)  # the open animation
    return c


def sel(name):
    return f'window = "class:^{name}$"'


def perch_on(name):
    dsp(f"hl.dsp.focus({{ {sel(name)} }})")
    time.sleep(0.2)
    r = ctl("perch")
    st = settled_perch()
    return r, st


with RigApp(BIN, PACK, LOG, env={"SA_IPC_LOG": "1"}) as app:
    w = app.world
    hyprctl("output", "create", "headless", "HEADLESS-2")
    until(lambda: len(mons()) == 2)
    time.sleep(1.0)
    ms = mons()
    # IPC at rest, not perched.
    n0, p0 = ipc_count(), ptr_count()
    time.sleep(REST_S)
    n1, p1 = ipc_count(), ptr_count()
    # No polling: what's left are reads at her decisions (a look, a roam), a few a minute.
    line("ipc-rest-floor", (n1 - n0) * 60 / REST_S <= 6, f"{n1 - n0} requests in {REST_S} s on the floor (+ {p1 - p0} pointer-watch reads)")

    # 1. A floating window dragged in 10 steps (50 ms apart).
    c = open_win(w, "kit", (500, 700, 900, 500))
    r, st = perch_on("kit")
    line("perch-float", bool(st), f"{r}; {st and st['placement']}")
    if st:
        g0 = st["ground"]
        samples, stop = [], threading.Event()

        def sample():
            while not stop.is_set():
                samples.append((time.time(), tuple(state()["ground"])))
                time.sleep(0.005)
        th = threading.Thread(target=sample)
        th.start()
        time.sleep(0.2)
        sent = []
        for i in range(1, 11):
            dsp(f"hl.dsp.window.move({{ x = {500 + 30 * i}, y = {700 - 12 * i}, {sel('kit')} }})")
            sent.append((i, time.time(), (g0[0] + 30 * i, g0[1] - 12 * i)))
            time.sleep(0.05)
        time.sleep(0.8)
        stop.set()
        th.join()
        lags = []
        for i, t, want in sent:
            hit = next((ts for ts, g in samples if ts >= t and abs(g[0] - want[0]) <= 2 and abs(g[1] - want[1]) <= 2), None)
            lags.append((i, None if hit is None else round((hit - t) * 1000)))
        seen = [l for _, l in lags if l is not None]
        last = lags[-1][1]
        line("perch-drag-follow", last is not None and last <= 100 and len(seen) >= 7,
             f"lag ms per step (None: skipped) {lags}; last step {last} ms")
        n0 = ipc_count()
        time.sleep(REST_S)
        n1 = ipc_count()
        line("ipc-rest-perched-floating", (n1 - n0) * 60 / REST_S <= 450, f"{n1 - n0} requests in {REST_S} s perched on a floating window")
        ctl("perch off")
        until(lambda: state()["placement"]["kind"] == "ground", 4)
    w.destroy("kit")
    time.sleep(0.5)

    # 2. Tiled windows (room above them: a tall top gap), the IPC rate perched on one, then its
    #    edge slides (the split turned: it goes from the left half to the bottom half).
    ev('hl.config({ general = { gaps_out = { top = 460, right = 14, bottom = 14, left = 14 } } })')
    open_win(w, "tila")
    open_win(w, "tilb")
    r, st = perch_on("tila")
    line("perch-tiled", bool(st), f"{r}; {st and st['placement']}")
    if st:
        n0 = ipc_count()
        time.sleep(REST_S)
        n1 = ipc_count()
        line("ipc-rest-perched-tiled", (n1 - n0) * 60 / REST_S <= 450, f"{n1 - n0} requests in {REST_S} s perched on a tiled window")
        a0 = client("tila")
        hang = state()["ground"][1] - a0["at"][1]
        for msg in ("togglesplit", "swapsplit"):
            dsp(f'hl.dsp.layout("{msg}")')
            time.sleep(1.5)
            a = client("tila")
            if a["at"][1] > a0["at"][1] + 50:
                break
        st = state()
        # Seated, her feet hang a fixed distance below the edge (as at the perch).
        ok = perched(st) and abs(st["ground"][1] - a["at"][1] - hang) <= 2 and a["at"][0] <= st["ground"][0] <= a["at"][0] + a["size"][0]
        line("perch-tiled-slide", ok, f"window {a0['at']} {a0['size']} -> {a['at']} {a['size']}, her ground {st['ground']} {st['placement']}")
    w.destroy("tilb")
    time.sleep(1.0)

    # 3. Workspaces: her monitor's switch → falls in place there; back within the grace → stays;
    #    the other monitor's switch → nothing.
    r, st = perch_on("tila")
    if not st:
        line("perch-again", False, r)
    else:
        p0 = st["placement"]
        ev('hl.get_monitor("HEADLESS-2"):set_workspace(7)')
        time.sleep(1.0)
        st = state()
        line("ws-other-monitor", st["placement"] == p0, f"HEADLESS-2 → ws 7: {st['placement']}")
        ev('hl.get_monitor("WAYLAND-1"):set_workspace(2)')
        time.sleep(0.12)
        ev('hl.get_monitor("WAYLAND-1"):set_workspace(1)')
        time.sleep(1.0)
        st = state()
        line("ws-flick-back", st["placement"] == p0, f"WAYLAND-1 → 2 → 1 in 120 ms: {st['placement']}")
        ev('hl.get_monitor("WAYLAND-1"):set_workspace(2)')
        time.sleep(1.5)
        st = state()
        m1 = ms["WAYLAND-1"]
        on1 = m1["x"] <= st["ground"][0] < m1["x"] + m1["width"]
        line("ws-switch-falls", st["placement"]["kind"] == "ground" and on1, f"WAYLAND-1 → ws 2: {st['placement']} ground {st['ground']} (WAYLAND-1 x {m1['x']}..{m1['x'] + m1['width']})")
        ev('hl.get_monitor("WAYLAND-1"):set_workspace(1)')
        time.sleep(0.8)

    # 4. Close → falls in place; sent to the other monitor's workspace → falls in place, doesn't follow.
    r, st = perch_on("tila")
    if st:
        x0 = st["ground"][0]
        w.destroy("tila")
        time.sleep(1.2)
        st = state()
        line("close-falls", st["placement"]["kind"] == "ground" and abs(st["ground"][0] - x0) <= 4, f"{st['placement']} ground {st['ground']} (was x {x0})")
    open_win(w, "tilc")
    r, st = perch_on("tilc")
    if st:
        x0 = st["ground"][0]
        ws2 = mons()["HEADLESS-2"]["activeWorkspace"]["id"]
        dsp(f"hl.dsp.window.move({{ workspace = {ws2}, follow = false, {sel('tilc')} }})")
        time.sleep(1.5)
        st = state()
        line("moved-away-falls", st["placement"]["kind"] == "ground" and abs(st["ground"][0] - x0) <= 4,
             f"tilc → ws {ws2} on HEADLESS-2: {st['placement']} ground {st['ground']} (was x {x0})")
    w.destroy("tilc")
    ev('hl.config({ general = { gaps_out = 14 } })')
    hyprctl("output", "remove", "HEADLESS-2")
    time.sleep(0.5)

print(f"p2: {fails} failed", flush=True)
sys.exit(1 if fails else 0)
