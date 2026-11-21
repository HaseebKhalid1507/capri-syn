#!/usr/bin/env python3
"""Tests for tools/make-pack: the v1.0.4 pin, the pinned pose-sleep, the v1.2.2 files (wave, cels,
no-jacket look), what gets copied, the optional
capri-desktop art (--lachy/--mochi: pins, auto-fetch, anim.json, NOTICE) and the --upscale pins.

python3 tools/test_make_pack.py   (stdlib only; fake checkouts, no CapriCore art needed)"""
import hashlib, importlib.machinery, importlib.util, io, json, os, subprocess, sys, tempfile, types, unittest, zipfile
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


def v122_cache(root):
    return root / "cache/synaps-avatar/capri-desktop" / f"capricore-{mp.V122_COMMIT[:7]}"


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.fake = Fake(self.root)
        self.saved = (mp.pins, mp.SLEEP_SHA, mp.WEIGHTS, mp.v122_pins, mp.fetch_url)
        mp.pins = lambda repo: dict(self.fake.pinned)
        mp.SLEEP_SHA = sha(b"sleep frame")
        mp.WEIGHTS = {}  # no fontTools here
        # v1.2.2's files (the real list, tiny stand-ins), in the download cache; never the network.
        self.v122 = {}
        for rel in self.saved[3](HERE.parent):
            data = b"# Character assets\n\nThe wave was contributed by a member of the Capri community.\n" if rel.endswith(".md") else f"v122 {rel}".encode()
            p = v122_cache(self.root) / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
            self.v122[rel] = sha(data)
        mp.v122_pins = lambda repo: dict(self.v122)
        mp.fetch_url = lambda url: (_ for _ in ()).throw(OSError(f"no network in tests: {url}"))
        self.cache_env = os.environ.get("XDG_CACHE_HOME")
        os.environ["XDG_CACHE_HOME"] = str(self.root / "cache")

    def tearDown(self):
        mp.pins, mp.SLEEP_SHA, mp.WEIGHTS, mp.v122_pins, mp.fetch_url = self.saved
        if self.cache_env is None:
            os.environ.pop("XDG_CACHE_HOME", None)
        else:
            os.environ["XDG_CACHE_HOME"] = self.cache_env
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


class MakePack(Base):
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
        self.assertIn("v1.2.2 (828f272)", mp.NOTICE_LINE)
        self.assertIn("contributed by a member of the Capri community", notice, "v1.2.2's own notice")
        self.assertEqual(json.loads((out / "moods.json").read_text())["icon"], "icon.png")
        self.assertEqual((out / "persona.md").read_bytes(), (HERE.parent / "packs/capri/persona.md").read_bytes())

    def test_v1_2_2_gives_the_wave_the_cels_and_the_no_jacket_look(self):
        out = self.root / "pack"
        rc, msg = self.run_mp(str(self.fake.src), str(out), "--sleep", str(self.fake.sleep))
        self.assertEqual(rc, 0, msg)
        doc = json.loads((out / "anim/anim.json").read_text())
        side = doc["capricore"]
        self.assertEqual(set(doc) - {"schemaVersion", "about"}, {"capricore"}, "no lachy/mochi without their flags")
        for name, st in side["strips"].items():
            rel = side["root"] + st["file"]
            self.assertEqual((out / "anim/capricore" / st["file"]).read_bytes(), f"v122 {rel}".encode(), name)
            self.assertEqual(side["files"][st["file"]]["sha256"], self.v122[rel])
            self.assertEqual(st["outfit"], "no-jacket", name)
        cat = json.loads((out / "outfits/no-jacket/sprites.json").read_text())
        tpl = json.loads((HERE.parent / mp.OUTFIT).read_text())
        self.assertEqual([s["id"] for s in cat["sprites"]], [s["id"] for s in tpl["sprites"]])
        for s, t in zip(cat["sprites"], tpl["sprites"]):
            self.assertEqual(s["sha256"], self.v122[t["from"]], s["id"])
            self.assertEqual((out / "outfits/no-jacket" / s["file"]).read_bytes(), f"v122 {t['from']}".encode())
            self.assertEqual(s.get("patch"), t.get("patch"))
        moods = json.loads((out / "moods.json").read_text())
        self.assertEqual(moods["variants"]["outfits"][1]["sprites"], "outfits/no-jacket/sprites.json")

    def test_v1_2_2_from_a_dir_else_refused_without_the_network(self):
        given = self.root / "v122"
        rel = "assets/default/capri-wave.webp"
        (given / rel).parent.mkdir(parents=True)
        (given / rel).write_bytes(f"v122 {rel}".encode())
        (v122_cache(self.root) / rel).write_bytes(b"truncated")
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "a"), "--sleep", str(self.fake.sleep), "--v122", str(given))
        self.assertEqual(rc, 0, msg)
        rc, msg = self.run_mp(str(self.fake.src), str(self.root / "b"), "--sleep", str(self.fake.sleep))
        self.assertEqual(rc, 1)
        self.assertIn("1 CapriCore v1.2.2 file(s) match tools/capricore-v1.2.2.sha256 nowhere", msg)
        self.assertIn(rel, msg)
        self.assertIn("no network in tests", msg)
        self.assertFalse((self.root / "b/anim").exists(), "nothing written")

    def test_persona_is_the_one_given(self):
        out, persona = self.root / "pack", self.root / "mine.md"
        persona.write_text("You are someone else.\n")
        rc, msg = self.run_mp(str(self.fake.src), str(out), "--sleep", str(self.fake.sleep), "--persona", str(persona))
        self.assertEqual(rc, 0, msg)
        self.assertEqual((out / "persona.md").read_text(), "You are someone else.\n")
        rc, msg = self.run_mp(str(self.fake.src), str(out), "--sleep", str(self.fake.sleep), "--persona", str(self.root / "none.md"))
        self.assertEqual(rc, 1)
        self.assertIn("missing", msg)

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

    def test_the_real_v1_2_2_pins_cover_the_outfit_and_the_strips(self):
        repo = HERE.parent
        pins = self.saved[3](repo)
        self.assertEqual(len(pins), 48)
        self.assertTrue(all(len(h) == 64 for h in pins.values()))
        self.assertEqual(mp.V122_COMMIT[:7], "828f272")
        tpl = json.loads((repo / mp.OUTFIT).read_text())
        geo = json.loads((repo / mp.GEOMETRY).read_text())["capricore"]
        used = {s["from"] for s in tpl["sprites"]} | {geo["root"] + s["file"] for s in geo["strips"].values()} | {"docs/character-assets.md"}
        self.assertEqual(used, set(pins), "every pinned file is used, every used file pinned")
        ids = [s["id"] for s in tpl["sprites"]]
        self.assertEqual(len(ids), len(set(ids)))
        names = [Path(f).name for f in {s["from"] for s in tpl["sprites"]}]
        self.assertEqual(len(names), len(set(names)), "the outfit's files land side by side")
        for skip in ["lava.svg", "installer-sleep", "uninstall-portrait", "07-ready", "01-signal", "capri-headpat.png"]:
            self.assertNotIn(skip, " ".join(pins))


def zip_bytes(members):
    b = io.BytesIO()
    with zipfile.ZipFile(b, "w") as z:
        for name, data in members.items():
            z.writestr(name, data)
    return b.getvalue()


LACHY_LICENSE = ("Required Notice: fake notice one\n"
                 "Required Notice: fake notice two (credit the project)\n\n# PolyForm Noncommercial License 1.0.0\n")
MOCHI_README = ("# Fake\n\n## Credits and rights\n\nThe rights paragraph,\nover two lines.\n\n"
                "**License:** not part of it.\n")
MOCHI_LICENSE = "Required Notice: fake notice three\n\nThis license covers the source code.\n"


class FakeSide:
    """A fake capri-desktop checkout holding a small pinned set."""

    def __init__(self, root, side):
        self.dir = root / side
        self.files = {}
        if side == "lachy":
            self.files = {"LICENSE.md": LACHY_LICENSE.encode(), "assets/sprites/idle.png": b"idle strip",
                          "assets/sprites/idle_flipped.png": b"idle mirror art", "assets/perch.png": b"perch still",
                          "assets/emotes/heart.zip": zip_bytes({"frames.txt": b"emote heart\nfps 30\n", "006.png": b"f6"})}
        else:
            self.files = {"LICENSE": MOCHI_LICENSE.encode(), "README.md": MOCHI_README.encode(),
                          "assets/capri/face-calm.png": b"face", "assets/capri/intro-wave1.webp": b"wave"}
        for rel, b in self.files.items():
            (self.dir / rel).parent.mkdir(parents=True, exist_ok=True)
            (self.dir / rel).write_bytes(b)
        (self.dir / "app").mkdir(exist_ok=True)
        (self.dir / "app/Capri.cs").write_text("code never leaves")
        self.pinned = {rel: sha(b) for rel, b in self.files.items()}


GEOMETRY = {"schemaVersion": 1, "about": "test",
            "lachy": {"commit": "156ed18", "strips": {"idle": {"file": "sprites/idle.png", "frames": 2, "cell": [4, 4]}}},
            "mochi": {"commit": "c011764", "localOnly": True, "images": {}}}


class FakeHair(types.ModuleType):
    """Stands in for tools/mochi_hair.py (no numpy/Pillow): prefixes the bytes with the mode."""

    def __init__(self):
        super().__init__("mochi_hair")
        self.calls = []

    def recolor_file(self, src, dst, mode):
        self.calls.append((Path(src).name, mode))
        Path(dst).write_bytes(f"{mode}:".encode() + Path(src).read_bytes())


class SidesBase(Base):
    def setUp(self):
        super().setUp()
        self.sides = {s: FakeSide(self.root, s) for s in ("lachy", "mochi")}
        self.saved_side = (mp.side_pins, mp.fetch_url, mp.GEOMETRY, mp.mochi_hair)
        self.hair = FakeHair()
        mp.mochi_hair = lambda repo, mode: None if mode == "original" else self.hair
        mp.side_pins = lambda repo, side: dict(self.sides[side].pinned)
        geo = self.root / "desktop-art.json"
        real = json.loads((HERE.parent / "tools/desktop-art.json").read_text())
        geo.write_text(json.dumps({**GEOMETRY, "capricore": real["capricore"]}))
        mp.GEOMETRY = str(geo)

    def tearDown(self):
        mp.side_pins, mp.fetch_url, mp.GEOMETRY, mp.mochi_hair = self.saved_side
        super().tearDown()

    def build(self, *extra, out="pack"):
        return self.run_mp(str(self.fake.src), str(self.root / out), "--sleep", str(self.fake.sleep), *extra)


class DesktopArt(SidesBase):
    def test_lachy_and_mochi_are_copied_with_anim_json_and_notice(self):
        rc, msg = self.build("--lachy", str(self.sides["lachy"].dir), "--mochi", str(self.sides["mochi"].dir))
        self.assertEqual(rc, 0, msg)
        a = self.root / "pack/anim"
        self.assertEqual((a / "lachy/sprites/idle.png").read_bytes(), b"idle strip")
        self.assertEqual((a / "lachy/perch.png").read_bytes(), b"perch still")
        self.assertEqual(sorted(p.name for p in (a / "lachy/emotes/heart").iterdir()), ["006.png", "frames.txt"])
        self.assertFalse((a / "lachy/emotes/heart.zip").exists(), "emotes are unzipped")
        self.assertTrue((a / "lachy/LICENSE.md").is_file() and (a / "mochi/LICENSE").is_file())
        self.assertEqual((a / "mochi/intro-wave1.webp").read_bytes(), b"red:wave", "her hair is red by default")
        self.assertEqual(self.hair.calls, [("intro-wave1.webp", "red")], "only the intro close-ups")
        self.assertEqual((a / "mochi/face-calm.png").read_bytes(), b"face")
        self.assertFalse(list(self.root.joinpath("pack").rglob("*.cs")), "no code is copied")
        doc = json.loads((a / "anim.json").read_text())
        self.assertEqual(doc["lachy"]["strips"]["idle"]["frames"], 2)
        self.assertEqual(doc["lachy"]["files"]["sprites/idle.png"]["sha256"], sha(b"idle strip"))
        self.assertEqual(doc["lachy"]["files"]["emotes/heart.zip"]["unzippedTo"], "emotes/heart")
        self.assertEqual(doc["mochi"]["files"]["face-calm.png"], {"sha256": sha(b"face"), "kind": "face"})
        self.assertEqual(doc["mochi"]["files"]["intro-wave1.webp"],
                         {"sha256": sha(b"red:wave"), "kind": "intro", "hair": "red", "sourceSha256": sha(b"wave")},
                         "the recoloured frame is pinned by its own bytes; the source sha stays alongside")
        self.assertFalse((a / "mochi/README.md").exists(), "the README is read for the notice, not copied")
        n = (self.root / "pack/NOTICE.md").read_text()
        self.assertTrue(n.startswith("# Character notice"), "CapriCore's notice stays first")
        for line in LACHY_LICENSE.splitlines()[:2] + MOCHI_LICENSE.splitlines()[:1]:
            self.assertIn(line, n, "Required Notice lines verbatim")
        self.assertIn(mp.POLYFORM_NC, n)
        self.assertIn("Click effect: mixmix312352", n)
        self.assertIn("Qewiw (mix_qewiw1412_86144)", n)
        self.assertIn("> The rights paragraph, over two lines.\n", n, "its rights statement, read from its README")
        self.assertNotIn("not part of it", n)
        self.assertIn("LOCAL PERSONAL USE ONLY", n)

    def test_mochi_hair_flat_and_original(self):
        d = str(self.sides["mochi"].dir)
        rc, msg = self.build("--mochi", d, "--mochi-hair", "flat")
        self.assertEqual(rc, 0, msg)
        doc = json.loads((self.root / "pack/anim/anim.json").read_text())
        self.assertEqual(doc["mochi"]["files"]["intro-wave1.webp"]["sha256"], sha(b"flat:wave"))
        self.assertIn("intro hair: flat", msg)
        rc, msg = self.build("--mochi", d, "--mochi-hair", "original", out="p2")
        self.assertEqual(rc, 0, msg)
        self.assertEqual((self.root / "p2/anim/mochi/intro-wave1.webp").read_bytes(), b"wave", "untouched")
        doc = json.loads((self.root / "p2/anim/anim.json").read_text())
        self.assertEqual(doc["mochi"]["files"]["intro-wave1.webp"], {"sha256": sha(b"wave"), "kind": "intro"})
        self.assertEqual(self.hair.calls, [("intro-wave1.webp", "flat")])
        rc, msg = self.build("--mochi", d, "--mochi-hair", "blue", out="p3")
        self.assertNotEqual(rc, 0)
        self.assertIn("red, flat or original", msg)
        self.assertFalse((self.root / "p3").exists())

    def test_lachy_only_never_loads_the_recolor(self):
        mp.mochi_hair = lambda repo, mode: self.fail("no mochi, no recolor")
        rc, msg = self.build("--lachy", str(self.sides["lachy"].dir))
        self.assertEqual(rc, 0, msg)

    def test_without_the_flags_the_pack_is_as_before(self):
        rc, msg = self.build()
        self.assertEqual(rc, 0, msg)
        self.assertEqual(sorted(p.name for p in (self.root / "pack/anim").iterdir()), ["anim.json", "capricore"], "v1.2.2's only")
        self.assertNotIn("capri-desktop", (self.root / "pack/NOTICE.md").read_text())

    def test_lachy_only(self):
        rc, msg = self.build("--lachy", str(self.sides["lachy"].dir))
        self.assertEqual(rc, 0, msg)
        doc = json.loads((self.root / "pack/anim/anim.json").read_text())
        self.assertIn("lachy", doc)
        self.assertNotIn("mochi", doc)
        self.assertFalse((self.root / "pack/anim/mochi").exists())
        self.assertNotIn("mochiiwuff", (self.root / "pack/NOTICE.md").read_text())

    def test_refuses_a_changed_or_missing_side_file_and_writes_nothing(self):
        d = self.sides["lachy"].dir
        (d / "assets/sprites/idle.png").write_bytes(b"redrawn")
        rc, msg = self.build("--lachy", str(d))
        self.assertNotEqual(rc, 0)
        self.assertIn("changed assets/sprites/idle.png", msg)
        (d / "assets/sprites/idle.png").unlink()
        rc, msg = self.build("--lachy", str(d), out="p2")
        self.assertIn("missing assets/sprites/idle.png", msg)
        self.assertFalse((self.root / "pack").exists() or (self.root / "p2").exists(), "nothing written")

    def test_refuses_another_commit(self):
        d = self.sides["mochi"].dir
        g = ["git", "-C", str(d)]
        subprocess.run(g + ["init", "-q"], check=True)
        subprocess.run(g + ["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"], check=True)
        rc, msg = self.build("--mochi", str(d))
        self.assertNotEqual(rc, 0)
        self.assertIn("not mochiiwuff/capri-desktop c011764", msg)

    def test_refuses_a_zip_with_paths(self):
        d = self.sides["lachy"].dir
        bad = zip_bytes({"../escape.png": b"x"})
        (d / "assets/emotes/heart.zip").write_bytes(bad)
        self.sides["lachy"].pinned["assets/emotes/heart.zip"] = sha(bad)
        rc, msg = self.build("--lachy", str(d))
        self.assertNotEqual(rc, 0)
        self.assertIn("unexpected member", msg)
        self.assertFalse((self.root / "escape.png").exists() or (self.root / "pack/anim/lachy/escape.png").exists())

    def test_auto_downloads_the_pinned_files_once(self):
        files = self.sides["mochi"].files
        got = []

        def fetch(url):
            got.append(url)
            rel = url.split("/c011764ca1fdbd191925321e3aaa8af5b84babe8/", 1)[1]
            return files[rel]
        mp.fetch_url = fetch
        rc, msg = self.build("--mochi", "auto")
        self.assertEqual(rc, 0, msg)
        self.assertEqual(len(got), 4)
        self.assertTrue(all(u.startswith("https://raw.githubusercontent.com/mochiiwuff/capri-desktop/c011764ca1fdbd191925321e3aaa8af5b84babe8/") for u in got))
        rc, msg = self.build("--mochi", "auto", out="p2")
        self.assertEqual(rc, 0, msg)
        self.assertEqual(len(got), 4, "cached: no second download")

    def test_auto_refuses_a_download_that_doesnt_match(self):
        mp.fetch_url = lambda url: b"something else"
        rc, msg = self.build("--mochi", "auto")
        self.assertNotEqual(rc, 0)
        self.assertIn("does not match tools/mochi-desktop-c011764.sha256", msg)


class FakeUpscaler(types.ModuleType):
    """Stands in for tools/upscale_art.py (no torch): x2 nearest of a 1x1 strip."""
    FACTOR, MODEL_SHA = 2, "0" * 64

    def __init__(self):
        super().__init__("upscale_art")
        self.calls = 0

    def load_net(self, cache):
        return None

    def upscale_strip(self, net, path, cell_w, frames):
        self.calls += 1
        return path.read_bytes() * 2

    def pixel_sha(self, im):
        return sha(im)


class Upscale(SidesBase):
    def setUp(self):
        super().setUp()
        self.fakeup = FakeUpscaler()
        self.saved_mod = sys.modules.get("upscale_art")
        sys.modules["upscale_art"] = self.fakeup
        self.saved_pins = mp.UPSCALE_PINS
        mp.UPSCALE_PINS = str(self.root / "x2.sha256")

    def tearDown(self):
        mp.UPSCALE_PINS = self.saved_pins
        if self.saved_mod is None:
            sys.modules.pop("upscale_art", None)
        else:
            sys.modules["upscale_art"] = self.saved_mod
        super().tearDown()

    def run_up(self, record):
        out = self.root / "pack"
        out.joinpath("anim/lachy/sprites").mkdir(parents=True, exist_ok=True)
        out.joinpath("anim/lachy/sprites/idle.png").write_bytes(b"ab")
        out.joinpath("anim/anim.json").write_text(json.dumps(GEOMETRY))

        class Img(bytes):
            def save(s, p):
                Path(p).write_bytes(s)
        orig = self.fakeup.upscale_strip
        self.fakeup.upscale_strip = lambda net, path, cw, n: Img(orig(net, path, cw, n))
        try:
            return mp.upscale(out, HERE.parent, record, self.root / "models")
        except SystemExit as e:
            return str(e.code)

    def test_upscale_needs_lachy_and_pins(self):
        rc, msg = self.build("--upscale")
        self.assertIn("--upscale needs --lachy", msg)
        rc, msg = self.build("--lachy", str(self.sides["lachy"].dir), "--upscale")
        self.assertNotEqual(rc, 0)
        self.assertIn("no ", msg)
        self.assertIn("--record-upscale", msg)
        self.assertFalse((self.root / "pack").exists(), "nothing written")

    def test_record_then_verify(self):
        self.assertEqual(self.run_up(record=True), 1)
        pins = Path(mp.UPSCALE_PINS).read_text()
        self.assertIn(f"{sha(b'abab')}  x2/sprites/idle.png", pins)
        self.assertEqual((self.root / "pack/anim/lachy/x2/sprites/idle.png").read_bytes(), b"abab")
        self.assertEqual(self.run_up(record=False), 1)
        Path(mp.UPSCALE_PINS).write_text(f"{'1' * 64}  x2/sprites/idle.png\n")
        msg = self.run_up(record=False)
        self.assertIn("differ from", msg)
        self.assertIn("x2/sprites/idle.png", msg)


class RealPins(unittest.TestCase):
    def test_the_desktop_pin_lists_and_geometry_agree(self):
        repo = HERE.parent
        geo = json.loads((repo / "tools/desktop-art.json").read_text())
        lachy, mochi = mp.side_pins(repo, "lachy"), mp.side_pins(repo, "mochi")
        self.assertEqual(len(lachy), 59)
        self.assertEqual(len(mochi), 26)
        self.assertTrue(all(len(h) == 64 for h in {**lachy, **mochi}.values()))
        self.assertIn("LICENSE.md", lachy)
        self.assertIn("LICENSE", mochi)
        self.assertIn("README.md", mochi)
        for rel in lachy:
            self.assertTrue(rel == "LICENSE.md" or rel.startswith("assets/"), rel)
            self.assertFalse(rel.endswith((".cs", ".wav", ".ttf", ".mp3", ".js")), rel)
        strips = geo["lachy"]["strips"]
        pinned_sprites = {r[len("assets/"):] for r in lachy if r.startswith("assets/sprites/")}
        self.assertEqual({s["file"] for s in strips.values()} | {"sprites/face.png"}, pinned_sprites,
                         "every pinned strip has our geometry, and nothing unpinned is described")
        for name, s in strips.items():
            if "mirrorArt" in s:
                self.assertEqual(strips[s["mirrorArt"]]["mirrorArtFor"], name)
                self.assertEqual(strips[s["mirrorArt"]]["cell"], s["cell"])
            if "anchor" in s and s["family"] != "fx":
                a, (w, h) = s["anchor"], s["cell"]
                self.assertTrue(0 <= a["x"] < w and 0 <= a["y"] < h, (name, a))
            elif "anchor" in s:
                # An effect (the form bloom) is placed by where her feet will be: below its cell.
                self.assertEqual(s["anchor"]["name"], "feet", name)
        for rel in mochi:
            self.assertTrue(rel in ("LICENSE", "README.md") or rel.startswith("assets/capri/"), rel)
            if rel.startswith("assets/"):
                self.assertNotEqual(mp.mochi_kind(rel.rsplit("/", 1)[1]), "other", rel)
        self.assertTrue(geo["mochi"]["localOnly"])
        self.assertNotIn("images", geo["mochi"], "no listing of that repo's files beyond the pin file")


if __name__ == "__main__":
    unittest.main()
