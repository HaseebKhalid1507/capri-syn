#!/usr/bin/env python3
"""sheet.py IN.mp4 OUT.png [--fps 6] [--n 12] [--start S|auto] [--crop x,y,w,h] [--cols 6] [--scale 0.5]
Contact sheet: N frames sampled at FPS from START (auto = 2 frames before the first change),
each labelled with its time."""
import glob, os, subprocess, sys, tempfile
from PIL import Image, ImageChops, ImageDraw

def main():
    a = sys.argv[1:]
    src, out = a.pop(0), a.pop(0)
    opt = {"--fps": "6", "--n": "12", "--start": "auto", "--crop": "", "--cols": "6", "--scale": "0.5"}
    while a:
        k = a.pop(0); opt[k] = a.pop(0)
    tmp = tempfile.mkdtemp()
    subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-i", src, "-vf", "fps=30", f"{tmp}/f%04d.png"], check=True)
    fs = sorted(glob.glob(f"{tmp}/f*.png"))
    if opt["--start"] == "auto":
        start = 0
        prev = Image.open(fs[0]).convert("L")
        for i, f in enumerate(fs[1:], 1):
            im = Image.open(f).convert("L")
            if ImageChops.difference(im, prev).point(lambda x: 255 if x > 30 else 0).getbbox():
                start = max(0, i - 2); break
            prev = im
    else:
        start = int(float(opt["--start"]) * 30)
    step = 30 / float(opt["--fps"])
    idx = [int(start + k * step) for k in range(int(opt["--n"]))]
    idx = [i for i in idx if i < len(fs)]
    ims = []
    for i in idx:
        im = Image.open(fs[i]).convert("RGB")
        if opt["--crop"]:
            x, y, w, h = (int(v) for v in opt["--crop"].split(","))
            im = im.crop((x, y, x + w, y + h))
        s = float(opt["--scale"])
        im = im.resize((int(im.width * s), int(im.height * s)), Image.LANCZOS)
        ImageDraw.Draw(im).text((6, 4), f"{i / 30:.2f}s", fill=(255, 255, 255))
        ims.append(im)
    cols = int(opt["--cols"]); rows = (len(ims) + cols - 1) // cols
    w, h = ims[0].size
    sheet = Image.new("RGB", (w * cols, h * rows), (0, 0, 0))
    for k, im in enumerate(ims):
        sheet.paste(im, ((k % cols) * w, (k // cols) * h))
    sheet.save(out)
    print(out, sheet.size, "frames", idx)
    for f in fs: os.remove(f)
    os.rmdir(tmp)

main()
