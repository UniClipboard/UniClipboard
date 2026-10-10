#!/usr/bin/env python3
"""Build the Windows Go GUI (the counterpart of build.sh, which is macOS-only).

  python apps/gui-go/e2e/build_windows.py --mode e2e|production [--cross-check-only]

Full build (run on a Windows host with Rust, Go, bun): uniclipd.exe (cargo), uniclip.exe (apps/cli-go), the shared
frontend bundle, and gui-go.exe, all placed in target/gui-go/windows-<mode>/. `--cross-check-only` runs on any host
with Go: it compiles gui-go.exe for windows/amd64 from an existing frontend bundle (apps/gui-go/frontend/dist) with
CGO disabled. That proves the code compiles and links for Windows; it does NOT prove it runs.
The manifest records the source commit and SHA-256 of each artifact.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TAGS = {'e2e': 'e2e', 'production': 'production'}


def run(cmd, cwd=ROOT, env=None):
    print('+', ' '.join(map(str, cmd)), flush=True)
    subprocess.run(cmd, cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--mode', choices=sorted(TAGS), required=True)
    parser.add_argument('--cross-check-only', action='store_true')
    args = parser.parse_args()
    out = ROOT / 'target/gui-go' / f'windows-{args.mode}{"-crosscheck" if args.cross_check_only else ""}'
    out.mkdir(parents=True, exist_ok=True)
    conf = json.loads((ROOT / 'apps/gui-go/app.json').read_text())
    bundle_id = conf['identifier'] + ('.e2e' if args.mode == 'e2e' else '')
    pubkey = '' if args.mode == 'e2e' else conf['updater']['pubkey']
    env = dict(os.environ, GOOS='windows', GOARCH='amd64', CGO_ENABLED='0') if args.cross_check_only else dict(os.environ, CGO_ENABLED='0')
    if not args.cross_check_only:
        run(['cargo', 'build', '--locked', '-p', 'uc-daemon'])
        run(['go', 'generate', './buildinfo'], cwd=ROOT / 'packages/desktop-host-go')
        run(['go', 'build', '-o', str(out / 'uniclip.exe'), './cmd/uniclip'], cwd=ROOT / 'apps/cli-go', env=env)
        run(['bun', '--bun', 'run', '--cwd', 'apps/gui-go/frontend', 'build'], env=dict(env, VITE_GUI_GO_E2E='1' if args.mode == 'e2e' else '0'))
        shutil.copy2(ROOT / 'target/debug/uniclipd.exe', out / 'uniclipd.exe')
    elif not (ROOT / 'apps/gui-go/frontend/dist').is_dir():
        sys.exit('apps/gui-go/frontend/dist is missing: build the frontend once (bun --bun run --cwd apps/gui-go/frontend build)')
    ldflags = f'-X main.updaterPublicKey={pubkey} -X main.productName={conf["productName"]} -X main.bundleID={bundle_id}'
    # -H windowsgui: a GUI-subsystem executable, so no console window appears next to the app.
    run(['go', 'build', '-tags', TAGS[args.mode], '-ldflags', ldflags + ' -H windowsgui', '-o', str(out / 'gui-go.exe'), '.'], cwd=ROOT / 'apps/gui-go', env=env)
    manifest = {'mode': args.mode, 'crossCheckOnly': args.cross_check_only,
                'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                # The artifacts are built from the working tree: a dirty tree means they contain changes HEAD does not.
                'dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip()),
                'diffSha256': hashlib.sha256(subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)).hexdigest(),
                'sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(out.glob('*.exe'))}}
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(manifest, indent=2))


if __name__ == '__main__':
    main()
