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
PRODUCTION_WINDOWS_PROVIDER = 'signed'


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
    problems = []
    backend = os.environ.get('WINDOWS_SIGN_BACKEND', '')
    if backend not in ('azure', 'pfx'):
        problems.append('WINDOWS_SIGN_BACKEND is not azure or pfx: no production Windows code-signing backend is configured. '
                        'Test signing (self-test, SignPath test-signing) never satisfies a release.')
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


def cmd_assets(args):
    artifacts, assets = Path(args.artifacts), Path(args.assets)
    v, sha = args.version, args.source_sha
    problems = []
    if not SEMVER.match(v):
        problems.append(f'release version {v!r} is not X.Y.Z or X.Y.Z-channel.N')

    # Mixed-run guard: a test-mode or test-signed artifact must not be present, whether or not it contributed a file.
    for top in sorted(p.name for p in artifacts.iterdir() if p.is_dir()):
        if INTERMEDIATE_ARTIFACT.fullmatch(top):
            continue
        if TEST_ARTIFACT.search(top):
            problems.append(f'artifact {top!r} is a test-mode or test-signed build and must not take part in a release')

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

    # Evidence: every package record belongs to the pinned commit and version, and Windows is signed by a production backend.
    seen = {'macos': 0, 'linux': 0, 'windows': 0}
    for f, doc in evidence_docs(artifacts):
        rel = f.relative_to(artifacts)
        if doc is None:
            problems.append(f'{rel} is not valid JSON')
            continue
        if doc.get('purpose') == 'acceptance-newer-version' or 'newer' in rel.parts:
            continue  # the upgrade-acceptance package carries a deliberately different version
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
        top = rel.parts[0]
        platform = 'macos' if top.startswith('macos-gui') else 'windows' if top.startswith('windows-gui') else 'linux' if top.startswith('linux-gui') else None
        if platform:
            seen[platform] += 1
        if platform == 'windows':
            provider = (doc.get('signing') or {}).get('provider')
            expected = PRODUCTION_WINDOWS_PROVIDER
            if not doc.get('signed') or provider != expected:
                problems.append(f'{rel}: Windows signing provider is {provider!r}; a release requires a production signature ({expected!r}). '
                                'Self-test, SignPath test-signing and unsigned packages never satisfy it.')
    for platform, count in seen.items():
        if not count:
            problems.append(f'no package evidence from {platform} was found in the artifacts')

    if problems:
        fail(problems)
    record = {'mode': args.mode, 'version': v, 'sourceSha': sha,
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
    a = sub.add_parser('assets')
    a.add_argument('--version', required=True)
    a.add_argument('--source-sha', required=True)
    a.add_argument('--artifacts', required=True)
    a.add_argument('--assets', required=True)
    a.add_argument('--mode', choices=['release', 'fixture'], default='release')
    a.add_argument('--out', required=True)
    a.set_defaults(func=cmd_assets)
    args = p.parse_args()
    args.func(args)


if __name__ == '__main__':
    main()
