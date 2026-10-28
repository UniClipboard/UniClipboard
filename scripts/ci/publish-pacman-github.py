#!/usr/bin/env python3
"""Publish a signed snapshot to a dedicated GitHub Release, preserving old packages."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile

SPEC = importlib.util.spec_from_file_location('assemble', Path(__file__).with_name('assemble-pacman-repo.py'))
ASSEMBLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ASSEMBLE)


def run(*args):
    return subprocess.check_output([str(arg) for arg in args], text=True)


def digest(path):
    with path.open('rb') as stream:
        return 'sha256:' + hashlib.file_digest(stream, 'sha256').hexdigest()


def version(database):
    names = run('bsdtar', '-tf', database).splitlines()
    descriptions = [name for name in names if name.endswith('/desc')]
    if len(descriptions) != 1:
        raise ValueError('expected a single package in repository database')
    text = run('bsdtar', '-xOf', database, descriptions[0])
    fields = dict(re.findall(r'%([A-Z]+)%\n([^\n]+)', text))
    if fields.get('NAME') != 'uniclipboard':
        raise ValueError('unexpected repository package')
    return fields['VERSION']


def publish(snapshot, architecture, fingerprint, repository, apply):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('invalid GitHub repository')
    directory = snapshot / architecture
    ASSEMBLE.verify(directory / 'uniclipboard.db', fingerprint)
    packages = list(directory.glob('*.pkg.tar.zst'))
    if len(packages) != 1:
        raise ValueError('expected one signed package')
    ASSEMBLE.verify(packages[0], fingerprint)
    if (snapshot / 'signing-fingerprint.txt').read_text().strip() != fingerprint:
        raise ValueError('snapshot fingerprint mismatch')
    tag = f'pacman-stable-{architecture}'
    # Query the complete release list: distinguish an absent release from API/auth failures.
    pages = run('gh', 'api', '--paginate', '--slurp', f'repos/{repository}/releases?per_page=100')
    matches = [release for page in json.loads(pages) for release in page if release['tag_name'] == tag]
    release = matches[0] if matches else None
    old_assets = {}
    if release:
        if release.get('immutable') or release['draft']:
            raise ValueError('channel release must be published and mutable')
        old_assets = {asset['name']: asset for asset in release['assets']}
        if old_assets:
            with tempfile.TemporaryDirectory(prefix='uc-old-repo-') as tmp:
                # Require both files. Partial previous publication must be repaired explicitly.
                for name in ('uniclipboard.db', 'uniclipboard.db.sig'):
                    if name not in old_assets:
                        raise ValueError('existing channel database is incomplete')
                    run('gh', 'release', 'download', tag, '--repo', repository,
                        '--pattern', name, '--dir', tmp)
                old_database = Path(tmp) / 'uniclipboard.db'
                ASSEMBLE.verify(old_database, fingerprint)
                comparison = run('vercmp', version(directory / 'uniclipboard.db'), version(old_database)).strip()
                if comparison != '1':
                    raise ValueError('channel only accepts a strictly newer version')
    immutable = [*packages, *(directory.glob('*.pkg.tar.zst.sig'))]
    metadata = [snapshot / 'uniclipboard.asc', snapshot / 'signing-fingerprint.txt']
    indexes = [path for path in directory.iterdir() if path.name.startswith('uniclipboard.db') or
               path.name.startswith('uniclipboard.files')]
    # Existing package names must never change their bytes.
    for path in immutable:
        if path.name in old_assets and old_assets[path.name].get('digest') != digest(path):
            raise ValueError(f'asset collision: {path.name}')
    print(f'Channel: https://github.com/{repository}/releases/download/{tag}')
    print(f'Version: {version(directory / "uniclipboard.db")}; publish={apply}')
    if not apply:
        return
    if not release:
        run('gh', 'release', 'create', tag, '--repo', repository, '--draft', '--prerelease', '--latest=false',
            '--title', f'Pacman stable repository ({architecture})', '--notes',
            'Pacman repository channel. Retains old package assets. Not an application release.')
    for path in immutable:
        if path.name not in old_assets:
            run('gh', 'release', 'upload', tag, path, '--repo', repository)
    # Verify package upload completion before making the new database visible.
    assets = json.loads(run('gh', 'api', '--paginate', '--slurp',
                            f'repos/{repository}/releases/tags/{tag}'))[0]['assets']
    remote = {asset['name']: asset for asset in assets}
    for path in immutable:
        if remote[path.name].get('digest') != digest(path):
            raise ValueError(f'uploaded asset digest mismatch: {path.name}')
    # GitHub cannot swap two assets atomically. Required signatures make transitional
    # database/signature mismatches fail closed; users can retry synchronization.
    run('gh', 'release', 'upload', tag, *metadata, *sorted(indexes), '--repo', repository, '--clobber')
    if not release:
        run('gh', 'release', 'edit', tag, '--repo', repository, '--draft=false', '--latest=false')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('--arch', choices=['x86_64', 'aarch64'], required=True)
    parser.add_argument('--fingerprint', required=True)
    parser.add_argument('--repository', default='UniClipboard/UniClipboard')
    parser.add_argument('--publish', action='store_true', help='write release assets; default is validation only')
    args = parser.parse_args()
    publish(args.snapshot, args.arch, args.fingerprint, args.repository, args.publish)
