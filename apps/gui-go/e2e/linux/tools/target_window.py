#!/usr/bin/env python3
"""A plain GTK3 window standing in for the application that had the focus before the quick panel (an ordinary X11 client)."""
import sys

import gi

gi.require_version('Gtk', '3.0')
from gi.repository import Gtk  # noqa: E402

window = Gtk.Window(title=sys.argv[1] if len(sys.argv) > 1 else 'uc-target')
window.set_default_size(400, 200)
window.add(Gtk.Entry())
window.connect('destroy', Gtk.main_quit)
window.show_all()
Gtk.main()
