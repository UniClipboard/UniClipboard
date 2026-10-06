#!/usr/bin/env python3
"""Startup modes and the cold-launch sequence E2E.

One isolated profile; the launch preferences are stored through the shared settings API by a setup launch
(a full quit then stops the daemon, so each following launch is a cold start that spawns the daemon):
  - silent:      the main window is not created or shown, the "still running" notice is shown, the app keeps running;
  - lightweight cold start: the GUI exits by itself with the daemon left running, after the notice;
  - lightweight reopen: the daemon is already running, so the GUI shows the main window and does not notify;
  - normal cold start with restore enabled: the cold-launch sequence runs (auto-unlock from the keyring,
    lifecycle retry, restore attempt) and the window is shown.
The e2e build records notifications instead of showing them, and a native observer reports the window state
of a launch (a hidden window may not run page scripts). Not covered: an actual restore of a history entry
(the profile has no history; the restore call sequence stops at "no clipboard history entry to restore").
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


class Profile:
    def __init__(self, out):
        self.out = out
        self.home = tempfile.mkdtemp(prefix='uc-gui-go-')
        self.profile = 'gui-go-' + os.path.basename(self.home)
        path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
        self.cli_env = isolated_env(self.home, self.profile, {'PATH': path})
        self.base = {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1'}
        self.notify_log = Path(tempfile.mkdtemp(prefix='uc-gui-go-notify-')) / 'n.log'

    def launch(self, label, phase, exit_mode='full', observe=None, wait=True, timeout=150):
        evidence = self.out / f'startup-{label}.jsonl'
        evidence.write_text('')
        env = isolated_env(self.home, self.profile, dict(self.base, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_PHASE=phase,
                                                          UC_GUI_GO_EXIT_MODE=exit_mode, UC_GUI_GO_E2E_NOTIFY_LOG=str(self.notify_log),
                                                          **({'UC_GUI_GO_E2E_OBSERVE': str(observe)} if observe else {})))
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
        return cli(self.cli_env, '--json', 'start', check=False).returncode == 0


def set_mode(p, mode, restore=False):
    proc, evidence, _ = p.launch(f'set-{mode}', f'startup-set:{mode}' + (':restore' if restore else ''))
    assert proc.wait(timeout=60) == 0
    row = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-saved'][0]
    assert row['ok'], row


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
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

        # Silent: hidden window, notice, keeps running.
        set_mode(p, 'silent')
        proc, evidence, log = p.launch('silent', 'startup-observe', observe=12, exit_mode='full')
        assert proc.wait(timeout=90) == 0
        state = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-state'][0]['detail']
        assert not state['mainVisible'], f'silent start must keep the window hidden: {state}'
        silent_notes = [n for n in p.notes() if n.startswith('silent-start|')]
        assert len(silent_notes) == 1 and 'Still running in the background' in silent_notes[0], p.notes()
        assert 'encryption auto-unlocked via daemon' in log.read_text() or 'skip auto-unlock' in log.read_text(), 'cold launch recovery did not run'
        results['silent'] = {'state': state, 'notice': silent_notes[0][:60]}

        # Lightweight cold start: the GUI hands off by itself and leaves the daemon running.
        set_mode(p, 'lightweight')
        before = len(p.notes())
        proc, evidence, log = p.launch('lightweight-cold', 'startup-observe', observe=60, exit_mode='full')
        code = proc.wait(timeout=90)
        assert code == 0, code
        notes = p.notes()[before:]
        assert len(notes) == 1 and notes[0].startswith('lightweight-') and 'UniClipboard 仍在后台运行' in notes[0], notes
        assert not any(r['step'] == 'startup-state' for r in read_steps(evidence, 0)), 'the GUI should have exited before the observer'
        assert 'entering lightweight mode' in log.read_text() or 'lightweight cold start' in log.read_text()
        assert p.daemon_alive(), 'the daemon must keep running after the lightweight hand-off'
        results['lightweightCold'] = {'exitCode': code, 'notice': notes[0][:40], 'daemonAlive': True}

        # Lightweight reopen: the daemon is already running, so the window is shown and nothing is announced.
        before = len(p.notes())
        proc, evidence, log = p.launch('lightweight-reopen', 'startup-observe', observe=12, exit_mode='keep')
        assert proc.wait(timeout=90) == 0
        state = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-state'][0]['detail']
        assert state['mainVisible'], f'a lightweight reopen must show the window: {state}'
        assert p.notes()[before:] == [], p.notes()[before:]
        assert 'skipping cold-start recovery' in log.read_text()
        results['lightweightReopen'] = state

        # Normal cold start with restore: the setup launch quits fully and so stops the daemon, which makes
        # the next launch spawn it.
        set_mode(p, 'normal', restore=True)
        proc, evidence, log = p.launch('normal-restore', 'startup-observe', observe=15, exit_mode='full')
        assert proc.wait(timeout=90) == 0
        state = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-state'][0]['detail']
        text = log.read_text()
        assert state['mainVisible'], state
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
