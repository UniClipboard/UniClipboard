#!/usr/bin/env python3
"""Run under an owned Xvfb; never connect to the user's graphical session."""
import json
import sys
import gi

gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
from gi.repository import Gtk, WebKit2, GLib

context = WebKit2.WebContext.new_ephemeral()
view = WebKit2.WebView.new_with_context(context)
view.get_settings().set_enable_developer_extras(True)


def finished(webview, result, _):
    value = webview.evaluate_javascript_finish(result).to_string()
    if value != 'isolated WebKitGTK':
        raise RuntimeError('native JavaScript read-back failed')
    print(json.dumps({'nativeRuntime': 'WebKitGTK', 'ephemeralStore': True,
                      'javascriptReadback': True, 'testerArmyAcceptance': False}), flush=True)
    Gtk.main_quit()


def loaded(webview, event):
    if event == WebKit2.LoadEvent.FINISHED:
        webview.evaluate_javascript("document.getElementById('probe').textContent", -1,
                                   None, None, None, finished, None)


view.connect('load-changed', loaded)
view.load_html("<p id='probe'>isolated WebKitGTK</p>", None)
GLib.timeout_add_seconds(30, lambda: sys.exit(2))
Gtk.main()
