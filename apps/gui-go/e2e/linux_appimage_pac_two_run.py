#!/usr/bin/env python3
"""Two GUIs on ONE session bus (slice 17c12, PAC helper supervision): dedicated driver that reuses the components of linux_appimage_proxy_run.py.

  linux_appimage_pac_two_run.py --out DIR --appimage X.AppImage [--kill]

Container, non-portable first GUI (real HOME, user session bus + Secret Service), a second GUI that is a PORTABLE copy of the same AppImage (its own HOME and
data root, hence its own daemon), both pointed at the SAME session bus. GNOME settings (system dconf database) select a PAC script; the host PAC service is made
unavailable (binary and service file renamed, restored in the finally block), so the bundled helper is the only provider. Sequence:
  1. both GUIs are launched back to back (cold-start race for the bus name), both daemons and WebViews come up;
  2. steady state: exactly ONE bundled glib-pacrunner exists, and BOTH WebViews get PAC-proxied (external target, own hostname per request, proxy log window);
  3. the GUI that owns the helper (the helper's parent) exits: normally (default) or with SIGKILL (--kill); its helper must be gone and the surviving GUI's
     supervisor must start its own (new pid, parent = the survivor); the survivor's WebView must still get PAC;
  4. the survivor is SIGKILLed: no bundled helper is left.
Results: DIR/appimage-assertions.json (+ run.log from the caller); exit 0 only when every check passed.
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


def helpers():
    """Bundled glib-pacrunner processes (executables below any AppImage mount)."""
    return {pid: exe for pid, (exe, comm) in procs().items() if comm == 'glib-pacrunner'}


def ppid_of(pid):
    try:
        return int(re.search(r'PPid:\s+(\d+)', Path(f'/proc/{pid}/status').read_text()).group(1))
    except (OSError, AttributeError):
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--manifest', type=Path)
    ap.add_argument('--owned', action='store_true', help='ONE portable GUI on the known bus with a pre-existing non-activatable host owner; the owner is killed and the bundled helper must take over')
    ap.add_argument('--kill', action='store_true', help='the owner GUI is SIGKILLed instead of exiting normally')
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    home = Path(account.pw_dir)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-two-'))
    os.chown(sandbox, account.pw_uid, account.pw_gid)
    runtime_dir, install = sandbox / 'run', sandbox / 'install'
    for d in (runtime_dir, install, base.BUS_DIR):
        d.mkdir(exist_ok=True)
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    app_a, app_b = install / 'UniClipboard.AppImage', install / 'UniClipboard-B.AppImage'
    for a in (app_a, app_b):
        shutil.copy2(args.appimage, a)
        os.chown(a, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=account.pw_dir, XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', XDG_SESSION_TYPE='x11',
               UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE, USER=USER, LOGNAME=USER,
               DBUS_SESSION_BUS_ADDRESS=f'unix:path={base.BUS_DIR}/bus')
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME', 'UC_E2E_BUS', 'SSL_CERT_FILE', 'SSL_CERT_DIR',
                'GIO_USE_TLS', 'GIO_MODULE_DIR', 'GIO_EXTRA_MODULES', 'XDG_CURRENT_DESKTOP', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY'):
        env.pop(key, None)
    run = UserRun(out, app_a, env)
    r = run.results
    r.update({'mode': 'pac-owned-portable' if args.owned else 'pac-two-gui-' + ('sigkill-owner' if args.kill else 'normal-exit-owner'), 'user': USER, 'kernelMachine': os.uname().machine,
              'scope': 'container (--internal network), Xvfb; GUI A non-portable real HOME, GUI B portable copy, ONE shared user session bus; no desktop, GPU, Wayland, native amd64'})
    r['appimageSha256'] = base.sha256(app_a)
    r['requirements'] = []

    def req(name, ok, detail=None):
        r['requirements'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('REQ-PASS ' if ok else 'REQ-FAIL ') + name, flush=True)

    xvfb = start_xvfb(out)
    launches, servers, pacs, proxy, bus, owned = [], [], [], None, None, {}
    sc = r['scenario'] = {}
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
        run.check('T1 the shared user session bus and unlocked Secret Service are ready', (base.BUS_DIR / 'ready').exists())
        if not (base.BUS_DIR / 'ready').exists():
            raise StopScenario()
        (base.BUS_DIR / 'bus').chmod(0o777)
        made = as_user([str(app_b), '--appimage-portable-home'], env, timeout=60)
        run.check('T1 GUI B is a portable copy with its own HOME', made.returncode == 0 and Path(str(app_b) + '.home').is_dir(), made.stderr[-200:])
        # no host PAC service: the bundled helper is the only provider
        if base.HOST_PACRUNNER.exists() and not args.owned:  # --owned keeps the host binary: it is started below as the pre-existing owner (its service file is renamed: not activatable)
            base.HOST_PACRUNNER.rename(base.HOST_PACRUNNER.with_name('glib-pacrunner.off'))
        if base.HOST_PACSERVICE.exists():
            base.HOST_PACSERVICE.rename(base.HOST_PACSERVICE.with_name(base.HOST_PACSERVICE.name + '.off'))
        proxy = base.Proxy('two', 'allow', None)
        pac = base.PacServer(proxy.port)
        pacs.append(pac)
        sc['dconf'] = base.write_dconf_proxy(proxy.port, 'sys', app_a, True, None, f'http://127.0.0.1:{pac.port}/proxy.pac')  # system database: visible to both HOMEs
        penv = {'XDG_CURRENT_DESKTOP': 'GNOME'}
        if args.owned:
            owner = subprocess.Popen([str(base.HOST_PACRUNNER)], env=dict(env, PATH=os.environ['PATH']), user=USER, group=USER, extra_groups=[], stdout=(out / 'owned-pacrunner.log').open('w'), stderr=subprocess.STDOUT)
            time.sleep(2)
            sc['preExistingOwner'] = {'pid': owner.pid, 'exe': base.HOST_PACRUNNER.as_posix(), 'activatable': base.HOST_PACSERVICE.exists()}
            run.check('fixture: a host PAC helper owns the name before the GUI starts and is NOT activatable (service file renamed)', owner.poll() is None and not base.HOST_PACSERVICE.exists(), sc['preExistingOwner'])
            gui_b = run.launch('gui-b', extra_env=penv, appimage=app_b)  # PORTABLE GUI on the known bus (DBUS_SESSION_BUS_ADDRESS is in env)
            launches.append(gui_b)
            _, conn_b = wait_daemon(Path(str(app_b) + '.home'))
            run.check('the portable GUI\'s real daemon started', conn_b is not None)
            if conn_b is None:
                raise StopScenario()
            gui_b.step('bootstrapped', 90)
            state = wait_panel_ready(gui_b, 'b', 90)
            run.check('the portable GUI\'s real WebView loaded the frontend', state.get('panelReady') is True, state)
            time.sleep(4)

            def pac_gui_o(tag):
                nonce = secrets.token_hex(6)
                n0 = len(proxy.lines())
                gui_b.ctl(f'panel-js owned-{tag} {reports.script(f"ext-owned-{tag}", f"https://{base.WV_HOST}/webview-{tag}-{nonce}")}', f'panel-js-owned-{tag}')
                reports.wait(f'ext-owned-{tag}-ok', 40) or reports.wait(f'ext-owned-{tag}-err', 25)
                time.sleep(1)
                return base.classify(base.WV_HOST, nonce, proxy.lines()[n0:], target)
            sc['helpersBefore'] = {str(k): v for k, v in helpers().items()}
            req('REQUIRE the supervisor respects the existing owner: no AppImage-origin helper is started (only the pre-existing owner exists)', set(helpers()) == {owner.pid}, sc['helpersBefore'])
            sc['beforeKill'] = pac_gui_o('before')
            req('REQUIRE PAC works through the pre-existing owner (proxied)', sc['beforeKill']['route'] == 'proxied', sc['beforeKill'])
            owner.kill()
            owner.wait()
            dl = time.monotonic() + 15
            while not [k for k in helpers() if k != owner.pid] and time.monotonic() < dl:
                time.sleep(.3)
            after = {k: v for k, v in helpers().items() if k != owner.pid}
            sc['helpersAfterOwnerKill'] = {str(k): {'exe': v, 'ppid': ppid_of(k)} for k, v in after.items()}
            req('REQUIRE after the owner is killed the GUI\'s supervisor takes over with its own helper (parent = the GUI)', len(after) == 1 and ppid_of(next(iter(after))) == gui_b.proc.pid, sc['helpersAfterOwnerKill'])
            sc['afterTakeover'] = pac_gui_o('after')
            req('REQUIRE PAC works after the takeover (proxied)', sc['afterTakeover']['route'] == 'proxied', sc['afterTakeover'])
            sc['exitCode'] = stop(gui_b, conn_b)
            dl = time.monotonic() + 10
            while helpers() and time.monotonic() < dl:
                time.sleep(.3)
            sc['helpersAfterNormalExit'] = {str(k): v for k, v in helpers().items()}
            req('REQUIRE after the GUI\'s normal exit no AppImage-origin helper is left', not [k for k, v in helpers().items() if '/tmp/.mount_' in v], sc['helpersAfterNormalExit'])
            sc['completed'] = True
            raise StopScenario()
        t0 = time.monotonic()
        gui_a = run.launch('gui-a', extra_env=penv)
        launches.append(gui_a)
        gui_b = run.launch('gui-b', extra_env=penv, appimage=app_b)  # cold-start race: no wait between the two launches
        launches.append(gui_b)
        sc['secondLaunchAfterSeconds'] = round(time.monotonic() - t0, 3)
        _, conn_a = wait_daemon(home)
        _, conn_b = wait_daemon(Path(str(app_b) + '.home'))
        run.check('the two GUIs each started their own real daemon (different data roots)', bool(conn_a and conn_b) and conn_a['pid'] != conn_b['pid'], [conn_a and conn_a['pid'], conn_b and conn_b['pid']])
        if not (conn_a and conn_b):
            raise StopScenario()
        for tag, g in (('a', gui_a), ('b', gui_b)):
            g.step('bootstrapped', 90)
            state = wait_panel_ready(g, tag, 90)
            run.check(f'GUI {tag.upper()}: the real WebView loaded the frontend', state.get('panelReady') is True, state)
        time.sleep(4)  # a losing helper quits on name loss; the supervisors settle

        def pac_gui(g, tag):
            nonce = secrets.token_hex(6)
            n0 = len(proxy.lines())
            g.ctl(f'panel-js two-{tag} {reports.script(f"ext-two-{tag}", f"https://{base.WV_HOST}/webview-{tag}-{nonce}")}', f'panel-js-two-{tag}')
            reports.wait(f'ext-two-{tag}-ok', 40) or reports.wait(f'ext-two-{tag}-err', 25)
            time.sleep(1)
            return base.classify(base.WV_HOST, nonce, proxy.lines()[n0:], target)

        steady = helpers()
        sc['helpersSteadyState'] = {str(k): {'ppid': ppid_of(k), 'exe': v} for k, v in steady.items()}
        req('REQUIRE exactly one bundled glib-pacrunner provides PAC for the two GUIs in steady state', len(steady) == 1, sc['helpersSteadyState'])
        sc['routes'] = {'A': pac_gui(gui_a, 'a'), 'B': pac_gui(gui_b, 'b')}
        req('REQUIRE both GUIs\' WebViews are proxied through PAC', all(v['route'] == 'proxied' for v in sc['routes'].values()), sc['routes'])
        guis = {gui_a.proc.pid: 'A', gui_b.proc.pid: 'B'}
        owner = guis.get(next((ppid_of(k) for k in steady), None))
        sc['owner'], sc['guiPids'] = owner, guis
        req('REQUIRE the helper\'s parent is one of the two GUIs', owner is not None, {'guis': guis, 'steady': sc['helpersSteadyState']})
        if owner is None:
            raise StopScenario()
        (og, oc, sg, scn) = (gui_a, conn_a, gui_b, conn_b) if owner == 'A' else (gui_b, conn_b, gui_a, conn_a)
        old = set(steady)
        if args.kill:
            os.kill(og.proc.pid, 9)
            og.proc.wait()
        else:
            sc['ownerExitCode'] = stop(og, oc)
        dl = time.monotonic() + 15
        while (not helpers() or set(helpers()) & old) and time.monotonic() < dl:
            time.sleep(.3)
        after = helpers()
        sc['helpersAfterOwnerExit'] = {str(k): {'ppid': ppid_of(k)} for k in after}
        req('REQUIRE the owner\'s helper is gone and the survivor\'s supervisor started its own (one helper, new pid, parent = the survivor)',
            len(after) == 1 and not set(after) & old and ppid_of(next(iter(after))) == sg.proc.pid, sc['helpersAfterOwnerExit'])
        sc['survivorRoute'] = pac_gui(sg, 'survivor')
        req('REQUIRE the surviving GUI\'s WebView still gets PAC (proxied)', sc['survivorRoute']['route'] == 'proxied', sc['survivorRoute'])
        sc['pacFetches'] = len(pac.fetches)
        os.kill(sg.proc.pid, 9)
        sg.proc.wait()
        dl = time.monotonic() + 10
        while helpers() and time.monotonic() < dl:
            time.sleep(.3)
        sc['helpersAfterSurvivorSigkill'] = sorted(helpers())
        req('REQUIRE after SIGKILL of the surviving GUI no bundled helper is left', not helpers(), sc['helpersAfterSurvivorSigkill'])
        sc['completed'] = True
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
        for c in list(home.rglob('daemon.conn')) + list(Path(str(app_b) + '.home').rglob('daemon.conn')):
            try:
                os.kill(json.loads(c.read_text())['pid'], 15)
            except (OSError, ValueError, KeyError):
                pass
        if bus:
            bus.terminate()
        off = base.HOST_PACRUNNER.with_name('glib-pacrunner.off')
        if off.exists():
            off.rename(base.HOST_PACRUNNER)
        svc = base.HOST_PACSERVICE.with_name(base.HOST_PACSERVICE.name + '.off')
        if svc.exists():
            svc.rename(base.HOST_PACSERVICE)
        for p in pacs:
            p.stop()
        if proxy:
            proxy.stop()
            for f in ('tinyproxy.log', 'tinyproxy.conf', 'stdout.log'):
                try:
                    shutil.copy2(proxy.dir / f, out / f'proxy-two-{f}')
                except OSError:
                    pass
        base.clean_dconf(app_a)
        time.sleep(1)
        xvfb.terminate()
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        r['functionalPassed'] = r['passed'] and sc.get('completed') is True and bool(r['requirements']) and all(q['ok'] for q in r['requirements'])
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
        for f in out.glob('proxy-*-tinyproxy.log'):  # a loopback URL that reached a proxy carries a session token: never keep it
            f.write_text(re.sub(r'(auth=|token=)[A-Za-z0-9._-]+', r'\1<redacted>', f.read_text(errors='replace')))
        print(json.dumps({'passed': r['passed'], 'functionalPassed': r['functionalPassed'], 'mode': r['mode']}), flush=True)
        sys.exit(0 if r['functionalPassed'] else 3)


if __name__ == '__main__':
    main()
