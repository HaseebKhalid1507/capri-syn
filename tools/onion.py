#!/usr/bin/env python3
"""onion.py CLIP.mp4 OUT.png --from S --to S [--every 3] [--crop x,y,w,h] [--bg 0x34404d] [--scale 0.5]

One image of an arc: every Nth frame between --from and --to composited over the clean
backdrop, older frames fainter (35 % → 80 %), the last one solid. The figure is whatever
differs from the backdrop colour.
"""
import argparse
import subprocess

import numpy as np
from PIL import Image


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("clip")
    ap.add_argument("out")
    ap.add_argument("--from", dest="t0", type=float, required=True)
    ap.add_argument("--to", dest="t1", type=float, required=True)
    ap.add_argument("--every", type=int, default=3)
    ap.add_argument("--crop")
    ap.add_argument("--bg", default="0x34404d")
    ap.add_argument("--scale", type=float, default=0.5)
    a = ap.parse_args()
    vf = [f"select='between(t,{a.t0},{a.t1})*not(mod(n,{a.every}))'"]
    if a.crop:
        x, y, w, h = (int(v) for v in a.crop.split(","))
        vf.append(f"crop={w}:{h}:{x}:{y}")
    probe = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "stream=width,height", "-of", "csv=p=0", a.clip],
                           capture_output=True, text=True, check=True).stdout.split(",")
    W, H = (w, h) if a.crop else (int(probe[0]), int(probe[1]))
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", a.clip, "-vf", ",".join(vf), "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                         capture_output=True, check=True).stdout
    n = len(raw) // (W * H * 3)
    fr = np.frombuffer(raw[: n * W * H * 3], np.uint8).reshape(n, H, W, 3).astype(np.float32)
    bg = np.array([(int(a.bg, 16) >> s) & 255 for s in (16, 8, 0)], np.float32)
    out = np.broadcast_to(bg, (H, W, 3)).copy()
    for i, f in enumerate(fr):
        m = (np.abs(f - bg).max(axis=2) > 14)[..., None]
        al = 1.0 if i == n - 1 else 0.35 + 0.45 * i / max(1, n - 1)
        out = np.where(m, out * (1 - al) + f * al, out)
    im = Image.fromarray(out.clip(0, 255).astype(np.uint8))
    im = im.resize((int(W * a.scale), int(H * a.scale)), Image.LANCZOS)
    im.save(a.out)
    print(a.out, im.size, f"{n} frames")


if __name__ == "__main__":
    main()
