"""Real paired Engine traffic for the SDK-owned native receiver window.

Failure modes: separate single-file entries, incomplete bytes, hash mismatch,
missing item snapshots, premature completion, or unowned/forced cleanup.
No GUI observation or assertion lives in this fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time
from urllib.parse import urlparse, unquote

ROOT = Path(__file__).resolve().parents[4]


class FileTransfer:
    def __init__(self, backend_type, output):
        self.backend_type = backend_type
        self.output = output
        self.source = None
        self.receiver = None
        self.thread = None
        self.send = None
        self.entry = None
        self.expected = []
        self.result = {'passed': False, 'files': 3, 'mib': [64, 128, 192]}

    def prepare(self):
        self.source = self.backend_type('empty')
        self.source.start()
        fixtures = []
        for index, mib in enumerate(self.result['mib']):
            file = Path(self.source.home) / f'native-transfer-{index + 1}.bin'
            digest = hashlib.sha256()
            with file.open('wb') as stream:
                for _ in range(mib):
                    block = os.urandom(1024 * 1024)
                    stream.write(block)
                    digest.update(block)
            fixtures.append(file)
            self.expected.append(digest.hexdigest())
        # The existing diagnostic captures one entry while its owned daemon is stopped.
        self.source.command('--json', 'stop', timeout=60)
        assert self.source.process.wait(timeout=30) == 0
        self.source.reaper.join(timeout=5)
        self.source.process = self.source.reaper = None
        capture = subprocess.run(
            [str(ROOT / 'target/debug/uc-dev-cli'), '--dev', '--json', 'dev', 'capture-files'] +
            [part for file in fixtures for part in ('--path', str(file))],
            env=self.source.env, capture_output=True, text=True, timeout=120)
        assert capture.returncode == 0, 'single-entry capture failed'
        self.entry = json.loads(capture.stdout)['entryId']
        self.source.spawn()
        self.source.command('start')
        self.source.connect()
        self.receiver = self.backend_type('empty')
        self.receiver.passphrase = self.source.passphrase
        self.receiver.spawn()
        invite = subprocess.Popen([str(ROOT / 'target/gui-go/uniclip'), 'space', 'invite'],
            env=self.source.env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            os.set_blocking(invite.stdout.fileno(), False)
            text = ''
            code = None
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline and code is None:
                try:
                    text += os.read(invite.stdout.fileno(), 4096).decode()
                except BlockingIOError:
                    pass
                code = next((line.split('=', 1)[1].strip() for line in text.splitlines()
                             if line.startswith('INVITATION_CODE=')), None)
                time.sleep(.1)
            assert code, 'invitation was not issued'
            self.receiver.command('space', 'join', '--code', code,
                '--passphrase', self.source.passphrase, '--device-name', 'native-receiver', timeout=120)
        finally:
            if invite.poll() is None:
                invite.send_signal(signal.SIGINT)
            invite.wait(timeout=15)
        self.receiver.command('start')
        self.receiver.connect()
        assert self.source.home != self.receiver.home
        assert self.source.env['UC_DISABLE_SYSTEM_CLIPBOARD'] == self.receiver.env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1'
        return self.receiver

    def start(self):
        assert self.thread is None
        self.thread = threading.Thread(target=self._transfer, daemon=True)
        self.thread.start()

    def _transfer(self):
        samples = []
        stage = 'dispatch'
        started = time.monotonic()
        try:
            self.send = subprocess.Popen([str(ROOT / 'target/gui-go/uniclip'), '--json', 'send',
                '--resend', self.entry, '--confirm-new-transfer'], env=self.source.env,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            receive_entry = None
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                stage = 'receive-list'
                active = self.receiver.request('/clipboard/receives')
                if active:
                    receive_entry = active[0]['entryId']
                stage = 'receive-query'
                snapshot = self.receiver.request(f'/clipboard/entries/{receive_entry}/receive') if receive_entry else None
                if snapshot and (not samples or samples[-1] != snapshot):
                    samples.append(snapshot)
                stage = 'outbound-query'
                outbound = [item for item in self.source.request('/clipboard/outbound') if item['entryId'] == self.entry]
                self.result['senderStates'] = [{'state': item['state'], 'freshness': item['freshness'],
                    'itemsTotal': item['itemsTotal'], 'itemsCompleted': item['itemsCompleted']} for item in outbound]
                if snapshot and snapshot['state'] == 'completed' and outbound and all(item['state'] == 'completed' for item in outbound):
                    break
                time.sleep(.05)
            assert snapshot and snapshot['state'] == 'completed'
            assert snapshot['itemsCompleted'] == 3
            assert self.send.wait(timeout=30) == 0
            detail = self.receiver.request(f'/clipboard/entries/{receive_entry}')
            paths = [Path(unquote(urlparse(uri).path)) for uri in detail['content'].splitlines() if uri.startswith('file://')]
            assert len(paths) == 3
            assert all(file.resolve().is_relative_to(Path(self.receiver.home).resolve()) for file in paths)
            hashes = [hashlib.sha256(file.read_bytes()).hexdigest() for file in paths]
            assert sorted(hashes) == sorted(self.expected)
            for item in snapshot['items']:
                assert item['content']['kind'] == 'file'
                assert isinstance(item['transfer']['receivedBytes'], str)
            self.result.update(passed=True, sha256=hashes, receiveSnapshots=samples,
                senderStates=[{'state': item['state'], 'freshness': item['freshness'],
                    'itemsTotal': item['itemsTotal'], 'itemsCompleted': item['itemsCompleted']} for item in outbound])
        except Exception as error:
            self.result.update(errorType=type(error).__name__, errorStage=stage,
                reasonType=type(getattr(error, 'reason', None)).__name__,
                elapsedMs=round((time.monotonic() - started) * 1000))
        finally:
            self.result['receiveSnapshots'] = samples
            (self.output / 'native-transfer.json').write_text(json.dumps(self.result, indent=2) + '\n')

    def finish(self):
        forced = False
        if self.thread:
            self.thread.join(timeout=150)
            if self.thread.is_alive():
                forced = True
                if self.send and self.send.poll() is None:
                    self.send.terminate()
                    self.send.wait(timeout=15)
        sender = self.source.close() if self.source else {}
        return {'transferPassed': self.result['passed'], 'senderCleanup': sender,
                'senderCleanupPassed': sender.get('daemonExitCode') == 0 and
                    sender.get('homeRemoved') is True and not sender.get('forced'),
                'transferForced': forced}
