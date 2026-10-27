#!/usr/bin/env python3
"""Real host/daemon on private Xvfb and D-Bus: pacman refuses artifact downloads.

Run under dbus-run-session. A private mount and disposable pacman database model an installed package;
package files and the physical desktop are never changed.
WebKit sandboxing is disabled for this synthetic Xvfb test only.
"""
import argparse
import collections
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import urllib.request
import urllib.parse

from linux_xvfb_run import read_steps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('gui', 'cli', 'daemon', 'xvfb', 'out', 'fixture'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-update-', dir=out))
    home, runtime, temporary = sandbox / 'home', sandbox / 'run', sandbox / 'tmp'
    for path in (home, runtime, temporary):
        path.mkdir(mode=0o700)
    for source, name in ((args.gui, 'gui-go'), (args.cli, 'uniclip'), (args.daemon, 'uniclipd')):
        shutil.copy2(source.resolve(), sandbox / name)
    tools = sandbox / 'tools'
    tools.mkdir()
    wrapper = tools / 'pacman'
    wrapper.write_text('#!/bin/sh\nexec /usr/bin/pacman --dbpath "$UC_POLICY_DB" "$@"\n')
    wrapper.chmod(0o755)
    database = sandbox / 'pacman-db'
    package = database / 'local/uniclipboard-policy-1-1'
    package.mkdir(parents=True)
    (database / 'local/ALPM_DB_VERSION').write_bytes(Path('/var/lib/pacman/local/ALPM_DB_VERSION').read_bytes())
    (package / 'desc').write_text('%NAME%\nuniclipboard-policy\n\n%VERSION%\n1-1\n\n%ARCH%\naarch64\n')
    (package / 'files').write_text('%FILES%\nopt/uc-gui-go-policy/gui-go\n')
    requests = collections.Counter()
    mode = {'kind': 'pacman'}
    fixture = args.fixture.resolve()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            path = urllib.parse.urlparse(self.path).path
            requests[mode['kind'] + path] += 1
            if path == '/feed':
                payload = {'version': '99.0.0', 'notes': 'synthetic update',
                           'platforms': {'linux-aarch64': {'url': base + '/payload',
                           'signature': (fixture / 'good.sig.b64').read_text().strip()}}}
                data = json.dumps(payload).encode()
            elif path == '/payload':
                data = (fixture / 'payload').read_bytes()
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    base = f'http://127.0.0.1:{server.server_port}'
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = {k: v for k, v in os.environ.items() if not k.startswith(('UC_', 'UNICLIPBOARD', 'WAYLAND', 'HYPRLAND', 'AT_SPI', 'XDG_', 'APPIMAGE', 'APPDIR'))}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_DATA_HOME=str(home / '.local/share'),
               XDG_STATE_HOME=str(home / '.local/state'), XDG_CACHE_HOME=str(home / '.cache'), XDG_RUNTIME_DIR=str(runtime),
               TMPDIR=str(temporary), UC_POLICY_DB=str(database), PATH=str(tools) + os.pathsep + env['PATH'], UC_PORTABLE='1', UC_PROFILE='gui-go-' + sandbox.name, UNICLIPBOARD_ENV='development',
               UC_DISABLE_SYSTEM_CLIPBOARD='1', UC_GUI_GO_ISOLATED='1', UC_GPUI_QUICK_PANEL='0',
               UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_UPDATE_ENDPOINT=base + '/feed',
               UC_UPDATE_PUBKEY=(fixture / 'pubkey.b64').read_text().strip(), UC_UPDATE_SCHEDULER_INTERVAL='1s',
               UC_GUI_GO_E2E_NOTIFY_LOG=str(out / 'notifications.log'), WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS='1',
               GDK_BACKEND='x11', XDG_SESSION_TYPE='x11', GTK_A11Y='none', NO_AT_BRIDGE='1')
    readfd, writefd = os.pipe()
    xvfb = subprocess.Popen([str(args.xvfb), '-displayfd', str(writefd), '-screen', '0', '1280x800x24', '-nolisten', 'tcp'],
                            pass_fds=(writefd,), stdout=subprocess.DEVNULL, stderr=(out / 'xvfb.log').open('w'))
    os.close(writefd)
    with os.fdopen(readfd) as display:
        env['DISPLAY'] = ':' + display.readline().strip()
    result, proc = {'passed': False}, None
    evidence, control = out / 'native.jsonl', out / 'gui.control'
    env.update(UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control))

    def wait(probe, label, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = probe()
            if value:
                return value
            time.sleep(.1)
        raise AssertionError('timeout: ' + label)

    def invoke(label, command):
        with control.open('a') as file:
            file.write(f'invoke {label} {command} {{}}\n')
        return wait(lambda: next((r['detail'] for r in read_steps(evidence) if r['step'] == 'invoke-' + label), None), label)

    try:
        subprocess.run([str(sandbox / 'uniclip'), 'space', 'init', '--passphrase', 'gui-go-synthetic-passphrase', '--device-name', 'update-test'],
                       env=env, check=True, capture_output=True, timeout=90)
        subprocess.run([str(sandbox / 'uniclip'), 'start'], env=env, check=True, capture_output=True, timeout=90)
        conn = json.loads(next(sandbox.rglob('daemon.conn')).read_text())
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        endpoint = f"http://{conn['host']}:{conn['port']}"

        def request(path, body, auth):
            req = urllib.request.Request(endpoint + path, data=json.dumps(body).encode(),
                                         method='PUT' if path == '/settings' else 'POST',
                                         headers={'Authorization': auth, 'Content-Type': 'application/json'})
            with opener.open(req, timeout=15) as response:
                return json.load(response)

        token = request('/auth/connect', {'pid': os.getpid(), 'clientType': 'cli'}, 'Bearer ' + conn['token'])['data']['sessionToken']
        request('/settings', {'general': {'autoCheckUpdate': True, 'autoDownloadUpdate': True,
                'usageAnalyticsEnabled': False}}, 'Session ' + token)
        for kind in ('pacman', 'appimage'):
            mode['kind'] = kind
            evidence.write_text('')
            control.write_text('')
            case_env = dict(env)
            if kind == 'appimage':
                case_env['APPIMAGE'] = str(sandbox / 'gui-go')
            command = ['bwrap', '--die-with-parent', '--ro-bind', '/', '/', '--bind', str(out), str(out),
                       '--tmpfs', '/opt', '--bind', str(sandbox), '/opt/uc-gui-go-policy',
                       '--proc', '/proc', '--dev', '/dev', '/opt/uc-gui-go-policy/gui-go']
            with (out / (kind + '.log')).open('w') as log:
                proc = subprocess.Popen(command, env=case_env, stdout=log, stderr=subprocess.STDOUT)
            wait(lambda: any(r['step'] == 'bootstrapped' for r in read_steps(evidence)), 'bootstrap ' + kind)
            installed = invoke('kind-' + kind, 'get_install_kind')
            assert installed['ok'] and installed['data'] == kind, installed
            wait(lambda: requests[kind + '/feed'] >= 2, 'scheduled checks ' + kind)
            if kind == 'pacman':
                assert requests[kind + '/payload'] == 0, requests
                rejected = invoke('download-' + kind, 'download_update')
                assert not rejected['ok'], rejected
                refused_install = invoke('install-' + kind, 'install_update')
                assert not refused_install['ok'], refused_install
                assert requests[kind + '/payload'] == 0, requests
                assert not list(temporary.glob('uniclipboard-update-*'))
                result['pacmanDownloadRefused'] = rejected
                result['pacmanInstallRefused'] = refused_install
                with control.open('a') as file:
                    file.write('close-main memory\n')
                wait(lambda: any(r['step'] == 'close-main-memory' for r in read_steps(evidence)), 'close WebView')
                with control.open('a') as file:
                    file.write('state memory\n')
                state = wait(lambda: next((r['detail'] for r in read_steps(evidence)
                             if r['step'] == 'control-state' and r['detail'].get('label') == 'memory'), None), 'memory state')
                assert state['uiPid'] == 0 and state['trayPresent'], state
                for line in Path(f"/proc/{state['pid']}/smaps_rollup").read_text().splitlines():
                    if line.startswith('Pss:'):
                        result['pacmanTrayPssKiB'] = int(line.split()[1])
            else:
                ready = wait(lambda: (snapshot if (snapshot := invoke('progress-' + str(time.monotonic_ns()), 'get_download_progress'))['data']['phase'] == 'ready' else None), 'verified download')
                assert ready['ok'] and requests[kind + '/payload'] == 1, ready
                assert len(list(temporary.glob('uniclipboard-update-*'))) == 1
                result['selfUpdateReady'] = ready
            # Stop only the process handle started by this driver; keep the
            # synthetic daemon for the next case and verify exit cleanup.
            with control.open('a') as file:
                file.write('exit\n')
            proc.wait(timeout=30)
            proc = None
            wait(lambda: not list(temporary.glob('uniclipboard-update-*')), 'shutdown file cleanup')
        result.update(passed=True, requests=dict(requests), shutdownCleaned=True)
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=30)
        subprocess.run([str(sandbox / 'uniclip'), 'stop'], env=env, capture_output=True, timeout=80)
        xvfb.terminate()
        xvfb.wait(timeout=10)
        server.shutdown()
        server.server_close()
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
