#!/usr/bin/env python3
"""Two isolated native WebViews, real daemons, real file bytes, visible progress assertions.

Failure modes: dispatch fails, bytes never arrive, received bytes differ, or either
WebView never displays a numeric progress value during the actual transfer.
Only synthetic data is used. Tokens and pairing codes never enter evidence.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request

from run import ROOT, isolated_env, read_steps, screenshot
from file_preview_run import cli
from peers import pair


def capture_gui(owner, path):
    # The independently identified UI child owns the window, not the tray owner.
    rows = subprocess.check_output(['ps', '-Ao', 'pid=,ppid=,comm='], text=True).splitlines()
    for row in rows:
        fields = row.strip().split(None, 2)
        if len(fields) == 3 and int(fields[1]) == owner and 'Helpers/UniClipboardUI.app/' in fields[2]:
            return screenshot(int(fields[0]), path)
    return False


def outbound_states(home):
    # Credentials stay in memory; evidence contains only non-content task facts.
    conn = json.loads(next(home.rglob('daemon.conn')).read_text())
    base = f"http://{conn['host']}:{conn['port']}"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(path, auth, body=None):
        req = urllib.request.Request(base + path, headers={'Authorization': auth, 'Content-Type': 'application/json'},
                                     data=json.dumps(body).encode() if body else None)
        with opener.open(req, timeout=10) as response:
            return json.load(response)['data']
    session = request('/auth/connect', 'Bearer ' + conn['token'], {'pid': os.getpid(), 'clientType': 'gui'})['sessionToken']
    return [{key: snapshot[key] for key in ['state', 'freshness', 'itemsTotal', 'itemsCompleted']}
            for snapshot in request('/clipboard/outbound', 'Session ' + session)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--mib', type=int, default=64)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
    homes = [Path(tempfile.mkdtemp(prefix='uc-gui-go-progress-')) for _ in range(2)]
    envs = [isolated_env(str(home), 'gui-go-' + home.name, {
        'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'],
        'UC_GUI_GO_ISOLATED': '1', 'UC_GPUI_QUICK_PANEL': '0',
        'UC_GUI_GO_E2E_PHASE': 'file-progress', 'UC_GUI_GO_EXIT_MODE': 'keep',
        'UC_GUI_GO_E2E_VISIBLE': '1',
        'UC_GUI_GO_EVIDENCE': str(out / f'peer-{i}.jsonl'),
    }) for i, home in enumerate(homes)]
    result = {'passed': False, 'clipboardDisabled': True, 'mib': args.mib,
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
              'homes': [str(home) for home in homes],
              'scope': 'Explicit file dispatch; two native Desktop WebViews'}
    processes = []
    try:
        pair(*envs, 'progress-sender', 'progress-receiver')
        result['paired'] = True
        fixture = homes[0] / 'synthetic-transfer.bin'
        digest = hashlib.sha256()
        with fixture.open('wb') as stream:
            for _ in range(args.mib):
                block = os.urandom(1024 * 1024)
                stream.write(block)
                digest.update(block)
        result['sha256'] = digest.hexdigest()
        for i, env in enumerate(envs):
            Path(env['UC_GUI_GO_EVIDENCE']).write_text('')
            processes.append(subprocess.Popen([str(binary)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        deadline = time.monotonic() + 100
        while time.monotonic() < deadline:
            if all(any(row['step'] == 'progress-observer-ready' for row in read_steps(Path(env['UC_GUI_GO_EVIDENCE']), 0)) for env in envs):
                break
            time.sleep(.2)
        else:
            raise RuntimeError('native observers did not become ready')
        send = subprocess.Popen([str(ROOT / 'target/gui-go/uniclip'), '--json', 'send', '--file', str(fixture)],
                                env=envs[0], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        processes.append(send)
        shots = set()
        deadline = time.monotonic() + 140
        received = None
        while time.monotonic() < deadline:
            for i, env in enumerate(envs):
                rows = read_steps(Path(env['UC_GUI_GO_EVIDENCE']), 0)
                if i not in shots and any(row['step'] == 'transfer-event' for row in rows):
                    shots.add(i)
                    result[f'screenshot{i}'] = capture_gui(processes[i].pid, out / f'peer-{i}.png')
            matches = [path for path in homes[1].rglob('*') if path.is_file()]
            received = next((path for path in matches if path.stat().st_size == fixture.stat().st_size), None)
            if received and send.poll() is not None:
                result['terminalStates'] = outbound_states(homes[0])
                if result['terminalStates'] and all(snapshot['state'] == 'completed' for snapshot in result['terminalStates']):
                    break
            time.sleep(.1)
        result['dispatchExit'] = send.poll()
        result['received'] = received is not None
        result['hashMatches'] = received is not None and hashlib.sha256(received.read_bytes()).hexdigest() == digest.hexdigest()
        result['visibleProgress'] = []
        for i, env in enumerate(envs):
            rows = read_steps(Path(env['UC_GUI_GO_EVIDENCE']), 0)
            samples = [row['detail'] for row in rows if row['step'] == 'progress-dom']
            values = {float(bar['value']) for sample in samples for bar in sample['bars'] if bar['value'] is not None}
            result.setdefault('progressValues', []).append(sorted(values))
            result['visibleProgress'].append(len(values) >= 2 and any(0 < value < 100 for value in values))
            if i not in shots:
                result[f'screenshot{i}'] = capture_gui(processes[i].pid, out / f'peer-{i}.png')
        result['passed'] = result['hashMatches'] and all(result['visibleProgress']) and bool(result.get('terminalStates')) and all(
            snapshot['state'] == 'completed' for snapshot in result['terminalStates'])
    finally:
        for proc in processes:
            if proc.poll() is None:
                proc.terminate()
                proc.wait(timeout=20)
        for env in envs:
            cli(env, '--json', 'stop', check=False, timeout=60)
        result['binaries'] = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in
                              [binary, ROOT / 'target/debug/uniclipd', ROOT / 'target/gui-go/uniclip'] if path.exists()}
        (out / 'assertions.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    assert result['passed'], 'real transfer or double-sided visible progress failed; inspect assertions.json'


if __name__ == '__main__':
    main()
