#!/usr/bin/env python3
"""Loopback guard WITHOUT / WITH a downstream resolver (slice 17c12): what the guard does when the resolver GLib would have picked is missing.

  linux_appimage_guard_downstream_run.py --out DIR --appimage X.AppImage

The package is EXTRACTED (--appimage-extract) and AppRun is started from the extracted tree (the same files, one directory of GIO modules edited per variant; recorded with hashes):
  no-downstream      libgiognomeproxy.so and libgiolibproxy.so removed: the guard has nothing behind it. Env proxy variables and GNOME settings both name a real proxy; neither can be
                     honoured, so the guard answers direct:// (the same as GLib without any proxy resolver, which is what the old Tauri AppImage always did). Required: the WebView's external
                     request is DIRECT (not failed, not proxied), loopback works (fresh authenticated HTTP + WebSocket), the proxy saw nothing.
  libproxy-only      libgiognomeproxy.so removed, libgiolibproxy.so kept: the guard delegates to libproxy, so the env proxy IS honoured (proxied) and loopback stays direct.
The APPDIR-based PAC supervisor is not exercised here (AppRun started from the extracted tree does not receive the runtime's APPDIR); that is covered by the PAC scenarios.
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
from linux_appimage_tls_run import Reports, StopScenario, install_trust, run_cmd, wait_new_daemon  # noqa: E402






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
    sandbox = Path(tempfile.mkdtemp(prefix='uc-guard-'))
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
    r.update({'mode': 'guard-downstream', 'user': USER, 'kernelMachine': os.uname().machine, 'appimageSha256': base.sha256(app),
              'scope': 'container (--internal network), Xvfb, non-portable real HOME, extracted tree started through AppRun; no desktop, GPU, Wayland, native amd64'})
    r['requirements'] = []

    def req(name, ok, detail=None):
        r['requirements'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('REQ-PASS ' if ok else 'REQ-FAIL ') + name, flush=True)

    xvfb = start_xvfb(out)
    launches, servers, proxies, bus = [], [], [], None
    r['variants'] = {}
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
        ex = as_user([str(app), '--appimage-extract'], env, timeout=300, cwd=str(install))
        tree = install / 'squashfs-root'
        run.check('T1 the AppImage is extracted for the module edit', ex.returncode == 0 and (tree / 'AppRun').exists(), ex.stderr[-200:])
        moddir = tree / 'usr/lib/gio/modules'
        r['originalModules'] = {p.name: base.sha256(p) for p in sorted(moddir.glob('*.so'))}
        old_pid = None
        for variant, removed, expected in (('no-downstream', ['libgiognomeproxy.so', 'libgiolibproxy.so'], 'direct'), ('libproxy-only', ['libgiognomeproxy.so'], 'proxied')):
            sc = r['variants'][variant] = {'removed': removed, 'expected': expected}
            for held in moddir.glob('*.removed-for-test'):  # every variant starts from the COMPLETE shipped module set
                held.rename(held.with_name(held.name[:-len('.removed-for-test')]))
            for f in removed:
                try:
                    (moddir / f).rename(moddir / (f + '.removed-for-test'))
                except FileNotFoundError:
                    pass
            sc['modulesPresent'] = sorted(p.name for p in moddir.glob('*.so'))
            proxy = base.Proxy(variant, 'allow', None)
            proxies.append(proxy)
            base.write_dconf_proxy(proxy.port, 'user', app, True)  # GNOME settings name the proxy as well: it can only be honoured by the GNOME resolver, which is gone
            penv = {k: f'http://127.0.0.1:{proxy.port}' for k in ('http_proxy', 'https_proxy', 'HTTP_PROXY', 'HTTPS_PROXY')}
            penv['XDG_CURRENT_DESKTOP'] = 'GNOME'
            gui = run.launch(f'gui-{variant}', extra_env=penv, appimage=tree / 'AppRun')
            launches.append(gui)
            conn_path, conn = wait_new_daemon(home, old_pid) if old_pid else wait_daemon(home)
            run.check(f'[{variant}] the real bundled daemon started', conn is not None, str(conn_path))
            if conn is None:
                raise StopScenario()
            old_pid = conn['pid']
            gui.step('bootstrapped', 90)
            state = wait_panel_ready(gui, variant, 90)
            run.check(f'[{variant}] the real WebView loaded the frontend', state.get('panelReady') is True, state)
            time.sleep(3)
            pfx = f'pp-{variant}-'
            gui.ctl(f'panel-js pagepp-{variant} {base.page_probe_script(pfx, reports.port, f"http://{conn[chr(104)+chr(111)+chr(115)+chr(116)]}:{conn[chr(112)+chr(111)+chr(114)+chr(116)]}", conn["token"], gui.proc.pid)}', f'panel-js-pagepp-{variant}')
            got = {k: reports.wait(pfx + k, 25) for k in ('connect', 'http', 'wsopen', 'wsframe')}
            try:
                http_ok = json.loads((got['http'] or {}).get('value') or '{}').get('status') == 200
            except ValueError:
                http_ok = False
            sc['pageProbe'] = {k: (v or {}).get('value') for k, v in got.items()}
            req(f'[{variant}] REQUIRE loopback works: the WebView completes a fresh session exchange, an authenticated GET /settings and a WebSocket event frame with the guard alone / delegating', http_ok and bool(got['wsopen']) and bool(got['wsframe']), sc['pageProbe'])
            nonce = secrets.token_hex(6)
            marks = len(proxy.lines())
            gui.ctl(f'panel-js ext-{variant} {reports.script(f"ext-{variant}", f"https://{base.WV_HOST}/webview-{nonce}")}', f'panel-js-ext-{variant}')
            reports.wait(f'ext-{variant}-ok', 40) or reports.wait(f'ext-{variant}-err', 25)
            time.sleep(1)
            sc['webview'] = base.classify(base.WV_HOST, nonce, proxy.lines()[marks:], target)
            req(f'[{variant}] REQUIRE the external WebView request is {expected} (observed {sc["webview"]["route"]})', sc['webview']['route'] == expected, sc['webview'])
            leaked = [l for l in proxy.lines() if base.LOOPBACK.search(l) and f':{conn["port"]}' in l]
            req(f'[{variant}] REQUIRE the proxy never saw the daemon\'s loopback port', not leaked, leaked[:3])
            mods = base.modules_loaded(next(iter({x["pid"] for x in base.sockets_of(("WebKit",))}), gui.proc.pid))
            sc['modulesLoaded'] = mods
            stop(gui, conn)
            launches.clear()
            base.clean_dconf(app)
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
        for f in out.glob('proxy-*-tinyproxy.log'):
            f.write_text(re.sub(r'(auth=|token=)[A-Za-z0-9._-]+', r'\1<redacted>', f.read_text(errors='replace')))
        print(json.dumps({'passed': r['passed'], 'functionalPassed': r['functionalPassed'], 'mode': r['mode']}), flush=True)
        sys.exit(0 if r['functionalPassed'] else 3)


if __name__ == '__main__':
    main()
