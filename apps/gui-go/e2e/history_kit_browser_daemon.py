#!/usr/bin/env python3
"""Isolated real daemon with seeded history for the browser run of the kit History page.

`start` creates a throwaway HOME and a unique gui-go-* profile (system clipboard disabled), initialises a space,
starts `uniclipd` (from target/debug, found on PATH by `uniclip`), sends text, code, link and file entries through
`uniclip send`, opens a GUI client session (`POST /auth/connect`, clientType gui) and writes the daemon address and
daemon.conn path to <out>/daemon.json for frontend/e2e/history-kit-browser.mjs. `stop` stops that daemon.

Needs target/debug/uniclipd (`cargo build -p uc-daemon --bin uniclipd`) and target/gui-go/uniclip
(`cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip`). Nothing touches the real profile.
"""
import argparse
import json
import os
import tempfile
import time
import urllib.request
from pathlib import Path

from file_preview_run import PASSPHRASE, PNG, cli
from peers import start_daemon
from run import ROOT, isolated_env

TEXTS = [
    'Q4 roadmap: ship LAN-only mode, then per-device delivery receipts',
    'https://github.com/UniClipboard/UniClipboard',
    'git rebase -i HEAD~3',
    'docker compose -f compose.prod.yml \\\n  --env-file .env.prod \\\n  up -d --build --remove-orphans',
    'Meeting notes\n- decide the Windows title bar layout\n- review keyboard focus order',
]


def env_for(home, profile):
    return isolated_env(home, profile, {'PATH': str(ROOT / 'target/debug') + os.pathsep + os.environ['PATH']})


def conn_file(home):
    return next(Path(home).rglob('daemon.conn'))


def session(home):
    conn_path = conn_file(home)
    conn = json.loads(conn_path.read_text())
    base = f"http://{conn.get('host', '127.0.0.1')}:{conn['port']}"
    request = urllib.request.Request(f'{base}/auth/connect', method='POST', data=json.dumps({'pid': os.getpid(), 'clientType': 'gui'}).encode(),
                                     headers={'Authorization': f"Bearer {conn['token']}", 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=15) as response:
        return base, json.load(response)['data']['sessionToken']


def send(env, *args):
    # With no paired peer `send` exits 1 ("0 accepted"), but the entry is captured locally all the same.
    result = cli(env, '--json', 'send', *args, check=False)
    if 'snapshotHash' not in result.stdout:
        raise RuntimeError(f'uniclip send {args} captured nothing: {result.stdout} {result.stderr}')


def start(out, extra):
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    env = env_for(home, profile)
    (out / 'daemon.json').write_text(json.dumps({'home': home, 'profile': profile}) + '\n')  # lets `stop` clean up a failed start
    cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'kit-browser')
    start_daemon(env)
    files = Path(home) / 'seed'
    files.mkdir()
    (files / 'pixel.png').write_bytes(PNG)
    (files / 'invoice-0921.txt').write_text('Invoice 0921\nTotal: 42.00\n')
    for index in range(extra):
        send(env, '--text', f'Paging entry {index:03d}')
    for path in sorted(files.iterdir()):
        send(env, '--file', str(path))
    for text in TEXTS:
        send(env, '--text', text)
        time.sleep(.2)  # distinct capture times keep the list order deterministic
    base, _ = session(home)  # proves a GUI client session opens; the browser run opens its own
    info = {'home': home, 'profile': profile, 'daemonUrl': base, 'conn': str(conn_file(home)), 'passphrase': PASSPHRASE, 'seeded': len(TEXTS) + extra + 2}
    (out / 'daemon.json').write_text(json.dumps(info, indent=2) + '\n')
    print(json.dumps(info, indent=2))


def stop(out):
    info = json.loads((out / 'daemon.json').read_text())
    print(cli(env_for(info['home'], info['profile']), '--json', 'stop', check=False, timeout=60).stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['start', 'stop'])
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--extra', type=int, default=60, help='plain entries sent first, so the list pages')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    start(out, args.extra) if args.action == 'start' else stop(out)


if __name__ == '__main__':
    main()
