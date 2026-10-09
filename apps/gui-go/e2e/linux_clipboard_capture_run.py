#!/usr/bin/env python3
"""Clipboard capture on a real Linux graphical session: disabled/enabled control with a click-driven synthetic source.

  linux_clipboard_capture_run.py --appimage X.AppImage --seed-home DIR --out DIR --mode enabled|disabled [--click-wait 60]

--seed-home is a portable home of a synthetic, already set-up test profile (never a real profile); it is COPIED, so every run starts from
the same state. The run starts the e2e GUI in portable mode with a private session bus; `disabled` sets UC_DISABLE_SYSTEM_CLIPBOARD=1
(the daemon must not read the system clipboard), `enabled` does not. It then starts linux/clipboard_source_button.py, a window whose
button sets the clipboard from a click handler (a Wayland compositor ignores a selection set without a valid input serial, so a
program-only set is not a test). The click is real input from the driver (a person or the UTM window) and is awaited for --click-wait s.
Evidence: the daemon's own logged clipboard backend choice, the source's `set` line, and the daemon's /clipboard/entries after the click.
Expected: enabled -> the synthetic text is listed; disabled -> it is not. The daemon token is read from daemon.conn and never printed.
Exit 0 only when the expectation of the chosen mode holds. Output: DIR/clipboard-capture-result.json.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
TEXT = 't0221-synthetic-text-A'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--appimage', type=Path, required=True)
    ap.add_argument('--seed-home', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--mode', choices=('enabled', 'disabled'), required=True)
    ap.add_argument('--click-wait', type=int, default=60)
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = out / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, app)
    home = Path(str(app) + '.home')
    shutil.copytree(args.seed_home, home, symlinks=True)
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}')
    wayland = next((p.name for p in sorted(runtime.glob('wayland-[0-9]*')) if not p.name.endswith('.lock')), None)
    if not wayland:
        sys.exit('no Wayland socket: a graphical session is required')
    env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|UC_|UNICLIPBOARD|GIO_|APPIMAGE|APPDIR|GDK_BACKEND|DBUS_SESSION|XDG_SESSION_TYPE', k)}
    env.update(XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY=wayland, XDG_SESSION_TYPE='wayland', NO_COLOR='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake',
               UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET='clipboard-capture-run', UC_GUI_GO_EVIDENCE=str(out / 'gui.jsonl'), UC_GUI_GO_E2E_CONTROL_FILE=str(out / 'gui.control'),
               APPIMAGE_EXTRACT_AND_RUN='1')
    if args.mode == 'disabled':
        env['UC_DISABLE_SYSTEM_CLIPBOARD'] = '1'
    for f in ('gui.jsonl', 'gui.control'):
        (out / f).write_text('')  # an inherited control file would replay old commands
    cfg = out / 'private-bus.conf'
    cfg.write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">\n<busconfig><type>session</type>'
                   f'<listen>unix:path={out}/bus.sock</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>\n')
    bus = subprocess.Popen(['dbus-daemon', '--config-file', str(cfg), '--nofork'], stdout=(out / 'private-bus.log').open('w'), stderr=subprocess.STDOUT)
    for _ in range(50):
        if (out / 'bus.sock').exists():
            break
        time.sleep(.1)
    env['DBUS_SESSION_BUS_ADDRESS'] = f'unix:path={out}/bus.sock'
    gui = subprocess.Popen([str(app)], env=env, cwd=str(out), stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT)
    source = None
    result = {'mode': args.mode, 'text': TEXT, 'checks': []}

    def check(name, ok, detail=None):
        result['checks'].append({'check': name, 'ok': ok, 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def entries():
        conn = next((json.loads(p.read_text()) for p in home.rglob('daemon.conn')), None)
        if not conn:
            return None
        req = urllib.request.Request(f"http://{conn['host']}:{conn['port']}/clipboard/entries?limit=50", headers={'Authorization': 'Bearer ' + conn['token']})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.read().decode()
        except Exception as e:  # noqa: BLE001 - recorded, not hidden
            return f'error: {e}'

    try:
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline and 'bootstrapped' not in (out / 'gui.jsonl').read_text():
            time.sleep(1)
        check('the page ran (evidence step bootstrapped)', 'bootstrapped' in (out / 'gui.jsonl').read_text())
        time.sleep(8)
        before = entries()
        result['entriesBeforeHaveText'] = bool(before and TEXT in before)
        source = subprocess.Popen([sys.executable, str(HERE / 'linux/clipboard_source_button.py'), str(args.click_wait + 20), TEXT], env=env,
                                  stdout=(out / 'source.out').open('w'), stderr=subprocess.STDOUT)
        print(f'CLICK the "copy synthetic text" window within {args.click_wait} s', flush=True)
        end = time.monotonic() + args.click_wait
        clicked = False
        while time.monotonic() < end:
            if ' set ' in (out / 'source.out').read_text():
                clicked = True
                break
            time.sleep(1)
        check('the source was clicked (it set the selection from its handler)', clicked, (out / 'source.out').read_text()[-200:])
        time.sleep(6)
        after = entries()
        listed = bool(after and TEXT in after)
        check('capture enabled: the synthetic text is listed' if args.mode == 'enabled' else 'capture disabled: the synthetic text is NOT listed',
              listed if args.mode == 'enabled' else (after is not None and not after.startswith('error') and not listed), None if after is None else after[:80])
        logs = ''.join(p.read_text(errors='replace') for p in (home / 'data/logs').glob('uniclipboard-daemon.json.*')) if (home / 'data/logs').is_dir() else ''
        backend = re.findall(r'"message":"(Linux clipboard[^"]*)"', logs)
        result['backendLogLines'] = backend[-4:]
        result['dataControlProbe'] = re.findall(r'wayland data-control protocol probe[^}]*', logs)[-1:]
    finally:
        for p in (source, gui):
            if p and p.poll() is None:
                p.terminate()
        time.sleep(2)
        subprocess.run(['pkill', '-f', str(out) + '/'], check=False)  # only processes whose command line names this run directory
        bus.terminate()
    ok = all(c['ok'] for c in result['checks'])
    result['ok'] = ok
    (out / 'clipboard-capture-result.json').write_text(json.dumps(result, indent=2) + '\n')
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
