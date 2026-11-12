#!/usr/bin/env python3
"""Record compile-only platform evidence without launching a native application."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    out = args.output.resolve()
    if out.exists():
        parser.error('output already exists')
    out.mkdir(parents=True)
    paths = sorted(set(subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=root).decode().split('\0')) - {''})
    inputs = {name: sha(root / name) for name in paths if (root / name).is_file() and (name in ('go.mod', 'go.sum') or name.startswith(('engine/', 'internal/', 'plugin/', 'app/'))) and not name.endswith('.md')}
    receipt = {'status': 'running', 'native_ui': 'NOT_RUN', 'checks': [], 'binary_sha256': {}, 'platform': platform.platform()}
    with tempfile.TemporaryDirectory(prefix='unic-0024-compile-', dir='/private/tmp' if platform.system() == 'Darwin' else None) as private:
        for target, package, extra in [
            ('macos-desktop', './app/desktop/cmd/unic-desktop', {}),
            ('windows-service', './app/service/cmd/unicd', {'GOOS': 'windows', 'GOARCH': 'amd64', 'CGO_ENABLED': '0'}),
            ('linux-service', './app/service/cmd/unicd', {'GOOS': 'linux', 'GOARCH': 'amd64', 'CGO_ENABLED': '0'}),
        ]:
            binary = Path(private) / target
            command = ['go', 'build', '-buildvcs=false', '-trimpath', '-o', str(binary), package]
            result = subprocess.run(command, cwd=root, env=dict(os.environ, **extra), capture_output=True, text=True)
            receipt['checks'].append({'target': target, 'exit_code': result.returncode, 'flags': ['-buildvcs=false', '-trimpath']})
            if result.returncode:
                raise RuntimeError(result.stderr)
            receipt['binary_sha256'][target] = sha(binary)
        result = subprocess.run(['go', 'build', './app/desktop/internal/board'], cwd=root, env=dict(os.environ, GOOS='windows', GOARCH='amd64', CGO_ENABLED='0'), capture_output=True, text=True)
        receipt['checks'].append({'target': 'windows-clipboard-adapter', 'exit_code': result.returncode})
        if result.returncode:
            raise RuntimeError(result.stderr)
        receipt['source_unchanged'] = all(sha(root / name) == value for name, value in inputs.items())
    receipt['private_cleanup'] = 'PASS'
    receipt['status'] = 'PASS' if receipt['source_unchanged'] else 'FAIL'
    receipt['go'] = subprocess.check_output(['go', 'version'], text=True).strip()
    receipt['source_sha256'] = hashlib.sha256(json.dumps(inputs, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    (out / 'source-files.json.gz').write_bytes(gzip.compress(json.dumps(inputs, sort_keys=True, indent=2).encode(), mtime=0))
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    (out / 'SHA256SUMS').write_text('\n'.join(sha(path) + '  ' + path.name for path in sorted(out.iterdir()) if path.name != 'SHA256SUMS') + '\n')
    print(receipt['status'])
    return 0 if receipt['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
