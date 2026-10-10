#!/usr/bin/env python3
"""Windows production-form E2E (slice 17b). STATUS: AUTHORED, NOT YET RUN.

No Windows host was available when this was written (runner uniclipboard-windows-x64-vm-01: offline, read-only
`gh api`, 2026-10-06), so nothing below has executed on Windows. Every check is unverified until a run produces
artifacts; macOS-side evidence for the shared logic is in `library/e2e-windows-17b/` and does not replace this.

Run on a DEDICATED, disposable Windows session (a test VM): it sends real key events, writes HKCU Run/Uninstall values
and runs the installer. It refuses to start unless UC_GUI_GO_E2E_DEDICATED_HOST=1, and refuses part C when a
UniClipboard installation or process already exists (the installer's hook kills `UniClipboard.exe`/`uniclipd.exe` by image name).

    python build_windows.py --mode e2e                                   # e2e binaries (gui-go.exe, uniclipd.exe, uniclip.exe)
    python package_windows.py --arch amd64 --daemon <REAL uniclipd.exe> --out <new dir>
    set UC_GUI_GO_E2E_DEDICATED_HOST=1
    python windows_production_run.py --out <dir> [--include-run-value] [--include-packaging --package <package dir>]

Default scope is A and B only. C writes under the session's real HKCU Run key (autostart/legacy migration) and D/E are the
packaging acceptance; neither is part of the native-host acceptance, so both stay off unless asked for.

Isolation: parts A and B run from throwaway `uc-gui-go-*` directories with UC_PORTABLE=1 (neither the host nor the
daemon read HOME/LOCALAPPDATA, and without portable mode the daemon would use the real Credential Manager) and
UC_DISABLE_SYSTEM_CLIPBOARD=1. Only PIDs this script started are stopped. The data root is verified to be inside the
sandbox before anything is trusted. `taskkill`/TerminateProcess are forced terminations; they are used only on the
PIDs recorded for this run.

  A  (e2e build) daemon stop at exit: GUI exit 0 and its daemon gone (TerminateProcess + handle wait)
  B  (e2e build) modifier double-tap with REAL SendInput Alt taps: two taps trigger, one tap / slow second tap / another
     key do not; the Win32 GetAsyncKeyState read and the 20 ms poll are what is under test here
  C  (opt-in: --include-run-value) legacy login item: a Run value of the item's name that launches another exe is swept; enable/disable
     round trip through Wails writes and removes the value for this executable
  D  (opt-in: --include-packaging; release portable zip) production entry: no UC_PROFILE accepted, data root = <portable dir>\\data\\app.uniclipboard.desktop,
     the daemon is the sibling uniclipd.exe, a second launch exits and leaves the first instance and daemon alone
  E  (opt-in: --include-packaging; release setup.exe) silent install to a temp dir, registry identity (InstallLocation/UninstallString), a second
     `/UPDATE /ARGS` run over it, silent uninstall; no /R (the installed app is not started: it would use the real profile)
"""
import argparse
import ctypes
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import windows_quick_panel_run as q  # noqa: E402  (helpers: send_chord, Gui-like control file protocol)

UNINST = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard'
RUN = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Run'
q.VK.setdefault('alt', 0x12)
q.VK.setdefault('x', 0x58)


def reg_query(key, value=None):
    cmd = ['reg', 'query', key] + (['/v', value] if value else [])
    r = subprocess.run(cmd, capture_output=True, text=True, errors='replace')
    return r.stdout if r.returncode == 0 else None


def process_running(image):
    out = subprocess.run(['tasklist', '/FI', f'IMAGENAME eq {image}', '/NH'], capture_output=True, text=True, errors='replace').stdout
    return image.lower() in out.lower()


def pid_alive(pid):
    return str(pid) in subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH'], capture_output=True, text=True, errors='replace').stdout


def process_path(pid):
    r = subprocess.run(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {pid}).Path'], capture_output=True, text=True, errors='replace')
    return r.stdout.strip()


class Gui(q.Gui):
    """The control-file protocol of the e2e build, with this script's own verbs."""

    def modifier_state(self, label):
        return self.ctl(f'modifier-state {label}', f'modifier-state-{label}')['detail']


def tap(key='alt', hold=0.06):
    q.send_chord(key, hold=hold)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, default=q.ROOT / 'target/gui-go/windows-e2e')
    parser.add_argument('--package', type=Path, help='output directory of package_windows.py (with the REAL daemon); only needed with --include-packaging')
    parser.add_argument('--include-run-value', action='store_true',
                        help='also run part C: it seeds, sweeps and writes values under the session\'s real HKCU Run key (autostart/legacy migration, not the native-host scope)')
    parser.add_argument('--include-packaging', action='store_true',
                        help='also run parts D and E (portable zip, installer): they belong to the Windows packaging acceptance')
    args = parser.parse_args()
    if os.name != 'nt':
        sys.exit('this script runs on Windows only')
    if os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('refusing to run on a session that is not a dedicated test host (set UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    if args.include_packaging and not args.package:
        sys.exit('--include-packaging needs --package')
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f'{out} is not empty: failure artifacts are never overwritten, pick a new directory')
    out.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((args.package / 'package-manifest.json').read_text()) if args.package else None
    if manifest and (manifest['source'].get('dirty') or manifest.get('windowsRuntimeVerified') is not False):
        print('note: package built from a dirty tree; recorded in the results', flush=True)
    results = {'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME'), 'package': manifest['source'] if manifest else None, 'dedicatedHost': True,
               'parts': 'AB' + ('C' if args.include_run_value else '') + ('DE' if args.include_packaging else '')}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    started = []  # (Popen) objects this script owns
    owned_daemon_pids = []
    cleanup_dirs, cleanup_run_values = [], []
    try:
        # ---------------- parts A-C: e2e build in a portable sandbox ----------------
        sandbox, profile, _ = q.make_sandbox()
        cleanup_dirs.append(sandbox)
        for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe'):
            shutil.copy2(args.binaries / name, sandbox / name)
        base_env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
        gui_env = dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_EXIT_MODE='full')
        gui_env.pop('UC_GPUI_QUICK_PANEL', None)
        uniclip = str(sandbox / 'uniclip.exe')
        # The login item name of this profile is "<productName>-<profile>"; seed a legacy value of that name.
        item = f'UniClipboard-{profile}'
        if args.include_run_value:
            cleanup_run_values.append(item)
        legacy = r'"C:\uc-gui-go-nonexistent\old\UniClipboard.exe" --autostart'
        if args.include_run_value:
            seeded = subprocess.run(['reg', 'add', RUN, '/v', item, '/t', 'REG_SZ', '/d', legacy, '/f'], capture_output=True).returncode == 0
            check('C0 seeded a legacy Run value for this sandbox profile only', seeded and reg_query(RUN, item) is not None)

        subprocess.run([uniclip, 'space', 'init', '--passphrase', 'windows-production-pass', '--device-name', 'win-prod'], env=base_env, check=True, timeout=120)
        gui = Gui(sandbox, gui_env, out)
        started.append(gui.proc)
        gui.step('bootstrapped', 120)
        conn = sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn'
        check('data root is inside the sandbox (daemon.conn)', conn.is_file(), str(conn))
        daemon_pid = json.loads(conn.read_text())['pid']
        owned_daemon_pids.append(daemon_pid)
        check('the daemon runs from the sandbox copy', Path(process_path(daemon_pid)).resolve() == (sandbox / 'uniclipd.exe').resolve(), process_path(daemon_pid))

        # C: legacy sweep happens at startup reconcile (asynchronously)
        if args.include_run_value:
            for _ in range(40):
                if reg_query(RUN, item) is None:
                    break
                time.sleep(.25)
            check('C1 the legacy Run value pointing at another exe was removed at startup', reg_query(RUN, item) is None)
            r = gui.invoke('as-on', 'update_autostart', {'enabled': True})
            value = reg_query(RUN)
            exe_lower = str(sandbox / 'gui-go.exe').lower()
            check('C2 enabling writes a Run value for this executable (Wails)', r['ok'] and value and exe_lower in value.lower(), r)
            r = gui.invoke('as-off', 'update_autostart', {'enabled': False})
            value = reg_query(RUN)
            check('C3 disabling removes it', r['ok'] and not (value and exe_lower in value.lower()), r)

        # B: real key taps
        r = gui.invoke('mod-on', 'set_quick_panel_double_tap_modifier', {'modifier': 'alt'})
        check('B0 alt selected, availability supported', r['ok'] and gui.invoke('avail', 'get_quick_panel_double_tap_availability')['data'] == 'supported', r)
        time.sleep(.5)
        base = gui.modifier_state('b0')['triggers']
        tap(); time.sleep(.12); tap(); time.sleep(.6)
        two = gui.modifier_state('b1')['triggers']
        check('B1 two real Alt taps within 400 ms trigger once', two == base + 1, [base, two])
        tap(); time.sleep(.7)
        one = gui.modifier_state('b2')['triggers']
        check('B2 a single tap does not trigger', one == two, [two, one])
        tap(); time.sleep(.6); tap(); time.sleep(.6)
        slow = gui.modifier_state('b3')['triggers']
        check('B3 a second tap after 600 ms does not trigger', slow == one, [one, slow])
        q.send_chord('alt', 'x', hold=.06); time.sleep(.12); tap(); time.sleep(.6)
        other = gui.modifier_state('b4')['triggers']
        check('B4 Alt+X then a tap does not trigger', other == slow, [slow, other])

        # A: stop at exit
        gui.ctl('exit', 'control-exit')
        code = gui.proc.wait(timeout=60)
        deadline = time.monotonic() + 20
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        check('A1 GUI exit 0 and its daemon is gone (TerminateProcess + handle wait)', code == 0 and not pid_alive(daemon_pid), {'exit': code})

        if args.include_packaging:
            # ---------------- part D: release portable zip ----------------
            zips = list(args.package.glob('*-portable.zip'))
            check('D0 the package has one portable zip', len(zips) == 1, [str(z) for z in zips])
            portable = Path(tempfile.mkdtemp(prefix='uc-gui-go-portable-'))
            cleanup_dirs.append(portable)
            with zipfile.ZipFile(zips[0]) as z:
                z.extractall(portable)
            rel_env = {k: v for k, v in os.environ.items() if k not in ('UC_PROFILE', 'UNICLIPBOARD_ENV', 'UC_PORTABLE')}
            rel_env['UC_DISABLE_SYSTEM_CLIPBOARD'] = '1'
            bad = subprocess.run([str(portable / 'UniClipboard.exe')], env=dict(rel_env, UC_PROFILE='x'), capture_output=True, text=True, timeout=30)
            check('D1 the release build refuses UC_PROFILE', bad.returncode != 0, [bad.returncode, bad.stdout, bad.stderr])
            first = subprocess.Popen([str(portable / 'UniClipboard.exe')], env=rel_env, cwd=portable, stdout=(out / 'release.log').open('w'), stderr=subprocess.STDOUT)
            started.append(first)
            rconn = portable / 'data' / 'app.uniclipboard.desktop' / 'daemon.conn'
            for _ in range(240):
                if rconn.is_file() or first.poll() is not None:
                    break
                time.sleep(.5)
            check('D2 production data root (no profile) is <portable>\\data\\app.uniclipboard.desktop', rconn.is_file(), str(rconn))
            rpid = json.loads(rconn.read_text())['pid'] if rconn.is_file() else None
            if rpid:
                owned_daemon_pids.append(rpid)
            check('D3 the daemon is the sibling uniclipd.exe', rpid and Path(process_path(rpid)).resolve() == (portable / 'uniclipd.exe').resolve(), process_path(rpid) if rpid else None)
            t0 = time.monotonic()
            second = subprocess.run([str(portable / 'UniClipboard.exe')], env=rel_env, cwd=portable, timeout=60, capture_output=True)
            check('D4 a second launch exits 0 quickly; the first instance and daemon are untouched',
                  second.returncode == 0 and time.monotonic() - t0 < 20 and first.poll() is None and pid_alive(rpid), [second.returncode, time.monotonic() - t0])

            # ---------------- part E: installer ----------------
            first.terminate()  # forced; only the Popen this script owns
            first.wait(timeout=30)
            if rpid and pid_alive(rpid):
                subprocess.run(['taskkill', '/F', '/PID', str(rpid)], capture_output=True)  # the recorded daemon of the portable sandbox only
            time.sleep(1)
            if (reg_query(UNINST) is not None or reg_query(RUN, 'UniClipboard') is not None
                    or process_running('UniClipboard.exe') or process_running('uniclipd.exe')):
                checks.append({'check': 'E *', 'ok': None, 'skipped': 'an installation, a UniClipboard Run value or a UniClipboard process already exists on this host'})
            else:
                setups = list(args.package.glob('*-setup.exe'))
                check('E0 the package has one setup exe', len(setups) == 1, [str(x) for x in setups])
                if len(setups) == 1:
                    inst = Path(tempfile.mkdtemp(prefix='uc-gui-go-inst-')) / 'app'
                    cleanup_dirs.append(inst.parent)
                    cleanup_run_values.append('UniClipboard')
                    r = subprocess.run([str(setups[0]), '/S', f'/D={inst}'], timeout=300)
                    loc = reg_query(UNINST, 'InstallLocation') or ''
                    check('E1 silent install: files, InstallLocation (quoted), UninstallString',
                          r.returncode == 0 and (inst / 'UniClipboard.exe').is_file() and (inst / 'uniclipd.exe').is_file() and (inst / 'uninstall.exe').is_file()
                          and str(inst).lower() in loc.lower() and reg_query(UNINST, 'UninstallString') is not None, [r.returncode, loc])
                    check('E2 no portable.dat is installed (installed form uses the user data root)', not (inst / 'portable.dat').exists())
                    r = subprocess.run([str(setups[0]), '/S', '/UPDATE', '/ARGS', '--autostart', f'/D={inst}'], timeout=300)
                    check('E3 a second run with /UPDATE /ARGS over the same directory succeeds and keeps the install', r.returncode == 0 and (inst / 'UniClipboard.exe').is_file(), r.returncode)
                    if not (inst / 'uninstall.exe').is_file():
                        check('E4 silent uninstall removes the exe, the daemon and the registry identity', False, 'uninstall.exe was not installed')
                    else:
                        r = subprocess.run([str(inst / 'uninstall.exe'), '/S', f'_?={inst}'], timeout=300)
                        time.sleep(1)
                        check('E4 silent uninstall removes the exe, the daemon and the registry identity',
                              r.returncode == 0 and not (inst / 'UniClipboard.exe').exists() and not (inst / 'uniclipd.exe').exists() and reg_query(UNINST) is None, r.returncode)
        results['passed'] = all(c['ok'] is not False for c in checks) and not any(c['ok'] is None for c in checks)
    finally:
        for p in started:
            if p.poll() is None:
                p.terminate()
        for pid in owned_daemon_pids:
            if pid and pid_alive(pid):
                subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        for name in cleanup_run_values:
            subprocess.run(['reg', 'delete', RUN, '/v', name, '/f'], capture_output=True)
        for d in cleanup_dirs:
            shutil.rmtree(d, ignore_errors=True)
        (out / 'windows-production-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps({'passed': results['passed'], 'checks': len(checks)}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
