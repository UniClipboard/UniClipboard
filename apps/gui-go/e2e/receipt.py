"""Evidence receipts for the host command contract E2E runs.

A receipt ties one run to the exact source it ran from: the git HEAD (and whether the work tree was clean), the
SHA256 index of the generated Wails bindings and of the error catalog file, the SHA256 of the application and
daemon binaries that were launched, the scope of calls the run made, and the platforms that were NOT run. Evidence of
different source SHAs must never share a directory: `write_receipt` refuses to write into a directory that already
holds a receipt of another SHA, and it never deletes earlier evidence.
"""
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BINDINGS = ROOT / 'apps/gui-go/frontend/bindings'
CATALOG = ROOT / 'apps/gui-go/frontend/src/lib/host-errors.generated.ts'

# Platforms without an authorised host or VM in this work: cross-compiling is a build check, not a runtime check.
NOT_RUN = [
    {'platform': 'linux', 'reason': 'no authorised host or VM for this work; Linux gates are static (CI) only'},
    {'platform': 'windows', 'reason': 'no authorised host or VM for this work; cross-compilation is not runtime evidence'},
]


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b''):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True).strip()


def source_identity():
    # Generated frontend output (dist) and evidence directories are build products, not source.
    status = [line for line in git('status', '--porcelain', '--', 'apps', 'packages', 'scripts', 'docs', 'schema').splitlines()
              if '/frontend/dist/' not in line]
    return {'head': git('rev-parse', 'HEAD'), 'dirty': bool(status), 'dirtyPaths': status}


def bindings_index():
    files = {str(p.relative_to(BINDINGS)): sha256_file(p) for p in sorted(BINDINGS.rglob('*')) if p.is_file()}
    tree = hashlib.sha256('\n'.join(f'{name} {digest}' for name, digest in files.items()).encode()).hexdigest()
    return {'treeSha256': tree, 'files': files, 'catalogSha256': sha256_file(CATALOG)}


def binary_identity(path):
    path = Path(path)
    return {'path': str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path), 'sha256': sha256_file(path)}


def write_receipt(out, name, *, scope, assertions, binaries, before=None, after=None, extra=None):
    """Write `<name>-receipt.json` and refresh `SHA256SUMS` (an index of every file in `out`)."""
    out = Path(out)
    source = source_identity()
    receipt_path = out / f'{name}-receipt.json'
    for existing in out.glob('*-receipt.json'):
        other = json.loads(existing.read_text())
        if other.get('source', {}).get('head') != source['head']:
            raise RuntimeError(f'{out} holds evidence of {other["source"]["head"]}; use a separate directory for {source["head"]}')
    receipt = {
        'receipt': name,
        'source': source,
        'bindings': bindings_index(),
        'binaries': {key: binary_identity(value) for key, value in binaries.items()},
        'scope': scope,
        'before': before,
        'after': after,
        'assertions': assertions,
        'notRun': NOT_RUN,
    }
    if extra:
        receipt.update(extra)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    index = [f'{sha256_file(p)}  {p.name}' for p in sorted(out.iterdir()) if p.is_file() and p.name != 'SHA256SUMS']
    (out / 'SHA256SUMS').write_text('\n'.join(index) + '\n')
    return receipt
