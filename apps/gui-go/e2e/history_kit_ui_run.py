#!/usr/bin/env python3
"""History page built on `@/ui-kit` in the real macOS app: Wails WKWebView -> HTTP/WS -> isolated daemon.

A throwaway HOME and profile get a space (`uniclip space init`, no network) and synthetic entries sent
from the CLI. The E2E app (apps/gui-go/build.sh e2e, which builds with -tags private_mac_apis) opens on
screen and is kept in front: WebKit suspends the page of a window that is off screen or covered,
which stops rendering and timers mid-run. The in-WebView driver (`history-kit` phase of
frontend/src/e2e-driver.ts) drives the page through its real DOM: rows, keyboard selection, copy, pin, multi-select, the delete confirmation, the
sidebar toggle and both themes. Each `shot:<name>` step makes this script capture the app window, and
once over a synthetic striped backdrop (the capture tools of cmd/macos-glass-preview/capture), so the
screenshot shows what the window material samples without exposing other windows.

The system clipboard stays off (UC_DISABLE_SYSTEM_CLIPBOARD=1): Copy is checked up to the daemon's
restore, not the system pasteboard. Needs the Screen Recording permission for the backdrop capture.

  apps/gui-go/build.sh e2e && python3 apps/gui-go/e2e/history_kit_ui_run.py --out <dir>
"""
import argparse
import base64
import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, read_steps  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
CAPTURE = ROOT / 'apps/gui-go/cmd/macos-glass-preview/capture'
CODE = 'docker compose -f compose.prod.yml up -d --build'
LINK = 'https://github.com/UniClipboard/UniClipboard'
TEXTS = ['Q4 roadmap: ship LAN-only mode, then per-device gates', 'Release checklist for the desktop app',
         'Meeting notes from the design review']
SHOT_PAUSE_MS = 9000
IMAGE_SIZE = (640, 400)


def tools(tmp):
    built = {}
    for name in ('Backdrop', 'FrontCheck'):
        out = Path(tmp) / name
        subprocess.run(['swiftc', '-O', str(CAPTURE / f'{name}.swift'), '-o', str(out)], check=True, timeout=300)
        built[name] = out
    out = Path(tmp) / 'window_id'
    subprocess.run(['swiftc', '-O', str(HERE / 'window_id.swift'), '-o', str(out)], check=True, timeout=300)
    built['window_id'] = out
    # Brings the test app (by PID) in front: a covered WKWebView stops rendering.
    source = Path(tmp) / 'activate.swift'
    source.write_text('import AppKit\n'
                      'let pid = pid_t(CommandLine.arguments[1])!\n'
                      'NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateIgnoringOtherApps])\n')
    out = Path(tmp) / 'activate'
    subprocess.run(['swiftc', '-O', str(source), '-o', str(out)], check=True, timeout=300)
    built['activate'] = out
    return built


def window_bounds(wid):
    script = ('import CoreGraphics\nlet l = CGWindowListCopyWindowInfo([.optionIncludingWindow], CGWindowID(%d)) as! [[String: Any]]\n'
              'let b = l[0][kCGWindowBounds as String] as! [String: Double]\n'
              'print(Int(b["X"]!), Int(b["Y"]!), Int(b["Width"]!), Int(b["Height"]!))\n') % wid
    out = subprocess.check_output(['swift', '-'], input=script, text=True, timeout=120)
    return [int(v) for v in out.split()]


def capture(bin_, pid, out, name):
    """The window alone, then the window over the striped backdrop (refused unless only those two are in front)."""
    wid = subprocess.check_output([str(bin_['window_id']), str(pid)], text=True, timeout=30).strip()
    result = {'name': name, 'window': f'{name}.png'}
    subprocess.run(['screencapture', '-x', '-o', '-l', wid, str(out / f'{name}.png')], check=True, timeout=30)
    x, y, w, h = window_bounds(int(wid))
    x, y, w, h = x - 20, y - 20, w + 40, h + 40
    backdrop = subprocess.Popen([str(bin_['Backdrop']), wid, str(x), str(y), str(w), str(h), '5'], stdout=subprocess.DEVNULL)
    time.sleep(2.5)
    front = subprocess.check_output([str(bin_['FrontCheck']), str(x), str(y), str(w), str(h)], text=True).split('\n')
    owners = [line.split(' ', 1)[1] for line in front if ' ' in line][:2]
    if len(owners) == 2 and owners[1] == 'Backdrop':
        subprocess.run(['screencapture', '-x', '-R', f'{x},{y},{w},{h}', str(out / f'{name}-over-backdrop.png')], check=True, timeout=30)
        result['overBackdrop'] = f'{name}-over-backdrop.png'
    else:
        result['overBackdropRefused'] = owners
    backdrop.wait(timeout=30)
    return result


def seed(env):
    for text in [*TEXTS, LINK, CODE]:
        # Text goes through stdin: an argument that looks like a path (a URL) is taken as a file. With no
        # paired device the send exits 1 ("no online peers") after storing the entry; the driver checks
        # that every entry reached the page.
        subprocess.run([str(ROOT / 'target/gui-go/uniclip'), 'send', '--connect-timeout', '0'], env=env,
                       input=text, text=True, capture_output=True, timeout=120)
        time.sleep(1.1)  # distinct copy times keep the list order deterministic


def synthetic_png(width, height):
    """A diagonal gradient, so Fit and 100% look different."""
    rows = b''.join(b'\x00' + bytes(v for x in range(width)
                                    for v in (x * 255 // width, y * 255 // height, 160)) for y in range(height))

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def seed_image(env):
    """An image entry the way a phone sends one: a mobile device registered on this profile uploads a
    PNG to the daemon's mobile gateway over loopback (no pairing, no rendezvous). The gateway's listener
    binds every interface for the length of the run, behind the device's random credentials."""
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    setup = json.loads(subprocess.check_output(
        [str(ROOT / 'target/gui-go/uniclip'), '--json', 'mobile', 'setup', '--non-interactive', '--label', 'e2e-phone',
         '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)], env=env, text=True, timeout=120))
    auth = 'Basic ' + base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode()

    def put(path, body, kind):
        request = urllib.request.Request(f'http://127.0.0.1:{port}{path}', data=body, method='PUT',
                                         headers={'Authorization': auth, 'Content-Type': kind})
        with urllib.request.urlopen(request, timeout=60) as response:
            assert response.status < 300, response.status
    put('/file/photo.png', synthetic_png(*IMAGE_SIZE), 'image/png')
    put('/SyncClipboard.json', json.dumps({'type': 'Image', 'text': 'photo.png', 'dataName': 'photo.png',
                                           'hasData': True}).encode(), 'application/json')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--shots', action='store_true',
                        help='capture the shot:<name> steps, alone and over a striped backdrop (needs Screen Recording)')
    parser.add_argument('--zoom', action='store_true',
                        help='also check 150%% interface scale at the narrowest window; fails today: the macOS host '
                             'magnifies the WKWebView (setMagnification) instead of laying the page out again')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env = isolated_env(home, profile, {'PATH': path})
    evidence = out / 'history-kit-native.jsonl'
    evidence.write_text('')
    secret = json.dumps({'code': CODE, 'link': LINK, 'texts': TEXTS, 'image': {'dimensions': f'{IMAGE_SIZE[0]} × {IMAGE_SIZE[1]}'}, 'shotPauseMs': SHOT_PAUSE_MS if args.shots else 0, 'zoom': args.zoom})
    gui_env = isolated_env(home, profile, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1',
                                           'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'history-kit',
                                           'UC_GUI_GO_E2E_SECRET': secret, 'UC_GUI_GO_EXIT_MODE': 'full',
                                           'UC_GUI_GO_E2E_VISIBLE': '1'})
    results = {'profile': profile, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
               'systemClipboardDisabled': True, 'shots': [], 'passed': False}
    proc = None
    with tempfile.TemporaryDirectory() as tmp:
        try:
            bin_ = tools(tmp)
            cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'Kit Mac')
            cli(env, 'start')
            proc = subprocess.Popen([str(BINARY)], env=gui_env, stdout=(out / 'history-kit-gui.log').open('w'), stderr=subprocess.STDOUT)
            deadline, seen, steps = time.monotonic() + 480, set(), {}
            seeded = False
            last_front = 0.0
            started = time.monotonic()
            while time.monotonic() < deadline and 'history-kit-complete' not in steps:
                for row in read_steps(evidence, 0):
                    if row['step'] == 'driver-error':
                        raise RuntimeError(f"driver error: {row.get('detail')}")
                    steps[row['step']] = row
                    # The first-launch state is checked on an empty history; the entries then arrive
                    # live, as copies would.
                    if row['step'] == 'kit-ready-for-entries' and not seeded:
                        seeded = True
                        seed(env)
                    if row['step'] == 'kit-ready-for-image' and 'image' not in results:
                        seed_image(env)
                        results['image'] = 'uploaded through a loopback mobile device'
                    if args.shots and row['step'].startswith('shot:') and row['step'] not in seen:
                        seen.add(row['step'])
                        subprocess.run([str(bin_['activate']), str(proc.pid)], timeout=30)
                        time.sleep(.5)
                        shot = capture(bin_, proc.pid, out, row['step'][5:])
                        shot['detail'] = row.get('detail')
                        results['shots'].append(shot)
                if proc.poll() is not None:
                    break
                # WebKit suspends a covered page, so keep the test app in front, and record what is in
                # front of its window now and then; a stalled run then names the cause.
                if time.monotonic() - last_front > 3:
                    last_front = time.monotonic()
                    subprocess.run([str(bin_['activate']), str(proc.pid)], timeout=30)
                    if len(results.setdefault('frontWindows', [])) < (time.monotonic() - started) / 15:
                        try:
                            wid = subprocess.check_output([str(bin_['window_id']), str(proc.pid)], text=True, timeout=30).strip()
                            x, y, w, h = window_bounds(int(wid))
                            front = subprocess.check_output([str(bin_['FrontCheck']), str(x), str(y), str(w), str(h)], text=True, timeout=30)
                            results['frontWindows'].append([line.split(' ', 1)[1] for line in front.splitlines() if ' ' in line][:3])
                        except Exception as error:  # noqa: BLE001 - diagnostics only
                            results['frontWindows'].append([f'unavailable: {error}'])
                time.sleep(.2)
            results['steps'] = {name: {'ok': row['ok'], 'detail': row.get('detail')} for name, row in steps.items()}
            failed = [name for name, row in steps.items() if not row['ok']]
            assert 'history-kit-complete' in steps, f'driver did not finish; last steps {sorted(steps)}'
            assert not failed, f'failed steps: {failed}'
            assert proc.wait(timeout=60) == 0
            # A full quit stops the profile's daemon, which removes its connection file.
            conn = Path(home) / 'Library/Application Support' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn'
            deadline = time.monotonic() + 15
            while conn.exists() and time.monotonic() < deadline:
                time.sleep(.2)
            results['daemonStopped'] = not conn.exists()
            results['passed'] = True
        finally:
            if proc and proc.poll() is None:
                proc.terminate()
                proc.wait(timeout=30)
            # Stop the profile's daemon so no task-owned process outlives the run.
            subprocess.run([str(ROOT / 'target/gui-go/uniclip'), 'stop'], env=env, capture_output=True, timeout=60)
            (out / 'history-kit-results.json').write_text(json.dumps(results, indent=2, ensure_ascii=False))
    print(json.dumps({'passed': results['passed'], 'shots': [s['name'] for s in results['shots']]}))
    return 0 if results['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
