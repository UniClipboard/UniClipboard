#!/usr/bin/env python3
"""Build the Windows Go GUI (the counterpart of build.sh, which is macOS-only).

  python apps/gui-go/e2e/build_windows.py --mode e2e|production [--arch amd64|arm64] [--part all|go|rust]
                                          [--cross-check-only]

Full build (run on a Windows host with Rust, Go, bun): uniclipd.exe and the quick panel helper
uniclip-quick-panel.exe (cargo), uniclip.exe (apps/cli-go), the shared frontend bundle, and gui-go.exe, all placed in
target/gui-go/windows-<mode>/ (plus `-<arch>` when --arch is not the host's). `--cross-check-only` runs on any host
with Go: it compiles gui-go.exe for windows/amd64 from an existing frontend bundle (apps/gui-go/frontend/dist) with
CGO disabled. That proves the code compiles and links for Windows; it does NOT prove it runs.

`--part go` builds the Go side only (uniclip.exe, gui-go.exe, and the frontend if there is no bundle yet) and, with
--arch, cross-compiles it from any host with Go and bun. `--part rust` builds only the cargo side, which needs a
Windows host. A machine without Go builds the Rust half and receives the Go half from another machine, which is how
`windows_native_panel_remote.py` drives a test machine.
The manifest records the source commit and SHA-256 of each artifact.
"""
import argparse
import hashlib
import json
import os
import platform
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
    parser.add_argument('--arch', choices=['amd64', 'arm64'], default=None)
    parser.add_argument('--part', choices=['all', 'go', 'rust'], default='all')
    args = parser.parse_args()
    if args.cross_check_only and (args.arch or args.part != 'all'):
        sys.exit('--cross-check-only builds gui-go.exe for amd64 and takes neither --arch nor --part')
    host_arch = 'arm64' if platform.machine().lower() in ('arm64', 'aarch64') else 'amd64'
    arch = args.arch or host_arch
    suffix = f'-{arch}' if arch != host_arch or sys.platform != 'win32' else ''
    out = ROOT / 'target/gui-go' / f'windows-{args.mode}{suffix}{"-crosscheck" if args.cross_check_only else ""}'
    out.mkdir(parents=True, exist_ok=True)
    conf = json.loads((ROOT / 'apps/gui-go/app.json').read_text())
    bundle_id = conf['identifier'] + ('.e2e' if args.mode == 'e2e' else '')
    pubkey = '' if args.mode == 'e2e' else conf['updater']['pubkey']
    env = dict(os.environ, CGO_ENABLED='0')
    if args.cross_check_only:
        env.update(GOOS='windows', GOARCH='amd64')
    elif sys.platform != 'win32' or arch != host_arch:
        env.update(GOOS='windows', GOARCH=arch)
    if args.part in ('all', 'rust') and not args.cross_check_only:
        if sys.platform != 'win32':
            sys.exit('the cargo half builds Windows executables and has to run on a Windows host')
        run(['cargo', 'build', '--locked', '-p', 'uc-daemon', '-p', 'quick-panel', '--bin', 'uniclipd', '--bin', 'uniclip-quick-panel'])
        for name in ('uniclipd.exe', 'uniclip-quick-panel.exe'):
            shutil.copy2(ROOT / 'target/debug' / name, out / name)
    if args.part in ('all', 'go') and not args.cross_check_only:
        run(['go', 'generate', './buildinfo'], cwd=ROOT / 'packages/desktop-host-go')
        run(['go', 'build', '-o', str(out / 'uniclip.exe'), './cmd/uniclip'], cwd=ROOT / 'apps/cli-go', env=env)
        run(['bun', '--bun', 'run', '--cwd', 'apps/gui-go/frontend', 'build'], env=dict(env, VITE_GUI_GO_E2E='1' if args.mode == 'e2e' else '0', VITE_APP_VERSION=conf['version']))
    if args.cross_check_only and not (ROOT / 'apps/gui-go/frontend/dist').is_dir():
        sys.exit('apps/gui-go/frontend/dist is missing: build the frontend once (bun --bun run --cwd apps/gui-go/frontend build)')
    if args.part == 'rust':
        print(json.dumps({p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(out.glob('*.exe'))}, indent=2))
        return
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
