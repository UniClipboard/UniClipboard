#!/usr/bin/env python3
"""A plain xdg-toplevel that logs keyboard focus changes and key presses: the "other application" of the keyboard-
exclusivity check. Appends JSON lines to the file given as argv[1]. Runs against the compositor in WAYLAND_DISPLAY."""
import json
import sys
import time

import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk

log = open(sys.argv[1], 'a', buffering=1)


def emit(event, **extra):
    log.write(json.dumps(dict(event=event, t=time.monotonic(), **extra)) + '\n')


window = Gtk.Window(title='uc-focus-probe')
window.set_default_size(300, 200)
entry = Gtk.Entry()
window.add(entry)
window.connect('destroy', Gtk.main_quit)
window.connect('focus-in-event', lambda *_: emit('focus-in'))
window.connect('focus-out-event', lambda *_: emit('focus-out'))
window.connect('key-press-event', lambda _w, e: emit('key', keyval=e.keyval))
window.show_all()
emit('ready')
Gtk.main()
