#!/usr/bin/env python3
"""Build an immutable, signed repository snapshot using an existing GPG agent."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True)


def verify(path, fingerprint):
    status = run('gpg', '--batch', '--status-fd=1', '--verify', str(path) + '.sig', str(path))
    valid = [line.split() for line in status.splitlines() if line.startswith('[GNUPG:] VALIDSIG ')]
    if not any(row[2] == fingerprint or row[-1] == fingerprint for row in valid):
        raise ValueError('signature does not match the requested fingerprint')


def assemble(package, output, architecture, fingerprint):
    if not re.fullmatch(r'[A-F0-9]{40}|[A-F0-9]{64}', fingerprint):
        raise ValueError('provide the complete uppercase signing key fingerprint')
    metadata = run('bsdtar', '-xOf', str(package), '.PKGINFO')
    fields = {}
    for line in metadata.splitlines():
        if ' = ' in line:
            key, value = line.split(' = ', 1)
            fields.setdefault(key, []).append(value)
    if fields.get('pkgname') != ['uniclipboard'] or fields.get('arch') != [architecture]:
        raise ValueError('expected uniclipboard package for the selected architecture')
    version, = fields['pkgver']
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+-[1-9][0-9]*', version):
        raise ValueError('only stable package versions are supported')
    if output.exists():
        raise ValueError('snapshot output already exists; never overwrite a published snapshot')
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.pacman-', dir=output.parent))
    try:
        archdir = staging / architecture
        archdir.mkdir()
        target = archdir / f'uniclipboard-{version}-{architecture}.pkg.tar.zst'
        shutil.copyfile(package, target)
        subprocess.run(['gpg', '--batch', '--local-user', fingerprint, '--detach-sign', str(target)], check=True)
        verify(target, fingerprint)
        subprocess.run(['repo-add', '--sign', '--key', fingerprint,
                        'uniclipboard.db.tar.gz', target.name], cwd=archdir, check=True)
        for name in ('uniclipboard.db.tar.gz', 'uniclipboard.files.tar.gz'):
            verify(archdir / name, fingerprint)
        # Object stores do not preserve symlinks. Materialize repo-add's aliases.
        for alias in archdir.iterdir():
            if alias.is_symlink():
                content = alias.read_bytes()
                alias.unlink()
                alias.write_bytes(content)
        public = subprocess.check_output(['gpg', '--batch', '--armor', '--export', fingerprint])
        if not public:
            raise ValueError('public key export is empty')
        (staging / 'uniclipboard.asc').write_bytes(public)
        (staging / 'signing-fingerprint.txt').write_text(fingerprint + '\n')
        os.rename(staging, output)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--arch', required=True, choices=['x86_64', 'aarch64'])
    parser.add_argument('--fingerprint', required=True)
    args = parser.parse_args()
    assemble(args.package.resolve(), args.output.resolve(), args.arch, args.fingerprint)
