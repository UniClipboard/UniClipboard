#!/usr/bin/env python3
"""Reject Linux release artifacts that omit the GTK3 Layer Shell runtime."""

import pathlib
import re
import subprocess
import sys
import tempfile


def output(*args):
    return subprocess.check_output(args, text=True)


def check(bundle_root):
    root = pathlib.Path(bundle_root).resolve()
    for extension in ("deb", "rpm", "AppImage"):
        packages = list(root.rglob(f"*.{extension}"))
        if not packages:
            raise RuntimeError(f"No {extension} artifact found under {root}")
        for package in packages:
            if extension == "deb":
                dependencies = output("dpkg-deb", "-f", str(package), "Depends")
                if not re.search(r"(?:^|,)\s*libgtk-layer-shell0(?:\s|,|$)", dependencies):
                    raise RuntimeError(f"{package.name}: missing libgtk-layer-shell0 dependency")
            elif extension == "rpm":
                dependencies = output("rpm", "-qp", "--requires", str(package))
                if not re.search(r"^gtk-layer-shell(?:\s|$)", dependencies, re.MULTILINE):
                    raise RuntimeError(f"{package.name}: missing gtk-layer-shell dependency")
            else:
                with tempfile.TemporaryDirectory(prefix="uniclipboard-appimage-") as directory:
                    subprocess.run(
                        [str(package), "--appimage-extract", "usr/lib/libgtk-layer-shell.so.0"],
                        cwd=directory, check=True, stdout=subprocess.DEVNULL,
                    )
                    library = pathlib.Path(directory) / "squashfs-root/usr/lib/libgtk-layer-shell.so.0"
                    staged = pathlib.Path(__file__).resolve().parent.parent / "src-tauri/binaries/linux/libgtk-layer-shell.so.0"
                    if not library.is_file():
                        raise RuntimeError(f"{package.name}: missing or incorrect bundled GTK3 Layer Shell")
                    # linuxdeploy may strip or patch the library. Compare ELF
                    # architecture, not bytes, and verify its runtime identity.
                    header = library.read_bytes()[:20]
                    expected = staged.read_bytes()[:20]
                    if header[:4] != b"\x7fELF" or header[4:6] != expected[4:6] or header[18:20] != expected[18:20]:
                        raise RuntimeError(f"{package.name}: incorrect GTK3 Layer Shell architecture")
                    dynamic = output("readelf", "-d", str(library))
                    symbols = output("readelf", "--dyn-syms", "--wide", str(library))
                    if "[libgtk-layer-shell.so.0]" not in dynamic or "gtk_layer_init_for_window" not in symbols:
                        raise RuntimeError(f"{package.name}: invalid GTK3 Layer Shell library")
            print(f"Verified GTK3 Layer Shell: {package.name}")


if __name__ == "__main__":
    check(sys.argv[1])
