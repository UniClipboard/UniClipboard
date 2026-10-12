#!/usr/bin/env python3
"""Run sidebar-toggle in the real Wails WKWebView with a synthetic isolated profile.

Build first with apps/gui-go/build.sh e2e. This visibly keeps the test window in front
for up to two minutes; schedule it with the user before running on a personal Mac.
No OS keyboard input or system clipboard access is used. Native reduced-motion
uses the existing system preference and is reported, never changed by this runner.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from file_preview_run import PASSPHRASE, cli
from history_kit_ui_run import tools
from run import ROOT, isolated_env, read_steps


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
    home = tempfile.mkdtemp(prefix='uc-gui-go-sidebar-')
    profile = 'gui-go-' + os.path.basename(home)
    env = isolated_env(home, profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    evidence = out / 'native.jsonl'
    evidence.write_text('')
    result = {'passed': False, 'home': home, 'profile': profile, 'systemClipboardDisabled': True,
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'guiSha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'daemonSha256': hashlib.sha256((ROOT / 'target/debug/uniclipd').read_bytes()).hexdigest(),
              'nativeReducedMotionForced': False, 'osKeyboardInput': False, 'shots': []}
    proc = None
    try:
        with tempfile.TemporaryDirectory(prefix='uc-sidebar-tools-') as tmp:
            bin_ = tools(tmp)
            cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'Sidebar test')
            cli(env, 'start')
            gui_env = dict(env, UC_GPUI_QUICK_PANEL='0', UC_GUI_GO_ISOLATED='1',
                           UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_PHASE='sidebar-toggle',
                           UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_VISIBLE='1')
            with (out / 'gui.log').open('w') as log:
                proc = subprocess.Popen([str(binary)], env=gui_env, stdout=log, stderr=subprocess.STDOUT)
                result['guiPid'] = proc.pid
                deadline = time.monotonic() + 120
                last_front = 0
                steps = {}
                while time.monotonic() < deadline:
                    for row in read_steps(evidence, 0):
                        if row['step'] in steps:
                            continue
                        steps[row['step']] = row
                        if row['step'].startswith('shot:'):
                            name = row['step'][5:]
                            wid = subprocess.check_output([str(bin_['window_id']), str(proc.pid)], text=True, timeout=15).strip()
                            subprocess.run(['screencapture', '-x', '-o', '-l', wid, str(out / f'{name}.png')], check=True, timeout=15)
                            result['shots'].append(name)
                    if 'sidebar-toggle-complete' in steps or proc.poll() is not None:
                        break
                    if time.monotonic() - last_front > 3:
                        subprocess.run([str(bin_['activate']), str(proc.pid)], check=True, timeout=15)
                        last_front = time.monotonic()
                    time.sleep(.1)
                result['steps'] = steps
                assert 'sidebar-toggle-complete' in steps, 'native driver did not complete'
                assert all(row['ok'] for row in steps.values()), 'native assertions failed'
                assert proc.wait(timeout=30) == 0
                result['passed'] = True
    finally:
        if proc and proc.poll() is None:
            # Verify executable identity before signalling the still-owned child PID.
            command = subprocess.check_output(['ps', '-p', str(proc.pid), '-o', 'comm='], text=True).strip()
            if Path(command).resolve() != binary.resolve():
                raise RuntimeError('refusing to signal an executable identity mismatch')
            proc.terminate()
            proc.wait(timeout=30)
        subprocess.run([str(ROOT / 'target/gui-go/uniclip'), 'stop'], env=env, capture_output=True, timeout=60)
        result['syntheticHomeRetained'] = True
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'passed': result['passed'], 'out': str(out)}))


if __name__ == '__main__':
    main()
