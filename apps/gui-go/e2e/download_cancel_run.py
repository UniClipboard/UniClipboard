#!/usr/bin/env python3
"""Update download cancellation E2E: a throttled signed feed, a real cancel, then a completed second download.

The feed serves the artifact slowly on the first request only. The in-WebView driver (phase `download-cancel`)
starts the download through the generated binding, waits for real progress events, calls `cancel_download`, and then
checks the rejection, the `Failed` event, the pending update being available again, and a second download finishing.
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
from receipt import write_receipt  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

VERSION = '99.0.0-e2e'
STEPS = ['cancel-check-found-update', 'cancel-progress-started', 'cancel-phase-downloading', 'cancel-call-rejected',
         'cancel-phase-available-again', 'cancel-second-download-completes', 'cancel-phase-ready', 'cancel-event-order']


def serve(directory, stats):
    class Handler(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def copyfile(self, source, outputfile):
            if not self.path.endswith('update.app.tar.gz'):
                super().copyfile(source, outputfile)
                return
            stats['artifactRequests'] += 1
            slow = stats['artifactRequests'] == 1
            try:
                while chunk := source.read(64 * 1024):
                    outputfile.write(chunk)
                    outputfile.flush()
                    stats['bytesSent'] += len(chunk)
                    if slow:
                        time.sleep(.08)
            except (BrokenPipeError, ConnectionResetError):
                stats['aborted'] += 1

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), lambda *a, **k: Handler(*a, directory=str(directory), **k))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-cancel-'))
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'download-cancel-native.jsonl'
    evidence.write_text('')
    built = ROOT / 'target/gui-go/UniClipboardGoE2E.app'
    cli = ROOT / 'target/gui-go/uniclip'
    feed = work / 'feed'
    feed.mkdir()
    stage = work / 'stage'
    stage.mkdir()
    shutil.copytree(built, stage / built.name, symlinks=True)
    artifact = feed / 'update.app.tar.gz'
    subprocess.run(['tar', '-czf', str(artifact), '-C', str(stage), built.name], check=True)
    subprocess.run(['go', 'run', './e2e/updatetool', str(artifact), str(feed)], cwd=ROOT / 'apps/gui-go', check=True)
    pubkey = (feed / 'pubkey.b64').read_text()
    stats = {'artifactRequests': 0, 'bytesSent': 0, 'aborted': 0, 'artifactBytes': artifact.stat().st_size}
    server = serve(feed, stats)
    base = f'http://127.0.0.1:{server.server_address[1]}'
    arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    (feed / 'good.json').write_text(json.dumps({
        'version': VERSION, 'notes': 'E2E update notes', 'pub_date': '2026-10-06T00:00:00Z',
        'platforms': {f'darwin-{arch}-app': {'url': f'{base}/update.app.tar.gz', 'signature': (feed / 'good.sig.b64').read_text()}}}))
    env = isolated_env(home, profile, {'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_UPDATE_PUBKEY': pubkey,
                                       'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'], 'UC_GUI_GO_EXIT_MODE': 'full',
                                       'UC_GUI_GO_E2E_PHASE': 'download-cancel', 'UC_UPDATE_ENDPOINT': f'{base}/good.json'})
    results = {'home': home, 'profile': profile, 'passed': False, 'version': VERSION}
    proc = None
    try:
        proc = subprocess.Popen([str(built / 'Contents/MacOS/gui-go')], env=env, stdout=(out / 'download-cancel-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline, seen = time.monotonic() + 240, {}
        while time.monotonic() < deadline and not all(s in seen for s in STEPS):
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                seen[row['step']] = row
            if proc.poll() is not None:
                seen.update({r['step']: r for r in read_steps(evidence, 0)})
                break
            time.sleep(.2)
        assert proc.wait(timeout=90) == 0, 'GUI did not exit cleanly'
        for step in STEPS:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        assert stats['artifactRequests'] == 2, stats
        assert stats['aborted'] >= 1, f'the cancelled download was not cut off at the server: {stats}'
        assert stats['bytesSent'] < 2 * stats['artifactBytes'], f'the cancelled download kept running: {stats}'
        results.update({'server': stats, 'steps': {s: seen[s].get('detail') for s in STEPS}, 'passed': True})
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        server.shutdown()
        stop = subprocess.run([str(cli), '--json', 'stop'], env=env, capture_output=True, text=True, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        (out / 'download-cancel-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
        shutil.rmtree(work, ignore_errors=True)
        if results['passed']:
            write_receipt(out, 'download-cancel',
                          scope={'commands': ['check_for_update', 'download_update', 'cancel_download', 'get_download_progress'],
                                 'events': ['update-download-progress'], 'errors': ['text (cancelled download)'],
                                 'layers': ['generated binding', 'Go service', 'throttled signed feed']},
                          binaries={'gui': built / 'Contents/MacOS/gui-go'}, assertions=results)
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
