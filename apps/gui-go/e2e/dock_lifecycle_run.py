#!/usr/bin/env python3
"""macOS Dock lifecycle E2E (issue #2040): the real activation policy, Dock tile, app menu and focus of the e2e bundle.

Unlike startup_run.py (quiet mode, which only records the requested Dock state), this run uses the visible mode
(UC_GUI_GO_E2E_VISIBLE=1), so the host really switches between the Regular and the Accessory activation policy. What the
system reports is read from outside the process by e2e/dock_ax.swift: NSRunningApplication.activationPolicy, the frontmost app,
NSWorkspace.menuBarOwningApplication (whose app menu the menu bar shows), the pid's AX windows and the Dock's own AX tile list.
The Dock tile is also captured with screencapture and matched against icons/icon.png by normalized cross-correlation
(a blank or wrong tile fails).

Scenarios (one isolated profile; the startup mode is stored by a quiet setup launch, like startup_run.py):
  S  silent start: no main window, Accessory, no Dock tile, the menu bar is not ours;
  O  tray Open, both entries, with real clicks: the menu item (a verified right click on this pid's own status item opens its
     real menu, then a verified left click on its Open item) and a left click on the tray icon; window shown,
     the focused window, app active and frontmost, Regular, Dock tile present and running, the menu bar shows our app menu
     at once, the tile shows the app icon;
  W  Cmd+W (the app menu item whose key equivalent is Cmd+W, pressed through AX: the action the key triggers): window hidden,
     Accessory, no Dock tile, focus and the menu bar go back to another app, the daemon keeps running;
  R  the red close button (AXPress): the same as W;
     O/W/O/R is repeated three times; after every step the host must still answer (control-file state and AX within the
     timeouts), which is how a main-thread deadlock of the policy switch shows up;
  F  reopen while closed (`open -a <bundle>`, the LaunchServices path of a Finder or Spotlight open; refused unless
     LaunchServices lists exactly this pid for the bundle id, and any new process is killed and fails the run): window shown
     with the Dock tile, no second process;
  Q  tray Quit with the window closed: the process exits and the Dock tile is gone;
  S2 a fresh silent start reopened before any window was built (`open -a`): the window is built and shown, Regular;
     tray Quit with the window open;
  N  normal start: window shown and focused, Regular, Dock tile; minimise it and click the Dock tile (AXPress): the window
     comes back; Cmd+W: Accessory; tray Quit.

This run ACTS ON THE REAL SESSION: windows appear on screen and take focus, the Dock changes, the status item menu opens. The
tray menu opens only through a real right click (as tray_tracking_run.py found, AXShowMenu is unsupported on this status item):
tray_ax.swift `clickat ... owner|item` re-verifies that the element under the point is this pid's status item (or an item of
its open menu), moves the pointer there, clicks and puts the pointer back. The tray entries use real clicks rather than AX
presses because an AX press delivers no input event to the app, and macOS (cooperative activation) may then refuse the
activation a click grants. Nothing else moves the pointer, and no key events are sent. Like every script that changes the
foreground window (apps/gui-go/AGENTS.md), run it only on a dedicated test host, or where the user explicitly authorized it. It needs the Accessibility permission (AX)
and Screen Recording (the Dock tile capture) for the terminal, and an awake display. It refuses to start without
--allow-session. Not covered: clipboard sync across devices while the window is closed, Cmd+Tab (it lists exactly the Regular
apps, which the policy check covers), the Dock tile's look beyond the icon comparison.
"""
import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from startup_run import BINARY, PASSPHRASE, Profile, cli, read_steps, set_mode  # noqa: E402

BUNDLE = BINARY.parents[2]
BUNDLE_ID = 'app.uniclipboard.desktop.e2e'
GUI_DIR = HERE.parent
MAIN_TITLE = 'UniClipboard'
REGULAR, ACCESSORY = 0, 1


def tray_labels():
    """Open and Quit in every tray language, from tray_labels.go (the single source)."""
    rows = re.findall(r'^\s*"[^"]+":\s*\{([^}]*)\}', (GUI_DIR / 'tray_labels.go').read_text(), re.M)
    cols = [re.findall(r'"([^"]*)"', r) for r in rows]
    return {c[0] for c in cols}, {c[5] for c in cols}


OPEN_LABELS, QUIT_LABELS = tray_labels()


class Run:
    def __init__(self, out):
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
        return self.tool('dock_ax', 'state', pid, BUNDLE)

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


def main_window(st):
    return next((w for w in st.get('windows', []) if w.get('title') == MAIN_TITLE), None)


def shown(pid):
    def pred(st):
        w = main_window(st)
        return bool(w and not w['minimized'] and st['policy'] == REGULAR and st['dockTile']['present'])
    return pred


def active(pid):
    def pred(st):
        return shown(pid)(st) and st['frontmostPid'] == pid and st['menuBarOwnerPid'] == pid and st.get('focusedWindow') == MAIN_TITLE
    return pred


def hidden(pid):
    def pred(st):
        return (main_window(st) is None and st['policy'] == ACCESSORY and not st['dockTile']['present']
                and st['frontmostPid'] != pid and st['menuBarOwnerPid'] != pid)
    return pred


class Gui:
    """One visible launch of the e2e bundle, driven through its control file."""

    def __init__(self, run, profile, label):
        self.run, self.label = run, label
        self.control = run.out / f'{label}.control'
        self.control.write_text('')
        visible = dict(profile.base)
        profile.base = dict(visible, UC_GUI_GO_E2E_VISIBLE='1')
        try:
            self.proc, self.evidence, self.log = profile.launch(label, 'wake', exit_mode='full', wait=False, control=self.control)
        finally:
            profile.base = visible
        self.pid = self.proc.pid
        self.sent = 0

    def wait_bootstrapped(self, timeout=120):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if any(r['step'] == 'bootstrapped' for r in read_steps(self.evidence, 0)):
                return True
            if self.proc.poll() is not None:
                return False
            time.sleep(.3)
        return False

    def control_state(self, tag, timeout=10):
        """A control-file state round trip; the host answers it on a goroutine that reads the main window (main thread)."""
        before = sum(1 for r in read_steps(self.evidence, 0) if r['step'] == 'control-state')
        with self.control.open('a') as f:
            f.write(f'state {tag}\n')
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(self.evidence, 0) if r['step'] == 'control-state']
            if len(rows) > before:
                return rows[-1]['detail']
            time.sleep(.2)
        return None

    def alive(self, what):
        """The host still answers: the control file (main-thread window reads) and AX (its run loop)."""
        detail = self.control_state(what)
        st = self.run.state(self.pid)
        return self.run.check(f'{what}: the host still answers (control state and AX, no main-thread deadlock)',
                              detail is not None and st.get('windowsAxError') == 0 and self.proc.poll() is None,
                              {'control': detail, 'axError': st.get('windowsAxError'), 'exit': self.proc.poll()})

    def wait_exit(self, timeout=60):
        try:
            return self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return None

    def kill(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.proc.kill()


def expect_shown(run, gui, what, tile_label):
    """The window is shown with the Dock tile (gates the rest of the scenario), then, checked separately, the app is activated:
    frontmost, our app menu in the menu bar (within 2 s) and the main window focused."""
    st, waited = run.wait_state(gui.pid, shown(gui.pid))
    ok = run.check(f'{what}: window shown, Regular, Dock tile present', waited is not None, {'waitedS': waited, 'state': st})
    if ok:
        act, act_waited = run.wait_state(gui.pid, active(gui.pid), timeout=3)
        run.check(f'{what}: the app is activated: frontmost, our menu bar within 2 s, the main window focused',
                  act_waited is not None and act_waited <= 2,
                  {'waitedS': act_waited, 'frontmostPid': act['frontmostPid'], 'menuBarOwnerPid': act['menuBarOwnerPid'],
                   'focusedWindow': act.get('focusedWindow'), 'active': act.get('active'), 'pid': gui.pid})
        # The Dock animates a tile that appears; capture once its frame read the same twice in a row.
        prev = None
        for _ in range(30):
            st = run.state(gui.pid)
            frame = st['dockTile'].get('frame')
            if frame and frame == prev:
                break
            prev = frame
            time.sleep(.2)
        cap = run.capture_tile(st, tile_label)
        run.check(f'{what}: the Dock tile shows the app icon (not a blank tile)', cap['ncc'] >= .85, cap)
        run.check(f'{what}: the Dock tile shows the running indicator', st['dockTile'].get('running'), st['dockTile'])
    gui.alive(what)
    return ok


def close_key(run, gui):
    return run.tool('dock_ax', 'menu-key', gui.pid, 'W')


def reopen(run, gui, what):
    """`open -a` the bundle, only when the GUI executable of its bundle id runs as this pid alone (the bundle's daemon is listed
    under the same id, as in a real install); a new GUI process fails the check and is killed."""
    listed = run.tool('dock_ax', 'instances', BUNDLE_ID).get('instances', [])
    guis = [i['pid'] for i in listed if Path(i.get('executable', '')).resolve() == BINARY.resolve()]
    if not run.check(f'{what}: LaunchServices lists exactly this pid as the GUI of the bundle id', guis == [gui.pid], listed):
        return False
    before = gui_processes()
    r = subprocess.run(['open', '-a', str(BUNDLE)], capture_output=True, text=True, timeout=30)
    time.sleep(1.5)
    extra = [p for p in gui_processes() if p not in before]
    for p in extra:
        subprocess.run(['kill', str(p)])
    run.check(f'{what}: open -a reused the running process (no second GUI)', r.returncode == 0 and not extra, {'rc': r.returncode, 'stderr': r.stderr[-200:], 'new': extra})
    return not extra


def expect_hidden(run, gui, profile, what):
    st, waited = run.wait_state(gui.pid, hidden(gui.pid))
    run.check(f'{what}: window hidden, Accessory, no Dock tile, focus and menu bar returned to another app', waited is not None,
              {'waitedS': waited, 'state': st})
    run.check(f'{what}: the daemon keeps running', profile.daemon_alive())
    gui.alive(what)


def silent_cycle(run, profile):
    gui = Gui(run, profile, 'dock-silent')
    try:
        if not run.check('S: the silent launch finished its bootstrap', gui.wait_bootstrapped()):
            return
        st, waited = run.wait_state(gui.pid, lambda s: s['policy'] == ACCESSORY and not s['dockTile']['present'] and main_window(s) is None)
        run.check('S: silent start: no window, Accessory, no Dock tile', waited is not None, {'waitedS': waited, 'state': st})
        run.check('S: silent start: the menu bar is not ours', st and st['menuBarOwnerPid'] != gui.pid, st and st['menuBarOwnerPid'])
        gui.alive('S')
        for cycle in (1, 2, 3):
            # A close is only meaningful on a window that was shown: a failed open stops the run.
            if not (run.tray_press(gui.pid, OPEN_LABELS, f'O{cycle} tray Open') and expect_shown(run, gui, f'O{cycle} tray Open', f'open{cycle}')):
                return
            run.check(f'W{cycle} Cmd+W menu item pressed', close_key(run, gui).get('ok'))
            expect_hidden(run, gui, profile, f'W{cycle} Cmd+W')
            if not (run.tray_click(gui.pid, f'O{cycle}b tray icon click') and expect_shown(run, gui, f'O{cycle}b tray icon click', f'click{cycle}')):
                return
            run.check(f'R{cycle} red close button pressed', run.tool('dock_ax', 'close-button', gui.pid, MAIN_TITLE).get('ok'))
            expect_hidden(run, gui, profile, f'R{cycle} red button')
        if not (reopen(run, gui, 'F reopen while closed') and expect_shown(run, gui, 'F reopen while closed', 'reopen')):
            return
        close_key(run, gui)
        expect_hidden(run, gui, profile, 'F close again')
        if run.tray_press(gui.pid, QUIT_LABELS, 'Q tray Quit (window closed)'):
            code = gui.wait_exit()
            run.check('Q the process exits after Quit', code is not None, code)
            st, waited = run.wait_state(gui.pid, lambda s: not s['dockTile']['present'])  # the Dock animates the removal
            run.check('Q no Dock tile remains', waited is not None, {'waitedS': waited, 'tile': st['dockTile']})
    finally:
        gui.kill()

    gui = Gui(run, profile, 'dock-silent-reopen')
    try:
        if not run.check('S2: the silent launch finished its bootstrap', gui.wait_bootstrapped()):
            return
        run.wait_state(gui.pid, lambda s: s['policy'] == ACCESSORY)
        if not (reopen(run, gui, 'S2 reopen after silent start') and expect_shown(run, gui, 'S2 reopen after silent start', 'silent-reopen')):
            return
        if run.tray_press(gui.pid, QUIT_LABELS, 'S2 tray Quit (window open)'):
            code = gui.wait_exit()
            run.check('S2 the process exits after Quit', code is not None, code)
            st, waited = run.wait_state(gui.pid, lambda s: not s['dockTile']['present'])  # the Dock animates the removal
            run.check('S2 no Dock tile remains', waited is not None, {'waitedS': waited, 'tile': st['dockTile']})
    finally:
        gui.kill()


def normal_cycle(run, profile):
    gui = Gui(run, profile, 'dock-normal')
    try:
        if not run.check('N: the normal launch finished its bootstrap', gui.wait_bootstrapped()):
            return
        if not expect_shown(run, gui, 'N normal start', 'normal'):
            return
        run.check('N minimise the main window (AX)', run.tool('dock_ax', 'minimize', gui.pid, MAIN_TITLE).get('ok'))
        st, waited = run.wait_state(gui.pid, lambda s: (main_window(s) or {}).get('minimized'))
        run.check('N the window is minimised and the Dock tile stays', waited is not None and st['dockTile']['present'], st)
        run.check('N Dock tile click (AXPress)', run.tool('dock_ax', 'dock-press', BUNDLE).get('ok'))
        expect_shown(run, gui, 'N Dock click restores the window', 'dock-click')
        close_key(run, gui)
        expect_hidden(run, gui, profile, 'N Cmd+W')
        if run.tray_press(gui.pid, QUIT_LABELS, 'N tray Quit'):
            run.check('N the process exits after Quit', gui.wait_exit() is not None)
    finally:
        gui.kill()


def gui_processes():
    r = subprocess.run(['pgrep', '-f', str(BINARY)], capture_output=True, text=True)
    return sorted(int(x) for x in r.stdout.split())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--allow-session', action='store_true', help='required: the run shows windows, takes focus and changes the Dock')
    args = parser.parse_args()
    if not args.allow_session:
        sys.exit('refusing to run: this E2E acts on the real session (windows, focus, Dock); pass --allow-session')
    if sys.platform != 'darwin':
        sys.exit('macOS only')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    run = Run(out)
    results = {'bundle': str(BUNDLE), 'binarySha256': hashlib.sha256(BINARY.read_bytes()).hexdigest(), 'passed': False}
    results['tools'] = run.build_tools()
    display = subprocess.run([str(run.tools / 'tray_ax'), 'display', '0'], capture_output=True, text=True).stdout.strip()
    results['display'] = display
    if gui_processes():
        sys.exit(f'another e2e GUI of this bundle is running: {gui_processes()}')
    p = Profile(out)
    results.update(home=p.home, profile=p.profile)
    try:
        cli(p.cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'dock-a')
        for _ in range(3):
            if p.daemon_alive():
                break
            time.sleep(3)
        set_mode(p, 'silent')
        silent_cycle(run, p)
        set_mode(p, 'normal')
        normal_cycle(run, p)
        results['passed'] = bool(run.checks) and all(c['ok'] for c in run.checks)
    finally:
        cli(p.cli_env, '--json', 'stop', check=False, timeout=80)
        results['checks'] = run.checks
        (out / 'dock-lifecycle-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    failed = [c['check'] for c in run.checks if not c['ok']]
    print(json.dumps({'passed': results['passed'], 'checks': len(run.checks), 'failed': failed}, indent=2, ensure_ascii=False))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
