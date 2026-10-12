#!/usr/bin/env python3
"""Seed mixed types and a mobile source into the isolated History browser daemon."""
import argparse
import base64
import json
import socket
import time
import urllib.request
from pathlib import Path

from file_preview_run import PNG, cli
from history_kit_browser_daemon import env_for, send, start, stop


def seed(out):
    info = json.loads((out / 'daemon.json').read_text())
    home = Path(info['home'])
    assert info['profile'].startswith('gui-go-') and home.name.startswith('uc-gui-go-')
    env = env_for(str(home), info['profile'])
    file = home / 'pix-notes.md'
    file.write_text('Synthetic file content for History filter acceptance.\n')
    send(env, '--file', str(file))
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    setup = json.loads(cli(env, '--json', 'mobile', 'setup', '--non-interactive', '--label', 'token-phone',
                           '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)).stdout)
    auth = 'Basic ' + base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode()
    for _ in range(60):
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=1):
                break
        except OSError:
            time.sleep(.2)

    def put(route, body, content_type):
        request = urllib.request.Request(f'http://127.0.0.1:{port}{route}', data=body, method='PUT',
                                         headers={'Authorization': auth, 'Content-Type': content_type})
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status < 300

    put('/SyncClipboard.json', json.dumps({'type': 'Text', 'text': 'pix phone synthetic', 'hasData': False}).encode(), 'application/json')
    put('/file/pixel-synthetic.png', PNG, 'image/png')
    put('/SyncClipboard.json', json.dumps({'type': 'Image', 'text': 'pixel-synthetic.png', 'dataName': 'pixel-synthetic.png', 'hasData': True}).encode(), 'application/json')
    print('Seeded synthetic file, mobile text and image; system clipboard disabled.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['start', 'seed', 'stop'])
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if args.action == 'start':
        start(out, 0, True)
        seed(out)
    elif args.action == 'seed':
        seed(out)
    else:
        stop(out)
