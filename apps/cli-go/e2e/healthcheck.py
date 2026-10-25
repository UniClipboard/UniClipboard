#!/usr/bin/env python3
"""Real-process acceptance of the read-only `uniclip health` check (container healthcheck).

Usage: healthcheck.py --bin DIR --out DIR
DIR holds `uniclip` and `uniclipd` built from the current source. The runner uses a
throwaway HOME, file-based key storage and a no-op clipboard, and writes `result.json`.

Failure modes covered:
  F1 fixed-port probe of 42715 fails while a healthy daemon runs (the reported bug);
  F2 no daemon.conn, stale daemon.conn (daemon killed), partial/corrupt daemon.conn,
     daemon alive but not answering (SIGSTOP) must all be unhealthy;
  F3 a probe must never spawn, restart or repair a daemon (pid and conn unchanged);
  F4 a restarted daemon on a new port must become healthy again;
  F5 a profile must never be judged by another profile's daemon.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid

from isolated import isolated_env


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--default-profile', action='store_true',
                        help='run profile A with UC_PROFILE unset (needs a host whose default profile can start, '
                             'e.g. a Linux container); otherwise A is a named profile')
    args = parser.parse_args()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    bindir = Path(args.bin).resolve()
    uniclip = str(bindir / 'uniclip')
    home = tempfile.mkdtemp(prefix='health-e2e-', dir=tempfile.gettempdir())
    # Profile A is the default profile (UC_PROFILE unset) with --default-profile, else a named one.
    default_env = isolated_env(home, 'hc-a-' + uuid.uuid4().hex[:8])
    if args.default_profile:
        del default_env['UC_PROFILE']
    named_env = isolated_env(home, 'hc-' + uuid.uuid4().hex[:8])
    default_env['PATH'] = named_env['PATH'] = str(bindir) + os.pathsep + default_env['PATH']
    records, children = [], []

    def data_root(env):
        base = Path(home) / 'Library' / 'Application Support' if sys.platform == 'darwin' \
            else Path(env['XDG_DATA_HOME'])
        suffix = '-' + env['UC_PROFILE'] if 'UC_PROFILE' in env else ''
        return base / ('app.uniclipboard.desktop' + suffix)

    def check(name, ok, detail=''):
        records.append(dict(check=name, result='PASS' if ok else 'FAIL', detail=detail))
        print(('PASS ' if ok else 'FAIL ') + name + (' :: ' + detail if detail else ''), flush=True)
        (out / 'result.json').write_text(json.dumps(records, indent=2))

    def health(env, timeout=30):
        r = subprocess.run([uniclip, 'health'], env=env, capture_output=True, text=True, timeout=timeout)
        return r.returncode, (r.stdout + r.stderr).strip()

    def start(env, label):
        log = open(out / (label + '.log'), 'w')
        p = subprocess.Popen([uniclip, 'run', '--server'], env=env, stdout=log, stderr=log)
        log.close()
        children.append(p)
        return p

    def conn(env):
        path = data_root(env) / 'daemon.conn'
        return json.loads(path.read_text()) if path.exists() else None

    def wait_conn(env, not_pid=None):
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            try:
                c = conn(env)
            except ValueError:
                c = None
            if c and c['pid'] != not_pid and health(env)[0] == 0:
                return c
            time.sleep(0.5)
        raise SystemExit('daemon did not become healthy')

    def daemon_pids():
        # Every uniclipd started from BINDIR; this run is the only user of that binary.
        ps = subprocess.run(['ps', '-axo', 'pid=,command='], capture_output=True, text=True).stdout
        return {int(l.split(None, 1)[0]) for l in ps.splitlines()
                if l.split(None, 1)[1].startswith(str(bindir / 'uniclipd'))}

    try:
        # F2: nothing published yet.
        code, msg = health(default_env)
        check('no daemon.conn is unhealthy (profile A)', code != 0, msg)
        check('probe did not create a daemon.conn', conn(default_env) is None)

        # F1 + healthy default profile.
        proc = start(default_env, 'default-daemon')
        c1 = wait_conn(default_env)
        try:
            urllib.request.urlopen('http://127.0.0.1:42715/health', timeout=3)
            fixed_ok = True
        except Exception:
            fixed_ok = False
        check('F1 fixed port 42715 does not serve /health (daemon uses a dynamic port)',
              not fixed_ok and c1['port'] != 42715, 'daemon.conn port=%d' % c1['port'])
        code, msg = health(default_env)
        check('healthy profile-A daemon reports healthy', code == 0, msg)

        # F3: repeated probes change nothing.
        before = daemon_pids()
        for _ in range(5):
            health(default_env)
        c1b = conn(default_env)
        check('F3 probes keep pid, port and startedAtMs unchanged',
              c1b == c1 and proc.poll() is None, 'pid=%d' % c1['pid'])
        check('F3 probes started no extra daemon', daemon_pids() == before, 'daemons: %s' % sorted(before))

        # F5: named profile has no daemon of its own, even though the default one is healthy.
        code, msg = health(named_env)
        check('F5 named profile is unhealthy while only profile A runs', code != 0, msg)
        check('F5 probe did not create a named-profile daemon.conn', conn(named_env) is None)

        # Healthy named profile; killing it must not affect the default verdict, and vice versa.
        named = start(named_env, 'named-daemon')
        n1 = wait_conn(named_env)
        check('named profile publishes its own port', n1['port'] != c1['port'] and n1['pid'] != c1['pid'])
        code, msg = health(named_env)
        check('healthy named-profile daemon reports healthy', code == 0, msg)
        named.send_signal(signal.SIGKILL)
        named.wait()
        code, msg = health(named_env)
        check('F2 stale daemon.conn (daemon killed) is unhealthy', code != 0, msg)
        check('F5 killing the named daemon leaves profile A healthy', health(default_env)[0] == 0)
        check('F3 stale probe did not respawn a daemon', daemon_pids() == {c1['pid']}, 'daemons: %s' % sorted(daemon_pids()))

        # F2: partial conn write on the named profile.
        path = data_root(named_env) / 'daemon.conn'
        path.write_text('{"format":1,"host":"127.0.0.1","po')
        code, msg = health(named_env)
        check('F2 partial daemon.conn is unhealthy', code != 0, msg)
        path.write_text('')
        check('F2 empty daemon.conn is unhealthy', health(named_env)[0] != 0)
        check('F5 profile A stays healthy with a corrupt named record', health(default_env)[0] == 0)

        # F2: live process that does not answer.
        os.kill(c1['pid'], signal.SIGSTOP)
        try:
            code, msg = health(default_env)
            check('F2 live but non-answering daemon is unhealthy', code != 0, msg)
        finally:
            os.kill(c1['pid'], signal.SIGCONT)
        check('F2 daemon recovers once resumed', wait_conn(default_env)['pid'] == c1['pid'])
        check('F3 non-answering probe did not replace the daemon', conn(default_env) == c1)

        # F4: restart on a new port.
        proc.terminate()
        proc.wait(timeout=60)
        code, msg = health(default_env)
        check('stopped daemon is unhealthy', code != 0, msg)
        start(default_env, 'default-daemon-restarted')
        c2 = wait_conn(default_env, not_pid=c1['pid'])
        code, msg = health(default_env)
        check('F4 restarted daemon is healthy again', code == 0, msg)
        check('F4 pid changed after restart', c2['pid'] != c1['pid'], '%d -> %d' % (c1['pid'], c2['pid']))
        check('F4 port observed: %d -> %d' % (c1['port'], c2['port']), True,
              'a changed port is likely but not guaranteed; the check follows daemon.conn either way')
    finally:
        for p in children:
            if p.poll() is None:
                p.terminate()
        for p in children:
            try:
                p.wait(timeout=60)
            except subprocess.TimeoutExpired:
                p.kill()
    failed = [r for r in records if r['result'] == 'FAIL']
    print('%d checks, %d failed' % (len(records), len(failed)))
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
