#!/usr/bin/env python3
"""Native Dock/tray observers and compatibility entry for ui_process_run.py.

The previous single-process lifecycle scenario was replaced by the UI-process scenario.
The entry still requires --allow-session. Importing these observers starts no application.
"""
import json
import re
import subprocess
import time
from pathlib import Path

from startup_run import BINARY

BUNDLE = BINARY.parents[2]
GUI_DIR = Path(__file__).resolve().parent.parent


def tray_labels():
    rows = re.findall(r'^\s*"[^\"]+":\s*\{([^}]*)\}', (GUI_DIR / 'tray_labels.go').read_text(), re.M)
    cols = [re.findall(r'"([^\"]*)"', row) for row in rows]
    return {c[0] for c in cols}, {c[5] for c in cols}, {c[4] for c in cols}


OPEN_LABELS, QUIT_LABELS, LIGHTWEIGHT_LABELS = tray_labels()


class Run:
    def __init__(self, out, bundle=BUNDLE):
        self.bundle = bundle
        self.out = out
        self.tools = out / 'tools'
        self.tools.mkdir(parents=True, exist_ok=True)
        self.checks = []
        self.shots = 0
        self.icon = None

    def build_tools(self):
        info = {}
        for name in ('dock_ax', 'tray_ax'):
            src = HERE / f'{name}.swift'
            subprocess.run(['swiftc', '-O', str(src), '-o', str(self.tools / name)], check=True)
            info[name] = {'sourceSha256': hashlib.sha256(src.read_bytes()).hexdigest(),
                          'binarySha256': hashlib.sha256((self.tools / name).read_bytes()).hexdigest()}
        return info

    def tool(self, name, *args, timeout=20):
        try:
            p = subprocess.run([str(self.tools / name), *map(str, args)], capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return {'ok': False, 'error': f'{name} timeout after {timeout}s: {args[:2]}'}
        try:
            return json.loads(p.stdout.strip().splitlines()[-1])
        except (ValueError, IndexError):
            return {'ok': False, 'error': 'unparsable output', 'stdout': p.stdout[-300:], 'stderr': p.stderr[-300:]}

    def check(self, name, ok, detail=None):
        self.checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)
        return ok

    def state(self, pid):
        return self.tool('dock_ax', 'state', pid, self.bundle)

    def wait_state(self, pid, pred, timeout=5.0):
        """Poll the native state until pred holds; returns (state, seconds waited) or the last state and None."""
        start = time.monotonic()
        st = None
        while time.monotonic() - start < timeout:
            st = self.state(pid)
            if pred(st):
                return st, round(time.monotonic() - start, 2)
            time.sleep(.1)
        return st, None

    def capture_tile(self, st, label):
        """Capture the Dock tile and match its icon square against icons/icon.png: the best normalized cross-correlation over a
        small range of scales and offsets (the Dock insets the icon in its tile). A blank tile has no structure and scores ~0."""
        from PIL import Image
        x, y, w, h = st['dockTile']['frame']
        self.shots += 1
        shot = self.out / f'dock-tile-{self.shots:02d}-{label}.png'
        subprocess.run(['screencapture', '-x', '-R', f'{x},{y},{w},{h}', str(shot)], check=True)
        img = Image.open(shot).convert('L')
        side = min(img.width, img.height)
        n = 64
        tile = list(img.crop(((img.width - side) // 2, 0, (img.width + side) // 2, side)).resize((n, n)).getdata())
        if self.icon is None:
            self.icon = Image.open(GUI_DIR / 'icons/icon.png').convert('L')
        best = (-1.0, None)
        for scale in [s / 100 for s in range(60, 101, 3)]:
            size = round(n * scale)
            ref = list(self.icon.resize((size, size)).getdata())
            for dy in range(0, n - size + 1, 2):
                for dx in range(0, n - size + 1, 2):
                    part = [tile[(dy + r) * n + dx + c] for r in range(size) for c in range(size)]
                    score = ncc(part, ref)
                    if score > best[0]:
                        best = (score, {'scale': scale, 'dx': dx, 'dy': dy})
        return {'shot': shot.name, 'ncc': round(best[0], 3), 'at': best[1]}

    def status_center(self, pid):
        frame = ((self.tool('tray_ax', 'describe', pid).get('items') or [{}])[0]).get('frame') or {}
        return frame and (frame['x'] + frame['w'] / 2, frame['y'] + frame['h'] / 2)

    def tray_press(self, pid, labels, what):
        """Open the tray menu with a verified right click on this pid's status item, then click the item with one of the labels."""
        center = self.status_center(pid)
        clicked = self.tool('tray_ax', 'clickat', pid, *center, 'right', 'owner') if center else {'ok': False, 'error': 'no status item frame'}
        menu = None
        for _ in range(30):
            menu = self.tool('tray_ax', 'read', pid)
            if menu.get('ok') and menu.get('popupWindows'):
                break
            time.sleep(.1)
        items = (menu or {}).get('menu', [])
        item = next((i for i in items if i.get('title') in labels), None)
        if not self.check(f'{what}: the tray menu opened (verified right click) and has the item', clicked.get('ok') and item and item.get('frame'),
                          {'click': clicked, 'titles': [i.get('title') for i in items]}):
            self.tool('tray_ax', 'cancel', pid)
            return False
        f = item['frame']
        r = self.tool('tray_ax', 'clickat', pid, f['x'] + f['w'] / 2, f['y'] + f['h'] / 2, 'left', 'item')
        return self.check(f'{what}: clicked "{item["title"]}" (verified left click)', r.get('ok'), r)

    def tray_click(self, pid, what):
        """A verified left click on this pid's tray icon (the icon's own click handler shows the main window)."""
        center = self.status_center(pid)
        r = self.tool('tray_ax', 'clickat', pid, *center, 'left', 'owner') if center else {'ok': False, 'error': 'no status item frame'}
        return self.check(f'{what}: verified left click on the tray icon', r.get('ok'), r)

def ncc(a, b):
    ma, mb = sum(a) / len(a), sum(b) / len(b)
    da, db = [v - ma for v in a], [v - mb for v in b]
    den = (sum(v * v for v in da) * sum(v * v for v in db)) ** .5
    return sum(p * q for p, q in zip(da, db)) / den if den else 0.0



if __name__ == '__main__':
    from ui_process_run import main
    main()
