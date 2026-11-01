"""Real isolated daemon history recovery; called by diagnostics_acceptance_run.py."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import zipfile
from run import ROOT, isolated_env, pid_alive
from file_preview_run import cli, PASSPHRASE


def run(out):
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home = Path(tempfile.mkdtemp(prefix='uc-gui-go-history-recovery-'))
    profile = 'gui-go-' + home.name
    env = isolated_env(str(home), profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    data = home / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile)
    directory = home / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile) / 'otel-history'
    history = directory / 'history.enc'
    evidence = directory / 'history.quarantine.enc'
    report = {'passed': False, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'enginePin': next(line for line in (ROOT / 'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine =')), 'cases': {}, 'cleanupFailures': {}}
    running = False
    held = None

    def digest(content):
        return hashlib.sha256(content).hexdigest()

    def start():
        nonlocal running
        cli(env, 'start')
        running = True

    def stop():
        nonlocal running
        conn = json.loads((data / 'daemon.conn').read_text()) if (data / 'daemon.conn').exists() else None
        done = cli(env, 'stop', check=False, timeout=30)
        assert done.returncode == 0, 'owned daemon stop failed'
        deadline = time.monotonic() + 15
        while conn and pid_alive(conn['pid']) and time.monotonic() < deadline:
            time.sleep(0.1)
        assert not conn or not pid_alive(conn['pid']), 'owned daemon still running'
        running = False

    def export(name):
        value = json.loads(cli(env, '--json', 'debug', 'export-logs').stdout)
        target = out / (name + '.zip')
        target.write_bytes(Path(value['path']).read_bytes())
        with zipfile.ZipFile(target) as archive:
            manifest = json.loads(archive.read('otel/manifest.json'))
        return manifest['history'], digest(target.read_bytes())

    def wait_encrypted(name):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            summary, sha = export(name)
            if summary['persistence'] == 'encrypted' and history.exists():
                return summary, sha
            time.sleep(0.2)
        raise AssertionError('history persistence did not recover: ' + summary['persistence'])

    def unchanged(name, original, expected):
        for index in range(2):
            summary, sha = export(name + '-' + str(index))
            assert summary['persistence'] == expected, summary['persistence']
            assert history.read_bytes() == original, 'unavailable key/IO overwrote original history'
            assert not evidence.exists(), 'unavailable key/IO quarantined original history'
            time.sleep(1)
        report['cases'][name] = {'passed': True, 'state': summary['persistence'], 'originalSha256': digest(original), 'zipSha256': sha}

    try:
        cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'synthetic-history-recovery')
        start()
        wait_encrypted('seed')
        stop()
        original = history.read_bytes()
        assert original.startswith(b'UCP3'), 'fixture requires the current V3 profile cipher'
        (out / 'seed.enc').write_bytes(original)

        history.write_bytes(b'invalid-encrypted-history-fixture')
        start()
        summary, sha = wait_encrypted('initial-corrupt')
        assert evidence.read_bytes() == b'invalid-encrypted-history-fixture'
        report['cases']['initial-corrupt'] = {'passed': True, 'zipSha256': sha}
        stop()
        evidence.unlink()
        history.write_bytes(original)

        # Exercise the error that the V3 adapter maps into InvalidCiphertext despite missing key material.
        missing = bytearray(original)
        key_length = int.from_bytes(missing[7:9], 'big')
        key_last = 41 + key_length - 1
        missing[key_last] = ord('0') if missing[key_last] != ord('0') else ord('1')
        history.write_bytes(missing)
        start()
        unchanged('missing-historical-key', bytes(missing), 'key_unavailable')
        stop()
        history.write_bytes(original)
        start()
        wait_encrypted('restored-key-history')
        stop()

        # Locked startup plus invalid framing must not be mistaken for authorized recovery.
        keys = list((data / 'keyring').glob('*.bin'))
        assert len(keys) == 1, 'unexpected isolated development key store'
        held = (keys[0], keys[0].with_suffix('.held'))
        held[0].rename(held[1])
        damaged = b'invalid-encrypted-history-fixture'
        history.write_bytes(damaged)
        start()
        unchanged('locked-invalid-history', damaged, 'key_unavailable')
        stop()
        held[1].rename(held[0])
        held = None

        for name, payload in [('invalid-framing', damaged), ('invalid-authentication', original[:-1] + bytes([original[-1] ^ 1])), ('oversized', original + b'\x00' * (8 * 1024 * 1024 + 1))]:
            history.write_bytes(payload)
            before = digest(payload)
            start()
            summary, sha = wait_encrypted(name)
            fresh = history.read_bytes()
            assert fresh != payload and fresh.startswith(b'UCP3')
            assert evidence.read_bytes() == payload[:8 * 1024 * 1024]
            recovery = summary['historyRecovery']
            assert recovery['originalBytes'] == len(payload)
            assert recovery['retainedBytes'] == min(len(payload), 8 * 1024 * 1024)
            assert recovery['truncated'] == (len(payload) > 8 * 1024 * 1024)
            assert b'resourceMetrics' not in fresh and b'process.memory.usage' not in fresh
            assert len(list(directory.glob('history.quarantine*'))) == 1
            (out / (name + '-evidence.enc')).write_bytes(evidence.read_bytes())
            old_runs = summary['runId']
            stop()
            start()
            after, restart_sha = wait_encrypted(name + '-restart')
            assert after['runId'] != old_runs
            assert after['historyRecovery'] == recovery
            with zipfile.ZipFile(out / (name + '-restart.zip')) as archive:
                assert old_runs.encode() in archive.read('otel/metrics.jsonl'), 'previous run was not reloaded'
            report['cases'][name] = {'passed': True, 'damagedSha256': before, 'evidenceSha256': digest(evidence.read_bytes()), 'freshSha256': digest(fresh), 'zipSha256': sha, 'restartZipSha256': restart_sha, 'recovery': recovery, 'restartReloaded': True}
            stop()

        # IO denial is retryable and must not touch either original ciphertext or evidence slots.
        evidence.unlink()
        observer = history.open('rb')
        before_io = observer.read()
        inode = os.fstat(observer.fileno()).st_ino
        history.chmod(0)
        start()
        try:
            for index in range(2):
                summary, sha = export('io-denied-' + str(index))
                assert summary['persistence'] == 'io_failure'
                assert not evidence.exists()
                assert history.stat().st_ino == inode
                observer.seek(0)
                assert observer.read() == before_io
                time.sleep(0.5)
        finally:
            observer.close()
            history.chmod(0o600)
        wait_encrypted('io-restored')
        stop()
        report['cases']['io-denied'] = {'passed': True, 'originalSha256': digest(before_io), 'unchangedDuringDenial': True, 'recoveredAfterPermissionRestore': True}
        report['passed'] = True
    finally:
        body_failed = sys.exc_info()[0] is not None
        if body_failed:
            report['scenarioErrorType'] = type(sys.exc_info()[1]).__name__
        if held:
            try:
                held[1].rename(held[0])
            except Exception as error:
                report['cleanupFailures']['restoreKeyStore'] = type(error).__name__
        if history.exists():
            try:
                history.chmod(0o600)
            except Exception as error:
                report['cleanupFailures']['restorePermissions'] = type(error).__name__
        if running:
            try:
                stop()
            except Exception as error:
                report['cleanupFailures']['daemonStop'] = type(error).__name__
        for name in ['target/debug/uniclipd', 'target/gui-go/uniclip']:
            report.setdefault('binarySha256', {})[name] = None
            try:
                report.setdefault('binarySha256', {})[name] = digest((ROOT / name).read_bytes())
            except Exception as error:
                report['cleanupFailures']['hash:' + name] = type(error).__name__
        if report['cleanupFailures']:
            report['passed'] = False
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        if report['cleanupFailures'] and not body_failed:
            raise RuntimeError('history acceptance cleanup failed; inspect result.json')
