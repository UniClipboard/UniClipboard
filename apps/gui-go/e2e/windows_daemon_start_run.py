#!/usr/bin/env python3
"""Windows start-after-init repetition: does the host come up when it is started right after `space init`?

`space init` leaves a oneshot daemon that withdraws (removes daemon.conn, stops answering /health) but still holds the
instance lock for a moment. A host started in that window used to spawn a daemon that failed to take the lock and the
launch died with "local daemon did not become healthy". This script repeats init -> immediate GUI start -> exit in
throwaway sandboxes and records, per round, the time until the host is bootstrapped, the time to exit, and the
outcome, so a pass rate (not one lucky run) backs the claim. Only PIDs started by this script are ever stopped.

    set UC_GUI_GO_E2E_DEDICATED_HOST=1
    set UC_GUI_GO_E2E_SANDBOX_ROOT=D:\\w
    python windows_daemon_start_run.py --out <new dir> --binaries <dir with gui-go.exe uniclip.exe uniclipd.exe> --rounds 10
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import windows_quick_panel_run as q  # noqa: E402


def pid_alive(pid):
    out = subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH', '/FO', 'CSV'], capture_output=True, text=True, errors='replace').stdout
    return f'"{pid}"' in out


def one_round(n, binaries, out, delay):
    sandbox, profile, root = q.make_sandbox()
    for name in ('gui-go.exe', 'uniclipd.exe', 'uniclip.exe'):
        shutil.copy2(binaries / name, sandbox / name)
    env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    genv = dict(env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_EXIT_MODE='full')
    rd = out / f'round-{n:02d}'
    rd.mkdir()
    res = {'round': n}
    gui = None
    daemon_pid = None
    try:
        subprocess.run([str(sandbox / 'uniclip.exe'), 'space', 'init', '--passphrase', 'start-race-pass', '--device-name', 'start-race'], env=env, check=True, timeout=120, capture_output=True)
        if delay:
            time.sleep(delay)
        t0 = time.monotonic()
        gui = q.Gui(sandbox, genv, rd)
        try:
            gui.step('bootstrapped', 90)
            res['bootstrap_seconds'] = round(time.monotonic() - t0, 2)
            res['ok'] = True
        except RuntimeError as e:
            res['ok'] = False
            res['error'] = str(e)
            res['bootstrap_seconds'] = round(time.monotonic() - t0, 2)
        conn = sandbox / 'data' / f'app.uniclipboard.desktop-{profile}' / 'daemon.conn'
        if conn.is_file():
            daemon_pid = json.loads(conn.read_text())['pid']
        if res['ok']:
            t1 = time.monotonic()
            gui.ctl('exit', 'control-exit')
            try:
                res['exit_code'] = gui.proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                res['exit_code'] = 'TIMEOUT>60s'
            res['exit_seconds'] = round(time.monotonic() - t1, 2)
            res['daemon_gone'] = not (daemon_pid and pid_alive(daemon_pid))
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.kill()
        if daemon_pid and pid_alive(daemon_pid):
            subprocess.run(['taskkill', '/F', '/PID', str(daemon_pid)], capture_output=True)
        subprocess.run([str(sandbox / 'uniclip.exe'), '--json', 'stop'], env=env, capture_output=True, timeout=80)
        shutil.copytree(sandbox / 'data' / 'logs', rd / 'logs', dirs_exist_ok=True) if (sandbox / 'data' / 'logs').is_dir() else None
        shutil.rmtree(sandbox, ignore_errors=True)
    print(json.dumps(res), flush=True)
    return res


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=10)
    parser.add_argument('--delay-after-init', type=float, default=0.0)
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('Windows dedicated host only (UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    args.out.mkdir(parents=True)
    rounds = [one_round(n, args.binaries.resolve(), args.out, args.delay_after_init) for n in range(1, args.rounds + 1)]
    summary = {'binaries': str(args.binaries), 'rounds': len(rounds), 'ok': sum(1 for r in rounds if r.get('ok')),
               'failed': sum(1 for r in rounds if not r.get('ok')), 'exit_timeouts': sum(1 for r in rounds if r.get('exit_code') == 'TIMEOUT>60s'),
               'executed_on': os.environ.get('COMPUTERNAME'), 'results': rounds}
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: v for k, v in summary.items() if k != 'results'}))
    sys.exit(0 if summary['failed'] == 0 and not summary['exit_timeouts'] else 1)


if __name__ == '__main__':
    main()
