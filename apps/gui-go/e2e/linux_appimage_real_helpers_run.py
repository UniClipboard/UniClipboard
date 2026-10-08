#!/usr/bin/env python3
"""Real browser / file manager / image viewer E2E of the self-contained AppImage, NON-portable (slice 17c11,
docs/architecture/gui-go-linux-appimage-real-helpers.md).

  linux_appimage_real_helpers_run.py --out DIR --appimage X.AppImage --manifest package-manifest.json --desktop generic|gnome [--f7]

Runs in a distribution image that has the distribution's OWN applications (Ubuntu: Epiphany, Nautilus, Loupe; Fedora: Firefox, Nautilus, Loupe) and the
Secret Service in the same user session. The GUI runs as an unprivileged user with the real HOME (non-portable, release-no-profile data root), under
`strace -f -u uc` (execve/clone only; library origin comes from /proc/<pid>/maps of the LIVE applications). The shared frontend starts the product
actions (open logs / data directory, reveal, open image externally, open URL); what is asserted is what the real applications did: the controlled HTTP
server saw the browser's request, a window with the target's name exists, the application processes map no library from the AppImage mount and carry
no variable that points into it. Default applications are the packages' own; the "user default" scenario is written with the host's `xdg-mime default`
into the user's real ~/.config/mimeapps.list. Nothing is registered system-wide.
Not proven: real desktop session, portals, Wayland, GPU, native amd64.
"""
import argparse
import base64
import http.server
import json
import os
import pwd
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_appimage_portable_run import USER, as_user, environ_of, wait_daemon  # noqa: E402
from linux_appimage_run import DISPLAY, PASSPHRASE, Launch, copy_logs, Run, maps_of, pid_alive, procs, sha256, start_xvfb, wait_panel_ready  # noqa: E402
from linux_appimage_tls_run import Reports, StopScenario  # noqa: E402
from linux_appimage_helpers_run import png_bytes, parse_trace, tree_of  # noqa: E402

MIMES = ('inode/directory', 'image/png', 'x-scheme-handler/http', 'x-scheme-handler/https')
APP_EXES = {'epiphany', 'firefox', 'firefox-bin', 'nautilus', 'loupe', 'WebKitWebProcess', 'WebKitNetworkProcess', 'WebKitGPUProcess', 'plugin-container', 'glxtest'}
BUS_DIR = Path('/bus')


class StraceRun(Run):
    """GUI under strace run as ROOT with -u uc (a non-root tracer breaks fusermount's setuid, 17c10 attempt1); execve and process creation only."""

    def launch(self, tag, extra_env=None, args=()):
        evidence, control, trace = self.out / f'{tag}.jsonl', self.out / f'{tag}.control', self.out / f'{tag}.strace'
        for f in (evidence, control, trace):
            f.write_text('')
            f.chmod(0o666)
        env = dict(self.env, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control), **(extra_env or {}))
        log = self.out / f'{tag}.log'
        log.write_text('')
        log.chmod(0o666)
        cmd = ['strace', '-u', USER, '-f', '-q', '-v', '-s', '16384', '-e', 'trace=execve,clone,clone3,fork,vfork', '-o', str(trace), str(self.appimage), *args]
        proc = subprocess.Popen(cmd, env=env, cwd=str(Path(self.appimage).parent), stdout=log.open('a'), stderr=subprocess.STDOUT)
        self.trace = trace
        return Launch(proc, evidence, control, log)


class Target(http.server.ThreadingHTTPServer):
    """Controlled HTTP target on loopback: records every request (path, User-Agent) and answers a page whose <title> carries the nonce."""
    daemon_threads = True

    def __init__(self):
        outer = self
        self.requests = []

        class H(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                outer.requests.append({'t': time.time(), 'path': self.path, 'ua': self.headers.get('User-Agent', ''), 'host': self.headers.get('Host', '')})
                body = b''
                if self.path.startswith('/uc11-'):
                    n = self.path[1:].split('?')[0]
                    body = f'<!doctype html><html><head><title>{n}</title></head><body><h1>{n}</h1></body></html>'.encode()
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/html; charset=utf-8')
                else:
                    self.send_response(404)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        super().__init__(('127.0.0.1', 0), H)
        self.port = self.server_address[1]
        threading.Thread(target=self.serve_forever, daemon=True).start()

    def hits(self, path):
        return [r for r in self.requests if r['path'] == path]


def children_exes(pid):
    """Executables of the direct children of `pid` (root reads /proc/<pid>/task/*/children): the application a foreground xdg-open is waiting for."""
    exes = []
    try:
        for task in Path(f'/proc/{pid}/task').iterdir():
            for c in (task / 'children').read_text().split():
                try:
                    exes.append(os.readlink(f'/proc/{c}/exe'))
                except OSError:
                    pass
    except OSError:
        pass
    return exes


def open_as_user(out, label, target, env):
    """Start the host's own xdg-open as the user and do NOT wait for it. xdg-open waits for the application it starts when it runs it directly (the generic
    dispatch runs e.g. `nautilus --new-window` as its foreground child and returns only when that first instance exits; `gio open` under GNOME returns at once and
    the application is a D-Bus service: diag_xdg_open_generic.sh, artifacts xdg-open-generic-diag-ubuntu-v2). A blocking subprocess.run therefore turned a healthy
    generic dispatch into a TimeoutExpired (final-b3aca3d11/real-*-generic, kept). Output goes to files, not pipes. The contract is the EFFECT (window / request); the
    exit status of xdg-open is checked only if it has exited by then (must be 0), otherwise it is recorded as `running` with the executables of its children."""
    o, e = out / f'host-{label}.stdout', out / f'host-{label}.stderr'
    fo, fe = o.open('w'), e.open('w')
    proc = subprocess.Popen(['xdg-open', target], env=env, user=USER, group=USER, extra_groups=[], stdout=fo, stderr=fe)
    return proc


def host_state(proc, err_file):
    rc = proc.poll()
    return {'rc': rc, 'status': 'running' if rc is None else f'exited {rc}', 'childExes': children_exes(proc.pid) if rc is None else [],
            'stderrTail': Path(err_file).read_text(errors='replace')[-300:]}


def window_titles(env):
    out = subprocess.run(['xwininfo', '-root', '-tree', '-display', DISPLAY], capture_output=True, text=True, env=env).stdout
    return [m.group(1) for m in re.finditer(r'^\s+0x[0-9a-f]+ "([^"]+)"', out, re.M)]


def wait_window(env, fragment, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        titles = [t for t in window_titles(env) if fragment in t]
        if titles:
            return titles
        time.sleep(.4)
    return []


def uid_of(pid):
    try:
        for line in Path(f'/proc/{pid}/status').read_text().splitlines():
            if line.startswith('Uid:'):
                return int(line.split()[1])
    except OSError:
        pass
    return None


def cmdline_of(pid):
    try:
        return [a for a in Path(f'/proc/{pid}/cmdline').read_bytes().decode(errors='replace').split('\0') if a]
    except OSError:
        return []


def app_processes(uid, mount=None):
    """Live processes of the user's real applications (not the GUI's own mount)."""
    rows = []
    for pid, (exe, comm) in procs().items():
        if uid_of(pid) != uid or (mount and exe.startswith(mount)):
            continue
        if exe.rsplit('/', 1)[-1] in APP_EXES:
            rows.append(pid)
    return rows


def inspect(pid, mount):
    """One live application process: executable, environment and mapped libraries, each READ HERE (not through the swallowing helpers of the other runners). `status` is
    `ok`, `exited` (the process is gone: excluded from the G2 verdict, recorded) or `unreadable` (alive but exe/environ/maps could not be read, errno recorded: the G2 checks
    FAIL on it, an unread process is never evidence of "no AppImage library"). Reads are retried briefly: a process mid-exec is transiently unreadable."""
    row = {'pid': pid, 'status': 'ok', 'errno': None}
    for attempt in range(10):
        try:
            exe = os.readlink(f'/proc/{pid}/exe')
            env = dict(item.split('=', 1) for item in Path(f'/proc/{pid}/environ').read_bytes().decode(errors='replace').split('\0') if '=' in item)
            libs = sorted({line.split(None, 5)[5].replace(' (deleted)', '') for line in Path(f'/proc/{pid}/maps').read_text().splitlines()
                           if len(line.split(None, 5)) == 6 and '.so' in line.split(None, 5)[5]})
            break
        except OSError as e:
            row['errno'] = f'{e.errno} {e.strerror}'
            if not pid_alive(pid):
                row['status'] = 'exited'
                break
            time.sleep(.2)
    else:
        row['status'] = 'unreadable'
    if row['status'] != 'ok':
        return {**row, 'exe': '', 'cmdline': [], 'envPointingIntoMount': [], 'ldLibraryPath': None, 'gdkBackend': None, 'gtkTheme': None, 'libsMapped': 0, 'mountLibs': []}
    return {**row, 'exe': exe, 'cmdline': cmdline_of(pid)[:6], 'envPointingIntoMount': sorted(k for k, v in env.items() if mount in v),
            'ldLibraryPath': env.get('LD_LIBRARY_PATH'), 'gdkBackend': env.get('GDK_BACKEND'), 'gtkTheme': env.get('GTK_THEME'), 'libsMapped': len(libs),
            'mountLibs': [l for l in libs if l.startswith(mount)]}


def kill_apps(uid):
    for pid in app_processes(uid):
        try:
            os.kill(pid, 9)
        except OSError:
            pass
    time.sleep(1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--desktop', choices=('generic', 'gnome'), required=True)
    parser.add_argument('--f7', action='store_true', help='after the main scenario, reproduce F7 (portable HOME hides the user default application) with the real applications: recorded, not asserted')
    parser.add_argument('--browser-exe', default='epiphany', help='executable name of the distribution browser (UA and process checks)')
    parser.add_argument('--browser-ua', default='AppleWebKit', help="substring of the browser User-Agent (Epiphany sends a Safari-compatible string without its name; the process check names the browser)")
    parser.add_argument('--browser-desktop', default='org.gnome.Epiphany.desktop', help='desktop entry of that browser (user default scenario)')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    home = Path(account.pw_dir)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-real-'))
    runtime_dir, install, fixtures = sandbox / 'run', sandbox / 'install', sandbox / 'fixtures'
    for d in (sandbox, runtime_dir, install, fixtures, BUS_DIR):
        d.mkdir(exist_ok=True)
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    target = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, target)
    os.chown(target, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=str(home), XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
               XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE,
               USER=USER, LOGNAME=USER, DBUS_SESSION_BUS_ADDRESS=f'unix:path={BUS_DIR}/bus')
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME', 'UC_E2E_BUS', 'GIO_MODULE_DIR',
                'GIO_EXTRA_MODULES', 'XDG_CURRENT_DESKTOP', 'BROWSER', 'GNOME_DESKTOP_SESSION_ID', 'UC_GUI_GO_E2E_OPEN_LOG'):
        env.pop(key, None)
    if args.desktop == 'gnome':
        env['XDG_CURRENT_DESKTOP'] = 'GNOME'
    run = StraceRun(out, target, env)
    r = run.results
    osrel = dict(l.split('=', 1) for l in Path('/etc/os-release').read_text().splitlines() if '=' in l)
    r.update({'mode': f'real-helpers-{args.desktop}', 'appimageSha256': sha256(target), 'user': USER, 'distribution': osrel.get('PRETTY_NAME', '').strip('"'),
              'kernelMachine': os.uname().machine, 'xdgCurrentDesktop': env.get('XDG_CURRENT_DESKTOP'), 'nonPortable': True, 'home': str(home),
              'scope': 'container (no network), unprivileged user, Xvfb, real distribution browser / file manager / image viewer, non-portable real HOME, no desktop session, no portal, no Wayland, no GPU'})
    xvfb = start_xvfb(out)
    launches, bus = [], None
    try:
        route = subprocess.run('ip route show default; [ -n "$(ip route show default)" ] || ip route add default dev eth0; ip route show default', shell=True, capture_output=True, text=True)
        r['defaultRoute'] = route.stdout + route.stderr
        run.check('T0 fixture: the container has a default route (the Engine fails its p2p bind without one, dev4; the network is internal: no route out)', 'default' in route.stdout, r['defaultRoute'])
        # --- the user session's D-Bus + Secret Service (same container as the applications: they share one session bus)
        bus = subprocess.Popen(['sh', '/work/apps/gui-go/e2e/linux/keyring_service.sh'], env=dict(env, PATH=os.environ['PATH']), user=USER, group=USER, extra_groups=[],
                               stdout=(out / 'keyring-service.log').open('w'), stderr=subprocess.STDOUT)
        for _ in range(150):
            if (BUS_DIR / 'ready').exists():
                break
            time.sleep(.2)
        run.check('T0 user session bus and unlocked Secret Service are ready (store and lookup through the Secret Service API)', (BUS_DIR / 'ready').exists(), None)
        if not (BUS_DIR / 'ready').exists():
            raise StopScenario()
        (BUS_DIR / 'bus').chmod(0o777)

        # --- which applications the package defaults pick (no fixture registers anything)
        def query(label, extra=None):
            e = dict(env, **(extra or {}))
            return {m: as_user(['xdg-mime', 'query', 'default', m], e).stdout.strip() for m in MIMES}
        r['packageDefaults'] = query('package')
        r['gioMimeInodeDirectory'] = as_user(['gio', 'mime', 'inode/directory'], env).stdout
        mimeapps_present = {str(p): p.exists() for p in (home / '.config/mimeapps.list', Path('/etc/xdg/mimeapps.list'))}
        r['mimeappsFiles'] = mimeapps_present
        run.check('T0 package defaults exist for all four types according to the host\'s own xdg-mime, and no user or system mimeapps.list was registered by the task',
                  all(r['packageDefaults'].values()) and not any(mimeapps_present.values()), {'defaults': r['packageDefaults'], 'files': mimeapps_present})
        r['hostApps'] = {t: shutil.which(t) for t in ('xdg-open', 'xdg-mime', 'gio', 'nautilus', 'loupe', args.browser_exe)}

        nonce = secrets.token_hex(6)
        target_http = Target()
        reveal_dir = fixtures / f'uc11-reveal-{nonce}'
        reveal_dir.mkdir()
        os.chown(reveal_dir, account.pw_uid, account.pw_gid)
        reveal_file = reveal_dir / 'item.txt'
        reveal_file.write_text('x')
        os.chown(reveal_file, account.pw_uid, account.pw_gid)
        mount_marker = '/tmp/.mount_'

        # --- T0 host control: the host's own xdg-open with the host environment reaches the REAL applications (also the baseline of "what a real app looks like")
        base_pids = set(app_processes(account.pw_uid))
        hdir = fixtures / f'uc11-hostctl-{nonce}'
        hdir.mkdir()
        os.chown(hdir, account.pw_uid, account.pw_gid)
        hpath = f'/uc11-host-{nonce}'
        p_dir = open_as_user(out, 'dir', str(hdir), env)
        titles = wait_window(env, hdir.name, 30)
        st_dir = host_state(p_dir, out / 'host-dir.stderr')
        run.check('T0 control: the host xdg-open (host environment, no GUI) opens a REAL file manager window titled with the directory name', st_dir['rc'] in (0, None) and bool(titles), {**st_dir, 'titles': titles})
        p_url = open_as_user(out, 'url', f'http://127.0.0.1:{target_http.port}{hpath}', env)
        deadline = time.monotonic() + 40
        while not target_http.hits(hpath) and time.monotonic() < deadline:
            time.sleep(.4)
        hits = target_http.hits(hpath)
        st_url = host_state(p_url, out / 'host-url.stderr')
        run.check('T0 control: the host xdg-open reaches the REAL browser, which requests the controlled HTTP target (browser User-Agent)', st_url['rc'] in (0, None) and len(hits) == 1 and args.browser_ua in hits[0]['ua'],
                  {**st_url, 'hits': hits})
        title_http = wait_window(env, f'uc11-host-{nonce}', 20)
        run.check('T0 control: the browser window shows the page title served by the target (page evidence)', bool(title_http), title_http)
        hpng = fixtures / f'uc11-hostimg-{nonce}.png'
        hpng.write_bytes(png_bytes())
        os.chown(hpng, account.pw_uid, account.pw_gid)
        p_png = open_as_user(out, 'png', str(hpng), env)
        titles = wait_window(env, hpng.name, 30)
        st_png = host_state(p_png, out / 'host-png.stderr')
        run.check('T0 control: the host xdg-open opens the PNG in a REAL image viewer (window titled with the file name)', st_png['rc'] in (0, None) and bool(titles), {**st_png, 'titles': titles})
        r['hostControlApps'] = [inspect(p, mount_marker) for p in app_processes(account.pw_uid)]
        # negative control: a file whose type the HOST says has no default application. The first version wrote the text "x" (text/plain): under the generic dispatch some
        # application is registered for it and it opened (dev10), so that file was not a handlerless type. The fixture is therefore checked with the host's own xdg-mime.
        nohandler = fixtures / f'uc11-nohandler-{nonce}.uc11nohandler'
        nohandler.write_bytes(b'\x00\x01\x02UC11\xfe\xff' * 8)
        os.chown(nohandler, account.pw_uid, account.pw_gid)
        nh_type = as_user(['xdg-mime', 'query', 'filetype', str(nohandler)], env).stdout.strip()
        nh_default = as_user(['xdg-mime', 'query', 'default', nh_type], env).stdout.strip()
        r['negativeControlFixture'] = {'filetype': nh_type, 'defaultAccordingToHost': nh_default}
        run.check('T0 negative-control fixture: the host\'s own xdg-mime reports NO default application for the control file\'s type', nh_type != '' and nh_default == '', r['negativeControlFixture'])
        # Dispatch tracing (strace -f, execve only, root with -u uc): what xdg-open ACTUALLY started, window titles and application processes before/after.
        def traced_dispatch(label, target):
            titles_before, apps_before = set(window_titles(env)), set(app_processes(account.pw_uid))
            trace, err = out / f'host-{label}.strace', out / f'host-{label}.stderr'
            with err.open('w') as fe:
                proc = subprocess.Popen(['strace', '-u', USER, '-f', '-q', '-s', '256', '-e', 'trace=execve', '-o', str(trace), 'xdg-open', target], env=env, stdout=fe, stderr=fe)
                try:
                    proc.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    pass
            time.sleep(4)
            st = host_state(proc, err)
            names = sorted({m.group(1).rsplit('/', 1)[-1] for m in re.finditer(r'execve\("([^"]+)"', trace.read_text(errors='replace'))})
            titles_after, apps_after = set(window_titles(env)), set(app_processes(account.pw_uid))
            return proc, {**st, 'executablesRun': names, 'newWindowTitles': sorted(titles_after - titles_before), 'newApplicationPids': sorted(apps_after - apps_before),
                          'realApplicationRun': sorted(set(names) & (APP_EXES | {'firefox', 'epiphany', 'gio-launch-desktop'}))}

        # (1) A file whose type the host says has NO default application. The first hypothesis ("no registration => nothing is opened") is REFUTED by dev10/dev12 (kept):
        # under the generic dispatch xdg-open falls back to the browser (`x-www-browser` -> Epiphany ran: executablesRun below), under GNOME `gio open` rejects with a native
        # error (exit 4, "Failed to find default application for content type"). Both are the host's own behaviour and are RECORDED as observations; only the GNOME rejection
        # is a check, because only there the host rejects. This scenario is NOT the negative control of the harness.
        p_no, nd = traced_dispatch('nohandler', str(nohandler))
        r['unregisteredTypeDispatch'] = {**nd, 'fixture': r['negativeControlFixture'], 'observation': 'browser fallback observed (generic dispatch)' if nd['realApplicationRun'] else 'no application started'}
        if args.desktop == 'gnome':
            run.check('T0 observation checked (GNOME dispatch): for a type with no default application `gio open` rejects with its native error (exit 4, "Failed to find default application") and starts no real application',
                      nd['rc'] == 4 and 'Failed to find default application' in nd['stderrTail'] and not nd['realApplicationRun'], nd)
        # (2) The harness negative control: a path that does not exist. xdg-open's documented exit status is 2 ("one of the files ... did not exist"), observed under the generic
        # dispatch; under GNOME xdg-open hands the path to `gio open`, which rejects with ITS exit status 4 and ITS message (dev13: first version asserted 2 for both and failed on
        # gnome, kept). The contract: non-zero exit, the host's native "does not exist / no such file" message, nothing delivered; the exit status per dispatch is recorded.
        missing = fixtures / f'uc11-does-not-exist-{nonce}'
        p_miss, md = traced_dispatch('missing', str(missing))
        r['missingPathDispatch'] = md
        run.check('T0 negative control (independent of any registration): a path that does not exist is rejected by the host xdg-open (generic: xdg-open exit 2; GNOME: `gio open` exit 4; both recorded) with the native message of the host, starts no real application, creates no window and no application process',
                  md['rc'] not in (0, None) and md['rc'] == (2 if args.desktop == 'generic' else 4) and re.search(r'(?i)does not exist|no such file', md['stderrTail']) is not None and not md['realApplicationRun'] and not md['newWindowTitles'] and not md['newApplicationPids'], md)
        # Stopping the applications ends a foreground xdg-open: its exit status HERE (e.g. 4 after SIGTERM of the first Nautilus instance) is the consequence of the
        # runner's cleanup, not of the dispatch. Recorded to keep it apart from a natural exit (st_* above, sampled before the cleanup).
        kill_apps(account.pw_uid)
        r['hostControlCleanupExits'] = {}
        for label, proc in (('dir', p_dir), ('url', p_url), ('png', p_png)):
            try:
                r['hostControlCleanupExits'][label] = proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
                r['hostControlCleanupExits'][label] = 'killed by the runner after 20 s'
        r['hostControlStates'] = {'dir': st_dir, 'url': st_url, 'png': st_png}
        run.check('T0 the real applications of the host control are stopped before the GUI starts (every later window comes from the GUI chain)', not app_processes(account.pw_uid), app_processes(account.pw_uid))

        # --- the real GUI, non-portable
        gui = run.launch('gui1')
        launches.append(gui)
        conn_path, conn = wait_daemon(home)
        run.check('T1 the real bundled daemon started and published daemon.conn under the real HOME', conn is not None, str(conn_path))
        if conn is None:
            raise StopScenario()
        gui.step('bootstrapped', 120)
        state = wait_panel_ready(gui, 'h', 120)
        run.check('T1 the real WebView loaded the frontend (quick panel page ready)', state.get('panelReady') is True, state)
        table = procs()
        gui_pid = next((pid for pid, (exe, _) in table.items() if exe.endswith('/usr/bin/uniclipboard')), None)
        mount = table[gui_pid][0].split('/usr/bin/')[0] if gui_pid else None
        run.check('T1 the GUI executes from the AppImage mount and the data root is the real HOME (non-portable: no <AppImage>.home)',
                  bool(mount) and mount.startswith(mount_marker) and not Path(str(target) + '.home').exists() and (home / '.local/share').exists(), {'exe': table.get(gui_pid), 'portableHome': Path(str(target) + '.home').exists()})
        if not mount:
            raise StopScenario()
        genv = environ_of(gui_pid)
        r['guiEnvironment'] = {k: genv.get(k) for k in ('HOME', 'LD_LIBRARY_PATH', 'APPDIR', 'GIO_MODULE_DIR', 'XDG_DATA_DIRS', 'GDK_BACKEND', 'GTK_THEME', 'XDG_CURRENT_DESKTOP')}
        # G7: host GTK is present in this image; the GUI and its WebKit helpers must still take their toolkit from the mount
        helpers = [pid for pid, (exe, _) in procs().items() if exe.startswith(mount) and exe.endswith(('WebKitWebProcess', 'WebKitNetworkProcess'))]
        mapped = {}
        for pid in [gui_pid] + helpers[:2]:
            libs = maps_of(pid)
            mapped[str(pid)] = {k: sorted(p for p in libs if k in p.rsplit('/', 1)[-1]) for k in ('libglib-2.0', 'libgio-2.0', 'libgtk-3', 'libwebkit2gtk-4.1', 'libjavascriptcoregtk-4.1')}
        run.check('G7 although the host has GTK, the GUI and its WebKit helpers map GTK, WebKitGTK, JavaScriptCore, GLib and GIO from the AppImage mount, not from /usr',
                  bool(helpers) and all(paths and all(p.startswith(mount) for p in paths) for libs in mapped.values() for paths in libs.values()), mapped)
        host_gtk = subprocess.run('ldconfig -p | grep -E "libgtk-3|libgtk-4|libwebkit" | head', shell=True, capture_output=True, text=True).stdout
        r['hostToolkitInLoaderCache'] = host_gtk.strip().splitlines()

        reports = Reports()
        actions = []
        browser_seen, observed = [], {}

        def finish(name, kind, drive_result, **extra):
            apps = [inspect(p, mount) for p in app_processes(account.pw_uid, mount)]
            row = {'name': name, 'kind': kind, 'driveResult': drive_result, 'apps': apps, **extra}
            actions.append(row)
            return row

        def dir_action(name, command, params, title):
            res = gui.invoke(name, command, params)
            titles = wait_window(env, title, 40)
            row = finish(name, 'dir', res, expectedTitle=title, titles=titles)
            run.check(f'A {name}: a REAL file manager window appeared titled with the product\'s target "{title}"', bool(titles), {'driveResult': res, 'titles': titles})
            return row

        reveal_title = reveal_dir.name
        dir_action('reveal_path', 'reveal_path', {'path': str(reveal_file)}, reveal_title)
        logs_row = gui.invoke('logs', 'open_logs_directory', {})
        # the logs directory name is the product's own (…/Logs or …/logs); the windows list is the evidence
        time.sleep(6)
        logs_titles = [t for t in window_titles(env) if re.search(r'(?i)^logs?$', t)]
        finish('open_logs_directory', 'dir', logs_row, titles=logs_titles)
        run.check('A open_logs_directory: a REAL file manager window titled "logs" appeared', bool(logs_titles), {'driveResult': logs_row, 'titles': window_titles(env)})
        data_row = gui.invoke('data', 'open_data_directory', {})
        time.sleep(6)
        data_titles = [t for t in window_titles(env) if t.startswith('app.uniclipboard.desktop')]
        finish('open_data_directory', 'dir', data_row, titles=data_titles)
        run.check('A open_data_directory: a REAL file manager window titled with the data directory name appeared', bool(data_titles), {'driveResult': data_row, 'titles': window_titles(env)})

        def image_action(name, fname, expect_in_title=None):
            res = gui.invoke(name, 'open_image_externally', {'fileName': fname, 'data': base64.b64encode(png_bytes()).decode()})
            titles = wait_window(env, fname, 40)
            return finish(name, 'image', res, expectedTitle=fname, titles=titles)

        row = image_action('image-package-default', f'uc11-img-a-{nonce}.png')
        run.check('A open_image_externally (package default): a REAL image viewer window titled with the file name appeared', bool(row['titles']), {'driveResult': row['driveResult'], 'titles': row['titles']})
        row['apps_exes'] = sorted({a['exe'].rsplit('/', 1)[-1] for a in row['apps'] if a['status'] == 'ok'})
        run.check('A open_image_externally (package default): the application that opened it is the package default for image/png (Loupe), not an arbitrary one',
                  'loupe' in row['apps_exes'], row['apps_exes'])

        # --- G5 user default application in the real HOME: set with the host's own xdg-mime, GUI chain must follow it, and follow its removal
        e_user = dict(env)
        set_res = as_user(['xdg-mime', 'default', args.browser_desktop, 'image/png'], e_user)
        r['userDefaultSet'] = {'rc': set_res.returncode, 'now': as_user(['xdg-mime', 'query', 'default', 'image/png'], e_user).stdout.strip(), 'file': (home / '.config/mimeapps.list').read_text() if (home / '.config/mimeapps.list').exists() else None}
        run.check('G5 setup: the user default for image/png is now the browser in the real HOME (host xdg-mime query)', r['userDefaultSet']['now'] == args.browser_desktop, r['userDefaultSet'])
        fname_b = f'uc11-img-b-{nonce}.png'
        before_http = len(target_http.requests)
        row = image_action('image-user-default', fname_b)
        row['apps_exes'] = sorted({a['exe'].rsplit('/', 1)[-1] for a in row['apps'] if a['status'] == 'ok'})
        run.check('G5 open_image_externally follows the user\'s own default in the real HOME: the browser (not the package default viewer) shows the image', bool(row['titles']) and args.browser_exe in row['apps_exes'], {'titles': row['titles'], 'apps': row['apps_exes']})
        (home / '.config/mimeapps.list').unlink()
        r['userDefaultRemovedNow'] = as_user(['xdg-mime', 'query', 'default', 'image/png'], e_user).stdout.strip()
        fname_c = f'uc11-img-c-{nonce}.png'
        row = image_action('image-after-user-default-removed', fname_c)
        run.check('G5 negative control: with the user registration removed, the same product action opens the package default viewer window again',
                  bool(row['titles']) and r['userDefaultRemovedNow'] != args.browser_desktop, {'titles': row['titles'], 'default': r['userDefaultRemovedNow']})

        # --- the URL: shared frontend openUrl -> product host command -> real browser -> controlled HTTP target
        url_path = f'/uc11-url-{nonce}'
        url = f'http://127.0.0.1:{target_http.port}{url_path}'
        gui.ctl(f'panel-js openurl window.__ucE2eOpenUrl("{url}","http://127.0.0.1:{reports.port}/open")', 'panel-js-openurl')
        reports.wait('open', 20)
        deadline = time.monotonic() + 60
        while not target_http.hits(url_path) and time.monotonic() < deadline:
            time.sleep(.4)
        hits = target_http.hits(url_path)
        title = wait_window(env, f'uc11-url-{nonce}', 30)
        row = finish('Browser.OpenURL via the page', 'url', 'reported' if reports.wait('open', 1) else None, hits=hits, titles=title)
        run.check('A URL: the REAL browser made exactly one request for the product\'s URL path to the controlled server, with the browser User-Agent', len(hits) == 1 and args.browser_ua in hits[0]['ua'], hits)
        run.check('A URL: the browser window shows the controlled page\'s title (page evidence)', bool(title), title)
        run.check('A URL: no other request reached the controlled server (nothing but the product\'s target)', len([q for q in target_http.requests if q['path'] != url_path and 'host' not in q['path'] and not q['path'].startswith('/uc11-host-') and q['path'] != '/favicon.ico']) == 0,
                  [q['path'] for q in target_http.requests])

        # --- the live applications: library origin and environment (G2)
        inspected = [inspect(p, mount) for p in app_processes(account.pw_uid, mount)]
        all_apps = [a for a in inspected if a['status'] == 'ok']
        r['liveApplicationsNotRead'] = [a for a in inspected if a['status'] != 'ok']
        exes = sorted({a['exe'].rsplit('/', 1)[-1] for a in all_apps})
        r['liveApplications'] = all_apps
        run.check('G2 the real applications the GUI started are running (file manager, image viewer, browser and its WebKit/Gecko processes)', {'nautilus', 'loupe', args.browser_exe} <= set(exes), exes)
        run.check('G2 every live application process was READ (exe, environ, maps); a live process that could not be read counts as not verified, an exited one is recorded and excluded', not [a for a in inspected if a['status'] == 'unreadable'], r['liveApplicationsNotRead'])
        run.check('G2 no live application process maps a library from the AppImage mount', all(not a['mountLibs'] for a in all_apps), [a for a in all_apps if a['mountLibs']])
        run.check('G2 no live application process carries a variable that points into the AppImage mount or an AppImage LD_LIBRARY_PATH', all(not a['envPointingIntoMount'] and not (a['ldLibraryPath'] and 'usr/lib' in a['ldLibraryPath'] and mount in a['ldLibraryPath']) for a in all_apps),
                  [a for a in all_apps if a['envPointingIntoMount']])
        r['recordedNotAsserted'] = {'gdkBackend': sorted({str(a['gdkBackend']) for a in all_apps}), 'gtkTheme': sorted({str(a['gtkTheme']) for a in all_apps}), 'note': 'GDK_BACKEND / GTK_THEME are inherited: AppRun hook vs user setting cannot be told apart (17c10); changed only on a real new failure'}

        # --- the strace chain: xdg-open was started by the GUI with the sanitised environment. Under the generic dispatch xdg-open stays alive as the foreground parent
        # of the first application instance: sample which xdg-open processes are still running (and their children) BEFORE the GUI exit and the cleanup, so that a
        # natural exit, a still-running foreground wait and an exit caused by the cleanup can be told apart.
        alive_xdg = {pid: children_exes(pid) for pid, (exe, _) in procs().items() if exe.endswith('/xdg-open') or any(a == 'xdg-open' or a.endswith('/xdg-open') for a in cmdline_of(pid))}
        r['xdgOpenAliveBeforeGuiExit'] = {str(k): v for k, v in alive_xdg.items()}
        gui.ctl('exit', 'control-exit')
        deadline = time.monotonic() + 40
        while pid_alive(gui_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        r['guiExitedAfterControlExit'] = not pid_alive(gui_pid)
        gui.proc.terminate()
        try:
            gui.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            gui.proc.kill()
        launches.clear()
        execs, children, _, exits = parse_trace(run.trace)
        gui_started = [e for e in execs if e['rc'] == 0 and e['exe'].endswith('/xdg-open')]
        r['xdgOpenFromGui'] = [{'pid': e['pid'], 'argv': e['argv'][1:], 'exit': exits.get(e['pid']), 'aliveBeforeGuiExit': e['pid'] in alive_xdg, 'childrenWhileAlive': alive_xdg.get(e['pid']),
                                'varsIntoMount': sorted(k for k, v in e['env'].items() if mount in v), 'ldLibraryPath': e['env'].get('LD_LIBRARY_PATH')} for e in gui_started]
        # a natural exit must be 0; a still-running foreground xdg-open is allowed only with a live application child (its exit after the runner's cleanup is not judged)
        ok_exit = lambda x: (x['exit'] == 'exit 0') or (x['aliveBeforeGuiExit'] and bool(x['childrenWhileAlive']))
        run.check('G1 every xdg-open the GUI started (strace execve) carries no variable pointing into the mount and no LD_LIBRARY_PATH; each either exited 0 or was still running as the foreground parent of a live application (generic dispatch), none exited non-zero on its own',
                  len(gui_started) >= 6 and all(ok_exit(x) and not x['varsIntoMount'] and not x['ldLibraryPath'] for x in r['xdgOpenFromGui']), r['xdgOpenFromGui'])
        if args.f7:
            # --- F7 (OPEN product question): portable mode replaces HOME with <AppImage>.home for the whole process tree. The user's own default application (written to the
            # real HOME, as in G5) is then invisible to xdg-open. Reproduced with the real applications and RECORDED; nothing here passes or fails on it.
            kill_apps(account.pw_uid)
            as_user(['xdg-mime', 'default', args.browser_desktop, 'image/png'], dict(env))
            r['f7'] = {'userDefaultInRealHome': as_user(['xdg-mime', 'query', 'default', 'image/png'], dict(env)).stdout.strip()}
            made = as_user([str(target), '--appimage-portable-home'], env, timeout=60)
            r['f7']['portableHomeCreated'] = made.returncode == 0 and Path(str(target) + '.home').is_dir()
            gui2 = run.launch('gui2')
            launches.append(gui2)
            _, conn2 = wait_daemon(Path(str(target) + '.home'))
            gui2.step('bootstrapped', 120)
            wait_panel_ready(gui2, 'f', 120)
            fname_f = f'uc11-img-f7-{nonce}.png'
            res = gui2.invoke('f7img', 'open_image_externally', {'fileName': fname_f, 'data': base64.b64encode(png_bytes()).decode()})
            titles = wait_window(env, fname_f, 40)
            table2 = procs()
            gpid = next((pid for pid, (exe, _) in table2.items() if exe.endswith('/usr/bin/uniclipboard')), None)
            f7_apps = [inspect(p, mount) for p in app_processes(account.pw_uid, mount)]
            f7_exes = sorted({a['exe'].rsplit('/', 1)[-1] for a in f7_apps if a['status'] == 'ok'})
            r['f7'].update({'driveResult': res, 'titles': titles, 'openedBy': f7_exes, 'applicationsNotRead': [a for a in f7_apps if a['status'] != 'ok'],
                            'guiHome': environ_of(gpid).get('HOME') if gpid else None,
                            'verdict': 'user default NOT honoured (package default opened it)' if 'loupe' in f7_exes and args.browser_exe not in f7_exes else 'user default honoured or unclear: see openedBy'})
            gui2.ctl('exit', 'control-exit')
            try:
                gui2.proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                gui2.proc.terminate()
            launches.clear()
        r['passed'] = all(c['ok'] for c in r['checks'])
    except StopScenario:
        pass
    except Exception as e:
        r['error'] = repr(e)
        import traceback
        r['traceback'] = traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        try:
            r['screenshotNote'] = 'see windows.txt (xwininfo) and screenshot.png'
            (out / 'windows.txt').write_text(subprocess.run(['xwininfo', '-root', '-tree', '-display', DISPLAY], capture_output=True, text=True).stdout)
            if shutil.which('import'):
                subprocess.run(['import', '-display', DISPLAY, '-window', 'root', str(out / 'screenshot.png')], timeout=20)
        except Exception:
            pass
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for conn in home.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, 15)
            except (OSError, ValueError, KeyError):
                pass
        kill_apps(account.pw_uid)
        for sub in ('.local/state', '.local/share'):
            for logs in (home / sub).glob('app.uniclipboard.desktop*'):
                for e in copy_logs(logs, out / 'home-copy' / sub.replace('/', '_') / logs.name):
                    print('evidence copy error:', e, file=sys.stderr)
        if bus:
            bus.terminate()
        time.sleep(1)
        xvfb.terminate()
        r['actions'] = actions if 'actions' in dir() else None
        r['targetRequests'] = target_http.requests if 'target_http' in dir() else None
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
    print(json.dumps({'passed': r['passed'], 'mode': r['mode']}))
    sys.exit(0 if r['passed'] else 1)


if __name__ == '__main__':
    main()
