#!/usr/bin/env python3
"""fx.py BIN PACK OUT_DIR: her effects in the rig, recorded lossless at 30 fps (screencopy), for dark and stray
pixels (P1.6). Glints at idle, headpat hearts, music notes, then the contact shadow perched on a window.
Writes OUT_DIR/fx-floor.mp4 and fx-perch.mp4 and runs slabcheck.py on them (black slabs: present lag), checks
near-black pixels outside her box (her box per frame from SA_ATOMIC_LOG: her body's drawn shape, not the effects),
and that once the effects are over nothing is left outside it. Residue: perched on a tiled window that slides (a
neighbour opens above it), the scene with her vs the same scene once she has quit: anything that differs outside
her box (+ her contact shadow) is a surface of hers that isn't clear (her window, a dormant body host, an effect
host); the same on HEADLESS-2, where only her dormant hosts are. Inside the sandbox only."""
import json, os, subprocess, sys, threading, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import numpy as np  # noqa: E402
import shoot  # noqa: E402
from rigapp import BG, RigApp, around, drawn  # noqa: E402
from wlworld import Grab, encode, hyprctl, record_frames  # noqa: E402

BIN, PACK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "fx-app.log")
INK = "6,1,2"  # Capri's outline ink (slabcheck lists hits nearer it as art, not slabs)
bg = np.array([(BG >> 16) & 255, (BG >> 8) & 255, BG & 255], dtype=np.int16)


def region(pad_up=260, pad=120):
    win = json.loads(shoot.ctl("state"))["window"]
    x, y = max(0, win["x"] - pad), max(0, win["y"] - pad_up)
    return (x, y, min(win["w"] + 2 * pad, 2560 - x), min(win["h"] + pad_up, 1600 - y))


def outside(frames, R, since=0.0, dark=False):
    """Pixels outside her box (+4 px) in the frames from `since`: off the backdrop, or (dark) near-black."""
    states = drawn(LOG)[0]
    n = 0
    for t, px in frames:
        if t < since:
            continue
        f = np.frombuffer(px, dtype=np.uint8).reshape(R[3], R[2], 3).astype(np.int16)
        m = (f.max(axis=2) < 24) if dark else (np.abs(f - bg) > 8).any(axis=2)
        for x, y, w, h in around(states, t):
            x0, y0 = x - 4 - R[0], y - 4 - R[1]
            m[max(0, y0):max(0, y0 + h + 8), max(0, x0):max(0, x0 + w + 8)] = False
        n += int(m.sum())
    return n


def slabs(name):
    r = subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "slabcheck.py"),
                        os.path.join(OUT, name), "--bg", f"0x{BG:06x}", "--ink", INK], capture_output=True, text=True)
    return r.stdout.strip().splitlines()[-1] if r.stdout.strip() else r.stderr.strip()[-200:]


def rec(R, secs, steps):
    """Record R for `secs` while `steps` (time → ctl line) run; the frames."""
    g, frames, stop = Grab(), [], threading.Event()
    th = threading.Thread(target=lambda: frames.extend(record_frames(g, R, 30, stop)), daemon=True)
    th.start()
    t0 = time.monotonic()
    for at, line in steps:
        time.sleep(max(0.0, t0 + at - time.monotonic()))
        print(f"  {at:4.1f} s  {line}: {shoot.ctl(line)[:60]}", flush=True)
    time.sleep(max(0.0, t0 + secs - time.monotonic()))
    stop.set()
    th.join()
    return t0, frames


def save(frames, R, name):
    encode(frames, R[2], R[3], 30, os.path.join(OUT, name))


def grab1(R):
    """One frame of R, as int16 rows."""
    t, px = Grab().grab(*R)
    return np.frombuffer(px, dtype=np.uint8).reshape(R[3], R[2], 3).astype(np.int16)


def residue(a, b, boxes, R, shadow=0):
    """Pixels of a (with her) that differ from b (without) outside her boxes (+4 px, + `shadow` px below), and their box."""
    m = (np.abs(a - b) > 6).any(axis=2)
    for x, y, w, h in boxes:
        x0, y0 = x - 4 - R[0], y - 4 - R[1]
        m[max(0, y0):max(0, y0 + h + 8 + shadow), max(0, x0):max(0, x0 + w + 8)] = False
    ys, xs = np.nonzero(m)
    return int(m.sum()), ((int(xs.min()) + R[0], int(ys.min()) + R[1], int(xs.max()) + R[0], int(ys.max()) + R[1]) if len(ys) else None)


hyprctl("output", "create", "headless", "HEADLESS-2")
time.sleep(1.0)
H2 = next(m for m in json.loads(hyprctl("-j", "monitors")) if m["name"] == "HEADLESS-2")
RH = (H2["x"], H2["y"], 700, 900)
with RigApp(BIN, PACK, LOG, env={"SA_ATOMIC_LOG": "1"}) as r:
    R = region()
    t0, fr = rec(R, 16, [(4, "tap head"), (6, "music on"), (12, "music off")])
    save(fr, R, "fx-floor.mp4")
    print("  " + slabs("fx-floor.mp4"), flush=True)
    print(f"fx floor: {len(fr)} frames; effect pixels outside her box (hearts, glints, notes: > 0 means the check sees "
          f"them): {outside(fr, R)}; dark pixels outside her box: {outside(fr, R, dark=True)}; "
          f"anything left outside her box once the effects are over (last 1.5 s): {outside(fr, R, t0 + 14.5)}", flush=True)
    hyprctl("eval", 'hl.window_rule({ name = "fx-seat", match = { class = "^fx-seat$" }, float = true, move = "900 900", size = "900 500" })')
    r.world.toplevel("fx-seat", 900, 500)
    time.sleep(0.5)
    print("  perch:", shoot.ctl("perch sit")[:80], flush=True)
    time.sleep(4)
    R = region()
    t0, fr = rec(R, 8, [])
    save(fr, R, "fx-perch.mp4")
    print("  " + slabs("fx-perch.mp4"), flush=True)
    print(f"fx perch: {len(fr)} frames, placement {json.loads(shoot.ctl('state'))['placement']}; dark pixels outside her box "
          f"(the shadow belongs under her feet): {outside(fr, R, dark=True)}", flush=True)
    r.world.destroy("fx-seat")
    shoot.ctl("perch off")
    time.sleep(2.5)
    # Residue: perched on a tiled window; a neighbour opens above it and its top edge slides down.
    hyprctl("eval", 'hl.config({ general = { gaps_out = { top = 460, right = 14, bottom = 14, left = 14 } } })')
    # Focus styling off (dim, opacity, shadow): her quitting moves the focus.
    hyprctl("eval", 'hl.config({ decoration = { inactive_opacity = 1.0, dim_inactive = false, shadow = { enabled = false } } })')
    r.world.toplevel("res-a", 800, 600, 0x8a8f99)
    time.sleep(0.8)
    hyprctl("dispatch", 'hl.dsp.focus({ window = "class:^res-a$" })')
    print("  perch:", shoot.ctl("perch")[:80], flush=True)
    time.sleep(3)
    hyprctl("dispatch", 'hl.dsp.layout("preselect u")')
    r.world.toplevel("res-b", 800, 600, 0x6f7480)
    time.sleep(2.5)
    R = region()
    with_her, at, h2_with = grab1(R), time.monotonic(), grab1(RH)
    print(f"  residue scene: placement {json.loads(shoot.ctl('state'))['placement']}", flush=True)
    shoot.ctl("quit")
    r.app.wait(timeout=5)
    time.sleep(1.0)
    without, h2_without = grab1(R), grab1(RH)
    r.world.destroy("res-a")
    r.world.destroy("res-b")
for name, img in (("residue-with", with_her), ("residue-without", without)):
    a8 = np.ascontiguousarray(img.astype(np.uint8))
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{R[2]}x{R[3]}", "-i", "-",
                    os.path.join(OUT, name + ".png")], input=a8.tobytes(), check=True)
boxes = around(drawn(LOG)[0], at)
n, box = residue(with_her, without, boxes, R, shadow=40)
n2, box2 = residue(h2_with, h2_without, [], RH)
print(f"fx residue: perched after the slide, pixels that differ outside her box (+ shadow) with her vs after she quit: {n} {box}; "
      f"HEADLESS-2 (her dormant hosts only): {n2} {box2}", flush=True)
hyprctl("eval", 'hl.config({ general = { gaps_out = 14 } })')
hyprctl("eval", 'hl.config({ decoration = { inactive_opacity = 0.92, dim_inactive = true, shadow = { enabled = true } } })')
hyprctl("output", "remove", "HEADLESS-2")
