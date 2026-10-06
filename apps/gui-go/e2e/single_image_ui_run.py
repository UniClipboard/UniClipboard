#!/usr/bin/env python3
"""Received single image in the real history UI, and the `/host-file` boundary.

Peer B (a CLI profile paired through the production rendezvous service; needs network) sends A one PNG. The GUI of
A opens the entry. Verified against the real DOM: the image decodes from a `blob:` URL (daemon resource bytes) and no
`/host-file` request was made for it, which is the shared frontend's rule for a single image.

Boundary: the only `/host-file` consumer in the shared frontend is the thumbnail grid of an image *group* (one entry,
several image files). The daemon dispatches one entry per file and refuses directories, and a group can otherwise only
come from copying several files on the system clipboard, which isolated runs disable. That UI path is therefore NOT
exercised here; the route itself is covered by `file_preview_run.py` (direct requests, refusals) and stays unverified
end to end in the UI until a visible-mode run with a real multi-file copy.
"""
import argparse
import json
import os
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'


def png(width, rgb):
    """A valid width x 1 RGB PNG, so each file has a distinct, checkable natural width."""
    def chunk(kind, data):
        body = kind + data
        return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body))
    row = b'\x00' + bytes(rgb) * width
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, 1, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(row)) + chunk(b'IEND', b''))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-peer-')
    prof_a, prof_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a, env_b = isolated_env(home_a, prof_a, {'PATH': path}), isolated_env(home_b, prof_b, {'PATH': path})
    stamp = int(time.time())
    name = f'single-{int(time.time())}.png'
    evidence = out / 'single-image-ui-native.jsonl'
    evidence.write_text('')
    gui_env = isolated_env(home_a, prof_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                           'UC_GUI_GO_E2E_PHASE': 'single-image-ui', 'UC_GUI_GO_E2E_SECRET': name, 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'profile': prof_a, 'name': name, 'passed': False}
    proc = None
    try:
        from peers import pair  # noqa: E402
        pair(env_a, env_b, 'hostfile-a', 'hostfile-b')
        source = Path(home_b) / name
        source.write_bytes(png(1, (40, 90, 200)))
        cli(env_b, 'send', '--file', str(source), timeout=120)
        proc = subprocess.Popen([str(BINARY)], env=gui_env, stdout=(out / 'single-image-ui-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline, steps = time.monotonic() + 240, {}
        while time.monotonic() < deadline and 'single-image-uses-daemon-bytes' not in steps:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                steps[row['step']] = row
            if proc.poll() is not None:
                break
            time.sleep(.3)
        single = steps.get('single-image-uses-daemon-bytes')
        assert single and single['ok'], single
        assert single['detail']['width'] == 1 and single['detail']['hostFileRequests'] == 0, single['detail']
        results['single'] = single['detail']
        assert proc.wait(timeout=60) == 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=60)
        (out / 'single-image-ui-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
