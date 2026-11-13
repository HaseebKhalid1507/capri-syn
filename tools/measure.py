#!/usr/bin/env python3
"""measure.py BIN [IDLE_S] [PACK]: the body alone (--no-session): cold start → first frame, idle RSS after 60 s, idle CPU over 30 s,
RSS during a say cycle. Prints load average next to each number. One instance; always killed."""
import json, os, socket, subprocess, sys, time
B = sys.argv[1]
idle_wait = float(sys.argv[2]) if len(sys.argv) > 2 else 60
PACK = sys.argv[3] if len(sys.argv) > 3 else None
SOCK = os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "synaps-avatar.sock")
os.environ.setdefault("DISPLAY", ":0"); os.environ.setdefault("XAUTHORITY", "/run/user/1000/lyxauth")
HZ = os.sysconf("SC_CLK_TCK")

def load(): return open("/proc/loadavg").read().split()[:3]
def ctl(line):
    s = socket.socket(socket.AF_UNIX); s.settimeout(5); s.connect(SOCK)
    s.sendall((line + "\n").encode()); r = b""
    while not r.endswith(b"\n"):
        c = s.recv(65536)
        if not c: break
        r += c
    return r.decode().strip()
def rss(pid): return int(subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip() or 0) // 1024
def cpu_ns(pid):
    """CPU time of every thread, ns (schedstat: much finer than 100 Hz ticks)."""
    t = 0
    for d in os.listdir(f"/proc/{pid}/task"):
        try: t += int(open(f"/proc/{pid}/task/{d}/schedstat").read().split()[0])
        except OSError: pass
    return t
def cpu_over(pid, secs, action=None):
    c0, w0 = cpu_ns(pid), time.time()
    if action: action()
    time.sleep(max(0, secs - (time.time() - w0)))
    c1, w1 = cpu_ns(pid), time.time()
    return 100 * (c1 - c0) / 1e9 / (w1 - w0), (c1 - c0) / 1e6, w1 - w0
def cpu_ticks(pid):
    f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return int(f[11]) + int(f[12])

if subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True).returncode == 0:
    sys.exit("already running")
print("binary size:", os.path.getsize(B), "bytes")
t0 = time.time()
log = open("/tmp/sa-measure.log", "w")
# --no-session: never a live daemon (session numbers: shoot.py on the desktopproto sandbox).
p = subprocess.Popen(["timeout", "170", B, "--no-session"] + (["--pack", PACK] if PACK else []), stdout=log, stderr=subprocess.STDOUT)
try:
    first = None
    while time.time() - t0 < 10:
        for l in open("/tmp/sa-measure.log"):
            if l.startswith("first-frame: painted"):
                first = l.strip()
        if first: break
        time.sleep(0.02)
    print(f"{first}   [load {load()}]")
    app = int(subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True, text=True).stdout.split()[0])
    time.sleep(3)
    print([l.strip() for l in open("/tmp/sa-measure.log") if "decoded" in l])
    print(f"waiting {idle_wait:.0f} s idle (autonomy keeps running)...", flush=True)
    time.sleep(max(0, idle_wait - 3))
    print(f"(app pid {app})")
    print(f"idle RSS after {idle_wait:.0f} s: {rss(app)} MB   [load {load()}]")
    c0, w0, n0 = cpu_ticks(app), time.time(), cpu_ns(app)
    time.sleep(30)
    c1, w1, n1 = cpu_ticks(app), time.time(), cpu_ns(app)
    print(f"idle CPU over 30 s: {100*(n1-n0)/1e9/(w1-w0):.3f} %  ({(n1-n0)/1e6:.0f} ms CPU; {c1-c0} ticks @ {HZ} Hz)   [load {load()}]")
    peak = 0
    ctl("say Measuring memory while I talk. This is a longer line so the typewriter has a few seconds of work to do, and the bubble grows.")
    c0, w0 = cpu_ticks(app), time.time()
    for _ in range(40):
        peak = max(peak, rss(app)); time.sleep(0.2)
    c1, w1 = cpu_ticks(app), time.time()
    print(f"say cycle: peak RSS {peak} MB, CPU {100*(c1-c0)/HZ/(w1-w0):.2f} % over {w1-w0:.1f} s   [load {load()}]")
    time.sleep(8)  # bubble gone, back to idle
    st = json.loads(ctl("state")); gx, gy = st["ground"]
    pct, ms_, wall = cpu_over(app, 0.95, lambda: ctl("glitch"))
    print(f"big glitch (850 ms: 0.55 s hit + 3 stutters, tag torn): CPU {pct:.1f} % of one core ({ms_:.0f} ms over {wall:.2f} s)   [load {load()}]")
    time.sleep(0.5)
    pct, ms_, wall = cpu_over(app, 1.05, lambda: ctl(f"pos {gx-420} {gy}"))
    print(f"teleport (vanish 300 + bolt 277 + land 330 ms, overlapping): CPU {pct:.1f} % ({ms_:.0f} ms over {wall:.2f} s)   [load {load()}]")
    time.sleep(0.5)
    ctl(f"pos {gx} {gy} now")
    ctl("mood needs_you"); time.sleep(3)
    pct, ms_, wall = cpu_over(app, 10)
    print(f"needs_you loop (wave every ~3.5 s + hop every ~3.2 s): CPU {pct:.2f} % over {wall:.1f} s   [load {load()}]")
    ctl("mood idle"); time.sleep(4)
    pct, ms_, wall = cpu_over(app, 10)
    print(f"idle again, 10 s (breath + sway + glints + blink): CPU {pct:.3f} %   [load {load()}]")
    ctl("music on"); time.sleep(3)
    pct, ms_, wall = cpu_over(app, 15)
    print(f"music on, 15 s (notes at 25 Hz + nod/sway on twos): CPU {pct:.2f} %  RSS {rss(app)} MB   [load {load()}]")
    ctl("music off"); print(f"state right after music off: music={json.loads(ctl('state'))['music']}"); time.sleep(3)
    pct, ms_, wall = cpu_over(app, 10)
    print(f"music off again, 10 s: CPU {pct:.3f} %   state music={json.loads(ctl('state'))['music']}   [load {load()}]")
    print(f"final RSS {rss(app)} MB")
finally:
    try: ctl("quit")
    except Exception: pass
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: subprocess.run(["pkill", "-KILL", "-x", "synaps-avatar"])
    print("exited:", subprocess.run(["pgrep", "-x", "synaps-avatar"], capture_output=True).returncode != 0)
