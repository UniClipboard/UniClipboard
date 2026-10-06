#!/usr/bin/env python3
"""Layer Shell quick panel E2E (L5/L6) against a REAL headless wlroots compositor (sway 1.9). RUNS INSIDE the container
(image uc-gui-go-linux-build:17c2):

  docker run ... python3 /work/apps/gui-go/e2e/linux_wayland_run.py --out <dir> --binaries /cache/out [--no-layer-library]

SCOPE (read before trusting a pass): sway with the headless backend and the pixman renderer implements the real
wlr-layer-shell protocol, real xdg-shell, real seat/keyboard focus and a virtual-pointer/virtual-keyboard input path, so
protocol role, layer, keyboard mode, per-output surfaces, pointer dismissal and per-output geometry are observed on the
compositor side (its debug log, wayland-info, screenshots through grim, a separate xdg toplevel that logs focus). It does
NOT prove: Hyprland itself (cursor position and the active window come from a SCRIPTED Hyprland IPC socket whose cursor
the driver keeps equal to where it parks the sway pointer), GNOME (no layer-shell), KDE, a real GPU, a real desktop
input stack, or a portal. It is not a native-desktop acceptance. Nothing here touches a host desktop: the compositor, the
clients and the sockets all live in this container's throwaway directory.

Isolation: like linux_xvfb_run.py (UC_PORTABLE sandbox, gui-go-* profile, HOME/XDG_* in the sandbox, system clipboard
disabled, PIDs started here are the only ones stopped).

Checks are written to wayland-assertions.json; every failure is kept with its detail.
"""
import argparse
import glob
import json
import os
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linux_xvfb_run import FakeHyprland, Gui, pid_alive, read_steps  # noqa: E402

PASSPHRASE = 'linux-panel-passphrase'
PAD = 16  # the panel window's transparent padding around its content, in logical px (panelWindowPadding)
CAP_W, CAP_H = 0.9, 0.8
BASE_W, BASE_H, GAP = 800.0, 560.0, 6.0
OUT1 = dict(name='HEADLESS-1', x=0, y=0, w=1280, h=800, scale=1)
OUT2 = dict(name='HEADLESS-2', x=1280, y=0, w=800, h=500, scale=2)  # mode 1600x1000 at scale 2
LAYOUT_W, LAYOUT_H = 2080, 800


class FakeHypr(FakeHyprland):
    """Scripted Hyprland socket with a settable cursor; commands are recorded with their arrival time."""
    cursor = (0.0, 0.0)

    def run(self):
        self.sock.settimeout(0.2)
        self.times = []
        while not self.stop:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                continue
            with conn:
                cmd = conn.recv(4096).decode()
                self.commands.append(cmd)
                self.times.append(time.monotonic())
                if cmd == 'j/cursorpos':
                    reply = json.dumps({'x': self.cursor[0], 'y': self.cursor[1]})
                elif cmd == 'j/activewindow':
                    reply = json.dumps(self.window)
                elif cmd == 'j/clients':
                    reply = json.dumps([self.window])
                elif cmd.startswith('/dispatch'):
                    reply = 'ok'
                else:
                    reply = '{}'
                conn.sendall(reply.encode())


def last_focus(events):
    focus = [e['event'] for e in events if e['event'] in ('focus-in', 'focus-out')]
    return focus[-1] if focus else None


def axis(cursor, origin, extent, panel):
    """The Tauri axis rule, written again here (not imported) as the independent expectation."""
    end = origin + extent
    if cursor + GAP + panel <= end:
        return cursor + GAP
    if cursor - GAP - panel >= origin:
        return cursor - GAP - panel
    return max(end - panel, origin)


def expected_rect(out, cursor, base=(BASE_W, BASE_H), follow=True):
    w = max(int(min(base[0], out['w'] * CAP_W)), 1)
    h = max(int(min(base[1], out['h'] * CAP_H)), 1)
    if follow and cursor:
        x = axis(cursor[0] - out['x'], 0, out['w'], w)
        y = axis(cursor[1] - out['y'], 0, out['h'], h)
    else:
        x, y = (out['w'] - w) / 2, (out['h'] - h) / 2
    return (out['x'] + round(x), out['y'] + round(y), w, h)


def read_ppm(path):
    data = Path(path).read_bytes()
    parts = data.split(b'\n', 3)
    assert parts[0] == b'P6', 'not a PPM'
    width, height = map(int, parts[1].split())
    return width, height, parts[3]


def diff_bbox(before, after, scale):
    """Bounding box (logical px, output-local) of the pixels that differ between two screenshots of one output."""
    w, h, a = read_ppm(before)
    w2, h2, b = read_ppm(after)
    assert (w, h) == (w2, h2)
    x0 = y0 = 10**9
    x1 = y1 = -1
    for y in range(h):
        row = slice(y * w * 3, (y + 1) * w * 3)
        ra, rb = a[row], b[row]
        if ra == rb:
            continue
        for x in range(w):
            if ra[x * 3:x * 3 + 3] != rb[x * 3:x * 3 + 3]:
                x0, x1 = min(x0, x), max(x1, x)
                y0, y1 = min(y0, y), max(y1, y)
    if x1 < 0:
        return None
    return (x0 / scale, y0 / scale, (x1 + 1 - x0) / scale, (y1 + 1 - y0) / scale)


class Beacons:
    """Loopback listener the injected page script reports to (key/focus events seen by the panel page itself)."""
    def __init__(self):
        import http.server
        import threading
        self.events = []
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                outer.events.append({'t': time.monotonic(), 'path': self.path})
                self.send_response(204)
                self.send_header('Access-Control-Allow-Origin', '*')
                self.end_headers()

            def log_message(self, *a):
                pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def script(self):
        return ("(function(){var p=%d;function s(t){try{fetch('http://127.0.0.1:'+p+'/'+t,{mode:'no-cors'})}catch(e){}}"
                "function st(){return 'focus='+document.hasFocus()+'/vis='+document.visibilityState+'/active='+(document.activeElement?document.activeElement.tagName:'none')}"
                "window.addEventListener('keydown',function(e){s('keydown/'+encodeURIComponent(e.key)+'/'+st())},true);"
                "window.addEventListener('focus',function(){s('window-focus/'+st())});window.addEventListener('blur',function(){s('window-blur/'+st())});"
                "window.addEventListener('mousedown',function(e){s('mousedown/'+Math.round(e.clientX)+'/'+Math.round(e.clientY)+'/'+st())},true);document.addEventListener('visibilitychange',function(){s('visibility/'+st())});s('installed/'+st())})()") % self.port

    def seen(self, prefix, since=0.0):
        return [e['path'] for e in self.events if e['t'] >= since and e['path'].lstrip('/').startswith(prefix)]


class Sway:
    def __init__(self, runtime, out, tag='sway'):
        self.runtime, self.out = runtime, out
        cfg = runtime / 'sway.conf'
        cfg.write_text('default_border none\nfont pango:monospace 8\n'
                       f'output {OUT1["name"]} mode {OUT1["w"]}x{OUT1["h"]} position 0 0 scale 1 bg #303030 solid_color\n')
        env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), WLR_BACKENDS='headless', WLR_LIBINPUT_NO_DEVICES='1',
                   WLR_RENDERER='pixman', XDG_SESSION_TYPE='wayland')
        for k in ('DISPLAY', 'WAYLAND_DISPLAY', 'SWAYSOCK', 'HYPRLAND_INSTANCE_SIGNATURE'):
            env.pop(k, None)
        self.log = (out / f'{tag}.log').open('w')
        self.proc = subprocess.Popen(['sway', '-d', '-c', str(cfg)], env=env, stdout=self.log, stderr=subprocess.STDOUT, cwd=runtime)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            socks = glob.glob(str(runtime / 'sway-ipc.*.sock'))
            if socks and (runtime / 'wayland-1').exists():
                self.sock = socks[0]
                break
            if self.proc.poll() is not None:
                raise RuntimeError('sway exited early')
            time.sleep(.2)
        else:
            raise RuntimeError('sway did not come up')
        self.env = dict(env, WAYLAND_DISPLAY='wayland-1', SWAYSOCK=self.sock)
        self.env.pop('WLR_BACKENDS')

    def msg(self, *args, parse=False):
        r = subprocess.run(['swaymsg', '-s', self.sock, *(['-t'] if parse else []), *args], capture_output=True, text=True, timeout=20)
        if r.returncode != 0:
            raise RuntimeError(f'swaymsg {args}: {r.stdout} {r.stderr}')
        return json.loads(r.stdout) if parse else r.stdout

    def log_text(self):
        return (self.out / 'sway.log').read_text(errors='replace')

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def build_tools(sandbox, out):
    """Compile the virtual pointer helper from the vendored wlr protocol XML (source and hash in e2e/linux/protocols)."""
    here = Path(__file__).resolve().parent / 'linux'
    build = sandbox / 'tools'
    build.mkdir()
    xml = here / 'protocols' / 'wlr-virtual-pointer-unstable-v1.xml'
    for kind, name in (('client-header', 'wlr-virtual-pointer-unstable-v1-client-protocol.h'), ('private-code', 'wlr-virtual-pointer-unstable-v1-protocol.c')):
        subprocess.run(['wayland-scanner', kind, str(xml), str(build / name)], check=True)
    subprocess.run(['gcc', '-O1', '-o', str(build / 'vpointer'), str(here / 'tools' / 'vpointer.c'), str(build / 'wlr-virtual-pointer-unstable-v1-protocol.c'),
                    f'-I{build}', '-lwayland-client'], check=True, stdout=(out / 'tools-build.log').open('w'), stderr=subprocess.STDOUT)
    return build / 'vpointer', here / 'tools' / 'focus_probe.py'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--wayland-debug', action='store_true', help='WAYLAND_DEBUG=client for the GUI (large log): shows which wl_keyboard/wl_pointer events the client really received')
    parser.add_argument('--no-layer-library', action='store_true',
                        help='scenario for a container where libgtk-layer-shell is really not installed: only the fallback checks run')
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('this script runs on Linux only (inside the build container)')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
    profile = 'gui-go-' + sandbox.name
    for name in ('gui-go', 'uniclipd', 'uniclip'):
        shutil.copy2(args.binaries / name, sandbox / name)
    home, runtime = sandbox / 'home', sandbox / 'run'
    for d in (home, runtime):
        d.mkdir(mode=0o700)
    results = {'sandbox': str(sandbox), 'profile': profile, 'checks': [], 'passed': False, 'noLayerLibrary': args.no_layer_library,
               'scope': 'sway 1.9 headless (wlroots, pixman) in a container; Hyprland IPC scripted; no GNOME/KDE/real GPU/portal',
               'uname': list(os.uname())}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    sway = gui = fake = probe = vp = keeper = None
    daemon_pid = None
    base_env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_RUNTIME_DIR=str(runtime), UC_PORTABLE='1', UC_PROFILE=profile,
                    UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    for k in ('DISPLAY', 'WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'APPIMAGE'):
        base_env.pop(k, None)
    uniclip = str(sandbox / 'uniclip')
    try:
        vpointer, focus_probe = build_tools(sandbox, out)
        sway = Sway(runtime, out)
        sway.msg('create_output')
        sway.msg('output', OUT2['name'], 'mode', f'{OUT2["w"] * OUT2["scale"]}x{OUT2["h"] * OUT2["scale"]}', 'position', str(OUT2['x']), '0',
                 'scale', str(OUT2['scale']), 'bg', '#303030', 'solid_color')
        outputs = {o['name']: o for o in sway.msg('get_outputs', parse=True)}
        rects = {n: (o['rect']['x'], o['rect']['y'], o['rect']['width'], o['rect']['height'], o['scale']) for n, o in outputs.items()}
        check('compositor outputs: two outputs with different size/scale/position (logical rects as the compositor reports them)',
              rects.get(OUT1['name']) == (0, 0, 1280, 800, 1.0) and rects.get(OUT2['name']) == (1280, 0, 800, 500, 2.0), rects)
        info = subprocess.run(['wayland-info'], env=sway.env, capture_output=True, text=True, timeout=20).stdout
        (out / 'wayland-info.txt').write_text(info)
        check('W1 the compositor offers zwlr_layer_shell_v1 (wayland-info, protocol global)', 'zwlr_layer_shell_v1' in info)

        vp = subprocess.Popen([str(vpointer)], env=sway.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=(out / 'vpointer.err').open('w'), text=True)

        def vp_reply(what, timeout=10):
            """One line from the virtual pointer helper within a deadline; its exit status is reported, never waited on blindly."""
            ready, _, _ = select.select([vp.stdout], [], [], timeout)
            if not ready:
                raise RuntimeError(f'virtual pointer helper did not answer {what!r} within {timeout}s (poll={vp.poll()})')
            line = vp.stdout.readline()
            if not line:
                raise RuntimeError(f'virtual pointer helper closed its output after {what!r} (exit={vp.wait(timeout=5)})')
            return line.strip()

        assert vp_reply('startup', 20) == 'ready', 'virtual pointer did not start'

        def pointer_cmd(line):
            vp.stdin.write(line + '\n')
            vp.stdin.flush()
            answer = vp_reply(line)
            assert answer == 'ok', f'virtual pointer: {answer!r} for {line!r}'

        pointer_cmd(f'size {LAYOUT_W} {LAYOUT_H}')
        # A long-lived virtual keyboard (holding an unused key) so the seat advertises the keyboard capability and keyboard
        # focus is delivered to whichever client gets it; wtype calls later create their own short-lived devices.
        keeper = subprocess.Popen(['wtype', '-s', '3000000', '-P', 'F24'], env=sway.env, stdout=subprocess.DEVNULL, stderr=(out / 'wtype-keeper.err').open('w'))
        time.sleep(1)
        gui_env = dict(base_env, **{k: v for k, v in sway.env.items() if k in ('WAYLAND_DISPLAY', 'SWAYSOCK', 'XDG_SESSION_TYPE')},
                       GDK_BACKEND='wayland', UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake',
                       UC_GUI_GO_E2E_SHORTCUTS='1', UC_GUI_GO_E2E_DEFAULT_SHORTCUT='ctrl+alt+shift+f11', UC_GUI_GO_EXIT_MODE='full')
        sig = 'uctest_instance'
        hypr_dir = runtime / 'hypr' / sig
        hypr_dir.mkdir(parents=True)
        window = {'address': '0xabc123', 'pid': 4242, 'class': 'kitty'}
        fake = FakeHypr(hypr_dir / '.socket.sock', window)
        fake.start()
        gui_env['HYPRLAND_INSTANCE_SIGNATURE'] = sig

        subprocess.run([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'linux-layer'], env=base_env, check=True, timeout=120)
        subprocess.run([uniclip, 'start'], env=base_env, check=True, timeout=120)
        if args.wayland_debug:
            gui_env['WAYLAND_DEBUG'] = 'client'
        gui = Gui(sandbox, gui_env, out, 'gui1')
        gui.step('bootstrapped', 120)
        gui.wait_state('boot', lambda s: s['panelReady'], 60)
        conn = next(sandbox.rglob('daemon.conn'))
        daemon_pid = json.loads(conn.read_text())['pid']
        guilog = (out / 'gui1.log').read_text(errors='replace')

        def layer(label):
            return gui.ctl(f'layer-state {label}', f'layer-state-{label}')['detail']

        def trigger_second():
            return subprocess.run([str(sandbox / 'gui-go'), '--quick-panel'], env=dict(gui_env, UC_GUI_GO_EVIDENCE=str(out / 'second.jsonl')),
                                  timeout=60, capture_output=True, cwd=sandbox)

        probe_seq = [0]

        def visible():
            # Every control step needs its own label: Gui.step returns the newest row with the same name, so a reused label
            # would hand back the PREVIOUS answer (run7: a stale 'v' made resync() and the inside-click precondition lie).
            probe_seq[0] += 1
            return gui.state(f'v{probe_seq[0]}')['panelVisible']

        def resync(label):
            """Containment only: when an earlier check already FAILED and left the panel shown, hide it through the real host command
            so the following checks start from a hidden panel (toggle parity) instead of burning their timeouts. Every use is recorded."""
            if visible():
                results.setdefault('resyncs', []).append(label)
                gui.invoke(f'resync-{label}', 'dismiss_quick_panel')
                time.sleep(.5)

        def wait_visible(want, label, timeout=10):
            return gui.wait_state(label, lambda s: s['panelVisible'] == want, timeout)

        if args.no_layer_library:
            ls = layer('nolib')
            check('F2 the library is really absent: the GUI reports the missing runtime and keeps the ordinary window',
                  'GTK3 Layer Shell runtime is not installed' in guilog and not ls['active'], {'state': ls, 'log': [l for l in guilog.splitlines() if 'quick panel' in l]})
            r = subprocess.run(['python3', '-c', "import ctypes; ctypes.CDLL('libgtk-layer-shell.so.0')"], capture_output=True, text=True)
            check('F2 the dynamic loader itself cannot load libgtk-layer-shell.so.0 here (dlopen evidence, not an env-var trick)',
                  r.returncode != 0 and 'cannot open shared object file' in r.stderr, r.stderr.strip()[-300:])
            # the history view (with the Esc handler) is mounted only once the content grant is held; wayland-nolib-run1 showed the
            # locked view ignoring Esc, which is the product's behaviour, not the fallback's
            ur = gui.invoke('unlock', 'unlock_content', {'request': {'passphrase': PASSPHRASE}})
            check('set-up: content unlocked through the host command', ur['ok'], ur)
            time.sleep(1.5)
            trigger_second()
            s = wait_visible(True, 'nolib-show')
            check('F2 the ordinary panel window still shows (fallback is usable; this is NOT L5/L6 completion)', s['panelVisible'], s)
            beacons = Beacons()
            gui.ctl(f'panel-js beacon {beacons.script()}', 'panel-js-beacon')
            subprocess.run(['wtype', '-k', 'Escape'], env=sway.env, check=True, timeout=15)
            s = wait_visible(False, 'nolib-esc', 6)
            results['escDiagnosticsOrdinaryWindow'] = {'beacons': [e['path'] for e in beacons.events], 'hidden': not s['panelVisible']}
            check('F2 diagnostic: Esc typed through the compositor on the ORDINARY panel window (no Layer Shell) -> hidden?', not s['panelVisible'], results['escDiagnosticsOrdinaryWindow'])
            results['passed'] = all(c['ok'] for c in checks)
            gui.ctl('exit', 'control-exit')
            gui.proc.wait(timeout=60)
            gui = None
            return

        # Esc/ click behaviour belongs to the unlocked history view (QuickPanelApp mounts ClipboardHistoryPanel, and its Esc handler,
        # only when the content grant is held), so unlock through the real host command and daemon first.
        r = gui.invoke('unlock', 'unlock_content', {'request': {'passphrase': PASSPHRASE}})
        check('set-up: the content lock is released through the host command (the panel then mounts the history view with its Esc handler)', r['ok'], r)
        time.sleep(1.5)
        beacons = Beacons()
        gui.ctl(f'panel-js beacon {beacons.script()}', 'panel-js-beacon')
        time.sleep(1)
        ls = layer('boot')
        check('W2 Layer Shell backend initialized on the hidden, still unrealized panel (host state; the compositor side follows at first show)',
              ls['supported'] and ls['active'] and ls['state']['isLayer'] and not ls['state']['realized'] and 'Layer Shell backend initialized' in guilog, ls)
        check('F1 negative: attaching an already realized window (the visible main window) is refused, not faked',
              gui.ctl('layer-negative n1', 'layer-negative-n1')['ok'], None)

        # ---- keyboard exclusivity set-up: another real application with the focus ----
        probe_log = out / 'focus-probe.jsonl'
        probe_log.write_text('')
        probe = subprocess.Popen(['python3', str(focus_probe), str(probe_log)], env=sway.env, stdout=(out / 'focus-probe.out').open('w'), stderr=subprocess.STDOUT)

        def probe_events():
            return [json.loads(l) for l in probe_log.read_text().splitlines() if l.strip()]

        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and not any(e['event'] == 'focus-in' for e in probe_events()):
            time.sleep(.2)
        check('keyboard set-up: the other application (a plain xdg toplevel) owns the keyboard focus', any(e['event'] == 'focus-in' for e in probe_events()), probe_events())

        def wtype(*a):
            subprocess.run(['wtype', *a], env=sway.env, check=True, timeout=15)

        def sh(output, name):
            path = out / name
            subprocess.run(['grim', '-t', 'ppm', '-o', output, str(path)], env=sway.env, check=True, timeout=30)
            return path

        def park(x, y, click=False):
            fake.cursor = (float(x), float(y))
            pointer_cmd(f'move {x} {y}')
            if click:
                time.sleep(.15)
                pointer_cmd('press')
                time.sleep(.06)
                pointer_cmd('release')

        gui.invoke('pos', 'set_quick_panel_position', {'position': 'follow_cursor'})
        park(100, 100)

        # ---- show: protocol role, layer, namespaces, keyboard, backdrops ----
        n_log_before = len(sway.log_text())
        trigger_second()
        s = wait_visible(True, 'w3-shown')
        time.sleep(1.0)
        ls = layer('shown')
        log_new = sway.log_text()[n_log_before:]
        layer_lines = [l for l in log_new.splitlines() if 'layer surface' in l.lower()]
        panel_lines = [l for l in layer_lines if 'uniclipboard-quick-panel ' in l + ' ' and 'dismiss' not in l]
        dismiss_lines = [l for l in layer_lines if 'uniclipboard-quick-panel-dismiss' in l]
        (out / 'sway-layer-lines.txt').write_text('\n'.join(layer_lines) + '\n')
        check('W3 the compositor created a layer surface for the panel (namespace uniclipboard-quick-panel, layer 3 = overlay)',
              len(panel_lines) >= 1 and all(re.search(r'layer\D*3\b', l) for l in panel_lines), layer_lines)
        check('W3 the compositor created one dismissal surface per output (2) with the dismiss namespace', len(dismiss_lines) == 2, layer_lines)
        check('W3 host state: layer window, keyboard exclusive (1), two backdrops, visible', ls['state']['isLayer'] and ls['state']['keyboardMode'] == 1
              and ls['state']['backdrops'] == 2 and ls['state']['visible'], ls)
        ev = probe_events()
        check('F5 the other application lost the keyboard focus when the panel appeared (exclusive layer keyboard)',
              last_focus(ev) == 'focus-out', ev)
        wtype('q')
        time.sleep(.5)
        check('F5 typed text does not reach the other application while the panel is shown', not any(e['event'] == 'key' for e in probe_events()), probe_events())
        t_esc = time.monotonic()
        wtype('-k', 'Escape')
        s = wait_visible(False, 'w4-esc')
        ls = layer('after-esc')
        results['escDiagnostics'] = {'beacons': [e['path'] for e in beacons.events], 'layer': ls,
                                     'keydownEscapeSeenByPage': beacons.seen('keydown/Escape', t_esc)}
        check('F5/F11 Escape typed through the compositor reaches the panel WebView (exclusive focus) and the real frontend dismisses it', not s['panelVisible'], s)
        escape_worked = not s['panelVisible']
        if not escape_worked:  # the Esc check above stays FAILED; hide with the toggle so the remaining checks start from a hidden panel
            results['escFallbackToggleUsed'] = True
            trigger_second()
            s = wait_visible(False, 'w4-fallback-hide')
            ls = layer('after-fallback-hide')
        time.sleep(.5)
        ev = probe_events()
        check('F5 the keyboard went back to the other application after the panel hid (by Esc, or by the toggle when Esc failed: see escFallbackToggleUsed) and the keyboard mode is none',
              last_focus(ev) == 'focus-in' and ls['state']['keyboardMode'] == 0 and ls['state']['backdrops'] == 0, [ev, ls])
        wtype('z')
        time.sleep(.4)
        check('F5 after hiding, typed text reaches the other application again', any(e['event'] == 'key' for e in probe_events()), probe_events())

        # ---- backdrop dismissal by a real pointer click ----
        park(100, 100)
        trigger_second()
        wait_visible(True, 'w5-shown')
        time.sleep(1.0)
        rect = ls_rect = None
        pre = wait_visible(True, 'w5-pre')
        check('F6 precondition: the panel is shown (not a vacuous pass)', pre['panelVisible'], pre)
        placed = layer('w5')['placed']
        px, py, pw, ph = placed[1] + OUT1['x'], placed[2], placed[3], placed[4]  # output 1, logical
        t_click = time.monotonic()
        park(px + pw // 2, py + ph // 2, click=True)
        time.sleep(.6)
        check('F6 a click INSIDE the panel (real pointer at its centre) reaches the panel page and does not dismiss it',
              visible() and bool(beacons.seen('mousedown', t_click)), {'placed': placed, 'mousedown': beacons.seen('mousedown', t_click), 'visible': visible()})
        outside = (OUT1['w'] - 40, OUT1['h'] - 40) if (px + pw < OUT1['w'] - 60 and py + ph < OUT1['h'] - 60) else (20, OUT1['h'] - 20)
        park(*outside, click=True)
        s = wait_visible(False, 'w5-click')
        ls = layer('w5-after')
        check('F6 a real pointer click on the dismissal surface outside the panel (press+release) hides it, releases the keyboard and destroys the backdrops',
              not s['panelVisible'] and ls['state']['keyboardMode'] == 0 and ls['state']['backdrops'] == 0, {'state': s, 'layer': ls})
        park(OUT2['x'] + 20, OUT2['y'] + 20)
        trigger_second()
        wait_visible(True, 'w5b-shown')
        time.sleep(1.0)
        presses0 = layer('w5b-pre')['state']['backdropPresses']
        park(OUT2['x'] + OUT2['w'] - 10, OUT2['y'] + OUT2['h'] - 10, click=True)
        s = wait_visible(False, 'w5b-click')
        presses1 = layer('w5b-post')['state']['backdropPresses']
        check('F6 the dismissal surface of the SECOND output also dismisses (every output has one)', not s['panelVisible'], {'state': s, 'backdropPressesBeforeAfter': [presses0, presses1]})

        # The other application must not be on the output while measuring: it is a tiled window whose focus border and text
        # change when the panel takes the keyboard, which pollutes a screenshot difference (run2/run3 showed exactly that).
        probe.terminate()
        probe.wait(timeout=10)
        time.sleep(1)

        # Diagnostic (not a pass criterion): where on the second output do real clicks reach the dismissal surface's GTK handler?
        probes = []
        for (dx, dy) in ((790, 490), (700, 480), (780, 200), (400, 480), (790, 20)):
            resync('f6b')
            park(OUT2['x'] + 20, OUT2['y'] + 20)
            trigger_second()
            wait_visible(True, f'f6b-{dx}-{dy}-shown')
            time.sleep(.8)
            before = layer(f'f6b-{dx}-{dy}-pre')
            park(OUT2['x'] + dx, OUT2['y'] + dy, click=True)
            time.sleep(.6)
            after = layer(f'f6b-{dx}-{dy}-post')
            probes.append({'point': [dx, dy], 'placed': before['placed'], 'presses': [before['state']['backdropPresses'], after['state']['backdropPresses']],
                           'hiddenAfter': not after['state']['visible']})
        results['f6bProbe'] = probes
        resync('f6b-end')

        # ---- placement and caps, per output, measured from screenshots ----
        def measure(label, cursor, out_spec, follow=True, base=(BASE_W, BASE_H), tolerance=2.5):
            resync(label)
            park(*cursor)
            exp = expected_rect(out_spec, cursor if follow else None, base, follow)
            time.sleep(1.2)
            base_img = sh(out_spec['name'], f'{label}-before.ppm')  # panel hidden, pointer already parked where it stays
            trigger_second()
            wait_visible(True, f'{label}-shown')
            time.sleep(1.2)
            img = sh(out_spec['name'], f'{label}.ppm')
            box = diff_bbox(base_img, img, out_spec['scale'])
            st = layer(label)
            gui_placed = st['placed']
            trigger_second()
            wait_visible(False, f'{label}-hidden')
            # the window includes PAD of transparent padding the user cannot see: the visible box lies inside the window
            # and at least covers it minus the padding
            if box is None:
                check(f'F7 {label}: the panel is visible in the screenshot', False, {'expected': exp, 'placed': gui_placed})
                return
            gx, gy = box[0] + out_spec['x'], box[1] + out_spec['y']
            inside = gx >= exp[0] - tolerance and gy >= exp[1] - tolerance and gx + box[2] <= exp[0] + exp[2] + tolerance and gy + box[3] <= exp[1] + exp[3] + tolerance
            covers = gx <= exp[0] + 2 * PAD + tolerance and gy <= exp[1] + 2 * PAD + tolerance and gx + box[2] >= exp[0] + exp[2] - 2 * PAD - tolerance \
                and gy + box[3] >= exp[1] + exp[3] - 2 * PAD - tolerance
            check(f'F7 {label}: measured on the compositor, the panel lies inside the expected rectangle {exp} (cursor {cursor}) and fills it up to the padding',
                  inside and covers, {'expected': exp, 'measuredLayoutRect': [gx, gy, box[2], box[3]], 'hostPlaced': gui_placed})

        measure('place-out1-center', (640, 400), OUT1)
        measure('place-out1-near-origin', (50, 50), OUT1)
        measure('place-out1-far-corner', (1260, 780), OUT1)
        measure('place-out2-small-output-cap-720x400', (OUT2['x'] + 100, 100), OUT2)
        measure('place-out2-far-corner', (OUT2['x'] + 790, 490), OUT2)
        gui.invoke('pos2', 'set_quick_panel_position', {'position': 'center'})
        measure('place-out2-centered', (OUT2['x'] + 300, 250), OUT2, follow=False)
        measure('place-out1-centered', (300, 300), OUT1, follow=False)
        gui.invoke('pos3', 'set_quick_panel_position', {'position': 'follow_cursor'})

        # ---- window scale (F9) ----
        resync('f9')
        park(640, 400)
        trigger_second()
        wait_visible(True, 'f9-shown')
        for scale, label in ((1.5, 'f9-1.5'), (0.8, 'f9-0.8')):
            r = gui.invoke(label, 'set_quick_panel_layout', {'scale': None, 'previewExpanded': False, 'windowScale': scale})
            time.sleep(.6)
            st = layer(label + '-state')
            want_w = min(BASE_W * scale, OUT1['w'] * CAP_W)
            want_h = min(BASE_H * scale, OUT1['h'] * CAP_H)
            check(f'F9 windowScale {scale}: the layout command applies it inside the 90%/80% caps (host placement {int(want_w)}x{int(want_h)})',
                  r['ok'] and st['placed'][3] == int(want_w) and st['placed'][4] == int(want_h), {'result': r, 'placed': st['placed']})
        trigger_second()
        wait_visible(False, 'f9-hidden')

        # ---- fallback of the cursor (F8): scripted Hyprland gives garbage / nothing ----
        resync('f8')
        fake.cursor = (float('nan'), 0.0)
        # json.dumps writes NaN, which Go refuses: the host must fall back to the primary output centre of work area
        trigger_second()
        wait_visible(True, 'f8-shown')
        time.sleep(1.0)
        st = layer('f8')
        check('F8 an invalid cursor from the compositor falls back to the default output instead of failing (placement without cursor)',
              st['state']['visible'] and st['placement'] is not None and not st['placement']['haveCursor'], st)
        trigger_second()
        wait_visible(False, 'f8-hidden')
        fake.cursor = (640.0, 400.0)

        # ---- cycles: no leaks, process alive (F4/F6) ----
        for i in range(15):
            resync(f'cycle{i}')
            trigger_second()
            wait_visible(True, f'cy-s{i}')
            trigger_second()
            wait_visible(False, f'cy-h{i}')
        st = layer('cycles')
        check('F4/F6 15 show/hide cycles: the process is alive, no backdrops leaked, keyboard released',
              gui.proc.poll() is None and st['state']['backdrops'] == 0 and st['state']['keyboardMode'] == 0, st)

        # ---- paste chain (F10): hide (keyboard released) happens, then the Hyprland chain ----
        resync('f10')
        park(640, 400)
        trigger_second()
        wait_visible(True, 'f10-shown')
        time.sleep(.8)
        pre = list(fake.commands)
        r = gui.invoke('paste', 'paste_to_previous_app')
        time.sleep(.6)
        st = layer('f10')
        cmds = fake.commands[len(pre):]
        check('F10 paste: the panel is hidden and the layer keyboard released before/with the Hyprland focus+shortcut chain',
              r['ok'] and not st['state']['visible'] and st['state']['keyboardMode'] == 0 and st['state']['backdrops'] == 0
              and any('hl.dsp.focus' in c for c in cmds) and any('send_shortcut' in c for c in cmds), {'result': r, 'commands': cmds, 'layer': st})

        gui.ctl('exit', 'control-exit')
        code = gui.proc.wait(timeout=60)
        deadline = time.monotonic() + 20
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        check('exit: GUI exit 0 and the daemon stopped by the GUI', code == 0 and not pid_alive(daemon_pid), {'exit': code})
        gui = None
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        for p in (probe, keeper):
            if p and p.poll() is None:
                p.terminate()
        if vp and vp.poll() is None:
            vp.stdin.close()
            vp.wait(timeout=10)
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        if fake:
            fake.stop = True
        subprocess.run([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid and pid_alive(daemon_pid):
            os.kill(daemon_pid, 15)
        if sway:
            sway.stop()
        (out / 'wayland-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False, default=str) + '\n', encoding='utf-8')
        if sandbox.name.startswith('uc-gui-go-') and sandbox.parent == Path(tempfile.gettempdir()):
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
