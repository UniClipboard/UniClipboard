#!/usr/bin/env python3
"""Own a real macOS product, its isolated backend, and its normal shutdown."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import sys
import time

UNKNOWN_IDENTITY = object()
ROOT = Path(__file__).resolve().parents[4]
spec = importlib.util.spec_from_file_location('isolated_backend', ROOT / 'apps/gui-go/frontend/e2e/tester-army/backend.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def identity(pid):
    result = subprocess.run(['ps', '-p', str(pid), '-o', 'lstart=,comm='], capture_output=True, text=True, timeout=5)
    if result.returncode == 0 and result.stdout.strip():
        return result.stdout.strip()
    if result.returncode == 1 and not result.stdout.strip() and not result.stderr.strip():
        return None
    raise OSError('native process identity probe failed')


def cleanup_identity(pid):
    try:
        return identity(pid)
    except (subprocess.TimeoutExpired, OSError):
        # Unknown ownership must not authorize signaling or private-home removal.
        return UNKNOWN_IDENTITY


def emit(value):
    print(json.dumps(value), flush=True)


def evidence_records(path):
    try:
        lines = path.read_bytes().splitlines()
    except OSError:
        return []
    records = []
    for line in lines:
        try:
            record = json.loads(line.decode('utf-8'))
        except (UnicodeDecodeError, json.JSONDecodeError):
            # A partial writer record must not prevent owned-process cleanup.
            continue
        if isinstance(record, dict):
            records.append(record)
    return records


def run():
    if not __debug__:
        raise RuntimeError('native ownership checks require Python assertions enabled')
    backend = host = None
    ui_pid = ui_identity = None
    control = evidence = None
    failed = forced = False
    receipt = {}
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        request = json.loads(sys.stdin.readline())
        output = Path(request['artifactsDir']).resolve()
        output.mkdir(parents=True, exist_ok=True)
        texts = request['texts']
        assert len(texts) == 2 and len(set(texts)) == 2
        assert all(isinstance(text, str) and text.startswith('Native history adapter ') for text in texts)
        app = ROOT / 'target/gui-go/UniClipboardGoE2E.app'
        assert Path(request['appPath']).resolve() == app
        with (app / 'Contents/Info.plist').open('rb') as file:
            plist = plistlib.load(file)
        assert plist['CFBundleIdentifier'].endswith('.e2e')
        binary = app / 'Contents/MacOS/gui-go'
        ui_binary = app / 'Contents/Helpers/UniClipboardUI.app/Contents/MacOS/gui-go'
        build = json.loads((ROOT / 'target/gui-go/macos-native-build.json').read_text())
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, timeout=10).strip()
        assert build['head'] == head
        binary_hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in
                         [('host', binary), ('ui', ui_binary), ('daemon', ROOT / 'target/debug/uniclipd'),
                          ('cli', ROOT / 'target/gui-go/uniclip')]}
        assert binary_hashes == build['binariesSha256'], 'native binaries differ from the recorded build'
        backend = module.Backend('empty')
        backend.start()
        for text in texts:
            backend.send(text)
        entries = backend.request('/clipboard/entries?limit=10')
        assert len(entries) == 2
        control = Path(backend.home) / 'native-control'
        evidence = Path(backend.home) / 'native-evidence.jsonl'
        control.write_text('')
        evidence.write_text('')
        inspector = Path(backend.home) / 'window-id'
        subprocess.run(['swiftc', str(ROOT / 'apps/gui-go/e2e/window_id.swift'), '-o', str(inspector)],
                       check=True, capture_output=True, timeout=120)
        env = dict(backend.env, UC_GUI_GO_E2E_PHASE='external', UC_GUI_GO_E2E_VISIBLE='1',
                   UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control),
                   UC_GUI_GO_EXIT_MODE='full', UC_GPUI_QUICK_PANEL='0')
        host = subprocess.Popen([str(binary)], env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            assert host.poll() is None, 'owned host exited before readiness'
            rows = evidence_records(evidence)
            ready = next((row for row in rows if row.get('step') == 'ui-page-ready'), None)
            if ready:
                assert ready['detail']['daemonConnected']
                ui_pid = ready['detail']['pid']
                parent_pid = int(subprocess.check_output(['ps', '-p', str(ui_pid), '-o', 'ppid='], text=True, timeout=10))
                assert parent_pid == host.pid
                ui_identity = identity(ui_pid)
                assert ui_identity and ui_identity.endswith(str(ui_binary))
                window = subprocess.run([str(inspector), str(ui_pid)], capture_output=True, text=True, timeout=5)
                if window.returncode == 0:
                    break
            time.sleep(.1)
        else:
            raise RuntimeError('owned native window readiness timed out')
        connections = list(Path(backend.home).rglob('daemon.conn'))
        assert len(connections) == 1
        assert json.loads(connections[0].read_text())['pid'] == backend.process.pid, 'product must reuse the owned daemon'
        health = backend.request('/health')
        assert health['residency'] == 'standalone'
        receipt = {
            'runId': request['runId'], 'attemptId': request['attemptId'],
            'profile': backend.profile, 'hostPid': host.pid, 'uiPid': ui_pid,
            'daemonPid': backend.process.pid, 'hostIdentity': identity(host.pid), 'uiIdentity': ui_identity,
            'windowId': 'w-' + window.stdout.strip(), 'windowInspector': str(inspector),
            'agentHome': str(Path(backend.home) / 'agent-desktop'), 'appName': plist['CFBundleName'],
            'systemClipboardDisabled': env['UC_DISABLE_SYSTEM_CLIPBOARD'] == '1',
            'guiIsolated': env['UC_GUI_GO_ISOLATED'] == '1', 'seededEntries': 2,
            'buildHead': build['head'], 'buildDirty': build['dirty'], 'binariesSha256': binary_hashes,
        }
        (output / 'native-launch.json').write_text(json.dumps(receipt, indent=2) + '\n')
        emit({'phase': 'ready', **receipt})
        sys.stdin.buffer.read()
    except (Exception, KeyboardInterrupt) as error:
        failed = True
        emit({'phase': 'failure', 'errorType': type(error).__name__})
    finally:
        # A second cancellation must not interrupt the owned cleanup already in progress.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        if host is not None and host.poll() is None:
            control.write_text('exit\n')
            try:
                host.wait(timeout=45)
            except subprocess.TimeoutExpired:
                forced = True
                host.terminate()
                try:
                    host.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    host.kill()
                    host.wait(timeout=10)
        if ui_pid and ui_identity and cleanup_identity(ui_pid) == ui_identity:
            deadline = time.monotonic() + 10
            while cleanup_identity(ui_pid) == ui_identity and time.monotonic() < deadline:
                time.sleep(.1)
            if cleanup_identity(ui_pid) == ui_identity:
                forced = True
                try:
                    os.kill(ui_pid, signal.SIGKILL)
                except ProcessLookupError:
                    # The owned UI exited after the identity check.
                    pass
                deadline = time.monotonic() + 10
                while cleanup_identity(ui_pid) == ui_identity and time.monotonic() < deadline:
                    time.sleep(.1)
        ui_state = cleanup_identity(ui_pid) if ui_pid else None
        ui_exited = not ui_pid or (ui_identity is not None and ui_state is not UNKNOWN_IDENTITY
                                   and ui_state != ui_identity)
        acknowledged = bool(evidence and evidence.exists() and any(
            row.get('step') == 'control-exit' and row.get('ok') is True
            for row in evidence_records(evidence)))
        cleanup = {}
        connection_owned = True
        if backend is not None:
            try:
                connections = list(Path(backend.home).rglob('daemon.conn'))
                connection_owned = all(json.loads(file.read_text())['pid'] == backend.process.pid for file in connections)
            except Exception:
                connection_owned = False
        if backend is not None and ui_exited and connection_owned:
            try:
                cleanup = backend.close()
            except Exception as error:
                cleanup = {'cleanupErrorType': type(error).__name__, 'sandboxPreserved': True}
        elif backend is not None:
            # Stop only the direct child, and preserve the private home if a UI still holds it.
            if backend.process and backend.process.poll() is None:
                forced = True
                backend.process.terminate()
                backend.process.wait(timeout=30)
            cleanup = {'sandboxPreserved': True, 'homeRemoved': False,
                       'daemonExited': backend.process is not None and backend.process.poll() is not None,
                       'daemonExitCode': backend.process.returncode if backend.process else None}
        cleanup.update(phase='cleanup', hostExitCode=host.returncode if host else None,
                       uiExited=ui_exited, uiIdentityKnown=ui_state is not UNKNOWN_IDENTITY,
                       controlExitAcknowledged=acknowledged,
                       forced=forced or cleanup.get('forced', False))
        if 'request' in locals():
            cleanup.update(runId=request.get('runId'), attemptId=request.get('attemptId'))
        passed = (not failed and host is not None and host.returncode == 0 and ui_exited and acknowledged
                  and cleanup.get('daemonExited') is True and cleanup.get('homeRemoved') is True
                  and cleanup.get('daemonExitCode') == 0 and not cleanup['forced'])
        cleanup['passed'] = passed
        if 'output' in locals():
            (output / 'native-isolation.json').write_text(json.dumps(cleanup, indent=2) + '\n')
        emit(cleanup)
    return 0 if passed else 1


if __name__ == '__main__':
    sys.exit(run())
