#!/usr/bin/env python3
"""Signed in-place update E2E: real feed, real minisign verification, real bundle swap.

Two launches of a copy of the e2e app bundle (the copy is the install target):
  1. update-bad:  the feed signs the artifact with an untrusted key; the download must be rejected
                  and the bundle must stay untouched.
  2. update-good: the feed is trusted; the app downloads, verifies, stops its daemon, replaces its own
                  bundle, relaunches, and the relaunched process finds the update marker.
"""
import argparse
import http.server
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import run as common  # noqa: E402
from run import ROOT, isolated_env, pid_alive, read_steps  # noqa: E402

VERSION = '99.0.0-e2e'


def serve(directory):
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(directory), **k)  # noqa: E731
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
    handler.log_message = lambda *a: None
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def wait_step(evidence, start, step, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        rows = read_steps(evidence, start)
        for row in rows:
            if row['step'] in ('update-driver-error', 'driver-error'):
                raise RuntimeError(f"driver error: {row.get('detail')}")
        for row in rows:
            if row['step'] == step:
                return row, rows
        time.sleep(.2)
    raise RuntimeError(f'timeout waiting for {step}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-update-'))
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'update-native.jsonl'
    evidence.write_text('')
    built = ROOT / 'target/gui-go/UniClipboardGoE2E.app'
    cli = ROOT / 'target/gui-go/uniclip'
    target = work / 'install' / built.name
    (work / 'install').mkdir()
    shutil.copytree(built, target, symlinks=True)
    feed = work / 'feed'
    feed.mkdir()
    # The update payload is the same bundle plus a marker the relaunched process looks for.
    new_bundle = work / 'stage' / built.name
    (work / 'stage').mkdir()
    shutil.copytree(built, new_bundle, symlinks=True)
    (new_bundle / 'Contents/Resources').mkdir(parents=True, exist_ok=True)
    (new_bundle / 'Contents/Resources/update-marker.txt').write_text('installed\n')
    artifact = feed / 'update.app.tar.gz'
    subprocess.run(['tar', '-czf', str(artifact), '-C', str(work / 'stage'), built.name], check=True)
    subprocess.run(['go', 'run', './e2e/updatetool', str(artifact), str(feed)], cwd=ROOT / 'apps/gui-go', check=True)
    pubkey = (feed / 'pubkey.b64').read_text()
    server = serve(feed)
    base = f'http://127.0.0.1:{server.server_address[1]}'
    arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    for name, sig in (('good', 'good.sig.b64'), ('bad', 'bad.sig.b64')):
        (feed / f'{name}.json').write_text(json.dumps({
            'version': VERSION, 'notes': 'E2E update notes', 'pub_date': '2026-10-06T00:00:00Z',
            'platforms': {f'darwin-{arch}-app': {'url': f'{base}/update.app.tar.gz', 'signature': (feed / sig).read_text()}},
        }))
    results = {'home': home, 'profile': profile, 'passed': False, 'version': VERSION}
    env_base = isolated_env(home, profile, {'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_UPDATE_PUBKEY': pubkey,
                                            'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'], 'UC_GUI_GO_EXIT_MODE': 'full'})
    binary = target / 'Contents/MacOS/gui-go'
    marker = target / 'Contents/Resources/update-marker.txt'
    procs = []
    try:
        # Round 1: untrusted signature.
        env = dict(env_base, UC_GUI_GO_E2E_PHASE='update-bad', UC_UPDATE_ENDPOINT=f'{base}/bad.json')
        start = len(evidence.read_text().splitlines())
        proc = subprocess.Popen([str(binary)], env=env, stdout=(out / 'update-gui-0.log').open('w'), stderr=subprocess.STDOUT)
        procs.append(proc)
        wait_step(evidence, start, 'update-check')
        row, _ = wait_step(evidence, start, 'update-download-rejected')
        assert row['ok'] and 'signature' in row['detail']['error'], row
        assert not marker.exists(), 'untrusted update modified the bundle'
        results['untrustedSignatureRejected'] = True
        assert proc.wait(timeout=60) == 0
        # Round 2: trusted signature, real install and relaunch.
        env = dict(env_base, UC_GUI_GO_E2E_PHASE='update-good', UC_UPDATE_ENDPOINT=f'{base}/good.json')
        start = len(evidence.read_text().splitlines())
        proc = subprocess.Popen([str(binary)], env=env, stdout=(out / 'update-gui-1.log').open('w'), stderr=subprocess.STDOUT)
        procs.append(proc)
        row, _ = wait_step(evidence, start, 'update-state')
        first_pid = row['detail']['pid']
        assert not row['detail']['installed']
        conn_path = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
        old_daemon = json.loads(conn_path.read_text())['pid']
        wait_step(evidence, start, 'update-download-verified')
        wait_step(evidence, start, 'update-install-clicked')
        row, rows = wait_step(evidence, start, 'update-relaunched', 180)
        assert marker.exists(), 'bundle was not replaced'
        states = [r for r in read_steps(evidence, start) if r['step'] == 'update-state']
        assert len(states) == 2 and states[1]['detail']['installed'] and states[1]['detail']['pid'] != first_pid
        results.update({'bundleReplaced': True, 'relaunchedPID': states[1]['detail']['pid'], 'previousPID': first_pid})
        assert proc.wait(timeout=60) == 0, 'old process did not exit'
        deadline = time.monotonic() + 30
        while pid_alive(states[1]['detail']['pid']) and time.monotonic() < deadline:
            time.sleep(.3)
        assert not pid_alive(states[1]['detail']['pid']), 'relaunched process did not exit'
        assert not pid_alive(old_daemon), 'old daemon survived the update'
        results['oldDaemonStopped'] = True
        results['passed'] = True
    finally:
        for p in procs:
            if p.poll() is None:
                p.terminate()
        server.shutdown()
        stop = subprocess.run([str(cli), '--json', 'stop'], env=env_base, capture_output=True, text=True, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        (out / 'update-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        shutil.rmtree(work, ignore_errors=True)
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
