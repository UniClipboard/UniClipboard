#!/usr/bin/env python3
"""Observe real macOS WebView entry frames on cold, warm and restarted launches.

No system input is sent. Synthetic setup uses the existing in-WebView E2E driver.
"""
import argparse
import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

from run import ROOT, isolated_env, read_steps, wait_for_step
from file_preview_run import cli


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-startup-')
    profile = 'gui-go-' + Path(home).name
    env = isolated_env(home, profile, {
        'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'],
        'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1',
        'UC_GUI_GO_E2E_PHASE': 'startup-frames',
    })
    results = {'home': home, 'profile': profile, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
               'kind': 'native-macos-webview-daemon', 'systemClipboardDisabled': True, 'rounds': [], 'passed': False}
    proc = None
    try:
        for label, exit_mode in [('cold', 'keep'), ('warm', 'full'), ('restart', 'full')]:
            evidence = out / f'{label}.jsonl'
            evidence.write_text('')
            with (out / f'{label}.log').open('w') as log:
                proc = subprocess.Popen([str(args.binary.resolve())], env=dict(env, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_EXIT_MODE=exit_mode), stdout=log, stderr=log)
                rows = wait_for_step(proc, evidence, 0, 'startup-frames', timeout=180)
                frames = next(row['detail'] for row in rows if row['step'] == 'startup-frames')
                assert frames and all(frame['skin'] == 'macos' for frame in frames), frames
                assert any(frame['phase'] == 'react' for frame in frames), frames
                conn = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
                daemon_pid = json.loads(conn.read_text())['pid']
                shot = out / f'{label}.png'
                captured = False
                screenshot_error = None
                try:
                    wid = subprocess.check_output([str(ROOT / 'target/gui-go/window-id'), str(proc.pid)], text=True, timeout=5).strip()
                    subprocess.run(['screencapture', '-x', '-o', '-l', wid, str(shot)], check=True, timeout=5)
                    captured = True
                except (subprocess.SubprocessError, OSError) as error:
                    screenshot_error = str(error)
                code = proc.wait(timeout=30)
                assert code == 0, code
                assert all(row['ok'] for row in read_steps(evidence, 0)), rows
                results['rounds'].append({'label': label, 'daemonPID': daemon_pid, 'frames': frames, 'screenshot': captured, 'screenshotError': screenshot_error, 'exitCode': code})
                if exit_mode == 'full':
                    deadline = time.monotonic() + 15
                    while conn.exists() and time.monotonic() < deadline:
                        time.sleep(.2)
                    assert not conn.exists(), 'full quit did not remove the owned daemon connection'
        assert results['rounds'][0]['daemonPID'] == results['rounds'][1]['daemonPID'], 'warm launch replaced the daemon'
        assert results['rounds'][1]['daemonPID'] != results['rounds'][2]['daemonPID'], 'restart did not start a new daemon'
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=30)
        cli(env, 'stop', check=False, timeout=80)
        results['connectionRemoved'] = not (Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn').exists()
        (out / 'results.json').write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(results, ensure_ascii=False, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
