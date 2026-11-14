#!/usr/bin/env python3
"""Frame pacing and input lag of a synaps-avatar TEST instance, read off the screen.

Run while tools/shoot.py holds a test instance on the stage (socket synaps-avatar.sock, never
the user's own avatar). Strips of the stage are grabbed at ~2 kHz: every change is a visible
update, so update intervals give the presented FPS and pacing, and the first change after a
synthetic input gives the input lag. Pointer actions are XTEST, only on the avatar's own
input shape, only once the user has been idle 4 s; the pointer goes back where it was, and a
phase is marked TAINT if the pointer isn't where we put it (he moved the mouse).

usage: lagmeter.py STAGE_X,STAGE_Y,W,H phase... [--out FILE]
phases: idle:SECS  drag:SECS  steps:N  press:N  fling  watch:LABEL:SECS (no input, e.g. a teleport
started by the caller)"""
import json
import os
import statistics
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shoot  # noqa: E402  (ctl, xroot, on_avatar, user_idle_ms, body_point)

from Xlib import X, display  # noqa: E402
from Xlib.ext import xtest  # noqa: E402

ROWS = 6


class Probe:
    """Grabs ROWS full-width strips of the stage in a loop; records when any changes."""

    def __init__(self, stage):
        self.x, self.y, self.w, self.h = stage
        self.d = display.Display()
        self.r = self.d.screen().root
        self.ys = [self.y + int(self.h * (0.25 + 0.6 * i / (ROWS - 1))) for i in range(ROWS)]
        self.changes, self.moves = [], []
        self.run = False
        self.off = False
        self.polls = 0
        self.bg = 0x34404D

    def grab(self):
        return b"".join(self.r.get_image(self.x, y, self.w, 2, X.ZPixmap, 0xFFFFFFFF).data for y in self.ys)

    def extents(self, data):
        """Leftmost/rightmost non-stage pixel of the first line of each strip: where she is."""
        import array
        out = []
        n = self.w * 4
        for i in range(ROWS):
            row = array.array("I", data[i * 2 * n:i * 2 * n + n])
            idx = [k for k, v in enumerate(row) if (v & 0xFFFFFF) != self.bg]
            out.append((idx[0], idx[-1]) if idx else None)
        return tuple(out)

    def loop(self):
        last = self.grab()
        lastx = self.extents(last)
        while self.run:
            if self.off:
                time.sleep(0.05)
                continue
            cur = self.grab()
            self.polls += 1
            if cur != last:
                t = time.perf_counter()
                self.changes.append(t)
                ex = self.extents(cur)
                if ex != lastx:
                    self.moves.append(t)
                    lastx = ex
                last = cur
            time.sleep(0.0003)

    def start(self):
        self.changes, self.moves, self.polls, self.run = [], [], 0, True
        self.t = threading.Thread(target=self.loop, daemon=True)
        self.t.start()

    def stop(self):
        self.run = False
        self.t.join()
        return self.changes


class Cpu:
    """CPU % of one core for a process over a phase, from /proc."""

    def __init__(self, name):
        p = subprocess.run(["pgrep", "-x", name], capture_output=True, text=True).stdout.split()
        self.pid = p[0] if p else None
        self.gone = False

    def ticks(self):
        if not self.pid:
            return 0
        try:
            f = open(f"/proc/{self.pid}/stat").read().rsplit(")", 1)[1].split()
        except FileNotFoundError:
            self.gone = True
            return 0
        return int(f[11]) + int(f[12])

    def start(self):
        self.a, self.t0 = self.ticks(), time.perf_counter()

    def stop(self):
        return 100.0 * (self.ticks() - self.a) / os.sysconf("SC_CLK_TCK") / max(1e-3, time.perf_counter() - self.t0)


class Gpu:
    """nvidia-smi utilization sampled every 0.5 s in the background."""

    def __init__(self):
        self.samples, self.run = [], False

    def loop(self):
        while self.run:
            try:
                out = subprocess.run(["nvidia-smi", "--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"],
                                     capture_output=True, text=True, timeout=3).stdout.split()
                self.samples.append(int(out[0]))
            except Exception:
                pass
            time.sleep(0.5)

    def start(self):
        self.samples, self.run = [], True
        self.t = threading.Thread(target=self.loop, daemon=True)
        self.t.start()

    def stop(self):
        self.run = False
        self.t.join()
        return round(statistics.mean(self.samples)) if self.samples else None


class Hand:
    """XTEST pointer, watched: if the pointer isn't where we put it, the user moved it."""

    def __init__(self):
        self.d = display.Display()
        self.r = self.d.screen().root
        q = self.r.query_pointer()
        self.home = (q.root_x, q.root_y)
        self.at = self.home
        self.taint = False
        self.down = False

    def check(self):
        q = self.r.query_pointer()
        if abs(q.root_x - self.at[0]) > 1 or abs(q.root_y - self.at[1]) > 1:
            self.taint = True
        return not self.taint

    def move(self, x, y):
        if self.taint:
            return False
        if not self.check():
            return False
        xtest.fake_input(self.d, X.MotionNotify, x=int(x), y=int(y))
        self.d.sync()
        self.at = (int(x), int(y))
        return True

    def press(self):
        xtest.fake_input(self.d, X.ButtonPress, 1)
        self.d.sync()
        self.down = True

    def release(self):
        if self.down:
            xtest.fake_input(self.d, X.ButtonRelease, 1)
            self.d.sync()
            self.down = False

    def restore(self):
        self.release()
        if not self.taint:
            xtest.fake_input(self.d, X.MotionNotify, x=self.home[0], y=self.home[1])
            self.d.sync()


def stats(ts):
    iv = [1000 * (b - a) for a, b in zip(ts, ts[1:])]
    if len(iv) < 3:
        return {"updates": len(ts)}
    iv.sort()
    q = lambda f: round(iv[min(len(iv) - 1, int(f * len(iv)))], 1)  # noqa: E731
    span = ts[-1] - ts[0]
    return {"updates": len(ts), "fps": round((len(ts) - 1) / span, 1) if span > 0 else None, "p50_ms": q(0.5), "p95_ms": q(0.95),
            "max_ms": round(iv[-1], 1)}


def counters():
    """The app's own ticks/renders/window moves (None from builds without them)."""
    try:
        return json.loads(shoot.ctl("state")).get("counters")
    except Exception:
        return None


def grip_point():
    """A point on her torso that takes clicks."""
    a = json.loads(shoot.ctl("state"))["avatar"]
    return shoot.body_point(a["x"] + a["w"] // 2, a["y"] + int(a["h"] * 0.45))


def main():
    args = sys.argv[1:]
    out = None
    if "--out" in args:
        i = args.index("--out")
        out = args[i + 1]
        del args[i:i + 2]
    noprobe = "--noprobe" in args
    if noprobe:
        args.remove("--noprobe")
    stage = [int(v) for v in args.pop(0).split(",")][:4]
    probe, xorg, us, gpu = Probe(stage), Cpu("Xorg"), Cpu("synaps-avatar"), Gpu()
    probe.off = noprobe
    results = []

    def phase(label, body):
        for attempt in range(3):
            r = phase1(label, body)
            if r and not r.get("taint") and not r.get("skipped"):
                break
            time.sleep(5)
        results.append(r)
        print(json.dumps(r), flush=True)
        time.sleep(1.0)

    def phase1(label, body):
        if not shoot.wait_user_idle(4000, 60):
            return {"phase": label, "skipped": "user active"}
        hand = Hand()
        c0 = counters()
        probe.start()
        xorg.start()
        us.start()
        gpu.start()
        extra = {}
        try:
            extra = body(hand) or {}
        finally:
            hand.restore()
        ch = probe.stop()
        c1 = counters()
        r = {"phase": label, **extra, "xorg_cpu": round(xorg.stop(), 1), "avatar_cpu": round(us.stop(), 1), "gpu_util": gpu.stop(),
             "probe_hz": round(probe.polls / max(1e-3, ch[-1] - ch[0]) if len(ch) > 1 else 0), "taint": hand.taint}
        mv = probe.moves
        if "window" in extra:
            t0, t1 = extra.pop("window")
            ch = [t for t in ch if t0 <= t <= t1]
            mv = [t for t in mv if t0 <= t <= t1]
            r.pop("window", None)
            r.pop("probe_hz", None)
        if us.gone:
            r["avatar_gone"] = True
        if c0 and c1:
            r["app"] = {k: c1[k] - c0[k] for k in c0}
        if probe.off:
            r.pop("probe_hz", None)
        else:
            r.update(stats(ch))
            r["moves"] = stats(mv)
        return r

    for ph in args:
        kind, _, rest = ph.partition(":")
        if kind == "idle":
            secs = float(rest or 10)
            phase("idle", lambda h: (time.sleep(secs), {})[1])
        elif kind == "watch":
            label, _, secs = rest.partition(":")
            phase(label, lambda h: (time.sleep(float(secs or 5)), {})[1])
        elif kind == "drag":
            secs = float(rest or 4)

            def drag(h):
                x0, y0 = grip_point()
                if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                    return {"error": "no grip"}
                time.sleep(0.05)
                h.press()
                time.sleep(0.25)
                t0 = time.perf_counter()
                import math
                while time.perf_counter() - t0 < secs and not h.taint:
                    t = time.perf_counter() - t0
                    # Figure-eight, 160 x 90 px at 0.5 Hz, pointer updates at ~500 Hz.
                    h.move(x0 + 160 * math.sin(t * math.pi), y0 - 60 - 90 * math.sin(2 * t * math.pi) / 2)
                    time.sleep(0.002)
                t1 = time.perf_counter()
                h.move(x0, y0 - 60)
                time.sleep(0.4)
                h.release()
                time.sleep(1.5)
                return {"window": (t0 + 0.3, t1)}
            phase(f"drag {secs:g}s", drag)
        elif kind == "steps":
            n = int(rest or 12)

            def steps(h):
                x0, y0 = grip_point()
                if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                    return {"error": "no grip"}
                time.sleep(0.05)
                h.press()
                time.sleep(0.6)
                lat = []
                trace = []
                for i in range(n):
                    if h.taint:
                        break
                    # Settle: no visible change for 200 ms (the chain asleep), up to 3 s.
                    t_settle = time.perf_counter()
                    while time.perf_counter() - t_settle < 3.0:
                        last = probe.changes[-1] if probe.changes else 0
                        if time.perf_counter() - last > 0.2:
                            break
                        time.sleep(0.01)
                    k = len(probe.changes)
                    dx = 24 if i % 2 == 0 else -24
                    t = time.perf_counter()
                    h.move(h.at[0] + dx, h.at[1])
                    trace.append(round(t, 6))
                    while time.perf_counter() - t < 0.3:
                        if len(probe.changes) > k and probe.changes[-1] > t:
                            first = next(c for c in probe.changes[k:] if c > t)
                            lat.append(1000 * (first - t))
                            trace.append(round(first, 6))
                            break
                        time.sleep(0.0005)
                h.release()
                time.sleep(1.2)
                lat.sort()
                return {"step_t": trace, "step_lat_ms": {"n": len(lat), "p50": round(lat[len(lat) // 2], 1) if lat else None,
                                        "p95": round(lat[min(len(lat) - 1, int(0.95 * len(lat)))], 1) if lat else None,
                                        "min": round(lat[0], 1) if lat else None}, "window": (0, 0)}
            phase(f"steps x{n}", steps)
        elif kind == "press":
            n = int(rest or 8)

            def press(h):
                lat = []
                tries = [0]
                for i in range(n):
                    if h.taint:
                        break
                    x0, y0 = grip_point()
                    if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                        continue
                    time.sleep(0.5)
                    k = len(probe.changes)
                    t = time.perf_counter()
                    tries[0] += 1
                    h.press()
                    while time.perf_counter() - t < 0.4:
                        if len(probe.changes) > k and probe.changes[-1] > t:
                            lat.append(1000 * (next(c for c in probe.changes[k:] if c > t) - t))
                            break
                        time.sleep(0.0005)
                    time.sleep(0.3)
                    h.release()
                    time.sleep(1.6)
                lat.sort()
                return {"press_lat_ms": {"n": len(lat), "tries": tries[0], "p50": round(lat[len(lat) // 2], 1) if lat else None,
                                         "p95": round(lat[min(len(lat) - 1, int(0.95 * len(lat)))], 1) if lat else None,
                                         "min": round(lat[0], 1) if lat else None}, "window": (0, 0)}
            phase(f"press x{n}", press)
        elif kind == "ctl":
            # ctl:SECS:LINE  a dev command (e.g. `pos X Y`: a teleport), then watch SECS
            secs, _, line = rest.partition(":")

            def ctlp(h, secs=float(secs), line=line):
                t0 = time.perf_counter()
                shoot.ctl(line)
                time.sleep(secs)
                return {"cmd": line, "window": (t0, time.perf_counter())}
            phase(f"ctl {line}", ctlp)
        elif kind == "dangle":
            def dangle(h):
                x0, y0 = grip_point()
                if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                    return {"error": "no grip"}
                time.sleep(0.05)
                h.press()
                time.sleep(0.3)
                for i in range(30):  # a shake, then held still: she swings
                    h.move(x0 + (40 if (i // 4) % 2 else -40), y0 - 40)
                    time.sleep(0.02)
                h.move(x0, y0 - 40)
                t0 = time.perf_counter()
                time.sleep(2.5)
                t1 = time.perf_counter()
                h.release()
                time.sleep(1.5)
                return {"window": (t0, t1)}
            phase("dangle (swing)", dangle)
        elif kind == "recdrag":
            # recdrag:SECS:PATH  the drag, recorded (x11grab, cursor drawn, lossless, 60 fps)
            secs, _, path = rest.partition(":")

            def recdrag(h, secs=float(secs or 4), path=path):
                import math
                x0, y0 = grip_point()
                if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                    return {"error": "no grip"}
                X0, Y0, W, H = stage
                rec = subprocess.Popen(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "1",
                                        "-framerate", "60", "-video_size", f"{W}x{H}", "-i", f"{os.environ['DISPLAY']}+{X0},{Y0}",
                                        "-t", str(secs + 2.0), "-c:v", "libx264rgb", "-qp", "0", "-preset", "ultrafast", path])
                time.sleep(0.6)
                h.press()
                time.sleep(0.25)
                t0 = time.perf_counter()
                while time.perf_counter() - t0 < secs and not h.taint:
                    t = time.perf_counter() - t0
                    h.move(x0 + 160 * math.sin(t * math.pi), y0 - 60 - 90 * math.sin(2 * t * math.pi) / 2)
                    time.sleep(0.002)
                h.move(x0, y0 - 60)
                time.sleep(0.4)
                h.release()
                rec.wait(timeout=20)
                return {"rec": path, "window": (0, 0)}
            phase("recdrag", recdrag)
        elif kind == "grab":
            # grab:N  press, then 12 px of motion: the first visible change after the motion
            # that starts the drag (5 px threshold)
            n = int(rest or 6)

            def grab(h):
                lat = []
                for i in range(n):
                    if h.taint:
                        break
                    x0, y0 = grip_point()
                    if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                        continue
                    time.sleep(0.3)
                    h.press()
                    time.sleep(0.7)  # past any press reaction
                    k = len(probe.changes)
                    t = time.perf_counter()
                    h.move(x0 + 12, y0)
                    while time.perf_counter() - t < 0.4:
                        if len(probe.changes) > k and probe.changes[-1] > t:
                            lat.append(1000 * (next(c for c in probe.changes[k:] if c > t) - t))
                            break
                        time.sleep(0.0005)
                    time.sleep(0.3)
                    h.move(x0, y0)
                    time.sleep(0.3)
                    h.release()
                    time.sleep(1.8)
                lat.sort()
                return {"grab_lat_ms": {"n": len(lat), "p50": round(lat[len(lat) // 2], 1) if lat else None,
                                        "p95": round(lat[min(len(lat) - 1, int(0.95 * len(lat)))], 1) if lat else None,
                                        "min": round(lat[0], 1) if lat else None}, "window": (0, 0)}
            phase(f"grab x{n}", grab)
        elif kind == "fling":
            def fling(h):
                x0, y0 = grip_point()
                if not shoot.on_avatar(x0, y0) or not h.move(x0, y0):
                    return {"error": "no grip"}
                time.sleep(0.05)
                h.press()
                time.sleep(0.3)
                # A quick throw up and to the left, released in motion.
                for i in range(18):
                    h.move(x0 - 14 * i, y0 - 12 * i)
                    time.sleep(0.006)
                t0 = time.perf_counter()
                h.release()
                time.sleep(2.5)
                return {"window": (t0, time.perf_counter())}
            phase("fling+fall", fling)
    if out:
        json.dump(results, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
