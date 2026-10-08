#!/usr/bin/env python3
"""Self-contained AppImage E2E on a host WITHOUT GTK/WebKitGTK (slice 17c4, docs/architecture/gui-go-linux-appimage.md).

Runs inside uc-gui-go-linux-runtime:17c4 (Xvfb, D-Bus client, Mesa/libglvnd, FUSE, the libraries linuxdeploy's exclude list leaves to the
host; no GTK, WebKitGTK, libsoup, cairo, pango). The Secret Service lives in a separate container whose session bus is shared through
a volume (UC_E2E_BUS): the daemon refuses to start without one, and gnome-keyring would put GTK on this host.

  linux_appimage_run.py --mode full     --out DIR --appimage v1.AppImage --uniclip uniclip [--feed DIR]
  linux_appimage_run.py --mode negative --out DIR --appimage NEGCONTROL.AppImage             (must fail to come up)
  linux_appimage_run.py --mode smoke    --out DIR --appimage release.AppImage                (release build: no control plane)

`full` runs the shipped form (tags gtk3,production,release,e2e: release-no-profile, non-portable data root, no UC_PROFILE) from an
AppImage that is the install target:
  1 clean host           no libwebkit2gtk / libgtk-3 in the loader cache
  2 launch               the AppImage mounts (FUSE) and runs; GUI, daemon and WebKit helper processes execute from the mount; the
                         daemon is byte-identical to the build evidence; GTK/WebKit/GLib/GIO are mapped from the mount, not from /usr
  3 handshake            the WebView ran the frontend and reached the daemon (evidence step, shortcut state)
  4 data root            non-portable XDG data root, no profile suffix, nothing written next to the AppImage
  5 autostart            Exec= is the AppImage file (not the temporary mount), a legacy entry is replaced, disable removes it
  6 update-bad           untrusted signature: download rejected, AppImage bytes unchanged
  7 update-good          verified download, file replaced, restart from the new file, old daemon stopped, new image mounted
Not proven: real desktop, GPU, Wayland (GDK_BACKEND=x11 is forced by the plugin hook), official signing key, real feed server.
"""
import argparse
import hashlib
import http.server
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_xvfb_run import read_steps  # noqa: E402


def pid_alive(pid):
    """Running, not a zombie (a reaped-by-nobody child still answers kill 0)."""
    try:
        state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
    except OSError:
        return False
    return state != 'Z'

PASSPHRASE = 'appimage-e2e-passphrase'
VERSION = '99.0.0-e2e'
DISPLAY = ':99'
PLATFORM_KEY = {'aarch64': 'linux-aarch64', 'x86_64': 'linux-x86_64'}


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def procs():
    """pid -> (exe, comm) for every process this container can see."""
    rows = {}
    for d in Path('/proc').iterdir():
        if d.name.isdigit():
            try:
                rows[int(d.name)] = (os.readlink(d / 'exe'), (d / 'comm').read_text().strip())
            except OSError:
                pass
    return rows


def daemon_status(home, pid):
    """Is the daemon running? /proc state AND the HTTP health endpoint (kill 0 alone answers for a zombie)."""
    state = None
    try:
        state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
    except OSError:
        pass
    health = None
    for conn in home.rglob('daemon.conn'):
        try:
            c = json.loads(conn.read_text())
            if c.get('pid') != pid:
                continue
            with urllib.request.urlopen(f"http://{c['host']}:{c['port']}/health", timeout=3) as r:
                health = r.status
        except (OSError, ValueError, KeyError):
            health = 'unreachable'
    return {'pid': pid, 'procState': state, 'health': health, 'running': state not in (None, 'Z') or health == 200}


def maps_of(pid):
    libs = set()
    try:
        for line in Path(f'/proc/{pid}/maps').read_text().splitlines():
            parts = line.split(None, 5)
            if len(parts) == 6 and '.so' in parts[5]:
                libs.add(parts[5].replace(' (deleted)', ''))
    except OSError:
        pass
    return libs


def serve(directory):
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(directory), **k)  # noqa: E731
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
    handler.log_message = lambda *a: None
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


class Run:
    def __init__(self, out, appimage, env):
        self.out, self.appimage, self.env = out, appimage, env
        self.results = {'checks': [], 'passed': False}

    def check(self, name, ok, detail=None):
        self.results['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)
        return ok

    def launch(self, tag, extra_env=None):
        evidence, control = self.out / f'{tag}.jsonl', self.out / f'{tag}.control'
        for f in (evidence, control):
            f.write_text('')
        env = dict(self.env, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control), **(extra_env or {}))
        proc = subprocess.Popen([str(self.appimage)], env=env, cwd=str(self.appimage.parent), stdout=(self.out / f'{tag}.log').open('w'),
                                stderr=subprocess.STDOUT)
        return Launch(proc, evidence, control, self.out / f'{tag}.log')


class Launch:
    def __init__(self, proc, evidence, control, log):
        self.proc, self.evidence, self.control, self.log = proc, evidence, control, log

    def step(self, name, timeout=90, allow_exit=False):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(self.evidence) if r['step'] == name]
            if rows:
                return rows[-1]
            for r in read_steps(self.evidence):
                if r['step'] in ('driver-error', 'update-driver-error'):
                    raise RuntimeError(f"driver error: {r.get('detail')}")
            if not allow_exit and self.proc.poll() is not None:
                raise RuntimeError(f'GUI exited ({self.proc.returncode}) before {name}')
            time.sleep(.2)
        raise RuntimeError(f'timeout waiting for {name}')

    def ctl(self, line, label, timeout=60):
        with self.control.open('a') as f:
            f.write(line + '\n')
        return self.step(label, timeout)

    def invoke(self, label, command, args=None):
        return self.ctl(f'invoke {label} {command} {json.dumps(args) if args is not None else ""}', f'invoke-{label}')['detail']


def descendants_of_mount(mount_marker):
    """Processes whose executable lives under a mounted/extracted AppImage (`/tmp/.mount_*` or `/tmp/appimage_extracted_*`)."""
    return {pid: v for pid, v in procs().items() if mount_marker in v[0]}


def start_xvfb(out):
    log = (out / 'xvfb.log').open('w')
    proc = subprocess.Popen(['Xvfb', DISPLAY, '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], stdout=log, stderr=subprocess.STDOUT)
    for _ in range(50):
        if subprocess.run(['xdpyinfo', '-display', DISPLAY], capture_output=True).returncode == 0:
            return proc
        time.sleep(.2)
    raise RuntimeError('Xvfb did not start')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--mode', choices=('full', 'negative', 'smoke'), default='full')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--uniclip', type=Path)
    parser.add_argument('--feed', type=Path, help='directory with update.AppImage.tar.gz, pubkey.b64, good.sig.b64, bad.sig.b64 and v2.sha256')
    parser.add_argument('--manifest', type=Path, help='package-manifest.json of the AppImage under test (daemon SHA-256)')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-appimage-'))
    home, runtime_dir, install = sandbox / 'home', sandbox / 'run', sandbox / 'install'
    for d in (home, runtime_dir, install):
        d.mkdir(mode=0o700)
    target = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, target)
    original_sha = sha256(target)
    env = dict(os.environ, HOME=str(home), XDG_RUNTIME_DIR=str(runtime_dir), XDG_CONFIG_HOME=str(home / '.config'), DISPLAY=DISPLAY,
               UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake',
               UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE)
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND'):
        env.pop(key, None)
    if os.environ.get('UC_E2E_BUS'):
        env['DBUS_SESSION_BUS_ADDRESS'] = os.environ['UC_E2E_BUS']
    run = Run(out, target, env)
    r = run.results
    r.update({'mode': args.mode, 'appimageSha256': original_sha, 'sandbox': str(sandbox),
              'scope': 'container with Xvfb, no GTK/WebKitGTK on the host, Secret Service in a sibling container; no desktop, no GPU, no Wayland'})
    xvfb = start_xvfb(out)
    launches = []
    try:
        loader = subprocess.run(['ldconfig', '-p'], capture_output=True, text=True).stdout
        bad = [l for l in loader.splitlines() if 'libwebkit2gtk' in l or 'libgtk-3' in l or 'libjavascriptcoregtk' in l]
        run.check('1 clean host: no libwebkit2gtk / libgtk-3 / libjavascriptcoregtk in the loader cache',
                  not bad and not list(Path('/usr/lib').glob('*/webkit2gtk-4.1')), bad)
        r['hostGLib'] = [l.strip() for l in loader.splitlines() if 'libglib-2.0' in l or 'libgio-2.0' in l]
        if args.mode == 'negative':
            negative(run, launches)
        elif args.mode == 'smoke':
            smoke(run, launches, home)
        else:
            full(run, launches, args, sandbox, home, target, original_sha)
        r['passed'] = all(c['ok'] for c in r['checks'])
    except Exception as e:  # keep the evidence of a failed run
        r['error'] = repr(e)
        print('ERROR', repr(e), flush=True)
    finally:
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        # Only daemons recorded in this sandbox's own daemon.conn.
        for conn in home.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, signal.SIGTERM)
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(1)
        for sub in ('.local/state', '.local/share'):
            for logs in (home / sub).glob('app.uniclipboard.desktop*'):
                copy_logs(logs, out / 'home-copy' / sub.replace('/', '_') / logs.name)
        xvfb.terminate()
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
    print(json.dumps({'passed': r['passed'], 'mode': args.mode}))
    sys.exit(0 if r['passed'] else 1)


def copy_logs(root, dest):
    """Copy only the log files of a sandbox profile: never the databases, identity, keys or connection files. A file that vanishes while the
    daemon shuts down is skipped, so evidence collection cannot fail the run after its assertions were made."""
    for f in sorted(Path(root).rglob('*')):
        if not f.is_file() or f.is_symlink() or not (f.name.endswith(('.log', '.jsonl')) or '.json.' in f.name or f.name.endswith('.json') and f.parent.name == 'logs'):
            continue
        target = Path(dest) / f.relative_to(root)
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(f, target)
        except OSError:
            pass


def wait_daemon(home, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for conn in home.rglob('daemon.conn'):
            try:
                return conn, json.loads(conn.read_text())['pid']
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(.3)
    return None, None


def inspect_processes(run, label, launch_pid, expected_daemon_sha, daemon_pid):
    table = procs()
    gui_exe = table.get(launch_pid, ('', ''))[0]
    marker = next((m for m in ('/tmp/.mount_', '/tmp/appimage_extracted_') if gui_exe.startswith(m)), None)
    mount = gui_exe.split('/usr/bin/')[0] if marker else None
    under = descendants_of_mount(mount) if mount else {}
    names = sorted(v[0].rsplit('/', 1)[-1] for v in under.values())
    helpers = [pid for pid, v in under.items() if v[0].endswith(('WebKitWebProcess', 'WebKitNetworkProcess'))]
    run.check(f'{label} the GUI executes from the mounted AppImage ({gui_exe})', bool(marker) and gui_exe.endswith('/usr/bin/uniclipboard'), gui_exe)
    run.check(f'{label} WebKitWebProcess and WebKitNetworkProcess execute from the same mount (not from /usr/lib)',
              {'WebKitWebProcess', 'WebKitNetworkProcess'} <= set(names), names)
    daemon_exe = table.get(daemon_pid, ('', ''))[0]
    run.check(f'{label} the daemon executes from the mount and is byte-identical to the build evidence',
              bool(mount) and daemon_exe.startswith(mount) and expected_daemon_sha is not None and sha256(f'/proc/{daemon_pid}/exe') == expected_daemon_sha,
              {'exe': daemon_exe, 'expected': expected_daemon_sha})
    mapped = {}
    for pid in [launch_pid] + helpers[:2]:
        libs = maps_of(pid)
        picked = {k: sorted(p for p in libs if k in p.rsplit('/', 1)[-1]) for k in ('libglib-2.0', 'libgio-2.0', 'libgtk-3', 'libwebkit2gtk-4.1', 'libjavascriptcoregtk-4.1')}
        mapped[str(pid)] = picked
    ok = bool(mount) and all(paths and all(p.startswith(mount) for p in paths) for libs in mapped.values() for paths in libs.values()
                             if True) if mapped else False
    run.check(f'{label} GTK, WebKitGTK, JavaScriptCore, GLib and GIO are mapped from the mount in the GUI and the WebKit helpers (/proc/<pid>/maps)',
              ok, mapped)
    return mount


def wait_panel_ready(launch, label, timeout=60):
    """The quick panel page is preloaded after the main page; poll the shortcut state until it reports ready (a late answer is
    recorded with its delay, a missing one stays a failure)."""
    start, n, state = time.monotonic(), 0, {}
    while time.monotonic() - start < timeout:
        n += 1
        state = launch.ctl(f'shortcut-state {label}-{n}', f'shortcut-state-{label}-{n}')['detail']
        if state.get('panelReady') is True:
            break
        time.sleep(.5)
    state['waitedSeconds'] = round(time.monotonic() - start, 1)
    return state


def cli(run, args, *cmd):
    try:
        r = subprocess.run([str(args.uniclip), '--json', *cmd], env=run.env, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired as e:  # keep what the CLI printed: a hang is a finding, not an exception that ends the run
        return {'rc': None, 'timeout': 60, 'stdout': (e.stdout or b'').decode(errors='replace').strip() if isinstance(e.stdout, bytes) else (e.stdout or '').strip(),
                'stderr': (e.stderr or b'').decode(errors='replace')[-500:] if isinstance(e.stderr, bytes) else (e.stderr or '')[-500:]}
    return {'rc': r.returncode, 'stdout': r.stdout.strip(), 'stderr': r.stderr.strip()[-500:]}


# Lifecycle state, not user data: the daemon writes the start marker at boot and clears it on a clean shutdown
# (crates/uc-daemon-local/src/crash_marker.rs), `daemon.conn` is its connection record.
RUNTIME_STATE_FILES = ('daemon.conn', 'daemon-startup.conn', '.daemon-pid', '.uniclipd.lock', 'daemon-run.json', 'daemon-run.json.tmp')


def volatile(key):
    return bool(re.search(r'(?i)(time|pid|port|started|token)|(_at|At|Ms)$', key))


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if not volatile(k)}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def full(run, launches, args, sandbox, home, target, original_sha):
    out = run.out
    manifest = json.loads(args.manifest.read_text()) if args.manifest else {}
    daemon_sha = manifest.get('daemon', {}).get('sha256')
    gui = run.launch('gui1')
    launches.append(gui)
    conn, daemon_pid = wait_daemon(home)
    run.check('2 the bundled daemon started and published daemon.conn', conn is not None, str(conn))
    if conn is None:
        return
    subprocess.run([str(args.uniclip), 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'appimage-e2e'], env=run.env, check=True, timeout=120,
                   stdout=(out / 'space-init.log').open('w'), stderr=subprocess.STDOUT)
    boot = gui.step('bootstrapped', 120)
    run.check('3 the WebView ran the frontend (evidence step `bootstrapped` is written by the page, through the host service)', boot['ok'], boot)
    mount = inspect_processes(run, '2', gui.proc.pid, daemon_sha, daemon_pid)
    state = wait_panel_ready(gui, 'boot')
    run.check('3 the frontend reached the daemon: the panel reports ready', state.get('panelReady') is True, state)

    # User data written through the real daemon API before the update; read back after it (check 8).
    default = gui.invoke('ad0', 'get_auto_download_update')
    flipped = not default['data']
    written = gui.ctl(f"setting autoDownloadUpdate {'on' if flipped else 'off'}", 'control-setting')
    readback = gui.invoke('ad1', 'get_auto_download_update')
    run.check('8 a user setting (general.autoDownloadUpdate, flipped from its default) is written through the daemon API and reads back',
              written['ok'] and readback['data'] == flipped and flipped != default['data'], [default, written, readback])
    space_before = cli(run, args, 'space', 'status')
    run.results['spaceStatusBefore'] = space_before
    run.check('8 `uniclip space status` works against the bundled daemon (encrypted space, keyring unlocked)', space_before['rc'] == 0 and space_before['stdout'], space_before)

    data_root = home / '.local/share/app.uniclipboard.desktop'
    run.check('4 release-no-profile, non-portable data root: ~/.local/share/app.uniclipboard.desktop holds daemon.conn, no profile suffix',
              (data_root / 'daemon.conn').exists() and not list(home.glob('.local/share/app.uniclipboard.desktop-*')), str(data_root))
    run.check('4 nothing was written next to the AppImage (no portable data directory, no extra files)',
              sorted(p.name for p in target.parent.iterdir()) == ['UniClipboard.AppImage'], sorted(p.name for p in target.parent.iterdir()))

    entry = home / '.config/autostart/UniClipboard.desktop'
    entry.parent.mkdir(parents=True, exist_ok=True)
    entry.write_text('[Desktop Entry]\nType=Application\nName=UniClipboard\nExec=/opt/legacy/UniClipboard_1.0.0_amd64.AppImage\nTerminal=false\n')
    res = gui.invoke('as-on', 'update_autostart', {'enabled': True})
    body = entry.read_text() if entry.exists() else ''
    exec_line = next((l for l in body.splitlines() if l.startswith('Exec=')), '')
    run.check('5 autostart replaces the legacy entry: Exec= is the AppImage file (stable path outside the mount) and carries --autostart',
              res.get('ok') and str(target) in exec_line and '--autostart' in exec_line and '.mount_' not in body and 'legacy' not in body, [res, body])
    res = gui.invoke('as-off', 'update_autostart', {'enabled': False})
    run.check('5 disabling autostart removes the entry', res.get('ok') and not entry.exists(), res)
    gui.ctl('exit', 'control-exit')
    code = gui.proc.wait(timeout=60)
    deadline = time.monotonic() + 20
    while daemon_status(home, daemon_pid)['running'] and time.monotonic() < deadline:
        time.sleep(.3)
    status = daemon_status(home, daemon_pid)
    run.check('2 GUI exit 0 and the bundled daemon stopped by the GUI (no running process state, health endpoint not answering)',
              code == 0 and not status['running'], {'exit': code, 'daemon': status})
    # The runtime unmounts after the child exits; the FUSE teardown is asynchronous, so wait for it instead of sampling once.
    deadline = time.monotonic() + 20
    while mount is not None and Path(mount).exists() and time.monotonic() < deadline:
        time.sleep(.3)
    run.check('2 the mount is gone after exit', mount is None or not Path(mount).exists(), mount)

    if not args.feed:
        run.check('6/7 update scenarios were not run (no --feed)', False)
        return
    feed = args.feed
    pubkey = (feed / 'pubkey.b64').read_text()
    v2_sha = (feed / 'v2.sha256').read_text().split()[0]
    server = serve(feed)
    base = f'http://127.0.0.1:{server.server_address[1]}'
    try:
        for name, sig in (('good', 'good.sig.b64'), ('bad', 'bad.sig.b64')):
            (feed / f'{name}.json').write_text(json.dumps({
                'version': VERSION, 'notes': 'E2E update notes', 'pub_date': '2026-10-06T00:00:00Z',
                'platforms': {PLATFORM_KEY[os.uname().machine]: {'url': f'{base}/update.AppImage.tar.gz', 'signature': (feed / sig).read_text()}}}))
        # 6: untrusted signature
        bad = run.launch('update-bad', {'UC_GUI_GO_E2E_PHASE': 'update-bad', 'UC_UPDATE_ENDPOINT': f'{base}/bad.json', 'UC_UPDATE_PUBKEY': pubkey})
        launches.append(bad)
        bad.step('update-check', 120)
        row = bad.step('update-download-rejected', 120)
        run.check('6 an artifact signed by an untrusted key is rejected, naming the signature', row['ok'] and 'signature' in json.dumps(row['detail']).lower(), row)
        run.check('6 the AppImage file is byte-identical after the rejected update', sha256(target) == original_sha)
        try:
            bad.proc.wait(timeout=90)
        except subprocess.TimeoutExpired:
            bad.proc.terminate()
        # 7: trusted signature, real replacement and restart
        good = run.launch('update-good', {'UC_GUI_GO_E2E_PHASE': 'update-good', 'UC_UPDATE_ENDPOINT': f'{base}/good.json', 'UC_UPDATE_PUBKEY': pubkey})
        launches.append(good)
        first = good.step('update-state', 120)
        first_pid = first['detail']['pid']
        run.check('7 the first process runs the v1 image (no update marker)', first['detail']['installed'] is False, first)
        conn, old_daemon = wait_daemon(home)
        data_before = sorted(str(p.relative_to(home / '.local/share')) for p in (home / '.local/share/app.uniclipboard.desktop').rglob('*') if p.is_file()
                             and p.name not in RUNTIME_STATE_FILES
                             and not p.name.endswith(('-wal', '-shm')))  # SQLite sidecars vanish at a clean close after the checkpoint; the database file stays compared
        # The first process hands over to the replaced file and exits, so from here the evidence file is read without it.
        good.step('update-relaunched', 240, allow_exit=True)
        states = [x for x in read_steps(good.evidence) if x['step'] == 'update-state']
        run.check('7 the AppImage file now holds the v2 bytes (SHA-256 equals the signed artifact)', sha256(target) == v2_sha, {'v2': v2_sha, 'file': sha256(target)})
        run.check('7 the restarted process is a new process running the v2 image (the marker exists in its mount)',
                  len(states) == 2 and states[1]['detail']['installed'] and states[1]['detail']['pid'] != first_pid, states)
        try:
            good.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            pass
        deadline = time.monotonic() + 40
        while pid_alive(states[-1]['detail']['pid']) and time.monotonic() < deadline:
            time.sleep(.5)
        old_status = daemon_status(home, old_daemon) if old_daemon else None
        run.check('7 the old daemon is gone after the update (not running by /proc state, health endpoint silent)', old_status is not None and not old_status['running'], old_status)
        boots = [x for x in read_steps(good.evidence) if x['step'] == 'bootstrapped']
        run.check('7 the restarted process bootstrapped its page against the existing profile (a second `bootstrapped` in the same evidence file)', len(boots) == 2, boots)
        data_after = sorted(str(p.relative_to(home / '.local/share')) for p in (home / '.local/share/app.uniclipboard.desktop').rglob('*') if p.is_file())
        run.check('7 persisted user data files survived the update (lifecycle state files excluded: they are rewritten at daemon start)',
                  set(data_before) <= set(data_after), sorted(set(data_before) - set(data_after)))
        # A fresh launch of the replaced AppImage: same profile, new daemon, user data readable through the real API.
        post = run.launch('post-update')
        launches.append(post)
        pconn, pdaemon = wait_daemon(home)
        run.check('8 the replaced AppImage starts a daemon on the existing data root', pconn is not None and pdaemon not in (None, old_daemon), [str(pconn), pdaemon, old_daemon])
        post.step('bootstrapped', 120)
        pstate = wait_panel_ready(post, 'post')
        run.check('8 the v2 page reached the daemon (panel ready)', pstate.get('panelReady') is True, pstate)
        pmount = inspect_processes(run, '8', post.proc.pid, daemon_sha, pdaemon)
        run.check('8 the running image is v2 (marker file in its mount)', pmount is not None and (Path(pmount) / 'usr/share/uniclipboard/update-marker.txt').exists(), pmount)
        after = post.invoke('ad2', 'get_auto_download_update')
        run.check('8 the user setting written before the update reads back through the new daemon', after['data'] == flipped, after)
        space_after = cli(run, args, 'space', 'status')
        run.results['spaceStatusAfter'] = space_after
        try:
            same = stable(json.loads(space_before['stdout'])) == stable(json.loads(space_after['stdout']))
        except ValueError:
            same = space_before['stdout'] == space_after['stdout']
        run.check('8 the encrypted space is still initialised and unlocked after the update, with the same stable status fields', space_after['rc'] == 0 and same,
                  [stable(json.loads(space_before['stdout'])) if space_before['stdout'].startswith('{') else space_before['stdout'], space_after['stdout'][:600]])
        post.ctl('exit', 'control-exit')
        post.proc.wait(timeout=60)
        run.check('7 the relaunched process exited at the end of its scenario', not pid_alive(states[-1]['detail']['pid']))
    finally:
        server.shutdown()


def negative(run, launches):
    """The negative control: an AppImage without the WebKit helper relocation must NOT come up on this host."""
    gui = run.launch('negative')
    launches.append(gui)
    try:
        step = gui.step('bootstrapped', 75)
        run.check('N the negative-control package unexpectedly reached the page', False, step)
    except RuntimeError as e:
        time.sleep(1)
        log = gui.log.read_text(errors='replace')
        cause = [l for l in log.splitlines() if 'Failed to spawn child process' in l and 'webkit2gtk-4.1/WebKit' in l and 'No such file or directory' in l]
        run.check('N the package without helper relocation never reaches the page, and the log names the cause: the host helper path does not exist',
                  bool(cause), {'wait': str(e), 'cause': cause[:1]})
        run.results['negativeLogTail'] = log[-3000:]


def smoke(run, launches, home):
    """Release build (no control plane): window, daemon and helpers exist and the process keeps running. NOT a frontend handshake."""
    gui = run.launch('smoke', {'UC_GUI_GO_E2E_PHASE': ''})
    launches.append(gui)
    conn, daemon_pid = wait_daemon(home)
    run.check('S the bundled daemon of the release build published daemon.conn', conn is not None, str(conn))
    time.sleep(45)
    table = procs()
    names = sorted(v[0].rsplit('/', 1)[-1] for v in table.values())
    run.check('S the release GUI keeps running for 45 s with WebKit helpers started from the mount',
              gui.proc.poll() is None and 'WebKitWebProcess' in names and 'WebKitNetworkProcess' in names, names)
    windows = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '.'], env=dict(os.environ, DISPLAY=DISPLAY), capture_output=True, text=True).stdout.split()
    run.check('S an X window is mapped', bool(windows), windows)
    log = gui.log.read_text(errors='replace')
    fatal = [l for l in log.splitlines() if any(k in l for k in ('SIGABRT', 'SIGSEGV', 'fatal error', 'cannot open shared object', "Couldn't open lib"))]
    run.check('S no loader or fatal error in the log', not fatal, fatal)


if __name__ == '__main__':
    main()
