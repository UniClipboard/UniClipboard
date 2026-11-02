"""Real daemon acceptance of memory-only OTLP collection through the existing entry.

Failures: automatic history writes; encrypted attachments; lost readable metrics;
records reloaded after restart; stale history used as an offline source; failed cleanup.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import zipfile
from run import ROOT, isolated_env, pid_alive
from file_preview_run import cli, PASSPHRASE

GO_EXPORT = '''package main
import (
 "os"
 "github.com/UniClipboard/UniClipboard/packages/desktop-host-go/diaglogs"
)
func main() {
 if _, err := diaglogs.ExportStartup(os.Args[1], os.Args[2], nil); err != nil { os.Exit(1) }
}
'''


def run(out):
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home = Path(tempfile.mkdtemp(prefix='uc-gui-go-memory-only-'))
    profile = 'gui-go-' + home.name
    env = isolated_env(str(home), profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    logs = home / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile)
    connection = home / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
    report = {'passed': False, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True)),
              'enginePin': next(line for line in (ROOT / 'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine =')),
              'checks': {}, 'cleanupFailures': {}}
    running = False

    def stop():
        nonlocal running
        pid = json.loads(connection.read_text())['pid'] if connection.exists() else None
        assert cli(env, 'stop', check=False).returncode == 0
        deadline = time.monotonic() + 15
        while pid and pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(0.1)
        assert not pid or not pid_alive(pid), 'owned daemon still running'
        running = False

    def export(name):
        result = json.loads(cli(env, '--json', 'debug', 'export-logs').stdout)
        archive = out / (name + '.zip')
        shutil.copyfile(result['path'], archive)
        with zipfile.ZipFile(archive) as z:
            manifest = json.loads(z.read('otel/manifest.json'))
            assert not any(n.endswith('.enc') for n in z.namelist())
            metrics = z.read('otel/metrics.jsonl').decode()
            assert 'process.memory.usage' in metrics and 'uc.process.cpu.time' in metrics
            assert not {'persistence', 'persistenceTrimmedRecords', 'historyRecovery'} & manifest['history'].keys()
            for item in manifest['files']:
                content = z.read(item['name'])
                assert len(content) == item['bytes']
                assert hashlib.sha256(content).hexdigest() == item['sha256']
            report['checks'][name] = hashlib.sha256(archive.read_bytes()).hexdigest()
            return manifest['history']['runId'], metrics

    try:
        cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'synthetic-memory-diagnostics')
        running = True
        cli(env, 'start')
        first, _ = export('live')
        assert not (logs / 'otel-history').exists(), 'collector persisted diagnostic history'
        stop()
        assert not (logs / 'otel-history').exists(), 'shutdown persisted diagnostic history'
        running = True
        cli(env, 'start')
        second, metrics = export('restarted')
        assert second != first and first not in metrics, 'previous process history reloaded'
        stop()
        report['checks']['noAutomaticHistoryFiles'] = True
        report['checks']['restartStartsEmpty'] = True

        # Retired ciphertext is synthetic. Exporters must ignore it without modifying it.
        stale = logs / 'otel-history/history.enc'
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b'synthetic-retired-ciphertext')
        go_source = out / 'offline_export.go'
        go_source.write_text(GO_EXPORT)
        go_binary = out / 'offline-export-go'
        subprocess.run(['go', 'build', '-o', str(go_binary), str(go_source)], cwd=ROOT / 'packages/desktop-host-go', check=True)
        rust_binary = ROOT / 'target/debug/examples/diagnostic_offline_export'
        for name, binary in [('go', go_binary), ('rust', rust_binary)]:
            archive = out / ('offline-' + name + '.zip')
            subprocess.run([str(binary), str(logs), str(archive)], env=env, check=True)
            with zipfile.ZipFile(archive) as z:
                manifest = json.loads(z.read('otel/manifest.json'))
                assert manifest['state'] == 'unavailable'
                assert manifest['reason'] == 'daemon_not_running'
                assert manifest['files'] == [] and not manifest['liveFlush']
                assert not any(n.endswith('.enc') or n == 'otel/metrics.jsonl' for n in z.namelist())
            assert stale.read_bytes() == b'synthetic-retired-ciphertext'
            empty = out / ('only-stale-' + name)
            (empty / 'otel-history').mkdir(parents=True)
            (empty / 'otel-history/history.enc').write_bytes(stale.read_bytes())
            rejected = subprocess.run([str(binary), str(empty), str(out / ('rejected-' + name + '.zip'))], env=env, capture_output=True)
            assert rejected.returncode != 0, 'retired ciphertext counted as exportable logs'
            report['checks']['offline-' + name] = hashlib.sha256(archive.read_bytes()).hexdigest()
        report['passed'] = True
    finally:
        if running:
            try:
                stop()
            except Exception as error:
                report['cleanupFailures']['daemonStop'] = type(error).__name__
        if report['cleanupFailures']:
            report['passed'] = False
        report['binarySha256'] = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
                                 for name in ['target/debug/uniclipd', 'target/gui-go/uniclip']}
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        if not running:
            shutil.rmtree(home)
    assert report['passed'], report
