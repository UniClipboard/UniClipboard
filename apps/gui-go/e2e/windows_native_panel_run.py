#!/usr/bin/env python3
r"""Windows native quick panel E2E: Go host -> GPUI helper (`uniclip-quick-panel.exe`) -> isolated daemon.

Run on a DEDICATED, unattended Windows session (a test VM): this script sends real key events, moves the foreground
window and overwrites the clipboard (the daemon captures it into the isolated profile, then the panel restores a chosen
entry to it). It refuses to start unless UC_GUI_GO_E2E_DEDICATED_HOST=1. The clipboard text before the run is
restored afterwards; only text is saved, so any other clipboard format is lost.

    python windows_native_panel_run.py --out <dir> --binaries <dir with gui-go.exe uniclipd.exe uniclip.exe
                                                    uniclip-quick-panel.exe>

`gui-go.exe` must be the e2e build (`go build -tags e2e`, see build_windows.py); every executable must be built for the
architecture of the host, an emulated run is not evidence for the other one.

Windows Firewall: the daemon listens on the network, so Windows asks once per program path whether to allow it, and holds the
daemon behind that prompt until someone clicks it (a mouse is needed; the keyboard does not reach it). The sandbox is therefore
the fixed directory C:\w\uc-gui-go-native (UC_GUI_GO_E2E_SANDBOX_ROOT moves the parent). Answer the prompt for
C:\w\uc-gui-go-native\uniclipd.exe once (Cancel is enough: this test needs no inbound traffic); without that rule the script stops with instructions.

Isolation: the e2e host refuses to run isolated with the real clipboard unless UC_GUI_GO_E2E_REAL_CLIPBOARD=1 and
UC_GUI_GO_E2E_DEDICATED_HOST=1 are both set (realClipboardAllowed); this script sets the first for the GUI it starts. The executables are copied into a throwaway `uc-gui-go-*` directory and run with UC_PORTABLE=1, so the data
root, caches and file-based keystore live there. Only processes this script started are stopped, by PID.

What is checked (each recorded in native-assertions.json):
  1  the host starts exactly one helper, as its own child, from the sandbox, and a hidden panel is not on screen
  2  the default shortcut (Ctrl+Alt+V, real SendInput events) shows the panel, which takes the foreground, and the same
     shortcut hides it again and gives the foreground back to the window that had it
  3a with an empty query, Down twice and Enter paste the third (oldest) entry into the previous window
  3  typing a digit query and Enter pastes the matching history entry into the previous window, and the panel is gone
  4  Esc closes the panel without pasting and returns the foreground
  4b Ctrl+Shift+O in the panel (the `show_main_window` request) brings the minimized main window back to the foreground
  4c Ctrl+, in the panel (the `open_settings` request) does the same (the settings route itself is not machine-checked)
  5  a shortcut owned by another process is refused as Conflict and the old binding stays; after the other process lets
     go the same change succeeds, the helper is restarted, the old shortcut is dead and the new one opens the panel
  6  the modifier double tap (Ctrl, twice, alone) opens the panel after the setting is saved; a Ctrl chord does not
  7  a helper killed by this script is brought back by the supervisor
  8  exit: GUI exit code 0, no helper, no daemon of this run remains, and the shortcut is free again
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from windows_quick_panel_run import (  # noqa: E402
    KEY, PASSPHRASE, TARGET_PS1, VK, Gui, foreground_pid, get_clipboard_text, hold_hotkey, hotkey_is_free,
    kernel32, send_chord, set_clipboard_text, user32,
)

# Window handles are pointer-sized: without declared types ctypes passes and returns 32-bit ints.
if user32 is not None:
    user32.FindWindowW.argtypes, user32.FindWindowW.restype = [wt.LPCWSTR, wt.LPCWSTR], wt.HWND
    user32.GetForegroundWindow.restype = wt.HWND
    user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
    user32.AttachThreadInput.argtypes = [wt.DWORD, wt.DWORD, wt.BOOL]
    user32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]
    user32.BringWindowToTop.argtypes = [wt.HWND]
    user32.SetForegroundWindow.argtypes = [wt.HWND]
    user32.IsWindowVisible.argtypes = [wt.HWND]
    user32.IsIconic.argtypes = [wt.HWND]
    user32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
    user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
    user32.EnumWindows.argtypes = [ctypes.c_void_p, wt.LPARAM]
    kernel32.GetCurrentThreadId.restype = wt.DWORD

VK.update({'v': 0x56, 'enter': 0x0D, 'esc': 0x1B, 'f13': 0x7C, 'f14': 0x7D, 'shift': 0x10, 'down': 0x28, 'o': 0x4F, 'comma': 0xBC, 'back': 0x08})
HELPER = 'uniclip-quick-panel.exe'
PANEL_TITLE = 'UniClipboard History'
DEFAULT_SHORTCUT = 'ctrl+alt+v'
SANDBOX_ROOT = Path(os.environ.get('UC_GUI_GO_E2E_SANDBOX_ROOT', r'C:\w'))
# One fixed sandbox directory: Windows keys its Firewall decision for the daemon by program path, see firewall_rule().
SANDBOX_NAME = 'uc-gui-go-native'
NEW_SHORTCUT = 'ctrl+alt+shift+f13'
MARKERS = ['uc-native-alpha-1111', 'uc-native-beta-2222', 'uc-native-gamma-3333']


def run_quiet(*args, **kwargs):
    """subprocess.run without a console window. A new window takes the foreground, and the panel hides itself (by design)
    when it loses it, so every helper process of this test must stay invisible."""
    kwargs.setdefault('creationflags', 0x08000000)  # CREATE_NO_WINDOW
    return subprocess.run(*args, **kwargs)


def powershell_json(command):
    out = run_quiet(['powershell', '-NoProfile', '-Command', command], capture_output=True, text=True, errors='replace').stdout.strip()
    if not out:
        return []
    data = json.loads(out)
    return data if isinstance(data, list) else [data]


def processes(name):
    """pid, parent pid and image path of every process with this image name."""
    return powershell_json(
        f"Get-CimInstance Win32_Process -Filter \"Name='{name}'\" | Select-Object ProcessId,ParentProcessId,ExecutablePath | ConvertTo-Json")


def alive(pid):
    out = run_quiet(['tasklist', '/FI', f'PID eq {pid}', '/NH'], capture_output=True, text=True, errors='replace').stdout
    return str(pid) in out


def panel_windows(pids):
    """Visible top-level windows titled like the panel that belong to one of `pids`."""
    found = []
    proc_type = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def visit(hwnd, _):
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and user32.IsWindowVisible(hwnd):
            buffer = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, buffer, 256)
            if buffer.value == PANEL_TITLE:
                rect = wt.RECT()
                user32.GetWindowRect(hwnd, ctypes.byref(rect))
                found.append({'hwnd': hwnd, 'rect': [rect.left, rect.top, rect.right, rect.bottom]})
        return True
    user32.EnumWindows(proc_type(visit), 0)
    return found


def windows_of(pid):
    """Top-level windows of one process: hwnd, title, visible, minimized."""
    found = []
    proc_type = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def visit(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            buffer = ctypes.create_unicode_buffer(256)
            user32.GetWindowTextW(hwnd, buffer, 256)
            found.append({'hwnd': hwnd, 'title': buffer.value, 'visible': bool(user32.IsWindowVisible(hwnd)),
                          'minimized': bool(user32.IsIconic(hwnd))})
        return True
    user32.EnumWindows(proc_type(visit), 0)
    return found


def type_digits(digits):
    """Types digits as key events with virtual-key codes. Digits reach the focused window as plain keys even when an input
    method editor in Chinese mode is active; letters would start an IME composition."""
    for digit in digits:
        VK[digit] = 0x30 + int(digit)
        send_chord(digit, hold=0.05)
        time.sleep(0.05)


def focus_window(title):
    """Brings the window to the foreground. A process that has not received input may not take it, so this joins the input
    queue of the current foreground thread for the call (AttachThreadInput), the same workaround the host and the helper use."""
    hwnd = user32.FindWindowW(None, title)
    current = kernel32.GetCurrentThreadId()
    for _ in range(5):
        front = user32.GetWindowThreadProcessId(user32.GetForegroundWindow(), None)
        attached = front and front != current and user32.AttachThreadInput(front, current, True)
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
        if attached:
            user32.AttachThreadInput(front, current, False)
        time.sleep(.6)
        if user32.GetForegroundWindow() == hwnd:
            return


def firewall_rule(program):
    """The action (Allow/Block) of an inbound Windows Firewall rule for exactly this program, or None.

    Windows raises an "allow network access" prompt for every new path of `uniclipd.exe` and keeps the daemon's start
    blocked behind it (it takes the foreground and needs a mouse). Any answer (Allow, or Cancel which blocks) is a
    persistent system rule for that path, so this test always runs the daemon from one fixed path and expects an operator to have answered the prompt for it once.
    """
    wanted = str(program).replace("'", "''")
    rows = powershell_json(
        f"Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -eq '{wanted}' }} | ForEach-Object {{ $r = $_ | Get-NetFirewallRule; "
        "[pscustomobject]@{Program=$_.Program;Action=[string]$r.Action;Name=$r.Name} } | ConvertTo-Json")
    actions = {row['Action'] for row in rows if (row.get('Program') or '').lower() == str(program).lower()
               and 'Query User' not in row['Name']}
    return 'Allow' if 'Allow' in actions else (next(iter(actions)) if actions else None)


def screenshot(path):
    """The whole virtual screen as a PNG: evidence of what was actually drawn."""
    command = ("Add-Type -AssemblyName System.Windows.Forms,System.Drawing; "
               "$b=[System.Windows.Forms.SystemInformation]::VirtualScreen; "
               "$bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; "
               "$g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size); "
               f"$bmp.Save('{path}')")
    run_quiet(['powershell', '-NoProfile', '-Command', command], capture_output=True)


def wait_for(predicate, timeout=10, step=0.1):
    deadline = time.monotonic() + timeout
    while True:
        value = predicate()
        if value or time.monotonic() > deadline:
            return value
        time.sleep(step)


def clipboard_formats():
    """The clipboard format ids in use right now (empty when the clipboard is empty or cannot be opened)."""
    user32.EnumClipboardFormats.argtypes = [wt.UINT]
    user32.EnumClipboardFormats.restype = wt.UINT
    if not user32.OpenClipboard(None):
        return None
    try:
        formats, fmt = [], user32.EnumClipboardFormats(0)
        while fmt:
            formats.append(fmt)
            fmt = user32.EnumClipboardFormats(fmt)
        return formats
    finally:
        user32.CloseClipboard()


def clipboard_format_name(fmt):
    names = {2: 'CF_BITMAP', 8: 'CF_DIB', 15: 'CF_HDROP', 17: 'CF_DIBV5'}
    if fmt in names:
        return names[fmt]
    buffer = ctypes.create_unicode_buffer(128)
    return buffer.value if user32.GetClipboardFormatNameW(fmt, buffer, 128) else str(fmt)


# CF_TEXT, CF_OEMTEXT, CF_UNICODETEXT, CF_LOCALE: what restoring the clipboard as one text string preserves.
TEXT_FORMATS = {1, 7, 13, 16}
# Marker formats that carry no content: clipboard sharing and history tools put them next to the text.
MARKER_FORMATS = {'Deskflow Ownership', 'Clipboard Viewer Ignore', 'ExcludeClipboardContentFromMonitorProcessing',
                  'CanIncludeInClipboardHistory', 'CanUploadToCloudClipboard'}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--clobber-clipboard', action='store_true',
                        help='run although the clipboard holds more than text (images, files, rich text), which is lost')
    args = parser.parse_args()
    if os.name != 'nt':
        sys.exit('this script runs on Windows only')
    if os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('refusing to send key events on a session that is not a dedicated test host (set UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    formats = clipboard_formats()
    extra = sorted(f for f in set(formats or []) - TEXT_FORMATS if clipboard_format_name(f) not in MARKER_FORMATS)
    if extra and not args.clobber_clipboard:
        sys.exit(f'the clipboard holds data this test cannot give back (formats {[clipboard_format_name(f) for f in extra]}); copy plain text or nothing, or pass --clobber-clipboard')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox, profile, sandbox_root = SANDBOX_ROOT / SANDBOX_NAME, 'gui-go-native', SANDBOX_ROOT
    if firewall_rule(sandbox / 'uniclipd.exe') is None and os.environ.get('UC_GUI_GO_E2E_FIREWALL_PROMPT_OK') != '1':
        sys.exit(f'no Windows Firewall decision is recorded for {sandbox / "uniclipd.exe"}: the first start of its daemon raises a '
                 'prompt that holds the daemon back and takes the foreground. Set UC_GUI_GO_E2E_FIREWALL_PROMPT_OK=1, start this script '
                 'once, answer the prompt when it appears (Cancel keeps the daemon blocked, which this test does not mind), and run it again.')
    if sandbox.exists():
        shutil.rmtree(sandbox)
    sandbox.mkdir(parents=True)
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe', HELPER):
        shutil.copy2(args.binaries / name, sandbox / name)
    base_env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', NO_COLOR='1')
    gui_env = dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_SHORTCUTS='1',
                   UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_REAL_CLIPBOARD='1')
    for name in ('UC_GPUI_SHORTCUT', 'UC_GPUI_QUICK_PANEL', 'UC_GUI_GO_E2E_DEFAULT_SHORTCUT', 'UC_DISABLE_SYSTEM_CLIPBOARD'):  # the native panel is the default
        gui_env.pop(name, None)
        base_env.pop(name, None)
    uniclip = str(sandbox / 'uniclip.exe')
    results = {'sandbox': str(sandbox), 'profile': profile, 'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME'),
               'arch': os.environ.get('PROCESSOR_ARCHITECTURE'),
               'sha256': {}}
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe', HELPER):
        results['sha256'][name] = hashlib.sha256((sandbox / name).read_bytes()).hexdigest()
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def helper_pids():
        return {p['ProcessId'] for p in processes(HELPER) if str(sandbox).lower() in (p['ExecutablePath'] or '').lower()}

    gui = target = release = None
    saved_clip = get_clipboard_text()
    daemon_pid = None
    try:
        run_quiet([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'win-native-panel'], env=base_env, check=True, timeout=120)
        # `space init` leaves a one-shot daemon that exits on its own; let a persistent one take over before the host starts, as
        # native_panel_run.py does on macOS, so the host reuses a settled daemon instead of racing the exiting one.
        for _ in range(3):
            started = run_quiet([uniclip, 'start'], env=base_env, capture_output=True, text=True, errors='replace', timeout=120)
            if started.returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError(f'daemon start failed three times (exit {started.returncode}): {(started.stdout + started.stderr)[-600:]}')
        target_text = out / 'target-text.txt'
        ps1 = out / 'target.ps1'
        ps1.write_text(TARGET_PS1, encoding='utf-8')
        target = subprocess.Popen(['powershell', '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', str(ps1), str(target_text)])
        time.sleep(4)
        focus_target = lambda: focus_window('uc-gui-go-paste-target')

        def clear_editor():
            """Focus the editor and empty it (the file the editor writes follows its text)."""
            focus_target()
            # A multiline WinForms text box has no Ctrl+A; the longest text this run puts in it is two entries.
            for _ in range(80):
                send_chord('back')
            wait_for(lambda: (target_text.read_text(encoding='utf-8-sig') if target_text.exists() else '') == '', 5)
        held = [vk for vk in range(0x07, 0xFF) if user32.GetAsyncKeyState(vk) & 0x8000]
        check('precondition: no key is held down (a stuck key would also disable the modifier double tap)', not held, {'held': held})
        focus_target()
        check('paste target window is the foreground window', foreground_pid() == target.pid, {'foreground': foreground_pid(), 'target': target.pid})

        gui = Gui(sandbox, gui_env, out)
        gui.step('bootstrapped', 120)
        daemon_pid = json.loads((sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn').read_text())['pid']
        found = wait_for(lambda: [p for p in processes(HELPER) if p['ParentProcessId'] == gui.proc.pid], 30)
        pids = helper_pids()
        state = gui.ctl('state native', 'control-state')['detail']
        check('1 the host starts exactly one helper, as its own child, from the sandbox',
              len(found) == 1 and pids == {found[0]['ProcessId']} and state.get('helperRunning') is True, {'children': found, 'sandbox': sorted(pids), 'state': state})
        check('1 a hidden panel is not on screen', not panel_windows(pids))
        focus_target()

        # The daemon keeps visible content locked until the GUI authorizes it; the helper then reads history.
        r = gui.invoke('unlock', 'unlock_content', {'request': {'passphrase': PASSPHRASE}})
        check('content unlocked through the host (the helper reads history only afterwards)', r['ok'], r)
        time.sleep(1)

        # Seed the history through the real capture path: the daemon watches the system clipboard.
        for marker in MARKERS:
            set_clipboard_text(marker)
            time.sleep(2.5)
        def captured():
            raw = run_quiet([uniclip, '--json', 'search', 'uc-native'], env=base_env, capture_output=True, text=True, errors='replace').stdout
            return raw if all(m in raw for m in MARKERS) else None
        seen = wait_for(captured, 30, 1)
        check('history holds the three clipboard entries (captured by the isolated daemon)', bool(seen))
        focus_target()

        send_chord('ctrl', 'alt', 'v', hold=0.15)
        shown = wait_for(lambda: panel_windows(helper_pids()), 10)
        panel_pid = next(iter(helper_pids()))
        # The first frame of the panel is slow in a VM, and the helper takes the foreground when it has drawn it.
        wait_for(lambda: foreground_pid() == panel_pid, 8)
        check('2 the default shortcut shows the panel and the panel takes the foreground',
              bool(shown) and foreground_pid() == panel_pid, {'windows': shown, 'foreground': foreground_pid(), 'helper': panel_pid})
        send_chord('ctrl', 'alt', 'v', hold=0.15)
        hidden = wait_for(lambda: not panel_windows(helper_pids()), 10)
        wait_for(lambda: foreground_pid() == target.pid, 8)
        check('2 the same shortcut hides it and the previous window is the foreground again', hidden and foreground_pid() == target.pid,
              {'foreground': foreground_pid(), 'target': target.pid})

        # 3a: the history is newest first (gamma, beta, alpha), so two Downs from the first row reach alpha.
        clear_editor()  # the panel pastes into whatever window was in front when it opened
        send_chord('ctrl', 'alt', 'v', hold=0.15)
        wait_for(lambda: panel_windows(helper_pids()), 10)
        wait_for(lambda: foreground_pid() == panel_pid, 8)
        time.sleep(1.5)  # the first search result has to be there before the selection moves
        for _ in range(2):
            send_chord('down', hold=0.05)
            time.sleep(0.3)
        screenshot(out / 'panel-arrows.png')
        send_chord('enter', hold=0.1)
        typed = wait_for(lambda: (target_text.read_text(encoding='utf-8-sig') if target_text.exists() else '') or None, 8) or ''
        check('3a Down twice and Enter paste the third entry into the previous window', typed == MARKERS[0], {'typed': typed})
        wait_for(lambda: not panel_windows(helper_pids()) and foreground_pid() == target.pid, 8)

        clear_editor()  # the panel pastes into whatever window was in front when it opened
        send_chord('ctrl', 'alt', 'v', hold=0.15)
        wait_for(lambda: panel_windows(helper_pids()), 10)
        wait_for(lambda: foreground_pid() == panel_pid, 8)
        time.sleep(.5)
        type_digits('2222')  # only the beta entry contains 2222
        time.sleep(1.5)  # the search is debounced
        screenshot(out / 'panel-query.png')
        send_chord('enter', hold=0.1)
        time.sleep(0.6)
        screenshot(out / 'panel-after-enter.png')
        time.sleep(1.4)
        typed = target_text.read_text(encoding='utf-8-sig') if target_text.exists() else ''
        check('3 typing 2222 and Enter pastes the matching entry into the previous window', typed == MARKERS[1], {'typed': typed})
        check('3 the panel is gone and the previous window is the foreground',
              not panel_windows(helper_pids()) and foreground_pid() == target.pid, {'foreground': foreground_pid()})

        clear_editor()
        send_chord('ctrl', 'alt', 'v', hold=0.15)
        wait_for(lambda: panel_windows(helper_pids()), 10)
        time.sleep(.5)
        send_chord('esc', hold=0.1)
        gone = wait_for(lambda: not panel_windows(helper_pids()), 5)
        time.sleep(.6)
        check('4 Esc closes the panel without pasting and returns the foreground',
              gone and foreground_pid() == target.pid and (not target_text.exists() or target_text.read_text(encoding='utf-8-sig') == ''),
              {'foreground': foreground_pid()})

        # 4b / 4c: the panel's requests to the host. The main window is minimized first, so that showing it is visible.
        def main_window():
            return next((w for w in windows_of(gui.proc.pid) if w['title'] == 'UniClipboard'), None)

        for label, keys in (('4b Ctrl+Shift+O (show_main_window)', ('ctrl', 'shift', 'o')), ('4c Ctrl+, (open_settings)', ('ctrl', 'comma'))):
            main = main_window()
            if not main:
                check(f'{label}: the host has a main window', False, {'windows': windows_of(gui.proc.pid)})
                continue
            user32.ShowWindow(main['hwnd'], 6)  # SW_MINIMIZE
            wait_for(lambda: user32.IsIconic(main['hwnd']), 5)
            focus_target()
            send_chord('ctrl', 'alt', 'v', hold=0.15)
            wait_for(lambda: panel_windows(helper_pids()), 10)
            wait_for(lambda: foreground_pid() == panel_pid, 8)
            time.sleep(1.0)
            send_chord(*keys, hold=0.1)
            shown_main = wait_for(lambda: not user32.IsIconic(main['hwnd']) and user32.IsWindowVisible(main['hwnd']) and foreground_pid() == gui.proc.pid, 10)
            time.sleep(1.0)
            screenshot(out / f'{label[:2]}-main-window.png')
            check(f'{label} brings the main window to the foreground', bool(shown_main),
                  {'main': main_window(), 'foreground': foreground_pid(), 'host': gui.proc.pid})
            if panel_windows(helper_pids()):
                send_chord('esc', hold=0.1)
                wait_for(lambda: not panel_windows(helper_pids()), 5)
            user32.ShowWindow(main['hwnd'], 6)
            wait_for(lambda: user32.IsIconic(main['hwnd']), 5)
            focus_target()

        # 5: a conflicting shortcut is refused before it is saved; once free it is accepted and the helper restarts.
        release = hold_hotkey(NEW_SHORTCUT)
        before_pids = helper_pids()
        r = gui.invoke('conflict', 'update_keyboard_shortcuts', {'shortcuts': {KEY: NEW_SHORTCUT}})
        state = gui.state('after-conflict')
        check('5 a shortcut owned by another process is refused as Conflict and nothing is saved',
              not r['ok'] and r['error']['code'] == 'Conflict' and state['stored'] is None and helper_pids() == before_pids, [r, state])
        release()
        release = None
        r = gui.invoke('rebind', 'update_keyboard_shortcuts', {'shortcuts': {KEY: NEW_SHORTCUT}})
        restarted = wait_for(lambda: (lambda now: now and now != before_pids and now)(helper_pids()), 20)
        time.sleep(2)
        check('5 the same change succeeds once the other process let go, and the helper is restarted',
              r['ok'] and bool(restarted), {'result': r, 'before': sorted(before_pids), 'after': sorted(helper_pids())})
        focus_target()
        send_chord('ctrl', 'alt', 'v', hold=0.15)
        time.sleep(1.2)
        check('5 the old shortcut no longer opens the panel', not panel_windows(helper_pids()))
        send_chord('ctrl', 'alt', 'shift', 'f13', hold=0.15)
        shown = wait_for(lambda: panel_windows(helper_pids()), 10)
        check('5 the new shortcut opens it', bool(shown))
        send_chord('esc', hold=0.1)
        wait_for(lambda: not panel_windows(helper_pids()), 5)
        focus_target()

        # 6: the modifier double tap.
        r = gui.invoke('double-tap', 'set_quick_panel_double_tap_modifier', {'modifier': 'control'})
        wait_for(lambda: helper_pids(), 20)
        time.sleep(3)
        focus_target()
        # The restarted helper applies the persisted setting once it has reached the daemon, so tap until it reacts.
        shown = None
        for _ in range(8):
            send_chord('ctrl', hold=0.06)
            time.sleep(0.12)
            send_chord('ctrl', hold=0.06)
            shown = wait_for(lambda: panel_windows(helper_pids()), 2.5)
            if shown:
                break
        check('6 Ctrl tapped twice opens the panel', r['ok'] and bool(shown), {'result': r})
        send_chord('esc', hold=0.1)
        wait_for(lambda: not panel_windows(helper_pids()), 5)
        focus_target()
        send_chord('ctrl', 'v', hold=0.06)
        time.sleep(0.12)
        send_chord('ctrl', 'v', hold=0.06)
        time.sleep(1.0)
        check('6 a Ctrl chord (Ctrl+V twice) does not open the panel', not panel_windows(helper_pids()))
        gui.invoke('double-tap-off', 'set_quick_panel_double_tap_modifier', {'modifier': 'disabled'})
        wait_for(lambda: helper_pids(), 20)

        # 7: supervision.
        victims = helper_pids()
        for pid in victims:
            run_quiet(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        revived = wait_for(lambda: (lambda now: now and now.isdisjoint(victims) and now)(helper_pids()), 30)
        check('7 a killed helper is restarted by the supervisor', bool(revived), {'killed': sorted(victims), 'revived': sorted(helper_pids())})

        # 8: exit leaves nothing behind.
        gui.ctl('exit', 'control-exit')
        code = gui.proc.wait(timeout=60)
        wait_for(lambda: not helper_pids() and not alive(daemon_pid), 20)
        check('8 GUI exit 0, no helper and no daemon of this run remain', code == 0 and not helper_pids() and not alive(daemon_pid),
              {'exit': code, 'helpers': sorted(helper_pids()), 'daemonAlive': alive(daemon_pid)})
        check('8 both shortcuts are free after exit', hotkey_is_free('ctrl+alt+shift+f13') and hotkey_is_free('ctrl+alt+v'))
        results['passed'] = all(c['ok'] is not False for c in checks)
    finally:
        if release:
            release()
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        if target and target.poll() is None:
            target.terminate()
        for pid in helper_pids():
            run_quiet(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        run_quiet([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid and alive(daemon_pid):  # only the PID recorded in this sandbox's own daemon.conn
            run_quiet(['taskkill', '/F', '/PID', str(daemon_pid)], capture_output=True)
        if saved_clip is not None:
            set_clipboard_text(saved_clip)
        (out / 'native-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
        if not results['passed']:
            try:
                shutil.copytree(sandbox, out / 'sandbox-failed', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
            except OSError as e:
                print(f'could not keep the failed sandbox: {e}', file=sys.stderr)
        if sandbox.name == SANDBOX_NAME and sandbox.parent == sandbox_root:
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
