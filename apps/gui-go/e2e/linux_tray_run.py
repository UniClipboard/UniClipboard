#!/usr/bin/env python3
"""Linux tray lifecycle / menu / action E2E (17c14). RUNS INSIDE the build container under Xvfb + a private D-Bus session.

  docker run ... uc-gui-go-linux-build:17c9-wm  (see linux/run_17c14.sh)

The real gui-go E2E binary and the real Rust daemon run in a throwaway portable sandbox. The StatusNotifierWatcher and
the dbusmenu reader are the observer in linux/tray_probe/sni_host.py (a tray HOST: the container has no desktop shell,
so this is NOT a native tray). Real peer B joins through the production rendezvous service (needs network).

Checks: the item registers; the root menu order; the device submenu shows the paired peer through the periodic refresh
(the path that logged Gtk-CRITICAL on Linux); a dbusmenu click on the peer item saves through the daemon and the check
state follows; the sync item click flips its label; no Gtk-CRITICAL from the menu code across >= 3 refresh periods; the
Quit item exits the GUI with 0, stops the daemon and removes the item.
"""
import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'linux' / 'tray_probe'))
from sni_host import SniHost, flat  # noqa: E402
from linux_xvfb_run import Gui, pid_alive, PASSPHRASE, read_steps  # noqa: E402

ROOT_ORDER = ['Device Sync', '-', 'Open', 'Settings', 'Check for Updates…', '-', 'Restart', 'Lightweight Mode (Background Sync)', 'Quit']
MENU_CRITICAL = re.compile(r'(gtk_container_foreach|gtk_menu_shell_insert|gtk_menu_item_set_submenu|gtk_menu_|GtkMenu|GTK_IS_MENU)')


def run_cli(binary, env, *args, timeout=120, check=True):
    p = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=timeout)
    if check and p.returncode != 0:
        raise RuntimeError(f'uniclip {" ".join(args)} failed: {p.returncode} {p.stderr[:300]}')
    return p


def daemon_get(binary, env, path):
    """The daemon's own answer (JSON) for an enveloped GET, read with the GUI's client; None when it cannot be read."""
    p = subprocess.run([str(binary), path], env=env, capture_output=True, text=True, timeout=60)
    try:
        return json.loads(p.stdout) if p.returncode == 0 else None
    except ValueError:
        return None


def wait_daemon(binary, env, path, predicate, timeout=40):
    deadline, last = time.time() + timeout, None
    while time.time() < deadline:
        last = daemon_get(binary, env, path)
        if last is not None and predicate(last):
            return last
        time.sleep(1)
    return last


def labels(layout):
    return [('-' if n['type'] == 'separator' else n['label']) for n in layout['children']]


def submenu(layout, name):
    return [n for n in layout['children'] if n['label'] == name][0]['children']


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--binaries', type=Path, required=True)
    ap.add_argument('--tag', default='tray')
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
    peer = Path(tempfile.mkdtemp(prefix='uc-gui-go-peer-'))
    profile, pprofile = 'gui-go-' + sandbox.name, 'gui-go-' + peer.name
    for name in ('gui-go', 'uniclipd', 'uniclip', 'daemonget'):
        shutil.copy2(args.binaries / name, sandbox / name)
        shutil.copy2(args.binaries / name, peer / name)
    envs = []
    for base, prof in ((sandbox, profile), (peer, pprofile)):
        home, rt = base / 'home', base / 'run'
        home.mkdir(mode=0o700)
        rt.mkdir(mode=0o700)
        env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), UC_PORTABLE='1', UC_PROFILE=prof,
                   UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', GDK_BACKEND=os.environ.get('GDK_BACKEND', 'x11'))
        env['XDG_RUNTIME_DIR'] = str(rt)
        for k in ('HYPRLAND_INSTANCE_SIGNATURE', 'APPIMAGE'):
            env.pop(k, None)
        envs.append(env)
    env_a, env_b = envs
    # The peer's daemon and CLI live in its own runtime dir but share the private session bus; the GUI only needs A.
    cli_a, cli_b = sandbox / 'uniclip', peer / 'uniclip'
    dget = sandbox / 'daemonget'
    results = {'tag': args.tag, 'sandbox': str(sandbox), 'checks': [], 'passed': False,
               'scope': 'Xvfb + private D-Bus session in a container; tray HOST is the sni_host.py observer, not a desktop shell'}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)
        return bool(ok)

    host = SniHost(str(out / 'host.jsonl'))
    gui = None
    daemon_pid = None
    try:
        run_cli(cli_a, env_a, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'tray-a')
        run_cli(cli_a, env_a, 'start')
        gui_env = dict(env_a, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_EXIT_MODE='full', UC_GPUI_QUICK_PANEL='0',
                       XDG_SESSION_TYPE=os.environ.get('XDG_SESSION_TYPE', 'x11'))
        gui = Gui(sandbox, gui_env, out, 'gui1')
        gui.step('bootstrapped', 120)
        conn = next(sandbox.rglob('daemon.conn'))
        daemon_pid = json.loads(conn.read_text())['pid']
        check('1 the tray item registers with the StatusNotifierWatcher', host.registered.wait(40), host.item)
        lay = host.wait(lambda l: len(l['children']) >= 9, 30, 'root menu')
        check('2 the root menu order and the localized labels reach the host through dbusmenu',
              lay is not None and labels(lay)[1:] == ROOT_ORDER and labels(lay)[0] in ('Enable Sync', 'Disable Sync'), labels(lay) if lay else None)
        sync0 = labels(lay)[0] if lay else None
        sub0 = [n['label'] for n in submenu(lay, 'Device Sync')] if lay else None
        check('3 before pairing the device submenu holds only a disabled placeholder', sub0 in (['No paired devices'], ['Devices unavailable']), sub0)

        # Pair peer B through the production rendezvous service; the periodic refresh must bring the row to the host.
        invite = subprocess.Popen([str(cli_a), 'space', 'invite'], env=env_a, stdout=subprocess.PIPE, text=True)
        code, deadline = None, time.time() + 90
        os.set_blocking(invite.stdout.fileno(), False)
        buf = ''
        while code is None and time.time() < deadline:
            try:
                buf += os.read(invite.stdout.fileno(), 4096).decode(errors='replace')
            except BlockingIOError:
                pass
            for line in buf.splitlines():
                if line.startswith('INVITATION_CODE='):
                    code = line.split('=', 1)[1].strip()
            time.sleep(.3)
        check('4 an invitation code was obtained from the production rendezvous service', bool(code))
        assert code, buf[:300]
        t_join = time.time()
        join = run_cli(cli_b, env_b, 'space', 'join', '--code', code, '--passphrase', PASSPHRASE, '--device-name', 'tray-peer-b', timeout=120, check=False)
        results['join'] = {'rc': join.returncode, 'stderr': join.stderr[-300:]}
        # Pairing is verified on the daemons themselves, independent of the tray: each side lists itself (is_local) and the other,
        # and the device ids cross-match (A's remote member id == B's local id and the reverse).
        roster = {'a': [], 'b': []}
        paired = False
        for _ in range(45):
            for k, (cli, env) in {'a': (cli_a, env_a), 'b': (cli_b, env_b)}.items():
                try:
                    roster[k] = json.loads(run_cli(cli, env, '--json', 'member', 'list', check=False).stdout or '[]')
                except ValueError:
                    roster[k] = []
            loc = {k: [m for m in v if m.get('is_local')] for k, v in roster.items()}
            rem = {k: [m for m in v if not m.get('is_local')] for k, v in roster.items()}
            paired = all(len(loc[k]) == 1 and len(rem[k]) == 1 for k in 'ab') and \
                loc['a'][0]['device_name'] == 'tray-a' and loc['b'][0]['device_name'] == 'tray-peer-b' and \
                rem['a'][0]['device_id'] == loc['b'][0]['device_id'] and rem['b'][0]['device_id'] == loc['a'][0]['device_id'] and \
                rem['a'][0]['device_name'] == 'tray-peer-b' and rem['b'][0]['device_name'] == 'tray-a'
            if paired:
                break
            time.sleep(2)
        invite.send_signal(signal.SIGINT)
        results['roster'] = roster
        check('4b A and B are paired with each other on their own daemons (member list --json: names, is_local, device ids cross-match)', paired,
              {'a': roster['a'], 'b': roster['b'], 'join': results['join']})
        if not paired:
            results['inconclusive'] = 'pairing did not complete; the device checks below are not attributable to the tray'
            raise RuntimeError(results['inconclusive'])
        peer_id = [m for m in roster['a'] if not m.get('is_local')][0]['device_id']
        prefs_path = '/member/' + peer_id + '/sync-preferences'
        sync_path = '/settings'
        prefs0 = daemon_get(dget, env_a, prefs_path)
        settings0 = daemon_get(dget, env_a, sync_path)
        results['daemonBefore'] = {'prefs': prefs0, 'syncEnabled': ((settings0 or {}).get('sync') or {}).get('syncEnabled')}
        lay = host.wait(lambda l: [n['label'] for n in submenu(l, 'Device Sync')] == ['tray-peer-b'], 60, 'peer row')
        row = submenu(lay, 'Device Sync')[0] if lay else None
        check('5 the periodic refresh publishes the paired peer into the device submenu seen by the host (checked, enabled)',
              bool(row) and row['toggle'] == 1 and row['enabled'] is True, {'row': row, 'seconds_after_join': round(time.time() - t_join, 1)})

        check('5b the daemon\'s own state before the click: send and receive are on for the peer', bool(prefs0) and prefs0.get('sendEnabled') is True and prefs0.get('receiveEnabled') is True, prefs0)
        host.click('tray-peer-b')
        d_off = wait_daemon(dget, env_a, prefs_path, lambda p: p.get('sendEnabled') is False and p.get('receiveEnabled') is False)
        check('6 after the dbusmenu click the DAEMON reports send=false and receive=false for that peer (authoritative read, not the menu)',
              bool(d_off) and d_off.get('sendEnabled') is False and d_off.get('receiveEnabled') is False, d_off)
        lay = host.wait(lambda l: (submenu(l, 'Device Sync') or [{}])[0].get('toggle') == 0 and submenu(l, 'Device Sync')[0]['enabled'], 40, 'peer off')
        check('6 and the menu follows: unchecked and enabled again', lay is not None, submenu(lay, 'Device Sync') if lay else None)
        host.click('tray-peer-b')
        d_on = wait_daemon(dget, env_a, prefs_path, lambda p: p.get('sendEnabled') is True and p.get('receiveEnabled') is True)
        check('6 the second click restores send=true and receive=true in the daemon', bool(d_on) and d_on.get('sendEnabled') is True and d_on.get('receiveEnabled') is True, d_on)
        lay = host.wait(lambda l: (submenu(l, 'Device Sync') or [{}])[0].get('toggle') == 1 and submenu(l, 'Device Sync')[0]['enabled'], 40, 'peer on')
        check('6 and the menu shows it checked again', lay is not None, submenu(lay, 'Device Sync') if lay else None)

        flip = {'Enable Sync': 'Disable Sync', 'Disable Sync': 'Enable Sync'}
        en0 = ((settings0 or {}).get('sync') or {}).get('syncEnabled')
        check('7a the daemon\'s global sync switch agrees with the initial label', en0 is not None and sync0 == ('Disable Sync' if en0 else 'Enable Sync'), {'syncEnabled': en0, 'label': sync0})
        host.click(sync0)
        s1 = wait_daemon(dget, env_a, sync_path, lambda x: ((x.get('sync') or {}).get('syncEnabled')) is (not en0))
        check('7 the sync item click flips syncEnabled in the DAEMON (authoritative read)', ((s1 or {}).get('sync') or {}).get('syncEnabled') is (not en0), ((s1 or {}).get('sync') or {}))
        lay = host.wait(lambda l: labels(l)[0] == flip[sync0], 30, 'sync flips')
        check('7 and the label follows', lay is not None, labels(lay)[0] if lay else None)
        host.click(flip[sync0])
        s2 = wait_daemon(dget, env_a, sync_path, lambda x: ((x.get('sync') or {}).get('syncEnabled')) is en0)
        check('7 the second click restores syncEnabled in the daemon', ((s2 or {}).get('sync') or {}).get('syncEnabled') is en0, ((s2 or {}).get('sync') or {}))
        lay = host.wait(lambda l: labels(l)[0] == sync0, 30, 'sync restores')
        check('7 and the label returns', lay is not None, labels(lay)[0] if lay else None)

        ZH = ['设备同步', '-', '打开', '设置', '检查更新…', '-', '重启', '轻量模式（后台同步）', '退出']
        def lang_calls():
            return [r['detail'] for r in read_steps(gui.evidence) if r['step'] == 'tray-language-call']
        # The frontend's own settings effect also calls set_tray_language (once, when its settings load); our change must come after it.
        t_wait = time.time()
        while not lang_calls() and time.time() - t_wait < 60:
            time.sleep(.5)
        results['languageCallsBefore'] = lang_calls()
        check('7b precondition: the frontend\'s own initial tray-language call was seen before the test changes the language (ordering)', bool(lang_calls()), lang_calls())
        r = gui.invoke('lang-zh', 'set_tray_language', {'language': 'zh-CN'})
        lay = host.wait(lambda l: labels(l)[1:] == ZH, 30, 'zh labels')
        check('7b set_tray_language(zh-CN) relabels the whole menu in the host, including the device submenu title and keeping the peer row',
              r['ok'] and lay is not None and [n['label'] for n in submenu(lay, '设备同步')] == ['tray-peer-b'], [r, labels(lay) if lay else None])
        results['languageCallsAfterZh'] = lang_calls()
        check('7b no later call overwrote zh-CN (the last recorded tray-language call is the test\'s)', lang_calls()[-1:] == ['zh-CN'], lang_calls())
        r = gui.invoke('lang-en', 'set_tray_language', {'language': 'en'})
        lay = host.wait(lambda l: labels(l)[1:] == ROOT_ORDER, 30, 'en labels')
        check('7b and back to English', r['ok'] and lay is not None, labels(lay) if lay else None)

        time.sleep(max(0, 35 - (time.time() - host.t0 - 10)))  # make sure >= 3 full refresh periods elapsed since the tray existed
        log = (out / 'gui1.log').read_text(errors='replace')
        crit = [l for l in log.splitlines() if 'CRITICAL' in l]
        menu_crit = [l for l in crit if MENU_CRITICAL.search(l)]
        results['criticalLines'] = crit
        check('8 no menu-related Gtk-CRITICAL in the GUI log across the refresh periods', not menu_crit, menu_crit[:5])
        results['criticalOther'] = [l for l in crit if l not in menu_crit]

        t_quit = time.time()
        host.click('Quit')
        try:
            rc = gui.proc.wait(timeout=40)
        except subprocess.TimeoutExpired:
            rc = None
        check('9 the Quit item exits the GUI with 0', rc == 0, {'rc': rc, 'seconds': round(time.time() - t_quit, 1)})
        t_dead = time.time()
        while pid_alive(daemon_pid) and time.time() - t_dead < 30:
            time.sleep(.5)
        check('9 the daemon is stopped by the tray quit (full exit)', daemon_pid is not None and not pid_alive(daemon_pid),
              {'pid': daemon_pid, 'seconds_after_gui_exit': round(time.time() - t_dead, 1)})
        check('9 the tray item is gone from the host after exit', host.layout() is None or 'error' in (host.layout() or {}))
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        for cli, env in ((cli_a, env_a), (cli_b, env_b)):
            run_cli(cli, env, '--json', 'stop', check=False, timeout=80)
        host.emit('host-exit')
        (out / 'tray-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: results[k] for k in ('passed',)}, indent=2))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
