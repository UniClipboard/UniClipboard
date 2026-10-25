#!/usr/bin/env python3
"""Native runtime-mode/activation acceptance, on an authorized dedicated desktop.

Uses a fresh profile and disabled system clipboard. Records real host/UI/daemon
identities; ui_process*_run.py separately checks native browser reclamation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow-session', action='store_true')
    parser.add_argument('--binaries', type=Path)
    parser.add_argument('--bundle', type=Path)
    parser.add_argument('--cli', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not args.allow_session or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        parser.error('requires --allow-session and UC_GUI_GO_E2E_DEDICATED_HOST=1')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-modes-'))
    profile = 'gui-go-' + sandbox.name
    evidence, control = out / 'native.jsonl', out / 'gui.control'
    evidence.write_text('')
    control.write_text('')
    if sys.platform == 'darwin':
        if not args.bundle or not args.cli:
            parser.error('macOS requires --bundle and --cli')
        sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
        from isolated import isolated_env
        env = isolated_env(str(sandbox), profile)
        binary, cli = args.bundle.resolve() / 'Contents/MacOS/gui-go', args.cli.resolve()
        root = sandbox / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile)
    else:
        if not args.binaries:
            parser.error('Windows/Linux require --binaries')
        suffix = '.exe' if os.name == 'nt' else ''
        for name in ('gui-go', 'uniclip', 'uniclipd', 'uniclip-quick-panel'):
            source = args.binaries / (name + suffix)
            if name != 'uniclip-quick-panel' or source.exists():
                shutil.copy2(source, sandbox / source.name)
        binary, cli = sandbox / ('gui-go' + suffix), sandbox / ('uniclip' + suffix)
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(('UC_', 'UNICLIPBOARD', 'APPIMAGE', 'APPDIR'))}
        env.update(UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development')
        root = sandbox / 'data' / ('app.uniclipboard.desktop-' + profile)
    env.update(UC_GUI_GO_ISOLATED='1', UC_DISABLE_SYSTEM_CLIPBOARD='1',
               UC_GPUI_QUICK_PANEL='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
               UC_GUI_GO_E2E_SECRET='gui-go-synthetic-passphrase', UC_GUI_GO_EXIT_MODE='full',
               UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control))
    result = {'passed': False, 'platform': sys.platform, 'checks': [], 'sandbox': str(sandbox),
              'sourceHead': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'notRun': ['physical login-item activation', 'cross-device synchronization',
                         'native main-window browser reclamation (separate lifecycle driver)']}
    processes = []
    sequence = 0
    token = None
    daemon_pid = None

    def until(probe, label, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            # Reap launcher exits so an internal handoff does not wait on a zombie.
            for proc in processes:
                proc.poll()
            value = probe()
            if value:
                return value
            time.sleep(.2)
        raise AssertionError('timeout: ' + label)

    def rows():
        values = []
        for line in evidence.read_text().splitlines():
            try:
                values.append(json.loads(line))
            except ValueError:
                pass
        return values

    def command(value):
        with control.open('a') as stream:
            stream.write(value + '\n')

    def state():
        nonlocal sequence
        # A replacement skips commands already in its predecessor's file.
        # Retry read-only snapshots with new labels across that handoff.
        for attempt in range(3):
            sequence += 1
            label = 'mode-' + str(sequence)
            command('state ' + label)
            try:
                return until(lambda: next((r['detail'] for r in rows() if r.get('step') == 'control-state'
                                           and r.get('detail', {}).get('label') == label), None), 'state', 20)
            except AssertionError:
                if attempt == 2:
                    raise

    def alive(pid):
        if os.name == 'nt':
            return subprocess.run(['powershell.exe', '-NoProfile', '-Command',
                                   f'if (Get-Process -Id {int(pid)} -ErrorAction SilentlyContinue) {{exit 0}} else {{exit 1}}'],
                                  capture_output=True).returncode == 0
        # Reap our own children before testing PID liveness.
        for proc in processes:
            proc.poll()
        if sys.platform == 'linux':
            try:
                stat = Path(f'/proc/{pid}/stat').read_text()
                if stat[stat.rfind(')') + 2:].split()[0] == 'Z':
                    return False
            except FileNotFoundError:
                return False
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def request(path, method='GET', body=None, auth=None):
        conn = json.loads((root / 'daemon.conn').read_text())
        req = urllib.request.Request(f"http://{conn['host']}:{conn['port']}{path}", method=method,
                                     data=json.dumps(body).encode() if body is not None else None,
                                     headers={'Authorization': auth or 'Session ' + token,
                                              'Content-Type': 'application/json'})
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=15) as response:
            return json.load(response)

    def settings(mode, background):
        request('/settings', 'PUT', {'general': {'startupMode': mode, 'backgroundStartup': background}})

    def launch(automatic=False, reset=False):
        if reset:
            control.write_text('')  # a fresh owner must not replay an earlier close/quit
        with (out / 'gui.log').open('a') as log:
            proc = subprocess.Popen([str(binary)] + (['--autostart'] if automatic else []),
                                    env=env, stdout=log, stderr=log)
        processes.append(proc)
        return proc

    def snapshot(mode, visible):
        value = until(lambda: (s if (s := state()).get('bootstrapped')
                               and s.get('startupMode') == mode and s.get('mainVisible') == visible
                               and s.get('lightweight') == (mode == 'lightweight') else None), 'settled mode')
        assert value['daemonPid'] == daemon_pid, value
        assert value['trayPresent'] == (mode == 'normal'), value
        if mode == 'lightweight':
            assert not value.get('helperRunning') and value['shortcutCount'] == 0, value
        if visible:
            until(lambda: any(r.get('step') == 'ui-page-ready' and r.get('ok') and
                              r.get('detail', {}).get('pid') == value['uiPid'] for r in rows()), 'page round trip')
        result['checks'].append(value)
        return value

    def daemon_unchanged():
        assert json.loads((root / 'daemon.conn').read_text())['pid'] == daemon_pid
        assert alive(daemon_pid)
        request('/health')
        subprocess.run([str(cli), '--json', 'status'], env=env, check=True, capture_output=True, timeout=20)

    try:
        subprocess.run([str(cli), '--json', 'space', 'init', '--passphrase', env['UC_GUI_GO_E2E_SECRET'],
                        '--device-name', 'desktop-modes'], env=env, check=True, capture_output=True, timeout=120)
        conn = json.loads((root / 'daemon.conn').read_text())
        daemon_pid = conn['pid']
        token = request('/auth/connect', 'POST', {'pid': os.getpid(), 'clientType': 'cli'},
                        'Bearer ' + conn['token'])['data']['sessionToken']
        settings('normal', True)
        launch(automatic=True, reset=True)
        hidden = snapshot('normal', False)
        launch().wait(timeout=30)
        opened = snapshot('normal', True)
        assert opened['pid'] == hidden['pid']
        for _ in range(3):
            launch().wait(timeout=30)
        repeated = snapshot('normal', True)
        assert (repeated['pid'], repeated['uiPid']) == (opened['pid'], opened['uiPid'])
        command('close-main normal')
        closed = snapshot('normal', False)
        assert closed['pid'] == opened['pid'] and closed['uiPid'] == 0
        launch(automatic=True).wait(timeout=30)
        hidden = snapshot('normal', False)
        settings('lightweight', True)
        until(lambda: not alive(hidden['pid']), 'hidden mode switch releases owner')
        daemon_unchanged()
        launch(reset=True)
        light = snapshot('lightweight', True)
        settings('normal', True)
        opened = snapshot('normal', True)
        until(lambda: not alive(light['pid']), 'normal mode replaces temporary owner')
        settings('lightweight', True)
        light = snapshot('lightweight', True)
        assert light['pid'] != opened['pid']
        until(lambda: not alive(opened['pid']), 'old normal owner exits')
        command('close-main light')
        until(lambda: not alive(light['pid']), 'temporary lightweight owner exits')
        until(lambda: not alive(light['uiPid']), 'temporary UI exits')
        daemon_unchanged()
        auto = launch(automatic=True, reset=True)
        assert auto.wait(timeout=90) == 0
        daemon_unchanged()
        launch(reset=True)
        light = snapshot('lightweight', True)
        assert request('/settings')['data']['general']['startupMode'] == 'lightweight'
        settings('normal', False)
        normal = snapshot('normal', True)
        assert normal['pid'] != light['pid']
        until(lambda: not alive(light['pid']), 'old lightweight owner exits')
        daemon_unchanged()
        command('quit-app')
        until(lambda: not alive(normal['pid']) and not alive(daemon_pid), 'full quit')
        result['passed'] = True
    finally:
        command('quit-app')
        cleanup = subprocess.run([str(cli), '--json', 'stop'], env=env, capture_output=True, timeout=80)
        result['cleanupCLIExit'] = cleanup.returncode
        for proc in processes:
            if proc.poll() is None:
                try:
                    proc.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    proc.terminate()  # only a handle created by this driver
                    proc.wait(timeout=15)
        (out / 'desktop-modes-assertions.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
