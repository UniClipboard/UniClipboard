# Simulates an XWayland-bridged Chrome that supplies clipboard data LAZILY:
# owns CLIPBOARD immediately (one XFIXES notify), but only advertises a private
# target at first; text/plain becomes available DELAY_MS later, WITHOUT
# re-asserting ownership (so no new XFIXES). Reproduces the #1029 race.
import sys, time, threading
from Xlib import X, display, Xatom
from Xlib.protocol import event as Xevent

DELAY_MS = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
TEXT = b"http://example.com/lazy-chrome-url-AABBCC"

d = display.Display()
root = d.screen().root
win = root.create_window(0, 0, 1, 1, 0, d.screen().root_depth,
                         X.InputOutput, X.CopyFromParent,
                         event_mask=X.PropertyChangeMask)

def atom(n): return d.get_atom(n)
CLIPBOARD = atom("CLIPBOARD"); TARGETS = atom("TARGETS"); TIMESTAMP = atom("TIMESTAMP")
UTF8 = atom("UTF8_STRING"); TPLAIN = atom("text/plain;charset=utf-8")
PRIVATE = atom("chromium/x-source-url")

ready = [False]
win.set_selection_owner(CLIPBOARD, X.CurrentTime)
d.sync()
print(f"owner set; own_ok={d.get_selection_owner(CLIPBOARD)==win}; text/plain in {DELAY_MS}ms", flush=True)

def flip():
    time.sleep(DELAY_MS/1000.0)
    ready[0] = True
    print(f"[t={DELAY_MS}ms] text/plain NOW available (no new XFIXES)", flush=True)
threading.Thread(target=flip, daemon=True).start()

while True:
    e = d.next_event()
    if e.type != X.SelectionRequest:
        continue
    target = e.target
    prop = e.property if e.property != X.NONE else e.target
    req = e.requestor
    filled = False
    name = d.get_atom_name(target)
    if target == TARGETS:
        lst = [TIMESTAMP, TARGETS, PRIVATE]
        if ready[0]:
            lst += [UTF8, TPLAIN]
        req.change_property(prop, Xatom.ATOM, 32, lst)
        filled = True
    elif target in (UTF8, TPLAIN):
        if ready[0]:
            req.change_property(prop, target, 8, TEXT)
            filled = True
    elif target == TIMESTAMP:
        req.change_property(prop, Xatom.INTEGER, 32, [0]); filled = True
    print(f"  req target={name} ready={ready[0]} filled={filled}", flush=True)
    n = Xevent.SelectionNotify(time=e.time, requestor=req, selection=e.selection,
                               target=target, property=(prop if filled else X.NONE))
    req.send_event(n, propagate=False)
    d.flush()
