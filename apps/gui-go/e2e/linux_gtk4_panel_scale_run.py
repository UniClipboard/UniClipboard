#!/usr/bin/env python3
"""Measure actual GTK4 panel sizes after DOM shortcuts on a private Wayland compositor.

Supply a headless compositor's WAYLAND_DISPLAY/XDG_RUNTIME_DIR and a private
D-Bus session. No physical input is generated; all profiles and clipboard access
are isolated. GUI must use gtk4,production,e2e; frontend is the normal Linux build.
"""
import argparse
import http.server
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import urllib.parse
import uuid

from linux_xvfb_run import Gui
from run import ROOT, isolated_env


def fixed_window_constraints(protocol, expected):
    panel_ids = set(re.findall(r'xdg_toplevel#(\d+)\.set_title\("(?:Quick Panel|app\.uniclipboard\.desktop)"\)', protocol))

    def sizes(kind):
        pattern = rf'xdg_toplevel#(\d+)\.set_{kind}_size\((\d+), (\d+)\)'
        return {(int(width), int(height)) for ident, width, height in re.findall(pattern, protocol) if ident in panel_ids}

    minimums, maximums = sizes('min'), sizes('max')
    return {'expected': sorted(expected), 'minimums': sorted(minimums), 'maximums': sorted(maximums),
            'ok': expected <= minimums and expected <= maximums}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gui', type=Path, required=True)
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/gui-go/uniclip')
    parser.add_argument('--daemon', type=Path, default=ROOT / 'target/debug/uniclipd')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    result = {'passed': False, 'measurements': [], 'scope': 'GTK4 WebView on private Wayland; DOM shortcuts, no physical input'}
    replies = {}

    class Reporter(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            query = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            if 'id' in query and 'value' in query:
                replies[query['id'][0]] = json.loads(query['value'][0])
            self.send_response(200)
            self.end_headers()

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Reporter)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix='uc-gui-go-scale-', dir=out) as temp:
        sandbox = Path(temp)
        for source, name in [(args.gui, 'gui-go'), (args.cli, 'uniclip'), (args.daemon, 'uniclipd')]:
            shutil.copy2(source.resolve(), sandbox / name)
        env = isolated_env(str(sandbox / 'home'), 'gui-go-scale-' + uuid.uuid4().hex[:8], {
            'WAYLAND_DISPLAY': os.environ['WAYLAND_DISPLAY'], 'XDG_RUNTIME_DIR': os.environ['XDG_RUNTIME_DIR'],
            'DBUS_SESSION_BUS_ADDRESS': os.environ['DBUS_SESSION_BUS_ADDRESS'], 'GDK_BACKEND': 'wayland',
            'WAYLAND_DEBUG': 'client', 'GTK_A11Y': 'none', 'XDG_SESSION_TYPE': 'wayland', 'UC_PORTABLE': '1',
            'UC_UI_PLATFORM': 'linux', 'UC_GUI_GO_ISOLATED': '1', 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_E2E_PHASE': 'wake',
            'UC_GUI_GO_EXIT_MODE': 'full', 'PATH': str(sandbox) + os.pathsep + os.environ['PATH'],
        })
        Path(env['HOME']).mkdir()
        gui = None

        def probe(script):
            label = uuid.uuid4().hex
            report = f'http://127.0.0.1:{server.server_port}/?id={label}&value='
            js = '(async()=>{try{const value=await(async()=>{' + script + '})();await fetch(' + json.dumps(report) + '+encodeURIComponent(JSON.stringify(value)),{mode:"no-cors"})}catch(e){await fetch(' + json.dumps(report) + '+encodeURIComponent(JSON.stringify({error:String(e)})),{mode:"no-cors"})}})()'
            gui.ctl(f'panel-js {label} {js}', f'panel-js-{label}')
            deadline = time.monotonic() + 20
            while label not in replies and time.monotonic() < deadline:
                time.sleep(.1)
            value = replies.pop(label)
            if 'error' in value:
                raise RuntimeError(value['error'])
            return value

        def measure(target):
            deadline = time.monotonic() + 5
            while True:
                label = uuid.uuid4().hex
                frame = gui.ctl(f'window-frame-state {label} quick-panel', f'window-frame-state-{label}')['detail']
                if (frame.get('width'), frame.get('height')) == target or time.monotonic() > deadline:
                    break
                time.sleep(.1)
            dom = probe('return {width:innerWidth,height:innerHeight,scale:localStorage.getItem("uniclipboard.quickPanel.windowScale"),dpr:devicePixelRatio}')
            ok = (frame.get('width'), frame.get('height')) == target and abs(dom['width'] * dom['dpr'] - target[0]) < 2 and abs(dom['height'] * dom['dpr'] - target[1]) < 2
            result['measurements'].append({'target': target, 'native': frame, 'dom': dom, 'ok': ok})
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')

        try:
            with (out / 'cli.log').open('w') as log:
                subprocess.run([str(sandbox / 'uniclip'), 'space', 'init', '--passphrase', 'gui-go-synthetic-passphrase', '--device-name', 'scale-test'],
                               env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
            gui = Gui(sandbox, env, out, 'gui', args=('--autostart',))
            gui.step('bootstrapped', 90)
            subprocess.run([str(sandbox / 'gui-go'), '--quick-panel'], env=env, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            # Wait for the page's normal ready/finalize handshake and daemon connection.
            time.sleep(2)
            deadline = time.monotonic() + 20
            while not probe('return {ready:!!document.querySelector("[data-ui-skin]")}')['ready'] and time.monotonic() < deadline:
                time.sleep(.2)
            result['platform'] = probe('return {host:window.__UC_DESKTOP_HOST__,platform:navigator.platform,ua:navigator.userAgent}')
            print(json.dumps(result['platform']), flush=True)
            measure((800, 560))
            for key, code, target in [('=', 'Equal', (880, 616)), ('-', 'Minus', (800, 560)),
                                      ('-', 'Minus', (720, 504)), ('-', 'Minus', (640, 448)),
                                      ('-', 'Minus', (640, 448)), ('=', 'Equal', (720, 504))]:
                probe('for(const type of ["keydown","keyup"])document.body.dispatchEvent(new KeyboardEvent(type,{key:' + json.dumps(key) + ',code:' + json.dumps(code) + ',ctrlKey:true,bubbles:true}));return {}')
                time.sleep(.2)
                measure(target)
            # Hiding and showing the panel must restore its saved window scale.
            subprocess.run([str(sandbox / 'gui-go'), '--quick-panel'], env=env, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            time.sleep(1)
            subprocess.run([str(sandbox / 'gui-go'), '--quick-panel'], env=env, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            time.sleep(2)
            measure((720, 504))
            for factor in [1, 1.1, 1.2, 1.3, 1.4, 1.5, 1.5]:
                probe('for(const type of ["keydown","keyup"])document.body.dispatchEvent(new KeyboardEvent(type,{key:"=",code:"Equal",ctrlKey:true,bubbles:true}));return {}')
                time.sleep(.2)
                measure((round(800 * factor), round(560 * factor)))
            main_label = uuid.uuid4().hex
            main_before = gui.ctl(f'window-frame-state {main_label} main', f'window-frame-state-{main_label}')['detail']
            result['textMeasurements'] = []
            for key, code, scale in [('_', 'Minus', .9), ('_', 'Minus', .8), ('_', 'Minus', .8),
                                     ('+', 'Equal', .9), ('+', 'Equal', 1), ('+', 'Equal', 1.1),
                                     ('+', 'Equal', 1.25), ('+', 'Equal', 1.5), ('+', 'Equal', 1.5)]:
                probe('for(const type of ["keydown","keyup"])document.body.dispatchEvent(new KeyboardEvent(type,{key:' + json.dumps(key) + ',code:' + json.dumps(code) + ',ctrlKey:true,shiftKey:true,bubbles:true}));return {}')
                time.sleep(.3)
                label = uuid.uuid4().hex
                frame = gui.ctl(f'window-frame-state {label} quick-panel', f'window-frame-state-{label}')['detail']
                dom = probe('return {scale:localStorage.getItem("uniclipboard.uiScale"),dpr:devicePixelRatio}')
                ok = abs(frame.get('zoom', 0) - scale) < .001 and abs(float(dom['scale']) - scale) < .001 and (frame['width'], frame['height']) == (1200, 840)
                result['textMeasurements'].append({'target': scale, 'native': frame, 'dom': dom, 'ok': ok})
            main_label = uuid.uuid4().hex
            main_after = gui.ctl(f'window-frame-state {main_label} main', f'window-frame-state-{main_label}')['detail']
            result['sharedUiScale'] = {'before': main_before, 'after': main_after, 'ok': not main_after.get('exists') or (abs(main_after.get('zoom', 0) - 1.5) < .001 and (main_before.get('width'), main_before.get('height')) == (main_after.get('width'), main_after.get('height')))}
            protocol = (out / 'gui.log').read_text()
            expected = {tuple(row['target']) for row in result['measurements']}
            result['fixedWindowConstraints'] = fixed_window_constraints(protocol, expected)
            result['passed'] = all(row['ok'] for row in result['measurements']) and result['fixedWindowConstraints']['ok'] and all(row['ok'] for row in result['textMeasurements']) and result['sharedUiScale']['ok']
        finally:
            if gui is not None and gui.proc.poll() is None:
                gui.proc.terminate()
                gui.proc.wait(timeout=20)
            subprocess.run([str(sandbox / 'uniclip'), 'stop'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=90)
            server.shutdown()
            (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    assert result['passed'], 'GTK4 sizes or Wayland fixed-size constraints disagree with the requested window scale'


if __name__ == '__main__':
    main()
