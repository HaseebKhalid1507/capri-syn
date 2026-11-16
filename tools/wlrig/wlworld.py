#!/usr/bin/env python3
"""wlworld.py: the Wayland rig's world, on the wire protocol directly (no pywayland on bella).

  wlworld.py serve            line commands on stdin, one reply line each (the contract's Hyprland driver)
  wlworld.py poke move X Y | click X Y [BTN] | press [BTN] | release [BTN] | drag X0 Y0 X1 Y1 [STEPS] [HZ]
                              the virtual pointer (zwlr_virtual_pointer_v1), root (global layout) px
  wlworld.py record X Y W H FPS SECS OUT.mp4   a lossless clip of a region (zwlr_screencopy_manager_v1)

serve commands (names are app ids / layer namespaces; rects are root px):
  toplevel NAME W H [RGB]     an xdg_toplevel (placement: the caller's window rules / dispatches)
  layer NAME X Y W H [top|overlay|bottom] [EXCL] [RGB]   a layer surface (EXCL > 0: a panel along the top)
  move NAME X Y W H           re-place a layer surface
  destroy NAME
  under                       a full-layout Bottom layer whose pointer focus `probe` reads
  probe X Y                   the pointer to (X, Y): "world NAME" if one of ours has it, else "other"
  warp X Y | click X Y | press | release
  hypr ARGS...                hyprctl ARGS (its stdout, one line)
Runs inside the rig's sandbox (run.sh). Never ydotool: everything goes through the rig compositor.
"""
import json, mmap, os, select, socket, struct, subprocess, sys, time

BTN_LEFT = 0x110


def hyprctl(*args):
    return subprocess.run(["hyprctl", *args], capture_output=True, text=True, timeout=5).stdout


def layout_box():
    ms = [m for m in json.loads(hyprctl("-j", "monitors")) if not m.get("disabled")]
    x0 = min(m["x"] for m in ms)
    y0 = min(m["y"] for m in ms)
    x1 = max(m["x"] + int(m["width"] / m["scale"]) for m in ms)
    y1 = max(m["y"] + int(m["height"] / m["scale"]) for m in ms)
    return x0, y0, x1 - x0, y1 - y0


class Wl:
    def __init__(self):
        path = os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ["WAYLAND_DISPLAY"])
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.s.connect(path)
        self.next = 2
        self.ifaces = {1: "wl_display"}
        self.handlers = {}
        self.globals = {}
        self.buf = b""
        self.registry = self.new("wl_registry")
        self.send(1, 1, "n", self.registry)
        self.roundtrip()

    def new(self, iface):
        i = self.next
        self.next += 1
        self.ifaces[i] = iface
        return i

    def send(self, obj, op, sig, *args, fds=()):
        b = b""
        for t, a in zip(sig, args):
            if t in "un":
                b += struct.pack("<I", a)
            elif t == "i":
                b += struct.pack("<i", a)
            elif t == "f":
                b += struct.pack("<i", int(round(a * 256)))
            elif t == "s":
                e = a.encode() + b"\0"
                b += struct.pack("<I", len(e)) + e + b"\0" * ((4 - len(e) % 4) % 4)
        msg = struct.pack("<II", obj, ((8 + len(b)) << 16) | op) + b
        if os.environ.get("WLWORLD_DEBUG"):
            sys.stderr.write(f"-> {self.ifaces.get(obj)}#{obj}.{op} {args} fds={list(fds)}\n")
        anc = [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack(f"{len(fds)}i", *fds))] if fds else []
        self.s.sendmsg([msg], anc)

    def bind(self, iface, maxver):
        name, ver = next((n, v) for n, (i, v) in self.globals.items() if i == iface)
        oid = self.new(iface)
        e = iface.encode() + b"\0"
        v = min(ver, maxver)
        b = struct.pack("<I", name) + struct.pack("<I", len(e)) + e + b"\0" * ((4 - len(e) % 4) % 4) + struct.pack("<II", v, oid)
        self.s.sendmsg([struct.pack("<II", self.registry, ((8 + len(b)) << 16) | 0) + b])
        return oid

    def dispatch(self, timeout=0.0):
        r, _, _ = select.select([self.s], [], [], timeout)
        if r:
            d = self.s.recv(65536)
            if not d:
                raise SystemExit("wlworld: compositor gone")
            self.buf += d
        while len(self.buf) >= 8:
            obj, so = struct.unpack_from("<II", self.buf)
            size, op = so >> 16, so & 0xFFFF
            if len(self.buf) < size:
                break
            body, self.buf = self.buf[8:size], self.buf[size:]
            self.event(obj, op, body)
        return bool(r)

    def event(self, obj, op, b):
        iface = self.ifaces.get(obj)
        if iface == "wl_display" and op == 0:
            oid, code = struct.unpack_from("<II", b)
            n = struct.unpack_from("<I", b, 8)[0]
            raise SystemExit(f"wlworld: protocol error on {self.ifaces.get(oid)}#{oid} code {code}: {b[12:12 + n - 1].decode()}")
        if iface == "wl_registry" and op == 0:
            name = struct.unpack_from("<I", b)[0]
            n = struct.unpack_from("<I", b, 4)[0]
            i = b[8:8 + n - 1].decode()
            self.globals[name] = (i, struct.unpack_from("<I", b, 8 + ((n + 3) & ~3))[0])
        elif iface == "xdg_wm_base" and op == 0:
            self.send(obj, 3, "u", struct.unpack_from("<I", b)[0])
        h = self.handlers.get(obj)
        if h:
            h(op, b)

    def roundtrip(self):
        cb = self.new("wl_callback")
        done = []
        self.handlers[cb] = lambda op, b: done.append(1)
        self.send(1, 0, "n", cb)
        end = time.time() + 5
        while not done and time.time() < end:
            self.dispatch(0.05)
        self.handlers.pop(cb, None)


class World:
    def __init__(self):
        self.w = Wl()
        g = {i for i, _ in self.w.globals.values()}
        need = {"wl_compositor", "wl_shm", "wl_seat", "xdg_wm_base", "zwlr_layer_shell_v1", "zwlr_virtual_pointer_manager_v1"}
        missing = need - g
        if missing:
            raise SystemExit(f"wlworld: compositor lacks {missing}")
        self.comp = self.w.bind("wl_compositor", 4)
        self.shm = self.w.bind("wl_shm", 1)
        self.seat = self.w.bind("wl_seat", 5)
        self.xdg = self.w.bind("xdg_wm_base", 2)
        self.ls = self.w.bind("zwlr_layer_shell_v1", 4)
        vpm = self.w.bind("zwlr_virtual_pointer_manager_v1", 1)
        self.vp = self.w.new("zwlr_virtual_pointer_v1")
        self.w.send(vpm, 0, "un", self.seat, self.vp)
        self.ptr = self.w.new("wl_pointer")
        self.w.send(self.seat, 0, "n", self.ptr)
        self.focus = None  # surface id with our pointer focus
        self.w.handlers[self.ptr] = self.on_pointer
        self.kbd = self.w.new("wl_keyboard")
        self.w.send(self.seat, 1, "n", self.kbd)
        self.kfocus = None  # surface id with our keyboard focus
        self.w.handlers[self.kbd] = self.on_keyboard
        self.surfaces = {}  # name -> dict(surface, role, ...)
        self.btn_t0 = time.time()
        self.w.roundtrip()

    def on_pointer(self, op, b):
        if op == 0:
            self.focus = struct.unpack_from("<I", b, 4)[0]
        elif op == 1:
            self.focus = None

    def on_keyboard(self, op, b):
        if op == 1:
            self.kfocus = struct.unpack_from("<I", b, 4)[0]
        elif op == 2:
            self.kfocus = None

    def keyboard_focus(self):
        """The name of ours with the keyboard, else None."""
        self.w.roundtrip()
        return next((n for n, s in self.surfaces.items() if s["surface"] == self.kfocus), None)

    def set_ki(self, name, ki):
        """A layer's keyboard interactivity at run time (0 none, 1 exclusive, 2 on demand), committed."""
        s = self.surfaces[name]
        self.w.send(s["layer"], 4, "u", ki)
        self.w.send(s["surface"], 6, "")
        self.w.roundtrip()

    def ms(self):
        return int((time.time() - self.btn_t0) * 1000) & 0xFFFFFFFF

    def buffer(self, w, h, rgb):
        stride = w * 4
        size = stride * h
        fd = os.memfd_create("wlworld")
        os.ftruncate(fd, size)
        m = mmap.mmap(fd, size)
        m.write(struct.pack("<I", 0xFF000000 | rgb) * (w * h))
        m.close()
        pool = self.w.new("wl_shm_pool")
        # create_pool(new_id, fd, size): the fd travels out of band, so the signature has no slot for it
        self.w.send(self.shm, 0, "ni", pool, size, fds=[fd])
        os.close(fd)
        buf = self.w.new("wl_buffer")
        self.w.send(pool, 0, "niiiiu", buf, 0, w, h, stride, 1)  # XRGB8888
        self.w.send(pool, 1, "")

        def released(op, b, buf=buf):
            if op == 0 and self.w.handlers.pop(buf, None):
                self.w.send(buf, 0, "")

        self.w.handlers[buf] = released
        return buf

    def paint(self, s, w, h, rgb):
        sid = s["surface"]
        self.w.send(sid, 1, "uii", self.buffer(w, h, rgb), 0, 0)
        self.w.send(sid, 2, "iiii", 0, 0, w, h)
        self.w.send(sid, 6, "")

    def toplevel(self, name, w, h, rgb=0x808080):
        sid = self.w.new("wl_surface")
        self.w.send(self.comp, 0, "n", sid)
        xs = self.w.new("xdg_surface")
        self.w.send(self.xdg, 2, "nu", xs, sid)
        tl = self.w.new("xdg_toplevel")
        self.w.send(xs, 1, "n", tl)
        self.w.send(tl, 2, "s", name)
        self.w.send(tl, 3, "s", name)
        s = {"surface": sid, "xdg": xs, "toplevel": tl, "size": (w, h), "rgb": rgb, "kind": "toplevel"}
        self.surfaces[name] = s

        def on_tl(op, b):
            if op == 0:
                cw, ch = struct.unpack_from("<ii", b)
                if cw > 0 and ch > 0:
                    s["size"] = (cw, ch)

        def on_xs(op, b):
            if op == 0:
                self.w.send(xs, 4, "u", struct.unpack_from("<I", b)[0])
                self.paint(s, *s["size"], s["rgb"])

        self.w.handlers[tl] = on_tl
        self.w.handlers[xs] = on_xs
        self.w.send(sid, 6, "")
        self.w.roundtrip()
        self.w.roundtrip()

    def layer(self, name, x, y, w, h, layer="top", excl=0, rgb=0x606060):
        sid = self.w.new("wl_surface")
        self.w.send(self.comp, 0, "n", sid)
        lsurf = self.w.new("zwlr_layer_surface_v1")
        self.w.send(self.ls, 0, "nuuus", lsurf, sid, 0, {"background": 0, "bottom": 1, "top": 2, "overlay": 3}[layer], name)
        s = {"surface": sid, "layer": lsurf, "size": (w, h), "rgb": rgb, "kind": "layer"}
        self.surfaces[name] = s
        if excl > 0:  # a panel: the top edge, full width, reserving its height
            self.w.send(lsurf, 1, "u", 1 | 4 | 8)
            self.w.send(lsurf, 0, "uu", 0, h)
            self.w.send(lsurf, 2, "i", excl)
        else:
            self.w.send(lsurf, 1, "u", 1 | 4)  # top | left
            self.w.send(lsurf, 0, "uu", w, h)
            self.w.send(lsurf, 2, "i", -1)
            self.w.send(lsurf, 3, "iiii", y, 0, 0, x)

        def on_ls(op, b):
            if op == 0:
                serial, cw, ch = struct.unpack_from("<III", b)
                if cw > 0 and ch > 0:
                    s["size"] = (cw, ch)
                self.w.send(lsurf, 6, "u", serial)
                self.paint(s, *s["size"], s["rgb"])

        self.w.handlers[lsurf] = on_ls
        self.w.send(sid, 6, "")
        self.w.roundtrip()
        self.w.roundtrip()

    def move(self, name, x, y, w, h):
        s = self.surfaces[name]
        self.w.send(s["layer"], 3, "iiii", y, 0, 0, x)
        if (w, h) != s["size"]:
            self.w.send(s["layer"], 0, "uu", w, h)
            s["size"] = (w, h)
            self.paint(s, w, h, s["rgb"])
        else:
            self.w.send(s["surface"], 6, "")
        self.w.roundtrip()

    def destroy(self, name):
        s = self.surfaces.pop(name)
        for k in ("toplevel", "xdg", "layer"):
            self.w.handlers.pop(s.get(k), None)
        if s["kind"] == "toplevel":
            self.w.send(s["toplevel"], 0, "")
            self.w.send(s["xdg"], 0, "")
        else:
            self.w.send(s["layer"], 7, "")
        self.w.send(s["surface"], 0, "")
        self.w.roundtrip()

    def warp(self, x, y):
        bx, by, bw, bh = layout_box()
        self.w.send(self.vp, 1, "uuuuu", self.ms(), x - bx, y - by, bw, bh)
        self.w.send(self.vp, 4, "")
        self.w.roundtrip()

    def button(self, down, btn=BTN_LEFT):
        self.w.send(self.vp, 2, "uuu", self.ms(), btn, 1 if down else 0)
        self.w.send(self.vp, 4, "")
        self.w.roundtrip()

    def probe(self, x, y):
        self.warp(x, y)
        time.sleep(0.03)
        self.w.roundtrip()
        for _ in range(4):
            self.w.dispatch(0.01)
        name = next((n for n, s in self.surfaces.items() if s["surface"] == self.focus), None)
        return f"world {name}" if name else "other"


class Grab:
    """Screen regions through zwlr_screencopy_manager_v1, one compositor frame per capture (what was
    composited, with its presentation time on CLOCK_MONOTONIC, the clock of time.monotonic()). Root px; the
    region must lie on one output."""

    def __init__(self, w=None):
        self.w = w or Wl()
        names = [n for n, (i, _) in self.w.globals.items() if i == "wl_output"]
        if not any(i == "zwlr_screencopy_manager_v1" for i, _ in self.w.globals.values()):
            raise SystemExit("wlworld: compositor lacks zwlr_screencopy_manager_v1")
        self.mgr = self.w.bind("zwlr_screencopy_manager_v1", 3)
        self.outputs = {}  # wl_output id -> name
        for n in names:
            v = min(self.w.globals[n][1], 4)
            oid = self.w.new("wl_output")
            e = b"wl_output\0"
            body = struct.pack("<I", n) + struct.pack("<I", len(e)) + e + b"\0" * ((4 - len(e) % 4) % 4) + struct.pack("<II", v, oid)
            self.w.s.sendmsg([struct.pack("<II", self.w.registry, ((8 + len(body)) << 16) | 0) + body])
            self.w.handlers[oid] = lambda op, b, oid=oid: op == 4 and self.outputs.__setitem__(oid, b[4:4 + struct.unpack_from("<I", b)[0] - 1].decode())
        self.w.roundtrip()
        self.w.roundtrip()
        self.mon = {m["name"]: m for m in json.loads(hyprctl("-j", "monitors"))}
        self.pool = None  # (key, wl_buffer, mmap)

    def _buffer(self, fmt, w, h, stride):
        key = (fmt, w, h, stride)
        if self.pool and self.pool[0] == key:
            return self.pool[1], self.pool[2]
        size = stride * h
        fd = os.memfd_create("wlgrab")
        os.ftruncate(fd, size)
        m = mmap.mmap(fd, size)
        # (wl_shm first: new ids must arrive in the order they were made)
        self.shm = getattr(self, "shm", None) or self.w.bind("wl_shm", 1)
        pool = self.w.new("wl_shm_pool")
        self.w.send(self.shm, 0, "ni", pool, size, fds=[fd])
        os.close(fd)
        buf = self.w.new("wl_buffer")
        self.w.send(pool, 0, "niiiiu", buf, 0, w, h, stride, fmt)
        self.w.send(pool, 1, "")
        self.pool = (key, buf, m)
        return buf, m

    def grab(self, x, y, w, h):
        """(monotonic seconds of the frame, RGB bytes w*h*3) or None if the compositor refused."""
        name, m = next((n, m) for n, m in self.mon.items() if m["x"] <= x < m["x"] + m["width"] and m["y"] <= y < m["y"] + m["height"])
        out = next(o for o, n in self.outputs.items() if n == name)
        fr = self.w.new("zwlr_screencopy_frame_v1")
        st = {}

        def ev(op, b):
            if op == 0:
                st["fmt"] = struct.unpack_from("<IIII", b)
            elif op == 1:
                st["flags"] = struct.unpack_from("<I", b)[0]
            elif op == 2:
                hi, lo, ns = struct.unpack_from("<III", b)
                st["t"] = ((hi << 32) | lo) + ns / 1e9
            elif op == 3:
                st["failed"] = True
            elif op == 6:
                st["done"] = True

        self.w.handlers[fr] = ev
        self.w.send(self.mgr, 1, "niniiii", fr, 0, out, x - m["x"], y - m["y"], w, h)
        end = time.monotonic() + 2
        while "done" not in st and "failed" not in st and time.monotonic() < end:
            self.w.dispatch(0.05)
        if "fmt" not in st or "failed" in st:
            self.w.handlers.pop(fr, None)
            return None
        fmt, bw, bh, stride = st["fmt"]
        buf, mm = self._buffer(fmt, bw, bh, stride)
        self.w.send(fr, 0, "u", buf)
        while "t" not in st and "failed" not in st and time.monotonic() < end:
            self.w.dispatch(0.05)
        self.w.send(fr, 1, "")
        self.w.handlers.pop(fr, None)
        if "t" not in st:
            return None
        import numpy as np
        a = np.frombuffer(mm, dtype=np.uint8).reshape(bh, stride)[:, :bw * 4].reshape(bh, bw, 4)
        if st.get("flags", 0) & 1:
            a = a[::-1]
        return st["t"], np.ascontiguousarray(a[:, :, 2::-1]).tobytes()


def record_frames(g, r, fps, stop):
    """[(presentation time, RGB bytes)] of region r at `fps` until `stop` (a threading.Event) is set. fps 0: back
    to back, each grab asking for the next composited frame (every frame the output shows, while it changes)."""
    frames, t0, k = [], time.monotonic(), 0
    while not stop.is_set():
        if fps > 0:
            time.sleep(max(0.0, t0 + k / fps - time.monotonic()))
        k += 1
        f = g.grab(*r)
        if f:
            frames.append(f)
    return frames


def encode(frames, w, h, fps, out):
    """Frames into a lossless RGB clip (libx264rgb -qp 0) at `fps`, and OUT.times (each frame's presentation time)."""
    enc = subprocess.Popen(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}",
                            "-r", str(fps), "-i", "-", "-c:v", "libx264rgb", "-qp", "0", "-preset", "ultrafast", out], stdin=subprocess.PIPE)
    for _, px in frames:
        enc.stdin.write(px)
    enc.stdin.close()
    enc.wait()
    open(out + ".times", "w").write("".join(f"{t:.6f}\n" for t, _ in frames))


def record(a):
    """record X Y W H FPS SECS OUT.mp4: a lossless clip of the region (frames repeat while nothing changes)."""
    r, fps, secs, out = tuple(int(v) for v in a[:4]), float(a[4]), float(a[5]), a[6]
    import threading
    stop = threading.Event()
    threading.Timer(secs, stop.set).start()
    frames = record_frames(Grab(), r, fps, stop)
    encode(frames, r[2], r[3], fps, out)
    print(f"recorded {len(frames)} frames to {out}", flush=True)


def serve():
    world = World()
    out = sys.stdout
    out.write("ready\n")
    out.flush()
    for line in sys.stdin:
        a = line.split()
        if not a:
            continue
        try:
            c = a[0]
            n = lambda i: int(a[i])
            rgb = lambda i: int(a[i], 16) if len(a) > i else None
            if c == "toplevel":
                world.toplevel(a[1], n(2), n(3), rgb(4) or 0x808080)
                r = "ok"
            elif c == "layer":
                world.layer(a[1], n(2), n(3), n(4), n(5), a[6] if len(a) > 6 else "top", n(7) if len(a) > 7 else 0, rgb(8) or 0x606060)
                r = "ok"
            elif c == "move":
                world.move(a[1], n(2), n(3), n(4), n(5))
                r = "ok"
            elif c == "destroy":
                world.destroy(a[1])
                r = "ok"
            elif c == "under":
                bx, by, bw, bh = layout_box()
                world.layer("under", bx, by, bw, bh, "bottom", 0, 0x203040)
                r = "ok"
            elif c == "probe":
                r = world.probe(n(1), n(2))
            elif c == "warp":
                world.warp(n(1), n(2))
                r = "ok"
            elif c == "click":
                world.warp(n(1), n(2))
                world.button(True)
                world.button(False)
                r = "ok"
            elif c in ("press", "release"):
                world.button(c == "press")
                r = "ok"
            elif c == "hypr":
                r = " ".join(hyprctl(*a[1:]).split())
            else:
                r = f"err unknown {c}"
        except SystemExit:
            raise
        except Exception as e:  # noqa: BLE001 - reported to the driver, which fails the scenario
            r = f"err {type(e).__name__}: {e}"
        world.w.dispatch(0)
        out.write(r + "\n")
        out.flush()


def poke(a):
    world = World()
    c = a[0]
    if c == "move":
        world.warp(int(a[1]), int(a[2]))
    elif c == "click":
        world.warp(int(a[1]), int(a[2]))
        world.button(True, int(a[3]) if len(a) > 3 else BTN_LEFT)
        world.button(False, int(a[3]) if len(a) > 3 else BTN_LEFT)
    elif c in ("press", "release"):
        world.button(c == "press", int(a[1]) if len(a) > 1 else BTN_LEFT)
    elif c == "drag":
        x0, y0, x1, y1 = map(int, a[1:5])
        steps = int(a[5]) if len(a) > 5 else 40
        hz = float(a[6]) if len(a) > 6 else 120.0
        world.warp(x0, y0)
        world.button(True)
        t0 = time.time()
        for k in range(1, steps + 1):
            world.warp(round(x0 + (x1 - x0) * k / steps), round(y0 + (y1 - y0) * k / steps))
            time.sleep(max(0.0, t0 + k / hz - time.time()))
        world.button(False)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "serve":
        serve()
    elif len(sys.argv) > 2 and sys.argv[1] == "poke":
        poke(sys.argv[2:])
    elif len(sys.argv) > 2 and sys.argv[1] == "record":
        record(sys.argv[2:])
    else:
        raise SystemExit(__doc__)
