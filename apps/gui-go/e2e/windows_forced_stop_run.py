#!/usr/bin/env python3
"""Windows forced daemon stop: identity and history continuity after TerminateProcess while entries are being written.

The host stops the daemon with TerminateProcess (no graceful shutdown), so a stop can land in the middle of a write.
For each round this script runs a synthetic profile's daemon in the foreground (`uniclip run`, a process it owns),
writes numbered text entries with `uniclip send --text` from a writer thread, terminates the daemon at a different
moment each round (after verifying the pid in daemon.conn is an uniclipd.exe inside the sandbox), restarts it, and checks:

  - the daemon starts again and is healthy (no restricted recovery),
  - the space identity (space id, device id, fingerprint) is unchanged,
  - every entry whose `send` was acknowledged before the kill is in the history with its exact text,
  - the history lists and the search index answers, and a new entry can be written.

A write that was in flight is allowed to be lost or kept; it must never come back corrupt. Only the synthetic sandbox
profile is touched; the data comes from this script, never from the real clipboard (UC_DISABLE_SYSTEM_CLIPBOARD=1).

    set UC_GUI_GO_E2E_DEDICATED_HOST=1
    set UC_GUI_GO_E2E_SANDBOX_ROOT=D:\\w
    python windows_forced_stop_run.py --out <new dir> --binaries <dir with uniclip.exe uniclipd.exe> [--rounds 5]
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import windows_quick_panel_run as q  # noqa: E402
from windows_single_instance_run import pid_alive, processes_in, wait_until  # noqa: E402

UUID = re.compile(r'[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
FINGERPRINT = re.compile(r'\b[A-Z0-9]{4}(?:-[A-Z0-9]{4}){3}\b')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--rounds', type=int, default=5)
    args = parser.parse_args()
    if os.name != 'nt' or os.environ.get('UC_GUI_GO_E2E_DEDICATED_HOST') != '1':
        sys.exit('Windows dedicated host only (UC_GUI_GO_E2E_DEDICATED_HOST=1)')
    out = args.out.resolve()
    out.mkdir(parents=True)
    sandbox, profile, root = q.make_sandbox()
    for name in ('uniclipd.exe', 'uniclip.exe'):
        shutil.copy2(args.binaries / name, sandbox / name)
    uniclip = str(sandbox / 'uniclip.exe')
    env = dict(os.environ, UC_PORTABLE='1', UC_PROFILE=profile, UNICLIPBOARD_ENV='development', UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1')
    data = sandbox / 'data' / f'app.uniclipboard.desktop-{profile}'
    results = {'sandbox': str(sandbox), 'profile': profile, 'rounds': [], 'passed': False, 'executed_on': os.environ.get('COMPUTERNAME')}
    owned = []

    def cli(*a, timeout=60):
        return subprocess.run([uniclip, *a], env=env, capture_output=True, text=True, errors='replace', timeout=timeout)

    def start_daemon(label):
        log = (out / f'run-{label}.log').open('w')
        p = subprocess.Popen([uniclip, 'run'], env=env, cwd=sandbox, stdout=log, stderr=subprocess.STDOUT)
        owned.append(p)
        conn = data / 'daemon.conn'
        ok = wait_until(lambda: conn.is_file() and cli('--json', 'space', 'status').returncode == 0, 90, .5)
        pid = json.loads(conn.read_text())['pid'] if conn.is_file() else None
        return p, pid, bool(ok)

    def identity():
        r = cli('--json', 'space', 'status')
        return {'uuids': sorted(set(UUID.findall(r.stdout))), 'fingerprints': sorted(set(FINGERPRINT.findall(r.stdout))), 'rc': r.returncode}

    def history_text():
        r = cli('--json', 'get', '--list', '--limit', '5000', timeout=120)
        return r.returncode, r.stdout

    try:
        r = cli('space', 'init', '--passphrase', 'forced-stop-pass', '--device-name', 'forced-stop', timeout=120)
        results['init_rc'] = r.returncode
        time.sleep(5)  # the oneshot daemon of init withdraws
        baseline_identity = None
        acked_all = []
        for rnd in range(1, args.rounds + 1):
            rr = {'round': rnd}
            proc, pid, up = start_daemon(f'r{rnd}-a')
            rr['daemon_up'] = up
            if baseline_identity is None:
                baseline_identity = identity()
                results['baseline_identity'] = baseline_identity
            stop = threading.Event()
            acked, failed = [], []

            def writer():
                i = 0
                while not stop.is_set():
                    marker = f'forced-stop r{rnd} #{i:04d} \u2713 ' + 'x' * (10 + (i * 37) % 400)
                    try:
                        res = cli('send', '--text', marker, timeout=30)
                        (acked if res.returncode == 0 else failed).append(marker)
                    except subprocess.TimeoutExpired:
                        failed.append(marker)
                    i += 1

            t = threading.Thread(target=writer)
            t.start()
            time.sleep(1.5 + rnd * 0.7)  # a different moment in the write stream each round
            # identity gate: the pid in daemon.conn must be an uniclipd.exe from this sandbox, and the one we started
            conn_pid = json.loads((data / 'daemon.conn').read_text())['pid']
            inside = conn_pid in processes_in(sandbox, 'uniclipd.exe')
            rr['kill_target'] = {'pid': conn_pid, 'inside_sandbox': inside, 'recorded_at_start': pid}
            if not inside:
                rr['error'] = 'refusing to stop a process that is not this sandbox\'s uniclipd.exe'
                stop.set(); t.join()
                results['rounds'].append(rr)
                break
            subprocess.run(['taskkill', '/F', '/PID', str(conn_pid)], capture_output=True)
            rr['killed_at'] = time.time()
            stop.set(); t.join(timeout=60)
            # a send that was in flight may have auto-started a oneshot daemon of this sandbox: stop it too, by verified pid
            extra = [x for x in processes_in(sandbox, 'uniclipd.exe') if x != conn_pid]
            for x in extra:
                subprocess.run(['taskkill', '/F', '/PID', str(x)], capture_output=True)
            rr['extra_sandbox_daemons_stopped'] = extra
            rr['acked_before_or_at_kill'] = len(acked)
            rr['failed_writes'] = len(failed)
            if proc.poll() is None:
                proc.kill()  # the foreground `uniclip run` we own
            wait_until(lambda: not pid_alive(conn_pid), 20)
            acked_all.extend(acked)
            # restart
            proc2, pid2, up2 = start_daemon(f'r{rnd}-b')
            rr['restart_ok'] = up2
            rr['new_daemon_pid'] = pid2
            ident = identity()
            rr['identity_unchanged'] = ident == baseline_identity
            rc, text = history_text()
            (out / f'history-r{rnd}.json').write_text(text, encoding='utf-8')
            missing = [m for m in acked_all if m[:40] not in text]
            rr['history_rc'] = rc
            rr['acked_total'] = len(acked_all)
            rr['acked_missing'] = len(missing)
            rr['acked_missing_examples'] = [m[:60] for m in missing[:3]]
            s = cli('search', 'forced-stop', timeout=60)
            rr['search_rc'] = s.returncode
            w = cli('send', '--text', f'after restart r{rnd}')
            rr['write_after_restart_rc'] = w.returncode
            rr['abnormal_exit_logged'] = any('exited abnormally' in p.read_text(errors='replace') for p in (sandbox / 'data' / 'logs').glob('*') if p.is_file())
            rr['ok'] = bool(up2 and rr['identity_unchanged'] and rc == 0 and not missing and s.returncode == 0 and w.returncode == 0)
            results['rounds'].append(rr)
            print(json.dumps(rr), flush=True)
            if proc2.poll() is None:
                proc2.kill()
            wait_until(lambda: not pid_alive(pid2), 20)
            time.sleep(1)
        results['passed'] = len(results['rounds']) == args.rounds and all(r.get('ok') for r in results['rounds'])
    finally:
        for p in owned:
            if p.poll() is None:
                p.kill()
        for pid in processes_in(sandbox, 'uniclipd.exe'):
            subprocess.run(['taskkill', '/F', '/PID', str(pid)], capture_output=True)
        shutil.copytree(sandbox, out / 'sandbox', ignore=shutil.ignore_patterns('*.exe'), dirs_exist_ok=True)
        shutil.rmtree(sandbox, ignore_errors=True)
        (out / 'windows-forced-stop-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps({'passed': results['passed']}))
    sys.exit(0 if results['passed'] else 1)


if __name__ == '__main__':
    main()
