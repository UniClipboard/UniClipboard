#!/usr/bin/env python3
"""Import one immutable diagnostic ZIP through Collector Contrib 0.139.0."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import urllib.request
import zipfile

VERSION = '0.139.0'
SIGNALS = {'metrics': ('metric_points', 'resourceMetrics', 'scopeMetrics', 'metrics'),
           'logs': ('log_records', 'resourceLogs', 'scopeLogs', 'logRecords'),
           'traces': ('spans', 'resourceSpans', 'scopeSpans', 'spans')}


def count_records(signal, data):
    _, resources, scopes, records = SIGNALS[signal]
    count = 0
    for line in data.splitlines():
        request = json.loads(line)
        if set(request) != {resources}:
            raise ValueError(f'invalid {signal} OTLP request')
        for resource in request[resources]:
            for scope in resource.get(scopes, []):
                for record in scope.get(records, []):
                    if signal == 'metrics':
                        count += sum(len(record.get(kind, {}).get('dataPoints', []))
                                     for kind in ('gauge', 'sum', 'histogram', 'exponentialHistogram', 'summary'))
                    else:
                        count += 1
    return count


def totals(text, name):
    return sum(float(line.rsplit(' ', 1)[1]) for line in text.splitlines()
               if re.match(re.escape(name) + r'(?:_total)?(?:\{| )', line))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('zip', type=Path)
    parser.add_argument('--collector', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=60)
    parser.add_argument('--allow-duplicate', action='store_true')
    args = parser.parse_args()
    if args.receipt.exists() and not args.allow_duplicate:
        parser.error('receipt already exists; inspect the previous outcome before --allow-duplicate')
    endpoint = os.environ.get('OTEL_IMPORT_ENDPOINT')
    if not endpoint:
        parser.error('set OTEL_IMPORT_ENDPOINT on the developer machine')
    collector = args.collector.resolve()
    version = subprocess.check_output([str(collector), '--version'], text=True).strip()
    if version != 'otelcol-contrib version ' + VERSION:
        parser.error('requires Collector Contrib ' + VERSION)
    receipt = {'status': 'started', 'zipSha256': hashlib.sha256(args.zip.read_bytes()).hexdigest(),
               'collectorVersion': VERSION, 'duplicateAllowed': args.allow_duplicate, 'duplicateRecords': None,
               'duplicateReason': 'backend deduplication is not observable through OTLP', 'expected': {}}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
    process = None
    try:
        with tempfile.TemporaryDirectory(prefix='uc-otel-import-') as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(args.zip) as archive:
                manifest = json.loads(archive.read('otel/manifest.json'))
                expected, receivers, pipelines = {}, {}, {}
                for signal, (counter, *_) in SIGNALS.items():
                    name = f'otel/{signal}.jsonl'
                    declared = next((f for f in manifest['files'] if f['name'] == name), None)
                    if declared is None:
                        expected[signal] = 0
                        continue
                    data = archive.read(name)
                    if len(data) != declared['bytes'] or hashlib.sha256(data).hexdigest() != declared['sha256']:
                        raise ValueError(f'{signal} hash or length mismatch')
                    expected[signal] = count_records(signal, data)
                    if not expected[signal]:
                        continue
                    path = root / f'{signal}.jsonl'
                    path.write_bytes(data)
                    receiver = f'otlpjsonfile/{signal}'
                    receivers[receiver] = {'include': [str(path)], 'start_at': 'beginning', 'include_file_name': False}
                    pipelines[signal] = {'receivers': [receiver], 'exporters': ['otlphttp']}
            receipt['expected'] = expected
            if not pipelines:
                raise ValueError('ZIP has no readable OTLP records; inspect coverage and locked-history state')
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            config = {'receivers': receivers, 'exporters': {'otlphttp': {
                'endpoint': '${env:OTEL_IMPORT_ENDPOINT}', 'retry_on_failure': {'enabled': False},
                'sending_queue': {'enabled': False}}}, 'service': {'pipelines': pipelines, 'telemetry': {'metrics': {
                'readers': [{'pull': {'exporter': {'prometheus': {'host': '127.0.0.1', 'port': port}}}}]}}}}
            if os.environ.get('OTEL_IMPORT_AUTHORIZATION'):
                config['exporters']['otlphttp']['headers'] = {'Authorization': '${env:OTEL_IMPORT_AUTHORIZATION}'}
            config_path = root / 'collector.json'
            config_path.write_text(json.dumps(config))
            # Do not copy credentials or an authenticated endpoint into the receipt.
            with (root / 'collector.log').open('wb') as log:
                process = subprocess.Popen([str(collector), '--config=' + str(config_path)], stdout=log, stderr=log)
                deadline = time.monotonic() + args.timeout
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError('Collector exited before verified delivery')
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{port}/metrics', timeout=1) as response:
                            metrics = response.read().decode()
                    except OSError:
                        time.sleep(.2)
                        continue
                    sent = {s: totals(metrics, 'otelcol_exporter_sent_' + v[0]) for s, v in SIGNALS.items()}
                    failed = {s: totals(metrics, 'otelcol_exporter_send_failed_' + v[0]) for s, v in SIGNALS.items()}
                    accepted = {s: totals(metrics, 'otelcol_receiver_accepted_' + v[0]) for s, v in SIGNALS.items()}
                    refused = {s: totals(metrics, 'otelcol_receiver_refused_' + v[0]) for s, v in SIGNALS.items()}
                    receipt.update(sent=sent, failed=failed, accepted=accepted, refused=refused)
                    if any(failed.values()) or any(refused.values()):
                        raise RuntimeError('Collector reported delivery failure; do not automatically retry')
                    if sent == expected and accepted == expected:
                        receipt['status'] = 'delivered'
                        receipt['backendQuery'] = 'NOT_RUN; query the target platform independently'
                        break
                    time.sleep(.2)
                else:
                    raise TimeoutError('delivery counters did not match the immutable ZIP')
    except Exception as error:
        receipt['status'] = 'failed_or_unknown'
        receipt['errorType'] = type(error).__name__
        raise
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
                receipt['cleanup'] = 'forced'
            else:
                receipt['cleanup'] = 'graceful' if process.returncode == 0 else 'nonzero'
        args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
