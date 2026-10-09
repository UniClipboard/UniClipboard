#!/usr/bin/env python3
"""Reproduce and check the release asset collection over the real artifact layout of `download-artifact`.

`release.yml` downloads every workflow artifact into `artifacts/<artifact name>/` and hands the tree to
`scripts/collect-release-assets.py`. Real packaging artifacts (gui packages, evidence, SignPath stage 2) are linked in; only
the `cli-<target>` artifact of the `build.yml` build-cli job is synthetic (that job did not run in the source run), marked as
such in its bytes, and different from the evidence copy so the test can tell which one was collected.

Without --baseline the new collector must pass; with --baseline (the collector before the fix) it must fail on a duplicate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CLI = 'uniclipboard-cli-1.1.1-x86_64-pc-windows-msvc.zip'


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True, help='updater-inputs from download-updater-inputs.py')
    parser.add_argument('--evidence', type=Path, required=True, help='updater-evidence from download-updater-inputs.py')
    parser.add_argument('--stage2', type=Path, required=True, help='downloaded signpath-stage2-amd64-* artifact')
    parser.add_argument('--baseline', type=Path, help='collector before the fix; expected to fail')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix='uc-collect-') as temporary:
        tree = Path(temporary) / 'artifacts'
        tree.mkdir()
        for base in (args.inputs, args.evidence):
            for artifact in sorted(base.glob('*/*')):
                shutil.copytree(artifact, tree / artifact.name, copy_function=os.link)
        shutil.copytree(args.stage2, tree / 'signpath-stage2-amd64-0-1', copy_function=os.link)
        evidence_cli = next(tree.glob('windows-gui-evidence-x86_64-*/windows-gui/cli-package/' + CLI))
        official = tree / 'cli-x86_64-pc-windows-msvc'
        official.mkdir()
        (official / CLI).write_bytes(b'SYNTHETIC build.yml build-cli artifact, distinct from the evidence copy\n')
        collector = args.baseline or ROOT / 'scripts/collect-release-assets.py'
        result = subprocess.run([sys.executable, '-I', str(collector), '--source', str(tree),
                                 '--destination', str(Path(temporary) / 'release-assets')],
                                capture_output=True, text=True)
        (out / 'collector.log').write_text(result.stdout + result.stderr)
        report = {'collector': str(collector.relative_to(ROOT)) if collector.is_relative_to(ROOT) else 'baseline',
                  'exit': result.returncode, 'evidenceCliSha256': sha256(evidence_cli),
                  'officialCliSha256': sha256(official / CLI)}
        if args.baseline:
            assert result.returncode != 0 and 'duplicate release asset' in result.stderr, result.stderr
        else:
            assert result.returncode == 0, result.stderr
            collected = Path(temporary) / 'release-assets'
            names = sorted(f.name for f in collected.iterdir())
            assert sha256(collected / CLI) == report['officialCliSha256'], 'CLI must come from the cli-<target> artifact'
            assert not any(n.startswith('ACCEPTANCE-') for n in names)
            setup = collected / 'UniClipboard_1.1.1_x64-setup.exe'
            final = next(tree.glob('windows-gui-x86_64-*/UniClipboard_1.1.1_x64-setup.exe'))
            assert sha256(setup) == sha256(final), 'installer must come from the final windows-gui artifact'
            report['collected'] = {n: sha256(collected / n) for n in names}
    report['ok'] = True
    (out / 'result.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'ok': True, 'exit': report['exit']}))


if __name__ == '__main__':
    main()
