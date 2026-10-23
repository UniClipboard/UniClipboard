#!/usr/bin/env python3
"""Keep shared libraries supplied by the GNOME content snap out of the app snap."""
import argparse
from pathlib import Path
import re
import subprocess


def remove_platform_libraries(prime: Path, platform: Path):
    if not platform.is_dir():
        raise ValueError(f'GNOME content snap is not mounted: {platform}')
    removed = []
    for path in sorted(prime.rglob('*')):
        if '.so' not in path.name or not (path.is_file() or path.is_symlink()):
            continue
        relative = path.relative_to(prime)
        supplied = platform / relative
        owned = supplied.is_file() or supplied.is_symlink()
        is_elf = False
        if not owned and path.is_file():
            with path.open('rb') as stream:
                is_elf = stream.read(4) == b'\x7fELF'
        if is_elf:
            dynamic = subprocess.run(['readelf', '-d', str(path)], check=True,
                                     capture_output=True, text=True).stdout
            soname = re.search(r'\(SONAME\).*\[([^\]]+)\]', dynamic)
            if soname:
                supplied = platform / relative.parent / soname.group(1)
                owned = supplied.is_file() or supplied.is_symlink()
        if owned:
            path.unlink()
            removed.append(str(relative))
    return removed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prime', type=Path, required=True)
    parser.add_argument('--platform', type=Path, required=True)
    args = parser.parse_args()
    for relative in remove_platform_libraries(args.prime, args.platform):
        print(f'Platform-owned library removed: {relative}')


if __name__ == '__main__':
    main()
