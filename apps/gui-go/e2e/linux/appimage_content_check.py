#!/usr/bin/env python3
"""Mechanical content assertions on an EXTRACTED AppImage (slice 17c7, docs/architecture/gui-go-linux-appimage-runtime-deps.md).

  appimage_content_check.py <squashfs-root> <package-manifest.json> <out.json> [--expect-no-tls-module]

Reads the artifact itself (not the packager's own report): the bundled GIO modules are exactly the TLS backend and match the manifest's recorded
source and SHA-256, the module's NEEDED libraries are in the AppDir or libc-family, and none of the host-owned libraries (GL/EGL/GLES entry points,
libdrm, libgbm, libwayland-client, libdbus-1) is inside. Uses readelf (binutils), as package_linux.py does.
"""
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

root, manifest_path, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
no_tls = '--expect-no-tls-module' in sys.argv
manifest = json.loads(manifest_path.read_text())
gio = manifest['appimage'].get('gioModules')
checks = []


def check(name, ok, detail=None):
    checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
    print(('PASS ' if ok else 'FAIL ') + name)


files = [p for p in root.rglob('*') if p.is_file() or p.is_symlink()]
names = {p.name for p in files}
moddir = root / 'usr/lib/gio/modules'
present = sorted(p.name for p in moddir.iterdir()) if moddir.is_dir() else None
if no_tls:
    check('C1 negative control: the bundled GIO module directory exists and is EMPTY', present == [], present)
else:
    check('C1 the bundled GIO module directory holds exactly libgiognutls.so', present == ['libgiognutls.so'], present)
    rec = gio['modules'][0] if isinstance(gio, dict) else {}
    mod = moddir / 'libgiognutls.so'
    sha = hashlib.sha256(mod.read_bytes()).hexdigest() if mod.is_file() else None
    check('C2 the module bytes equal the package manifest (package glib-networking, recorded SHA-256)', sha is not None and sha == rec.get('sha256') and rec.get('package') == 'glib-networking',
          {'sha256': sha, 'manifest': {k: rec.get(k) for k in ('package', 'packageVersion', 'source', 'sha256')}})
    needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', subprocess.run(['readelf', '-d', '-W', str(mod)], capture_output=True, text=True, check=True).stdout)
    libc_family = re.compile(r'^(libc|libm|libdl|libpthread|librt|ld-linux.*)\.so(\.\d+)*$')
    missing = [n for n in needed if n not in names and not libc_family.match(n)]
    check('C3 every library the module needs is in the AppDir or libc-family', not missing, {'needed': needed, 'missing': missing})
    check('C4 the TLS library the module needs is shipped (libgnutls.so.30) and libsoup 3 is the HTTP stack', 'libgnutls.so.30' in names and 'libsoup-3.0.so.0' in names, None)
host_owned = re.compile(r'^(libEGL|libGL|libGLX|libGLdispatch|libOpenGL|libGLESv1_CM|libGLESv2|libdrm|libgbm|libwayland-client|libdbus-1|libvulkan)\.so')
found = sorted(n for n in names if host_owned.match(n))
check('C5 no host-owned library (GL/EGL/GLES entry points, libdrm, libgbm, libwayland-client, libdbus-1) is inside the AppDir', not found, found)
check('C6 the shared MIME cache of 17c5 is still bundled', (root / 'usr/share/mime/mime.cache').is_file(), None)
apprun = (root / 'AppRun.wrapped').read_text() if (root / 'AppRun.wrapped').is_file() else ''
check('C7 AppRun points GIO_MODULE_DIR at the bundled directory', 'GIO_MODULE_DIR' in apprun and 'gio/modules' in apprun, None)
check('C8 the manifest records the libdbus removal for this package', bool(manifest['appimage'].get('libdbusRemoved')), manifest['appimage'].get('libdbusRemoved'))
out.write_text(json.dumps({'passed': all(c['ok'] for c in checks), 'checks': checks}, indent=2) + '\n')
sys.exit(0 if all(c['ok'] for c in checks) else 1)
