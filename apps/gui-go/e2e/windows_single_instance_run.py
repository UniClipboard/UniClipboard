#!/usr/bin/env python3
"""Windows single-instance E2E: the Wails named-mutex lock, second-launch delivery, burst launches, restart handoff.

Real pieces: the e2e GUI build, real daemons, real second launches of the same executable (each a real process that
really exits), the real restart path (`fullRestart`: stop the daemon, spawn a new one, start a new GUI that waits for
UC_GUI_RESTART_PARENT_PID). Sandboxed like the other Windows runs (UC_PORTABLE, `gui-go-*` profile, no system
clipboard). Only PIDs this script started, or that the GUI reported itself and whose image path is inside the sandbox,
are ever stopped.

  N  `--quick-panel` with no first instance exits 1 and starts nothing
  A  first instance; second launches plain / --autostart / --quick-panel are delivered with the right action and
     exit 0; a burst of 8 leaves one GUI process and the same GUI and daemon pids
  R  restart handoff: a new GUI process becomes the first instance, the old one is gone, one GUI process remains
  T  after the first instance quits, a new launch takes over
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import windows_quick_panel_run as q  # noqa: E402


def processes_in(sandbox, image='gui-go.exe'):
    """(pid, path) of the processes whose executable is inside the sandbox."""
    ps = f"Get-CimInstance Win32_Process -Filter \"Name='{image}'\" | % {{ \"$($_.ProcessId)|$($_.ExecutablePath)\" }}"
    out = subprocess.run(['powershell', '-NoProfile', '-Command', ps], capture_output=True, text=True, errors='replace').stdout
    found = []
    for line in out.splitlines():
        pid, _, path = line.partition('|')
        if pid.strip().isdigit() and path and Path(path).resolve().parent == sandbox.resolve():
            found.append(int(pid))
    return found


def pid_alive(pid):
    out = subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH', '/FO', 'CSV'], capture_output=True, text=True, errors='replace').stdout
    return f'"{pid}"' in out


def steps(path, name):
    return [r for r in q.read_steps(path) if r['step'] == name]


def wait_until(predicate, timeout=30, interval=.2):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        v = predicate()
        if v:
            return v
        time.sleep(interval)
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('Windows dedicated host only (UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    out = args.out.resolve()
    out.mkdir(parents=True)
    sandbox, profile, root = q.make_sandbox()
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe'):
        shutil.copy2(args.binaries / name, sandbox / name)
    exe = str(sandbox / 'gui-go.exe')
    env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    genv = dict(env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_SHORTCUTS='1',
                UC_GUI_GO_E2E_DEFAULT_SHORTCUT=q.F13, UC_GUI_GO_EXIT_MODE='full')
    results = {'sandbox': str(sandbox), 'profile': profile, 'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME')}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    owned = []
    try:
        subprocess.run([str(sandbox / 'uniclip.exe'), 'space', 'init', '--passphrase', 'single-instance-pass', '--device-name', 'single'], env=env, check=True, timeout=120, capture_output=True)
        time.sleep(6)  # let init's oneshot daemon withdraw: the start-right-after-init flow has its own harness (windows_daemon_start_run.py)
        # N: no first instance
        t = time.monotonic()
        r = subprocess.run([exe, '--quick-panel'], env=genv, cwd=sandbox, capture_output=True, timeout=60)
        check('N --quick-panel without a first instance exits 1 quickly and starts no GUI', r.returncode == 1 and time.monotonic() - t < 15 and not processes_in(sandbox),
              {'exit': r.returncode, 'seconds': round(time.monotonic() - t, 1)})

        # A: first instance
        gui = q.Gui(sandbox, genv, out)
        owned.append(gui.proc)
        boot = gui.step('bootstrapped', 120)
        ev = out / 'native.jsonl'
        state0 = gui.ctl('state a0', 'control-state')['detail']
        gui_pid, daemon_pid = state0['pid'], state0.get('daemonPid')
        check('A0 first instance is up and the only GUI process', gui_pid == gui.proc.pid and processes_in(sandbox) == [gui_pid], {'state': state0, 'processes': processes_in(sandbox)})

        def second(args_, label, action):
            n = len(steps(ev, 'second-instance'))
            t = time.monotonic()
            p = subprocess.run([exe] + args_, env=genv, cwd=sandbox, capture_output=True, timeout=60)
            took = time.monotonic() - t
            got = wait_until(lambda: len(steps(ev, 'second-instance')) > n and steps(ev, 'second-instance')[-1], 15)
            ok = p.returncode == 0 and took < 20 and bool(got) and got['detail']['action'] == action and got['detail']['pid'] == gui_pid
            check(label, ok, {'exit': p.returncode, 'seconds': round(took, 1), 'delivered': got and got['detail']})

        second([], 'A1 plain second launch exits 0 and the first instance shows the main window', 'show-main-window')
        second(['--autostart'], 'A2 --autostart second launch exits 0 and is ignored', 'ignore-autostart')
        before = gui.state('qp0')
        second(['--quick-panel'], 'A3 --quick-panel second launch exits 0 and toggles the panel', 'toggle-quick-panel')
        shown = gui.wait_state('qp1', lambda s: s['panelVisible'] != before['panelVisible'])
        check('A3b the quick panel visibility changed with the request', shown['panelVisible'] != before['panelVisible'], {'before': before['panelVisible'], 'after': shown['panelVisible']})
        if shown['panelVisible']:
            subprocess.run([exe, '--quick-panel'], env=genv, cwd=sandbox, capture_output=True, timeout=60)
            gui.wait_state('qp2', lambda s: not s['panelVisible'])

        n = len(steps(ev, 'second-instance'))
        with ThreadPoolExecutor(8) as ex:
            t = time.monotonic()
            futs = [ex.submit(lambda: subprocess.run([exe], env=genv, cwd=sandbox, capture_output=True, timeout=90).returncode) for _ in range(8)]
            codes = [f.result() for f in futs]
        took = time.monotonic() - t
        wait_until(lambda: len(steps(ev, 'second-instance')) >= n + 8, 20)
        state1 = gui.ctl('state a1', 'control-state')['detail']
        delivered = len(steps(ev, 'second-instance')) - n
        check('A4 a burst of 8 launches all exit 0; one GUI process, same GUI pid and daemon pid',
              codes == [0] * 8 and processes_in(sandbox) == [gui_pid] and state1['pid'] == gui_pid and state1.get('daemonPid') == daemon_pid,
              {'codes': codes, 'seconds': round(took, 1), 'delivered': delivered, 'processes': processes_in(sandbox), 'daemonPid': [daemon_pid, state1.get('daemonPid')]})

        # R: restart handoff
        gui.ctl('restart', 'control-restart')
        nb = wait_until(lambda: [r for r in steps(ev, 'bootstrapped') if r['detail']['pid'] != gui_pid], 120)
        new_pid = nb[-1]['detail']['pid'] if nb else None
        if new_pid:
            owned_pids = [new_pid]
        old_gone = wait_until(lambda: not pid_alive(gui_pid), 30)
        procs = processes_in(sandbox)
        state2 = None
        if new_pid:
            # the shared control file is read by whichever process polls it, so ask until the new process answers
            def _ask():
                gui.ctl('state r1', 'control-state')
                mine = [r for r in steps(ev, 'control-state') if r['detail'].get('pid') == new_pid]
                return mine[-1]['detail'] if mine else None
            state2 = wait_until(_ask, 30, 1.5)
        check('R1 restart: a new GUI process bootstrapped, the old one exited, exactly one GUI process remains',
              bool(new_pid) and bool(old_gone) and procs == [new_pid], {'old': gui_pid, 'new': new_pid, 'processes': procs})
        check('R2 restart replaced the daemon and the new daemon is healthy', bool(state2) and state2.get('daemonPid') not in (None, daemon_pid) and pid_alive(state2['daemonPid']),
              {'oldDaemon': daemon_pid, 'state': state2})
        if new_pid:
            n = len(steps(ev, 'second-instance'))
            p = subprocess.run([exe], env=genv, cwd=sandbox, capture_output=True, timeout=60)
            got = wait_until(lambda: len(steps(ev, 'second-instance')) > n and steps(ev, 'second-instance')[-1], 15)
            check('R3 a second launch now reaches the new first instance', p.returncode == 0 and bool(got) and got['detail']['pid'] == new_pid, got and got['detail'])

        # T: quit, then take over
        with (gui.control).open('a') as f:
            f.write('exit\n')
        quit_ok = wait_until(lambda: not processes_in(sandbox), 60)
        check('T1 the first instance quits and no GUI process remains', bool(quit_ok), processes_in(sandbox))
        gui2 = q.Gui(sandbox, genv, out)  # truncates the evidence: a brand-new first instance
        owned.append(gui2.proc)
        b2 = gui2.step('bootstrapped', 120)
        check('T2 a new launch after the quit becomes the first instance', gui2.proc.poll() is None and b2['detail']['pid'] == gui2.proc.pid, b2['detail'])
        gui2.ctl('exit', 'control-exit')
        check('T3 and exits 0 again', gui2.proc.wait(timeout=60) == 0)
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        for p in owned:
            if p.poll() is None:
                p.kill()
        for pid in processes_in(sandbox):  # GUI processes launched from this sandbox's copy only
            subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        for pid in processes_in(sandbox, 'uniclipd.exe'):
            subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        subprocess.run([str(sandbox / 'uniclip.exe'), '--json', 'stop'], env=env, capture_output=True, timeout=80)
        shutil.copytree(sandbox, out / 'sandbox', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
        shutil.rmtree(sandbox, ignore_errors=True)
        (out / 'windows-single-instance-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
