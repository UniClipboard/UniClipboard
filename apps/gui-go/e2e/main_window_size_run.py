#!/usr/bin/env python3
"""Main-window size persistence through real GTK/WebView and daemon launches.

Run inside a private Xvfb (1920x1200) + Openbox + D-Bus session, with the
frontend built with VITE_GUI_GO_E2E=1 and the GUI built with gtk4,e2e:
  python3 main_window_size_run.py --gui <gui> --daemon <uniclipd> --cli <uniclip> --out <artifacts>
Only the private display receives input. All data and credentials use a temporary portable profile.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import uuid
import urllib.error
import urllib.request

from linux_xvfb_run import Gui, read_steps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('gui', 'daemon', 'cli', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    results = []
    gui = None
    with tempfile.TemporaryDirectory(prefix='uc-gui-go-size-') as temporary:
        sandbox = Path(temporary)
        for source, name in ((args.gui, 'gui-go'), (args.daemon, 'uniclipd'), (args.cli, 'uniclip')):
            shutil.copy2(source, sandbox / name)
        profile = 'gui-go-size-' + uuid.uuid4().hex[:12]
        env = dict(os.environ, HOME=str(sandbox), UC_PROFILE=profile, UC_PORTABLE='1',
                   UC_GUI_GO_ISOLATED='1', UC_DISABLE_SYSTEM_CLIPBOARD='1',
                   UNICLIPBOARD_ENV='development', UC_GPUI_QUICK_PANEL='0',
                   UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1',
                   UC_GUI_GO_EXIT_MODE='full', WEBKIT_DISABLE_DMABUF_RENDERER='1',
                   GDK_BACKEND='x11', GDK_SCALE='1', GDK_DPI_SCALE='1',
                   XDG_CURRENT_DESKTOP='Openbox', XDG_SESSION_TYPE='x11')
        for name in ('DATA_HOME', 'CONFIG_HOME', 'CACHE_HOME', 'STATE_HOME', 'RUNTIME_DIR'):
            directory = sandbox / name.lower()
            directory.mkdir(mode=0o700)
            env['XDG_' + name] = str(directory)
        env.pop('WAYLAND_DISPLAY', None)
        env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
        env.pop('NIRI_SOCKET', None)
        env.pop('APPIMAGE', None)
        env['PATH'] = str(sandbox) + os.pathsep + env['PATH']
        cli = str(sandbox / 'uniclip')
        root = sandbox / 'data' / ('app.uniclipboard.desktop-' + profile)
        config = root / 'desktop-settings.json'

        def settings_request(method='GET', body=None):
            active_root = sandbox / 'data' / ('app.uniclipboard.desktop-' + env['UC_PROFILE'])
            conn = json.loads((active_root / 'daemon.conn').read_text())

            def call(path, method, body, auth):
                request = urllib.request.Request(f"http://{conn['host']}:{conn['port']}{path}", method=method,
                                                 data=json.dumps(body).encode() if body is not None else None,
                                                 headers={'Authorization': auth, 'Content-Type': 'application/json'})
                with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=15) as response:
                    return json.load(response)['data']

            token = call('/auth/connect', 'POST', {'pid': os.getpid(), 'clientType': 'cli'},
                         'Bearer ' + conn['token'])['sessionToken']
            return call('/settings', method, body, 'Session ' + token)

        def check(name, condition, detail=None):
            results.append({'check': name, 'passed': bool(condition), 'detail': detail})
            assert condition, (name, detail)
            print('PASS ' + name, flush=True)

        def launch(label):
            nonlocal gui
            gui = Gui(sandbox, env, out, label)
            gui.step('bootstrapped')
            if settings_request()['general']['startupMode'] == 'normal':
                gui.step('ui-page-ready')
            return gui

        sample = 0

        def size():
            nonlocal sample
            sample += 1
            label = f'size-{sample}'
            detail = gui.ctl('window-frame-state ' + label, 'window-frame-state-' + label)['detail']
            return detail['width'], detail['height']

        def window_id():
            nonlocal sample
            sample += 1
            label = f'ui-pid-{sample}'
            gui.ctl('state ' + label, 'control-state')
            deadline = time.monotonic() + 20
            while True:
                rows = read_steps(gui.evidence)
                state = next((r['detail'] for r in rows if r.get('step') == 'control-state' and r.get('detail', {}).get('label') == label), None)
                if state is not None:
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError('UI PID request timed out')
                time.sleep(.1)
            check('main window belongs to UI child', state['uiPid'] > 0 and state['uiPid'] != gui.proc.pid, state)
            return subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--pid', str(state['uiPid']),
                                            '--name', '^UniClipboard$'], env=env, text=True).splitlines()[0]

        def resize(width, height):
            subprocess.run(['xdotool', 'windowsize', '--sync', window_id(), str(width), str(height)],
                           env=env, check=True, timeout=15)
            deadline = time.monotonic() + 10
            while size() != (width, height) and time.monotonic() < deadline:
                time.sleep(.1)
            check('native resize', size() == (width, height), size())

        def quit_gui():
            gui.ctl('exit', 'control-exit')
            gui.proc.wait(timeout=40)
            check('clean exit', gui.proc.returncode == 0)

        try:
            with (out / 'space-init.log').open('w') as log:
                subprocess.run([cli, 'space', 'init', '--passphrase', 'size-test-passphrase',
                                '--device-name', 'size-test'], env=env, stdout=log,
                               stderr=subprocess.STDOUT, check=True, timeout=90)
            gui = launch('fresh')
            check('fresh default', size() == (1100, 720), size())
            check('daemon exposes desktop window preference', settings_request()['general']['mainWindowSize'] ==
                  {'width': 1100, 'height': 720})
            for invalid in ({'width': 0, 'height': 810}, {'width': 1200}, {'width': 1200, 'height': 2147483648}):
                try:
                    settings_request('PUT', {'general': {'mainWindowSize': invalid}})
                except urllib.error.HTTPError as error:
                    check('API rejects invalid window preference', error.code == 400, error.code)
                else:
                    check('API rejects invalid window preference', False)
            check('fresh window is not maximized', settings_request()['general']['mainWindowMaximized'] is False)
            try:
                settings_request('PUT', {'general': {'mainWindowMaximized': 'true'}})
            except urllib.error.HTTPError as error:
                check('API rejects invalid maximized state', error.code == 400, error.code)
            else:
                check('API rejects invalid maximized state', False)
            resize(1234, 810)
            quit_gui()
            gui = launch('restart')
            check('resize survives full exit and restart', size() == (1234, 810), size())
            check('restart reads the daemon-owned preference', settings_request()['general']['mainWindowSize'] ==
                  {'width': 1234, 'height': 810})
            check('no independent window preference file', not (root / 'main-window.json').exists())
            resize(1210, 790)
            restart = gui.invoke('restart-daemon', 'restart_daemon')
            check('daemon restarts successfully', restart.get('error') is None, restart)
            check('daemon restart flushes the latest native size', settings_request()['general']['mainWindowSize'] ==
                  {'width': 1210, 'height': 790})
            resize(1030, 680)
            gui.ctl('close-main hidden', 'close-main-hidden')
            quit_gui()
            gui = launch('hidden-restart')
            check('hidden size survives restart', size() == (1030, 680), size())
            resize(1140, 760)
            subprocess.run(['wmctrl', '-ir', window_id(), '-b', 'add,maximized_vert,maximized_horz'],
                           env=env, check=True, timeout=15)
            time.sleep(1)
            check('window maximized', size() != (1140, 760), size())
            quit_gui()
            gui = launch('maximized-restart')
            check('maximized state survives restart', size() != (1140, 760), size())
            stored = settings_request()['general']
            check('daemon persists maximized state', stored['mainWindowMaximized'] is True)
            check('maximization preserves normal size', stored['mainWindowSize'] == {'width': 1140, 'height': 760})
            subprocess.run(['wmctrl', '-ir', window_id(), '-b', 'remove,maximized_vert,maximized_horz'],
                           env=env, check=True, timeout=15)
            time.sleep(1)
            check('unmaximize restores normal size', size() == (1140, 760), size())
            quit_gui()
            gui = launch('unmaximized-restart')
            check('unmaximized state survives restart', size() == (1140, 760), size())
            check('daemon clears maximized state', settings_request()['general']['mainWindowMaximized'] is False)
            subprocess.run(['wmctrl', '-ir', window_id(), '-b', 'add,fullscreen'],
                           env=env, check=True, timeout=15)
            time.sleep(1)
            check('window fullscreen', size() != (1140, 760), size())
            quit_gui()
            gui = launch('fullscreen-restart')
            check('fullscreen size does not replace normal size', size() == (1140, 760), size())
            subprocess.run(['xdotool', 'windowminimize', '--sync', window_id()],
                           env=env, check=True, timeout=15)
            quit_gui()
            gui = launch('minimized-restart')
            check('minimized size does not replace normal size', size() == (1140, 760), size())
            settings_request('PUT', {'general': {'startupMode': 'silent'}})
            quit_gui()
            gui = launch('silent')
            check('silent launch creates no main window', not gui.step('bootstrapped')['detail']['mainExists'])
            settings_request('PUT', {'general': {'startupMode': 'normal'}})
            quit_gui()
            gui = launch('silent-restart')
            check('silent exit preserves saved size', size() == (1140, 760), size())
            quit_gui()
            for label, value in (('corrupt', '{broken'), ('invalid', '{"general":{"mainWindowSize":{"width":0,"height":-5}}}')):
                config.write_text(value)
                gui = launch(label)
                check(label + ' config uses defaults', size() == (1100, 720), size())
                quit_gui()
            (out / 'stored-config.json').write_bytes(config.read_bytes())
            original = config.read_bytes()
            env['UC_PROFILE'] = profile + '-other'
            gui = launch('other-profile')
            check('another profile uses defaults', size() == (1100, 720), size())
            resize(980, 640)
            quit_gui()
            check('another profile leaves original preference unchanged', config.read_bytes() == original)
        finally:
            if gui and gui.proc.poll() is None:
                gui.proc.terminate()
                gui.proc.wait(timeout=30)
            subprocess.run([cli, 'stop'], env=env, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=30)
            (out / 'result.json').write_text(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
