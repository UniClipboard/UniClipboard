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


repo = os.environ['GITHUB_REPOSITORY']
run_ids = os.environ['ARTIFACT_RUNS'].split(',')
if not run_ids or any(not re.fullmatch(r'[0-9]+', i.strip()) for i in run_ids):
    raise SystemExit('ARTIFACT_RUNS must be comma-separated workflow run IDs')
root = Path('updater-inputs')
root.mkdir(exist_ok=False)
metadata = []
for item in run_ids:
    run_id = item.strip()
    run = json.loads(gh('api', f'repos/{repo}/actions/runs/{run_id}'))
    if (run['conclusion'] != 'success' or run['path'] != '.github/workflows/build.yml'
            or run['event'] not in ('workflow_dispatch', 'push', 'workflow_call')
            or run['repository']['full_name'] != repo
            or run['head_repository']['full_name'] != repo):
        raise SystemExit(f'Run {run_id} is not a successful trusted Build Desktop run')
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repo}/actions/runs/{run_id}/artifacts'))
    artifacts = [a for page in pages for a in page['artifacts']]
    selected = [a for a in artifacts if re.fullmatch(
        r'(?:macos-gui|linux-gui|windows-gui)-(?:aarch64|x86_64)-(?:apple-darwin|unknown-linux-gnu|pc-windows-msvc)(?:-test)?', a['name'])]
    if not selected or any(a['expired'] for a in selected):
        raise SystemExit(f'Run {run_id} has no unexpired named GUI artifacts')
    dest = root / run_id
    dest.mkdir()
    for artifact in selected:
        gh('run', 'download', run_id, '--repo', repo, '--name', artifact['name'],
           '--dir', str(dest / artifact['name']))
    evidence = [a for a in artifacts if a['name'].startswith('macos-gui-evidence-') and not a['expired']]
    for artifact in evidence:
        gh('run', 'download', run_id, '--repo', repo, '--name', artifact['name'],
           '--dir', str(dest / artifact['name']))
    metadata.append({'id': run_id, 'url': run['html_url'], 'headSha': run['head_sha'],
                     'event': run['event'], 'runAttempt': run['run_attempt'], 'artifacts': selected,
                     'packagingEvidence': evidence})
Path('updater-source.json').write_text(json.dumps(metadata, indent=2) + '\n')
