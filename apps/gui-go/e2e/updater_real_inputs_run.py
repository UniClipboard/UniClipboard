#!/usr/bin/env python3
"""Real Go packages -> collector -> signer -> single generator -> local HTTP feed -> Go consumer.

The inputs are the CI artifacts of real packaging runs (`scripts/ci/download-updater-inputs.py` layout: one directory per
artifact). Only the signing key is disposable (generated here, encrypted like the Tauri key, kept outside the evidence
directory), so this proves the pipeline on the real bytes of all six platforms but NOT the production key, the real
service registration, an installation or a native OS. Large package bytes stay in a temporary directory; the evidence
holds their names, sizes and SHA-256 plus the signatures, feed, registration payload and logs.
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
KEYS = {'darwin-aarch64', 'darwin-x86_64', 'linux-aarch64', 'linux-x86_64', 'windows-aarch64', 'windows-x86_64'}
SIGNED = ('.app.tar.gz', '.AppImage.tar.gz', '-setup.exe')


def sha256(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True, help='directory holding the downloaded CI artifact directories')
    parser.add_argument('--out', type=Path, required=True, help='new evidence directory')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    checks = []

    def run(label, cmd, *, env=None, cwd=ROOT, ok=True):
        result = subprocess.run(list(map(str, cmd)), cwd=cwd, env=env, capture_output=True, text=True)
        (out / (label + '.log')).write_text(result.stdout + result.stderr)
        checks.append({'name': label, 'exit': result.returncode, 'ok': (result.returncode == 0) == ok})
        (out / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
        assert checks[-1]['ok'], label
        return result.stdout

    version = json.loads((GUI / 'app.json').read_text())['version']
    with tempfile.TemporaryDirectory(prefix='uc-updater-real-') as temporary:
        work = Path(temporary)
        signer, probe = work / 'signer', work / 'probe'
        run('build-signer', ['go', 'build', '-o', signer, './cmd/updater-sign'], cwd=GUI)
        run('build-probe', ['go', 'build', '-o', probe, './e2e/signatureprobe'], cwd=GUI)
        run('fixture-key', [probe, 'fixture-key', work / 'secret'])
        config = work / 'secret/app.json'
        env = dict(os.environ, TAURI_SIGNING_PRIVATE_KEY=base64.b64encode((work / 'secret/key').read_bytes()).decode(),
                   TAURI_SIGNING_PRIVATE_KEY_PASSWORD='disposable-e2e-password')
        assets = work / 'assets'
        run('collect-named', ['python3', 'scripts/collect-release-assets.py', '--source', args.inputs, '--destination', assets])
        collected = sorted(f.name for f in assets.iterdir())
        signable = [n for n in collected if n.endswith(SIGNED)]
        assert len(signable) == 6, f'expected six updater archives, got {signable}'
        inputs = {n: {'size': (assets / n).stat().st_size, 'sha256': sha256(assets / n)} for n in collected}
        (out / 'inputs.json').write_text(json.dumps(inputs, indent=2) + '\n')

        cmd = [signer, '--app-config', config, '--artifacts-dir', assets]
        run('sign', cmd + ['--evidence', out / 'signatures.json'], env=env)
        run('verify-without-secret', cmd + ['--verify-only'], env=dict(env, TAURI_SIGNING_PRIVATE_KEY=''))
        signatures = out / 'signatures'
        signatures.mkdir()
        for name in signable:
            (signatures / (name + '.sig')).write_bytes((assets / (name + '.sig')).read_bytes())
        notes, zh = work / 'notes.md', work / 'notes.zh.md'
        notes.write_text('Real-package pipeline acceptance with a disposable key; not published.')
        zh.write_text('真实安装包流水线验收，使用一次性密钥；未发布。')

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *a, **kw):
                super().__init__(*a, directory=str(work), **kw)
            def log_message(self, *a):
                pass
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_port}'
        try:
            manifest = work / 'manifest.json'
            run('manifest', ['node', 'scripts/assemble-update-manifest.js', '--version', version, '--require-all-platforms',
                             '--artifacts-dir', assets, '--output', manifest, '--base-url', base + '/assets',
                             '--notes-file', notes, '--zh-notes-file', zh])
            assert set(json.loads(manifest.read_text())['platforms']) == KEYS
            run('registration', ['node', 'scripts/build-flare-release-registration.js', '--version', version,
                                 '--channel', 'stable', '--manifest', manifest, '--artifacts-dir', assets,
                                 '--output', out / 'registration.json', '--source', 'isolated-real-input-acceptance'])
            consumed = json.loads(run('consume-six', [probe, 'consume', config, base + '/manifest.json']))
            assert set(consumed) == KEYS
            by_key = {}
            for key, entry in consumed.items():
                assert entry['verified'] and entry['tamperRejected'], key
                by_key[key] = next(n for n in signable if (assets / n).stat().st_size and inputs[n]['sha256'] == entry['sha256'])
            assert len(set(by_key.values())) == 6, 'each platform must download its own archive'
            (out / 'platform-artifacts.json').write_text(json.dumps(by_key, indent=2, sort_keys=True) + '\n')
            # Bytes changed after signing must be refused by the production-path verifier.
            victim = assets / signable[0]
            original = victim.read_bytes()
            victim.write_bytes(original + b'x')
            run('tampered-after-signing', cmd + ['--verify-only'], ok=False)
            victim.write_bytes(original)
            run('final-verification', cmd + ['--verify-only'])
        finally:
            server.shutdown()
            server.server_close()
        (out / 'manifest.json').write_bytes(manifest.read_bytes())

    (out / 'scope.json').write_text(json.dumps({
        'productionKey': False, 'disposableKey': True, 'realPackages': True, 'sixPlatformFeed': True,
        'localFeed': True, 'serviceRegistration': False, 'installed': False, 'nativeOSCoverage': False}, indent=2) + '\n')
    (out / 'SHA256SUMS.txt').write_text(''.join(
        f'{sha256(f)}  {f.relative_to(out)}\n' for f in sorted(out.rglob('*')) if f.is_file()))
    print(json.dumps({'ok': True, 'platforms': sorted(KEYS), 'out': str(out)}))


if __name__ == '__main__':
    main()
