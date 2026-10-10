#!/usr/bin/env python3
"""Verify Go CLI / bundled Rust daemon path agreement across isolated profiles.

Failure modes and scope are recorded in the GUI host retirement document.
This does not launch the GUI, touch the system clipboard or register services.
"""
import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env  # noqa: E402


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--cli', required=True, type=Path)
    parser.add_argument('--daemon', required=True, type=Path)
    args = parser.parse_args()
    if sys.platform not in ('darwin', 'linux'):
        parser.error('this isolated path runner supports macOS and Linux')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-profile-layout-'))
    result = {'passed': False, 'sandbox': str(sandbox), 'platform': platform.platform(),
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'diffSha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)).hexdigest(),
              'inputs': {'cli': sha256(args.cli), 'daemon': sha256(args.daemon)}, 'cases': []}
    try:
        for case, profile, portable in [('unset', None, False), ('empty', '', False),
                                        ('explicit', 'gui-go-profile-layout', False),
                                        ('portable', 'gui-go-profile-layout', True)]:
            work = sandbox / case
            home = work / 'home'
            home.mkdir(parents=True)
            cli = work / 'uniclip'
            daemon = work / 'uniclipd'
            shutil.copy2(args.cli, cli)
            shutil.copy2(args.daemon, daemon)
            env = isolated_env(str(home), profile or '')
            if profile is None:
                env.pop('UC_PROFILE', None)
            if portable:
                env['UC_PORTABLE'] = '1'
            name = 'app.uniclipboard.desktop' + (('-' + profile) if profile else '')
            base = work / 'data' if portable else (
                home / 'Library/Application Support' if sys.platform == 'darwin' else home / '.local/share')
            data = base / name
            logs = work / 'data/logs' if portable else (
                home / 'Library/Logs' / name if sys.platform == 'darwin' else home / '.local/state' / name / 'logs')
            record = {'case': case, 'data': str(data), 'logs': str(logs), 'commands': [], 'passed': False}
            result['cases'].append(record)

            def run(*argv, check=True):
                p = subprocess.run([str(cli), '--json', *argv], env=env, capture_output=True, text=True, timeout=120)
                record['commands'].append({'argv': list(argv), 'code': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr})
                if check and p.returncode:
                    raise RuntimeError(f'{case}: {argv[0]} failed ({p.returncode})')
                return p

            try:
                run('space', 'init', '--passphrase', 'profile-layout-e2e-passphrase', '--device-name', 'layout-e2e')
                run('start')
                conn = json.loads((data / 'daemon.conn').read_text())
                record['pids'] = [conn['pid']]
                # Confirm the path Go discovered was written by this sandbox's Rust daemon.
                executable = (os.readlink(f'/proc/{conn["pid"]}/exe') if sys.platform == 'linux' else
                              subprocess.check_output(['ps', '-p', str(conn['pid']), '-o', 'comm='], text=True).strip())
                assert Path(executable).resolve() == daemon.resolve(), executable
                first = json.loads(run('space', 'status').stdout)
                assert first, 'empty space status'
                run('stop')
                run('start')
                record['pids'].append(json.loads((data / 'daemon.conn').read_text())['pid'])
                second = json.loads(run('space', 'status').stdout)
                assert first == second, 'space state changed across daemon restart'
                assert logs.is_dir() and list(logs.glob('uniclipboard-daemon.json.*')), 'missing per-role daemon log'
                assert not (data / 'logs').exists(), 'logs incorrectly written inside data root'
                assert (data / 'keyring').is_dir(), 'development did not use the file keystore'
                record.update({'statusBefore': first, 'statusAfter': second, 'passed': True})
            finally:
                stopped = run('stop', check=False)
                alive = []
                for pid in record.get('pids', []):
                    if sys.platform == 'linux':
                        try:
                            running = os.readlink(f'/proc/{pid}/exe')
                        except FileNotFoundError:
                            running = ''
                    else:
                        p = subprocess.run(['ps', '-p', str(pid), '-o', 'comm='], capture_output=True, text=True)
                        running = p.stdout.strip()
                    if running == str(daemon):
                        alive.append(pid)
                record['daemonPidsAliveAfterStop'] = alive
                if alive:
                    record['passed'] = False
                    raise RuntimeError('sandbox daemon remains after stop')
                record['cleanupExit'] = stopped.returncode
                retained = out / case / 'logs'
                retained.mkdir(parents=True)
                if logs.is_dir():
                    for path in logs.glob('uniclipboard-daemon.json.*'):
                        if path.is_file():
                            shutil.copy2(path, retained / path.name)
        result['passed'] = all(row['passed'] for row in result['cases'])
    finally:
        (out / 'assertions.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'passed': result['passed'], 'cases': len(result['cases'])}))


if __name__ == '__main__':
    main()
