#!/usr/bin/env python3
"""Nonvisual daemon image resource E2E. Writes only synthetic measurements.

Build uniclipd and uniclip as described in history_kit_browser_daemon.py, then run
python3 apps/gui-go/e2e/thumbnail_resource_run.py --out /path/to/results.json
No GUI, browser, system clipboard, or real user profile is used.
"""
import argparse
import base64
import hashlib
import json
import os
import random
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.request
import urllib.error
import zlib
from pathlib import Path

from file_preview_run import PASSPHRASE, cli
from history_kit_browser_daemon import env_for, start, stop
from run import ROOT


def png(width, height, seed):
    rng = random.Random(seed)
    rows = b''.join(b'\0' + rng.randbytes(width * 3) for _ in range(height))
    def chunk(kind, data):
        value = kind + data
        return struct.pack('>I', len(data)) + value + struct.pack('>I', zlib.crc32(value))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows, 1)) + chunk(b'IEND', b''))


def request(base, path, method='GET', data=None, headers=None):
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers or {})
    with urllib.request.urlopen(req, timeout=120) as response:
        return response.read(), response.headers.get_content_type()


def measure_client(base, paths, authorization):
    # A fresh nonvisual process buffers each response. This measures HTTP client
    # RSS, not WebView image decoding or GUI RSS. Credentials only cross stdin.
    program = '''import json, resource, sys, urllib.request
config = json.load(sys.stdin)
buffers = []
for path in config["paths"]:
    req = urllib.request.Request(config["base"] + path, headers={"Authorization": config["authorization"]})
    with urllib.request.urlopen(req, timeout=120) as response:
        buffers.append(response.read())
peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
if sys.platform != "darwin": peak *= 1024
print(json.dumps({"peak_rss_bytes": peak, "retained_bytes": sum(map(len, buffers))}))
'''
    result = subprocess.run([sys.executable, '-c', program], input=json.dumps({
        'base': base, 'paths': paths, 'authorization': authorization,
    }), text=True, capture_output=True, check=True, timeout=180)
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    result = {'desktop_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'desktop_worktree_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
              'engine_revision': manifest['workspace']['dependencies']['uc-engine']['rev'],
              'scenario_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'gui_started': False, 'passed': False}
    with tempfile.TemporaryDirectory(prefix='uc-thumbnail-run-') as tmp:
        out = Path(tmp)
        home = None
        try:
            start(out, 0, True)
            info = json.loads((out / 'daemon.json').read_text())
            home = Path(info['home'])
            env = env_for(str(home), info['profile'])
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            setup = json.loads(cli(env, '--json', 'mobile', 'setup', '--non-interactive', '--label', 'thumbnail-e2e',
                                   '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)).stdout)
            mobile = f'http://127.0.0.1:{port}'
            auth = 'Basic ' + base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode()
            for _ in range(100):
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=1):
                        break
                except OSError:
                    time.sleep(.1)
            for index in range(3):
                name = f'synthetic-{index}.png'
                body = png(2048, 1536, index)
                request(mobile, '/file/' + name, 'PUT', body, {'Authorization': auth, 'Content-Type': 'image/png'})
                payload = json.dumps({'type': 'Image', 'text': name, 'dataName': name, 'hasData': True}).encode()
                request(mobile, '/SyncClipboard.json', 'PUT', payload, {'Authorization': auth, 'Content-Type': 'application/json'})
            base = info['daemonUrl']
            conn = json.loads(Path(info['conn']).read_text())
            hdr = {'Authorization': 'Bearer ' + conn['token'], 'Content-Type': 'application/json'}
            raw, _ = request(base, '/auth/connect', 'POST', json.dumps({'pid': os.getpid(), 'clientType': 'gui'}).encode(), hdr)
            session = json.loads(raw)['data']['sessionToken']
            hdr = {'Authorization': 'Session ' + session, 'Content-Type': 'application/json'}
            request(base, '/content-lock/unlock', 'POST', json.dumps({'passphrase': PASSPHRASE}).encode(), hdr)
            entries = []
            wait_started = time.monotonic()
            for _ in range(480):
                raw, _ = request(base, '/clipboard/entries?limit=20&offset=0', headers=hdr)
                entries = [x for x in json.loads(raw)['data'] if x['contentType'] == 'image']
                if len(entries) == 3:
                    break
                time.sleep(.5)
            result['thumbnail_wait_seconds'] = round(time.monotonic() - wait_started, 2)
            assert len(entries) == 3, f'expected three image entries, got {len(entries)}'
            images = []
            original_paths = []
            thumbnail_paths = []
            hashes = {hashlib.sha256(png(2048, 1536, i)).hexdigest() for i in range(3)}
            for entry in entries:
                for _ in range(240):
                    raw, _ = request(base, f"/clipboard/entries/{entry['id']}/resource", headers=hdr)
                    resource = json.loads(raw)['data']
                    if resource['url']:
                        break
                    time.sleep(.5)
                assert resource['url'], 'original image did not become ready'
                original, original_type = request(base, resource['url'], headers=hdr)
                thumbnail_url = f"/clipboard/entries/{entry['id']}/thumbnail"
                original_paths.append(resource['url'])
                thumbnail_paths.append(thumbnail_url)
                for attempt in range(8):
                    try:
                        thumb, thumb_type = request(base, thumbnail_url, headers=hdr)
                        break
                    except urllib.error.HTTPError as error:
                        if error.code != 503 or attempt == 7:
                            raise
                        time.sleep(min(.25 * 2 ** attempt, 4))
                images.append({'original_bytes': len(original), 'original_sha256': hashlib.sha256(original).hexdigest(),
                               'original_type': original_type, 'thumbnail_bytes': len(thumb), 'thumbnail_type': thumb_type,
                               'thumbnail_available': bool(thumbnail_url)})
            assert {image['original_sha256'] for image in images} == hashes, 'original image bytes changed'
            raw, _ = request(base, '/search/query?query=&limit=20', headers=hdr)
            search = json.loads(raw)['data']['items']
            image_search = [item for item in search if item['contentType'] == 'image']
            result['search_image_count'] = len(image_search)
            result['search_exposes_thumbnail'] = any('thumbnailUrl' in item for item in image_search)
            result['images'] = images
            if len(thumbnail_paths) == 3:
                result['http_client_originals'] = measure_client(base, original_paths, hdr['Authorization'])
                result['http_client_thumbnails'] = measure_client(base, thumbnail_paths, hdr['Authorization'])
                client = subprocess.run(
                    ['bun', 'run', 'e2e/image-cache-daemon.ts'],
                    cwd=ROOT / 'apps/gui-go/frontend',
                    input=json.dumps({'base': base, 'authorization': hdr['Authorization'],
                                      'originalPath': original_paths[0], 'thumbnailPath': thumbnail_paths[0],
                                      'expectedSha256': images[0]['original_sha256']}),
                    text=True, capture_output=True, check=True, timeout=180,
                )
                result['frontend_cache_integration'] = json.loads(client.stdout.strip().splitlines()[-1])
            try:
                request(base, '/clipboard/entries/00000000-0000-0000-0000-000000000000/thumbnail', headers=hdr)
                raise AssertionError('missing entry unexpectedly succeeded')
            except urllib.error.HTTPError as error:
                assert error.code == 404, f'missing entry returned {error.code}'
            request(base, '/encryption/lock', 'POST', b'{}', hdr)
            for _ in range(3):
                try:
                    request(base, thumbnail_paths[0], headers=hdr)
                    raise AssertionError('encrypted session lock leaked a thumbnail')
                except urllib.error.HTTPError as error:
                    assert error.code == 423, f'encryption lock returned {error.code}'
            request(base, '/content-lock/unlock', 'POST', json.dumps({'passphrase': PASSPHRASE}).encode(), hdr)
            original_after_unlock, _ = request(base, original_paths[0], headers=hdr)
            assert hashlib.sha256(original_after_unlock).hexdigest() == images[0]['original_sha256']
            unlocked_thumbnail, _ = request(base, thumbnail_paths[0], headers=hdr)
            assert unlocked_thumbnail, 'thumbnail did not load after unlock'
            result['encryption_lock_repeated_status'] = [423, 423, 423]
            result['original_unchanged_after_unlock'] = True
            result['thumbnail_after_unlock'] = True
            request(base, '/content-lock/revoke', 'POST', b'{}', hdr)
            try:
                request(base, thumbnail_paths[0], headers=hdr)
                raise AssertionError('locked thumbnail unexpectedly succeeded')
            except urllib.error.HTTPError as error:
                assert error.code == 423, f'locked thumbnail returned {error.code}'
            result['missing_entry_status'] = 404
            result['locked_thumbnail_status'] = 423
            result['decoded_rgba_bytes_if_originals'] = 3 * 2048 * 1536 * 4
            result['decoded_rgba_bytes_if_128x96_thumbnails'] = 3 * 128 * 96 * 4
            result['measurement_note'] = ('HTTP client peak RSS is measured in separate nonvisual processes; '
                                          'RGBA figures are pixel-size estimates. Neither is GUI RSS.')
            result['daemon_sha256'] = hashlib.sha256((ROOT / 'target/debug/uniclipd').read_bytes()).hexdigest()
            result['passed'] = result.get('frontend_cache_integration', {}).get('passed', False) and len(image_search) == 3 and all(
                i['thumbnail_available'] and i['thumbnail_type'] == 'image/webp' for i in images
            )
        finally:
            if (out / 'daemon.json').exists():
                stop(out)
            if home and home.name.startswith('uc-gui-go-'):
                shutil.rmtree(home, ignore_errors=True)
            args.out.parent.mkdir(parents=True, exist_ok=True)
            args.out.write_text(json.dumps(result, indent=2) + '\n')
    if not result['passed']:
        raise SystemExit('Thumbnail resource scenario failed; see result file')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
