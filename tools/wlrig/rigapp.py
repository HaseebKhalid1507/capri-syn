"""rigapp.py: one test instance of her in the rig, for the rig's scripts (p1, atomic, fx, withapp).

  with RigApp(BIN, PACK, log, env={...}) as r:   # the backdrop layer, her process, brain off once ready
      r.world, r.app, shoot.ctl(...)
Inside the sandbox only."""
import os, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import shoot  # noqa: E402
from wlworld import World  # noqa: E402

BG = 0x203040  # the backdrop, also the X11 comparison's root colour
GROUND = (1200, 1590)


def drawn(log):
    """Her box on screen over time from an SA_ATOMIC_LOG=1 log: [(t, (x, y, w, h))], sorted. Each painted frame's
    body box, and each move not painted with the frame after it (the last frame's box, moved)."""
    paints, moves = [], []
    for l in open(log):
        p = l.split()
        if l.startswith("atomic paint"):
            ox, oy = map(int, p[3].split(","))
            bx, by, bw, bh = map(int, p[4].split(","))
            paints.append((float(p[2]), (ox + bx, oy + by, bw, bh), (ox, oy)))
        elif l.startswith("atomic move"):
            moves.append((float(p[2]), tuple(map(int, p[3].split(",")))))
    states = [(t, b) for t, b, _ in paints]
    for t, o in moves:
        nxt = next((q for q in paints if q[0] >= t), None)
        if nxt and nxt[2] == o and nxt[0] - t < 0.005:
            continue  # painted with its frame
        prev = next((q for q in reversed(paints) if q[0] <= t), None)
        if prev:
            b, po = prev[1], prev[2]
            states.append((t, (b[0] - po[0] + o[0], b[1] - po[1] + o[1], b[2], b[3])))
    return sorted(states), len(paints), len(moves)


def around(states, t, lag=0.1):
    """The boxes she may show in a frame presented at t: what she put up in the `lag` s before it, and the last
    one before that."""
    return [b for ts, b in states if ts <= t - lag][-1:] + [b for ts, b in states if t - lag < ts <= t]


class RigApp:
    def __init__(self, bin_, pack, log, env=None, brain_off=True):
        assert os.path.exists(os.path.join(os.environ["XDG_RUNTIME_DIR"], "rig.ready")), "inside the rig only"
        self.bin, self.pack, self.log, self.env, self.brain_off = bin_, pack, log, env or {}, brain_off

    def __enter__(self):
        self.world = World()
        self.world.layer("under", 0, 0, 2560, 1600, "bottom", 0, BG)
        d = os.path.join(os.environ["XDG_STATE_HOME"], "synaps-avatar")
        os.makedirs(d, exist_ok=True)
        open(os.path.join(d, "state.json"), "w").write('{"ground":[%d,%d]}' % GROUND)
        self.t0 = time.time()
        self.app = subprocess.Popen([self.bin, "--pack", self.pack, "--no-session"], stdout=open(self.log, "w"), stderr=subprocess.STDOUT,
                                    env=dict(os.environ, **self.env))
        for _ in range(150):
            if os.path.exists(shoot.SOCK) and "all frames ready" in open(self.log).read():
                break
            time.sleep(0.1)
        time.sleep(1)
        if self.brain_off:
            shoot.ctl("brain off")
        return self

    def first_frame(self):
        return next((l.strip() for l in open(self.log) if l.startswith("first-frame")), None)

    def __exit__(self, *exc):
        try:
            shoot.ctl("quit")
        except Exception:
            pass
        try:
            self.app.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.app.kill()
        self.world.destroy("under")
