#!/usr/bin/env python3
"""wlclamp.py LISTEN UPSTREAM: a Wayland relay that clamps every wl_registry.bind to the version the
server advertised. Hyprland 0.57's nested (Wayland) backend binds xdg_wm_base v6 unconditionally and
sway 1.12 offers v5 ("invalid version for global xdg_wm_base"), so the rig puts this between them.
Bytes and fds pass through unchanged otherwise. Runs inside the rig's sandbox (inner.sh)."""
import os, socket, struct, sys, threading

LISTEN, UP = sys.argv[1], sys.argv[2]


def strarg(b, off):
    n = struct.unpack_from("<I", b, off)[0]
    s = b[off + 4: off + 4 + n - 1].decode() if n else ""
    return s, off + 4 + ((n + 3) & ~3)


def relay(c, u):
    registries, adv = set(), {}  # registry ids; global name -> advertised version
    lock = threading.Lock()

    def down():  # server -> client: pass through, note the registries' globals
        buf = b""
        while True:
            try:
                data, fds, _, _ = socket.recv_fds(u, 65536, 255)
            except OSError:
                data, fds = b"", []
            if not data:
                break
            c.sendmsg([data], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack(f"{len(fds)}i", *fds))] if fds else [])
            for fd in fds:
                os.close(fd)
            buf += data
            while len(buf) >= 8:
                obj, so = struct.unpack_from("<II", buf)
                size, op = so >> 16, so & 0xFFFF
                if len(buf) < size:
                    break
                with lock:
                    if obj in registries and op == 0:  # global(name, interface, version)
                        name = struct.unpack_from("<I", buf, 8)[0]
                        _, off = strarg(buf, 12)
                        adv[name] = struct.unpack_from("<I", buf, off)[0]
                buf = buf[size:]
        c.close()

    threading.Thread(target=down, daemon=True).start()
    buf, pend = bytearray(), []
    while True:
        try:
            data, fds, _, _ = socket.recv_fds(c, 65536, 255)
        except OSError:
            data, fds = b"", []
        if not data:
            break
        buf += data
        pend += fds
        out = bytearray()
        while len(buf) >= 8:
            obj, so = struct.unpack_from("<II", buf)
            size, op = so >> 16, so & 0xFFFF
            if len(buf) < size:
                break
            m = bytearray(buf[:size])
            with lock:
                if obj == 1 and op == 1:  # wl_display.get_registry(new_id)
                    registries.add(struct.unpack_from("<I", m, 8)[0])
                elif obj in registries and op == 0:  # bind(name, interface, version, id)
                    name = struct.unpack_from("<I", m, 8)[0]
                    iface, off = strarg(m, 12)
                    ver = struct.unpack_from("<I", m, off)[0]
                    if name in adv and ver > adv[name]:
                        sys.stderr.write(f"wlclamp: {iface} v{ver} -> v{adv[name]}\n")
                        struct.pack_into("<I", m, off, adv[name])
            out += m
            del buf[:size]
        if out:
            u.sendmsg([bytes(out)], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack(f"{len(pend)}i", *pend))] if pend else [])
            for fd in pend:
                os.close(fd)
            pend = []
    u.close()


if os.path.exists(LISTEN):
    os.unlink(LISTEN)
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(LISTEN)
srv.listen(4)
while True:
    c, _ = srv.accept()
    u = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    u.connect(UP)
    threading.Thread(target=relay, args=(c, u), daemon=True).start()
