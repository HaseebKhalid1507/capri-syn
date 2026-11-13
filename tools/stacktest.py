#!/usr/bin/env python3
"""stacktest.py: map an i3-managed floating dialog over the avatar, let i3 focus/raise it,
then report the X stacking order of the avatar vs the dialog's frame. Cleans up and
gives focus back to whatever had it."""
import subprocess, sys, time
from Xlib import display, X, Xatom
d = display.Display(); s = d.screen(); root = s.root
prev = d.get_input_focus().focus
prev_id = prev.id if hasattr(prev, "id") else 0
x, y = int(sys.argv[1]), int(sys.argv[2])
win = root.create_window(x, y, 360, 240, 0, s.root_depth, X.InputOutput, X.CopyFromParent,
                         background_pixel=0x553322, event_mask=X.StructureNotifyMask)
win.set_wm_name("stacktest-dialog"); win.set_wm_class("stacktest", "stacktest")
win.change_property(d.intern_atom("_NET_WM_WINDOW_TYPE"), Xatom.ATOM, 32, [d.intern_atom("_NET_WM_WINDOW_TYPE_DIALOG")])
win.set_wm_normal_hints(flags=1 | 4, x=x, y=y, min_width=360, min_height=240)  # USPosition|PPosition
win.map(); d.sync()
time.sleep(1.0)
subprocess.run(["i3-msg", "-q", f"[id={win.id}] focus"])
time.sleep(0.8)

def top_of(w):
    while True:
        t = w.query_tree()
        if t.parent.id == root.id:
            return w
        w = t.parent

frame = top_of(win)
kids = [k.id for k in root.query_tree().children]
av = [k for k in kids if (lambda c: c and c[0] == "synaps-avatar")(d.create_resource_object("window", k).get_wm_class())]
print(f"dialog 0x{win.id:x} frame 0x{frame.id:x} at stack index {kids.index(frame.id)}; avatar {[hex(a) for a in av]} at {[kids.index(a) for a in av]} of {len(kids)}")
print("focused:", hex(d.get_input_focus().focus.id))
print("RESULT:", "avatar ABOVE the focused floating dialog" if av and kids.index(av[0]) > kids.index(frame.id) else "avatar BELOW the dialog")
win.destroy(); d.sync()
time.sleep(0.2)
if prev_id:
    subprocess.run(["i3-msg", "-q", f"[id={prev_id}] focus"])
print("restored focus to", hex(prev_id))
