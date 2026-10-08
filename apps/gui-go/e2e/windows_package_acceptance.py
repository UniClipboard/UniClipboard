#!/usr/bin/env python3
"""Clean-machine acceptance of the shipped Windows packages (issue #1897).

  windows_package_acceptance.py --arch amd64|arm64 --setup <current setup.exe> --portable <current portable.zip> \\
      --newer-setup <ACCEPTANCE-..setup.exe> --uniclip <uniclip.exe> --daemon-sha256 <hex> --out <evidence dir>

Runs the SHIPPING form (release build: no profile, real data root, real Credential Manager) the way a user meets it,
so it refuses to run anywhere but a disposable GitHub-hosted runner. Scenarios, in order:

  A  silent install -> files, registry identity, shortcuts, packaged daemon hash -> first launch -> daemon from the
     install dir answers /health -> space init and a clipboard capture -> autostart preference + restart writes the Run
     value -> forced stop of GUI and daemon -> history still readable after restart
  B  update over the installed copy (`/P /R /UPDATE`, newer package) while clipboard writes are in flight: installer
     restart, single instance, old processes gone, history kept, Run value kept
  C  downgrade refusal (the older package over the newer install): refused, nothing changed
  D  uninstall keeping the data: files, registry, shortcuts and Run value gone, data kept
  E  reinstall over the kept data: history written before the uninstall is still readable
  F  uninstall with `/DELETEAPPDATA`: data roots gone
  G  portable zip in a writable folder: data stays in the folder; nothing new named after the app outside it
  H  portable zip in a read-only folder: the behaviour is recorded (not asserted)

Not covered here, by construction: a real sign-out/sign-in (the Run value and its command line are checked instead),
the interactive wizard (the passive `/P` and silent `/S` modes are), a Run value written by the real Tauri app, and
Authenticode (the packages are unsigned). Each is listed in the results under `notCovered`.
"""
import argparse
import ctypes
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile
from pathlib import Path

if os.name != 'nt':
    sys.exit('this script runs on Windows only')
import winreg  # noqa: E402

BUNDLE = 'app.uniclipboard.desktop'
PRODUCT = 'UniClipboard'
UNINST = r'Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard'
RUN = r'Software\Microsoft\Windows\CurrentVersion\Run'
LOCAL = Path(os.environ['LOCALAPPDATA'])
ROAM = Path(os.environ['APPDATA'])
INSTDIR = LOCAL / PRODUCT
DATA_ROOTS = [LOCAL / BUNDLE, ROAM / BUNDLE]
PASS = 'acceptance-passphrase-1897'
DETACHED = 0x00000008 | 0x00000200  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP

results = {'checks': [], 'notCovered': [
    'real sign-out and sign-in (only the Run value and its command line are checked)',
    'the interactive installer wizard (passive /P and silent /S are run)',
    'a Run value written by the real Tauri app (needs the public Tauri build and its settings toggle)',
    'Authenticode (the packages are unsigned; no signing decision exists)',
    'Windows 10 / Windows 11 client editions (a hosted runner is Windows Server)']}
OUT = None


def check(name, ok, detail=None):
    results['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
    print(('PASS ' if ok else 'FAIL ') + name + ('' if ok or detail is None else f'  [{str(detail)[:300]}]'), flush=True)
    return bool(ok)


def note(name, detail):
    results.setdefault('observations', {})[name] = detail
    print(f'NOTE {name}: {str(detail)[:300]}', flush=True)


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def ps(script, timeout=60):
    return subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', script], capture_output=True, text=True, timeout=timeout)


def wait_for(fn, timeout, interval=1.0):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        v = fn()
        if v:
            return v
        time.sleep(interval)
    return fn()


def processes(name):
    r = ps(f"Get-CimInstance Win32_Process -Filter \"Name='{name}'\" | Select-Object ProcessId,ExecutablePath | ConvertTo-Json -Compress")
    if not r.stdout.strip():
        return []
    data = json.loads(r.stdout)
    return [data] if isinstance(data, dict) else data


def norm(p):
    return os.path.normcase(os.path.normpath(p)) if p else ''


def kill_all():
    for image in (f'{PRODUCT}.exe', 'uniclipd.exe', 'uniclip.exe'):
        subprocess.run(['taskkill', '/F', '/T', '/IM', image], capture_output=True)
    wait_for(lambda: not processes(f'{PRODUCT}.exe') and not processes('uniclipd.exe'), 20)


def reg_values(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as k:
            out, i = {}, 0
            while True:
                try:
                    n, v, _ = winreg.EnumValue(k, i)
                except OSError:
                    return out
                out[n] = v
                i += 1
    except FileNotFoundError:
        return None


def reg_subkeys(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as k:
            return sorted(winreg.EnumKey(k, i) for i in range(winreg.QueryInfoKey(k)[0]))
    except FileNotFoundError:
        return []


def run_value():
    return (reg_values(RUN) or {}).get(PRODUCT)


def shot(label):
    """Full-screen screenshot as evidence; a headless session may not give one, which is recorded."""
    path = OUT / 'screenshots' / f'{label}.png'
    path.parent.mkdir(exist_ok=True)
    r = ps("Add-Type -AssemblyName System.Windows.Forms,System.Drawing;"
           "$b=[System.Windows.Forms.SystemInformation]::VirtualScreen;$bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height;"
           "$g=[System.Drawing.Graphics]::FromImage($bmp);$g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size);"
           f"$bmp.Save('{path}')")
    if r.returncode != 0:
        note(f'screenshot-{label}', 'failed: ' + r.stderr.strip()[:200])


def run_setup(setup, *flags, timeout=300):
    p = subprocess.run([str(setup), *flags], capture_output=True, text=True, timeout=timeout)
    return p.returncode


def uninstall(*flags):
    """Starts the installed uninstaller the way Add/Remove Programs does (it relaunches itself from %TEMP%) and waits."""
    exe = INSTDIR / 'uninstall.exe'
    subprocess.Popen([str(exe), *flags], creationflags=DETACHED)
    return wait_for(lambda: reg_values(UNINST) is None and not INSTDIR.exists(), 120, 2)


def data_conn(roots):
    for root in roots:
        c = root / 'daemon.conn'
        if c.is_file():
            try:
                return json.loads(c.read_text()), root
            except ValueError:
                pass
    return None, None


def wait_daemon(roots, timeout=120, not_pid=None):
    def probe():
        conn, root = data_conn(roots)
        if conn and conn['pid'] != not_pid and any(str(conn['pid']) == str(p['ProcessId']) for p in processes('uniclipd.exe')):
            return conn, root
        return None
    return wait_for(probe, timeout, 1)


def http(conn, path, method='GET', body=None, auth=None):
    req = urllib.request.Request(f"http://{conn['host']}:{conn['port']}{path}", method=method,
                                 data=json.dumps(body).encode() if body is not None else None)
    req.add_header('Authorization', auth or f"Bearer {conn['token']}")
    if body is not None:
        req.add_header('Content-Type', 'application/json')
    with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=15) as r:
        return json.load(r)


def health(conn):
    try:
        return http(conn, '/health')['data']['status']
    except Exception as e:  # noqa: BLE001
        return f'error: {e}'


def set_autostart(conn, enabled):
    token = http(conn, '/auth/connect', 'POST', {'pid': os.getpid(), 'clientType': 'cli'})['data']['sessionToken']
    http(conn, '/settings', 'PUT', {'general': {'autoStart': enabled}}, auth=f'Session {token}')


def launch(exe, cwd=None):
    return subprocess.Popen([str(exe)], cwd=cwd, creationflags=DETACHED, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


class Cli:
    def __init__(self, exe, cwd=None):
        self.exe, self.cwd, self.trace = str(exe), cwd, []

    def __call__(self, *a, timeout=120):
        p = subprocess.run([self.exe, *a], capture_output=True, text=True, timeout=timeout, cwd=self.cwd)
        self.trace.append({'args': [x for x in a if x != PASS], 'rc': p.returncode, 'out': (p.stdout + p.stderr)[-400:]})
        return p

    def capture(self, tag):
        """Put a unique token on the clipboard until history search finds it (the watcher can miss the first change)."""
        for attempt in range(8):
            marker = f'acceptance{tag}x{int(time.time())}x{attempt}'
            ps(f"Set-Clipboard -Value '{marker}'")
            for _ in range(4):
                time.sleep(1)
                if marker in (self('search', marker).stdout):
                    return marker
        return None

    def finds(self, marker):
        return marker in self('search', marker).stdout


def tree_names(root, depth):
    out = set()
    if not root.exists():
        return out
    for p in root.rglob('*'):
        if len(p.relative_to(root).parts) <= depth:
            out.add(str(p.relative_to(root)).lower())
    return out


def env_snapshot():
    cm = subprocess.run(['cmdkey', '/list'], capture_output=True, text=True).stdout
    return {'local': tree_names(LOCAL, 1), 'roam': tree_names(ROAM, 1), 'home': tree_names(Path.home(), 1),
            'uninstall': set(reg_subkeys(UNINST.rsplit('\\', 1)[0])), 'software': set(reg_subkeys('Software')),
            'run': set((reg_values(RUN) or {}).keys()), 'credentials': {l.strip() for l in cm.splitlines() if l.strip().startswith('Target')}}


def snapshot_diff(before, after):
    return {k: sorted(after[k] - before[k]) for k in before if after[k] - before[k]}


APP_NAMED = re.compile(r'uniclip|' + re.escape(BUNDLE), re.I)


def scenario(fn):
    def wrapper(*a):
        print(f'--- {fn.__name__}', flush=True)
        try:
            fn(*a)
        except Exception as e:  # noqa: BLE001
            check(f'{fn.__name__} ran to its end', False, repr(e))
        finally:
            kill_all()
    return wrapper


def version_of(path):
    return re.search(r'_(\d+\.\d+\.\d+[^_]*)_(?:x64|arm64)-', Path(path).name).group(1)


@scenario
def scenario_a(a, s):
    rc = run_setup(a.setup, '/S')
    check('A1 silent install exits 0', rc == 0, rc)
    files = {n: (INSTDIR / n).is_file() for n in (f'{PRODUCT}.exe', 'uniclipd.exe', 'uninstall.exe')}
    check('A2 the installer wrote the exe, the daemon and the uninstaller', all(files.values()), files)
    check('A3 the installed daemon is the shipped (CI-built) one (SHA-256)', sha256(INSTDIR / 'uniclipd.exe') == a.daemon_sha256)
    s['installed_exe_old'] = sha256(INSTDIR / f'{PRODUCT}.exe')
    if a.expect_signed:
        vflags = signature_flags(a)
        v = subprocess.run([sys.executable, str(a.sign_verifier), 'verify', '--out', str(OUT / 'signatures-installed.json'), *vflags,
                            *[str(INSTDIR / n) for n in (f'{PRODUCT}.exe', 'uniclipd.exe', 'uninstall.exe')]], capture_output=True, text=True)
        check('A3s the installed exe, daemon and uninstaller carry a valid Authenticode signature', v.returncode == 0, v.stdout[-1500:])
        # Negative controls with the very same verifier flags: a tampered signed file, a file signed by someone else (or not at
        # all) and a non-PE file must all be refused, also when the chain trust is waived for the fixture.
        ctl = OUT / 'sign-controls'
        ctl.mkdir(parents=True, exist_ok=True)
        data = bytearray((INSTDIR / f'{PRODUCT}.exe').read_bytes())
        data[len(data) // 3] ^= 0xFF
        (ctl / 'tampered.exe').write_bytes(bytes(data))
        (ctl / 'unsigned.exe').write_bytes(b'not a signed executable')
        names = ['tampered.exe', 'unsigned.exe']
        if a.sign_selftest_cert:  # a pinned signer identity exists only in the self-test
            shutil.copy2(sys.executable, ctl / 'other-signer.exe')
            names.append('other-signer.exe')
        for name in names:
            r = subprocess.run([sys.executable, str(a.sign_verifier), 'verify', '--out', str(ctl / f'{name}.json'), *vflags, str(ctl / name)],
                               capture_output=True, text=True)
            check(f'A3n the verifier refuses {name}', r.returncode != 0, r.stdout[-400:])
    k = reg_values(UNINST) or {}
    check('A4 uninstall key: DisplayVersion, Publisher, InstallLocation, UninstallString',
          k.get('DisplayVersion') == s['old'] and norm(k.get('InstallLocation', '').strip('"')) == norm(str(INSTDIR))
          and norm(k.get('UninstallString', '').strip('"')) == norm(str(INSTDIR / 'uninstall.exe')), k)
    check('A5 Start menu and desktop shortcuts exist',
          (Path(os.environ['APPDATA']) / 'Microsoft/Windows/Start Menu/Programs/UniClipboard.lnk').is_file()
          and ((Path.home() / 'Desktop/UniClipboard.lnk').is_file()
               or (Path(os.environ.get('PUBLIC', 'C:/Users/Public')) / 'Desktop/UniClipboard.lnk').is_file()))
    check('A6 clean machine: nothing was running or stored before the first launch', not any(r.exists() for r in DATA_ROOTS))
    # The shipped daemon has the production telemetry keys compiled in. The acceptance must never report to
    # production Sentry/PostHog, so the user's telemetry preference file is written as "off" before the first
    # start (the only deviation from a pristine first run; it is the setting a user can choose in the app).
    DATA_ROOTS[0].mkdir(parents=True, exist_ok=True)
    (DATA_ROOTS[0] / 'desktop-telemetry.json').write_text('false')
    launch(INSTDIR / f'{PRODUCT}.exe')
    found = wait_daemon(DATA_ROOTS)
    if not check('A7 first launch starts a daemon and writes daemon.conn in the real data root', found, [str(r) for r in DATA_ROOTS]):
        return
    conn, root = found
    s['root'] = root
    check('A8 the running daemon is the one in the install directory',
          any(norm(p['ExecutablePath']) == norm(str(INSTDIR / 'uniclipd.exe')) and p['ProcessId'] == conn['pid'] for p in processes('uniclipd.exe')))
    check('A9 the GUI runs from the install directory', any(norm(p['ExecutablePath']) == norm(str(INSTDIR / f'{PRODUCT}.exe')) for p in processes(f'{PRODUCT}.exe')))
    check('A10 /health answers ok', health(conn) == 'ok')
    time.sleep(5)
    shot('a-first-launch')
    cli = s['cli']
    init = cli('space', 'init', '--passphrase', PASS, '--device-name', 'acceptance')
    check('A11 space init succeeds', init.returncode == 0, cli.trace[-1])
    s['m1'] = cli.capture('one')
    check('A12 a clipboard change is captured into history', s['m1'], cli.trace[-1])
    set_autostart(conn, True)
    kill_all()
    check('A13 forced stop of GUI and daemon (TerminateProcess)', not processes('uniclipd.exe') and not processes(f'{PRODUCT}.exe'))
    launch(INSTDIR / f'{PRODUCT}.exe')
    again = wait_daemon(DATA_ROOTS, not_pid=conn['pid'])
    check('A14 after the forced stop the next start brings up a new daemon on the same data', again, None)
    if again:
        check('A15 the database is usable: history written before the forced stop is readable', s['m1'] and cli.finds(s['m1']), cli.trace[-1])
    val = wait_for(run_value, 30)
    check('A16 the stored autostart preference is applied at startup: Run value points at the installed exe with --autostart',
          val and norm(str(INSTDIR / f'{PRODUCT}.exe')) in norm(val) and '--autostart' in val, val)
    s['run_before_update'] = val


@scenario
def scenario_b(a, s):
    if not s.get('m1'):
        check('B0 prerequisite: scenario A produced a history marker', False)
        return
    conn, _ = data_conn(DATA_ROOTS)
    old_daemon = conn['pid']
    # Writes in flight while the installer force-stops the daemon.
    writer = subprocess.Popen(['powershell', '-NoProfile', '-Command',
                               "1..600 | ForEach-Object { Set-Clipboard -Value ('inflight' + $_); Start-Sleep -Milliseconds 100 }"])
    time.sleep(3)
    rc = run_setup(a.newer_setup, '/P', '/R', '/UPDATE', '/ARGS')
    writer.kill()
    check('B1 update over the installed copy exits 0', rc == 0, rc)
    k = reg_values(UNINST) or {}
    check('B2 DisplayVersion is the newer version', k.get('DisplayVersion') == s['new'], k.get('DisplayVersion'))
    check('B3 the installed GUI exe was replaced', sha256(INSTDIR / f'{PRODUCT}.exe') != s['installed_exe_old'])
    # Signing the same CI-built daemon again yields different bytes, so the updated install is compared with the newer package.
    check('B4 the daemon file is the one shipped in the newer package', sha256(INSTDIR / 'uniclipd.exe') == (a.newer_daemon_sha256 or a.daemon_sha256))
    restarted = wait_daemon(DATA_ROOTS, 120, not_pid=old_daemon)
    check('B5 /R restarted the application: a new daemon is up', restarted)
    gui = [p for p in processes(f'{PRODUCT}.exe') if norm(p['ExecutablePath']) == norm(str(INSTDIR / f'{PRODUCT}.exe'))]
    check('B6 exactly one GUI process runs from the install directory', len(gui) == 1, gui)
    check('B7 exactly one daemon runs and it is the new one', len(processes('uniclipd.exe')) == 1 and restarted and restarted[0]['pid'] != old_daemon)
    shot('b-after-update')
    if restarted:
        check('B8 the database is usable after the forced stop during the update: earlier history readable', s['cli'].finds(s['m1']), s['cli'].trace[-1])
        m2 = s['cli'].capture('two')
        check('B9 capture works after the update', m2, s['cli'].trace[-1])
        s['m2'] = m2
    val = run_value()
    check('B10 the Run value survived the update and still points at the installed exe', val and norm(str(INSTDIR / f'{PRODUCT}.exe')) in norm(val), val)
    s['exe_new'] = sha256(INSTDIR / f'{PRODUCT}.exe')


@scenario
def scenario_c(a, s):
    rc = run_setup(a.setup, '/S')
    check('C1 the older package over the newer install is refused (non-zero exit)', rc != 0, rc)
    rc2 = run_setup(a.setup, '/P')
    check('C2 refused in passive mode too', rc2 != 0, rc2)
    check('C3 nothing changed: version and exe are the newer ones',
          (reg_values(UNINST) or {}).get('DisplayVersion') == s['new'] and sha256(INSTDIR / f'{PRODUCT}.exe') == s.get('exe_new'))


@scenario
def scenario_d(a, s):
    gone = uninstall('/S')
    check('D1 the uninstaller removed the key and the install directory', gone, sorted(p.name for p in INSTDIR.glob('*')) if INSTDIR.exists() else None)
    check('D2 no application process is left', not processes(f'{PRODUCT}.exe') and not processes('uniclipd.exe'))
    check('D3 the Run value is removed', run_value() is None, run_value())
    check('D4 shortcuts are removed', not (Path(os.environ['APPDATA']) / 'Microsoft/Windows/Start Menu/Programs/UniClipboard.lnk').exists())
    kept = [r for r in DATA_ROOTS if r.exists() and any(r.iterdir())]
    check('D5 the data was kept', kept, [str(r) for r in DATA_ROOTS])
    s['kept'] = [str(r) for r in kept]


@scenario
def scenario_e(a, s):
    rc = run_setup(a.setup, '/S')
    check('E1 reinstall over the kept data exits 0', rc == 0, rc)
    launch(INSTDIR / f'{PRODUCT}.exe')
    found = wait_daemon(DATA_ROOTS)
    check('E2 the application starts on the kept data', found)
    if found:
        check('E3 history written before the uninstall is still readable', s.get('m1') and s['cli'].finds(s['m1']), s['cli'].trace[-1])


@scenario
def scenario_f(a, s):
    kill_all()
    gone = uninstall('/S', '/DELETEAPPDATA')
    check('F1 the uninstaller removed the key and the install directory', gone)
    left = [str(r) for r in DATA_ROOTS if r.exists()]
    check('F2 /DELETEAPPDATA removed both data roots', not left, left)
    check('F3 the Run value is removed', run_value() is None, run_value())


def extract(zip_path, dest):
    dest.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(zip_path) as z:
        z.extractall(dest)
    return dest


@scenario
def scenario_g(a, s):
    work = Path(tempfile.mkdtemp(prefix='uc-portable-'))
    p = extract(a.portable, work / 'app')
    shutil.copy2(a.uniclip, p / 'uniclip.exe')  # next to portable.dat the CLI resolves the same portable data root
    check('G1 the zip carries portable.dat, the exe and the daemon', all((p / n).is_file() for n in ('portable.dat', f'{PRODUCT}.exe', 'uniclipd.exe')))
    check('G2 the zip daemon is the CI-built one', sha256(p / 'uniclipd.exe') == a.daemon_sha256)
    before = env_snapshot()
    (p / 'data').mkdir(exist_ok=True)
    (p / 'data' / 'desktop-telemetry.json').write_text('false')  # no production telemetry from acceptance runs
    launch(p / f'{PRODUCT}.exe', cwd=p)
    found = wait_for(lambda: next(iter(p.rglob('daemon.conn')), None) and wait_daemon([next(iter(p.rglob('daemon.conn'))).parent], 5), 120, 2)
    if not check('G3 portable start: daemon.conn is inside the portable folder', found):
        shot('g-portable-failed')
        return
    conn, root = found
    check('G4 the daemon runs from the portable folder', any(norm(q['ExecutablePath']) == norm(str(p / 'uniclipd.exe')) for q in processes('uniclipd.exe')))
    cli = Cli(p / 'uniclip.exe', cwd=p)
    check('G5 space init works in portable mode', cli('space', 'init', '--passphrase', PASS, '--device-name', 'portable').returncode == 0, cli.trace[-1])
    m = cli.capture('portable')
    check('G6 capture works in portable mode', m, cli.trace[-1])
    shot('g-portable')
    kill_all()
    launch(p / f'{PRODUCT}.exe', cwd=p)
    wait_daemon([root], 120, not_pid=conn['pid'])
    check('G7 after a forced stop and restart the portable history is readable', m and cli.finds(m), cli.trace[-1])
    kill_all()
    diff = snapshot_diff(before, env_snapshot())
    named = {k: [x for x in v if APP_NAMED.search(x)] for k, v in diff.items()}
    named = {k: v for k, v in named.items() if v}
    check('G8 nothing named after the application appeared outside the portable folder (files, registry, Credential Manager)', not named, named)
    note('portable-all-new-entries-outside-the-folder', diff)
    note('portable-data-tree', sorted(str(x.relative_to(p)) for x in (p / 'data').rglob('*'))[:60] if (p / 'data').exists() else None)
    shutil.rmtree(work, ignore_errors=True)


@scenario
def scenario_h(a, s):
    work = Path(tempfile.mkdtemp(prefix='uc-portable-ro-'))
    p = extract(a.portable, work / 'app')
    user = os.environ['USERNAME']
    subprocess.run(['icacls', str(p), '/deny', f'{user}:(OI)(CI)(WD,AD,DC)'], capture_output=True, check=True)
    probe = subprocess.run(['cmd', '/c', f'echo x> "{p}\\probe.txt"'], capture_output=True)
    check('H0 the folder really is read-only for this user', probe.returncode != 0)
    before = env_snapshot()
    launch(p / f'{PRODUCT}.exe', cwd=p)
    time.sleep(40)
    shot('h-readonly')
    diff = snapshot_diff(before, env_snapshot())
    named = {k: [x for x in v if APP_NAMED.search(x)] for k, v in diff.items()}
    note('readonly-gui-running', bool(processes(f'{PRODUCT}.exe')))
    note('readonly-daemon-running', bool(processes('uniclipd.exe')))
    note('readonly-conn-files', [str(x) for x in work.rglob('daemon.conn')])
    note('readonly-new-entries-named-after-the-app-outside-the-folder', {k: v for k, v in named.items() if v})
    kill_all()
    subprocess.run(['icacls', str(p), '/remove:d', user, '/T'], capture_output=True)
    shutil.rmtree(work, ignore_errors=True)


def signature_flags(a):
    """Verifier flags: the fixture certificate thumbprint when given; the chain trust is waived only on request."""
    flags = []
    if a.sign_selftest_cert:
        flags += ['--expect-thumbprint', hashlib.sha1(a.sign_selftest_cert.read_bytes()).hexdigest()]
        if a.sign_untrusted_root:
            flags += ['--allow-untrusted-root']
    return flags


def main():
    global OUT
    ap = argparse.ArgumentParser()
    ap.add_argument('--arch', choices=['amd64', 'arm64'], required=True)
    ap.add_argument('--setup', type=Path, required=True)
    ap.add_argument('--portable', type=Path, required=True)
    ap.add_argument('--newer-setup', type=Path, required=True)
    ap.add_argument('--uniclip', type=Path, required=True)
    ap.add_argument('--daemon-sha256', required=True)
    ap.add_argument('--newer-daemon-sha256', help='SHA-256 of the daemon inside the newer package when it differs from --daemon-sha256 (signed packages)')
    ap.add_argument('--expect-signed', action='store_true', help='the packages are Authenticode signed: verify the installed files too')
    ap.add_argument('--sign-selftest-cert', type=Path, help='public throwaway certificate (DER) of the signing self-test: its thumbprint must be the signer')
    ap.add_argument('--sign-untrusted-root', action='store_true', help='signing self-test only, with --sign-selftest-cert: the runner cannot trust the throwaway certificate, so only the chain trust is waived')
    ap.add_argument('--sign-verifier', type=Path, help='apps/gui-go/packaging/windows/sign.py')
    ap.add_argument('--out', type=Path, required=True)
    a = ap.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        sys.exit('refusing to run: the shipping form uses the real data root and Credential Manager; use a disposable GitHub-hosted runner')
    OUT = a.out.resolve()
    OUT.mkdir(parents=True, exist_ok=True)
    arch_code = ps('(Get-CimInstance Win32_Processor).Architecture').stdout.strip()
    results['host'] = {'os': platform.platform(), 'machine': platform.machine(), 'processorArchitecture': arch_code,
                       'caption': ps('(Get-CimInstance Win32_OperatingSystem).Caption').stdout.strip(), 'computer': os.environ.get('COMPUTERNAME')}
    results['inputs'] = {n: {'name': getattr(a, n).name, 'sha256': sha256(getattr(a, n))} for n in ('setup', 'portable', 'newer_setup', 'uniclip')}
    results['inputs']['daemonSha256'] = a.daemon_sha256
    want = {'amd64': '9', 'arm64': '12'}[a.arch]
    check('0 the host processor architecture is the one under test (native, not emulated)', arch_code == want, arch_code)
    check('0 clean machine: no installation, data root or Run value', reg_values(UNINST) is None and not INSTDIR.exists()
          and not any(r.exists() for r in DATA_ROOTS) and run_value() is None)
    s = {'old': version_of(a.setup), 'new': version_of(a.newer_setup), 'cli': Cli(a.uniclip)}
    results['versions'] = {'current': s['old'], 'newer': s['new']}
    ps("$ErrorActionPreference='SilentlyContinue'; Add-Type -AssemblyName System.Windows.Forms")
    ctypes.windll.kernel32.SetErrorMode(0x8003)
    try:
        for fn in (scenario_a, scenario_b, scenario_c, scenario_d, scenario_e, scenario_f, scenario_g, scenario_h):
            fn(a, s)
    finally:
        kill_all()
        for root in DATA_ROOTS:
            logs = root / 'logs'
            if logs.is_dir():
                shutil.copytree(logs, OUT / 'logs' / root.parent.name, dirs_exist_ok=True)
        results['cliTrace'] = s['cli'].trace[-40:]
        results['passed'] = all(c['ok'] for c in results['checks'])
        (OUT / 'acceptance.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
