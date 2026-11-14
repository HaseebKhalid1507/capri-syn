#!/usr/bin/env python3
"""A fake MPRIS player on the session bus for synaps-avatar tests (never touches real players).
usage: fakempris.py NAME   then write commands to /tmp/sa-mpris.ctl (a FIFO): play | pause | stop | bpm N | quit"""
import os, sys, threading
import dbus, dbus.service, dbus.mainloop.glib
from gi.repository import GLib

NAME = "org.mpris.MediaPlayer2." + (sys.argv[1] if len(sys.argv) > 1 else "satest")
PATH = "/org/mpris/MediaPlayer2"
PLAYER = "org.mpris.MediaPlayer2.Player"
FIFO = "/tmp/sa-mpris.ctl"

class Player(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, PATH)
        self.status = "Stopped"
        self.meta = dbus.Dictionary({"mpris:trackid": dbus.ObjectPath("/satest/1"), "xesam:title": "synthetic"}, signature="sv")

    def props(self):
        return {"PlaybackStatus": self.status, "Metadata": self.meta, "CanPlay": True, "CanPause": True}

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return self.props()[prop]

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.props()

    @dbus.service.signal(dbus.PROPERTIES_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    def set(self, **kw):
        changed = {}
        if "status" in kw:
            self.status = kw["status"]; changed["PlaybackStatus"] = self.status
        if "bpm" in kw:
            self.meta["xesam:audioBPM"] = dbus.Int32(kw["bpm"]); changed["Metadata"] = self.meta
        self.PropertiesChanged(PLAYER, changed, [])
        print("fakempris:", kw, flush=True)

def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    p = Player(bus)
    name = dbus.service.BusName(NAME, bus)  # noqa: F841 (kept alive)
    loop = GLib.MainLoop()
    if os.path.exists(FIFO):
        os.unlink(FIFO)
    os.mkfifo(FIFO)
    def reader():
        while True:
            with open(FIFO) as f:
                for line in f:
                    c = line.split()
                    if not c:
                        continue
                    if c[0] == "quit":
                        GLib.idle_add(loop.quit); return
                    st = {"play": "Playing", "pause": "Paused", "stop": "Stopped"}.get(c[0])
                    if st:
                        GLib.idle_add(lambda st=st: p.set(status=st) and False)
                    elif c[0] == "bpm":
                        GLib.idle_add(lambda b=int(c[1]): p.set(bpm=b) and False)
    threading.Thread(target=reader, daemon=True).start()
    print("fakempris: up as", NAME, flush=True)
    loop.run()
    os.unlink(FIFO)

main()
