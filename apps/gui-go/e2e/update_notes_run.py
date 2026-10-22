#!/usr/bin/env python3
"""Exercise the production update client over HTTP; no install or native UI coverage."""
import argparse
import http.server
import json
from pathlib import Path
import subprocess
import threading
from urllib.parse import parse_qs, urlsplit

GUI = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    out = parser.parse_args().out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    probe = out / 'manifestprobe'
    subprocess.run(['go', 'build', '-o', str(probe), './e2e/manifestprobe'], cwd=GUI, check=True)
    requests = []
    notes = '## v1.2.1\n\nLatest\n\n## v1.2.0\n\nIntermediate\n\n<!-- zh -->\n\n## v1.2.1\n\n最新\n\n## v1.2.0\n\n中间版本'

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlsplit(self.path)
            query = parse_qs(url.query)
            requests.append({'path': url.path, 'query': query})
            if url.path == '/unavailable.json':
                self.send_response(503)
                self.end_headers()
                return
            # A version-aware feed returns the cumulative bilingual body; a
            # static fallback ignores the query and keeps single-version notes.
            body = notes if query.get('from') and url.path != '/static.json' else 'Latest only'
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({
                'version': '1.2.1', 'notes': body,
                'platforms': {'linux-x86_64': {'url': 'https://example.invalid/update', 'signature': 'fixture'}},
            }).encode())

        def log_message(self, *args):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    checks = []

    def run(name, current, channel, paths, expected_body):
        requests.clear()
        result = subprocess.run([str(probe), 'notes', current, channel, *[base + p for p in paths]],
                                text=True, capture_output=True, check=True)
        release = json.loads(result.stdout)
        (out / f'{name}.json').write_text(json.dumps({'release': release, 'requests': requests}, indent=2))
        assert all(r['query'].get('from') == [current] for r in requests), name + ': missing installed version'
        assert all(r['query'].get('source') == ['e2e'] for r in requests), name + ': lost existing query'
        if expected_body is None:
            assert release is None, name
        else:
            assert release['body'] == expected_body, name + ': wrong release notes'
            assert release['version'] == '1.2.1' and release['currentVersion'] == current, name
        checks.append(name)

    try:
        run('cumulative-stable', '1.1.9', 'stable', ['/stable.json?source=e2e'], notes)
        run('prerelease-and-encoding', 'v1.2.0-alpha.1+local', 'alpha', ['/alpha.json?source=e2e'], notes)
        run('replace-stale-from', '1.2.0', 'stable', ['/stable.json?source=e2e&from=0.1.0'], notes)
        run('static-fallback', '1.1.9', 'stable',
            ['/unavailable.json?source=e2e', '/static.json?source=e2e'], 'Latest only')
        run('already-current', '1.2.1', 'stable', ['/stable.json?source=e2e'], None)
    finally:
        server.shutdown()
        server.server_close()
        (out / 'checks.json').write_text(json.dumps(checks, indent=2))
    print(f'{len(checks)} HTTP integration scenarios passed')


if __name__ == '__main__':
    main()
