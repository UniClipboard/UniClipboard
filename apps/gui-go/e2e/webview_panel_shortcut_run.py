#!/usr/bin/env python3
"""WebView quick panel global-shortcut E2E (macOS, quiet).

Real: the e2e GUI build with the WebView panel (UC_GPUI_QUICK_PANEL=0), a real daemon and settings API, and the real
Wails `app.GlobalShortcut` registration (Carbon hot keys on macOS) of an unused combination. Checked through the same
the generated `HostService` bindings the WebView calls: default registration, rebinding, an unparsable shortcut being refused with the old binding
kept (and the setting unchanged), a two-step chord, disabling and re-enabling the panel, `--quick-panel` from a real
second process toggling the panel, and the paste commands refusing instead of pretending to succeed.

INJECTED, stated plainly: no keyboard event is generated (quiet rules). `shortcut-press` calls the handlers the OS
callback would call, so toggle parity and chord timing are covered but NOT the OS binding firing on a real key press.
macOS Carbon does not report a hot key owned by another process, so conflict reporting is not covered here either;
real key delivery, conflicts and paste are the Windows host script `windows_quick_panel_run.py` (not run: no host).
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
F13, F14 = 'ctrl+alt+shift+f13', 'ctrl+alt+shift+f14'
KEY = 'global.toggleQuickPanel'


class Run:
    def __init__(self, out, env):
        self.out, self.evidence = out, out / 'shortcut-native.jsonl'
        self.evidence.write_text('')
        self.control = out / 'shortcut.control'
        self.control.write_text('')
        self.env = dict(env, UC_GUI_GO_EVIDENCE=str(self.evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(self.control))
        self.proc = subprocess.Popen([str(BINARY)], env=self.env, stdout=(out / 'shortcut-gui.log').open('w'), stderr=subprocess.STDOUT)
        self.checks = []
        self.asleep = display_asleep()

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

    def state(self, label):
        return self.ctl(f'shortcut-state {label}', f'shortcut-state-{label}')['detail']

    def invoke(self, label, command, args=None):
        return self.ctl(f'invoke {label} {command} {json.dumps(args) if args is not None else ""}', f'invoke-{label}')['detail']

    def press(self, label, *spec):
        return self.ctl(f'shortcut-press {label} {" ".join(spec)}', f'shortcut-press-{label}')

    def check(self, name, ok, detail=None):
        self.checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def visible_check(self, name, ok, detail=None):
        """The two-phase show needs the page to repaint, which a sleeping display never does: report, do not fail."""
        if self.asleep:
            self.checks.append({'check': name, 'ok': None, 'skipped': 'display asleep: window visibility cannot be observed', 'observed': ok})
            print('SKIP ' + name + f' (display asleep; observed={ok})', flush=True)
        else:
            self.check(name, ok, detail)


def display_asleep():
    import ctypes
    cg = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
    cg.CGMainDisplayID.restype = ctypes.c_uint32
    cg.CGDisplayIsAsleep.argtypes = [ctypes.c_uint32]
    return bool(cg.CGDisplayIsAsleep(cg.CGMainDisplayID()))


def combos(state):
    return sorted(c.lower() for c in state['wails'])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_E2E_PHASE': 'wake',
                                           'UC_GUI_GO_E2E_SHORTCUTS': '1', 'UC_GUI_GO_E2E_DEFAULT_SHORTCUT': F13, 'UC_GUI_GO_EXIT_MODE': 'full'})
    results = {'home': home, 'profile': profile, 'passed': False, 'injectedKeyPresses': True, 'displayAsleep': display_asleep()}
    run = None
    daemon_pid = None
    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'shortcut-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        run = Run(out, gui_env)
        run.step('bootstrapped', 120)
        for _ in range(100):  # the panel page reports ready once it has loaded
            if run.state('boot')['panelReady']:
                break
            time.sleep(.3)
        s = run.state('default')
        daemon_pid = json.loads((Path(home) / 'Library/Application Support' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn').read_text())['pid']
        run.check('default shortcut registered with Wails', len(combos(s)) == 1 and 'f13' in combos(s)[0] and s['recorded'] == [F13], s)
        run.check('panel starts hidden', not s['panelVisible'])

        shown = s['lastShown']

        def initiated(label):
            """Whether the last step started a show (phase one stamps lastShown), which needs no rendering."""
            nonlocal shown
            current = run.state(label)
            changed, shown = current['lastShown'] != shown, current['lastShown']
            return changed, current

        run.press('p1', 'single')
        changed, cur = initiated('after-p1')
        run.check('injected press starts showing the panel', changed, cur)
        run.visible_check('and the panel becomes visible', cur['panelVisible'])
        run.press('p2', 'single')
        changed, cur = initiated('after-p2')
        run.visible_check('a second injected press hides it again (no new show)', not changed and not cur['panelVisible'])

        code = subprocess.run([str(BINARY), '--quick-panel'], env=dict(gui_env, UC_GUI_GO_EVIDENCE=str(out / 'second.jsonl')), timeout=60,
                              stdout=(out / 'second.log').open('w'), stderr=subprocess.STDOUT).returncode
        time.sleep(.8)
        changed, cur = initiated('after-second')
        run.check('real second `--quick-panel` process exits 0 and the first instance starts a show', code == 0 and changed, [code, cur])
        subprocess.run([str(BINARY), '--quick-panel'], env=dict(gui_env, UC_GUI_GO_EVIDENCE=str(out / 'second.jsonl')), timeout=60,
                       stdout=(out / 'second.log').open('a'), stderr=subprocess.STDOUT)
        time.sleep(.8)
        changed, cur = initiated('after-second-2')
        run.visible_check('and the next one hides it (no new show)', not changed and not cur['panelVisible'])

        r = run.invoke('rebind', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F14}})
        s = run.state('rebound')
        run.check('rebind: old released, new registered, setting stored',
                  r['ok'] and len(combos(s)) == 1 and 'f14' in combos(s)[0] and s['stored'] == F14 and s['recorded'] == [F14], [r, s])

        r = run.invoke('invalid', 'update_keyboard_shortcuts', {'shortcuts': {KEY: 'ctrl+alt+shift+nokey'}})
        s = run.state('after-invalid')
        run.check('unparsable shortcut: refused as Conflict, old binding and stored setting kept',
                  not r['ok'] and r['error']['code'] == 'Conflict' and len(combos(s)) == 1 and 'f14' in combos(s)[0] and s['stored'] == F14 and s['recorded'] == [F14], [r, s])

        chord = f'{F13} {F14}'
        r = run.invoke('chord', 'update_keyboard_shortcuts', {'shortcuts': {KEY: chord}})
        s = run.state('chord')
        run.check('two-step chord registers both steps', r['ok'] and len(combos(s)) == 2 and s['stored'] == chord, [r, s])
        initiated('chord-base')
        run.press('c1', 'second', F14)
        run.check('second step alone does nothing', not initiated('c1')[0])
        run.press('c2', 'leader', F13, F14)
        run.press('c3', 'second', F14)
        run.check('leader then second within the window starts a show', initiated('c3')[0])
        run.press('c4', 'single')  # toggles again (hides when visible; starts another show on a sleeping display)
        initiated('c4')
        run.press('c5', 'leader', F13, F14)
        time.sleep(1.2)
        run.press('c6', 'second', F14)
        run.check('second step after the 1 s window does nothing', not initiated('c6')[0])

        r = run.invoke('double', 'update_keyboard_shortcuts', {'shortcuts': {KEY: f'{F13} {F13}'}})
        s = run.state('double')
        run.check('same-key chord registers one OS shortcut', r['ok'] and len(combos(s)) == 1, [r, s])
        initiated('double-base')
        run.press('d1', 'leader', F13, F13)
        run.check('first tap of a same-key chord does nothing', not initiated('d1')[0])
        run.press('d2', 'leader', F13, F13)
        run.check('second tap within the window starts a show', initiated('d2')[0])

        r = run.invoke('disable', 'set_quick_panel_enabled', {'enabled': False})
        s = run.state('disabled')
        initiated('disabled-base')
        run.press('x1', 'single')
        run.check('disabled panel: shortcuts released, stored enabled=false, a press is ignored',
                  r['ok'] and s['wails'] == [] and s['recorded'] == [] and s['enabled'] is False and not initiated('x1')[0], [r, s])
        r = run.invoke('enable', 'set_quick_panel_enabled', {'enabled': True})
        s = run.state('enabled')
        run.check('re-enabled panel registers the stored shortcut again', r['ok'] and len(combos(s)) == 1 and s['enabled'] is True, [r, s])

        a = run.invoke('paste', 'paste_to_previous_app')
        b = run.invoke('type-paths', 'type_file_paths_to_previous_app', {'request': {'filePaths': ['/tmp/a b.txt']}})
        c = run.invoke('type-empty', 'type_file_paths_to_previous_app', {'request': {'filePaths': []}})
        run.check('paste commands refuse on this platform instead of succeeding',
                  not a['ok'] and 'not yet supported' in str(a['error']) and not b['ok'] and 'not yet supported' in str(b['error']) and not c['ok'], [a, b, c])

        run.ctl('exit', 'control-exit')
        code = run.proc.wait(timeout=60)
        deadline = time.monotonic() + 15
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.2)
        run.check('quit: GUI exits 0 and the daemon it started is gone', code == 0 and not pid_alive(daemon_pid), {'exit': code})
        results['checks'] = run.checks
        controller = [c for c in run.checks if c['ok'] is not None]
        unobserved = [c for c in run.checks if c['ok'] is None]
        results['controllerAssertions'] = {'passed': all(c['ok'] for c in controller), 'count': len(controller)}
        # Window visibility is the native panel contract; the lastShown stamp only shows that the controller started a show.
        results['nativeVisibleAssertions'] = {'status': 'unverified' if unobserved else 'passed', 'checks': [c['check'] for c in unobserved],
                                              'reason': 'main display asleep (CGDisplayIsAsleep=1); correlation only, not a diagnosed cause' if unobserved else None}
        results['displayAsleepAtEnd'] = display_asleep()
        results['passed'] = results['controllerAssertions']['passed'] and not unobserved
    finally:
        if run and run.proc.poll() is None:
            run.proc.terminate()
            run.proc.wait(timeout=20)
        if run:
            results.setdefault('checks', run.checks)
        cli(cli_env, '--json', 'stop', check=False, timeout=80)
        (out / 'shortcut-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps({k: results.get(k) for k in ('controllerAssertions', 'nativeVisibleAssertions', 'displayAsleep', 'displayAsleepAtEnd', 'passed')}, indent=2))
    if not results['controllerAssertions']['passed']:
        sys.exit(1)
    if not results['passed']:
        sys.exit(3)  # partial: controller path verified, native visibility not observable


if __name__ == '__main__':
    main()
