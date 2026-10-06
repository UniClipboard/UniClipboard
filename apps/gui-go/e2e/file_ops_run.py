#!/usr/bin/env python3
"""File and directory commands E2E: dialogs, the system opener and the log export, end to end.

The in-WebView driver calls each command through the same generated bindings the UI uses. The e2e
build answers native dialogs from UC_GUI_GO_E2E_DIALOG_<KIND> (a `|`-separated answer list, empty =
cancel) and records what the system opener would launch in UC_GUI_GO_E2E_OPEN_LOG instead of opening
the Finder or a viewer on the tester's desktop. The orchestrator then checks the effects on disk:
the picked directory, the saved image bytes, the one-copy image hand-off with a sanitized name and
0600 mode, the opener calls, the NotFound refusal, and the exported zip (logs plus the manifest).
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

PNG = bytes([137, 80, 78, 71, 13, 10, 26, 10, 0, 1, 2, 3])
STEPS = ['pick-directory-chosen', 'pick-directory-cancelled', 'save-image-cancelled', 'save-image-saved', 'open-image-first',
         'open-image-second', 'open-data-directory', 'open-logs-directory', 'reveal-existing', 'reveal-missing',
         'export-logs-cancelled', 'export-logs-saved']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-fileops-'))
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'file-ops-native.jsonl'
    evidence.write_text('')
    picked, saved, zip_path, open_log = work / 'picked', work / 'saved.png', work / 'logs.zip', work / 'open.log'
    picked.mkdir()
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {
        'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
        'UC_GUI_GO_E2E_PHASE': 'file-ops', 'UC_GUI_GO_EXIT_MODE': 'full', 'UC_GUI_GO_E2E_OPEN_LOG': str(open_log),
        'UC_GUI_GO_E2E_DIALOG_DIRECTORY': f'{picked}|', 'UC_GUI_GO_E2E_DIALOG_SAVE': f'|{saved}|' + f'|{zip_path}'})
    results = {'home': home, 'profile': profile, 'passed': False}
    proc = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'fileops-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'file-ops-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline, seen = time.monotonic() + 120, {}
        while time.monotonic() < deadline and not all(s in seen for s in STEPS):
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                seen[row['step']] = row
            if proc.poll() is not None:
                seen.update({r['step']: r for r in read_steps(evidence, 0)})
                break
            time.sleep(.2)
        assert proc.wait(timeout=60) == 0
        for step in STEPS:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        res = {k: seen[k]['detail']['result'] for k in STEPS}
        assert res['pick-directory-chosen']['data'] == str(picked)
        assert res['pick-directory-cancelled']['data'] is None
        assert res['save-image-cancelled']['data'] is None
        assert res['save-image-saved']['data'] == str(saved)
        assert saved.read_bytes() == PNG, 'saved image differs'
        handoff = Path(tempfile.gettempdir()) / 'uniclipboard-image-handoff'
        assert not (handoff / 'first.png').exists(), 'the previous hand-off copy was not wiped'
        second = handoff / 'second.png'
        assert second.read_bytes() == PNG[:4] and oct(second.stat().st_mode & 0o777) == '0o600', 'hand-off copy wrong'
        data_root = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile)
        logs_dir = Path(home) / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile)
        calls = open_log.read_text().splitlines()
        expected = [f'open {handoff}/first.png', f'open {second}', f'open {data_root}', f'open {logs_dir}', f'reveal {picked}']
        os_calls = [c.replace('/private', '', 1) if c.split(' ', 1)[1].startswith('/private') else c for c in calls]
        for want in expected:
            assert any(c.replace('/private/', '/') == want.replace('/private/', '/') for c in os_calls), f'missing opener call {want}: {calls}'
        assert logs_dir.is_dir()
        assert res['reveal-missing']['error']['code'] == 'NotFound'
        with zipfile.ZipFile(zip_path) as z:
            names = z.namelist()
            manifest = json.loads(z.read('manifest.json'))
        assert 'manifest.json' in names and any(n.startswith('logs/') for n in names), names
        assert manifest['schemaVersion'] == 1 and manifest['mode'] == 'offline', manifest
        # The daemon only serves /startup while starting (the Rust export behaves the same), so a healthy
        # daemon yields no snapshot; the manifest field must still be present.
        assert 'startupStatus' in manifest
        assert sorted(manifest['collection']['includedFiles']) == sorted(n[5:] for n in names if n.startswith('logs/'))
        assert oct(zip_path.stat().st_mode & 0o777) == '0o600'
        results.update({'openerCalls': calls, 'zipEntries': names, 'manifestKeys': sorted(manifest), 'passed': True})
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'file-ops-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
