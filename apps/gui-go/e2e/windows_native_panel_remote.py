#!/usr/bin/env python3
"""One command: the native quick panel E2E on a real Windows machine reachable over SSH.

    python3 apps/gui-go/e2e/windows_native_panel_remote.py <ssh-host> [--out DIR] [--work DIR] [--clobber-clipboard]

The check is a REAL desktop run: the Go host starts the GPUI helper, the helper talks to an isolated daemon, real key
events open the panel, search, select and paste into an isolated editor, and the exit leaves nothing behind
(apps/gui-go/e2e/windows_native_panel_run.py has the list of checks). The machine must have a logged-in desktop session,
Rust (rustup, the repository's pinned toolchain) and tar; Go, bun and Python are not needed on it. This machine builds the
Go half for the architecture the machine reports and sends the sources, the Go binaries and an embeddable Python over.

What it does to the machine: everything stays under --work (default C:\\uc-e2e): sources, a Python, build output, the
sandbox. It starts a scheduled task for the logged-in user to run in the desktop session and deletes it afterwards.
The run takes over the keyboard and the foreground of that desktop for a few minutes: use a machine nobody works on.
The clipboard is saved as text and given back (the run refuses to start if the clipboard holds other formats, unless
--clobber-clipboard is given).

Windows Firewall may raise its "allow access" prompt the first time the daemon of a new --work path starts. The prompt
blocks the daemon until it is answered with the mouse (Cancel is fine), which makes the run fail on the daemon check;
answer it on the machine and run again. The answer is a system rule for that program path, so it is asked once.

Exit status 0 only when every check passed. The assertions, screenshots and logs come back to --out
(default target/e2e/windows-native-panel/<host>-<time>).
"""
import argparse
import hashlib
import os
import subprocess
import sys
import tarfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
E2E = ROOT / 'apps/gui-go/e2e'
PYTHON = {  # python.org embeddable distributions
    'amd64': ('https://www.python.org/ftp/python/3.12.8/python-3.12.8-embed-amd64.zip',
              '8d3f33be9eb810f23c102f08475af2854e50484b8e4e06275e937be61ce3d2fb'),
    'arm64': ('https://www.python.org/ftp/python/3.12.8/python-3.12.8-embed-arm64.zip',
              'd34db37675973785a2a539cd1c8dde1b6d45665f48c615ef55274b3798bf9fd3'),
}
TASK = 'uniclip-native-panel-e2e'
SSH = ['ssh', '-o', 'BatchMode=yes', '-o', 'ServerAliveInterval=30', '-o', 'LogLevel=ERROR']


def ssh(host, command, check=True, timeout=600):
    result = subprocess.run(SSH + [host, command], capture_output=True, timeout=timeout)
    text = (result.stdout + result.stderr).decode('utf-8', errors='replace')
    if check and result.returncode != 0:
        sys.exit(f'ssh {host} {command!r} failed ({result.returncode}):\n{text}')
    return text.strip()


def scp(host, source, destination, to_remote=True):
    pair = [str(source), f'{host}:{destination}'] if to_remote else [f'{host}:{source}', str(destination)]
    subprocess.run(['scp', '-q', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR', *(['-r'] if not to_remote else []), *pair], check=True)


def remote_arch(host):
    value = ssh(host, 'echo %PROCESSOR_ARCHITECTURE%').upper()
    if value == 'AMD64':
        return 'amd64'
    if value == 'ARM64':
        return 'arm64'
    sys.exit(f'unsupported processor architecture {value!r} on {host}')


def python_zip(arch):
    url, digest = PYTHON[arch]
    cache = ROOT / 'target/e2e-cache'
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / Path(url).name
    if not path.exists():
        urllib.request.urlretrieve(url, path)
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        path.unlink()
        sys.exit(f'{url} does not match its pinned SHA-256')
    return path


def source_archive(path):
    """The working tree as the build sees it: tracked and not-ignored files, deleted ones left out."""
    names = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT).decode().split('\0')
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, 'w') as tar:
        for name in names:
            if name and (ROOT / name).is_file():
                tar.add(ROOT / name, arcname=name, recursive=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('host', help='ssh host name of the Windows machine')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--work', default='C:/uc-e2e', help='working directory on the machine')
    parser.add_argument('--clobber-clipboard', action='store_true')
    parser.add_argument('--timeout', type=int, default=3600, help='seconds to wait for the run')
    args = parser.parse_args()
    work = args.work.replace('\\', '/').rstrip('/')
    work_win = work.replace('/', '\\')
    out = args.out or ROOT / 'target/e2e/windows-native-panel' / f'{args.host}-{time.strftime("%Y%m%d-%H%M%S")}'
    out.mkdir(parents=True, exist_ok=True)

    arch = remote_arch(args.host)
    print(f'{args.host}: Windows {arch}', flush=True)
    if 'console' not in ssh(args.host, 'query user', check=False).lower():
        sys.exit(f'{args.host} has no logged-in desktop session: a real desktop run needs one')

    print('building the Go half here', flush=True)
    subprocess.run([sys.executable, str(E2E / 'build_windows.py'), '--mode', 'e2e', '--part', 'go', '--arch', arch], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    go_out = ROOT / 'target/gui-go' / f'windows-e2e-{arch}'

    print('sending sources, binaries and Python', flush=True)
    archive = ROOT / 'target/e2e-cache/source.tar'
    source_archive(archive)
    ssh(args.host, f'cmd /c "mkdir {work_win}\\go {work_win}\\src {work_win}\\run 2>nul & del /q {work_win}\\run\\status.txt {work_win}\\run\\*.log 2>nul & rmdir /s /q {work_win}\\run\\out 2>nul & exit 0"')
    scp(args.host, archive, f'{work}/source.tar')
    ssh(args.host, f'tar -xf {work_win}\\source.tar -C {work_win}\\src')
    for name in ('gui-go.exe', 'uniclip.exe'):
        scp(args.host, go_out / name, f'{work}/go/{name}')
    if 'python.exe' not in ssh(args.host, f'dir /b {work_win}\\python', check=False):
        scp(args.host, python_zip(arch), f'{work}/python.zip')
        ssh(args.host, f'powershell -NoProfile -Command "Expand-Archive -Force {work_win}\\python.zip {work_win}\\python"')
    # The embeddable Python ignores the script directory unless it is listed here (the scripts import each other).
    ssh(args.host, f'powershell -NoProfile -Command "Set-Content -Encoding ascii {work_win}\\python\\python312._pth '
                   f'\'python312.zip\',\'.\',\'{work_win}\\src\\apps\\gui-go\\e2e\'"')
    scp(args.host, E2E / 'windows_native_panel_host.ps1', f'{work}/host.ps1')

    print('starting the run in the desktop session', flush=True)
    flags = ' -FirewallPromptOk' + (' -ClobberClipboard' if args.clobber_clipboard else '')
    user = ssh(args.host, 'echo %USERNAME%')
    command = f'powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File {work_win}\\host.ps1 -Work {work_win}{flags}'
    ssh(args.host, f'schtasks /Create /TN {TASK} /TR "{command}" /SC ONCE /ST 00:00 /RU {user} /IT /F')
    try:
        ssh(args.host, f'schtasks /Run /TN {TASK}')
        deadline, shown, status = time.monotonic() + args.timeout, 0, ''
        while time.monotonic() < deadline:
            time.sleep(5)
            status = ssh(args.host, f'type {work_win}\\run\\status.txt', check=False)
            log = ssh(args.host, f'type {work_win}\\run\\host.log', check=False).splitlines()
            for line in log[shown:]:
                print(line, flush=True)
            shown = len(log)
            if status in ('PASSED', 'FAILED', 'ERROR'):
                break
        else:
            status = 'TIMEOUT'
    finally:
        ssh(args.host, f'schtasks /Delete /TN {TASK} /F', check=False)
    scp(args.host, f'{work}/run', out, to_remote=False)
    print(f'artifacts: {out}')
    results = out / 'run/out/native-assertions.json'
    if results.exists():
        import json
        for check in json.loads(results.read_text(encoding='utf-8-sig')).get('checks', []):
            print(('PASS ' if check.get('ok') is not False else 'FAIL ') + check.get('check', ''))
    print(f'result: {status}')
    sys.exit(0 if status == 'PASSED' else 1)


if __name__ == '__main__':
    main()
