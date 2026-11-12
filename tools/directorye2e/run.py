#!/usr/bin/env python3
"""Freeze source and accept directory transfers without a native clipboard."""
import argparse
import hashlib
import gzip
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--run', default='^TestCLI/(directory.*|content-formats|pair-and-sync-text|file-size-policy|host-received-files|history-auto-cleanup|history-cache-quota|history-restore-sync|sync-settings|send-to-device)$')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    if output.exists():
        parser.error('output already exists; choose a new receipt directory')
    output.mkdir(parents=True)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    paths = sorted(set(subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=root).decode().split('\0')) - {''})
    receipt = {'source_head': head, 'platform': platform.platform(), 'native_clipboard': 'NOT_RUN', 'real_devices': 'NOT_RUN', 'status': 'running', 'commands': [], 'results': []}
    started = time.monotonic()
    try:
        with tempfile.TemporaryDirectory(prefix='unic-0024-', dir='/private/tmp' if platform.system() == 'Darwin' else None) as private:
            private = Path(private)
            source, binaries, trees = private / 'source', private / 'bin', private / 'trees'
            source.mkdir(); binaries.mkdir(); trees.mkdir()
            source_files = {}
            for name in paths:
                path = root / name
                # Documentation receipts are not build inputs. Exclude the new output.
                if not path.exists() or output == path or output in path.parents:
                    continue
                if not path.is_file():
                    continue
                target = source / name
                target.parent.mkdir(parents=True, exist_ok=True)
                if path.is_symlink():
                    if root not in path.resolve().parents:
                        raise RuntimeError('source symlink escapes repository: ' + name)
                    target.symlink_to(os.readlink(path))
                else:
                    shutil.copy2(path, target)
                source_files[name] = digest(path)
            encoded = json.dumps(source_files, sort_keys=True, separators=(',', ':')).encode()
            receipt['source_sha256'] = hashlib.sha256(encoded).hexdigest()
            build_inputs = {k: v for k, v in source_files.items() if not k.startswith('docs/') and not k.endswith('.md')}
            receipt['build_input_sha256'] = hashlib.sha256(json.dumps(build_inputs, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
            (output / 'source-files.json.gz').write_bytes(gzip.compress((json.dumps(source_files, indent=2, sort_keys=True) + '\n').encode(), mtime=0))
            env = os.environ.copy()
            env.update({'TMPDIR': str(private), 'UNIC_E2E': '1', 'UNIC_E2E_BIN': str(binaries), 'UNIC_E2E_ARTIFACTS': str(trees)})
            receipt['go'] = subprocess.check_output(['go', 'version'], text=True).strip()
            for name in ('unic', 'unicd'):
                command = ['go', 'build', '-buildvcs=false', '-trimpath', '-race', '-tags', 'unic_e2e', '-o', str(binaries / name), './app/service/cmd/' + name]
                result = subprocess.run(command, cwd=source, env=env, capture_output=True, text=True)
                receipt['commands'].append({'command': ['go', 'build', '-buildvcs=false', '-trimpath', '-race', '-tags', 'unic_e2e', '-o', '<private>/bin/' + name, './app/service/cmd/' + name], 'exit_code': result.returncode})
                if result.returncode:
                    raise RuntimeError(result.stderr.replace(str(private), '<private>'))
            receipt['binary_sha256'] = {name: digest(binaries / name) for name in ('unic', 'unicd')}
            command = ['go', 'test', '-race', '-json', './e2e/cli', '-run', args.run, '-parallel', '4', '-count=1']
            stdout_path, stderr_path = private / 'tests.jsonl', private / 'tests.stderr'
            with stdout_path.open('w') as stdout_file, stderr_path.open('w') as stderr_file:
                process = subprocess.Popen(command, cwd=source, env=env, stdout=subprocess.PIPE, stderr=stderr_file, text=True)
                for line in process.stdout:
                    stdout_file.write(line)
                    try:
                        event = json.loads(line)
                        if event.get('Action') in ('pass', 'fail', 'skip') and event.get('Test'):
                            print(event['Action'].upper() + ' ' + event['Test'], flush=True)
                    except ValueError:
                        pass
                exit_code = process.wait()
            result = subprocess.CompletedProcess(command, exit_code, stdout_path.read_text(), stderr_path.read_text())
            receipt['commands'].append({'command': command, 'exit_code': result.returncode})
            for line in result.stdout.splitlines():
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if event.get('Action') == 'output' and 'FAIL:' in event.get('Output', ''):
                    messages = [line.strip().replace(str(private), '<private>') for line in event['Output'].splitlines() if 'FAIL:' in line]
                    receipt.setdefault('failures', []).append({'case': event.get('Test'), 'messages': messages[-3:]})
                if event.get('Action') in ('pass', 'fail', 'skip'):
                    receipt['results'].append({'case': event.get('Test', event.get('Package')), 'result': event['Action'].upper(), 'seconds': event.get('Elapsed')})
            for path in trees.glob('*.json'):
                shutil.copy2(path, output / path.name)
            # Verify the source used for building did not change during acceptance.
            receipt['frozen_source_unchanged'] = all(digest(source / name) == value for name, value in source_files.items())
            receipt['status'] = 'PASS' if result.returncode == 0 and receipt['frozen_source_unchanged'] else 'FAIL'
            if result.returncode:
                # Private logs are shown for diagnosis but not retained in the receipt.
                failed = {event['case'] for event in receipt.get('failures', [])}
                for raw in result.stdout.splitlines():
                    try:
                        event = json.loads(raw)
                        if event.get('Test') in failed and event.get('Action') == 'output':
                            print(event.get('Output', '').replace(str(private), '<private>'), end='')
                    except ValueError:
                        pass
                print(result.stderr[-2000:].replace(str(private), '<private>'))
        receipt['private_cleanup'] = 'PASS'
    except Exception as error:
        receipt['status'] = 'FAIL'
        receipt['failure'] = str(error)
        receipt['private_cleanup'] = 'attempted by temporary-directory owner'
    finally:
        receipt['seconds'] = round(time.monotonic() - started, 3)
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
        (output / 'report.md').write_text('# Directory process acceptance\n\nStatus: ' + receipt['status'] + '\n\nNative clipboard and real-device acceptance: NOT_RUN.\n\nSee `receipt.json` for exact source, binary, command and scenario identities. Private source, binaries, profiles and raw logs are removed after the run.\n')
        (output / 'replay.sh').write_text('#!/bin/sh\nset -eu\n# Run from the repository with the source digest recorded in receipt.json.\npython3 tools/directorye2e/run.py --output "$1"\n')
        sums = [digest(p) + '  ' + p.name for p in sorted(output.iterdir()) if p.is_file() and p.name != 'SHA256SUMS']
        (output / 'SHA256SUMS').write_text('\n'.join(sums) + '\n')
    print(str(output) + ': ' + receipt['status'])
    return 0 if receipt['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
