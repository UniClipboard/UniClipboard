#!/usr/bin/env python3
"""Real daemon/production CLI checks of retained rejected imports and replacement consent."""
import argparse
import json
from pathlib import Path
import subprocess
import tomllib

from adoption import Profile, PASSWORD, ROOT, bundle_members, digest, finish_profiles


def inventory(profile):
    paths = [profile.data / 'pending-import.json', *sorted((profile.data / 'import-staging').rglob('*'))]
    return {str(p.relative_to(profile.data)): digest(p.read_bytes()) if p.is_file() else 'directory' for p in paths}


def cli(profile, *args, expected=0):
    result = subprocess.run([str(profile.cli), '--json', *args], env=profile.env, capture_output=True, timeout=90)
    profile.calls.append({'route': 'cli', 'cli': args, 'exitCode': result.returncode,
                          'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()})
    assert result.returncode == expected, (args, result.returncode, result.stdout, result.stderr)
    return result.stdout.decode()


def cache_read(process, command):
    process.stdin.write(command + '\n'); process.stdin.flush()
    while True:
        line = process.stdout.readline()
        if not line: raise AssertionError('Frontend cache driver exited before its receipt')
        if line.startswith('CACHE_RECEIPT '): return json.loads(line[len('CACHE_RECEIPT '):])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve(); out.mkdir(parents=True, exist_ok=False)
    profiles, cases = [], []
    cache = None
    cache_reads = []
    result = {'passed': False, 'cases': cases,
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'diffSha256': digest(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)),
              'engineSha': tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['dependencies']['uc-engine']['rev'],
              'daemonSha256': digest(args.daemon.read_bytes()), 'cliSha256': digest(args.cli.read_bytes()),
              'statusCacheDriverSha256': digest(Path(__file__).with_name('status-cache.ts').read_bytes()),
              'sourceSha256': {str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in [Path(__file__).resolve(), ROOT / 'Cargo.toml', ROOT / 'Cargo.lock']}}
    def create(name):
        p = Profile(out, name, args.daemon.resolve(), args.cli.resolve()); profiles.append(p); return p
    try:
        source = create('source'); source.start(); source.init()
        source.api('PUT', '/settings', {'general': {'theme': 'dark', 'language': 'ja-JP'}})
        bundle = out / 'new-export.ucbundle'
        source.api('POST', '/config/export', {'targetPath': str(bundle)})
        _, _, members = bundle_members(bundle)
        assert 'ui-state/desktop-settings.json' in members
        source.stop()
        target = create('original'); target.start(); target.init()
        sent = json.loads(cli(target, 'send', '--text', 'synthetic-original-preserved-content', expected=1))
        assert sent['snapshotHash'] and sent['perTarget'] == [] and sent['totalErrored'] == 0
        assert 'synthetic-original-preserved-content' in cli(target, 'get', '--list')
        original_identity = (target.data / 'vault/device_id.txt').read_bytes()
        target.stop()
        staging = target.data / 'import-staging'; staging.mkdir()
        # Reproduce an older incomplete staged import, with the outside package absent.
        for name, data in members.items():
            if name.startswith(('runtime-', 'db/control', 'vault/profile-secrets')): continue
            path = staging / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(data)
        (staging / 'unique-preserved-input.bin').write_bytes(b'unique synthetic staged input')
        marker = target.data / 'pending-import.json'
        old_marker = {'schema_ver': 1, 'staging_dir': 'import-staging', 'has_kek': True, 'staged_at_unix_ms': 1}
        marker.write_text(json.dumps(old_marker))
        retained = inventory(target)
        (out / 'retained-before.json').write_text(json.dumps(retained, indent=2))
        for restart in range(2):
            target.start()
            for _ in range(2):
                state = target.api('GET', '/config/import/status')['rejectedImport']
                assert state['reason'] == 'unsupported_format' and 'Re-export' in state['message']
            assert (target.data / 'vault/device_id.txt').read_bytes() == original_identity
            assert 'synthetic-original-preserved-content' in cli(target, 'get', '--list')
            assert inventory(target) == retained
            target.stop()
        cases.append('old-staging-original-space-history-and-repeated-query-survive-two-starts')
        target.start()
        request = {'sourcePath': str(bundle), 'password': PASSWORD, 'confirmed': True}
        response = target.api('POST', '/config/import', request, expected=422)
        assert response['code'] == 'INCOMPATIBLE_BUNDLE' and inventory(target) == retained
        response = target.api('POST', '/config/import', {**request, 'replaceRejectedImport': False}, expected=422)
        assert response['code'] == 'INCOMPATIBLE_BUNDLE' and inventory(target) == retained
        response = target.api('POST', '/config/import', {**request, 'password': 'wrong', 'replaceRejectedImport': True}, expected=400)
        assert response['code'] == 'INVALID_PASSWORD_OR_CORRUPT' and inventory(target) == retained
        cases.append('omitted-and-explicit-false-consent-and-wrong-password-preserve-every-byte')
        # A filesystem error after successful startup must remain an API failure, never null.
        cache_log = (out / 'status-cache-stderr.log').open('w')
        cache = subprocess.Popen(['bun', str(Path(__file__).with_name('status-cache.ts').resolve())],
            cwd=ROOT / 'apps/gui-go/frontend', stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=cache_log, text=True)
        cache_log.close()
        assert cache_read(cache, json.dumps({'endpoint': target.endpoint, 'session': target.session}))['ready']
        initial = cache_read(cache, 'read'); cache_reads.append(initial)
        assert initial['isSuccess'] and initial['rejected'] == 'unsupported_format'
        marker.rename(target.data / 'marker-preserved-for-io-case')
        marker.mkdir()
        target.api('GET', '/config/import/status', expected=500)
        failed = cache_read(cache, 'read'); cache_reads.append(failed)
        assert failed['isError'] and not failed['isSuccess']
        marker.rmdir(); (target.data / 'marker-preserved-for-io-case').rename(marker)
        recovered = cache_read(cache, 'read'); cache_reads.append(recovered)
        assert recovered['isSuccess'] and not recovered['isError'] and recovered['rejected'] == 'unsupported_format'
        cache.stdin.write('stop\n'); cache.stdin.flush()
        code = cache.wait(timeout=20)
        (out / 'status-cache-receipt.json').write_text(json.dumps({'reads': cache_reads, 'exitCode': code}, indent=2))
        assert code == 0
        assert inventory(target) == retained
        cases.append('diagnostics-io-error-propagates-and-real-frontend-cache-recovers-with-staging-preserved')
        target.stop()
        for reason, contents in [('malformed_marker', b'{'), ('missing_runtime_members', json.dumps({**old_marker, 'schema_ver': 2}).encode())]:
            marker.write_bytes(contents); before = inventory(target); target.start()
            assert target.api('GET', '/config/import/status')['rejectedImport']['reason'] == reason
            assert inventory(target) == before
            assert 'synthetic-original-preserved-content' in cli(target, 'get', '--list')
            target.stop()
        marker.write_text(json.dumps(old_marker)); assert inventory(target) == retained
        target.start()
        assert target.api('POST', '/config/import/preview', {'sourcePath': str(bundle), 'password': PASSWORD})['appVersion']
        assert target.api('POST', '/config/import', {**request, 'replaceRejectedImport': True})['stagedOk']
        target.stop(); target.start()
        assert target.api('GET', '/config/import/status')['rejectedImport'] is None
        settings = target.api('GET', '/settings')['general']
        assert settings['theme'] == 'dark' and settings['language'] == 'ja-JP'
        assert target.product.read_bytes() == members['ui-state/desktop-settings.json']
        assert (target.data / 'vault/device_id.txt').read_bytes() == members['vault/device_id.txt']
        # Validate real adoption separately from the absent rejection above.
        target.api('POST', '/config/export', {'targetPath': str(out / 'reexport-after-recovery.ucbundle')})
        cases.append('three-reasons-and-explicit-replacement-real-apply-and-reexport')
        # Sparse input crosses the aggregate bound without allocating/reading 2 GiB.
        blobs = list(target.data.glob('profile-data-generations/*/v3-payloads/blobs'))
        assert len(blobs) == 1, blobs
        huge = blobs[0] / 'synthetic-export-limit.bin'
        with huge.open('wb') as file: file.truncate(2 * 1024**3)
        destination = out / 'preserved-export-output.ucbundle'; destination.write_bytes(b'previous export stays')
        before_output = digest(destination.read_bytes())
        failure = target.api('POST', '/config/export', {'targetPath': str(destination)}, expected=500)
        assert failure['code'] == 'internal_error' and digest(destination.read_bytes()) == before_output
        huge.unlink()
        assert target.api('GET', '/settings')['general']['theme'] == 'dark'
        cases.append('aggregate-export-limit-propagates-with-existing-output-preserved')
        result['passed'] = True
    except Exception as error:
        result['error'] = repr(error)
        raise
    finally:
        cache_cleanup_error = None
        if cache is not None:
            try:
                if cache.poll() is None:
                    try: cache.stdin.close()
                    except BrokenPipeError: pass
                    try: cache.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        cache.terminate()
                        try: cache.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            cache.kill(); cache.wait(timeout=10)
                result['statusCacheExitCode'] = cache.returncode
            except Exception as cleanup_error:
                cache_cleanup_error = str(cleanup_error)
            (out / 'status-cache-receipt.json').write_text(json.dumps({
                'reads': cache_reads, 'exitCode': cache.returncode, 'cleanupError': cache_cleanup_error}, indent=2))
        result['cleanupErrors'] = finish_profiles(profiles)
        if cache_cleanup_error:
            result['cleanupErrors'].append({'profile': 'status-cache', 'error': cache_cleanup_error})
        if result['cleanupErrors']: result['passed'] = False
        (out / 'result.json').write_text(json.dumps(result, indent=2))
    assert result['passed']
    print(json.dumps({'passed': True, 'cases': len(cases), 'artifact': str(out / 'result.json')}))


if __name__ == '__main__': main()
