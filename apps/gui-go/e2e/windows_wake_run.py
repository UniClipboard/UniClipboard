#!/usr/bin/env python3
"""Windows resume (wake) path: WM_POWERBROADCAST / PBT_APMRESUMEAUTOMATIC reaches the update scheduler.

NOT a real sleep: the e2e build sends the message Windows delivers on resume to the host's own windows, so Wails'
window procedure -> Windows.APMResumeAutomatic -> Common.SystemDidWake -> scheduler chain runs for real. A real
suspend/resume of the machine is a separate acceptance item (it ends the interactive session and needs a lease).
Evidence: the scheduler logs "update scheduler: system wake" once per delivered resume.
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
from windows_single_instance_run import processes_in, wait_until  # noqa: E402


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
    env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    genv = dict(env, UC_GUI_GO_ISOLATED='1', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_EXIT_MODE='full')
    results = {'sandbox': str(sandbox), 'checks': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME')}
    checks = results['checks']

    def check(name, ok, detail=None):
        checks.append({'check': name, 'ok': bool(ok), 'detail': detail})
        print(('PASS ' if ok else 'FAIL ') + name, flush=True)

    gui = None
    try:
        subprocess.run([str(sandbox / 'uniclip.exe'), 'space', 'init', '--passphrase', 'wake-pass', '--device-name', 'wake'], env=env, check=True, timeout=120, capture_output=True)
        time.sleep(6)  # let init's oneshot daemon withdraw: the start-right-after-init flow has its own harness (windows_daemon_start_run.py)
        gui = q.Gui(sandbox, genv, out)
        gui.step('bootstrapped', 120)
        log = out / 'gui.log'
        count = lambda: log.read_text(encoding='utf-8', errors='replace').count('update scheduler: system wake')
        time.sleep(2)
        base = count()
        r = gui.ctl('wake 1', 'control-wake')
        got = wait_until(lambda: count() > base, 10)
        check('W1 one resume message reaches the scheduler (it logs a system wake)', r['ok'] and bool(got), {'posted': r.get('detail'), 'before': base, 'after': count()})
        base = count()
        r = gui.ctl('wake 5', 'control-wake')
        time.sleep(2)
        check('W2 a burst of 5 resume broadcasts reaches the scheduler (each one once per host window) without blocking the host', r['ok'] and count() - base >= 5 and gui.proc.poll() is None, {'delivered': count() - base})
        gui.ctl('exit', 'control-exit')
        check('W3 the host still exits 0 after the wakes', gui.proc.wait(timeout=60) == 0)
        results['passed'] = all(c['ok'] for c in checks)
    finally:
        if gui and gui.proc.poll() is None:
            gui.proc.kill()
        for pid in processes_in(sandbox) + processes_in(sandbox, 'uniclipd.exe'):
            subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        shutil.copytree(sandbox, out / 'sandbox', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
        shutil.rmtree(sandbox, ignore_errors=True)
        (out / 'windows-wake-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
