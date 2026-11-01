#!/usr/bin/env python3
"""Detailed capture and network-log acceptance through the native GUI (macOS).

Two isolated peers pair through the product flow: peer A owns the GUI profile, peer B is a CLI profile in
its own HOME. Three GUI launches run the in-WebView scenario (frontend/src/e2e/diagnostics-acceptance.ts):

  1. capture  - one Detailed switch (no separate network switch), linked business and network windows,
                a unique synthetic text synced from B, a GUI export during the capture, stop and restart,
                then the full 600 s automatic expiry. The GUI then exits and leaves the daemon running.
  2. reopen   - the GUI closes and reopens against the same daemon, then quits fully (graceful daemon stop).
  3. offline  - a new daemon run; export again (previous run must be in the package); then the task-owned
                daemon is stopped and the export runs through the offline path.

Every export is read back from disk: run.json source identity, per-file bytes and SHA-256, the accepted tail.
Window-only screenshots are saved; no clipboard content other than the synthetic text is ever used.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import cli  # noqa: E402
from peers import pair  # noqa: E402
from run import ROOT, isolated_env, pid_alive, read_steps, screenshot  # noqa: E402

SCREENSHOT_STEPS = ['diag-settings-open', 'diag-capture-started', 'diag-export-during-capture', 'diag-capture-stopped',
                    'diag-capture-restarted', 'diag-capture-expired', 'diag-reopen-state', 'diag-export-after-reopen',
                    'diag-export-offline']


def now():
    return datetime.now(timezone.utc).isoformat(timespec='milliseconds')


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def verify_export(path, pinned):
    """Read an export zip back: source identity, per-file bytes and SHA-256, record classes."""
    out = {'zip': Path(path).name, 'zipSha256': sha256(path)}
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        manifest = json.loads(z.read('manifest.json'))
        network = manifest['collection']['network']
        out['outcome'] = network['outcome']
        out['reasons'] = network['reasons']
        out['missingSources'] = manifest['collection'].get('missingSources')
        out['enginePreparationNetwork'] = (manifest.get('enginePreparation') or {}).get('network')
        out['runs'] = []
        for run in network['runs']:
            prefix = f"network/{run['role']}/{run['runId']}/"
            info = {'role': run['role'], 'runId': run['runId'], 'currentRun': run.get('currentRun'), 'files': [],
                    'unreadable': run.get('unreadableFiles')}
            total = classified = unreviewed = 0
            for file in run['files']:
                data = z.read(prefix + file['name'])
                ok = file['sha256'] == hashlib.sha256(data).hexdigest() and file['bytes'] == str(len(data))
                info['files'].append({'name': file['name'], 'bytes': file['bytes'], 'sha256': file['sha256'], 'matches': ok,
                                      'truncated': file['truncated'], 'matchesPrepared': file.get('matchesPrepared')})
                if file['name'].endswith('.jsonl'):
                    for line in data.decode().splitlines():
                        record = json.loads(line)
                        total += 1
                        if record.get('event') == 'unreviewed_callsite':
                            unreviewed += 1
                        else:
                            classified += 1
            run_json = json.loads(z.read(prefix + 'run.json'))
            info.update(records=total, classified=classified, unreviewed=unreviewed,
                        runJsonCommit=run_json.get('engine_source_commit'), runJsonState=run_json.get('engine_source_state'),
                        commitMatchesPin=run_json.get('engine_source_commit') == pinned)
            out['runs'].append(info)
        otel = json.loads(z.read('otel/manifest.json'))
        out['otel'] = otel
        for file in otel['files']:
            data = z.read(file['name'])
            assert len(data) == file['bytes']
            assert hashlib.sha256(data).hexdigest() == file['sha256']
        if manifest['mode'] == 'online':
            assert otel['history']['coverage'] == 'registered_processes'
            assert sum(host['role'] == 'gui' and host['state'] == 'active' for host in otel['history']['hostCoverage']) >= 2
            metrics = z.read('otel/metrics.jsonl').decode()
            assert 'process.memory.usage' in metrics
            assert 'uc.process.cpu.time' in metrics
            assert 'resourceMetrics' in metrics
            from diagnostics_correlation import compare
            out['correlation'] = compare(path)
            assert out['correlation']['passed'], out['correlation']
        else:
            assert otel['liveFlush'] is False
            assert otel['state'] in ('unavailable', 'encrypted_unavailable')
        assert 'import/import.py' in names
        out['entries'] = len(names)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--history-recovery-only', action='store_true', help='run isolated encrypted history recovery acceptance')
    parser.add_argument('--performance-only', action='store_true', help='run isolated CPU, memory, process exit and quota acceptance')
    parser.add_argument('--no-expiry-wait', action='store_true', help='offline re-run only; skips the 600 s wait')
    parser.add_argument('--skip-expiry', action='store_true', help='not used for acceptance: the 600 s expiry is the point')
    args = parser.parse_args()
    if args.history_recovery_only:
        from diagnostics_history_recovery import run
        return run(args.out)
    if args.performance_only:
        from diagnostics_performance import run
        return run(args.out)
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    pinned = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    engine = [l for l in (ROOT / 'Cargo.toml').read_text().splitlines() if 'uc-engine' in l][0].split('rev = "')[1].split('"')[0]
    home_a, home_b = tempfile.mkdtemp(prefix='uc-gui-go-'), tempfile.mkdtemp(prefix='uc-gui-go-')
    profile_a, profile_b = 'gui-go-' + os.path.basename(home_a), 'gui-go-' + os.path.basename(home_b)
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    env_a = isolated_env(home_a, profile_a, {'PATH': path})
    env_b = isolated_env(home_b, profile_b, {'PATH': path})
    binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
    marker = 'diag-accept-' + hashlib.sha256(os.urandom(16)).hexdigest()[:20]
    exports = Path(tempfile.mkdtemp(prefix='uc-gui-go-exports-'))
    results = {'passed': False, 'steps': {}, 'engine': engine, 'head': pinned, 'marker': marker,
               'binaries': {str(f.relative_to(ROOT)): sha256(f) for f in [binary, ROOT / 'target/gui-go/uniclip',
                            ROOT / 'target/debug/uniclipd'] if f.exists()}}
    proc = None
    daemon_pid = None

    def conn(home, profile):
        p = Path(home) / 'Library/Application Support' / ('app.uniclipboard.desktop-' + profile) / 'daemon.conn'
        return json.loads(p.read_text())

    def launch(name, phase, exit_mode, extra=None, on_step=None, timeout=1500):
        nonlocal proc
        evidence = out / f'{name}.jsonl'
        evidence.write_text('')
        env = isolated_env(home_a, profile_a, {'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1',
                                               'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': phase,
                                               'UC_GUI_GO_EXIT_MODE': exit_mode, **(extra or {})})
        results['steps'][name + ':launchedAt'] = now()
        proc = subprocess.Popen([str(binary)], env=env, stdout=(out / f'{name}-gui.log').open('w'), stderr=subprocess.STDOUT)
        seen, shot, cursor, deadline = {}, set(), 0, time.monotonic() + timeout
        window_pid = proc.pid
        while time.monotonic() < deadline and proc.poll() is None:
            rows = read_steps(evidence, cursor)
            cursor += len(rows)
            for row in rows:
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                row['seenAt'] = now()
                seen[row['step']] = row
                if row['step'] == 'ui-page-ready':
                    window_pid = int(row['detail']['pid'])
                if row['step'] in SCREENSHOT_STEPS and row['step'] not in shot:
                    shot.add(row['step'])
                    for _ in range(8):  # the parked WebView may not have painted yet
                        time.sleep(1)
                        target = out / f"{name}-{row['step']}.png"
                        if screenshot(window_pid, target) and target.stat().st_size > 150_000:
                            break
                if on_step:
                    on_step(row)
            time.sleep(.2)
        for row in read_steps(evidence, cursor):
            row['seenAt'] = now()
            seen[row['step']] = row
        code = proc.poll()
        if code is None:
            proc.terminate()
            raise RuntimeError(f'{name}: timeout')
        results['steps'][name] = {k: {'ok': v['ok'], 'seenAt': v['seenAt'], 'detail': v.get('detail')} for k, v in seen.items()}
        results['steps'][name + ':guiExit'] = code
        results['steps'][name + ':exitedAt'] = now()
        return seen

    try:
        pair(env_a, env_b, 'diag-a', 'diag-b')
        results['pairedAt'] = now()
        daemon_pid = conn(home_a, profile_a)['pid']
        results['daemonPidRun1'] = daemon_pid

        def send_marker(row):
            if row['step'] == 'diag-capture-started' and 'sentAt' not in results:
                results['sentAt'] = now()
                sent = cli(env_b, '--json', 'send', marker, check=False)
                results['sendExit'] = sent.returncode

        # 1. capture; an independent poller records the daemon's own timeline outside the WebView
        timeline, stop_poll = [], threading.Event()

        def poll():
            while not stop_poll.is_set():
                got = cli(env_a, '--json', 'debug', 'capture', 'status', check=False, timeout=30)
                timeline.append({'at': now(), 'exit': got.returncode, 'out': got.stdout.strip()[:300]})
                stop_poll.wait(5)

        threading.Thread(target=poll, daemon=True).start()
        seen = launch('capture', f'diag:capture:{marker}' + (':nowait' if args.no_expiry_wait else ''), 'keep', on_step=send_marker, timeout=2400)
        stop_poll.set()
        results['captureTimeline'] = timeline
        for step in ['diag-settings-open', 'diag-capture-started', 'diag-marker-received', 'diag-export-during-capture',
                     'diag-capture-stopped', 'diag-capture-restarted'] + ([] if args.no_expiry_wait else ['diag-capture-expired']):
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        assert seen['diag-settings-open']['detail']['networkSwitchCount'] == 0
        assert results['steps']['capture:guiExit'] == 0
        assert pid_alive(daemon_pid), 'closing the GUI stopped the daemon (keep mode must leave it running)'
        results['daemonAliveAfterGuiClose'] = True
        # Exact received text, read through peer A's own CLI.
        got = cli(env_a, 'get', check=False)
        results['receivedExact'] = got.stdout.strip() == marker
        results['getExit'] = got.returncode
        seen_run1 = seen['diag-capture-started']['detail']['snapshot']['network']['runId']
        exported = Path(seen['diag-export-during-capture']['detail']['path'] or '')
        results['export1Path'] = exported.name
        if exported.is_file():
            (exports / 'export1.zip').write_bytes(exported.read_bytes())
            results['export1'] = verify_export(exports / 'export1.zip', engine)

        # 2. reopen against the same daemon, open a fresh Detailed capture, send fresh traffic from peer B, then
        # quit fully. The network counters read right before the exit are compared with what the run holds on disk.
        def send_tail(row):
            if row['step'] == 'diag-tail-ready' and 'tailSentAt' not in results:
                results['tailSentAt'] = now()
                results['tailSendExits'] = [cli(env_b, '--json', 'send', f'{marker}-tail-{i}', check=False).returncode
                                            for i in range(1, 4)]

        seen = launch('reopen', f'diag:reopen:{marker}', 'full', on_step=send_tail)
        assert seen.get('diag-reopen-state', {}).get('ok')
        guiExitAt = time.monotonic()
        deadline = guiExitAt + 60
        while pid_alive(daemon_pid) and time.monotonic() < deadline:
            time.sleep(.2)
        results['daemonStoppedByFullQuit'] = not pid_alive(daemon_pid)
        results['daemonStopSecondsAfterGuiExit'] = round(time.monotonic() - guiExitAt, 1)
        results['daemonStopObservedAt'] = now()
        assert results['daemonStoppedByFullQuit']
        run1_dir = Path(home_a) / 'Library/Logs' / f'app.uniclipboard.desktop-{profile_a}/network/daemon' / seen_run1
        assert run1_dir.is_dir(), run1_dir
        results['run1Dir'] = run1_dir.name
        results['run1OnDiskRecords'] = sum(len(f.read_text().splitlines()) for f in run1_dir.glob('network.*.jsonl'))
        results['run1RunJson'] = json.loads((run1_dir / 'run.json').read_text())
        final = seen.get('diag-tail-final', {})
        counters = (final.get('detail') or {}).get('final', {}).get('network', {}).get('counters') or {}
        dropped = sum(int(counters.get(k, 0)) for k in ('queueDropped', 'closedDropped', 'writeFailed'))
        accepted = (final.get('detail') or {}).get('acceptedFinal')
        results['tail'] = {'freshRecordsObserved': bool(final.get('ok')), 'acceptedBefore': (final.get('detail') or {}).get('acceptedBefore'),
                           'acceptedFinal': accepted, 'dropped': dropped, 'onDisk': results['run1OnDiskRecords'],
                           'persisted': accepted is not None and results['run1OnDiskRecords'] + dropped >= accepted,
                           'counters': counters}
        assert seen.get('diag-tail-final', {}).get('ok'), 'no fresh accepted network records before the graceful exit'

        # 3. new daemon run: export again, then the offline path
        saved = exports / 'offline.zip'

        def stop_daemon(row):
            if row['step'] == 'diag-ready-for-offline' and 'stoppedAt' not in results:
                results['stoppedAt'] = now()
                pid_before = conn(home_a, profile_a)['pid']
                t0 = time.monotonic()
                stop = cli(env_a, '--json', 'stop', check=False, timeout=80)
                results['offlineStopExit'] = stop.returncode
                results['offlineStopOutput'] = (stop.stdout + stop.stderr)[:400]
                results['offlineStopCommandSeconds'] = round(time.monotonic() - t0, 1)
                while pid_alive(pid_before) and time.monotonic() - t0 < 120:
                    time.sleep(.2)
                results['offlineDaemonGoneAfterSeconds'] = round(time.monotonic() - t0, 1)
                results['offlineDaemonStillAlive'] = pid_alive(pid_before)

        seen = launch('offline', 'diag:offline', 'full', on_step=stop_daemon,
                      extra={'UC_GUI_GO_E2E_DIALOG_SAVE': str(saved)})
        for step in ['diag-export-after-reopen', 'diag-daemon-unreachable', 'diag-export-offline']:
            assert seen.get(step, {}).get('ok'), f'{step}: {seen.get(step)}'
        exported = Path(seen['diag-export-after-reopen']['detail']['path'] or '')
        if exported.is_file():
            (exports / 'export2.zip').write_bytes(exported.read_bytes())
            results['export2'] = verify_export(exports / 'export2.zip', engine)
        if saved.is_file():
            results['exportOffline'] = verify_export(saved, engine)
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
        for env in (env_a, env_b):
            cli(env, '--json', 'stop', check=False, timeout=80)
        for f in exports.glob('*.zip'):
            (out / f.name).write_bytes(f.read_bytes())
        logs = Path(home_a) / 'Library/Logs' / f'app.uniclipboard.desktop-{profile_a}'
        if logs.is_dir():  # kept for diagnosis; may hold peer addresses, so not for publishing
            shutil.copytree(logs, out / 'profile-a-logs', dirs_exist_ok=True)
        results['screenshots'] = [{'name': p.name, 'bytes': p.stat().st_size, 'sha256': sha256(p)} for p in out.glob('*.png')]
        (out / 'assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps({k: v for k, v in results.items() if k != 'steps'}, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
