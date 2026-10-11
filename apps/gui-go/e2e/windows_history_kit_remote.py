#!/usr/bin/env python3
"""The kit History page in the REAL Windows app (native WebView2), driven from this machine over SSH.

    python3 apps/gui-go/e2e/windows_history_kit_remote.py <ssh-host> [--out DIR] [--work DIR] [--arch amd64|arm64]

What runs where:
- here: the Go half for the exact HEAD (build_windows.py --mode e2e --part go: gui-go.exe with the e2e tag, uniclip.exe,
  the frontend built with UC_UI_PLATFORM=windows), the seeding over HTTP, and the browser driver
  (apps/gui-go/frontend/e2e/history-kit-browser.mjs) attached to the app's WebView2 over the DevTools protocol;
- on the machine: uniclipd.exe built with cargo from `git archive HEAD`, and the app itself in the logged-in desktop
  session (a one-off scheduled task), with --remote-debugging-port bound to 127.0.0.1 and reached through an SSH
  tunnel, like the daemon port.

Isolation: a portable sandbox under --work (UC_PORTABLE=1: data and a file key store next to the executables, never
the user's profile or Credential Manager), a unique gui-go-* profile, UC_GUI_GO_ISOLATED=1, UC_DISABLE_SYSTEM_CLIPBOARD=1
(the daemon neither reads nor writes the Windows clipboard; Copy is checked through the daemon's restore response) and
UC_GPUI_QUICK_PANEL=0 (no quick panel helper, no global shortcut). The run sends no OS key or mouse events: input goes
through CDP into the WebView only. It does show the app window and its tray icon on that desktop while it runs.
Windows Firewall may ask once about the daemon of a new --work path; answer it on the machine (Cancel is fine).

Afterwards it stops the app and the daemon of the sandbox (matched by executable path), deletes the scheduled task and
the sandbox, and keeps the cargo cache under --work for the next run. Screenshots, assertions and logs come back to
--out (default target/e2e/windows-history-kit/<host>-<time>). Exit status 0 only when every step passed.
"""
import argparse
import base64
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, PNG  # noqa: E402
from history_kit_browser_daemon import TEXTS  # noqa: E402

ROOT = HERE.parents[2]
FRONTEND = ROOT / 'apps/gui-go/frontend'
SSH = ['ssh', '-o', 'BatchMode=yes', '-o', 'ServerAliveInterval=30', '-o', 'LogLevel=ERROR']


def ps(host, script, check=True, timeout=1800):
    """Runs a PowerShell script on the host (UTF-16LE base64, so no quoting through cmd and SSH)."""
    encoded = base64.b64encode(("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'\n" + script)
                               .encode('utf-16-le')).decode()
    result = subprocess.run(SSH + [host, f'powershell -NoProfile -NonInteractive -EncodedCommand {encoded}'],
                            capture_output=True, timeout=timeout)
    text = (result.stdout + result.stderr).decode('utf-8', errors='replace').strip()
    if check and result.returncode != 0:
        raise RuntimeError(f'remote PowerShell failed ({result.returncode}):\n{text}')
    return text


def scp(host, source, destination):
    subprocess.run(['scp', '-q', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR', str(source),
                    f'{host}:{destination.replace(chr(92), "/")}'], check=True)


def scp_back(host, source, destination):
    subprocess.run(['scp', '-q', '-r', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR',
                    f'{host}:{source.replace(chr(92), "/")}', str(destination)], check=False)


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


def post(base, token, path, body, auth='Session'):
    request = urllib.request.Request(f'{base}{path}', method='POST', data=json.dumps(body).encode(),
                                     headers={'Authorization': f'{auth} {token}', 'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def sandbox_env(profile, extra=''):
    return (f"$env:UC_PORTABLE='1'; $env:UC_PROFILE='{profile}'; $env:UNICLIPBOARD_ENV='development'; "
            f"$env:UC_DISABLE_SYSTEM_CLIPBOARD='1'; $env:UC_GUI_GO_ISOLATED='1'; $env:UC_GPUI_QUICK_PANEL='0'; {extra}")


def build_go_half(arch):
    subprocess.run([sys.executable, str(HERE / 'build_windows.py'), '--mode', 'e2e', '--part', 'go', '--arch', arch], check=True)
    out = ROOT / f'target/gui-go/windows-e2e-{arch}'
    manifest = json.loads((out / 'manifest.json').read_text())
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if manifest['commit'] != head or manifest['dirty']:
        sys.exit(f'the Go half is not from a clean HEAD {head}: {manifest}')
    return out, manifest


def build_daemon(host, work, head, out):
    """uniclipd.exe for HEAD, built on the machine from `git archive HEAD` (cargo cache kept under work)."""
    tar = out / f'source-{head[:12]}.tar'
    subprocess.run(['git', 'archive', '--format=tar', '-o', str(tar), head], cwd=ROOT, check=True)
    src = f'{work}\\src-{head[:12]}'
    ps(host, f"New-Item -ItemType Directory -Force '{work}' | Out-Null")
    scp(host, tar, f'{work}\\source-{head[:12]}.tar')
    log = ps(host, f"""
if (-not (Test-Path '{src}\\Cargo.toml')) {{
  New-Item -ItemType Directory -Force '{src}' | Out-Null
  tar -xf '{work}\\source-{head[:12]}.tar' -C '{src}'
}}
$env:CARGO_TARGET_DIR='{work}\\target'
Set-Location '{src}'
cargo build --locked -p uc-daemon --bin uniclipd 2>&1 | Select-Object -Last 3 | Out-String
(Get-FileHash '{work}\\target\\debug\\uniclipd.exe' -Algorithm SHA256).Hash
""", timeout=7200)
    return f'{work}\\target\\debug\\uniclipd.exe', log.splitlines()[-1].strip().lower()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('host')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--work', default='C:\\uc-history-kit')
    parser.add_argument('--arch', default='amd64', choices=['amd64', 'arm64'])
    parser.add_argument('--extra', type=int, default=130)
    parser.add_argument('--empty', action='store_true', help='first-launch run: a space without history')
    args = parser.parse_args()
    stamp = time.strftime('%Y%m%d-%H%M%S')
    out = (args.out or ROOT / f'target/e2e/windows-history-kit/{args.host}-{stamp}').resolve()
    out.mkdir(parents=True, exist_ok=True)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    record = {'host': args.host, 'head': head, 'stamp': stamp, 'passed': False}

    go_out, manifest = build_go_half(args.arch)
    record['goHalf'] = manifest['sha256']
    daemon_exe, daemon_sha = build_daemon(args.host, args.work, head, out)
    record['uniclipdSha256'] = daemon_sha
    record['machine'] = ps(args.host, "(Get-CimInstance Win32_OperatingSystem | Select Caption,Version,OSArchitecture | ConvertTo-Json -Compress)")

    run_dir = f'{args.work}\\run-{stamp}'
    app = f'{run_dir}\\app'
    profile = f'gui-go-kit-{stamp}'
    task = f'uc-history-kit-{stamp}'
    debug_port = 9300 + int(time.time()) % 500
    tunnels = []
    try:
        ps(args.host, f"New-Item -ItemType Directory -Force '{app}' | Out-Null; Copy-Item '{daemon_exe}' '{app}\\uniclipd.exe'")
        for name in ('gui-go.exe', 'uniclip.exe'):
            scp(args.host, go_out / name, f'{app}\\{name}')
        # Space and daemon of the sandbox, through its own CLI.
        ps(args.host, sandbox_env(profile) + f"""
Set-Location '{app}'
& .\\uniclip.exe space init --passphrase '{PASSPHRASE}' --device-name kit-windows | Out-Null
& .\\uniclip.exe start | Out-Null
""", timeout=600)
        conn = json.loads(ps(args.host, f"Get-Content (Get-ChildItem '{app}\\data' -Recurse -Filter daemon.conn | Select -First 1).FullName -Raw"))
        daemon_port, t = tunnel(args.host, conn['port'])
        tunnels.append(t)
        base = f'http://127.0.0.1:{daemon_port}'
        wait_http(f'{base}/health')
        (out / 'daemon.conn').write_text(json.dumps({'token': conn['token']}))
        token = post(base, conn['token'], '/auth/connect', {'pid': os.getpid(), 'clientType': 'gui'}, auth='Bearer')['data']['sessionToken']
        post(base, token, '/content-lock/unlock', {'passphrase': PASSPHRASE})
        seeded = 0
        if not args.empty:
            for index in range(args.extra):
                post(base, token, '/clipboard/dispatch', {'text': f'Paging entry {index:03d}'})
            png = base64.b64encode(PNG).decode()
            ps(args.host, sandbox_env(profile) + f"""
Set-Location '{app}'
New-Item -ItemType Directory -Force seed | Out-Null
[IO.File]::WriteAllBytes("$PWD\\seed\\pixel.png", [Convert]::FromBase64String('{png}'))
Set-Content -Path seed\\invoice-0921.txt -Value "Invoice 0921`nTotal: 42.00"
& .\\uniclip.exe send --file "$PWD\\seed\\invoice-0921.txt" | Out-Null
& .\\uniclip.exe send --file "$PWD\\seed\\pixel.png" | Out-Null
exit 0
""")
            for text in TEXTS:
                post(base, token, '/clipboard/dispatch', {'text': text})
                time.sleep(.2)
            seeded = args.extra + 2 + len(TEXTS)
        (out / 'daemon.json').write_text(json.dumps({'home': app, 'profile': profile, 'daemonUrl': base,
                                                     'conn': str(out / 'daemon.conn'), 'passphrase': PASSPHRASE,
                                                     'seeded': seeded}, indent=2) + '\n')
        # The app in the logged-in desktop session: a one-off scheduled task runs a launcher with the sandbox env.
        launcher = sandbox_env(profile, f"$env:UC_GUI_GO_E2E_BROWSER_ARGS='--remote-debugging-port={debug_port}'; "
                                        f"$env:UC_GUI_GO_EXIT_MODE='full'; Set-Location '{app}'; & '{app}\\gui-go.exe'")
        ps(args.host, f"""
Set-Content -Path '{run_dir}\\launch.ps1' -Value @'
{launcher}
'@
schtasks /Create /TN '{task}' /TR "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File {run_dir}\\launch.ps1" /SC ONCE /ST 23:59 /IT /F | Out-Null
schtasks /Run /TN '{task}' | Out-Null
""")
        cdp_port, t = tunnel(args.host, debug_port)
        tunnels.append(t)
        wait_http(f'http://127.0.0.1:{cdp_port}/json/version', timeout=180)
        record['webview'] = json.loads(urllib.request.urlopen(f'http://127.0.0.1:{cdp_port}/json/version').read())
        env = dict(os.environ, HISTORY_KIT_CDP=f'http://127.0.0.1:{cdp_port}', UC_UI_PLATFORM='windows')
        driver = subprocess.run(['node', 'e2e/history-kit-browser.mjs', str(out)], cwd=FRONTEND, env=env)
        record['driverExit'] = driver.returncode
        record['passed'] = driver.returncode == 0
    finally:
        ps(args.host, sandbox_env(profile) + f"""
Get-Process gui-go -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\\gui-go.exe' }} | Stop-Process -Force
if (Test-Path '{app}\\uniclip.exe') {{ Set-Location '{app}'; & .\\uniclip.exe stop 2>&1 | Out-Null }}
Get-Process uniclipd -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\\uniclipd.exe' }} | Stop-Process -Force
schtasks /Delete /TN '{task}' /F 2>&1 | Out-Null
exit 0
""", check=False)
        logs = ps(args.host, f"(Get-ChildItem '{app}\\data' -Recurse -Directory -Filter logs -ErrorAction SilentlyContinue | Select -First 1).FullName", check=False)
        if logs:
            scp_back(args.host, logs, out / 'remote-logs')
        for t in tunnels:
            t.terminate()
        time.sleep(2)
        ps(args.host, f"Remove-Item -Recurse -Force '{run_dir}' -ErrorAction SilentlyContinue; exit 0", check=False)
        (out / 'native-run.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record, indent=2))
    sys.exit(0 if record['passed'] else 1)


if __name__ == '__main__':
    main()
