"""Real daemon resource acceptance, called only through diagnostics_acceptance_run.py."""
import os
import sys
import tempfile
import subprocess
import time
import json
import zipfile
import hashlib
import urllib.request
import signal
import ctypes
from pathlib import Path
from run import ROOT, isolated_env
from file_preview_run import cli, PASSPHRASE
WORKER_CODE = 'import sys,time,gc\nmemory=None\nprint("ready",flush=True)\nfor command in sys.stdin:\n if command.strip()=="load":\n  memory=bytearray(128*1024*1024)\n  end=time.monotonic()+9\n  while time.monotonic()<end: pass\n  print("loaded",flush=True)\n elif command.strip()=="release":\n  memory=None;gc.collect();print("released",flush=True)\n elif command.strip()=="quit":break\n'

def run(out):
    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home = tempfile.mkdtemp(prefix='uc-gui-go-resource-load-')
    profile = 'gui-go-' + Path(home).name
    env = isolated_env(home, profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']})
    report = {'desktopHead': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'enginePin': next((line for line in (ROOT / 'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine ='))), 'passed': False, 'fixture': 'task-owned synthetic registered host; not a product GUI claim', 'phases': {}, 'samePidReuse': 'NOT_RUN', 'actualSleep': 'NOT_RUN'}
    worker = None
    helper = None
    helper_log = None
    fixtures = []
    stopped = False

    def spawn():
        p = subprocess.Popen([sys.executable, '-u', '-c', WORKER_CODE], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        assert p.stdout.readline().strip() == 'ready'
        return p

    def register(pid):
        body = json.dumps({'pid': pid, 'clientType': 'gui'}).encode()
        req = urllib.request.Request(base + '/auth/connect', data=body, headers={'Authorization': 'Bearer ' + conn['token'], 'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=3) as response:
            assert response.status == 200

    def export(name):
        result = json.loads(cli(env, '--json', 'debug', 'export-logs').stdout)
        dest = out / (name + '.zip')
        dest.write_bytes(Path(result['path']).read_bytes())
        return dest

    def records(path):
        with zipfile.ZipFile(path) as z:
            return ([json.loads(l) for l in z.read('otel/metrics.jsonl').splitlines()], json.loads(z.read('otel/manifest.json')))

    def for_pid(rows, pid):
        found = []
        for r in rows:
            for rm in r['resourceMetrics']:
                attrs = {a['key']: next(iter(a['value'].values())) for a in rm['resource']['attributes']}
                if int(attrs['process.pid']) == pid:
                    for sm in rm['scopeMetrics']:
                        for metric in sm['metrics']:
                            for kind in ['gauge', 'sum']:
                                for point in metric.get(kind, {}).get('dataPoints', []):
                                    found.append({'name': metric['name'], 'at': int(point['timeUnixNano']), 'value': point.get('asDouble', point.get('asInt')), 'state': next((a['value']['stringValue'] for a in point['attributes'] if a['key'] == 'uc.sample.state')), 'resourceRunId': attrs['service.instance.id']})
        return found
    try:
        cli(env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'synthetic-resource-fixture')
        cli(env, 'start')
        conn = json.loads((Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn').read_text())
        base = 'http://' + conn['host'] + ':' + str(conn['port'])
        cli(env, 'debug', 'capture', 'start', '--minutes', '2')
        worker = spawn()
        pid = worker.pid
        register(pid)
        report['phases']['baselineAtNs'] = time.time_ns()
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            rows, _ = records(export('baseline'))
            if any(point['name'] == 'process.memory.usage' for point in for_pid(rows, pid)):
                break
            time.sleep(0.2)
        else:
            raise TimeoutError('registered host baseline was not sampled')
        report['phases']['loadAtNs'] = time.time_ns()
        worker.stdin.write('load\n')
        worker.stdin.flush()
        assert worker.stdout.readline().strip() == 'loaded'
        report['phases']['releaseAtNs'] = time.time_ns()
        worker.stdin.write('release\n')
        worker.stdin.flush()
        assert worker.stdout.readline().strip() == 'released'
        time.sleep(3)
        path = export('load')
        rows, meta = records(path)
        points = for_pid(rows, pid)
        cpu = [float(p['value']) for p in points if p['name'] == 'uc.process.cpu.utilization']
        mem = [float(p['value']) for p in points if p['name'] == 'process.memory.usage']
        assert max(cpu) > 0.5, (cpu, meta)
        assert max(mem) - min(mem) > 64 * 1024 * 1024, mem
        first = min((p['at'] for p in points))
        assert all((p['name'] != 'uc.process.cpu.utilization' for p in points if p['at'] == first))
        report.update(peakSingleCoreFraction=max(cpu), residentIncreaseBytes=max(mem) - min(mem), hostFirstCpuOmitted=True)
        worker.stdin.write('quit\n')
        worker.stdin.flush()
        worker.wait(timeout=5)
        report['phases']['exitedAtNs'] = time.time_ns()
        time.sleep(2)
        rows, meta = records(export('exited'))
        assert any((h['pid'] == pid and h['state'] == 'exited' for h in meta['history']['hostCoverage']))
        report['exitedHostMarked'] = True
        worker = spawn()
        newpid = worker.pid
        register(newpid)
        time.sleep(2)
        rows, meta = records(export('restarted'))
        newpoints = for_pid(rows, newpid)
        first = min((p['at'] for p in newpoints))
        assert all((p['name'] != 'uc.process.cpu.utilization' for p in newpoints if p['at'] == first))
        assert newpoints[0]['resourceRunId'] != points[0]['resourceRunId']
        report['newProcessBaselineReset'] = True
        lib = ctypes.CDLL('/usr/lib/libproc.dylib')
        lib.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
        expected = (ROOT / 'target/debug/uniclipd').resolve()

        def verified_signal(sig):
            buf = ctypes.create_string_buffer(4096)
            assert lib.proc_pidpath(conn['pid'], buf, len(buf)) > 0
            assert Path(os.fsdecode(buf.value)).resolve() == expected
            os.kill(conn['pid'], sig)
        verified_signal(signal.SIGSTOP)
        stopped = True
        time.sleep(17)
        verified_signal(signal.SIGCONT)
        stopped = False
        time.sleep(2)
        rows, meta = records(export('gap'))
        daemonpoints = for_pid(rows, conn['pid'])
        gap = [p for p in daemonpoints if p['state'] == 'gap']
        assert gap and all((p['name'] != 'uc.process.cpu.utilization' for p in gap))
        report['schedulerGapCpuOmitted'] = True
        helper_log = (out / 'quick-panel.log').open('wb')
        helper = subprocess.Popen([str(ROOT / 'target/debug/uniclip-quick-panel'), '--exit-when-stdin-closes'], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=helper_log)
        helper.stdin.write(b'{"command":"reload_settings"}\n')
        helper.stdin.flush()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if helper.poll() is not None:
                raise RuntimeError('product Quick Panel exited before registration')
            rows, meta = records(export('quick-panel'))
            if any((h['pid'] == helper.pid and h['role'] == 'quick-panel' and (h['state'] == 'active') for h in meta['history']['hostCoverage'])):
                break
            time.sleep(0.5)
        else:
            raise TimeoutError('product Quick Panel did not register resource coverage')
        report['productQuickPanelRegistered'] = True
        helper.stdin.close()
        helper.wait(timeout=10)
        helper_log.close()
        assert helper.returncode == 0
        time.sleep(2)
        rows, meta = records(export('quick-panel-exited'))
        assert any((h['pid'] == helper.pid and h['role'] == 'quick-panel' and (h['state'] == 'exited') for h in meta['history']['hostCoverage']))
        report['productQuickPanelExited'] = True
        for i in range(33):
            p = subprocess.Popen(['/bin/sleep', '60'])
            fixtures.append(p)
            register(p.pid)
        rows, meta = records(export('quota'))
        assert len(meta['history']['hostCoverage']) <= 32 and meta['history']['hostRegistrationFailures'] > 0
        report['registrationQuotaBounded'] = True
        report['hostRegistrationFailures'] = meta['history']['hostRegistrationFailures']
        report['passed'] = True
        (out / 'points.json').write_text(json.dumps(points, indent=2) + '\n')
    finally:
        body_failed = sys.exc_info()[0] is not None
        report['stepFailures'] = {}

        def attempt(name, action):
            try:
                return action()
            except Exception as error:
                report['stepFailures'][name] = type(error).__name__
                report['passed'] = False
                return None

        if stopped:
            attempt('resumeDaemon', lambda: verified_signal(signal.SIGCONT))
        if helper is not None and helper.poll() is None:
            attempt('helperClose', helper.stdin.close)
            attempt('helperWait', lambda: helper.wait(timeout=10))
        if helper_log is not None:
            attempt('helperLogClose', helper_log.close)
        if worker is not None and worker.poll() is None:
            attempt('workerQuit', lambda: worker.stdin.write('quit\n'))
            attempt('workerFlush', worker.stdin.flush)
            attempt('workerWait', lambda: worker.wait(timeout=5))
        for index, fixture in enumerate(fixtures):
            if fixture.poll() is None:
                attempt(f'fixtureTerminate:{index}', fixture.terminate)
                attempt(f'fixtureWait:{index}', lambda: fixture.wait(timeout=5))
        stop = attempt('daemonStop', lambda: cli(env, 'stop', check=False))
        report['daemonStopExit'] = stop.returncode if stop is not None else None
        if stop is not None and stop.returncode != 0:
            report['stepFailures']['daemonStop'] = 'NonzeroExit'
            report['passed'] = False
        report['fixtureCleanup'] = 'failed' if report['stepFailures'] else 'all exited and reaped'
        report['binarySha256'] = {}
        for name in ['target/debug/uniclipd', 'target/debug/uniclip-quick-panel', 'target/gui-go/uniclip']:
            report['binarySha256'][name] = attempt('hash:' + name, lambda: hashlib.sha256((ROOT / name).read_bytes()).hexdigest())
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))
        if report['stepFailures'] and not body_failed:
            raise RuntimeError('resource acceptance cleanup or binary hashing failed; inspect result.json')
