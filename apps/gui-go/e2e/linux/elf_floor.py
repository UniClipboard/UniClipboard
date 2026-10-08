#!/usr/bin/env python3
"""Measure the host-library floor a Linux package imposes: the highest GLIBC / GLIBCXX / CXXABI / GCC symbol version
that any ELF file in a tree needs from the host.

  elf_floor.py <tree> [--max-glibc 2.36] [--json out.json]

<tree> is an extracted deb (`dpkg-deb -x`) or an extracted AppImage (`unsquashfs`, or `--appimage-extract`). Only the
versions the files REQUIRE (readelf -V "Version needs") are counted, not what they define, so a bundled libc-independent
library such as libgtk adds nothing, but a bundled library built against a newer glibc raises the floor of the whole
package. With --max-glibc the command fails when any file needs a newer GLIBC than the stated floor and names the files.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

FAMILIES = ('GLIBC', 'GLIBCXX', 'CXXABI', 'GCC')
NEED = re.compile(r'Name: ((?:GLIBC|GLIBCXX|CXXABI|GCC)_[0-9][0-9.]*)')


def version_key(text):
    return tuple(int(p) for p in text.split('.') if p)


def is_elf(path):
    try:
        with open(path, 'rb') as f:
            return f.read(4) == b'\x7fELF'
    except OSError:
        return False


def needs(path):
    out = subprocess.run(['readelf', '-V', '-W', str(path)], capture_output=True, text=True)
    found = {}
    for name in NEED.findall(out.stdout):
        family, version = name.split('_', 1)
        if family not in found or version_key(version) > version_key(found[family]):
            found[family] = version
    return found


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('tree', type=Path)
    parser.add_argument('--max-glibc', help='fail when any ELF file needs a newer GLIBC symbol version')
    parser.add_argument('--json', type=Path, help='write the full per-file result here')
    args = parser.parse_args()
    files = {}
    for path in sorted(args.tree.rglob('*')):
        if path.is_file() and not path.is_symlink() and is_elf(path):
            found = needs(path)
            if found:
                files[str(path.relative_to(args.tree))] = found
    floor = {}
    for family in FAMILIES:
        versions = [v[family] for v in files.values() if family in v]
        if versions:
            floor[family] = max(versions, key=version_key)
    top = {}
    for family in FAMILIES:
        if family in floor:
            top[family] = sorted(f for f, v in files.items() if v.get(family) == floor[family])[:8]
    result = {'tree': str(args.tree), 'elfFilesWithNeeds': len(files), 'floor': floor, 'filesAtFloor': top, 'files': files}
    if args.json:
        args.json.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: result[k] for k in ('tree', 'elfFilesWithNeeds', 'floor', 'filesAtFloor')}, indent=2))
    if args.max_glibc:
        over = sorted(f for f, v in files.items() if 'GLIBC' in v and version_key(v['GLIBC']) > version_key(args.max_glibc))
        if over:
            print(f'FAIL: {len(over)} file(s) need a GLIBC newer than {args.max_glibc}: {over[:20]}', file=sys.stderr)
            sys.exit(1)


if __name__ == '__main__':
    main()
