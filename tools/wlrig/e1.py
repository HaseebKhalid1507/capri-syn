#!/usr/bin/env python3
"""e1.py W H SECONDS: experiment E1 (BRIEF-wayland §2.1, §4.7): one layer surface W×H at (0, 0), a 24 px
square blinking at 5 Hz (two shm buffers, damage = the square only), for SECONDS. Hyprland's and sway's
CPU are read from the host by e1.sh, which runs this in the rig."""
import mmap, os, struct, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wlworld import World

W, H, T = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3])
w = World()
w.layer("capri-e1", 0, 0, W, H, "top", 0, 0x203040)
sid = w.surfaces["capri-e1"]["surface"]


def buf(on):
    stride, size = W * 4, W * H * 4
    fd = os.memfd_create("e1")
    os.ftruncate(fd, size)
    m = mmap.mmap(fd, size)
    m.write(struct.pack("<I", 0x40203040) * (W * H))  # translucent, as her window is
    if on:
        for y in range(100, 124):
            m[y * stride + 100 * 4: y * stride + 124 * 4] = struct.pack("<I", 0xFFFF2040) * 24
    m.close()
    pool = w.w.new("wl_shm_pool")
    w.w.send(w.shm, 0, "ni", pool, size, fds=[fd])
    os.close(fd)
    b = w.w.new("wl_buffer")
    w.w.send(pool, 0, "niiiiu", b, 0, W, H, stride, 0)  # ARGB8888
    w.w.send(pool, 1, "")
    return b


bufs = [buf(False), buf(True)]
t0, k = time.time(), 0
while time.time() - t0 < T:
    k += 1
    w.w.send(sid, 1, "uii", bufs[k % 2], 0, 0)
    w.w.send(sid, 9, "iiii", 100, 100, 24, 24)  # damage_buffer: the square
    w.w.send(sid, 6, "")
    w.w.dispatch(0)
    time.sleep(max(0.0, t0 + k * 0.2 - time.time()))
print(f"e1 {W}x{H}: {k} presents in {time.time() - t0:.1f} s")
