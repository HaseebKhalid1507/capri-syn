#!/usr/bin/env python3
"""Tests for tools/make-pack: the v1.0.4 pin, the pinned pose-sleep, what gets copied.

python3 tools/test_make_pack.py   (stdlib only; fake checkouts, no CapriCore art needed)"""
import hashlib, importlib.machinery, importlib.util, io, json, subprocess, sys, tempfile, unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
loader = importlib.machinery.SourceFileLoader("make_pack", str(HERE / "make-pack"))
spec = importlib.util.spec_from_loader("make_pack", loader)
mp = importlib.util.module_from_spec(spec)
loader.exec_module(mp)


def sha(b):
    return hashlib.sha256(b).hexdigest()


class Fake:
    """A fake CapriCore checkout whose files are the pinned set (tiny stand-ins)."""

    def __init__(self, root):
        self.src = root / "cc"
        self.pinned = {}
        for rel in mp.pins(HERE.parent):
            p = self.src / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            data = b'{"character": "Capri"}' if rel.endswith("runtime-sprite-set.json") else f"fake {rel}".encode()
            p.write_bytes(data)
            self.pinned[rel] = sha(data)
        (self.src / "docs/character-assets.md").write_text("# Character notice\n")
        self.pinned["docs/character-assets.md"] = sha(b"# Character notice\n")
        # Unpinned files never leave the checkout.
        (self.src / "assets/capri/intro-v1/PNG/01-signal.png").write_bytes(b"bolt drawing")
        (self.src / "assets/default/screenshot-capture.mp3").write_bytes(b"blip")
        self.sleep = root / "sleep.png"
        self.sleep.write_bytes(b"sleep frame")


class MakePack(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.fake = Fake(self.root)
        self.saved = (mp.pins, mp.SLEEP_SHA, mp.WEIGHTS)
        mp.pins = lambda repo: dict(self.fake.pinned)
        mp.SLEEP_SHA = sha(b"sleep frame")
        mp.WEIGHTS = {}  # no fontTools here

    def tearDown(self):
        mp.pins, mp.SLEEP_SHA, mp.WEIGHTS = self.saved
        self.tmp.cleanup()

    def run_mp(self, *args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            try:
                rc = mp.main(list(args))
            except SystemExit as e:
                # sys.exit("message"): the message is the code (printed to stderr, exit 1).
                return (1 if isinstance(e.code, str) else e.code), f"{e.code}" + err.getvalue()
        return rc, out.getvalue()

    def test_builds_the_pack_from_pinned_files_only(self):
        out = self.root / "pack"
        rc, msg = self.run_mp(str(self.fake.src), str(out), "--sleep", str(self.fake.sleep))
        self.assertEqual(rc, 0, msg)
        self.assertEqual((out / "poses/capri-sleep.png").read_bytes(), b"sleep frame")
        self.assertEqual(sorted(p.name for p in (out / "intro").iterdir()),
                         ["02-gaze.png", "03-arrival.png", "04-landing.png", "05-greeting.png", "06-wink.png", "runtime-intro.json"])
        self.assertTrue((out / "icon.png").is_file())
        self.assertFalse(list(out.rglob("*.mp3")), "the blip stays out")
        self.assertFalse((out / "intro/01-signal.png").exists(), "her bolt replaces 01")
        notice = (out / "NOTICE.md").read_text()
        self.assertTrue(notice.startswith("# Character notice"))
        self.assertEqual(notice.strip().splitlines()[-1], mp.NOTICE_LINE)
        self.assertIn("v1.0.4 (bc93892)", mp.NOTICE_LINE)
        self.assertIn("v1.1.2 (bb0c2d7)", mp.NOTICE_LINE)
        self.assertEqual(json.loads((out / "moods.json").read_text())["icon"], "icon.png")

    def test_refuses_a_changed_or_missing_file(self):
        p = self.fake.src / "assets/capri/sprites/v1/capri-neutral.png"
        p.write_bytes(b"a 1.1 redraw")
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "p1"), "--sleep", str(self.fake.sleep))
        self.assertNotEqual(rc, 0)
        self.assertIn("changed assets/capri/sprites/v1/capri-neutral.png", msg)
        p.unlink()
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "p2"), "--sleep", str(self.fake.sleep))
        self.assertIn("missing assets/capri/sprites/v1/capri-neutral.png", msg)
        self.assertFalse((self.root / "p1").exists() or (self.root / "p2").exists(), "nothing written")

    def test_refuses_another_git_checkout(self):
        g = ["git", "-C", str(self.fake.src)]
        subprocess.run(g + ["init", "-q"], check=True)
        subprocess.run(g + ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"], check=True)
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "p"), "--sleep", str(self.fake.sleep))
        self.assertNotEqual(rc, 0)
        self.assertIn("not CapriCore v1.0.4", msg)
        self.assertIn("checkout v1.0.4", msg)

    def test_refuses_a_sleep_frame_that_isnt_v1_1_2s(self):
        bad = self.root / "bad.png"
        bad.write_bytes(b"something else")
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "p"), "--sleep", str(bad))
        self.assertNotEqual(rc, 0)
        self.assertIn("is not v1.1.2's capri-sleep.png", msg)

    def test_the_real_pin_list(self):
        pins = self.saved[0](HERE.parent)
        self.assertEqual(len(pins), 94)
        self.assertTrue(all(len(h) == 64 for h in pins.values()))
        names = " ".join(pins)
        for want in ["intro-v1/PNG/02-gaze.png", "intro-v1/PNG/06-wink.png", "default/icon-source.png", "poses-v2/PNG/capri-crouch.png"]:
            self.assertIn(want, names)
        for skip in ["01-signal", "07-ready", "screenshot-capture", "avatar.png", "brand/", "app.ico", "capri-sleep"]:
            self.assertNotIn(skip, names)
        self.assertEqual(mp.PIN_COMMIT[:7], "bc93892")


if __name__ == "__main__":
    unittest.main()
