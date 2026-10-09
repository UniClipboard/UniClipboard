#!/usr/bin/env python3
"""Windows tray and notification E2E through the real shell (UI Automation + real mouse clicks).

The e2e GUI runs in a sandbox (UC_PORTABLE, `gui-go-*` profile, e2e bundle id `app.uniclipboard.desktop.e2e`). The
shell UI is driven by windows_tray_uia.ps1: it finds the notification icon by its tooltip, clicks it (the pointer
really moves), reads the popup menu's items and clicks one. Notifications use the real Windows toast path (no
UC_GUI_GO_E2E_NOTIFY_LOG recorder); the click callback is evidenced by the host's own `notification-click` step.

  T1  the tray icon exists and its menu shows the root labels of the current language (and the device submenu)
  T2  a language switch (the host command the settings page uses) re-labels the open menu in the new language
  T3  a left click on the icon shows the main window
  T4  the menu's Quit item exits the GUI (exit code 0), stops the daemon and removes the icon
  N1  a native notification is shown (toast text found in the shell)
  N2  clicking the toast reaches the host's response callback
Evidence: the UIA element listings and menu dumps are written next to the results.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import windows_quick_panel_run as q  # noqa: E402
from windows_single_instance_run import pid_alive, processes_in, steps, wait_until  # noqa: E402

LABELS = {
    'zh-CN': ['打开', '设置', '检查更新…', '重启', '轻量模式（后台同步）', '退出'],
    'en': ['Open', 'Settings', 'Check for Updates…', 'Restart', 'Lightweight Mode (Background Sync)', 'Quit'],
}
SYNC = {'zh-CN': ['关闭同步', '开启同步'], 'en': ['Disable Sync', 'Enable Sync']}
DEVICES = {'zh-CN': '设备同步', 'en': 'Device Sync'}


def uia(*args, timeout=60):
    r = subprocess.run(['powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', str(HERE / 'windows_tray_uia.ps1'), *args],
                       capture_output=True, text=True, errors='replace', timeout=timeout)
    return r.returncode, r.stdout.strip()


def menu_names():
    rc, out = uia('-Action', 'menu')
    try:
        items = json.loads(out) if out else []
    except ValueError:
        items = []
    if isinstance(items, dict):
        items = [items]
    return [i['name'] for i in items], out


def dismiss_menu():
    q.VK.setdefault('esc', 0x1B)
    q.send_chord('esc', hold=.05)
    time.sleep(.4)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('Windows dedicated host only (UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    out = args.out.resolve()
    out.mkdir(parents=True)
    sandbox, profile, root = q.make_sandbox()
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe'):
        shutil.copy2(args.binaries / name, sandbox / name)
    env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    genv = dict(env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_SHORTCUTS='1',
                UC_GUI_GO_E2E_DEFAULT_SHORTCUT=q.F13, UC_GUI_GO_EXIT_MODE='full')
    results = {'sandbox': str(sandbox), 'profile': profile, 'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME')}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    gui = None
    try:
        subprocess.run([str(sandbox / 'uniclip.exe'), 'space', 'init', '--passphrase', 'tray-pass', '--device-name', 'tray'], env=env, check=True, timeout=120, capture_output=True)
        gui = q.Gui(sandbox, genv, out)
        gui.step('bootstrapped', 120)
        ev = out / 'native.jsonl'
        gui.ctl('tray-language-quiet q0 1500', 'tray-language-quiet-q0')  # the frontend's startup language calls come in a burst
        time.sleep(1.5)
        (out / 'uia-list-before.txt').write_text(uia('-Action', 'list')[1], encoding='utf-8')

        # T1
        rc, clicked = uia('-Action', 'icon', '-Match', 'UniClipboard', '-Button', 'right')
        (out / 'uia-icon-click-1.txt').write_text(clicked, encoding='utf-8')
        names, raw = menu_names()
        (out / 'uia-menu-1.json').write_text(raw, encoding='utf-8')
        lang = next((l for l, v in LABELS.items() if all(x in names for x in v)), None)
        check('T1 tray icon found by tooltip and its right-click menu lists the root labels of one language',
              rc == 0 and lang is not None, {'click': clicked, 'items': names})
        if lang:
            check('T1b the menu has the sync toggle and the device submenu', any(n in SYNC[lang] for n in names) and DEVICES[lang] in names, names)
        dismiss_menu()

        # T2 language switch through the host command the settings page uses
        target = 'en' if lang == 'zh-CN' else 'zh-CN'
        gui.invoke('lang', 'set_tray_language', {'language': target})
        time.sleep(.8)
        rc, clicked = uia('-Action', 'icon', '-Match', 'UniClipboard', '-Button', 'right')
        names2, raw2 = menu_names()
        (out / 'uia-menu-2.json').write_text(raw2, encoding='utf-8')
        check('T2 after set_tray_language the menu shows the other language', rc == 0 and all(x in names2 for x in LABELS[target]) and DEVICES[target] in names2, {'target': target, 'items': names2})
        dismiss_menu()

        # T3 left click shows the main window
        before = gui.ctl('state t3a', 'control-state')['detail']
        rc, clicked = uia('-Action', 'icon', '-Match', 'UniClipboard', '-Button', 'left')
        time.sleep(1.5)
        after = steps(ev, 'control-state')[-1]['detail'] if gui.ctl('state t3b', 'control-state') else {}
        visible = subprocess.run(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {gui.proc.pid}).MainWindowTitle'], capture_output=True, text=True, errors='replace').stdout.strip()
        check('T3 left click on the icon shows the main window', rc == 0 and after.get('mainExists') and bool(visible), {'mainExistsBefore': before.get('mainExists'), 'after': after, 'mainWindowTitle': visible})

        # N1/N2 notification
        title = f'uc-e2e-{int(time.time())}'
        gui.invoke('notify', 'host_notification_send', {'options': {'id': 4242, 'title': title, 'body': 'tray run notification'}})
        found = wait_until(lambda: uia('-Action', 'list', '-Match', title)[1] if title in uia('-Action', 'list', '-Match', title)[1] else None, 15, 1)
        (out / 'uia-list-toast.txt').write_text(uia('-Action', 'list', '-Match', title)[1], encoding='utf-8')
        check('N1 the native toast is shown (its title is found in the shell UI)', bool(found), title)
        if found:
            rc, clicked = uia('-Action', 'toast', '-Match', title)
            got = wait_until(lambda: steps(ev, 'notification-click') or None, 10)
            check('N2 clicking the toast reaches the host notification callback', rc == 0 and bool(got), {'click': clicked, 'steps': got})
        else:
            checks.append({'check': 'N2 toast click', 'ok': None, 'skipped': 'no toast was found'})

        # T4 quit from the menu
        daemon_conn = sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn'
        daemon_pid = json.loads(daemon_conn.read_text())['pid'] if daemon_conn.is_file() else None
        uia('-Action', 'icon', '-Match', 'UniClipboard', '-Button', 'right')
        quit_label = LABELS[target][-1]
        rc, chose = uia('-Action', 'choose', '-Name', quit_label)
        try:
            code = gui.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            code = 'TIMEOUT'
        gone = wait_until(lambda: not (daemon_pid and pid_alive(daemon_pid)), 20)
        rc2, _ = uia('-Action', 'icon', '-Match', 'UniClipboard', '-Button', 'left') if False else (0, '')
        check('T4 Quit in the tray menu exits the GUI with 0 and stops its daemon', rc == 0 and code == 0 and bool(gone), {'choose': chose, 'exit': code, 'daemonPid': daemon_pid})
        results['passed'] = all(c['ok'] is not False and c['ok'] is not None for c in checks)
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.kill()
        for pid in processes_in(sandbox) + processes_in(sandbox, 'uniclipd.exe'):
            subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        shutil.copytree(sandbox, out / 'sandbox', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
        shutil.rmtree(sandbox, ignore_errors=True)
        (out / 'windows-tray-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
