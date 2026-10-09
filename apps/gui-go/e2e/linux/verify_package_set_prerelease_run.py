#!/usr/bin/env python3
"""`verify_package_set.py` over a pre-release package set whose payload is a REAL CI package set.

Failure model: the deb control Version of a pre-release is `X.Y.Z~alpha.N` while the file name keeps `X.Y.Z-alpha.N`; a check that
compares the two spellings for equality would fail every pre-release Linux packaging job (the rpm side already allowed for it).

Inputs (read-only): a real package set of one architecture, downloaded from an earlier CI run of package-linux-gui.yml
  --real-packages  the `linux-gui-<arch>` artifact (deb, rpm, AppImage, updater archive)
  --real-evidence  the `linux-gui-evidence-<arch>` artifact (package-manifest.json, build-evidence.txt)
Everything that carries the version is then REBUILT with the shipped `build_deb` / `build_rpm` for the pre-release version (the
payload, daemon and GUI executable stay the real bytes); the AppImage and its updater archive are renamed byte for byte. The
manifest copy only gets the new version and file hashes. That makes the result a DERIVED set, recorded as such in derived.json.
The verifier is then run from a scratch tree whose app.json says the pre-release version, exactly as in the workflow, and must
accept it; a deb whose Version keeps the dash must be rejected.

Host mode starts uc-package-build:bookworm (native arch of the artifact, dpkg-deb + rpmbuild) and runs this file with --inside.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[4]
VERSION = '1.3.0-alpha.1'


def sha256(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def sh(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    return r


def inside(args):
    out = Path('/out')
    real, evid, repo = Path('/real'), Path('/evidence'), Path('/repo')
    arch = 'arm64' if os.uname().machine == 'aarch64' else 'amd64'
    deb_arch, rpm_arch, ai_arch = ('arm64', 'aarch64', 'aarch64') if arch == 'arm64' else ('amd64', 'x86_64', 'amd64')
    scratch = out / 'scratch'
    for rel in ('apps/gui-go/e2e/linux/verify_package_set.py', 'apps/gui-go/e2e/linux/elf_floor.py', 'apps/gui-go/e2e/package_linux.py',
                'scripts/collect-release-assets.py'):
        (scratch / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(repo / rel, scratch / rel)
    app = json.loads((repo / 'apps/gui-go/app.json').read_text())
    real_version = app['version']
    real_manifest = json.loads((evid / 'package-manifest.json').read_text())
    assert real_manifest['version'] != VERSION
    (scratch / 'apps/gui-go').mkdir(parents=True, exist_ok=True)
    (scratch / 'apps/gui-go/app.json').write_text(json.dumps(dict(app, version=VERSION)))
    sys.path.insert(0, str(scratch / 'apps/gui-go/e2e'))
    import package_linux

    real_ver = real_manifest['version']
    real_deb = next(real.glob(f'UniClipboard_{real_ver}_{deb_arch}.deb'))
    stage = out / 'stage'
    assert sh(['dpkg-deb', '-x', str(real_deb), str(stage)]).returncode == 0
    packages, build = out / 'packages', out / 'build'
    packages.mkdir()
    build.mkdir()
    deb = package_linux.build_deb(stage, build, VERSION, arch, f'UniClipboard_{VERSION}_{deb_arch}.deb')
    rpm = package_linux.build_rpm(stage, build, VERSION, arch, f'UniClipboard-{VERSION}-1.{rpm_arch}.rpm')
    deb, rpm = (Path(shutil.copy2(f, packages / f.name)) for f in (deb, rpm))
    image = packages / f'UniClipboard_{VERSION}_{ai_arch}.AppImage'
    shutil.copy2(real / f'UniClipboard_{real_ver}_{ai_arch}.AppImage', image)
    with tarfile.open(packages / (image.name + '.tar.gz'), 'w:gz') as t:
        t.add(image, arcname=image.name)
    # the raw GUI binary the manifest also lists stays as it was
    for extra in real.iterdir():
        if extra.is_file() and not extra.name.startswith('UniClipboard') and extra.name not in {p.name for p in packages.iterdir()}:
            shutil.copy2(extra, packages / extra.name)
    manifest = json.loads(json.dumps(real_manifest))
    manifest['version'] = VERSION
    old = manifest['sha256']
    manifest['sha256'] = {k: v for k, v in old.items() if not k.startswith('UniClipboard')}
    for f in packages.iterdir():
        if f.name.startswith('UniClipboard'):
            manifest['sha256'][f.name] = sha256(f)
    (packages / 'package-manifest.json').write_text(json.dumps(manifest, indent=2))
    derived = {'derivedFrom': {'run': args.run, 'artifacts': ['linux-gui-' + arch, 'linux-gui-evidence-' + arch], 'version': real_ver,
                               'sourceHead': real_manifest['source']['head']},
               'prereleaseVersion': VERSION, 'rebuilt': [deb.name, rpm.name], 'renamedUnchanged': [image.name, image.name + '.tar.gz'],
               'manifestEdits': 'version and sha256 of the four renamed/rebuilt files only',
               'debControlVersion': sh(['dpkg-deb', '-f', str(deb), 'Version']).stdout.strip(),
               'rpmHeader': sh(['rpm', '-qp', '--qf', '%{NAME} %{VERSION} %{RELEASE} %{ARCH}', str(rpm)]).stdout.strip(),
               'daemonSha256': {'evidence': real_manifest['daemon']['sha256'], 'inDeb': sha256(stage / 'usr/bin/uniclipd')}}
    (out / 'derived.json').write_text(json.dumps(derived, indent=2) + '\n')
    verifier = scratch / 'apps/gui-go/e2e/linux/verify_package_set.py'
    head = real_manifest['source']['head']
    cmd = [sys.executable, str(verifier), '--arch', arch, '--packages', str(packages), '--daemon-evidence', str(evid / 'build-evidence.txt'),
           '--expect-head', head]
    ok = sh(cmd + ['--upload-dir', str(out / 'upload-ok'), '--report', str(out / 'verify-report.json')])
    (out / 'verify.stdout.txt').write_text(ok.stdout + ok.stderr)
    # negative: the deb keeps the dash in its control Version
    bad_dir = out / 'packages-dash'
    shutil.copytree(packages, bad_dir)
    control = out / 'bad-deb'
    sh(['dpkg-deb', '-R', str(deb), str(control)])
    text = (control / 'DEBIAN/control').read_text().replace(f'Version: {VERSION.replace("-", "~", 1)}', f'Version: {VERSION}')
    (control / 'DEBIAN/control').write_text(text)
    (bad_dir / deb.name).unlink()
    sh(['dpkg-deb', '--root-owner-group', '--build', str(control), str(bad_dir / deb.name)])
    m = json.loads((bad_dir / 'package-manifest.json').read_text())
    m['sha256'][deb.name] = sha256(bad_dir / deb.name)
    (bad_dir / 'package-manifest.json').write_text(json.dumps(m))
    bad = sh(cmd[:5] + [str(bad_dir)] + cmd[6:] + ['--upload-dir', str(out / 'upload-bad')])
    (out / 'verify-dash.stdout.txt').write_text(bad.stdout + bad.stderr)
    result = {'verifyAcceptsTildeDeb': ok.returncode == 0, 'verifyRejectsDashDeb': bad.returncode != 0 and 'deb control does not match' in bad.stdout,
              'okExit': ok.returncode, 'dashExit': bad.returncode}
    (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    sys.exit(0 if all(v is True for k, v in result.items() if k.startswith('verify')) else 1)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--real-packages', type=Path)
    p.add_argument('--real-evidence', type=Path)
    p.add_argument('--run', default='37894007183')
    p.add_argument('--out', type=Path)
    p.add_argument('--image', default='uc-package-build:bookworm')
    p.add_argument('--inside', action='store_true')
    a = p.parse_args()
    if a.inside:
        inside(a)
        return
    a.out.mkdir(parents=True, exist_ok=False)
    r = subprocess.run(['docker', 'run', '--rm', '-v', f'{ROOT}:/repo:ro', '-v', f'{a.real_packages.resolve()}:/real:ro',
                        '-v', f'{a.real_evidence.resolve()}:/evidence:ro', '-v', f'{a.out.resolve()}:/out', a.image,
                        'python3', '-I', '/repo/apps/gui-go/e2e/linux/verify_package_set_prerelease_run.py', '--inside', '--run', a.run])
    sys.exit(r.returncode)


if __name__ == '__main__':
    main()
