#!/usr/bin/env python3
"""atomic.py BIN PACK OUT_DIR [LIFT]: move/pixel atomicity during a drag in the rig (BRIEF-wayland §2.5, P1.5).

She runs with SA_ATOMIC_LOG=1: every window move and every painted frame is logged (CLOCK_MONOTONIC, the
origin, the bbox of what the frame draws). A virtual-pointer drag (the lagmeter figure-eight, 4 s) is recorded
at 30 fps through screencopy, each frame with its presentation time. A frame passes when her pixels' bbox is a
state she put on screen in the 100 ms before it: a painted frame at its origin, or a pure move (the last frame
at the new origin). A move landing without its frame, or a frame without its move, shows a bbox that is
neither. Writes OUT_DIR/drag.mp4 (lossless) + .times, prints one line per failing frame and a summary.
Inside the sandbox only (run.sh python3 atomic.py ...)."""
import json, math, os, subprocess, sys, threading, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import numpy as np  # noqa: E402
import shoot  # noqa: E402  (ctl)
from rigapp import BG, RigApp, around, drawn  # noqa: E402
from wlworld import Grab, encode, record_frames  # noqa: E402

BIN, PACK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
LIFT = int(sys.argv[4]) if len(sys.argv) > 4 else 200  # the figure-eight above the grip (lagmeter's, lifted)
TOL = 3
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "atomic-app.log")
with RigApp(BIN, PACK, LOG, env={"SA_ATOMIC_LOG": "1"}) as r:
    w = r.world
    time.sleep(1)
    st = json.loads(shoot.ctl("state"))
    a, win = st["avatar"], st["window"]
    x0, y0 = a["x"] + a["w"] // 2, a["y"] + int(a["h"] * 0.45)
    # The capture: her window plus the figure-eight's reach, on the first output.
    R = (max(0, win["x"] - 260), max(0, win["y"] - LIFT - 120), win["w"] + 520, min(win["h"] + LIFT + 180, 1600 - max(0, win["y"] - LIFT - 120)))
    g = Grab()
    frames, stop = [], threading.Event()
    th = threading.Thread(target=lambda: frames.extend(record_frames(g, R, 30, stop)), daemon=True)
    w.warp(x0, y0)
    time.sleep(0.3)
    th.start()
    time.sleep(0.5)
    w.button(True)
    time.sleep(0.25)
    t0 = time.monotonic()
    while time.monotonic() - t0 < 4.0:
        t = time.monotonic() - t0
        w.warp(round(x0 + 160 * math.sin(t * math.pi)), round(y0 - LIFT - 90 * math.sin(2 * t * math.pi) / 2))
        time.sleep(1 / 120)
    w.warp(x0, y0 - LIFT)
    time.sleep(0.4)
    w.button(False)
    time.sleep(1.5)
    stop.set()
    th.join()

states, n_paints, n_moves = drawn(LOG)

def png(path, rgb, w, h):
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}", "-i", "-", path], input=rgb.tobytes(), check=True)


H, W = R[3], R[2]
encode(frames, W, H, 30, os.path.join(OUT, "drag.mp4"))
bg = np.array([(BG >> 16) & 255, (BG >> 8) & 255, BG & 255], dtype=np.int16)
bad = checked = 0
edge_only = []
strip = []  # the first 24 checked frames (0.8 s at 30 fps), her state's box drawn over the pixels
held = (t0, t0 + 4.0)
# Per frame: the state she put up (in the 100 ms before it, or the last one before that) whose
# box sits nearest the pixels: position (both edges of an axis shifted) first, then extent.
for t, px in frames:
    if not held[0] + 0.1 <= t <= held[1]:
        continue
    a = np.frombuffer(px, dtype=np.uint8).reshape(H, W, 3).astype(np.int16)
    ys, xs = np.nonzero((np.abs(a - bg) > 8).any(axis=2))
    if len(xs) == 0:
        continue
    box = (R[0] + int(xs.min()), R[1] + int(ys.min()), int(xs.max() - xs.min()) + 1, int(ys.max() - ys.min()) + 1)
    checked += 1
    cands = around(states, t)
    if not cands:
        bad += 1
        print(f"frame t={t:.4f} bbox {box}: no state to compare", flush=True)
        continue
    def off(b):
        # A frame drawn at the wrong place has both edges of an axis off the same way; a tip
        # drawn past the body shape's rects moves one edge only. Shift = the common part.
        def axis(lo, hi):
            return (min(abs(lo), abs(hi)) if lo * hi > 0 else 0), abs(lo - hi)
        sx, ex = axis(box[0] - b[0], box[0] + box[2] - b[0] - b[2])
        sy, ey = axis(box[1] - b[1], box[1] + box[3] - b[1] - b[3])
        return max(sx, sy), max(ex, ey)
    near = min(cands, key=off)
    pos, ext = off(near)
    if len(strip) < 24:
        o = a.astype(np.uint8).copy()
        x, y, w, h = near[0] - R[0], near[1] - R[1], near[2], near[3]
        for xx, yy, ww, hh in ((x - 1, y - 1, w + 2, 1), (x - 1, y + h, w + 2, 1), (x - 1, y - 1, 1, h + 2), (x + w, y - 1, 1, h + 2)):
            o[max(0, yy):max(0, yy + hh), max(0, xx):max(0, xx + ww)] = (0, 255, 0)
        strip.append((t, o))
    if pos > TOL:
        bad += 1
        print(f"frame t={t:.4f} bbox {box}: nearest state {near}: position off by {pos} px", flush=True)
        png(os.path.join(OUT, f"disagree-{bad}.png"), a.astype(np.uint8), W, H)
    elif ext > TOL:
        edge_only.append(ext)
        print(f"frame t={t:.4f} bbox {box}: nearest state {near}: position within {pos} px, one edge off by {ext} px", flush=True)
        png(os.path.join(OUT, f"edge-{len(edge_only)}.png"), a.astype(np.uint8), W, H)
if strip:
    # One strip: the frames cropped to where she went, 8 wide.
    ink = [(np.abs(o.astype(np.int16) - bg) > 8).any(axis=2) for _, o in strip]
    xs = [np.nonzero(m.any(axis=0))[0] for m in ink]
    ys = [np.nonzero(m.any(axis=1))[0] for m in ink]
    cx0, cx1 = max(0, min(int(x.min()) for x in xs) - 8), min(W, max(int(x.max()) for x in xs) + 9)
    cy0, cy1 = max(0, min(int(y.min()) for y in ys) - 8), min(H, max(int(y.max()) for y in ys) + 9)
    rows = [np.concatenate([o[cy0:cy1, cx0:cx1] for _, o in strip[r:r + 8]], axis=1) for r in range(0, len(strip), 8)]
    rows = [np.pad(r, ((0, 0), (0, rows[0].shape[1] - r.shape[1]), (0, 0))) for r in rows]
    tile = np.concatenate(rows, axis=0)
    png(os.path.join(OUT, "strip.png"), np.ascontiguousarray(tile), tile.shape[1], tile.shape[0])
    print(f"strip: {len(strip)} consecutive frames from t={strip[0][0]:.3f} to {strip[-1][0]:.3f}, green = her state's box", flush=True)
print(f"atomic: {checked} frames checked during the held drag, position disagrees in {bad} (tolerance {TOL} px); "
      f"one edge only (extent) in {len(edge_only)} {sorted(edge_only)}; {n_paints} paints, "
      f"{n_moves} moves logged; {len(frames)} frames captured at 30 fps", flush=True)
