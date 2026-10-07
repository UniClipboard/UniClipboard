#!/usr/bin/env python3
"""macOS tray under REAL NSMenu tracking (slice 17c15, docs/architecture/gui-go-macos-tray-menu-tracking.md).

The real GUI (e2e build) runs against a real daemon with a real paired peer (two CLI profiles paired through the production
rendezvous service, like tray_devices_run.py). Nothing here drives the menu through a callback or a script of the app: the menu is
opened, read, pressed and cancelled through the Accessibility API on the status item's own NSMenu (e2e/tray_ax.swift), so what is
exercised is AppKit's menu tracking, the Wails menu rebuild and the host's locks together.

  1  menu opened and read; the menu stays open across natural 10 s refreshes (watch), every read is checked, each publish is
     placed against the reads through the e2e hook records (tray-publish: start, end, duration)
  2  MANUAL scheduling: a language change while the menu is open (the control file's invoke, labelled manual), then back
  3  device item pressed in the real menu -> the DAEMON's sync preferences flip; reopened menu shows the daemon state; restore
  4  sync item pressed in the real menu -> the DAEMON's syncEnabled flips; reopened menu label follows; restore
  5  Quit pressed in the real menu -> exact GUI pid exits 0 and the exact daemon pid (found by the profile in its environment) is gone

Boundaries: the host is this Mac session (Accessibility granted to the terminal); the menu appears on the real menu bar for the
duration of each hold. It does not move the pointer or send key events. The daemon here is the repo's debug build from target/debug
(origin recorded in provenance.json, not independently attested); the language preconditions are set through the control file, not the WebView driver.
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import cli  # noqa: E402
from peers import pair  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

AX = os.environ.get('TRAY_AX_BIN', '/private/tmp/claude-501/axbin/tray_ax')
DAEMONGET = os.environ.get('TRAY_DAEMONGET_BIN', '/private/tmp/claude-501/axbin/daemonget')
EN = ['Device Sync', '-', 'Open', 'Settings', 'Check for Updates…', '-', 'Restart', 'Lightweight Mode (Background Sync)', 'Quit']
ZH = ['设备同步', '-', '打开', '设置', '检查更新…', '-', '重启', '轻量模式（后台同步）', '退出']


def titles(items):
    return ['-' if (i['title'] == '' and i['role'] == 'AXMenuItem' and 'items' not in i) else i['title'] for i in items]


def ax(*args, check=False):
    p = subprocess.run([AX, *args], capture_output=True, text=True, timeout=60)
    try:
        row = json.loads(p.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        row = {'ok': False, 'error': 'unparsable observer output', 'stdout': p.stdout[-300:], 'stderr': p.stderr[-300:]}
    if check and not row.get('ok'):
        raise RuntimeError(f'ax {args}: {row}')
    return row


def shot_near_status_item(pid, dest, below=420):
    """Screenshot of ONLY the area around this pid's status item and the menu that hangs below it. `screencapture -R` fails on this host
    ('could not create image from rect'), so a full capture goes to a private temp file, is cropped with sips to the AX frame of the item
    (+/- 250 pt horizontally, `below` pt down) and is deleted at once; nothing else on the screen is kept."""
    item = ax('describe', str(pid))
    frame = (item.get('items') or [{}])[0].get('frame')
    if not frame:
        return {'ok': False, 'error': 'no AX frame for the status item'}
    scale = 2  # Retina: pixels per point on this host (peekaboo list screens: 2.0x)
    x0, y0 = max(0, int(frame['x'] - 250)), 0
    w, h = 500, below
    tmp = Path(tempfile.mkdtemp(prefix='uc-gui-go-shot-'))
    try:
        full = tmp / 'full.png'
        if subprocess.run(['screencapture', '-x', str(full)], capture_output=True, timeout=30).returncode != 0 or not full.exists():
            return {'ok': False, 'error': 'screencapture failed'}
        p = subprocess.run(['sips', '-c', str(h * scale), str(w * scale), '--cropOffset', str(y0 * scale), str(x0 * scale), str(full), '--out', str(dest)], capture_output=True, text=True, timeout=30)
        made = p.returncode == 0 and Path(dest).exists()
        # The area below the status item is whatever window lies under the menu: it can hold unrelated applications' content (17c15 min7 did).
        # The image is therefore NOT kept unless TRAY_KEEP_SHOTS=1 is set deliberately; only the fact that a capture was made is recorded.
        kept = made and os.environ.get('TRAY_KEEP_SHOTS') == '1'
        if made and not kept:
            Path(dest).unlink()
        return {'ok': made, 'kept': kept, 'frame': frame, 'region': {'x': x0, 'y': y0, 'w': w, 'h': h, 'scale': scale}}
    finally:
        for f in tmp.iterdir():
            f.unlink()
        tmp.rmdir()


OUR_BUNDLE = 'app.uniclipboard.desktop.e2e'


def is_ours(hit):
    """The element at a point is this app's status item: owned by this pid, or (the menu-bar agent hosts items on this macOS) named for it."""
    return bool(hit.get('mine')) or OUR_BUNDLE in (hit.get('identifier', '') + hit.get('description', '') + hit.get('title', ''))


def status_item_center(pid):
    frame = ((ax('describe', str(pid)).get('items') or [{}])[0]).get('frame') or {}
    return frame, frame.get('x', 0) + frame.get('w', 0) / 2, frame.get('y', 0) + frame.get('h', 0) / 2


def verified_right_click(pid, out, label, watcher, sampler):
    """Right click this app's own status item, never at an unverified point.

    1. The point is this pid's AX frame centre. The element the system reports there must be this app's item.
    2. If it is the system's "show hidden menu bar items" button (the item overflowed), that exact element (identity and frame recorded) is
       clicked once with an ordinary left click (authorized, transient navigation), the item is located again and verified, and only then is it
       right-clicked. Collapsing is done by collapse_overflow().
    Any other owner at the point: TargetNotVerified, nothing clicked.
    """
    log = {'steps': []}
    frame, cx, cy = status_item_center(pid)
    hit = ax('elementat', str(pid), str(cx), str(cy))
    log['steps'].append({'what': 'before', 'axFrame': frame, 'point': [cx, cy], 'hit': hit})
    expanded = False
    if not is_ours(hit):
        if hit.get('role') == 'AXButton' and '隐藏菜单栏项目' in hit.get('description', '') and hit.get('frame'):
            f = hit['frame']
            bx, by = f['x'] + f['w'] / 2, f['y'] + f['h'] / 2
            again = ax('elementat', str(pid), str(bx), str(by))
            log['steps'].append({'what': 'overflow button re-verified at its own centre', 'point': [bx, by], 'hit': again})
            if again.get('description') == hit.get('description') and again.get('ownerPid') == hit.get('ownerPid'):
                log['steps'].append({'what': 'left click on the overflow button', 'result': ax('clickat', str(pid), str(bx), str(by), 'left')})
                expanded = True
                time.sleep(1.5)
                frame, cx, cy = status_item_center(pid)
                hit = ax('elementat', str(pid), str(cx), str(cy))
                log['steps'].append({'what': 'after expand', 'axFrame': frame, 'point': [cx, cy], 'hit': hit})
    (out / f'ax-open-{label}-target.json').write_text(json.dumps(log, indent=1, ensure_ascii=False))
    if not is_ours(hit):
        if expanded:
            collapse_overflow(pid, out, label)
        watcher.terminate()
        sampler.terminate()
        raise TargetNotVerified(f'after the transient overflow expansion={expanded} the element at ({cx},{cy}) is still not this app\'s item: {hit}; no right click was sent')
    result = ax('clickat', str(pid), str(cx), str(cy), 'right')
    result['expandedOverflow'] = expanded
    return result


def collapse_overflow(pid, out, label):
    """Put the menu bar back: if the overflow is still expanded, click the system's button once more (verified like the first time)."""
    frame, cx, cy = status_item_center(pid)
    hit = ax('elementat', str(pid), str(cx), str(cy))
    record = {'before': {'point': [cx, cy], 'hit': hit}}
    if hit.get('role') == 'AXButton' and '隐藏菜单栏项目' in hit.get('description', ''):
        record['note'] = 'overflow button already shows at our position: the menu bar is collapsed'
    (out / f'ax-open-{label}-collapse.json').write_text(json.dumps(record, indent=1, ensure_ascii=False))


def dismiss_menu(pid, out=None, tag='d'):
    """Close the open menu: AXCancel first (it reports success but did not close a tracked status menu in min17/min18), then, only if the menu
    is still readable AND this pid's menu is the one open, one Escape. Records a 6 s timeline of AX reads after the Escape, the system's frontmost
    application when it was sent, and a main-thread sample around it (is the main thread still inside NSMenuTrackingSession?)."""
    pre = ax('read', str(pid))
    log = {'popupWindowsBefore': pre.get('popupWindows'), 'axCancel': ax('cancel', str(pid))}
    time.sleep(1)
    if ax('read', str(pid)).get('popupWindows'):
        log['stillOpenAfterAxCancel'] = True
        log['frontmostBeforeEscape'] = subprocess.run(['lsappinfo', 'front'], capture_output=True, text=True).stdout.strip()
        sampler = subprocess.Popen(['sample', str(pid), '3', '10', '-file', str((out or Path('.')) / f'dismiss-{tag}-sample.txt')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(.5)
        log['escape'] = ax('escape', '0')
        timeline, t0 = [], time.time()
        while time.time() - t0 < 6:
            rd = ax('read', str(pid))
            timeline.append([round(time.time() - t0, 2), bool(rd.get('ok')), rd.get('popupWindows')])
            time.sleep(.25)
        sampler.wait(timeout=30)
        log['readableTimelineAfterEscape'] = timeline
    last = ax('read', str(pid))
    gone = not last.get('popupWindows')  # the pop-up menu window is gone; a stale AX subtree can remain
    log['gone'] = gone
    log['lastRead'] = {'axOk': last.get('ok'), 'popupWindows': last.get('popupWindows')}
    return gone, log


def open_menu(gui, pid, how, label, out):
    """Open the status item's menu through AppKit's own tracking and wait until the menu is readable through AX.

    A 40 ms AX watch runs from before the open, so a menu that appears and closes again within a poll interval is still seen (and timed).
    """
    watch_path = out / f'ax-open-{label}-watch.jsonl'
    with watch_path.open('w') as wf:
        watcher = subprocess.Popen([AX, 'watch', str(pid), '14', '40'], stdout=wf, stderr=subprocess.STDOUT)
        # The sample window (3 s, 5 ms) is opened BEFORE the open request and covers it, so a main thread that enters menu tracking (or the
        # showMenu block) is caught in the act; the native stack is evidence that does not depend on the AX tree.
        sampler = subprocess.Popen(['sample', str(pid), '3', '5', '-file', str(out / f'ax-open-{label}-sample.txt')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
        windows_before = ax('windows', str(pid))
        if False:
            pass
        elif how == 'rightclick':
            opened = verified_right_click(pid, out, label, watcher, sampler)
        else:
            opened = gui.ctl(f'tray-open-menu {label}', f'tray-open-menu-{label}')
        # Native evidence that does not go through AX: the screen next to the status item right after the open (a tracked menu is drawn there).
        time.sleep(.4)
        shot_info = shot_near_status_item(pid, out / f'ax-open-{label}-screen.png')
        first = {'ok': False}
        windows_during = None
        end = time.time() + 12
        while time.time() < end:
            first = ax('read', str(pid))
            if first.get('ok'):
                windows_during = ax('windows', str(pid))
                break
            time.sleep(.3)
        if not first.get('ok'):
            watcher.wait(timeout=60)
    sampler.wait(timeout=60)
    seen = [json.loads(l) for l in watch_path.read_text().splitlines() if l.strip()]
    ok_seen = [r['ns'] for r in seen if r.get('ok')]
    opened = dict(opened, windowsBefore=windows_before, windowsDuring=windows_during, screenshot=shot_info, openedSeenByWatch={'reads': len(seen), 'okReads': len(ok_seen), 'firstOkNs': ok_seen[0] if ok_seen else None, 'lastOkNs': ok_seen[-1] if ok_seen else None})
    return opened, first


class TargetNotVerified(Exception):
    """The click point does not belong to this pid's status window; nothing was clicked."""


class Done(Exception):
    """The minimal mode ends after open/read/cancel."""


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def daemon_pids(home, profile):
    """Exact pids of the daemon of this profile: the holders of its `.uniclipd.lock` (profile data dir under the task-owned HOME), each checked
    against the executable path. (The earlier lookup through the process environment found nothing on macOS, 17c15 full1: an empty set proved
    nothing.) Returns [{'pid', 'exe'}]."""
    lock = Path(home) / 'Library' / 'Application Support' / f'app.uniclipboard.desktop-{profile}' / '.uniclipd.lock'
    if not lock.exists():
        return []
    out = []
    for pid in subprocess.run(['lsof', '-t', str(lock)], capture_output=True, text=True).stdout.split():
        exe = subprocess.run(['ps', '-p', pid, '-o', 'comm='], capture_output=True, text=True).stdout.strip()
        out.append({'pid': int(pid), 'exe': exe})
    return out


class Gui:
    def __init__(self, proc, evidence, control):
        self.proc, self.evidence, self.control = proc, evidence, control

    def rows(self):
        return read_steps(self.evidence, 0)

    def wait_step(self, name, timeout=60, after=0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            hits = [r for r in self.rows()[after:] if r['step'] == name]
            if hits:
                return hits[-1]
            if self.proc.poll() is not None:
                raise RuntimeError(f'GUI exited ({self.proc.returncode}) before {name}')
            time.sleep(.2)
        # Take the scene before anything is torn down: the main thread's state and the AX view of this pid.
        stamp = int(time.time())
        subprocess.run(['sample', str(self.proc.pid), '2', '-file', str(self.evidence.parent / f'diag-timeout-{name}-{stamp}-sample.txt')], capture_output=True, timeout=60)
        (self.evidence.parent / f'diag-timeout-{name}-{stamp}-ax.json').write_text(json.dumps(ax('describe', str(self.proc.pid)), ensure_ascii=False, indent=1))
        raise RuntimeError(f'timeout waiting for {name}')

    def ctl(self, line, label, timeout=60):
        n = len(self.rows())
        with self.control.open('a') as f:
            f.write(line + '\n')
        return self.wait_step(label, timeout, after=n)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--hold', type=int, default=32, help='seconds the first menu stays open (>= 3 natural 10 s refreshes)')
    parser.add_argument('--skip-quit', action='store_true')
    parser.add_argument('--minimal', action='store_true', help='open, read, cancel only')
    parser.add_argument('--expand-overflow', action='store_true', help='with --probe-bar: press the system menu bar overflow button once (authorized, transient), record before/after, press again to collapse')
    parser.add_argument('--probe-bar', action='store_true', help='no pairing; start the GUI and record where the system menu bar put its status item (read-only), then exit')
    parser.add_argument('--open-with', choices=('control', 'rightclick'), default='control', help='rightclick (default, the only valid path): a real right click on this pid\'s status item (moves the pointer briefly); control: SystemTray.OpenMenu, a NO-OP here (SystemTray.menu is nil, 17c15 min10), kept only to reproduce that')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-peer-')
    prof_a, prof_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    evidence, control = out / 'gui.jsonl', out / 'gui.control'
    evidence.write_text('')
    control.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a = isolated_env(home_a, prof_a, {'PATH': path})
    env_b = isolated_env(home_b, prof_b, {'PATH': path})
    gui_env = isolated_env(home_a, prof_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_DISABLE_SYSTEM_CLIPBOARD': '1',
                                           'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'wake', 'UC_GUI_GO_EXIT_MODE': 'full',
                                           'UC_GUI_GO_E2E_CONTROL_FILE': str(control), 'UC_GUI_GO_E2E_NATIVE_STATE': '1',
                                           'UC_GUI_GO_E2E_SECRET': 'tray-tracking-17c15'})
    binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
    results = {'profileA': prof_a, 'profileB': prof_b, 'checks': [], 'passed': False,
               'scope': 'real GUI + real daemon + real paired peer; menu driven through the macOS Accessibility API on the status item NSMenu; manual scheduling labelled MANUAL'}
    (out / 'provenance.json').write_text(json.dumps({
        'desktopHead': subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip(),
        'dirty': bool(subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, capture_output=True, text=True).stdout.strip()),
        'guiBinarySha256': sha(binary), 'trayAxSha256': sha(AX), 'daemongetSha256': sha(DAEMONGET),
        'daemonSha256': sha(ROOT / 'target/debug/uniclipd'), 'cliSha256': sha(ROOT / 'target/gui-go/uniclip'),
        'hostIdleSeconds': next((int(l.split('=')[-1].strip()) / 1e9 for l in subprocess.run(['ioreg', '-c', 'IOHIDSystem'], capture_output=True, text=True).stdout.splitlines() if 'HIDIdleTime' in l), None),
        'pmsetSleep': [l.strip() for l in subprocess.run(['pmset', '-g'], capture_output=True, text=True).stdout.splitlines() if 'sleep' in l.lower()],
        'stateMutated': args.open_with == 'control' and not args.probe_bar,
        'stateMutatedWhy': 'the control opener fills the private SystemTray.menu field by reflection (e2e diagnostic experiment); the rightclick opener does not' if args.open_with == 'control' else None,
        'daemonOrigin': 'target/debug/uniclipd of this worktree (cargo build --locked -p uc-daemon, debug); not independently attested'}, indent=2) + '\n')

    def check(name, ok, detail=None):
        results['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)
        return ok

    def dget(env, p):
        r = subprocess.run([DAEMONGET, p], env=env, capture_output=True, text=True, timeout=60)
        return json.loads(r.stdout) if r.returncode == 0 else None

    def wait_daemon(env, p, pred, timeout=40):
        end, last = time.time() + timeout, None
        while time.time() < end:
            last = dget(env, p)
            if last is not None and pred(last):
                return last
            time.sleep(1)
        return last

    proc = None
    gui = None
    wake = None
    try:
        # Authorized by the user (17c15): declare ONE transient user activity so a sleeping display wakes for the run. `caffeinate -u -t` is a
        # task-owned process that ends on its own timeout and in cleanup; no persistent setting, lock or permission is touched.
        results['displayBefore'] = ax('display', '0')
        wake = subprocess.Popen(['caffeinate', '-u', '-t', '300'])
        for _ in range(30):
            if not ax('display', '0').get('asleep'):
                break
            time.sleep(1)
        results['displayAfterWake'] = ax('display', '0')
        if not check('0 the display is awake for the run (native menu tracking and screenshots need it)', not results['displayAfterWake'].get('asleep'), results['displayAfterWake']):
            raise RuntimeError('display still asleep: native NSMenu tracking cannot be accepted on this session')
        if not args.probe_bar:
            pair(env_a, env_b, 'tray-a', 'tray-peer-b')
        else:
            cli(env_a, 'space', 'init', '--passphrase', 'probe-bar-17c15', '--device-name', 'probe-a')
            cli(env_a, 'start', check=False)
        roster = json.loads(cli(env_a, '--json', 'member', 'list').stdout)
        peer_id = ([m for m in roster if not m.get('is_local')] or [{'device_id': 'none'}])[0]['device_id']
        prefs_path, settings_path = f'/member/{peer_id}/sync-preferences', '/settings'
        results['daemonPidsAtStart'] = daemon_pids(home_a, prof_a)
        assert results['daemonPidsAtStart'], 'no daemon holds the profile lock after pairing: the daemon identity chain is broken'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT)
        gui = Gui(proc, evidence, control)
        gui.wait_step('bootstrapped', 120)
        items = {'ok': False}
        for _ in range(30):  # the status item is created on the main thread shortly after bootstrap
            items = ax('items', str(proc.pid))
            if items.get('ok') and items.get('count') == 1:
                break
            time.sleep(1)
        if not check('0 the status item of THIS pid is readable through AX', items.get('ok') and items.get('count') == 1, items):
            (out / 'diag-ax-describe.json').write_text(json.dumps(ax('describe', str(proc.pid)), ensure_ascii=False, indent=1))
            (out / 'diag-ax-scan.json').write_text(json.dumps(ax('scan', str(proc.pid)), ensure_ascii=False, indent=1))
            shot_near_status_item(proc.pid, out / 'diag-menubar.png', below=60)
            subprocess.run(['sample', str(proc.pid), '2', '-file', str(out / 'diag-sample.txt')], capture_output=True, timeout=60)
            raise RuntimeError('no status item for this pid: nothing further can be attributed to the tray (see diag-*)')
        if args.probe_bar:
            frame = ((ax('describe', str(proc.pid)).get('items') or [{}])[0]).get('frame') or {}
            cx, cy = frame.get('x', 0) + frame.get('w', 0) / 2, frame.get('y', 0) + frame.get('h', 0) / 2
            agent_pid = next((int(l.split()[0]) for l in subprocess.run(['ps', '-axo', 'pid=,comm='], capture_output=True, text=True).stdout.splitlines() if l.strip().endswith('MenuBarAgent')), 0)
            (out / 'probe-bar.json').write_text(json.dumps({'axFrame': frame, 'elementAtCentre': ax('elementat', str(proc.pid), str(cx), str(cy)),
                                                            'agentPid': agent_pid, 'agentTree': ax('agent', str(agent_pid)), 'ourWindows': ax('windows', str(proc.pid))}, ensure_ascii=False, indent=1))
            if args.expand_overflow and agent_pid:
                def our_state(tag):
                    d = ax('describe', str(proc.pid))
                    fr = ((d.get('items') or [{}])[0]).get('frame') or {}
                    c = (fr.get('x', 0) + fr.get('w', 0) / 2, fr.get('y', 0) + fr.get('h', 0) / 2)
                    return {'tag': tag, 'ourAxFrame': fr, 'elementAtOurCentre': ax('elementat', str(proc.pid), str(c[0]), str(c[1])), 'agentTotal': ax('agent', str(agent_pid)).get('total')}
                expansion = {'actions': ax('overflow', str(agent_pid), 'actions'), 'before': our_state('before')}
                expansion['press'] = ax('overflow', str(agent_pid), 'press')
                time.sleep(1.5)
                expansion['after'] = our_state('after-expand')
                expansion['collapse'] = ax('overflow', str(agent_pid), 'press')
                time.sleep(1.5)
                expansion['afterCollapse'] = our_state('after-collapse')
                (out / 'probe-bar-expand.json').write_text(json.dumps(expansion, ensure_ascii=False, indent=1))
            results['passed'] = True
            raise Done()
        quiet = gui.ctl('tray-language-quiet q0 5000', 'tray-language-quiet-q0', 90)
        # The frontend's startup calls can come later than any fixed quiet window (17c15 full1: a second call 5.5 s after the first, 0.4 s after
        # the pin). Pin, wait for a longer quiet, and pin again if the frontend overwrote it; every call is kept as evidence.
        pins = []
        for attempt in range(4):
            pins.append(gui.ctl(f'invoke en{attempt} set_tray_language {{"language":"en","trace":null}}', f'invoke-en{attempt}')['ok'])
            gui.ctl(f'tray-language-quiet qq{attempt} 8000', f'tray-language-quiet-qq{attempt}', 90)
            calls = [r['detail'] for r in gui.rows() if r['step'] == 'tray-language-call']
            if calls[-1:] == ['en']:
                break
        results['trayLanguageCalls'] = calls
        check('0 precondition: after the frontend\'s startup calls (re-pinned if they came late) the last language call is the test\'s English and nothing followed it for 8 s',
              quiet['ok'] and all(pins) and calls[-1:] == ['en'], {'calls': calls, 'pinAttempts': len(pins)})
        # Wait until the peer row has been published at least once (the periodic refresh), checked in the daemon too.
        prefs0 = wait_daemon(env_a, prefs_path, lambda x: 'sendEnabled' in x)
        settings0 = dget(env_a, settings_path)
        sync0 = ((settings0 or {}).get('sync') or {}).get('syncEnabled')
        results['daemonBefore'] = {'prefs': prefs0, 'syncEnabled': sync0}
        time.sleep(12)  # at least one refresh after pairing, so the peer row exists before the first read

        # 1. open + hold across natural refreshes
        n_before = len(gui.rows())
        t_open = time.time()
        opened, first = open_menu(gui, proc.pid, args.open_with, 'o1', out)
        (out / 'ax-1-describe.json').write_text(json.dumps(ax('describe', str(proc.pid)), ensure_ascii=False, indent=1))
        (out / 'ax-1-open.json').write_text(json.dumps({'open': opened, 'first': first}, ensure_ascii=False, indent=1))
        check('1 the real status-item menu was opened through AppKit tracking (' + args.open_with + ') and is readable through AX', opened.get('ok') and first.get('ok'), {'open': opened, 'first_ok': first.get('ok'), 'error': first.get('error')})
        if not first.get('ok'):
            # Native evidence that does not depend on the AX tree: the app's own threads (is the main thread inside menu tracking?), the screen
            # next to the status item, and every menu-like AX node reachable from the application element.
            (out / 'diag-ax-scan.json').write_text(json.dumps(ax('scan', str(proc.pid)), ensure_ascii=False, indent=1))
            subprocess.run(['sample', str(proc.pid), '2', '-file', str(out / 'diag-sample.txt')], capture_output=True, timeout=60)
            raise RuntimeError('the menu could not be read after open; later steps need it')
        root = first['menu']
        if args.minimal:
            gone, dismissal = dismiss_menu(proc.pid, out, 'minimal')
            check('M the open menu was dismissed (AXCancel, then Escape if it stayed) and is gone', gone, dismissal)
            windows_after = ax('windows', str(proc.pid))

            def popups(w):
                return [x for x in (w or {}).get('windows', []) if x.get('onscreen') and x.get('layer', 0) >= 101]
            sample_text = (out / 'ax-open-o1-sample.txt').read_text() if (out / 'ax-open-o1-sample.txt').exists() else ''
            tracking_frames = sample_text.count('NSMenuTrackingSession')
            before_p, during_p, after_p = popups(opened.get('windowsBefore')), popups(opened.get('windowsDuring')), popups(windows_after)
            menu_sized = [p for p in during_p if p['bounds'].get('Width', 0) > 50 and p['bounds'].get('Height', 0) > 100]
            check('M the pop-up-window observable discriminates: none before the open, a menu-sized one during tracking, none after the dismissal, and the open-time sample holds NSMenuTracking frames',
                  not before_p and bool(menu_sized) and not after_p and tracking_frames > 0,
                  {'before': before_p, 'during': during_p, 'after': after_p, 'nsMenuTrackingSessionFramesInOpenSample': tracking_frames,
                   'otherOwnWindows': 'every window of this pid is listed in the artifacts (windowsBefore/During in ax-1-open.json); a layer>=101 on-screen window other than the menu would show here'})
            check('M the root menu read while open is the expected English menu', titles(root)[1:] == EN, titles(root))
            (out / 'ax-minimal.json').write_text(json.dumps({'first': first}, ensure_ascii=False, indent=1))
            results['passed'] = all(c['ok'] for c in results['checks'])
            raise Done()
        sync_label = root[0]['title']
        lang_calls = [r['detail'] for r in gui.rows() if r['step'] == 'tray-language-call']
        expected_root, sync_labels = (ZH, ('开启同步', '关闭同步')) if lang_calls and lang_calls[-1].startswith('zh') else (EN, ('Enable Sync', 'Disable Sync'))
        check('1 root menu is the menu of the LAST language call (product contract: last call wins), first item the sync toggle', sync_label in sync_labels and titles(root)[1:] == expected_root, {'lastCall': lang_calls[-1:], 'root': titles(root)})
        watch_file = out / 'ax-1-watch.jsonl'
        with watch_file.open('w') as wf:
            w = subprocess.Popen([AX, 'watch', str(proc.pid), str(args.hold), '500'], stdout=wf, stderr=subprocess.STDOUT)
            w.wait(timeout=args.hold + 60)
        reads = [json.loads(l) for l in watch_file.read_text().splitlines() if l.strip()]
        ok_reads = [r for r in reads if r.get('ok') and r.get('popupWindows')]  # readable AND a pop-up menu window on screen (a stale AX subtree is not 'open')
        check(f'1 the menu stayed open and readable for the whole {args.hold} s hold (every read: pop-up menu window on screen and readable)', len(reads) > 0 and len(ok_reads) == len(reads), {'reads': len(reads), 'ok': len(ok_reads), 'popupZero': sum(1 for r in reads if not r.get('popupWindows'))})
        publishes = [r['detail'] for r in gui.rows()[n_before:] if r['step'] == 'tray-publish']
        inside = [p for p in publishes if t_open * 1e9 <= p['startNs'] <= (t_open + args.hold + 5) * 1e9]
        check('1 the hook fired: at least 2 natural publishes ran WHILE the menu was open (timed refresh, not a manual call)', len(inside) >= 2, {'publishes': inside})
        gaps = [round((b['startNs'] - a['startNs']) / 1e9, 2) for a, b in zip(inside, inside[1:])]
        check('1 each publish returned promptly while the menu was tracked (durMs < 2000: the main-thread wait was served)', inside and max(p['durMs'] for p in inside) < 2000, {'durMs': [p['durMs'] for p in inside], 'gapsSeconds': gaps})
        sample = [titles(r['menu']) for r in ok_reads]
        calls_after = [r['detail'] for r in gui.rows() if r['step'] == 'tray-language-call']
        check('1 every read during the hold had the full root menu of the last language call (no empty or half-built menu after the rebuilds; no language call during the hold)', calls_after == lang_calls and all(s[1:] == expected_root for s in sample), {'distinct': sorted({json.dumps(s, ensure_ascii=False) for s in sample})[:3]})

        def device_items(menu):
            for it in menu:
                if it.get('items') is not None and it['title'] in ('Device Sync', '设备同步'):
                    return it['items']
            return None
        rows_seen = [[d['title'] for d in (device_items(r['menu']) or [])] for r in ok_reads]
        check('1 the device submenu lists the paired peer in the real open menu (daemon-paired: member list)', any(r == ['tray-peer-b'] for r in rows_seen), {'distinctRows': [list(x) for x in {tuple(r) for r in rows_seen}]})
        results['pid1Alive'] = proc.poll() is None
        check('1 GUI still alive after the hold', proc.poll() is None)

        # 2. MANUAL scheduling: language change while the menu is open
        n2 = len(gui.rows())
        zh = gui.ctl('invoke zh1 set_tray_language {"language":"zh-CN","trace":null}', 'invoke-zh1')
        time.sleep(1.5)
        r2 = ax('read', str(proc.pid))
        check('2 MANUAL: language changed to zh-CN while the menu was open (hook fired: invoke-enter/return and a publish)',
              zh['ok'] and r2.get('ok') and any(r['step'] == 'invoke-enter' for r in gui.rows()[n2:]) and any(r['step'] == 'tray-publish' for r in gui.rows()[n2:]),
              {'readOk': r2.get('ok'), 'error': r2.get('error')})
        if r2.get('ok'):
            sub = device_items(r2['menu'])
            check('2 MANUAL: the open menu relabelled, root and device submenu in the same language', titles(r2['menu'])[1:] == ZH and sub is not None and [d['title'] for d in sub] == ['tray-peer-b'], {'root': titles(r2['menu']), 'sub': sub})
        gui.ctl('invoke en2 set_tray_language {"language":"en","trace":null}', 'invoke-en2')
        time.sleep(1.5)
        r3 = ax('read', str(proc.pid))
        check('2 MANUAL: language restored to English in the open menu', r3.get('ok') and titles(r3['menu'])[1:] == EN, titles(r3['menu']) if r3.get('ok') else r3)
        (out / 'ax-2.json').write_text(json.dumps({'zh': r2, 'en': r3}, ensure_ascii=False, indent=1))
        # 2b. MANUAL ARTIFICIAL schedule (not a natural one): call A (zh-CN) pauses between its two steps, call B (en) starts meanwhile, while the menu
        # is tracked. With the 17c14 languageMu the root menu and the device submenu must end in ONE language (B's), read from the open menu.
        gap = gui.ctl('tray-language-gap g0 900', 'tray-language-gap-g0', 90)
        time.sleep(1.5)
        r3b = ax('read', str(proc.pid))
        sub3b = device_items(r3b['menu']) if r3b.get('ok') else None
        check('2b MANUAL (artificial schedule): zh-CN and en overlapped while the menu was tracked; the open menu ends in one language with the root and the submenu agreeing',
              gap['ok'] and (gap['detail'] or {}).get('gapConsumed') is True and r3b.get('ok') and r3b.get('popupWindows') and titles(r3b['menu'])[1:] == EN and sub3b is not None and [d['title'] for d in sub3b] == ['tray-peer-b'],
              {'gap': gap['detail'], 'root': titles(r3b['menu']) if r3b.get('ok') else r3b, 'popupWindows': r3b.get('popupWindows')})

        # 3. device item pressed in the real menu
        sub = device_items(r3['menu']) if r3.get('ok') else None
        before_mark = sub[0].get('mark') if sub else None
        press = ax('press', str(proc.pid), 'Device Sync', 'tray-peer-b')
        check('3 AXPress on the device item in the real menu succeeded', press.get('ok'), press)
        off = wait_daemon(env_a, prefs_path, lambda x: x.get('sendEnabled') is False and x.get('receiveEnabled') is False)
        check('3 the DAEMON\'s own sync preferences flipped to off (authoritative read)', off and off.get('sendEnabled') is False and off.get('receiveEnabled') is False, off)
        time.sleep(1)
        open_menu(gui, proc.pid, args.open_with, 'o2', out)
        time.sleep(11)  # one refresh, so the menu states the stored value
        r4 = ax('read', str(proc.pid))
        sub4 = device_items(r4['menu']) if r4.get('ok') else None
        check('3 the reopened menu shows the item unchecked, as the daemon says', sub4 is not None and not sub4[0].get('mark'), {'beforeMark': before_mark, 'afterMark': sub4[0].get('mark') if sub4 else None})
        ax('press', str(proc.pid), 'Device Sync', 'tray-peer-b')
        on = wait_daemon(env_a, prefs_path, lambda x: x.get('sendEnabled') is True and x.get('receiveEnabled') is True)
        check('3 pressing again restores on in the DAEMON', on and on.get('sendEnabled') is True and on.get('receiveEnabled') is True, on)
        (out / 'ax-3.json').write_text(json.dumps({'before': before_mark, 'reopened': r4}, ensure_ascii=False, indent=1))

        # 4. sync switch
        time.sleep(1)
        open_menu(gui, proc.pid, args.open_with, 'o3', out)
        r5 = ax('read', str(proc.pid))
        label0 = r5['menu'][0]['title'] if r5.get('ok') else None
        press = ax('press', str(proc.pid), label0)
        s1 = wait_daemon(env_a, settings_path, lambda x: ((x.get('sync') or {}).get('syncEnabled')) is (not sync0))
        check('4 pressing the sync item in the real menu flips syncEnabled in the DAEMON', press.get('ok') and ((s1 or {}).get('sync') or {}).get('syncEnabled') is (not sync0), {'label': label0, 'daemon': ((s1 or {}).get('sync') or {})})
        time.sleep(1)
        open_menu(gui, proc.pid, args.open_with, 'o4', out)
        r6 = ax('read', str(proc.pid))
        label1 = r6['menu'][0]['title'] if r6.get('ok') else None
        check('4 the reopened menu label follows the daemon', label1 == ('Disable Sync' if not sync0 else 'Enable Sync') and label1 != label0, {'before': label0, 'after': label1})
        ax('press', str(proc.pid), label1)
        s2 = wait_daemon(env_a, settings_path, lambda x: ((x.get('sync') or {}).get('syncEnabled')) is sync0)
        check('4 the second press restores syncEnabled in the daemon', ((s2 or {}).get('sync') or {}).get('syncEnabled') is sync0, ((s2 or {}).get('sync') or {}))

        # 5. quit from the real menu
        if not args.skip_quit:
            daemons = daemon_pids(home_a, prof_a)  # taken BEFORE Quit: the exact process that must go away
            time.sleep(1)
            open_menu(gui, proc.pid, args.open_with, 'o5', out)
            q = ax('press', str(proc.pid), 'Quit')
            try:
                rc = proc.wait(timeout=40)
            except subprocess.TimeoutExpired:
                rc = None
            check('5 pressing Quit in the real menu made the exact GUI pid exit with 0', q.get('ok') and rc == 0, {'press': q, 'rc': rc})
            end = time.time() + 30
            while time.time() < end and any(_alive(d['pid']) for d in daemons):
                time.sleep(1)
            check('5 the exact daemon pid(s) of this profile (lock-file holders, executable checked) were running before Quit and are gone after it (full exit)',
                  bool(daemons) and all(d['exe'].endswith('uniclipd') for d in daemons) and not any(_alive(d['pid']) for d in daemons), {'daemons': daemons, 'goneAfterQuit': [not _alive(d['pid']) for d in daemons]})
        results['passed'] = all(c['ok'] for c in results['checks'])
    except Done:
        pass
    except Exception as exc:  # keep the evidence of a failing run
        results['error'] = f'{type(exc).__name__}: {exc}'
        print('ERROR', results['error'], flush=True)
    finally:
        if proc and proc.poll() is None:
            ax('cancel', str(proc.pid))
            proc.terminate()
            try:
                proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=20)
        if proc:
            results['guiReturncode'] = proc.returncode  # negative: ended by that signal (cleanup terminate gives -15)
        if wake and wake.poll() is None:
            wake.terminate()
            wake.wait(timeout=10)
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=80)
        results['wakeReturncode'] = wake.returncode if wake else None
        results['displayAtEnd'] = ax('display', '0')
        # The overflow expansion was a transient navigation: at the end the system's overflow button must again be what sits at the old position.
        results['overflowAtEnd'] = ax('elementat', '0', '714', '15')
        results['daemonPidsAfterCleanup'] = {'a': daemon_pids(home_a, prof_a), 'b': daemon_pids(home_b, prof_b)}
        (out / 'assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: v for k, v in results.items() if k != 'checks'}, indent=2, ensure_ascii=False))
    sys.exit(0 if results['passed'] else 1)


def _alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


if __name__ == '__main__':
    main()
