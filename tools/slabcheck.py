#!/usr/bin/env python3
"""slabcheck.py CLIP.mp4 [--crop x,y,w,h] [--bg 0x34404d] [--tol 10] [--save DIR]

Black slabs: un-painted window pixels showing inside the bounding shape (present lag, no
compositor). They are near-black rectangles with a straight edge on the backdrop: a run of
>= 8 px near-black on three rows with the same ends (±1 px, for H.264), the row above or below
the block plain backdrop along the whole run. Hand-drawn outlines (soles, hair tips) taper, so
their row ends move; glitch bands carry art, whose sides aren't vertical.

Ink: a pack's dark outline art can be cut flat by a glitch band (the band above slides away),
which looks the same. Unpainted pixels are neutral black (R ~ B ~ 0); --ink R,G,B marks a hit
whose mean is nearer that ink colour as "ink" (listed, not counted). Capri's ink is a dark red
(~6,1,2); a real slab measured 0.8,1.1,3.1.

Prints every hit (frame, x, y, w) and the total; exit 1 if any. Use lossless clips
(SA_LOSSLESS=1) or a tolerant --tol for H.264.
"""
import argparse
import os
import subprocess
import sys

import numpy as np


def frames(path, crop):
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
    i = 0
    while True:
        b = p.stdout.read(n)
        if len(b) < n:
            break
        yield i, np.frombuffer(b, np.uint8).reshape(H, W, 3)
        i += 1


def runs(row):
    """(x0, x1) of True runs in a bool row."""
    d = np.diff(np.concatenate(([0], row.astype(np.int8), [0])))
    return list(zip(np.flatnonzero(d == 1), np.flatnonzero(d == -1)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("clip")
    ap.add_argument("--crop")
    ap.add_argument("--bg", default="0x34404d")
    ap.add_argument("--tol", type=int, default=10)
    ap.add_argument("--min", type=int, default=8)
    ap.add_argument("--save")
    ap.add_argument("--ink", help="R,G,B of the pack's dark outline art (hits nearer it are listed as ink)")
    a = ap.parse_args()
    crop = tuple(int(v) for v in a.crop.split(",")) if a.crop else None
    bg = np.array([(int(a.bg, 16) >> s) & 255 for s in (16, 8, 0)], np.int16)
    ink = np.array([float(v) for v in a.ink.split(",")]) if a.ink else None
    hits = []
    inks = []
    nframes = 0
    for i, f in frames(a.clip, crop):
        nframes += 1
        f16 = f.astype(np.int16)
        dark = f16.max(axis=2) <= a.tol
        back = np.abs(f16 - bg).max(axis=2) <= a.tol
        H = f.shape[0]
        for y in range(1, H - 3):
            r0 = dark[y]
            if not r0.any():
                continue
            for x0, x1 in runs(r0):
                if x1 - x0 < a.min:
                    continue
                # The same ends (±1) on the next two rows: a rectangle.
                def same(yy):
                    rr = [r for r in runs(dark[yy]) if abs(r[0] - x0) <= 1 and abs(r[1] - x1) <= 1]
                    return bool(rr)
                if not (same(y + 1) and same(y + 2)):
                    continue
                # A straight edge on the backdrop just above or just below the block.
                if back[y - 1, x0:x1].all() or back[y + 3, x0:x1].all():
                    mean = f16[y:y + 3, x0:x1].reshape(-1, 3).mean(axis=0)
                    if ink is not None and np.linalg.norm(mean - ink) < np.linalg.norm(mean):
                        inks.append((i, x0, y, x1 - x0, mean))
                        continue
                    hits.append((i, x0, y, x1 - x0))
                    if a.save:
                        os.makedirs(a.save, exist_ok=True)
                        from PIL import Image
                        Image.fromarray(f).save(os.path.join(a.save, f"slab-{i:05d}.png"))
    for h in inks[:40]:
        print(f"ink (art cut by a band, not counted): frame {h[0]} at ({h[1]},{h[2]}) {h[3]} px wide, mean {h[4].round(1).tolist()}")
    for h in hits[:40]:
        print(f"slab: frame {h[0]} at ({h[1]},{h[2]}) {h[3]} px wide")
    print(f"slabcheck {a.clip}: {len(hits)} slab(s) in {nframes} frames")
    sys.exit(1 if hits else 0)


if __name__ == "__main__":
    main()
