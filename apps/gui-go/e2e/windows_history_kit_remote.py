#!/usr/bin/env python3
"""The kit History page in the REAL Windows app (native WebView2), driven from this machine.

    python3 apps/gui-go/e2e/windows_history_kit_remote.py ssh:<host> | parallels:<vm>
                                                         [--out DIR] [--work DIR] [--arch amd64|arm64] [--empty]

Two ways to reach the machine:
- `ssh:<host>`: a Windows machine over SSH. The app starts in the logged-in desktop session through a one-off
  scheduled task; its DevTools port and the daemon port come back through SSH tunnels, and the browser driver and the
  seeding run here.
- `parallels:<vm>`: a local Parallels VM through `prlctl exec --current-user` (the logged-in session), with the Mac home
  shared as Z:. The DevTools port stays on the VM's loopback, so the seeding and the driver run inside the VM with a
  portable Node (official zip, checksum-verified, kept under --work) and this checkout's scripts over the share.

What runs where, either way:
- here: the Go half for the exact HEAD (build_windows.py --mode e2e --part go: gui-go.exe with the e2e tag, uniclip.exe,
  the frontend built with UC_UI_PLATFORM=windows);
- on the machine: uniclipd.exe built with cargo from `git archive HEAD` (cargo cache under --work), and the app.

Isolation: a portable sandbox under --work (UC_PORTABLE=1: data and a file key store next to the executables, never
the user's profile or Credential Manager), a unique gui-go-* profile, UC_GUI_GO_ISOLATED=1, UC_DISABLE_SYSTEM_CLIPBOARD=1
(the daemon neither reads nor writes the Windows clipboard; Copy is checked through the daemon's restore response) and
UC_GPUI_QUICK_PANEL=0 (no quick panel helper, no global shortcut). No OS key or mouse events are sent: input goes
through CDP into the WebView only. The app window and its tray icon show on that desktop while it runs. Windows
Firewall may ask once about the daemon of a new --work path; answer it on the machine (Cancel is fine).

Afterwards it stops the app and the daemon of the sandbox (matched by executable path), removes the sandbox (and the
scheduled task over SSH), and keeps the cargo cache and the portable Node under --work. Screenshots, assertions and
logs come back to --out (default target/e2e/windows-history-kit/<machine>-<time>). Exit status 0 only when every step
passed.
"""
import argparse
import base64
import hashlib
import json
import os
import socket
import subprocess
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, PNG  # noqa: E402

ROOT = HERE.parents[2]
FRONTEND = ROOT / 'apps/gui-go/frontend'
HOME = Path.home()
PRODUCT_PATHS = ['apps', 'packages', 'crates', 'scripts', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'package.json', 'bun.lock']
SSH = ['ssh', '-o', 'BatchMode=yes', '-o', 'ServerAliveInterval=30', '-o', 'LogLevel=ERROR']
NODE = ('https://nodejs.org/dist/v22.19.0/node-v22.19.0-win-arm64.zip',
        'e4a7336010d58ff35b53d9dd5869095c56089c70913cf22508cf8183593e56b2')
NODE_X64 = ('https://nodejs.org/dist/v22.19.0/node-v22.19.0-win-x64.zip',
            'ea3fad0e67a991d8477d8c01344b56e69c676ccb733f065b22436994b1253f86')


def encoded(script):
    return base64.b64encode(("$ProgressPreference='SilentlyContinue'\n" + script).encode('utf-16-le')).decode()


class Machine:
    """Runs PowerShell on the Windows machine and moves files there."""

    def __init__(self, spec):
        self.kind, _, self.name = spec.partition(':')
        if self.kind not in ('ssh', 'parallels') or not self.name:
            sys.exit('machine must be ssh:<host> or parallels:<vm>')

    def ps(self, script, check=True, timeout=1800):
        command = f'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {encoded(script)}'
        argv = SSH + [self.name, command] if self.kind == 'ssh' else ['prlctl', 'exec', self.name, '--current-user', *command.split(' ')]
        result = subprocess.run(argv, capture_output=True, timeout=timeout)
        text = (result.stdout + result.stderr).decode('utf-8', errors='replace').strip()
        if check and result.returncode != 0:
            raise RuntimeError(f'remote PowerShell failed ({result.returncode}):\n{text}')
        return text

    def share(self, local):
        """The Windows path of a local file: the Parallels share of the Mac home (Z:)."""
        relative = Path(local).resolve().relative_to(HOME)
        return 'Z:\\' + str(relative).replace('/', '\\')

    def put(self, local, remote):
        if self.kind == 'ssh':
            subprocess.run(['scp', '-q', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR', str(local),
                            f'{self.name}:{remote.replace(chr(92), "/")}'], check=True)
        else:
            self.ps(f"Copy-Item -Force '{self.share(local)}' '{remote}'")

    def fetch_dir(self, remote, local):
        if self.kind == 'ssh':
            subprocess.run(['scp', '-q', '-r', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR',
                            f'{self.name}:{remote.replace(chr(92), "/")}', str(local)], check=False)
        else:
            local.mkdir(parents=True, exist_ok=True)
            self.ps(f"Copy-Item -Recurse -Force '{remote}\\*' '{self.share(local)}'", check=False)


def _json_object(text):
    """The JSON object a CLI printed, ignoring anything around it."""
    return json.loads(text[text.index('{'):text.rindex('}') + 1])


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def tunnel(host, remote_port):
    local = free_port()
    proc = subprocess.Popen(SSH + ['-N', '-o', 'ExitOnForwardFailure=yes', '-L', f'{local}:127.0.0.1:{remote_port}', host])
    return local, proc


def wait_http(url, timeout=120):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=3):
                return True
        except Exception:  # noqa: BLE001 - not up yet
            time.sleep(1)
    raise RuntimeError(f'timed out waiting for {url}')


def sandbox_env(profile, extra=''):
    return (f"$env:UC_PORTABLE='1'; $env:UC_PROFILE='{profile}'; $env:UNICLIPBOARD_ENV='development'; "
            f"$env:UC_DISABLE_SYSTEM_CLIPBOARD='1'; $env:UC_GUI_GO_ISOLATED='1'; $env:UC_GPUI_QUICK_PANEL='0'; {extra}")


def build_go_half(arch):
    subprocess.run([sys.executable, str(HERE / 'build_windows.py'), '--mode', 'e2e', '--part', 'go', '--arch', arch], check=True)
    out = ROOT / f'target/gui-go/windows-e2e-{arch}'
    manifest = json.loads((out / 'manifest.json').read_text())
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    # Only the product sources must be clean; tool state elsewhere in the checkout (editor or agent skills) is
    # recorded, not built.
    product = subprocess.check_output(['git', 'status', '--porcelain', '--', *PRODUCT_PATHS], cwd=ROOT, text=True).strip()
    if manifest['commit'] != head or product:
        sys.exit(f'the Go half is not from a clean HEAD {head}: {product or manifest}')
    manifest['unrelatedChanges'] = subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).splitlines()
    return out, manifest


def build_daemon(machine, work, head, arch, out):
    """uniclipd.exe for HEAD, built on the machine from `git archive HEAD`, with the pinned toolchain."""
    tar = out / f'source-{head[:12]}.tar'
    subprocess.run(['git', 'archive', '--format=tar', '-o', str(tar), head], cwd=ROOT, check=True)
    src = f'{work}\\src-{head[:12]}'
    triple = 'aarch64-pc-windows-msvc' if arch == 'arm64' else 'x86_64-pc-windows-msvc'
    toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
    machine.ps(f"New-Item -ItemType Directory -Force '{work}' | Out-Null")
    machine.put(tar, f'{work}\\source-{head[:12]}.tar')
    # ring builds C with clang on Windows ARM64; the Build Tools' own LLVM goes on PATH for this process only.
    llvm = "C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Tools\\Llvm\\ARM64\\bin"
    log = machine.ps(f"""
if (-not (Test-Path '{src}\\Cargo.toml')) {{
  New-Item -ItemType Directory -Force '{src}' | Out-Null
  # Windows tar cannot create the repository's symbolic links (e.g. CLAUDE.md -> AGENTS.md) and says so on
  # stderr; the build does not need them. The message is kept as TAR= rather than mistaken for the result.
  $tarOut = (tar -xf '{work}\\source-{head[:12]}.tar' -C '{src}' 2>&1 | Out-String).Trim()
  if ($tarOut) {{ 'TAR=' + ($tarOut -replace '\\s+', ' ') }}
}}
$env:CARGO_TARGET_DIR='{work}\\target'
if (Test-Path '{llvm}') {{ $env:PATH = '{llvm};' + $env:PATH }}
Set-Location '{src}'
cmd /c "cargo +{toolchain}-{triple} build --locked -j 3 -p uc-daemon --bin uniclipd > {work}\\cargo-{head[:12]}.log 2>&1"
if ($LASTEXITCODE -ne 0) {{ Get-Content '{work}\\cargo-{head[:12]}.log' -Tail 30; exit 1 }}
'SHA256=' + (Get-FileHash '{work}\\target\\debug\\uniclipd.exe' -Algorithm SHA256).Hash
""", timeout=14400)
    sha = next(line.split('=', 1)[1].strip().lower() for line in log.splitlines() if line.startswith('SHA256='))
    tar_notes = [line[4:] for line in log.splitlines() if line.startswith('TAR=')]
    return f'{work}\\target\\debug\\uniclipd.exe', sha, tar_notes


def portable_node(machine, work, arch, out):
    """A portable Node and this machine's playwright-core under work (the driver runs inside a Parallels VM)."""
    url, digest = NODE if arch == 'arm64' else NODE_X64
    zip_path = out / Path(url).name
    if not zip_path.exists():
        urllib.request.urlretrieve(url, zip_path)
    if hashlib.sha256(zip_path.read_bytes()).hexdigest() != digest:
        sys.exit(f'{zip_path.name}: checksum mismatch')
    playwright = Path(os.environ['PLAYWRIGHT_MODULE']).parent
    node_dir = f'{work}\\node'
    machine.ps(f"""
if (-not (Test-Path '{node_dir}\\node.exe')) {{
  Expand-Archive -Force '{machine.share(zip_path)}' '{work}\\node-unpack'
  Move-Item (Get-ChildItem '{work}\\node-unpack' | Select -First 1).FullName '{node_dir}'
  Remove-Item -Recurse -Force '{work}\\node-unpack'
}}
if (-not (Test-Path '{work}\\driver\\node_modules\\playwright-core\\index.mjs')) {{
  robocopy '{machine.share(playwright)}' '{work}\\driver\\node_modules\\playwright-core' /E /NFL /NDL /NJH /NJS /NP | Out-Null
}}
exit 0
""", timeout=1200)
    return f'{node_dir}\\node.exe', f'{work}\\driver\\node_modules\\playwright-core\\index.mjs'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('machine', help='ssh:<host> or parallels:<vm>')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--work', default='C:\\uc-history-kit')
    parser.add_argument('--arch', default='amd64', choices=['amd64', 'arm64'])
    parser.add_argument('--extra', type=int, default=130)
    parser.add_argument('--empty', action='store_true', help='first-launch run: a space without history')
    args = parser.parse_args()
    machine = Machine(args.machine)
    stamp = time.strftime('%Y%m%d-%H%M%S')
    out = (args.out or ROOT / f'target/e2e/windows-history-kit/{machine.name.replace(" ", "-")}-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=True)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    record = {'machine': args.machine, 'arch': args.arch, 'head': head, 'stamp': stamp, 'empty': args.empty, 'passed': False}

    go_out, manifest = build_go_half(args.arch)
    record['goHalf'] = manifest['sha256']
    record['unrelatedChanges'] = manifest['unrelatedChanges']
    daemon_exe, daemon_sha, tar_notes = build_daemon(machine, args.work, head, args.arch, out)
    record['uniclipdSha256'] = daemon_sha
    record['sourceExtractNotes'] = tar_notes
    record['os'] = machine.ps("Get-CimInstance Win32_OperatingSystem | Select Caption,Version,OSArchitecture | ConvertTo-Json -Compress")
    record['cpuLoadBefore'] = machine.ps("(Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average")
    inside = machine.kind == 'parallels'
    node = playwright = None
    if inside:
        node, playwright = portable_node(machine, args.work, args.arch, out)

    # Isolated mode requires the executables in a uc-gui-go-* directory (environment.go).
    run_dir = f'{args.work}\\uc-gui-go-{stamp}'
    app = run_dir
    profile = f'gui-go-kit-{stamp}'
    task = f'uc-history-kit-{stamp}'
    debug_port = 9300 + int(time.time()) % 500
    tunnels = []
    seed = FRONTEND / 'e2e/history-kit-seed.mjs'
    driver = FRONTEND / 'e2e/history-kit-browser.mjs'
    try:
        machine.ps(f"New-Item -ItemType Directory -Force '{app}' | Out-Null; Copy-Item '{daemon_exe}' '{app}\\uniclipd.exe'")
        for name in ('gui-go.exe', 'uniclip.exe'):
            machine.put(go_out / name, f'{app}\\{name}')
        # On Windows ARM64 a daemon started while the one `space init` left is still running fails with
        # profile_storage_upgrade 'corrupt' (Engine, recover_active_target), so init's daemon is stopped first; any
        # failed attempt is still recorded rather than hidden.
        record['daemonStartAttempts'] = machine.ps(sandbox_env(profile) + f"""
Set-Location '{app}'
& .\\uniclip.exe space init --passphrase '{PASSPHRASE}' --device-name kit-windows 2>&1 | Out-Null
# `space init` leaves its daemon running; starting another one while it holds the profile storage is reported as
# 'corrupt'. Stop it first.
& .\\uniclip.exe stop 2>&1 | Out-Null
Start-Sleep 2
foreach ($i in 1..3) {{
  $out = ((& .\\uniclip.exe start 2>&1 | Out-String) -replace '\\s+', ' ')
  $reason = if ($out -match 'reason=(\\w+)') {{ $Matches[1] }} else {{ '' }}
  'attempt ' + $i + ' exit=' + $LASTEXITCODE + ' reason=' + $reason
  if (Get-ChildItem '{app}\\data' -Recurse -Filter daemon.conn -ErrorAction SilentlyContinue) {{ break }}
  Start-Sleep 5
}}
exit 0
""", timeout=900).splitlines()
        conn_remote = machine.ps(f"(Get-ChildItem '{app}\\data' -Recurse -Filter daemon.conn | Select -First 1).FullName")
        conn = json.loads(machine.ps(f"Get-Content '{conn_remote}' -Raw"))
        record['daemonPort'] = conn['port']
        seeded = 0 if args.empty else args.extra + 3 + 5

        def node_seed(*seed_args):
            if inside:
                quoted = ' '.join(f"'{a}'" for a in seed_args)
                machine.ps(f"& '{node}' '{machine.share(seed)}' '{machine.share(out)}' 'http://127.0.0.1:{conn['port']}' '{conn_remote}' {quoted}; exit $LASTEXITCODE", timeout=1200)
            else:
                (out / 'remote-daemon.conn').write_text(json.dumps(conn))
                subprocess.run(['node', str(seed), str(out), base, str(out / 'remote-daemon.conn'), *seed_args], check=True)

        base = None
        if not inside:
            daemon_port, t = tunnel(machine.name, conn['port'])
            tunnels.append(t)
            base = f'http://127.0.0.1:{daemon_port}'
            wait_http(f'{base}/health')
        if not args.empty:
            node_seed(PASSPHRASE, 'paging', str(args.extra))
            png = base64.b64encode(PNG).decode()
            machine.ps(sandbox_env(profile) + f"""
Set-Location '{app}'
New-Item -ItemType Directory -Force seed | Out-Null
[IO.File]::WriteAllBytes("$PWD\\seed\\pixel.png", [Convert]::FromBase64String('{png}'))
Set-Content -Path seed\\invoice-0921.txt -Value "Invoice 0921`nTotal: 42.00"
& .\\uniclip.exe send --file "$PWD\\seed\\invoice-0921.txt" | Out-Null
& .\\uniclip.exe send --file "$PWD\\seed\\pixel.png" | Out-Null
exit 0
""")
            # One image entry through the loopback mobile LAN listener (SyncClipboard upload), no paired peer.
            mobile_port = 9800 + int(time.time()) % 150
            setup = _json_object(machine.ps(sandbox_env(profile) + f"""
Set-Location '{app}'
& .\\uniclip.exe --json mobile setup --non-interactive --label kit-image --ip 127.0.0.1 --accept-network-risk --port {mobile_port} | Out-String
"""))
            if not inside:
                mobile_local, t = tunnel(machine.name, mobile_port)
                tunnels.append(t)
            mobile_url = f'http://127.0.0.1:{mobile_port if inside else mobile_local}'
            node_seed(PASSPHRASE, 'image', mobile_url, setup['username'], setup['password'])
        node_seed(PASSPHRASE, 'texts', profile, app, str(seeded))

        launcher = sandbox_env(profile, f"$env:UC_GUI_GO_E2E_BROWSER_ARGS='--remote-debugging-port={debug_port}'; "
                                        f"$env:UC_GUI_GO_EXIT_MODE='full'; $env:UC_GUI_GO_E2E_VISIBLE='1'; "
                                        f"$env:UC_GUI_GO_EVIDENCE='{run_dir}\\native.jsonl'")
        if inside:
            # Already the logged-in session: start the app detached with the sandbox env. A hidden PowerShell does
            # the output redirection: redirecting in Start-Process itself hands prlctl's pipes to the app, and
            # `prlctl exec` then never returns.
            machine.ps(launcher + f"""
Start-Process -FilePath powershell.exe -WindowStyle Hidden -WorkingDirectory '{app}' -ArgumentList '-NoProfile','-Command',"& '{app}\\gui-go.exe' > '{run_dir}\\gui.out' 2> '{run_dir}\\gui.err'"
""", timeout=120)
        else:
            machine.ps(f"""
Set-Content -Path '{run_dir}\\launch.ps1' -Value @'
{launcher}; Set-Location '{app}'; & '{app}\\gui-go.exe'
'@
schtasks /Create /TN '{task}' /TR "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File {run_dir}\\launch.ps1" /SC ONCE /ST 23:59 /IT /F | Out-Null
schtasks /Run /TN '{task}' | Out-Null
""")
        driver_env = {'HISTORY_KIT_HEAD': head, 'HISTORY_KIT_DIRTY': '0', 'UC_UI_PLATFORM': 'windows'}
        if inside:
            machine.ps(f"""
$deadline = (Get-Date).AddMinutes(3)
while ((Get-Date) -lt $deadline) {{ try {{ Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:{debug_port}/json/version' | Out-Null; exit 0 }} catch {{ Start-Sleep 1 }} }}
exit 1
""", timeout=240, check=False)
            if not machine.ps(f"try {{ Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:{debug_port}/json/version' | Out-Null; 'up' }} catch {{ 'down' }}", check=False).endswith('up'):
                record['launchDiagnosis'] = machine.ps(f"""
'running=' + @(Get-Process gui-go -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\\gui-go.exe' }}).Count
'--- gui.err'; Get-Content '{run_dir}\\gui.err' -Tail 40 -ErrorAction SilentlyContinue
'--- gui.out'; Get-Content '{run_dir}\\gui.out' -Tail 40 -ErrorAction SilentlyContinue
'--- native.jsonl'; Get-Content '{run_dir}\\native.jsonl' -Tail 20 -ErrorAction SilentlyContinue
""", check=False)
                raise RuntimeError('the app opened no DevTools endpoint; see launchDiagnosis in native-run.json')
            record['webview'] = machine.ps(f"(Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:{debug_port}/json/version').Content")
            envs = '; '.join(f"$env:{k}='{v}'" for k, v in {**driver_env, # Node's import() on Windows takes a file:// URL, not a drive path.
                                                             'PLAYWRIGHT_MODULE': 'file:///' + playwright.replace('\\', '/'),
                                                             'HISTORY_KIT_CDP': f'http://127.0.0.1:{debug_port}'}.items())
            # The checkout is reached over the share; the conn path in daemon.json is the share path too.
            result = machine.ps(f"{envs}; & '{node}' '{machine.share(driver)}' '{machine.share(out)}' *>&1 | Out-String; exit $LASTEXITCODE",
                                check=False, timeout=3600)
            (out / 'driver-output.txt').write_text(result)
            # The driver's own verdict, which needs at least one step; its printed summary is only a log.
            verdict = json.loads((out / 'browser-results.json').read_text(encoding='utf-8-sig')) if (out / 'browser-results.json').exists() else {}
            record['passed'] = bool(verdict.get('passed')) and len(verdict.get('steps', [])) > 0
        else:
            cdp_port, t = tunnel(machine.name, debug_port)
            tunnels.append(t)
            wait_http(f'http://127.0.0.1:{cdp_port}/json/version', timeout=180)
            record['webview'] = urllib.request.urlopen(f'http://127.0.0.1:{cdp_port}/json/version').read().decode()
            env = dict(os.environ, HISTORY_KIT_CDP=f'http://127.0.0.1:{cdp_port}', **driver_env)
            record['passed'] = subprocess.run(['node', str(driver), str(out)], cwd=FRONTEND, env=env).returncode == 0
        record['cpuLoadDuring'] = machine.ps("(Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average", check=False)
    finally:
        machine.ps(sandbox_env(profile) + f"""
Get-Process gui-go -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\\gui-go.exe' }} | Stop-Process -Force
if (Test-Path '{app}\\uniclip.exe') {{ Set-Location '{app}'; & .\\uniclip.exe stop 2>&1 | Out-Null }}
Start-Sleep 2
Get-Process uniclipd -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\\uniclipd.exe' }} | Stop-Process -Force
if ('{machine.kind}' -eq 'ssh') {{ schtasks /Delete /TN '{task}' /F 2>&1 | Out-Null }}
exit 0
""", check=False)
        logs = machine.ps(f"(Get-ChildItem '{app}\\data' -Recurse -Directory -Filter logs -ErrorAction SilentlyContinue | Select -First 1).FullName", check=False)
        if logs:
            machine.fetch_dir(logs, out / 'remote-logs')
        for t in tunnels:
            t.terminate()
        left = machine.ps(f"Set-Location C:\\; Remove-Item -Recurse -Force '{run_dir}' -ErrorAction SilentlyContinue; Test-Path '{run_dir}'", check=False)
        record['sandboxRemoved'] = left.strip().endswith('False')
        (out / 'native-run.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))
    sys.exit(0 if record['passed'] else 1)


if __name__ == '__main__':
    main()
