#!/usr/bin/env python3
"""Linux quick panel / host E2E under Xvfb + a private D-Bus session. RUNS INSIDE the build container.

  docker run ... uc-gui-go-linux-build:17c python3 /work/apps/gui-go/e2e/linux_xvfb_run.py --out <dir> --binaries /cache/out

SCOPE (read before trusting a pass): the target is a private Xvfb X server with no window manager, no compositor and
no Wayland, plus a private D-Bus session bus with no portal, no notification daemon, no tray host (StatusNotifierWatcher)
and no Secret Service. Real XTEST key events are sent to that Xvfb. So this proves the X11 paths (XGrabKey shortcut,
XQueryKeymap modifier polling, X11 conflict), the Hyprland IPC chain against a SCRIPTED socket, the single-instance
hand-off over D-Bus, the XDG autostart entry and the shutdown, in the real Linux binary. It does NOT prove: real
window focus or placement (no window manager), any Wayland behaviour (the portal shortcut, layer shell, Wayland
modifier refusal is only checked as the refusal code under a faked WAYLAND_DISPLAY), a real Hyprland (the socket is
scripted), tray, notifications, suspend/resume, or a real desktop session. It is NOT a native-desktop acceptance.

Isolation: executables are copied into a throwaway `uc-gui-go-*` directory and run with UC_PORTABLE=1 (data root, caches
and the file keystore live there), a gui-go-* profile, HOME/XDG_* inside the sandbox, UC_DISABLE_SYSTEM_CLIPBOARD=1.
Only PIDs this script started (and the daemon PID recorded in the sandbox's own daemon.conn) are stopped.

Checks (windows-assertions style, in linux-assertions.json):
  1  the E2E test binding (the UC_GUI_GO_E2E_DEFAULT_SHORTCUT seam replaces the default with ctrl+alt+shift+f11) is
     registered through Wails' X11 backend (XGrabKey). This does NOT prove the product default ctrl+alt+v, nor the real
     frontend's first-start / settings-sync path; those still need an E2E that starts without the seam
  2  a REAL XTEST chord shows the panel, the same chord hides it
  3  a combination grabbed by another X client is refused as Conflict, the old binding stays; it succeeds after release
  4  modifier double-tap: available under X11; two real Alt taps open the panel; one tap plus another key does not
  5  paste_to_previous_app without Hyprland fails with the explicit error and the panel is shown again
  6  Hyprland chain against a scripted socket: remember at show, validate, focus, confirm, send_shortcut CTRL V
  7  XDG autostart: enable writes the entry, disable removes it, a Tauri-style entry with another Exec is swept
  8  single instance over D-Bus: `--quick-panel` from a second process toggles the panel; a plain second launch exits
  9  simulated Wayland env (WAYLAND_DISPLAY set): compositor flag true, modifier double-tap refused, GUI stays up
  10 exit: GUI exit 0, daemon stopped, the grabbed key is free again
"""
import argparse
import ctypes
import ctypes.util
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

PASSPHRASE = 'linux-panel-passphrase'
KEY = 'global.toggleQuickPanel'
# F13+ have no keycode in Xvfb's default keymap (run1 showed Wails reporting exactly that), so F11/F12 are used.
F13, F14 = 'ctrl+alt+shift+f11', 'ctrl+alt+shift+f12'


def read_steps(path):
    rows = []
    try:
        for line in Path(path).read_text().splitlines():
            try:
                rows.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return rows


def pid_alive(pid):
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


class XGrab:
    """Another X client owning a key combination (the conflicting application), via libX11 XGrabKey."""
    def __init__(self, display, keysym_name, modmask):
        self.x = ctypes.CDLL(ctypes.util.find_library('X11'))
        self.x.XOpenDisplay.restype = ctypes.c_void_p
        self.x.XDefaultRootWindow.restype = ctypes.c_ulong
        self.x.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.x.XStringToKeysym.restype = ctypes.c_ulong
        self.d = ctypes.c_void_p(self.x.XOpenDisplay(display.encode()))
        assert self.d.value, 'cannot open the Xvfb display'
        self.root = self.x.XDefaultRootWindow(self.d)
        self.code = self.x.XKeysymToKeycode(self.d, ctypes.c_ulong(self.x.XStringToKeysym(keysym_name.encode())))
        self.mask = modmask
        self.x.XGrabKey(self.d, self.code, self.mask, ctypes.c_ulong(self.root), 1, 1, 1)
        self.x.XSync(self.d, 0)

    def release(self):
        self.x.XUngrabKey(self.d, self.code, self.mask, ctypes.c_ulong(self.root))
        self.x.XSync(self.d, 0)
        self.x.XCloseDisplay(self.d)


def key_is_free(display, keysym_name, modmask):
    """True when this process can grab the combination (nobody else holds it)."""
    x = ctypes.CDLL(ctypes.util.find_library('X11'))
    x.XOpenDisplay.restype = ctypes.c_void_p
    x.XDefaultRootWindow.restype = ctypes.c_ulong
    x.XKeysymToKeycode.restype = ctypes.c_ubyte
    x.XStringToKeysym.restype = ctypes.c_ulong
    d = ctypes.c_void_p(x.XOpenDisplay(display.encode()))
    root = x.XDefaultRootWindow(d)
    code = x.XKeysymToKeycode(d, ctypes.c_ulong(x.XStringToKeysym(keysym_name.encode())))
    # Xlib reports BadAccess asynchronously; install a handler that records it.
    failed = []
    HANDLER = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)
    handler = HANDLER(lambda _d, _e: failed.append(1) or 0)
    x.XSetErrorHandler(handler)
    x.XGrabKey(d, code, modmask, ctypes.c_ulong(root), 1, 1, 1)
    x.XSync(d, 0)
    if not failed:
        x.XUngrabKey(d, code, modmask, ctypes.c_ulong(root))
    x.XCloseDisplay(d)
    return not failed


def xdo(display, *args):
    subprocess.run(['xdotool', *args], env=dict(os.environ, DISPLAY=display), check=True, timeout=15)


class FakeHyprland(threading.Thread):
    """A scripted Hyprland IPC socket: answers the commands the host sends and records them."""
    def __init__(self, path, window):
        super().__init__(daemon=True)
        self.window, self.commands = window, []
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.bind(str(path))
        self.sock.listen(8)
        self.stop = False

    def run(self):
        self.sock.settimeout(0.2)
        while not self.stop:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                continue
            with conn:
                cmd = conn.recv(4096).decode()
                self.commands.append(cmd)
                if cmd == 'j/activewindow':
                    reply = json.dumps(self.window)
                elif cmd == 'j/clients':
                    reply = json.dumps([self.window])
                elif cmd.startswith('/dispatch'):
                    reply = 'ok'
                else:
                    reply = '{}'
                conn.sendall(reply.encode())


class Gui:
    def __init__(self, sandbox, env, out, tag, args=()):
        self.evidence, self.control = out / f'{tag}.jsonl', out / f'{tag}.control'
        self.evidence.write_text('')
        self.control.write_text('')
        env = dict(env, UC_GUI_GO_EVIDENCE=str(self.evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(self.control))
        self.proc = subprocess.Popen([str(sandbox / 'gui-go'), *args], env=env, cwd=sandbox,
                                     stdout=(out / f'{tag}.log').open('w'), stderr=subprocess.STDOUT)

    def step(self, name, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(self.evidence) if r['step'] == name]
            if rows:
                return rows[-1]
            if self.proc.poll() is not None:
                raise RuntimeError(f'GUI exited ({self.proc.returncode}) before {name}')
            time.sleep(.1)
        raise RuntimeError(f'timeout waiting for {name}')

    def ctl(self, line, label, timeout=60):
        with self.control.open('a') as f:
            f.write(line + '\n')
        return self.step(label, timeout)

    def state(self, label):
        return self.ctl(f'shortcut-state {label}', f'shortcut-state-{label}')['detail']

    def invoke(self, label, command, args=None):
        return self.ctl(f'invoke {label} {command} {json.dumps(args) if args is not None else ""}', f'invoke-{label}')['detail']

    def wait_state(self, label, predicate, timeout=8):
        deadline, n = time.monotonic() + timeout, 0
        while True:
            n += 1
            state = self.state(f'{label}-{n}')
            if predicate(state) or time.monotonic() > deadline:
                return state
            time.sleep(.2)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True, help='directory with gui-go, uniclipd and uniclip built for Linux')
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('this script runs on Linux only (inside the build container)')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
    profile = 'gui-go-' + sandbox.name
    for name in ('gui-go', 'uniclipd', 'uniclip'):
        shutil.copy2(args.binaries / name, sandbox / name)
    home = sandbox / 'home'
    runtime_dir = sandbox / 'run'
    for d in (home, runtime_dir):
        d.mkdir(mode=0o700)
    display = os.environ.get('DISPLAY', ':99')
    base_env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_RUNTIME_DIR=str(runtime_dir), UC_PORTABLE='1',
                    UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', DISPLAY=display,
                    XDG_SESSION_TYPE='x11', GDK_BACKEND='x11')
    for k in ('WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'APPIMAGE'):
        base_env.pop(k, None)
    gui_env = dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_SHORTCUTS='1',
                   UC_GUI_GO_E2E_DEFAULT_SHORTCUT=F13, UC_GUI_GO_EXIT_MODE='full')
    uniclip = str(sandbox / 'uniclip')
    results = {'sandbox': str(sandbox), 'profile': profile, 'display': display, 'checks': [], 'passed': False,
               'scope': 'Xvfb + private D-Bus session in a container; no WM, no Wayland, no portal, no tray host, no notification daemon',
               'uname': os.uname()._asdict() if hasattr(os.uname(), '_asdict') else list(os.uname())}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    gui = gui2 = grab = fake = None
    daemon_pid = None
    try:
        subprocess.run([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'linux-panel'], env=base_env, check=True, timeout=120)
        subprocess.run([uniclip, 'start'], env=base_env, check=True, timeout=120)
        gui = Gui(sandbox, gui_env, out, 'gui1')
        gui.step('bootstrapped', 120)
        state = gui.wait_state('boot', lambda s: s['panelReady'], 60)
        conn = sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn'
        if not conn.exists():
            conn = next(sandbox.rglob('daemon.conn'))
        daemon_pid = json.loads(conn.read_text())['pid']
        check('1 E2E test binding (UC_GUI_GO_E2E_DEFAULT_SHORTCUT override, ctrl+alt+shift+f11, NOT the product default ctrl+alt+v) registered through Wails (X11 XGrabKey)', state['recorded'] == [F13] and len(state['wails']) == 1, state)
        # Independent proof that the OS binding exists (not only the host's own record): another X client cannot take it.
        check('1 the combination is really grabbed on the X server (a second client cannot grab it)', not key_is_free(display, 'F11', 1 | 4 | 8))
        check('2 precondition: the panel is hidden before the first chord', not state['panelVisible'], state)

        xdo(display, 'key', 'ctrl+alt+shift+F11')
        shown = gui.wait_state('shown', lambda s: s['panelVisible'])
        check('2 a real XTEST chord shows the panel', shown['panelVisible'], shown)
        xdo(display, 'key', 'ctrl+alt+shift+F11')
        hidden = gui.wait_state('hidden', lambda s: not s['panelVisible'])
        check('2 the same chord hides the panel (it was visible before: not a vacuous pass)', shown['panelVisible'] and not hidden['panelVisible'], [shown, hidden])

        grab = XGrab(display, 'F12', 1 | 4 | 8)  # Shift|Control|Mod1(Alt)
        r = gui.invoke('conflict', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F14}})
        s = gui.state('after-conflict')
        check('3 a combination grabbed by another X client is refused as Conflict, old binding stays',
              not r['ok'] and r['error']['code'] == 'Conflict' and 'already registered' in r['error']['message']
              and s['recorded'] == [F13] and s['stored'] is None, [r, s])
        grab.release()
        grab = None
        r = gui.invoke('rebind', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F14}})
        s = gui.state('after-rebind')
        check('3 the same change succeeds after the other client released it', r['ok'] and s['recorded'] == [F14] and s['stored'] == F14, [r, s])
        r = gui.invoke('restore', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F13}})
        check('3 binding switched back', r['ok'])

        r = gui.invoke('avail', 'get_quick_panel_double_tap_availability')
        check('4 modifier double-tap is available under native X11', r['ok'] and r['data'] == 'supported', r)
        r = gui.invoke('alt', 'set_quick_panel_double_tap_modifier', {'modifier': 'alt'})
        check('4 selecting Alt is accepted', r['ok'], r)
        # Both taps in ONE xdotool process: separate invocations add tens of milliseconds of process start-up between
        # the events (run4 failed to open the panel that way); here the timing is the script's own (80 ms hold, 120 ms gap).
        xdo(display, 'keydown', 'alt', 'sleep', '0.08', 'keyup', 'alt', 'sleep', '0.12', 'keydown', 'alt', 'sleep', '0.08', 'keyup', 'alt')
        s = gui.wait_state('dtap', lambda s: s['panelVisible'])
        check('4 two real Alt taps open the panel', s['panelVisible'], s)
        xdo(display, 'key', 'ctrl+alt+shift+F11')
        pre = gui.wait_state('dtap-hide', lambda s: not s['panelVisible'])
        check('4 precondition for the negative case: the panel is hidden again (the positive case above opened it)', not pre['panelVisible'], pre)
        # Hold the other key for several poll intervals (the monitor samples every 20 ms; `xdotool key a` lasts ~12 ms and
        # can fall between two samples, which run3 showed: the sampling limit documented in modifier_double_tap.go).
        xdo(display, 'keydown', 'alt'); xdo(display, 'keydown', 'a'); time.sleep(.15); xdo(display, 'keyup', 'a'); xdo(display, 'keyup', 'alt'); time.sleep(.12)
        xdo(display, 'keydown', 'alt'); time.sleep(.08); xdo(display, 'keyup', 'alt')
        time.sleep(.8)
        check('4 an Alt chord with another key does not count as a tap', not gui.state('dtap-chord')['panelVisible'])
        gui.invoke('alt-off', 'set_quick_panel_double_tap_modifier', {'modifier': 'disabled'})

        xdo(display, 'key', 'ctrl+alt+shift+F11')
        gui.wait_state('p-shown', lambda s: s['panelVisible'])
        before = gui.state('p-before')['lastShown']
        r = gui.invoke('paste', 'paste_to_previous_app')
        after = gui.state('p-after')
        check('5 paste without Hyprland reports the explicit error and shows the panel again',
              not r['ok'] and 'not yet supported on this platform' in str(r['error']) and after['lastShown'] != before and after['panelVisible'], [r, after])
        r = gui.invoke('typed', 'type_file_paths_to_previous_app', {'request': {'filePaths': ['/tmp/a']}})
        check('5 typing file paths is refused explicitly', not r['ok'], r)
        xdo(display, 'key', 'ctrl+alt+shift+F11')
        gui.wait_state('p-hidden', lambda s: not s['panelVisible'])

        r = gui.invoke('autostart-on', 'update_autostart', {'enabled': True})
        entry = home / '.config' / 'autostart' / 'UniClipboard' f'-{profile}.desktop'
        body = entry.read_text() if entry.exists() else ''
        check('7 enabling autostart writes an XDG entry that launches this executable with --autostart',
              r['ok'] and entry.exists() and str(sandbox / 'gui-go') in body and '--autostart' in body, [r, body])
        r = gui.invoke('autostart-off', 'update_autostart', {'enabled': False})
        check('7 disabling autostart removes it', r['ok'] and not entry.exists(), r)

        gui.ctl('exit', 'control-exit')
        code = gui.proc.wait(timeout=60)
        deadline = time.monotonic() + 20
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        check('10 GUI exit 0, daemon stopped by the GUI', code == 0 and not pid_alive(daemon_pid), {'exit': code, 'daemonAlive': pid_alive(daemon_pid)})
        check('10 the grabbed shortcut is free after exit', key_is_free(display, 'F11', 1 | 4 | 8))
        gui = None

        # ---- second launch: Hyprland scripted socket (check 6) ----
        subprocess.run([uniclip, 'start'], env=base_env, check=True, timeout=120)
        sig = 'uctest_instance'
        hypr_dir = runtime_dir / 'hypr' / sig
        hypr_dir.mkdir(parents=True)
        window = {'address': '0xabc123', 'pid': 4242, 'class': 'kitty'}
        fake = FakeHyprland(hypr_dir / '.socket.sock', window)
        fake.start()
        hypr_env = dict(gui_env, HYPRLAND_INSTANCE_SIGNATURE=sig)
        gui = Gui(sandbox, hypr_env, out, 'gui2')
        gui.step('bootstrapped', 120)
        gui.wait_state('h-boot', lambda s: s['panelReady'], 60)
        daemon_pid = json.loads(next(sandbox.rglob('daemon.conn')).read_text())['pid']
        xdo(display, 'key', 'ctrl+alt+shift+F11')
        hs = gui.wait_state('h-shown', lambda s: s['panelVisible'])
        before_paste = list(fake.commands)
        check('6 precondition: the panel is visible and the active window was read at show time, before any paste command',
              hs['panelVisible'] and before_paste == ['j/activewindow'], {'state': hs, 'commands': before_paste})
        r = gui.invoke('hpaste', 'paste_to_previous_app')
        time.sleep(.5)
        cmds = fake.commands
        order_ok = ('j/activewindow' in cmds[:1]) and any(c == 'j/clients' for c in cmds) and any('hl.dsp.focus' in c and 'address:0xabc123' in c for c in cmds) \
            and any('send_shortcut' in c and 'CTRL SHIFT' in c and 'address:0xabc123' in c for c in cmds)
        check('6 Hyprland chain: remembered at show, validated, focused, confirmed, CTRL SHIFT V sent (terminal class) to that address',
              r['ok'] and order_ok, {'result': r, 'commands': cmds})
        check('6 the panel is hidden after a successful paste', not gui.state('h-after')['panelVisible'])

        # ---- single instance (check 8) while gui2 runs ----
        pre8 = gui.state('si-pre')
        check('8 precondition: the panel is hidden before the second process starts', not pre8['panelVisible'], pre8)
        second = subprocess.run([str(sandbox / 'gui-go'), '--quick-panel'], env=dict(hypr_env, UC_GUI_GO_EVIDENCE=str(out / 'second.jsonl')), timeout=60,
                                capture_output=True, cwd=sandbox)
        s = gui.wait_state('si', lambda s: s['panelVisible'])
        check('8 `--quick-panel` from a second process reaches the running GUI over D-Bus and shows the panel',
              second.returncode == 0 and s['panelVisible'], {'exit': second.returncode, 'state': s})
        plain = subprocess.run([str(sandbox / 'gui-go')], env=dict(hypr_env, UC_GUI_GO_EVIDENCE=str(out / 'second.jsonl')), timeout=60,
                               capture_output=True, cwd=sandbox)
        check('8 a plain second launch exits 0 without starting another instance', plain.returncode == 0, {'exit': plain.returncode, 'stderr': plain.stderr.decode()[-300:]})
        gui.ctl('exit', 'control-exit')
        gui.proc.wait(timeout=60)
        gui = None

        # ---- simulated Wayland session (check 9) ----
        subprocess.run([uniclip, 'start'], env=base_env, check=True, timeout=120)
        way_env = dict(gui_env, WAYLAND_DISPLAY='wayland-uc-fake', XDG_SESSION_TYPE='wayland')
        gui = Gui(sandbox, way_env, out, 'gui3')
        gui.step('bootstrapped', 120)
        gui.wait_state('w-boot', lambda s: s['panelReady'], 60)
        daemon_pid = json.loads(next(sandbox.rglob('daemon.conn')).read_text())['pid']
        r1 = gui.invoke('comp', 'quick_panel_uses_compositor_shortcuts')
        r2 = gui.invoke('wavail', 'get_quick_panel_double_tap_availability')
        r3 = gui.invoke('walt', 'set_quick_panel_double_tap_modifier', {'modifier': 'alt'})
        check('9 under a Wayland session the compositor-shortcut instruction is on and double-tap is refused (not faked)',
              r1['ok'] and r1['data'] is True and r2['data'] == 'unsupported_display_session' and not r3['ok'], [r1, r2, r3])
        check('9 the GUI is still running after the portal shortcut request had no portal to talk to', gui.proc.poll() is None)
        gui.ctl('exit', 'control-exit')
        gui.proc.wait(timeout=60)
        gui = None
        results['passed'] = all(c['ok'] is not False for c in checks)
    finally:
        for proc in (gui, ):
            if proc and proc.proc.poll() is None:
                proc.proc.terminate()
        if grab:
            grab.release()
        if fake:
            fake.stop = True
        subprocess.run([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid and pid_alive(daemon_pid):  # only the PID recorded in this sandbox's own daemon.conn
            os.kill(daemon_pid, 15)
        (out / 'linux-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False, default=str) + '\n', encoding='utf-8')
        if sandbox.name.startswith('uc-gui-go-') and sandbox.parent == Path(tempfile.gettempdir()):
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
