#!/usr/bin/env python3
"""shoot.py: run synaps-avatar on :0 for one short scenario, then make sure it is gone.

usage: shoot.py [--bin PATH] [--args "..."] [--timeout 28] STEP...
steps:  cmd:<avatarctl line>      send a dev-control command
        sleep:<seconds>
        shot:<name>[:pad]         maim the window region (+pad px around) -> shots/<name>.png
        desk:<name>[:WxH]         maim a WxH region of the monitor around the avatar (default 1100x760)
        click:<dx>:<dy>[:btn]     xdotool click at avatar-relative device px (window coords)
        rclick:<x>:<y>            xdotool click at root coords
        state                     print the state JSON
        log                       print the app's stderr so far
        pwin:<x>:<y>:<w>:<h>[:light|dark]  map a managed (i3-floating) test window; {pwin} in cmds = its id
        waitframe:<prefix>[:secs]  poll state until the current frame id starts with prefix
        pmove:<x>:<y>             move the test window; pclose closes it
--stage X,Y,W,H[,FLOOR[,0xRRGGBB]]: before the app starts, map an override-redirect backdrop that shows
        a plain colour, or the root wallpaper (ParentRelative: no focus change, no layout change) and a dark floor
        slab FLOOR px tall at its bottom; desk/rec default to the stage rect; {floor} = slab top - 6 (ground room).
Always kills the app at the end (quit, then SIGTERM, then SIGKILL) and removes test windows."""
import json, os, socket, subprocess, sys, time

# SA_LOSSLESS=1: lossless RGB (for pixel diffs); default: small H.264 for viewing.
ENC = (["-c:v", "libx264rgb", "-preset", "ultrafast", "-crf", "0"] if os.environ.get("SA_LOSSLESS")
       else ["-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p"])

HOME = os.path.expanduser("~")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SHOTS = os.environ.get("SA_SHOTS", os.path.join(REPO, "shots"))
SOCK = os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "synaps-avatar.sock")
os.environ.setdefault("DISPLAY", ":0")
os.environ.setdefault("XAUTHORITY", "/run/user/1000/lyxauth")


_disp = None


def xdisp():
    global _disp
    if _disp is None:
        from Xlib import display
        _disp = display.Display()
    return _disp


def xroot():
    return xdisp().screen().root


def ctl(line, timeout=5):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(SOCK)
    s.sendall((line.replace("\n", "\\n") + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(65536)
        if not c:
            break
        buf += c
    s.close()
    return buf.decode().strip()


def stage_windows(x, y, w, h, floor, plain=None):
    """Backdrop (wallpaper via ParentRelative, or a plain colour) + floor slab. Returns the windows."""
    from Xlib import X
    d = xdisp()
    root = xroot()
    out = []
    bgattr = {"background_pixel": plain} if plain is not None else {"background_pixmap": X.ParentRelative}
    bg = root.create_window(x, y, w, h, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent,
                            override_redirect=True, **bgattr)
    bg.set_wm_class("sa-stage", "sa-stage")
    out.append(bg)
    if floor:
        slab = root.create_window(x, y + h - floor, w, floor, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent,
                                  background_pixel=0x0e131a, override_redirect=True)
        slab.set_wm_class("sa-stage", "sa-stage")
        edge = slab.create_window(0, 0, w, 2, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent,
                                  background_pixel=0x2b3a4a)
        edge.map()
        out.append(slab)
    for win in out:
        win.map()
    d.sync()
    return out


THEMES = {
    "dark": (0x11161d, 0x222c39, [0x3a4a5c, 0x2f3d4c]),
    "light": (0xeef1f5, 0xd5dbe3, [0x9aa5b4, 0xb8c1cc]),
}


def perch_window(x, y, w, h, theme="dark"):
    """A managed dialog (i3 floats dialogs): body, title strip, a few 'text' bars."""
    body, title, ink = THEMES[theme]
    from Xlib import X, Xatom
    d = xdisp()
    root = xroot()
    win = root.create_window(x, y, w, h, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent,
                             background_pixel=body, event_mask=X.StructureNotifyMask)
    win.set_wm_name("perch test")
    win.set_wm_class("sa-perch-test", "sa-perch-test")
    win.change_property(d.intern_atom("_NET_WM_WINDOW_TYPE"), Xatom.ATOM, 32, [d.intern_atom("_NET_WM_WINDOW_TYPE_DIALOG")])
    win.set_wm_normal_hints(flags=1 | 2 | 4 | 8, x=x, y=y, width=w, height=h)  # USPosition|USSize|PPosition|PSize
    bar = win.create_window(0, 0, w, 30, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent, background_pixel=title)
    bar.map()
    for i, (lw, c) in enumerate([(0.62, ink[0]), (0.48, ink[1]), (0.7, ink[1]), (0.36, ink[1])]):
        line = win.create_window(22, 52 + i * 24, int((w - 44) * lw), 9, 0, X.CopyFromParent, X.InputOutput, X.CopyFromParent, background_pixel=c)
        line.map()
    win.map()
    d.sync()
    return win


def focus_is_ours():
    """Typing goes to whatever has the keyboard: on the live desktop only ever the avatar."""
    try:
        xid = json.loads(ctl("state")).get("xid")
    except Exception:
        return False
    f = xdisp().get_input_focus().focus
    return bool(xid) and getattr(f, "id", None) == xid


def user_idle_ms():
    return xroot().screensaver_query_info().idle


def wait_user_idle(need_ms=4000, max_s=12):
    """Haseeb is on this desktop: only touch the pointer once he has left it alone a moment."""
    t = time.time()
    while user_idle_ms() < need_ms:
        if time.time() - t > max_s:
            return False
        time.sleep(0.2)
    return True


def input_rects():
    """The avatar window's X input shape in root coords: where a click reaches it (anywhere
    else goes through to the window below)."""
    from Xlib.ext import shape
    st = json.loads(ctl("state"))
    w = st["window"]
    win = xdisp().create_resource_object("window", st["xid"])
    r = win.shape_get_rectangles(shape.SK.Input)
    return [(w["x"] + q.x, w["y"] + q.y, q.width, q.height) for q in r.rectangles]


def on_avatar(x, y, rects=None):
    return any(rx <= x < rx + rw and ry <= y < ry + rh for rx, ry, rw, rh in (rects or input_rects()))


def body_point(x, y):
    """(x, y) if the avatar's body takes clicks there, else the nearest point of the input
    shape inside her box (the shape has holes, e.g. between the legs; the chip and bubble
    above the box don't count)."""
    a = json.loads(ctl("state"))["avatar"]
    rects = [r for r in input_rects() if r[2] >= 8
             and r[1] >= a["y"] and r[1] + r[3] <= a["y"] + a["h"] and r[0] >= a["x"] and r[0] + r[2] <= a["x"] + a["w"]]
    if not rects:
        return x, y
    best = None
    for rx, ry, rw, rh in rects:
        px, py = min(max(x, rx + 3), rx + rw - 4), min(max(y, ry), ry + rh - 1)
        d = (px - x) ** 2 + (py - y) ** 2
        if best is None or d < best[0]:
            best = (d, px, py)
    return best[1], best[2]


def safe_click(x, y, btn="1"):
    """One XTEST click on the avatar (GPUI reads XInput2 buttons, which XSendEvent can't fake).
    Refused unless the point is inside the avatar's input shape (so it can't go through to a
    window below) and the user is idle; the pointer goes straight back in the same xdotool call."""
    if not on_avatar(x, y):
        print(f"shoot: {x},{y} is not on the avatar's input shape; click refused", file=sys.stderr)
        return False
    if not wait_user_idle():
        print("shoot: user is active; click skipped", file=sys.stderr)
        return False
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", btn, "mousemove", "restore"], check=True)
    return True


PUNCT = {" ": "space", "'": "apostrophe", ",": "comma", ".": "period", "?": "question", "!": "exclam",
         ":": "colon", ";": "semicolon", "-": "minus", "_": "underscore", "/": "slash", "(": "parenleft",
         ")": "parenright", '"': "quotedbl", "=": "equal", "+": "plus", "*": "asterisk", "#": "numbersign",
         "|": "bar", "&": "ampersand", "<": "less", ">": "greater", "$": "dollar", "%": "percent", "@": "at",
         "~": "asciitilde", "`": "grave", "[": "bracketleft", "]": "bracketright", "{": "braceleft",
         "}": "braceright", "\\": "backslash", "^": "asciicircum"}


def send_keys(xid, keys, delay=0.07):
    """Synthetic KeyPress/KeyRelease (XSendEvent) addressed to the avatar's window only: they
    can't land in another window whatever has the keyboard. `keys`: chars or X key names."""
    from Xlib import X, XK, protocol
    d = xdisp()
    win = d.create_resource_object("window", xid)
    for k in keys:
        sym = XK.string_to_keysym(PUNCT.get(k, k))
        codes = list(d.keysym_to_keycodes(sym)) if sym else []
        if not codes:
            print(f"shoot: no keycode for {k!r}", file=sys.stderr)
            continue
        code, index = codes[0]
        state = X.ShiftMask if index & 1 else 0
        for cls in (protocol.event.KeyPress, protocol.event.KeyRelease):
            ev = cls(time=X.CurrentTime, root=xroot(), window=win, same_screen=1, child=X.NONE,
                     root_x=0, root_y=0, event_x=1, event_y=1, state=state, detail=code)
            win.send_event(ev, propagate=False)
        d.sync()
        time.sleep(delay)


def i3(cmd):
    return subprocess.run(["i3-msg", cmd], capture_output=True, text=True).stdout.strip()


def main():
    argv = sys.argv[1:]
    binp = f"{HOME}/.cache/cargo-target-synaps-avatar/debug/synaps-avatar"
    extra, tmo, stage, ground, rec_start, bg_rec = [], 28, None, None, None, None
    remote_sandbox = False
    while argv and argv[0].startswith("--"):
        k = argv.pop(0)
        if k == "--bin":
            binp = argv.pop(0)
        elif k == "--args":
            extra = argv.pop(0).split()
        elif k == "--timeout":
            tmo = int(argv.pop(0))
        elif k == "--rec-start":
            rec_start = argv.pop(0)
        elif k == "--remote-sandbox":
            # Run through synaps-avatar-remote: an ssh tunnel to bella itself (localhost) onto the
            # desktopproto SANDBOX socket, never a live daemon.
            remote_sandbox = True
        elif k == "--idle":
            need = int(argv.pop(0))
            t = time.time()
            ok = wait_user_idle(need, 60)
            print(f"shoot: user idle {need} ms before start: {'ok' if ok else 'NO (user active)'} after {time.time()-t:.1f}s")
        elif k == "--ground":
            ground = argv.pop(0)
        elif k == "--stage":
            stage = [int(v, 0) for v in argv.pop(0).split(",")]
    # Daemon safety: only ever the desktopproto sandbox, or no session at all.
    if remote_sandbox:
        if "--profile" in extra or "--socket" in extra:
            print("shoot: --remote-sandbox picks the socket itself", file=sys.stderr)
            return 1
        os.environ["SYNAPS_AVATAR_BIN"] = binp
        os.environ["SYNAPS_AVATAR_REMOTE_SOCK"] = f"{HOME}/.synaps-cli/run/daemon-desktopproto.sock"
        os.environ["SYNAPS_AVATAR_SSH_OPTS"] = "-o UserKnownHostsFile=/tmp/sa-kh -o StrictHostKeyChecking=accept-new"
        binp = os.path.join(os.path.dirname(os.path.abspath(__file__)), "synaps-avatar-remote")
        extra = ["localhost"] + extra
    elif "--profile" in extra:
        if extra[extra.index("--profile") + 1] != "desktopproto":
            print("shoot: only --profile desktopproto is allowed", file=sys.stderr)
            return 1
    elif "--socket" in extra:
        sp = extra[extra.index("--socket") + 1]
        if not (sp.startswith("/tmp/sa-fake/") or sp == f"{HOME}/.synaps-cli/run/daemon-desktopproto.sock"):
            print("shoot: --socket must be a fake daemon under /tmp/sa-fake/ or the desktopproto sandbox", file=sys.stderr)
            return 1
    elif "--no-session" not in extra:
        extra.append("--no-session")
    os.environ.pop("SYNAPS_PROFILE", None)
    if subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True).returncode == 0:
        print("shoot: an avatar is already running; refusing", file=sys.stderr)
        return 1
    tmo = min(tmo, 30)
    focus0 = xdisp().get_input_focus().focus
    focus0 = focus0.id if hasattr(focus0, "id") else None
    staged, pwin = [], None
    if stage:
        sx, sy, sw, sh = stage[:4]
        floor = stage[4] if len(stage) > 4 else 0
        plain = stage[5] if len(stage) > 5 else None
        staged = stage_windows(sx, sy, sw, sh, floor, plain)
        # Feet keep the app's 6 px ground room above the slab, as above the monitor edge.
        os.environ["SA_FLOOR"] = str(sy + sh - floor - 6)
    if ground:
        gx, gy = ground.replace("{floor}", os.environ.get("SA_FLOOR", "0")).split(",")
        sp = f"{HOME}/.local/state/synaps-avatar/state.json"
        os.makedirs(os.path.dirname(sp), exist_ok=True)
        open(sp, "w").write(json.dumps({"ground": [int(gx), int(gy)]}))
    log = open("/tmp/sa-shoot.log", "w")
    t0 = time.time()
    if rec_start and stage:
        name, secs = rec_start.split(":")
        x, y, W, H = stage[:4]
        out = f"{SHOTS}/{name}.mp4"
        bg_rec = subprocess.Popen(["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0", "-framerate", "30",
                                   "-video_size", f"{W}x{H}", "-i", f"{os.environ['DISPLAY']}+{x},{y}", "-t", secs,
                                   *ENC, out])
        time.sleep(0.4)
        print(f"rec-start {out}")
    p = subprocess.Popen(["timeout", str(tmo), binp] + extra, stdout=log, stderr=subprocess.STDOUT, env=os.environ)
    st = None
    q = xroot().query_pointer()
    home = {"X": str(q.root_x), "Y": str(q.root_y)}
    try:
        for _ in range(200):
            try:
                st = json.loads(ctl("state"))
                if st.get("loaded"):
                    break
            except Exception:
                pass
            time.sleep(0.05)
        print(f"shoot: loaded after {time.time()-t0:.2f}s")
        for step in argv:
            kind, _, rest = step.partition(":")
            if kind == "cmd":
                line = rest.replace("{pwin}", hex(pwin.id) if pwin else "0")
                if stage:
                    line = line.replace("{floor}", os.environ["SA_FLOOR"])
                print(f"> {line}: {ctl(line)}")
            elif kind == "pwin":
                parts = rest.split(":")
                x, y, w, h = (int(v) for v in parts[:4])
                pwin = perch_window(x, y, w, h, parts[4] if len(parts) > 4 else "dark")
                time.sleep(0.4)
                print(i3(f'[id="{pwin.id}"] floating enable, resize set {w} px {h} px, move absolute position {x} {y}'))
                time.sleep(0.3)
                frame = pwin.query_tree().parent
                # The stage is override-redirect and sits above i3's frames: tuck it under.
                from Xlib import X
                for w in staged:
                    w.configure(sibling=frame, stack_mode=X.Below)
                xdisp().sync()
                g = frame.get_geometry()
                print(f"pwin {hex(pwin.id)} frame at {g.x},{g.y} {g.width}x{g.height}; focus {hex(xdisp().get_input_focus().focus.id)}")
            elif kind == "pmove":
                x, y = rest.split(":")
                print(i3(f'[id="{pwin.id}"] move absolute position {x} {y}'))
            elif kind == "pclose":
                pwin.destroy()
                xdisp().sync()
                pwin = None
                print("pwin closed")
            elif kind == "sleep":
                time.sleep(float(rest))
            elif kind == "waitframe":
                pre, _, secs = rest.partition(":")
                t_end = time.time() + float(secs or 3)
                while time.time() < t_end and not json.loads(ctl("state"))["frame"].startswith(pre):
                    time.sleep(0.01)
                print(f"waitframe {pre}: {json.loads(ctl('state'))['frame']}")
            elif kind == "state":
                print(ctl("state"))
            elif kind in ("shot", "desk"):
                st = json.loads(ctl("state"))
                w = st["window"]
                name, _, opt = rest.partition(":")
                if kind == "shot":
                    pad = int(opt or 16)
                    m = st["monitor"]
                    x0, y0 = max(m["x"], w["x"] - pad), max(m["y"], w["y"] - pad)
                    x1 = min(m["x"] + m["w"], w["x"] + w["w"] + pad)
                    y1 = min(m["y"] + m["h"], w["y"] + w["h"] + pad)
                    g = f"{x1-x0}x{y1-y0}+{x0}+{y0}"
                elif stage and not opt:
                    g = f"{stage[2]}x{stage[3]}+{stage[0]}+{stage[1]}"
                else:
                    W, H = (int(v) for v in (opt or "1100x760").split("x"))
                    gx, gy = st["ground"]
                    mon = st.get("monitor") or {"x": 0, "y": 0, "w": 99999, "h": 99999}
                    x = max(mon["x"], min(gx - W // 2, mon["x"] + mon["w"] - W))
                    y = max(mon["y"], min(gy - H, mon["y"] + mon["h"] - H))
                    g = f"{W}x{H}+{x}+{y}"
                out = f"{SHOTS}/{name}.png"
                subprocess.run(["maim", "-u", "-g", g, out], check=True)
                print(f"shot {out} {g}")
            elif kind == "click":
                parts = rest.split(":")
                st = json.loads(ctl("state"))
                w = st["window"]
                btn = parts[2] if len(parts) > 2 else "1"
                x, y = w["x"] + int(parts[0]), w["y"] + int(parts[1])
                if safe_click(x, y, btn):
                    print(f"click {btn} at {x},{y}")
            elif kind == "clickrel":
                # clickrel:<fx>:<fy>  click at a fraction of the avatar's box (0.5:0.65 = the body)
                fx, fy = (float(v) for v in rest.split(":"))
                a = json.loads(ctl("state"))["avatar"]
                x, y = body_point(a["x"] + int(a["w"] * fx), a["y"] + int(a["h"] * fy))
                if safe_click(x, y):
                    print(f"clickrel at {x},{y}")
            elif kind == "button":
                # button:<id>  click a bubble button (allow, deny, answer, abort) by its laid-out
                # rect; it should be gone after the click, else read the rect again (up to 3 tries).
                for attempt in range(3):
                    b = json.loads(ctl("state")).get("buttons", {}).get(rest)
                    if not b:
                        if attempt == 0:
                            print(f"shoot: no button {rest!r} on screen", file=sys.stderr)
                        break
                    x, y = b[0] + b[2] // 2, b[1] + b[3] // 2
                    if not safe_click(x, y):
                        break
                    print(f"button {rest} at {x},{y} (try {attempt + 1})")
                    if rest == "chip":
                        break  # the tag stays
                    time.sleep(0.4)
            elif kind == "probe" and not os.environ.get("SA_POINTER_OK"):
                print("shoot: probe moves the user's pointer; refused (SA_POINTER_OK=1 overrides)", file=sys.stderr)
            elif kind == "probe":
                # Move the pointer to window-relative px and ask X which top-level is under it.
                dx, dy = (int(v) for v in rest.split(":"))
                w = json.loads(ctl("state"))["window"]
                x, y = w["x"] + dx, w["y"] + dy
                subprocess.run(["xdotool", "mousemove", str(x), str(y)], check=True)
                time.sleep(0.15)
                q = xroot().query_pointer()
                child = q.child
                name = child.get_wm_class() if child else None
                print(f"probe {x},{y} (pointer {q.root_x},{q.root_y}): top-level under pointer {hex(child.id) if child else None} {name}")
            elif kind == "drag" and not os.environ.get("SA_POINTER_OK"):
                print("shoot: drag moves the user's pointer; refused (SA_POINTER_OK=1 overrides)", file=sys.stderr)
            elif kind == "drag":
                dx, dy, mx, my = (int(v) for v in rest.split(":"))
                w = json.loads(ctl("state"))["window"]
                x, y = w["x"] + dx, w["y"] + dy
                subprocess.run(["xdotool", "mousemove", str(x), str(y), "mousedown", "1"], check=True)
                for i in range(1, 11):
                    time.sleep(0.03)
                    subprocess.run(["xdotool", "mousemove", str(x + mx * i // 10), str(y + my * i // 10)], check=True)
                time.sleep(0.1)
                subprocess.run(["xdotool", "mouseup", "1"], check=True)
                print(f"drag from {x},{y} by {mx},{my}")
            elif kind == "rclick" and not os.environ.get("SA_POINTER_OK"):
                print("shoot: rclick moves the user's pointer; refused (SA_POINTER_OK=1 overrides)", file=sys.stderr)
            elif kind == "rclick":
                x, y = rest.split(":")
                subprocess.run(["xdotool", "mousemove", x, y, "click", "1"], check=True)
            elif kind == "rec":
                # rec:<name>:<seconds>:<WxH>  ffmpeg x11grab of a region around the avatar
                name, secs, *wh = rest.split(":")
                st = json.loads(ctl("state"))
                if stage and not wh:
                    x, y, W, H = stage[:4]
                else:
                    W, H = (int(v) for v in wh[0].split("x"))
                    gx, gy = st["ground"]
                    mon = st["monitor"]
                    x = max(mon["x"], min(gx - W // 2, mon["x"] + mon["w"] - W))
                    y = max(mon["y"], min(gy - H, mon["y"] + mon["h"] - H))
                out = f"{SHOTS}/{name}.mp4"
                cmd = ["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0", "-framerate", "30",
                       "-video_size", f"{W}x{H}", "-i", f"{os.environ['DISPLAY']}+{x},{y}",
                       "-t", secs, *ENC, out]
                t = time.time()
                subprocess.run(cmd, check=True)
                print(f"rec {out} {W}x{H}+{x}+{y} {time.time()-t:.1f}s")
            elif kind == "park":
                pass
            elif kind == "key":
                if not focus_is_ours():
                    print("shoot: X focus is not the avatar; key step refused (live desktop)", file=sys.stderr)
                    continue
                send_keys(json.loads(ctl("state"))["xid"], rest.split(), delay=0.08)
                print(f"key {rest}")
            elif kind == "waitfor":
                # waitfor:<python expr over the state dict s>:<secs>
                expr, _, secs = rest.rpartition(":")
                t = time.time()
                ok = False
                while time.time() - t < float(secs):
                    try:
                        s_ = json.loads(ctl("state"))
                        if eval(expr, {}, {"s": s_}):
                            ok = True
                            break
                    except Exception:
                        pass
                    time.sleep(0.05)
                print(f"waitfor {expr}: {'ok' if ok else 'TIMEOUT'} after {time.time()-t:.2f}s")
            elif kind == "type":
                if not focus_is_ours():
                    print("shoot: X focus is not the avatar; type step refused (live desktop)", file=sys.stderr)
                    continue
                send_keys(json.loads(ctl("state"))["xid"], list(rest), delay=0.03)
                print(f"typed {rest!r}")
            elif kind == "daemon":
                # daemon:stop|start|reload  the desktopproto SANDBOX only (never another profile)
                verb = {"stop": ["stop"], "start": ["start", "--detach", "--idle-exit", "900"], "reload": ["reload"]}[rest]
                t = time.time()
                r = subprocess.run(["synaps", "daemon", *verb, "--profile", "desktopproto"], capture_output=True, text=True, timeout=20)
                print(f"daemon {rest}: rc {r.returncode} {(r.stdout + r.stderr).strip()[:120]!r} ({time.time()-t:.1f}s)")
            elif kind == "fake":
                # fake:<secret|confirm|compact|compacted|idle>  drive tools/fakedaemon.py
                c = socket.socket(socket.AF_UNIX)
                c.connect("/tmp/sa-fake/ctl.sock")
                c.sendall((rest + "\n").encode())
                print(f"fake {rest}: {c.recv(100).decode().strip()}")
                c.close()
            elif kind in ("cpu", "cpuuntil"):
                # cpu:<secs>  the app's CPU (all threads, schedstat ns) and RSS over <secs>
                # cpuuntil:<expr>:<max secs>  the same from now until <expr> holds
                pid = int(subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True, text=True).stdout.split()[0])
                def ns():
                    t = 0
                    for d in os.listdir(f"/proc/{pid}/task"):
                        try:
                            t += int(open(f"/proc/{pid}/task/{d}/schedstat").read().split()[0])
                        except OSError:
                            pass
                    return t
                def rss():
                    return int(open(f"/proc/{pid}/status").read().split("VmRSS:")[1].split()[0]) // 1024
                c0, w0, peak = ns(), time.time(), rss()
                if kind == "cpu":
                    end = w0 + float(rest)
                    while time.time() < end:
                        time.sleep(0.2)
                        peak = max(peak, rss())
                else:
                    expr, _, mx = rest.rpartition(":")
                    while time.time() - w0 < float(mx):
                        peak = max(peak, rss())
                        try:
                            if eval(expr, {}, {"s": json.loads(ctl("state"))}):
                                break
                        except Exception:
                            pass
                        time.sleep(0.1)
                c1, w1 = ns(), time.time()
                print(f"{kind} {rest}: CPU {100 * (c1 - c0) / 1e9 / (w1 - w0):.2f} % of one core over {w1 - w0:.1f} s "
                      f"({(c1 - c0) / 1e6:.0f} ms); RSS now {rss()} MB, peak {peak} MB; threads {len(os.listdir(f'/proc/{pid}/task'))}; "
                      f"load {open('/proc/loadavg').read().split()[0]}")
            elif kind == "killtunnel":
                r = subprocess.run(["pkill", "-f", "ssh -N .*synaps-localhost-capri.sock"], capture_output=True)
                print(f"killtunnel: {'killed' if r.returncode == 0 else 'no tunnel'}")
            elif kind == "tunnel":
                r = subprocess.run(["pgrep", "-af", "ssh -N .*synaps-localhost-capri.sock"], capture_output=True, text=True)
                print(f"tunnel: {len(r.stdout.splitlines())} ssh; socket {'up' if os.path.exists(os.environ.get('XDG_RUNTIME_DIR', '/tmp') + '/synaps-localhost-capri.sock') else 'gone'}")
            elif kind == "waitidle":
                # waitidle:<ms>:<max s>  wait until the user has left the desktop alone <ms>
                ms, _, mx = rest.partition(":")
                t = time.time()
                ok = wait_user_idle(int(ms), float(mx or 12))
                print(f"waitidle {ms} ms: {'ok' if ok else 'USER ACTIVE'} after {time.time()-t:.1f}s")
            elif kind == "focus":
                f = xdisp().get_input_focus().focus
                print("X input focus:", hex(f.id) if hasattr(f, "id") else f)
            elif kind == "bgrec":
                # bgrec:<name>:<seconds>  start recording the stage now, keep going with steps
                name, secs = rest.split(":")
                x, y, W, H = stage[:4]
                out = f"{SHOTS}/{name}.mp4"
                cmd = ["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "0", "-framerate", "30",
                       "-video_size", f"{W}x{H}", "-i", f"{os.environ['DISPLAY']}+{x},{y}",
                       "-t", secs, *ENC, out]
                bg_rec = subprocess.Popen(cmd)
                print(f"bgrec {out} started")
            elif kind == "waitrec":
                bg_rec.wait()
                print("bgrec done")
            elif kind == "log":
                log.flush()
                print(open("/tmp/sa-shoot.log").read())
            else:
                print(f"shoot: unknown step {step}", file=sys.stderr)
    finally:
        if bg_rec is not None:
            bg_rec.wait(timeout=30)
        if pwin is not None:
            pwin.destroy()
            xdisp().sync()
        for w in staged:
            w.destroy()
        if staged:
            xdisp().sync()
        try:
            ctl("quit", timeout=2)
        except Exception:
            pass
        for sig in (None, "TERM", "KILL"):
            try:
                p.wait(timeout=1.5)
                break
            except subprocess.TimeoutExpired:
                if sig:
                    subprocess.run(["pkill", f"-{sig}", "-x", "synaps-avatar"])
        alive = subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True).returncode == 0
        # Only hand focus back if nothing has it (the avatar had it and is gone): never take
        # it from a window the user picked meanwhile.
        f = xdisp().get_input_focus().focus
        fid = getattr(f, "id", f)
        if focus0 and (not isinstance(fid, int) or fid <= 1 or fid == xroot().id):
            print("restore focus:", i3(f'[id="{focus0}"] focus'))
        print("shoot: app exited" if not alive else "shoot: APP STILL RUNNING")
        log.close()
        print("---- app log ----")
        print("".join(l for l in open("/tmp/sa-shoot.log") if "MESA" not in l and "pci id" not in l))


if __name__ == "__main__":
    sys.exit(main())
