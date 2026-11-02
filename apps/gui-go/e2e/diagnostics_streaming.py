"""Real daemon file-stream acceptance through diagnostics_acceptance_run.py.

Failures: no growth before export; lost offline metrics; changed run identities;
whole-file export allocation; omitted integrity; retired ciphertext access; leaked processes.
"""
import ctypes
import fcntl
import http.server
import threading
import sys
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import zipfile
import urllib.request
import urllib.error
from run import ROOT, isolated_env, pid_alive
from file_preview_run import cli, PASSPHRASE

GO_EXPORT = '''package main
import (
 "os"
 "github.com/UniClipboard/UniClipboard/packages/desktop-host-go/diaglogs"
)
func main() { if _,err:=diaglogs.ExportStartup(os.Args[1],os.Args[2],nil);err!=nil { panic(err) } }
'''


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as source:
        for chunk in iter(lambda: source.read(65536), b''):
            h.update(chunk)
    return h.hexdigest()


def verify(path):
    with zipfile.ZipFile(path) as archive:
        manifest = json.loads(archive.read('otel/manifest.json'))
        assert not any(name.endswith('.enc') for name in archive.namelist())
        counts = {}
        metric_min = metric_max = None
        metric_names = set()
        for name in archive.namelist():
            with archive.open(name) as source:
                carry = b''
                for chunk in iter(lambda: source.read(65536), b''):
                    data = carry + chunk
                    assert b'synthetic-sensitive-streaming' not in data
                    carry = data[-64:]

        for item in manifest['files']:
            h = hashlib.sha256()
            size = lines = 0
            with archive.open(item['name']) as source:
                while True:
                    line = source.readline(65537)
                    if not line:
                        break
                    assert len(line) <= 65536 and line.endswith(b'\n')
                    record = json.loads(line)
                    if item['name'] == 'otel/metrics.jsonl':
                        for resource in record.get('resourceMetrics', []):
                            for scope in resource.get('scopeMetrics', []):
                                for metric in scope.get('metrics', []):
                                    metric_names.add(metric.get('name'))
                                    for kind in ('gauge', 'sum'):
                                        for point in metric.get(kind, {}).get('dataPoints', []):
                                            at = int(point.get('timeUnixNano', 0))
                                            metric_min = at if metric_min is None else min(metric_min, at)
                                            metric_max = at if metric_max is None else max(metric_max, at)
                    assert b'synthetic-sensitive-streaming' not in line
                    size += len(line)
                    lines += 1
                    h.update(line)
            assert size == item['bytes'] and lines == item['requests'] and h.hexdigest() == item['sha256']
            counts[item['name']] = lines
        assert counts['otel/metrics.jsonl'] > 0
        return {'sha256': digest(path), 'counts': counts, 'manifest': manifest, 'metricNames': sorted(metric_names), 'metricSpanSeconds': (metric_max - metric_min) / 1e9 if metric_min is not None else None}


def run(out, duration=15, load_bytes=0, engine_probe=None, engine_root=None, collector=None):
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home = Path(tempfile.mkdtemp(prefix='uc-gui-go-streaming-'))
    profile = 'gui-go-' + home.name
    env = isolated_env(str(home), profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    logs = home / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile)
    connection = home / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
    report = {'passed': False, 'command': sys.argv, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True)),
              'enginePin': next(line for line in (ROOT / 'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine =')),
              'host': subprocess.check_output(['uname', '-a'], text=True).strip(),
              'checks': {}, 'timeline': [], 'cleanupFailures': {},
              'notRun': ['allocator heap high-water profiling: instrumentation is not configured', 'physical full-volume exhaustion and power loss', 'physical slow-storage hardware', 'deterministic concurrent truncation/removal during ZIP reads', 'native GUI matrix: macOS/Windows/Linux']}
    if duration < 1860:
        report['notRun'].append('31-minute daemon soak: duration below 1860 seconds')
    if load_bytes <= 8 * 1024 * 1024:
        report['notRun'].append('real-daemon payload beyond 8 MiB: load target below threshold')
    if not engine_probe:
        report['notRun'].append('Engine storage fault probe: no binary supplied')
    if not collector:
        report['notRun'].append('fixed Collector import: no binary supplied')
    if engine_root:
        report['engineHead'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=engine_root, text=True).strip()
        report['engineDirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=engine_root, text=True))
    running = False
    daemon_pid = None

    def stop():
        nonlocal running
        pid = json.loads(connection.read_text())['pid'] if connection.exists() else daemon_pid
        result = cli(env, 'stop', check=False)
        deadline = time.monotonic() + 15
        while pid and pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(.1)
        assert not pid or not pid_alive(pid), 'owned daemon still running'
        report.setdefault('stopExits', []).append(result.returncode)
        running = False

    def live(name):
        value = json.loads(cli(env, '--json', 'debug', 'export-logs').stdout)
        archive = out / (name + '.zip')
        shutil.copyfile(value['path'], archive)
        report['checks'][name] = verify(archive)
        with zipfile.ZipFile(archive) as source:
            host = json.loads(source.read('manifest.json'))['host']
            assert host['buildCommit'] == report['head'], 'daemon binary does not match the Desktop source head'
        expected = report['enginePin'].split('rev = "')[1].split('"')[0]
        run_id = report['checks'][name]['manifest']['recording']['runId']
        metadata = next(item for item in report['checks'][name]['manifest']['runs'] if item['runId'] == run_id)
        assert metadata['engineSourceCommit'] == expected and metadata['engineSourceState'] == 'clean', 'daemon Engine source provenance mismatch'
        return report['checks'][name]['manifest']['recording']['runId']

    def offline(name, root):
        for kind, binary in [('go', out / 'offline-export-go'), ('rust', ROOT / 'target/debug/examples/diagnostic_offline_export')]:
            target = out / (name + '-' + kind + '.zip')
            subprocess.run([str(binary), str(root), str(target)], env=env, check=True, timeout=60)
            result = verify(target)
            assert result['manifest']['liveFlush'] is False
            report['checks'][name + '-' + kind] = result

    try:
        running = True
        cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'synthetic-sensitive-streaming')
        cli(env, 'start')
        daemon_pid = json.loads(connection.read_text())['pid']
        conn = json.loads(connection.read_text())
        base = 'http://' + conn['host'] + ':' + str(conn['port'])
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path, body, authorization):
            req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={'Content-Type': 'application/json', 'Authorization': authorization})
            with opener.open(req, timeout=10) as response:
                return json.load(response)['data']
        session, refresh_at = None, 0
        def refresh_session():
            nonlocal session, refresh_at
            response = request('/auth/connect', {'pid': os.getpid(), 'clientType': 'cli'}, 'Bearer ' + conn['token'])
            assert 0 < response['refreshAtSecs'] < response['expiresInSecs']
            session = response['sessionToken']
            refresh_at = time.monotonic() + response['refreshAtSecs']
            report['authRefreshes'] = report.get('authRefreshes', 0) + 1
        def session_request(path, body):
            if time.monotonic() >= refresh_at:
                refresh_session()
            try:
                return request(path, body, 'Session ' + session)
            except urllib.error.HTTPError as error:
                if error.code != 401:
                    raise
                # Authentication rejects before the operation. Match the native client's refresh behavior.
                refresh_session()
                return request(path, body, 'Session ' + session)
        refresh_session()
        current_run = None
        def snapshot():
            paths = list(logs.glob('diagnostics/*/*/metrics.*.jsonl'))
            statuses = [logs / 'diagnostics/engine' / current_run / 'status.json'] if current_run else list(logs.glob('diagnostics/*/*/status.json'))
            status = json.loads(statuses[-1].read_text()) if statuses else {}
            return {'elapsed': time.monotonic() - start, 'metricBytes': sum(p.stat().st_size for p in paths),
                    'files': len(paths), 'writtenBytes': status.get('writtenBytes', 0),
                    'rssKiB': int(subprocess.check_output(['ps', '-o', 'rss=', '-p', str(daemon_pid)], text=True).strip())}
        baseline = None
        start = time.monotonic()
        while time.monotonic() - start < 12:
            row = snapshot()
            report['timeline'].append(row)
            if baseline is None and row['metricBytes']:
                baseline = row['metricBytes']
            time.sleep(1)
        total = row['metricBytes']
        if load_bytes:
            current_run = live('load-baseline')
            load_start = time.monotonic()
            cycles = 0
            while snapshot()['writtenBytes'] < load_bytes:
                active = session_request('/diagnostics/capture/start', {'durationSeconds': 2})
                session_request('/diagnostics/capture/stop', {'captureId': active['capture']['captureId']})
                cycles += 1
                time.sleep(.05)
                if cycles % 100 == 0:
                    report['timeline'].append(snapshot())
                if time.monotonic() - load_start > 600:
                    raise TimeoutError('real capture load did not reach the required written payload')
            report['checks']['load'] = {'cycles': cycles, 'writtenBytes': snapshot()['writtenBytes'], 'seconds': time.monotonic() - load_start}
        while time.monotonic() - start < max(duration, 15):
            report['timeline'].append(snapshot())
            time.sleep(1)
        assert baseline and total > baseline, 'metrics did not grow without export'
        report['checks']['growthBeforeExport'] = True
        first = live('online')
        from diagnostics_correlation import compare
        report['checks']['correlation'] = compare(out / 'online.zip')
        assert report['checks']['correlation']['passed']
        if duration >= 1860:
            assert report['checks']['online']['metricSpanSeconds'] >= 1800, 'the first retained metric did not survive the old retention window'
        stop()
        source = out / 'offline_export.go'
        source.write_text(GO_EXPORT)
        subprocess.run(['go', 'build', '-o', str(out / 'offline-export-go'), str(source)], cwd=ROOT / 'packages/desktop-host-go', check=True)
        offline('stopped', logs)
        # Export only metrics: no ordinary logs or network files exist in this fixture.
        only = out / 'metrics-only'
        for path in logs.glob('diagnostics/*/*/metrics.*.jsonl'):
            target = only / path.relative_to(logs)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
        stale = only / 'otel-history/history.enc'
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b'synthetic-retired-ciphertext')
        offline('metrics-only', only)
        assert stale.read_bytes() == b'synthetic-retired-ciphertext'
        damaged = out / 'damaged'
        shutil.copytree(only, damaged)
        damaged_file = next(damaged.glob('diagnostics/*/*/metrics.*.jsonl'))
        with damaged_file.open('ab') as stream:
            stream.write(b'not-json\n' + b'x' * 70000 + b'\n{"tail":')
        offline('damaged', damaged)
        for kind in ('go', 'rust'):
            sources = report['checks']['damaged-' + kind]['manifest']['sources']
            assert sum(item.get('invalidLines', 0) for item in sources) >= 2
            assert sum(item.get('tailBytes', 0) for item in sources) > 0
        denied = out / 'denied'
        shutil.copytree(only, denied)
        denied_runs = list(denied.glob('diagnostics/*/*'))
        for denied_run in denied_runs:
            denied_run.chmod(0)
        try:
            for kind, binary in [('go', out / 'offline-export-go'), ('rust', ROOT / 'target/debug/examples/diagnostic_offline_export')]:
                target = out / ('denied-' + kind + '.zip')
                subprocess.run([str(binary), str(denied), str(target)], env=env, check=True, timeout=60)
                with zipfile.ZipFile(target) as archive:
                    manifest = json.loads(archive.read('otel/manifest.json'))
                    assert manifest['state'] == 'unavailable'
                    assert any(item['state'] == 'unreadable' for item in manifest['sources'])
                report['checks']['denied-' + kind] = {'sha256': digest(target), 'unreadableReported': True}
        finally:
            for denied_run in denied_runs:
                denied_run.chmod(0o700)
        blocked = out / 'blocked-destination'
        blocked.mkdir()
        target = blocked / 'existing.zip'
        target.write_bytes(b'synthetic-existing-target')
        blocked.chmod(0o500)
        try:
            for kind, binary in [('go', out / 'offline-export-go'), ('rust', ROOT / 'target/debug/examples/diagnostic_offline_export')]:
                result = subprocess.run([str(binary), str(only), str(target)], env=env, capture_output=True, timeout=60)
                assert result.returncode != 0 and target.read_bytes() == b'synthetic-existing-target'
                report['checks']['write-failure-' + kind] = {'existingTargetPreserved': True, 'exit': result.returncode}
        finally:
            blocked.chmod(0o700)

        cli(env, 'start')
        running = True
        daemon_pid = json.loads(connection.read_text())['pid']
        second = live('restarted')
        assert first != second
        assert (logs / 'diagnostics/engine' / first).is_dir()
        # Verify the owned executable immediately before the sudden-exit signal.
        lib = ctypes.CDLL('/usr/lib/libproc.dylib')
        lib.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
        buf = ctypes.create_string_buffer(4096)
        assert lib.proc_pidpath(daemon_pid, buf, len(buf)) > 0
        assert Path(os.fsdecode(buf.value)).resolve() == (ROOT / 'target/debug/uniclipd').resolve()
        os.kill(daemon_pid, signal.SIGKILL)
        deadline = time.monotonic() + 15
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.1)
        assert not pid_alive(daemon_pid)
        running = False
        offline('killed', logs)
        # Two real daemons share only the task-owned log root. Profile data remains isolated.
        other_home = Path(tempfile.mkdtemp(prefix='uc-gui-go-streaming-peer-'))
        other_profile = 'gui-go-' + other_home.name
        other_env = isolated_env(str(other_home), other_profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
        other_logs = other_home / 'Library/Logs' / ('app.uniclipboard.desktop-' + other_profile)
        other_logs.parent.mkdir(parents=True, exist_ok=True)
        other_logs.symlink_to(logs, target_is_directory=True)
        other_connection = other_home / 'Library/Application Support' / ('app.uniclipboard.desktop-' + other_profile) / 'daemon.conn'
        other_running = paused = False
        def signal_owned(pid, sig):
            identity = ctypes.create_string_buffer(4096)
            assert lib.proc_pidpath(pid, identity, len(identity)) > 0
            assert Path(os.fsdecode(identity.value)).resolve() == (ROOT / 'target/debug/uniclipd').resolve()
            os.kill(pid, sig)
        def lease_held(run_id):
            with (logs / 'diagnostics/engine' / run_id / 'lease.lock').open('r+b') as lease:
                try:
                    fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    return True
                fcntl.flock(lease, fcntl.LOCK_UN)
                return False
        try:
            running = True
            cli(env, 'start')
            daemon_pid = json.loads(connection.read_text())['pid']
            active_a = live('multiprocess-a')
            signal_owned(daemon_pid, signal.SIGSTOP)
            paused = True
            active_directory = logs / 'diagnostics/engine' / active_a
            old = time.time() - 8 * 86400
            for item in active_directory.iterdir():
                if item.is_file():
                    os.utime(item, (old, old))
            other_running = True
            cli(other_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'synthetic-sensitive-streaming')
            cli(other_env, 'start')
            value = json.loads(cli(other_env, '--json', 'debug', 'export-logs').stdout)
            other_zip = out / 'multiprocess-b.zip'
            shutil.copyfile(value['path'], other_zip)
            other_result = verify(other_zip)
            active_b = other_result['manifest']['recording']['runId']
            assert active_a != active_b and active_directory.is_dir()
            assert lease_held(active_a) and lease_held(active_b)
            report['checks']['multiprocess'] = {'runA': active_a, 'runB': active_b, 'expiredActiveRunRetained': True, 'bothLeasesHeld': True, 'peerExport': other_result}
        finally:
            if other_running:
                peer_pid = json.loads(other_connection.read_text())['pid'] if other_connection.exists() else None
                stopped = cli(other_env, 'stop', check=False)
                deadline = time.monotonic() + 15
                while peer_pid and pid_alive(peer_pid) and time.monotonic() < deadline:
                    time.sleep(.1)
                if peer_pid and pid_alive(peer_pid):
                    report['cleanupFailures']['sharedRootPeer'] = 'owned_daemon_still_running'
                else:
                    other_running = False
            if paused:
                signal_owned(daemon_pid, signal.SIGCONT)
                paused = False
            if not other_running:
                try:
                    shutil.rmtree(other_home)
                except OSError:
                    report['cleanupFailures']['peerSandboxRemoval'] = 'io_error'
        if 'multiprocess' in report['checks']:
            assert not lease_held(report['checks']['multiprocess']['runB'])
            report['checks']['multiprocess']['peerLeaseReleased'] = True
        if collector:
            deliveries = []
            class Sink(http.server.BaseHTTPRequestHandler):
                def do_POST(self):
                    remaining = int(self.headers.get('Content-Length', 0))
                    h = hashlib.sha256()
                    while remaining:
                        chunk = self.rfile.read(min(65536, remaining))
                        if not chunk:
                            break
                        h.update(chunk)
                        remaining -= len(chunk)
                    deliveries.append({'path': self.path, 'sha256': h.hexdigest(), 'completeBody': remaining == 0})
                    self.send_response(200)
                    self.send_header('Content-Type', 'application/x-protobuf')
                    self.send_header('Content-Length', '0')
                    self.end_headers()
                def log_message(self, *args):
                    pass
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Sink)
            thread = threading.Thread(target=server.serve_forever)
            thread.start()
            try:
                import_env = dict(os.environ, OTEL_IMPORT_ENDPOINT='http://127.0.0.1:' + str(server.server_port))
                import_env.pop('OTEL_IMPORT_AUTHORIZATION', None)
                subprocess.run([sys.executable, str(ROOT / 'packages/desktop-host-go/diaglogs/import/import.py'), str(out / 'online.zip'), '--collector', str(collector.resolve()), '--receipt', str(out / 'collector-import.json')], env=import_env, check=True, timeout=90)
                receipt = json.loads((out / 'collector-import.json').read_text())
                assert receipt['status'] == 'delivered'
                assert all(receipt['expected'][signal] > 0 for signal in ('metrics', 'logs', 'traces'))
                assert {item['path'] for item in deliveries} == {'/v1/metrics', '/v1/logs', '/v1/traces'}
                report['checks']['collector'] = {'receipt': receipt, 'deliveries': deliveries, 'binarySha256': digest(collector), 'scope': 'fixed Collector decode and isolated HTTP sink; backend query NOT_RUN'}
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=5)
                assert not thread.is_alive()
        if engine_probe:
            assert engine_root, 'the Engine probe requires source identity'
            probe_out = out / 'engine-storage'
            probe_env = dict(os.environ, UC_TEST_ARTIFACTS_DIR=str(probe_out))
            with (out / 'engine-storage.log').open('wb') as log:
                probe = subprocess.Popen([str(engine_probe.resolve()), '--diagnostic-streaming'], env=probe_env, stdout=log, stderr=log)
                rss = []
                probe_deadline = time.monotonic() + 120
                while probe.poll() is None:
                    if time.monotonic() > probe_deadline:
                        probe.terminate()
                        try:
                            probe.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            probe.kill()
                            probe.wait(timeout=5)
                        report["cleanupFailures"]["probeTimeout"] = "forced_owned_child_stop"
                        raise TimeoutError("Engine storage probe exceeded its deadline")
                    value = subprocess.run(['ps', '-o', 'rss=', '-p', str(probe.pid)], capture_output=True, text=True)
                    if value.returncode == 0 and value.stdout.strip():
                        rss.append({'at': time.monotonic(), 'rssKiB': int(value.stdout.strip())})
                    time.sleep(.1)
                assert probe.returncode == 0
            probe_result = json.loads((probe_out / 'diagnostic-streaming.json').read_text())
            assert probe_result['passed']
            probe_result['rss'] = rss
            probe_result['binarySha256'] = digest(engine_probe)
            probe_result['engineHead'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=engine_root, text=True).strip()
            probe_result['engineDirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=engine_root, text=True))
            report['checks']['engineStorage'] = probe_result
            offline('engine-storage', probe_out / 'logs')
        report['passed'] = True
    finally:
        if sys.exc_info()[1] is not None:
            report["failureType"] = type(sys.exc_info()[1]).__name__
        if running:
            try:
                stop()
            except Exception as error:
                report['cleanupFailures']['daemonStop'] = type(error).__name__
        if report['cleanupFailures']:
            report['passed'] = False
        report['binarySha256'] = {name: digest(ROOT / name) for name in ['target/debug/uniclipd', 'target/gui-go/uniclip']}
        report['binarySha256']['rust-exporter'] = digest(ROOT / 'target/debug/examples/diagnostic_offline_export')
        if (out / 'offline-export-go').exists():
            report['binarySha256']['go-exporter'] = digest(out / 'offline-export-go')
        if not running:
            try:
                shutil.rmtree(home)
                report['sandboxRemoved'] = True
            except OSError:
                report['cleanupFailures']['sandboxRemoval'] = 'io_error'
                report['passed'] = False
        report['cleanupComplete'] = not running and not report['cleanupFailures']
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    assert report['passed'], report
