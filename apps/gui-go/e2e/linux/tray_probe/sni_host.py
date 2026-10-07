#!/usr/bin/env python3
"""Minimal StatusNotifier host for the tray checks: owns org.kde.StatusNotifierWatcher on the session bus, records the
registered item, reads its dbusmenu layout through com.canonical.dbusmenu and triggers items by label. Observation only,
through the standard Gio D-Bus API; it implements no tray of its own. Importable (SniHost) and runnable for the probe."""
import argparse, json, threading, time
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

XML = """<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<property name="ProtocolVersion" type="i" access="read"/>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
</interface></node>"""


def flat(n, out=None):
    out = [] if out is None else out
    out.append(n)
    for k in n["children"]:
        flat(k, out)
    return out


class SniHost:
    def __init__(self, log_path):
        self.t0 = time.time()
        self.log = open(log_path, "a", buffering=1)
        self.item = None  # (bus name, object path)
        self.registered = threading.Event()
        self.items_registered = 0
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        self.bus.set_exit_on_close(False)  # GDBus otherwise raises SIGTERM in this process when the bus goes away (seen in 17c14 nat2-nat4)
        node = Gio.DBusNodeInfo.new_for_xml(XML)
        self.bus.register_object("/StatusNotifierWatcher", node.interfaces[0], self._call, self._prop, None)
        Gio.bus_own_name_on_connection(self.bus, "org.kde.StatusNotifierWatcher", Gio.BusNameOwnerFlags.NONE, None, None)
        self.loop = GLib.MainLoop()
        self.thread = threading.Thread(target=self.loop.run, daemon=True)
        self.thread.start()
        self.emit("watcher-up")

    def emit(self, kind, **kw):
        self.log.write(json.dumps({"t": round(time.time() - self.t0, 2), "kind": kind, **kw}) + "\n")

    def _call(self, conn, sender, path, iface, method, params, inv):
        if method == "RegisterStatusNotifierItem":
            self.item = (sender, params.unpack()[0])
            self.items_registered += 1
            self.emit("registered", sender=sender, path=self.item[1])
            self.registered.set()
            inv.return_value(None)

    def _prop(self, conn, sender, path, iface, name):
        return {"IsStatusNotifierHostRegistered": GLib.Variant("b", True), "ProtocolVersion": GLib.Variant("i", 0),
                "RegisteredStatusNotifierItems": GLib.Variant("as", [])}[name]

    def _menu_call(self, method, args, sig):
        return self.bus.call_sync(self.item[0], "/StatusNotifierMenu", "com.canonical.dbusmenu", method,
                                  GLib.Variant(sig, args), None, Gio.DBusCallFlags.NONE, 3000, None).unpack()

    def layout(self):
        if not self.item:
            return None
        try:
            r = self._menu_call("GetLayout", (0, -1, []), "(iias)")
        except Exception as e:  # the item is gone or not answering
            return {"error": str(e)[:200]}

        def walk(n):
            i, props, kids = n
            return {"id": i, "label": props.get("label"), "type": props.get("type"), "enabled": props.get("enabled"),
                    "toggle": props.get("toggle-state"), "children": [walk(k) for k in kids]}
        return walk(r[1])

    def find(self, label, lay=None):
        lay = lay or self.layout()
        if not lay or "error" in lay:
            return None
        hit = [n for n in flat(lay) if n["label"] == label]
        return hit[0] if hit else None

    def click(self, label):
        node = self.find(label)
        if not node:
            return False
        self._menu_call("Event", (node["id"], "clicked", GLib.Variant("i", 0), int(time.time())), "(isvu)")
        self.emit("clicked", label=label, id=node["id"])
        return True

    def wait(self, predicate, timeout, what=""):
        """Poll the layout until predicate(layout) is truthy; log every distinct layout seen."""
        deadline, last = time.time() + timeout, None
        while time.time() < deadline:
            lay = self.layout()
            if lay is not None and lay != last:
                self.emit("layout", layout=lay)
                last = lay
            if lay and "error" not in lay and predicate(lay):
                return lay
            time.sleep(0.4)
        self.emit("wait-timeout", what=what)
        return None


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--log", required=True)
    p.add_argument("--click", action="append", default=[], help="LABEL@SECONDS")
    p.add_argument("--duration", type=float, default=20)
    a = p.parse_args()
    h = SniHost(a.log)
    clicks = [(c.rsplit("@", 1)[0], float(c.rsplit("@", 1)[1])) for c in a.click]
    done, last = set(), None
    while time.time() - h.t0 < a.duration:
        lay = h.layout()
        if lay is not None and lay != last:
            h.emit("layout", layout=lay)
            last = lay
        for label, at in clicks:
            if label not in done and time.time() - h.t0 >= at and h.find(label, lay):
                h.click(label)
                done.add(label)
        time.sleep(0.5)
    h.emit("host-exit")


if __name__ == "__main__":
    main()
