#!/usr/bin/env python3
"""Real dpkg upgrade ordering of pre-release debs built by the shipped `build_deb`.

Failure model: a deb whose control Version keeps the release spelling `X.Y.Z-alpha.N` is upstream X.Y.Z with revision
alpha.N, which dpkg sorts AFTER the stable X.Y.Z, so the stable release would look like a downgrade of the alpha. The control
Version must use `~` while the file name keeps the release version (the updater semantics).

Host mode starts a Debian container (real dpkg) and runs this file inside it with `--inside`. The payload is a labelled
fixture (a shell script standing in for the executables): only the package metadata and dpkg behaviour are under test.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[4]
VERSIONS = ['1.3.0-alpha.1', '1.3.0-alpha.2', '1.3.0']


def sh(cmd, check=True):
    r = subprocess.run(cmd, shell=isinstance(cmd, str), capture_output=True, text=True)
    if check and r.returncode:
        raise SystemExit(f'failed: {cmd}\n{r.stdout}{r.stderr}')
    return r


def inside(out):
    sys.path.insert(0, os.environ.get('UC_PACKAGE_LINUX_DIR', '/repo/apps/gui-go/e2e'))
    import package_linux
    out = Path(out)
    stage = out / 'stage'
    (stage / 'usr/bin').mkdir(parents=True)
    for exe in ('uniclipboard', 'uniclipd'):
        (stage / 'usr/bin' / exe).write_text('#!/bin/sh\n# FIXTURE payload: package metadata only\n')
        (stage / 'usr/bin' / exe).chmod(0o755)
    debs, checks = {}, []

    def check(name, ok, detail=''):
        checks.append({'name': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name + (' :: ' + detail if detail else ''))

    for v in VERSIONS:
        d = out / v
        d.mkdir()
        debs[v] = package_linux.build_deb(stage, d, v, 'arm64' if os.uname().machine == 'aarch64' else 'amd64',
                                          f'UniClipboard_{v}_fixture.deb')
        control = sh(['dpkg-deb', '-f', str(debs[v]), 'Version']).stdout.strip()
        check(f'control Version of {v}', control == v.replace('-', '~', 1), control)
        check(f'file name keeps the release version {v}', f'_{v}_' in debs[v].name, debs[v].name)
        rel = sh(['dpkg-deb', '-f', str(debs[v]), 'Provides', 'Conflicts', 'Replaces']).stdout
        check(f'legacy relationships of {v} use the dpkg spelling', '-alpha' not in rel, rel.replace('\n', ' | '))

    lt = lambda a, b: sh(['dpkg', '--compare-versions', a, 'lt', b], check=False).returncode == 0
    check('dpkg orders 1.1.1 < 1.3.0~alpha.1', lt('1.1.1', '1.3.0~alpha.1'))
    check('dpkg orders alpha.1 < alpha.2', lt('1.3.0~alpha.1', '1.3.0~alpha.2'))
    check('dpkg orders alpha.2 < stable', lt('1.3.0~alpha.2', '1.3.0'))
    check('the old spelling would have sorted the alpha AFTER stable (the defect)', lt('1.3.0', '1.3.0-alpha.2'))

    def install(v, expect_ok=True):
        r = sh(['dpkg', '-i', '--force-depends', str(debs[v])], check=False)
        installed = sh(['dpkg-query', '-W', '-f', '${Version}', 'uniclipboard'], check=False).stdout
        return r, installed

    for v in VERSIONS:
        r, installed = install(v)
        check(f'dpkg -i {v} succeeds and installs {v.replace("-", "~", 1)}', r.returncode == 0 and installed == v.replace('-', '~', 1),
              installed)
        check(f'no downgrade warning on the step to {v}', 'downgrading' not in r.stderr)
    install('1.3.0')  # the downgrade check must not depend on the order of VERSIONS
    r, installed = install('1.3.0-alpha.2')
    check('installing alpha.2 over stable is reported by dpkg as a downgrade', 'downgrading uniclipboard from 1.3.0 to 1.3.0~alpha.2' in r.stderr,
          next((l for l in r.stderr.splitlines() if 'downgrading' in l), r.stderr.strip()[:200]))
    (out / 'result.json').write_text(json.dumps({'checks': checks, 'dpkg': sh(['dpkg', '--version']).stdout.splitlines()[0],
                                                 'assertions': len(checks)}, indent=2) + '\n')
    sys.exit(0 if all(c['ok'] for c in checks) else 1)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--image', default='debian:bookworm')
    p.add_argument('--baseline', type=Path, help='directory holding the pre-fix package_linux.py; the run is expected to fail')
    p.add_argument('--inside', action='store_true')
    a = p.parse_args()
    if a.inside:
        inside(a.out)
        return
    a.out.mkdir(parents=True, exist_ok=False)
    extra = ['-v', f'{a.baseline.resolve()}:/b/1/2/3:ro', '-e', 'UC_PACKAGE_LINUX_DIR=/b/1/2/3'] if a.baseline else []
    r = subprocess.run(['docker', 'run', '--rm', '-v', f'{ROOT}:/repo:ro', '-v', f'{a.out.resolve()}:/out', *extra, a.image,
                        'sh', '-c', 'command -v python3 >/dev/null || (apt-get update -qq && apt-get install -y -qq python3 >/dev/null); ' +
                                   'python3 -I /repo/apps/gui-go/e2e/linux/deb_version_order_run.py --inside --out /out/run'],
                       text=True)
    sys.exit(r.returncode)


if __name__ == '__main__':
    main()
