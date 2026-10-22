#!/usr/bin/env python3
"""Windows/Linux native lifecycle acceptance on an explicitly authorized dedicated host.

Use an E2E build with its platform frontend and daemon/CLI siblings. This script opens
windows. It never uses the system clipboard. It does not validate installer/AppImage updates.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--allow-session', action='store_true')
    ap.add_argument('--binaries', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    if not args.allow_session or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        ap.error('requires --allow-session and UC_GUI_GO_E2E_DEDICATED_HOST=1')
    if sys.platform not in ('win32', 'linux'):
        ap.error('use ui_process_run.py on macOS')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
    profile = 'gui-go-' + sandbox.name
    suffix = '.exe' if os.name == 'nt' else ''
    names = ['gui-go' + suffix, 'uniclipd' + suffix, 'uniclip' + suffix]
    helper = 'uniclip-quick-panel' + suffix
    if (args.binaries / helper).exists():
        names.append(helper)
    for name in names:
        shutil.copy2(args.binaries / name, sandbox / name)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('UC_', 'UNICLIPBOARD', 'APPIMAGE', 'APPDIR'))}
    evidence, control = out / 'native.jsonl', out / 'gui.control'
    evidence.write_text('')
    control.write_text('')
    env.update(UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development',
               UC_GUI_GO_ISOLATED='1', UC_DISABLE_SYSTEM_CLIPBOARD='1',
               UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
               UC_GUI_GO_E2E_SECRET='gui-go-synthetic-passphrase', UC_GUI_GO_EXIT_MODE='full',
               UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control))
    binary, cli = sandbox / names[0], sandbox / names[2]
    result = {'passed': False, 'platform': sys.platform, 'sandbox': str(sandbox),
              'checks': [], 'binaries': {name: digest(sandbox / name) for name in names},
              'notRun': ['OS tray clicks and keyboard shortcuts', 'installer/AppImage update',
                         'native panel interaction', 'cross-device synchronization']}
    result['sourceHead'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    result['sourceDiffSha256'] = hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)).hexdigest()
    changed = subprocess.check_output(['git', 'diff', '--name-only', 'HEAD'], cwd=ROOT, text=True).splitlines()
    changed += subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard'], cwd=ROOT, text=True).splitlines()
    result['sourceFiles'] = {p: digest(ROOT / p) for p in sorted(set(changed)) if (ROOT / p).is_file()}
    proc = None
    sequence = 0
    tracked = set()

    def rows():
        values = []
        for line in evidence.read_text().splitlines():
            try:
                values.append(json.loads(line))
            except ValueError:
                pass  # another process may still be appending its last line
        return values

    def command(line):
        with control.open('a') as f:
            f.write(line + '\n')

    def until(probe, description, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = probe()
            if value:
                return value
            if proc is not None and proc.poll() is not None:
                raise AssertionError('resident exited before ' + description)
            time.sleep(.2)
        raise AssertionError('timeout: ' + description)

    def state():
        nonlocal sequence
        sequence += 1
        label = str(sequence)
        command('state ' + label)
        return until(lambda: next((r['detail'] for r in rows() if r.get('step') == 'control-state'
                                  and r.get('detail', {}).get('label') == label), None), 'state reply', 20)

    def processes():
        if os.name == 'nt':
            raw = subprocess.check_output(['powershell.exe', '-NoProfile', '-Command',
                'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate,Name,CommandLine | ConvertTo-Json -Compress'], text=True)
            values = json.loads(raw)
            return [{'pid': r['ProcessId'], 'parent': r['ParentProcessId'], 'start': r['CreationDate'],
                     'name': r['Name'], 'command': r['CommandLine'] or ''} for r in values]
        values = []
        for path in Path('/proc').glob('[0-9]*'):
            try:
                stat = (path / 'stat').read_text()
                fields = stat[stat.rfind(')') + 2:].split()
                if fields[0] == 'Z':
                    continue
                values.append({'pid': int(path.name), 'parent': int(fields[1]), 'start': fields[19],
                               'name': stat[stat.find('(') + 1:stat.rfind(')')],
                               'command': (path / 'cmdline').read_bytes().replace(b'\0', b' ').decode(errors='replace')})
            except (OSError, ValueError):
                pass
        return values

    def browsers(ui_pid):
        values = processes()
        descendants = {ui_pid}
        for _ in values:
            before = len(descendants)
            descendants.update(r['pid'] for r in values if r['parent'] in descendants)
            if len(descendants) == before:
                break
        owned = []
        for row in values:
            browser = re.search(r'(?i)msedgewebview2|WebKit(Web|Network|GPU)', row['name'] + row['command'])
            if not browser:
                continue
            identity = (row['pid'], row['start'])
            attributable = row['pid'] in descendants or str(sandbox).lower() in row['command'].lower()
            if sys.platform == 'linux' and not attributable:
                try:
                    attributable = ('UC_PROFILE=' + profile).encode() in Path(f'/proc/{row["pid"]}/environ').read_bytes().split(b'\0')
                except OSError:
                    pass
            if attributable or identity in tracked:
                tracked.add(identity)
                owned.append({k: row[k] for k in ('pid', 'parent', 'start', 'name')})
        return owned

    def page_ready(pid):
        return until(lambda: next((r for r in rows() if r.get('step') == 'ui-page-ready' and r.get('ok')
                                  and r.get('detail', {}).get('pid') == pid), None), 'page/daemon round trip')

    try:
        subprocess.run([str(cli), '--json', 'space', 'init', '--passphrase', env['UC_GUI_GO_E2E_SECRET'],
                        '--device-name', 'ui-lifecycle'], env=env, check=True, capture_output=True, timeout=120)
        with (out / 'gui.log').open('w') as log:
            proc = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        initial = until(lambda: (s if (s := state()).get('bootstrapped') and s.get('mainVisible') else None), 'boot', 120)
        assert initial['uiPid'] != proc.pid and initial['uiPid'] > 0, initial
        assert initial['residentWindowCount'] == 0, initial
        previous = initial['uiPid']
        helpers = [(r['pid'], r['start']) for r in processes() if r['parent'] == proc.pid and r['name'] == helper]
        for cycle in range(3):
            page_ready(previous)
            baseline = until(lambda: browsers(previous), 'positive browser attribution')
            command('close-main cycle-' + str(cycle))
            closed = until(lambda: (s if (s := state()).get('uiPid') == 0 else None), 'UI exit')
            until(lambda: not browsers(previous), 'browser helpers exit', 45)
            assert closed['residentWindowCount'] == 0, closed
            assert closed['daemonPid'] == initial['daemonPid'], closed
            assert closed['startupMode'] == initial['startupMode'], closed
            assert closed.get('helperRunning') == initial.get('helperRunning'), closed
            assert [(r['pid'], r['start']) for r in processes() if r['parent'] == proc.pid and r['name'] == helper] == helpers
            # The daemon still answers a real client while the UI is absent.
            subprocess.run([str(cli), '--json', 'status'], env=env, check=True, capture_output=True, timeout=20)
            command('tray-menu closed-' + str(cycle))
            until(lambda: any(r.get('step') == 'tray-menu-closed-' + str(cycle) and r.get('ok') for r in rows()), 'tray retained')
            # Alternate the real second-launch route and the tray Open handler.
            if cycle == 1:
                command('open-main cycle-' + str(cycle))
            else:
                subprocess.run([str(binary)], env=env, check=True, timeout=30)
            opened = until(lambda: (s if (s := state()).get('mainVisible') and s.get('uiPid', 0) > 0 else None), 'reopen')
            assert opened['uiPid'] != previous and opened['daemonPid'] == initial['daemonPid'], opened
            result['checks'].append({'cycle': cycle, 'browserBefore': baseline, 'closed': closed, 'opened': opened})
            previous = opened['uiPid']
        page_ready(previous)
        command('quit-app')
        assert proc.wait(timeout=40) == 0
        deadline = time.monotonic() + 45
        while browsers(previous) and time.monotonic() < deadline:
            time.sleep(.3)
        assert not browsers(previous), 'browser helpers survived full quit'
        live = {r['pid'] for r in processes()}
        assert not any(pid in live for pid, _ in helpers), 'full quit left native helper'
        assert previous not in live and initial['daemonPid'] not in live, 'full quit left UI or daemon'
        result['passed'] = True
    finally:
        if proc is not None and proc.poll() is None:
            command('quit-app')
            try:
                proc.wait(timeout=40)
            except subprocess.TimeoutExpired:
                proc.kill()  # exact owned Popen handle; never kill by process name
                proc.wait()
        cleanup = subprocess.run([str(cli), '--json', 'stop'], env=env, capture_output=True, text=True, timeout=80)
        result['cleanupCLIExit'] = cleanup.returncode
        (out / 'ui-process-assertions.json').write_text(json.dumps(result, indent=2) + '\n')
    assert result['passed']


if __name__ == '__main__':
    main()
