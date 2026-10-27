#!/usr/bin/env python3
"""macOS startup acceptance: runtime modes plus cold-start recovery/restore.

The shared mode driver checks temporary hosts and automatic versus explicit
activation. This wrapper also preserves the cold recovery/restore-sequence check.
No clipboard entry is present; this does not verify restoring real content.
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'


class Profile:
    def __init__(self, out):
        self.out = out
        self.home = tempfile.mkdtemp(prefix='uc-gui-go-')
        self.profile = 'gui-go-' + os.path.basename(self.home)
        path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
        self.cli_env = isolated_env(self.home, self.profile, {'PATH': path})
        self.base = {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1'}
        self.notify_log = Path(tempfile.mkdtemp(prefix='uc-gui-go-notify-')) / 'n.log'

    def launch(self, label, phase, exit_mode='full', observe=None, wait=True, timeout=150, control=None):
        evidence = self.out / f'startup-{label}.jsonl'
        evidence.write_text('')
        env = isolated_env(self.home, self.profile, dict(self.base, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_PHASE=phase,
                                                          UC_GUI_GO_EXIT_MODE=exit_mode, UC_GUI_GO_E2E_NOTIFY_LOG=str(self.notify_log),
                                                          **({'UC_GUI_GO_E2E_OBSERVE': str(observe)} if observe else {}),
                                                          **({'UC_GUI_GO_E2E_CONTROL_FILE': str(control)} if control else {})))
        log = self.out / f'startup-{label}-gui.log'
        proc = subprocess.Popen([str(BINARY)], env=env, stdout=log.open('w'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + timeout
        while wait and time.monotonic() < deadline and proc.poll() is None:
            for row in read_steps(evidence, 0):
                if row['step'] == 'driver-error':
                    proc.terminate()
                    raise RuntimeError(f"driver error: {row.get('detail')}")
            time.sleep(.3)
        return proc, evidence, log

    def notes(self):
        return self.notify_log.read_text().splitlines() if self.notify_log.exists() else []

    def daemon_alive(self):
        # Observe health without starting or replacing the daemon under test.
        path = Path(self.home) / 'Library/Application Support' / f'app.uniclipboard.desktop-{self.profile}' / 'daemon.conn'
        try:
            conn = json.loads(path.read_text())
            request = urllib.request.Request(f"http://{conn['host']}:{conn['port']}/health")
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open(request, timeout=2) as response:
                return response.status == 200
        except (OSError, ValueError, KeyError):
            return False


def set_mode(p, mode, restore=False):
    proc, evidence, _ = p.launch(f'set-{mode}', f'startup-set:{mode}' + (':restore' if restore else ''))
    assert proc.wait(timeout=60) == 0
    row = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-saved'][0]
    assert row['ok'], row


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--allow-session', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not args.allow_session or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        parser.error('requires --allow-session and UC_GUI_GO_E2E_DEDICATED_HOST=1')
    subprocess.run([sys.executable, str(HERE / 'desktop_modes_run.py'), '--allow-session',
                    '--bundle', str(BINARY.parents[2]), '--cli', str(ROOT / 'target/gui-go/uniclip'),
                    '--out', str(args.out / 'modes')], check=True)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    p = Profile(out)
    results = {'home': p.home, 'profile': p.profile, 'passed': False}
    try:
        cli(p.cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'startup-a')
        for _ in range(3):
            if p.daemon_alive():
                break
            time.sleep(3)

        # Normal cold start with restore: the setup launch quits fully and so stops the daemon, which makes
        # the next launch spawn it.
        set_mode(p, 'normal', restore=True)
        proc, evidence, log = p.launch('normal-restore', 'startup-observe', observe=15, exit_mode='full')
        assert proc.wait(timeout=90) == 0
        # A cold normal launch hands off once to construct the native tray before
        # the event loop. Observe the replacement, not just the launcher exit.
        deadline = time.monotonic() + 120
        while not any(r['step'] == 'startup-state' for r in read_steps(evidence, 0)):
            assert time.monotonic() < deadline, 'replacement startup observer timed out'
            time.sleep(.2)
        state = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-state'][0]['detail']
        text = log.read_text()
        assert state['mainVisible'], state
        assert state['dockIconVisible'] is True, f'a normal start must keep the Dock icon: {state}'
        assert 'encryption auto-unlocked via daemon' in text or 'skip auto-unlock' in text, text[-800:]
        assert 'no clipboard history entry to restore' in text or 'restored the most recent clipboard entry' in text, text[-800:]
        results['normalRestore'] = {'state': state, 'unlocked': 'encryption auto-unlocked via daemon' in text,
                                    'restoreStep': 'no clipboard history entry to restore' in text}
        results['passed'] = True
    finally:
        cli(p.cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'startup-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps(results, indent=2, ensure_ascii=False))
    assert results['passed']


if __name__ == '__main__':
    main()
