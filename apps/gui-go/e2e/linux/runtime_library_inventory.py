#!/usr/bin/env python3
"""Runtime shared-library inventory of a real AppImage execution (issue #1903).

The static scan in audit_dlopen.py cannot see libraries that are opened with dlopen or found by directory scan. This tool observes a
real run instead and feeds appimage_content_check.py --runtime-inventory:

  runtime_library_inventory.py observe <AppImage> <out-dir> [--extract] [--seconds N]
      Start the AppImage on an X11 display (Xvfb) with a private session bus, LD_DEBUG=libs,files and a loader-trace file per process.
      Poll until the daemon is healthy and a WebKit web process is up, then record /proc/<pid>/maps and the loader trace of every
      process whose executable lives inside the AppImage. Writes <out-dir>/observation.json and <out-dir>/loader.<pid>.
  runtime_library_inventory.py merge <squashfs-root> <out.json> <observation.json>...
      Classify every mapped library as bundled (inside the AppDir, hashed against <squashfs-root>) or host-owned, per process role,
      and list the dlopen requests that failed. All observations must come from the same AppImage.

Coverage is the executed scenario only: GUI start, WebKit page load, daemon health. It is evidence for what ran, never a proof that
nothing else is ever dlopen'ed. Run it as a non-root user on a host that has Xvfb and dbus-daemon.
"""
import argparse
import contextlib
import hashlib
import json
import os
import re
import selectors
import signal
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROLES = {'uniclipboard': 'gui', 'uniclipd': 'daemon', 'WebKitWebProcess': 'webkit-web', 'WebKitNetworkProcess': 'webkit-network', 'WebKitGPUProcess': 'webkit-gpu'}
ENV_DROP = ('UC_PROFILE', 'UC_GUI_GO_ISOLATED', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'WAYLAND_DISPLAY')
DLOPEN = re.compile(r'^\s*(\d+):\s+file=(\S+) \[\d+\];\s+dynamically loaded by (\S+)')


def read_line(proc, seconds, what):
    """Read one complete line from a helper's stdout within one shared deadline; fail on EOF, an empty line or a timeout."""
    deadline, buf = time.monotonic() + seconds, b''
    with selectors.DefaultSelector() as sel:
        sel.register(proc.stdout, selectors.EVENT_READ)
        while not buf.endswith(b'\n'):
            if not sel.select(max(0.0, deadline - time.monotonic())):
                raise RuntimeError(f'{what} was not announced within {seconds} s')
            chunk = os.read(proc.stdout.fileno(), 4096)
            if not chunk:
                raise RuntimeError(f'{what}: the helper closed its output before announcing it')
            buf += chunk
    line = buf.decode().strip()
    if not line:
        raise RuntimeError(f'{what} is empty')
    return line


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def ancestors(pid):
    names = []
    while pid > 1:
        try:
            stat = Path(f'/proc/{pid}/stat').read_text()
            comm = stat[stat.index('(') + 1:stat.rindex(')')]
            pid = int(stat[stat.rindex(')') + 2:].split()[1])
        except (OSError, ValueError):
            break
        names.append(comm)
    return names


def elf_soname(path):
    """DT_SONAME of a little-endian ELF64 shared object, read on the host that loaded it (a clean host has no binutils).

    Anything else is refused rather than guessed. A library without DT_SONAME is known by its file name."""
    with open(path, 'rb') as f:
        head = f.read(64)
        if head[:4] != b'\x7fELF' or head[4] != 2 or head[5] != 1:
            raise SystemExit(f'{path}: not a little-endian ELF64 file')
        phoff, phentsize, phnum = int.from_bytes(head[32:40], 'little'), int.from_bytes(head[54:56], 'little'), int.from_bytes(head[56:58], 'little')
        loads, dynamic = [], None
        for i in range(phnum):
            f.seek(phoff + i * phentsize)
            ph = f.read(56)
            kind, offset, vaddr, filesz = (int.from_bytes(ph[0:4], 'little'), int.from_bytes(ph[8:16], 'little'), int.from_bytes(ph[16:24], 'little'), int.from_bytes(ph[32:40], 'little'))
            if kind == 1:
                loads.append((vaddr, offset, filesz))
            elif kind == 2:
                dynamic = (offset, filesz)
        if dynamic is None:
            return Path(path).name
        f.seek(dynamic[0])
        entries = f.read(dynamic[1])
        tags = {}
        for i in range(0, len(entries) - 15, 16):
            tag, value = int.from_bytes(entries[i:i + 8], 'little'), int.from_bytes(entries[i + 8:i + 16], 'little')
            if tag == 0:
                break
            tags.setdefault(tag, value)  # DT_STRTAB = 5, DT_SONAME = 14
        if 14 not in tags or 5 not in tags:
            return Path(path).name
        strtab = next((offset + tags[5] - vaddr for vaddr, offset, filesz in loads if vaddr <= tags[5] < vaddr + filesz), None)
        if strtab is None:
            raise SystemExit(f'{path}: DT_STRTAB is not inside a PT_LOAD segment')
        f.seek(strtab + tags[14])
        return f.read(256).split(b'\0', 1)[0].decode()


def still_running(pid, exe):
    try:
        return os.readlink(f'/proc/{pid}/exe') == exe
    except OSError:
        return False


def mapped_libraries(pid):
    libs = set()
    for line in Path(f'/proc/{pid}/maps').read_text().splitlines():
        fields = line.split(None, 5)
        if len(fields) == 6 and '.so' in fields[5] and fields[5].startswith('/'):
            libs.add(fields[5].replace(' (deleted)', ''))
    return sorted(libs)


def dlopen_requests(lines, pids):
    """Return the dlopen requests of the given pids in a loader trace and, of those, the ones that found no library."""
    starts = [(i, DLOPEN.match(line)) for i, line in enumerate(lines)]
    starts = [(i, m) for i, m in starts if m and int(m.group(1)) in pids]
    requests, failed = [], []
    for n, (i, m) in enumerate(starts):
        end = starts[n + 1][0] if n + 1 < len(starts) else len(lines)
        pid, target, requester = m.groups()
        requests.append([pid, target, requester])
        # glibc prints "generating link map" for a library it really opened; a request without one found nothing (dlerror at the caller).
        if not any(re.match(rf'^\s*{pid}:\s+file={re.escape(target)} \[\d+\];\s+generating link map', line) for line in lines[i:end]):
            failed.append([pid, target])
    return requests, failed


def observe(args):
    image, out = Path(args.image).resolve(), Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=False)
    home = Path(str(image) + '.home')  # portable mode: the AppImage runtime redirects $HOME to this directory only if it already exists
    for sub in ('run', 'config', 'data', 'cache'):
        (home / sub).mkdir(parents=True, mode=0o700, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if k not in ENV_DROP}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / 'config'), XDG_DATA_HOME=str(home / 'data'), XDG_CACHE_HOME=str(home / 'cache'),
               XDG_RUNTIME_DIR=str(home / 'run'), XDG_SESSION_TYPE='x11', GDK_BACKEND='x11', UC_PORTABLE='1', UC_DISABLE_SYSTEM_CLIPBOARD='1')
    helpers = []
    xvfb = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=(out / 'xvfb.log').open('w'), text=True)
    helpers.append(xvfb)
    try:
        env['DISPLAY'] = ':' + read_line(xvfb, 10, 'Xvfb display number')
        bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address=1'], stdout=subprocess.PIPE, stderr=(out / 'bus.log').open('w'), text=True, env=env)
        helpers.append(bus)
        env['DBUS_SESSION_BUS_ADDRESS'] = read_line(bus, 10, 'D-Bus session address')
    except RuntimeError as e:
        for h in helpers:
            h.terminate()
        sys.exit(str(e))
    env.update(LD_DEBUG='libs,files', LD_DEBUG_OUTPUT=str(out / 'loader'))  # after the bus: the bus itself is not under test
    proc = subprocess.Popen([str(image)] + (['--appimage-extract-and-run'] if args.extract else []), env=env, stdout=(out / 'gui.log').open('w'), stderr=subprocess.STDOUT, start_new_session=True)
    scoped, health = {}, None
    result = {'imageSha256': sha256(image), 'extract': args.extract, 'processes': [], 'sonames': {}}
    try:
        for _ in range(args.seconds):
            for entry in Path('/proc').iterdir():
                if not entry.name.isdigit():
                    continue
                with contextlib.suppress(OSError):  # the process exited between listing /proc and reading it
                    exe = os.readlink(entry / 'exe')
                    if os.getpgid(int(entry.name)) == proc.pid and Path(exe).name in ROLES and ('.mount_' in exe or 'appimage_extracted_' in exe):
                        scoped[int(entry.name)] = exe
            for conn in home.rglob('daemon.conn'):
                with contextlib.suppress(OSError, ValueError, KeyError):  # daemon.conn is stale or the daemon is not up yet; the next poll retries and the health check gates the result
                    c = json.loads(conn.read_text())
                    daemon_exe = os.readlink(f"/proc/{c['pid']}/exe")  # the daemon detaches from the GUI's process group
                    if '.mount_' not in daemon_exe and 'appimage_extracted_' not in daemon_exe:
                        raise ValueError(f'daemon executable outside the test image: {daemon_exe}')
                    scoped[c['pid']] = daemon_exe
                    with urllib.request.urlopen(f"http://{c['host']}:{c['port']}/health", timeout=2) as r:
                        health = r.status
            if health == 200 and any(Path(e).name == 'WebKitWebProcess' for e in scoped.values()):
                time.sleep(4)  # let the page load finish so late dlopen calls land in the trace
                break
            if proc.poll() is not None:
                break
            time.sleep(1)
        for pid, exe in sorted(scoped.items()):
            with contextlib.suppress(OSError):  # the process exited before its maps were read; the role coverage assertions catch a missing process
                if os.readlink(f'/proc/{pid}/exe') != exe:
                    continue
                libraries = mapped_libraries(pid)
                result['processes'].append({'pid': pid, 'exe': exe, 'role': ROLES[Path(exe).name], 'libraries': libraries, 'ancestors': ancestors(pid)})
                for path in libraries:
                    if Path(path).name != 'ld.so.cache' and path not in result['sonames']:
                        result['sonames'][path] = elf_soname(path)
        result.update(health=health, guiAlive=proc.poll() is None)
    finally:
        for pid, exe in scoped.items():
            with contextlib.suppress(OSError):  # already gone
                if os.readlink(f'/proc/{pid}/exe') == exe:
                    os.kill(pid, signal.SIGTERM)
        if proc.poll() is None:
            proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        for h in helpers:
            if h.poll() is None:
                h.terminate()
            h.wait(timeout=5)
        deadline = time.monotonic() + 15
        while True:
            left = [pid for pid, exe in scoped.items() if still_running(pid, exe)]
            if not left or time.monotonic() >= deadline:
                break
            time.sleep(0.1)
        result['remainingTestProcesses'] = left
    known = {p['pid'] for p in result['processes']}
    requests, failed = [], []
    for trace in sorted(out.glob('loader.*')):
        found, missing = dlopen_requests(trace.read_text(errors='replace').splitlines(), known)
        requests += found
        failed += missing
    result['dlopenRequests'] = [{'pid': int(p), 'target': t, 'requester': r} for p, t, r in requests]
    result['failedLoads'] = [{'pid': int(p), 'target': t} for p, t in failed]
    result['smokePassed'] = bool(result['guiAlive'] and health == 200 and any(p['role'] == 'webkit-web' for p in result['processes']) and not result['remainingTestProcesses'])
    (out / 'observation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({k: v for k, v in result.items() if k not in ('processes', 'dlopenRequests', 'failedLoads')}, indent=2))
    print(f"{len(result['processes'])} processes, {len(result['dlopenRequests'])} dlopen requests, {len(result['failedLoads'])} failed loads")
    return 0 if result['smokePassed'] else 1


def merge(args):
    root = Path(args.root).resolve()
    rows, hashes, failed, sandbox = {}, set(), {}, []
    for obs_path in args.observations:
        obs = json.loads(Path(obs_path).read_text())
        hashes.add(obs['imageSha256'])
        scenario = 'extract-and-run' if obs['extract'] else 'fuse-mount'
        for proc in obs['processes']:
            appdir = re.match(r'(/.+?/(?:\.mount_[^/]+|appimage_extracted_[^/]+))/', proc['exe'])
            if not appdir:
                raise SystemExit(f"cannot find the AppDir of {proc['exe']}")
            if proc['role'].startswith('webkit'):
                sandbox.append({'scenario': scenario, 'role': proc['role'], 'underBwrap': 'bwrap' in proc['ancestors']})
            for path in proc['libraries']:
                name = Path(path).name
                if name == 'ld.so.cache' or '.so' not in name:
                    continue
                if path.startswith(appdir[1] + '/'):
                    rel = path[len(appdir[1]) + 1:]
                    elf = root / rel
                    if not elf.is_file():
                        raise SystemExit(f'observed bundled file is missing from the extracted image: {rel}')
                    classification, digest = 'bundled', sha256(elf)
                else:
                    rel = digest = None
                    classification = 'host-owned'
                row = rows.setdefault((name, classification, rel), {'name': name, 'classification': classification, 'soname': obs['sonames'][path], 'processes': set(), 'scenarios': set()})
                if rel:
                    row.update(bundleRelativePath=rel, sha256=digest)
                row['processes'].add(proc['role'])
                row['scenarios'].add(scenario)
        shipped = {p.name for p in root.rglob('*') if p.is_file() or p.is_symlink()}
        roles = {p['pid']: p['role'] for p in obs['processes']}
        for load in obs['failedLoads']:
            entry = failed.setdefault(load['target'], {'target': load['target'], 'processes': set(), 'shippedInBundle': Path(load['target']).name in shipped})
            entry['processes'].add(roles[load['pid']])
    if len(hashes) != 1:
        raise SystemExit(f'observations come from {len(hashes)} different AppImages')
    libraries = [dict(r, processes=sorted(r['processes']), scenarios=sorted(r['scenarios'])) for r in sorted(rows.values(), key=lambda r: (r['name'], r['classification']))]
    failed_loads = [dict(f, processes=sorted(f['processes'])) for f in sorted(failed.values(), key=lambda f: f['target'])]
    scope = 'Executed scenarios only (GUI start, WebKit page load, daemon health) on one host: ' + ', '.join(sorted({'extract-and-run' if json.loads(Path(p).read_text())['extract'] else 'fuse-mount' for p in args.observations})) + '. Not an exhaustive inventory of potential dlopen targets.'
    Path(args.out).write_text(json.dumps({'imageSha256': hashes.pop(), 'scope': scope, 'libraries': libraries, 'failedLoads': failed_loads, 'webkitSandbox': sandbox}, indent=2) + '\n')
    counts = {c: sum(1 for r in libraries if r['classification'] == c) for c in ('bundled', 'host-owned')}
    print(counts, f'{len(failed_loads)} distinct failed loads')
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    o = sub.add_parser('observe')
    o.add_argument('image')
    o.add_argument('out')
    o.add_argument('--extract', action='store_true')
    o.add_argument('--seconds', type=int, default=40)
    o.set_defaults(run=observe)
    m = sub.add_parser('merge')
    m.add_argument('root')
    m.add_argument('out')
    m.add_argument('observations', nargs='+')
    m.set_defaults(run=merge)
    args = parser.parse_args()
    sys.exit(args.run(args))


if __name__ == '__main__':
    main()
