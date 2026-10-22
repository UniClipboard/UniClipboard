#!/usr/bin/env python3
"""Publish preserved bytes, rejecting conflicting assets and resuming partial uploads."""
import hashlib
import json
from pathlib import Path
import subprocess

from recover_release_artifacts import api, lock, require, verify_tag

verify_tag()
repo, tag = lock['repository'], lock['tag']
assets = {p.name: p for p in Path('release-assets').iterdir() if p.is_file()}
require('SHA256SUMS.txt.minisig' in assets, 'missing authenticated checksums')
# Verify the preserved archive again before the first external write.
subprocess.run(['sha256sum', '--check', 'SHA256SUMS.txt'], cwd='release-assets', check=True)
marker = f'<!-- recovery:{lock["runId"]}:{lock["sourceSha"]} -->'
notes = Path('release-notes.md').read_text() + '\n' + marker + '\n'
Path('release-notes.md').write_text(notes)
result = subprocess.run(['gh', 'api', f'repos/{repo}/releases/tags/{tag}'], capture_output=True, text=True)
if result.returncode:
    # Distinguish absent release from auth/network failure. Never create on an ambiguous read.
    require('HTTP 404' in result.stderr, 'cannot establish release absence')
    subprocess.run(['gh', 'release', 'create', tag, '--repo', repo, '--verify-tag', '--draft',
                    '--prerelease', '--title', tag, '--notes-file', 'release-notes.md'], check=True)
release = api(f'releases/tags/{tag}')
require(release['tag_name'] == tag and release['prerelease'] and marker in (release['body'] or ''),
        'existing release is not this authorized recovery')


def listed_assets():
    return json.loads(subprocess.check_output(['gh', 'api', '--paginate', '--slurp',
                      f'repos/{repo}/releases/{release["id"]}/assets?per_page=100']))


def compare(remote):
    for item in remote:
        require(item['name'] in assets, f'unexpected release asset: {item["name"]}')
        local = assets[item['name']]
        with local.open('rb') as stream:
            expected = 'sha256:' + hashlib.file_digest(stream, 'sha256').hexdigest()
        require(item['state'] == 'uploaded' and item['size'] == local.stat().st_size
                and item.get('digest') == expected, f'conflicting release asset: {item["name"]}')


remote = [item for page in listed_assets() for item in page]
compare(remote)
existing = {item['name'] for item in remote}
for name, local in sorted(assets.items()):
    if name not in existing:
        subprocess.run(['gh', 'release', 'upload', tag, str(local), '--repo', repo], check=True)
remote = [item for page in listed_assets() for item in page]
compare(remote)
require({a['name'] for a in remote} == set(assets), 'release assets incomplete')
if release['draft']:
    subprocess.run(['gh', 'release', 'edit', tag, '--repo', repo, '--draft=false'], check=True)
Path('publication-receipt.json').write_text(json.dumps({
    'originRunId': lock['runId'], 'sourceSha': lock['sourceSha'], 'tag': tag,
    'url': release['html_url'], 'assets': remote,
}, indent=2) + '\n')
print(release['html_url'])
