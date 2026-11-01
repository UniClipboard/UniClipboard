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
        fields = {k: v for k, v in record.get('fields', {}).items() if isinstance(v, (str, bool, int))}
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
        assert set(attrs) <= set(fields) | TOP_LEVEL_ATTRIBUTES, 'unapproved top-level attribute escaped'
        for key, otlp in [('trace_id', 'traceId'), ('span_id', 'spanId')]:
            if record.get(key):
                assert log.get(otlp, '').lower() == record[key].lower(), 'trace/span context changed'
                result['traceContexts'] += 1
    result['runs'] = sorted(result['runs'])
    result['unmatchedOtlp'] = sum(len(items) for items in index.values())
    result['passed'] = result['matched'] > 0 and not result['mismatches'] and result['unmatchedOtlp'] == 0
    return result
