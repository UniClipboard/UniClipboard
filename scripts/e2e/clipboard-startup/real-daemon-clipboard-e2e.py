#!/usr/bin/env python3
"""Exercise a real macOS daemon on a private named pasteboard and isolated profile."""
import argparse, hashlib, json, os, shutil, signal, sqlite3, subprocess, sys, time, tomllib, urllib.request, uuid, zipfile
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--daemon', type=Path, required=True)
parser.add_argument('--tools', type=Path, required=True)
parser.add_argument('--artifacts', type=Path, required=True)
args = parser.parse_args()
root = args.artifacts.resolve() / ('run-' + uuid.uuid4().hex)
root.mkdir(parents=True)
bin_dir = root / 'bin'
bin_dir.mkdir()
binary = bin_dir / 'uniclipd'
shutil.copy2(args.daemon.resolve(), binary)
profile = 'e2e-clipboard-' + uuid.uuid4().hex
pasteboard = 'org.uniclipboard.e2e.' + uuid.uuid4().hex
source = root / 'private-file-sentinel.png'
source.write_bytes(bytes([42]) * (256 * 1024))
base_env = {**os.environ, 'UC_PROFILE': profile, 'UC_PORTABLE': '1', 'UNICLIPBOARD_ENV': 'development',
            'UC_DAEMON_RUN_MODE': 'standalone', 'UC_E2E_PRIVATE_PASTEBOARD': pasteboard,
            'UC_E2E_SOURCE_FILE': str(source), 'DYLD_INSERT_LIBRARIES': str((args.tools / 'private-pasteboard.dylib').resolve())}
base_env.pop('UC_DISABLE_SYSTEM_CLIPBOARD', None)
base_env.pop('UC_LOG_DIR', None)
base_env.pop('UC_ENGINE_LOG_DIR', None)
data = bin_dir / 'data' / ('app.uniclipboard.desktop-' + profile)
process = None
session = None
endpoint = None
cases = []
processes = []
repository = Path(__file__).resolve().parents[3]
engine_revision = tomllib.loads((repository / 'Cargo.toml').read_text())['workspace']['dependencies']['uc-engine']['rev']
source_paths = ['Cargo.toml', 'Cargo.lock', 'crates/uc-bootstrap/src/wiring/desktop_host.rs',
                'crates/uc-platform/src/clipboard/common.rs',
                'scripts/e2e/clipboard-startup/private-pasteboard.m',
                'scripts/e2e/clipboard-startup/real-daemon-clipboard-e2e.py']
provenance = {
    'engine_revision': engine_revision,
    'daemon_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    'source_sha256': {name: hashlib.sha256((repository / name).read_bytes()).hexdigest() for name in source_paths},
    'tools_sha256': {name: hashlib.sha256((args.tools / name).read_bytes()).hexdigest() for name in ['private-pasteboard', 'private-pasteboard.dylib']},
    'command': ['python3', str(Path(__file__)), '--daemon', str(args.daemon), '--tools', str(args.tools), '--artifacts', str(args.artifacts)],
}
(root / 'source-provenance.json').write_text(json.dumps(provenance, indent=2))


def board(kind, value=None):
    command = [str((args.tools / 'private-pasteboard').resolve()), kind]
    if value is not None: command.append(str(value))
    subprocess.run(command, env={**os.environ, 'UC_E2E_PRIVATE_PASTEBOARD': pasteboard}, check=True, stdout=subprocess.DEVNULL)


def api(method, route, payload=None, authorization=None):
    request = urllib.request.Request(endpoint + route, method=method,
        data=None if payload is None else json.dumps(payload).encode(),
        headers={'Content-Type':'application/json', **({'Authorization': authorization or 'Session ' + session} if authorization or session else {})})
    with urllib.request.urlopen(request, timeout=10) as response:
        value=json.load(response)
    return value.get('data', value)


def start(fault='none'):
    global process, session, endpoint
    log = (root / ('daemon-' + str(len(processes)) + '-' + fault + '.log')).open('wb')
    process = subprocess.Popen([str(binary)], env={**base_env, 'UC_E2E_FILE_FAULT': fault}, stdout=log, stderr=log)
    log.close()
    processes.append({'pid': process.pid, 'fault': fault})
    deadline=time.monotonic()+40
    while time.monotonic()<deadline:
        if process.poll() is not None: raise AssertionError('daemon exited before health')
        try:
            connection=json.loads((data/'daemon.conn').read_text())
            if connection['pid'] != process.pid: raise ValueError('old connection')
            endpoint='http://127.0.0.1:'+str(connection['port'])
            health=api('GET','/health')
            session=api('POST','/auth/connect',{'pid':os.getpid(),'clientType':'cli'},'Bearer '+connection['token'])['sessionToken']
            return {'pid':process.pid, 'health':health.get('status', 'ready')}
        except (OSError, ValueError, KeyError): time.sleep(.1)
    raise AssertionError('daemon health timeout')


def stop():
    global process, session
    if process:
        process.send_signal(signal.SIGTERM)
        try: rc=process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait(); raise
        processes[-1]['exit_code']=rc
        process=None; session=None
        return rc
    return None


def register_count():
    paths=list(data.glob('profile-data-generations/*/v3-payloads/profile.sqlite'))
    assert len(paths)==1, 'profile database missing'
    with sqlite3.connect('file:'+str(paths[0])+'?mode=ro', uri=True) as db:
        return db.execute('SELECT COUNT(*) FROM active_clipboard_register').fetchone()[0]


def activate_text():
    board('text','isolated-daemon-text-'+uuid.uuid4().hex)
    entry=api('POST','/clipboard/capture-current',{})['entryId']
    assert entry, 'normal text capture missing'
    api('POST','/clipboard/restore/'+entry,{})
    assert register_count()==1, 'active register not established'

try:
    board('text','initial-isolated-clipboard')
    ready=start()
    api('POST','/v2/setup/initialize', {'passphrase':'isolated-daemon-passphrase','passphraseConfirm':'isolated-daemon-passphrase','deviceName':'isolated daemon'})
    activate_text()
    rc=stop(); assert rc==0
    cases.append({'case':'initialization','pid':ready['pid'],'exit_code':rc})
    for fault in ['denied','missing','middle']:
        source.chmod(0o000 if fault=='denied' else 0o600)
        board('file',source)
        ready=start(fault)
        assert register_count()==0, 'untrusted register not cleared'
        imports=data/'cache/engine-tmp/clipboard-imports'
        assert not imports.exists() or not list(imports.iterdir()), 'failed import remnants'
        first_pid=ready['pid']
        assert stop()==0
        ready=start(fault)
        assert register_count()==0, 'cleared register did not survive same-fault restart'
        source.chmod(0o600)
        activate_text()
        rc=stop(); assert rc==0
        cases.append({'case':fault,'pid':ready['pid'],'exit_code':rc,'register_cleared':True,'subsequent_text_capture':True,'no_failed_imports':True,'first_recovery_pid':first_pid,'same_fault_restart':True})
    ready=start()
    assert register_count()==1, 'matching text register not retained on restart'
    # Capture a real host file with fresh contents to avoid snapshot deduplication.
    source.write_bytes(bytes([43])*(256*1024))
    board('file',source)
    captured=api('POST','/clipboard/capture-current',{})
    assert captured['entryId'], 'normal file capture missing'
    before=set(Path.home().joinpath('Downloads').glob('uniclipboard-diagnostics-*.zip'))
    exported=api('POST','/diagnostics/log-export',{'sinceHours':24})
    export_path=Path(exported['path'])
    assert export_path not in before, 'export would replace an existing archive'
    shutil.copy2(export_path, root/'diagnostics.zip')
    export_path.unlink()
    with zipfile.ZipFile(root/'diagnostics.zip') as bundle:
        all_logs='\n'.join(bundle.read(name).decode() for name in bundle.namelist() if name.startswith('logs/'))
        logs='\n'.join(bundle.read(name).decode() for name in bundle.namelist() if name.endswith('.jsonl'))
    records=[json.loads(line) for line in logs.splitlines() if line]
    reads=[record for record in records if record.get('fields',{}).get('error_kind')=='active_clipboard_os_read']
    assert len(reads)>=3, 'default standard archive lost reconcile failures'
    for kind, code in [('PermissionDenied', 13), ('NotFound', 2), ('Uncategorized', 5)]:
        assert any(kind in str(record.get('error.chain')) and 'os_code='+str(code) in str(record.get('error.chain')) for record in reads), 'source classification/code missing: '+kind
    assert all(record.get('capture_mode')=='standard' for record in reads)
    assert {record['source_commit'] for record in records if 'source_commit' in record} == {engine_revision}, 'runtime Engine revision differs from product pin'
    assert all(record.get('source_state')=='clean' for record in records if 'source_commit' in record), 'runtime Engine source is dirty'
    assert any('os_code=5' in str(record.get('error.chain')) for record in reads), 'middle-read source missing'
    assert any(name.startswith('logs/engine.') for name in bundle.namelist()), 'Engine logs missing'
    assert any(name.startswith('logs/uniclipboard-daemon.') for name in bundle.namelist()), 'host logs missing'
    assert 'private-file-sentinel' not in all_logs, 'source filename leaked into standard archive'
    status=api('GET','/status')
    rc=stop(); assert rc==0
    cases.append({'case':'normal_file_and_restart','pid':ready['pid'],'exit_code':rc,'file_captured':True,'standard_read_failures':len(reads)})
    (root/'result.json').write_text(json.dumps({'passed':True,'cases':cases,'processes':processes,'boundary':'real uniclipd/HTTP/profile/native NSPasteboard and file IO; private pasteboard swizzle plus missing/middle syscall fault injection; no real Finder/TCC permission proof','engine_source_status':status.get('version', 'inspect diagnostic source_commit/source_state')},indent=2))
    print(json.dumps({'passed':True,'artifact_directory':str(root),'daemon_pids':[case['pid'] for case in cases]}))
except BaseException as error:
    (root/'result.json').write_text(json.dumps({'passed':False,'completed_cases':cases,'processes':processes,'failure_type':type(error).__name__},indent=2))
    print(json.dumps({'passed':False,'artifact_directory':str(root)}))
    raise
finally:
    primary_failed = sys.exc_info()[0] is not None
    cleanup_errors = []
    if process:
        try:
            process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=20)
            processes[-1]['exit_code'] = process.returncode
        except Exception as cleanup_error:
            cleanup_errors.append({'step': 'daemon', 'error_type': type(cleanup_error).__name__})
    for step, action in [
        ('pasteboard', lambda: board('release')),
        ('source_permissions', lambda: source.chmod(0o600)),
        # Discard only this run's generated profile, keys and history.
        ('profile', lambda: shutil.rmtree(bin_dir/'data') if (bin_dir/'data').exists() else None),
    ]:
        try:
            action()
        except Exception as cleanup_error:
            cleanup_errors.append({'step': step, 'error_type': type(cleanup_error).__name__})
    try:
        (root/'cleanup.json').write_text(json.dumps({'passed': not cleanup_errors, 'errors': cleanup_errors, 'processes': processes}, indent=2))
    except Exception as cleanup_error:
        cleanup_errors.append({'step': 'cleanup_evidence', 'error_type': type(cleanup_error).__name__})
    if cleanup_errors:
        print(json.dumps({'cleanup_errors': cleanup_errors}))
        if not primary_failed:
            result = json.loads((root/'result.json').read_text())
            result.update({'passed': False, 'failure_type': 'CleanupError'})
            (root/'result.json').write_text(json.dumps(result, indent=2))
            raise RuntimeError('test resource cleanup failed; inspect cleanup.json')
