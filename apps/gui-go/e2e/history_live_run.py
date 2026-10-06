#!/usr/bin/env python3
"""Live history E2E: an entry that arrives over the transport shows up on the open history page without any reload.

A second profile is paired with the GUI's profile through the production rendezvous service (needs network). The
in-WebView driver reaches the main layout, records `history-watch-armed`, and then only watches: it must see a
clipboard or search WebSocket frame and a new history card, in the same document and on the same route. The
orchestrator makes the peer send a text once that step shows up, so the entry travels peer -> transport -> daemon ->
WebSocket -> shared frontend. The frames seen after arming are recorded as evidence.

Not used as evidence: `uniclip send` from the GUI's own profile also adds a card, but no WebSocket frame was observed
for it in some runs, so it cannot show that the update was pushed.
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    source = 'peer'
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-peer-')
    prof_a, prof_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    marker = f'live-history-{source}-{int(time.time())}'
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a, env_b = isolated_env(home_a, prof_a, {'PATH': path}), isolated_env(home_b, prof_b, {'PATH': path})
    evidence = out / f'history-live-{source}-native.jsonl'
    evidence.write_text('')
    gui_env = isolated_env(home_a, prof_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
                                           'UC_GUI_GO_E2E_PHASE': 'history-live', 'UC_GUI_GO_E2E_SECRET': marker, 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'source': source, 'profile': prof_a, 'marker': marker, 'passed': False}
    proc = None
    try:
        from peers import pair  # noqa: E402
        pair(env_a, env_b, 'live-a', 'live-b')
        cli(env_a, 'send', 'baseline-entry', check=False)  # the history page needs one card to be "rendered"
        proc = subprocess.Popen([str(BINARY)], env=gui_env, stdout=(out / f'history-live-{source}-gui.log').open('w'), stderr=subprocess.STDOUT)
        deadline, armed, injected, steps = time.monotonic() + 240, False, False, {}
        while time.monotonic() < deadline:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                steps[row['step']] = row
            if 'history-watch-armed' in steps and not injected:
                assert steps['history-watch-armed']['ok'], steps['history-watch-armed']
                cli(env_b, 'send', marker)
                injected = True
            if 'history-live-update' in steps or proc.poll() is not None:
                break
            time.sleep(.2)
        assert injected, 'the driver never armed the watch'
        assert steps.get('history-live-update', {}).get('ok'), steps.get('history-live-update')
        results['update'] = steps['history-live-update']['detail']
        assert proc.wait(timeout=60) == 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=60)
        (out / f'history-live-{source}-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
