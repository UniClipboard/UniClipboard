#!/usr/bin/env python3
"""Reject snaps whose desktop entries point at a missing or malformed icon.

The Snap Store rejects uploads with "nonexistent icon path" when a packed
meta/gui/*.desktop names an ${SNAP} icon the snap does not contain. Check the
packed artifact itself so the failure surfaces before publishing.
"""

import configparser
import pathlib
import re
import struct
import subprocess
import sys
import tempfile

SNAP_PREFIX = "${SNAP}/"


def validate_format(path, relative):
    data = path.read_bytes()
    suffix = path.suffix.lower()
    if suffix == ".png":
        if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
            raise RuntimeError(f"{relative}: not a valid PNG")
        width, height = struct.unpack(">II", data[16:24])
        size = re.search(r"/(\d+)x(\d+)/", relative)
        if size and (width, height) != (int(size[1]), int(size[2])):
            raise RuntimeError(
                f"{relative}: image is {width}x{height}, directory declares {size[1]}x{size[2]}"
            )
    elif suffix == ".svg":
        if b"<svg" not in data[:4096]:
            raise RuntimeError(f"{relative}: not a valid SVG")
    elif suffix == ".xpm":
        if not data.startswith(b"/* XPM */"):
            raise RuntimeError(f"{relative}: not a valid XPM")
    else:
        raise RuntimeError(f"{relative}: unsupported icon format")


def check_tree(root):
    root = pathlib.Path(root).resolve()
    entries = sorted((root / "meta/gui").glob("*.desktop"))
    if not entries:
        raise RuntimeError(f"No desktop entries found under {root / 'meta/gui'}")
    for entry in entries:
        parser = configparser.ConfigParser(interpolation=None)
        parser.optionxform = str
        parser.read(entry, encoding="utf-8")
        for section in parser.sections():
            icon = parser[section].get("Icon")
            if icon is None:
                continue
            if not icon.startswith(SNAP_PREFIX):
                raise RuntimeError(f"{entry.name} [{section}]: Icon={icon} is not a ${{SNAP}} path")
            relative = icon[len(SNAP_PREFIX):]
            path = (root / relative).resolve()
            if not path.is_relative_to(root):
                raise RuntimeError(f"{entry.name} [{section}]: Icon={icon} escapes the snap")
            if not path.is_file():
                raise RuntimeError(f"{entry.name} [{section}]: Icon={icon} does not exist in the snap")
            validate_format(path, relative)
            print(f"Verified desktop icon: {entry.name} [{section}] -> {relative}")


def check(snap):
    with tempfile.TemporaryDirectory(prefix="uniclipboard-snap-") as directory:
        root = pathlib.Path(directory) / "root"
        subprocess.run(
            ["unsquashfs", "-no-progress", "-d", str(root), str(snap)],
            check=True, stdout=subprocess.DEVNULL,
        )
        check_tree(root)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: check-snap-desktop-icons.py SNAP...")
    for snap in sys.argv[1:]:
        check(snap)
