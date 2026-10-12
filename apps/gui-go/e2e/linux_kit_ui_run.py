#!/usr/bin/env python3
"""Real-GUI E2E of the Linux UI-kit build: WebKitGTK -> in-WebView driver -> isolated daemon.

  linux_kit_ui_run.py --out DIR --binaries DIR [--session sway|xvfb|existing] [--shot-pause-ms 1500]

DIR/binaries holds `gui-go` (built with tags gtk3,e2e from a frontend built with VITE_GUI_GO_E2E=1 UC_UI_PLATFORM=linux),
`uniclipd` and `uniclip`. Everything runs in a throwaway sandbox: UC_PORTABLE=1 (data root, caches and the file keystore inside
it), a unique gui-go-* UC_PROFILE, UC_GUI_GO_ISOLATED=1, HOME/XDG_* in the sandbox, UC_DISABLE_SYSTEM_CLIPBOARD=1 and a private
D-Bus session bus. Entries are synthetic (`uniclip send`, a loopback mobile device); the real clipboard and profile are never used.

Stages, each a launch of the GUI on the SAME profile (full quit between them), each driven by frontend/src/e2e-driver.ts:
  A entry        no phase: a fresh profile; the setup entry pages create the space in the GUI, then Devices/Settings navigation
  B history-kit  unlock (keyring) and the whole history page: first launch, rows, keyboard, copy, pin, tags, multi-select, context
                 menu, delete, grid, narrow window, search/filters, image detail, themes + Settings write-back, paging
  C restart      phase unlock-restart: the entry left by B is still in the history after a quit and a new daemon
  D resize       phase wake: the runner resizes the window from outside (swaymsg / xdotool) and reads GTK's size, then quits
Result: DIR/linux-kit-ui-result.json, one row per check with status passed | failed | not_run and the reason. A step the driver never
reached because it stopped earlier is not_run (the first one it stopped at is failed). Screenshots: DIR/screenshots/*.png
(grim on wlroots, gnome-screenshot/spectacle on KDE/GNOME, `import` on X11), synthetic data only.

Sessions
  sway      start a headless sway (wlroots, pixman) in the sandbox; windows float so the in-page Window.SetSize works
  xvfb      start a private Xvfb with no window manager
  existing  use the running desktop (WAYLAND_DISPLAY or DISPLAY, XDG_RUNTIME_DIR from the environment)

Docker (macOS or Linux host, linux/arm64 or amd64; builds the bundle, the binaries and runs this in a sway container):
  apps/gui-go/e2e/linux/run.sh kit-ui <outdir>        # UC_DOCKER_CPUS=4 caps the containers

Fedora VM with a native GNOME session (aarch64 or x86_64), over ssh:
  sudo dnf install gtk3-devel webkit2gtk4.1-devel libX11-devel dbus-devel openssl-devel gcc pkgconf-pkg-config \
       python3 dbus-daemon xdotool ImageMagick gnome-screenshot golang rust cargo
  # in the repo; the frontend needs bun (or npm) and the bundle must be built for the Linux skin:
  VITE_GUI_GO_E2E=1 UC_UI_PLATFORM=linux bun --bun run --cwd apps/gui-go/frontend build
  (cd packages/desktop-host-go && go generate ./buildinfo)
  cargo build -p uc-daemon && mkdir -p /tmp/kit-bin && cp target/debug/uniclipd /tmp/kit-bin/
  (cd apps/cli-go && go build -o /tmp/kit-bin/uniclip ./cmd/uniclip)
  (cd apps/gui-go && CGO_ENABLED=1 go build -tags gtk3,e2e -ldflags "-X main.updaterPublicKey=" -o /tmp/kit-bin/gui-go .)
  # an ssh shell has no desktop: point it at the GNOME session that is logged in on the VM (Wayland socket is usually wayland-0)
  export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
  python3 apps/gui-go/e2e/linux_kit_ui_run.py --session existing --binaries /tmp/kit-bin --out ~/kit-ui-out
  The session must be unlocked and the screen on (GNOME suspends a locked session's windows). GNOME has no window-resize tool for
  other clients: stage D's external resize is reported not_run there (the in-page resize of stage B still runs); an X11 GNOME session
  (DISPLAY set, WAYLAND_DISPLAY unset) resizes through xdotool. The private D-Bus bus hides the Secret Service from the app on
  purpose (portable mode uses its file keystore); screenshots use the desktop's own bus.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from history_kit_ui_run import CODE, IMAGE_SIZE, LINK, TEXTS, synthetic_png  # noqa: E402
from linux_xvfb_run import Gui, pid_alive, read_steps  # noqa: E402

ROOT = HERE.parents[2]
PASSPHRASE = 'gui-go-synthetic-passphrase'  # the one the driver's default scenario sets in the setup flow
PASS, FAIL, NOT_RUN = 'passed', 'failed', 'not_run'

# (id, spec item, stage, driver steps that must all be ok), in the order the driver reaches them.
CHECKS = [
    ('entry-first-screen', 'enter GUI: first screen', 'entry', ['shared-app-mounted', 'ws-status-snapshot', 'first-screen']),
    ('entry-setup', 'enter GUI: setup entry pages create the space', 'entry', ['setup-created-in-gui']),
    ('entry-home', 'enter GUI: history route with the sidebar shell', 'entry', ['home']),
    ('entry-devices-settings', 'nav: Devices panel, Settings page, back', 'entry', ['devices', 'settings', 'navigate-back']),
    ('entry-webview-alive', 'WebView alive after hide/show', 'entry', ['webview-alive-after-reopen']),
    ('entry-complete', 'default scenario ran to its end (updater/quick panel/tray windows included)', 'entry', ['file-preview-refusals', 'driver-complete']),
    ('history-first-launch', 'history: first-launch state on an empty profile', 'history', ['kit-first-launch']),
    ('history-skin', 'history: the Linux skin is mounted', 'history', ['kit-page-mounted']),
    ('keyboard-search-fresh', 'keyboard: Ctrl+F focuses search on a fresh page', 'history', ['kit-search-shortcut-fresh']),
    ('history-rows', 'history: seeded rows and layout', 'history', ['kit-geometry', 'kit-rows']),
    ('skin-material', 'skin: solid surfaces (no native window material)', 'history', ['kit-material']),
    ('history-aria-detail', 'a11y: ARIA roles valid (detail)', 'history', ['kit-aria-roles-detail']),
    ('row-switch', 'row switch: Down moves selection, detail follows', 'history', ['kit-keyboard-select']),
    ('copy', 'detail: Enter copies (daemon restore answers 200)', 'history', ['kit-copy-restored']),
    ('keyboard-search', 'keyboard: Ctrl+F focuses search', 'history', ['kit-search-shortcut']),
    ('keyboard-settings', 'keyboard: Ctrl+, opens Settings', 'history', ['kit-settings-shortcut']),
    ('pin', 'pin from toolbar, unpin with Ctrl+P', 'history', ['kit-pin-toolbar', 'kit-unpin-shortcut']),
    ('tag-create', 'tags: T creates and attaches a tag', 'history', ['kit-tag-created']),
    ('multi-select', 'multi-select: Ctrl+click, selection summary', 'history', ['kit-multi-select', 'kit-aria-roles-selection', 'kit-tag-added-to-all']),
    ('context-menu', 'context menu: tag manager row menu', 'history', ['kit-tag-manager-menu', 'kit-native-menu-opaque']),
    ('tag-manager', 'tag manager: rename, show in sidebar', 'history', ['kit-tag-manager-rename-sidebar', 'kit-aria-roles-tag-manager']),
    ('delete', 'delete: confirmation, Escape keeps, confirm deletes', 'history', ['kit-delete-alert', 'kit-selection-after-escape', 'kit-delete-cancelled', 'kit-delete-confirmed']),
    ('sidebar-toggle', 'sidebar hide/show', 'history', ['kit-sidebar-toggle']),
    ('grid-inspector', 'grid layout, inspector, card context menu', 'history', ['kit-grid', 'kit-grid-inspector', 'kit-grid-context-tag']),
    ('resize-in-page', 'resize: narrowest window and compact sidebar (Window.SetSize)', 'history', ['kit-narrow-window', 'kit-narrow-sidebar']),
    ('search-text', 'search: text query with highlight', 'history', ['kit-search-text']),
    ('search-filter', 'filter: tag token, suggestions, no results, clear', 'history', ['kit-search-suggestions', 'kit-search-token', 'kit-search-no-results', 'kit-search-cleared']),
    ('query-failure', 'search failure state and retry', 'history', ['kit-query-failure', 'kit-query-retry']),
    ('detail-image', 'detail: image preview, Fit/100%, Quick Look', 'history', ['kit-image-detail', 'kit-image-actual-size', 'kit-quick-look', 'kit-quick-look-closed']),
    ('theme-light', 'theme: light applied, written back to the profile settings', 'history', ['kit-settings-writeback-light', 'kit-material-light']),
    ('theme-dark', 'theme: dark applied, written back to the profile settings', 'history', ['kit-settings-writeback-dark', 'kit-material-dark']),
    ('pagination', 'pagination: End reaches the oldest of 230+ entries', 'history', ['kit-paging-past-200']),
    ('history-complete', 'history scenario ran to its end', 'history', ['history-kit-complete']),
    ('restart-unlock', 'restart: locked profile unlocks (or opens) after a full quit', 'restart', ['restart-first-screen']),
    ('restart-persistence', 'restart: history persisted across the quit', 'restart', ['history-restored-after-recovery']),
]


def run(cmd, **kw):
    return subprocess.run(cmd, check=kw.pop('check', True), timeout=kw.pop('timeout', 120), **kw)


class Session:
    """The display the GUI runs on, plus the tools to look at it and resize its window."""

    def __init__(self, kind, runtime, out):
        self.kind, self.runtime, self.out = kind, runtime, out
        self.env, self.sway, self.xvfb, self.keeper = {}, None, None, None
        self.desktop_env = dict(os.environ)  # screenshot tools talk to the desktop's own bus, not the private one
        if kind == 'sway':
            from linux_wayland_run import Sway
            self.sway = Sway(runtime, out)
            self.sway.msg('for_window', '[all]', 'floating', 'enable')  # tiled windows ignore Window.SetSize
            self.keeper = subprocess.Popen(['wtype', '-s', '3000000', '-P', 'F24'], env=self.sway.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.env = {k: self.sway.env[k] for k in ('WAYLAND_DISPLAY', 'SWAYSOCK', 'XDG_SESSION_TYPE')} | {'GDK_BACKEND': 'wayland'}
            self.desktop_env = dict(self.sway.env)
        elif kind == 'xvfb':
            display = f':{90 + os.getpid() % 9}'
            self.xvfb = subprocess.Popen(['Xvfb', display, '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(50):
                if subprocess.run(['xdpyinfo', '-display', display], capture_output=True).returncode == 0:
                    break
                time.sleep(.2)
            self.env = {'DISPLAY': display, 'GDK_BACKEND': 'x11', 'XDG_SESSION_TYPE': 'x11'}
            self.desktop_env = dict(os.environ, DISPLAY=display)
        else:
            wayland = os.environ.get('WAYLAND_DISPLAY') or next((p.name for p in sorted(Path(runtime).glob('wayland-[0-9]*')) if not p.name.endswith('.lock')), None)
            if wayland:
                self.env = {'WAYLAND_DISPLAY': wayland, 'GDK_BACKEND': 'wayland', 'XDG_SESSION_TYPE': 'wayland'}
            elif os.environ.get('DISPLAY'):
                self.env = {'DISPLAY': os.environ['DISPLAY'], 'GDK_BACKEND': 'x11', 'XDG_SESSION_TYPE': 'x11'}
            else:
                raise SystemExit('--session existing: neither WAYLAND_DISPLAY nor DISPLAY is available (see the header for ssh)')
        self.wayland = 'WAYLAND_DISPLAY' in self.env

    def screenshot(self, path):
        """Returns None on success, else the reason no screenshot could be taken."""
        env = self.desktop_env
        tools = []
        if self.wayland:
            tools = [['grim', '-t', 'png', str(path)], ['gnome-screenshot', '-f', str(path)], ['spectacle', '-b', '-n', '-f', '-o', str(path)]]
        else:
            tools = [['import', '-window', 'root', str(path)], ['scrot', '-o', str(path)], ['gnome-screenshot', '-f', str(path)]]
        tried = []
        for cmd in tools:
            if not shutil.which(cmd[0]):
                continue
            r = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=60)
            if r.returncode == 0 and path.exists() and path.stat().st_size > 0:
                return None
            tried.append(f'{cmd[0]}: {(r.stderr or r.stdout).strip()[:120]}')
        return 'no screenshot tool worked (' + ('; '.join(tried) or 'none of grim/gnome-screenshot/spectacle/import/scrot installed') + ')'

    def resize(self, pid, width, height):
        """Resize the app's windows from the outside; returns None on success, else the reason it cannot be done here."""
        if self.kind == 'sway':
            self.sway.msg(f'[pid={pid}]', 'resize', 'set', 'width', str(width), 'px', 'height', str(height), 'px')
            return None
        if self.wayland:
            return 'an existing Wayland desktop offers no tool to resize another client window (compositor owns it)'
        if not shutil.which('xdotool'):
            return 'xdotool is not installed'
        ids = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], env=self.desktop_env, capture_output=True, text=True).stdout.split()
        if not ids:
            return 'xdotool found no visible window of the app'
        for wid in ids:
            subprocess.run(['xdotool', 'windowsize', wid, str(width), str(height)], env=self.desktop_env, check=False, timeout=15)
        return None

    def stop(self):
        for proc in (self.keeper, self.xvfb):
            if proc and proc.poll() is None:
                proc.terminate()
        if self.sway:
            self.sway.stop()


class PrivateBus:
    def __init__(self, out):
        cfg = out / 'private-bus.conf'
        sock = out / 'bus.sock'
        cfg.write_text('<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">\n'
                       f'<busconfig><type>session</type><listen>unix:path={sock}</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*" eavesdrop="true"/>'
                       '<allow eavesdrop="true"/><allow own="*"/></policy></busconfig>\n')
        sock.unlink(missing_ok=True)
        self.proc = subprocess.Popen(['dbus-daemon', '--config-file', str(cfg), '--nofork'], stdout=(out / 'private-bus.log').open('w'), stderr=subprocess.STDOUT)
        for _ in range(50):
            if sock.exists():
                break
            time.sleep(.1)
        else:
            raise RuntimeError('the private session bus did not create its socket')
        self.address = f'unix:path={sock}'

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()


class Runner:
    def __init__(self, args):
        self.args = args
        self.out = args.out.resolve()
        self.shots_dir = self.out / 'screenshots'
        self.shots_dir.mkdir(parents=True, exist_ok=True)
        self.sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
        self.profile = 'gui-go-' + self.sandbox.name
        for name in ('gui-go', 'uniclipd', 'uniclip'):
            shutil.copy2(args.binaries / name, self.sandbox / name)
        if args.gui_wrapper:  # a debugger around the GUI: the real binary keeps its place so it still finds uniclipd next to it
            (self.sandbox / 'gui-go').rename(self.sandbox / 'gui-go.real')
            script = self.sandbox / 'gui-go'
            script.write_text(f'#!/bin/sh\nexec {args.gui_wrapper} "{self.sandbox}/gui-go.real" "$@"\n')
            script.chmod(0o755)
        self.home, self.runtime = self.sandbox / 'home', self.sandbox / 'run'
        self.home.mkdir(mode=0o700)
        if args.session == 'existing':
            self.runtime = Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}')
        else:
            self.runtime.mkdir(mode=0o700)
        self.uniclip = str(self.sandbox / 'uniclip')
        self.stages, self.shots, self.extra = {}, [], []
        self.session = self.bus = None
        self.mobile = None

    def base_env(self):
        env = {k: v for k, v in os.environ.items() if not re.match(r'(?i)(http|https|all|no)_proxy$|^UC_|^UNICLIPBOARD|^GIO_|^APPIMAGE|^APPDIR|^GDK_BACKEND|^DBUS_SESSION|^XDG_SESSION_TYPE|^WAYLAND_DISPLAY|^DISPLAY$|^HYPRLAND', k)}
        env.update(HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / '.config'), XDG_DATA_HOME=str(self.home / '.local/share'),
                   XDG_CACHE_HOME=str(self.home / '.cache'), XDG_STATE_HOME=str(self.home / '.local/state'), XDG_RUNTIME_DIR=str(self.runtime),
                   UC_PORTABLE='1', UC_PROFILE=self.profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
                   DBUS_SESSION_BUS_ADDRESS=self.bus.address)
        return env

    def gui_env(self, phase, secret=None):
        env = dict(self.base_env(), **self.session.env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_VISIBLE='1')
        if phase:
            env['UC_GUI_GO_E2E_PHASE'] = phase
        if secret is not None:
            env['UC_GUI_GO_E2E_SECRET'] = secret
        return env

    def cli(self, *argv, check=True, timeout=120, stdin=None):
        r = subprocess.run([self.uniclip, *argv], env=self.base_env(), capture_output=True, text=True, timeout=timeout, input=stdin)
        if check and r.returncode != 0:
            raise RuntimeError(f'uniclip {" ".join(argv)} failed: {r.returncode} {r.stderr}')
        return r

    # ---- synthetic entries ----
    def seed_texts(self):
        for text in [*TEXTS, LINK, CODE]:
            # With no paired device the send exits 1 ("no online peers") after storing the entry.
            self.cli('send', '--connect-timeout', '0', check=False, stdin=text)
            time.sleep(1.1)  # distinct copy times keep the list order deterministic

    def put_mobile(self, path, body, kind):
        request = urllib.request.Request(f'http://127.0.0.1:{self.mobile[0]}{path}', data=body, method='PUT', headers={'Authorization': self.mobile[1], 'Content-Type': kind})
        with urllib.request.urlopen(request, timeout=60) as response:
            assert response.status < 300, response.status

    def seed_image(self):
        import base64
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        setup = json.loads(self.cli('--json', 'mobile', 'setup', '--non-interactive', '--label', 'e2e-phone', '--ip', '127.0.0.1', '--accept-network-risk', '--port', str(port)).stdout)
        self.mobile = (port, 'Basic ' + base64.b64encode(f"{setup['username']}:{setup['password']}".encode()).decode())
        self.put_mobile('/file/photo.png', synthetic_png(*IMAGE_SIZE), 'image/png')
        self.put_mobile('/SyncClipboard.json', json.dumps({'type': 'Image', 'text': 'photo.png', 'dataName': 'photo.png', 'hasData': True}).encode(), 'application/json')

    def seed_bulk(self, count=230):
        for index in range(count):
            self.put_mobile('/SyncClipboard.json', json.dumps({'type': 'Text', 'text': f'Bulk entry {index:03d}'}).encode(), 'application/json')

    def shot(self, name):
        path = self.shots_dir / f'{name.replace(":", "-").replace("/", "-")}.png'
        reason = self.session.screenshot(path)
        self.shots.append({'name': name, 'file': path.name if reason is None else None, 'notRun': reason})
        return reason

    # ---- one GUI launch ----
    def stage(self, name, phase, secret=None, timeout=600, done=None, on_row=None):
        evidence = self.out / f'{name}.jsonl'
        info = {'phase': phase or '(default)', 'steps': {}, 'order': [], 'error': None, 'exit': None}
        self.stages[name] = info
        gui = Gui(self.sandbox, self.gui_env(phase, secret), self.out, name)
        info['pid'] = gui.proc.pid
        seen, deadline = 0, time.monotonic() + timeout
        try:
            while time.monotonic() < deadline:
                rows = read_steps(evidence)
                for row in rows[seen:]:
                    info['steps'][row['step']] = {'ok': row.get('ok'), 'detail': row.get('detail')}
                    info['order'].append(row['step'])
                    if row['step'] == 'driver-error':
                        info['error'] = str(row.get('detail'))[:400]
                    if on_row:
                        on_row(row, gui)
                seen = len(rows)
                if info['error'] or (done and done in info['steps']) or gui.proc.poll() is not None:
                    break
                time.sleep(.2)
            else:
                info['error'] = f'timeout after {timeout}s; last step {info["order"][-1] if info["order"] else None}'
            if gui.proc.poll() is None:
                try:  # a finished driver quits the app itself; give it the time, then ask through the control file
                    info['exit'] = gui.proc.wait(20 if not info['error'] else 1)
                except subprocess.TimeoutExpired:
                    with gui.control.open('a') as f:
                        f.write('exit\n')
                    try:
                        info['exit'] = gui.proc.wait(20)
                    except subprocess.TimeoutExpired:
                        info['exit'] = None
            else:
                info['exit'] = gui.proc.returncode
            if info['exit'] not in (0, None) and not info['error']:
                log = (self.out / f'{name}.log').read_text(errors='replace')
                crash = next((line for line in log.splitlines() if re.match(r'(SIGSEGV|SIGABRT|SIGBUS|fatal error|panic:)', line)), '')
                info['error'] = f"the GUI process died (exit {info['exit']}) after step {info['order'][-1] if info['order'] else None}: {crash or 'see ' + name + '.log'}"
        finally:
            if gui.proc.poll() is None:
                gui.proc.terminate()
                try:
                    gui.proc.wait(30)
                except subprocess.TimeoutExpired:
                    gui.proc.kill()
                    gui.proc.wait(30)
            self.cli('stop', check=False, timeout=60)
        return gui, info

    def history_rows(self, row, gui):
        step = row['step']
        if step == 'kit-ready-for-entries':
            self.seed_texts()
        elif step == 'kit-ready-for-image' and not self.mobile:
            self.seed_image()
        elif step == 'kit-ready-for-bulk' and self.mobile:
            self.seed_bulk()
        elif step.startswith('shot:'):
            self.shot(step[5:])

    def entry_rows(self, row, gui):
        if row['step'] in ('home', 'settings'):
            self.shot('entry-' + row['step'])

    def restart_rows(self, row, gui):
        if row['step'] == 'history-restored-after-recovery':
            self.shot('restart-history')

    # ---- stage D: window resize from the outside ----
    def resize_stage(self):
        info = {'phase': 'wake', 'steps': {}, 'order': [], 'error': None, 'exit': None, 'sizes': []}
        self.stages['resize'] = info
        evidence = self.out / 'resize.jsonl'
        gui = Gui(self.sandbox, self.gui_env('wake', 'kit-ui-run'), self.out, 'resize')
        info['pid'] = gui.proc.pid
        seq = [0]

        def frame():
            seq[0] += 1
            label = f'size{seq[0]}'
            return gui.ctl(f'window-frame-state {label} main', f'window-frame-state-{label}', 30)['detail']

        try:
            gui.step('bootstrapped', 180)
            time.sleep(8)  # the page unlocks through the keystore and renders the history
            self.shot('resize-initial')
            info['sizes'].append(('initial', frame()))
            for label, (w, h) in (('narrow', (900, 600)), ('wide', (1180, 740))):
                reason = self.session.resize(gui.proc.pid, w, h)
                if reason:
                    info['resizeNotRun'] = reason
                    break
                time.sleep(3)
                size = frame()
                info['sizes'].append((label, size))
                self.shot(f'resize-{label}')
                info.setdefault('requested', {})[label] = [w, h]
            gui.ctl('exit', 'control-exit', 20)
            try:
                info['exit'] = gui.proc.wait(30)
            except subprocess.TimeoutExpired:
                info['exit'] = None
        except Exception as error:  # noqa: BLE001 - recorded, never swallowed
            info['error'] = f'{type(error).__name__}: {error}'
        finally:
            if gui.proc.poll() is None:
                gui.proc.kill()
                gui.proc.wait(30)
            self.cli('stop', check=False, timeout=60)
        return info

    # ---- evaluation ----
    def evaluate(self):
        results = []
        first_missing = {}
        for cid, label, stage, steps in CHECKS:
            info = self.stages.get(stage)
            row = {'id': cid, 'check': label, 'stage': stage, 'steps': steps}
            if cid == 'theme-dark' and self.args.skip_dark_theme:
                row.update(status=NOT_RUN, reason='left out on request (--skip-dark-theme)')
            elif info is None:
                row.update(status=NOT_RUN, reason=self.skipped.get(stage, 'stage did not run'))
            else:
                got = [info['steps'].get(s) for s in steps]
                bad = [s for s, g in zip(steps, got) if g is not None and not g['ok']]
                missing = [s for s, g in zip(steps, got) if g is None]
                if bad:
                    row.update(status=FAIL, reason='step failed: ' + ', '.join(bad), detail={s: info['steps'][s]['detail'] for s in bad})
                elif not missing:
                    row.update(status=PASS, reason=None)
                elif cid == 'entry-setup' and info['steps'].get('first-screen', {}).get('detail', {}).get('state') not in (None, 'setup'):
                    row.update(status=NOT_RUN, reason=f"first screen was {info['steps']['first-screen']['detail'].get('state')!r}, not a fresh profile")
                elif info['error'] and stage not in first_missing:
                    first_missing[stage] = cid
                    row.update(status=FAIL, reason=f"driver stopped here: {info['error']}; missing {missing}")
                elif info['error']:
                    row.update(status=NOT_RUN, reason=f"not reached: driver stopped earlier at {first_missing[stage]} ({info['error'][:160]})")
                else:
                    row.update(status=FAIL, reason=f'the stage ended without {missing}')
            results.append(row)
        return results

    def extras(self):
        out = []
        d = self.stages.get('resize')
        if d is None:
            out.append({'id': 'resize-external', 'check': 'resize from outside (compositor/X11) and GTK reports the size', 'status': NOT_RUN, 'reason': self.skipped.get('resize', 'stage did not run')})
        elif d['error']:
            out.append({'id': 'resize-external', 'check': 'resize from outside', 'status': FAIL, 'reason': d['error']})
        elif d.get('resizeNotRun'):
            out.append({'id': 'resize-external', 'check': 'resize from outside', 'status': NOT_RUN, 'reason': d['resizeNotRun']})
        else:
            sizes = dict(d['sizes'])
            ok = all(abs(sizes[k]['width'] - w) <= 40 and abs(sizes[k]['height'] - h) <= 80 for k, (w, h) in d['requested'].items())
            out.append({'id': 'resize-external', 'check': 'resize from outside, GTK reports the new size', 'status': PASS if ok else FAIL,
                        'reason': None if ok else 'GTK size differs from the request', 'detail': {'requested': d['requested'], 'gtk': {k: [v.get('width'), v.get('height')] for k, v in sizes.items()}}})
        quit_stages = [(n, s) for n, s in self.stages.items() if s['exit'] is not None]
        out.append({'id': 'quit', 'check': 'quit: every launch exits 0 and leaves no daemon', 'status': PASS if self.stages and all(s['exit'] == 0 for _, s in quit_stages) and len(quit_stages) == len(self.stages) and self.daemon_gone else FAIL,
                    'reason': None, 'detail': {n: s['exit'] for n, s in self.stages.items()} | {'daemonGone': self.daemon_gone}})
        taken = [s for s in self.shots if s['file']]
        out.append({'id': 'screenshots', 'check': 'screenshots captured', 'status': PASS if taken and len(taken) == len(self.shots) else (FAIL if taken else NOT_RUN),
                    'reason': None if taken and len(taken) == len(self.shots) else '; '.join(sorted({s['notRun'] for s in self.shots if s['notRun']})) or 'none requested', 'detail': {'taken': len(taken), 'requested': len(self.shots)}})
        return out

    def main(self):
        self.skipped = {}
        self.daemon_gone = False
        self.bus = PrivateBus(self.out)
        try:
            self.session = Session(self.args.session, self.runtime, self.out)
            shot_pause = self.args.shot_pause_ms
            seeded = json.dumps({'code': CODE, 'link': LINK, 'texts': TEXTS, 'image': {'dimensions': f'{IMAGE_SIZE[0]} × {IMAGE_SIZE[1]}'}, 'shotPauseMs': shot_pause, 'zoom': False, 'themes': ['light'] if self.args.skip_dark_theme else ['light', 'dark']})
            self.stage('entry', None, timeout=400, on_row=self.entry_rows)
            if self.stages['entry']['error'] and 'home' not in self.stages['entry']['steps']:
                for stage in ('history', 'restart', 'resize'):
                    self.skipped[stage] = 'the entry stage did not reach the history route: ' + self.stages['entry']['error'][:200]
            else:
                self.stage('history', 'history-kit', seeded, timeout=1500, done='history-kit-complete', on_row=self.history_rows)
                if not self.stages['history']['steps'].get('kit-rows', {}).get('ok'):
                    self.skipped['restart'] = 'the history stage never listed the seeded entries'
                else:  # the entry the restart looks for (the oldest text) is never selected by the history scenario, so it survives a partial run
                    # unlock-restart reads the newest 50 entries: after the paging pass that is the last bulk entry, else the oldest text
                    marker = 'Bulk entry 229' if self.stages['history']['steps'].get('kit-paging-past-200', {}).get('ok') else TEXTS[0]
                    self.stage('restart', 'unlock-restart', marker, timeout=240, done='history-restored-after-recovery', on_row=self.restart_rows)
                self.resize_stage()
            conns = list(self.sandbox.rglob('daemon.conn'))
            deadline = time.monotonic() + 20
            while conns and time.monotonic() < deadline:
                conns = [c for c in conns if c.exists()]
                time.sleep(.3)
            self.daemon_gone = not conns
        finally:
            self.cli('stop', check=False, timeout=60)
            if self.session:
                self.session.stop()
            self.bus.stop()
        checks = self.evaluate() + self.extras()
        counts = {s: sum(c['status'] == s for c in checks) for s in (PASS, FAIL, NOT_RUN)}
        head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, capture_output=True, text=True).stdout
        diff = subprocess.run(['git', 'diff', 'HEAD'], cwd=ROOT, capture_output=True).stdout
        result = {'profile': self.profile, 'session': self.args.session, 'systemClipboardDisabled': True, 'head': head,
                  'dirtyDiffSha256': hashlib.sha256(diff).hexdigest(), 'dirtyFiles': dirty.splitlines(),
                  'uname': list(os.uname()), 'counts': counts, 'checks': checks, 'screenshots': self.shots,
                  'stages': {n: {k: v for k, v in s.items() if k != 'order'} | {'lastSteps': s['order'][-5:]} for n, s in self.stages.items()},
                  'scope': 'synthetic data only; private D-Bus bus; real WebKitGTK rendering; no real clipboard or profile'}
        (self.out / 'linux-kit-ui-result.json').write_text(json.dumps(result, indent=2, ensure_ascii=False, default=str) + '\n')
        for c in checks:
            print(f"{c['status'].upper():8} {c['id']}: {c['check']}" + (f" -- {c['reason']}" if c.get('reason') else ''), flush=True)
        print(json.dumps(counts))
        shutil.rmtree(self.sandbox, ignore_errors=True)
        return 0 if counts[FAIL] == 0 else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True, help='directory with gui-go (gtk3,e2e), uniclipd and uniclip')
    parser.add_argument('--session', choices=('sway', 'xvfb', 'existing'), default='sway')
    parser.add_argument('--gui-wrapper', default='', help='command that runs the GUI under a tool, e.g. "gdb -batch -ex run -ex bt --args"')
    parser.add_argument('--skip-dark-theme', action='store_true', help='leave the dark pass out of the theme check (its theme-dark row becomes not_run); libwebkit2gtk segfaulted there in the Ubuntu 24.04 arm64 sway container')
    parser.add_argument('--shot-pause-ms', type=int, default=1500, help='how long the driver waits after each shot:<name> step')
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('this script runs on Linux only')
    args.out.mkdir(parents=True, exist_ok=True)
    return Runner(args).main()


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    sys.exit(main())
