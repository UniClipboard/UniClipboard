#!/usr/bin/env python3
"""Package the Windows Go GUI: release executable, NSIS installer and portable zip.

  python apps/gui-go/e2e/package_windows.py --arch amd64|arm64 --daemon <path to uniclipd.exe> \
      --daemon-provenance <sidecar-provenance.json> --out <dir>

Runs on any host that has Go, bun, makensis and network access to install the pinned wails3 CLI (macOS, Linux or
Windows). The daemon is NOT built here: `uniclipd.exe` for the same architecture must be supplied (the CI
`build-sidecar` artifact); a missing file is a hard error, as in `.github/workflows/build.yml`. The supplied file
must be the one `build-sidecar` recorded in `sidecar-provenance.json` (same SHA-256, same source commit, same target
triple, clean tree): anything else is refused, so a package can only carry a daemon that a CI build produced from
the commit being packaged.

Outputs in <dir>:
  UniClipboard.exe                       release build (tags production,release; -H windowsgui; resources via `wails3 generate syso`)
  UniClipboard_<version>_<x64|arm64>-setup.exe      NSIS installer (apps/gui-go/windows/installer.nsi, Tauri-compatible contract)
  UniClipboard_<version>_<x64|arm64>-portable.zip   exe + uniclipd.exe + portable.dat + README (same content as build.yml)
  package-manifest.json                  provenance: HEAD, dirty flag, diff hash, tool versions, SHA-256 of every output

What this proves: the artifacts build, the installer script compiles and the packaged daemon is the CI-built one
(the payload itself is checked afterwards by windows_package_verify.py). It does NOT prove the installer, the exe or
the update flow run on Windows (windows_package_acceptance.py does, on a Windows host), and the outputs are NOT signed
(no Authenticode; the updater `.sig` comes from the updater-signatures workflow): signing stays in the release workflow.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
GUI = ROOT / 'apps/gui-go'
ARCH_NAMES = {'amd64': 'x64', 'arm64': 'arm64'}


def run(cmd, cwd=ROOT, env=None, capture=False):
    print('+', ' '.join(map(str, cmd)), flush=True)
    r = subprocess.run(cmd, cwd=cwd, env=env, check=True, text=True, capture_output=capture)
    return r.stdout.strip() if capture else None


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def provenance():
    """The commit the sources come from and whether the build used anything newer: a dirty tree is stated, not hidden."""
    head = run(['git', 'rev-parse', 'HEAD'], capture=True)
    status = run(['git', 'status', '--porcelain'], capture=True)
    diff = subprocess.run(['git', 'diff', 'HEAD'], cwd=ROOT, capture_output=True).stdout
    untracked = run(['git', 'ls-files', '--others', '--exclude-standard'], capture=True).splitlines()
    h = hashlib.sha256(diff)
    for name in sorted(untracked):
        h.update(name.encode())
        try:
            h.update((ROOT / name).read_bytes())
        except OSError:
            pass
    return {'head': head, 'dirty': bool(status), 'dirtyDiffSha256': h.hexdigest() if status else None,
            'immutable': not status}


def check_daemon(path, arch):
    """Structure and architecture of the daemon file, by Go's debug/pe (e2e/pecheck). Returns (ok, reason).

    This is the only thing checked. It does NOT show the file is the Rust daemon, where it came from, or that it runs.
    """
    r = subprocess.run(['go', 'run', './e2e/pecheck', arch, str(path)], cwd=GUI, capture_output=True, text=True)
    return r.returncode == 0, (r.stdout.strip() if r.returncode == 0 else r.stderr.strip())


TRIPLES = {'amd64': 'x86_64-pc-windows-msvc', 'arm64': 'aarch64-pc-windows-msvc'}


def verify_daemon_provenance(daemon, arch, provenance_path, source):
    """Refuse a daemon that is not the one build-sidecar recorded for this commit and target. Returns the evidence to keep."""
    rec = json.loads(Path(provenance_path).read_text())
    triple = TRIPLES[arch]
    name = f'uniclipd-{triple}.exe'
    entry = (rec.get('files') or {}).get(name)
    problems = []
    if rec.get('schema') != 1:
        problems.append(f"unknown provenance schema {rec.get('schema')!r}")
    if rec.get('target') != triple:
        problems.append(f"provenance is for target {rec.get('target')!r}, not {triple}")
    if not entry:
        problems.append(f'provenance has no entry for {name}')
    elif entry['sha256'] != sha256(daemon):
        problems.append(f"{daemon} has SHA-256 {sha256(daemon)}, build-sidecar recorded {entry['sha256']}")
    if rec.get('sourceDirty'):
        problems.append('the daemon was built from a dirty tree')
    if rec.get('sourceHead') != source['head']:
        problems.append(f"the daemon was built from {rec.get('sourceHead')}, this package is built from {source['head']}")
    if problems:
        sys.exit('the daemon is not the CI-built one:\n  ' + '\n  '.join(problems))
    return {'file': name, 'sha256': entry['sha256'], 'bytes': entry['bytes'], 'sourceHead': rec['sourceHead'], 'buildMode': rec.get('buildMode'),
            'rustc': rec.get('rustc'), 'cargoLockSha256': rec.get('cargoLockSha256'), 'run': rec.get('run')}


# The NSIS plugin of the Tauri installer (SemverCompare, RunAsUser). The URL and SHA-1 are the ones the Tauri bundler
# pins (nsis_tauri_utils v0.5.3, from its embedded NSIS setup); the download is refused when the hash differs.
TAURI_UTILS_URL = 'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll'
TAURI_UTILS_SHA1 = '75197FEE3C6A814FE035788D1C34EAD39349B860'


def fetch_tauri_utils(dest):
    import urllib.request
    dest.mkdir(parents=True, exist_ok=True)
    data = urllib.request.urlopen(TAURI_UTILS_URL, timeout=60).read()
    if hashlib.sha1(data).hexdigest().upper() != TAURI_UTILS_SHA1:
        sys.exit('nsis_tauri_utils.dll does not match the hash pinned by the Tauri bundler')
    (dest / 'nsis_tauri_utils.dll').write_bytes(data)
    return dest


def wails_version():
    mod = (GUI / 'go.mod').read_text()
    return re.search(r'github.com/wailsapp/wails/v3 (v[0-9][^\s]*)', mod).group(1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--arch', choices=sorted(ARCH_NAMES), required=True)
    parser.add_argument('--daemon', type=Path, required=True, help='uniclipd.exe built for the same architecture')
    parser.add_argument('--daemon-provenance', type=Path,
                        help='sidecar-provenance.json written by build-sidecar next to the daemon (required unless --packaging-check-fixture)')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--acceptance-version', metavar='X.Y.Z',
                        help='ACCEPTANCE ONLY: build the package with this version instead of app.json\'s, so the install/update/downgrade '
                             'acceptance has a newer package. Outputs are prefixed ACCEPTANCE- and are never uploaded or released')
    parser.add_argument('--packaging-check-fixture', action='store_true',
                        help='PACKAGING CHECK ONLY: accept a placeholder daemon so the installer script can be compiled; outputs are '
                             'marked fixture, prefixed FIXTURE- and are not a product')
    args = parser.parse_args()
    if not args.daemon.is_file():
        sys.exit(f'{args.daemon} not found: a package without the daemon cannot start')
    daemon_ok, daemon_reason = check_daemon(args.daemon, args.arch)
    if not daemon_ok and not args.packaging_check_fixture:
        sys.exit(f'{args.daemon} is not a valid {args.arch} PE executable: {daemon_reason}')
    # --packaging-check-fixture ALWAYS means fixture, even when the file is a valid PE. Without it, a valid PE is
    # accepted as an input of unknown origin: this script cannot tell the Rust daemon from any other executable.
    fixture = args.packaging_check_fixture
    if not fixture and not args.daemon_provenance:
        sys.exit('--daemon-provenance is required: a package must carry a daemon with recorded build evidence')
    if fixture and args.acceptance_version:
        sys.exit('--acceptance-version and --packaging-check-fixture are exclusive')
    prefix = 'FIXTURE-' if fixture else 'ACCEPTANCE-' if args.acceptance_version else ''
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        sys.exit(f'{out} is not empty: pick a new directory, earlier artifacts are not overwritten')
    out.mkdir(parents=True, exist_ok=True)
    conf = json.loads((ROOT / 'apps/gui-go/app.json').read_text())
    product, version, ident = conf['productName'], args.acceptance_version or conf['version'], conf['identifier']
    pubkey = conf['updater']['pubkey']
    arch = ARCH_NAMES[args.arch]
    # Tauri's default publisher is the second element of the identifier (tauri-utils config.rs `publisher`).
    manufacturer = ident.split('.')[1]
    prov = provenance()
    daemon_prov = None if fixture else verify_daemon_provenance(args.daemon, args.arch, args.daemon_provenance, prov)

    # Resources (icon, version info, manifest) with the pinned Wails CLI, not a hand-made .rc.
    tools = out / 'tools'
    tools.mkdir()
    env = dict(os.environ, GOBIN=str(tools))
    run(['go', 'install', f'github.com/wailsapp/wails/v3/cmd/wails3@{wails_version()}'], cwd=tools, env=env)
    wails3 = tools / ('wails3.exe' if os.name == 'nt' else 'wails3')
    res = out / 'resources'
    res.mkdir()
    (res / 'info.json').write_text(json.dumps({'fixed': {'file_version': version}, 'info': {'0000': {
        'ProductVersion': version, 'CompanyName': product, 'FileDescription': product,
        'LegalCopyright': product, 'ProductName': product, 'Comments': ''}}}, indent=2))
    manifest = Path(subprocess.check_output(['go', 'list', '-m', '-f', '{{.Dir}}', 'github.com/wailsapp/wails/v3'], cwd=GUI, text=True).strip())
    tmpl = (manifest / 'internal/commands/updatable_build_assets/windows/wails.exe.manifest.tmpl').read_text()
    (res / 'app.manifest').write_text(tmpl.replace('{{.ProductIdentifier}}', ident).replace('{{.ProductVersion}}', version + '.0'))
    syso = GUI / f'wails_windows_{args.arch}.syso'
    run([str(wails3), 'generate', 'syso', '-arch', args.arch, '-icon', str(ROOT / 'apps/gui-go/icons/icon.ico'),
         '-manifest', str(res / 'app.manifest'), '-info', str(res / 'info.json'), '-out', str(syso)])

    exe = out / 'UniClipboard.exe'
    try:
        run(['go', 'generate', './buildinfo'], cwd=ROOT / 'packages/desktop-host-go')
        (GUI / 'assets').mkdir(exist_ok=True)
        shutil.copy2(ROOT / 'apps/gui-go/icons/tray-icon@2x.png', GUI / 'assets/tray-icon@2x.png')
        run(['bun', '--bun', 'run', '--cwd', 'apps/gui-go', 'build'], env=dict(os.environ, VITE_GUI_GO_E2E='0'))
        ldflags = f'-w -s -H windowsgui -X main.updaterPublicKey={pubkey} -X main.productName={product} -X main.bundleID={ident}'
        run(['go', 'build', '-tags', 'production,release', '-trimpath', '-buildvcs=false', '-ldflags', ldflags, '-o', str(exe), '.'],
            cwd=GUI, env=dict(os.environ, GOOS='windows', GOARCH=args.arch, CGO_ENABLED='0'))
    finally:
        syso.unlink(missing_ok=True)

    plugins = fetch_tauri_utils(out / 'plugins')
    setup = out / f'{prefix}{product}_{version}_{arch}-setup.exe'
    run(['makensis', '-V2', f'-DPRODUCTNAME={product}', f'-DVERSION={version}', f'-DVERSIONWITHBUILD={version}.0',
         f'-DMANUFACTURER={manufacturer}', f'-DBUNDLEID={ident}', f'-DMAINBINARYNAME={product}.exe', f'-DSRC_MAIN={exe}',
         f'-DSRC_DAEMON={args.daemon.resolve()}', f'-DICON={ROOT / "apps/gui-go/icons/icon.ico"}', f'-DOUTFILE={setup}',
         f'-DHOOKS={ROOT / "apps/gui-go/windows/installer-hooks.nsh"}', f'-DPLUGINDIR={plugins}', str(GUI / 'windows/installer.nsi')])

    portable = out / f'{prefix}{product}_{version}_{arch}-portable.zip'
    with zipfile.ZipFile(portable, 'w', zipfile.ZIP_DEFLATED) as z:
        z.write(exe, f'{product}.exe')
        z.write(args.daemon, 'uniclipd.exe')
        z.writestr('portable.dat', '')  # the marker that enables portable mode; the installer does not ship it
        z.write(ROOT / 'packaging/windows/portable/README.txt', 'README.txt')

    outputs = [exe, setup, portable]
    (out / 'package-manifest.json').write_text(json.dumps({
        'source': prov, 'arch': args.arch, 'version': version, 'tags': 'production,release',
        'go': run(['go', 'version'], capture=True), 'wails': wails_version(), 'makensis': run(['makensis', '-VERSION'], capture=True),
        'purpose': 'packaging-check' if fixture else 'acceptance-newer-version' if args.acceptance_version else 'package',
        'productionUsable': False,  # never claimed here: unsigned, and no Authenticode decision exists
        'daemon': {'kind': 'fixture' if fixture else 'ci-built-rust-daemon', 'bytes': args.daemon.stat().st_size, 'sha256': sha256(args.daemon),
                   'peValid': daemon_ok, 'peCheck': daemon_reason, 'path': str(args.daemon), 'identityVerified': daemon_prov is not None,
                   'buildEvidence': daemon_prov, 'runsVerified': False,
                   'note': 'Fixture: PE structure only, not a product.' if fixture else
                           'Identity: SHA-256, source commit and target equal the build-sidecar record. Whether it runs is shown by windows_package_acceptance.py.'},
        'signed': False, 'windowsRuntimeVerified': False,
        'note': ('FIXTURE (--packaging-check-fixture): only proves the exe builds and the installer script compiles. ' if fixture else '') + 'Built and compiled only; running it is shown by windows_package_acceptance.py. Not signed. If source.dirty is true the artifacts contain uncommitted changes and are not reproducible from `head`.'}, indent=2) + '\n')
    shutil.rmtree(tools)
    print('built', *[p.name for p in outputs])


if __name__ == '__main__':
    main()
