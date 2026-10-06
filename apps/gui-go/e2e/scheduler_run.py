#!/usr/bin/env python3
"""Background update scheduler E2E: the app finds, announces and de-duplicates an update on its own.

The e2e build shortens the scheduler cadence (UC_UPDATE_SCHEDULER_INTERVAL). A real daemon profile has
completed setup (the scheduler waits for it), a local signed feed offers a newer release, and nothing in
the WebView asks for a check. The run asserts that
  1. the updater window opens by itself (screenshot taken while it is visible),
  2. last_notified_update.json and update_prompt_throttle.json record the announcement,
  3. after the window is closed several further iterations hit the feed without re-opening it.
"""
import argparse
import http.server
import json
import os
import platform
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps, screenshot  # noqa: E402
from update_run import wait_step  # noqa: E402

VERSION = '99.0.0-e2e'
INTERVAL = 3


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-scheduler-'))
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'scheduler-native.jsonl'
    evidence.write_text('')
    feed = work / 'feed'
    feed.mkdir()
    artifact = feed / 'update.app.tar.gz'
    artifact.write_bytes(b'not a real bundle: the scheduler never installs')
    subprocess.run(['go', 'run', './e2e/updatetool', str(artifact), str(feed)], cwd=ROOT / 'apps/gui-go', check=True)
    requests = []
    handler = lambda *a, **k: http.server.SimpleHTTPRequestHandler(*a, directory=str(feed), **k)  # noqa: E731
    handler.log_message = lambda *a: None
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
    orig = http.server.SimpleHTTPRequestHandler.do_GET

    def counted(self):
        if self.path.endswith('.json'):
            requests.append(time.monotonic())
        orig(self)
    http.server.SimpleHTTPRequestHandler.do_GET = counted
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_address[1]}'
    arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    (feed / 'good.json').write_text(json.dumps({
        'version': VERSION, 'notes': 'E2E scheduler notes', 'pub_date': '2026-10-06T00:00:00Z',
        'platforms': {f'darwin-{arch}-app': {'url': f'{base}/update.app.tar.gz', 'signature': (feed / 'good.sig.b64').read_text()}},
    }))
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {
        'PATH': path, 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'scheduler',
        'UC_GUI_GO_EXIT_MODE': 'full', 'UC_UPDATE_PUBKEY': (feed / 'pubkey.b64').read_text(),
        'UC_UPDATE_ENDPOINT': f'{base}/good.json', 'UC_UPDATE_SCHEDULER_INTERVAL': f'{INTERVAL}s'})
    data_root = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile)
    results = {'home': home, 'profile': profile, 'version': VERSION, 'passed': False}
    proc = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'scheduler-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'scheduler-gui.log').open('w'), stderr=subprocess.STDOUT)
        row, _ = wait_step(evidence, 0, 'scheduler-updater-opened', 90)
        assert row['ok'], 'updater window did not open on its own'
        results['openedWithoutRequest'] = True
        time.sleep(1.5)  # let the page render before the capture
        results['screenshot'] = screenshot(proc.pid, out / 'scheduler-updater.png', 'Software Update')
        notified = json.loads((data_root / 'last_notified_update.json').read_text())
        assert VERSION in notified.values(), notified
        throttle = json.loads((data_root / 'update_prompt_throttle.json').read_text())
        assert isinstance(throttle.get('last_prompt_at'), int), throttle
        results.update({'lastNotified': notified, 'promptThrottle': throttle})
        before = len(requests)
        row, _ = wait_step(evidence, 0, 'scheduler-no-reprompt', 90)
        assert row['ok'], 'the same version was announced twice'
        iterations = len(requests) - before
        assert iterations >= 3, f'scheduler did not keep checking ({iterations} further feed hits)'
        results.update({'noReprompt': True, 'furtherFeedChecks': iterations})
        assert proc.wait(timeout=60) == 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        server.shutdown()
        stop = cli(cli_env, '--json', 'stop', check=False, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        (out / 'scheduler-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
