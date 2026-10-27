#!/usr/bin/env python3
"""Exercise the tray action on private Xvfb and D-Bus; never send physical input.

Run under dbus-run-session. Requires a gtk4,production,e2e GUI. Verifies native
window teardown, daemon survival and explicit reopening, not real tray clicks.
WebKit sandboxing is disabled only for this synthetic lifecycle test.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import urllib.request

from linux_xvfb_run import Gui, read_steps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('gui', 'cli', 'daemon', 'xvfb', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-lightweight-', dir=out))
    for source, name in ((args.gui, 'gui-go'), (args.cli, 'uniclip'), (args.daemon, 'uniclipd')):
        shutil.copy2(source.resolve(), sandbox / name)
    home = sandbox / 'home'
    home.mkdir()
    runtime = sandbox / 'run'
    runtime.mkdir(mode=0o700)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('UC_', 'UNICLIPBOARD', 'WAYLAND', 'HYPRLAND', 'APPIMAGE', 'APPDIR', 'AT_SPI', 'XDG_'))}
    profile = 'gui-go-' + sandbox.name
    # Xvfb validates lifecycle only; bypass WebKit's colliding accessibility
    # D-Bus proxies in this synthetic profile, never in the product build.
    env['WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS'] = '1'
    env.update(HOME=str(home), XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(home / '.config'),
               XDG_DATA_HOME=str(home / '.local/share'), XDG_STATE_HOME=str(home / '.local/state'),
               XDG_CACHE_HOME=str(home / '.cache'), XDG_SESSION_TYPE='x11', GDK_BACKEND='x11',
               UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development',
               UC_DISABLE_SYSTEM_CLIPBOARD='1', UC_GUI_GO_ISOLATED='1', UC_GPUI_QUICK_PANEL='0',
               NO_AT_BRIDGE='1', UC_GUI_GO_E2E_SHORTCUTS='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', GTK_A11Y='none',
               UC_GUI_GO_E2E_NOTIFY_LOG=str(out / 'notifications.log'))
    result = {'passed': False, 'scope': 'private Xvfb; actual tray handler, no physical clicks; WebKit sandbox disabled', 'sandbox': str(sandbox)}
    gui = None
    readfd, writefd = os.pipe()
    xvfb = subprocess.Popen([str(args.xvfb), '-displayfd', str(writefd), '-screen', '0', '1280x800x24', '-nolisten', 'tcp'],
                            pass_fds=(writefd,), stdout=subprocess.DEVNULL, stderr=(out / 'xvfb.log').open('w'))
    os.close(writefd)
    with os.fdopen(readfd) as display:
        env['DISPLAY'] = ':' + display.readline().strip()

    def owners():
        matches = []
        for entry in Path('/proc').iterdir():
            if entry.name.isdigit():
                try:
                    if (entry / 'exe').resolve() == sandbox / 'gui-go':
                        matches.append(int(entry.name))
                except OSError:
                    pass
        return matches

    def cli(*argv):
        return subprocess.run([str(sandbox / 'uniclip'), *argv], env=env, check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)

    def window_state(label, mode=None):
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            tag = f'{label}-{time.monotonic_ns()}'
            with gui.control.open('a') as control:
                control.write(f'state {tag}\n')
            time.sleep(.2)
            rows = [r['detail'] for r in read_steps(gui.evidence)
                    if r['step'] == 'control-state' and r['detail'].get('label') == tag]
            if rows and rows[-1]['mainExists'] and (mode is None or rows[-1]['startupMode'] == mode and rows[-1]['lightweight'] == (mode == 'lightweight')):
                return rows[-1]
        raise AssertionError('main window did not become ready')

    try:
        cli('space', 'init', '--passphrase', 'gui-go-synthetic-passphrase', '--device-name', 'lightweight-test')
        cli('start')
        gui = Gui(sandbox, env, out, 'gui')
        gui.step('bootstrapped', 90)
        state = window_state('before')
        assert state['mainExists'] and state['uiPid'] and state['trayPresent'], state
        result['before'] = state
        with gui.control.open('a') as control:
            control.write('tray-lightweight\n')
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            gui.proc.poll()
            if not owners():
                break
            time.sleep(.2)
        result['remainingGUIProcesses'] = owners()
        assert not result['remainingGUIProcesses'], result
        time.sleep(2)
        assert not owners(), 'GUI reopened after the tray action'
        conn = json.loads(next(sandbox.rglob('daemon.conn')).read_text())
        assert conn['pid'] == state['daemonPid']
        assert Path(f'/proc/{conn["pid"]}/exe').resolve() == sandbox / 'uniclipd'
        result['daemonPreserved'] = True
        gui = Gui(sandbox, env, out, 'reopened')
        gui.step('bootstrapped', 90)
        reopened = window_state('reopened')
        assert reopened['mainExists'] and reopened['lightweight'] and reopened['trayPresent'], reopened
        result['reopened'] = reopened
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def request(path, body, authorization):
            req = urllib.request.Request(f"http://{conn['host']}:{conn['port']}{path}",
                                         data=json.dumps(body).encode(),
                                         method='PUT' if path == '/settings' else 'POST',
                                         headers={'Content-Type': 'application/json', 'Authorization': authorization})
            with opener.open(req, timeout=15) as response:
                return json.load(response)

        token = request('/auth/connect', {'pid': os.getpid(), 'clientType': 'cli'},
                        'Bearer ' + conn['token'])['data']['sessionToken']
        identity = ('pid', 'uiPid', 'mainWindowId', 'daemonPid')
        for mode in ('normal', 'lightweight', 'normal', 'lightweight'):
            request('/settings', {'general': {'startupMode': mode}}, 'Session ' + token)
            switched = window_state('switch-' + mode, mode)
            assert all(switched[key] == reopened[key] for key in identity), switched
            assert switched['trayPresent'] and switched['mainVisible'], switched
            deadline = time.monotonic() + 10
            while (switched['shortcutCount'] > 0) != (mode == 'normal') and time.monotonic() < deadline:
                switched = window_state('shortcuts-' + mode, mode)
            assert (switched['shortcutCount'] > 0) == (mode == 'normal'), switched
            result.setdefault('modeSwitches', []).append(switched)
        with gui.control.open('a') as control:
            control.write('close-main lightweight\n')
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            gui.proc.poll()
            if not owners():
                break
            time.sleep(.2)
        assert not owners(), 'closing the lightweight main window left its host/tray alive'
        assert Path(f'/proc/{conn["pid"]}/exe').resolve() == sandbox / 'uniclipd'
        result['lightweightClosePreservedDaemon'] = True
        result['passed'] = True
    finally:
        for pid in owners():
            if Path(f'/proc/{pid}/exe').resolve() == sandbox / 'gui-go':
                os.kill(pid, signal.SIGTERM)
        if gui:
            try:
                gui.proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                gui.proc.kill()
        subprocess.run([str(sandbox / 'uniclip'), 'stop'], env=env, capture_output=True, timeout=80)
        xvfb.terminate()
        xvfb.wait(timeout=10)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
