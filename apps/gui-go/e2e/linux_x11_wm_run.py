#!/usr/bin/env python3
"""Linux X11 quick panel under a REAL window manager (Openbox): window scale, workspaces, first map, show/hide, focus (slice 17c9).
RUNS INSIDE the build container (image uc-gui-go-linux-build:17c9-wm), started by linux/run_17c9.sh.

  python3 linux_x11_wm_run.py --out <dir> --binaries <dir with gui-go, uniclipd, uniclip>

Contract (docs/architecture/gui-go-linux-x11-wm-window-scale.md): the panel window is 800x560 x the window scale (0.8-1.5, step 0.1,
clamped), the scale is changed by the user with the REAL keys ctrl+= / ctrl+- while the panel has the focus (the shared frontend owns
and persists it), a mapped panel is resized at once, every (re)show and every restart maps the panel at the stored scale's size.
Nothing here calls a host command or the daemon to change a value; the observer reads the X server and the window manager only
(wmctrl/xprop over EWMH, xev structure events on the root window). The default-shortcut seam is not set.

Launches (one fresh isolated profile, GUI exit 0 + daemon stopped between them):
  L1 scale 1.0 (default): WM sanity, first map, show/hide, Escape, focus from a previous target window, blur, workspaces, scale 1.0 -> 1.1
  L2 restart: stored 1.1 is the first-map size; scale -> 0.8 (clamp press beyond)
  L3 restart: stored 0.8; scale -> 1.5 (clamp press beyond)
  L4 restart: stored 1.5 is the first-map size

Scope: private Xvfb 1920x1200 + Openbox 3.6.1 (default config) + private D-Bus in a container. No native desktop, no Wayland, no GPU.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linux_x11_default_shortcut_run import DEFAULT_CHORD, PASSPHRASE, XTrace, daemon_pid_of  # noqa: E402
from linux_xvfb_run import Gui, key_is_free, pid_alive, xdo  # noqa: E402

BASE_W, BASE_H = 800, 560
KEY_V = 'v'
CTRL, MOD1 = 4, 8


def sized(scale):
    return round(BASE_W * scale), round(BASE_H * scale)


def clamp_step(scale, delta):
    return round(min(1.5, max(0.8, scale + delta)) * 10) / 10


class WMTrace(XTrace):
    """XTrace plus the ReparentNotify parent: under a WM the root sees the FRAME windows, not the reparented client."""

    def run(self):
        current = None
        with open(self.path, 'w') as raw:
            for line in self.proc.stdout:
                t = time.monotonic_ns()
                raw.write(f'{t} {line}')
                raw.flush()
                match = self.EVENT.match(line)
                if match:
                    current = {'t': t, 'type': match.group(1), 'serial': int(match.group(2))}
                    self.events.append(current)
                elif current is not None:
                    m = re.search(r'(?:parent|event) 0x[0-9a-f]+, window (0x[0-9a-f]+)', line)
                    if m:
                        current['window'] = m.group(1)
                    m = re.search(r'window 0x[0-9a-f]+, parent (0x[0-9a-f]+)', line)
                    if m:
                        current['new_parent'] = m.group(1)
                    m = re.search(r'\((-?\d+),(-?\d+)\), width (\d+), height (\d+)', line)
                    if m:
                        current['geometry'] = [int(g) for g in m.groups()]
                    m = re.search(r'override (YES|NO)', line)
                    if m:
                        current['override'] = m.group(1)


def x(display, *cmd, check=False):
    r = subprocess.run(list(cmd), env=dict(os.environ, DISPLAY=display), capture_output=True, text=True, timeout=20)
    return r.stdout


def wm_rows(display):
    """EWMH managed client windows: wmctrl -lGp = id desktop pid x y w h host title."""
    rows = []
    for line in x(display, 'wmctrl', '-lGp').splitlines():
        f = line.split(None, 8)
        if len(f) >= 8:
            rows.append({'id': f[0], 'desktop': int(f[1]), 'pid': int(f[2]), 'x': int(f[3]), 'y': int(f[4]), 'w': int(f[5]),
                         'h': int(f[6]), 'title': f[8] if len(f) > 8 else ''})
    return rows


def panel_of(rows, gui_pid):
    cand = [r for r in rows if r['pid'] == gui_pid and r['title'] == 'gui-go' and r['w'] * r['h'] > 100 * 100]
    return cand[0] if cand else None


def root_prop(display, name):
    return x(display, 'xprop', '-root', name).strip()


def active_window(display):
    m = re.search(r'window id # (0x[0-9a-f]+)', root_prop(display, '_NET_ACTIVE_WINDOW'))
    return int(m.group(1), 16) if m else None


def current_desktop(display):
    m = re.search(r'= (\d+)', root_prop(display, '_NET_CURRENT_DESKTOP'))
    return int(m.group(1)) if m else None


def window_props(display, wid):
    return x(display, 'xprop', '-id', wid, '_NET_WM_DESKTOP', '_NET_WM_STATE', '_NET_FRAME_EXTENTS', '_NET_WM_WINDOW_TYPE', '_MOTIF_WM_HINTS',
             'WM_NORMAL_HINTS')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'linux':
        sys.exit('this script runs on Linux only (inside the build container)')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-gui-go-'))
    profile = 'gui-go-' + sandbox.name
    for name in ('gui-go', 'uniclipd', 'uniclip'):
        shutil.copy2(args.binaries / name, sandbox / name)
    home, runtime_dir = sandbox / 'home', sandbox / 'run'
    for d in (home, runtime_dir):
        d.mkdir(mode=0o700)
    display = os.environ.get('DISPLAY', ':99')
    base_env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / '.config'), XDG_RUNTIME_DIR=str(runtime_dir), UC_PORTABLE='1',
                    UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1', DISPLAY=display,
                    XDG_SESSION_TYPE='x11', GDK_BACKEND='x11')
    for k in ('WAYLAND_DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'APPIMAGE', 'UC_GUI_GO_E2E_DEFAULT_SHORTCUT'):
        base_env.pop(k, None)

    def gui_env(phase):
        return dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE=phase, UC_GUI_GO_E2E_SHORTCUTS='1',
                    UC_GUI_GO_EXIT_MODE='full')

    uniclip = str(sandbox / 'uniclip')
    results = {'sandbox': str(sandbox), 'profile': profile, 'display': display, 'checks': [], 'passed': False, 'facts': {},
               'daemon_sha256': subprocess.run(['sha256sum', str(sandbox / 'uniclipd')], capture_output=True, text=True).stdout.split()[0],
               'scope': 'Xvfb + Openbox in a container; not a native desktop, no Wayland, no GPU',
               'seam_UC_GUI_GO_E2E_DEFAULT_SHORTCUT': 'not set (asserted)'}
    checks, facts = results['checks'], results['facts']
    snaps = (out / 'snapshots.jsonl').open('w')

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail, 't': time.monotonic_ns()})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    def snap(label, pid=None):
        rows = wm_rows(display)
        panel = panel_of(rows, pid) if pid else None
        s = {'label': label, 't': time.monotonic_ns(), 'active': active_window(display), 'desktop': current_desktop(display), 'rows': rows,
             'panel': panel, 'panel_props': window_props(display, panel['id']) if panel else None}
        snaps.write(json.dumps(s) + '\n')
        snaps.flush()
        return s

    # ---------------- the window manager: a mature one, started unmodified, and proven to manage windows ----------------
    wm_log = (out / 'openbox.log').open('w')
    wm = subprocess.Popen(['openbox', '--sm-disable'], env=base_env, stdout=wm_log, stderr=subprocess.STDOUT)
    for _ in range(50):
        if 'Name:' in x(display, 'wmctrl', '-m'):
            break
        time.sleep(0.2)
    wmm = x(display, 'wmctrl', '-m')
    (out / 'wmctrl-m.txt').write_text(wmm)
    (out / 'wm-desktops.txt').write_text(x(display, 'wmctrl', '-d'))
    facts['wm'] = wmm
    check('WM: Openbox is the EWMH window manager of this X server (wmctrl -m)', 'Name: Openbox' in wmm, wmm)
    n_desktops = len(x(display, 'wmctrl', '-d').strip().splitlines())
    facts['desktops'] = n_desktops
    check('WM: more than one desktop (workspace) exists', n_desktops >= 2, n_desktops)

    trace = WMTrace(display, out / 'xev-root-substructure.log')
    trace.start()
    time.sleep(0.5)
    gui = None
    daemon_pid = None
    launch = [0]
    target = None

    def start_gui(phase):
        launch[0] += 1
        subprocess.run([uniclip, 'start'], env=base_env, check=True, timeout=120)
        g = Gui(sandbox, gui_env(phase), out, f'gui{launch[0]}-{phase.replace(":", "-")}')
        facts[f'launch{launch[0]}'] = {'phase': phase, 't_start': time.monotonic_ns()}
        g.step('bootstrapped', 120)
        return g

    def stop_gui(g, pid):
        g.ctl('exit', 'control-exit')
        code = g.proc.wait(timeout=60)
        deadline = time.monotonic() + 20
        while pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(.3)
        return code, pid_alive(pid)

    def ready(g, label):
        return g.wait_state(label, lambda s: s['panelReady'], 60)

    def panel_now(g):
        return panel_of(wm_rows(display), g.proc.pid)

    def wait_panel(g, visible, timeout=6):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            p = panel_now(g)
            if bool(p) == visible:
                return p
            time.sleep(0.05)
        return panel_now(g)

    def stable_geometry(g, timeout=5):
        """Panel (w, h) once it has not changed for 0.5 s; every sample is kept as evidence."""
        samples, deadline = [], time.monotonic() + timeout
        while time.monotonic() < deadline:
            p = panel_now(g)
            samples.append((time.monotonic_ns(), None if p is None else (p['x'], p['y'], p['w'], p['h'])))
            last = [s[1] for s in samples if time.monotonic_ns() - s[0] < 5e8]
            if p and len(last) >= 5 and len(set(last)) == 1:
                break
            time.sleep(0.1)
        return samples[-1][1], samples

    def chord(g, label):
        t0 = time.monotonic_ns()
        xdo(display, 'key', DEFAULT_CHORD)
        return t0

    def show(g, label, desktop=None):
        chord(g, label)
        p = wait_panel(g, True)
        geo, samples = stable_geometry(g)
        facts.setdefault('geometry_samples', {})[label] = samples
        return p, geo

    def hide(g, label):
        chord(g, label)
        return wait_panel(g, False)

    def first_map_analysis():
        trace.resolve()
        parents = {}  # client -> frame
        clients = {}
        for e in trace.events:
            if e['type'] == 'CreateNotify' and 'geometry' in e and e.get('window'):
                clients.setdefault(e['window'], e['geometry'])
            if e['type'] == 'ReparentNotify' and e.get('new_parent') and e.get('window'):
                parents[e['window']] = e['new_parent']
        panels = [w for w, g_ in clients.items() if g_[2] * g_[3] > 100 * 100 and trace.names.get(w) not in (None, 'UniClipboard')]
        out_rows = []
        for e in trace.events:
            w = e.get('window')
            if e['type'] in ('CreateNotify', 'MapNotify', 'UnmapNotify', 'ConfigureNotify', 'ReparentNotify') and e.get('override') != 'YES':
                for pw in panels:
                    if w == pw or w == parents.get(pw):
                        out_rows.append(dict(e, role='client' if w == pw else 'frame', panel=pw))
        return panels, parents, out_rows

    def window_name(wid):
        return trace.names.get(wid)

    try:
        subprocess.run([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'linux-wm-panel'], env=base_env, check=True, timeout=120)
        scale = 1.0

        # ---------------- L1 ----------------
        gui = start_gui('wake')
        s = ready(gui, 'l1')
        daemon_pid = daemon_pid_of(sandbox)
        facts['L1_state'] = s
        check('L1 default: the host registered ctrl+alt+v (nothing seamed) and the panel is hidden', s['recorded'] == [DEFAULT_CHORD] and not s['panelVisible'], s)
        time.sleep(1.5)
        base = snap('l1-boot', gui.proc.pid)
        main_rows = [r for r in base['rows'] if r['pid'] == gui.proc.pid]
        facts['L1_boot_windows'] = main_rows
        check('WM manages the real GUI: the main window is in _NET_CLIENT_LIST with a desktop', any(r['title'] == 'UniClipboard' for r in main_rows), main_rows)
        check('L1 the hidden panel is not a managed (mapped) window', base['panel'] is None, base['panel'])

        # a previous target application (an ordinary GTK3 window) that has the focus
        target = subprocess.Popen(['python3', str(Path(__file__).resolve().parent / 'linux' / 'tools' / 'target_window.py'), 'uc-target'], env=base_env,
                                  stdout=(out / 'target.log').open('w'), stderr=subprocess.STDOUT)
        t_row = None
        for _ in range(100):
            t_row = next((r for r in wm_rows(display) if r['title'] == 'uc-target'), None)
            if t_row:
                break
            time.sleep(0.1)
        time.sleep(0.5)
        t_id = int(t_row['id'], 16) if t_row else None
        check('target app: managed by Openbox and active (the previous focus owner)', t_row and active_window(display) == t_id, {'row': t_row, 'active': active_window(display)})

        # first show at scale 1.0
        p, geo = show(gui, 'l1-show1')
        sn = snap('l1-show1', gui.proc.pid)
        facts['L1_show1'] = {'panel': p, 'geo': geo, 'snap': sn}
        check('L1 show: the panel is a managed window at 800x560 (scale 1.0)', p and (geo[2], geo[3]) == sized(1.0), {'geo': geo})
        trace.resolve()
        check('L1 show: the panel window is the active (focused) window of the WM, the target app lost the focus',
              p and active_window(display) == int(p['id'], 16) and active_window(display) != t_id, {'active': active_window(display), 'panel': p and p['id']})
        check('L1 show: the panel is on the current desktop', p and p['desktop'] == current_desktop(display), {'panel': p and p['desktop'], 'current': current_desktop(display)})
        facts['L1_props'] = sn['panel_props']
        st = gui.state('l1-visible')
        check('L1 show: the host and the X server agree the panel is visible', st['panelVisible'] and p is not None, st)

        # Escape typed with the real key reaches the page in the panel (real keyboard focus) and dismisses it
        xdo(display, 'key', 'Escape')
        gone = wait_panel(gui, False)
        check('L1 Escape (real key) reaches the focused panel page and hides the panel', gone is None, panel_now(gui))
        time.sleep(0.5)
        check('L1 after the panel hides, the WM gives the focus back to the previous target app', active_window(display) == t_id, {'active': active_window(display), 'target': t_id})

        # second show: re-map size
        p, geo = show(gui, 'l1-show2')
        check('L1 re-show after hide: the panel is mapped at 800x560 again', p and (geo[2], geo[3]) == sized(1.0), geo)
        gone = hide(gui, 'l1-hide2')
        check('L1 the chord hides the panel again', gone is None)

        # blur: the panel shown, another window takes the focus (user clicks the target) -> the panel hides
        p, geo = show(gui, 'l1-show-blur')
        x(display, 'wmctrl', '-i', '-a', t_row['id'])
        gone = wait_panel(gui, False, 4)
        check('L1 blur: another window taking the focus hides the panel', gone is None, panel_now(gui))
        time.sleep(0.5)

        # ---------------- workspaces ----------------
        x(display, 'wmctrl', '-s', '1')
        time.sleep(0.5)
        check('WS the current desktop is now 1', current_desktop(display) == 1, current_desktop(display))
        p, geo = show(gui, 'l1-ws1')
        sn = snap('l1-ws1', gui.proc.pid)
        facts['L1_ws1'] = {'panel': p, 'geo': geo, 'snap': sn}
        check('WS panel shown while desktop 1 is current: it is a visible managed window at 800x560 on desktop 1',
              p and p['desktop'] == 1 and (geo[2], geo[3]) == sized(1.0), {'panel': p, 'geo': geo})
        check('WS the panel is active on desktop 1', p and active_window(display) == int(p['id'], 16), {'active': active_window(display)})
        # leave the desktop while the panel is open
        x(display, 'wmctrl', '-s', '2')
        time.sleep(1.0)
        sn = snap('l1-ws2-away', gui.proc.pid)
        facts['L1_ws2_away'] = {'snap': sn, 'host_state': gui.state('l1-ws2-away')}
        gone = hide(gui, 'l1-ws2-hide-attempt')  # may already be hidden by the blur handler; record, do not assume
        time.sleep(0.5)
        st = gui.state('l1-ws2-after')
        facts['L1_ws2_after_chord'] = {'wm_panel': panel_now(gui), 'host': st}
        # on a third desktop the chord shows it there
        p = panel_now(gui)
        if p is None:
            p, geo = show(gui, 'l1-ws2-show')
        check('WS panel shown while desktop 2 is current: managed on desktop 2', p and p['desktop'] == 2, {'panel': p, 'current': current_desktop(display)})
        if panel_now(gui):
            hide(gui, 'l1-ws2-hide')
        x(display, 'wmctrl', '-s', '0')
        time.sleep(0.5)

        # ---------------- scale 1.0 -> 1.1 while open, then re-show ----------------
        def scale_step(g, key, tag, new_scale_expected, label_prefix):
            p0 = panel_now(g)
            xdo(display, 'key', key)
            geo, samples = stable_geometry(g)
            facts.setdefault('geometry_samples', {})[tag] = samples
            exp = sized(new_scale_expected)
            check(f'{label_prefix} live: ctrl-key to scale {new_scale_expected} resizes the MAPPED panel to {exp[0]}x{exp[1]}', geo and (geo[2], geo[3]) == exp,
                  {'before': p0 and (p0['w'], p0['h']), 'after': geo})
            return geo

        p, geo = show(gui, 'l1-scale-open')
        scale = clamp_step(scale, 0.1)
        scale_step(gui, 'ctrl+equal', 'l1-up1', scale, 'L1 scale 1.0->1.1')
        gone = hide(gui, 'l1-scale-hide')
        p, geo = show(gui, 'l1-scale-reshow')
        check(f'L1 scale 1.1: after hide + show the panel is mapped at {sized(scale)}', geo and (geo[2], geo[3]) == sized(scale), geo)
        hide(gui, 'l1-scale-hide2')
        code, alive = stop_gui(gui, daemon_pid)
        check('L1 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
        gui = None

        # ---------------- L2: restart (1.1 persisted), then to 0.8 ----------------
        def restart_and_check(phase_label, expected_scale):
            g = start_gui('wake')
            st = ready(g, phase_label + '-boot')
            pid = daemon_pid_of(sandbox)
            time.sleep(1.0)
            p, geo = show(g, phase_label + '-first')
            trace.resolve()
            check(f'{phase_label} restart: the persisted scale {expected_scale} gives the first-map size {sized(expected_scale)}',
                  geo and (geo[2], geo[3]) == sized(expected_scale), {'geo': geo})
            return g, pid

        gui, daemon_pid = restart_and_check('L2', scale)
        for i in range(3):  # 1.1 -> 1.0 -> 0.9 -> 0.8, and one more press beyond the lower bound
            new = clamp_step(scale, -0.1)
            scale_step(gui, 'ctrl+minus', f'l2-down{i + 1}', new, f'L2 scale {scale}->{new}')
            scale = new
        check('L2 lower bound reached: 0.8', scale == 0.8)
        scale_step(gui, 'ctrl+minus', 'l2-clamp', 0.8, 'L2 clamp (press beyond 0.8 keeps 0.8)')
        hide(gui, 'l2-hide')
        p, geo = show(gui, 'l2-reshow')
        check(f'L2 scale 0.8: re-show maps {sized(0.8)}', geo and (geo[2], geo[3]) == sized(0.8), geo)
        hide(gui, 'l2-hide2')
        code, alive = stop_gui(gui, daemon_pid)
        check('L2 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
        gui = None

        # ---------------- L3: restart (0.8 persisted), then to 1.5 ----------------
        gui, daemon_pid = restart_and_check('L3', 0.8)
        for i in range(7):  # 0.8 -> 1.5
            new = clamp_step(scale, 0.1)
            scale_step(gui, 'ctrl+equal', f'l3-up{i + 1}', new, f'L3 scale {scale}->{new}')
            scale = new
        check('L3 upper bound reached: 1.5', scale == 1.5)
        scale_step(gui, 'ctrl+equal', 'l3-clamp', 1.5, 'L3 clamp (press beyond 1.5 keeps 1.5)')
        hide(gui, 'l3-hide')
        p, geo = show(gui, 'l3-reshow')
        check(f'L3 scale 1.5: re-show maps {sized(1.5)}', geo and (geo[2], geo[3]) == sized(1.5), geo)
        hide(gui, 'l3-hide2')
        code, alive = stop_gui(gui, daemon_pid)
        check('L3 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
        gui = None

        # ---------------- L4: restart (1.5 persisted) ----------------
        gui, daemon_pid = restart_and_check('L4', 1.5)
        hide(gui, 'l4-hide')
        code, alive = stop_gui(gui, daemon_pid)
        check('L4 exit 0, daemon stopped, ctrl+alt+v free again', code == 0 and not alive and key_is_free(display, KEY_V, CTRL | MOD1), {'exit': code, 'daemonAlive': alive})
        gui = None

        panels, parents, rows = first_map_analysis()
        facts['x11_structure'] = {'panel_client_windows': panels, 'frames': parents, 'names': trace.names}
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        if target and target.poll() is None:
            target.terminate()
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        subprocess.run([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid and pid_alive(daemon_pid):
            os.kill(daemon_pid, 15)
        time.sleep(0.5)
        trace.stop()
        try:
            panels, parents, rows = first_map_analysis()
            results['panel_x11_timeline'] = rows
            results['window_names'] = trace.names
        except Exception as exc:  # noqa: BLE001  (evidence writer must not hide the run result)
            results['timeline_error'] = repr(exc)
        wm.terminate()
        snaps.close()
        (out / 'linux-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False, default=str) + '\n', encoding='utf-8')
        if sandbox.name.startswith('uc-gui-go-') and sandbox.parent == Path(tempfile.gettempdir()):
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
