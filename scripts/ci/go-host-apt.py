#!/usr/bin/env python3
"""Cache only authenticated apt downloads; resolve against fresh indexes on every run."""

import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

PACKAGES = ['libgtk-4-dev', 'libwebkitgtk-6.0-dev', 'libx11-dev', 'libdbus-1-dev', 'pkg-config']


def digest(data):
    return hashlib.sha256(data).hexdigest()


def manifest(output):
    downloads = {}
    for line in output.splitlines():
        if not line.startswith("'"):
            continue
        uri, name, size, checksum = shlex.split(line)
        if Path(name).name != name or not name.endswith('.deb'):
            raise ValueError(f'Unexpected apt archive name: {name}')
        if not re.fullmatch(r'SHA256:[0-9a-fA-F]{64}', checksum):
            raise ValueError(f'Missing SHA256 for {name}')
        downloads[name] = {'uri': uri, 'size': int(size), 'sha256': checksum[7:].lower()}
    return downloads


def cache_key(downloads, os_release, architecture, sources, packages=PACKAGES):
    inputs = [downloads, os_release, architecture, sources, packages, digest(Path(__file__).read_bytes())]
    return 'go-host-apt-v1-' + digest(json.dumps(inputs, sort_keys=True).encode())


def validate(archives, downloads):
    """Discard unverified cache bytes before apt can trust an existing archive."""
    for archive in archives.glob('*.deb'):
        expected = downloads.get(archive.name)
        if (archive.is_symlink() or not archive.is_file()):
            raise ValueError(f'Unexpected cached archive type: {archive.name}')
        if (not expected or archive.stat().st_size != expected['size']
                or digest(archive.read_bytes()) != expected['sha256']):
            print(f'Discarding invalid or obsolete cached archive: {archive.name}')
            archive.unlink()


def main():
    operation = sys.argv[1]
    root = Path(os.environ['RUNNER_TEMP']) / 'go-host-apt'
    archives = root / 'archives'
    archives.mkdir(parents=True, exist_ok=True)
    sudo = [] if os.geteuid() == 0 else ['sudo']
    apt = sudo + ['apt-get', '-o', f'Dir::Cache::archives={archives}',
                  '-o', 'Acquire::ForceHash=SHA256']
    install = ['install', '-y', '--no-install-recommends', *PACKAGES]
    plan = root / 'downloads.json'
    if operation == 'prepare':
        # Never cache indexes or dpkg state, and fail closed if any source cannot refresh.
        subprocess.run(sudo + ['apt-get', 'update', '-qq', '-o', 'APT::Update::Error-Mode=any'], check=True)
        output = subprocess.check_output(apt + ['--print-uris', '--download-only', *install], text=True)
        downloads = manifest(output)
        plan.write_text(json.dumps(downloads, sort_keys=True))
        sources = {str(p): p.read_text() for p in sorted(Path('/etc/apt').rglob('*'))
                   if p.is_file() and (p.name == 'sources.list' or p.suffix in {'.list', '.sources'})}
        key = cache_key(downloads, Path('/etc/os-release').read_text(),
                        subprocess.check_output(['dpkg', '--print-architecture'], text=True), sources)
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output_file:
            output_file.write(f'key={key}\narchives={archives}\n')
    elif operation in {'download', 'install'}:
        downloads = json.loads(plan.read_text())
        validate(archives, downloads)
        # apt authenticates new downloads against its signed indexes as usual.
        subprocess.run(apt + ['--download-only', *install], check=True)
        validate(archives, downloads)
        if operation == 'install':
            subprocess.run(apt + install, check=True)
    else:
        raise ValueError(f'Unknown operation: {operation}')


if __name__ == '__main__':
    main()
