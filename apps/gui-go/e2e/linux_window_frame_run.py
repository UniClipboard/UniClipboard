#!/usr/bin/env python3
"""Main-window frame E2E for the Linux Go host (needs the gtk3,e2e GUI build; the official package has no control plane).

  linux_window_frame_run.py --appimage X.AppImage --out DIR [--expect fixed|unfixed]

The page owns the frame preference and tells the host through `set_window_decorations` (the Tauri `setDecorations` shim). With the
default preference the page draws its own window controls, so the toolkit must NOT also draw a title bar. The run starts the app in
portable mode (task-owned HOME, private session bus, system clipboard disabled), waits for the page, then reads GTK's own
`decorated` claim for the main window through the control verb `window-frame-state`:

  F1 after the page bootstrapped with the default preference the main window is not decorated (fails on a build whose shim is a no-op)
  F2 the page's own request `set_window_decorations {decorations:true}` turns the system frame on
  F3 `{decorations:false}` turns it off again

--expect fixed (default) exits 0 only when F1-F3 pass; --expect unfixed exits 0 only when F1 fails (the before-fix control).
Output: DIR/window-frame-result.json. A value that could not be read is recorded as unknown, never as a pass.
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--expect', choices=('fixed', 'unfixed'), default='fixed')
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = out / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}')
    wayland = next((p.name for p in sorted(runtime.glob('wayland-[0-9]*')) if not p.name.endswith('.lock')), None)
    env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|UC_|UNICLIPBOARD|GIO_|APPIMAGE|APPDIR|GDK_BACKEND|DBUS_SESSION|XDG_SESSION_TYPE', k)}
    env.update(XDG_RUNTIME_DIR=str(runtime), UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake',
               UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET='window-frame-run', UC_GUI_GO_EVIDENCE=str(out / 'gui.jsonl'), UC_GUI_GO_E2E_CONTROL_FILE=str(out / 'gui.control'))
    if wayland:
        env.update(WAYLAND_DISPLAY=wayland, XDG_SESSION_TYPE='wayland')
    elif os.environ.get('DISPLAY'):
        env['DISPLAY'] = os.environ['DISPLAY']
    else:
        sys.exit('no Wayland socket and no DISPLAY: a graphical session is required')
    if not any(Path(d).glob('libfuse.so.2') for d in ('/usr/lib', '/usr/lib64', '/usr/lib/aarch64-linux-gnu', '/lib64')):
        env['APPIMAGE_EXTRACT_AND_RUN'] = '1'
    for f in ('gui.jsonl', 'gui.control'):
        (out / f).write_text('')
    (Path(str(app) + '.home')).mkdir(exist_ok=True)  # portable mode: the home sits next to the copy (creating it does not start the app)
    cfg = out / 'private-bus.conf'
    cfg.write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">\n<busconfig><type>session</type>'
                   f'<listen>unix:path={out}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>\n')
    with (out / 'private-bus.log').open('w') as bus_log:
        bus = subprocess.Popen(['dbus-daemon', '--config-file', str(cfg), '--nofork'], stdout=bus_log, stderr=subprocess.STDOUT)

    def stop_bus():
        bus.terminate()
        try:
            bus.wait(10)
        except subprocess.TimeoutExpired:
            bus.kill()
            bus.wait()

    for _ in range(50):
        if (out / 'bus.sock').exists():
            break
        time.sleep(.1)
    else:
        stop_bus()
        sys.exit('the private session bus did not create its socket')
    env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={out}/bus.sock'
    try:
        with (out / 'gui.log').open('w') as gui_log:
            proc = subprocess.Popen([str(app)], env=env, cwd=str(out), stdout=gui_log, stderr=subprocess.STDOUT)
    except OSError:
        stop_bus()
        raise
    checks, seq = [], [0]

    def step(name, timeout):
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

    def verb(line, name, timeout=30):
        seq[0] += 1
        label = str(seq[0])
        with (out / 'gui.control').open('a') as f:
            f.write(line.replace('<n>', label) + '\n')
        return step(name.replace('<n>', label), timeout)

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': ok, 'detail': detail})
        print(('PASS ' if ok else 'UNKNOWN ' if ok is None else 'FAIL ') + name, flush=True)

    def decorated():
        r = verb('window-frame-state <n> main', 'window-frame-state-<n>')
        d = (r or {}).get('detail') or {}
        return (d.get('decorated') if d.get('exists') else None), d

    try:
        boot = step('bootstrapped', 120)
        check('the page ran (evidence step bootstrapped)', bool(boot and boot.get('ok')), boot)
        time.sleep(3)  # the page applies its frame preference right after the first render
        value, detail = decorated()
        check('F1 default preference: the main window is not decorated', None if value is None else value is False, detail)
        r = verb('invoke <n> set_window_decorations {"decorations":true}', 'invoke-<n>')
        time.sleep(1)
        value, detail = decorated()
        check('F2 set_window_decorations true: the main window is decorated', None if value is None else value is True, {'invoke': r, 'state': detail})
        r = verb('invoke <n> set_window_decorations {"decorations":false}', 'invoke-<n>')
        time.sleep(1)
        value, detail = decorated()
        check('F3 set_window_decorations false: the main window is not decorated', None if value is None else value is False, {'invoke': r, 'state': detail})
        verb('exit <n>', 'control-exit', 15)
        try:
            proc.wait(20)
        except subprocess.TimeoutExpired:
            proc.kill()
    finally:
        if proc.poll() is None:
            proc.kill()
        stop_bus()
    f1 = next((c for c in checks if c['check'].startswith('F1')), {})
    ok = all(c['ok'] is True for c in checks) if args.expect == 'fixed' else f1.get('ok') is False
    (out / 'window-frame-result.json').write_text(json.dumps({'expect': args.expect, 'ok': ok, 'checks': checks, 'appimage': str(args.appimage)}, indent=2) + '\n')
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
