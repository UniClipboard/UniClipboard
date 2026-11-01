#!/usr/bin/env python3
"""Real LAN upload -> Engine -> native WebView progress, with synthetic bytes only.

Failure modes: upload fails, SyncClipboard apply fails, file bytes differ, or
provisional upload progress never reaches a visible control before apply.
Network writes are paced; displayed progress must still come from real bytes.
This is a protocol-client test, not Android-device acceptance.
"""
import argparse
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

from run import ROOT, isolated_env, read_steps
from file_preview_run import cli, PASSPHRASE
from file_progress_run import capture_gui


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = Path(tempfile.mkdtemp(prefix='uc-gui-go-lan-progress-'))
    evidence = out / 'receiver.jsonl'
    evidence.write_text('')
    env = isolated_env(str(home), 'gui-go-' + home.name, {
        'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'],
        'UC_GUI_GO_ISOLATED': '1', 'UC_GPUI_QUICK_PANEL': '0',
        'UC_GUI_GO_E2E_PHASE': 'file-progress', 'UC_GUI_GO_EXIT_MODE': 'keep',
        'UC_GUI_GO_E2E_VISIBLE': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
    })
    result = {'passed': False, 'scope': 'LAN protocol upload; native Desktop receiver; no Android device',
              'clipboardDisabled': True, 'home': str(home)}
    proc = None
    worker = None
    errors = []
    payload = os.urandom(16 * 1024 * 1024)
    digest = hashlib.sha256(payload).hexdigest()
    try:
        cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'lan-receiver')
        cli(env, 'start')
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        setup = json.loads(cli(env, '--json', 'mobile', 'setup', '--non-interactive', '--label', 'synthetic-lan',
                               '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)).stdout)
        auth = 'Basic ' + base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode()
        proc = subprocess.Popen([str(ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go')],
                                env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            if any(row['step'] == 'progress-observer-ready' for row in read_steps(evidence, 0)):
                break
            time.sleep(.1)
        else:
            raise RuntimeError('native observer did not become ready')

        def upload():
            try:
                connection = http.client.HTTPConnection('127.0.0.1', port, timeout=60)
                connection.putrequest('PUT', '/file/synthetic-lan.bin')
                for key, value in {'Authorization': auth, 'Content-Length': str(len(payload)),
                                   'Content-Type': 'application/octet-stream'}.items():
                    connection.putheader(key, value)
                connection.endheaders()
                for start in range(0, len(payload), 64 * 1024):
                    connection.send(payload[start:start + 64 * 1024])
                    time.sleep(.02)
                response = connection.getresponse()
                result['uploadStatus'] = response.status
                response.read()
                result['uploadFinishedAt'] = time.time_ns() // 1_000_000
                connection.close()
            except Exception as error:
                errors.append(type(error).__name__)

        worker = threading.Thread(target=upload)
        worker.start()
        shot = False
        while worker.is_alive():
            if not shot and any(row['step'] == 'transfer-event' for row in read_steps(evidence, 0)):
                result['screenshot'] = capture_gui(proc.pid, out / 'receiving.png')
                shot = True
            time.sleep(.05)
        worker.join()
        assert not errors and result.get('uploadStatus') == 200, 'LAN upload failed'
        before = read_steps(evidence, 0)
        result['progressValuesBeforeApply'] = sorted({float(bar['value']) for row in before
            if row['step'] == 'progress-dom' for bar in row['detail']['bars'] if bar['value'] is not None})
        result['progressEventsBeforeApply'] = sum(row['step'] == 'transfer-event' and
            row['detail']['type'] == 'file-transfer.progress' for row in before)
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=60)
        body = json.dumps({'type': 'File', 'text': 'synthetic-lan.bin', 'dataName': 'synthetic-lan.bin', 'hasData': True})
        connection.request('PUT', '/SyncClipboard.json', body=body,
                           headers={'Authorization': auth, 'Content-Type': 'application/json'})
        response = connection.getresponse()
        result['applyStatus'] = response.status
        response.read()
        connection.close()
        result['sha256'] = digest
        result['hashMatches'] = any(hashlib.sha256(path.read_bytes()).hexdigest() == digest for path in home.rglob('*')
                                    if path.is_file() and path.stat().st_size == len(payload))
        if not shot:
            result['screenshot'] = capture_gui(proc.pid, out / 'receiving.png')
        values = result['progressValuesBeforeApply']
        result['passed'] = result['applyStatus'] == 200 and result['hashMatches'] and len(values) > 1 and any(0 < value < 100 for value in values)
    finally:
        if worker:
            worker.join(timeout=65)
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=20)
        cli(env, '--json', 'stop', check=False)
        (out / 'assertions.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    assert result['passed'], 'LAN transfer or visible pre-apply progress failed'


if __name__ == '__main__':
    main()
