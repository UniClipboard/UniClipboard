#!/usr/bin/env python3
"""Config package E2E: export, preview, cancel paths and a staged import that the next daemon boot applies.

Launch 1 drives the shared frontend bindings: save a setting, export a `.ucbundle` (native dialogs answered
by the e2e build), check the cancel paths, preview with a wrong and the right password, then change the
setting and stage the import. A full quit stops the daemon, and launch 2 starts it again: the staged
import must have been applied, which shows as the saved setting coming back.
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
from run import ROOT, isolated_env, read_steps  # noqa: E402

STEPS = ['set-position', 'export', 'export-cancelled', 'pick-bundle', 'pick-bundle-cancelled', 'preview-wrong-password',
         'preview', 'import-staged']
BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'


def launch(env, evidence, log, done_steps, timeout=150):
    proc = subprocess.Popen([str(BINARY)], env=env, stdout=log.open('w'), stderr=subprocess.STDOUT)
    deadline, seen = time.monotonic() + timeout, {}
    while time.monotonic() < deadline:
        for row in read_steps(evidence, 0):
            if row['step'] == 'driver-error':
                proc.terminate()
                raise RuntimeError(f"driver error: {row.get('detail')}")
            seen[row['step']] = row
        if proc.poll() is not None:
            seen.update({r['step']: r for r in read_steps(evidence, 0)})
            break
        time.sleep(.2)
    else:
        proc.terminate()
        raise RuntimeError('timeout')
    assert proc.returncode == 0, proc.returncode
    for step in done_steps:
        assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
    return seen


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-config-'))
    profile = 'gui-go-' + os.path.basename(home)
    bundle = work / 'config.ucbundle'
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    base = {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EXIT_MODE': 'full',
            'UC_GUI_GO_E2E_SECRET': PASSPHRASE, 'UC_GUI_GO_E2E_DIALOG_SAVE': f'{bundle}|',
            'UC_GUI_GO_E2E_DIALOG_OPEN': f'{bundle}|'}
    results = {'home': home, 'profile': profile, 'passed': False}
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'config-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        ev1 = out / 'config-export-native.jsonl'
        ev1.write_text('')
        seen = launch(isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev1), UC_GUI_GO_E2E_PHASE='config-export')),
                      ev1, out / 'config-export-gui.log', STEPS)
        assert bundle.is_file() and bundle.stat().st_size > 0, 'no bundle was written'
        results['bundleBytes'] = bundle.stat().st_size
        results['preview'] = seen['preview']['detail']['result']['data']
        results['wrongPassword'] = seen['preview-wrong-password']['detail']['result']['error']
        results['importStaged'] = seen['import-staged']['detail']['result']['data']
        ev2 = out / 'config-applied-native.jsonl'
        ev2.write_text('')
        seen2 = launch(isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev2), UC_GUI_GO_E2E_PHASE='config-applied')),
                       ev2, out / 'config-applied-gui.log', ['prefs'])
        prefs = seen2['prefs']['detail']
        results['prefsAfterReboot'] = prefs
        assert prefs['position'] == 'follow_cursor', f'the staged import was not applied: {prefs}'
        results['passed'] = True
    finally:
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'config-package-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
