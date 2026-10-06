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
  UniClipboard_<version>_<amd64|aarch64>.AppImage    AppDir built with appimagetool (host GTK3/WebKitGTK, see below)
  UniClipboard_<version>_<amd64|aarch64>.AppImage.tar.gz   the updater artifact (the release workflow signs it: `.sig`)
  package-manifest.json                              provenance and an explicit list of what is NOT proven

What this proves: the artifacts build. It does NOT prove they install or run on a real desktop, and:
  * the AppImage is NOT self-contained: the Tauri AppImage bundles its libraries with linuxdeploy (and a pinned
    plugin, docs/architecture/linux-appimage-library-policy.md); this one relies on the host's GTK3 and WebKitGTK 4.1.
    Reproducing the Tauri library policy for the Go binary is open work;
  * nothing is signed (no minisign `.sig`): signing stays in the release workflow;
  * with --packaging-check-fixture the daemon is a placeholder and every output is prefixed FIXTURE-.
"""
import argparse
import hashlib
import json
import os
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
APPIMAGETOOL = {  # a fixed release tag (the continuous builds move); the downloaded file's SHA-256 goes into the manifest
    'amd64': ('https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-x86_64.AppImage', None),
    'arm64': ('https://github.com/AppImage/appimagetool/releases/download/1.9.0/appimagetool-aarch64.AppImage', None),
}
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
    # Only GTK3/WebKitGTK/X11. The Tauri deb also needs libayatana-appindicator3 and libgtk-layer-shell0: Wails' tray is
    # StatusNotifierItem over D-Bus, and the Layer Shell panel (L5) is not implemented yet, so its dependency is
    # decided together with that work.
    (d / 'DEBIAN/control').write_text(
        f'Package: uniclipboard\nVersion: {version}\nArchitecture: {ARCH[arch][0]}\nSection: utils\nPriority: optional\n'
        f'Installed-Size: {size_kib}\nMaintainer: UniClipboard <support@uniclipboard.app>\n'
        'Depends: libgtk-3-0, libwebkit2gtk-4.1-0, libx11-6\n'
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
        'License: Proprietary\nRequires: gtk3, webkit2gtk4.1\nAutoReqProv: no\n\n%%description\n'
        'Encrypted peer-to-peer clipboard sync between your devices.\n\n%%install\ncp -a %s/. %%{buildroot}/\n\n%%files\n%s\n'
        % (version, stage, '\n'.join(files)))
    run(['rpmbuild', '-bb', '--define', f'_topdir {top}', '--define', f'_rpmfilename {name}', '--target', ARCH[arch][1] + '-linux', str(spec)])
    produced = next((top / 'RPMS').rglob('*.rpm'))
    rpm = out / name
    shutil.copy2(produced, rpm)
    return rpm


def build_appimage(stage, out, arch, name, tools):
    appdir = out / 'UniClipboard.AppDir'
    shutil.copytree(stage, appdir, symlinks=True)
    (appdir / 'AppRun').symlink_to('usr/bin/uniclipboard')
    shutil.copy2(ROOT / 'packaging/linux/uniclipboard.desktop', appdir / 'uniclipboard.desktop')
    shutil.copy2(ROOT / 'apps/gui/src-tauri/icons/128x128.png', appdir / 'uniclipboard.png')
    url, _ = APPIMAGETOOL[arch]
    tool = tools / 'appimagetool'
    tools.mkdir(exist_ok=True)
    urllib.request.urlretrieve(url, tool)
    tool.chmod(0o755)
    image = out / name
    # --appimage-extract-and-run: the build container has no FUSE (the same setting the Tauri CI uses).
    run([str(tool), '--appimage-extract-and-run', '--no-appstream', str(appdir), str(image)], env=dict(os.environ, ARCH=ARCH[arch][1]))
    return image, sha256(tool)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--arch', choices=sorted(ARCH), required=True)
    parser.add_argument('--daemon', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--packaging-check-fixture', action='store_true',
                        help='PACKAGING CHECK ONLY: accept a placeholder daemon; outputs are marked fixture, prefixed FIXTURE- and are not a product')
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('package_linux.py must run on Linux (cgo against GTK3/WebKitGTK); use e2e/linux/Dockerfile')
    if not (GUI / 'frontend/dist').is_dir():
        sys.exit('apps/gui-go/frontend/dist is missing: build the frontend once (bun --bun run --cwd apps/gui-go build)')
    if not args.daemon.is_file():
        sys.exit(f'{args.daemon} not found: a package without the daemon cannot start')
    ok, reason = check_daemon(args.daemon, args.arch)
    if not ok and not args.packaging_check_fixture:
        sys.exit(f'{args.daemon} is not a valid {args.arch} ELF executable: {reason}')
    fixture = args.packaging_check_fixture
    prefix = 'FIXTURE-' if fixture else ''
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f'{out} is not empty: pick a new directory, earlier artifacts are not overwritten')
    out.mkdir(parents=True, exist_ok=True)
    conf = json.loads((ROOT / 'apps/gui/src-tauri/tauri.conf.json').read_text())
    product, version, ident = conf['productName'], conf['version'], conf['identifier']
    pubkey = conf['plugins']['updater']['pubkey']
    prov = provenance()

    run(['go', 'generate', './buildinfo'], cwd=ROOT / 'packages/desktop-host-go')
    (GUI / 'assets').mkdir(exist_ok=True)
    shutil.copy2(ROOT / 'apps/gui/src-tauri/icons/tray-icon@2x.png', GUI / 'assets/tray-icon@2x.png')
    binary = out / 'uniclipboard'
    ldflags = f'-w -s -X main.updaterPublicKey={pubkey} -X main.productName={product} -X main.bundleID={ident}'
    run(['go', 'build', '-tags', 'gtk3,production,release', '-trimpath', '-buildvcs=false', '-ldflags', ldflags, '-o', str(binary), '.'],
        cwd=GUI, env=dict(os.environ, CGO_ENABLED='1', GOARCH=args.arch))

    stage = out / 'stage'
    stage_tree(stage, binary, args.daemon)
    deb_name, rpm_arch = ARCH[args.arch]
    deb = build_deb(stage, out, version, args.arch, f'{prefix}{product}_{version}_{deb_name}.deb')
    rpm = build_rpm(stage, out, version, args.arch, f'{prefix}{product}-{version}-1.{rpm_arch}.rpm')
    image, tool_sha = build_appimage(stage, out, args.arch, f'{prefix}{product}_{version}_{deb_name}.AppImage', out / 'tools')
    archive = out / f'{image.name}.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        tar.add(image, arcname=image.name)

    outputs = [binary, deb, rpm, image, archive]
    (out / 'package-manifest.json').write_text(json.dumps({
        'source': prov, 'arch': args.arch, 'version': version, 'tags': 'gtk3,production,release',
        'go': run(['go', 'version'], capture=True), 'appimagetoolSha256': tool_sha,
        'purpose': 'packaging-check' if fixture else 'package', 'productionUsable': False,
        'daemon': {'kind': 'fixture' if fixture else 'supplied-unverified-origin', 'bytes': args.daemon.stat().st_size, 'sha256': sha256(args.daemon),
                   'elfValid': ok, 'elfCheck': reason, 'identityVerified': False, 'runsVerified': False,
                   'note': 'ELF structure and architecture only (debug/elf). Not shown to be the Rust daemon; origin not verified.'},
        'sha256': {p.name: sha256(p) for p in outputs},
        'signed': False, 'selfContainedAppImage': False, 'nativeDesktopVerified': False,
        'note': ('FIXTURE: placeholder daemon. ' if fixture else '') + 'Built only; not installed or run on a real Linux desktop; not signed; AppImage relies on host GTK3/WebKitGTK. If source.dirty is true the artifacts contain uncommitted changes.'}, indent=2) + '\n')
    shutil.rmtree(stage)
    shutil.rmtree(out / 'deb-root')
    shutil.rmtree(out / 'rpmbuild')
    shutil.rmtree(out / 'UniClipboard.AppDir')
    shutil.rmtree(out / 'tools')
    print('built', *[p.name for p in outputs])


if __name__ == '__main__':
    main()
