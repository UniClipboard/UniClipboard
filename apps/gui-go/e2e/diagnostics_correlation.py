"""Compare approved local correlation metadata with OTLP in the same real ZIP."""
from collections import defaultdict
from datetime import datetime, timezone
import json
import zipfile

TOP_LEVEL_ATTRIBUTES = {
    'local_schema_version', 'run_id', 'monotonic_offset_ms', 'engine_version',
    'host_version', 'platform', 'environment', 'app_channel', 'source_commit', 'source_state',
    'peer_ref', 'candidate_set_ref', 'candidate_generation', 'candidate_values_changed',
    'connection_id', 'path_ref', 'device_ref', 'address_record_ref',
    'correlation_status', 'connection_correlation_status', 'record_correlation_status',
    'capture_mode', 'capture_id', 'capture_tail_completion',
}


def nanos(value):
    delta = datetime.fromisoformat(value.replace('Z', '+00:00')) - datetime(1970, 1, 1, tzinfo=timezone.utc)
    return (delta.days * 86400 + delta.seconds) * 1_000_000_000 + delta.microseconds * 1000


def attributes(record):
    result = {}
    for attr in record.get('attributes', []):
        value = attr.get('value', {})
        item = next(iter(value.values()), None)
        if 'intValue' in value:
            item = int(item)
        assert attr['key'] not in result, 'duplicate OTLP attribute'
        result[attr['key']] = item
    return result


def compare(path):
    index = defaultdict(list)
    raw = []
    with zipfile.ZipFile(path) as archive:
        for line in archive.read('otel/logs.jsonl').splitlines():
            for resource in json.loads(line)['resourceLogs']:
                for scope in resource['scopeLogs']:
                    for record in scope['logRecords']:
                        index[int(record['timeUnixNano'])].append((record, attributes(record)))
        for name in archive.namelist():
            if name.startswith('logs/engine.') and name.endswith('.jsonl'):
                raw.extend(json.loads(line) for line in archive.read(name).splitlines() if line)
    result = {'otlpRecords': sum(len(items) for items in index.values()), 'matched': 0, 'comparedValues': {}, 'mismatches': [], 'runs': set(), 'traceContexts': 0}
    for record in raw:
        if not any(key in record for key in TOP_LEVEL_ATTRIBUTES):
            continue
        fields = {k: v for k, v in record.get('fields', {}).items() if v is None or isinstance(v, (str, bool, int))}
        candidates = index.get(nanos(record['timestamp']), [])
        match = next(((log, attrs) for log, attrs in candidates if all(attrs.get(k) == v for k, v in fields.items())), None)
        if match is None:
            continue  # Raw files can extend beyond the bounded OTLP history.
        log, attrs = match
        candidates.remove(match)
        result['matched'] += 1
        result['runs'].add(record['run_id'])
        for key in TOP_LEVEL_ATTRIBUTES & record.keys():
            result['comparedValues'][key] = result['comparedValues'].get(key, 0) + 1
            if key not in attrs or attrs[key] != record[key] or type(attrs[key]) is not type(record[key]):
                result['mismatches'].append({'key': key, 'at': record['timestamp']})
        assert set(attrs) <= set(record.get('fields', {})) | TOP_LEVEL_ATTRIBUTES, 'unapproved top-level attribute escaped'
        for key, otlp in [('trace_id', 'traceId'), ('span_id', 'spanId')]:
            if record.get(key):
                assert log.get(otlp, '').lower() == record[key].lower(), 'trace/span context changed'
                result['traceContexts'] += 1
    result['runs'] = sorted(result['runs'])
    result['unmatchedOtlp'] = sum(len(items) for items in index.values())
    result['passed'] = result['matched'] > 0 and not result['mismatches'] and result['unmatchedOtlp'] == 0
    return result


def run(out):
    """Exercise real peer traffic, export, restart, and reloaded run scopes without a GUI driver."""
    import hashlib
    import os
    from pathlib import Path
    import subprocess
    import sys
    import tempfile
    import time
    from run import ROOT, isolated_env
    from file_preview_run import cli
    from peers import pair

    out = Path(out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    homes = [tempfile.mkdtemp(prefix='uc-gui-go-correlation-') for _ in range(2)]
    environments = [isolated_env(home, 'gui-go-' + Path(home).name, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH']}) for home in homes]
    report = {'passed': False, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'enginePin': next(line for line in (ROOT / 'Cargo.toml').read_text().splitlines() if line.startswith('uc-engine =')), 'exports': {}, 'cleanupFailures': {}}
    marker = 'synthetic-correlation-' + os.urandom(8).hex()

    def export(name):
        value = json.loads(cli(environments[0], '--json', 'debug', 'export-logs').stdout)
        target = out / (name + '.zip')
        target.write_bytes(Path(value['path']).read_bytes())
        result = compare(target)
        result['zipSha256'] = hashlib.sha256(target.read_bytes()).hexdigest()
        report['exports'][name] = result
        assert result['passed'], result
        return result

    try:
        pair(*environments, 'synthetic-correlation-a', 'synthetic-correlation-b')
        cli(environments[0], 'debug', 'capture', 'start', '--minutes', '2')
        cli(environments[1], 'stop')
        cli(environments[1], 'start')
        report['sendExit'] = cli(environments[1], 'send', marker, check=False).returncode
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            got = cli(environments[0], 'get', check=False)
            if got.returncode == 0 and got.stdout.strip() == marker:
                break
            time.sleep(0.5)
        else:
            raise AssertionError('real paired transfer did not arrive')
        report['receivedExact'] = True
        deadline = time.monotonic() + 30
        required = {'peer_ref', 'connection_id', 'path_ref', 'capture_id', 'capture_mode', 'run_id'}
        while time.monotonic() < deadline:
            first = export('before-restart')
            if all(first['comparedValues'].get(key, 0) > 0 for key in required) and first['traceContexts'] > 0:
                break
            time.sleep(0.5)
        else:
            raise AssertionError('real traffic did not exercise all required correlation attributes')
        cli(environments[0], 'stop')
        cli(environments[0], 'start')
        after = export('after-restart')
        assert len(after['runs']) >= 2, 'source run scopes did not survive reload'
        assert set(first['runs']) & set(after['runs']), 'original run scope missing after restart'
        report['passed'] = True
    finally:
        body_failed = sys.exc_info()[0] is not None
        if body_failed:
            report['scenarioErrorType'] = type(sys.exc_info()[1]).__name__
        for index, env in enumerate(environments):
            try:
                result = cli(env, 'stop', check=False, timeout=30)
                report.setdefault('stopExits', {})[str(index)] = result.returncode
                if result.returncode:
                    report['cleanupFailures']['daemon:' + str(index)] = 'NonzeroExit'
            except Exception as error:
                report['cleanupFailures']['daemon:' + str(index)] = type(error).__name__
        for name in ['target/debug/uniclipd', 'target/gui-go/uniclip']:
            report.setdefault('binarySha256', {})[name] = None
            try:
                report['binarySha256'][name] = hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
            except Exception as error:
                report['cleanupFailures']['hash:' + name] = type(error).__name__
        if report['cleanupFailures']:
            report['passed'] = False
        (out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        if report['cleanupFailures'] and not body_failed:
            raise RuntimeError('correlation acceptance cleanup failed; inspect result.json')
