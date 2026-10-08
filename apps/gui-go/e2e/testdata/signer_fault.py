"""Test-only signer used by the packaging negative check (signing self-test).

usage: signer_fault.py <fail|skip> <trusted-dir> sign <file>...

Files below <trusted-dir> (the GUI exe, the daemon copy, the finished setup) are signed by the real signer. Any other
file is the temporary uninstaller NSIS hands to `!uninstfinalize`: `fail` exits non-zero, `skip` exits 0 without signing.
The packaging must not produce a shippable result in either case.
"""
import subprocess
import sys
from pathlib import Path

mode, trusted, verb, *files = sys.argv[1:]
real = Path(__file__).resolve().parents[2] / 'packaging/windows/sign.py'
inside = [f for f in files if Path(trusted).resolve() in Path(f).resolve().parents]
outside = [f for f in files if f not in inside]
if inside:
    r = subprocess.run([sys.executable, str(real), verb, *inside])
    if r.returncode:
        sys.exit(r.returncode)
if outside:
    print(f'signer_fault: {mode} for {outside}', flush=True)
    sys.exit(1 if mode == 'fail' else 0)
