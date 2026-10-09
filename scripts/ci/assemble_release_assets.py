#!/usr/bin/env python3
"""Everything between the downloaded workflow artifacts and the first external write of a release.

collect named distributables -> gate (coverage, version, source SHA, no test/unsigned artifacts) -> updater signatures
-> signature re-verification without any secret -> update manifest (all six platforms) -> FlareRelease registration
-> SHA-256 index. Nothing here talks to a network service or writes to a release, bucket, tag or channel.

`release.yml` runs this with `--mode release`. The offline acceptance run (`apps/gui-go/e2e/release_assembly_run.py`) runs the
same code with `--mode fixture`, which only adds the ability to point at a disposable signer and key; the stages, their
order and their checks are the same.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_gate  # noqa: E402


def run(label, cmd, out, *, env=None, cwd=ROOT):
    result = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, capture_output=True, text=True)
    (out / f'{label}.log').write_text(result.stdout + result.stderr)
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if result.returncode:
        sys.exit(f'stage {label} failed with exit {result.returncode}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--artifacts', type=Path, required=True, help='downloaded workflow artifacts (artifacts/<name>/...)')
    p.add_argument('--work', type=Path, required=True, help='new directory for release-assets/ and evidence/')
    p.add_argument('--version', required=True)
    p.add_argument('--channel', required=True)
    p.add_argument('--source-sha', required=True)
    p.add_argument('--source-record', type=Path, required=True, help='output of `release_gate.py source`')
    p.add_argument('--base-url', required=True)
    p.add_argument('--notes-file', type=Path, required=True)
    p.add_argument('--zh-notes-file', type=Path, required=True)
    p.add_argument('--registration-source', required=True)
    p.add_argument('--mode', choices=['release', 'fixture'], default='release')
    p.add_argument('--signer', type=Path, help='fixture mode only: prebuilt updater-sign binary')
    p.add_argument('--app-config', type=Path, help='fixture mode only: app.json carrying the fixture public key')
    args = p.parse_args()

    if args.mode == 'release' and (args.signer or args.app_config):
        sys.exit('release mode signs with apps/gui-go/app.json and the production updater key only')
    record = json.loads(args.source_record.read_text())
    if record.get('mode') != args.mode or record.get('sourceSha') != args.source_sha or record.get('version') != args.version:
        sys.exit(f'source record {args.source_record} does not describe {args.mode} {args.version} at {args.source_sha}')

    work = args.work.resolve()
    assets, evidence = work / 'release-assets', work / 'evidence'
    work.mkdir(parents=True, exist_ok=False)
    evidence.mkdir()
    (evidence / 'source-record.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
    py = [sys.executable, '-I']

    run('collect', py + [ROOT / 'scripts/collect-release-assets.py', '--source', args.artifacts, '--destination', assets], evidence)
    run('gate-assets', py + [ROOT / 'scripts/ci/release_gate.py', 'assets', '--version', args.version, '--source-sha', args.source_sha,
                             '--artifacts', args.artifacts, '--assets', assets, '--mode', args.mode,
                             '--out', evidence / 'release-assets-index.json'], evidence)

    config = args.app_config or ROOT / 'apps/gui-go/app.json'
    signer = [args.signer] if args.signer else ['go', 'run', './cmd/updater-sign']
    sign_cwd = ROOT if args.signer else ROOT / 'apps/gui-go'
    run('updater-sign', signer + ['--app-config', config, '--artifacts-dir', assets, '--evidence', evidence / 'flare-signatures.json'],
        evidence, cwd=sign_cwd)
    no_secret = {k: v for k, v in os.environ.items() if not k.startswith('TAURI_SIGNING_')}
    run('updater-verify-without-secret', signer + ['--app-config', config, '--artifacts-dir', assets, '--verify-only'],
        evidence, env=no_secret, cwd=sign_cwd)

    manifest, registration = evidence / 'manifest.json', evidence / 'registration.json'
    run('manifest', ['node', ROOT / 'scripts/assemble-update-manifest.js', '--require-all-platforms', '--version', args.version,
                     '--artifacts-dir', assets, '--output', manifest, '--base-url', args.base_url,
                     '--notes-file', args.notes_file, '--zh-notes-file', args.zh_notes_file], evidence)
    run('registration', ['node', ROOT / 'scripts/build-flare-release-registration.js', '--version', args.version,
                         '--channel', args.channel, '--manifest', manifest, '--artifacts-dir', assets,
                         '--source', args.registration_source, '--output', registration], evidence)

    index = {'mode': args.mode, 'version': args.version, 'channel': args.channel, 'sourceSha': args.source_sha,
             'enginePin': record['enginePin'],
             'files': {f.name: release_gate.sha256(f) for f in sorted(assets.iterdir()) if f.is_file()},
             'evidence': {f.name: release_gate.sha256(f) for f in sorted(evidence.iterdir()) if f.is_file() and f.name != 'assembly-index.json'}}
    (evidence / 'assembly-index.json').write_text(json.dumps(index, indent=2, sort_keys=True) + '\n')
    print(f'assembled {len(index["files"])} files for {args.version} at {args.source_sha} ({args.mode}); no external write was performed')


if __name__ == '__main__':
    main()
