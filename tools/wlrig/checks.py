#!/usr/bin/env python3
"""checks.py: phase-0 checks that need clients (BRIEF-wayland §4.7): V5 pointer, V6 binds, V7 Lua dispatch,
E2 margin moves, E3 the Line's keyboard modes. Inside the rig (run.sh python3 checks.py). One line each."""
import json, os, subprocess, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wlworld import World, hyprctl

RIG = os.environ["RIG"]
fails = 0


def line(name, ok, detail):
    global fails
    fails += not ok
    print(f"{name} {'PASS' if ok else 'FAIL'} {detail}", flush=True)


def ev(code):
    return hyprctl("eval", code).strip()


def client(cls):
    return next((c for c in json.loads(hyprctl("-j", "clients")) if c["class"] == cls), None)


def active():
    return json.loads(hyprctl("-j", "activewindow") or "{}").get("class")


def rule(cls, x, y, w, h):
    ev(f'hl.window_rule({{ name = "{cls}", match = {{ class = "^{cls}$" }}, float = true, move = "{x} {y}", size = "{w} {h}" }})')


w = World()
# V5: clicks land (follow_mouse = 0: focus moves by click only)
ev("hl.config({ input = { follow_mouse = 0 } })")
rule("v5-a", 200, 300, 600, 400)
rule("v5-b", 1000, 300, 600, 400)
w.toplevel("v5-a", 600, 400)
w.toplevel("v5-b", 600, 400)
time.sleep(0.3)
seen = []
for x, y in ((500, 500), (1300, 500), (500, 500)):
    w.warp(x, y)
    w.button(True)
    w.button(False)
    time.sleep(0.2)
    seen.append(active())
line("V5", seen == ["v5-a", "v5-b", "v5-a"], f"clicks on a, b, a -> activewindow {seen}")
w.destroy("v5-a")
w.destroy("v5-b")
ev("hl.config({ input = { follow_mouse = 1 } })")

# V6: SUPER+C press/release fires both binds, also with SUPER let go first
log = os.path.join(RIG, "out", "avatarctl.log")
open(log, "a").close()
n0 = len(open(log).read().splitlines())
subprocess.run(["wtype", "-M", "logo", "-P", "c", "-s", "300", "-p", "c", "-m", "logo"], timeout=5)
time.sleep(0.6)
subprocess.run(["wtype", "-M", "logo", "-P", "c", "-s", "300", "-m", "logo", "-s", "100", "-p", "c"], timeout=5)
time.sleep(0.8)
got = [" ".join(l.split()[1:]) for l in open(log).read().splitlines()[n0:]]
line("V6", got == ["talk press", "talk release"] * 2, f"avatarctl calls: {got}" + ("" if got else
     " (wtype's virtual keyboard registers as the main keyboard but no bind fires for it, a plain `X` bind neither;"
     " exec_cmd dispatches do run: P3 drives `avatarctl talk press/release` with the same timings)"))

# V7: Lua dispatch (exec_cmd rules, window move/resize/fullscreen, workspace focus, cursor move) + the layer rule
out = []
hyprctl("dispatch", "hl.dsp.exec_cmd('true', { float = true, size = '300 200', move = '40 60' })")
rule("v7", 300, 200, 900, 600)
w.toplevel("v7", 900, 600)
sel = 'window = "class:^v7$"'
steps = [
    (f"hl.dsp.window.move({{ x = 150, y = 350, {sel} }})", lambda c: c["at"] == [150, 350]),
    (f"hl.dsp.window.resize({{ x = 520, y = 310, {sel} }})", lambda c: c["size"] == [520, 310]),
    (f"hl.dsp.window.fullscreen({{ mode = 'fullscreen', action = 'set', {sel} }})", lambda c: c["fullscreen"] == 2),
    (f"hl.dsp.window.fullscreen({{ mode = 'fullscreen', action = 'unset', {sel} }})", lambda c: c["fullscreen"] == 0),
    (f"hl.dsp.window.move({{ workspace = 9, follow = false, {sel} }})", lambda c: c["workspace"]["id"] == 9),
    ("hl.dsp.focus({ workspace = 9 })", lambda c: json.loads(hyprctl("-j", "activeworkspace"))["id"] == 9),
    ("hl.dsp.focus({ workspace = 1 })", lambda c: json.loads(hyprctl("-j", "activeworkspace"))["id"] == 1),
    (f"hl.dsp.window.move({{ workspace = 1, follow = false, {sel} }})", lambda c: c["workspace"]["id"] == 1),
    ("hl.dsp.cursor.move({ x = 700, y = 500 })", lambda c: json.loads(hyprctl("-j", "cursorpos")) == {"x": 700, "y": 500}),
]
for d, check in steps:
    r = hyprctl("dispatch", d).strip()
    time.sleep(0.4)
    out.append(r == "ok" and check(client("v7")))
w.destroy("v7")
errs = hyprctl("configerrors").strip()
line("V7", all(out) and errs in ("", "no errors"), f"{sum(out)}/{len(out)} dispatches; configerrors: {errs!r}")

# E2: margin moves warp with the capri- layer rule (without it they slide); negative margins are accepted,
# and a layer above the top is clipped, not clamped
def span(y, x0=380, n=660):
    p = subprocess.run(["grim", "-g", f"{x0},{y} {n}x1", "-t", "ppm", "-"], capture_output=True).stdout[-n * 3:]
    red = [i for i in range(n) if p[3 * i] > 200 and p[3 * i + 1] < 60 and p[3 * i + 2] < 60]
    return (x0 + red[0], x0 + red[-1]) if red else None


def at(ns):
    for mon in json.loads(hyprctl("-j", "layers")).values():
        for ls in mon["levels"].values():
            for l in ls:
                if l["namespace"] == ns:
                    return [l["x"], l["y"]]


res = {}
for ns in ("capri-e2", "e2-plain"):
    w.layer(ns, 400, 400, 120, 80, "top", 0, 0xff0000)
    time.sleep(1.0)  # past the map animation
    w.move(ns, 900, 400, 120, 80)
    time.sleep(0.03)
    res[ns] = span(440)
    w.destroy(ns)
w.layer("capri-e2", 900, 400, 120, 80, "top", 0, 0xff0000)
time.sleep(0.5)
w.move("capri-e2", 900, -40, 120, 80)
time.sleep(0.1)
neg = at("capri-e2")
clipped = span(10) == (900, 1019) and span(60) is None
w.destroy("capri-e2")
line("E2", res["capri-e2"] == (900, 1019) and neg == [900, -40] and clipped,
     f"30 ms after a 500 px margin move: capri- layer at x {res['capri-e2']}, a layer without the rule at x {res['e2-plain']} (sliding);"
     f" margin top -40 -> y {neg[1] if neg else None}, rows 0-39 shown and 40-79 not (clipped, not clamped): {clipped}")

# E3: the Line's keyboard modes under follow_mouse = 1 (a long wtype holds a keyboard on the seat meanwhile:
# with none, nobody gets keyboard focus)
kb = subprocess.Popen(["wtype", "-s", "20000", "x"])
time.sleep(0.5)
rule("e3-term", 200, 300, 900, 600)
w.toplevel("e3-term", 900, 600)
w.warp(600, 600)
time.sleep(0.3)
before = w.keyboard_focus()
w.layer("capri-e3", 1300, 400, 600, 120, "overlay", 0, 0x334455)
w.set_ki("capri-e3", 1)
excl = w.keyboard_focus()
w.set_ki("capri-e3", 2)
time.sleep(0.1)
after_switch = w.keyboard_focus()
w.set_ki("capri-e3", 1)
w.warp(610, 605)
time.sleep(0.2)
twitch_excl = w.keyboard_focus()
w.set_ki("capri-e3", 2)
w.warp(620, 600)
time.sleep(0.2)
twitch_od = w.keyboard_focus()
w.warp(820, 600)
time.sleep(0.2)
moved = w.keyboard_focus()
w.set_ki("capri-e3", 0)
none = w.keyboard_focus()
w.destroy("capri-e3")
w.destroy("e3-term")
kb.kill()
line("E3", excl == "capri-e3", f"before {before}; None->Exclusive: {excl}; Exclusive->OnDemand (pointer still): {after_switch}; "
     f"10 px twitch under Exclusive: {twitch_excl}; 10 px twitch under OnDemand: {twitch_od}; 200 px move: {moved}; ->None: {none}")
sys.exit(1 if fails else 0)
