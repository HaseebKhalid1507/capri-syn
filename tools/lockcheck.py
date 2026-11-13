#!/usr/bin/env python3
"""lockcheck.py A.mp4 B.mp4 [--crop x,y,w,h] [--min-hold 2]

SA_RAG_LOCKSTEP=1 recordings of the same script: the displayed frame sequences must match.
x11grab samples the screen on its own clock, so each clip is reduced to the frames that were
on screen for at least --min-hold captures in a row (a capture that caught a move half-done
is dropped), consecutive repeats merged. Then the two sequences are compared pixel for pixel.
Lossless clips (SA_LOSSLESS=1). Exit 0 if identical.
"""
import argparse
import hashlib
import subprocess
import sys


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
            runs.append([h, 1])
    out = []
    for h, c in runs:
        if c >= min_hold and (not out or out[-1] != h):
            out.append(h)
    return out, len(runs)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--crop")
    ap.add_argument("--min-hold", type=int, default=2)
    a = ap.parse_args()
    crop = tuple(int(v) for v in a.crop.split(",")) if a.crop else None
    sa, ra = seq(a.a, crop, a.min_hold)
    sb, rb = seq(a.b, crop, a.min_hold)
    # Leading/trailing rest frames can differ in count only; compare the motion between.
    same = sa == sb
    first = next((i for i, (x, y) in enumerate(zip(sa, sb)) if x != y), None)
    print(f"lockcheck: A {len(sa)} held states ({ra} runs), B {len(sb)} held states ({rb} runs): "
          f"{'IDENTICAL' if same else f'DIFFER (first at {first}, len {len(sa)} vs {len(sb)})'}")
    sys.exit(0 if same else 1)


if __name__ == "__main__":
    main()
