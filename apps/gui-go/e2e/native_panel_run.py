#!/usr/bin/env python3
"""Native quick panel helper E2E: the Go host supervises the real GPUI helper like the Tauri shell.

Run 1 (real `uniclip-quick-panel` from the bundle, real daemon profile): the helper starts as a child of
the GUI; disabling the quick panel stops it, enabling starts a new one, changing the global shortcut or the
modifier double-tap setting restarts it, killing it makes the supervisor bring it back, and a full quit
leaves no helper behind. The in-WebView driver changes settings through the shared frontend bindings and
marks each action; the orchestrator samples the helper process table throughout and judges it afterwards.

Run 2 (stand-in helper through the e2e-only override): the host passes --exit-when-stdin-closes, tolerates
noise on stdout, shows the hidden main window on `show_main_window`, opens the settings page on
`open_settings`, and closes the helper's stdin on exit.

Not covered: pressing the global shortcut (needs Accessibility for the test runner); the helper's own checks
(apps/quick-panel/tests) cover its trigger and window behaviour.
"""
import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
SETTLE = 3.0  # seconds after an action before the process table must reflect it


def helper_pids(gui_pid):
    out = subprocess.run(['pgrep', '-P', str(gui_pid), '-f', 'uniclip-quick-panel'], capture_output=True, text=True)
    return sorted(int(p) for p in out.stdout.split())


def prepare(label, out):
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / f'native-panel-{label}.jsonl'
    evidence.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', f'native-{label}')
    for _ in range(3):
        if cli(cli_env, 'start', check=False).returncode == 0:
            break
        time.sleep(3)
    else:
        raise RuntimeError('daemon start failed three times')
    return home, profile, evidence, path, cli_env


def run_real_helper(out, results):
    home, profile, evidence, path, cli_env = prepare('real', out)
    env = isolated_env(home, profile, {'PATH': path, 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                       'UC_GUI_GO_E2E_PHASE': 'native-panel', 'UC_GUI_GO_EXIT_MODE': 'full'})
    proc = subprocess.Popen([str(BINARY)], env=env, stdout=(out / 'native-panel-real-gui.log').open('w'), stderr=subprocess.STDOUT)
    timeline, stop, killed = [], threading.Event(), {}

    def sample():
        while not stop.is_set():
            timeline.append((time.time(), helper_pids(proc.pid)))
            time.sleep(.1)
    threading.Thread(target=sample, daemon=True).start()
    try:
        deadline, seen = time.monotonic() + 120, {}
        while time.monotonic() < deadline and proc.poll() is None:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                if row['step'] not in seen:
                    seen[row['step']] = row
                    if row['step'] == 'act-kill':  # the orchestrator plays the crash
                        pids = timeline[-1][1] if timeline else []
                        killed['pid'] = pids[0] if pids else None
                        if pids:
                            os.kill(pids[0], signal.SIGKILL)
            time.sleep(.1)
        stop.set()
        assert proc.wait(timeout=60) == 0, 'GUI did not exit cleanly'
        seen.update({r['step']: r for r in read_steps(evidence, 0)})
        exit_at = time.time()
        for step in ('shortcut-saved', 'double-tap-saved', 'double-tap-availability'):
            assert seen[step]['ok'], seen[step]

        def at(step):
            return seen[step]['at'] / 1000.0

        def pids_at(moment):  # helper pids last sampled at or before `moment`
            last = []
            for t, pids in timeline:
                if t > moment:
                    break
                last = pids
            return last
        before = pids_at(at('act-disable') - .2)
        assert len(before) == 1, f'expected exactly one helper before disabling, got {before}'
        assert pids_at(at('act-enable') - .2) == [], 'helper still running after the quick panel was disabled'
        after_enable = pids_at(at('act-shortcut') - .2)
        assert len(after_enable) == 1 and after_enable != before, f'enable must start a new helper: {before} -> {after_enable}'
        after_shortcut = pids_at(at('act-double-tap') - .2)
        assert len(after_shortcut) == 1 and after_shortcut != after_enable, f'shortcut change must restart: {after_enable} -> {after_shortcut}'
        after_tap = pids_at(at('act-kill') - .2)
        assert len(after_tap) == 1 and after_tap != after_shortcut, f'double-tap change must restart: {after_shortcut} -> {after_tap}'
        assert killed.get('pid') == after_tap[0], (killed, after_tap)
        revived = pids_at(at('act-exit') - .2)
        assert len(revived) == 1 and revived != after_tap, f'killed helper was not restarted: {after_tap} -> {revived}'
        time.sleep(1)
        for pid in revived:
            try:
                os.kill(pid, 0)
                raise AssertionError(f'helper {pid} outlived the GUI')
            except ProcessLookupError:
                pass
        results['real'] = {'start': before, 'afterEnable': after_enable, 'afterShortcut': after_shortcut,
                           'afterDoubleTap': after_tap, 'killed': killed['pid'], 'revived': revived,
                           'availability': seen['double-tap-availability']['detail'], 'exitedWithoutHelper': True}
    finally:
        stop.set()
        if proc.poll() is None:
            proc.terminate()
        cli(cli_env, '--json', 'stop', check=False, timeout=80)


def run_fake_helper(out, results):
    home, profile, evidence, path, cli_env = prepare('fake', out)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-helper-'))
    fake = work / 'fake-helper.sh'
    fake.write_text(f'''#!/bin/sh
echo "$@" > "{work}/args"
sleep 3
echo "plain log noise"
echo '{{"request":"show_main_window"}}'
sleep 3
echo '{{"request":"open_settings"}}'
cat > /dev/null
touch "{work}/stdin-closed"
''')
    fake.chmod(0o755)
    env = isolated_env(home, profile, {'PATH': path, 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                       'UC_GUI_GO_E2E_PHASE': 'native-requests', 'UC_GUI_GO_EXIT_MODE': 'full',
                                       'UC_QUICK_PANEL_HELPER_EXE': str(fake)})
    proc = subprocess.Popen([str(BINARY)], env=env, stdout=(out / 'native-panel-fake-gui.log').open('w'), stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline and proc.poll() is None:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
            time.sleep(.2)
        assert proc.wait(timeout=60) == 0
        steps = {r['step']: r for r in read_steps(evidence, 0)}
        assert steps['helper-show-main']['ok'], steps.get('helper-show-main')
        assert steps['helper-open-settings']['ok'], steps.get('helper-open-settings')
        time.sleep(1)
        assert (work / 'stdin-closed').exists(), 'helper stdin was not closed on exit'
        results['fake'] = {'args': (work / 'args').read_text().strip(), 'showMain': True, 'openSettings': True, 'stdinClosed': True}
        assert results['fake']['args'] == '--exit-when-stdin-closes'
    finally:
        if proc.poll() is None:
            proc.terminate()
        cli(cli_env, '--json', 'stop', check=False, timeout=80)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    results = {'passed': False}
    try:
        run_real_helper(out, results)
        run_fake_helper(out, results)
        results['passed'] = True
    finally:
        (out / 'native-panel-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
