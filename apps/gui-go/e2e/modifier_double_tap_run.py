#!/usr/bin/env python3
"""Modifier double-tap monitor E2E (macOS host, quiet; exercises the Windows WebView-panel logic).

Real: the e2e GUI build with the WebView panel (UC_GPUI_QUICK_PANEL=0), a real daemon and settings API, the real
monitor goroutine (20 ms poll, detector, toggle request) and the host commands the settings page invokes
(`set_quick_panel_double_tap_modifier`, `set_quick_panel_enabled`, `get_quick_panel_double_tap_availability`).

SCRIPTED, stated plainly: UC_GUI_GO_E2E_SCRIPTED_KEYS=1 replaces the keyboard snapshot with a state the control file
drives (`modifier-script`), so no keyboard event is generated (quiet rules). That covers the Uni detection logic,
the timing window, enable/disable lifecycle, persistence across a restart and exit; it does NOT cover the Win32
GetAsyncKeyState read, which only a Windows host can (`windows_quick_panel_run.py`, not run: no host).
Panel visibility is not asserted (a sleeping display never repaints); `triggers` counts monitor triggers and
`lastShown` shows that a trigger started a show.
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
from run import ROOT, isolated_env, pid_alive, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
DOWN, UP, OTHER_DOWN = '1:0', '0:0', '1:1'


def tap(hold=60, gap=70):
    """One standalone tap: modifier down for `hold` ms, then released for `gap` ms."""
    return [f'{hold}:{DOWN}', f'{gap}:{UP}']


def script(*steps):
    return ' '.join(s for part in steps for s in (part if isinstance(part, list) else [part]))


class Run:
    def __init__(self, out, env, name):
        self.name = name
        self.evidence = out / f'{name}-native.jsonl'
        self.evidence.write_text('')
        self.control = out / f'{name}.control'
        self.control.write_text('')
        env = dict(env, UC_GUI_GO_EVIDENCE=str(self.evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(self.control))
        self.proc = subprocess.Popen([str(BINARY)], env=env, stdout=(out / f'{name}-gui.log').open('w'), stderr=subprocess.STDOUT)
        self.n = 0

    def step(self, name, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(self.evidence, 0) if r['step'] == name]
            if rows:
                return rows[-1]
            if self.proc.poll() is not None:
                raise RuntimeError(f'GUI exited ({self.proc.returncode}) before {name}')
            time.sleep(.1)
        raise RuntimeError(f'timeout waiting for {name}')

    def ctl(self, line, label):
        with self.control.open('a') as f:
            f.write(line + '\n')
        return self.step(label)

    def label(self):
        self.n += 1
        return f'{self.name}{self.n}'

    def state(self):
        label = self.label()
        return self.ctl(f'modifier-state {label}', f'modifier-state-{label}')['detail']

    def play(self, *steps):
        label = self.label()
        return self.ctl(f'modifier-script {label} {script(*steps)}', f'modifier-script-{label}')['detail']

    def invoke(self, command, args=None):
        label = self.label()
        return self.ctl(f'invoke {label} {command} {json.dumps(args) if args is not None else ""}', f'invoke-{label}')['detail']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    out = parser.parse_args().out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_E2E_PHASE': 'wake',
                                           'UC_GUI_GO_E2E_SHORTCUTS': '1', 'UC_GUI_GO_E2E_DEFAULT_SHORTCUT': 'ctrl+alt+shift+f13',
                                           'UC_GUI_GO_E2E_SCRIPTED_KEYS': '1', 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'home': home, 'profile': profile, 'passed': False, 'scriptedKeys': True, 'checks': []}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    run = None
    daemon_pid = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'modifier-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        run = Run(out, gui_env, 'a')
        run.step('bootstrapped', 120)
        daemon_pid = json.loads((Path(home) / 'Library/Application Support' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn').read_text())['pid']
        base = run.state()
        check('fresh profile: no modifier watched, poll worker absent', base['monitor'] == 'disabled', base)
        av = run.invoke('get_quick_panel_double_tap_availability')
        check('availability is supported for the WebView panel', av['ok'] and av['data'] == 'supported', av)

        bad = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'shift'})
        check('unknown modifier is a ValidationError', not bad['ok'] and bad['error']['code'] == 'ValidationError', bad)
        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'alt'})
        s = run.state()
        check('selecting alt persists it and starts the monitor', r['ok'] and s['monitor'] == 'alt', [r, s])

        def triggers_of(*steps):
            before = run.state()
            d = run.play(*steps)
            after = run.state()
            return d['triggers'], after['lastShown'] != before['lastShown'], d

        n, shown, d = triggers_of(f'60:{UP}', tap(), tap())
        check('two clean taps inside 400 ms trigger once and start a panel show', n == 1 and shown, d)
        n, shown, d = triggers_of(f'60:{UP}', tap(), f'600:{UP}', tap())
        check('second tap after 600 ms does not trigger', n == 0 and not shown, d)
        n, _, d = triggers_of(f'60:{UP}', tap(), tap(), tap())
        check('taps 1+2 trigger once; tap 3 right after does not trigger again (pending tap cleared)', n == 1, d)
        n, _, d = triggers_of(f'60:{UP}', tap(), tap())
        check('...and a following double tap triggers again', n == 1, d)
        n, _, d = triggers_of(f'60:{UP}', f'60:{DOWN}', f'40:{OTHER_DOWN}', f'70:{UP}', tap())
        check('another key during a tap invalidates it: that tap is not a first tap (invalid tap, then one clean tap: no trigger)', n == 0, d)
        n, _, d = triggers_of(f'60:{UP}', tap(), f'40:0:1', f'60:{UP}', tap())
        check('another key between taps clears the pending first tap', n == 0, d)
        # Seeding: restart the detection while the modifier is held. Its release must not count as a first tap, so a
        # single clean tap afterwards cannot complete a double tap.
        run.play(f'60:{DOWN}')
        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'control'})
        time.sleep(.2)
        n, _, d = triggers_of(f'60:{UP}', tap())
        check('modifier already held when detection starts: its release is not a first tap (one tap afterwards: no trigger)', r['ok'] and n == 0, [r, d])
        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'alt'})

        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'control'})
        s = run.state()
        check('switching to control keeps one monitor and restarts detection', r['ok'] and s['monitor'] == 'control', [r, s])
        n, _, d = triggers_of(f'60:{UP}', tap(), tap())
        check('control double tap triggers', n == 1, d)

        r = run.invoke('set_quick_panel_enabled', {'enabled': False})
        s = run.state()
        n, _, d = triggers_of(f'60:{UP}', tap(), tap())
        check('disabling the quick panel stops the monitor; taps do nothing', r['ok'] and s['monitor'] == 'disabled' and n == 0, [r, s, d])
        r = run.invoke('set_quick_panel_enabled', {'enabled': True})
        s = run.state()
        n, _, d = triggers_of(f'60:{UP}', tap(), tap())
        check('re-enabling resumes the stored modifier (control) and it triggers', r['ok'] and s['monitor'] == 'control' and n == 1, [r, s, d])

        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'meta'})
        check('meta accepted', r['ok'] and run.state()['monitor'] == 'meta', r)
        run.ctl('exit', 'control-exit')
        code = run.proc.wait(timeout=60)
        deadline = time.monotonic() + 15
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.2)
        check('quit with a running monitor: GUI exits 0 and the daemon is gone', code == 0 and not pid_alive(daemon_pid), {'exit': code})

        # Second launch against the same data root: the stored choice is applied at startup.
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        run = Run(out, gui_env, 'b')
        run.step('bootstrapped', 120)
        s = None
        for _ in range(40):
            s = run.state()
            if s['monitor'] != 'disabled':
                break
            time.sleep(.3)
        check('restart: the stored modifier (meta) is watched again from the settings', s['monitor'] == 'meta', s)
        n = run.play(f'60:{UP}', tap(), tap())['triggers']
        check('restart: a double tap triggers without touching the setting', n == 1, n)
        r = run.invoke('set_quick_panel_double_tap_modifier', {'modifier': 'disabled'})
        s = run.state()
        n = run.play(f'60:{UP}', tap(), tap())['triggers']
        check('disabled: the monitor is released and taps do nothing', r['ok'] and s['monitor'] == 'disabled' and n == 0, [r, s, n])
        run.ctl('exit', 'control-exit')
        code = run.proc.wait(timeout=60)
        check('second run exits 0', code == 0, code)
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        for r in [run]:
            if r and r.proc.poll() is None:
                r.proc.terminate()
                r.proc.wait(timeout=20)
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'modifier-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps({'passed': results['passed'], 'checks': len(checks)}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
