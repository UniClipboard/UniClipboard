#!/usr/bin/env python3
"""Download named packages from completed same-repository Build Desktop runs.

The workflow operator chooses immutable run IDs. This does not infer packaging
source from run head_sha: per-package provenance is downloaded alongside bytes.
"""
import json
import os
from pathlib import Path
import re
import subprocess


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


# The Linux packages are produced by package-linux-gui.yml, which build.yml calls and which can also be dispatched alone.
TRUSTED_WORKFLOWS = ('.github/workflows/build.yml', '.github/workflows/package-linux-gui.yml')
# Artifact names are fixed by the packaging jobs: macOS and Windows by Rust target triple (plus the build-mode and
# Windows signing suffixes), Linux by package architecture.
GUI_ARTIFACT = re.compile(
    r'(?:macos-gui-(?:aarch64|x86_64)-apple-darwin(?:-test)?'
    r'|windows-gui-(?:aarch64|x86_64)-pc-windows-msvc(?:-test)?(?:-signpath-test)?'
    r'|linux-gui-(?:amd64|arm64))')
EVIDENCE_ARTIFACT = re.compile(r'(?:macos|windows|linux)-gui-evidence-.+')

repo = os.environ['GITHUB_REPOSITORY']
run_ids = os.environ['ARTIFACT_RUNS'].split(',')
if not run_ids or any(not re.fullmatch(r'[0-9]+', i.strip()) for i in run_ids):
    raise SystemExit('ARTIFACT_RUNS must be comma-separated workflow run IDs')
root = Path('updater-inputs')
root.mkdir(exist_ok=False)
# Packaging evidence stays outside `updater-inputs`: the release asset collector scans that tree for named distributables,
# and the Windows evidence carries a copy of the signed CLI archive that must never be collected twice.
evidence_root = Path('updater-evidence')
evidence_root.mkdir(exist_ok=False)
metadata = []
for item in run_ids:
    run_id = item.strip()
    run = json.loads(gh('api', f'repos/{repo}/actions/runs/{run_id}'))
    if (run['conclusion'] != 'success' or run['path'] not in TRUSTED_WORKFLOWS
            or run['event'] not in ('workflow_dispatch', 'push', 'workflow_call')
            or run['repository']['full_name'] != repo
            or run['head_repository']['full_name'] != repo):
        raise SystemExit(f'Run {run_id} is not a successful trusted Build Desktop run')
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repo}/actions/runs/{run_id}/artifacts'))
    artifacts = [a for page in pages for a in page['artifacts']]
    selected = [a for a in artifacts if GUI_ARTIFACT.fullmatch(a['name'])]
    if not selected or any(a['expired'] for a in selected):
        raise SystemExit(f'Run {run_id} has no unexpired named GUI artifacts')
    dest = root / run_id
    dest.mkdir()
    for artifact in selected:
        gh('run', 'download', run_id, '--repo', repo, '--name', artifact['name'],
           '--dir', str(dest / artifact['name']))
    evidence = [a for a in artifacts if EVIDENCE_ARTIFACT.fullmatch(a['name']) and not a['expired']]
    for artifact in evidence:
        gh('run', 'download', run_id, '--repo', repo, '--name', artifact['name'],
           '--dir', str(evidence_root / run_id / artifact['name']))
    metadata.append({'id': run_id, 'url': run['html_url'], 'headSha': run['head_sha'],
                     'event': run['event'], 'runAttempt': run['run_attempt'], 'artifacts': selected,
                     'packagingEvidence': evidence})
Path('updater-source.json').write_text(json.dumps(metadata, indent=2) + '\n')
