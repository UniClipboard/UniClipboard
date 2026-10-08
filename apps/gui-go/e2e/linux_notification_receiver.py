#!/usr/bin/env python3
"""Receive synthetic GUI notifications on the acceptance session bus."""

import json
import pathlib
import sys
import dbus
import dbus.service
import dbus.mainloop.glib
from gi.repository import GLib

out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=False)
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.Notifications", bus)


class Receiver(dbus.service.Object):
    @dbus.service.method(
        "org.freedesktop.Notifications", in_signature="", out_signature="as"
    )
    def GetCapabilities(self):
        return ["body"]

    @dbus.service.method(
        "org.freedesktop.Notifications", in_signature="", out_signature="ssss"
    )
    def GetServerInformation(self):
        return ("acceptance", "UniClipboard", "1", "1.2")

    @dbus.service.method(
        "org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u"
    )
    def Notify(self, app, replaces, icon, summary, body, actions, hints, expiry):
        with (out / "received.jsonl").open("a") as file:
            file.write(json.dumps({"summary": str(summary), "body": str(body)}) + "\n")
        return 1


service = Receiver(bus, "/org/freedesktop/Notifications")
(out / "ready").touch()
GLib.MainLoop().run()
