#!/usr/bin/env python3
"""Download only digest-pinned archives from the authorized original release run."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

control = Path(__file__).resolve().parents[2]
lock = json.loads((control / '.github/release-recovery/alpha2.json').read_text())
repo, run_id = lock['repository'], lock['runId']


def api(path):
    return json.loads(subprocess.check_output(['gh', 'api', f'repos/{repo}/{path}']))


def require(condition, message):
    if not condition:
        sys.exit(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify_tag():
    obj = api(f'git/ref/tags/{lock["tag"]}')['object']
    while obj['type'] == 'tag':
        obj = api(f'git/tags/{obj["sha"]}')['object']
    require(obj['type'] == 'commit' and obj['sha'] == lock['sourceSha'], 'tag source changed')



def download_artifact(item, expected_run, source_sha, destination):
    artifact = api(f'actions/artifacts/{item["id"]}')
    require(not artifact['expired'] and artifact['name'] == item['name']
            and artifact['digest'] == item['digest']
            and artifact['workflow_run']['id'] == expected_run
            and artifact['workflow_run']['head_sha'] == source_sha,
            f'artifact provenance mismatch: {item["name"]}')
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / 'artifact.zip'
        with archive.open('wb') as output:
            subprocess.run(['gh', 'api', f'repos/{repo}/actions/artifacts/{item["id"]}/zip'],
                           stdout=output, check=True)
        require('sha256:' + digest(archive) == item['digest'], f'archive digest mismatch: {item["name"]}')
        destination = destination.resolve()
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.infolist():
                target = (destination / member.filename).resolve()
                require(target.is_relative_to(destination), 'unsafe artifact archive path')
                require((member.external_attr >> 16) & 0o170000 != 0o120000, 'artifact symlink refused')
            bundle.extractall(destination)

def main():
    if '--prepared' in sys.argv:
        prepared = lock['prepared']
        verify_tag()
        run = api(f'actions/runs/{prepared["runId"]}')
        require(run['head_sha'] == prepared['controlSha'] and run['path'] == '.github/workflows/release.yml',
                'prepared publication workflow provenance changed')
        jobs = api(f'actions/runs/{prepared["runId"]}/jobs?per_page=100')['jobs']
        require(any(j['name'] == 'recover-existing / prepare' and j['conclusion'] == 'success' for j in jobs),
                'prepared publication did not pass production preparation')
        download_artifact(prepared['artifact'], prepared['runId'], prepared['controlSha'], Path('publication'))
        record = json.loads(Path('publication/assembly/evidence/source-record.json').read_text())
        require(record['sourceSha'] == lock['sourceSha'] and record['version'] == lock['version']
                and record['mode'] == 'release', 'prepared source record mismatch')
        print('Reused digest-pinned production publication bytes; no signing was repeated')
    elif '--verify-packages' in sys.argv:
        original = json.loads(Path('artifacts/updater-verification/assembly-index.json').read_text())
        current = json.loads(Path('assembly/evidence/assembly-index.json').read_text())
        for field in ['version', 'sourceSha', 'channel', 'enginePin', 'windowsSigning']:
            require(original[field] == current[field], f'original assembly mismatch: {field}')
        require(set(original['files']) == set(current['files']), 'original asset set changed')
        packages = {n: h for n, h in original['files'].items() if not n.endswith('.sig')}
        for name, expected in packages.items():
            require(digest(Path('release-assets') / name) == expected, f'package bytes changed: {name}')
        Path('assembly/evidence/original-package-comparison.json').write_text(
            json.dumps({'runId': run_id, 'sourceSha': lock['sourceSha'], 'unchangedPackages': packages}, indent=2) + '\n')
        print(f'Original package byte comparison passed: {len(packages)} packages')
    else:
        require(lock['channel'] == 'alpha' and lock['version'] == '1.4.0-alpha.2', 'wrong recovery target')
        require(len(lock['artifacts']) == 54, 'incomplete original artifact lock')
        verify_tag()
        run = api(f'actions/runs/{run_id}')
        require(run['head_sha'] == lock['sourceSha'] and run['event'] == 'push'
                and run['path'] == '.github/workflows/release.yml' and run['run_attempt'] == 1
                and run['conclusion'] == 'failure', 'original run provenance changed')
        inventory = api(f'actions/runs/{run_id}/artifacts?per_page=100')
        require(inventory['total_count'] == len(inventory['artifacts']) == len(lock['artifacts'])
                and {a['id'] for a in inventory['artifacts']} == {a['id'] for a in lock['artifacts']},
                'original artifact inventory changed or is incomplete')
        # Each item is fetched by immutable ID after the complete inventory check.
        root = Path('artifacts')
        root.mkdir(exist_ok=False)
        receipts = []
        for item in lock['artifacts']:
            download_artifact(item, run_id, lock['sourceSha'], root / item['name'])
            receipts.append(item)
            print(f'Verified original artifact: {item["name"]}', flush=True)
        Path('artifact-provenance.json').write_text(json.dumps({'run': run, 'artifacts': receipts}, indent=2) + '\n')


if __name__ == '__main__':
    main()
