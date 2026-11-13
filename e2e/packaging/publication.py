#!/usr/bin/env python3
"""Exercise real release subprocesses and boto3 against disposable HTTP/S3 services."""
import argparse
import copy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument('--output', default=str(ROOT / '.cache/publication-e2e'))
a = p.parse_args()
out = Path(a.output).resolve(); out.mkdir(parents=True, exist_ok=True)
report = {'status': 'running', 'checks': [], 'scope': 'real boto3 conditional writes and release HTTP process fixture; no production credentials or mutations'}
sys.path.insert(0, str(ROOT / 'tools/release'))
import desktop_release as release
report['source_sha'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
report['source_files'] = {str(f.relative_to(ROOT)): hashlib.sha256(f.read_bytes()).hexdigest() for f in [ROOT / 'tools/release/desktop_release.py', ROOT / 'tools/release/requirements.txt', Path(__file__)]}

try:
    with tempfile.TemporaryDirectory(prefix='unic-publication-e2e-') as temporary:
        temp = Path(temporary)
        records = temp / 'records'; records.mkdir()
        metadata = temp / 'products.toml'
        metadata.write_text('schema=1\n[products.desktop]\nversion="2.0.0"\nbuild_number=1\ncli_manifest_sha256="' + 'b' * 64 + '"\ntargets=' + json.dumps(release.TARGETS) + '\n')
        for target in release.TARGETS:
            for kind in release.FORMATS[target.split('-')[0]]:
                directory = records / target / kind; directory.mkdir(parents=True)
                name = target + '.' + kind
                blob = ('fixture ' + name).encode()
                (directory / name).write_bytes(blob)
                release.save(directory / 'artifacts.json', {'schema': 1, 'product': 'desktop', 'version': '2.0.0', 'build_number': 1, 'source_sha': 'a' * 40, 'source_clean': False, 'development': True, 'cli': {'manifest_sha256': 'b' * 64}, 'target': target, 'acceptance': {'native': True, 'version_smoke': True}, 'artifacts': [{'file': name, 'format': kind, 'sha256': hashlib.sha256(blob).hexdigest(), 'size': len(blob), 'signature': 'fixture-only'}]})
        objects, registrations, writes = {}, [], []
        revision = 0
        contract = 'product-version-build-v1'
        inject = None
        class Server(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def send(self, status, data, content_type='application/json'):
                if isinstance(data, (dict, list)): data = json.dumps(data).encode()
                self.send_response(status); self.send_header('Content-Type', content_type); self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)
            def access(self):
                return self.headers.get('CF-Access-Client-Id') == 'fixture-id' and self.headers.get('CF-Access-Client-Secret') == 'fixture-secret'
            def do_GET(self):
                path = unquote(urlparse(self.path).path)
                if path.startswith('/api/') and not self.access(): return self.send(403, {})
                if path == '/api/contract': return self.send(200, {'buildIdentity': contract, 'products': ['desktop']})
                if path == '/api/channels/desktop/alpha': return self.send(200, {'data': {'internalRevision': revision}})
                if path == '/v1/desktop/alpha.json':
                    m = registrations[-1]
                    return self.send(200, {k: m[k] for k in ['product', 'version', 'buildNumber', 'artifacts']})
                key = path.removeprefix('/bucket/') if path.startswith('/bucket/') else path.lstrip('/')
                if key not in objects: return self.send(404, b'<Error><Code>NoSuchKey</Code></Error>', 'application/xml')
                return self.send(200, objects[key], 'application/octet-stream')
            def do_PUT(self):
                global inject
                assert self.headers.get('Authorization', '').startswith('AWS4-HMAC-SHA256 ')
                assert self.headers.get('If-None-Match') == '*'
                key = unquote(urlparse(self.path).path).removeprefix('/bucket/')
                body = self.rfile.read(int(self.headers['Content-Length']))
                if inject == 'race':
                    objects[key] = b'racing-conflict'
                    inject = None
                if key in objects: return self.send(412, b'<Error><Code>PreconditionFailed</Code></Error>', 'application/xml')
                objects[key] = body; writes.append(key)
                return self.send(200, b'', 'application/xml')
            def do_POST(self):
                global revision
                if not self.access(): return self.send(403, {})
                data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                path = unquote(self.path)
                if path == '/api/releases/register':
                    assert len(data['artifacts']) == 6
                    if registrations and data != registrations[0]: return self.send(409, {})
                    if not registrations: registrations.append(data)
                    return self.send(201, {'data': {'id': 'desktop/2.0.0/build.1'}})
                if path == '/api/channels/desktop/alpha/promote':
                    if data['expectedRevisions']['desktop/alpha'] != revision: return self.send(409, {})
                    revision += 1
                    return self.send(200, {'data': {'internalRevision': revision}})
                return self.send(404, {})
        server = ThreadingHTTPServer(('127.0.0.1', 0), Server)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{server.server_port}'
        env = dict(os.environ, AWS_ACCESS_KEY_ID='fixture-id', AWS_SECRET_ACCESS_KEY='fixture-secret', AWS_EC2_METADATA_DISABLED='true', CF_ACCESS_CLIENT_ID='fixture-id', CF_ACCESS_CLIENT_SECRET='fixture-secret')
        tool = ROOT / 'tools/release/desktop_release.py'
        def run(args, passed=True):
            r = subprocess.run([sys.executable, str(tool), *map(str, args)], cwd=ROOT, env=env, text=True, capture_output=True, timeout=90)
            if (r.returncode == 0) != passed: raise RuntimeError(r.stderr[-3000:])
        catalogue = temp / 'catalogue'
        run(['assemble', '--input', records, '--output', catalogue, '--metadata', metadata, '--development'])
        manifest = catalogue / 'release-manifest.json'
        report['checks'].append('complete target/format catalogue assembled')
        notes = temp / 'notes.json'; release.save(notes, {'en': 'Fixture', 'zh': 'Fixture'})
        publication = temp / 'publication.json'
        command = ['publish', '--manifest', manifest, '--fixture', '--admin', base, '--public', base, '--endpoint', base, '--bucket', 'bucket', '--notes', notes, '--published-at', '2026-11-13T00:00:00Z', '--receipt', publication]
        # The production path must reject this development fixture before contacting services.
        production = command.copy(); production.remove('--fixture')
        run(production, passed=False)
        assert not writes and not registrations
        report['checks'].append('development candidate rejected before production writes')
        contract = 'legacy'
        run(command, passed=False)
        assert not writes
        report['checks'].append('legacy service contract rejected before writes')
        contract = 'product-version-build-v1'
        run(command)
        assert len(writes) == 2 + sum(len(release.FORMATS[t.split("-")[0]]) for t in release.TARGETS) and len(registrations) == 1
        report['checks'].append('all 22 packages, catalogue and publication receipt uploaded and read back')
        previous = len(writes)
        run(command)
        assert len(writes) == previous
        report['checks'].append('retry preserves exact immutable bytes without overwrite')
        first = next(iter(objects)); original = objects[first]; objects[first] = b'conflict'
        run(command, passed=False)
        assert len(writes) == previous
        report['checks'].append('existing conflicting object rejected before mutation')
        objects[first] = original
        run(command)
        promote = ['promote', '--manifest', manifest, '--publication', publication, '--fixture', '--admin', base, '--public', base, '--channel', 'alpha', '--expected-revision', '0', '--receipt', temp / 'promotion.json']
        # Only a current successful read-back can authorize promotion.
        run(promote)
        assert revision == 1
        report['checks'].append('revision-bound promotion and public numbered identity read-back')
        run(promote, passed=False)
        assert revision == 1
        report['checks'].append('stale revision rejected')
        objects.clear(); writes.clear(); registrations.clear(); inject = 'race'
        run(command, passed=False)
        assert not writes and not registrations
        report['checks'].append('racing conditional writer cannot overwrite an object')
        server.shutdown(); server.server_close()
        report['status'] = 'passed'
finally:
    release.save(out / 'report.json', report)
    (out / 'SHA256SUMS').write_text(hashlib.sha256((out / 'report.json').read_bytes()).hexdigest() + '  report.json\n')
print(out / 'report.json')
