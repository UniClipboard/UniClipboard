#!/usr/bin/env python3
"""Host command contract E2E: the generated bindings called from the real WebView against a real daemon.

The in-WebView driver (phase `host-contract`) calls the commands through the same wrapper the pages use and records
what crossed the bridge: connection and identity results, typed events, business versus system error classification,
argument handling (null, wrong arity, unknown method, bad base64) and a real daemon restart. The orchestrator checks
the part the page cannot see: that the daemon process really changed, the old one is gone, and the new one is alive.
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
from receipt import write_receipt  # noqa: E402
from run import ROOT, isolated_env, pid_alive, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
STEPS = ['connection-info', 'daemon-session', 'device-identity', 'profile-recovery-passthrough', 'startup-status-passthrough',
         'content-unlocked-boolean', 'install-kind-enum', 'bootstrap-failure-null', 'visual-effects-typed-event',
         'desktop-theme-unavailable', 'error-validation-user-facing', 'error-not-found-user-facing',
         'null-argument-is-zero-value', 'error-text-is-system', 'error-wrong-arity-is-system',
         'error-unknown-method-is-system', 'error-bad-base64-is-system', 'restart-daemon-start', 'restart-daemon-result',
         'restart-daemon-events', 'restart-daemon-client-replaced', 'restart-daemon-new-session']


def daemon_pid(home, profile):
    conn = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
    return json.loads(conn.read_text())['pid']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'host-contract-native.jsonl'
    evidence.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                          'UC_GUI_GO_E2E_PHASE': 'host-contract', 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'home': home, 'profile': profile, 'passed': False}
    proc = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'contract-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        proc = subprocess.Popen([str(BINARY)], env=gui_env, stdout=(out / 'host-contract-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline, seen, old_pid = time.monotonic() + 180, {}, None
        while time.monotonic() < deadline and not all(s in seen for s in STEPS):
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                seen[row['step']] = row
            if 'restart-daemon-start' in seen and old_pid is None:
                old_pid = daemon_pid(home, profile)  # read before the page's RestartDaemon call returns
            if proc.poll() is not None:
                seen.update({r['step']: r for r in read_steps(evidence, 0)})
                break
            time.sleep(.1)
        assert old_pid is not None, 'restart step never started'
        new_pid = daemon_pid(home, profile)
        assert proc.wait(timeout=90) == 0, 'GUI did not exit cleanly'
        for step in STEPS:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        assert new_pid != old_pid, 'the daemon process did not change'
        assert not pid_alive(old_pid), 'the old daemon survived the restart'
        results.update({'oldDaemonPid': old_pid, 'newDaemonPid': new_pid, 'steps': {s: seen[s].get('detail') for s in STEPS}, 'passed': True})
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'host-contract-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        if results['passed']:
            write_receipt(out, 'host-contract',
                          scope={'commands': ['get_daemon_connection_info', 'get_daemon_session', 'get_device_id', 'get_device_meta',
                                              'get_profile_recovery', 'get_daemon_startup_status', 'get_content_unlocked', 'get_install_kind',
                                              'get_daemon_bootstrap_failure', 'set_visual_effects_mode', 'set_follow_omarchy_theme',
                                              'get_desktop_theme', 'reveal_path', 'install_update', 'save_image_as', 'restart_daemon'],
                                 'events': ['visual-effects://changed', 'app://shutting-down', 'app://daemon-connection-changed'],
                                 'errors': ['ValidationError', 'NotFound', 'text', 'wrong arity', 'unknown method', 'bad base64'],
                                 'layers': ['ipc wrapper', 'generated binding', 'Go service', 'daemon process']},
                          binaries={'gui': BINARY, 'daemon': ROOT / 'target/debug/uniclipd'},
                          before={'daemonPid': results.get('oldDaemonPid')}, after={'daemonPid': results.get('newDaemonPid')},
                          assertions=results)
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
