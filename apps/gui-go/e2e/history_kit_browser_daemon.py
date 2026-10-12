#!/usr/bin/env python3
"""Isolated real daemon with seeded history for the browser run of the kit History page.

`start` creates a throwaway HOME and a unique gui-go-* profile (system clipboard disabled), initialises a space,
starts `uniclipd` (from target/debug, found on PATH by `uniclip`), seeds text entries over one GUI session through
frontend/e2e/history-kit-seed.mjs, two files through `uniclip send` and one image through the loopback mobile LAN
listener, and writes the daemon address and a copy of its
file token to <out> (daemon.json, daemon.conn) for frontend/e2e/history-kit-browser.mjs. `stop` stops that daemon.

Needs target/debug/uniclipd (`cargo build -p uc-daemon --bin uniclipd`) and target/gui-go/uniclip
(`cd apps/cli-go && go build -o ../../target/gui-go/uniclip ./cmd/uniclip`). Nothing touches the real profile.
"""
import argparse
import json
import os
import socket
import subprocess
import tempfile
import time
from pathlib import Path

from file_preview_run import PASSPHRASE, PNG, cli
from peers import start_daemon
from run import ROOT, isolated_env



def env_for(home, profile):
    return isolated_env(home, profile, {'PATH': str(ROOT / 'target/debug') + os.pathsep + os.environ['PATH']})


def conn_file(home):
    return next(Path(home).rglob('daemon.conn'))


def seed(out, home, *seed_args):
    """frontend/e2e/history-kit-seed.mjs, the one source of the seeded entries (shared with the Windows run)."""
    conn = json.loads(conn_file(home).read_text())
    base = f"http://{conn.get('host', '127.0.0.1')}:{conn['port']}"
    subprocess.run(['node', str(ROOT / 'apps/gui-go/frontend/e2e/history-kit-seed.mjs'), str(out), base, str(conn_file(home)), PASSPHRASE,
                    *seed_args], check=True)


def mobile_image(out, home, env):
    """An image entry without a paired peer: the mobile LAN listener on loopback takes a SyncClipboard image upload."""
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port = s.getsockname()[1]
    setup = json.loads(cli(env, '--json', 'mobile', 'setup', '--non-interactive', '--label', 'kit-image', '--ip', '127.0.0.1',
                           '--accept-network-risk', '--port', str(port)).stdout)
    url = f'http://127.0.0.1:{port}'
    for _ in range(60):
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=1):
                break
        except OSError:
            time.sleep(.5)
    seed(out, home, 'image', url, setup['username'], setup['password'])


def send(env, *args):
    # With no paired peer `send` exits 1 ("0 accepted"), but the entry is captured locally all the same. A send can
    # also fail before capture: every CLI call opens a session and the daemon rate-limits `/auth/connect` (HTTP 429), so
    # a failed send backs off and retries.
    for delay in (2, 5, 10, 20, 30):
        result = cli(env, '--json', 'send', *args, check=False)
        if 'snapshotHash' in result.stdout:
            return
        time.sleep(delay)
    raise RuntimeError(f'uniclip send {args} captured nothing: {result.stdout} {result.stderr}')


def start(out, extra, empty):
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    env = env_for(home, profile)
    (out / 'daemon.json').write_text(json.dumps({'home': home, 'profile': profile}) + '\n')  # lets `stop` clean up a failed start
    cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'kit-browser')
    start_daemon(env)
    if not empty:
        seed(out, home, 'paging', str(extra))
        files = Path(home) / 'seed'
        files.mkdir()
        (files / 'pixel.png').write_bytes(PNG)
        (files / 'invoice-0921.txt').write_text('Invoice 0921\nTotal: 42.00\n')
        for path in sorted(files.iterdir()):
            send(env, '--file', str(path))
        mobile_image(out, home, env)
    # Writes daemon.json (with home and profile, which `stop` uses); an empty run is a space without history.
    seed(out, home, 'texts', profile, home, '0' if empty else str(extra + 3 + 5))
    print((out / 'daemon.json').read_text())


def stop(out):
    info = json.loads((out / 'daemon.json').read_text())
    print(cli(env_for(info['home'], info['profile']), '--json', 'stop', check=False, timeout=60).stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['start', 'stop'])
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--extra', type=int, default=130, help='plain entries sent first, so the list pages (page size 100)')
    parser.add_argument('--empty', action='store_true', help='no history at all (first-launch state)')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    start(out, args.extra, args.empty) if args.action == 'start' else stop(out)


if __name__ == '__main__':
    main()
