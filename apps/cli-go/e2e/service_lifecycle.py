#!/usr/bin/env python3
"""Real isolated foreground and user-service acceptance, with reproducible artifacts.

Usage: service_lifecycle.py --bin DIR --out DIR
macOS requires a GUI login domain. Linux requires a live user systemd manager.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from isolated import isolated_env


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--bin', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    home = out / ('home with spaces ' + uuid.uuid4().hex[:10])
    home.mkdir()
    bindir = Path(args.bin).resolve()
    binary = str(bindir / 'uniclip')
    profile = 'service-e2e-' + uuid.uuid4().hex[:12]
    env = isolated_env(str(home), profile)
    records = []
    children = []
    service_name = None

    def record(label, **values):
        records.append(dict(label=label, **values))
        (out / 'result.json').write_text(json.dumps(records, indent=2))

    def run(label, argv, expected=0, selected_env=env):
        result = subprocess.run([binary, *argv], env=selected_env, capture_output=True,
                                text=True, timeout=100)
        record(label, argv=argv, exit=result.returncode, stdout=result.stdout, stderr=result.stderr)
        if expected is not None:
            assert result.returncode == expected, (label, result.stderr)
        return result

    def spawn(label, argv, selected_env=env):
        log = open(out / (label + '.log'), 'w')
        child = subprocess.Popen([binary, *argv], env=selected_env, stdout=log, stderr=log)
        log.close()
        children.append(child)
        return child

    def wait_status(label):
        deadline = time.monotonic() + 80
        while time.monotonic() < deadline:
            result = run(label, ['--json', 'service', 'status'], expected=None)
            state = json.loads(result.stdout)
            if state['running'] and state['http_health'] in ('ok', 'recovery_required'):
                return state
            time.sleep(.5)
        raise RuntimeError('service health timeout')

    def conn_path():
        if os.uname().sysname == 'Darwin':
            return home / 'Library' / 'Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
        return home / '.local' / 'share' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'

    def wait_conn(child):
        deadline = time.monotonic() + 80
        while time.monotonic() < deadline:
            assert child.poll() is None, 'foreground exited before publishing endpoint'
            if conn_path().exists():
                conn = json.loads(conn_path().read_text())
                if conn['pid'] == child.pid:
                    return conn
            time.sleep(.2)
        raise RuntimeError('foreground startup timeout')

    try:
        record('provenance', source=subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
               binaries={name: hashlib.sha256((bindir / name).read_bytes()).hexdigest()
                         for name in ('uniclip', 'uniclipd')}, home=str(home), profile=profile)
        run('run-help', ['run', '--help'])
        run('service-help', ['service', '--help'])
        portable = dict(env, UC_PORTABLE='1')
        run('reject-portable-service', ['service', 'start'], expected=1, selected_env=portable)
        invalid_portable = dict(portable, APPDIR=str(bindir), APPIMAGE=str(out / 'missing.AppImage'))
        invalid = run('reject-invalid-portable-request', ['service', 'start'],
                      expected=1, selected_env=invalid_portable)
        assert 'without a valid $APPIMAGE' in invalid.stderr
        initial = run('not-installed', ['--json', 'service', 'status'], expected=1)
        service_name = json.loads(initial.stdout)['name']
        assert not json.loads(initial.stdout)['installed']
        child = spawn('foreground-term', ['run', '--server'])
        conn = wait_conn(child)
        assert conn['pid'] == child.pid, 'Unix foreground must be the daemon itself'
        run('refuse-incumbent-run', ['run', '--server'], expected=1)
        run('refuse-incumbent-service', ['service', 'start', '--server'], expected=1)
        assert child.poll() is None
        contention = subprocess.run([str(bindir / 'uniclipd')],
                                    env=dict(env, UC_DAEMON_NO_TAKEOVER='1', UC_DAEMON_RUN_MODE='server'),
                                    capture_output=True, text=True, timeout=30)
        record('atomic-no-takeover', exit=contention.returncode, stderr=contention.stderr)
        assert contention.returncode != 0 and child.poll() is None
        run('initialize-isolated-space', ['space', 'init', '--passphrase', 'synthetic-e2e-passphrase',
                                         '--device-name', 'Isolated service acceptance'])
        child.send_signal(signal.SIGTERM)
        child.wait(timeout=80)
        record('foreground-term-exit', exit=child.returncode)
        assert child.returncode == 0
        child = spawn('foreground-interrupt', ['run', '--server'])
        wait_conn(child)
        child.send_signal(signal.SIGINT)
        child.wait(timeout=80)
        record('foreground-interrupt-exit', exit=child.returncode)
        assert child.returncode == 0

        # Process-boundary fixture tests exit propagation, without claiming daemon behavior.
        fixture = out / 'exit fixture'
        fixture.mkdir()
        shutil.copy2(binary, fixture / 'uniclip')
        shim = fixture / 'uniclipd'
        shim.write_text('#!/bin/sh\necho visible-daemon-failure >&2\nexit 37\n')
        shim.chmod(0o755)
        result = subprocess.run([str(fixture / 'uniclip'), 'run'], env=env,
                                capture_output=True, text=True, timeout=10)
        record('process-fixture-exit', exit=result.returncode, stderr=result.stderr)
        assert result.returncode == 37 and 'visible-daemon-failure' in result.stderr

        run('service-start', ['service', 'start', '--server'])
        first = wait_status('service-status')
        run('service-start-again', ['service', 'start', '--server'])
        same = wait_status('service-status-again')
        assert first['pid'] == same['pid']
        run('reject-running-option-change', ['service', 'start'], expected=1)
        other = isolated_env(str(home), profile + '-other')
        isolated = run('profile-isolation', ['--json', 'service', 'status'], expected=1, selected_env=other)
        assert not json.loads(isolated.stdout)['installed']
        assert json.loads(isolated.stdout)['name'] != service_name
        run('service-restart', ['service', 'restart'])
        restarted = wait_status('service-status-restarted')
        assert restarted['pid'] != first['pid']
        path = (home / 'Library' / 'LaunchAgents' / (service_name + '.plist') if os.uname().sysname == 'Darwin'
                else home / '.config' / 'systemd' / 'user' / (service_name + '.service'))
        shutil.copy2(path, out / path.name)
        definition = path.read_text()
        assert str(home) in definition and profile in definition
        assert 'development' in definition and 'UC_DISABLE_SYSTEM_CLIPBOARD' in definition
        assert 'synthetic-e2e-passphrase' not in definition
        # SIGSTOP only the isolated managed daemon: PID stays present, HTTP must fail.
        os.kill(restarted['pid'], signal.SIGSTOP)
        try:
            health = run('running-http-failure', ['--json', 'service', 'status'], expected=1)
            assert json.loads(health.stdout)['running']
            assert json.loads(health.stdout)['http_health'] == 'unreachable'
        finally:
            os.kill(restarted['pid'], signal.SIGCONT)
        run('service-stop', ['service', 'stop'])
        run('service-stop-again', ['service', 'stop'])
        stopped = run('service-stopped', ['--json', 'service', 'status'], expected=1)
        assert not json.loads(stopped.stdout)['running']
        run('service-restart-after-stop', ['service', 'restart'])
        wait_status('service-status-restart-after-stop')
        run('service-start-after-stop', ['service', 'start', '--server'])
        wait_status('service-status-after-stop')
        legacy = run('legacy-start', ['start', '--server'])
        assert 'deprecated' in legacy.stderr
        # Legacy start must return without blocking or replacing the managed daemon.
        run('service-final-stop', ['service', 'stop'])
        cold_legacy = run('legacy-background-cold-start', ['start', '--server'])
        assert 'deprecated' in cold_legacy.stderr
        legacy_conn = json.loads(conn_path().read_text())
        record('legacy-background-pid', pid=legacy_conn['pid'])
        run('legacy-background-stop', ['stop'])
        production = dict(env, UNICLIPBOARD_ENV='production')
        run('reject-production-worktree', ['service', 'start', '--server'], expected=1, selected_env=production)
        record('acceptance', status='passed')
    finally:
        original_error = sys.exc_info()[0] is not None
        cleanup_errors = []
        for child in children:
            try:
                if child.poll() is None:
                    child.send_signal(signal.SIGTERM)
                    try:
                        child.wait(timeout=80)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=10)
            except Exception as error:
                cleanup_errors.append(f'child {child.pid}: {error}')
        if service_name:
            # All native operations address only the hash of this throwaway HOME/profile.
            for label, argv in [('cleanup-stop', ['service', 'stop']),
                                ('cleanup-profile-stop', ['stop'])]:
                try:
                    run(label, argv, expected=None)
                except Exception as error:
                    cleanup_errors.append(f'{label}: {error}')
            macos = os.uname().sysname == 'Darwin'
            path = (home / 'Library' / 'LaunchAgents' / (service_name + '.plist') if macos
                    else home / '.config' / 'systemd' / 'user' / (service_name + '.service'))
            loaded = None
            try:
                if macos:
                    target = f'gui/{os.getuid()}/{service_name}'
                    subprocess.run(['/bin/launchctl', 'enable', target], capture_output=True)
                    check = subprocess.run(['/bin/launchctl', 'print', target], capture_output=True, text=True)
                    loaded = check.returncode == 0
                else:
                    check = subprocess.run(['systemctl', '--user', 'is-active', service_name + '.service'],
                                           env=env, capture_output=True, text=True)
                    loaded = check.returncode == 0
                    record('cleanup-systemd-state', exit=check.returncode,
                           stdout=check.stdout, stderr=check.stderr)
                    if check.returncode not in (0, 3, 4):
                        cleanup_errors.append('systemd state check failed')
            except Exception as error:
                cleanup_errors.append(f'native state check: {error}')
            finally:
                try:
                    path.unlink(missing_ok=True)
                    if not macos:
                        reload = subprocess.run(['systemctl', '--user', 'daemon-reload'],
                                                env=env, capture_output=True, text=True)
                        if reload.returncode != 0:
                            cleanup_errors.append(f'daemon-reload: {reload.stderr}')
                except Exception as error:
                    cleanup_errors.append(f'definition removal: {error}')
            record('cleanup', loaded=loaded, definition_exists=path.exists(),
                   foreground_alive=[p.pid for p in children if p.poll() is None], errors=cleanup_errors)
            if loaded or path.exists() or any(p.poll() is None for p in children):
                cleanup_errors.append('service definition or owned process remains')
        if cleanup_errors:
            if original_error:
                print('Cleanup errors: ' + '; '.join(cleanup_errors), file=sys.stderr)
            else:
                raise RuntimeError('Cleanup errors: ' + '; '.join(cleanup_errors))


if __name__ == '__main__':
    main()
