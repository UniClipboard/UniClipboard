#!/usr/bin/env python3
"""Download already signed real inputs from a LOCAL generated feed via Client.

No secret input, installer execution, production service write or release.
A subset of platforms is deliberate when upstream packaging is incomplete.
"""
import argparse
import http.server
import json
from pathlib import Path
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[3]
GUI = ROOT / 'apps/gui-go'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    assets = args.assets.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    probe = out / 'probe'
    subprocess.run(['go', 'build', '-o', str(probe), './e2e/signatureprobe'], cwd=GUI, check=True)

    class FeedHandler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **kw):
            super().__init__(*a, directory=str(out), **kw)
        def translate_path(self, path):
            if path.startswith('/artifacts/'):
                return str(assets / Path(path).name)
            return super().translate_path(path)
        def log_message(self, *a):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), FeedHandler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    version = json.loads((GUI / 'app.json').read_text())['version']
    try:
        (out / 'notes.md').write_text('Isolated production-key acceptance; not published.')
        (out / 'notes.zh.md').write_text('生产密钥隔离验收；未发布。')
        subprocess.run(['node', 'scripts/assemble-update-manifest.js', '--version', version,
                        '--artifacts-dir', str(assets), '--output', str(out / 'manifest.json'),
                        '--base-url', base + '/artifacts', '--notes-file', str(out / 'notes.md'),
                        '--zh-notes-file', str(out / 'notes.zh.md')], cwd=ROOT, check=True)
        subprocess.run(['node', 'scripts/build-flare-release-registration.js', '--version', version,
                        '--channel', 'stable', '--manifest', str(out / 'manifest.json'),
                        '--artifacts-dir', str(assets), '--output', str(out / 'registration.json'),
                        '--source', 'isolated-updater-acceptance'], cwd=ROOT, check=True)
        result = subprocess.run([str(probe), 'consume', str(GUI / 'app.json'), base + '/manifest.json'],
                                check=True, capture_output=True, text=True)
        (out / 'consumer.json').write_text(result.stdout)
        print(result.stdout)
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
