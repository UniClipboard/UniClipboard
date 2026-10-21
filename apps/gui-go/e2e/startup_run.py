#!/usr/bin/env python3
"""Startup modes and the cold-launch sequence E2E.

One isolated profile; the launch preferences are stored through the shared settings API by a setup launch
(a full quit then stops the daemon, so each following launch is a cold start that spawns the daemon):
  - silent:      the main window is not created or shown, the "still running" notice is shown, the app keeps running
                 without a Dock icon;
  - silent Dock lifecycle: the tray's Open builds and shows the window with the Dock icon, a close request hides
    the window and removes the Dock icon, twice in a row;
  - lightweight cold start: the GUI exits by itself with the daemon left running, after the notice;
  - lightweight reopen: the daemon is already running, so the GUI shows the main window and does not notify;
  - normal cold start with restore enabled: the cold-launch sequence runs (auto-unlock from the keyring,
    lifecycle retry, restore attempt) and the window is shown with the Dock icon.
The e2e build records notifications and the requested Dock icon state (quiet mode keeps the test app an accessory, so the
real activation policy is not changed: the native Dock, app menu and focus behaviour is a manual check) instead of showing them, and a native observer reports the window state
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
        return cli(self.cli_env, '--json', 'start', check=False).returncode == 0


def set_mode(p, mode, restore=False):
    proc, evidence, _ = p.launch(f'set-{mode}', f'startup-set:{mode}' + (':restore' if restore else ''))
    assert proc.wait(timeout=60) == 0
    row = [r for r in read_steps(evidence, 0) if r['step'] == 'startup-saved'][0]
    assert row['ok'], row


def silent_dock_lifecycle(p):
    control = p.out / 'startup-silent-dock.control'
    control.write_text('')
    proc, evidence, _ = p.launch('silent-dock', 'wake', exit_mode='full', wait=False, control=control)
    commands = ['state boot', 'open-main 1', 'close-main 1', 'open-main 2', 'close-main 2', 'state end']
    want = ['control-state', 'open-main-1', 'close-main-1', 'open-main-2', 'close-main-2', 'control-state']
    rows = []
    try:
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline and not any(r['step'] == 'bootstrapped' for r in read_steps(evidence, 0)):
            assert proc.poll() is None, 'the silent GUI exited before its bootstrap finished'
            time.sleep(.3)
        for command, step in zip(commands, want):
            with control.open('a') as f:
                f.write(command + '\n')
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                done = [r for r in read_steps(evidence, 0) if r['step'] == step]
                if len(done) > sum(1 for r in rows if r['step'] == step):
                    rows.append(done[-1])
                    break
                time.sleep(.2)
            else:
                raise AssertionError(f'no {step} step after "{command}"')
        with control.open('a') as f:
            f.write('exit\n')
        assert proc.wait(timeout=90) == 0
    finally:
        if proc.poll() is None:
            proc.terminate()
    boot, opened1, closed1, opened2, closed2, end = rows
    assert not boot['detail']['mainExists'] and boot['detail']['dockIconVisible'] is False, boot
    for opened in (opened1, opened2):
        assert opened['ok'] and opened['detail']['dockIconVisible'] is True, opened
    for closed in (closed1, closed2):
        assert closed['ok'] and closed['detail']['dockIconVisible'] is False, closed
    assert end['detail']['mainExists'] and not end['detail']['mainVisible'] and end['detail']['dockIconVisible'] is False, end
    return [{'step': r['step'], 'ok': r['ok'], 'detail': r['detail']} for r in rows]


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
        assert state['dockIconVisible'] is False, f'silent start must run without a Dock icon: {state}'
        silent_notes = [n for n in p.notes() if n.startswith('silent-start|')]
        assert len(silent_notes) == 1 and 'Still running in the background' in silent_notes[0], p.notes()
        assert 'encryption auto-unlocked via daemon' in log.read_text() or 'skip auto-unlock' in log.read_text(), 'cold launch recovery did not run'
        results['silent'] = {'state': state, 'notice': silent_notes[0][:60]}

        # Silent Dock lifecycle: tray Open -> close -> tray Open -> close, driven through the control file.
        results['silentDock'] = silent_dock_lifecycle(p)

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
