#!/usr/bin/env python3
"""perchslide.py BIN PACK OUT: does she ride a window that slides (Hyprland animates it to its goal), measured on
screen at every composited frame. Two cases: tiled (a neighbour opens above, the top edge slides down 300+ px;
a socket2 event) and floating (moved 300 px down by dispatch; no event: the poller finds it). Per frame: the
window's top edge (its colour, in a column clear of her) and her hair's top (her red); the gap's change from
before the move is how far she is off the edge. Writes OUT/perch-{tiled,float}.mp4 (+ .times). Inside the sandbox."""
import json, os, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import numpy as np  # noqa: E402
from rigapp import RigApp  # noqa: E402
from shoot import ctl  # noqa: E402
from wlworld import Grab, encode, hyprctl, record_frames  # noqa: E402

BIN, PACK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
SEAT, OTHER = (0x8a, 0x8f, 0x99), 0x3a5a40


def ev(code):
    assert hyprctl("eval", code).strip() == "ok", code


def dsp(code):
    assert hyprctl("dispatch", code).strip() == "ok", code


def measure(frames, R, xs):
    """Per frame: (t, the seat's top edge, her hair's top), root px."""
    out = []
    for t, px in frames:
        f = np.frombuffer(px, dtype=np.uint8).reshape(R[3], R[2], 3).astype(np.int16)
        col = f[:, xs[1]]
        seat = np.abs(col - np.array(SEAT)).max(axis=1) < 30  # active or dimmed
        run = np.convolve(seat, np.ones(12), "same") >= 12
        edge = int(np.argmax(run)) - 6 if run.any() else None
        her = f[:, xs[0] - 160:xs[0] + 160]
        red = (her[..., 0] > 180) & (her[..., 1] < 90) & (her[..., 2] < 120)
        rows = np.nonzero(red.any(axis=1))[0]
        out.append((t, None if edge is None else edge + R[1], int(rows.min()) + R[1] if len(rows) else None))
    return out


def run(name, setup, move):
    setup()
    print(f"{name}: perch", ctl("perch")[:60], flush=True)
    time.sleep(3.0)
    st = json.loads(ctl("state"))
    w, g = st["window"], st["ground"]
    R = (max(0, g[0] - 500), 0, 1000, 1600)
    xs = (g[0] - R[0], min(R[2] - 1, g[0] - R[0] + 380))
    stop, frames = threading.Event(), []
    th = threading.Thread(target=lambda: frames.extend(record_frames(Grab(), R, 0, stop)))
    th.start()
    time.sleep(0.4)
    t_move = time.monotonic()
    move()
    time.sleep(1.6)
    stop.set()
    th.join()
    encode(frames, R[2], R[3], 60, os.path.join(OUT, f"perch-{name}.mp4"))
    allm = measure(frames, R, xs)
    m = [x for x in allm if x[1] is not None and x[2] is not None]
    if not m or not any(t < t_move for t, _, _ in m) or not any(t >= t_move for t, _, _ in m):
        f0 = np.frombuffer(frames[0][1], dtype=np.uint8).reshape(R[3], R[2], 3) if frames else None
        print(f"{name}: nothing to measure ({len(frames)} frames, first {allm[:2]}, R {R}, xs {xs}, "
              f"column colours {[tuple(int(v) for v in f0[y, xs[1]]) for y in range(0, 1600, 100)] if f0 is not None else None})", flush=True)
        return
    rest = np.median([h - e for t, e, h in m if t < t_move])
    off = [(t - t_move, (h - e) - rest) for t, e, h in m if t >= t_move]
    moving = [o for o in off if o[0] < 1.0]
    worst = max(moving, key=lambda o: abs(o[1]))
    over = sum(1 for o in moving if abs(o[1]) > 6)
    edge0, edge1 = m[0][1], m[-1][1]
    print(f"{name}: edge {edge0} -> {edge1}; off the edge during the first 1 s of the move: worst {worst[1]:+.0f} px at "
          f"{worst[0] * 1000:.0f} ms, {over} of {len(moving)} frames > 6 px; settled {off[-1][1]:+.0f} px; "
          f"placement {json.loads(ctl('state'))['placement']['kind']}", flush=True)
    print(f"  {name} off px per frame (first 30): {[round(o[1]) for o in moving[:30]]}", flush=True)


with RigApp(BIN, PACK, os.path.join(OUT, "perchslide-app.log")) as r:
    w = r.world

    def tiled():
        ev('hl.config({ general = { gaps_out = { top = 460, right = 14, bottom = 14, left = 14 } } })')
        w.toplevel("ps-a", 800, 600, 0x8a8f99)
        time.sleep(0.8)
        dsp('hl.dsp.focus({ window = "class:^ps-a$" })')

    def tiled_move():
        dsp('hl.dsp.layout("preselect u")')
        w.toplevel("ps-b", 800, 600, OTHER)

    cases = os.environ.get("CASES", "tiled,float").split(",")
    if "tiled" in cases:
        run("tiled", tiled, tiled_move)
        w.destroy("ps-a")
        w.destroy("ps-b")
        ev('hl.config({ general = { gaps_out = 14 } })')
        ctl("perch off")
        time.sleep(2.5)

    def floating():
        ev('hl.window_rule({ name = "ps-f", match = { class = "^ps-f$" }, float = true, move = "700 500", size = "1100 600" })')
        w.toplevel("ps-f", 1100, 600, 0x8a8f99)
        time.sleep(0.8)
        dsp('hl.dsp.focus({ window = "class:^ps-f$" })')

    if "float" in cases:
        run("float", floating, lambda: dsp('hl.dsp.window.move({ x = 700, y = 820, window = "class:^ps-f$" })'))
        w.destroy("ps-f")
