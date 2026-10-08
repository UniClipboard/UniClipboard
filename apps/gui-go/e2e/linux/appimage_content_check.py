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
no_layer = '--expect-no-layer-shell' in sys.argv  # the NOLAYER- negative control omits the library
x11_hook = '--expect-x11-hook' in sys.argv  # the differential control package (17c13) keeps the hook line
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
    EXPECTED = {'libgiognutls.so': 'glib-networking', 'libgiognomeproxy.so': 'glib-networking', 'libdconfsettings.so': 'dconf-gsettings-backend',
                'libgiolibproxy.so': 'glib-networking',
                'libgiouniclipboardloopback.so': 'uniclipboard (built from source in the build image)'}  # keep in step with package_linux.GIO_MODULES + GUARD_MODULE
    check(f'C1 the bundled GIO module directory holds exactly {sorted(EXPECTED)}', present == sorted(EXPECTED), present)
    recs = {m['module']: m for m in (gio['modules'] if isinstance(gio, dict) else [])}
    libc_family = re.compile(r'^(libc|libm|libdl|libpthread|librt|ld-linux.*)\.so(\.\d+)*$')
    for name, package in sorted(EXPECTED.items()):
        mod, rec = moddir / name, recs.get(name, {})
        sha = hashlib.sha256(mod.read_bytes()).hexdigest() if mod.is_file() else None
        check(f'C2 {name}: the module bytes equal the package manifest (package {package}, recorded SHA-256)', sha is not None and sha == rec.get('sha256') and rec.get('package') == package,
              {'sha256': sha, 'manifest': {k: rec.get(k) for k in ('package', 'packageVersion', 'source', 'sha256')}})
        needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', subprocess.run(['readelf', '-d', '-W', str(mod)], capture_output=True, text=True, check=True).stdout)
        missing = [n for n in needed if n not in names and not libc_family.match(n)]
        check(f'C3 {name}: every library the module needs is in the AppDir or libc-family', not missing, {'needed': needed, 'missing': missing})
    # The support set follows the libproxy generation of the build image (package_linux.gio_support_libs): 0.5 on Ubuntu 24.04, 0.4 on Debian 12. The manifest says which one this package was built with.
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from package_linux import GIO_SUPPORT_LIBS_V4, GIO_SUPPORT_LIBS_V5
    recorded = {m['library'] for m in (gio.get('supportLibraries', []) if isinstance(gio, dict) else [])}
    SUPPORT = GIO_SUPPORT_LIBS_V5 if 'libpxbackend-1.0.so' in recorded else GIO_SUPPORT_LIBS_V4
    support = {m['library']: m for m in (gio.get('supportLibraries', []) if isinstance(gio, dict) else [])}
    for name, package in sorted(SUPPORT.items()):
        f, rec = root / 'usr/lib' / name, support.get(name, {})
        sha = hashlib.sha256(f.read_bytes()).hexdigest() if f.is_file() else None
        check(f'C2b {name}: the bytes equal the package manifest (package {package}, recorded SHA-256)', sha is not None and sha == rec.get('sha256') and rec.get('package') == package,
              {'sha256': sha, 'manifest': {k: rec.get(k) for k in ('package', 'packageVersion', 'source', 'sha256')}})
    check('C4 the TLS library the module needs is shipped (libgnutls.so.30) and libsoup 3 is the HTTP stack', 'libgnutls.so.30' in names and 'libsoup-3.0.so.0' in names, None)
    pac = gio.get('pacRunner', {}) if isinstance(gio, dict) else {}
    pac_file = root / 'usr/libexec/glib-pacrunner'
    pac_sha = hashlib.sha256(pac_file.read_bytes()).hexdigest() if pac_file.is_file() else None
    check('C9 the PAC helper glib-pacrunner is bundled and its bytes equal the package manifest (glib-networking-services)', pac_sha is not None and pac_sha == pac.get('sha256') and pac.get('package') == 'glib-networking-services',
          {'sha256': pac_sha, 'manifest': {k: pac.get(k) for k in ('package', 'packageVersion', 'source', 'sha256')}})
    pac_needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', subprocess.run(['readelf', '-d', '-W', str(pac_file)], capture_output=True, text=True, check=True).stdout) if pac_file.is_file() else []
    pac_missing = [n for n in pac_needed if n not in names and not libc_family.match(n)]
    check('C9b glib-pacrunner: every library it needs (libproxy, GLib, GIO) is in the AppDir or libc-family', bool(pac_needed) and not pac_missing, {'needed': pac_needed, 'missing': pac_missing})
host_owned = re.compile(r'^(libEGL|libGL|libGLX|libGLdispatch|libOpenGL|libGLESv1_CM|libGLESv2|libdrm|libgbm|libwayland-client|libdbus-1|libvulkan)\.so')
found = sorted(n for n in names if host_owned.match(n))
check('C5 no host-owned library (GL/EGL/GLES entry points, libdrm, libgbm, libwayland-client, libdbus-1) is inside the AppDir', not found, found)
check('C6 the shared MIME cache of 17c5 is still bundled', (root / 'usr/share/mime/mime.cache').is_file(), None)
apprun = (root / 'AppRun.wrapped').read_text() if (root / 'AppRun.wrapped').is_file() else ''
check('C7 AppRun points GIO_MODULE_DIR at the bundled directory', 'GIO_MODULE_DIR' in apprun and 'gio/modules' in apprun, None)
check('C8 the manifest records the libdbus removal for this package', bool(manifest['appimage'].get('libdbusRemoved')), manifest['appimage'].get('libdbusRemoved'))
# 17c13: native Wayland and Layer Shell. The library is carried (the quick panel dlopens it) and the GTK hook no longer forces the X11 backend.
ls = manifest['appimage'].get('layerShell') if isinstance(manifest['appimage'].get('layerShell'), dict) else {}
ls_file = root / 'usr/lib' / ls.get('file', 'libgtk-layer-shell.so.0')
ls_sha = hashlib.sha256(ls_file.read_bytes()).hexdigest() if ls_file.is_file() else None
check('C10 CONTROL: no libgtk-layer-shell file is in the AppDir', not [n for n in names if n.startswith('libgtk-layer-shell')], sorted(n for n in names if n.startswith('libgtk-layer-shell'))) if no_layer else check('C10 libgtk-layer-shell.so.0 is bundled, its bytes equal the package manifest (package libgtk-layer-shell0) and its soname resolves', ls_sha is not None and ls_sha == ls.get('sha256') and ls.get('package') == 'libgtk-layer-shell0'
      and (root / 'usr/lib/libgtk-layer-shell.so.0').exists(), {'sha256': ls_sha, 'manifest': {k: ls.get(k) for k in ('package', 'packageVersion', 'source', 'sha256', 'hostProvided')}})
ls_needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', subprocess.run(['readelf', '-d', '-W', str(ls_file)], capture_output=True, text=True, check=True).stdout) if ls_file.is_file() else []
ls_missing = [n for n in ls_needed if n not in names and not re.match(r'^(libc|libm|libdl|libpthread|librt|ld-linux.*)\.so(\.\d+)*$', n) and n != 'libwayland-client.so.0']
if not no_layer:
    check('C10b libgtk-layer-shell needs only AppDir libraries, libc-family and the host libwayland-client', bool(ls_needed) and not ls_missing, {'needed': ls_needed, 'missing': ls_missing})
hooks = [p for p in (root / 'apprun-hooks').glob('*.sh')] if (root / 'apprun-hooks').is_dir() else []
forced = [p.name for p in hooks if re.search(r'^\s*export GDK_BACKEND=', p.read_text(), re.M)]
check('C11 ' + ('CONTROL: the GTK hook still forces GDK_BACKEND (differential package)' if x11_hook else 'no AppRun hook exports GDK_BACKEND (GTK chooses; a user value is honoured)'),
      bool(forced) if x11_hook else (bool(hooks) and not forced), {'hooks': [p.name for p in hooks], 'forcing': forced, 'manifest': manifest['appimage'].get('gdkBackendHook')})
# Runtime observations are supplied by a real execution of this exact artifact.
# They extend the content assertions without claiming coverage of unexecuted dlopen paths.
if '--runtime-inventory' in sys.argv:
    inventory_path = Path(sys.argv[sys.argv.index('--runtime-inventory') + 1])
    inventory = json.loads(inventory_path.read_text())
    image_hashes = {value for name, value in manifest['sha256'].items() if name.endswith('.AppImage')}
    check('R1 the runtime inventory belongs to this exact AppImage',
          inventory.get('imageSha256') in image_hashes, inventory.get('imageSha256'))
    rows = inventory['libraries']
    check('R2 the runtime inventory is nonempty and records its coverage boundary',
          bool(rows) and bool(inventory.get('scope')), inventory.get('scope'))
    for row in rows:
        name, ownership = row['name'], row['classification']
        if ownership == 'bundled':
            relative = Path(row['bundleRelativePath'])
            valid_path = not relative.is_absolute() and '..' not in relative.parts
            file = root / relative
            digest = hashlib.sha256(file.read_bytes()).hexdigest() if valid_path and file.is_file() else None
            check(f'R3 {name}: the observed bundled file is present with identical bytes',
                  digest is not None and digest == row['sha256'], {'path': str(relative), 'sha256': digest})
        elif ownership in ('host-owned', 'host-staged'):
            check(f'R4 {name}: the observed {ownership} library is absent from the bundle', name not in names and row['soname'] not in names, {'file': name, 'soname': row['soname']})
        else:
            check(f'R5 {name}: the observation has a recognised ownership class', False, ownership)
out.write_text(json.dumps({'passed': all(c['ok'] for c in checks), 'checks': checks}, indent=2) + '\n')
sys.exit(0 if all(c['ok'] for c in checks) else 1)
