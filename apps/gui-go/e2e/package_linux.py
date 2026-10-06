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
# The linuxdeploy pin has one source of truth, shared with the Tauri bundle: scripts/linux-appimage-tools.mjs.
LINUXDEPLOY_PIN_FILE = ROOT / 'scripts/linux-appimage-tools.mjs'
LINUXDEPLOY_BASE = 'https://github.com/tauri-apps/binary-releases/releases/download'
# Libraries that belong to the host's driver stack and must never be inside the AppImage (policy document).
HOST_ONLY_LIBS = ('libwayland-client.so', 'libEGL.so', 'libGL.so', 'libGLX.so', 'libGLdispatch.so', 'libdrm.so', 'libgbm.so', 'libvulkan.so')
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


def fetch_verified(url, sha, dest):
    """Download once into the tools directory and refuse anything whose SHA-256 differs from the pin."""
    if not dest.exists() or sha256(dest) != sha:
        part = dest.with_name(dest.name + '.partial')
        urllib.request.urlretrieve(url, part)
        if sha256(part) != sha:
            got = sha256(part)
            part.unlink()
            sys.exit(f'{url}: SHA-256 {got} differs from the pinned {sha}')
        part.rename(dest)
    dest.chmod(0o755)
    return dest


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
        'gioModuleDirs': sorted(str(p.relative_to(appdir)) for p in appdir.rglob('modules') if p.parent.name == 'gio'),
        'hooks': sorted(p.name for p in (appdir / 'apprun-hooks').glob('*')) if (appdir / 'apprun-hooks').is_dir() else [],
        'fileCount': len(files),
    }


def build_appimage(stage, out, arch, name, tools, daemon, relocate=True, marker=None):
    tools.mkdir(exist_ok=True)
    tool_url, tool_pin = APPIMAGETOOL[arch]
    appimagetool = fetch_verified(tool_url, tool_pin, tools / 'appimagetool')
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
    # The pinned Wails GTK plugin deploys no GIO modules. An empty bundled directory is what AppRun's GIO_MODULE_DIR points
    # at, so the bundled GLib never loads the host's (ABI-incompatible) gvfs/dconf modules.
    (appdir / 'usr/lib/gio/modules').mkdir(parents=True)
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
            'webkitHelperDirectory': str(helper_dir), 'updateMarker': bool(marker)}
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
    if problems:
        sys.exit('AppDir inspection failed: ' + '; '.join(problems))
    image = out / name
    run([str(appimagetool), '--appimage-extract-and-run', '--no-appstream', str(appdir), str(image)], env=dict(os.environ, ARCH=ARCH[arch][1]))
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
    prefix = 'FIXTURE-' if fixture else ('NEGCONTROL-' if args.negative_control_no_relocation else ('E2E-' if args.gui_binary else ''))
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
                                     relocate=not args.negative_control_no_relocation, marker=args.update_marker)
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
        'purpose': 'packaging-check' if fixture else ('negative-control' if args.negative_control_no_relocation else ('e2e-package' if args.gui_binary else 'package')),
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
