#!/usr/bin/env python3
"""Real daemon/API/CLI adoption checks. Run with uv --with pynacl --with argon2-cffi.

Use only synthetic profiles, file secure storage and the no-op clipboard.
Failure modes are recorded in .planning/desktop-engine-adoption/failure-modes.md.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request

from argon2.low_level import hash_secret_raw, Type
from nacl.bindings import crypto_aead_xchacha20poly1305_ietf_decrypt, crypto_aead_xchacha20poly1305_ietf_encrypt

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'apps/cli-go/e2e'))
from isolated import isolated_env

PASSWORD = 'synthetic-adoption-passphrase'
UPDATERS = ['skipped_version.json', 'last_notified_update.json', 'update_prompt_throttle.json']


def digest(data):
    return hashlib.sha256(data).hexdigest()


def bundle_members(path):
    data = path.read_bytes()
    header = data[:63]
    memory, iterations, lanes = struct.unpack('<III', header[11:23])
    key = hash_secret_raw(PASSWORD.encode(), header[23:39], iterations, memory, lanes, 32, Type.ID)
    plain = crypto_aead_xchacha20poly1305_ietf_decrypt(data[63:], header, header[39:63], key)
    with tarfile.open(fileobj=io.BytesIO(plain)) as archive:
        members = {m.name: archive.extractfile(m).read() for m in archive if m.isfile()}
    return header, key, members


def incomplete_bundle(source, destination):
    header, key, members = bundle_members(source)
    manifest = json.loads(members['manifest.json'])
    manifest['schema_ver'] = 1
    members['manifest.json'] = json.dumps(manifest).encode()
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w') as archive:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    destination.write_bytes(header + crypto_aead_xchacha20poly1305_ietf_encrypt(buffer.getvalue(), header, header[39:63], key))


class Profile:
    def __init__(self, out, name, daemon, cli):
        self.root = out / name
        self.root.mkdir()
        self.binary = self.root / 'uniclipd'
        shutil.copy2(daemon, self.binary)
        self.cli = self.root / "uniclip"
        shutil.copy2(cli, self.cli)
        self.env = isolated_env(str(self.root / 'home'), 'adoption', {'UC_PORTABLE': '1', 'UC_DAEMON_RUN_MODE': 'standalone'})
        Path(self.env['HOME']).mkdir()
        self.data = self.root / 'data/app.uniclipboard.desktop-adoption'
        self.data.mkdir(parents=True)
        self.product = self.data / 'desktop-settings.json'
        self.legacy = self.data / 'settings.json'
        self.process = None
        self.runs = []
        self.calls = []

    def api(self, method, route, body=None, expected=200, auth=None):
        request = urllib.request.Request(self.endpoint + route, method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={'Content-Type': 'application/json', 'Authorization': auth or 'Session ' + self.session})
        try:
            response = urllib.request.urlopen(request, timeout=90)
        except urllib.error.HTTPError as error:
            response = error
        value = json.loads(response.read())
        self.calls.append({'method': method, 'route': route, 'status': response.code, 'response': value,
                           'inputSha256': digest(json.dumps(body, sort_keys=True).encode())})
        assert response.code == expected, (method, route, response.code, value)
        return value.get('data', value)

    def start(self, **extra):
        self.session = ''
        log = (self.root / f'daemon-{len(self.runs)}.log').open('wb')
        self.process = subprocess.Popen([str(self.binary)], env={**self.env, **extra}, stdout=log, stderr=log)
        log.close()
        self.runs.append({'pid': self.process.pid})
        until = time.monotonic() + 90
        while time.monotonic() < until:
            assert self.process.poll() is None, 'daemon exited before API readiness'
            try:
                connection = json.loads((self.data / 'daemon.conn').read_text())
                if connection['pid'] != self.process.pid:
                    raise ValueError('old connection')
                self.endpoint = 'http://127.0.0.1:' + str(connection['port'])
                self.session = self.api('POST', '/auth/connect', {'pid': os.getpid(), 'clientType': 'cli'},
                    auth='Bearer ' + connection['token'])['sessionToken']
                return
            except (OSError, ValueError, KeyError):
                time.sleep(.2)
        raise AssertionError('API readiness timeout')

    def stop(self):
        process, self.process = self.process, None
        if process is not None:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            try:
                code = process.wait(timeout=40)
            except subprocess.TimeoutExpired:
                process.kill()
                code = process.wait(timeout=10)
            self.runs[-1]['exitCode'] = code
            assert code == 0, code

    def init(self):
        return self.api('POST', '/v2/setup/initialize', {'passphrase': PASSWORD, 'passphraseConfirm': PASSWORD, 'deviceName': 'synthetic-adoption'})

    def snapshot(self, name):
        files = {}
        for field in ['settings.json', 'desktop-settings.json', 'desktop-telemetry.json', *UPDATERS]:
            path = self.data / field
            if path.is_file():
                data = path.read_bytes()
                files[field] = digest(data)
                (self.root / f'{name}-{field}').write_bytes(data)
        return files

    def finish(self):
        try:
            self.stop()
        finally:
            # Authentication responses contain ephemeral tokens; never retain them.
            for call in self.calls:
                if call['route'] == '/auth/connect':
                    call['response'] = {'redacted': True}
            (self.root / 'receipt.json').write_text(json.dumps({'processes': self.runs, 'api': self.calls}, indent=2))


def finish_profiles(profiles):
    errors = []
    for profile in profiles:
        try:
            profile.finish()
        except Exception as error:
            detail = {'profile': profile.root.name, 'error': str(error)}
            errors.append(detail)
            print('Profile cleanup failed: ' + json.dumps(detail), file=sys.stderr)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    profiles, cases = [], []
    result = {'passed': False, 'cases': cases, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'diffSha256': digest(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)),
              'daemonSha256': digest(args.daemon.read_bytes()), 'cliSha256': digest(args.cli.read_bytes()),
              'sourceSha256': {str(path.relative_to(ROOT)): digest(path.read_bytes()) for path in [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', *[ROOT / item for item in subprocess.check_output(['git', 'diff', '--name-only', 'HEAD'], cwd=ROOT, text=True).splitlines() if item.endswith('.rs')], Path(__file__), Path(__file__).with_name('watch.mjs')]},
              'engineSha': '4c363556f6deab7ce93fea679bcba33553db5571'}
    def profile(name):
        p = Profile(out, name, args.daemon.resolve(), args.cli.resolve())
        profiles.append(p)
        return p
    try:
        protected = profile('incomplete-migration')
        protected.legacy.write_text(json.dumps({'schema_version': 3, 'general': {'debug_mode': True, 'theme': 'dark'}}))
        protected.product.mkdir()
        before = protected.legacy.read_bytes()
        with (protected.root / 'failed-start.log').open('wb') as log:
            failure = subprocess.run([str(protected.binary)], env=protected.env, stdout=log, stderr=log, timeout=40)
        protected.runs.append({'exitCode': failure.returncode, 'case': 'preserve-legacy-on-failed-import'})
        assert failure.returncode != 0 and protected.legacy.read_bytes() == before
        assert not (protected.data / 'daemon.conn').exists()
        protected.product.rmdir()
        protected.start()
        assert protected.api('GET', '/settings')['general']['debugMode']
        protected.stop()
        cases.append({'name': 'failed-import-preserves-legacy-and-retries', 'passed': True})
        current = profile('unusable-current-product')
        current.product.mkdir()
        current.start()
        assert current.api('GET', '/settings')['general']['theme'] == 'system'
        current.stop()
        cases.append({'name': 'unusable-product-without-legacy-falls-back', 'passed': True})
        a = profile('direct-upgrade')
        legacy = {'schema_version': 3, 'general': {'debug_mode': True, 'theme': 'dark', 'silent_start': True, 'language': 'ja-JP'},
                  'quick_panel': {'position': 'follow_cursor'}, 'keyboard_shortcuts': {'toggle_quick_panel': 'Alt+Space'}}
        a.legacy.write_text(json.dumps(legacy))
        before = a.snapshot('input')
        a.start()
        settings = a.api('GET', '/settings')
        assert settings['general']['debugMode'] is True and settings['general']['theme'] == 'dark', settings
        assert settings['general']['startupMode'] == 'silent', settings
        assert a.api('GET', '/diagnostics/debug')['effectiveLogProfile'] == 'debug'
        a.init()
        a.api('PUT', '/settings', {'sync': {'syncEnabled': False}})
        engine_settings = json.loads(a.legacy.read_text())
        assert set(engine_settings['general']) <= {'device_name'}, engine_settings['general']
        assert 'keyboard_shortcuts' not in engine_settings and 'quick_panel' not in engine_settings
        imported = a.product.read_bytes()
        for _ in range(2):
            a.stop(); a.start()
            assert a.product.read_bytes() == imported, 'restart reimported erased keys'
            assert a.api('GET', '/settings')['general']['debugMode'] is True
        archives = list(a.root.rglob('*.archive'))
        inventory = {}
        for archive in archives:
            try:
                with tarfile.open(archive) as tar:
                    inventory[str(archive.relative_to(out))] = {m.name: digest(tar.extractfile(m).read()) for m in tar if m.isfile()}
            except tarfile.ReadError:
                continue
        assert any(any(k.endswith('desktop-settings.json') for k in items) for items in inventory.values()), 'upgrade backup missed product file'
        (out / 'upgrade-backup-inventory.json').write_text(json.dumps(inventory, indent=2))
        cases.append({'name': 'D6-upgrade-backup', 'passed': True})
        cases.append({'name': 'D2-direct-and-repeat' , 'passed': True, 'before': before, 'after': a.snapshot('migrated')})
        # Real production CLI against the same isolated daemon, including first save.
        for command in [('debug', 'off'), ('debug', 'status')]:
            run = subprocess.run([str(a.cli), '--json', *command], env=a.env, capture_output=True, text=True, timeout=90)
            (a.root / ('cli-' + command[-1] + '.json')).write_text(json.dumps({'argv': command, 'exitCode': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr}, indent=2))
            assert run.returncode == 0, run.stderr
        status = a.api('GET', '/diagnostics/debug')
        assert not status['debugMode'] and status['restartRequired'] and status['effectiveLogProfile'] == 'debug', status
        a.stop(); a.start(UC_LOG_PROFILE='prod')
        status = a.api('GET', '/diagnostics/debug')
        assert status == {'debugMode': False, 'effectiveLogProfile': 'prod', 'restartRequired': False}, status
        changed = a.api('PUT', '/diagnostics/debug', {'enabled': True})
        assert changed['restartRequired']
        assert a.api('GET', '/diagnostics/debug')['effectiveLogProfile'] == 'prod'
        cases.append({'name': 'D3-cli-env-effective-profile', 'passed': True})
        a.stop(); a.start()
        for name in UPDATERS:
            (a.data / name).write_text('{"synthetic":true}')
        telemetry = (a.data / 'desktop-telemetry.json').read_bytes()
        exported_product = a.product.read_bytes()
        bundle = out / 'config.ucbundle'
        a.api('POST', '/config/export', {'targetPath': str(bundle)})
        _, _, members = bundle_members(bundle)
        (out / 'bundle-inventory.json').write_text(json.dumps({k: digest(v) for k, v in members.items()}, indent=2))
        assert any(k.endswith('desktop-settings.json') and v == exported_product for k, v in members.items())
        assert not any('desktop-telemetry.json' in k for k in members)
        for name in UPDATERS:
            assert any(k.endswith(name) for k in members), name
        a.stop()  # Source and restored identity must never run together.
        for name, existing in [('fresh-import', False), ('existing-import', True)]:
            b = profile(name)
            if existing:
                b.product.write_text(json.dumps({'version': 1, 'general': {'theme': 'light'}, 'import': {'fieldSet': 2, 'userWritten': ['general.theme']}}))
            (b.data / 'desktop-telemetry.json').write_text('false')
            b.start()
            b.api('POST', '/config/import', {'sourcePath': str(bundle), 'password': PASSWORD, 'confirmed': True})
            b.stop(); b.start()
            assert (b.data / 'desktop-telemetry.json').read_text() == 'false'
            assert b.product.read_bytes() == exported_product
            assert b.api('GET', '/settings')['general']['theme'] == 'dark'
            b.api('PUT', '/settings', {'general': {'autoStart': True}})
            assert b.api('GET', '/settings')['general']['theme'] == 'dark', 'stale in-memory product store'
            for updater in UPDATERS:
                assert (b.data / updater).is_file()
            b.snapshot('restored'); b.stop()
            cases.append({'name': name, 'passed': True})
        bad = out / 'old-incomplete.ucbundle'
        incomplete_bundle(bundle, bad)
        b = profile('incomplete-import')
        b.start()
        for route in ['/config/import/preview', '/config/import']:
            response = b.api('POST', route, {'sourcePath': str(bad), 'password': PASSWORD, 'confirmed': True}, expected=422)
            assert response.get('error', response)['code'] == 'INCOMPATIBLE_BUNDLE', response
            assert 're-export' in response.get('error', response)['message'], response
        b.stop()
        cases.append({'name': 'incomplete-bundle-reexport', 'passed': True})
        p = profile('p1-upgrade')
        p.product.write_text(json.dumps({'version': 1, 'general': {'theme': 'light'}, 'import': {'fieldSet': 1, 'userWritten': ['general.theme']}}))
        p.legacy.write_text(json.dumps(legacy))
        p.start()
        settings = p.api('GET', '/settings')
        assert settings['general']['theme'] == 'light' and settings['general']['debugMode'] is True
        p.init()
        p.snapshot('p1-migrated')
        cases.append({'name': 'D1-D2a-field-set-2', 'passed': True})
        event_path = p.root / 'settings-events.jsonl'
        ready = p.root / 'ws.ready'
        watcher = subprocess.Popen(['bun', str(ROOT / 'scripts/e2e/desktop-settings/watch.mjs')], env={
            **os.environ, 'E2E_WS_URL': p.endpoint.replace('http:', 'ws:') + '/ws', 'E2E_SESSION': p.session,
            'E2E_EVENTS': str(event_path), 'E2E_READY': str(ready)})
        until = time.monotonic() + 10
        while not ready.exists():
            assert watcher.poll() is None and time.monotonic() < until, 'WS subscribe failed'
            time.sleep(.1)
        telemetry_before_reset = (p.data / 'desktop-telemetry.json').read_bytes()
        # Deny Engine reset using an invalid engine-owned updater file type.
        marker = p.data / UPDATERS[0]
        marker.mkdir()
        before = p.product.read_bytes()
        p.api('POST', '/encryption/factory-reset', {}, expected=500)
        assert p.product.read_bytes() == before
        marker.rmdir()
        for updater in UPDATERS:
            (p.data / updater).write_text('{"synthetic":true}')
        # Deny host deletion after Engine succeeds. Memory must remain intact.
        p.product.unlink(); p.product.mkdir()
        response = p.api('POST', '/encryption/factory-reset', {}, expected=500)
        assert response.get('error', response)['code'] == 'PRODUCT_SETTINGS_RESET_FAILED', response
        assert p.api('GET', '/settings')['general']['theme'] == 'light', 'failed deletion changed memory'
        assert not any((p.data / updater).exists() for updater in UPDATERS)
        p.product.rmdir(); p.product.write_bytes(before)
        p.api('POST', '/encryption/factory-reset', {})
        time.sleep(.3)
        watcher.terminate(); watcher.wait(timeout=5)
        events = [json.loads(line) for line in event_path.read_text().splitlines()]
        assert any(event.get('type') == 'settings.changed' or event.get('eventType') == 'settings.changed' for event in events), events
        assert (p.data / 'desktop-telemetry.json').read_bytes() == telemetry_before_reset
        assert not p.product.exists()
        assert p.api('GET', '/settings')['general']['theme'] == 'system'
        p.stop(); p.start()
        assert not p.api('GET', '/settings')['general']['debugMode']
        p.stop()
        cases.append({'name': 'D5-reset-failure-host-failure-retry', 'passed': True})
        q = profile('invalid-enums')
        q.legacy.write_text(json.dumps({'schema_version': 3, 'general': {'theme': 'invalid', 'startup_mode': 'invalid', 'debug_mode': 'yes', 'unknown': True}}))
        q.start(); q.init()
        assert q.api('GET', '/settings')['general']['theme'] == 'system'
        assert not q.api('GET', '/diagnostics/debug')['debugMode']
        q.stop()
        cases.append({'name': 'B3-invalid-legacy-enums', 'passed': True})
        result['passed'] = True
    finally:
        original_failure = sys.exc_info()[0] is not None
        cleanup_errors = []
        if 'watcher' in locals() and watcher.poll() is None:
            try:
                watcher.terminate()
                try:
                    watcher.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    watcher.kill(); watcher.wait(timeout=5)
            except Exception as error:
                cleanup_errors.append({'profile': 'settings-observer', 'error': str(error)})
        cleanup_errors.extend(finish_profiles(profiles))
        result['cleanupErrors'] = cleanup_errors
        if cleanup_errors:
            result['passed'] = False
        (out / 'result.json').write_text(json.dumps(result, indent=2))
        if cleanup_errors and not original_failure:
            raise RuntimeError('profile cleanup failed; see result.json')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
