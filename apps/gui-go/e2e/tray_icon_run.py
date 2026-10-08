#!/usr/bin/env python3
"""Tray icon (B solid cat) acceptance on macOS: docs/architecture/gui-go-tray-icon.md.

The real GUI (e2e build) runs against its own real daemon in a task-owned HOME and profile. The tray shows one static image, so what is checked:

  1  design alignment: the renderer's resting cat against the design board rasterized independently (e2e/tray-icon-design/synced.png), and
     the macOS template rule that the eyes are transparent knock-outs (control `tray-icon-compare`); the platform image is exported
  0  the status item of this pid is reachable on an unlocked, awake screen; if not, the native checks are reported as blocked
  2  the REAL status item is captured as a tight crop of the menu bar around this pid's status item
  3  the menu does not regress: right click opens it with the unchanged labels; the sync item flips syncEnabled in the daemon and back;
     Quit pressed in the real menu exits this exact GUI pid with 0 and the exact daemon pid goes away

Boundaries: macOS only, this Mac session (Accessibility granted to the terminal). The runner synthesizes mouse clicks on this pid's own status
item (tray_ax.swift re-verifies the target) and takes full-screen captures that are cropped at once to the menu bar around the status item. The
menu bar appearance (light or dark) is whatever the session has; the runner never changes system appearance. Windows and Linux are not exercised.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import ROOT, isolated_env  # noqa: E402
import tray_tracking_run as tt  # noqa: E402

DESIGN = HERE / 'tray-icon-design'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def backing_scale():
    out = subprocess.run(['swift', '-e', 'import AppKit; print(NSScreen.main!.backingScaleFactor)'], capture_output=True, text=True, timeout=120).stdout.strip()
    return float(out or 2)


def capture_item(pid, dest, scale, pad=3):
    """Crop the menu bar around this pid's status item (item frame +/- pad points): the full capture goes to a private temp file and is
    deleted at once, so nothing but the cropped slice of the menu bar is kept."""
    item = tt.ax('describe', str(pid))
    frame = (item.get('items') or [{}])[0].get('frame')
    if not frame:
        return {'ok': False, 'error': 'no AX frame for the status item'}
    tmp = Path(tempfile.mkdtemp(prefix='uc-gui-go-shot-'))
    try:
        full = tmp / 'full.png'
        if subprocess.run(['screencapture', '-x', str(full)], capture_output=True, timeout=30).returncode != 0 or not full.exists():
            return {'ok': False, 'error': 'screencapture failed'}
        x0, y0 = max(0, int(frame['x'] - pad)), max(0, int(frame['y'] - pad))
        w, h = int(frame['w'] + 2 * pad), int(frame['h'] + 2 * pad)
        p = subprocess.run(['sips', '-c', str(int(h * scale)), str(int(w * scale)), '--cropOffset', str(int(y0 * scale)), str(int(x0 * scale)), str(full), '--out', str(dest)],
                           capture_output=True, text=True, timeout=30)
        return {'ok': p.returncode == 0 and Path(dest).exists(), 'frame': frame, 'crop': {'x': x0, 'y': y0, 'w': w, 'h': h, 'scale': scale}}
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def preflight(pid):
    """Is this pid's status item reachable on the screen the user could see? A locked session (loginwindow's full-screen window) or a sleeping
    display make every capture a blurred wallpaper and every verified click refuse, so the native checks are blocked, not failed."""
    item = tt.ax('describe', str(pid))
    frame = (item.get('items') or [{}])[0].get('frame') or {}
    cx, cy = frame.get('x', 0) + frame.get('w', 0) / 2, frame.get('y', 0) + frame.get('h', 0) / 2
    hit = tt.ax('elementat', str(pid), str(cx), str(cy))
    display = tt.ax('display', '0')
    return {'frame': frame, 'hit': {k: hit.get(k) for k in ('role', 'ownerPid', 'mine', 'title', 'identifier')}, 'display': display,
            'reachable': tt.is_ours(hit, pid) and display.get('active') and not display.get('asleep')}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--skip-menu', action='store_true', help='skip the real-menu scenario (3)')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    (out / 'shots').mkdir(exist_ok=True)
    tt.TOOLS_INFO.update(tt.build_tools())
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence, control = out / 'gui.jsonl', out / 'gui.control'
    evidence.write_text('')
    control.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_DISABLE_SYSTEM_CLIPBOARD': '1',
                                          'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'wake', 'UC_GUI_GO_EXIT_MODE': 'full',
                                          'UC_GUI_GO_E2E_CONTROL_FILE': str(control), 'UC_GUI_GO_E2E_NATIVE_STATE': '1',
                                          'UC_GUI_GO_E2E_SECRET': 'tray-icon', 'UC_GUI_GO_E2E_NOTIFY_LOG': str(out / 'notifications.log')})
    binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
    results = {'profile': profile, 'checks': [], 'passed': False, 'note': 'the tray shows one static image; no state or animation is checked'}

    def check(name, ok, detail=None):
        results['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)
        return ok

    (out / 'provenance.json').write_text(json.dumps({
        'desktopHead': subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip(),
        'dirty': bool(subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, capture_output=True, text=True).stdout.strip()),
        'guiBinarySha256': sha(binary), 'daemonSha256': sha(ROOT / 'target/debug/uniclipd'), 'tools': tt.TOOLS_INFO,
        'runnerSha256': sha(__file__), 'designSnapshot': (DESIGN / 'SHA256SUMS').read_text(),
        'go': subprocess.run(['go', 'version'], capture_output=True, text=True).stdout.strip(),
        'engineRev': next((l.split('rev=')[1].split('#')[0] for l in (ROOT / 'Cargo.lock').read_text().splitlines() if 'Engine.git?rev=' in l), None),
        'daemonOrigin': 'target/debug/uniclipd of this worktree (cargo build --locked -p uc-daemon, debug); not independently attested'}, indent=2) + '\n')

    proc = None
    gui = None
    try:
        scale = backing_scale()
        results['backingScale'] = scale
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT)
        gui = tt.Gui(proc, evidence, control)
        results['guiPid'] = proc.pid
        gui.wait_step('bootstrapped', timeout=180)

        # 1. design alignment
        res = gui.ctl(f'tray-icon-compare:{DESIGN}', 'tray-icon-compare')
        check('1 renderer image matches the independently rasterized design board and the template alpha rule holds', res['ok'], res['detail'])
        exp = gui.ctl(f'tray-icon-export:{out / "frames"}', 'tray-icon-export')
        check('1 the platform image and the reference image export', exp['ok'], exp['detail'])

        pre = preflight(proc.pid)
        results['preflight'] = pre
        native = bool(pre['reachable'])
        check('0 this pid\'s status item is reachable on an unlocked, awake screen (native captures and menu clicks need it)', native, pre)

        # 2. the real status item
        shot = capture_item(proc.pid, out / 'shots' / 'status-item.png', scale) if native else {'ok': False, 'blocked': 'session not visible'}
        check('2 the real status item was captured', shot['ok'], shot)

        # 3. the real menu
        if not args.skip_menu and native:
            settings_path = '/settings'

            def dget(p):
                r = subprocess.run([tt.DAEMONGET, p], env=env, capture_output=True, text=True, timeout=60)
                return json.loads(r.stdout) if r.returncode == 0 else None

            def wait_daemon(pred, timeout=40):
                end, last = time.time() + timeout, None
                while time.time() < end:
                    last = dget(settings_path)
                    if last is not None and pred(last):
                        return last
                    time.sleep(1)
                return last

            def open_and_read(label):
                opened, read = tt.open_menu(gui, proc.pid, 'rightclick', label, out)
                return opened, read

            def press(label_title):
                rd = tt.ax('read', str(proc.pid))
                if not rd.get('ok') or not rd.get('popupWindows'):
                    return {'ok': False, 'error': 'menu not open before the press', 'read': {k: rd.get(k) for k in ('ok', 'popupWindows', 'error')}}
                return tt.ax('press', str(proc.pid), label_title)

            sync0 = ((dget(settings_path) or {}).get('sync') or {}).get('syncEnabled')
            _, rd = open_and_read('m1')
            titles = [i['title'] for i in rd.get('menu', [])]
            check('3 right click on this pid\'s status item opens the real menu with the unchanged labels',
                  rd.get('ok') and titles[0] in ('Disable Sync', 'Enable Sync') and 'Device Sync' in titles and 'Quit' in titles, {'titles': titles})
            label0 = titles[0] if titles else None
            p = press(label0)
            s1 = wait_daemon(lambda x: ((x.get('sync') or {}).get('syncEnabled')) is (not sync0))
            check('3 pressing the sync item in the real menu flips syncEnabled in the DAEMON', p.get('ok') and ((s1 or {}).get('sync') or {}).get('syncEnabled') is (not sync0), {'label': label0, 'daemon': (s1 or {}).get('sync')})
            time.sleep(1)
            _, rd = open_and_read('m2')
            label1 = rd['menu'][0]['title'] if rd.get('ok') else None
            press(label1)
            s2 = wait_daemon(lambda x: ((x.get('sync') or {}).get('syncEnabled')) is sync0)
            check('3 the second press restores syncEnabled in the daemon', ((s2 or {}).get('sync') or {}).get('syncEnabled') is sync0, (s2 or {}).get('sync'))

            daemons = tt.daemon_pids(home, profile)
            time.sleep(1)
            open_and_read('m3')
            q = press('Quit')
            try:
                rc = proc.wait(timeout=40)
            except subprocess.TimeoutExpired:
                rc = None
            time.sleep(3)
            gone = [d for d in daemons if subprocess.run(['ps', '-p', str(d['pid'])], capture_output=True).returncode == 0]
            check('3 Quit pressed in the real menu exits this GUI pid with 0 and the exact daemon pid goes away', q.get('ok') and rc == 0 and daemons and not gone, {'rc': rc, 'daemons': daemons, 'stillRunning': gone})
        results['passed'] = all(c['ok'] for c in results['checks'])
    except BaseException as exc:  # noqa: BLE001 - failure artifacts are kept
        results['error'] = f'{type(exc).__name__}: {exc}'
        print('ERROR', results['error'], flush=True)
        raise
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                proc.kill()
        for d in tt.daemon_pids(home, profile):
            subprocess.run(['kill', str(d['pid'])], capture_output=True)  # only the pid found through this profile's own lock file
        (out / 'assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
        shutil.rmtree(home, ignore_errors=True)
    print(json.dumps({'passed': results['passed'], 'checks': len(results['checks'])}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
