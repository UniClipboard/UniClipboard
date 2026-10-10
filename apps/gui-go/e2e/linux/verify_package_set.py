#!/usr/bin/env python3
"""Acceptance contract for one architecture's Linux package set (docs/architecture/gui-go-linux-ci-packaging.md, C1-C5, C10).

  verify_package_set.py --arch amd64|arm64 --packages <package_linux.py --out dir> --daemon-evidence <build-evidence.txt>
                        --expect-head <sha> --upload-dir <new dir> [--max-glibc 2.36] [--report report.json]

Reads what package_linux.py wrote and fails unless:
  * the shared release collector (scripts/collect-release-assets.py, the one definition of "named distributable") picks up
    EXACTLY the four expected files from the output directory, so a name that the release flow would silently drop is an error;
  * those four files, and only those, are copied to --upload-dir (raw binary, manifest, prefixed test packages never are);
  * package-manifest.json describes this checkout: source head equals --expect-head, the tree was clean, the daemon evidence
    hash is the hash of the evidence file given here, and the per-file SHA-256 values match the files;
  * the packages are for the requested architecture (deb/rpm metadata, ELF e_machine of the shipped executables) and carry
    the same daemon the evidence describes;
  * no ELF inside the deb or the AppImage needs a newer GLIBC than --max-glibc.
It runs the AppImage only with --appimage-extract (no GUI), so it must run on a host of the same architecture.
"""
import argparse
import hashlib
import importlib.util
import json
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'apps/gui-go/e2e'))
from package_linux import PACKAGE_NAME  # noqa: E402  (the one definition of the package identity)
ARCH = {'amd64': {'deb': 'amd64', 'rpm': 'x86_64', 'appimage': 'amd64', 'machine': 62, 'uname': 'x86_64'},
        'arm64': {'deb': 'arm64', 'rpm': 'aarch64', 'appimage': 'aarch64', 'machine': 183, 'uname': 'aarch64'}}


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def sh(cmd, **kw):
    return subprocess.run(cmd, check=True, text=True, capture_output=True, **kw).stdout.strip()


def load_collector():
    spec = importlib.util.spec_from_file_location('collect_release_assets', ROOT / 'scripts/collect-release-assets.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def e_machine(path):
    head = Path(path).read_bytes()[:20]
    return int.from_bytes(head[18:20], 'little') if head[:4] == b'\x7fELF' else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--arch', choices=sorted(ARCH), required=True)
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--daemon-evidence', type=Path, required=True)
    parser.add_argument('--expect-head', required=True)
    parser.add_argument('--upload-dir', type=Path, required=True)
    parser.add_argument('--max-glibc', default='2.36')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    a = ARCH[args.arch]
    problems, report = [], {'arch': args.arch}

    if platform.machine() != a['uname']:
        sys.exit(f"host is {platform.machine()}, not {a['uname']}: the package set is verified on a host of its own architecture")
    version = json.loads((ROOT / 'apps/gui-go/app.json').read_text())['version']
    expected = {f'UniClipboard_{version}_{a["deb"]}.deb', f'UniClipboard-{version}-1.{a["rpm"]}.rpm',
                f'UniClipboard_{version}_{a["appimage"]}.AppImage', f'UniClipboard_{version}_{a["appimage"]}.AppImage.tar.gz'}

    # C1/C2: the collector decides what is a named distributable.
    if args.upload_dir.exists():
        sys.exit(f'{args.upload_dir} exists: the upload directory is created by this command')
    collected = sorted(load_collector().collect(args.packages, args.upload_dir))
    report['collected'] = collected
    if set(collected) != expected or len(collected) != 4:
        problems.append(f'the release collector picks {collected}, expected exactly {sorted(expected)}')
    for stray in sorted(p.name for p in args.upload_dir.iterdir() if p.name not in expected):
        problems.append(f'unexpected file in the upload set: {stray}')

    # C3/C4/C10: the manifest describes this checkout and this daemon.
    manifest = json.loads((args.packages / 'package-manifest.json').read_text())
    src, daemon = manifest['source'], manifest['daemon']
    if manifest.get('arch') != args.arch:
        problems.append(f"manifest arch {manifest.get('arch')} != {args.arch}")
    if manifest.get('purpose') != 'package':
        problems.append(f"manifest purpose {manifest.get('purpose')!r} is not a plain release package")
    if src.get('head') != args.expect_head:
        problems.append(f"manifest source head {src.get('head')} != expected {args.expect_head}")
    if src.get('dirty') is not False or src.get('immutable') is not True:
        problems.append(f'the source tree was not clean when packaging: {src}')
    if daemon.get('kind') != 'release-build-with-evidence':
        problems.append(f"daemon kind {daemon.get('kind')!r}")
    if daemon.get('buildEvidence', {}).get('evidenceSha256') != sha256(args.daemon_evidence):
        problems.append('manifest daemon.buildEvidence.evidenceSha256 is not the SHA-256 of the evidence file given')
    for name in expected:
        if manifest['sha256'].get(name) != sha256(args.upload_dir / name) if (args.upload_dir / name).is_file() else True:
            problems.append(f'manifest SHA-256 for {name} is missing or differs from the file')

    # Package metadata and shipped executables, per architecture.
    work = Path(tempfile.mkdtemp(prefix='verify-package-set-'))
    deb = args.upload_dir / f'UniClipboard_{version}_{a["deb"]}.deb'
    rpm = args.upload_dir / f'UniClipboard-{version}-1.{a["rpm"]}.rpm'
    image = args.upload_dir / f'UniClipboard_{version}_{a["appimage"]}.AppImage'
    archive = args.upload_dir / f'{image.name}.tar.gz'
    if deb.is_file():
        # Same convention as the rpm below: the control Version spells the pre-release separator "~" (dpkg sorts it before the stable
        # release); the file name keeps the release version.
        control = sh(['dpkg-deb', '-f', str(deb), 'Package', 'Version', 'Architecture'])
        report['debControl'] = control
        if f'Package: {PACKAGE_NAME}\n' not in control + '\n' or f'Architecture: {a["deb"]}' not in control or f'Version: {version.replace("-", "~", 1)}' not in control:
            problems.append(f'deb control does not match {a["deb"]} {version}: {control}')
        sh(['dpkg-deb', '-x', str(deb), str(work / 'deb')])
        for exe in ('usr/bin/uniclipboard', 'usr/bin/uniclipd'):
            if e_machine(work / 'deb' / exe) != a['machine']:
                problems.append(f'deb {exe} is not an ELF for {args.arch}')
        daemon_in_deb = sha256(work / 'deb/usr/bin/uniclipd')
        if daemon_in_deb != daemon.get('sha256'):
            problems.append(f'deb daemon {daemon_in_deb} != evidence daemon {daemon.get("sha256")}')
    if rpm.is_file():
        header = sh(['rpm', '-qp', '--qf', '%{NAME} %{VERSION} %{RELEASE} %{ARCH}', str(rpm)])
        report['rpmHeader'] = header
        # A pre-release separator is "~" in the rpm Version tag (package_linux.py build_rpm); the file name keeps the release version.
        if header != f'{PACKAGE_NAME} {version.replace("-", "~", 1)} 1 {a["rpm"]}':
            problems.append(f'rpm header {header!r} does not match {a["rpm"]} {version}')
        (work / 'rpm').mkdir()
        subprocess.run(f'rpm2cpio {rpm} | cpio -idm --quiet', shell=True, cwd=work / 'rpm', check=True)
        if sha256(work / 'rpm/usr/bin/uniclipd') != daemon.get('sha256'):
            problems.append('rpm daemon is not the evidence daemon')
    if image.is_file():
        image.chmod(0o755)
        shutil.copy2(image, work / 'x.AppImage')
        subprocess.run([str(work / 'x.AppImage'), '--appimage-extract'], cwd=work, check=True, capture_output=True)
        if sha256(work / 'squashfs-root/usr/bin/uniclipd') != daemon.get('sha256'):
            problems.append('AppImage daemon is not the evidence daemon')
        if e_machine(work / 'squashfs-root/usr/bin/uniclipboard') != a['machine']:
            problems.append(f'AppImage GUI executable is not an ELF for {args.arch}')
    if archive.is_file() and image.is_file():
        with tarfile.open(archive) as tar:
            names = tar.getnames()
            inner = tar.extractfile(names[0]).read() if len(names) == 1 else b''
        if names != [image.name] or hashlib.sha256(inner).hexdigest() != sha256(image):
            problems.append(f'the updater archive must hold exactly {image.name} with the same bytes, holds {names}')

    # C5: glibc floor of what the packages ship.
    floors = {}
    for label, tree in (('deb', work / 'deb'), ('appimage', work / 'squashfs-root')):
        if tree.is_dir():
            r = subprocess.run([sys.executable, str(Path(__file__).with_name('elf_floor.py')), str(tree), '--max-glibc', args.max_glibc],
                               capture_output=True, text=True)
            floors[label] = json.loads(r.stdout)['floor'] if r.stdout.strip() else None
            if r.returncode != 0:
                problems.append(f'{label}: {r.stderr.strip()}')
    report['glibcFloor'] = floors
    report['problems'] = problems
    shutil.rmtree(work, ignore_errors=True)
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    sys.exit(1 if problems else 0)


if __name__ == '__main__':
    main()
