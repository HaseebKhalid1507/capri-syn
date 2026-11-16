#!/usr/bin/env python3
"""seam.py BIN PACK OUT: a teleport from WAYLAND-1 to HEADLESS-2 across the seam, recorded on both sides of it
(two screencopy strips, 30 fps), stitched by presentation time into one strip; per frame, the bolt's red at the seam
on each side (a continuous line has it on both, or on neither). After landing: her effect hosts dormant (1×1).
Inside the sandbox only (run.sh python3 seam.py ...)."""
import json, os, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import numpy as np  # noqa: E402
from rigapp import RigApp  # noqa: E402
from shoot import ctl  # noqa: E402
from wlworld import Grab, encode, hyprctl, record_frames  # noqa: E402

BIN, PACK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "seam-app.log")
W, H = 640, 1440  # each strip: this wide, the shorter output's height
RED = np.array([0xff, 0x17, 0x38])


def mons():
    return {m["name"]: m for m in json.loads(hyprctl("-j", "monitors"))}


def fx_layers():
    out = []
    for mon, v in json.loads(hyprctl("-j", "layers")).items():
        for ls in v["levels"].values():
            out += [(mon, l["w"], l["h"]) for l in ls if l["namespace"] == "capri-fx"]
    return sorted(out)


hyprctl("output", "create", "headless", "HEADLESS-2")
for _ in range(50):
    if len(mons()) == 2:
        break
    time.sleep(0.1)
time.sleep(0.5)
with RigApp(BIN, PACK, LOG) as app:
    m1, m2 = mons()["WAYLAND-1"], mons()["HEADLESS-2"]
    seam = m2["x"]
    ctl(f"pos {seam - 500} {m1['height']} now")
    time.sleep(1.5)
    st = json.loads(ctl("state"))
    stops = [threading.Event(), threading.Event()]
    res = [None, None]

    def rec(i, r):
        res[i] = record_frames(Grab(), r, 30, stops[i])
    ts = [threading.Thread(target=rec, args=(0, (seam - W, 0, W, H))), threading.Thread(target=rec, args=(1, (seam, 0, W, H)))]
    for t in ts:
        t.start()
    time.sleep(0.5)
    ctl(f"pos {seam + 400} {m2['height']}")
    time.sleep(3.0)
    for s in stops:
        s.set()
    for t in ts:
        t.join()
    time.sleep(1.0)
    dormant = fx_layers()
left, right = res
print(f"seam: {len(left)} + {len(right)} frames", flush=True)
rows = []
strip = []
for t, px in left:
    a = np.frombuffer(px, dtype=np.uint8).reshape(H, W, 3)
    tr, pr = min(right, key=lambda f: abs(f[0] - t))
    b = np.frombuffer(pr, dtype=np.uint8).reshape(H, W, 3)
    red_l = (np.abs(a[:, -3:].astype(int) - RED).sum(axis=2) < 60).any(axis=1)
    red_r = (np.abs(b[:, :3].astype(int) - RED).sum(axis=2) < 60).any(axis=1)
    anyred = (np.abs(a.astype(int) - RED).sum(axis=2) < 60).sum() + (np.abs(b.astype(int) - RED).sum(axis=2) < 60).sum()
    rows.append((t, int(red_l.sum()), int(red_r.sum()), int(anyred), abs(tr - t)))
    if anyred:
        strip.append(np.concatenate([a, b], axis=1))
bolt = [r for r in rows if r[3] > 0]
both = [r for r in bolt if r[1] and r[2]]
one = [r for r in bolt if bool(r[1]) != bool(r[2])]
print(f"bolt frames {len(bolt)}; at the seam on both sides {len(both)}, on one side only {len(one)}", flush=True)
for r in bolt:
    print(f"  t {r[0]:.3f} seam rows red left {r[1]} right {r[2]} red px {r[3]} (pair dt {r[4] * 1000:.0f} ms)", flush=True)
if strip:
    encode([(i / 30, s.tobytes()) for i, s in enumerate(strip)], 2 * W, H, 30, os.path.join(OUT, "seam-bolt.mp4"))
print(f"effect hosts after landing: {dormant}", flush=True)
hyprctl("output", "remove", "HEADLESS-2")
