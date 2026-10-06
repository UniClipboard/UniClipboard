#!/usr/bin/env python3
"""Drive the real Wails WebView hosting the shared React frontend against a real daemon.

Each launch runs the in-WebView driver (frontend/src/e2e-driver.ts), whose
assertions arrive as JSON lines in native.jsonl. The orchestrator owns process
lifecycle: only PIDs it started are stopped, and exits are verified.
"""
import argparse
import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env

HERE = Path(__file__).resolve().parent


def read_steps(path, start):
    return [json.loads(line) for line in path.read_text().splitlines()[start:]]


def wait_for_step(proc, path, start, step, timeout=120, on_step=None):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        rows = read_steps(path, start)
        for row in rows:
            if on_step:
                on_step(row)
            if row['step'] == 'driver-error':
                raise RuntimeError(f"driver error: {row.get('detail')}")
        if any(r['step'] == step for r in rows):
            return rows
        if proc.poll() is not None:
            raise RuntimeError(f'GUI exited early ({proc.returncode}) before step {step}')
        time.sleep(.2)
    raise RuntimeError(f'timeout waiting for step {step}')


def screenshot(pid, out, title=None):
    try:
        wid = subprocess.check_output(['swift', str(HERE / 'window_id.swift'), str(pid)] + ([title] if title else []), text=True, timeout=60).strip()
        subprocess.run(['screencapture', '-x', '-o', '-l', wid, str(out)], check=True, timeout=20)
        return True
    except Exception:
        return False


def pid_alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--interactive', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = 'UniClipboardGo' if args.interactive else 'UniClipboardGoE2E'
    binary = ROOT / f'target/gui-go/{app}.app/Contents/MacOS/gui-go'
    cli = ROOT / 'target/gui-go/uniclip'
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'native.jsonl'
    evidence.write_text('')
    env = isolated_env(home, profile, {'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence), 'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    results = {'home': home, 'profile': profile, 'systemClipboardDisabled': True, 'rounds': [], 'passed': False,
               'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()}
    proc = None
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        if args.interactive:
            print(f'Isolated profile: {profile}\nHOME: {home}\nClose the GUI (Cmd+Q) to end the demo.', flush=True)
            proc = subprocess.Popen([str(binary)], env=env)
            results['guiExit'] = proc.wait()
            results['passed'] = results['guiExit'] == 0
            return
        daemon_pids = []
        for run in range(2):
            start = len(evidence.read_text().splitlines())
            with (out / f'gui-{run}.log').open('w') as log:
                run_env = dict(env, UC_GUI_GO_EXIT_MODE='full' if run == 1 else 'keep')
                proc = subprocess.Popen([str(binary)], env=run_env, stdout=log, stderr=log)
                taken = set()

                def capture(row, run=run, taken=taken):
                    # The driver keeps each second window open long enough to be captured.
                    for step, title, name in (('native-updater-opened', 'Software Update', 'updater'), ('native-quick-panel-visible', '-', 'quick-panel')):
                        if row['step'] == step and step not in taken:
                            taken.add(step)
                            shot = out / f'{name}-{run}.png'
                            results.setdefault('screenshots', []).append({'file': shot.name, 'captured': screenshot(proc.pid, shot, title)})

                rows = wait_for_step(proc, evidence, start, 'driver-complete', 240, capture)
                steps = {r['step']: r for r in rows}
                assert all(r['ok'] for r in rows), f'failed steps: {[r for r in rows if not r["ok"]]}'
                assert 'shared-app-mounted' in steps and 'home' in steps and 'devices' in steps and 'settings' in steps
                assert 'native-main-closed' in steps and 'native-main-reopened' in steps
                for needed in ('updater-mounted', 'native-updater-opened', 'native-updater-closed', 'quick-panel-mounted', 'native-quick-panel-visible', 'quick-panel-shown-state', 'native-quick-panel-dismissed', 'tray-sync-toggle'):
                    assert needed in steps, f'missing step {needed}'
                shot = out / f'main-{run}.png'
                results.setdefault('screenshots', []).append({'file': shot.name, 'captured': screenshot(proc.pid, shot)})
                conn_path = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
                conn = json.loads(conn_path.read_text())
                daemon_pid = conn['pid']
                daemon_pids.append(daemon_pid)
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(f"http://{conn['host']}:{conn['port']}/health", timeout=5) as r:
                    assert json.load(r)['data']['status'] == 'ok'
                results['rounds'].append({'run': run, 'firstScreen': steps['first-screen']['detail']['state'], 'steps': sorted(steps), 'daemonPID': daemon_pid})
                # The driver exits the GUI itself: round 0 keeps the daemon (lightweight), round 1 quits fully.
                code = proc.wait(timeout=60)
                results['rounds'][-1]['guiExit'] = code
                assert code == 0, f'GUI exit code {code}'
                if run == 0:
                    assert pid_alive(daemon_pid), 'daemon stopped with a lightweight exit'
                else:
                    deadline = time.monotonic() + 15
                    while pid_alive(daemon_pid) and time.monotonic() < deadline:
                        time.sleep(.2)
                    assert not pid_alive(daemon_pid), 'full quit left the daemon running'
                    results['fullQuitStoppedDaemon'] = True
        assert daemon_pids[0] == daemon_pids[1], 'second GUI replaced the daemon'
        results['daemonReused'] = True
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=20)
        stop = subprocess.run([str(cli), '--json', 'stop'], env=env, capture_output=True, text=True, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        cleanup_pid = json.loads(stop.stdout).get('pid') if stop.returncode == 0 else None
        results['daemonExitedAfterCleanup'] = False
        if cleanup_pid:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if not pid_alive(cleanup_pid):
                    results['daemonExitedAfterCleanup'] = True
                    break
                time.sleep(.2)
        if results.get('fullQuitStoppedDaemon') and stop.returncode in (0, 1):
            # The last run quit fully, so the daemon is already gone and `stop` finds nothing.
            results['daemonExitedAfterCleanup'] = True
        if not results['daemonExitedAfterCleanup']:
            results['passed'] = False
        (out / 'cleanup.json').write_text(stop.stdout)
        (out / 'assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        manifest = {}
        for file in [binary, cli, ROOT / 'target/debug/uniclipd', ROOT / 'apps/gui-go/go.sum', ROOT / 'bun.lock']:
            if file.exists():
                manifest[str(file.relative_to(ROOT))] = hashlib.sha256(file.read_bytes()).hexdigest()
        (out / 'build-hashes.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed'], 'native E2E or cleanup failed'


if __name__ == '__main__':
    main()
