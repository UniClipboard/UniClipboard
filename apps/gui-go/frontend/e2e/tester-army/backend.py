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
import threading
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env


def diagnostic(event, stage, error=None, **fields):
    # Never serialize exception messages, command arguments, CLI output or HTTP bodies.
    value = {'event': event, 'stage': stage, **fields}
    if error is not None:
        value['errorType'] = type(error).__name__
        if isinstance(error, OSError):
            value['errno'] = error.errno
    print(json.dumps(value), file=sys.stderr, flush=True)


class Backend:
    def __init__(self, mode):
        if sys.platform != 'darwin' or Path.cwd() != ROOT:
            raise RuntimeError('backend acceptance requires repository-root macOS HOME isolation')
        self.stage = 'profile'
        self.mode = mode
        self.home = tempfile.mkdtemp(prefix='uc-gui-go-tester-')
        self.profile = 'gui-go-' + Path(self.home).name
        self.env = isolated_env(self.home, self.profile, {
            'PATH': str(ROOT / 'target/debug') + os.pathsep + os.environ['PATH'],
            'UC_GUI_GO_ISOLATED': '1',
        })
        # Hex preserves 192 random bits and satisfies the CLI's hyphen-value restriction.
        self.passphrase = secrets.token_hex(24)
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.connection = None
        self.process = None
        self.reaper = None

    def command(self, *args, allow_send_failure=False, timeout=90):
        assert self.process is not None, 'start the fixture-owned daemon before CLI commands'
        operation = args[0] if args[0] != '--json' else args[1]
        diagnostic('command-start', self.stage, operation=operation)
        try:
            result = subprocess.run([str(ROOT / 'target/gui-go/uniclip'), *args], env=self.env,
                                    capture_output=True, text=True, encoding='utf-8', timeout=timeout)
        except Exception as error:
            diagnostic('command-failed', self.stage, error, operation=operation)
            raise
        diagnostic('command-exit', self.stage, operation=operation, returnCode=result.returncode)
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

    def spawn(self):
        assert self.process is None, 'the fixture owns exactly one daemon child'
        self.stage = 'owned-daemon-startup'
        self.process = subprocess.Popen([str(ROOT / 'target/debug/uniclipd')],
            env={**self.env, 'UC_DAEMON_RUN_MODE': '', 'UC_DAEMON_NO_TAKEOVER': '1'},
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        # CLI stop waits for PID disappearance, so reap concurrently while that command runs.
        self.reaper = threading.Thread(target=self.process.wait, daemon=True)
        self.reaper.start()
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError('owned daemon exited before publishing its connection')
            connections = list(Path(self.home).rglob('daemon.conn'))
            if connections:
                assert len(connections) == 1, 'expected one owned daemon connection'
                connection = json.loads(connections[0].read_text(encoding='utf-8'))
                assert connection['pid'] == self.process.pid, 'daemon connection must belong to the owned child'
                self.connection = connection
                return
            time.sleep(.1)
        raise RuntimeError('owned daemon connection readiness timed out')

    def start(self):
        self.spawn()
        self.stage = 'space-init'
        self.command('space', 'init', '--passphrase', self.passphrase, '--device-name', 'search-node' if self.mode == 'search' else 'kit-browser')
        self.stage = 'daemon-start'
        self.command('start')
        self.connect()

    def connect(self):
        self.stage = 'connection'
        connections = list(Path(self.home).rglob('daemon.conn'))
        assert len(connections) == 1, 'expected one isolated daemon connection'
        self.connection = json.loads(connections[0].read_text(encoding='utf-8'))
        assert self.connection['pid'] == self.process.pid, 'CLI must reuse the fixture-owned standalone daemon'
        self.base = f"http://127.0.0.1:{self.connection['port']}"
        self.stage = 'authentication'
        self.session = self.request('/auth/connect', {'pid': os.getpid(), 'clientType': 'gui'},
                                    authorization='Bearer ' + self.connection['token'])['sessionToken']
        self.residency = self.request('/health')['residency']
        assert self.residency == 'standalone', 'the fixture requires a persistent daemon'
        self.stage = 'unlock'
        self.request('/content-lock/unlock', {'passphrase': self.passphrase})
        self.stage = 'settings'
        self.request('/settings', {'general': {'language': 'zh-CN' if self.mode == 'smoke' else 'en-US'}}, method='PUT')

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
        self.stage = 'seed-' + mode
        self.seeded = 0
        if mode in ('history', 'devices'):
            extra = 130 if mode == 'history' else 0
            for index in range(extra):
                self.send(f'Paging entry {index:03}')
            pixel = bytes.fromhex('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082')
            self.files([('invoice-0921.txt', b'Invoice 0921\nTotal: 42.00\n'), ('pixel.png', pixel)])
            put = self.mobile()
            png = subprocess.check_output(['node', '--input-type=module', '-e',
                "import { gradientPng } from './apps/gui-go/frontend/e2e/history-seed-data.mjs'; process.stdout.write(gradientPng())"], cwd=ROOT, timeout=60)
            put('/file/gradient-sample.png', png, 'image/png')
            put('/SyncClipboard.json', json.dumps({'type': 'Image', 'text': 'gradient-sample.png',
                'dataName': 'gradient-sample.png', 'hasData': True}).encode())
            texts = json.loads(subprocess.check_output(['node', '--input-type=module', '-e',
                "import { HISTORY_TEXTS } from './apps/gui-go/frontend/e2e/history-seed-data.mjs'; process.stdout.write(JSON.stringify(HISTORY_TEXTS))"], cwd=ROOT, timeout=60))
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
        return {'daemonPid': self.process.pid, 'residency': self.residency,
                'baseUrl': self.base, 'daemonUrl': self.base, 'sessionToken': self.session,
                'fileToken': self.connection['token'], 'passphrase': self.passphrase,
                'profile': self.profile, 'seeded': self.seeded,
                'text': 'TesterArmy isolated history acceptance'}

    def close(self):
        self.stage = 'cleanup'
        forced = False
        stop_succeeded = True
        code = None
        if self.process is not None:
            if self.process.poll() is None:
                remaining = list(Path(self.home).rglob('daemon.conn'))
                if remaining:
                    assert len(remaining) == 1, 'ambiguous isolated daemon ownership'
                    connection = json.loads(remaining[0].read_text(encoding='utf-8'))
                    assert connection['pid'] == self.process.pid, 'refuse to stop an unexpected daemon'
                    try:
                        self.command('--json', 'stop', timeout=60)
                    except Exception as error:
                        stop_succeeded = False
                        diagnostic('cleanup-command-failed', self.stage, error)
                else:
                    # A failed pre-readiness child is still owned by its Popen handle.
                    self.process.terminate()
                    forced = True
            try:
                code = self.process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.process.kill()
                forced = True
                code = self.process.wait(timeout=10)
        if self.reaper is not None:
            self.reaper.join(timeout=1)
        shutil.rmtree(self.home)
        return {'phase': 'cleanup', 'profile': self.profile,
                'daemonPid': self.process.pid if self.process else None,
                'daemonExitCode': code, 'forced': forced, 'stopSucceeded': stop_succeeded,
                'homeRemoved': not Path(self.home).exists(), 'daemonExited': True,
                'systemClipboardDisabled': self.env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1',
                'guiIsolated': self.env['UC_GUI_GO_ISOLATED'] == '1'}


def serve(mode):
    backend = None
    failed = False
    try:
        backend = Backend(mode)
        diagnostic('stage', backend.stage)
        backend.start()
        backend.seed(mode)
        backend.stage = 'ready'
        diagnostic('stage', backend.stage)
        print(json.dumps(backend.ready()), flush=True)
        sys.stdin.buffer.read()
    except Exception as error:
        failed = True
        diagnostic('failure', backend.stage if backend else 'profile', error)
    finally:
        if backend is not None:
            try:
                receipt = backend.close()
                if receipt['forced'] or not receipt['stopSucceeded'] or receipt['daemonExitCode'] not in (None, 0):
                    failed = True
            except Exception as error:
                failed = True
                diagnostic('cleanup-failed', backend.stage, error)
                receipt = {'phase': 'cleanup', 'profile': backend.profile,
                           'homeRemoved': not Path(backend.home).exists(),
                           'daemonExited': None, 'sandboxPreserved': Path(backend.home).exists(),
                           'systemClipboardDisabled': backend.env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1',
                           'guiIsolated': backend.env['UC_GUI_GO_ISOLATED'] == '1'}
            print(json.dumps(receipt), flush=True)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(serve(sys.argv[1] if len(sys.argv) > 1 else 'empty'))
