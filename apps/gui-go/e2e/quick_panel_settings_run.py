#!/usr/bin/env python3
"""Quick-panel settings E2E: preferences saved through the shared frontend API change the real panel.

Setup runs through the CLI so the daemon profile is complete. The in-WebView driver sets each preference
with the same settings API the UI uses; the native test service uses an injected pointer position, opens the panel and
checks its on-screen position against independently computed expectations (centered on the monitor, anchored
to the cursor, flipped at the screen corner), that a disabled panel stays hidden, and that an unsupported
modifier double-tap setting is refused. The panel window is captured while it is shown. Quiet mode: the pointer is injected, the panel is parked off-screen
and its placement recorded rather than applied (see updater_dev_e2e.go).
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps, screenshot  # noqa: E402

STEPS = ['panel-center', 'panel-follow-near', 'panel-follow-flipped', 'panel-disabled', 'panel-reenabled',
         'double-tap-unavailable-rejected', 'double-tap-disabled-accepted']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'quick-panel-settings-native.jsonl'
    evidence.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                           'UC_GUI_GO_E2E_PHASE': 'quick-panel-settings', 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'home': home, 'profile': profile, 'passed': False}
    proc = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'panel-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'quick-panel-settings-gui.log').open('w'), stderr=subprocess.STDOUT)
        seen, shot, deadline = {}, set(), time.monotonic() + 180
        while time.monotonic() < deadline and not all(step in seen for step in STEPS):
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                seen[row['step']] = row
                if row['step'].startswith('panel-') and row['step'] not in shot and row['step'] != 'panel-hide':
                    shot.add(row['step'])
                    if row['detail'].get('visible'):  # capture the panel window itself, wherever it is parked
                        screenshot(proc.pid, out / f"{row['step']}.png", '-')
            if proc.poll() is not None:
                seen.update({r['step']: r for r in read_steps(evidence, 0)})  # rows written just before exit
                break
            time.sleep(.1)
        for step in STEPS:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        results['steps'] = {k: seen[k].get('detail') for k in STEPS}
        assert proc.wait(timeout=60) == 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'quick-panel-settings-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
