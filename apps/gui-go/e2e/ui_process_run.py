#!/usr/bin/env python3
"""Native macOS UI-process lifecycle. Never run without explicit session authorization.

One isolated profile, no system clipboard. Assert that the resident host/daemon/native helper
survive three main-window closes, while the UI child and every app-owned WebKit helper exit.
Reopen through LaunchServices and tray-equivalent Open. Record identities and all observations.
"""
import argparse
import json
import hashlib
import plistlib
import re
import subprocess
import time
from pathlib import Path

from startup_run import BINARY, PASSPHRASE, Profile, cli, read_steps
from run import pid_alive
from dock_lifecycle_run import Run, OPEN_LABELS, QUIT_LABELS


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--allow-session', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not args.allow_session:
        parser.error('refusing native GUI interaction without --allow-session')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    bundle = BINARY.parents[2]
    name = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())['CFBundleName']
    if subprocess.run(['pgrep', '-f', str(BINARY)], capture_output=True).returncode == 0:
        raise RuntimeError('another instance of the test bundle is running')
    ui_bundle = bundle / 'Contents/Helpers/UniClipboardUI.app'
    native = Run(out, bundle=ui_bundle)
    native.build_tools()
    p = Profile(out)
    p.base.update(UC_GPUI_QUICK_PANEL='1', UC_GUI_GO_E2E_VISIBLE='1')
    control = out / 'ui.control'
    control.write_text('')
    result = {'passed': False, 'observations': [], 'bundle': str(bundle),
              'sourceHead': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
              'sourceDiffSha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'])).hexdigest(),
              'binarySha256': hashlib.sha256(BINARY.read_bytes()).hexdigest(),
              'uiBinarySha256': hashlib.sha256((ui_bundle / 'Contents/MacOS/gui-go').read_bytes()).hexdigest()}
    changed = subprocess.check_output(['git', 'diff', '--name-only', 'HEAD'], text=True).splitlines()
    changed += subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], text=True).splitlines()
    result['sourceFiles'] = {p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
                             for p in sorted(set(changed)) if Path(p).is_file()}
    proc = None

    def webkit():
        rows = []
        for block in re.split(r'(?m)(?=^\d+\) )', subprocess.check_output(['lsappinfo', 'list'], text=True)):
            title = re.match(r'^\d+\) "([^"]+)"', block)
            pid = re.search(r'\bpid = (\d+)', block)
            if title and pid and title[1].startswith(name + ' ') and 'bundleID="com.apple.WebKit.' in block:
                rows.append({'pid': int(pid[1]), 'name': title[1],
                             'bundleID': re.search(r'bundleID="([^"]+)"', block)[1]})
        return rows

    def command(line):
        with control.open('a') as f:
            f.write(line + '\n')

    def helper_pids():
        output = subprocess.run(['ps', '-axo', 'pid,ppid,comm'], capture_output=True, text=True).stdout
        return [int(fields[0]) for line in output.splitlines() if len(fields := line.split(None, 2)) == 3
                and fields[1] == str(proc.pid) and fields[2].endswith('/uniclip-quick-panel')]

    sequence = 0

    def state():
        nonlocal sequence
        sequence += 1
        label = 'ui-' + str(sequence)
        command('state ' + label)
        end = time.monotonic() + 15
        while time.monotonic() < end:
            for row in read_steps(evidence, 0):
                if row['step'] == 'control-state' and row['detail'].get('label') == label:
                    return row['detail']
            if proc.poll() is not None:
                raise RuntimeError('resident host exited')
            time.sleep(.1)
        raise RuntimeError('state request timed out')

    def until(predicate, description, timeout=45):
        end = time.monotonic() + timeout
        last = None
        while time.monotonic() < end:
            last = state()
            if predicate(last):
                return last
            time.sleep(.3)
        raise AssertionError(f'{description}: {last}')

    def page_ready(ui_pid):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            ready = [r for r in read_steps(evidence, 0) if r['step'] == 'ui-page-ready'
                     and r.get('detail', {}).get('pid') == ui_pid]
            owned = webkit()
            if ready and ready[-1]['ok'] and any(r['bundleID'] == 'com.apple.WebKit.WebContent' for r in owned):
                return {'page': ready[-1], 'webkit': owned}
            time.sleep(.3)
        raise AssertionError('new page/bridge/daemon round trip or WebContent attribution did not become ready')

    try:
        assert not webkit(), 'another app with the test bundle name owns WebKit processes'
        cli(p.cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'ui-lifecycle')
        proc, evidence, _ = p.launch('ui-process', 'wake', wait=False, control=control)
        initial = until(lambda s: s.get('bootstrapped') and s.get('uiPid', 0) > 0 and s.get('mainExists'), 'UI child starts', 120)
        assert initial['residentWindowCount'] == 0, initial
        assert initial['uiPid'] != proc.pid, initial  # baseline Close-only implementation fails here
        assert initial.get('helperRunning'), initial
        result['initialPage'] = page_ready(initial['uiPid'])
        helpers = helper_pids()
        assert len(helpers) == 1, helpers
        previous = initial['uiPid']
        for cycle in range(3):
            if cycle == 0:
                assert native.tool('dock_ax', 'close-button', previous, 'UniClipboard').get('ok')
            elif cycle == 1:
                assert native.tool('dock_ax', 'menu-key', previous, 'W').get('ok')
            else:
                # Mode transitions have their own daemon-preservation acceptance test.
                command('close-main third-cycle')
            closed = until(lambda s: s.get('uiPid', 0) == 0, 'UI child exits')
            assert not pid_alive(previous), previous
            assert closed['residentWindowCount'] == 0, closed
            deadline = time.monotonic() + 45
            while webkit() and time.monotonic() < deadline:
                time.sleep(.5)
            remaining = webkit()
            assert not remaining, remaining
            assert closed['daemonPid'] == initial['daemonPid'] and pid_alive(initial['daemonPid']), closed
            assert helper_pids() == helpers, helper_pids()
            host_native = native.state(proc.pid)
            assert host_native['policy'] == 1 and not host_native['dockTile']['present'], host_native
            assert native.status_center(proc.pid), 'resident tray is absent'
            assert closed.get('helperRunning') and closed['startupMode'] == initial['startupMode'], closed
            result['observations'].append({'cycle': cycle, 'closed': closed, 'webkit': remaining})
            if cycle == 1:
                assert native.tray_press(proc.pid, OPEN_LABELS, f'cycle {cycle} tray Open')
            else:
                subprocess.run(['open', '-a', str(bundle)], check=True)
            opened = until(lambda s: s.get('uiPid', 0) > 0 and s.get('mainVisible'), 'UI child reopens')
            shown = native.state(opened['uiPid'])
            assert shown['policy'] == 0 and shown['dockTile']['present'], shown
            assert opened['uiPid'] != previous and opened['daemonPid'] == initial['daemonPid'], opened
            previous = opened['uiPid']
            result['observations'][-1]['reopened'] = opened
            result['observations'][-1]['page'] = page_ready(opened['uiPid'])
        assert native.tray_press(proc.pid, QUIT_LABELS, 'full tray Quit')
        assert proc.wait(timeout=40) == 0
        assert not pid_alive(previous)
        assert not pid_alive(initial['daemonPid'])
        assert not any(pid_alive(pid) for pid in helpers)
        control.write_text('')
        proc, evidence, _ = p.launch('ui-process-cmd-q', 'wake', wait=False, control=control)
        again = until(lambda s: s.get('bootstrapped') and s.get('uiPid', 0) > 0 and s.get('mainVisible'),
                      'fresh UI for Cmd-Q', 120)
        assert native.tool('dock_ax', 'menu-key', again['uiPid'], 'Q').get('ok')
        assert proc.wait(timeout=40) == 0
        assert not pid_alive(again['uiPid']) and not pid_alive(again['daemonPid']), again
        result['cmdQ'] = again
        result['nativeChecks'] = native.checks
        result['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=20)
        cli(p.cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'ui-process-assertions.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
