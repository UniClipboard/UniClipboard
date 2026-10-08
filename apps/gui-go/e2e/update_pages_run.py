#!/usr/bin/env python3
"""Real HTTP/subprocess E2E for primary snapshots and Pages comparisons; no push."""
import argparse
import http.server
import json
from pathlib import Path
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / 'scripts/sync-update-pages.py'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True)
    out = p.parse_args().out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    payload = {'version': '1.1.1', 'notes': 'fixture', 'pub_date': '2026-10-08T00:00:00Z',
               'confirmation_required': True, 'confirmation_description': 'Read first',
               'platforms': {'linux-aarch64': {'url': 'https://example.invalid/artifact', 'signature': 'fixture'}}}
    state = {'primary': {c: payload for c in ('stable', 'alpha', 'beta', 'rc')},
             'fallback': {c: payload for c in ('stable', 'alpha', 'beta', 'rc')}}
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            _, role, filename = self.path.split('/')
            data = state[role][filename[:-5]]
            status = (204 if role == 'primary' else 404) if data is None else 200
            self.send_response(status)
            self.end_headers()
            if data is not None:
                self.wfile.write(json.dumps(data).encode())
        def log_message(self, *args):
            pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}'
    checks = []
    def run(name, mode, directory, expected=True, compare=False):
        cmd = ['python3', '-I', str(SCRIPT), mode, '--directory', str(directory), '--primary', base + '/primary']
        if compare:
            cmd += ['--fallback', base + '/fallback']
        r = subprocess.run(cmd, text=True, capture_output=True)
        (out / (name + '.log')).write_text(r.stdout + r.stderr)
        checks.append({'name': name, 'exit': r.returncode, 'ok': (r.returncode == 0) == expected})
        (out / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
        assert checks[-1]['ok'], name
    try:
        snap = out / 'snapshot'
        run('snapshot', 'snapshot', snap)
        run('equal', 'check', snap, compare=True)
        state['fallback']['stable'] = dict(payload, confirmation_description='Changed')
        run('confirmation-diff', 'check', snap, expected=False, compare=True)
        state['fallback']['stable'] = payload
        state['primary']['alpha'] = dict(payload, version='1.1.2')
        run('primary-race', 'check', snap, expected=False)
        state['primary']['alpha'] = payload
        state['primary']['alpha'] = None
        withdrawn = out / 'withdrawal'
        run('withdrawal-snapshot', 'snapshot', withdrawn)
        run('stale-withdrawn-fallback', 'check', withdrawn, expected=False, compare=True)
        state['fallback']['alpha'] = None
        run('withdrawn-fallback-absent', 'check', withdrawn, compare=True)
        assert not (withdrawn / 'alpha.json').exists()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
