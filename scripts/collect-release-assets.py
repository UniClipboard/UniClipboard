#!/usr/bin/env python3
"""Collect named distributables only; reject collisions instead of renaming them.

Packaging evidence, raw executables and pre-existing updater signatures stay out.
The updater signer runs after this collector, over the final published bytes.
"""
import argparse
from pathlib import Path
import re
import shutil


def collect(source, destination):
    destination.mkdir(parents=True, exist_ok=False)
    records = []
    for file in sorted(source.rglob('*')):
        if not file.is_file() or file.is_symlink():
            continue
        name = file.name
        if name == 'UniClipboard.app.tar.gz':
            targets = [t for t in ('aarch64-apple-darwin', 'x86_64-apple-darwin') if t in str(file.parent)]
            if len(targets) != 1:
                raise ValueError(f'macOS archive requires exactly one architecture in artifact name: {file}')
            name = f'UniClipboard_{targets[0]}.app.tar.gz'
        elif not (
            re.fullmatch(r'UniClipboard_[0-9][A-Za-z0-9.+_-]*_(?:aarch64|x64)\.dmg', name)
            or re.fullmatch(r'UniClipboard_[0-9][A-Za-z0-9.+_-]*_(?:amd64|arm64)\.deb', name)
            or re.fullmatch(r'UniClipboard-[0-9][A-Za-z0-9.+_-]*\.(?:x86_64|aarch64)\.rpm', name)
            or re.fullmatch(r'UniClipboard_[0-9][A-Za-z0-9.+_-]*_(?:amd64|aarch64)\.AppImage(?:\.tar\.gz)?', name)
            or re.fullmatch(r'UniClipboard_[0-9][A-Za-z0-9.+_-]*_(?:x64|arm64)-setup\.exe', name)
            or re.fullmatch(r'UniClipboard_[0-9][A-Za-z0-9.+_-]*_(?:x64|arm64)-portable\.zip', name)
            or re.fullmatch(r'uniclipboard-cli-[0-9][A-Za-z0-9.+_-]*\.(?:tar\.gz|zip)', name)
        ):
            continue
        dest = destination / name
        if dest.exists():
            raise ValueError(f'duplicate release asset: {name}')
        shutil.copyfile(file, dest)
        records.append(name)
    if not records:
        raise ValueError('no named release assets found')
    return records


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    for name in collect(args.source, args.destination):
        print(name)
