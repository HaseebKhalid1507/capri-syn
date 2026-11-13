#!/usr/bin/env python3
"""lockcheck.py A.mp4 B.mp4 [--crop x,y,w,h] [--min-hold 2]

SA_RAG_LOCKSTEP=1 recordings of the same script: the displayed frame sequences must match.
x11grab samples the screen on its own clock, so each clip is reduced to the frames that were
on screen for at least --min-hold captures in a row (a capture that caught a move half-done
is dropped), consecutive repeats merged. Only the drag itself is compared: from the first held
state that changes >= --big of the crop to the last one (idle blinks and breath before and
after run on the wall clock). Record with SA_RAG_TICK_MS >= 100 so every state is held for
3 captures. Lossless clips (SA_LOSSLESS=1). Exit 0 if identical.
"""
import argparse
import hashlib
import subprocess
import sys

import numpy as np


def seq(path, crop, min_hold):
    probe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "stream=width,height",
                            "-of", "csv=p=0", path], capture_output=True, text=True, check=True).stdout.split(",")
    W, H = int(probe[0]), int(probe[1])
    vf = []
    if crop:
        x, y, w, h = crop
        vf = ["-vf", f"crop={w}:{h}:{x}:{y}"]
        W, H = w, h
    p = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-i", path, *vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                         stdout=subprocess.PIPE)
    n = W * H * 3
    runs = []
    while True:
        b = p.stdout.read(n)
        if len(b) < n:
            break
        h = hashlib.sha1(b).hexdigest()
        if runs and runs[-1][0] == h:
            runs[-1][1] += 1
        else:
            runs.append([h, 1, np.frombuffer(b, np.uint8).reshape(H, W, 3)[::4, ::4].copy()])
    held = []
    for h, c, img in runs:
        if c >= min_hold and (not held or held[-1][0] != h):
            held.append((h, img))
    return held, len(runs)


def motion(held, big):
    """The held states from the first big change to the last one."""
    ch = [i for i in range(1, len(held))
          if (np.abs(held[i][1].astype(np.int16) - held[i - 1][1].astype(np.int16)).max(axis=2) > 16).mean() >= big]
    if not ch:
        return []
    return [h for h, _ in held[ch[0]:ch[-1] + 1]]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--crop")
    ap.add_argument("--min-hold", type=int, default=2)
    ap.add_argument("--big", type=float, default=0.01)
    a = ap.parse_args()
    crop = tuple(int(v) for v in a.crop.split(",")) if a.crop else None
    ha, ra = seq(a.a, crop, a.min_hold)
    hb, rb = seq(a.b, crop, a.min_hold)
    sa, sb = motion(ha, a.big), motion(hb, a.big)
    # Leading/trailing rest frames can differ in count only; compare the motion between.
    same = sa == sb and len(sa) > 0
    first = next((i for i, (x, y) in enumerate(zip(sa, sb)) if x != y), None)
    print(f"lockcheck: motion A {len(sa)} states ({ra} runs), B {len(sb)} states ({rb} runs): "
          f"{'IDENTICAL' if same else f'DIFFER (first at {first}, len {len(sa)} vs {len(sb)})'}")
    sys.exit(0 if same else 1)


if __name__ == "__main__":
    main()
