#!/usr/bin/env python3
"""Linux X11 product-default quick panel shortcut and first-map geometry E2E (slice 17c8). RUNS INSIDE the build container.

  docker run ... python3 /work/apps/gui-go/e2e/linux_x11_default_shortcut_run.py --out <dir> --binaries <dir with gui-go, uniclipd, uniclip>

What differs from linux_xvfb_run.py (which this reuses the helpers of): NO UC_GUI_GO_E2E_DEFAULT_SHORTCUT seam. The
default is whatever the shipped code and the real daemon settings give on a brand-new profile, and every settings
change goes through the real shared settings page (Settings > Quick panel: the switch, the shortcut recorder popover)
driven by e2e-driver.ts `linux-shortcut-ui*`, with real XTEST key events typed into the recorder. UC_GUI_GO_E2E_SHORTCUTS=1
is only the permission gate that lets this launch bind the (private Xvfb) X server at all; it does not change a value.

Launches (all on one fresh isolated profile; GUI exit 0 + daemon stopped between them):
  L1 observe  first start: what does the real page show, what did the host register, what does the daemon hold
  L2 enable   ONLY when L1 shows the panel disabled by default: the user turning the switch on in the real page (a
              separate, labelled user action: the default is never changed to make a check pass)
  L3 rebind   the real recorder: open it, type a new chord with real keys, save; old key released, new key grabbed,
              daemon /settings read back, the NEW chord (real XTEST) shows the panel
  L4 restart  same profile, fresh process: the persisted chord is registered, not the default

Geometry: an xev tracer on the X root window (SubstructureNotify) records CreateNotify / MapNotify / UnmapNotify /
ConfigureNotify for every top-level window with the receipt time (monotonic ns, same clock as the host evidence steps
stamped by this script); window names come from xprop. The FIRST MapNotify of the quick-panel window and every
ConfigureNotify after it are the evidence for a size jump. A final screenshot is not accepted as proof.

Scope: private Xvfb, NO window manager, no compositor, no portal. Not a native desktop acceptance.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from linux_xvfb_run import Gui, key_is_free, pid_alive, read_steps, xdo  # noqa: E402

PASSPHRASE = 'linux-default-shortcut-passphrase'
CTRL, MOD1 = 4, 8
DEFAULT_CHORD = 'ctrl+alt+v'
NEW_CHORD_XDO = 'ctrl+alt+shift+F9'
NEW_CHORD = 'ctrl+alt+shift+f9'
KEY_V, KEY_F9 = 'v', 'F9'


class XTrace(threading.Thread):
    """xev on the root window: structure events of every top-level window, stamped on receipt."""

    # xev prints e.g. `CreateNotify event, serial 18, synthetic NO, window 0x21f,` followed by `parent 0x21f, window 0x400001,
    # (10,10), width 10, height 10`; the event's own window is the root, the subject is the `window` after parent/event.
    EVENT = re.compile(r'^(CreateNotify|MapNotify|UnmapNotify|ConfigureNotify|DestroyNotify|ReparentNotify) event, serial (\d+)')

    def __init__(self, display, path):
        super().__init__(daemon=True)
        env = dict(os.environ, DISPLAY=display)
        self.proc = subprocess.Popen(['stdbuf', '-oL', 'xev', '-root', '-event', 'substructure'], env=env, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT, text=True, bufsize=1)
        self.path = path
        self.events = []
        self.display = display
        self.names = {}  # window id -> title, resolved WHILE the windows exist (xprop fails after the GUI exits)

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
                    m = re.search(r'\((-?\d+),(-?\d+)\), width (\d+), height (\d+)', line)
                    if m:
                        current['geometry'] = [int(g) for g in m.groups()]
                    m = re.search(r'override (YES|NO)', line)
                    if m:
                        current['override'] = m.group(1)

    def stop(self):
        self.proc.terminate()

    def resolve(self):
        for wid in {e.get('window') for e in list(self.events) if e.get('window')}:
            if self.names.get(wid) is None:
                self.names[wid] = self.window_name(self.display, wid)

    def window_name(self, display, wid):
        out = subprocess.run(['xprop', '-id', wid, 'WM_NAME', '_NET_WM_NAME'], env=dict(os.environ, DISPLAY=display), capture_output=True, text=True)
        names = re.findall(r'= "([^"]*)"', out.stdout)
        return names[0] if names else None

    def timeline(self, name_of):
        rows = []
        for e in self.events:
            if e['type'] in ('CreateNotify', 'MapNotify', 'UnmapNotify', 'ConfigureNotify') and e.get('override') != 'YES':
                rows.append(dict(e, name=name_of(e.get('window'))))
        return rows


def tracer_rows_for(trace, display, title):
    trace.resolve()
    rows = trace.timeline(lambda wid: trace.names.get(wid))
    # The Wails panel window carries WM_NAME "gui-go" under X11 (not its configured "Quick Panel" title), like the 10x10
    # helper window of the same process. The panel is the named window that is neither the main window ("UniClipboard")
    # nor that helper: the one whose first CreateNotify is larger than 100x100.
    first = {}
    for r in rows:
        if r['type'] == 'CreateNotify' and 'geometry' in r:
            first.setdefault(r['window'], r['geometry'])
    wins = {w for w, g in first.items() if g[2] * g[3] > 100 * 100 and trace.names.get(w) not in (None, 'UniClipboard')}
    return [r for r in rows if r['window'] in wins], trace.names


def first_map_analysis(trace, display, title='Quick Panel'):
    """Size known at the FIRST MapNotify of the panel window and every size change after it (the 'jump')."""
    rows, _ = tracer_rows_for(trace, display, title)
    size, first_map, later = None, None, []
    for r in rows:
        if r['type'] in ('CreateNotify', 'ConfigureNotify') and 'geometry' in r:
            w, h = r['geometry'][2], r['geometry'][3]
            if first_map is None:
                size = (w, h)
            elif (w, h) != first_map['size']:
                later.append({'t': r['t'], 'type': r['type'], 'size': (w, h), 'after_first_map_ms': (r['t'] - first_map['t']) / 1e6})
        elif r['type'] == 'MapNotify' and first_map is None:
            first_map = {'t': r['t'], 'size': size}
    return {'first_map_size': first_map['size'] if first_map else None, 'size_changes_after_first_map': later, 'events': len(rows)}


def daemon_pid_of(sandbox):
    return json.loads(next(sandbox.rglob('daemon.conn')).read_text())['pid']


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
        # No UC_GUI_GO_E2E_DEFAULT_SHORTCUT here, on purpose.
        return dict(base_env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE=phase, UC_GUI_GO_E2E_SHORTCUTS='1',
                    UC_GUI_GO_EXIT_MODE='full')

    uniclip = str(sandbox / 'uniclip')
    results = {'sandbox': str(sandbox), 'profile': profile, 'display': display, 'checks': [], 'passed': False, 'facts': {},
               'daemon_sha256': subprocess.run(['sha256sum', str(sandbox / 'uniclipd')], capture_output=True, text=True).stdout.split()[0],
               'scope': 'Xvfb + private D-Bus in a container; no WM, no compositor, no Wayland, no portal',
               'seam_UC_GUI_GO_E2E_DEFAULT_SHORTCUT': 'not set (asserted)'}
    checks, facts = results['checks'], results['facts']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail, 't': time.monotonic_ns()})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    trace = XTrace(display, out / 'xev-root-substructure.log')
    trace.start()
    time.sleep(0.5)
    gui = None
    daemon_pid = None
    launch = [0]

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

    def state(g, label):
        return g.wait_state(label, lambda s: s['panelReady'], 60)

    try:
        subprocess.run([uniclip, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'linux-default-shortcut'], env=base_env, check=True, timeout=120)

        # ---------------- L1: first start, observe only ----------------
        gui = start_gui('linux-shortcut-ui')
        s1 = state(gui, 'l1')
        daemon_pid = daemon_pid_of(sandbox)
        ui = gui.step('ui-quick-panel-state', 90)
        facts['L1'] = {'state': s1, 'ui': ui.get('detail')}
        enabled_by_default = bool(s1['enabled'])
        facts['product_default_enabled'] = enabled_by_default
        check('L1 the real settings page shows the product default shortcut (shared frontend default, nothing stored)',
              s1['stored'] is None and str(ui['detail'].get('shortcutLabel', '')).lower().replace(' ', '') in ('ctrl+alt+v',), {'ui': ui['detail'], 'stored': s1['stored']})
        if enabled_by_default:
            check('L1 default enabled: the host registered the product default ctrl+alt+v and Wails holds it',
                  s1['recorded'] == [DEFAULT_CHORD] and len(s1['wails']) == 1 and not key_is_free(display, KEY_V, CTRL | MOD1), s1)
        else:
            check('L1 default DISABLED (product contract, nothing enabled by this test): the host registered no shortcut and the key is free',
                  s1['recorded'] == [] and s1['wails'] in ([], None) and key_is_free(display, KEY_V, CTRL | MOD1), s1)
        code, alive = stop_gui(gui, daemon_pid)
        check('L1 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
        gui = None

        # ---------------- L2: the user enables the panel (only if it is off by default) ----------------
        if not enabled_by_default:
            gui = start_gui('linux-shortcut-ui:enable')
            state(gui, 'l2-boot')
            daemon_pid = daemon_pid_of(sandbox)
            en = gui.step('ui-enabled', 90)
            s2 = gui.wait_state('l2', lambda s: s['recorded'] == [DEFAULT_CHORD], 10)
            facts['L2'] = {'ui': en.get('detail'), 'state': s2}
            check('L2 USER ACTION (real switch click on the real page): enabling registers the product default ctrl+alt+v, saved enabled in the daemon',
                  s2['enabled'] and s2['recorded'] == [DEFAULT_CHORD] and not key_is_free(display, KEY_V, CTRL | MOD1), s2)
            code, alive = stop_gui(gui, daemon_pid)
            check('L2 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
            gui = None

        # ---------------- L3: rebind through the real recorder ----------------
        gui = start_gui('linux-shortcut-ui:rebind')
        s3 = state(gui, 'l3-boot')
        daemon_pid = daemon_pid_of(sandbox)
        check('L3 precondition: the persisted enabled panel registers the default at startup', s3['recorded'] == [DEFAULT_CHORD] and s3['enabled'], s3)
        gui.step('recorder-open', 90)
        xdo(display, 'mousemove', '300', '300')
        t_default = time.monotonic_ns()
        xdo(display, 'key', DEFAULT_CHORD)
        shown = gui.wait_state('l3-default-shown', lambda s: s['panelVisible'])
        check('L3 a real XTEST ctrl+alt+v (the DEFAULT) shows the quick panel', shown['panelVisible'], shown)
        facts['t_default_press'] = t_default
        time.sleep(1.5)
        trace.resolve()
        geo = first_map_analysis(trace, display)
        facts['first_map'] = geo
        check('L3 X11 first-map geometry: the panel is first mapped at the Tauri Linux contract size 800x560', geo['first_map_size'] == (800, 560), geo)
        check('L3 X11: no ConfigureNotify changes the size after the first MapNotify (no jump)', geo['first_map_size'] is not None and not geo['size_changes_after_first_map'], geo)
        xdo(display, 'key', DEFAULT_CHORD)
        hidden = gui.wait_state('l3-default-hidden', lambda s: not s['panelVisible'])
        check('L3 the same chord hides it again (not a vacuous pass)', not hidden['panelVisible'], hidden)
        xdo(display, 'key', NEW_CHORD_XDO)  # typed into the recorder: it has the focus on the real page
        cand = gui.step('recorder-candidate', 30)
        saved = gui.step('ui-shortcut-saved', 30)
        facts['L3'] = {'candidate': cand.get('detail'), 'saved': saved.get('detail')}
        s3b = gui.wait_state('l3-saved', lambda s: s['recorded'] == [NEW_CHORD], 10)
        check('L3 the recorder captured the new chord from real key events and the page saved it',
              s3b['recorded'] == [NEW_CHORD] and str(s3b['stored']).lower() == NEW_CHORD, {'candidate': cand.get('detail'), 'state': s3b})
        check('L3 old chord released on the X server (another client can now grab ctrl+alt+v)', key_is_free(display, KEY_V, CTRL | MOD1))
        check('L3 new chord is grabbed on the X server', not key_is_free(display, KEY_F9, CTRL | MOD1 | 1))
        xdo(display, 'key', DEFAULT_CHORD)
        time.sleep(1.0)
        check('L3 the old chord no longer shows the panel', not gui.state('l3-old-press')['panelVisible'])
        xdo(display, 'key', NEW_CHORD_XDO)
        shown = gui.wait_state('l3-new-shown', lambda s: s['panelVisible'])
        check('L3 a real XTEST press of the NEW chord shows the panel', shown['panelVisible'], shown)
        xdo(display, 'key', NEW_CHORD_XDO)
        gui.wait_state('l3-new-hidden', lambda s: not s['panelVisible'])
        code, alive = stop_gui(gui, daemon_pid)
        check('L3 exit 0, daemon stopped, new chord free again', code == 0 and not alive and key_is_free(display, KEY_F9, CTRL | MOD1 | 1), {'exit': code, 'daemonAlive': alive})
        gui = None

        # ---------------- L4: restart, persisted value ----------------
        gui = start_gui('wake')
        s4 = state(gui, 'l4')
        daemon_pid = daemon_pid_of(sandbox)
        check('L4 after a real restart of the same profile the PERSISTED chord is registered, not the default',
              s4['recorded'] == [NEW_CHORD] and str(s4['stored']).lower() == NEW_CHORD and key_is_free(display, KEY_V, CTRL | MOD1), s4)
        xdo(display, 'key', NEW_CHORD_XDO)
        shown = gui.wait_state('l4-shown', lambda s: s['panelVisible'])
        check('L4 the persisted chord shows the panel after the restart', shown['panelVisible'], shown)
        trace.resolve()
        xdo(display, 'key', NEW_CHORD_XDO)
        gui.wait_state('l4-hidden', lambda s: not s['panelVisible'])
        code, alive = stop_gui(gui, daemon_pid)
        check('L4 exit 0 and daemon stopped', code == 0 and not alive, {'exit': code, 'daemonAlive': alive})
        gui = None
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.terminate()
        subprocess.run([uniclip, '--json', 'stop'], env=base_env, capture_output=True, timeout=80)
        if daemon_pid and pid_alive(daemon_pid):  # only the PID recorded in this sandbox's own daemon.conn
            os.kill(daemon_pid, 15)
        time.sleep(0.5)
        trace.stop()
        rows, names = tracer_rows_for(trace, display, 'Quick Panel')  # names resolved earlier, while the windows existed
        results['quick_panel_x11_timeline'] = rows
        results['window_names'] = names
        (out / 'linux-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False, default=str) + '\n', encoding='utf-8')
        if sandbox.name.startswith('uc-gui-go-') and sandbox.parent == Path(tempfile.gettempdir()):
            shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
