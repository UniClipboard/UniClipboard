#!/usr/bin/env python3
"""Native-host runner of slice 17c13 (docs/architecture/gui-go-linux-appimage-native-wayland.md): the real AppImage on a real Wayland session
(Fedora niri VM, Omarchy Hyprland machine), as the logged-in user, inside a task-owned directory only.

  native_wayland_probe.py --appimage X.AppImage --out DIR --mode native|x11-env|x11-hook [--no-session-type] [--seconds 20]

Modes (the SAME package, only the user's environment differs, except x11-hook which is the differential control package):
  native     GDK_BACKEND unset: GTK must choose the Wayland backend; the quick panel must become a wlr-layer-shell surface.
  x11-env    GDK_BACKEND=x11 set by the user (must be honoured): XWayland, ordinary panel window, no Layer Shell.
  x11-hook   the X11HOOK- control package (hook still forces x11): XWayland, as 17c4-17c12 shipped.
  x11-session   XDG_SESSION_TYPE=x11 with a live Wayland socket and no GDK_BACKEND (W10): the session type must win, XWayland, no Layer Shell.
  native-no-layer-protocol   a Wayland compositor WITHOUT wlr-layer-shell (the Weston container control, --compositor-kind generic): native Wayland backend, Layer
             Shell reported unsupported, the panel stays an ordinary window, no crash.
  native-missing-library     the NOLAYER- control package (libgtk-layer-shell not carried) in a container whose host has no such library either, on a compositor that HAS the
             protocol (sway): the library must not be loaded at all (maps), Layer Shell reported unavailable with the loader error, the panel stays an ordinary window.

Evidence is observed, never inferred from an environment variable: the GUI's and WebKit's unix sockets are classified by the server they are connected to
(Wayland socket vs X11 socket), the in-process GDK display type comes from the GUI's own layer-state probe, the compositor lists the client (xwayland flag) and its
layer surfaces, /proc/<pid>/maps names the library files really loaded. A value that could not be read is recorded as `unknown`, never as a pass.
Isolation: the AppImage copy runs in PORTABLE mode (its HOME is <copy>.home), with a task-owned private session bus (no service directories) and the system clipboard
disabled; the user's own application, keyring, proxy, default applications, key bindings and autostart are not touched. Only pids started here are signalled.
Output: DIR/native-wayland-result.json (+ raw logs). Exit status 0 only when every check passed.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linux_wayland_run import Beacons  # noqa: E402  (loopback listener the injected page script reports key/focus events to)

NAMESPACE = 'uniclipboard-quick-panel'


def sh(cmd, env=None, timeout=20):
    try:
        r = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout)
        return r.returncode, r.stdout, r.stderr
    except (OSError, subprocess.TimeoutExpired) as e:
        return -1, '', str(e)


def procs():
    rows = {}
    for p in Path('/proc').iterdir():
        if p.name.isdigit():
            try:
                rows[int(p.name)] = os.readlink(p / 'exe')
            except OSError:
                pass
    return rows


def maps_of(pid):
    libs = set()
    try:
        for line in Path(f'/proc/{pid}/maps').read_text().splitlines():
            parts = line.split(None, 5)
            if len(parts) == 6 and '.so' in parts[5]:
                libs.add(parts[5].replace(' (deleted)', ''))
    except OSError:
        return None
    return libs


def environ_of(pid):
    try:
        return dict(i.split('=', 1) for i in Path(f'/proc/{pid}/environ').read_bytes().decode(errors='replace').split('\0') if '=' in i)
    except OSError:
        return None


def socket_classes(pids):
    """Classify each unix socket of the pids by the server it is connected to. `ss -xan` rows: netid state recvq sendq LOCALPATH LOCALINODE PEERPATH PEERINODE; an accepted
    server-side socket carries the listening path, so the peer row's local path names the server (wayland-N / X11)."""
    rows = {}
    for ln in sh(['ss', '-xanH'])[1].splitlines():
        f = ln.split()
        if len(f) >= 8:
            rows[f[5]] = (f[4], f[7])
    out = {}
    for pid in pids:
        c = {'wayland': 0, 'x11': 0, 'other': 0}
        try:
            fds = os.listdir(f'/proc/{pid}/fd')
        except OSError:
            out[str(pid)] = None
            continue
        for fd in fds:
            try:
                m = re.match(r'socket:\[(\d+)\]', os.readlink(f'/proc/{pid}/fd/{fd}'))
            except OSError:
                continue
            if not m or m.group(1) not in rows:
                continue
            peer = rows.get(rows[m.group(1)][1], ('', ''))[0]
            c['wayland' if 'wayland-' in peer else 'x11' if 'X11' in peer else 'other'] += 1
        out[str(pid)] = c
    return out


class Compositor:
    """The two compositors of the authorised hosts, through their own CLIs (no protocol code here)."""

    def __init__(self, runtime, env, kind='auto'):
        self.env = env
        if kind == 'generic':
            self.kind = 'generic'
            return
        sig = next((p.name for p in (runtime / 'hypr').glob('*') if (p / '.socket.sock').exists()), None) if (runtime / 'hypr').is_dir() else None
        niri = next(iter(sorted(runtime.glob('niri.*.sock'))), None)
        if sig:
            self.kind = 'hyprland'
            env['HYPRLAND_INSTANCE_SIGNATURE'] = sig
        elif niri:
            self.kind = 'niri'
            env['NIRI_SOCKET'] = str(niri)
        else:
            self.kind = None

    def version(self):
        cmd = ['hyprctl', 'version'] if self.kind == 'hyprland' else ['niri', 'msg', 'version'] if self.kind == 'niri' else None
        return sh(cmd, self.env)[1].strip().splitlines()[:2] if cmd else None

    def clients(self):
        """[{pid, xwayland (True/False/None=unknown), appId, raw}] for the compositor's own window list."""
        if self.kind == 'hyprland':
            rc, out, err = sh(['hyprctl', '-j', 'clients'], self.env)
            try:
                return [{'pid': c.get('pid'), 'xwayland': c.get('xwayland'), 'appId': c.get('class'), 'title': c.get('title'), 'mapped': c.get('mapped'), 'size': c.get('size'),
                         'at': c.get('at'), 'monitor': c.get('monitor')} for c in json.loads(out)]
            except ValueError:
                return None
        if self.kind == 'niri':
            rc, out, err = sh(['niri', 'msg', '--json', 'windows'], self.env)
            try:
                return [{'pid': c.get('pid'), 'xwayland': None, 'appId': c.get('app_id'), 'title': c.get('title'), 'focused': c.get('is_focused')} for c in json.loads(out)]
            except ValueError:
                return None
        return None

    def layers(self):
        """[{namespace, layer, output, x, y, w, h (None when the compositor does not report them)}] or None when the listing could not be read."""
        if self.kind == 'hyprland':
            rc, out, err = sh(['hyprctl', '-j', 'layers'], self.env)
            try:
                data = json.loads(out)
            except ValueError:
                return None
            names = {'0': 'background', '1': 'bottom', '2': 'top', '3': 'overlay'}
            rows = []
            for output, body in data.items():
                for level, items in body.get('levels', {}).items():
                    for it in items:
                        rows.append({'namespace': it.get('namespace'), 'layer': names.get(str(level), str(level)), 'output': output, 'x': it.get('x'), 'y': it.get('y'),
                                     'w': it.get('w'), 'h': it.get('h'), 'pid': it.get('pid')})
            return rows
        if self.kind == 'niri':
            rc, out, err = sh(['niri', 'msg', '--json', 'layers'], self.env)
            try:
                data = json.loads(out)
            except ValueError:
                return None
            return [{'namespace': it.get('namespace'), 'layer': str(it.get('layer', '')).lower(), 'output': it.get('output'), 'x': None, 'y': None, 'w': None, 'h': None,
                     'keyboard': it.get('keyboard_interactivity')} for it in data]
        return None

    def monitors(self):
        if self.kind == 'hyprland':
            rc, out, err = sh(['hyprctl', '-j', 'monitors'], self.env)
            try:
                return [{'name': m['name'], 'w': m['width'], 'h': m['height'], 'x': m['x'], 'y': m['y'], 'scale': m['scale']} for m in json.loads(out)]
            except (ValueError, KeyError):
                return None
        if self.kind == 'niri':
            rc, out, err = sh(['niri', 'msg', '--json', 'outputs'], self.env)
            try:
                return [{'name': n, 'logical': o.get('logical')} for n, o in json.loads(out).items()]
            except ValueError:
                return None
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--mode', choices=('native', 'x11-env', 'x11-hook', 'native-no-layer-protocol', 'native-missing-library', 'x11-session'), required=True)
    ap.add_argument('--compositor-kind', choices=('auto', 'generic'), default='auto', help='generic: a compositor without a CLI to list windows/layers (the Weston container control); those listings are then UNKNOWN')
    ap.add_argument('--no-session-type', action='store_true', help='observation scenario: leave XDG_SESSION_TYPE unset (what an SSH shell has); records what GTK/Wails then choose')
    ap.add_argument('--gdk-backend', help='native mode: export this GDK_BACKEND like the user session does (Omarchy exports `wayland,x11,*` for every app); default: unset')
    ap.add_argument('--seconds', type=int, default=20)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = out / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    uid = os.getuid()
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{uid}')
    wayland = next((p.name for p in sorted(runtime.glob('wayland-[0-9]*')) if not p.name.endswith('.lock')), None)
    result = {'mode': args.mode, 'noSessionType': args.no_session_type, 'host': sh(['uname', '-srm'])[1].strip(), 'waylandSocket': wayland, 'checks': [], 'unknown': [],
              'scope': 'native host, real Wayland session, portable mode (task-owned HOME), private session bus; one output unless stated; ARM64 only'}

    def check(name, ok, detail=None):
        result['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def unknown(what):
        result['unknown'].append(what)
        print('UNKNOWN ' + what, flush=True)

    env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|UC_|UNICLIPBOARD|GIO_|APPIMAGE|APPDIR|GDK_BACKEND|DBUS_SESSION|XDG_SESSION_TYPE', k)}
    env.update(XDG_RUNTIME_DIR=str(runtime), UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
               UC_GUI_GO_E2E_SECRET='native-wayland-probe-17c13', UC_GUI_GO_EVIDENCE=str(out / 'gui.jsonl'), UC_GUI_GO_E2E_CONTROL_FILE=str(out / 'gui.control'))
    if wayland:
        env['WAYLAND_DISPLAY'] = wayland
        if not args.no_session_type:
            env['XDG_SESSION_TYPE'] = 'x11' if args.mode == 'x11-session' else 'wayland'  # what the graphical session exports; an SSH shell lacks it (the scenario --no-session-type observes that case)
    x11 = sorted(p.name for p in Path('/tmp/.X11-unix').glob('X[0-9]*'))
    if x11:
        env['DISPLAY'] = ':' + x11[0][1:]  # the session's XWayland, needed by the x11 modes
    if args.mode == 'x11-env':
        env['GDK_BACKEND'] = 'x11'
    elif args.gdk_backend:
        env['GDK_BACKEND'] = args.gdk_backend
    result['gdkBackendSuppliedByProbe'] = env.get('GDK_BACKEND')
    comp = Compositor(runtime, env, args.compositor_kind)
    result['compositor'] = {'kind': comp.kind, 'version': comp.version(), 'monitors': comp.monitors()}
    check('a Wayland socket and a known compositor exist on this host', bool(wayland) and comp.kind, result['compositor'])
    if not (wayland and comp.kind):
        (out / 'native-wayland-result.json').write_text(json.dumps(result, indent=2) + '\n')
        sys.exit(1)
    fuse = any(Path(d).glob('libfuse.so.2') for d in ('/usr/lib', '/usr/lib64', '/usr/lib/aarch64-linux-gnu', '/lib64'))
    result['fuse2'] = fuse
    if not fuse:
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
    for f in ('gui.jsonl', 'gui.control'):
        (out / f).write_text('')
    made = subprocess.run([str(app), '--appimage-portable-home'], env=env, capture_output=True, text=True, timeout=60)
    home = Path(str(app) + '.home')
    check('portable home created next to the task-owned copy', made.returncode == 0 and home.is_dir(), {'rc': made.returncode, 'err': made.stderr[-200:]})
    result['appimageSha256'] = sh(['sha256sum', str(app)])[1].split()[0]

    cfg = out / 'private-bus.conf'  # a TASK-OWNED session bus: no service directories, so nothing of the user's session is activated
    cfg.write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">\n<busconfig><type>session</type>'
                   f'<listen>unix:path={out}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>\n')
    bus = subprocess.Popen(['dbus-daemon', '--config-file', str(cfg), '--nofork'], stdout=(out / 'private-bus.log').open('w'), stderr=subprocess.STDOUT)
    for _ in range(50):
        if (out / 'bus.sock').exists():
            break
        time.sleep(.1)
    env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={out}/bus.sock'

    beacons = Beacons()
    proc = subprocess.Popen([str(app)], env=env, cwd=str(out), stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT)

    def step(name, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = []
            for ln in (out / 'gui.jsonl').read_text().splitlines():
                try:
                    r = json.loads(ln)
                except ValueError:
                    continue
                if r.get('step') == name:
                    rows.append(r)
            if rows:
                return rows[-1]
            if proc.poll() is not None:
                return None
            time.sleep(.2)
        return None

    seq = [0]

    # NOTE: the control verbs write their own `<verb>-<label>` step, so the label passed to the verb is `<seq>` and the step name is `<verb>-<seq>`.
    def verb(name, arg_extra='', timeout=60):
        seq[0] += 1
        label = str(seq[0])
        with (out / 'gui.control').open('a') as f:
            f.write(f'{name} {label}{(" " + arg_extra) if arg_extra else ""}\n')
        return step(f'{name}-{label}', timeout)

    def panel_state():
        r = verb('shortcut-state')
        return (r or {}).get('detail') or {}

    def trigger():
        return subprocess.run([str(app), '--quick-panel'], env=env, cwd=str(out), capture_output=True, timeout=60)

    def wait_visible(want, timeout=12):
        deadline = time.monotonic() + timeout
        s = {}
        while time.monotonic() < deadline:
            s = panel_state()
            if bool(s.get('panelVisible')) == want:
                return s
            time.sleep(.3)
        return s

    t0 = time.monotonic()
    conn = None
    while time.monotonic() - t0 < 90 and proc.poll() is None:
        for c in home.rglob('daemon.conn'):
            try:
                cand = json.loads(c.read_text())
                os.kill(cand['pid'], 0)
                conn = cand
            except (OSError, ValueError, KeyError):
                pass
        if conn:
            break
        time.sleep(.3)
    check('the real bundled daemon published daemon.conn and is alive', conn is not None, {'seconds': round(time.monotonic() - t0, 1)})
    boot = step('bootstrapped', 90)
    check('the WebView ran the frontend (the page wrote evidence step `bootstrapped` through the host service)', bool(boot and boot.get('ok')), boot)
    deadline = time.monotonic() + 60
    state = {}
    while time.monotonic() < deadline:
        state = panel_state()
        if state.get('panelReady') is True:
            break
        time.sleep(1)
    check('the frontend reached the daemon: the quick panel page reports ready', state.get('panelReady') is True, state)
    time.sleep(args.seconds)

    table = procs()
    exe = table.get(proc.pid, '')
    mount = exe.split('/usr/bin/')[0] if '/usr/bin/' in exe else None
    result['mount'] = mount
    web = [pid for pid, e in table.items() if mount and e.startswith(mount) and e.rsplit('/', 1)[-1] in ('WebKitWebProcess', 'WebKitNetworkProcess', 'WebKitGPUProcess')]
    ours = [proc.pid] + web
    classes = socket_classes(ours)
    result['socketClasses'] = classes
    gui_env = environ_of(proc.pid) or {}
    result['guiEnvGdkBackend'] = gui_env.get('GDK_BACKEND')
    wl, xs = (classes.get(str(proc.pid)) or {}).get('wayland', 0), (classes.get(str(proc.pid)) or {}).get('x11', 0)
    web_wl = sum((classes.get(str(p)) or {}).get('wayland', 0) for p in web)
    web_x = sum((classes.get(str(p)) or {}).get('x11', 0) for p in web)
    mine = [c for c in (comp.clients() or []) if c.get('pid') == proc.pid]
    result['compositorClients'] = mine
    ls = verb('layer-state')
    layer = (ls or {}).get('detail') or {}
    result['layerState'] = layer
    if args.mode in ('native', 'native-no-layer-protocol', 'native-missing-library'):
        check('W1 the GUI process holds a connection to the Wayland compositor socket and none to an X11 server', wl >= 1 and xs == 0, {'gui': classes.get(str(proc.pid))})
        check('W1 the GUI environment carries no GDK_BACKEND other than the one the user session supplied (nothing forced x11)', gui_env.get('GDK_BACKEND') == (args.gdk_backend or None), {'gui': gui_env.get('GDK_BACKEND'), 'suppliedByProbe': args.gdk_backend})
        if comp.kind == 'generic':
            unknown('generic compositor: no window list; the socket classes are the evidence of the backend')
        else:
          check('W1 the compositor lists the GUI window as a native Wayland client', bool(mine) and all(c.get('xwayland') in (False, None) for c in mine) and (comp.kind != 'hyprland' or all(c.get('xwayland') is False for c in mine)), mine)
        if args.mode == 'native':
            check('W1 the GUI itself reports the Wayland GDK display (Layer Shell supported = GdkWaylandDisplay and the protocol)', layer.get('supported') is True, {k: layer.get(k) for k in ('supported', 'supportedError')})
        elif args.mode == 'native-missing-library':
            check('W3 the library is missing: the GUI reports Layer Shell unavailable with the loader error (no crash)', layer.get('supported') is False and 'not installed' in str(layer.get('supportedError', '')),
                  {k: layer.get(k) for k in ('supported', 'supportedError')})
        else:
            check('W7 the compositor has no wlr-layer-shell: the GUI reports Layer Shell unsupported on its Wayland display (no crash)', layer.get('supported') is False, {k: layer.get(k) for k in ('supported', 'supportedError')})
        if web:
            check('W1 the WebKit processes do not talk to an X11 server', web_x == 0, {'web': {str(p): classes.get(str(p)) for p in web}})
        else:
            unknown('WebKit processes not found under the mount')
    else:
        check('W6 the GUI process is an X11 client (a connection to the X server, none to the Wayland socket)', xs >= 1 and wl == 0, {'gui': classes.get(str(proc.pid))})
        if args.mode == 'x11-session':
            # /proc/<pid>/environ is the INITIAL block: the value Wails exports in-process (os.Setenv in its init) is not in it, so the effective GDK_BACKEND is not observable from outside.
            # The evidence for this mode is the socket classes above (X11 connection, no Wayland connection) and the compositor listing; the initial block must NOT carry the variable.
            check('W10 the process was started WITHOUT GDK_BACKEND (initial environment); the backend choice is read from the sockets, not from an environment variable', gui_env.get('GDK_BACKEND') is None, {'initialEnvironment': gui_env.get('GDK_BACKEND')})
        else:
            check('W6 the GUI environment shows the backend the mode sets (x11)', gui_env.get('GDK_BACKEND') == 'x11', gui_env.get('GDK_BACKEND'))
        check('W6 Layer Shell is not used on the X11 backend (layer-state: not supported)', layer.get('supported') is False, {k: layer.get(k) for k in ('supported', 'supportedError')})
        if comp.kind == 'hyprland':
            check('W6 the compositor lists the GUI window as an XWayland client', bool(mine) and all(c.get('xwayland') is True for c in mine), mine)
        else:
            unknown(f'{comp.kind}: the window list has no xwayland flag; the socket classes above are the evidence')
    # the frontend <-> daemon chain, as observed in the WebKit network process (HTTP keep-alive and the WebSocket)
    if conn:
        socks = sh(['ss', '-tnp'])[1]
        web_to_daemon = [l for l in socks.splitlines() if 'WebKitNetwork' in l and f":{conn['port']}" in l and 'ESTAB' in l]
        result['webkitToDaemonEstablished'] = len(web_to_daemon)
        check('the WebView holds established loopback connections to the daemon (HTTP / WebSocket)', len(web_to_daemon) >= 1, web_to_daemon[:3])
    # library provenance: the files really loaded
    libs = maps_of(proc.pid) or set()
    pick = sorted(p for p in libs if re.search(r'/(libgtk-layer-shell|libgtk-3|libgdk-3|libglib-2|libgio-2|libwebkit2gtk-4\.1|libwayland-client|libEGL)\.', p))
    result['loadedLibraries'] = pick
    if args.mode == 'native-missing-library':
        check('W3 no libgtk-layer-shell file of any origin is mapped into the GUI (bundled absent, container host has none)', not [p for p in libs if 'libgtk-layer-shell' in p], sorted(p for p in libs if 'layer-shell' in p))
    if args.mode in ('native', 'native-no-layer-protocol', 'native-missing-library') and mount:
        if args.mode != 'native-missing-library':
            shell_libs = [p for p in pick if 'libgtk-layer-shell' in p]
            check('W3/W4 libgtk-layer-shell is loaded from the AppImage mount (the bundled copy, not the host\'s)', bool(shell_libs) and all(p.startswith(mount) for p in shell_libs), shell_libs)
        closure = [p for p in pick if re.search(r'/(libgtk-3|libgdk-3|libglib-2|libgio-2|libwebkit2gtk-4\.1)\.', p)]
        check('W4 the GTK closure (gtk-3, gdk-3, glib, gio, webkit2gtk) is loaded from the AppImage mount', bool(closure) and all(p.startswith(mount) for p in closure), closure)
        host_wl = [p for p in pick if 'libwayland-client' in p]
        check('W4 libwayland-client is the HOST\'s (the AppImage keeps the driver stack out)', bool(host_wl) and not any(p.startswith(mount) for p in host_wl), host_wl)

    if args.mode == 'native':
        # panel as a layer surface
        base = comp.layers()
        if base is None:
            unknown('the compositor layer listing could not be read')
        before = [l for l in (base or []) if l.get('namespace') == NAMESPACE]
        check('precondition: no layer surface of the panel exists before it is shown', base is not None and not before, before)
        js = beacons.script()
        with (out / 'gui.control').open('a') as f:  # `panel-js <tag> <script>` runs the script in the quick-panel WebView and answers with the step `panel-js-<tag>`
            f.write(f'panel-js beacon {js}\n')
        armed = step('panel-js-beacon', 30)
        check('the key/focus beacon script ran in the panel page (control acknowledged)', bool(armed and armed.get('ok')), armed)
        t_show = time.monotonic()
        trigger()
        s = wait_visible(True)
        check('the panel is shown through the real single-instance trigger (`--quick-panel`)', s.get('panelVisible') is True, s)
        time.sleep(1.5)
        ls = verb('layer-state')
        d = (ls or {}).get('detail') or {}
        st = d.get('state') or {}
        check('W4 the panel window is a layer surface in the GUI (gtk_layer_is_layer_window)', st.get('IsLayer') is True or st.get('isLayer') is True, d)
        listing = comp.layers()
        shown = [l for l in (listing or []) if l.get('namespace') == NAMESPACE]
        result['layersWhileShown'] = listing
        check('W3/W5 the COMPOSITOR lists the panel as a layer surface with namespace ' + NAMESPACE, bool(shown), shown)
        check('W5 the layer is overlay', bool(shown) and all(l.get('layer') == 'overlay' for l in shown), shown)
        mons = comp.monitors() or []
        if shown and shown[0].get('w') is not None and comp.kind == 'hyprland':
            sl = shown[0]
            mon = next((m for m in mons if m['name'] == sl['output']), None)
            inside = bool(mon) and sl['x'] >= mon['x'] and sl['y'] >= mon['y'] and sl['x'] + sl['w'] <= mon['x'] + mon['w'] / mon['scale'] + 1 and sl['y'] + sl['h'] <= mon['y'] + mon['h'] / mon['scale'] + 1
            check('W5 the panel rectangle lies inside its output (compositor-reported, logical px)', inside, {'layer': {k: sl.get(k) for k in ('x', 'y', 'w', 'h', 'output')}, 'monitor': mon})
        else:
            unknown(f'{comp.kind}: the layer listing has no geometry; panel placement is not verified on this host')
        placement = d.get('placement') or {}
        if shown and shown[0].get('w') is not None and comp.kind == 'hyprland' and placement and not placement.get('haveCursor'):
            sl = shown[0]
            mon = next((m for m in mons if m['name'] == sl['output']), None)
            if mon:
                lw, lh = mon['w'] / mon['scale'], mon['h'] / mon['scale']
                check('W5 the default position centres the panel in the output (compositor-reported rectangle vs output logical size, 2 px)',
                      abs(sl['x'] - (lw - sl['w']) / 2) <= 2 and abs(sl['y'] - (lh - sl['h']) / 2) <= 2, {'layer': {k: sl[k] for k in ('x', 'y', 'w', 'h')}, 'outputLogical': [lw, lh]})
        else:
            unknown(f'{comp.kind}: panel centring / cursor-follow placement not verified here (no geometry, or follow-cursor needs the real pointer which is not moved)')
        km = st.get('KeyboardMode', st.get('keyboardMode'))
        check('W5 keyboard interactivity is exclusive while shown (client-side state; gtk-layer-shell enum NONE=0 EXCLUSIVE=1 ON_DEMAND=2)', km == 1, {'keyboardMode': km})
        if comp.kind == 'niri':
            check('W5 the compositor reports exclusive keyboard interactivity for the panel layer', any(str(l.get('keyboard', '')).lower() == 'exclusive' for l in shown), shown)
        # keyboard focus: a real virtual keystroke through the compositor must reach the panel page. Shift only (harmless if it went elsewhere).
        if shutil.which('wtype') and comp.kind == 'hyprland':
            n = len(beacons.events)
            wrc = sh(['wtype', '-k', 'Shift_L'], env)
            time.sleep(1.2)
            keys = [e['path'] for e in beacons.events[n:] if e['path'].startswith('/keydown/')]
            check('W5 a keystroke injected through the compositor (wtype) reaches the panel page: the layer has keyboard focus', bool(keys),
                  {'keys': keys, 'wtype': {'rc': wrc[0], 'stderr': wrc[2][-200:]}, 'allBeaconEvents': [e['path'] for e in beacons.events], 'layerStateWhileShown': st,
                   'hyprlandActiveWindow': sh(['hyprctl', 'activewindow'], env)[1][:300]})
        else:
            unknown(f'{comp.kind}: no key injection tool on this host (wtype absent): keyboard focus of the layer is verified from the client state and (niri) the compositor keyboard-interactivity report only')
        trigger()
        s = wait_visible(False)
        check('W5 the panel hides through the real trigger toggle', s.get('panelVisible') is False, s)
        deadline = time.monotonic() + 6
        gone = False
        while time.monotonic() < deadline:
            listing = comp.layers()
            if listing is not None and not [l for l in listing if l.get('namespace') == NAMESPACE]:
                gone = True
                break
            time.sleep(.4)
        check('W5 after hiding, the compositor no longer lists the panel layer surface', gone, listing)
        ls = verb('layer-state')
        d = (ls or {}).get('detail') or {}
        st = d.get('state') or {}
        check('W5 hidden: keyboard mode none and no dismissal backdrops left', st.get('KeyboardMode', st.get('keyboardMode')) == 0 and st.get('Backdrops', st.get('backdrops')) == 0, d)
        # a second cycle: the surface must be creatable again (hide destroys backdrops; the main role is stable)
        trigger()
        s = wait_visible(True)
        time.sleep(1)
        shown2 = [l for l in (comp.layers() or []) if l.get('namespace') == NAMESPACE]
        check('W5 the second show again appears as a layer surface in the compositor', s.get('panelVisible') is True and bool(shown2), shown2)
        trigger()
        wait_visible(False)
    else:
        trigger()
        s = wait_visible(True)
        listing = comp.layers()
        check('W6/W7 the panel still shows (ordinary window, no layer role)', s.get('panelVisible') is True, s)
        if comp.kind == 'generic':
            unknown('generic compositor: no layer listing; the absence of a layer surface is verified from the GUI state only (layer-state: not a layer window)')
            ls2 = (verb('layer-state') or {}).get('detail') or {}
            check('W6/W7 the panel window is NOT a layer surface in the GUI', (ls2.get('state') or {}).get('isLayer') is False, ls2)
        elif listing is None:
            check('W6 the compositor layer listing was readable (needed to say "no layer surface")', False, 'comp.layers() returned None: the absence of a layer is UNKNOWN, not a pass')
        else:
            check('W6 no layer surface with the panel namespace exists on the X11 backend', not [l for l in listing if l.get('namespace') == NAMESPACE], [l for l in listing if l.get('namespace') == NAMESPACE])
        trigger()
        s = wait_visible(False)
        check('W6 the panel hides through the same trigger', s.get('panelVisible') is False, s)

    # exit: the app's own quit path (E2E control `exit`, which stops the daemon in EXIT_MODE=full) must end the GUI with status 0 within 30 s. A signal death (negative status,
    # e.g. -11 SIGSEGV), any other non-zero status or a hang is a failure; the real status is recorded either way.
    log_text = (out / 'gui.log').read_text(errors='replace')
    result['guiLogMarkers'] = [l for l in log_text.splitlines() if re.search(r'Layer Shell|EGL|Gdk-CRITICAL|Gdk-ERROR|Wayland|Protocol error|segfault|SIGSEGV', l)][:20]
    with (out / 'gui.control').open('a') as f:
        f.write('exit\n')
    ack = step('control-exit', 20)
    try:
        rc = proc.wait(30)
        hung = False
    except subprocess.TimeoutExpired:
        hung = True
        proc.kill()
        rc = proc.wait(10)
    result['guiExit'] = {'status': rc, 'controlAck': bool(ack), 'hungAndKilled': hung}
    time.sleep(3)
    bus.terminate()
    bus.wait(10)
    left = [pid for pid, e in procs().items() if str(out) in e or (mount and e.startswith(mount)) or (conn and pid == conn['pid'])]
    check('the GUI exited through its own quit path with status 0 (not a signal death, not a hang)', ack is not None and not hung and rc == 0, result['guiExit'])
    check('no task-owned process is left after the exit', not left, left)
    if comp.kind == 'generic':
        unknown('generic compositor: no window/layer listing after the exit; exit is verified from the process table and the status only')
    elif comp.kind:
        after = comp.layers()
        check('after the exit the compositor lists no panel layer surface', after is not None and not [l for l in after if l.get('namespace') == NAMESPACE], after)
        after_clients = comp.clients()
        check('after the exit the compositor lists no window of the GUI', after_clients is not None and not [c for c in after_clients if c.get('pid') == proc.pid],
              after_clients if after_clients is None else [c for c in after_clients if c.get('pid') == proc.pid])
    check('no fatal GTK/Wayland/EGL message in the GUI log (Gdk-ERROR, protocol error, EGL abort, SIGSEGV)', not re.search(r'Gdk-ERROR|[Pp]rotocol error|EGL.*[Aa]bort|SIGSEGV|segmentation', log_text), result['guiLogMarkers'])
    result['passed'] = all(c['ok'] for c in result['checks'])
    (out / 'native-wayland-result.json').write_text(json.dumps(result, indent=2) + '\n')
    sys.exit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
