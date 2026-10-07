#!/usr/bin/env python3
"""Tray completion E2E: the device-sync submenu with a real paired peer, the localized menu order, the
system notification bridge, and the lightweight-mode exit.

Two CLI profiles are paired through the production rendezvous service (needs network); peer A is also the
GUI profile. The in-WebView driver and the e2e build then check, natively:
  - the root menu order matches the Tauri tray (sync, device sync submenu, separator, open, settings,
    check update, separator, restart, lightweight, quit) and the submenu lists the peer as a checked item;
  - clicking the peer's item (the item's own click handler) saves send/receive preferences through the
    daemon, flips the check mark, and clicking again restores it; the daemon values are read back;
  - a language change relabels the whole menu including the submenu;
  - the notification plugin shim reaches the host (the e2e build records notifications instead of showing
    them, since the real service needs the user's permission and would pop UI on the tester's desktop);
  - the tray's lightweight item shows the bilingual "still running" notice, exits the GUI and leaves the
    daemon running.
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
from file_preview_run import cli  # noqa: E402
from peers import pair  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

STEPS = ['tray-language-quiet', 'tray-menu-initial', 'tray-device-listed', 'tray-device-toggled', 'tray-language-set', 'tray-menu-zh', 'notification-bridge']


def labels(items):
    return [i if isinstance(i, str) else i['label'] for i in items]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-peer-')
    prof_a, prof_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    evidence = out / 'tray-devices-native.jsonl'
    evidence.write_text('')
    notify_log = Path(tempfile.mkdtemp(prefix='uc-gui-go-notify-')) / 'notifications.log'
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a = isolated_env(home_a, prof_a, {'PATH': path})
    env_b = isolated_env(home_b, prof_b, {'PATH': path})
    gui_env = isolated_env(home_a, prof_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1',
                                           'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'tray-devices',
                                           'UC_GUI_GO_EXIT_MODE': 'full', 'UC_GUI_GO_E2E_NOTIFY_LOG': str(notify_log)})
    results = {'profileA': prof_a, 'profileB': prof_b, 'passed': False}
    proc = None
    try:
        pair(env_a, env_b, 'tray-a', 'tray-peer-b')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'tray-devices-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline and proc.poll() is None:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
            time.sleep(.3)
        assert proc.wait(timeout=60) == 0, 'GUI did not exit cleanly after the lightweight item'
        rows = read_steps(evidence, 0)
        seen = {r['step']: r for r in rows}
        for step in STEPS:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'

        # Precondition (17c15): the English pin came after the frontend's own startup tray-language calls, so it was not overwritten.
        # The host language decides what the frontend sends, so it is recorded, not assumed.
        calls = [r['detail'] for r in rows if r['step'] == 'tray-language-call']
        results['trayLanguageCalls'] = calls
        results['hostLanguages'] = subprocess.run(['defaults', 'read', '-g', 'AppleLanguages'], capture_output=True, text=True).stdout.split()
        order = [r['step'] if r['step'] != 'tray-language-call' else 'call:' + r['detail'] for r in rows]
        quiet_at, initial_at = order.index('tray-language-quiet'), order.index('tray-menu-initial')
        pinned = [i for i, name in enumerate(order) if name == 'call:en' and quiet_at < i < initial_at]
        assert pinned and not [n for n in order[pinned[0] + 1:initial_at] if n.startswith('call:')], f'the English pin was not the last language call before the menu was read: {order}'
        menu = seen['tray-menu-initial']['detail']
        assert labels(menu)[0] in ('Enable Sync', 'Disable Sync'), labels(menu)
        assert seen['tray-language-en']['ok']
        assert labels(menu)[1:] == ['Device Sync', '-', 'Open', 'Settings', 'Check for Updates…', '-', 'Restart',
                                    'Lightweight Mode (Background Sync)', 'Quit'], labels(menu)
        listed = seen['tray-device-listed']['detail']
        assert listed['checked'] is True and listed['enabled'] is True, listed
        toggles = [r for r in rows if r['step'] == 'tray-device-toggled']
        assert len(toggles) == 2, toggles
        off, on = toggles[0]['detail'], toggles[1]['detail']
        assert off['checked'] is False and off['send'] is False and off['receive'] is False, off
        assert on['checked'] is True and on['send'] is True and on['receive'] is True, on
        zh = seen['tray-menu-zh']['detail']
        assert labels(zh)[1:] == ['设备同步', '-', '打开', '设置', '检查更新…', '-', '重启', '轻量模式（后台同步）', '退出'], labels(zh)
        submenu = [i for i in menu if isinstance(i, dict) and 'items' in i][0]['items']
        assert [i['label'] for i in submenu] == ['tray-peer-b'], submenu

        lines = notify_log.read_text().splitlines()
        assert any(line.startswith('21021|Device trust|Needs a decision') for line in lines), lines
        notice = [line for line in lines if line.startswith('lightweight-')]
        assert notice and 'UniClipboard 仍在后台运行' in notice[0] and 'Still running in the background' in notice[0], lines
        # Lightweight keeps the daemon running; the orchestrator stops it afterwards.
        daemon_alive = cli(env_a, '--json', 'start', check=False).returncode == 0
        results.update({'menuOrder': labels(menu), 'zhMenu': labels(zh), 'submenu': submenu, 'toggleOff': off, 'toggleOn': on,
                        'notifications': lines, 'daemonSurvivedLightweight': daemon_alive, 'passed': daemon_alive})
        assert daemon_alive, 'the daemon did not survive the lightweight exit'
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=80)
        (out / 'tray-devices-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps(results, indent=2, ensure_ascii=False))
    assert results['passed']


if __name__ == '__main__':
    main()
