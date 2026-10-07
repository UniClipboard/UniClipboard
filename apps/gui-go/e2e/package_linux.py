#!/usr/bin/env python3
"""Package the Linux Go GUI: release binary, deb, rpm, AppImage and the AppImage updater archive.

  python3 apps/gui-go/e2e/package_linux.py --arch amd64|arm64 --daemon <path to uniclipd> --out <dir> [--frontend-dist <dir>]

Runs on a Linux host (the Dockerfile in e2e/linux provides one): the GUI links GTK3 and WebKitGTK through cgo, so it
cannot be cross-compiled from macOS. The daemon is NOT built here: `uniclipd` for the same architecture must be
supplied; a missing or malformed file is a hard error. The frontend bundle (apps/gui-go/frontend/dist) must exist
(`bun --bun run --cwd apps/gui-go build`).

Outputs in <dir> (names follow the Tauri bundler, which the release workflow and updater feed already expect):
  uniclipboard                                       release build (tags gtk3,production,release)
  UniClipboard_<version>_<amd64|arm64>.deb           /usr/bin/uniclipboard + /usr/bin/uniclipd + desktop entry + icons
  UniClipboard-<version>-1.<x86_64|aarch64>.rpm      same payload
  UniClipboard_<version>_<amd64|aarch64>.AppImage    self-contained AppDir (linuxdeploy + the Wails GTK plugin, docs/architecture/gui-go-linux-appimage.md)
  UniClipboard_<version>_<amd64|aarch64>.AppImage.tar.gz   the updater artifact (the release workflow signs it: `.sig`)
  package-manifest.json                              provenance and an explicit list of what is NOT proven

What this proves: the artifacts build. It does NOT prove they install or run on a real desktop, and:
  * the AppImage bundles GTK3, WebKitGTK, GLib and the WebKit helper processes with linuxdeploy-07333c6 (the pinned build whose
    exclude list keeps the host driver stack out, docs/architecture/linux-appimage-library-policy.md); package-manifest.json
    records what the AppDir inspection found, but only linux_appimage_run.py in a host without GTK/WebKitGTK proves it runs;
  * nothing is signed (no minisign `.sig`): signing stays in the release workflow;
  * with --packaging-check-fixture the daemon is a placeholder and every output is prefixed FIXTURE-.
"""
import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
GUI = ROOT / 'apps/gui-go'
ARCH = {  # go arch -> (deb, rpm/AppImage)
    'amd64': ('amd64', 'x86_64'),
    'arm64': ('arm64', 'aarch64'),
}
APPIMAGETOOL = {  # a fixed release tag, SHA-256 verified after the download
    'amd64': ('https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage',
              '46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1'),
    'arm64': ('https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-aarch64.AppImage',
              '04f45ea45b5aa07bb2b071aed9dbf7a5185d3953b11b47358c1311f11ea94a96'),
}
# The AppImage runtime that appimagetool embeds in front of the SquashFS. Without --runtime-file appimagetool downloads the
# CURRENT `continuous` runtime at pack time (an unpinned input; a network blip also fails the package). The runtime provides the
# `<AppImage>.home` and `$APPIMAGE` semantics the portable mode relies on, so it is pinned here, next to appimagetool, the one consumer
# (scripts/linux-appimage-tools.mjs only serves the Tauri bundle, which cannot pass --runtime-file).
# Source: type2-runtime revision 8f39b89 (the build that carries "Create directories for extraction with mode 0700"; the dated tag
# 20251108 = dd6cebe lacks it). The `continuous` URL is mutable: when it moves on, the SHA-256 check fails closed and the pin must
# be updated deliberately (docs/architecture/gui-go-linux-appimage-runtime-pin.md). The SHA-256 values are the GitHub asset digests,
# re-verified by hand and by a gpg check of the published .sig files.
RUNTIME_REVISION = '8f39b89e2ac31e1640b3d3f7e9a5108e6ce805fa'
RUNTIME_BASE = 'https://github.com/AppImage/type2-runtime/releases/download/continuous'
RUNTIME = {  # go arch -> (asset, SHA-256, ELF e_machine)
    'amd64': ('runtime-x86_64', '156f4bdbde9c52d01814600013e0a273f0118dc2de98975f3c8c63427ec79074', 62),
    'arm64': ('runtime-aarch64', 'b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39d', 183),
}
# The linuxdeploy pin has one source of truth, shared with the Tauri bundle: scripts/linux-appimage-tools.mjs.
LINUXDEPLOY_PIN_FILE = ROOT / 'scripts/linux-appimage-tools.mjs'
LINUXDEPLOY_BASE = 'https://github.com/tauri-apps/binary-releases/releases/download'
# Libraries that belong to the host's driver stack and must never be inside the AppImage (policy document).
HOST_ONLY_LIBS = ('libwayland-client.so', 'libEGL.so', 'libGL.so', 'libGLX.so', 'libGLdispatch.so', 'libdrm.so', 'libgbm.so', 'libvulkan.so',
                # 17c7: libglvnd's other entry points (dlopen'd, not in linuxdeploy's exclude list)
                'libGLESv1_CM.so', 'libGLESv2.so', 'libOpenGL.so',
                # 17c7: the host's dbus-launch/daemon helpers load libdbus through AppRun's LD_LIBRARY_PATH (found on Fedora)
                'libdbus-1.so')
# The GIO modules the AppImage carries, each with the distribution package that owns it. They are copied from the build image, i.e. built against the SAME
# GLib as the bundled one (the 17c4 crash was a HOST module against the bundled GLib). gvfs stays out (docs/architecture/gui-go-linux-appimage-runtime-deps.md).
#  libgiognutls.so      the TLS backend of GLib (libsoup 3 and so WebKitGTK reach HTTPS through it)                       (17c7)
#  libgiognomeproxy.so  GProxyResolver reading the GNOME proxy settings (org.gnome.system.proxy: manual, ignore-hosts)    (17c12)
#  libdconfsettings.so  the GSettings backend that reads the user's and the system's dconf databases                      (17c12)
#  libgiolibproxy.so    GProxyResolver over libproxy: environment variables (http_proxy ...), PAC, KDE/sysconfig configuration                     (17c12)
GIO_MODULES = {'libgiognutls.so': 'glib-networking', 'libgiognomeproxy.so': 'glib-networking', 'libdconfsettings.so': 'dconf-gsettings-backend',
               'libgiolibproxy.so': 'glib-networking'}
# The libraries libgiolibproxy.so needs that the AppImage does not already carry (computed from the build image's own dependency closure, then frozen here: a new
# entry is a decision, not an accident). libproxy 0.5's backend hard-links the PAC runtime (duktape) and the PAC downloader (libcurl-gnutls), whose own closure
# (libssh, libldap/liblber, libsasl2, librtmp, OpenSSL's libcrypto) comes with it; this is the distribution's own dependency set for libproxy, not a choice of ours.
GIO_SUPPORT_LIBS = {'libproxy.so.1': 'libproxy1v5', 'libpxbackend-1.0.so': 'libproxy1v5', 'libduktape.so.207': 'libduktape207', 'libcurl-gnutls.so.4': 'libcurl3t64-gnutls',
                    'libssh.so.4': 'libssh-4', 'libldap.so.2': 'libldap2', 'liblber.so.2': 'libldap2', 'libsasl2.so.2': 'libsasl2-2', 'librtmp.so.1': 'librtmp1',
                    'libcrypto.so.3': 'libssl3t64'}
# Libraries every Linux host has and that the 17c7 classification (linux_appimage_tls_run.HOST_OK) already leaves to the host.
GIO_SUPPORT_HOST_OK = ('libz.so.1', 'libgmp.so.10', 'libcom_err.so.2', 'libresolv.so.2')
WEBKIT_HELPERS = ('WebKitWebProcess', 'WebKitNetworkProcess', 'WebKitGPUProcess')
APPRUN = GUI / 'e2e/linux/appimage/AppRun'
ICONS = {'32x32': '32x32.png', '128x128': '128x128.png', '256x256': '128x128@2x.png'}


def run(cmd, cwd=ROOT, env=None, capture=False):
    print('+', ' '.join(map(str, cmd)), flush=True)
    r = subprocess.run(cmd, cwd=cwd, env=env, check=True, text=True, capture_output=capture)
    return r.stdout.strip() if capture else None


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def provenance():
    head = run(['git', 'rev-parse', 'HEAD'], capture=True)
    status = run(['git', 'status', '--porcelain'], capture=True)
    diff = subprocess.run(['git', 'diff', 'HEAD'], cwd=ROOT, capture_output=True).stdout
    h = hashlib.sha256(diff)
    for name in sorted(run(['git', 'ls-files', '--others', '--exclude-standard'], capture=True).splitlines()):
        h.update(name.encode())
        try:
            h.update((ROOT / name).read_bytes())
        except OSError:
            pass
    return {'head': head, 'dirty': bool(status), 'dirtyDiffSha256': h.hexdigest() if status else None, 'immutable': not status}


def check_daemon(path, arch):
    """ELF structure and architecture only (e2e/elfcheck, debug/elf). Not identity, not origin, not that it runs."""
    r = subprocess.run(['go', 'run', './e2e/elfcheck', arch, str(path)], cwd=GUI, capture_output=True, text=True)
    return r.returncode == 0, (r.stdout.strip() if r.returncode == 0 else r.stderr.strip())


def stage_tree(root, binary, daemon):
    """The filesystem layout shared by the deb, the rpm and the AppImage (the Tauri deb layout and the AUR package)."""
    for sub in ('usr/bin', 'usr/share/applications'):
        (root / sub).mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, root / 'usr/bin/uniclipboard')
    shutil.copy2(daemon, root / 'usr/bin/uniclipd')
    for p in (root / 'usr/bin/uniclipboard', root / 'usr/bin/uniclipd'):
        p.chmod(0o755)
    shutil.copy2(ROOT / 'packaging/linux/uniclipboard.desktop', root / 'usr/share/applications/uniclipboard.desktop')
    for size, name in ICONS.items():
        d = root / f'usr/share/icons/hicolor/{size}/apps'
        d.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / 'apps/gui/src-tauri/icons' / name, d / 'uniclipboard.png')


def build_deb(stage, out, version, arch, name):
    d = out / 'deb-root'
    shutil.copytree(stage, d, symlinks=True)
    size_kib = sum(f.stat().st_size for f in d.rglob('*') if f.is_file()) // 1024
    (d / 'DEBIAN').mkdir()
    # GTK3/WebKitGTK/X11 plus libgtk-layer-shell0, as the Tauri deb declares it (the Layer Shell panel loads it with dlopen
    # and falls back to the ordinary window when it is absent, but the package still pulls it in for the Wayland panel).
    # The Tauri deb's libayatana-appindicator3 is not needed: Wails' tray is StatusNotifierItem over D-Bus.
    (d / 'DEBIAN/control').write_text(
        f'Package: uniclipboard\nVersion: {version}\nArchitecture: {ARCH[arch][0]}\nSection: utils\nPriority: optional\n'
        f'Installed-Size: {size_kib}\nMaintainer: UniClipboard <support@uniclipboard.app>\n'
        'Depends: libgtk-3-0, libwebkit2gtk-4.1-0, libx11-6, libgtk-layer-shell0\n'
        'Description: Encrypted peer-to-peer clipboard sync between your devices\n')
    deb = out / name
    run(['dpkg-deb', '--root-owner-group', '--build', str(d), str(deb)])
    return deb


def build_rpm(stage, out, version, arch, name):
    top = out / 'rpmbuild'
    for sub in ('BUILD', 'RPMS', 'SPECS'):
        (top / sub).mkdir(parents=True)
    files = sorted(str('/' / p.relative_to(stage)) for p in stage.rglob('*') if p.is_file())
    spec = top / 'SPECS/uniclipboard.spec'
    spec.write_text(
        'Name: uniclipboard\nVersion: %s\nRelease: 1\nSummary: Encrypted peer-to-peer clipboard sync between your devices\n'
        'License: Proprietary\nRequires: gtk3, webkit2gtk4.1, gtk-layer-shell\nAutoReqProv: no\n%%global _build_id_links none\n\n%%description\n'
        'Encrypted peer-to-peer clipboard sync between your devices.\n\n%%install\ncp -a %s/. %%{buildroot}/\n\n%%files\n%s\n'
        % (version, stage, '\n'.join(files)))
    run(['rpmbuild', '-bb', '--define', f'_topdir {top}', '--define', f'_rpmfilename {name}', '--target', ARCH[arch][1] + '-linux', str(spec)])
    produced = next((top / 'RPMS').rglob('*.rpm'))
    rpm = out / name
    shutil.copy2(produced, rpm)
    return rpm


def elf_machine(path):
    """e_machine of an ELF file, or None when the file is not a little-endian ELF."""
    head = Path(path).read_bytes()[:20]
    if len(head) < 20 or head[:4] != b'\x7fELF' or head[5] != 1:
        return None
    return int.from_bytes(head[18:20], 'little')


def reject(path, why):
    """Move a rejected file aside (never delete evidence) and say why."""
    aside = path.with_name(f'{path.name}.rejected-{sha256(path)[:8]}')
    path.rename(aside)
    print(f'REJECTED {path} ({why}); kept as {aside}', file=sys.stderr, flush=True)


def fetch_verified(url, sha, dest, machine=None):
    """Download once into the tools directory and refuse anything whose SHA-256 (or, with `machine`, ELF architecture) differs
    from the pin. A cached file that fails is moved aside and fetched again; a rejected download is kept as `.rejected-*` and ends
    the run: there is no fallback to another source."""
    def problem(path):
        if machine is not None and elf_machine(path) != machine:
            return f'ELF e_machine {elf_machine(path)}, expected {machine}'
        if sha256(path) != sha:
            return f'SHA-256 {sha256(path)} differs from the pinned {sha}'
        return None
    if dest.exists():
        why = problem(dest)
        if why:
            reject(dest, f'cached file: {why}')
    if not dest.exists():
        part = dest.with_name(dest.name + '.partial')
        try:
            urllib.request.urlretrieve(url, part)
        except OSError as e:
            sys.exit(f'{url}: cannot download the pinned input ({e}); nothing is packaged without it')
        why = problem(part)
        if why:
            reject(part, f'download of {url}: {why}')
            sys.exit(f'{url}: {why}')
        part.rename(dest)
    dest.chmod(0o755)
    return dest


def section(path, name):
    """(file offset, size) of an ELF section, from readelf."""
    m = re.search(r'\]\s+%s\s+PROGBITS\s+\S+\s+([0-9a-f]+)\s+([0-9a-f]+)' % re.escape(name), run(['readelf', '-S', '-W', str(path)], capture=True))
    if not m:
        sys.exit(f'{path} has no {name} section')
    return int(m.group(1), 16), int(m.group(2), 16)


def verify_embedded_runtime(image, runtime, arch):
    """appimagetool writes the runtime file in front of the SquashFS and only fills the runtime's own .digest_md5 section.
    Anything else that differs means the image does not carry the pinned runtime."""
    rt, img = runtime.read_bytes(), image.read_bytes()
    offset, size = section(runtime, '.digest_md5')
    prefix = img[:len(rt)]
    masked = lambda b: b[:offset] + b'\0' * size + b[offset + size:]
    if masked(prefix) != masked(rt):
        sys.exit(f'{image}: the first {len(rt)} bytes are not the pinned runtime (beyond the .digest_md5 section)')
    if img[len(rt):len(rt) + 4] != b'hsqs':
        sys.exit(f'{image}: no SquashFS right after the runtime ({len(rt)} bytes)')
    reported = None
    if elf_machine(runtime) == {'x86_64': 62, 'aarch64': 183}.get(platform.machine()):
        r = subprocess.run([str(image), '--appimage-version'], capture_output=True, text=True)
        reported = (r.stdout + r.stderr).strip()
        if r.returncode != 0 or RUNTIME_REVISION[:7] not in reported:
            sys.exit(f'{image} --appimage-version: rc={r.returncode} {reported!r}; expected revision {RUNTIME_REVISION[:7]}')
    return {'squashfsOffset': len(rt), 'digestMd5Section': {'offset': offset, 'size': size}, 'prefixSha256': hashlib.sha256(prefix).hexdigest(),
            'prefixWithDigestZeroedSha256': hashlib.sha256(masked(prefix)).hexdigest(), 'versionReportedByImage': reported or 'not run: the runtime is not for this host architecture'}


def linuxdeploy_pin(arch):
    text = LINUXDEPLOY_PIN_FILE.read_text()
    release = re.search(r"release:\s*'([^']+)'", text)
    sha = re.search(r"%s:\s*'([0-9a-f]{64})'" % ARCH[arch][1], text)
    if not release or not sha:
        sys.exit(f'cannot read the linuxdeploy pin for {ARCH[arch][1]} from {LINUXDEPLOY_PIN_FILE}')
    return release.group(1), sha.group(1)


def webkit_helper_dir():
    """The host directory that holds WebKitWebProcess (what libwebkit2gtk has compiled in as its helper directory)."""
    for base in sorted(Path('/usr').glob('lib*/**/webkit2gtk-4.1')):
        if (base / 'WebKitWebProcess').is_file():
            return base
    sys.exit('no webkit2gtk-4.1 helper directory under /usr (libwebkit2gtk-4.1-dev is required on the build host)')


def relocate_webkit(appdir, helper_dir):
    """Rewrite the helper directory compiled into libwebkit2gtk (`/usr/lib/<triple>/webkit2gtk-4.1`) to
    `././/lib/<triple>/webkit2gtk-4.1`, the same length, so it resolves relative to $APPDIR/usr (AppRun's working
    directory). The Tauri bundler does the same with a blanket /usr replacement; only the one path is changed here."""
    old = str(helper_dir).encode()
    assert old.startswith(b'/usr')
    new = b'././' + old[4:]
    libs = sorted((appdir / 'usr/lib').rglob('libwebkit2gtk-4.1.so*'))
    libs = [lib for lib in libs if lib.is_file() and not lib.is_symlink()]
    if not libs:
        sys.exit('libwebkit2gtk-4.1 was not deployed into the AppDir')
    count = 0
    for lib in libs:
        data = lib.read_bytes()
        n = data.count(old)
        if n:
            lib.write_bytes(data.replace(old, new))
            count += n
    if not count:
        sys.exit(f'{old.decode()} is not compiled into the deployed libwebkit2gtk: the layout assumption is wrong')
    return {'from': old.decode(), 'to': new.decode(), 'occurrences': count, 'libraries': [str(l.relative_to(appdir)) for l in libs]}


def inspect_appdir(appdir, helper_dir):
    files = [p for p in appdir.rglob('*') if p.is_file() or p.is_symlink()]
    names = [p.name for p in files]
    def has(prefix):
        return sorted({n for n in names if n.startswith(prefix)})
    inside = appdir / 'usr' / str(helper_dir).lstrip('/')[len('usr/'):]
    return {
        'webkit': has('libwebkit2gtk-4.1.so'), 'gtk3': has('libgtk-3.so'), 'glib': has('libglib-2.0.so'), 'gio': has('libgio-2.0.so'),
        'helpers': sorted(h for h in WEBKIT_HELPERS if (inside / h).is_file()), 'injectedBundle': (inside / 'injected-bundle').is_dir(),
        'hostOnlyLibrariesFound': sorted({n for n in names if any(n.startswith(p) for p in HOST_ONLY_LIBS)}),
        'gioModules': sorted(p.name for p in (appdir / 'usr/lib/gio/modules').iterdir()),
        'gioModuleDirs': sorted(str(p.relative_to(appdir)) for p in appdir.rglob('modules') if p.parent.name == 'gio'),
        'hooks': sorted(p.name for p in (appdir / 'apprun-hooks').glob('*')) if (appdir / 'apprun-hooks').is_dir() else [],
        'fileCount': len(files),
    }


def deploy_gio_modules(appdir):
    """Copy the GIO modules in GIO_MODULES into usr/lib/gio/modules and the libraries in GIO_SUPPORT_LIBS into usr/lib, prove each one's provenance (the owning
    distribution package, SHA-256) and that every library it needs is in the AppDir, libc-family or one of GIO_SUPPORT_HOST_OK: a missing dependency would only fail
    at run time, on a host that happens to lack it."""
    moddir = Path(run(['pkg-config', '--variable=giomoduledir', 'gio-2.0'], capture=True))
    libc_family = re.compile(r'^(libc|libm|libdl|libpthread|librt|ld-linux.*)\.so(\.\d+)*$')
    loader = run(['ldconfig', '-p'], capture=True)
    libdir_of = {}
    for line in loader.splitlines():
        m = re.match(r'\s*(\S+) \(.*\) => (\S+)', line)
        if m:
            libdir_of.setdefault(m.group(1), m.group(2))
    support_rows = []
    for name, package in GIO_SUPPORT_LIBS.items():
        src = Path(libdir_of.get(name) or (moddir.parent / 'libproxy' / name))
        if name == 'libpxbackend-1.0.so':
            src = moddir.parent / 'libproxy' / name
        if not src.is_file():
            sys.exit(f'{name} is missing in the build image: install {package}')
        real = src.resolve()
        owner = run(['dpkg', '-S', str(real)], capture=True)
        if not owner.startswith(package):
            sys.exit(f'{real} is not owned by {package}: {owner}')
        dest = appdir / 'usr/lib' / name
        shutil.copy2(real, dest)
        support_rows.append({'library': name, 'source': str(real), 'package': package, 'packageVersion': run(['dpkg-query', '-W', '-f', '${Version}', package], capture=True),
                             'sha256': sha256(dest)})
    shipped = {p.name for p in (appdir / 'usr/lib').rglob('*.so*') if p.is_file() or p.is_symlink()}
    for row in support_rows:  # the closure of the support libraries themselves
        needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', run(['readelf', '-d', str(appdir / 'usr/lib' / row['library'])], capture=True))
        missing = sorted(n for n in needed if n not in shipped and not libc_family.match(n) and n not in GIO_SUPPORT_HOST_OK)
        if missing:
            sys.exit(f"{row['library']} needs libraries that are neither in the AppDir, libc-family nor host-provided: {missing}")
        row['needed'] = needed
    rows = []
    for name, package in GIO_MODULES.items():
        src = moddir / name
        if not src.is_file():
            sys.exit(f'{src} is missing in the build image: install {package}')
        owner = run(['dpkg', '-S', str(src)], capture=True)
        if not owner.startswith(package):
            sys.exit(f'{src} is not owned by {package}: {owner}')
        version = run(['dpkg-query', '-W', '-f', '${Version}', package], capture=True)
        dest = appdir / 'usr/lib/gio/modules' / name
        shutil.copy2(src, dest)
        needed = re.findall(r'\(NEEDED\)\s+Shared library: \[(.+?)\]', run(['readelf', '-d', str(dest)], capture=True))
        missing = sorted(n for n in needed if n not in shipped and not libc_family.match(n))
        if missing:
            sys.exit(f'{name} needs libraries that are neither in the AppDir nor libc-family: {missing}')
        rows.append({'module': name, 'source': str(src), 'package': package, 'packageVersion': version, 'sha256': sha256(dest), 'needed': needed})
    glib_version = run(['dpkg-query', '-W', '-f', '${Version}', 'libglib2.0-0t64'], capture=True)
    return {'modules': rows, 'supportLibraries': support_rows, 'bundledGLibPackageVersion': glib_version}


def build_appimage(stage, out, arch, name, tools, daemon, relocate=True, marker=None, tls_module=True):
    tools.mkdir(exist_ok=True)
    tool_url, tool_pin = APPIMAGETOOL[arch]
    appimagetool = fetch_verified(tool_url, tool_pin, tools / 'appimagetool')
    asset, runtime_sha, machine = RUNTIME[arch]
    runtime = fetch_verified(f'{RUNTIME_BASE}/{asset}', runtime_sha, tools / f'appimage-{asset}', machine=machine)
    release, pin = linuxdeploy_pin(arch)
    linuxdeploy = fetch_verified(f'{LINUXDEPLOY_BASE}/{release}/linuxdeploy-{ARCH[arch][1]}.AppImage', pin, tools / 'linuxdeploy')
    # The GTK plugin is the one embedded in the pinned Wails module (`wails3 generate appimage` uses it); it is read from the
    # module cache, not copied into the repository.
    wails_dir = Path(run(['go', 'list', '-m', '-f', '{{.Dir}}', 'github.com/wailsapp/wails/v3'], cwd=GUI, capture=True))
    plugin_source = wails_dir / 'internal/commands/linuxdeploy-plugin-gtk.sh'
    plugin = tools / 'linuxdeploy-plugin-gtk.sh'
    shutil.copy2(plugin_source, plugin)
    plugin.chmod(0o755)

    appdir = out / 'UniClipboard.AppDir'
    shutil.copytree(stage, appdir, symlinks=True)
    helper_dir = webkit_helper_dir()
    target = appdir / 'usr' / str(helper_dir).lstrip('/')[len('usr/'):]
    target.mkdir(parents=True)
    for helper in WEBKIT_HELPERS:
        if (helper_dir / helper).is_file():
            shutil.copy2(helper_dir / helper, target / helper)
    shutil.copytree(helper_dir / 'injected-bundle', target / 'injected-bundle')
    # The pinned Wails GTK plugin deploys no GIO modules. The bundled directory is what AppRun's GIO_MODULE_DIR points at, so the bundled
    # GLib never loads the host's (ABI-incompatible) gvfs/dconf modules; deploy_gio_modules fills it with the TLS backend below (only the
    # --negative-control-no-tls-module variant leaves it empty).
    (appdir / 'usr/lib/gio/modules').mkdir(parents=True)
    gio_modules_info = None  # filled after linuxdeploy, once the libraries a module needs are in the AppDir
    # The bundled gdk-pixbuf recognises image formats through the shared MIME database (shared-mime-info). A host without
    # /usr/share/mime (a minimal container, but also any machine that never installed it) makes every GTK icon or dialog image
    # fail with "Couldn't recognize the image file format", and GTK aborts on the resulting assertion (observed in 17c5: the startup
    # error dialog crashed the process). The compiled cache is 157 KB; the hook's XDG_DATA_DIRS (`$APPDIR/usr/share` first) finds it.
    mime_cache = Path('/usr/share/mime/mime.cache')
    if not mime_cache.is_file():
        sys.exit('shared-mime-info is not installed in the build image: /usr/share/mime/mime.cache is missing')
    (appdir / 'usr/share/mime').mkdir(parents=True)
    shutil.copy2(mime_cache, appdir / 'usr/share/mime/mime.cache')
    if marker:
        (appdir / 'usr/share/uniclipboard').mkdir(parents=True)
        (appdir / 'usr/share/uniclipboard/update-marker.txt').write_text(marker)

    execs = [appdir / 'usr/bin/uniclipboard', appdir / 'usr/bin/uniclipd'] + [target / h for h in WEBKIT_HELPERS if (target / h).is_file()]
    cmd = [str(linuxdeploy), '--appimage-extract-and-run', '--appdir', str(appdir), '--plugin', 'gtk', '--custom-apprun', str(APPRUN),
           '-d', str(appdir / 'usr/share/applications/uniclipboard.desktop'), '-i', str(appdir / 'usr/share/icons/hicolor/128x128/apps/uniclipboard.png')]
    for e in execs:
        cmd += ['-e', str(e)]
    # NO_STRIP: the bundled strip cannot process the .relr.dyn sections of current distributions' GTK libraries (Wails does the same).
    run(cmd, env=dict(os.environ, DEPLOY_GTK_VERSION='3', NO_STRIP='1', ARCH=ARCH[arch][1], PATH=f'{tools}:{os.environ["PATH"]}'))

    # 17c7: the TLS backend. The module comes from the build image's glib-networking package, i.e. built against the very GLib/GnuTLS that
    # linuxdeploy just bundled (same distribution release), which is why it can be loaded where the host's gvfs/dconf modules cannot.
    gio_modules_info = deploy_gio_modules(appdir) if tls_module else 'DISABLED (negative control: the bundled GIO module directory stays empty)'

    # libdbus-1 is the host's. linuxdeploy's exclude list does not name it, and its `--exclude-library` option is honoured by the main run but not by the
    # GTK plugin's own deployment pass (the log shows "Skipping ... blacklisted libdbus" and then the plugin deploying it anyway). A bundled copy shadows
    # the host's for every host helper the GUI starts through AppRun's LD_LIBRARY_PATH: observed 17c7 on Fedora, whose dbus-launch (libdbus 1.16.2)
    # failed on the bundled older copy ("version LIBDBUS_PRIVATE_1.16.2 not found", rc 127) and the GUI then exited 1 without a message.
    # Removed after linuxdeploy; inspect_appdir fails the package if any libdbus is left.
    removed_dbus = sorted(p.name for p in (appdir / 'usr/lib').glob('libdbus-1.so*'))
    for p in (appdir / 'usr/lib').glob('libdbus-1.so*'):
        p.unlink()

    # linuxdeploy sets an $ORIGIN rpath on every executable it is given, which changes the bytes. The daemon is given to it so that
    # its libraries are deployed, then the original file is put back: AppRun's LD_LIBRARY_PATH covers the lookup, and the daemon
    # in the image stays byte-identical to the one built with evidence.
    after_linuxdeploy = sha256(appdir / 'usr/bin/uniclipd')
    runpath_after = run(['readelf', '-d', str(appdir / 'usr/bin/uniclipd')], capture=True)
    runpath_before = run(['readelf', '-d', str(daemon)], capture=True)
    shutil.copy2(daemon, appdir / 'usr/bin/uniclipd')
    (appdir / 'usr/bin/uniclipd').chmod(0o755)
    daemon_chain = {'builtSha256': sha256(daemon), 'afterLinuxdeploySha256': after_linuxdeploy, 'finalInAppDirSha256': sha256(appdir / 'usr/bin/uniclipd'),
                    'linuxdeployChangedBytes': after_linuxdeploy != sha256(daemon),
                    'dynamicBefore': [l for l in runpath_before.splitlines() if 'RUNPATH' in l or 'RPATH' in l],
                    'dynamicAfterLinuxdeploy': [l for l in runpath_after.splitlines() if 'RUNPATH' in l or 'RPATH' in l]}

    info = {'linuxdeploy': {'release': release, 'sha256': pin}, 'daemonRestoredAfterLinuxdeploy': True, 'daemonHashChain': daemon_chain, 'appimagetoolSha256': tool_pin,
            'gtkPlugin': {'source': str(plugin_source), 'wailsModuleDir': str(wails_dir), 'sha256': sha256(plugin)},
            'webkitHelperDirectory': str(helper_dir), 'updateMarker': bool(marker),
            'sharedMimeCache': {'source': str(mime_cache), 'sha256': sha256(mime_cache)}, 'gioModules': gio_modules_info, 'libdbusRemoved': removed_dbus}
    info['relocation'] = relocate_webkit(appdir, helper_dir) if relocate else 'DISABLED (negative control)'
    info['inspection'] = inspect_appdir(appdir, helper_dir)
    ins = info['inspection']
    problems = []
    if not (ins['webkit'] and ins['gtk3'] and ins['glib'] and ins['gio']):
        problems.append('GTK3/WebKitGTK/GLib not bundled')
    if len(ins['helpers']) < 2 or not ins['injectedBundle']:
        problems.append('WebKit helper processes or injected bundle missing')
    if ins['hostOnlyLibrariesFound']:
        problems.append(f"host driver libraries bundled: {ins['hostOnlyLibrariesFound']}")
    if not ins['gioModuleDirs']:
        problems.append('no bundled GIO module directory for GIO_MODULE_DIR')
    if ins['gioModules'] != (sorted(GIO_MODULES) if tls_module else []):
        problems.append(f"bundled GIO modules are {ins['gioModules']}, expected exactly {sorted(GIO_MODULES) if tls_module else []}")
    if problems:
        sys.exit('AppDir inspection failed: ' + '; '.join(problems))
    image = out / name
    run([str(appimagetool), '--appimage-extract-and-run', '--no-appstream', '--runtime-file', str(runtime), str(appdir), str(image)], env=dict(os.environ, ARCH=ARCH[arch][1]))
    info['runtime'] = {'revision': RUNTIME_REVISION, 'source': f'{RUNTIME_BASE}/{asset}', 'fileSha256': runtime_sha, 'fileBytes': runtime.stat().st_size,
                       'elfMachine': machine, 'embedded': verify_embedded_runtime(image, runtime, arch)}
    return image, info


def read_daemon_evidence(path, daemon):
    """Parse linux/build_daemon_release.sh's build-evidence.txt and check it describes THIS file."""
    lines = path.read_text().splitlines()
    fields = dict(l.split('=', 1) for l in lines if re.match(r'^[a-z_]+=', l))
    digest = next((l.split()[0] for l in lines if re.match(r'^[0-9a-f]{64}\s', l)), None)
    engine = [l for l in lines if l.startswith('name = "uc-engine"') or l.startswith('version = ') or l.startswith('source = ')]
    if digest != sha256(daemon):
        sys.exit(f'{path} describes a different binary (evidence {digest}, file {sha256(daemon)})')
    if fields.get('daemon_source_dirty') != 'false':
        sys.exit(f'{path}: the daemon was built from a dirty tree; an immutable build is required')
    if 'release' not in fields.get('build_mode', ''):
        sys.exit(f'{path}: not a release build')
    # The daemon was built at fields['head']; the packaging commit may be later. Every input of the daemon build must be unchanged.
    changed = subprocess.run(['git', 'diff', '--name-only', fields.get('head', ''), 'HEAD', '--', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'crates', 'apps/daemon'],
                             cwd=ROOT, capture_output=True, text=True)
    if changed.returncode != 0 or changed.stdout.strip():
        sys.exit(f'daemon sources changed between the build evidence head and HEAD (or the head is unknown): {changed.stdout.strip() or changed.stderr.strip()}')
    return {'head': fields.get('head'), 'daemonInputsUnchangedUntilHead': True, 'buildMode': fields.get('build_mode'), 'command': fields.get('command'),
            'engineLockEntry': engine[:3], 'evidenceFile': str(path), 'evidenceSha256': sha256(path)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--arch', choices=sorted(ARCH), required=True)
    parser.add_argument('--daemon', type=Path, required=True)
    parser.add_argument('--daemon-evidence', type=Path, help='build-evidence.txt written by linux/build_daemon_release.sh; required unless --packaging-check-fixture')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--packaging-check-fixture', action='store_true',
                        help='PACKAGING CHECK ONLY: accept a placeholder daemon; outputs are marked fixture, prefixed FIXTURE- and are not a product')
    parser.add_argument('--gui-binary', type=Path, help='package this prebuilt GUI (e.g. the gtk3,e2e build) instead of building the release one; marks the output E2E')
    parser.add_argument('--appimage-only', action='store_true', help='build only the AppImage and its updater archive (no deb/rpm)')
    parser.add_argument('--update-marker', help='write this text to usr/share/uniclipboard/update-marker.txt inside the AppImage (update E2E: tells v2 from v1)')
    parser.add_argument('--negative-control-no-tls-module', action='store_true',
                        help='NEGATIVE CONTROL (17c7): leave the bundled GIO module directory empty (what 17c4-17c6 shipped); HTTPS in the WebView must fail; prefixed NEGTLS-')
    parser.add_argument('--negative-control-no-relocation', action='store_true',
                        help='NEGATIVE CONTROL: skip the WebKit helper relocation; the result must not start without a host WebKitGTK; prefixed NEGCONTROL-')
    parser.add_argument('--tools-dir', type=Path, help='keep the downloaded, SHA-256-verified tools here (default: a throwaway directory in --out)')
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('package_linux.py must run on Linux (cgo against GTK3/WebKitGTK); use e2e/linux/Dockerfile')
    if not args.gui_binary and not (GUI / 'frontend/dist').is_dir():
        sys.exit('apps/gui-go/frontend/dist is missing: build the frontend once (bun --bun run --cwd apps/gui-go build)')
    if not args.daemon.is_file():
        sys.exit(f'{args.daemon} not found: a package without the daemon cannot start')
    ok, reason = check_daemon(args.daemon, args.arch)
    if not ok and not args.packaging_check_fixture:
        sys.exit(f'{args.daemon} is not a valid {args.arch} ELF executable: {reason}')
    fixture = args.packaging_check_fixture
    evidence = None
    if not fixture:
        if not args.daemon_evidence:
            sys.exit('--daemon-evidence is required: a daemon of unverified origin is not packaged (use --packaging-check-fixture for a marked check build)')
        evidence = read_daemon_evidence(args.daemon_evidence, args.daemon)
    if args.negative_control_no_relocation and args.negative_control_no_tls_module:
        sys.exit('pick one negative control')
    prefix = ('FIXTURE-' if fixture else 'NEGCONTROL-' if args.negative_control_no_relocation else 'NEGTLS-' if args.negative_control_no_tls_module
              else 'E2E-' if args.gui_binary else '')
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f'{out} is not empty: pick a new directory, earlier artifacts are not overwritten')
    out.mkdir(parents=True, exist_ok=True)
    conf = json.loads((ROOT / 'apps/gui/src-tauri/tauri.conf.json').read_text())
    product, version, ident = conf['productName'], conf['version'], conf['identifier']
    pubkey = conf['plugins']['updater']['pubkey']
    prov = provenance()

    if args.gui_binary:
        binary = out / 'uniclipboard'
        shutil.copy2(args.gui_binary, binary)
        info = {k: (Path(str(args.gui_binary) + '.' + k).read_text().strip() if Path(str(args.gui_binary) + '.' + k).exists() else None) for k in ('head', 'tags', 'sha256')}
        # build_release_e2e_in_container.sh wrote these next to the binary; the hash must be the binary's own.
        if info['sha256'] and info['sha256'].split()[0] != sha256(binary):
            sys.exit(f'{args.gui_binary}.sha256 does not match the file')
        tags = f"prebuilt (--gui-binary): tags={info['tags']}, built at head={info['head']}, sha256={sha256(binary)}"
    else:
        run(['go', 'generate', './buildinfo'], cwd=ROOT / 'packages/desktop-host-go')
        (GUI / 'assets').mkdir(exist_ok=True)
        shutil.copy2(ROOT / 'apps/gui/src-tauri/icons/tray-icon@2x.png', GUI / 'assets/tray-icon@2x.png')
        binary, tags = out / 'uniclipboard', 'gtk3,production,release'
        ldflags = f'-w -s -X main.updaterPublicKey={pubkey} -X main.productName={product} -X main.bundleID={ident}'
        run(['go', 'build', '-tags', tags, '-trimpath', '-buildvcs=false', '-ldflags', ldflags, '-o', str(binary), '.'],
            cwd=GUI, env=dict(os.environ, CGO_ENABLED='1', GOARCH=args.arch))

    stage = out / 'stage'
    stage_tree(stage, binary, args.daemon)
    deb_name, rpm_arch = ARCH[args.arch]
    outputs, extra = [binary], {}
    if not args.appimage_only:
        deb = build_deb(stage, out, version, args.arch, f'{prefix}{product}_{version}_{deb_name}.deb')
        rpm = build_rpm(stage, out, version, args.arch, f'{prefix}{product}-{version}-1.{rpm_arch}.rpm')
        outputs += [deb, rpm]
        extra['deb'] = run(['dpkg-deb', '-I', str(deb)], capture=True)
        extra['rpmRequires'] = run(['rpm', '-qpR', str(rpm)], capture=True)
        extra['rpmFiles'] = run(['rpm', '-qpl', str(rpm)], capture=True)
    tools = args.tools_dir.resolve() if args.tools_dir else out / 'tools'
    tools.mkdir(parents=True, exist_ok=True)
    image, appimage = build_appimage(stage, out, args.arch, f'{prefix}{product}_{version}_{deb_name}.AppImage', tools, args.daemon,
                                     relocate=not args.negative_control_no_relocation, marker=args.update_marker,
                                     tls_module=not args.negative_control_no_tls_module)
    archive = out / f'{image.name}.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        tar.add(image, arcname=image.name)
    outputs += [image, archive]

    daemon = {'kind': 'fixture' if fixture else 'release-build-with-evidence', 'bytes': args.daemon.stat().st_size, 'sha256': sha256(args.daemon),
              'elfValid': ok, 'elfCheck': reason, 'executedHere': False}
    if evidence:
        daemon['buildEvidence'] = evidence
        daemon['identityVerified'] = 'sha256 equals the build evidence of a locked release build of uc-daemon (the binary is never executed here: `uniclipd --version` was observed to start the daemon instead of printing a version); the GUI-to-daemon handshake is checked by e2e/linux_appimage_run.py'
    else:
        daemon['identityVerified'] = False
        daemon['note'] = 'placeholder daemon'
    (out / 'package-manifest.json').write_text(json.dumps({
        'source': prov, 'arch': args.arch, 'version': version, 'tags': tags, 'go': run(['go', 'version'], capture=True),
        'purpose': 'packaging-check' if fixture else ('negative-control' if (args.negative_control_no_relocation or args.negative_control_no_tls_module) else ('e2e-package' if args.gui_binary else 'package')),
        'productionUsable': False, 'daemon': daemon, 'appimage': appimage, 'extra': extra,
        'sha256': {p.name: sha256(p) for p in outputs},
        'signed': False, 'nativeDesktopVerified': False, 'appImageRunProven': False,
        'note': ('FIXTURE: placeholder daemon. ' if fixture else '') + 'AppDir inspected at build time; whether it runs without host GTK/WebKitGTK is shown only by e2e/linux_appimage_run.py. '
                'Not signed (the release workflow signs the updater archive). If source.dirty is true the artifacts contain uncommitted changes.'}, indent=2) + '\n')
    for leftover in ('stage', 'deb-root', 'rpmbuild', 'UniClipboard.AppDir'):
        shutil.rmtree(out / leftover, ignore_errors=True)
    if not args.tools_dir:
        shutil.rmtree(tools, ignore_errors=True)
    print('built', *[p.name for p in outputs])


if __name__ == '__main__':
    main()
