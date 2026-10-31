#!/usr/bin/env python3
"""Exercise entry thumbnail routing and encryption-session guards in a real daemon.

Run from the repository root after `cargo build -p uc-daemon`:
  python3 scripts/e2e/entry-thumbnail.py --out target/entry-thumbnail-evidence
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--daemon', type=Path, default=ROOT / 'target/debug/uniclipd')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    evidence = {'passed': False, 'checks': [], 'source': subprocess.check_output(
        ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'daemonSha256': hashlib.sha256(args.daemon.read_bytes()).hexdigest(),
        'engineRevision': tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['dependencies']['uc-engine']['rev'],
        'sourceDirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
        'sourceDiffSha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)).hexdigest(),
        'scriptSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    with tempfile.TemporaryDirectory(prefix='uc-entry-thumbnail-') as directory:
        work = Path(directory)
        binary = work / 'uniclipd'
        shutil.copy2(args.daemon, binary)
        home = work / 'home'
        home.mkdir()
        profile = 'e2e-thumbnail-' + uuid.uuid4().hex
        env = isolated_env(str(home), profile, {'UC_PORTABLE': '1', 'UC_DAEMON_RUN_MODE': 'standalone'})
        data = work / 'data' / ('app.uniclipboard.desktop-' + profile)
        process = None
        session = None

        def request(method, route, body=None, auth=None):
            req = urllib.request.Request(endpoint + route, method=method,
                data=None if body is None else json.dumps(body).encode(),
                headers={'Content-Type': 'application/json',
                         'Authorization': auth or 'Session ' + session})
            try:
                response = urllib.request.urlopen(req, timeout=90)
            except urllib.error.HTTPError as error:
                response = error
            raw = response.read()
            try:
                value = json.loads(raw)
            except ValueError:
                value = None
            return response.code, value

        def check(label, method, route, expected, body=None):
            status, value = request(method, route, body)
            evidence['checks'].append({'check': label, 'status': status, 'expected': expected})
            assert status == expected, (label, status, value)
            return value

        try:
            with (out / 'daemon.log').open('wb') as log:
                process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                assert process.poll() is None, 'daemon exited before readiness'
                try:
                    conn = json.loads((data / 'daemon.conn').read_text())
                    endpoint = 'http://127.0.0.1:' + str(conn['port'])
                    status, value = request('POST', '/auth/connect',
                        {'pid': os.getpid(), 'clientType': 'cli'}, 'Bearer ' + conn['token'])
                    assert status == 200
                    session = value['data']['sessionToken']
                    break
                except (OSError, KeyError, ValueError):
                    time.sleep(.2)
            assert session, 'authentication readiness timeout'
            health = check('daemon contract revision', 'GET', '/health', 200)
            assert health['data']['apiRevision'] == '2', health
            evidence['daemonApiRevision'] = health['data']['apiRevision']
            password = uuid.uuid4().hex
            check('initialize isolated Engine', 'POST', '/v2/setup/initialize', 200,
                {'passphrase': password, 'passphraseConfirm': password, 'deviceName': 'thumbnail-e2e'})
            route = '/clipboard/entries/' + str(uuid.uuid4()) + '/thumbnail'
            check('entry thumbnail not found', 'GET', route, 404)
            check('legacy representation route removed', 'GET', '/clipboard/thumbnails/' + str(uuid.uuid4()), 404)
            check('lock encryption session', 'POST', '/encryption/lock', 200, {})
            locked = check('locked thumbnail requires confirmed unlock', 'GET', route, 423)
            assert locked == 'encryption session locked', locked
            check('unlock encryption session', 'POST', '/encryption/unlock-with-passphrase', 200,
                {'passphrase': password})
            check('thumbnail recovers after unlock', 'GET', route, 404)
            evidence['passed'] = True
        finally:
            if process is not None and process.poll() is None:
                process.send_signal(signal.SIGTERM)
                process.wait(timeout=45)
                evidence['daemonExitCode'] = process.returncode
                evidence['passed'] = evidence['passed'] and process.returncode == 0
            (out / 'receipt.json').write_text(json.dumps(evidence, indent=2) + '\n')
    assert evidence['passed'], evidence
    print('PASS: real daemon entry thumbnail routing, lock, unlock and clean shutdown')


if __name__ == '__main__':
    main()
