#!/usr/bin/env python3
"""A fake MPRIS player on the session bus for synaps-avatar tests (never touches real players).
usage: fakempris.py NAME   then write commands to the FIFO /tmp/sa-mpris.ctl: play | pause | stop | bpm N | quit"""
import os, sys, threading
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

NAME = "org.mpris.MediaPlayer2." + (sys.argv[1] if len(sys.argv) > 1 else "satest")
PATH = "/org/mpris/MediaPlayer2"
PLAYER = "org.mpris.MediaPlayer2.Player"
FIFO = "/tmp/sa-mpris.ctl"
XML = f"""<node>
  <interface name="org.mpris.MediaPlayer2"><property name="Identity" type="s" access="read"/></interface>
  <interface name="{PLAYER}">
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Metadata" type="a{{sv}}" access="read"/>
  </interface>
</node>"""

state = {"status": "Stopped", "bpm": None}

def metadata():
    m = {"mpris:trackid": GLib.Variant("o", "/satest/1"), "xesam:title": GLib.Variant("s", "synthetic")}
    if state["bpm"] is not None:
        m["xesam:audioBPM"] = GLib.Variant("i", state["bpm"])
    return GLib.Variant("a{sv}", m)

def get_prop(conn, sender, path, iface, prop):
    if prop == "PlaybackStatus":
        return GLib.Variant("s", state["status"])
    if prop == "Metadata":
        return metadata()
    if prop == "Identity":
        return GLib.Variant("s", "synaps-avatar test player")
    return None

def main():
    loop = GLib.MainLoop()
    conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    node = Gio.DBusNodeInfo.new_for_xml(XML)
    for iface in node.interfaces:
        conn.register_object(PATH, iface, None, get_prop, None)
    Gio.bus_own_name_on_connection(conn, NAME, Gio.BusNameOwnerFlags.NONE, None, None)

    def changed(props):
        conn.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged",
                         GLib.Variant("(sa{sv}as)", (PLAYER, props, [])))
        return False

    def apply(c):
        if c[0] in ("play", "pause", "stop"):
            state["status"] = {"play": "Playing", "pause": "Paused", "stop": "Stopped"}[c[0]]
            changed({"PlaybackStatus": GLib.Variant("s", state["status"])})
        elif c[0] == "bpm":
            state["bpm"] = int(c[1])
            changed({"Metadata": metadata()})
        print("fakempris:", " ".join(c), flush=True)
        return False

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
                        GLib.idle_add(loop.quit)
                        return
                    GLib.idle_add(apply, c)

    threading.Thread(target=reader, daemon=True).start()
    print("fakempris: up as", NAME, flush=True)
    loop.run()
    os.unlink(FIFO)

main()
