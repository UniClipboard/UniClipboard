#!/usr/bin/env python3
"""Native-host increment N1 of slice 17c12 (docs/architecture/gui-go-linux-appimage-system-proxy.md): the real AppImage on a real Linux desktop session.

  native_proxy_probe.py --appimage X.AppImage --out DIR [--scenario env|gnome-empty|gnome-ignore] [--seconds 25]

Runs on the host under test (Fedora niri/Wayland VM, Omarchy Hyprland/Wayland machine), as the logged-in user, inside a task-owned directory only:
the AppImage is copied there and started in PORTABLE mode (its HOME is <AppImage>.home, so the user's dconf, keyring and application data are never read or
written; the user's own UniClipboard, if any, is not touched). The proxy is a SINK that records the request line of every connection and never forwards
anything: any request the product addresses to it is a leak of local traffic. Checked:
  * the GUI starts on the real Wayland session and the real bundled daemon publishes daemon.conn;
  * the WebKit network process (the process that resolves proxies) maps the bundled GIO modules (loopback guard, GNOME resolver, libproxy) from the AppImage mount;
  * with a proxy configured by GNOME settings (empty ignore-hosts, the 17c12 defect case) or by environment variables, the sink never sees a loopback target.
Not covered here (recorded, not claimed): requests to an external target through the proxy (that is the container runner's job; a native external-target
probe is a later increment), PAC, authentication, dynamic settings. Results are written to DIR/native-result.json; nothing outside DIR is changed.
"""
import argparse
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path


class Sink:
    """A listening socket that logs the first line of every connection and answers 502; it forwards nothing."""

    def __init__(self):
        self.server = socket.socket()
        self.server.bind(('127.0.0.1', 0))
        self.server.listen(64)
        self.port = self.server.getsockname()[1]
        self.lines = []
        threading.Thread(target=self.loop, daemon=True).start()

    def loop(self):
        while True:
            try:
                conn, _ = self.server.accept()
            except OSError:
                return
            threading.Thread(target=self.handle, args=(conn,), daemon=True).start()

    def handle(self, conn):
        try:
            conn.settimeout(3)
            data = conn.recv(4096).split(b'\r\n', 1)[0].decode(errors='replace')
            self.lines.append(re.sub(r'(auth=|token=)[^&\s]+', r'\1<redacted>', data))
            conn.sendall(b'HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n')
        except OSError:
            pass
        finally:
            conn.close()


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


def procs():
    rows = {}
    for p in Path('/proc').iterdir():
        if p.name.isdigit():
            try:
                rows[int(p.name)] = (os.readlink(p / 'exe'), (p / 'comm').read_text().strip())
            except OSError:
                pass
    return rows


def write_gnome(home, port, ignore):
    """GNOME proxy settings in the PORTABLE home's own dconf database (the user's real database is not touched)."""
    k = home / 'keyfile'
    k.mkdir(parents=True, exist_ok=True)
    body = f"[system/proxy]\nmode='manual'\n{ignore}\n[system/proxy/http]\nhost='127.0.0.1'\nport={port}\n\n[system/proxy/https]\nhost='127.0.0.1'\nport={port}\n"
    (k / 'proxy.key').write_text(body)
    (home / '.config' / 'dconf').mkdir(parents=True, exist_ok=True)
    r = subprocess.run(['dconf', 'compile', str(home / '.config' / 'dconf' / 'user'), str(k)], capture_output=True, text=True)
    return r.returncode, r.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--scenario', default='gnome-empty', choices=('env', 'gnome-empty', 'gnome-ignore'))
    ap.add_argument('--seconds', type=int, default=25)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = out / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    uid = os.getuid()
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{uid}')
    wayland = next((p.name for p in runtime.glob('wayland-[0-9]*') if not p.name.endswith('.lock')), None)
    result = {'scenario': args.scenario, 'host': subprocess.run(['uname', '-srm'], capture_output=True, text=True).stdout.strip(), 'waylandSocket': wayland,
              'sessionType': 'wayland' if wayland else 'none', 'checks': [], 'scope': 'native host, real Wayland session, portable mode (task-owned HOME), sink proxy; ARM64'}

    def check(name, ok, detail=None):
        result['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    sink = Sink()
    env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|UC_|UNICLIPBOARD|GIO_|APPIMAGE|APPDIR|GDK_BACKEND', k)}
    env.update(XDG_RUNTIME_DIR=str(runtime), UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
               UC_GUI_GO_E2E_SECRET='native-probe-secret-17c12', UC_GUI_GO_EVIDENCE=str(out / 'gui.jsonl'), UC_GUI_GO_E2E_CONTROL_FILE=str(out / 'gui.control'))
    if wayland:
        env['WAYLAND_DISPLAY'] = wayland
    x11 = sorted(p.name for p in Path('/tmp/.X11-unix').glob('X[0-9]*'))  # the Wails GTK plugin forces GDK_BACKEND=x11 (XWayland on a Wayland session), so the GUI needs the session's DISPLAY
    result['x11Sockets'] = x11
    if x11:
        env['DISPLAY'] = ':' + x11[0][1:]
    bus = runtime / 'bus'
    result['sessionBusSocket'] = bus.exists()
    if bus.exists():  # the user's real session bus: the GUI in portable mode never stores secrets there; the PAC supervisor only talks to it
        env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={bus}'
    fuse = any(Path(d).glob('libfuse.so.2') for d in ('/usr/lib', '/usr/lib64', '/usr/lib/aarch64-linux-gnu', '/lib64'))
    result['fuse2'] = fuse
    if not fuse:  # no FUSE 2 on this host: the AppImage runtime extracts itself instead of mounting (recorded; the mount path differs)
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
    for f in ('gui.jsonl', 'gui.control'):
        (out / f).write_text('')
    made = subprocess.run([str(app), '--appimage-portable-home'], env=env, capture_output=True, text=True, timeout=60)
    home = Path(str(app) + '.home')
    check('portable home created next to the task-owned copy', made.returncode == 0 and home.is_dir(), {'rc': made.returncode, 'err': made.stderr[-200:]})
    if args.scenario == 'env':
        for k in ('http_proxy', 'https_proxy', 'HTTP_PROXY', 'HTTPS_PROXY'):
            env[k] = f'http://127.0.0.1:{sink.port}'
    else:
        ignore = '' if args.scenario == 'gnome-empty' else None
        rc, err = write_gnome(home, sink.port, "ignore-hosts=@as []\n" if ignore == '' else "")
        check('GNOME proxy settings compiled into the portable HOME (distribution dconf)', rc == 0, err)
        env['XDG_CURRENT_DESKTOP'] = 'GNOME'
    log = (out / 'gui.log').open('w')
    proc = subprocess.Popen([str(app)], env=env, cwd=str(out), stdout=log, stderr=subprocess.STDOUT)
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
    if conn:
        time.sleep(args.seconds)  # the page opens its HTTP and WebSocket connections to the daemon
        table = procs()
        exe = table.get(proc.pid, ('', ''))[0]
        mount = exe.split('/usr/bin/')[0] if '/usr/bin/' in exe else None
        result['mount'] = mount
        net = [pid for pid, (e, _) in table.items() if mount and e.startswith(mount) and e.endswith('/WebKitNetworkProcess')]
        check('the WebKit network process is running (the process that resolves proxies)', bool(net), {'mount': mount, 'pids': net})
        if net:
            libs = maps_of(net[0]) or set()
            mods = sorted(p for p in libs if re.search(r'libgio(uniclipboardloopback|gnomeproxy|libproxy)|libdconfsettings', p))
            result['webkitNetworkGioModules'] = mods
            check('the network process maps the bundled loopback guard from the AppImage mount', any('libgiouniclipboardloopback' in p and p.startswith(mount) for p in mods), mods)
            check('the network process maps libglib/libgio/libsoup only from the AppImage mount', all(p.startswith(mount) for p in libs if re.search(r'/(libglib-2|libgio-2|libsoup-3)', p)),
                  sorted(p for p in libs if re.search(r'/(libglib-2|libgio-2|libsoup-3)', p)))
        socks = subprocess.run(['ss', '-tnp'], capture_output=True, text=True).stdout
        web_to_daemon = [l for l in socks.splitlines() if 'WebKitNetwork' in l and f":{conn['port']}" in l]
        result['webkitToDaemonSockets'] = len(web_to_daemon)
        check('the WebView holds established loopback connections to the daemon (it did not go to the proxy)', len(web_to_daemon) >= 1, web_to_daemon[:3])
        before = len(sink.lines)  # detection power: curl, told to use the sink, asking for the daemon's loopback port, MUST appear in the sink
        subprocess.run(['curl', '-s', '-m', '5', '-o', '/dev/null', '--proxy', f'http://127.0.0.1:{sink.port}', f"http://127.0.0.1:{conn['port']}/control-curl"], capture_output=True)
        time.sleep(.5)
        control = [l for l in sink.lines[before:] if 'control-curl' in l]
        check('control: a curl told to use the sink IS recorded with its loopback target (the sink can see a leak)', bool(control), control)
        sink.lines = [l for l in sink.lines if 'control-curl' not in l]
        loop = [l for l in sink.lines if re.search(r'127\.\d+\.\d+\.\d+|localhost|\[?::1\]?', l)]
        result['sinkLines'] = sink.lines
        check('the sink proxy saw NO loopback target of the product', not loop, loop[:5])
        gui_env = {}
        try:
            gui_env = dict(i.split('=', 1) for i in Path(f'/proc/{proc.pid}/environ').read_bytes().decode(errors='replace').split('\0') if '=' in i)
        except OSError:
            pass
        result['guiGioModuleDir'] = gui_env.get('GIO_MODULE_DIR')
    # normal exit through SIGTERM to the task-owned GUI only
    proc.terminate()
    try:
        proc.wait(30)
    except subprocess.TimeoutExpired:
        proc.kill()
    time.sleep(3)
    left = [pid for pid, (e, _) in procs().items() if str(out) in e or (conn and pid == conn['pid'])]
    check('no task-owned process is left after the exit', not left, left)
    result['passed'] = all(c['ok'] for c in result['checks'])
    (out / 'native-result.json').write_text(json.dumps(result, indent=2) + '\n')
    sys.exit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
