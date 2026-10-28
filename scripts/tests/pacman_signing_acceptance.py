#!/usr/bin/env python3
"""Exercise secret bootstrap and stdin signing with test keys held only in RAM."""
import importlib.util
import io
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('secrets_signer', ROOT / 'scripts/ci/pacman-signing-secrets.py')
SIGNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SIGNER)


def main():
    memory = SIGNER.require_memory_home()
    actual = SIGNER.command
    secrets = {}
    def command(*args, data=None):
        if args[0] != 'gh':
            return actual(*args, data=data)
        if args[1:3] == ('secret', 'list'):
            return b'[]'
        if args[1:3] == ('secret', 'set'):
            assert data
            secrets[args[3]] = data
        return b''
    with tempfile.TemporaryDirectory(prefix='uc-signing-test-') as tmp:
        base = Path(tmp)
        with patch.object(SIGNER, 'command', command):
            SIGNER.bootstrap('test/repository', base / 'public')
        assert 'PACMAN_GPG_PRIVATE_KEY' in secrets and 'PACMAN_GPG_REVOCATION_CERT' in secrets
        fingerprint = (base / 'public/signing-fingerprint.txt').read_text().strip()
        imported = memory / 'imported'
        imported.mkdir(mode=0o700)
        fixture = runpy.run_path(str(ROOT / 'scripts/tests/pacman_repo_acceptance.py'))['fixture']
        package = fixture(base, '1.0.0-1')
        with patch.dict(os.environ, GNUPGHOME=str(imported)):
            try:
                stdin = io.TextIOWrapper(io.BytesIO(secrets.pop('PACMAN_GPG_PRIVATE_KEY')))
                with patch('sys.stdin', stdin):
                    SIGNER.sign(package, base / 'signed', 'x86_64', fingerprint)
                SIGNER.ASSEMBLE.verify(base / 'signed/x86_64/uniclipboard.db', fingerprint)
                assert not list((base / 'signed').rglob('*.key'))
            finally:
                subprocess.run(['gpgconf', '--kill', 'all'], check=True)
        # A disk-backed home must be rejected before any key operation.
        with patch.dict(os.environ, GNUPGHOME=str(base)):
            try:
                SIGNER.require_memory_home()
            except ValueError:
                pass
            else:
                raise AssertionError('disk-backed GPG home accepted')
    print('PASS: RAM-only bootstrap, secret stdin transport, signed snapshot, disk-home rejection')


if __name__ == '__main__':
    main()
