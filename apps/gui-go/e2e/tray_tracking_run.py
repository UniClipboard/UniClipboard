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


def open_menu(gui, pid, how, label, out):
    """Open the status item's menu through AppKit's own tracking and wait until the menu is readable through AX.

    A 40 ms AX watch runs from before the open, so a menu that appears and closes again within a poll interval is still seen (and timed).
    """
    watch_path = out / f'ax-open-{label}-watch.jsonl'
    with watch_path.open('w') as wf:
        watcher = subprocess.Popen([AX, 'watch', str(pid), '14', '40'], stdout=wf, stderr=subprocess.STDOUT)
        time.sleep(1)
        if how == 'rightclick':
            opened = ax('rightclick', str(pid))
        else:
            opened = gui.ctl(f'tray-open-menu {label}', f'tray-open-menu-{label}')
        # Native evidence that does not go through AX: the screen next to the status item right after the open (a tracked menu is drawn there).
        time.sleep(.4)
        shot_info = shot_near_status_item(pid, out / f'ax-open-{label}-screen.png')
        first = {'ok': False}
        end = time.time() + 12
        while time.time() < end:
            first = ax('read', str(pid))
            if first.get('ok'):
                break
            time.sleep(.3)
        if not first.get('ok'):
            watcher.wait(timeout=60)
    seen = [json.loads(l) for l in watch_path.read_text().splitlines() if l.strip()]
    ok_seen = [r['ns'] for r in seen if r.get('ok')]
    opened = dict(opened, screenshot=shot_info, openedSeenByWatch={'reads': len(seen), 'okReads': len(ok_seen), 'firstOkNs': ok_seen[0] if ok_seen else None, 'lastOkNs': ok_seen[-1] if ok_seen else None})
    return opened, first


class Done(Exception):
    """The minimal mode ends after open/read/cancel."""


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def daemon_pids(profile):
    """Exact pids of daemons whose own environment carries the profile (no command-line pattern matching)."""
    out = []
    for line in subprocess.run(['ps', '-axeww', '-o', 'pid=,command='], capture_output=True, text=True).stdout.splitlines():
        if f'UC_PROFILE={profile}' in line and 'uniclipd' in line.split(' UC_', 1)[0]:
            out.append(int(line.split()[0]))
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
    parser.add_argument('--open-with', choices=('control', 'rightclick'), default='control', help='control: SystemTray.OpenMenu through the control file (no pointer); rightclick: a real right click on the status item (moves the pointer briefly)')
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
        pair(env_a, env_b, 'tray-a', 'tray-peer-b')
        roster = json.loads(cli(env_a, '--json', 'member', 'list').stdout)
        peer = [m for m in roster if not m.get('is_local')][0]
        peer_id = peer['device_id']
        prefs_path, settings_path = f'/member/{peer_id}/sync-preferences', '/settings'
        results['daemonPidsAtStart'] = daemon_pids(prof_a)
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
        quiet = gui.ctl('tray-language-quiet q0 5000', 'tray-language-quiet-q0', 90)
        pin = gui.ctl('invoke en0 set_tray_language {"language":"en","trace":null}', 'invoke-en0')
        calls = [r['detail'] for r in gui.rows() if r['step'] == 'tray-language-call']
        check('0 precondition: the frontend startup language calls went quiet, then the test pinned English (last language call is the test\'s)',
              quiet['ok'] and pin['ok'] and calls[-1:] == ['en'], {'calls': calls})
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
            cancelled = ax('cancel', str(proc.pid))
            time.sleep(1)
            after = ax('read', str(proc.pid))
            check('M the open menu was cancelled through AX and is gone', cancelled.get('ok') and not after.get('ok'), {'cancel': cancelled, 'after': after.get('error')})
            check('M the root menu read while open is the expected English menu', titles(root)[1:] == EN, titles(root))
            (out / 'ax-minimal.json').write_text(json.dumps({'first': first}, ensure_ascii=False, indent=1))
            results['passed'] = all(c['ok'] for c in results['checks'])
            raise Done()
        sync_label = root[0]['title']
        check('1 root menu is the expected English menu (first item is the sync toggle)', sync_label in ('Enable Sync', 'Disable Sync') and titles(root)[1:] == EN, titles(root))
        watch_file = out / 'ax-1-watch.jsonl'
        with watch_file.open('w') as wf:
            w = subprocess.Popen([AX, 'watch', str(proc.pid), str(args.hold), '500'], stdout=wf, stderr=subprocess.STDOUT)
            w.wait(timeout=args.hold + 60)
        reads = [json.loads(l) for l in watch_file.read_text().splitlines() if l.strip()]
        ok_reads = [r for r in reads if r.get('ok')]
        check(f'1 the menu stayed open and readable for the whole {args.hold} s hold (no "no open menu" read)', len(reads) > 0 and len(ok_reads) == len(reads), {'reads': len(reads), 'ok': len(ok_reads)})
        publishes = [r['detail'] for r in gui.rows()[n_before:] if r['step'] == 'tray-publish']
        inside = [p for p in publishes if t_open * 1e9 <= p['startNs'] <= (t_open + args.hold + 5) * 1e9]
        check('1 the hook fired: at least 2 natural publishes ran WHILE the menu was open (timed refresh, not a manual call)', len(inside) >= 2, {'publishes': inside})
        gaps = [round((b['startNs'] - a['startNs']) / 1e9, 2) for a, b in zip(inside, inside[1:])]
        check('1 each publish returned promptly while the menu was tracked (durMs < 2000: the main-thread wait was served)', inside and max(p['durMs'] for p in inside) < 2000, {'durMs': [p['durMs'] for p in inside], 'gapsSeconds': gaps})
        sample = [titles(r['menu']) for r in ok_reads]
        check('1 every read during the hold had the full root menu (no empty or half-built menu after the rebuilds)', all(s[1:] == EN for s in sample), {'distinct': sorted({json.dumps(s, ensure_ascii=False) for s in sample})[:3]})

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
            daemons = daemon_pids(prof_a)
            time.sleep(1)
            open_menu(gui, proc.pid, args.open_with, 'o5', out)
            q = ax('press', str(proc.pid), 'Quit')
            try:
                rc = proc.wait(timeout=40)
            except subprocess.TimeoutExpired:
                rc = None
            check('5 pressing Quit in the real menu made the exact GUI pid exit with 0', q.get('ok') and rc == 0, {'press': q, 'rc': rc})
            end = time.time() + 30
            while time.time() < end and any(_alive(p) for p in daemons):
                time.sleep(1)
            check('5 the exact daemon pid(s) of this profile are gone (full exit)', bool(daemons) and not any(_alive(p) for p in daemons), {'daemons': daemons})
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
        results['daemonPidsAfterCleanup'] = {'a': daemon_pids(prof_a), 'b': daemon_pids(prof_b)}
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
