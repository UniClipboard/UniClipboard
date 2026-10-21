#!/usr/bin/env python3
"""Prepare hash-pinned historical inputs for the real settings-migration E2E."""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
ASSET = 'UniClipboard_aarch64-apple-darwin.app.tar.gz'
ASSET_SHA = '10d68f789960973a525d6dac47909a85db033c55cf963ef5552bf8ccf04b537e'
FIXTURE_SHA = '362533af8f3443094f85f973ba52d97f5a2c19a0a60525baed947d804353878f'
TAG_COMMIT = '4438ff72bbf97fe9b2bdc23a0cf1ea67631b26fc'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--github-env', type=Path)
    args = parser.parse_args()
    if (platform.system(), platform.machine()) != ('Darwin', 'arm64'):
        raise RuntimeError('The pinned v1.2.0 release input requires macOS arm64')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    archive = out / ASSET
    url = f'https://github.com/UniClipboard/UniClipboard/releases/download/v1.2.0/{ASSET}'
    if not archive.exists():
        with urllib.request.urlopen(url, timeout=120) as response:
            data = response.read()
        if sha(data) != ASSET_SHA:
            raise RuntimeError('Historical release SHA-256 mismatch')
        archive.write_bytes(data)
    if sha(archive.read_bytes()) != ASSET_SHA:
        raise RuntimeError('Cached historical release SHA-256 mismatch')
    with tarfile.open(archive) as bundle:
        # Extract only the daemon; never install or start the GUI application.
        member = bundle.getmember('UniClipboard.app/Contents/MacOS/uniclipd')
        if not member.isfile():
            raise RuntimeError('Historical daemon must be a regular archive member')
        daemon_bytes = bundle.extractfile(member).read()
    daemon = out / 'uniclipd'
    daemon.write_bytes(daemon_bytes)
    daemon.chmod(0o755)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    engine = next(p for p in metadata['packages'] if p['name'] == 'uc-engine')
    engine_root = Path(engine['manifest_path']).parents[2]
    relative = 'tests/upgrade-matrix/fixtures/desktop-0.19.4/profile/settings.json'
    fixture_bytes = (engine_root / relative).read_bytes()
    if sha(fixture_bytes) != FIXTURE_SHA:
        raise RuntimeError('Engine 0.19.4 settings fixture SHA-256 mismatch')
    fixture = out / 'desktop-0.19.4-settings.json'
    fixture.write_bytes(fixture_bytes)
    environment = {'UC_E2E_V1_2_0_DAEMON': str(daemon),
                   'UC_E2E_DESKTOP_0_19_4_SETTINGS': str(fixture),
                   'UC_E2E_RUST_LOG': 'info'}
    (out / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
    (out / 'source.json').write_text(json.dumps({
        'releaseTag': 'v1.2.0', 'tagCommit': TAG_COMMIT, 'url': url,
        'archiveSha256': ASSET_SHA, 'daemonSha256': sha(daemon_bytes),
        'engineSource': engine['source'], 'fixturePath': relative,
        'fixtureSha256': FIXTURE_SHA,
    }, indent=2) + '\n')
    if args.github_env:
        with args.github_env.open('a') as output:
            for key, value in environment.items():
                if '\n' in value or '\r' in value:
                    raise RuntimeError('Invalid environment path')
                output.write(f'{key}={value}\n')
    print(out / 'source.json')


if __name__ == '__main__':
    main()
