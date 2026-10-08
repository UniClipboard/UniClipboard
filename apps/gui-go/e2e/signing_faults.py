#!/usr/bin/env python3
"""Fault injection for the signing hand-off of the Windows packages (issue #1897).

  python apps/gui-go/e2e/signing_faults.py --package <prepared out dir> --signed <dir with the genuinely signed files> --log-dir <dir>

The signing service sits between `package_windows.py --stage prepare` and `--stage assemble`. Whatever comes back from it
(a missing file, an unsigned file, a modified or another file, an extra file) must stop `--stage assemble` before an installer
is built. Each fault below is built from the genuinely signed files and fed to the real assemble stage; the run fails
unless every one of them is refused and none leaves an installer behind. Run it BEFORE the real assemble.
"""
import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STAGE_FILES = ('UniClipboard.exe', 'uniclipd.exe', 'uninstall.exe')


def faults(signed, unsigned, work):
    def fresh(name):
        d = work / name
        shutil.copytree(signed, d)
        return d

    d = fresh('missing-uninstaller')
    (d / 'uninstall.exe').unlink()
    yield 'missing-uninstaller', d, 'must hold exactly'

    d = fresh('extra-file')
    (d / 'extra.exe').write_bytes(b'x')
    yield 'extra-file', d, 'must hold exactly'

    d = fresh('tampered-gui')
    data = bytearray((d / 'UniClipboard.exe').read_bytes())
    data[len(data) // 3] ^= 0xFF
    (d / 'UniClipboard.exe').write_bytes(bytes(data))
    yield 'tampered-gui', d, 'content differs'

    d = fresh('swapped-daemon')
    shutil.copy2(signed / 'UniClipboard.exe', d / 'uniclipd.exe')
    yield 'swapped-daemon', d, 'content differs'

    d = fresh('truncated-uninstaller')
    (d / 'uninstall.exe').write_bytes((d / 'uninstall.exe').read_bytes()[:-64])
    yield 'truncated-uninstaller', d, ''

    d = fresh('unsigned-uninstaller')
    shutil.copy2(unsigned / 'uninstall.exe', d / 'uninstall.exe')
    yield 'unsigned-uninstaller', d, 'not signed'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--package', type=Path, required=True)
    ap.add_argument('--signed', type=Path, required=True)
    ap.add_argument('--log-dir', type=Path, required=True)
    a = ap.parse_args()
    a.log_dir.mkdir(parents=True, exist_ok=True)
    work = a.log_dir / 'faulted-inputs'
    results, failed = [], False
    for name, d, expect in faults(a.signed.resolve(), a.package / 'to-sign', work):
        r = subprocess.run([sys.executable, str(HERE / 'package_windows.py'), '--stage', 'assemble', '--out', str(a.package), '--signed-dir', str(d)],
                           capture_output=True, text=True)
        (a.log_dir / f'{name}.log').write_text(r.stdout + r.stderr)
        built = (a.package / 'to-sign-setup').exists()
        refused = r.returncode != 0 and not built and expect in (r.stdout + r.stderr)
        results.append({'fault': name, 'exit': r.returncode, 'installerBuilt': built, 'refused': refused})
        print(('PASS ' if refused else 'FAIL ') + f'fault {name}: assemble exit {r.returncode}, installer built: {built}', flush=True)
        failed |= not refused
    (a.log_dir / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
