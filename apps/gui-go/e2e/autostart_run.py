#!/usr/bin/env python3
"""Launch-at-login E2E against the Wails `app.Autostart` manager (pinned wails v3.0.0-beta.28).

Everything runs in quiet mode against a throwaway HOME and profile. Safety rules enforced before any native
enable: the test binary and bundle live under this repository's `target/`, never in /Applications; the test
bundle id ends in `.e2e` and its name differs from the real app; `launchctl` is a logging stub so Wails'
best-effort `launchctl bootstrap` can never start a real job; and the real ~/Library/LaunchAgents is fingerprinted
before and after.

Launches:
  1. bare binary (no bundle -> LaunchAgent strategy): repeated enable and disable, Status read back from Wails,
     the plist record at Status.Path, the launchctl stub log.
  2. startup reconcile: a stale entry is replaced, a Tauri-style primary `UniClipboard.plist` is never touched,
     and a healthy entry is not re-registered (no new launchctl bootstrap).
  3. read-only LaunchAgents directory: disable fails and the stored preference rolls back.
  4. bundle + profile with the production guard on: the OS is never reached.
  5. test bundle (SMAppService strategy): repeated enable/disable and the real mechanism; always followed by a
     disable-only cleanup launch. A real logout/login is NOT performed, so launch-at-login itself stays unverified.
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

BUNDLE = ROOT / 'target/gui-go/UniClipboardGoE2E.app'
BUNDLE_EXE = BUNDLE / 'Contents/MacOS/gui-go'
BARE_EXE = ROOT / 'target/gui-go/UniClipboardGoE2E-binary'
REAL_AGENTS = Path(os.path.expanduser('~')) / 'Library/LaunchAgents'
TAURI_EXE = '/Applications/UniClipboard.app/Contents/MacOS/uniclipboard'


def fingerprint(directory):
    """Names, sizes and mtimes of a directory's entries: any create/remove/rewrite changes it."""
    if not directory.exists():
        return None
    return sorted((e.name, e.stat().st_size, e.stat().st_mtime_ns) for e in directory.iterdir())


def launch(binary, env, evidence, log, steps):
    evidence.write_text('')
    proc = subprocess.Popen([str(binary)], env=env, stdout=log.open('w'), stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 120
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


def plist_at(detail):
    return plistlib.loads(detail['plist'].encode())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    # Safety: nothing may point at a real install, and the test bundle must be recognisably a test.
    for exe in (BARE_EXE, BUNDLE_EXE):
        assert str(exe.resolve()).startswith(str(ROOT / 'target/gui-go') + os.sep), exe
        assert not str(exe.resolve()).startswith('/Applications'), exe
    info = plistlib.loads((BUNDLE / 'Contents/Info.plist').read_bytes())
    assert info['CFBundleIdentifier'].endswith('.e2e'), info['CFBundleIdentifier']
    assert 'E2E' in info['CFBundleName'], info['CFBundleName']
    installed = Path('/Applications/UniClipboard.app/Contents/Info.plist')
    if installed.exists():
        assert plistlib.loads(installed.read_bytes())['CFBundleIdentifier'] != info['CFBundleIdentifier']

    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    stub_dir = Path(tempfile.mkdtemp(prefix='uc-launchctl-stub-'))
    stub_log = stub_dir / 'launchctl.log'
    stub = stub_dir / 'launchctl'
    stub.write_text(f'#!/bin/sh\necho "$@" >> "{stub_log}"\nexit 0\n')
    stub.chmod(0o755)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_path = f'{stub_dir}:{path}'
    base = {'PATH': gui_path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EXIT_MODE': 'full'}
    bare_env = {'UC_GUI_GO_E2E_UNBUNDLED': '1'}  # no bundle id: the notification service is skipped (test build only)
    agents = Path(home) / 'Library/LaunchAgents'
    item_name = 'UniClipboard-' + profile
    real_before = fingerprint(REAL_AGENTS)
    results = {'home': home, 'profile': profile, 'passed': False, 'bundleId': info['CFBundleIdentifier'],
               'bundleName': info['CFBundleName'], 'realLaunchAgentsUnchanged': None}
    bundle_ran = False

    def bootstraps():
        return stub_log.read_text().count('bootstrap ') if stub_log.exists() else 0

    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'autostart-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')

        def run_phase(binary, phase, steps, tag=None, extra=None):
            tag = tag or phase
            extra = dict(bare_env if binary == BARE_EXE else {}, **(extra or {}))
            ev = out / f'{tag}-native.jsonl'
            env = isolated_env(home, profile, dict(base, UC_GUI_GO_EVIDENCE=str(ev), UC_GUI_GO_E2E_PHASE=phase, **extra))
            return launch(binary, env, ev, out / f'{tag}-gui.log', steps)

        # 1. LaunchAgent strategy on the bare binary.
        seen = run_phase(BARE_EXE, 'autostart', ['enable', 'enable-repeat', 'disable', 'disable-repeat', 'enable-final'], 'launchagent')
        exe = os.path.realpath(seen['autostart-enabled']['detail']['executable'])
        assert exe == str(BARE_EXE.resolve()), exe
        agent = agents / f'{item_name}.plist'
        for label in ('enabled', 'enabled-repeat', 'enabled-final'):
            d = seen[f'autostart-{label}']['detail']
            assert d['setting'] is True and d['enabled'] is True and d['strategy'] == 'launchagent', d
            assert d['bundled'] is False and d['bundleID'] == '', d
            assert os.path.realpath(d['path']) == os.path.realpath(agent), d
            item = plist_at(d)
            assert item['Label'] == item_name == d['name'], item
            assert item['ProgramArguments'] == [exe, '--autostart'] and item['RunAtLoad'] is True, item
        for label in ('disabled', 'disabled-repeat'):
            d = seen[f'autostart-{label}']['detail']
            assert d['setting'] is False and d['enabled'] is False and d['strategy'] == '' and d['path'] == '', d
        assert agent.exists(), 'the final enable must leave the agent in place'
        assert sorted(p.name for p in agents.iterdir()) == [agent.name], list(agents.iterdir())
        launchctl = stub_log.read_text().splitlines()
        assert any(line.startswith('bootstrap gui/') and str(agent) in line for line in launchctl), launchctl
        assert any(line.startswith('bootout gui/') for line in launchctl), launchctl
        results['launchagent'] = {'strategy': 'launchagent', 'label': item_name, 'plist': plist_at(seen['autostart-enabled']['detail']),
                                  'launchctlStub': launchctl}

        # 2. Startup reconcile: stale entry replaced; Tauri-style primary entry never touched; healthy entry left alone.
        tauri = agents / 'UniClipboard.plist'
        tauri.write_bytes(plistlib.dumps({'Label': 'UniClipboard', 'ProgramArguments': [TAURI_EXE, '--autostart'], 'RunAtLoad': True}))
        tauri_before = tauri.read_bytes()
        agent.write_bytes(plistlib.dumps({'Label': item_name, 'ProgramArguments': ['/old/place/gui-go', '--autostart'], 'RunAtLoad': True}))
        seen = run_phase(BARE_EXE, 'autostart-reconcile', ['autostart-after-startup'], 'reconcile-stale')
        d = seen['autostart-after-startup']['detail']
        assert d['enabled'] is True and d['strategy'] == 'launchagent', d
        assert plist_at(d)['ProgramArguments'] == [exe, '--autostart'], d
        assert tauri.read_bytes() == tauri_before, 'a profile instance rewrote or removed the primary login item'
        count = bootstraps()
        stat_before = agent.stat()
        run_phase(BARE_EXE, 'autostart-reconcile', ['autostart-after-startup'], 'reconcile-healthy')
        assert bootstraps() == count, 'a healthy registration was bootstrapped again at startup'
        assert agent.stat().st_mtime_ns == stat_before.st_mtime_ns, 'a healthy registration was rewritten at startup'
        # A legacy entry that carries this login item's own name but another executable is swept.
        agent.write_bytes(plistlib.dumps({'Label': item_name, 'ProgramArguments': [TAURI_EXE, '--autostart'], 'RunAtLoad': True}))
        seen = run_phase(BARE_EXE, 'autostart-reconcile', ['autostart-after-startup'], 'reconcile-legacy')
        assert plist_at(seen['autostart-after-startup']['detail'])['ProgramArguments'] == [exe, '--autostart']
        assert tauri.read_bytes() == tauri_before
        results['reconcile'] = {'stale': 'healed', 'healthy': 'not re-registered', 'legacySameName': 'swept', 'primaryEntry': 'untouched'}

        # 3. OS failure rolls the preference back.
        agents.chmod(0o500)
        try:
            seen = run_phase(BARE_EXE, 'autostart-rollback', ['disable-fails', 'autostart-after-failure'], 'rollback')
        finally:
            agents.chmod(0o700)
        after = seen['autostart-after-failure']['detail']
        assert after['setting'] is True and after['enabled'] is True, after
        results['rolledBack'] = True
        # Leave the preference off and the disk clean for the bundle launches.
        run_phase(BARE_EXE, 'autostart-cleanup', ['cleanup-disable'], 'launchagent-cleanup')
        assert not agent.exists()
        tauri.unlink()

        # 4. Production guard: a named profile inside a bundle must not reach the OS.
        bootstraps_before = bootstraps()
        seen = run_phase(BUNDLE_EXE, 'autostart-refused', ['enable-refused'], 'refused', {'UC_GUI_GO_E2E_DENY_PROFILE_BUNDLE': '1'})
        d = seen['autostart-after-refusal']['detail']
        assert d['setting'] is False and d['enabled'] is False and d['bundled'] is True, d
        assert d['bundleID'] == info['CFBundleIdentifier'], d
        assert bootstraps() == bootstraps_before and not list(agents.iterdir())
        results['profileGuard'] = 'refused inside the bundle; preference rolled back; OS untouched'

        # 5. SMAppService strategy on the test bundle (real BTM entry for the `.e2e` id; always cleaned below).
        bundle_ran = True
        seen = run_phase(BUNDLE_EXE, 'autostart-bundle', ['enable', 'enable-repeat', 'disable', 'disable-repeat', 'enable-final'], 'smappservice')
        enable_result = seen['enable']['detail']['result']
        states = {k: seen[f'autostart-{k}']['detail'] for k in ('enabled', 'enabled-repeat', 'disabled', 'disabled-repeat', 'enabled-final')}
        for d in states.values():
            assert d['bundled'] is True and d['bundleID'] == info['CFBundleIdentifier'], d
        assert not list(agents.iterdir()), 'the SMAppService path must not write a LaunchAgent'
        if enable_result['status'] == 'ok':
            for k in ('enabled', 'enabled-repeat', 'enabled-final'):
                d = states[k]
                # RequiresApproval is reported by Wails as not enabled; record it rather than hide it.
                assert (d['enabled'] and d['strategy'] == 'smappservice' and d['path'] == info['CFBundleIdentifier']) or not d['enabled'], d
            results['smappservice'] = {'registered': states['enabled']['enabled'], 'strategy': states['enabled']['strategy'],
                                       'path': states['enabled']['path'], 'repeatStable': states['enabled'] == states['enabled-repeat'] or
                                       states['enabled-repeat']['enabled'] is states['enabled']['enabled']}
            for k in ('disabled', 'disabled-repeat'):
                assert states[k]['enabled'] is False and states[k]['strategy'] == '', states[k]
        else:
            msg = enable_result['error']['message']
            assert 'SMAppService register' in msg, msg
            assert all(not d['setting'] for d in states.values() if d is states['enabled'])
            results['smappservice'] = {'registered': False, 'registerError': msg}
        results['passed'] = True
    finally:
        try:
            agents.chmod(0o700)
        except OSError:
            pass
        if bundle_ran:
            try:
                seen = run_phase(BUNDLE_EXE, 'autostart-cleanup', ['cleanup-disable'], 'smappservice-cleanup')
                cleaned = seen['autostart-cleaned']['detail']
                results['bundleCleanup'] = {'enabled': cleaned['enabled'], 'strategy': cleaned['strategy']}
                assert cleaned['enabled'] is False, cleaned
            except Exception as exc:  # noqa: BLE001
                results['passed'] = False
                results['bundleCleanup'] = f'FAILED: {exc}'
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        results['realLaunchAgentsUnchanged'] = fingerprint(REAL_AGENTS) == real_before
        if not results['realLaunchAgentsUnchanged']:
            results['passed'] = False
        (out / 'autostart-assertions.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(json.dumps(results, indent=2, default=str))
    assert results['passed']


if __name__ == '__main__':
    main()
