#!/usr/bin/env python3
"""Release gate: refuse before any external write, record where every released byte came from.

The release version is chosen and committed by the maintainer through `prepare-release.yml`; this gate never selects,
sets or bumps a version. It only checks that the source commit, the version files and the collected assets all agree.

Subcommands
  prerequisites  The production signing inputs exist (Windows code-signing backend, updater key). Fails closed.
  source         The checked-out commit is the pinned one, every version carrier equals the release version,
                 and the Engine pin in Cargo.toml and Cargo.lock agree. Writes the source record.
  assets         The collected asset set is exactly the expected one for the version, comes from the pinned commit,
                 and no test-signed, unsigned or intermediate artifact took part. Writes the asset index.

Trust boundary of the Windows check. This gate runs on Linux and cannot run signtool or evaluate a certificate chain. It does NOT
independently prove Windows certificate trust. It requires that the trusted CI step that did (`sign.py verify` on the Windows
runner: `signtool verify /pa`, chain, timestamp, pinned signer) left a strict receipt in the same workflow's evidence artifact, and
it binds that receipt to the bytes that will be released (setup SHA-256, executables inside the portable zip and the CLI archive,
SHA256SUMS). A `signing.provider` label on its own, or a receipt that does not match the released files, is rejected. Anyone who can
alter the workflow or its artifacts can forge a receipt; the gate defends against mistakes and mixed inputs, not against that.
For `azure`/`pfx` the existing workflow pins no signer identity, so only "one consistent, trusted signer" is checked.

`--mode fixture` is for the non-publishing acceptance run only: it allows a dirty checkout and stamps the output so it
can never be mistaken for a release record. It relaxes nothing about versions, SHAs, coverage or signing providers.
The release workflow never passes it.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

SEMVER = re.compile(r'^\d+\.\d+\.\d+(?:-[A-Za-z]+\.\d+)?$')
# Cargo.lock path crates that do not inherit the workspace version.
NON_WORKSPACE_LOCK_VERSIONS = {'0.0.0', '0.1.0'}
# A top-level workflow artifact built in test mode or signed with a test certificate must not exist in a release run.
TEST_ARTIFACT = re.compile(r'(?:^|-)(?:signing-selftest|signpath-test|test)(?:-|$)')
# The collector never reads these, so they are not rejected here either.
INTERMEDIATE_ARTIFACT = re.compile(r'signpath-stage[0-9]-.*')
# `signed`: azure | pfx through sign.py. `signpath`: SignPath production policy. Test modes (selftest, signpath-test, unsigned) are never listed.
PRODUCTION_WINDOWS_PROVIDERS = ('signed', 'signpath')
EVIDENCE_ARTIFACT = re.compile(r'(macos|linux|windows)-gui-evidence-')


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def git(*args, cwd=ROOT):
    return subprocess.run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def required_assets(v):
    """(name, platform, arch) of every file a release must carry, for version `v`."""
    rows = []
    for target, arch in (('aarch64-apple-darwin', 'aarch64'), ('x86_64-apple-darwin', 'x86_64')):
        rows.append((f'UniClipboard_{target}.app.tar.gz', 'macos', arch))
    rows += [(f'UniClipboard_{v}_aarch64.dmg', 'macos', 'aarch64'), (f'UniClipboard_{v}_x64.dmg', 'macos', 'x86_64')]
    for deb, rpm, appimage, arch in (('amd64', 'x86_64', 'amd64', 'x86_64'), ('arm64', 'aarch64', 'aarch64', 'aarch64')):
        rows += [(f'UniClipboard_{v}_{deb}.deb', 'linux', arch), (f'UniClipboard-{v}-1.{rpm}.rpm', 'linux', arch),
                 (f'UniClipboard_{v}_{appimage}.AppImage.tar.gz', 'linux', arch)]
    for win, arch in (('x64', 'x86_64'), ('arm64', 'aarch64')):
        rows += [(f'UniClipboard_{v}_{win}-setup.exe', 'windows', arch), (f'UniClipboard_{v}_{win}-portable.zip', 'windows', arch)]
    return rows


OPTIONAL_PATTERNS = [
    r'UniClipboard_{v}_(?:amd64|aarch64)\.AppImage',
]
CLI_PATTERN = r'uniclipboard-cli-(?P<v>[0-9][A-Za-z0-9.+_-]*?)-(?P<t>[a-z0-9_]+-[a-z]+-[a-z0-9-]+)\.(?:tar\.gz|zip)'
VERSION_IN_NAME = re.compile(r'^(?:UniClipboard[_-])(?P<v>\d+\.\d+\.\d+(?:-[A-Za-z]+\.\d+)?)[_.-]')


def fail(problems):
    for p in problems:
        print(f'::error::{p}' if os.environ.get('GITHUB_ACTIONS') else f'error: {p}', file=sys.stderr)
    sys.exit(1)


def cmd_prerequisites(args):
    """Exactly one production Windows backend must be configured, or the release stops here.

    `azure` / `pfx` are the local backends of apps/gui-go/packaging/windows/sign.py (WINDOWS_SIGN_BACKEND). SignPath production is
    selected by a production policy slug and a pinned certificate thumbprint (repository variables, no defaults); its API token
    lives in the Environment `signpath-production`, which this job cannot read, so the build job re-checks it and fails closed.
    """
    problems = []
    backend = os.environ.get('WINDOWS_SIGN_BACKEND', '')
    policy = os.environ.get('SIGNPATH_PRODUCTION_POLICY_SLUG', '')
    thumbprint = os.environ.get('SIGNPATH_PRODUCTION_CERT_THUMBPRINT', '')
    if backend and backend not in ('azure', 'pfx'):
        problems.append(f'WINDOWS_SIGN_BACKEND must be azure or pfx (or unset when SignPath production is configured), not {backend!r}.')
    if backend and policy:
        problems.append('Both WINDOWS_SIGN_BACKEND and a SignPath production policy are configured; a release uses exactly one production backend.')
    ref = os.environ.get('RELEASE_REF', '')
    alpha = bool(re.search(r'-alpha\.\d+$', ref)) or ref.endswith('-alpha')
    if not backend and not policy and os.environ.get('ALLOW_UNSIGNED_WINDOWS_ALPHA') == 'true' and alpha:
        print('::notice::No production Windows signing backend: this ALPHA release will ship UNSIGNED Windows packages (ALLOW_UNSIGNED_WINDOWS_ALPHA=true).')
    elif not backend and not policy:
        problems.append('No production Windows code-signing backend is configured: set WINDOWS_SIGN_BACKEND (azure | pfx) or the SignPath '
                        'production variables SIGNPATH_PRODUCTION_POLICY_SLUG and SIGNPATH_PRODUCTION_CERT_THUMBPRINT. '
                        'Test signing (self-test, SignPath test-signing) never satisfies a release.')
    if policy:
        if policy == 'test-signing':
            problems.append('SIGNPATH_PRODUCTION_POLICY_SLUG names the test-signing policy, which is not a production policy.')
        if not re.fullmatch(r'[0-9A-Fa-f]{40}', thumbprint):
            problems.append('SIGNPATH_PRODUCTION_CERT_THUMBPRINT (40 hex digits) is required to pin the SignPath production certificate.')
    if not os.environ.get('TAURI_SIGNING_PRIVATE_KEY'):
        problems.append('TAURI_SIGNING_PRIVATE_KEY (the updater signing key) is not available to this run.')
    if problems:
        fail(problems)
    print('production signing prerequisites present (presence only; validity is proven by the signing and verification steps)')


def read_lock_packages(text):
    for block in text.split('[[package]]')[1:]:
        name = re.search(r'^name = "(.*)"$', block, re.M).group(1)
        version = re.search(r'^version = "(.*)"$', block, re.M).group(1)
        source = re.search(r'^source = "(.*)"$', block, re.M)
        yield name, version, source.group(1) if source else None


def engine_pin_problems(root):
    manifest = (root / 'Cargo.toml').read_text()
    m = re.search(r'^uc-engine\s*=\s*\{[^}]*rev\s*=\s*"([0-9a-f]{40})"', manifest, re.M)
    if not m:
        return None, ['Cargo.toml does not pin uc-engine to a 40-hex revision']
    rev = m.group(1)
    locked = [(v, s) for n, v, s in read_lock_packages((root / 'Cargo.lock').read_text()) if n == 'uc-engine']
    if len(locked) != 1 or not locked[0][1] or not locked[0][1].endswith('#' + rev):
        return rev, [f'Cargo.lock does not lock uc-engine at {rev}: {locked}']
    return rev, []


def version_carriers(root):
    carriers = {}
    carriers['package.json'] = json.loads((root / 'package.json').read_text())['version']
    carriers['apps/gui-go/app.json'] = json.loads((root / 'apps/gui-go/app.json').read_text())['version']
    cargo = (root / 'Cargo.toml').read_text()
    section = re.search(r'^\[workspace\.package\]\n(.*?)(?=^\[)', cargo, re.M | re.S)
    carriers['Cargo.toml [workspace.package]'] = re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.M).group(1)
    build = (root / 'packages/desktop-host-go/buildinfo/buildinfo.go').read_text()
    carriers['packages/desktop-host-go/buildinfo/buildinfo.go'] = re.search(r'PackageVersion = "([^"]+)"', build).group(1)
    lock = {v for n, v, s in read_lock_packages((root / 'Cargo.lock').read_text()) if s is None} - NON_WORKSPACE_LOCK_VERSIONS
    carriers['Cargo.lock workspace members'] = ','.join(sorted(lock))
    return carriers


def cmd_source(args):
    root = Path(args.root).resolve()
    problems = []
    if not SEMVER.match(args.version):
        problems.append(f'release version {args.version!r} is not X.Y.Z or X.Y.Z-channel.N')
    head = git('rev-parse', 'HEAD', cwd=root)
    if not re.fullmatch(r'[0-9a-f]{40}', head):
        problems.append(f'HEAD is not a full commit SHA: {head}')
    if args.expect_sha and args.expect_sha != head:
        problems.append(f'checked-out commit {head} is not the pinned source {args.expect_sha}')
    dirty = git('status', '--porcelain', cwd=root)
    if dirty and args.mode == 'release':
        problems.append('the checkout has uncommitted changes')
    carriers = version_carriers(root)
    for name, value in carriers.items():
        if value != args.version:
            problems.append(f'{name} is {value!r}, not the release version {args.version!r} (version preparation belongs to prepare-release)')
    engine, engine_problems = engine_pin_problems(root)
    problems += engine_problems
    if problems:
        fail(problems)
    record = {'mode': args.mode, 'sourceSha': head, 'version': args.version, 'versionCarriers': carriers,
              'enginePin': engine, 'enginePinSource': 'Cargo.toml == Cargo.lock'}
    Path(args.out).write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
    print(json.dumps(record, sort_keys=True))


def evidence_docs(artifacts):
    for f in sorted(artifacts.rglob('*.json')):
        if f.name not in ('package-manifest.json', 'provenance.json'):
            continue
        try:
            doc = json.loads(f.read_text())
        except ValueError:
            yield f, None
            continue
        yield f, doc


def evidence_source(doc):
    src = doc.get('source') if isinstance(doc.get('source'), dict) else doc.get('git')
    return src if isinstance(src, dict) else None


HEX40, HEX64 = re.compile(r'[0-9A-Fa-f]{40}'), re.compile(r'[0-9a-f]{64}')


def receipt_problems(receipt, label, expect_thumbprint=None):
    """A `sign.py verify` receipt written by the Windows job: strict means a trusted chain, a valid Authenticode status,
    `signtool verify /pa`, a timestamp and one pinned signer for every file. It is a record, not a proof (see the module docstring)."""
    problems = []
    if not isinstance(receipt, dict) or receipt.get('passed') is not True:
        return [f'{label}: the receipt does not say passed']
    if receipt.get('allowUntrustedRoot') is not False:
        problems.append(f'{label}: the receipt waived chain trust (allowUntrustedRoot is not false): a test certificate')
    files = receipt.get('files') or []
    if not files:
        problems.append(f'{label}: the receipt lists no files')
    thumbprints = set()
    for f in files:
        name, sig = f.get('file', '?'), f.get('signature') or {}
        thumbprints.add((sig.get('Thumbprint') or '').lower())
        if f.get('ok') is not True or f.get('chainTrusted') is not True or f.get('signtoolVerifyPa') is not True or sig.get('Status') != 'Valid':
            problems.append(f'{label}: {name} was not verified as a valid, trusted Authenticode signature '
                            f'(ok={f.get("ok")}, chainTrusted={f.get("chainTrusted")}, verify /pa={f.get("signtoolVerifyPa")}, status={sig.get("Status")})')
        if not sig.get('Timestamp') or not f.get('signedAt'):
            problems.append(f'{label}: {name} has no signature timestamp')
        if not HEX64.fullmatch(f.get('sha256') or ''):
            problems.append(f'{label}: {name} has no SHA-256')
        if not HEX40.fullmatch(sig.get('Thumbprint') or ''):
            problems.append(f'{label}: {name} has no signer thumbprint')
    if len(thumbprints) > 1:
        problems.append(f'{label}: the files were signed by different certificates: {sorted(thumbprints)}')
    pinned = receipt.get('expectThumbprint')
    if pinned and thumbprints and thumbprints != {pinned.lower()}:
        problems.append(f'{label}: the signer is not the pinned thumbprint {pinned}')
    if expect_thumbprint and thumbprints != {expect_thumbprint.lower()}:
        problems.append(f'{label}: the signer is not the production certificate configured for this release ({expect_thumbprint})')
    return problems


def zip_member_hashes(path, names):
    import zipfile
    out = {}
    with zipfile.ZipFile(path) as z:
        members = {Path(n).name: n for n in z.namelist()}
        for name in names:
            if name in members:
                out[name] = hashlib.sha256(z.read(members[name])).hexdigest()
    return out


def windows_binding_problems(artifacts, assets, version, top, doc, provider, expect_thumbprint):
    """Bind the receipts of one Windows evidence artifact to the files that will be released."""
    problems = []
    base = artifacts / top / 'windows-gui'
    arch = {'amd64': 'x64', 'x86_64': 'x64', 'arm64': 'arm64', 'aarch64': 'arm64'}.get(doc.get('arch'))
    if arch is None:
        return [f'{top}: the package record names no known architecture ({doc.get("arch")!r})']
    receipts = {}
    for name in ('signatures.json', 'signatures-stage1.json', 'signatures-stage2.json'):
        f = base / name
        if not f.is_file():
            problems.append(f'{top}: the Authenticode verification receipt {name} is missing; a "{provider}" label alone proves nothing')
            continue
        try:
            receipts[name] = json.loads(f.read_text())
        except ValueError:
            problems.append(f'{top}: {name} is not valid JSON')
            continue
        problems += receipt_problems(receipts[name], f'{top}/{name}', expect_thumbprint)
    if problems or assets is None:
        return problems
    receipt_hashes = {name: {f.get('sha256') for f in r.get('files', [])} for name, r in receipts.items()}
    setup, portable = assets / f'UniClipboard_{version}_{arch}-setup.exe', assets / f'UniClipboard_{version}_{arch}-portable.zip'
    if not setup.is_file() or sha256(setup) not in receipt_hashes['signatures.json']:
        problems.append(f'{top}: the released {setup.name} is not one of the files the Authenticode receipt verified (SHA-256 does not match)')
    shipped = doc.get('shipped') or {}
    if portable.is_file():
        inside = zip_member_hashes(portable, ['UniClipboard.exe', 'uniclipd.exe'])
        for exe, digest in inside.items():
            if shipped.get(exe) != digest:
                problems.append(f'{top}: {exe} inside the released {portable.name} is not the executable the package record shipped')
            if digest not in receipt_hashes['signatures-stage1.json'] | receipt_hashes['signatures.json']:
                problems.append(f'{top}: {exe} inside the released {portable.name} was never verified by the Authenticode receipts')
        if set(inside) != {'UniClipboard.exe', 'uniclipd.exe'}:
            problems.append(f'{top}: the released {portable.name} lacks UniClipboard.exe or uniclipd.exe')
    sums = base / 'SHA256SUMS.txt'
    listed = {}
    if sums.is_file():
        for line in sums.read_text().splitlines():
            parts = line.split()
            if len(parts) == 2:
                listed[parts[1].lstrip('*')] = parts[0]
    for released in (setup, portable):
        if released.is_file() and listed.get(released.name) != sha256(released):
            problems.append(f'{top}: SHA256SUMS.txt does not list the released {released.name} with its SHA-256')
    cli = assets / f'uniclipboard-cli-{version}-x86_64-pc-windows-msvc.zip'
    cli_receipt = base / 'signatures-cli.json'
    # Only under SignPath production is the released CLI archive the one the GUI job signed and verified. Under azure | pfx it comes
    # from build-cli, which signs its own copy (different bytes, because Authenticode timestamps differ); the GUI job's
    # signatures-cli.json describes another file, so it must not be compared (documented limitation).
    if arch == 'x64' and cli.is_file() and provider == 'signpath':
        if cli_receipt.is_file():
            r = json.loads(cli_receipt.read_text())
            problems += receipt_problems(r, f'{top}/signatures-cli.json', expect_thumbprint)
            hashes = zip_member_hashes(cli, ['uniclip.exe', 'uniclipd.exe'])
            if set(hashes) != {'uniclip.exe', 'uniclipd.exe'} or not set(hashes.values()) <= {f.get('sha256') for f in r.get('files', [])}:
                problems.append(f'{top}: the executables in the released CLI archive are not the ones the CLI receipt verified')
        else:
            problems.append(f'{top}: the Windows CLI archive has no Authenticode receipt (signatures-cli.json)')
    return problems


def artifact_name_problems(artifacts):
    """Mixed-run guard: a test-mode or test-signed artifact must not be present, whether or not it contributed a file."""
    problems = []
    for top in sorted(p.name for p in artifacts.iterdir() if p.is_dir()):
        if INTERMEDIATE_ARTIFACT.fullmatch(top):
            continue
        if TEST_ARTIFACT.search(top):
            problems.append(f'artifact {top!r} is a test-mode or test-signed build and must not take part in a release')
    return problems


def check_evidence(artifacts, v, sha, assets=None, expect_thumbprint=None, allow_unsigned=False):
    """Every package record belongs to the pinned commit and version, and Windows is signed by a production backend."""
    problems = []
    unsigned_windows = []
    seen = {'macos': 0, 'linux': 0, 'windows': 0}
    for f, doc in evidence_docs(artifacts):
        rel = f.relative_to(artifacts)
        if doc is None:
            problems.append(f'{rel} is not valid JSON')
            continue
        # Only the packaging-evidence artifacts speak for the release. Acceptance, install and legacy-upgrade artifacts carry
        # their own package manifests (other purposes, other versions) and are not release inputs.
        kind = EVIDENCE_ARTIFACT.match(rel.parts[0])
        if not kind:
            continue
        platform = kind.group(1)
        if doc.get('purpose') == 'acceptance-newer-version' or 'newer' in rel.parts:
            continue  # the upgrade-acceptance package carries a deliberately different version
        seen[platform] += 1
        src = evidence_source(doc)
        if src is None:
            problems.append(f'{rel} records no source commit')
            continue
        if src.get('head') != sha:
            problems.append(f'{rel} was built from {src.get("head")}, not the pinned source {sha}')
        if src.get('dirty'):
            problems.append(f'{rel} was built from a dirty checkout')
        if doc.get('version') != v:
            problems.append(f'{rel} is version {doc.get("version")!r}, not {v}')
        if platform == 'windows':
            signing = doc.get('signing') or {}
            provider, proof = signing.get('provider'), signing.get('evidence') or {}
            if allow_unsigned and doc.get('signed') is False and not doc.get('signing'):
                unsigned_windows.append(str(rel))  # the explicit alpha opt-in; nothing is claimed about a signature
                continue
            if not doc.get('signed') or provider not in PRODUCTION_WINDOWS_PROVIDERS:
                problems.append(f'{rel}: Windows signing provider is {provider!r}; a release requires a production signature '
                                f'({" or ".join(PRODUCTION_WINDOWS_PROVIDERS)}). Self-test, SignPath test-signing and unsigned packages never satisfy it.')
            elif proof.get('testCertificate') is not False:
                problems.append(f'{rel}: the signing evidence does not state that a production certificate signed this package')
            elif provider == 'signpath' and (not proof.get('policy') or proof.get('policy') == 'test-signing'
                                             or not re.fullmatch(r'[0-9A-Fa-f]{40}', proof.get('pinnedThumbprint') or '')):
                problems.append(f'{rel}: SignPath evidence lacks a production policy or a pinned certificate thumbprint')
            elif provider == 'signpath' and not expect_thumbprint:
                problems.append(f'{rel}: the production certificate thumbprint (SIGNPATH_PRODUCTION_CERT_THUMBPRINT) was not given to the gate')
            else:
                problems += windows_binding_problems(artifacts, assets, v, rel.parts[0], doc, provider, expect_thumbprint)
    for platform, count in seen.items():
        if not count:
            problems.append(f'no package evidence from {platform} was found in the artifacts')
    return problems, seen, unsigned_windows


def cmd_evidence(args):
    problems, seen, _ = check_evidence(Path(args.artifacts), args.version, args.source_sha, Path(args.assets) if args.assets else None, args.windows_thumbprint)
    problems = artifact_name_problems(Path(args.artifacts)) + problems
    if problems:
        fail(problems)
    print(f'package evidence accepted: {seen}')


def cmd_assets(args):
    artifacts, assets = Path(args.artifacts), Path(args.assets)
    v, sha = args.version, args.source_sha
    problems = []
    if not SEMVER.match(v):
        problems.append(f'release version {v!r} is not X.Y.Z or X.Y.Z-channel.N')

    problems += artifact_name_problems(artifacts)

    required = required_assets(v)
    present = {f.name for f in assets.iterdir() if f.is_file()}
    optional = [re.compile(p.format(v=re.escape(v))) for p in OPTIONAL_PATTERNS]
    cli = re.compile(CLI_PATTERN)
    wanted = {name for name, _, _ in required}
    for name, platform, arch in required:
        if name not in present:
            problems.append(f'missing {platform}/{arch} asset {name}')
    cli_names = []
    for name in sorted(present - wanted):
        m = cli.fullmatch(name)
        if m:
            cli_names.append(name)
            if m.group('v') != v:
                problems.append(f'CLI archive {name} is version {m.group("v")}, not {v}')
            continue
        if any(p.fullmatch(name) for p in optional):
            continue
        found = VERSION_IN_NAME.match(name)
        if found and found.group('v') != v:
            problems.append(f'asset {name} carries version {found.group("v")}, not {v}')
        else:
            problems.append(f'unexpected asset {name}')
    if not cli_names:
        problems.append('no CLI archive was collected')

    # Provenance of the bytes: each asset must equal a file of a workflow artifact, and the name of that artifact is recorded.
    by_hash = {}
    for f in artifacts.rglob('*'):
        if f.is_file() and not f.is_symlink() and not INTERMEDIATE_ARTIFACT.fullmatch(f.relative_to(artifacts).parts[0]):
            by_hash.setdefault(sha256(f), []).append(f.relative_to(artifacts))
    index = []
    for f in sorted(assets.iterdir()):
        if not f.is_file():
            continue
        digest = sha256(f)
        origins = by_hash.get(digest, [])
        if not origins:
            problems.append(f'{f.name} does not match any file of the downloaded workflow artifacts')
        index.append({'name': f.name, 'sha256': digest, 'bytes': f.stat().st_size,
                      'sourceArtifacts': sorted({o.parts[0] for o in origins})})

    allow_unsigned = args.allow_unsigned_windows_alpha
    if allow_unsigned and not re.search(r'-alpha\.\d+$', v):
        problems.append(f'unsigned Windows packages are allowed for alpha releases only, not for {v}')
        allow_unsigned = False
    evidence_problems, seen, unsigned_windows = check_evidence(artifacts, v, sha, assets, args.windows_thumbprint, allow_unsigned)
    problems += evidence_problems

    if problems:
        fail(problems)
    windows_signing = 'unsigned-alpha' if unsigned_windows else 'production'
    record = {'mode': args.mode, 'version': v, 'sourceSha': sha, 'windowsSigning': windows_signing, 'unsignedWindowsEvidence': unsigned_windows,
              'platformsCovered': sorted({p for _, p, _ in required}), 'requiredAssets': len(required), 'assets': index}
    Path(args.out).write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
    print(f'{len(index)} assets checked against {len(required)} required names; evidence records per platform: {seen}')


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest='command', required=True)
    sub.add_parser('prerequisites').set_defaults(func=cmd_prerequisites)
    s = sub.add_parser('source')
    s.add_argument('--version', required=True)
    s.add_argument('--expect-sha')
    s.add_argument('--root', default=str(ROOT))
    s.add_argument('--mode', choices=['release', 'fixture'], default='release')
    s.add_argument('--out', required=True)
    s.set_defaults(func=cmd_source)
    e = sub.add_parser('evidence', help='only the package-evidence checks of `assets`, for inputs that are not a complete release')
    e.add_argument('--version', required=True)
    e.add_argument('--source-sha', required=True)
    e.add_argument('--artifacts', required=True)
    e.add_argument('--assets', help='released files; when given, the Windows receipts are bound to their bytes')
    e.add_argument('--windows-thumbprint', default=os.environ.get('SIGNPATH_PRODUCTION_CERT_THUMBPRINT') or None)
    e.set_defaults(func=cmd_evidence)
    a = sub.add_parser('assets')
    a.add_argument('--version', required=True)
    a.add_argument('--source-sha', required=True)
    a.add_argument('--artifacts', required=True)
    a.add_argument('--assets', required=True)
    a.add_argument('--mode', choices=['release', 'fixture'], default='release')
    a.add_argument('--allow-unsigned-windows-alpha', action='store_true',
                   help='explicit alpha-only opt-in: Windows evidence may say unsigned; test certificates and forged labels are still refused')
    a.add_argument('--windows-thumbprint', default=os.environ.get('SIGNPATH_PRODUCTION_CERT_THUMBPRINT') or None,
                   help='the production certificate configured for this release (SignPath); defaults to $SIGNPATH_PRODUCTION_CERT_THUMBPRINT')
    a.add_argument('--out', required=True)
    a.set_defaults(func=cmd_assets)
    args = p.parse_args()
    args.func(args)


if __name__ == '__main__':
    main()
