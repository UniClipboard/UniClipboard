#!/usr/bin/env python3
"""Real-process signer -> single generator -> local HTTP feed -> Go consumer.

Encrypted disposable key and synthetic six-platform payloads only. No product install,
production key, real service registration or OS coverage is claimed. Secret fixtures
live in a temporary directory and never enter the evidence directory.
"""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[3]
GUI = ROOT / 'apps/gui-go'
NAMES = [
    'UniClipboard_aarch64-apple-darwin.app.tar.gz',
    'UniClipboard_x86_64-apple-darwin.app.tar.gz',
    'UniClipboard_1.1.1_amd64.AppImage.tar.gz',
    'UniClipboard_1.1.1_aarch64.AppImage.tar.gz',
    'UniClipboard_1.1.1_x64-setup.exe',
    'UniClipboard_1.1.1_arm64-setup.exe',
]
KEYS = {'darwin-aarch64', 'darwin-x86_64', 'linux-aarch64', 'linux-x86_64',
        'windows-aarch64', 'windows-x86_64'}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    out = a.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    checks = []

    def run(label, cmd, *, env=None, cwd=ROOT, ok=True):
        r = subprocess.run(list(map(str, cmd)), cwd=cwd, env=env, capture_output=True, text=True)
        # Commands contain paths only. Secret input is exclusively inherited environment.
        (out / (label + '.log')).write_text(r.stdout + r.stderr)
        checks.append({'name': label, 'exit': r.returncode, 'ok': (r.returncode == 0) == ok})
        (out / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
        assert checks[-1]['ok'], label
        return r.stdout

    with tempfile.TemporaryDirectory(prefix='uc-updater-key-') as secret_dir:
        secret = Path(secret_dir)
        signer, probe = secret / 'signer', secret / 'probe'
        run('build-signer', ['go', 'build', '-o', signer, './cmd/updater-sign'], cwd=GUI)
        run('build-probe', ['go', 'build', '-o', probe, './e2e/signatureprobe'], cwd=GUI)
        run('fixture-key', [probe, 'fixture-key', secret])
        config = secret / 'app.json'
        encoded = (secret / 'key').read_bytes()
        env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY=base64.b64encode(encoded).decode(),
                   TAURI_SIGNING_PRIVATE_KEY_PASSWORD='disposable-e2e-password')
        assets = out / 'assets'
        assets.mkdir()
        for name in NAMES:
            (assets / name).write_text('SYNTHETIC E2E payload: ' + name + '\n')
        cmd = [signer, '--app-config', config, '--artifacts-dir', assets]
        run('missing-key', cmd, env=dict(env, TAURI_SIGNING_PRIVATE_KEY=''), ok=False)
        run('wrong-password', cmd, env=dict(env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD='wrong'), ok=False)
        run('malformed-key', cmd, env=dict(env, TAURI_SIGNING_PRIVATE_KEY='invalid'), ok=False)
        run('sign-tauri-base64', cmd + ['--evidence', out / 'signatures.json'], env=env)
        run('sign-minisign-text', cmd, env=dict(env, TAURI_SIGNING_PRIVATE_KEY=encoded.decode()))
        run('verify-no-secret', cmd + ['--verify-only'], env=dict(env, TAURI_SIGNING_PRIVATE_KEY=''))
        wrong_config = secret / 'wrong.json'
        run('second-key', [probe, 'fixture-key', secret / 'other'])
        wrong_config.write_bytes((secret / 'other/app.json').read_bytes())
        run('wrong-public-key', [signer, '--app-config', wrong_config, '--artifacts-dir', assets], env=env, ok=False)

        inputs = out / 'build-inputs'
        inputs.mkdir()
        # The collector accepts only actual distributable names, even when raw
        # executables and packaging evidence arrive in the same CI artifact.
        for file in assets.iterdir():
            if not file.name.endswith('.sig') and not file.name.endswith('.app.tar.gz'):
                (inputs / file.name).write_bytes(file.read_bytes())
        for target in ('aarch64-apple-darwin', 'x86_64-apple-darwin'):
            folder = inputs / ('macos-gui-' + target)
            folder.mkdir()
            source = assets / ('UniClipboard_' + target + '.app.tar.gz')
            (folder / 'UniClipboard.app.tar.gz').write_bytes(source.read_bytes())
        (inputs / 'UniClipboard.exe').write_text('raw executable must not publish')
        (inputs / 'package-manifest.json').write_text('{}')
        collected = out / 'collected'
        run('collect-named', ['python3', 'scripts/collect-release-assets.py', '--source', inputs, '--destination', collected])
        assert set(f.name for f in collected.iterdir()) == set(NAMES)
        duplicate = inputs / 'duplicate'
        duplicate.mkdir()
        (duplicate / NAMES[-1]).write_bytes((assets / NAMES[-1]).read_bytes())
        run('collect-duplicate', ['python3', 'scripts/collect-release-assets.py', '--source', inputs,
                                  '--destination', out / 'duplicate-refused'], ok=False)

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(out), **kwargs)
            def log_message(self, *args):
                pass
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_port}'
        try:
            notes = out / 'notes.md'
            zh = out / 'notes.zh.md'
            notes.write_text('Synthetic E2E only')
            zh.write_text('仅用于合成端到端验证')
            manifest = out / 'manifest.json'
            run('manifest', ['node', 'scripts/assemble-update-manifest.js', '--version', '1.1.1',
                            '--require-all-platforms', '--artifacts-dir', assets, '--output', manifest, '--base-url', base + '/assets',
                            '--notes-file', notes, '--zh-notes-file', zh])
            assert set(json.loads(manifest.read_text())['platforms']) == KEYS
            missing_asset = assets / NAMES[-1]
            missing_signature = Path(str(missing_asset) + '.sig')
            saved_signature = missing_signature.read_bytes()
            missing_signature.unlink()
            run('incomplete-six-refused', ['node', 'scripts/assemble-update-manifest.js', '--version', '1.1.1',
                '--require-all-platforms', '--artifacts-dir', assets, '--output', out / 'incomplete.json',
                '--base-url', base + '/assets'], ok=False)
            missing_signature.write_bytes(saved_signature)
            run('registration', ['node', 'scripts/build-flare-release-registration.js', '--version', '1.1.1',
                                '--channel', 'stable', '--manifest', manifest, '--artifacts-dir', assets,
                                '--output', out / 'registration.json'])
            run('download-six', [probe, 'consume', config, base + '/manifest.json'])
            first = assets / NAMES[0]
            original = first.read_bytes()
            first.write_bytes(original + b'tampered')
            run('tampered-download', [probe, 'consume', config, base + '/manifest.json'], ok=False)
            first.write_bytes(original)
            sig = Path(str(first) + '.sig')
            original_sig = sig.read_bytes()
            text = base64.b64decode(original_sig).decode()
            text = text.replace('trusted comment: timestamp:', 'trusted comment: changed:')
            sig.write_bytes(base64.b64encode(text.encode()))
            run('tampered-comment', cmd + ['--verify-only'], ok=False)
            sig.write_bytes(original_sig)
            run('final-verification', cmd + ['--verify-only'])
        finally:
            server.shutdown()
            server.server_close()
        for file in out.rglob('*'):
            if file.is_file():
                # Only synthetic artifacts, signatures and logs are retained.
                assert encoded not in file.read_bytes(), 'private fixture accidentally archived'
        (out / 'SHA256SUMS.txt').write_text(''.join(
            f'{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.relative_to(out)}\n'
            for f in sorted(out.rglob('*')) if f.is_file()))
        (out / 'scope.json').write_text(json.dumps({'productionKey': False, 'realPackages': False,
            'localFeed': True, 'sixExplicitConsumerTargets': True, 'nativeSixOSCoverage': False,
            'serviceRegistration': False, 'installed': False}, indent=2) + '\n')


if __name__ == '__main__':
    main()
