import gi, sys, time
gi.require_version("Gtk","3.0")
from gi.repository import Gtk, Gdk, GLib
texts=sys.argv[2:]; i=[0]
w=Gtk.Window(title="t0221-synthetic-clipboard-source"); w.set_default_size(420,160); w.set_keep_above(True)
b=Gtk.Button(label="copy synthetic text (click)")
def click(_):
    t=texts[i[0]%len(texts)]; i[0]+=1
    Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).set_text(t,-1); print(round(time.time(),3),"set",t,flush=True)
b.connect("clicked", click); w.add(b); w.show_all(); w.present(); print(round(time.time(),3), "ready", flush=True)
GLib.timeout_add_seconds(int(sys.argv[1]), Gtk.main_quit); Gtk.main()
