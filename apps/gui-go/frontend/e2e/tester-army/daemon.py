#!/usr/bin/env python3
"""Own one isolated daemon; exchange its session over pipes, never artifact files."""
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env

if sys.platform != 'darwin':
    raise RuntimeError('real daemon browser acceptance currently requires macOS isolation')
if ROOT != Path.cwd():
    raise RuntimeError('run from the repository root')
home = tempfile.mkdtemp(prefix='uc-gui-go-tester-')
profile = 'gui-go-' + Path(home).name
env = isolated_env(home, profile, {
    'PATH': str(ROOT / 'target/debug') + os.pathsep + os.environ['PATH'],
    'UC_GUI_GO_ISOLATED': '1',
})
cli = ROOT / 'target/gui-go/uniclip'
passphrase = secrets.token_urlsafe(24)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def command(*args):
    result = subprocess.run([str(cli), *args], env=env, capture_output=True, text=True, timeout=30)
    if result.returncode:
        # Do not print command arguments, session material, or daemon output.
        raise RuntimeError(f'isolated CLI exited {result.returncode}')
    return result


def post(base, path, authorization, body):
    req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={
        'Authorization': authorization, 'Content-Type': 'application/json',
    })
    with opener.open(req, timeout=30) as response:
        return json.load(response)['data']


try:
    command('space', 'init', '--passphrase', passphrase, '--device-name', 'TesterArmy browser')
    command('start')
    connections = list(Path(home).rglob('daemon.conn'))
    assert len(connections) == 1, 'expected one isolated connection'
    connection = json.loads(connections[0].read_text())
    base = f"http://127.0.0.1:{connection['port']}"
    session = post(base, '/auth/connect', 'Bearer ' + connection['token'], {
        'pid': os.getpid(), 'clientType': 'gui',
    })['sessionToken']
    post(base, '/content-lock/unlock', 'Session ' + session, {'passphrase': passphrase})
    text = 'TesterArmy isolated history acceptance'
    post(base, '/clipboard/dispatch', 'Session ' + session, {'text': text})
    print(json.dumps({'baseUrl': base, 'sessionToken': session, 'text': text}), flush=True)
    # Closing stdin requests cleanup. The test waits for our successful exit.
    sys.stdin.buffer.read()
finally:
    remaining = list(Path(home).rglob('daemon.conn'))
    if remaining:
        assert len(remaining) == 1, 'ambiguous isolated daemon ownership'
        connection = json.loads(remaining[0].read_text())
        command('--json', 'stop')
        for _ in range(100):
            try:
                os.kill(connection['pid'], 0)
            except ProcessLookupError:
                break
            time.sleep(.1)
        else:
            raise RuntimeError('isolated daemon did not exit; preserve its private sandbox')
    # Keep no key material or connection token in the published evidence.
    shutil.rmtree(home)

print(json.dumps({'phase': 'cleanup', 'profile': profile,
                  'daemonPid': connection['pid'] if 'connection' in globals() else None,
                  'homeRemoved': not Path(home).exists(), 'daemonExited': True,
                  'systemClipboardDisabled': env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1',
                  'guiIsolated': env['UC_GUI_GO_ISOLATED'] == '1'}), flush=True)
