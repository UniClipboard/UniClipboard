#!/usr/bin/env python3
"""Launch-at-login E2E: the stored preference and the macOS LaunchAgent move together.

Three launches against one isolated profile (the HOME is a throwaway directory, so the real
~/Library/LaunchAgents is never touched, and a named profile gets its own login item name):
  1. enable writes the LaunchAgent (label, `<exe> --autostart`, RunAtLoad), disable removes it, enable again.
  2. the entry is made stale on disk; the startup reconcile rewrites it to the current executable.
  3. the LaunchAgents directory is made read-only; disabling fails, and the preference is rolled back to
     enabled so it never claims a state the OS did not reach.
"""
import argparse
import json
import os
import plistlib
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'


def launch(env, evidence, log, steps):
    evidence.write_text('')
    proc = subprocess.Popen([str(BINARY)], env=env, stdout=log.open('w'), stderr=subprocess.STDOUT)
    deadline, seen = time.monotonic() + 120, {}
    while time.monotonic() < deadline and proc.poll() is None:
        for row in read_steps(evidence, 0):
            if row['step'] == 'driver-error':
                proc.terminate()
                raise RuntimeError(f"driver error: {row.get('detail')}")
        time.sleep(.2)
    assert proc.wait(timeout=60) == 0
    seen = {r['step']: r for r in read_steps(evidence, 0)}
    for step in steps:
        assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
    return seen


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    base = {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EXIT_MODE': 'full'}
    agents = Path(home) / 'Library/LaunchAgents'
    results = {'home': home, 'profile': profile, 'passed': False}
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'autostart-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')

        def run_phase(phase, steps):
            ev = out / f'{phase}-native.jsonl'
            return launch(isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev), UC_GUI_GO_E2E_PHASE=phase)),
                          ev, out / f'{phase}-gui.log', steps)

        seen = run_phase('autostart', ['enable', 'autostart-enabled', 'disable', 'autostart-disabled', 'enable-again', 'autostart-enabled-again'])
        enabled = seen['autostart-enabled']['detail']
        assert enabled['setting'] is True and enabled['registered'] is True, enabled
        item = plistlib.loads(enabled['plist'].encode())
        exe = os.path.realpath(enabled['executable'])
        assert item['Label'] == enabled['name'] == 'UniClipboard-' + profile, item
        assert item['ProgramArguments'] == [exe, '--autostart'] and item['RunAtLoad'] is True, item
        assert exe.endswith('UniClipboardGoE2E.app/Contents/MacOS/gui-go'), exe
        disabled = seen['autostart-disabled']['detail']
        assert disabled['setting'] is False and disabled['registered'] is False, disabled
        assert seen['autostart-enabled-again']['detail']['registered'] is True
        results['loginItem'] = item
        assert not (agents / 'UniClipboard.plist').exists(), 'the primary app login item must stay untouched'

        # Round 2: a stale entry (moved binary) is rewritten at startup because the preference is enabled.
        agent = agents / f'UniClipboard-{profile}.plist'
        stale = dict(item, ProgramArguments=['/old/place/gui-go', '--autostart'])
        agent.write_bytes(plistlib.dumps(stale))
        seen = run_phase('autostart-reconcile', ['autostart-after-startup'])
        healed = plistlib.loads(seen['autostart-after-startup']['detail']['plist'].encode())
        assert healed['ProgramArguments'] == [exe, '--autostart'], healed
        results['healed'] = True

        # Round 3: the OS side cannot change; the preference must roll back.
        agents.chmod(0o500)
        try:
            seen = run_phase('autostart-rollback', ['disable-fails', 'autostart-after-failure'])
        finally:
            agents.chmod(0o700)
        after = seen['autostart-after-failure']['detail']
        assert after['setting'] is True and after['registered'] is True, after
        results['rolledBack'] = True
        results['passed'] = True
    finally:
        try:
            agents.chmod(0o700)
        except OSError:
            pass
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'autostart-assertions.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(json.dumps(results, indent=2, default=str))
    assert results['passed']


if __name__ == '__main__':
    main()
