"""Pair two CLI profiles through the production rendezvous service (needs network).

Peer A owns the GUI profile; peer B is a CLI profile in a separate HOME that joins A's space.
"""
import json
import os
import signal
import subprocess
import time

from file_preview_run import PASSPHRASE, cli
from run import ROOT


def start_daemon(env):
    for _ in range(3):
        if cli(env, 'start', check=False).returncode == 0:
            return
        time.sleep(3)
    raise RuntimeError('daemon start failed three times')


def pair(env_a, env_b, name_a, name_b):
    """Initialise A, start its daemon, and join B to A's space. Returns once both see two members."""
    cli(env_a, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', name_a)
    start_daemon(env_a)
    invite = subprocess.Popen([str(ROOT / 'target/gui-go/uniclip'), 'space', 'invite'], env=env_a, stdout=subprocess.PIPE)
    try:
        os.set_blocking(invite.stdout.fileno(), False)
        code, buf, deadline = None, '', time.time() + 90
        while code is None and time.time() < deadline:
            try:
                buf += os.read(invite.stdout.fileno(), 4096).decode(errors='replace')
            except BlockingIOError:
                pass
            for line in buf.splitlines():
                if line.startswith('INVITATION_CODE='):
                    code = line.split('=', 1)[1].strip()
            time.sleep(.3)
        assert code, 'no invitation code (rendezvous service reachable?): ' + buf
        cli(env_b, 'space', 'join', '--code', code, '--passphrase', PASSPHRASE, '--device-name', name_b, timeout=120)
        deadline = time.time() + 90
        members = []
        while time.time() < deadline:
            members = json.loads(cli(env_a, '--json', 'member', 'list', check=False).stdout or '[]')
            if len(members) >= 2:
                break
            time.sleep(2)
        assert len(members) >= 2, 'peer did not join'
    finally:
        invite.send_signal(signal.SIGINT)
