#!/usr/bin/env python3
"""Dual-peer E2E: an image file received from another device previews in the Go GUI.

Peer A owns the GUI profile; peer B is a CLI profile in a separate HOME. B joins A's space and sends a
PNG; A's history then holds a received file entry whose cache path the shared frontend turns into a
`/host-file` URL. The in-WebView driver opens the entry and asserts the image decoded, and that paths
outside the history are refused. Pairing uses the production rendezvous service, so this needs network.
"""
import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import ROOT, isolated_env, read_steps  # noqa: E402

PASSPHRASE = 'hunter22hunter22'
PNG = bytes.fromhex('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082')


def cli(env, *args, timeout=120, check=True):
    proc = subprocess.run([str(ROOT / 'target/gui-go/uniclip'), *args], env=env, capture_output=True, text=True, timeout=timeout)
    if check and proc.returncode != 0:
        raise RuntimeError(f'uniclip {" ".join(args)} failed: {proc.returncode} {proc.stderr}')
    return proc


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-peer-')
    prof_a, prof_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    evidence = out / 'file-preview-native.jsonl'
    evidence.write_text('')
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a = isolated_env(home_a, prof_a, {'PATH': path})
    env_b = isolated_env(home_b, prof_b, {'PATH': path})
    gui_env = isolated_env(home_a, prof_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                           'UC_GUI_GO_E2E_PHASE': 'file-preview', 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'profileA': prof_a, 'profileB': prof_b, 'passed': False}
    invite = proc = None
    try:
        cli(env_a, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'preview-a')
        for attempt in range(3):
            if cli(env_a, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        invite = subprocess.Popen([str(ROOT / 'target/gui-go/uniclip'), 'space', 'invite'], env=env_a, stdout=subprocess.PIPE)
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
        cli(env_b, 'space', 'join', '--code', code, '--passphrase', PASSPHRASE, '--device-name', 'preview-b', timeout=120)
        deadline = time.time() + 90
        while time.time() < deadline:
            members = json.loads(cli(env_a, '--json', 'member', 'list', check=False).stdout or '[]')
            if len(members) >= 2:
                break
            time.sleep(2)
        assert len(members) >= 2, 'peer did not join'
        invite.send_signal(signal.SIGINT)
        png = Path(home_b) / 'pixel.png'
        png.write_bytes(PNG)
        cli(env_b, 'send', '--file', str(png), timeout=120)
        results['peerSent'] = True
        # The GUI starts after the entry exists; it reuses A's persistent daemon.
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=(out / 'file-preview-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 180
        steps = {}
        while time.monotonic() < deadline and 'file-preview-image-loaded' not in steps:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                steps[row['step']] = row
            if proc.poll() is not None:
                break
            time.sleep(.3)
        assert steps.get('file-preview-refusals', {}).get('ok'), steps.get('file-preview-refusals')
        assert steps.get('file-preview-image-loaded', {}).get('ok'), 'image did not load'
        results.update({'refusals': steps['file-preview-refusals']['detail'], 'image': steps['file-preview-image-loaded']['detail']})
        assert proc.wait(timeout=60) == 0
        results['passed'] = True
    finally:
        for p in (proc, invite):
            if p and p.poll() is None:
                p.terminate()
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=60)
        (out / 'file-preview-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
