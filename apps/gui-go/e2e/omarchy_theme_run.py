#!/usr/bin/env python3
"""Exercise Omarchy theme commands in the native Linux GUI with an isolated profile.

Build the frontend with VITE_GUI_GO_E2E=1 UC_UI_PLATFORM=linux, then the GUI
with -tags gtk3,e2e. Run inside dbus-run-session on an available display:
  python3 apps/gui-go/e2e/omarchy_theme_run.py --binaries DIR --out DIR

DIR contains gui-go, uniclip and uniclipd. Only synthetic theme files in the
sandbox are changed. The control-file harness calls the real HostService;
this verifies native parsing, polling, persistence and errors, not DOM styling.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from linux_xvfb_run import Gui
from run import isolated_env


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-omarchy-'))
    for name in ('gui-go', 'uniclip', 'uniclipd'):
        shutil.copy2(args.binaries / name, sandbox / name)
    home = sandbox / 'home'
    home.mkdir()
    profile = 'gui-go-' + sandbox.name
    extra = {key: os.environ[key] for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XAUTHORITY') if key in os.environ}
    # Preserve only the display connection; the private bus and profile own all app state.
    extra.update(UC_PORTABLE='1', UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_PHASE='wake',
                 UC_GUI_GO_EXIT_MODE='full', GDK_BACKEND='wayland,x11', UC_GPUI_QUICK_PANEL='0')
    if 'DBUS_SESSION_BUS_ADDRESS' in os.environ:
        extra['DBUS_SESSION_BUS_ADDRESS'] = os.environ['DBUS_SESSION_BUS_ADDRESS']
    env = isolated_env(str(home), profile, extra)
    theme = home / '.local/state/omarchy/current/theme/colors.toml'
    theme.parent.mkdir(parents=True)
    dark = 'mode = "dark"\nbackground = "#1a1b26"\nforeground = "#c0caf5"\naccent = "#7aa2f7"\n'
    light = 'mode = "light"\nbackground = "#ffffff"\nforeground = "#202020"\naccent = "#7733aa"\n'
    theme.write_text(dark)
    gui = None
    checks = []

    def check(name, ok, detail=None):
        checks.append(dict(check=name, ok=bool(ok), detail=detail))
        assert ok, checks[-1]

    def invoke(label, command, values=None):
        result = gui.invoke(label, command, values)
        check(label + '-command', result['ok'], result)
        return result['data']

    def read(label):
        return invoke(label, 'get_desktop_theme')

    def poll(label, predicate):
        for attempt in range(30):
            value = read(f'{label}-{attempt}')
            if predicate(value):
                return value
            time.sleep(.1)
        raise AssertionError(f'theme did not change: {value}')

    def start(label):
        process = Gui(sandbox, env, out, label)
        process.step('bootstrapped', 120)
        return process

    def stop():
        with gui.control.open('a') as f:
            f.write('exit\n')
        check('clean-exit', gui.proc.wait(timeout=40) == 0)

    try:
        cli = str(sandbox / 'uniclip')
        with (out / 'init.log').open('w') as log:
            subprocess.run([cli, 'space', 'init', '--passphrase', 'omarchy-synthetic-passphrase', '--device-name', 'theme-fixture'], env=env, check=True, timeout=120, stdout=log, stderr=subprocess.STDOUT)
        gui = start('fresh')
        initial = read('initial')
        check('omarchy-visible-default-off', initial['omarchyAvailable'] and not initial['followOmarchyTheme'] and initial['theme']['dark'], initial)
        enabled = invoke('enable', 'set_follow_omarchy_theme', {'enabled': True})
        check('enabled', enabled['followOmarchyTheme'] and enabled['revision'] > initial['revision'], enabled)
        theme.with_suffix('.next').write_text(light)
        theme.with_suffix('.next').replace(theme)
        changed = poll('changed', lambda s: not s['theme']['dark'])
        check('live-palette', changed['followOmarchyTheme'] and changed['theme']['variables']['--primary'] == '#7733aa' and changed['revision'] > enabled['revision'], changed)
        stop()
        gui = start('restart')
        restored = read('restored')
        check('preference-survives-restart', restored['followOmarchyTheme'] and restored['omarchyAvailable'], restored)
        disabled = invoke('disable', 'set_follow_omarchy_theme', {'enabled': False})
        check('disabled', not disabled['followOmarchyTheme'], disabled)
        storage = next(sandbox.rglob('desktop-theme.json'))
        check('only-preference-persisted', json.loads(storage.read_text()) == {'followOmarchyTheme': False})
        storage.unlink()
        storage.mkdir()
        failed = gui.invoke('save-failure', 'set_follow_omarchy_theme', {'enabled': True})
        check('save-error-propagated', not failed['ok'] and failed['error']['code'] == 'InternalError', failed)
        check('save-error-preserves-state', not read('after-error')['followOmarchyTheme'])
        storage.rmdir()
        theme.write_text('background = "url(invalid)"\n')
        invalid = poll('invalid', lambda s: not s['omarchyAvailable'])
        check('invalid-palette-refused', invalid['theme'] is None, invalid)
        theme.unlink()
        stop()
        gui = start('unsupported')
        unavailable = read('unsupported')
        check('other-desktops-unavailable', not unavailable['omarchyAvailable'], unavailable)
        unchanged = invoke('unsupported-enable', 'set_follow_omarchy_theme', {'enabled': True})
        check('unsupported-no-write', not unchanged['followOmarchyTheme'] and not storage.exists())
        stop()
        # Exercise the older Omarchy directory with the same production reader.
        legacy = home / '.config/omarchy/current/theme/colors.toml'
        legacy.parent.mkdir(parents=True)
        legacy.write_text(dark)
        gui = start('legacy')
        check('legacy-location', read('legacy')['omarchyAvailable'])
        stop()
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
            gui.proc.wait(timeout=20)
        subprocess.run([str(sandbox / 'uniclip'), 'stop'], env=env, timeout=40, capture_output=True)
        (out / 'omarchy-result.json').write_text(json.dumps(dict(sandbox=str(sandbox), checks=checks), indent=2) + '\n')
    print(f'Passed {len(checks)} checks; artifacts: {out}')


if __name__ == '__main__':
    main()
