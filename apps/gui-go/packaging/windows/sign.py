#!/usr/bin/env python3
"""Authenticode signing and verification for the Windows packages (issue #1897).

  sign.py sign <file>...                      sign in place with the backend named by SIGN_BACKEND (azure | pfx)
  sign.py verify [--out evidence.json] [--expect-subject TEXT] [--expect-thumbprint SHA1] [--allow-untrusted-root] <file>...
  sign.py matches <submitted> <signed>        the signed file is the submitted one plus a signature (nothing else changed)

The `azure` and `pfx` backends sign locally. SignPath signs remotely (a GitHub Actions step submits the files, see
.github/workflows/build.yml); this file then only verifies and compares what comes back.

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
import hashlib
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


def _u32(data, off):
    return int.from_bytes(data[off:off + 4], 'little')


def pe_content(data, name):
    """The part of a PE file a signature does not change: (bytes without the certificate table and with the checksum and the
    certificate-table entry zeroed, size of the certificate table). Raises ValueError for anything that is not a PE file."""
    if data[:2] != b'MZ' or len(data) < 0x40:
        raise ValueError(f'{name} is not a PE file')
    pe = _u32(data, 0x3C)
    if data[pe:pe + 4] != b'PE\0\0':
        raise ValueError(f'{name} has no PE header')
    opt = pe + 24
    magic = int.from_bytes(data[opt:opt + 2], 'little')
    if magic not in (0x10B, 0x20B):
        raise ValueError(f'{name} has an unknown optional header magic {magic:#x}')
    checksum = opt + 64
    entry = opt + (96 if magic == 0x10B else 112) + 4 * 8  # data directory 4: the Authenticode certificate table
    cert_off, cert_size = _u32(data, entry), _u32(data, entry + 4)
    body = bytearray(data[:cert_off] if cert_size else data)
    if cert_size and (cert_off < 0x40 or cert_off + cert_size > len(data)):
        raise ValueError(f'{name} has a corrupt certificate table entry')
    body[checksum:checksum + 4] = b'\0\0\0\0'
    body[entry:entry + 8] = b'\0' * 8
    return bytes(body), cert_size


def check_signed_matches(submitted, returned, label, require_certificate=True):
    """The returned file is the submitted one plus a signature, and not another file, a truncated one or an unsigned one."""
    try:
        want, want_cert = pe_content(Path(submitted).read_bytes(), f'submitted {label}')
        got, got_cert = pe_content(Path(returned).read_bytes(), f'returned {label}')
    except ValueError as e:
        sys.exit(str(e))
    problems = []
    if want_cert:
        problems.append('the submitted file already carried a certificate')
    if require_certificate and not got_cert:
        problems.append('it carries no Authenticode certificate table (it was not signed)')
    n = len(want)
    if got[:n] != want or len(got) - n >= 8 or any(got[n:]):
        problems.append('its content differs from the file that was submitted for signing (wrong, truncated or modified file)')
    if problems:
        sys.exit(f'{label}: ' + '; '.join(problems))


# Statuses Get-AuthenticodeSignature reports when the signature and digest are fine but the chain does not end in a trusted
# root. Everything else (NotSigned, HashMismatch, NotSupportedFileFormat, ...) is a failure in every mode.
UNTRUSTED_CHAIN_STATUSES = ('UnknownError', 'NotTrusted')


def verify(files, expect_subject, allow_untrusted_root, out, expect_thumbprint=None):
    """Product mode (the default) requires a trusted chain. `allow_untrusted_root` is for TEST certificates only (the signing self-test or SignPath test-signing): it needs
    SIGNING_TEST_CERT=1 and the thumbprint of the test certificate, so a signature by any other certificate is still refused."""
    if allow_untrusted_root and (not expect_thumbprint or os.environ.get('SIGNING_TEST_CERT') != '1'):
        sys.exit('--allow-untrusted-root is only for test certificates: it needs --expect-thumbprint and SIGNING_TEST_CERT=1')
    tool = signtool(required=False)  # the arm64 hosted image may carry no SDK: then only Get-AuthenticodeSignature judges
    report, ok_all = [], True
    for f in files:
        pa = subprocess.run([tool, 'verify', '/pa', '/v', str(f)], capture_output=True, text=True) if tool else \
            subprocess.CompletedProcess([], 0, '', 'signtool not available on this host')
        sig = ps(f"$s = Get-AuthenticodeSignature -LiteralPath '{f}'; "
                 "[pscustomobject]@{Status=[string]$s.Status; Subject=$s.SignerCertificate.Subject; Thumbprint=$s.SignerCertificate.Thumbprint;"
                 " Issuer=$s.SignerCertificate.Issuer; Timestamp=if($s.TimeStamperCertificate){$s.TimeStamperCertificate.Subject}else{$null}} | ConvertTo-Json -Compress")
        info = json.loads(sig.stdout) if sig.stdout.strip() else {}
        identity_ok = (not expect_subject or expect_subject in (info.get('Subject') or '')) and \
            (not expect_thumbprint or (info.get('Thumbprint') or '').lower() == expect_thumbprint.lower())
        chain_trusted = pa.returncode == 0 and info.get('Status') == 'Valid' and bool(info.get('Timestamp'))
        ok = chain_trusted and identity_ok
        if allow_untrusted_root and not ok:
            ok = identity_ok and bool(info.get('Timestamp')) and info.get('Status') in UNTRUSTED_CHAIN_STATUSES
        # `signtool verify /pa /v` prints the signing time of an RFC 3161 timestamp ("The signature is timestamped: <date>").
        stamped = next((l.split(':', 1)[1].strip() for l in (pa.stdout or '').splitlines() if 'signature is timestamped' in l.lower()), None)
        report.append({'file': str(f), 'sha256': hashlib.sha256(Path(f).read_bytes()).hexdigest(), 'signedAt': stamped, 'ok': ok, 'chainTrusted': ok and chain_trusted, 'signtoolVerifyPa': pa.returncode == 0 if tool else None,
                       'signature': info, 'signtoolTail': (pa.stdout + pa.stderr)[-400:]})
        ok_all &= ok
        print(('PASS ' if ok else 'FAIL ') + str(f), info.get('Status'), info.get('Subject'), 'chain-trusted' if report[-1]['chainTrusted'] else 'chain-NOT-verified', flush=True)
    result = {'allowUntrustedRoot': allow_untrusted_root, 'expectThumbprint': expect_thumbprint, 'passed': ok_all, 'files': report}
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
    v.add_argument('--expect-thumbprint', help='SHA-1 thumbprint the signer certificate must have')
    v.add_argument('--allow-untrusted-root', action='store_true', help='test certificates only (needs SIGNING_TEST_CERT=1 and --expect-thumbprint): waive only the chain trust; the signer thumbprint, a timestamp and an intact digest are still required')
    m = sub.add_parser('matches', help='exit non-zero unless SIGNED is SUBMITTED plus an Authenticode signature (same content, certificate table present)')
    m.add_argument('submitted', type=Path)
    m.add_argument('signed', type=Path)
    a = ap.parse_args()
    if a.cmd == 'matches':  # a pure byte comparison: also runs off Windows
        check_signed_matches(a.submitted, a.signed, a.signed.name)
        return
    if os.name != 'nt':
        sys.exit('signing runs on Windows only')
    if a.cmd == 'sign':
        sign(a.files)
    else:
        sys.exit(0 if verify(a.files, a.expect_subject, a.allow_untrusted_root, a.out, a.expect_thumbprint) else 1)


if __name__ == '__main__':
    main()
