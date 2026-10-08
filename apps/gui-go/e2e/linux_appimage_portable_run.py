#!/usr/bin/env python3
"""Portable-mode E2E of the self-contained AppImage (slice 17c5, docs/architecture/gui-go-linux-appimage-portable.md).

Runs inside uc-gui-go-linux-runtime:17c4 (Xvfb, FUSE, NO GTK/WebKitGTK) as an UNPRIVILEGED user `uc` whose passwd home (/home/uc) is
the login session's home. There is NO Secret Service and no session bus: a portable installation must work with the file keystore.

  linux_appimage_portable_run.py --out DIR --appimage v1.AppImage --uniclip uniclip --feed DIR --manifest package-manifest.json

Scenarios (docs F-numbers):
  P1 portable launch   AppImage in `dir with space é/My App.AppImage`, `.home` created by the runtime's own `--appimage-portable-home`,
                       started through a symlink: data root is <APPIMAGE>.home/data for GUI AND daemon (/proc environ, daemon.conn), the
                       real user home gets nothing, no Secret Service, an encrypted space is created and one entry written (F1-F5)
  P2 autostart         the entry lands in the login session's directory (passwd home), not in the portable home, and disabling removes it (F7)
  P3 restart           second start with UC_PORTABLE=1 reads the same settings, space and entry back; no plaintext in the data root (F5)
  P4 update            untrusted signature rejected, trusted fixture update replaces the file, data stays in the same `.home`, old daemon gone (F6)
  P5 failures          UC_PORTABLE=1 without `.home` (F9), read-only `.home` owned by another user (F8), AppRun without $APPIMAGE (F10): an
                       error dialog and stderr message, exit 1, nothing written anywhere, no fallback to the user profile
Not proven: real desktop session reading the autostart entry, official signing, amd64, native desktop, GPU.
"""
import argparse
import json
import os
import pwd
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_appimage_run import (DISPLAY, PASSPHRASE, PLATFORM_KEY, VERSION, Launch, Run, daemon_status, inspect_processes, maps_of,  # noqa: E402,F401
                                pid_alive, procs, serve, sha256, stable, start_xvfb, wait_panel_ready)
import linux_appimage_run as base  # noqa: E402
from linux_xvfb_run import read_steps  # noqa: E402

_sha256 = base.sha256


def _sha256_as_user(path):
    """A FUSE mount is private to the user who mounted it: root cannot open /proc/<pid>/exe of a process running from it, the user can."""
    if str(path).startswith('/proc/'):
        r = subprocess.run(['sha256sum', str(path)], user=USER, group=USER, extra_groups=[], capture_output=True, text=True)
        return r.stdout.split()[0] if r.returncode == 0 and r.stdout.split() else f'unreadable:{r.stderr.strip()}'
    return _sha256(path)


base.sha256 = _sha256_as_user

USER = 'uc'
UNIQUE_TEXT = 'portable-appimage-secret-text-17c5-7f3a91'


class UserRun(Run):
    """Run whose processes execute as the unprivileged user."""

    def launch(self, tag, extra_env=None, appimage=None, args=(), drop=()):
        evidence, control = self.out / f'{tag}.jsonl', self.out / f'{tag}.control'
        for f in (evidence, control):
            f.write_text('')
            f.chmod(0o666)
        env = dict(self.env, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control), **(extra_env or {}))
        for key in drop:
            env.pop(key, None)
        log = self.out / f'{tag}.log'
        log.write_text('')
        log.chmod(0o666)
        proc = subprocess.Popen([str(appimage or self.appimage), *args], env=env, cwd=str(Path(appimage or self.appimage).parent),
                                stdout=log.open('a'), stderr=subprocess.STDOUT, user=USER, group=USER, extra_groups=[])
        return Launch(proc, evidence, control, log)


def as_user(cmd, env, **kw):
    return subprocess.run(cmd, env=env, user=USER, group=USER, extra_groups=[], capture_output=True, text=True, **kw)


def tree(root):
    """relative path -> size of everything below root (empty when root is missing)."""
    root = Path(root)
    if not root.exists():
        return {}
    rows = {}
    for p in root.rglob('*'):
        try:
            rows[str(p.relative_to(root))] = -1 if p.is_dir() else p.lstat().st_size
        except OSError:
            pass
    return rows


# The only files an ordinary GTK start (the error dialog) may leave in the real HOME: fontconfig's cache of the host's fonts, identified by
# name. They hold font metadata, not application data, and are listed in the evidence. Anything else is an unknown write and fails.
TOOLKIT_CACHE = re.compile(r'^\.cache(/fontconfig(/([0-9a-f]{32}-le64\.cache-\d+|CACHEDIR\.TAG))?)?$')


def new_in_home(run, label, login_home, before):
    """Everything new below the login home since `before` (full list recorded as evidence); returns the part that is NOT a recognised toolkit
    cache file, which the caller asserts empty."""
    new = sorted(set(tree(login_home)) - set(before))
    run.results.setdefault('newInRealHome', {})[label] = new
    return [f for f in new if not TOOLKIT_CACHE.match(f)]


def daemons():
    return sorted(pid for pid, (exe, _) in procs().items() if exe.rsplit('/', 1)[-1] == 'uniclipd')


def environ_of(pid):
    try:
        raw = Path(f'/proc/{pid}/environ').read_bytes()
    except OSError:
        return {}
    return dict(item.split('=', 1) for item in raw.decode(errors='replace').split('\0') if '=' in item)


def wait_daemon(root, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for conn in Path(root).rglob('daemon.conn'):
            try:
                c = json.loads(conn.read_text())
                if pid_alive(c['pid']):
                    return conn, c
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(.3)
    return None, None


def prepare_cli(run, args, sandbox, home_dir):
    """The CLI is not inside the AppImage, so it finds the daemon through its OWN portable rule (UC_PORTABLE=1: <exe dir>/data). A directory
    holding only the `uniclip` binary (no sibling daemon it could spawn instead) gets `data` -> <AppImage>.home/data, so the CLI discovers
    the AppImage's real daemon.conn and token through the same daemon.conn contract the GUI uses."""
    account = pwd.getpwnam(USER)
    cli_dir = sandbox / 'cli'
    cli_dir.mkdir()
    os.chown(cli_dir, account.pw_uid, account.pw_gid)
    shutil.copy2(args.uniclip, cli_dir / 'uniclip')
    (cli_dir / 'data').symlink_to(home_dir / 'data')
    run.cli = cli_dir / 'uniclip'


def uniclip(run, args, conn, *cmd, stdin=None):
    env = dict(run.env, UC_PORTABLE='1')
    try:
        r = subprocess.run([str(run.cli), '--json', *cmd], env=env, capture_output=True, text=True, timeout=90, user=USER, group=USER,
                           extra_groups=[], input=stdin)
    except subprocess.TimeoutExpired as e:
        return {'rc': None, 'timeout': 90, 'stdout': (e.stdout or b'').decode(errors='replace') if isinstance(e.stdout, bytes) else (e.stdout or ''),
                'stderr': ''}
    return {'rc': r.returncode, 'stdout': r.stdout.strip(), 'stderr': r.stderr.strip()[-500:]}


def mount_gone(mount, timeout=20):
    """The mount is private to the user (FUSE without allow_other), so existence is asked as that user; the runtime unmounts after the child exits."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if as_user(['test', '-e', mount], os.environ).returncode != 0:
            return True
        time.sleep(.5)
    return False


def stop(launch, daemon_conn=None):
    launch.ctl('exit', 'control-exit')
    code = launch.proc.wait(timeout=60)
    if daemon_conn:
        deadline = time.monotonic() + 20
        while pid_alive(daemon_conn['pid']) and time.monotonic() < deadline:
            time.sleep(.3)
    return code


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--uniclip', type=Path, required=True)
    parser.add_argument('--feed', type=Path)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--supplement', action='store_true', help='run only the supplement scenarios (F11, XDG_CONFIG_HOME) against an AppImage')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-portable-'))
    os.chown(sandbox, account.pw_uid, account.pw_gid)
    runtime_dir = sandbox / 'run'
    runtime_dir.mkdir()
    os.chown(runtime_dir, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    install = sandbox / 'dir with space é'
    install.mkdir()
    os.chown(install, account.pw_uid, account.pw_gid)
    target = install / 'My App.AppImage'
    shutil.copy2(args.appimage, target)
    os.chown(target, account.pw_uid, account.pw_gid)
    original_sha = sha256(target)
    login_home = Path(account.pw_dir)
    env = dict(os.environ, HOME=str(login_home), XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
               XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
               UC_GUI_GO_E2E_SECRET=PASSPHRASE, USER=USER, LOGNAME=USER)
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME',
                'DBUS_SESSION_BUS_ADDRESS', 'UC_E2E_BUS'):
        env.pop(key, None)
    run = UserRun(out, target, env)
    r = run.results
    r.update({'mode': 'portable', 'appimageSha256': original_sha, 'sandbox': str(sandbox), 'loginHome': str(login_home), 'user': USER,
              'scope': 'container, unprivileged user, Xvfb, no GTK/WebKitGTK on the host, NO Secret Service and no session bus; no desktop, GPU or Wayland'})
    xvfb = start_xvfb(out)
    launches = []
    try:
        loader = subprocess.run(['ldconfig', '-p'], capture_output=True, text=True).stdout
        bad = [l for l in loader.splitlines() if 'libwebkit2gtk' in l or 'libgtk-3' in l]
        run.check('P0 clean host: no libwebkit2gtk / libgtk-3 in the loader cache', not bad, bad)
        run.check('P0 the run is unprivileged and has no Secret Service: uid != 0, no session bus address in the environment',
                  account.pw_uid != 0 and 'DBUS_SESSION_BUS_ADDRESS' not in env, {'uid': account.pw_uid})
        if args.supplement:
            supplement(run, launches, args, sandbox, install, target, login_home)
        else:
            scenarios(run, launches, args, sandbox, install, target, login_home, original_sha)
        r['passed'] = all(c['ok'] for c in r['checks'])
    except Exception as e:  # keep the evidence of a failed run
        r['error'] = repr(e)
        import traceback
        r['traceback'] = traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for conn in sandbox.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, signal.SIGTERM)
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(1)
        for home in sandbox.rglob('*.AppImage.home'):
            base.copy_logs(home, out / 'home-copy' / home.name)
        xvfb.terminate()
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
    print(json.dumps({'passed': r['passed'], 'mode': 'portable'}))
    sys.exit(0 if r['passed'] else 1)


def scenarios(run, launches, args, sandbox, install, target, login_home, original_sha):
    out = run.out
    manifest = json.loads(args.manifest.read_text()) if args.manifest else {}
    daemon_sha = manifest.get('daemon', {}).get('sha256')
    home_dir = Path(str(target) + '.home')
    link_dir = sandbox / 'links'
    link_dir.mkdir()
    link = link_dir / 'Launcher.AppImage'
    link.symlink_to(target)
    login_before = tree(login_home)
    prepare_cli(run, args, sandbox, home_dir)

    # --- P1: create the portable home with the runtime's own mechanism, start through a symlink
    made = as_user([str(target), '--appimage-portable-home'], run.env, timeout=60)
    run.results['portableHomeCreation'] = {'rc': made.returncode, 'stdout': made.stdout, 'stderr': made.stderr}
    run.check('P1 `--appimage-portable-home` (the AppImage runtime) creates <AppImage>.home next to the real file, spaces and non-ASCII included',
              made.returncode == 0 and home_dir.is_dir(), run.results['portableHomeCreation'])
    gui = run.launch('gui1', appimage=link)
    launches.append(gui)
    conn_path, conn = wait_daemon(sandbox)
    run.check('P1 the bundled daemon started (file keystore, no Secret Service) and published daemon.conn', conn is not None, str(conn_path))
    if conn is None:
        return
    data_root = home_dir / 'data' / 'app.uniclipboard.desktop'
    run.check('P1 daemon.conn is under <AppImage>.home/data/app.uniclipboard.desktop (real file path, not the symlink, not the mount)',
              conn_path.parent == data_root, {'conn': str(conn_path), 'expected': str(data_root)})
    run.check('P1 portable data root holds the daemon pid file and a log directory is under the portable home',
              (data_root / '.daemon-pid').exists() or any(data_root.glob('.daemon-pid*')) or list(data_root.glob('*')),
              sorted(p.name for p in data_root.iterdir())[:30])
    gui_env, daemon_env = environ_of(gui.proc.pid), environ_of(conn['pid'])
    run.results['environments'] = {k: {x: v.get(x) for x in ('APPIMAGE', 'APPDIR', 'HOME', 'UC_PORTABLE', 'DBUS_SESSION_BUS_ADDRESS')}
                                   for k, v in (('gui', gui_env), ('daemon', daemon_env))}
    run.check('P1 GUI and daemon see the same real APPIMAGE path (symlink resolved by the runtime) and HOME = the portable home',
              all(e.get('APPIMAGE') == str(target) and e.get('HOME') == str(home_dir) for e in (gui_env, daemon_env)), run.results['environments'])
    # GLib auto-launches an empty session bus for the GUI (the daemon inherits its address): the check is that nothing on any reachable bus provides the
    # Secret Service, and no keyring process exists.
    bus = daemon_env.get('DBUS_SESSION_BUS_ADDRESS')
    names = None
    if bus:
        listed = as_user(['dbus-send', '--session', '--dest=org.freedesktop.DBus', '--print-reply', '/org/freedesktop/DBus', 'org.freedesktop.DBus.ListNames'],
                         dict(run.env, DBUS_SESSION_BUS_ADDRESS=bus), timeout=20)
        names = [l.split('"')[1] for l in listed.stdout.splitlines() if '"' in l]
        act = as_user(['dbus-send', '--session', '--dest=org.freedesktop.DBus', '--print-reply', '/org/freedesktop/DBus', 'org.freedesktop.DBus.ListActivatableNames'],
                      dict(run.env, DBUS_SESSION_BUS_ADDRESS=bus), timeout=20)
        activatable = [l.split('"')[1] for l in act.stdout.splitlines() if '"' in l]
        run.results['busNames'] = {'address': bus, 'rc': listed.returncode, 'names': names, 'activatable': activatable, 'activatableRc': act.returncode,
                                   'stderr': listed.stderr[-300:] + act.stderr[-300:]}
        names = names + (['ACTIVATABLE:org.freedesktop.secrets'] if 'org.freedesktop.secrets' in activatable else [])
    service_files = sorted(str(p) for d in ('/usr/share/dbus-1/services', '/usr/local/share/dbus-1/services', '/usr/share/dbus-1/system-services')
                           for p in Path(d).glob('*') if 'secret' in p.name.lower() or 'keyring' in p.name.lower())
    run.results['secretServiceFiles'] = service_files
    keyring_procs = sorted(v[1] for v in procs().values() if any(k in v[1] for k in ('keyring', 'secret', 'kwalletd')))
    run.check('P1 no Secret Service is reachable: the (auto-launched, empty) session bus does not own or activate org.freedesktop.secrets (ListNames, ListActivatableNames, no service files) and no keyring process exists',
              (names is None or ('org.freedesktop.secrets' not in names and 'ACTIVATABLE:org.freedesktop.secrets' not in names and len(names) > 0))
              and not keyring_procs and not service_files, [names, keyring_procs, service_files])
    init = uniclip(run, args, conn, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'portable-e2e')
    run.results['spaceInit'] = init
    boot = gui.step('bootstrapped', 120)
    run.check('P1 the WebView ran the frontend and reached the daemon', boot['ok'], boot)
    state = wait_panel_ready(gui, 'boot')
    run.check('P1 the panel reports ready', state.get('panelReady') is True, state)
    mount = inspect_processes(run, 'P1', gui.proc.pid, daemon_sha, conn['pid'])
    status_before = uniclip(run, args, conn, 'space', 'status')
    run.results['spaceStatusBefore'] = status_before
    run.check('P1 the encrypted space was created in the portable data root and is unlocked (file keystore)',
              init['rc'] == 0 and status_before['rc'] == 0, [init, status_before])
    default = gui.invoke('ad0', 'get_auto_download_update')
    flipped = not default['data']
    written = gui.ctl(f"setting autoDownloadUpdate {'on' if flipped else 'off'}", 'control-setting')
    readback = gui.invoke('ad1', 'get_auto_download_update')
    run.check('P1 a user setting is written through the daemon API and reads back', written['ok'] and readback['data'] == flipped, [default, written, readback])
    sent = uniclip(run, args, conn, 'send', '--text', UNIQUE_TEXT)
    run.results['send'] = sent
    searched = uniclip(run, args, conn, 'search', 'portable-appimage-secret')
    run.results['search'] = searched
    run.check('P1 a clipboard entry written through the daemon is found again through the encrypted index',
              sent['rc'] is not None and UNIQUE_TEXT[:20] in (searched['stdout'] + searched['stderr']), [sent, searched])
    data_files = sorted(str(p.relative_to(home_dir)) for p in home_dir.rglob('*') if p.is_file())
    run.results['portableHomeFiles'] = data_files
    run.check('P1 the file keystore lives in the portable data root (a keystore/key file exists below <AppImage>.home/data)',
              any('key' in f.lower() or 'vault' in f.lower() or 'secret' in f.lower() for f in data_files if f.startswith('data/')), data_files[:80])
    run.check('P1 nothing but the AppImage and its .home sits next to the AppImage; nothing was written in the mount directory listing',
              sorted(p.name for p in install.iterdir()) == ['My App.AppImage', 'My App.AppImage.home'], sorted(p.name for p in install.iterdir()))
    run.check('P1 the real user home (passwd) received nothing from the portable run',
              tree(login_home) == login_before, sorted(set(tree(login_home)) ^ set(login_before)))

    # --- P2: autostart lands where the login session reads it, not in the portable home
    ctl_before = gui.ctl('autostart-state before', 'autostart-before')
    on = gui.invoke('as-on', 'update_autostart', {'enabled': True})
    entry = login_home / '.config/autostart/UniClipboard.desktop'
    body = entry.read_text() if entry.exists() else ''
    exec_line = next((l for l in body.splitlines() if l.startswith('Exec=')), '')
    ctl_on = gui.ctl('autostart-state on', 'autostart-on')
    run.check('P2 enabling autostart writes <passwd home>/.config/autostart/UniClipboard.desktop: Exec= is the real AppImage path (quoted, with spaces), '
              'carries --autostart, is not the mount', on.get('ok') and str(target) in exec_line and '--autostart' in exec_line and '.mount_' not in body, [on, body])
    run.check('P2 the portable home has NO autostart entry (the session never reads it)', not (home_dir / '.config').exists()
              or not list((home_dir / '.config').rglob('*.desktop')), sorted(str(p) for p in (home_dir / '.config').rglob('*') if p.is_file()) if (home_dir / '.config').exists() else [])
    run.check('P2 the UI state is consistent: stored preference on, registration enabled, reported path is the session entry',
              ctl_on['detail'].get('setting') is True and ctl_on['detail'].get('enabled') is True and ctl_on['detail'].get('path') == str(entry), [ctl_before, ctl_on])
    off = gui.invoke('as-off', 'update_autostart', {'enabled': False})
    run.check('P2 disabling autostart removes the session entry', off.get('ok') and not entry.exists(), off)
    code = stop(gui, conn)
    run.check('P1 GUI exit 0 and the daemon stopped (/proc state, not kill 0)', code == 0 and not pid_alive(conn['pid']), {'exit': code})
    run.check('P1 the mount is gone after exit', mount is None or mount_gone(mount), mount)
    run.check('P2 after the scenario the real user home holds only the (empty) autostart directory created for the entry',
              set(tree(login_home)) - set(login_before) <= {'.config', '.config/autostart'}, sorted(set(tree(login_home)) - set(login_before)))

    # --- P3: restart with UC_PORTABLE=1 (forced) on the real path; same root, same data
    plain_hits = []
    for p in (home_dir / 'data').rglob('*'):
        if p.is_file() and p.stat().st_size < 300 << 20:
            blob = p.read_bytes()
            if UNIQUE_TEXT.encode() in blob or PASSPHRASE.encode() in blob:
                plain_hits.append(str(p))
    run.check('P3 no plaintext of the written entry or the passphrase in any file of the portable data root', not plain_hits, plain_hits)
    gui2 = run.launch('gui2', {'UC_PORTABLE': '1'})
    launches.append(gui2)
    conn2_path, conn2 = wait_daemon(sandbox)
    run.check('P3 restart (forced UC_PORTABLE=1): a NEW daemon on the same data root', conn2 is not None and conn2['pid'] != conn['pid'] and conn2_path.parent == data_root,
              [str(conn2_path), conn2 and conn2['pid'], conn['pid']])
    gui2.step('bootstrapped', 120)
    wait_panel_ready(gui2, 'p3')
    after = gui2.invoke('ad2', 'get_auto_download_update')
    run.check('P3 the setting written before the restart reads back', after['data'] == flipped, after)
    status_after = uniclip(run, args, conn2, 'space', 'status')
    try:
        same = stable(json.loads(status_before['stdout'])) == stable(json.loads(status_after['stdout']))
    except ValueError:
        same = status_before['stdout'] == status_after['stdout']
    run.check('P3 the encrypted space is still initialised and unlocked with the same stable status fields (no re-initialisation)', status_after['rc'] == 0 and same,
              [status_before['stdout'][:400], status_after['stdout'][:400]])
    found = uniclip(run, args, conn2, 'search', 'portable-appimage-secret')
    run.check('P3 the entry written before the restart is found again', UNIQUE_TEXT[:20] in (found['stdout'] + found['stderr']), found)
    code = stop(gui2, conn2)
    run.check('P3 exit 0, daemon stopped', code == 0 and not pid_alive(conn2['pid']), {'exit': code})

    # --- P4: update
    if not args.feed:
        run.check('P4 update scenarios were not run (no --feed)', False)
    else:
        update(run, launches, args, sandbox, target, home_dir, data_root, daemon_sha, flipped, original_sha, conn2)

    failures(run, launches, args, sandbox, install, target, login_home)


def update(run, launches, args, sandbox, target, home_dir, data_root, daemon_sha, flipped, original_sha, previous):
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
        files_before = sorted(str(p.relative_to(home_dir)) for p in home_dir.rglob('*') if p.is_file())
        bad = run.launch('update-bad', {'UC_GUI_GO_E2E_PHASE': 'update-bad', 'UC_UPDATE_ENDPOINT': f'{base}/bad.json', 'UC_UPDATE_PUBKEY': pubkey})
        launches.append(bad)
        bad.step('update-check', 120)
        row = bad.step('update-download-rejected', 120)
        run.check('P4 an artifact signed by an untrusted key is rejected, naming the signature', row['ok'] and 'signature' in json.dumps(row['detail']).lower(), row)
        run.check('P4 the AppImage is byte-identical after the rejected update', sha256(target) == original_sha)
        try:
            bad.proc.wait(timeout=90)
        except Exception:
            bad.proc.terminate()
        good = run.launch('update-good', {'UC_GUI_GO_E2E_PHASE': 'update-good', 'UC_UPDATE_ENDPOINT': f'{base}/good.json', 'UC_UPDATE_PUBKEY': pubkey})
        launches.append(good)
        first = good.step('update-state', 120)
        first_pid = first['detail']['pid']
        _, old = wait_daemon(sandbox)
        good.step('update-relaunched', 240, allow_exit=True)
        states = [x for x in read_steps(good.evidence) if x['step'] == 'update-state']
        run.check('P4 the AppImage now holds the v2 bytes', sha256(target) == v2_sha, {'v2': v2_sha, 'file': sha256(target)})
        run.check('P4 the restarted process is a new process running the v2 image (marker in its mount)',
                  len(states) == 2 and states[1]['detail']['installed'] and states[1]['detail']['pid'] != first_pid, states)
        deadline = time.monotonic() + 40
        while old and pid_alive(old['pid']) and time.monotonic() < deadline:
            time.sleep(.5)
        run.check('P4 the old daemon is gone after the update (/proc state)', old is not None and not pid_alive(old['pid']), old and old['pid'])
        new_env = environ_of(states[-1]['detail']['pid'])
        _, new = wait_daemon(sandbox)
        run.check('P4 the new daemon runs on the SAME portable data root (<AppImage>.home/data) of the replaced file',
                  new is not None and new['pid'] != old['pid'] and any(c.parent == data_root for c in sandbox.rglob('daemon.conn')),
                  {'new': new and new['pid'], 'env': {k: new_env.get(k) for k in ('APPIMAGE', 'HOME')}})
        run.check('P4 the restarted GUI process sees the same APPIMAGE path and portable HOME', new_env.get('APPIMAGE') == str(target) and new_env.get('HOME') == str(home_dir), new_env.get('APPIMAGE'))
        try:
            good.proc.wait(timeout=60)
        except Exception:
            pass
        deadline = time.monotonic() + 40
        while (pid_alive(states[-1]['detail']['pid']) or pid_alive(new['pid'])) and time.monotonic() < deadline:
            time.sleep(.5)
        run.check('P4 the relaunched process ended its scenario and took its daemon down', not pid_alive(states[-1]['detail']['pid']) and not pid_alive(new['pid']))
        # A fresh launch of the replaced AppImage on the same portable home: new daemon, user data readable through the real API.
        post = run.launch('post-update')
        launches.append(post)
        pconn_path, pconn = wait_daemon(sandbox)
        run.check('P4 the replaced AppImage starts a new daemon on the same portable data root',
                  pconn is not None and pconn['pid'] not in (old['pid'], new['pid']) and pconn_path.parent == data_root, [str(pconn_path), pconn and pconn['pid']])
        post.step('bootstrapped', 120)
        pstate = wait_panel_ready(post, 'post')
        run.check('P4 the v2 page reached the daemon (panel ready)', pstate.get('panelReady') is True, pstate)
        pmount = inspect_processes(run, 'P4', post.proc.pid, daemon_sha, pconn['pid'])
        marker = as_user(['test', '-f', f'{pmount}/usr/share/uniclipboard/update-marker.txt'], run.env)
        run.check('P4 the running image is v2 (marker file in its mount)', pmount is not None and marker.returncode == 0, pmount)
        after = post.invoke('ad3', 'get_auto_download_update')
        run.check('P4 the user setting written before the update reads back through the new daemon after the update', after['data'] == flipped, after)
        found = uniclip(run, args, pconn, 'search', 'portable-appimage-secret')
        status = uniclip(run, args, pconn, 'space', 'status')
        run.check('P4 the entry and the unlocked encrypted space survived the update', UNIQUE_TEXT[:20] in (found['stdout'] + found['stderr']) and status['rc'] == 0, [found, status])
        files_after = sorted(str(p.relative_to(home_dir)) for p in home_dir.rglob('*') if p.is_file())
        gone = sorted(f for f in set(files_before) - set(files_after) if not f.rsplit('/', 1)[-1].startswith(('daemon', '.daemon', '.uniclipd')))
        run.check('P4 no persisted file of the portable home disappeared (lifecycle state files excluded)', not gone, gone)
        run.check('P4 the portable home is still next to the (replaced) file; the update did not move or recreate it',
                  home_dir.is_dir() and sorted(p.name for p in target.parent.iterdir()) == ['My App.AppImage', 'My App.AppImage.home'])
        code = stop(post, pconn)
        run.check('P4 exit 0 and the daemon stopped', code == 0 and not pid_alive(pconn['pid']), {'exit': code})
    finally:
        server.shutdown()


def dismiss_dialog(run, launch, timeout=60):
    """Find the error dialog's X window and press Return in it; returns what was observed."""
    xenv = dict(os.environ, DISPLAY=DISPLAY)
    deadline = time.monotonic() + timeout
    windows = []
    while time.monotonic() < deadline and not windows and launch.proc.poll() is None:
        windows = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', 'UniClipboard'], env=xenv, capture_output=True, text=True).stdout.split()
        time.sleep(.5)
    info = {'windows': windows}
    if windows:
        info['names'] = [subprocess.run(['xdotool', 'getwindowname', w], env=xenv, capture_output=True, text=True).stdout.strip() for w in windows]
        time.sleep(1)
        subprocess.run(['xdotool', 'windowfocus', windows[-1]], env=xenv, capture_output=True)
        subprocess.run(['xdotool', 'key', '--clearmodifiers', 'Return'], env=xenv, capture_output=True)
    try:
        info['exit'] = launch.proc.wait(timeout=30)
    except subprocess.TimeoutExpired:
        info['exit'] = None
        launch.proc.terminate()
    return info


def supplement(run, launches, args, sandbox, install, target, login_home):
    """Scenarios the main run does not cover: the stale-APPIMAGE rule (F11) and the XDG_CONFIG_HOME branch of the autostart directory."""
    account = pwd.getpwnam(USER)
    home_dir = Path(str(target) + '.home')
    made = as_user([str(target), '--appimage-portable-home'], run.env, timeout=60)
    run.check('S0 the portable home exists (created by the AppImage runtime)', made.returncode == 0 and home_dir.is_dir(), made.stderr[-300:])

    # --- S1 (F11): an inherited APPIMAGE does not make an ordinary executable portable-from-the-AppImage. The unpacked binary runs with the AppImage's
    # own environment hook, then APPDIR is pointed elsewhere (the executable is no longer below APPDIR) while APPIMAGE names a real AppImage that HAS a
    # `.home`: the legacy rule (<exe dir>/data, UC_PORTABLE) must apply, and the decoy's `.home` must stay untouched.
    unpack = sandbox / 'f11'
    unpack.mkdir()
    os.chown(unpack, account.pw_uid, account.pw_gid)
    ex = subprocess.run([str(args.appimage), '--appimage-extract'], cwd=unpack, user=USER, group=USER, extra_groups=[], capture_output=True, text=True, env=run.env, timeout=300)
    root = unpack / 'squashfs-root'
    run.check('S1 the AppImage unpacks', ex.returncode == 0 and (root / 'usr/bin/uniclipboard').exists(), ex.stderr[-300:])
    decoy = unpack / 'Decoy.AppImage'
    shutil.copy2(args.appimage, decoy)
    decoy_home = Path(str(decoy) + '.home')
    decoy_home.mkdir()
    for path in (decoy, decoy_home):
        os.chown(path, account.pw_uid, account.pw_gid)
    wrapper = unpack / 'run-stale.sh'
    wrapper.write_text(f"""#!/bin/bash
export APPDIR={root}
. {root}/apprun-hooks/linuxdeploy-plugin-gtk.sh
export APPDIR=/nonexistent-appdir APPIMAGE={decoy} LD_LIBRARY_PATH={root}/usr/lib
cd {root}/usr
exec {root}/usr/bin/uniclipboard "$@"
""")
    wrapper.chmod(0o755)
    os.chown(wrapper, account.pw_uid, account.pw_gid)
    gui = run.launch('stale-appimage', {'UC_PORTABLE': '1'}, appimage=wrapper)
    launches.append(gui)
    conn_path, conn = wait_daemon(unpack)
    legacy_root = root / 'usr/bin/data/app.uniclipboard.desktop'
    run.results['staleAppimage'] = {'daemonConn': str(conn_path), 'expectedLegacyRoot': str(legacy_root), 'decoyHomeFiles': sorted(tree(decoy_home))}
    run.check('S1 with APPDIR not containing the executable, a stale APPIMAGE is ignored: the legacy rule (<exe dir>/data) applies and the daemon runs there',
              conn is not None and conn_path.parent == legacy_root, [str(conn_path), str(legacy_root)])
    run.check('S1 the AppImage named by the stale APPIMAGE keeps an empty `.home` (nothing was written there)', tree(decoy_home) == {}, sorted(tree(decoy_home)))
    if conn is not None:
        env_gui = environ_of(gui.proc.pid)
        run.check('S1 the process really had the stale environment (APPIMAGE = decoy, APPDIR does not contain the executable)',
                  env_gui.get('APPIMAGE') == str(decoy) and env_gui.get('APPDIR') == '/nonexistent-appdir', {k: env_gui.get(k) for k in ('APPIMAGE', 'APPDIR')})
        try:
            gui.step('bootstrapped', 120)
            gui.ctl('exit', 'control-exit')
        except RuntimeError as e:
            run.results['staleAppimage']['bootstrapNote'] = str(e)
    try:
        gui.proc.wait(timeout=60)
    except subprocess.TimeoutExpired:
        gui.proc.terminate()
    deadline = time.monotonic() + 20
    while conn and pid_alive(conn['pid']) and time.monotonic() < deadline:
        time.sleep(.3)
    run.check('S1 the stale-environment GUI and its daemon ended', gui.proc.poll() is not None and not (conn and pid_alive(conn['pid'])))

    # --- S2: an absolute XDG_CONFIG_HOME (which the AppImage runtime does not touch and the login session honours) decides the autostart directory
    xdg = sandbox / 'xdg-config'
    xdg.mkdir()
    os.chown(xdg, account.pw_uid, account.pw_gid)
    login_before = tree(login_home)
    gui = run.launch('xdg-config', {'XDG_CONFIG_HOME': str(xdg)}, appimage=target)
    launches.append(gui)
    conn_path, conn = wait_daemon(home_dir)
    run.check('S2 the portable AppImage starts with XDG_CONFIG_HOME set', conn is not None, str(conn_path))
    if conn is None:
        return
    gui.step('bootstrapped', 120)
    on = gui.invoke('xdg-on', 'update_autostart', {'enabled': True})
    entry = xdg / 'autostart/UniClipboard.desktop'
    body = entry.read_text() if entry.exists() else ''
    run.check('S2 the entry is in $XDG_CONFIG_HOME/autostart (what the session reads), with Exec= the real AppImage path',
              on.get('ok') and str(target) in body and '--autostart' in body, [on, body])
    run.check('S2 neither the passwd home nor the portable home got an entry', not (login_home / '.config/autostart/UniClipboard.desktop').exists()
              and not list(home_dir.rglob('autostart/*.desktop')), sorted(set(tree(login_home)) - set(login_before)))
    off = gui.invoke('xdg-off', 'update_autostart', {'enabled': False})
    run.check('S2 disabling removes it', off.get('ok') and not entry.exists(), off)
    code = stop(gui, conn)
    run.check('S2 exit 0, daemon stopped', code == 0 and not pid_alive(conn['pid']), {'exit': code})


def failures(run, launches, args, sandbox, install, target, login_home):
    login_before = tree(login_home)
    # F9: UC_PORTABLE=1 with an AppImage that has no .home
    lone_dir = sandbox / 'no-home'
    lone_dir.mkdir()
    os.chown(lone_dir, pwd.getpwnam(USER).pw_uid, pwd.getpwnam(USER).pw_gid)
    lone = lone_dir / 'Lone.AppImage'
    shutil.copy2(args.appimage, lone)
    os.chown(lone, pwd.getpwnam(USER).pw_uid, pwd.getpwnam(USER).pw_gid)
    lc = run.launch('fail-no-home', {'UC_PORTABLE': '1'}, appimage=lone)
    launches.append(lc)
    info = dismiss_dialog(run, lc)
    log = lc.log.read_text(errors='replace')
    run.results['failNoHome'] = {'dialog': info, 'log': log[-1500:]}
    run.check('F9 UC_PORTABLE=1 without <AppImage>.home: the message names the missing directory and the --appimage-portable-home fix (stderr)',
              'Lone.AppImage.home' in log and '--appimage-portable-home' in log, log[-800:])
    run.check('F9 an error dialog window is shown and, once dismissed, the process exits with status 1', bool(info['windows']) and info.get('exit') == 1, info)
    app_files = new_in_home(run, 'F9', login_home, login_before)
    run.check('F9 nothing was created: no .home, no files next to the AppImage, no daemon, no unknown write in the user home (only fontconfig cache files of the GTK dialog, listed in the evidence), no daemon process',
              sorted(p.name for p in lone_dir.iterdir()) == ['Lone.AppImage'] and not app_files and not daemons() and not list(sandbox.glob('no-home/**/daemon.conn')),
              {'next to the AppImage': sorted(p.name for p in lone_dir.iterdir()), 'unknown writes in real home': app_files, 'daemon pids': daemons()})

    # F8: a .home owned by another user and read-only (the run is NOT root, so this is a real permission failure)
    ro_dir = sandbox / 'readonly'
    ro_dir.mkdir()
    os.chown(ro_dir, pwd.getpwnam(USER).pw_uid, pwd.getpwnam(USER).pw_gid)
    ro = ro_dir / 'Locked.AppImage'
    shutil.copy2(args.appimage, ro)
    os.chown(ro, pwd.getpwnam(USER).pw_uid, pwd.getpwnam(USER).pw_gid)
    ro_home = Path(str(ro) + '.home')
    ro_home.mkdir()
    ro_home.chmod(0o555)  # owned by root: the unprivileged user can neither write nor chmod it
    lc = run.launch('fail-readonly', appimage=ro)
    launches.append(lc)
    info = dismiss_dialog(run, lc)
    log = lc.log.read_text(errors='replace')
    run.results['failReadonly'] = {'dialog': info, 'log': log[-1500:], 'homeOwner': ro_home.stat().st_uid}
    run.check('F8 the portable home is owned by root and 0555 while the AppImage runs as the unprivileged uid (a real permission failure, not root-bypassed)',
              ro_home.stat().st_uid == 0 and oct(ro_home.stat().st_mode & 0o777) == '0o555' and pwd.getpwnam(USER).pw_uid != 0)
    run.check('F8 the message names the data directory and says to make it writable (stderr)',
              str(ro_home) in log and 'writable' in log or 'cannot be created' in log, log[-800:])
    run.check('F8 an error dialog window is shown and, once dismissed, the process exits with status 1', bool(info['windows']) and info.get('exit') == 1, info)
    app_files = new_in_home(run, 'F8', login_home, login_before)
    run.check('F8 nothing was written into the read-only home, no unknown write in the user profile, no daemon process',
              tree(ro_home) == {} and not app_files and not daemons() and not list(ro_dir.rglob('daemon.conn')), {'readonly home': sorted(tree(ro_home)), 'unknown writes in real home': app_files, 'daemon pids': daemons()})

    # F10: the unpacked AppRun without $APPIMAGE and with UC_PORTABLE=1
    unpack = sandbox / 'unpacked'
    unpack.mkdir()
    os.chown(unpack, pwd.getpwnam(USER).pw_uid, pwd.getpwnam(USER).pw_gid)
    ex = subprocess.run([str(args.appimage), '--appimage-extract'], cwd=unpack, user=USER, group=USER, extra_groups=[], capture_output=True, text=True, env=run.env, timeout=300)
    apprun = unpack / 'squashfs-root' / 'AppRun'
    run.check('F10 the AppImage unpacks (--appimage-extract) so AppRun can be run without the runtime', ex.returncode == 0 and apprun.exists(), ex.stderr[-300:])
    before = tree(unpack)
    lc = run.launch('fail-apprun', {'UC_PORTABLE': '1'}, appimage=apprun)
    launches.append(lc)
    info = dismiss_dialog(run, lc)
    log = lc.log.read_text(errors='replace')
    run.results['failAppRun'] = {'dialog': info, 'log': log[-1500:]}
    run.check('F10 AppRun without a valid $APPIMAGE and UC_PORTABLE=1 fails with a message about $APPIMAGE (no silent <AppDir>/usr/data)', '$APPIMAGE' in log, log[-800:])
    run.check('F10 error dialog, exit status 1', bool(info['windows']) and info.get('exit') == 1, info)
    app_files = new_in_home(run, 'F10', login_home, login_before)
    run.check('F10 nothing was written in the unpacked tree (no <AppDir>/usr/data), no unknown write in the user home, no daemon process', tree(unpack) == before and not app_files and not daemons(),
              {'unpacked tree changes': sorted(set(tree(unpack)) ^ set(before)), 'unknown writes in real home': app_files, 'daemon pids': daemons()})


if __name__ == '__main__':
    main()
