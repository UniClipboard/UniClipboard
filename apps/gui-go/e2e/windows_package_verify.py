#!/usr/bin/env python3
"""Prove that the daemon inside the shipped Windows packages is the CI-built one.

  python apps/gui-go/e2e/windows_package_verify.py --package <package_windows.py output dir> [--out evidence.json]

Unpacks the files that would be uploaded (the NSIS installer with 7-Zip, the portable zip with zipfile), finds
`uniclipd.exe` in each payload and compares its SHA-256 with the build-sidecar record that package_windows.py copied
into package-manifest.json. Works on any host with `7z` on PATH. A mismatch or a missing file exits non-zero.
"""
import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def find(root, name):
    hits = [p for p in root.rglob('*') if p.is_file() and p.name.lower() == name.lower()]
    return hits[0] if len(hits) == 1 else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--package', type=Path, required=True)
    ap.add_argument('--out', type=Path)
    ap.add_argument('--require-signed', action='store_true', help='also run sign.py verify (signtool verify /pa) on the setup and the unpacked exe/daemon')
    ap.add_argument('--signature-out', type=Path)
    ap.add_argument('--sign-expect-thumbprint', help='test certificates only: the thumbprint the signer must have')
    ap.add_argument('--sign-untrusted-root', action='store_true', help='test certificates only (needs SIGNING_TEST_CERT=1): waive only the chain trust')
    args = ap.parse_args()
    manifest = json.loads((args.package / 'package-manifest.json').read_text())
    daemon = manifest['daemon']
    if daemon['kind'] != 'ci-built-rust-daemon' or not daemon['identityVerified']:
        sys.exit(f"the package manifest does not claim a CI-built daemon (kind={daemon['kind']})")
    # The daemon identity is the build-sidecar hash; with Authenticode signing the shipped file is that file plus a signature.
    expected = daemon.get('shippedSha256') or daemon['buildEvidence']['sha256']
    setup = next(args.package.glob('*-setup.exe'))
    portable = next(args.package.glob('*-portable.zip'))
    seven = shutil.which('7z') or shutil.which('7za') or sys.exit('7z not found on PATH')
    result = {'expectedDaemonSha256': expected, 'checks': []}
    work = Path(tempfile.mkdtemp(prefix='uc-pkg-verify-'))
    try:
        sx = work / 'setup'
        subprocess.run([seven, 'x', '-y', f'-o{sx}', str(setup)], check=True, capture_output=True)
        pz = work / 'portable'
        with zipfile.ZipFile(portable) as z:
            z.extractall(pz)
        for label, root, exe in (('setup payload', sx, 'uniclipd.exe'), ('portable zip', pz, 'uniclipd.exe')):
            f = find(root, exe)
            got = sha256(f) if f else None
            ok = got == expected
            result['checks'].append({'check': f'{label}: uniclipd.exe equals the CI-built daemon', 'ok': ok, 'sha256': got})
            print(('PASS ' if ok else 'FAIL ') + result['checks'][-1]['check'], flush=True)
        helper = manifest['helper']
        if helper['kind'] != 'ci-built-rust-quick-panel' or not helper['identityVerified']:
            sys.exit(f"the package manifest does not claim a CI-built quick panel (kind={helper['kind']})")
        expected_helper = helper.get('shippedSha256') or helper['buildEvidence']['sha256']
        for label, root in (('setup payload', sx), ('portable zip', pz)):
            f = find(root, 'uniclip-quick-panel.exe')
            got = sha256(f) if f else None
            result['checks'].append({'check': f'{label}: uniclip-quick-panel.exe equals the CI-built quick panel', 'ok': got == expected_helper, 'sha256': got})
            print(('PASS ' if got == expected_helper else 'FAIL ') + result['checks'][-1]['check'], flush=True)
        for label, root, exe in (('setup payload', sx, 'UniClipboard.exe'), ('portable zip', pz, 'UniClipboard.exe')):
            f = find(root, exe)
            result['checks'].append({'check': f'{label}: {exe} present', 'ok': f is not None, 'sha256': sha256(f) if f else None})
            print(('PASS ' if f else 'FAIL ') + result['checks'][-1]['check'], flush=True)
        if args.require_signed:
            # The uninstaller is generated at install time; package_windows.py keeps the signed copy NSIS embeds.
            uninstaller = args.package / 'uninstaller-signed.exe'
            targets = [setup, uninstaller] + [find(r, n) for r in (sx, pz) for n in ('UniClipboard.exe', 'uniclipd.exe', 'uniclip-quick-panel.exe')]
            if not uninstaller.exists():
                result['checks'].append({'check': 'Authenticode: the signed uninstaller copy exists', 'ok': False})
                print('FAIL ' + result['checks'][-1]['check'], flush=True)
                targets.remove(uninstaller)
            sign_py = Path(__file__).resolve().parents[1] / 'packaging/windows/sign.py'
            cmd = [sys.executable, str(sign_py), 'verify', *([] if not args.signature_out else ['--out', str(args.signature_out)]),
                   *(['--expect-thumbprint', args.sign_expect_thumbprint] if args.sign_expect_thumbprint else []),
                   *(['--allow-untrusted-root'] if args.sign_untrusted_root else []), *map(str, targets)]
            ok = subprocess.run(cmd).returncode == 0
            result['checks'].append({'check': 'Authenticode: setup, uninstaller and the unpacked GUI exe, daemon and quick panel verify (signtool verify /pa)', 'ok': ok})
            print(('PASS ' if ok else 'FAIL ') + result['checks'][-1]['check'], flush=True)
        same = [c['sha256'] for c in result['checks'] if 'UniClipboard.exe' in c['check']]
        result['checks'].append({'check': 'GUI exe is identical in the installer and the portable zip', 'ok': len(set(same)) == 1 and None not in same})
        print(('PASS ' if result['checks'][-1]['ok'] else 'FAIL ') + result['checks'][-1]['check'], flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    result['passed'] = all(c['ok'] for c in result['checks'])
    result['files'] = {p.name: {'sha256': sha256(p), 'bytes': p.stat().st_size} for p in (setup, portable)}
    if args.out:
        args.out.write_text(json.dumps(result, indent=2) + '\n')
    sys.exit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
