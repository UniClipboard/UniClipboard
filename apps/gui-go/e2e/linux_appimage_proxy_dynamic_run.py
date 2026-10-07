#!/usr/bin/env python3
"""Dynamic GNOME proxy settings (slice 17c12): ONE running GUI, the settings change underneath it (non-portable: real HOME, user session bus with the dconf service).

  linux_appimage_proxy_dynamic_run.py --out DIR --appimage X.AppImage

The real WebView requests an external controlled target after every `gsettings set` made through the session bus, with no GUI restart:
  none -> manual P1 (proxied via P1) -> manual P2 (proxied via P2, P1 silent) -> none (direct) -> manual deny proxy (refused) -> manual P1 with the target host in
  ignore-hosts (direct) -> ignore-hosts back to the default (proxied via P1).
Each state is retried for up to 12 s (GSettings notifications reach the network process lazily); the first attempt and the number of seconds until the expected
route are recorded. A state that never reaches its expected route fails. Reuses the components of linux_appimage_proxy_run.py.
"""
import argparse
import json
import os
import pwd
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import linux_appimage_proxy_run as base  # noqa: E402
from linux_appimage_portable_run import UserRun, USER, as_user, stop, wait_daemon  # noqa: E402
from linux_appimage_run import DISPLAY, PASSPHRASE, pid_alive, procs, start_xvfb, wait_panel_ready  # noqa: E402
from linux_appimage_tls_run import Reports, StopScenario, install_trust, run_cmd  # noqa: E402




SCHEMA = 'org.gnome.system.proxy'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--manifest', type=Path)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    home = Path(account.pw_dir)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-dyn-'))
    os.chown(sandbox, account.pw_uid, account.pw_gid)
    runtime_dir, install = sandbox / 'run', sandbox / 'install'
    for d in (runtime_dir, install, base.BUS_DIR):
        d.mkdir(exist_ok=True)
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    app = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    os.chown(app, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=account.pw_dir, XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', XDG_SESSION_TYPE='x11',
               UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE, USER=USER, LOGNAME=USER,
               DBUS_SESSION_BUS_ADDRESS=f'unix:path={base.BUS_DIR}/bus')
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME', 'UC_E2E_BUS', 'SSL_CERT_FILE', 'SSL_CERT_DIR',
                'GIO_USE_TLS', 'GIO_MODULE_DIR', 'GIO_EXTRA_MODULES', 'XDG_CURRENT_DESKTOP', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY'):
        env.pop(key, None)
    run = UserRun(out, app, env)
    r = run.results
    r.update({'mode': 'dynamic-gnome-settings', 'user': USER, 'kernelMachine': os.uname().machine,
              'scope': 'container (--internal network), Xvfb, non-portable real HOME, user session bus + dconf service; no desktop, GPU, Wayland, native amd64'})
    r['requirements'] = []

    def req(name, ok, detail=None):
        r['requirements'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('REQ-PASS ' if ok else 'REQ-FAIL ') + name, flush=True)

    xvfb = start_xvfb(out)
    launches, servers, proxies, bus = [], [], [], None
    sc = r['states'] = []
    done = False
    try:
        subprocess.run('[ -n "$(ip route show default)" ] || ip route add default dev eth0', shell=True)
        addr = run_cmd(['sh', '-c', "ip -4 -o addr show eth0 | awk '{print $4}' | cut -d/ -f1"]).stdout.strip()
        with open('/etc/hosts', 'a') as f:
            f.write(''.join(f'{addr} {h}\n' for h in base.HOSTS))
        material = base.make_target_cert(sandbox)
        trust = install_trust(material['ca'], 'proxy-target')
        run.check('T0 the target CA is installed through the container\'s own trust mechanism', trust['rc'] == 0, trust)
        target = base.Target(material, addr)
        servers.append(target)
        reports = Reports()
        bus = subprocess.Popen(['sh', '/work/apps/gui-go/e2e/linux/keyring_service.sh'], env=dict(env, PATH=os.environ['PATH']), user=USER, group=USER, extra_groups=[],
                               stdout=(out / 'keyring-service.log').open('w'), stderr=subprocess.STDOUT)
        for _ in range(150):
            if (base.BUS_DIR / 'ready').exists():
                break
            time.sleep(.2)
        run.check('T1 user session bus and unlocked Secret Service are ready', (base.BUS_DIR / 'ready').exists())
        if not (base.BUS_DIR / 'ready').exists():
            raise StopScenario()
        (base.BUS_DIR / 'bus').chmod(0o777)
        p1, p2, deny = base.Proxy('dyn1', 'allow', None), base.Proxy('dyn2', 'allow', None), base.Proxy('dyn-deny', 'deny', None)
        proxies.extend([p1, p2, deny])

        genv = dict(env, XDG_CURRENT_DESKTOP='GNOME')

        def gs(key_path, value):
            schema, key = (SCHEMA + '.' + key_path.rsplit('.', 1)[0], key_path.rsplit('.', 1)[1]) if '.' in key_path else (SCHEMA, key_path)
            c = as_user(['gsettings', 'set', schema, key, value], genv, timeout=30)
            if c.returncode != 0:
                raise RuntimeError(f'gsettings set {schema} {key} {value}: {c.stderr[-200:]}')

        def manual(port, ignore=None):
            for scheme in ('http', 'https'):
                gs(f'{scheme}.host', "'127.0.0.1'")
                gs(f'{scheme}.port', str(port))
            gs('ignore-hosts', ignore or "['localhost', '127.0.0.0/8', '::1']")
            gs('mode', "'manual'")

        gui = run.launch('gui', extra_env={'XDG_CURRENT_DESKTOP': 'GNOME'})
        launches.append(gui)
        _, conn = wait_daemon(home)
        run.check('the real bundled daemon started', conn is not None)
        if conn is None:
            raise StopScenario()
        gui.step('bootstrapped', 90)
        state = wait_panel_ready(gui, 'dyn', 90)
        run.check('the real WebView loaded the frontend', state.get('panelReady') is True, state)
        time.sleep(3)

        def request(tag):
            nonce = secrets.token_hex(6)
            marks = [len(p.lines()) for p in proxies]
            gui.ctl(f'panel-js dyn-{tag} {reports.script(f"ext-dyn-{tag}", f"https://{base.WV_HOST}/webview-{tag}-{nonce}")}', f'panel-js-dyn-{tag}')
            reports.wait(f'ext-dyn-{tag}-ok', 40) or reports.wait(f'ext-dyn-{tag}-err', 25)
            time.sleep(1)
            windows = {p.tag: p.lines()[m:] for p, m in zip(proxies, marks)}
            named_by = [p for p, w in windows.items() if any(base.REQ.search(l) and base.WV_HOST in base.REQ.search(l).group(2) for l in w)]
            refused_by = [p for p, w in windows.items() if any('refused' in l.lower() and f'"{base.WV_HOST}"' in l for l in w)]
            c = base.classify(base.WV_HOST, nonce, [l for w in windows.values() for l in w], target)
            c['via'] = named_by or refused_by
            return c

        def state_check(label, apply, expected, via):
            t0 = time.monotonic()
            apply()
            attempts = []
            for i in range(6):
                res = request(f'{label}-{i}')
                attempts.append({'route': res['route'], 'via': res['via'], 'seconds': round(time.monotonic() - t0, 1)})
                if res['route'] == expected and (via is None or res['via'] == [via]):
                    break
                time.sleep(2)
            ok = attempts[-1]['route'] == expected and (via is None or attempts[-1]['via'] == [via])
            sc.append({'state': label, 'expected': expected, 'via': via, 'attempts': attempts})
            req(f'REQUIRE state {label}: the running GUI\'s next external request is {expected}' + (f' via {via}' if via else '') + f' (first attempt {attempts[0]["route"]}/{attempts[0]["via"]}, settled after {attempts[-1]["seconds"]}s, {len(attempts)} attempt(s))', ok, attempts)

        state_check('none-initial', lambda: gs('mode', "'none'"), 'direct', None)
        state_check('manual-p1', lambda: manual(p1.port), 'proxied', 'dyn1')
        state_check('manual-p2', lambda: manual(p2.port), 'proxied', 'dyn2')
        state_check('none-again', lambda: gs('mode', "'none'"), 'direct', None)
        state_check('manual-deny', lambda: manual(deny.port), 'refused', 'dyn-deny')
        state_check('ignore-target', lambda: manual(p1.port, f"['localhost', '127.0.0.0/8', '::1', '{base.WV_HOST}']"), 'direct', None)
        state_check('ignore-default', lambda: manual(p1.port), 'proxied', 'dyn1')
        socks_lines = [l for p in proxies for l in p.lines() if base.LOOPBACK.search(l) and f':{conn["port"]}' in l]
        req('REQUIRE no proxy ever saw the daemon\'s loopback port during all state changes', not socks_lines, socks_lines[:3])
        done = True
    except StopScenario:
        pass
    except Exception as e:  # noqa: BLE001
        import traceback
        r['error'], r['traceback'] = repr(e), traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for c in home.rglob('daemon.conn'):
            try:
                os.kill(json.loads(c.read_text())['pid'], 15)
            except (OSError, ValueError, KeyError):
                pass
        if bus:
            bus.terminate()
        for p in proxies:
            p.stop()
            for f in ('tinyproxy.log', 'tinyproxy.conf', 'stdout.log'):
                try:
                    shutil.copy2(p.dir / f, out / f'proxy-{p.tag}-{f}')
                except OSError:
                    pass
        time.sleep(1)
        xvfb.terminate()
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        r['functionalPassed'] = r['passed'] and done and bool(r['requirements']) and all(q['ok'] for q in r['requirements'])
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
        for f in out.glob('proxy-*-tinyproxy.log'):  # a loopback URL that reached a proxy carries a session token: never keep it
            f.write_text(re.sub(r'(auth=|token=)[A-Za-z0-9._-]+', r'\1<redacted>', f.read_text(errors='replace')))
        print(json.dumps({'passed': r['passed'], 'functionalPassed': r['functionalPassed'], 'mode': r['mode']}), flush=True)
        sys.exit(0 if r['functionalPassed'] else 3)


if __name__ == '__main__':
    main()
