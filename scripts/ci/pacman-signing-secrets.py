#!/usr/bin/env python3
"""Bootstrap or consume the explicitly authorized pacman signing secret."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

SPEC = importlib.util.spec_from_file_location('assemble', Path(__file__).with_name('assemble-pacman-repo.py'))
ASSEMBLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSEMBLE)
SECRET = 'PACMAN_GPG_PRIVATE_KEY'


def command(*args, data=None):
    result = subprocess.run(args, input=data, capture_output=True)
    if result.returncode:
        # Never include captured output from operations that handle private material.
        raise RuntimeError(f'{args[0]} operation failed (exit {result.returncode})')
    return result.stdout


def require_memory_home():
    home = Path(os.environ['GNUPGHOME'])
    filesystem = command('stat', '-f', '-c', '%T', str(home)).decode().strip()
    if filesystem != 'ramfs':
        if filesystem != 'tmpfs' or len(Path('/proc/swaps').read_text().splitlines()) > 1:
            raise ValueError('GPG home must be ramfs, or tmpfs on a host with no active swap')
    if home.stat().st_mode & 0o077:
        raise ValueError('GPG home must be private (mode 0700)')
    return home


def bootstrap(repository, public_directory):
    home = require_memory_home()
    names = json.loads(command('gh', 'secret', 'list', '--repo', repository,
                              '--env', 'pacman-production', '--json', 'name'))
    if any(item['name'] == SECRET for item in names):
        raise ValueError('signing secret already exists; key rotation must be explicit')
    command('gpg', '--batch', '--pinentry-mode', 'loopback', '--passphrase', '',
            '--quick-generate-key', 'UniClipboard pacman repository signing', 'ed25519', 'sign', '2y')
    records = command('gpg', '--batch', '--with-colons', '--list-secret-keys').decode()
    fingerprints = [line.split(':')[9] for line in records.splitlines() if line.startswith('fpr:')]
    if len(fingerprints) != 1:
        raise ValueError('expected one freshly generated signing key')
    fingerprint = fingerprints[0]
    public = command('gpg', '--batch', '--armor', '--export', fingerprint)
    private = command('gpg', '--batch', '--armor', '--export-secret-keys', fingerprint)
    public_directory.mkdir(parents=True, exist_ok=True)
    (public_directory / 'uniclipboard.asc').write_bytes(public)
    (public_directory / 'signing-fingerprint.txt').write_text(fingerprint + '\n')
    # Pass secret bytes through stdin directly to GitHub's encrypted secret storage.
    command('gh', 'secret', 'set', SECRET, '--repo', repository, '--env', 'pacman-production', data=private)
    del private
    revocation = home / 'openpgp-revocs.d' / (fingerprint + '.rev')
    command('gh', 'secret', 'set', 'PACMAN_GPG_REVOCATION_CERT', '--repo', repository,
            '--env', 'pacman-production', data=revocation.read_bytes())
    command('gh', 'variable', 'set', 'PACMAN_SIGNING_FINGERPRINT', '--repo', repository, '--body', fingerprint)
    print(f'Configured pacman-production signing identity: {fingerprint}')


def sign(package, output, architecture, fingerprint):
    require_memory_home()
    if not re.fullmatch(r'[A-F0-9]{40}|[A-F0-9]{64}', fingerprint):
        raise ValueError('invalid fingerprint')
    # The caller pipes the secret; it never enters container configuration or argv.
    private = sys.stdin.buffer.read(65537)
    if not private or len(private) > 65536:
        raise ValueError('expected a private signing key on stdin (at most 64 KiB)')
    command('gpg', '--batch', '--import', data=private)
    del private
    ASSEMBLE.assemble(package, output, architecture, fingerprint)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest='operation', required=True)
    setup = subparsers.add_parser('bootstrap')
    setup.add_argument('--repository', default='UniClipboard/UniClipboard')
    setup.add_argument('--public-directory', type=Path, required=True)
    signing = subparsers.add_parser('sign')
    signing.add_argument('package', type=Path)
    signing.add_argument('output', type=Path)
    signing.add_argument('--arch', choices=['x86_64', 'aarch64'], required=True)
    signing.add_argument('--fingerprint', required=True)
    args = parser.parse_args()
    if args.operation == 'bootstrap':
        bootstrap(args.repository, args.public_directory)
    else:
        sign(args.package, args.output, args.arch, args.fingerprint)
