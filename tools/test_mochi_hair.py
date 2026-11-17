"""Tests for tools/mochi_hair.py (make-pack --mochi-hair): the mask on synthetic close-ups, determinism and
pinned output. Needs numpy and Pillow. python3 tools/test_mochi_hair.py
With MOCHI_DIR (a capri-desktop checkout at c011764, e.g. ~/.cache/capri-desktop-art/mochi) it also checks
the nine real close-ups: nothing outside the head changes, and the mask is the same hair in each."""
import os, sys, tempfile, unittest
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mochi_hair as mh

GREEN, WHITE, INKC = (0, 250, 90, 255), (250, 250, 250, 255), (1, 1, 7, 255)
SPIKE = [(850, 20), (960, 20), (905, 150)]
ARMS = [(1350, 500, 1600, 700), (100, 500, 350, 700), (1300, 200, 1600, 450)]
EYE = (840, 280, 880, 320)
STROKE = (600, 30, 630, 200)


def synthetic(size=mh.SIZE):
    """A close-up stand-in: a black spike in the hair, black arms and a waving hand, a green face with a
    black eye, a white stroke, on transparent; anti-aliased by drawing at x2 and downsampling."""
    sx, sy = size[0] / mh.SIZE[0], size[1] / mh.SIZE[1]
    im = Image.new("RGBA", (size[0] * 2, size[1] * 2), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    s = lambda pts: [(x * sx * 2, y * sy * 2) for x, y in pts]
    d.polygon(s(mh.FACE), fill=GREEN)
    d.ellipse(s([EYE[:2], EYE[2:]]), fill=INKC)
    d.polygon(s(SPIKE), fill=INKC)
    for r in ARMS:
        d.rectangle(s([r[:2], r[2:]]), fill=INKC)
    d.rectangle(s([STROKE[:2], STROKE[2:]]), fill=WHITE)
    return im.resize(size, Image.LANCZOS)


def box(a, r, inset=0):
    x0, y0, x1, y1 = r
    return a[y0 + inset:y1 - inset, x0 + inset:x1 - inset]


def spike_core():
    m = Image.new("L", mh.SIZE, 0)
    ImageDraw.Draw(m).polygon(SPIKE, fill=255)
    return mh._dilate(np.asarray(m) == 0, 8) == 0  # the spike, 8 px in from its edge


class Mask(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.im = synthetic()
        cls.src = np.asarray(cls.im).astype(int)
        cls.out = {m: np.asarray(mh.recolor(cls.im, m)).astype(int) for m in mh.MODES}

    def test_the_spike_is_the_mask_and_nothing_else_is(self):
        m = mh.hair_mask(self.im)
        core = spike_core()
        self.assertTrue(m[core].all(), "all of the spike's inside")
        for r in ARMS:
            self.assertFalse(box(m, r).any(), f"arm {r}")
        self.assertFalse(box(m, EYE).any(), "the eye")
        self.assertFalse(box(m, STROKE).any(), "the white stroke")
        whole = mh._poly(mh.SIZE, [SPIKE])
        self.assertLess(m.sum(), whole.sum() * 1.15, "no more than the spike and its anti-aliased edge")
        self.assertGreater(m.sum(), whole.sum() * 0.95, "all of it")

    def test_flat_paints_the_spike_red_and_leaves_the_rest(self):
        a = self.out["flat"]
        core = spike_core()
        self.assertTrue((np.abs(a[core][:, :3] - mh.RED).max(axis=1) <= 1).all(), "flat red inside")
        self.assertTrue((a[..., 3] == self.src[..., 3]).all(), "alpha untouched")
        changed = np.abs(a - self.src).max(axis=2) > 0
        region = mh._poly(mh.SIZE, [mh.HEAD]) & ~mh._poly(mh.SIZE, [mh.FACE])
        self.assertFalse((changed & ~region).any(), "nothing outside the head changes")
        for r in ARMS + [EYE, STROKE]:
            self.assertTrue((box(a, r) == box(self.src, r)).all(), f"{r} untouched")

    def test_red_keeps_a_dark_outline(self):
        a = self.out["flat"]
        outline = mh.hair_mask(self.im) & ~mh._dilate(~mh.hair_mask(self.im), 1)
        rim = outline & mh._dilate(self.src[..., 3] < 128, 1)
        self.assertTrue(rim.any())
        self.assertLess(a[rim][:, 0].mean(), 120, "the edge against the background stays dark")

    def test_red_has_her_shadow_and_stars(self):
        a = self.out["red"]
        core = spike_core()
        px = a[core][:, :3]
        lit = (np.abs(px - mh.RED).max(axis=1) <= 2).mean()
        star = ((px[:, 1] > 90) & (px[:, 0] > 200)).mean()
        self.assertGreater(lit, 0.9, "mostly her lit red")
        self.assertGreater(star, 0.001, "a starfield")
        self.assertLess(star, 0.05, "a subtle one")
        self.assertTrue((self.out["original"] == self.src).all(), "original is untouched")

    def test_any_size(self):
        small = synthetic((836, 470))
        m = mh.hair_mask(small)
        full = mh.hair_mask(self.im)
        self.assertAlmostEqual(m.sum() / full.sum(), 0.25, delta=0.03)
        self.assertTrue((np.asarray(mh.recolor(small, "red"))[..., 3] == np.asarray(small)[..., 3]).all())

    def test_bad_mode(self):
        with self.assertRaises(ValueError):
            mh.recolor(self.im, "blue")


# pixel_sha of recolor(synthetic(), mode): a fixed input gives these pixels (numpy + Pillow, no RNG).
PINNED = {
    "flat": "e9b110100a5a3ef3845da8d9a8132a18ae59a870eaaf59f4de9ddad2d5f55b80",
    "red": "14ad75b7aa911cd92995980e426e8917a2fcf51355d0cd84116fdf46140b1235",
}


class Determinism(unittest.TestCase):
    def test_same_input_same_pixels_and_pinned(self):
        im = synthetic()
        for mode, want in PINNED.items():
            a, b = mh.recolor(im, mode), mh.recolor(im.copy(), mode)
            self.assertEqual(a.tobytes(), b.tobytes(), mode)
            self.assertEqual(mh.pixel_sha(a), want, f"{mode}: pixels changed for a fixed input")

    def test_saved_webp_is_lossless_and_stable(self):
        im = synthetic()
        with tempfile.TemporaryDirectory() as t:
            src, d1, d2 = Path(t, "in.webp"), Path(t, "a.webp"), Path(t, "b.webp")
            mh.save(im, src)
            mh.recolor_file(src, d1, "red")
            mh.recolor_file(src, d2, "red")
            self.assertEqual(d1.read_bytes(), d2.read_bytes(), "same bytes twice (what anim.json pins)")
            with Image.open(d1) as got:
                self.assertEqual(mh.pixel_sha(got), mh.pixel_sha(mh.recolor(im, "red")), "lossless")


MOCHI = os.environ.get("MOCHI_DIR")


@unittest.skipUnless(MOCHI, "MOCHI_DIR not set (the real close-ups are local only)")
class RealCloseUps(unittest.TestCase):
    NAMES = ["grab", "smear", "wave1", "wave2", "wave3", "mouth1", "mouth2", "mouth3", "happy"]

    def test_only_her_hair_changes(self):
        head = mh._poly(mh.SIZE, [mh.HEAD])
        sizes = []
        for n in self.NAMES:
            with Image.open(Path(MOCHI) / f"assets/capri/intro-{n}.webp") as f:
                im = f.convert("RGBA")
            a = np.asarray(im).astype(int)
            out = np.asarray(mh.recolor(im, "red")).astype(int)
            changed = np.abs(out - a).max(axis=2) > 0
            self.assertFalse((changed & ~head).any(), f"{n}: outside the head")
            self.assertFalse(changed[500:].any(), f"{n}: below the jaw (arms, hands, body)")
            self.assertFalse(changed[:, 1345:].any(), f"{n}: the waving hand's side")
            self.assertTrue((out[..., 3] == a[..., 3]).all(), f"{n}: alpha")
            sizes.append(int(mh.hair_mask(im).sum()))
        self.assertLess(max(sizes) - min(sizes), 0.005 * max(sizes), f"the same hair in all nine: {sizes}")
        self.assertGreater(min(sizes), 90_000, sizes)


if __name__ == "__main__":
    unittest.main()
