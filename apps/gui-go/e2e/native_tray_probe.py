#!/usr/bin/env python3
"""Native-host tray check of slice 17c14 (docs/architecture/gui-go-linux-tray-menu-refresh.md): the real AppImage on a real
Wayland session (or its XWayland), as the logged-in user, inside a task-owned directory only.

  native_tray_probe.py --appimage X.AppImage --out DIR --mode native|x11-env [--seconds 40]

The tray HOST is the observer in linux/tray_probe/sni_host.py running on a TASK-OWNED private session bus: it owns
org.kde.StatusNotifierWatcher there and reads the dbusmenu layout. The user's own tray (quickshell) lives on the user's
session bus and is NOT used, so this proves the app's StatusNotifierItem lifecycle, menu, actions and exit against a
standard host, NOT how a desktop shell draws it. No peer is paired here: the device submenu holds the placeholder, which
the periodic refresh rebuilds every period (the path that logged Gtk-CRITICAL). Real paired-peer rows are covered only
by the container E2E.
Isolation, as native_wayland_probe.py: portable HOME next to the task copy, private bus, system clipboard disabled, only own pids signalled.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / 'linux' / 'tray_probe'))
from native_wayland_probe import sh, procs, socket_classes, environ_of  # noqa: E402

ROOT_ORDER = ['Device Sync', '-', 'Open', 'Settings', 'Check for Updates…', '-', 'Restart', 'Lightweight Mode (Background Sync)', 'Quit']
ZH = ['设备同步', '-', '打开', '设置', '检查更新…', '-', '重启', '轻量模式（后台同步）', '退出']
MENU_CRITICAL = re.compile(r'(gtk_container_foreach|gtk_menu_shell_insert|gtk_menu_item_set_submenu|gtk_menu_|GtkMenu|GTK_IS_MENU)')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--mode', choices=('native', 'x11-env'), required=True)
    ap.add_argument('--seconds', type=int, default=40)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = out / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}')
    wayland = next((p.name for p in sorted(runtime.glob('wayland-[0-9]*')) if not p.name.endswith('.lock')), None)
    result = {'mode': args.mode, 'host': sh(['uname', '-srm'])[1].strip(), 'waylandSocket': wayland, 'checks': [], 'unknown': [],
              'scope': 'native host session, portable mode, PRIVATE bus with the observer as tray host; not the desktop shell tray'}

    def check(name, ok, detail=None):
        result['checks'].append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|UC_|UNICLIPBOARD|GIO_|APPIMAGE|APPDIR|GDK_BACKEND|DBUS_SESSION|XDG_SESSION_TYPE', k)}
    env.update(XDG_RUNTIME_DIR=str(runtime), UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake',
               UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET='native-tray-probe-17c14', UC_GUI_GO_EVIDENCE=str(out / 'gui.jsonl'),
               UC_GUI_GO_E2E_CONTROL_FILE=str(out / 'gui.control'))
    if wayland:
        env['WAYLAND_DISPLAY'] = wayland
        env['XDG_SESSION_TYPE'] = 'wayland'
    x11 = sorted(p.name for p in Path('/tmp/.X11-unix').glob('X[0-9]*'))
    if x11:
        env['DISPLAY'] = ':' + x11[0][1:]
    if args.mode == 'x11-env':
        env['GDK_BACKEND'] = 'x11'
    if not any(Path(d).glob('libfuse.so.2') for d in ('/usr/lib', '/usr/lib64', '/usr/lib/aarch64-linux-gnu', '/lib64')):
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
    for f in ('gui.jsonl', 'gui.control'):
        (out / f).write_text('')
    made = subprocess.run([str(app), '--appimage-portable-home'], env=env, capture_output=True, text=True, timeout=60)
    home = Path(str(app) + '.home')
    check('portable home created next to the task-owned copy', made.returncode == 0 and home.is_dir(), {'rc': made.returncode})
    result['appimageSha256'] = sh(['sha256sum', str(app)])[1].split()[0]
    cfg = out / 'private-bus.conf'
    cfg.write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">\n<busconfig><type>session</type>'
                   f'<listen>unix:path={out}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>\n')
    bus = subprocess.Popen(['dbus-daemon', '--config-file', str(cfg), '--nofork'], stdout=(out / 'private-bus.log').open('w'), stderr=subprocess.STDOUT)
    for _ in range(50):
        if (out / 'bus.sock').exists():
            break
        time.sleep(.1)
    env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={out}/bus.sock'
    os.environ['DBUS_SESSION_BUS_ADDRESS'] = env['DBUS_SESSION_BUS_ADDRESS']  # the observer connects to the PRIVATE bus
    from sni_host import SniHost  # imported after the bus address is set
    from linux_tray_run import labels, submenu  # noqa: E402
    host = SniHost(str(out / 'host.jsonl'))
    proc = subprocess.Popen([str(app)], env=env, cwd=str(out), stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT, start_new_session=True)  # own session: nothing the app signals reaches this script
    seq = [0]

    def step(name, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for ln in (out / 'gui.jsonl').read_text().splitlines():
                try:
                    r = json.loads(ln)
                except ValueError:
                    continue
                if r.get('step') == name:
                    return r
            if proc.poll() is not None:
                return None
            time.sleep(.2)
        return None

    def lang_calls():
        rows = []
        for ln in (out / 'gui.jsonl').read_text().splitlines():
            try:
                r = json.loads(ln)
            except ValueError:
                continue
            if r.get('step') == 'tray-language-call':
                rows.append(r.get('detail'))
        return rows

    def invoke(command, arg):
        seq[0] += 1
        label = f'n{seq[0]}'
        with (out / 'gui.control').open('a') as f:
            f.write(f'invoke {label} {command} {json.dumps(arg)}\n')
        return (step(f'invoke-{label}') or {}).get('detail') or {}

    rc = None
    try:
        boot = step('bootstrapped', 120)
        check('the WebView ran the frontend (evidence step `bootstrapped`)', bool(boot and boot.get('ok')), boot)
        check('1 the tray item registers with the (observer) StatusNotifierWatcher on the private bus', host.registered.wait(40), host.item)
        lay = host.wait(lambda l: len(l['children']) >= 9, 30, 'root menu')
        check('2 root menu order through dbusmenu', lay is not None and labels(lay)[1:] == ROOT_ORDER and labels(lay)[0] in ('Enable Sync', 'Disable Sync'), labels(lay) if lay else None)
        sync0 = labels(lay)[0] if lay else None
        sub0 = [n['label'] for n in submenu(lay, 'Device Sync')] if lay else None
        check('3 device submenu: disabled placeholder (no peer paired on the host runs)', sub0 in (['No paired devices'], ['Devices unavailable']), sub0)
        if sync0:
            flip = {'Enable Sync': 'Disable Sync', 'Disable Sync': 'Enable Sync'}
            host.click(sync0)
            l2 = host.wait(lambda l: labels(l)[0] == flip[sync0], 30, 'sync flips')
            check('4 the sync item click flips its label through the daemon', l2 is not None, labels(l2)[0] if l2 else None)
            host.click(flip[sync0])
            l2 = host.wait(lambda l: labels(l)[0] == sync0, 30, 'sync restores')
            check('4 and flips back', l2 is not None, labels(l2)[0] if l2 else None)
        # The frontend's own settings effect also calls set_tray_language (once, when its settings load); our change must come after it.
        t_wait = time.time()
        while not lang_calls() and time.time() - t_wait < 60:
            time.sleep(.5)
        result['languageCallsBefore'] = lang_calls()
        check('5 precondition: the frontend\'s own initial tray-language call was seen before the test changes the language (ordering)', bool(lang_calls()), lang_calls())
        r = invoke('set_tray_language', {'language': 'zh-CN'})
        l2 = host.wait(lambda l: labels(l)[1:] == ZH, 30, 'zh')
        check('5 set_tray_language(zh-CN) relabels the whole menu in the host', bool(r.get('ok')) and l2 is not None, labels(l2) if l2 else None)
        result['languageCallsAfterZh'] = lang_calls()
        check('5 no later call overwrote zh-CN (the last recorded tray-language call is the test\'s)', lang_calls()[-1:] == ['zh-CN'], lang_calls())
        # Concurrent language changes (F-A): see linux_tray_run.py; here the device submenu holds the placeholder, whose language must match too.
        race = []
        for k in range(5):
            with (out / 'gui.control').open('a') as f:
                f.write(f'tray-language-race r{k} 8\n')
            row = step(f'tray-language-race-r{k}', 60) or {}
            final = (row.get('detail') or {}).get('final')
            want, ph = (ZH, '暂无已配对设备') if final == 'zh-CN' else (ROOT_ORDER, 'No paired devices')
            lay = host.wait(lambda l: labels(l)[1:] == want and [n['label'] for n in submenu(l, want[0])] == [ph], 20, f'race {k}')
            race.append({'final': final, 'rootLabels': labels(lay) if lay else None, 'consistent': lay is not None})
        result['languageRace'] = race
        check('5c five rounds of 8 concurrent set_tray_language calls always leave root labels, device title and placeholder in one language, the last recorded one',
              all(r['consistent'] for r in race), race)
        invoke('set_tray_language', {'language': 'en'})
        host.wait(lambda l: labels(l)[1:] == ROOT_ORDER, 20, 'en after race')
        r = invoke('set_tray_language', {'language': 'en'})
        l2 = host.wait(lambda l: labels(l)[1:] == ROOT_ORDER, 30, 'en')
        check('5 and back to English', bool(r.get('ok')) and l2 is not None, labels(l2) if l2 else None)
        # Observe the layout for the window: with no peer the placeholder is rebuilt (new item ids) every 10 s period, so the number of
        # distinct layouts seen is the number of structural republishes the host actually received.
        n_before = sum(1 for ln in (out / 'host.jsonl').read_text().splitlines() if '"kind": "layout"' in ln)
        host.wait(lambda l: False, args.seconds, 'observe refresh periods')
        n_after = sum(1 for ln in (out / 'host.jsonl').read_text().splitlines() if '"kind": "layout"' in ln)
        check('6a at least 3 structural republishes reached the host during the observation window (the refresh really ran)', n_after - n_before >= 3, {'layouts': n_after - n_before, 'seconds': args.seconds})
        table = procs()
        classes = socket_classes([proc.pid])
        c = classes.get(str(proc.pid)) or {}
        result['socketClasses'] = classes
        if args.mode == 'native':
            check('6 backend: the GUI holds a Wayland connection and none to an X11 server (native Wayland)', c.get('wayland', 0) >= 1 and c.get('x11', 0) == 0, c)
        else:
            check('6 backend: the GUI is an X11 client (XWayland)', c.get('x11', 0) >= 1 and c.get('wayland', 0) == 0, c)
        log = (out / 'gui.log').read_text(errors='replace')
        crit = [ln for ln in log.splitlines() if 'CRITICAL' in ln]
        result['criticalLines'] = crit
        menu_crit = [ln for ln in crit if MENU_CRITICAL.search(ln)]
        check('7 no menu-related Gtk-CRITICAL across the refresh periods', not menu_crit, menu_crit[:5])
        result['criticalOther'] = [ln for ln in crit if ln not in menu_crit]
        conns = []  # the daemon pid(s) of this run, read BEFORE the quit (the daemon removes daemon.conn when it stops)
        for c2 in home.rglob('daemon.conn'):
            try:
                conns.append(json.loads(c2.read_text())['pid'])
            except (OSError, ValueError, KeyError):
                pass
        check('8 precondition: the daemon of this run is alive and its pid is known before the quit', bool(conns) and all(Path(f'/proc/{p}').exists() for p in conns), conns)
        t_quit = time.time()
        host.click('Quit')
        try:
            rc = proc.wait(timeout=40)
        except subprocess.TimeoutExpired:
            rc = None
        check('8 the Quit item exits the GUI with 0', rc == 0, {'rc': rc, 'seconds': round(time.time() - t_quit, 1)})
        lay_after = host.layout()
        check('8 the tray item is gone from the host after exit', lay_after is None or 'error' in lay_after, lay_after)
        deadline = time.time() + 30
        alive = [p for p in conns if Path(f'/proc/{p}').exists()]
        while alive and time.time() < deadline:
            time.sleep(.5)
            alive = [p for p in conns if Path(f'/proc/{p}').exists()]
        check('8 the daemon of this run is stopped by the tray quit (full exit)', bool(conns) and not alive, {'pids': conns, 'alive': alive})
    finally:
        if proc.poll() is None:
            proc.terminate()
        bus.terminate()
        host.emit('host-exit')
        result['passed'] = all(c3['ok'] for c3 in result['checks'])
        (out / 'native-tray-result.json').write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    sys.exit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
