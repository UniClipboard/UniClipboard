"""Own two real receivers and a reversible receiver-storage failure.

Failure modes: peer states merge, a receipt hides a failed file, retries target
the completed peer, confirmation is bypassed, or owned daemons leak.
Only fixture files under private HOME directories may be moved.
"""
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import time
from urllib.parse import unquote, urlparse
from urllib.request import Request


class DeliveryPeers:
    def __init__(self, source):
        self.source = source
        self.receivers = []
        self.file = Path(source.home) / 'delivery-real-file.bin'
        self.blocked = None
        self.held = None

    def prepare(self):
        source = self.source
        cli = Path(__file__).resolve().parents[5] / 'target/gui-go/uniclip'
        for index in range(2):
            receiver = type(source)('empty')
            self.receivers.append(receiver)
            receiver.passphrase = source.passphrase
            receiver.spawn()
            invite = subprocess.Popen([str(cli), 'space', 'invite'], env=source.env,
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
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
                assert code, 'fixture invitation readiness'
                receiver.command('space', 'join', '--code', code, '--passphrase', source.passphrase,
                                 '--device-name', f'delivery-receiver-{index + 1}', timeout=120)
            finally:
                if invite.poll() is None:
                    invite.send_signal(signal.SIGINT)
                invite.wait(timeout=15)
            receiver.command('start')
            receiver.connect()
        # Closing the CLI does not dismiss the source's retained invitation screen.
        if source.request('/v2/setup/state').get('currentInvitation'):
            request = Request(source.base + '/v2/setup/cancel', method='POST',
                              headers={'Authorization': 'Session ' + source.session})
            with source.opener.open(request, timeout=30) as response:
                assert response.status == 204
        peers = source.request('/paired-devices')
        self.peers = [next(peer['peerId'] for peer in peers
                           if peer['deviceName'] == f'delivery-receiver-{index + 1}')
                      for index in range(2)]
        self.file.write_bytes(os.urandom(64 * 1024 * 1024))
        result = source.request('/clipboard/dispatch-file',
                                {'sourcePath': str(self.file), 'peers': [self.peers[0]]})
        self.entry = result['entryId']
        self.wait_state(self.peers[0], {'completed'})
        caches = list(Path(self.receivers[1].home).rglob('file-cache'))
        assert len(caches) == 1, 'one fixture-owned managed file cache'
        self.blocked = caches[0]
        self.held = self.blocked.with_name('file-cache-held')
        self.blocked.rename(self.held)
        self.blocked.write_bytes(b'fixture blocks directory creation')
        source.request('/clipboard/resend', {'entryId': self.entry,
                       'peers': [self.peers[1]], 'confirmNewTransfer': True})
        self.wait_state(self.peers[1], {'failed', 'partially_failed'})
        history = source.request('/search/query?query=&offset=0&limit=50')
        assert any(item['entryId'] == self.entry for item in history['items'])
        return {'entryId': self.entry, 'peerIds': self.peers}

    def wait_state(self, peer, states):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            snapshot = self.source.request(f'/clipboard/entries/{self.entry}/outbound/{peer}')
            if snapshot and snapshot['state'] in states:
                return snapshot
            time.sleep(.1)
        raise RuntimeError('real outbound state did not settle')

    def restore(self):
        if self.blocked is not None:
            self.blocked.unlink()
            self.held.rename(self.blocked)
            self.blocked = None

    def verify_files(self):
        expected = hashlib.sha256(self.file.read_bytes()).hexdigest()
        for receiver in self.receivers:
            entries = receiver.request('/clipboard/entries?limit=10')
            assert len(entries) == 1
            detail = receiver.request(f"/clipboard/entries/{entries[0]['id']}")
            paths = [Path(unquote(urlparse(uri).path)) for uri in detail['content'].splitlines()
                     if uri.startswith('file://')]
            assert len(paths) == 1
            assert paths[0].resolve().is_relative_to(Path(receiver.home).resolve())
            assert hashlib.sha256(paths[0].read_bytes()).hexdigest() == expected
        return {'filesVerified': len(self.receivers), 'sha256': expected}

    def close(self):
        errors = []
        receipts = []
        try:
            self.restore()
        except Exception as error:
            errors.append(type(error).__name__)
        for receiver in reversed(self.receivers):
            try:
                receipts.append(receiver.close())
            except Exception as error:
                errors.append(type(error).__name__)
        assert not errors, 'owned receiver cleanup failed'
        assert all(receipt['homeRemoved'] and receipt['daemonExitCode'] == 0 and
                   not receipt['forced'] for receipt in receipts)
        return receipts
