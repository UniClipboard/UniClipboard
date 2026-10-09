#!/usr/bin/env python3
"""Windows quick panel E2E: global shortcut, focus return, paste to the previous app. STATUS: AUTHORED, NOT YET RUN.

No Windows host was available when this was written (the only recorded runner, uniclipboard-windows-x64-vm-01, was
offline), so nothing below has executed on Windows. Treat every check as unverified until a run produces artifacts.

Run on a DEDICATED, unattended Windows session (a test VM): this script sends real key events, moves the foreground
window and, with --allow-clipboard, overwrites the clipboard. It refuses to start unless UC_GUI_GO_E2E_DEDICATED_HOST=1.

    python build_windows.py --mode e2e
    set UC_GUI_GO_E2E_DEDICATED_HOST=1
    python windows_quick_panel_run.py --out <dir> [--allow-clipboard]

Isolation: the executables are copied into a throwaway `uc-gui-go-*` directory and run with UC_PORTABLE=1, so the data
root, caches and the file-based keystore live there (neither the host nor the daemon read HOME/LOCALAPPDATA, and
without portable mode the daemon would use the real Credential Manager). Profile `gui-go-*`, no system clipboard for the
daemon (UC_DISABLE_SYSTEM_CLIPBOARD=1). Only PIDs this script started are stopped; the target window is a PowerShell
WinForms form it launches itself, whose text it reads back from a file.

What is checked (each recorded in windows-assertions.json):
  1  the default shortcut is registered through Wails (RegisterHotKey) at startup
  2  a REAL key chord (SendInput) shows the panel, the panel takes the foreground; the chord again hides it and the
     previous window gets the foreground back
  3  a combination held by another process is refused with a Conflict error, the old binding stays, and the same
     change succeeds after the other process releases it
  4  type_file_paths_to_previous_app types the paths ("\\n"-joined, Unicode incl. an astral character) into the
     previous window without touching the clipboard
  5  with --allow-clipboard: paste_to_previous_app pastes a clipboard marker into the previous window
  6  a failed paste (no recorded window) reports the error and shows the panel again
  7  exit: GUI exit code 0, daemon stopped by the GUI, and the shortcut is free again (RegisterHotKey by this script
     succeeds), no process of this run remains
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PASSPHRASE = 'windows-panel-passphrase'
KEY = 'global.toggleQuickPanel'
F13, F14 = 'ctrl+alt+shift+f13', 'ctrl+alt+shift+f14'
VK = {'ctrl': 0x11, 'alt': 0x12, 'shift': 0x10, 'f13': 0x7C, 'f14': 0x7D}
MOD = {'alt': 0x1, 'ctrl': 0x2, 'shift': 0x4}

user32 = ctypes.WinDLL('user32', use_last_error=True) if os.name == 'nt' else None
kernel32 = ctypes.WinDLL('kernel32', use_last_error=True) if os.name == 'nt' else None

TARGET_PS1 = r'''
Add-Type -AssemblyName System.Windows.Forms
$outPath = $args[0]
$form = New-Object System.Windows.Forms.Form
$form.Text = "uc-gui-go-paste-target"
$form.Width = 520; $form.Height = 300
$box = New-Object System.Windows.Forms.TextBox
$box.Multiline = $true; $box.Dock = "Fill"
$form.Controls.Add($box)
$box.Add_TextChanged({ [System.IO.File]::WriteAllText($outPath, $box.Text, [System.Text.Encoding]::UTF8) }.GetNewClosure())
$form.Add_Shown({ $form.Activate(); $box.Focus() })
[System.Windows.Forms.Application]::Run($form)
'''


def foreground_pid():
    hwnd = user32.GetForegroundWindow()
    pid = wt.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return pid.value


def send_chord(*names, hold=0.0):
    """Press the keys in order, release in reverse: a real keyboard event stream via SendInput.

    With hold > 0 the presses and releases are separate SendInput calls with the keys down in between, like a finger
    on a key; one batch (hold=0) releases within microseconds, which a key-state poll can never observe.
    """
    class KI(ctypes.Structure):
        _fields_ = [('vk', wt.WORD), ('scan', wt.WORD), ('flags', wt.DWORD), ('time', wt.DWORD), ('extra', ctypes.c_size_t)]

    class IN(ctypes.Structure):
        class U(ctypes.Union):
            _fields_ = [('ki', KI), ('pad', ctypes.c_byte * 32)]
        _anonymous_ = ('u',)
        _fields_ = [('type', wt.DWORD), ('u', U)]
    downs = [IN(1, IN.U(ki=KI(VK[n], 0, 0, 0, 0))) for n in names]
    ups = [IN(1, IN.U(ki=KI(VK[n], 0, 2, 0, 0))) for n in reversed(names)]
    batches = [downs, ups] if hold else [downs + ups]
    for i, events in enumerate(batches):
        if i:
            time.sleep(hold)
        arr = (IN * len(events))(*events)
        assert user32.SendInput(len(events), arr, ctypes.sizeof(IN)) == len(events), 'SendInput failed'


def hold_hotkey(spec):
    """Own a hot key from this process (the conflicting application). Returns a release() callable."""
    parts = spec.split('+')
    mods = sum(MOD[p] for p in parts[:-1])
    ready, stop, result = threading.Event(), threading.Event(), {}

    def loop():
        result['ok'] = bool(user32.RegisterHotKey(None, 1, mods, VK[parts[-1]]))
        ready.set()
        msg = wt.MSG()
        while not stop.is_set():
            user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1)
            time.sleep(.05)
        user32.UnregisterHotKey(None, 1)
    thread = threading.Thread(target=loop, daemon=True)
    thread.start()
    ready.wait(5)
    assert result.get('ok'), f'this script could not take {spec}: it is already owned'
    return lambda: (stop.set(), thread.join(5))


def hotkey_is_free(spec):
    parts = spec.split('+')
    mods = sum(MOD[p] for p in parts[:-1])
    ok = bool(user32.RegisterHotKey(None, 2, mods, VK[parts[-1]]))
    if ok:
        user32.UnregisterHotKey(None, 2)
    return ok


def get_clipboard_text():
    if not user32.OpenClipboard(None):
        return None
    try:
        handle = user32.GetClipboardData(13)  # CF_UNICODETEXT
        if not handle:
            return None
        kernel32.GlobalLock.restype = ctypes.c_void_p
        ptr = kernel32.GlobalLock(ctypes.c_void_p(handle))
        try:
            return ctypes.wstring_at(ptr)
        finally:
            kernel32.GlobalUnlock(ctypes.c_void_p(handle))
    finally:
        user32.CloseClipboard()


def set_clipboard_text(text):
    data = (text + '\0').encode('utf-16-le')
    kernel32.GlobalAlloc.restype = ctypes.c_void_p
    handle = kernel32.GlobalAlloc(0x2, len(data))  # GMEM_MOVEABLE
    kernel32.GlobalLock.restype = ctypes.c_void_p
    ptr = kernel32.GlobalLock(ctypes.c_void_p(handle))
    ctypes.memmove(ptr, data, len(data))
    kernel32.GlobalUnlock(ctypes.c_void_p(handle))
    assert user32.OpenClipboard(None), 'clipboard busy'
    try:
        user32.EmptyClipboard()
        user32.SetClipboardData(13, ctypes.c_void_p(handle))
    finally:
        user32.CloseClipboard()


def read_steps(path):
    return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines() if line.strip()]


class Gui:
    def __init__(self, sandbox, env, out):
        self.evidence, self.control = out / 'native.jsonl', out / 'native.control'
        self.evidence.write_text('')
        self.control.write_text('')
        env = dict(env, UC_GUI_GO_EVIDENCE=str(self.evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(self.control))
        self.proc = subprocess.Popen([str(sandbox / 'gui-go.exe')], env=env, cwd=sandbox,
                                     stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT)

    def step(self, name, timeout=60):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(self.evidence) if r['step'] == name]
            if rows:
                return rows[-1]
            if self.proc.poll() is not None:
                raise RuntimeError(f'GUI exited ({self.proc.returncode}) before {name}')
            time.sleep(.1)
        raise RuntimeError(f'timeout waiting for {name}')

    def ctl(self, line, label):
        with self.control.open('a') as f:
            f.write(line + '\n')
        return self.step(label)

    def state(self, label):
        return self.ctl(f'shortcut-state {label}', f'shortcut-state-{label}')['detail']

    def invoke(self, label, command, args=None):
        return self.ctl(f'invoke {label} {command} {json.dumps(args) if args is not None else ""}', f'invoke-{label}')['detail']

    def wait_state(self, label, predicate, timeout=8):
        deadline = time.monotonic() + timeout
        n = 0
        while True:
            n += 1
            state = self.state(f'{label}-{n}')
            if predicate(state) or time.monotonic() > deadline:
                return state
            time.sleep(.2)


def make_sandbox():
    """A throwaway directory and profile name short enough for the Engine data tree.

    The deepest file below the data root is ~175 characters long and the profile name appears in its path; on a default
    Windows temp path the daemon's storage upgrade fails (engine error 1101) once the sandbox path passes MAX_PATH.
    UC_GUI_GO_E2E_SANDBOX_ROOT points the sandbox at a short directory.
    """
    root = os.environ.get('UC_GUI_GO_E2E_SANDBOX_ROOT')
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-', dir=root))
    if len(str(sandbox)) > 30:
        shutil.rmtree(sandbox, ignore_errors=True)
        sys.exit(f'sandbox path {sandbox} is too long for the Engine data tree (MAX_PATH): set UC_GUI_GO_E2E_SANDBOX_ROOT to a short directory such as D:\\w')
    return sandbox, 'gui-go-' + sandbox.name[-6:], Path(root) if root else Path(tempfile.gettempdir())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, default=ROOT / 'target/gui-go/windows-e2e')
    parser.add_argument('--allow-clipboard', action='store_true', help='overwrite the clipboard for the paste check (restored afterwards)')
    args = parser.parse_args()
    if os.name != 'nt':
        sys.exit('this script runs on Windows only')
    if os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('refusing to send key events on a session that is not a dedicated test host (set UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox, profile, sandbox_root = make_sandbox()
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe'):
        shutil.copy2(args.binaries / name, sandbox / name)
    base_env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    gui_env = dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_SHORTCUTS='1',
                   UC_GUI_GO_E2E_DEFAULT_SHORTCUT=F13, UC_GUI_GO_EXIT_MODE='full')
    gui_env.pop('UC_GPUI_QUICK_PANEL', None)
    uniclip = str(sandbox / 'uniclip.exe')
    results = {'sandbox': str(sandbox), 'profile': profile, 'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME')}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    gui = target = release = None
    saved_clip = get_clipboard_text() if args.allow_clipboard else None
    daemon_pid = None
    try:
        subprocess.run([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'win-panel'], env=base_env, check=True, timeout=120)
        target_text = out / 'target-text.txt'
        ps1 = out / 'target.ps1'
        ps1.write_text(TARGET_PS1, encoding='utf-8')
        target = subprocess.Popen(['powershell', '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', str(ps1), str(target_text)])
        time.sleep(4)
        check('paste target window is the foreground window', foreground_pid() == target.pid, {'foreground': foreground_pid(), 'target': target.pid})

        gui = Gui(sandbox, gui_env, out)
        gui.step('bootstrapped', 120)
        state = gui.wait_state('boot', lambda s: s['panelReady'], 60)
        daemon_pid = json.loads((sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn').read_text())['pid']
        check('1 default shortcut registered through Wails', state['recorded'] == [F13] and len(state['wails']) == 1, state)
        # The GUI is a Wails app: make sure the target (not the GUI) still holds the foreground before the chord.
        user32.SetForegroundWindow(user32.FindWindowW(None, 'uc-gui-go-paste-target'))
        time.sleep(.5)

        send_chord('ctrl', 'alt', 'shift', 'f13')
        shown = gui.wait_state('shown', lambda s: s['panelVisible'])
        time.sleep(.6)
        check('2 real key chord shows the panel and the panel takes the foreground', shown['panelVisible'] and foreground_pid() == gui.proc.pid,
              {'panelVisible': shown['panelVisible'], 'foreground': foreground_pid(), 'gui': gui.proc.pid})
        send_chord('ctrl', 'alt', 'shift', 'f13')
        hidden = gui.wait_state('hidden', lambda s: not s['panelVisible'])
        time.sleep(.6)
        check('2 chord again hides the panel and the previous window is foreground again', not hidden['panelVisible'] and foreground_pid() == target.pid,
              {'panelVisible': hidden['panelVisible'], 'foreground': foreground_pid(), 'target': target.pid})

        release = hold_hotkey(F14)
        r = gui.invoke('conflict', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F14}})
        s = gui.state('after-conflict')
        check('3 a shortcut owned by another process is refused as Conflict and the old binding stays',
              not r['ok'] and r['error']['code'] == 'Conflict' and s['recorded'] == [F13] and s['stored'] is None, [r, s])
        release()
        release = None
        r = gui.invoke('rebind', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F14}})
        s = gui.state('after-rebind')
        check('3 the same change succeeds once the other process released it', r['ok'] and s['recorded'] == [F14] and s['stored'] == F14, [r, s])
        r = gui.invoke('restore', 'update_keyboard_shortcuts', {'shortcuts': {KEY: F13}})
        check('3 binding switched back', r['ok'])

        paths = ['C:\\uc-test\\a b.txt', 'C:\\uc-test\\文件🌍.txt']
        user32.SetForegroundWindow(user32.FindWindowW(None, 'uc-gui-go-paste-target'))
        time.sleep(.5)
        send_chord('ctrl', 'alt', 'shift', 'f13')
        gui.wait_state('shown2', lambda s: s['panelVisible'])
        time.sleep(.6)
        clip_before = get_clipboard_text() if args.allow_clipboard else None
        r = gui.invoke('type-paths', 'type_file_paths_to_previous_app', {'request': {'filePaths': paths}})
        time.sleep(1.5)
        typed = target_text.read_text(encoding='utf-8-sig') if target_text.exists() else None
        check('4 file paths typed into the previous window as "\\n"-joined text', r['ok'] and typed is not None and typed.replace('\r\n', '\n') == '\n'.join(paths),
              {'result': r, 'typed': typed})
        check('4 panel hidden and the previous window foreground afterwards', not gui.state('after-type')['panelVisible'] and foreground_pid() == target.pid)
        if args.allow_clipboard:
            check('4 the clipboard was not touched by typing', get_clipboard_text() == clip_before)

        if args.allow_clipboard:
            marker = 'UC-PASTE-MARKER-1234'
            set_clipboard_text(marker)
            user32.SetForegroundWindow(user32.FindWindowW(None, 'uc-gui-go-paste-target'))
            time.sleep(.5)
            send_chord('ctrl', 'alt', 'shift', 'f13')
            gui.wait_state('shown3', lambda s: s['panelVisible'])
            time.sleep(.6)
            r = gui.invoke('paste', 'paste_to_previous_app')
            time.sleep(1.5)
            typed = target_text.read_text(encoding='utf-8-sig')
            check('5 paste_to_previous_app pastes the clipboard into the previous window', r['ok'] and marker in typed, {'result': r, 'typed': typed})
        else:
            checks.append({'check': '5 paste_to_previous_app', 'ok': None, 'skipped': 'run with --allow-clipboard on a dedicated host'})

        before = gui.state('before-failed')['lastShown']
        r = gui.invoke('failed', 'type_file_paths_to_previous_app', {'request': {'filePaths': ['x']}})
        after = gui.wait_state('after-failed', lambda s: s['panelVisible'])  # phase two of the show is asynchronous
        check('6 a paste with no recorded window reports the error and shows the panel again',
              not r['ok'] and 'No previous foreground window' in str(r['error']) and after['lastShown'] != before and after['panelVisible'], [r, after])

        gui.ctl('exit', 'control-exit')
        code = gui.proc.wait(timeout=60)
        deadline = time.monotonic() + 20
        alive = lambda pid: subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH'], capture_output=True, text=True, errors='replace').stdout.find(str(pid)) >= 0
        while alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        check('7 GUI exit 0, daemon stopped by the GUI', code == 0 and not alive(daemon_pid), {'exit': code, 'daemonAlive': alive(daemon_pid)})
        check('7 the shortcut is free after exit', hotkey_is_free(F13))
        results['passed'] = all(c['ok'] is not False for c in checks) and not any(c['ok'] is None for c in checks)
    finally:
        if release:
            release()
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        if target and target.poll() is None:
            target.terminate()
        subprocess.run([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid:  # only the PID recorded in this sandbox's own daemon.conn
            subprocess.run(['taskkill', '/F', '/PID', str(daemon_pid)], capture_output=True)
        if saved_clip is not None:
            set_clipboard_text(saved_clip)
        (out / 'windows-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
        if not results['passed']:  # keep the daemon/host logs and data layout of a failed run as evidence
            try:
                shutil.copytree(sandbox, out / 'sandbox-failed', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
            except OSError as e:  # a file still held by the host must not hide the original failure
                print(f'could not keep the failed sandbox: {e}', file=sys.stderr)
        if sandbox.name.startswith('uc-gui-go-') and sandbox.parent == sandbox_root:
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
