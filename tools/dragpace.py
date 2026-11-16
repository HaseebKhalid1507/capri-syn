#!/usr/bin/env python3
"""dragpace.py CLIP.mp4 [--bg 0xRRGGBB] [--times CLIP.mp4.times] [--from S] [--to S]: drag pacing and lean, read
off a lossless clip of a drag (lagmeter recdrag on X11, tools/wlrig/atomic.py on Wayland). Backend-neutral.
dragpace.py --log APP.log: the same from her own log (SA_ATOMIC_LOG=1: the window origin each painted frame was
committed with; SA_FOLLOW_LOG=1 adds the lean each one drew): what she put up, frame by frame, free of the
capture's timing and of the chain's stretch (the top of her hair moves with it).

Per frame: her window's height on screen (the top of her ink: the status tag rides the window, so this is
where the window is), and her lean (hips to feet: the ink centroids at 45-65 % and 80-100 % of her height,
degrees from vertical). Reports the step sizes (px a frame), how unevenly they come (the change of step from one frame to
the next: 0 for a steady drag; a 9 -> 11 -> 5 -> 11 -> 5 hitch is 6), and the largest lean change in one frame.
Frames that repeat the one before (nothing new shown) are dropped; with --times (one presentation time per
frame) a change is scaled to one frame of the median period."""
import argparse
import statistics
import subprocess

import numpy as np


def frames(path):
    w, h = map(int, subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "stream=width,height",
                                     "-of", "csv=p=0", path], capture_output=True, text=True, check=True).stdout.strip().split(","))
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.uint8).reshape(-1, h, w, 3)


def measure(f, bg):
    """(top row, left column, lean): lean = the angle (degrees from vertical, feet to the right positive) from the
    ink centroid of her hips (45-65 % of her height) to that of her feet (80-100 %)."""
    m = (np.abs(f.astype(np.int16) - bg) > 8).any(axis=2)
    ys, xs = np.nonzero(m)
    if len(ys) < 50:
        return None
    top, bot = ys.min(), ys.max()
    h = bot - top

    def centroid(a, b):
        k = (ys >= top + a * h) & (ys <= top + b * h)
        return (xs[k].mean(), ys[k].mean()) if k.any() else None

    hip, feet = centroid(0.45, 0.65), centroid(0.80, 1.0)
    if not hip or not feet:
        return None
    lean = float(np.degrees(np.arctan2(feet[0] - hip[0], feet[1] - hip[1])))
    return int(top), int(xs.min()), lean


def pct(v, q):
    v = sorted(v)
    return v[min(len(v) - 1, int(q * len(v)))] if v else None


def report(name, rows):
    """rows: (t, y, x, lean or None), one per frame."""
    period = statistics.median(b[0] - a_[0] for a_, b in zip(rows, rows[1:])) if len(rows) > 2 else 1 / 60
    steps, uneven, dlean = [], [], []
    for (ta, ya, xa, la), (tb, yb, xb, lb) in zip(rows, rows[1:]):
        k = period / max(tb - ta, 1e-6)
        steps.append(((yb - ya) * k, (xb - xa) * k))
        if la is not None and lb is not None:
            dlean.append((abs(lb - la) * k, tb))
    for s0, s1 in zip(steps, steps[1:]):
        uneven.append(abs(s1[0] - s0[0]))
    print(f"dragpace {name}: {len(rows)} distinct frames, period {period * 1000:.1f} ms")
    print(f"  vertical steps px/frame (first 16): {[round(s[0], 1) for s in steps[:16]]}")
    print(f"  unevenness |step change| px: p50 {pct(uneven, 0.5):.2f}  p95 {pct(uneven, 0.95):.2f}  max {max(uneven):.2f}")
    if dlean:
        v = [d for d, _ in dlean]
        big = sorted(dlean, key=lambda d: -d[0])[:3]
        print(f"  lean change deg/frame: p50 {pct(v, 0.5):.2f}  p95 {pct(v, 0.95):.2f}  max {max(v):.2f} "
              f"(at t {', '.join(f'{t:.3f}' for _, t in big)})")


def from_log(path, t0, t1):
    """The held drag's painted frames from her log: from the first window move to the last."""
    paints, moves, leans = [], [], []
    for l in open(path):
        p = l.split()
        if l.startswith("atomic paint"):
            x, y = map(int, p[3].split(","))
            paints.append((float(p[2]), y, x))
        elif l.startswith("atomic move"):
            moves.append(float(p[2]))
        elif l.startswith("drawn "):
            leans.append((float(p[1]), float(p[5])))
    if not moves:
        return []
    a, b = moves[0], moves[-1]
    base = paints[0][0] if paints else a
    rows = []
    for t, y, x in paints:
        if a <= t <= b and t0 <= t - a <= t1:
            lean = next((ln for tl, ln in reversed(leans) if tl <= t + 1e-4), None)
            rows.append((t - base, y, x, lean if lean is not None and lean == lean else None))
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("clip", nargs="?")
    ap.add_argument("--log")
    ap.add_argument("--bg", default="0x203040")
    ap.add_argument("--times")
    ap.add_argument("--fps", type=float, default=60.0, help="the clip's rate when there are no --times")
    ap.add_argument("--from", dest="t0", type=float, default=0.0, help="seconds into the clip")
    ap.add_argument("--to", dest="t1", type=float, default=1e9)
    a = ap.parse_args()
    if a.log:
        report(a.log, from_log(a.log, a.t0, a.t1))
        return
    bgv = int(a.bg, 16)
    bg = np.array([(bgv >> 16) & 255, (bgv >> 8) & 255, bgv & 255], dtype=np.int16)
    fr = frames(a.clip)
    ts = [float(l) for l in open(a.times)] if a.times else [i / a.fps for i in range(len(fr))]
    ts = [t - ts[0] for t in ts]
    rows, prev = [], None
    for t, f in zip(ts, fr):
        if not a.t0 <= t <= a.t1:
            continue
        if prev is not None and np.array_equal(f, prev):
            continue
        prev = f
        q = measure(f, bg)
        if q:
            rows.append((t, *q))
    report(a.clip, [(t, y, x, lean) for t, y, x, lean in rows])


if __name__ == "__main__":
    main()
