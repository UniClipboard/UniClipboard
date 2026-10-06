#!/usr/bin/env python3
"""Exercise the real Wails WebView, its bindings and the Rust daemon in isolation."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import signal
import urllib.request
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env


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
    env = isolated_env(home, profile, {'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(out / 'native.jsonl'), 'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    evidence = out / 'native.jsonl'
    evidence.write_text('')
    results = {'home': home, 'profile': profile, 'systemClipboardDisabled': True, 'rounds': [], 'passed': False, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()}
    proc = None
    def interrupt(signum, frame):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, interrupt)
    try:
        if args.interactive:
            print(f'Isolated profile: {profile}\nHOME: {home}\nClose the GUI to end the demo and stop its isolated daemon.', flush=True)
            proc = subprocess.Popen([str(binary)], env=env)
            results['guiExit'] = proc.wait()
            results['passed'] = results['guiExit'] == 0
            return
        pids = []
        for run in range(2):
            previous = len(evidence.read_text().splitlines())
            with (out / f'gui-{run}.log').open('w') as log:
                proc = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 100
                rows = []
                while time.monotonic() < deadline:
                    rows = [json.loads(line) for line in evidence.read_text().splitlines()[previous:]]
                    if {'main', 'secondary'} <= {r['window'] for r in rows}:
                        break
                    if proc.poll() is not None:
                        raise RuntimeError(f'GUI exited before native assertions: {proc.returncode}; see gui-{run}.log')
                    time.sleep(.2)
                assert {'main', 'secondary'} <= {r['window'] for r in rows}, 'native WebView evidence timeout'
                assert all(r['http'] and r['ws'] and r['session'] and r['refresh'] for r in rows)
                assert len({r['pid'] for r in rows}) == 1
                code = proc.wait(timeout=60)
                assert code == 0, f'GUI exit {code}'
                pid = rows[0]['pid']
                os.kill(pid, 0)
                conn_path = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
                conn = json.loads(conn_path.read_text())
                assert conn['pid'] == pid
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(f"http://{conn['host']}:{conn['port']}/health", timeout=5) as response:
                    assert json.load(response)['data']['status'] == 'ok'

                if run == 0:
                    setup = subprocess.run([str(cli), 'space', 'init', '--passphrase', 'gui-go-synthetic-passphrase', '--device-name', 'gui-go-synthetic'], env=env, capture_output=True, text=True, timeout=60)
                    assert setup.returncode == 0, 'synthetic CLI setup failed'
                    results['syntheticCLISetup'] = True
                status = subprocess.run([str(cli), '--json', 'space', 'status'], env=env, capture_output=True, text=True, timeout=20)
                assert status.returncode == 0, status.stderr
                (out / f'cli-status-{run}.json').write_text(status.stdout)
                results['rounds'].append({'nativeWindows': sorted({r['window'] for r in rows}), 'daemonPID': pid, 'guiExit': code, 'daemonAliveAfterGUIExit': True, 'cliStatus': True})
                pids.append(pid)
        assert pids[0] == pids[1], 'second GUI replaced daemon'
        results['daemonReused'] = True
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=20)
        stop = subprocess.run([str(cli), '--json', 'stop'], env=env, capture_output=True, text=True, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        cleanup_pid = json.loads(stop.stdout).get('pid') if stop.returncode == 0 else None
        if cleanup_pid:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                try:
                    os.kill(cleanup_pid, 0)
                except ProcessLookupError:
                    results['daemonExitedAfterCleanup'] = True
                    break
                time.sleep(.2)
        else:
            results['daemonExitedAfterCleanup'] = stop.returncode == 0
        if not results.get('daemonExitedAfterCleanup'):
            results['passed'] = False

        (out / 'cleanup.json').write_text(stop.stdout)
        (out / 'assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        manifest = {}
        for file in [binary, cli, ROOT / 'target/debug/uniclipd', ROOT / 'apps/gui-go/go.sum', ROOT / 'bun.lock']:
            manifest[str(file.relative_to(ROOT))] = hashlib.sha256(file.read_bytes()).hexdigest()
        (out / 'build-hashes.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed'], 'native E2E or cleanup failed'

if __name__ == '__main__':
    main()
