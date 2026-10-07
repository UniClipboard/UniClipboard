#!/usr/bin/env python3
"""Tray icon (B solid cat) acceptance on macOS: docs/architecture/gui-go-tray-icon.md.

The real GUI (e2e build) runs against its own real daemon in a task-owned HOME and profile. What is checked:

  1  design alignment: the renderer's frames against the design boards rasterized independently (e2e/tray-icon-design), and the macOS
     template rule that eyes and the badge ring are transparent knock-outs (control `tray-icon-compare`)
  2  initial state: "synced", no animation, no timer, no cached frame churn
  3  every visual state on the REAL status item: MANUAL states (control `tray-icon-manual`, not daemon facts), each captured as a tight
     crop of the menu bar around this pid's status item
  4  ear motion: each animation is started MANUALLY (control `tray-icon-play`), its frames are read from the host's records and compared with the
     design's keyframes (expectations below are typed from the board's keyframe strip, independently of the Go timelines); the real status item is
     captured in a burst while it plays; while it plays the tray menu is not published (tray-publish records) and afterwards no animation or
     timer remains; idle CPU is measured
  5  authoritative chain: the sync item is pressed in the REAL tray menu (Accessibility API on this pid's own status item); the DAEMON's
     syncEnabled flips and the icon changes to "paused" because the host read it from the daemon; pressing again restores "synced"
  6  the menu does not regress: right click opens it with the unchanged labels; Quit pressed in the real menu exits this exact GUI pid with 0 and
     the exact daemon pid goes away

Boundaries (also in the result): macOS only, this Mac session (Accessibility granted to the terminal). The runner synthesizes mouse clicks on this
pid's own status item (tray_ax.swift re-verifies the target) and takes full-screen captures that are cropped at once to the menu bar around the status
item; nothing else is kept. The menu bar appearance (light or dark) is whatever the session has; the runner never changes system appearance, so the
other appearance is only simulated by compositing the template alpha over both colours. Windows and Linux are not exercised here.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import ROOT, isolated_env, read_steps  # noqa: E402
import tray_tracking_run as tt  # noqa: E402

DESIGN = HERE / 'tray-icon-design'
STATES = ['synced', 'transferring', 'paused', 'lan-only', 'offline', 'not-recording', 'locked', 'attention', 'synced+dot']

# The design's keyframe strips ("TrayBMotion"), typed here on their own: (ms, left ear degrees, right ear degrees).
STRIPS = {
    'new': {'ms': 520, 'keys': [(0, 0, 0), (110, 0, 18), (250, 0, 0), (370, 0, 12), (520, 0, 0)]},
    'sent': {'ms': 360, 'keys': [(0, 0, 0), (190, -14, 14), (360, 0, 0)]},
    'attention': {'ms': 700, 'keys': [(0, 0, 0), (110, -15, 15), (250, 0, 0), (390, -15, 15), (530, 0, 0), (670, -15, 15), (700, 0, 0)]},
    'transferring': {'ms': 3000, 'loop': 1600, 'keys': [(0, 0, 0), (400, -9, 0), (800, 0, 0), (1200, 0, 9), (1600, 0, 0)]},
}
JITTER_DEG = 5  # a frame lands up to ~25 ms off its ideal time; the steepest design slope is 0.16 deg/ms


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def interpolate(keys, t):
    for (t0, l0, r0), (t1, l1, r1) in zip(keys, keys[1:]):
        if t <= t1:
            k = 0 if t1 == t0 else (t - t0) / (t1 - t0)
            return l0 + (l1 - l0) * k, r0 + (r1 - r0) * k
    return 0.0, 0.0


def expected_pose(strip, t):
    if strip.get('loop'):
        t %= strip['loop']
    return interpolate(strip['keys'], t)


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


def cpu_seconds(pid):
    out = subprocess.run(['ps', '-o', 'cputime=', '-p', str(pid)], capture_output=True, text=True).stdout.strip()
    parts = out.replace('.', ':').split(':')
    # [[hh:]mm:]ss.cc
    cs = int(parts[-1]) / 100 if len(parts) >= 2 and len(parts[-1]) == 2 else 0
    nums = [int(x) for x in parts[:-1]] if len(parts) >= 2 else [int(parts[0])]
    sec = 0
    for n in nums:
        sec = sec * 60 + n
    return sec + cs


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--skip-menu', action='store_true', help='skip the real-menu scenarios (5 and 6)')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    (out / 'shots').mkdir(exist_ok=True)
    tt.TOOLS_INFO.update(tt.build_tools())
    patch_bin = tt.TOOLS_DIR / 'daemonpatch'
    patch_src = sorted((HERE / 'linux' / 'daemonpatch').glob('*.go'))
    p = subprocess.run(['go', 'build', '-o', str(patch_bin), './e2e/linux/daemonpatch'], cwd=HERE.parent, capture_output=True, text=True, timeout=600)
    if p.returncode != 0:
        raise RuntimeError('building daemonpatch failed: ' + p.stderr[-500:])
    tt.TOOLS_INFO['daemonpatch'] = {'sourceSha256': hashlib.sha256(b''.join(f.read_bytes() for f in patch_src)).hexdigest(), 'binarySha256': sha(patch_bin),
                                    'command': 'go build -o target/gui-go/e2e-tools/daemonpatch ./e2e/linux/daemonpatch'}
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
    results = {'profile': profile, 'checks': [], 'passed': False, 'manual': 'states set with tray-icon-manual and animations started with tray-icon-play are MANUAL; the sync chain is authoritative'}

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
        # The icon feed reads the daemon once at start; give that first snapshot time before the first state check.
        time.sleep(4)

        # 1. design alignment
        res = gui.ctl(f'tray-icon-compare:{DESIGN}', 'tray-icon-compare')
        check('1 renderer frames match the independently rasterized design boards and the template alpha rules hold', res['ok'], res['detail'])
        exp = gui.ctl(f'tray-icon-export:{out / "frames"}', 'tray-icon-export')
        check('1 every state exports as platform and reference frames', exp['ok'], exp['detail'])

        pre = preflight(proc.pid)
        results['preflight'] = pre
        native = bool(pre['reachable'])
        check('0 this pid\'s status item is reachable on an unlocked, awake screen (native captures and menu clicks need it)', native, pre)

        # 2. initial state
        st = gui.ctl('tray-icon-state', 'tray-icon-state')
        d = st['detail']
        check('2 initial state is "synced", still, with no timer', d['base'] == 'synced' and not d['dot'] and not d['animating'] and not d['timer'], d)
        results['initialShot'] = capture_item(proc.pid, out / 'shots' / 'initial.png', scale) if native else 'blocked: session not visible'

        # 3. every visual state on the real status item (MANUAL facts)
        shots = {}
        for state in STATES:
            gui.ctl(f'tray-icon-manual:{state if state != "synced+dot" else "none+dot"}', 'tray-icon-manual')
            time.sleep(.5)
            s = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
            want = state.split('+')[0]
            shots[state] = capture_item(proc.pid, out / 'shots' / f'state-{state}.png', scale) if native else {'ok': False, 'blocked': 'session not visible'}
            check(f'3 manual state {state}: the host holds it', s['base'] == want and s['dot'] == state.endswith('+dot'), s)
            check(f'3 manual state {state}: the real status item was captured', shots[state]['ok'], shots[state])
        gui.ctl('tray-icon-manual:none', 'tray-icon-manual')
        results['stateShots'] = shots

        # 4. ear motion (MANUAL triggers)
        gui.ctl('tray-icon-manual:none', 'tray-icon-manual')
        time.sleep(1)
        motion = {}
        for name, strip in STRIPS.items():
            burst, stop = [], threading.Event()

            def snap():
                i = 0
                while not stop.is_set():
                    burst.append(capture_item(proc.pid, out / 'shots' / f'motion-{name}-{i:02d}.png', scale))
                    i += 1

            th = threading.Thread(target=snap) if native else None
            if th:
                th.start()
            t_start = time.time_ns()
            gui.ctl(f'tray-icon-play:{name}', 'tray-icon-play')
            time.sleep(strip['ms'] / 1000 + 1.5)
            if th:
                stop.set()
                th.join()
            recs = gui.ctl(f'tray-icon-frames:{t_start}', 'tray-icon-frames')['detail']
            starts = [r['ns'] for r in recs if r['kind'] == 'animation']
            frames = [r for r in recs if r['kind'] == 'frame']
            t0 = starts[0] if starts else None
            rel = [((r['ns'] - t0) / 1e6, r['left'], r['right']) for r in frames] if t0 else []
            # A frame equal to the image already shown is not redrawn, so the first pose (0, 0) never appears as a frame of its own.
            worst = max((max(abs(l - expected_pose(strip, t)[0]), abs(r - expected_pose(strip, t)[1])) for t, l, r in rel if t <= strip['ms']), default=999)
            peaks = []
            for key_t, kl, kr in strip['keys']:
                if kl or kr:
                    got = [max(abs(l - kl), abs(r - kr)) for t, l, r in rel if abs(t - key_t) <= 60]
                    peaks.append({'ms': key_t, 'want': [kl, kr], 'closestMiss': min(got) if got else None})
            tail = rel[-1] if rel else None
            end_ms = strip['ms']
            gaps = [round(b[0] - a[0]) for a, b in zip(rel, rel[1:])]
            in_window = [r for r in gui.rows() if r['step'] == 'tray-publish' and t_start <= r['detail']['startNs'] <= t_start + int((strip['ms'] + 200) * 1e6)]
            last_state = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
            motion[name] = {'frames': len(rel), 'relFrames': [(round(t), l, r) for t, l, r in rel], 'frameGapsMs': gaps, 'worstAngleErrorDeg': worst, 'peaks': peaks,
                            'lastFrame': tail, 'menuPublishesDuringAnimation': len(in_window), 'stateAfter': last_state, 'burstShots': len(burst)}
            ok = (len(rel) >= 3 and worst <= JITTER_DEG and tail and tail[1] == 0 and tail[2] == 0
                  and all(p['closestMiss'] is not None and p['closestMiss'] <= JITTER_DEG for p in peaks)
                  and end_ms - 80 <= tail[0] <= end_ms + 120 and len(rel) <= end_ms // 40 + 4 and max(gaps, default=0) <= 100
                  and not in_window and not last_state['animating'] and not last_state['timer'])
            check(f'4 ear motion {name}: frames follow the design keyframes, end at rest, no menu publish, no timer left', ok, {k: motion[name][k] for k in motion[name] if k != 'relFrames'})
            if native:
                check(f'4 ear motion {name}: the real status item was captured while it played', any(b['ok'] for b in burst), {'shots': len(burst)})
        results['motion'] = motion
        # Idle: no timer, and the process is quiet.
        c0 = cpu_seconds(proc.pid)
        time.sleep(10)
        c1 = cpu_seconds(proc.pid)
        idle = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
        check('4 idle: no animation, no timer, and CPU use over 10 s stays small', not idle['animating'] and not idle['timer'] and (c1 - c0) / 10 < 0.05, {'cpuSecondsPer10s': c1 - c0, 'state': idle})


        # 5a. daemon-driven chain, no clicks: the daemon's own settings are changed from outside the GUI (daemonpatch, the GUI's daemon client); the icon can
        # only follow because the host read the daemon. The poll period is 10 s, so each change must show within 15 s.
        def patch(path, body):
            r = subprocess.run([str(patch_bin), 'PUT', path, json.dumps(body)], env=env, capture_output=True, text=True, timeout=60)
            return r.returncode == 0, (r.stdout or r.stderr)[-200:]

        def wait_base(want, timeout=15):
            t0, last = time.time(), None
            while time.time() - t0 < timeout:
                last = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
                if last['base'] == want:
                    return round(time.time() - t0, 1), last
                time.sleep(.5)
            return None, last

        gui.ctl('tray-icon-manual:none', 'tray-icon-manual')
        time.sleep(1)
        base0 = wait_base('synced')[1]
        for what, off_body, on_body, want in (('sync switch', {'sync': {'syncEnabled': False}}, {'sync': {'syncEnabled': True}}, 'paused'),
                                              ('LAN-only', {'network': {'allowRelayFallback': False}}, {'network': {'allowRelayFallback': True}}, 'lan-only')):
            ok1, out1 = patch('/settings', off_body)
            latency, state = wait_base(want)
            check(f'5a daemon {what} changed from outside the GUI: the icon follows to "{want}" within one poll', ok1 and latency is not None, {'patch': out1, 'latencySeconds': latency, 'state': state})
            ok2, out2 = patch('/settings', on_body)
            latency, state = wait_base('synced')
            check(f'5a restoring the daemon {what}: the icon returns to "synced"', ok2 and latency is not None, {'patch': out2, 'latencySeconds': latency, 'state': state})
        results['startedFrom'] = base0

        # 5/6. the real menu
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
            opened, rd = open_and_read('m1')
            titles = [i['title'] for i in rd.get('menu', [])]
            check('6 right click on this pid\'s status item opens the real menu with the unchanged labels',
                  rd.get('ok') and titles[0] in ('Disable Sync', 'Enable Sync') and 'Device Sync' in titles and 'Quit' in titles, {'titles': titles})
            label0 = titles[0] if titles else None
            before_publishes = len([r for r in gui.rows() if r['step'] == 'tray-publish'])
            p = press(label0)
            s1 = wait_daemon(lambda x: ((x.get('sync') or {}).get('syncEnabled')) is (not sync0))
            check('5 pressing the sync item in the real menu flips syncEnabled in the DAEMON', p.get('ok') and ((s1 or {}).get('sync') or {}).get('syncEnabled') is (not sync0), {'label': label0, 'daemon': (s1 or {}).get('sync')})
            time.sleep(1.5)
            paused = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
            shot = capture_item(proc.pid, out / 'shots' / 'authoritative-paused.png', scale)
            check('5 the icon is "paused" because the host read the daemon\'s answer', paused['base'] == ('paused' if sync0 else 'synced'), {'state': paused, 'shot': shot})
            time.sleep(1)
            opened, rd = open_and_read('m2')
            label1 = rd['menu'][0]['title'] if rd.get('ok') else None
            p = press(label1)
            s2 = wait_daemon(lambda x: ((x.get('sync') or {}).get('syncEnabled')) is sync0)
            check('5 the second press restores syncEnabled in the daemon', ((s2 or {}).get('sync') or {}).get('syncEnabled') is sync0, (s2 or {}).get('sync'))
            time.sleep(1.5)
            back = gui.ctl('tray-icon-state', 'tray-icon-state')['detail']
            check('5 the icon returns to "synced"', back['base'] == 'synced', back)
            results['menuPublishesAcrossSyncToggles'] = len([r for r in gui.rows() if r['step'] == 'tray-publish']) - before_publishes

            daemons = tt.daemon_pids(home, profile)
            time.sleep(1)
            opened, rd = open_and_read('m3')
            q = press('Quit')
            try:
                rc = proc.wait(timeout=40)
            except subprocess.TimeoutExpired:
                rc = None
            time.sleep(3)
            gone = [d for d in daemons if subprocess.run(['ps', '-p', str(d['pid'])], capture_output=True).returncode == 0]
            check('6 Quit pressed in the real menu exits this GUI pid with 0 and the exact daemon pid goes away', q.get('ok') and rc == 0 and daemons and not gone, {'rc': rc, 'daemons': daemons, 'stillRunning': gone})
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
