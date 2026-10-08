#!/usr/bin/env python3
"""Authenticode signing and verification for the Windows packages (issue #1897).

  sign.py sign <file>...                      sign in place with the backend named by SIGN_BACKEND (azure | pfx)
  sign.py verify [--out evidence.json] [--expect-subject TEXT] [--allow-untrusted-root] <file>...

Signing needs a certificate or service the project does not have yet (see docs/architecture/gui-go-windows-packaging.md);
this file only holds the mechanics so that the choice is a matter of secrets, not code. Backends:

  azure  Azure Artifact Signing (formerly Trusted Signing) through signtool and the Azure.CodeSigning.Dlib. Environment:
         AZURE_SIGN_ENDPOINT, AZURE_SIGN_ACCOUNT, AZURE_SIGN_PROFILE, AZURE_SIGN_DLIB (path of the dlib); credentials are
         the standard Azure environment (AZURE_TENANT_ID / AZURE_CLIENT_ID / AZURE_CLIENT_SECRET, or OIDC).
  pfx    a certificate file: SIGN_PFX_PATH and SIGN_PFX_PASSWORD. It is imported into the current user's store and signtool
         selects it by thumbprint, so the password never appears on a command line.

Common: SIGN_TIMESTAMP_URL (RFC 3161, default http://timestamp.digicert.com). Signatures are SHA-256 with a timestamp.

`verify` runs `signtool verify /pa /v` (the Authenticode policy a user's machine applies) and reads the signature with
Get-AuthenticodeSignature. A signature that does not chain to a trusted root fails, which is the point; a throwaway
self-test certificate trusted on a disposable runner passes only because that runner trusts it, and says so in the evidence.
"""
import argparse
import glob
import json
import os
import subprocess
import sys
from pathlib import Path


def signtool(required=True):
    hits = sorted(glob.glob(r'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe')
                  + glob.glob(r'C:\Program Files (x86)\Windows Kits\10\bin\*\arm64\signtool.exe'))
    # Prefer the newest SDK; an arm64 host runs the x64 tool under emulation if no native one exists.
    native = [h for h in hits if (os.environ.get('PROCESSOR_ARCHITECTURE', '').lower() == 'arm64') == ('arm64' in h.lower().split('\\')[-2])]
    pool = native or hits
    if not pool:
        if required:
            sys.exit('signtool.exe not found (install the Windows SDK)')
        return None
    return pool[-1]


def ps(script):
    return subprocess.run(['powershell', '-NoProfile', '-NonInteractive', '-Command', script], capture_output=True, text=True)


def timestamp_url():
    return os.environ.get('SIGN_TIMESTAMP_URL', 'http://timestamp.digicert.com')


def import_pfx():
    for name in ('SIGN_PFX_PATH', 'SIGN_PFX_PASSWORD'):  # read by the PowerShell below from the environment, never from a command line
        if not os.environ.get(name):
            sys.exit(f'{name} is required for the pfx backend')
    r = ps("$p = ConvertTo-SecureString -String $env:SIGN_PFX_PASSWORD -AsPlainText -Force;"
           "(Import-PfxCertificate -FilePath $env:SIGN_PFX_PATH -CertStoreLocation Cert:\\CurrentUser\\My -Password $p).Thumbprint")
    thumb = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else ''
    if r.returncode != 0 or len(thumb) != 40:
        sys.exit('could not import the signing certificate: ' + r.stderr.strip()[:300])
    return thumb


def sign(files):
    backend = os.environ.get('SIGN_BACKEND', '')
    tool = signtool()
    if backend == 'pfx':
        args = ['/sha1', import_pfx()]
    elif backend == 'azure':
        meta = Path(os.environ.get('RUNNER_TEMP', '.')) / 'artifact-signing-metadata.json'
        meta.write_text(json.dumps({'Endpoint': os.environ['AZURE_SIGN_ENDPOINT'], 'CodeSigningAccountName': os.environ['AZURE_SIGN_ACCOUNT'],
                                    'CertificateProfileName': os.environ['AZURE_SIGN_PROFILE']}))
        args = ['/dlib', os.environ['AZURE_SIGN_DLIB'], '/dmdf', str(meta)]
    else:
        sys.exit('SIGN_BACKEND must be azure or pfx')
    for f in files:
        cmd = [tool, 'sign', '/fd', 'SHA256', '/tr', timestamp_url(), '/td', 'SHA256', *args, str(f)]
        print('+', ' '.join(cmd), flush=True)
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit(f'signtool sign failed for {f}: {(r.stdout + r.stderr)[-600:]}')


def verify(files, expect_subject, allow_untrusted_root, out):
    tool = signtool(required=False)  # the arm64 hosted image may carry no SDK: then only Get-AuthenticodeSignature judges
    report, ok_all = [], True
    for f in files:
        pa = subprocess.run([tool, 'verify', '/pa', '/v', str(f)], capture_output=True, text=True) if tool else \
            subprocess.CompletedProcess([], 0, '', 'signtool not available on this host')
        sig = ps(f"$s = Get-AuthenticodeSignature -LiteralPath '{f}'; "
                 "[pscustomobject]@{Status=[string]$s.Status; Subject=$s.SignerCertificate.Subject; Thumbprint=$s.SignerCertificate.Thumbprint;"
                 " Issuer=$s.SignerCertificate.Issuer; Timestamp=if($s.TimeStamperCertificate){$s.TimeStamperCertificate.Subject}else{$null}} | ConvertTo-Json -Compress")
        info = json.loads(sig.stdout) if sig.stdout.strip() else {}
        ok = pa.returncode == 0 and info.get('Status') == 'Valid' and bool(info.get('Timestamp'))
        if allow_untrusted_root and not ok:
            # Throwaway self-test certificate on a runner that cannot trust it (not elevated: adding a root to the user store
            # needs an interactive confirmation). The signature must still be present, carry the expected subject and a timestamp;
            # only the chain trust is waived, and the report says so.
            ok = bool(expect_subject) and bool(info.get('Subject')) and expect_subject in info['Subject'] and bool(info.get('Timestamp')) \
                and info.get('Status') not in ('NotSigned', 'HashMismatch')
        if expect_subject and expect_subject not in (info.get('Subject') or ''):
            ok = False
        report.append({'file': str(f), 'ok': ok, 'signtoolVerifyPa': pa.returncode == 0 if tool else None, 'signature': info, 'signtoolTail': (pa.stdout + pa.stderr)[-400:]})
        ok_all &= ok
        print(('PASS ' if ok else 'FAIL ') + str(f), info.get('Status'), info.get('Subject'), flush=True)
    result = {'allowUntrustedRoot': allow_untrusted_root, 'passed': ok_all, 'files': report}
    if out:
        Path(out).write_text(json.dumps(result, indent=2) + '\n')
    return ok_all


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('sign')
    s.add_argument('files', nargs='+', type=Path)
    v = sub.add_parser('verify')
    v.add_argument('files', nargs='+', type=Path)
    v.add_argument('--out')
    v.add_argument('--expect-subject')
    v.add_argument('--allow-untrusted-root', action='store_true', help='signing self-test on a runner that cannot trust the throwaway certificate: waive only the chain trust, still require --expect-subject, a timestamp and an intact hash')
    a = ap.parse_args()
    if os.name != 'nt':
        sys.exit('signing runs on Windows only')
    if a.cmd == 'sign':
        sign(a.files)
    else:
        sys.exit(0 if verify(a.files, a.expect_subject, a.allow_untrusted_root, a.out) else 1)


if __name__ == '__main__':
    main()
