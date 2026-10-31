#!/usr/bin/env python3
"""Own a disposable daemon and real ingestion paths; credentials stay in pipes and memory."""
import base64
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env


class Backend:
    def __init__(self, mode):
        if sys.platform != 'darwin' or Path.cwd() != ROOT:
            raise RuntimeError('backend acceptance requires repository-root macOS HOME isolation')
        self.mode = mode
        self.home = tempfile.mkdtemp(prefix='uc-gui-go-tester-')
        self.profile = 'gui-go-' + Path(self.home).name
        self.env = isolated_env(self.home, self.profile, {
            'PATH': str(ROOT / 'target/debug') + os.pathsep + os.environ['PATH'],
            'UC_GUI_GO_ISOLATED': '1',
        })
        self.passphrase = secrets.token_urlsafe(24)
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.connection = None
        self.start_attempted = False

    def command(self, *args, allow_send_failure=False, timeout=90):
        result = subprocess.run([str(ROOT / 'target/gui-go/uniclip'), *args], env=self.env,
                                capture_output=True, text=True, encoding='utf-8', timeout=timeout)
        if result.returncode and not (allow_send_failure and 'snapshotHash' in result.stdout):
            raise RuntimeError(f'isolated CLI exited {result.returncode}')
        return result

    def request(self, route, body=None, method=None, authorization=None):
        req = urllib.request.Request(self.base + route,
            data=None if body is None else json.dumps(body).encode(), method=method,
            headers={'Authorization': authorization or 'Session ' + self.session,
                     'Content-Type': 'application/json'})
        with self.opener.open(req, timeout=30) as response:
            return json.load(response)['data']

    def start(self):
        self.command('space', 'init', '--passphrase', self.passphrase, '--device-name', 'search-node' if self.mode == 'search' else 'kit-browser')
        self.start_attempted = True
        self.command('start')
        connections = list(Path(self.home).rglob('daemon.conn'))
        assert len(connections) == 1, 'expected one isolated daemon connection'
        self.connection = json.loads(connections[0].read_text(encoding='utf-8'))
        self.base = f"http://127.0.0.1:{self.connection['port']}"
        self.session = self.request('/auth/connect', {'pid': os.getpid(), 'clientType': 'gui'},
                                    authorization='Bearer ' + self.connection['token'])['sessionToken']
        self.request('/content-lock/unlock', {'passphrase': self.passphrase})
        self.request('/settings', {'general': {'language': 'en-US'}}, method='PUT')

    def send(self, text):
        self.request('/clipboard/dispatch', {'text': text})

    def files(self, files):
        folder = Path(self.home) / 'seed'
        folder.mkdir(exist_ok=True)
        for name, content in files:
            target = folder / name
            target.write_bytes(content)
            self.command('--json', 'send', '--file', str(target), allow_send_failure=True)

    def mobile(self):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        credentials = json.loads(self.command('--json', 'mobile', 'setup', '--non-interactive',
            '--label', 'e2e-phone' if self.mode == 'search' else 'kit-image', '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)).stdout)
        for _ in range(100):
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=1):
                    break
            except OSError:
                time.sleep(.1)
        auth = base64.b64encode(f"{credentials['username']}:{credentials['password']}".encode()).decode()
        def put(route, data, content_type='application/json'):
            req = urllib.request.Request(f'http://127.0.0.1:{port}' + route, data=data, method='PUT',
                headers={'Authorization': 'Basic ' + auth, 'Content-Type': content_type})
            with self.opener.open(req, timeout=30) as response:
                assert response.status < 300
        return put

    def seed(self, mode):
        self.seeded = 0
        if mode in ('history', 'devices'):
            extra = 130 if mode == 'history' else 0
            for index in range(extra):
                self.send(f'Paging entry {index:03}')
            pixel = bytes.fromhex('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082')
            self.files([('invoice-0921.txt', b'Invoice 0921\nTotal: 42.00\n'), ('pixel.png', pixel)])
            put = self.mobile()
            png = subprocess.check_output(['node', '--input-type=module', '-e',
                "import { gradientPng } from './apps/gui-go/frontend/e2e/history-seed-data.mjs'; process.stdout.write(gradientPng())"], cwd=ROOT)
            put('/file/gradient-sample.png', png, 'image/png')
            put('/SyncClipboard.json', json.dumps({'type': 'Image', 'text': 'gradient-sample.png',
                'dataName': 'gradient-sample.png', 'hasData': True}).encode())
            texts = json.loads(subprocess.check_output(['node', '--input-type=module', '-e',
                "import { HISTORY_TEXTS } from './apps/gui-go/frontend/e2e/history-seed-data.mjs'; process.stdout.write(JSON.stringify(HISTORY_TEXTS))"], cwd=ROOT))
            for text in texts:
                self.send(text)
                time.sleep(.2)
            self.seeded = extra + 8
        elif mode == 'search':
            self.files([('design-notes.md', b'# Design notes\n'), ('todo.txt', b'todo\n')])
            put = self.mobile()
            for text in ['release notes for version 1.2', 'meeting agenda: roadmap review',
                         'grocery list: milk, eggs, coffee']:
                put('/SyncClipboard.json', json.dumps({'type': 'Text', 'text': text}).encode())
            self.request('/search/rebuild', {})
            self.seeded = 5
        elif mode == 'panel':
            self.send('standalone-panel-synthetic-history')
            self.seeded = 1
        elif mode == 'smoke':
            self.send('TesterArmy isolated history acceptance')
            self.seeded = 1
        elif mode != 'empty':
            raise ValueError('unknown isolated seed mode')
        if self.seeded:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                entries = self.request('/clipboard/entries?limit=200')
                if len(entries) == self.seeded:
                    break
                time.sleep(.2)
            else:
                raise RuntimeError('real ingress did not produce the exact expected seed count')
        if mode == 'panel':
            self.request('/content-lock/revoke', {})

    def ready(self):
        return {'baseUrl': self.base, 'daemonUrl': self.base, 'sessionToken': self.session,
                'fileToken': self.connection['token'], 'passphrase': self.passphrase,
                'profile': self.profile, 'seeded': self.seeded,
                'text': 'TesterArmy isolated history acceptance'}

    def close(self):
        remaining = list(Path(self.home).rglob('daemon.conn'))
        pid = None
        if remaining:
            assert len(remaining) == 1, 'ambiguous isolated daemon ownership'
            pid = json.loads(remaining[0].read_text(encoding='utf-8'))['pid']
            self.command('--json', 'stop', timeout=60)
            for _ in range(300):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(.1)
            else:
                raise RuntimeError('owned daemon did not exit; preserve its private sandbox')
        elif self.start_attempted:
            if self.connection is None:
                raise RuntimeError('startup ownership unresolved; preserve the private sandbox')
            try:
                os.kill(self.connection['pid'], 0)
            except ProcessLookupError:
                pass
            else:
                raise RuntimeError('owned daemon remains without its connection file; preserve the private sandbox')
        shutil.rmtree(self.home)
        return {'phase': 'cleanup', 'profile': self.profile, 'daemonPid': pid,
                'homeRemoved': not Path(self.home).exists(), 'daemonExited': True,
                'systemClipboardDisabled': self.env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1',
                'guiIsolated': self.env['UC_GUI_GO_ISOLATED'] == '1'}


def serve(mode):
    backend = Backend(mode)
    try:
        backend.start()
        backend.seed(mode)
        print(json.dumps(backend.ready()), flush=True)
        sys.stdin.buffer.read()
    finally:
        print(json.dumps(backend.close()), flush=True)


if __name__ == '__main__':
    serve(sys.argv[1] if len(sys.argv) > 1 else 'empty')
