#!/usr/bin/env python3
"""Collect Desktop E2E timings from runner manifests and report them in CI."""
import argparse
import json
from pathlib import Path

from run import cases, select_shard, write_json


def collect(manifest, output):
    data = json.loads(manifest.read_text(encoding='utf-8'))
    measured = {case_id: result['elapsed_ms'] for case_id, result in data['cases'].items()
                if 'elapsed_ms' in result}
    write_json(output, dict(shard=data['shard'], status=data['status'], source_head=data.get('source_head'),
                            elapsed_ms=data['elapsed_ms'], case_ms=measured))


def report(shards, directory, source, run_url):
    specs = [spec for spec in cases() if spec['environment'] == 'isolated']
    complete = True
    shard_ms, measured, seen = {}, {}, set()
    print('## Desktop E2E timings\n')
    for index in range(1, shards + 1):
        shard = f'{index}/{shards}'
        path = directory / f'desktop-e2e-timing-{index}' / 'timing.json'
        if not path.is_file():
            print(f'- Shard {shard}: timing missing')
            complete = False
            continue
        data = json.loads(path.read_text(encoding='utf-8'))
        timings = data['case_ms']
        expected = set(select_shard(specs, index, shards))
        if data['shard'] != shard or timings.keys() - expected or timings.keys() & seen or any(
                type(ms) is not int or ms <= 0 for ms in timings.values()):
            raise ValueError(f'invalid case coverage for shard {shard}')
        if data['status'] == 'PASS' and (timings.keys() != expected or data['source_head'] != source):
            raise ValueError(f'incomplete case timing or incorrect source for passing shard {shard}')
        complete &= data['status'] == 'PASS'
        seen.update(timings)
        measured.update(timings)
        shard_ms[shard] = data['elapsed_ms']
        print(f"- Shard {shard}: {data['elapsed_ms'] / 1000:.1f}s runner time ({data['status']})")
    print()
    if measured:
        case_id = max(measured, key=measured.__getitem__)
        print(f'**Slowest measured case:** `{case_id}` ({measured[case_id] / 1000:.1f}s)\n')
    if shard_ms:
        shard = max(shard_ms, key=shard_ms.__getitem__)
        print(f'**Slowest measured shard:** {shard} ({shard_ms[shard] / 1000:.1f}s runner time)\n')
    for case_id, ms in sorted(measured.items(), key=lambda item: (-item[1], item[0])):
        if ms >= 120_000:
            label = 'Review' if ms >= 180_000 else 'Notice'
            print(f'- {label}: `{case_id}` took {ms / 1000:.1f}s')
    if complete:
        output = directory / 'timings.json'
        write_json(output, dict(source_head=source, run_url=run_url, case_ms=dict(sorted(measured.items()))))
        print('\nThe `desktop-e2e-timing-baseline` artifact contains a candidate baseline for review.')
    else:
        print('\nTiming coverage is incomplete. A passing baseline requires every shard to pass.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest='action', required=True)
    collect_parser = actions.add_parser('collect')
    collect_parser.add_argument('manifest', type=Path)
    collect_parser.add_argument('output', type=Path)
    report_parser = actions.add_parser('report')
    report_parser.add_argument('shards', type=int)
    report_parser.add_argument('directory', type=Path)
    report_parser.add_argument('source')
    report_parser.add_argument('run_url')
    args = parser.parse_args()
    if args.action == 'collect':
        collect(args.manifest, args.output)
    else:
        report(args.shards, args.directory, args.source, args.run_url)


if __name__ == '__main__':
    main()
