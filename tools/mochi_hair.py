"""Red hair for mochi's intro close-ups (make-pack --mochi --mochi-hair red|flat|original).

In mochiiwuff/capri-desktop's close-ups (intro-*.webp, c011764, 1672x941) her hair is drawn black, like
her body, arms and hands; her avatar (CapriCore v1.0.4) has red hair with a fine starfield. This repaints
the hair only, at pack-build time, on the local copy: nothing here contains or describes those pixels
beyond the numbers below.

The mask: a measured head region (HEAD, close-up px; the nine share one framing) that leaves out the jaw,
the arms and the waving hand, minus her face (FACE: the eyes' black stays), times the ink (max(R,G,B) < INK). Ink within LINE px of anything else
opaque (her face, the white and purple linework) stays dark: that is the outline between hair and face.
Edges against the background keep their alpha, so the anti-aliasing survives.

  red   (default) her lit red, the back spikes in her shadow red, and a starfield like the avatar's
  flat  one flat red, outline kept
  original   untouched

Deterministic: no RNG, the stars come from an integer hash of (x, y), so a fixed input gives the same
pixels everywhere (pixel_sha); make-pack writes lossless WebP and pins its bytes in anim.json.
Needs numpy and Pillow (only for --mochi with a recolor)."""
import hashlib

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

MODES = ("red", "flat", "original")
SIZE = (1672, 941)
# CapriCore v1.0.4 capri-idle-happy.png, its hair: median lit, median shadow, median star speck.
RED = (251, 2, 51)
SHADOW = (182, 0, 20)
STAR = (255, 168, 200)
INK = 60          # max(R,G,B) below this is ink (her black)
LIT = 180         # max(R,G,B) at or above this is drawn colour (face, white, linework); between: halo/AA
LINE = 3          # px of outline kept dark around the hair (next to the face, the linework, the background)
INK_RGB = (2, 1, 7)  # her black (the median of the hair ink)
# Her hair: the spikes above and beside the face, down to the jaw line and above the arms and the
# waving hand (close-up px, measured on all nine frames).
HEAD = [(380, 0), (1250, 0), (1342, 72), (1305, 125), (1100, 372), (1020, 362), (683, 490), (600, 470),
        (500, 445), (440, 430), (380, 300)]
# Her face (the outer edge of its green, eyes inside): never hair.
FACE = [(604, 357), (626, 326), (634, 312), (662, 296), (755, 249), (760, 262), (778, 357), (828, 222),
        (980, 151), (1002, 207), (1017, 356), (920, 394), (764, 457), (678, 491)]
# The spikes behind the face take her shadow red: the far left one (soft edge), the low right one (a cel
# edge from the notch between the right spikes to its base).
BACK_SOFT = [(380, 150), (545, 150), (545, 470), (380, 470)]
BACK_CEL = [(1107, 213), (1345, 66), (1345, 140), (1100, 380), (1045, 372)]
# Starfield (avatar: ~25 specks per 10k hair px, most 1-2 px, a few up to ~50 px, some near white).
STARS_PER_PX = 0.0025
DUST_PER_PX = 0.004


def pixel_sha(im):
    """sha256 of the decoded pixels (size + RGBA bytes): independent of the encoder."""
    im = im.convert("RGBA")
    return hashlib.sha256(f"{im.width}x{im.height}\n".encode() + im.tobytes()).hexdigest()


def _poly(size, polys):
    w, h = size
    sx, sy = w / SIZE[0], h / SIZE[1]
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    for p in polys:
        d.polygon([(x * sx, y * sy) for x, y in p], fill=255)
    return np.asarray(m) > 0


def _hash(x, y, salt):
    """A 32-bit integer hash of pixel coordinates (wrapping uint32 arithmetic)."""
    h = (x.astype(np.uint32) * np.uint32(0x8DA6B343)) ^ (y.astype(np.uint32) * np.uint32(0xD8163841)) ^ np.uint32(salt)
    h ^= h >> np.uint32(15)
    h *= np.uint32(0x2C1B3C6D)
    h ^= h >> np.uint32(12)
    h *= np.uint32(0x297A2D39)
    h ^= h >> np.uint32(15)
    return h


def hair_mask(rgba):
    """Ink inside HEAD and outside FACE: bool HxW."""
    a = np.asarray(rgba)
    size = (a.shape[1], a.shape[0])
    ink = (a[..., :3].max(axis=2) < INK) & (a[..., 3] > 0)
    return ink & _poly(size, [HEAD]) & ~_poly(size, [FACE])


def _dilate(mask, px):
    im = Image.fromarray(mask.astype(np.uint8) * 255)
    return np.asarray(im.filter(ImageFilter.MaxFilter(2 * px + 1))) > 0


def _blur(f, r):
    im = Image.fromarray(np.clip(f * 255 + 0.5, 0, 255).astype(np.uint8))
    return np.asarray(im.filter(ImageFilter.GaussianBlur(r))).astype(np.float32) / 255


def _stars(mask):
    """Brightness 0..1 of the starfield over the hair, HxW float."""
    h, w = mask.shape
    s = np.zeros((h, w), np.float32)
    ys, xs = np.nonzero(mask)
    if not len(xs):
        return s
    k = _hash(xs, ys, 0x51A2)
    star = k < np.uint32(STARS_PER_PX * 2**32)
    dust = (k >= np.uint32(STARS_PER_PX * 2**32)) & (k < np.uint32((STARS_PER_PX + DUST_PER_PX) * 2**32))
    s[ys[dust], xs[dust]] = 0.25 + (k[dust] % 64).astype(np.float32) / 256
    img = Image.fromarray(np.zeros((h, w), np.uint8))
    d = ImageDraw.Draw(img)
    sx, sy, kk = xs[star], ys[star], _hash(xs[star], ys[star], 0x7E11)
    for x, y, v in zip(sx.tolist(), sy.tolist(), kk.tolist()):
        u = (v & 0xFFFF) / 65536
        r = 0 if u < 0.6 else 1 if u < 0.9 else 2 if u < 0.985 else 3
        lvl = 160 + (v >> 16) % 96
        if r == 0:
            d.point((x, y), fill=lvl)
        else:
            d.ellipse((x - r, y - r, x + r, y + r), fill=lvl)
    big = np.asarray(img.filter(ImageFilter.GaussianBlur(0.7))).astype(np.float32) / 255
    big = np.minimum(big * 1.6, 1.0)
    return np.maximum(s, big) * mask


def recolor(im, mode="red"):
    """The close-up with her hair repainted (mode red or flat); original returns it unchanged."""
    if mode not in MODES:
        raise ValueError(f"mochi hair: {mode!r} is not one of {', '.join(MODES)}")
    im = im.convert("RGBA")
    if mode == "original":
        return im
    a = np.asarray(im).astype(np.float32)
    size = im.size
    mx = a[..., :3].max(axis=2)
    region = (a[..., 3] > 0) & _poly(size, [HEAD]) & ~_poly(size, [FACE])
    hair = region & (mx < LIT) & _dilate(hair_mask(im), 2)
    lit = (a[..., 3] > 128) & (mx >= LIT)
    paint = hair_mask(im) & ~_dilate(lit | (a[..., 3] < 128), LINE)
    w = _blur(paint.astype(np.float32), 1.0) * hair
    # The outline: halo greys inside it go to ink; the 1 px next to drawn colour keeps its anti-aliasing.
    base = a[..., :3].copy()
    base[hair & ~_dilate(lit, 1)] = INK_RGB
    fill = np.empty(a.shape[:2] + (3,), np.float32)
    fill[:] = RED
    if mode == "red":
        back = np.maximum(_blur(_poly(size, [BACK_SOFT]).astype(np.float32), 6.0),
                          _blur(_poly(size, [BACK_CEL]).astype(np.float32), 1.0))[..., None]
        fill = fill * (1 - back) + np.array(SHADOW, np.float32) * back
        st = _stars(paint)[..., None]
        fill = fill * (1 - st) + np.array(STAR, np.float32) * st
        white = np.clip(st - 0.85, 0, None) / 0.15
        fill = fill * (1 - white) + 255 * white
    out = a.copy()
    out[..., :3] = base * (1 - w[..., None]) + fill * w[..., None]
    return Image.fromarray(np.clip(out + 0.5, 0, 255).astype(np.uint8), "RGBA")


def save(im, path):
    """Lossless WebP (the same extension the pack lists)."""
    im.save(path, format="WEBP", lossless=True, quality=100, method=4, exact=True)


def recolor_file(src, dst, mode):
    """make-pack's step: src (a pinned close-up) recoloured into dst."""
    with Image.open(src) as im:
        save(recolor(im, mode), dst)
