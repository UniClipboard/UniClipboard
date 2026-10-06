#!/usr/bin/env python3
"""Host-helper E2E of the self-contained AppImage (slice 17c10, docs/architecture/gui-go-linux-appimage-host-helpers.md).

AppRun puts `$APPDIR/usr/lib` in LD_LIBRARY_PATH. The GUI starts real HOST programs: `xdg-open` (Wails `Browser.OpenURL` for links, the product's
`openWithSystem` for "open data/logs folder", "reveal" and "open image externally"), which dispatches to `gio open` (GNOME) or to the registered
handler's Exec line (generic). Whether those programs, and the programs they start, see the AppImage's libraries is verified, not assumed.

  linux_appimage_helpers_run.py --out DIR --appimage X.AppImage --manifest package-manifest.json --desktop generic|gnome

Runs inside a distribution image WITHOUT GTK/WebKitGTK (uc-gui-go-linux-runtime-helpers:17c10-ubuntu / -fedora: real xdg-utils, real gio, real
shared-mime-info) as an unprivileged user, portable mode, real GUI + real release daemon inside the AppImage, the GUI run under `strace -f` (execve,
clone, openat: observer only, it changes nothing the helper sees).

Fixture (the task's, not the host's behaviour): one desktop entry registered through the host's own `xdg-mime default` for inode/directory, image/png and
x-scheme-handler/https whose Exec is a small `sh` recorder. It records argv, /proc/self/environ and the `.so` files mapped by the handler process.
What is asserted is host behaviour: xdg-open / gio resolve that handler and start it with the product's exact target, their exit status, the
environment they were started with and the origin of every library a host helper process opened.
Not proven: a real browser or file manager, a real desktop session, portals, Wayland, native amd64.
"""
import argparse
import base64
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from linux_appimage_portable_run import UserRun, USER, as_user, environ_of, stop, wait_daemon  # noqa: E402
from linux_appimage_run import DISPLAY, PASSPHRASE, Launch, pid_alive, procs, sha256, start_xvfb, wait_panel_ready  # noqa: E402
from linux_appimage_tls_run import Reports, StopScenario  # noqa: E402
import pwd  # noqa: E402

RECORDS = Path('/tmp/uc10-records')
MIMES = ('inode/directory', 'image/png', 'x-scheme-handler/https')
RECORDER = r'''#!/bin/sh
# 17c10 fixture handler: record what the host helper chain delivered.
f="/tmp/uc10-records/$(date +%s%N)-$$"
{
  echo "ARGC $#"
  for a in "$@"; do echo "ARG $a"; done
  echo "EXE $(readlink /proc/$$/exe)"
  echo "---ENV"
  tr '\0' '\n' < /proc/$$/environ
  echo "---MAPS"
  awk '$6 ~ /\.so/ {print $6}' /proc/$$/maps | sort -u
} > "$f.tmp"
mv "$f.tmp" "$f"
'''
DESKTOP = '''[Desktop Entry]
Type=Application
Name=UC 17c10 recorder
Exec=/usr/local/bin/uc-recorder %u
MimeType=inode/directory;image/png;x-scheme-handler/https;
NoDisplay=true
Terminal=false
'''


def png_bytes():
    def chunk(t, d):
        c = struct.pack('>I', len(d)) + t + d
        return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    raw = b'\x00\xff\x00\x00'
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')


def parse_record(path):
    text = path.read_text(errors='replace')
    head, _, rest = text.partition('---ENV\n')
    env, _, maps = rest.partition('---MAPS\n')
    args = [l[4:] for l in head.splitlines() if l.startswith('ARG ')]
    exe = next((l[4:] for l in head.splitlines() if l.startswith('EXE ')), '')
    return {'file': path.name, 'args': args, 'exe': exe, 'env': dict(l.split('=', 1) for l in env.splitlines() if '=' in l), 'maps': maps.split()}


def records():
    return sorted(RECORDS.glob('*')) if RECORDS.exists() else []


def wait_record(before, match, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for p in records():
            if p.name in before or p.name.endswith('.tmp'):
                continue
            rec = parse_record(p)
            if match(rec):
                return rec
        time.sleep(.3)
    return None


class StraceRun(UserRun):
    """UserRun whose GUI is started under strace run as ROOT with `-u` (a non-root tracer makes the kernel ignore the setuid bit of fusermount, which the AppImage runtime needs: "fusermount: mount failed: Operation not permitted", kept in dev1/helpers-ubuntu-gnome) (follows forks; execve with the full environment, clone/fork results for the process tree, openat for libraries)."""

    def launch(self, tag, extra_env=None, appimage=None, args=(), drop=()):
        evidence, control, trace = self.out / f'{tag}.jsonl', self.out / f'{tag}.control', self.out / f'{tag}.strace'
        for f in (evidence, control, trace):
            f.write_text('')
            f.chmod(0o666)
        env = dict(self.env, UC_GUI_GO_EVIDENCE=str(evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(control), **(extra_env or {}))
        for key in drop:
            env.pop(key, None)
        log = self.out / f'{tag}.log'
        log.write_text('')
        log.chmod(0o666)
        cmd = ['strace', '-u', USER, '-f', '-q', '-v', '-s', '16384', '-e', 'trace=execve,clone,clone3,fork,vfork,openat', '-o', str(trace), str(appimage or self.appimage), *args]
        proc = subprocess.Popen(cmd, env=env, cwd=str(Path(appimage or self.appimage).parent), stdout=log.open('a'), stderr=subprocess.STDOUT)
        self.trace = trace
        return Launch(proc, evidence, control, log)


EXEC_RE = re.compile(r'^(\d+)\s+execve\("((?:[^"\\]|\\.)*)", \[(.*?)\], \[(.*)\](?:\)\s+= (-?\d+)| <unfinished \.\.\.>)')
EXEC_RESUMED_RE = re.compile(r'^(\d+)\s+<\.\.\. execve resumed>.*\)\s+= (-?\d+)')
SPAWN_RE = re.compile(r'^(\d+)\s+.*(?:clone3?|fork|vfork)\b.*= (\d+)\s*$')
OPEN_RE = re.compile(r'^(\d+)\s+openat\(.*?"((?:[^"\\]|\\.)*)".*\) = (-?\d+)')
EXIT_RE = re.compile(r'^(\d+)\s+\+\+\+ (exited with (\d+)|killed by (\w+)).*\+\+\+')


def unquote(s):
    return s.encode().decode('unicode_escape', errors='replace')


def parse_trace(path):
    """-> (execs [{pid, exe, argv, env, rc}], children {pid: [pid]}, opens {pid: [path]}, exits {pid: 'exit N'|'signal X'})"""
    execs, children, opens, exits = [], {}, {}, {}
    pending = {}
    for line in Path(path).read_text(errors='replace').splitlines():
        m = EXEC_RE.match(line)
        if m:
            argv = [unquote(x) for x in re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(3))]
            env = dict(unquote(x).split('=', 1) for x in re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(4)) if '=' in x)
            entry = {'pid': int(m.group(1)), 'exe': unquote(m.group(2)), 'argv': argv, 'env': env, 'rc': int(m.group(5)) if m.group(5) is not None else None}
            execs.append(entry)
            if entry['rc'] is None:
                pending[entry['pid']] = entry
            continue
        m = EXEC_RESUMED_RE.match(line)
        if m:
            if int(m.group(1)) in pending:
                pending.pop(int(m.group(1)))['rc'] = int(m.group(2))
            continue
        m = OPEN_RE.match(line)
        if m:
            if int(m.group(3)) >= 0 and '.so' in m.group(2):
                opens.setdefault(int(m.group(1)), []).append(unquote(m.group(2)))
            continue
        m = EXIT_RE.match(line)
        if m:
            exits[int(m.group(1))] = f'exit {m.group(3)}' if m.group(3) is not None else f'signal {m.group(4)}'
            continue
        m = SPAWN_RE.match(line)
        if m:
            children.setdefault(int(m.group(1)), []).append(int(m.group(2)))
    return execs, children, opens, exits


def tree_of(root, children):
    seen, todo = set(), [root]
    while todo:
        p = todo.pop()
        if p not in seen:
            seen.add(p)
            todo += children.get(p, [])
    return seen


def analyse_chain(parsed, mount, target_pred):
    """The helper chain started for one action: the successful execve of xdg-open whose argv satisfies `target_pred`, every host process below it,
    their exit statuses, the environment xdg-open was started with and every library any of them opened, split into inside/outside the mount."""
    execs, children, opens, exits = parsed
    root = next((e for e in execs if e['rc'] == 0 and e['exe'].endswith('/xdg-open') and target_pred(e['argv'])), None)
    if root is None:
        return None
    pids = tree_of(root['pid'], children)
    chain = [{'pid': e['pid'], 'exe': e['exe'], 'argv': e['argv'][1:], 'rc': e['rc'], 'exit': exits.get(e['pid'])} for e in execs if e['pid'] in pids and e['rc'] == 0]
    libs_mount, libs_host = {}, {}
    for pid in pids:
        for lib in opens.get(pid, []):
            (libs_mount if lib.startswith(mount) else libs_host).setdefault(lib, []).append(pid)
    return {'xdgOpenPid': root['pid'], 'xdgOpenArgv': root['argv'], 'xdgOpenEnv': {k: v for k, v in root['env'].items() if mount in v or k in ('LD_LIBRARY_PATH', 'XDG_CURRENT_DESKTOP', 'APPDIR', 'APPIMAGE')},
            'processes': chain, 'exitOfXdgOpen': exits.get(root['pid']), 'mountLibsOpenedByHostProcesses': sorted(libs_mount),
            'hostLibsOpenedCount': len(libs_host), 'hostLibsOfInterest': sorted(l for l in libs_host if re.search(r'lib(gio|glib|gobject|gmodule|dbus|ffi|selinux)', l))}


def mount_vars(env, mount):
    return {k: v for k, v in env.items() if mount in v or k == 'LD_LIBRARY_PATH' and 'usr/lib' in v}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--appimage', type=Path, required=True)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--desktop', choices=('generic', 'gnome'), required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o777)
    subprocess.run(['useradd', '-m', '-d', f'/home/{USER}', '-u', '1500', '-s', '/bin/bash', USER], check=True)
    account = pwd.getpwnam(USER)
    sandbox = Path(tempfile.mkdtemp(prefix='uc-helpers-'))
    for d in (sandbox,):
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir, install, fixtures = sandbox / 'run', sandbox / 'install', sandbox / 'fixtures'
    for d in (runtime_dir, install, fixtures):
        d.mkdir()
        os.chown(d, account.pw_uid, account.pw_gid)
    runtime_dir.chmod(0o700)
    RECORDS.mkdir(mode=0o777, exist_ok=True)
    RECORDS.chmod(0o777)
    target = install / 'UniClipboard.AppImage'
    shutil.copy2(args.appimage, target)
    os.chown(target, account.pw_uid, account.pw_gid)
    env = dict(os.environ, HOME=pwd.getpwnam(USER).pw_dir, XDG_RUNTIME_DIR=str(runtime_dir), DISPLAY=DISPLAY, UC_DISABLE_SYSTEM_CLIPBOARD='1', NO_COLOR='1',
               XDG_SESSION_TYPE='x11', UC_GUI_GO_EXIT_MODE='full', UC_GUI_GO_E2E_PHASE='wake', UC_GUI_GO_E2E_VISIBLE='1', UC_GUI_GO_E2E_SECRET=PASSPHRASE,
               USER=USER, LOGNAME=USER)
    for key in ('WAYLAND_DISPLAY', 'UC_PROFILE', 'UC_PORTABLE', 'UNICLIPBOARD_ENV', 'APPIMAGE', 'APPDIR', 'GDK_BACKEND', 'XDG_CONFIG_HOME', 'DBUS_SESSION_BUS_ADDRESS',
                'UC_E2E_BUS', 'GIO_MODULE_DIR', 'GIO_EXTRA_MODULES', 'XDG_CURRENT_DESKTOP', 'BROWSER', 'GNOME_DESKTOP_SESSION_ID', 'UC_GUI_GO_E2E_OPEN_LOG'):
        env.pop(key, None)
    if args.desktop == 'gnome':
        env['XDG_CURRENT_DESKTOP'] = 'GNOME'
    run = StraceRun(out, target, env)
    r = run.results
    osrel = dict(l.split('=', 1) for l in Path('/etc/os-release').read_text().splitlines() if '=' in l)
    r.update({'mode': f'helpers-{args.desktop}', 'appimageSha256': sha256(target), 'sandbox': str(sandbox), 'user': USER, 'distribution': osrel.get('PRETTY_NAME', '').strip('"'),
              'kernelMachine': os.uname().machine, 'xdgCurrentDesktop': env.get('XDG_CURRENT_DESKTOP'),
              'hostHelpers': {t: shutil.which(t) for t in ('xdg-open', 'xdg-mime', 'gio')},
              'scope': 'container, unprivileged user, Xvfb, no GTK/WebKitGTK on the host, portable mode, sh recorder as the handler (no real browser/file manager)'})
    r['hostPackages'] = subprocess.run('(dpkg-query -W xdg-utils libglib2.0-bin libglib2.0-0t64 shared-mime-info 2>/dev/null || rpm -q xdg-utils glib2 shared-mime-info)', shell=True,
                                       capture_output=True, text=True).stdout.split('\n')
    xvfb = start_xvfb(out)
    launches = []
    try:
        # --- handler fixture: a SYSTEM-wide default (desktop entry in /usr/share/applications, default in /etc/xdg/mimeapps.list), the way a distribution or an
        # administrator registers one. A per-user registration would live under the user's real HOME, which portable mode replaces for the GUI process (HOME =
        # <AppImage>.home): that is the first red run (dev1/helpers-ubuntu-gnome-attempt2-*), a portable-mode effect kept apart from AppRun's environment leak.
        Path('/usr/local/bin/uc-recorder').write_text(RECORDER)
        Path('/usr/local/bin/uc-recorder').chmod(0o755)
        Path('/usr/share/applications').mkdir(parents=True, exist_ok=True)
        Path('/usr/share/applications/uc-recorder.desktop').write_text(DESKTOP)
        Path('/etc/xdg').mkdir(exist_ok=True)
        Path('/etc/xdg/mimeapps.list').write_text('[Default Applications]\n' + ''.join(f'{m}=uc-recorder.desktop\n' for m in MIMES))
        defaults = {m: as_user(['xdg-mime', 'query', 'default', m], env).stdout.strip() for m in MIMES}
        r['xdgMimeDefaults'] = defaults
        run.check('T0 the handler is the system default for the three types according to the host\'s own xdg-mime', all(v == 'uc-recorder.desktop' for v in defaults.values()), defaults)

        nonce = secrets.token_hex(6)
        reveal_dir = fixtures / f'uc10-reveal-{nonce}'
        reveal_dir.mkdir()
        os.chown(reveal_dir, account.pw_uid, account.pw_gid)
        reveal_file = reveal_dir / 'item.txt'
        reveal_file.write_text('x')
        os.chown(reveal_file, account.pw_uid, account.pw_gid)
        url = f'https://127.0.0.1:9/uc10-{nonce}'
        png = fixtures / f'uc10-control-{nonce}.png'
        png.write_bytes(png_bytes())
        os.chown(png, account.pw_uid, account.pw_gid)

        # --- T0 host control: the host's own chain with the host's own environment reaches the fixture (and a type without a handler does not)
        def host_open(tgt, extra=None, label=''):
            before = {p.name for p in records()}
            e = dict(env, **(extra or {}))
            res = as_user(['xdg-open', tgt], e, timeout=60)
            rec = wait_record(before, lambda x: x['args'] and x['args'][-1] in (tgt, f'file://{tgt}'), 15) if res.returncode == 0 else None
            return res, rec
        res, rec = host_open(str(reveal_dir))
        run.check('T0 control: the host xdg-open with the host environment delivers a directory to the handler', res.returncode == 0 and rec is not None, {'rc': res.returncode, 'err': res.stderr[-300:], 'record': bool(rec)})
        res, rec = host_open(str(png))
        run.check('T0 control: the host xdg-open delivers a PNG to the handler', res.returncode == 0 and rec is not None, {'rc': res.returncode, 'err': res.stderr[-300:], 'record': bool(rec)})
        res, rec = host_open(url)
        run.check('T0 control: the host xdg-open delivers an https URL to the handler', res.returncode == 0 and rec is not None, {'rc': res.returncode, 'err': res.stderr[-300:], 'record': bool(rec)})
        nohandler = fixtures / f'uc10-nohandler-{nonce}.uc10nohandler'
        nohandler.write_text('x')
        os.chown(nohandler, account.pw_uid, account.pw_gid)
        res, rec = host_open(str(nohandler))
        r['hostNoHandlerControl'] = {'rc': res.returncode, 'stderr': res.stderr[-300:]}
        run.check('T0 negative control: a file whose type has no registered handler is NOT delivered (the scenario can tell reached from not reached)', rec is None, r['hostNoHandlerControl'])

        # --- the real GUI
        made = as_user([str(target), '--appimage-portable-home'], env, timeout=60)
        run.check('T1 portable home created by the AppImage runtime', made.returncode == 0 and Path(str(target) + '.home').is_dir(), {'rc': made.returncode, 'err': made.stderr[-300:]})
        gui = run.launch('gui1')
        launches.append(gui)
        conn_path, conn = wait_daemon(sandbox)
        run.check('T1 the real bundled daemon started and published daemon.conn', conn is not None, str(conn_path))
        if conn is None:
            raise StopScenario()
        gui.step('bootstrapped', 120)
        state = wait_panel_ready(gui, 'h', 120)
        run.check('T1 the real WebView loaded the frontend (quick panel page ready)', state.get('panelReady') is True, state)
        table = procs()
        gui_pid = next((pid for pid, (exe, _) in table.items() if exe.endswith('/usr/bin/uniclipboard')), None)
        mount = table[gui_pid][0].split('/usr/bin/')[0] if gui_pid else None
        run.check('T1 the GUI executes from the AppImage mount', bool(mount) and mount.startswith('/tmp/.mount_'), table.get(gui_pid))
        if not mount:
            raise StopScenario()
        genv = environ_of(gui_pid)
        r['guiEnvironment'] = {k: genv.get(k) for k in ('LD_LIBRARY_PATH', 'APPDIR', 'APPIMAGE', 'GIO_MODULE_DIR', 'XDG_DATA_DIRS', 'GDK_BACKEND', 'GSETTINGS_SCHEMA_DIR', 'XDG_CURRENT_DESKTOP', 'PATH')}
        r['guiEnvVarsPointingIntoMount'] = sorted(k for k, v in genv.items() if mount in v)
        r['guiRecordedEnvironFile'] = 'gui-environ.txt'
        (out / 'gui-environ.txt').write_text('\n'.join(f'{k}={v}' for k, v in sorted(genv.items())) + '\n')

        reports = Reports()
        actions = []

        def act(name, drive, matcher, expect_reach=True, timeout=25):
            before = {p.name for p in records()}
            result = drive()
            rec = wait_record(before, matcher, timeout) if expect_reach else None
            if not expect_reach:
                time.sleep(4)
                rec = wait_record(before, matcher, 1)
            actions.append({'name': name, 'matcher': matcher, 'record': rec, 'driveResult': result, 'expectReach': expect_reach})
            return rec

        reveal_target = str(reveal_dir)
        act('reveal_path', lambda: gui.invoke('reveal', 'reveal_path', {'path': str(reveal_file)}), lambda x: bool(x['args']) and x['args'][-1] in (reveal_target, f'file://{reveal_target}'))
        image_name = f'uc10-img-{nonce}.png'
        act('open_image_externally', lambda: gui.invoke('image', 'open_image_externally', {'fileName': image_name, 'data': base64.b64encode(png_bytes()).decode()}),
            lambda x: bool(x['args']) and x['args'][-1].split('/')[-1] == image_name)
        act('open_logs_directory', lambda: gui.invoke('logs', 'open_logs_directory', {}), lambda x: bool(x['args']) and 'uniclipboard-image-handoff' not in x['args'][-1] and re.search(r'(?i)logs?', x['args'][-1]) is not None)
        url_nonce = f'https://127.0.0.1:9/uc10-url-{nonce}'
        act('Browser.OpenURL via the page', lambda: (gui.ctl(f'panel-js openurl window.__ucE2eOpenUrl("{url_nonce}","http://127.0.0.1:{reports.port}/open")', 'panel-js-openurl'), reports.wait('open', 20))[1],
            lambda x: bool(x['args']) and x['args'][-1] == url_nonce)
        nohandler_name = f'uc10-nohandler-{nonce}.uc10nohandler'
        act('open_image_externally (no handler for the type: negative control)',
            lambda: gui.invoke('nohandler', 'open_image_externally', {'fileName': nohandler_name, 'data': base64.b64encode(b'x').decode()}),
            lambda x: bool(x['args']) and x['args'][-1].split('/')[-1] == nohandler_name, expect_reach=False)

        # --- direct reproduction with the GUI's own environment (what the helper really inherits), and the loader's own account of where libgio comes from
        genv_full = environ_of(gui_pid)
        direct = {}
        host_env = dict(env)  # the runner's own (host) environment of the unprivileged user
        mount_group = sorted(k for k, v in genv_full.items() if mount in v and k not in ('LD_LIBRARY_PATH', 'GIO_MODULE_DIR', 'XDG_DATA_DIRS', 'PWD'))
        variants = [('gui-environment', {}), ('gui-environment-ld-debug', {'LD_DEBUG': 'libs'}), ('restore-LD_LIBRARY_PATH', {'LD_LIBRARY_PATH': None}),
                    ('restore-LD_LIBRARY_PATH-ld-debug', {'LD_LIBRARY_PATH': None, 'LD_DEBUG': 'libs'}), ('restore-GIO_MODULE_DIR', {'GIO_MODULE_DIR': None}),
                    ('restore-XDG_DATA_DIRS', {'XDG_DATA_DIRS': None}), ('restore-LD_LIBRARY_PATH+GIO_MODULE_DIR', {'LD_LIBRARY_PATH': None, 'GIO_MODULE_DIR': None}),
                    ('restore-all-AppRun-variables', {**{k: None for k in ('LD_LIBRARY_PATH', 'GIO_MODULE_DIR', 'XDG_DATA_DIRS', *mount_group)}})]
        loader_error = re.compile(r'error while loading|version `[^`]+\' not found|symbol lookup error|cannot open shared object|undefined symbol|no version information available')
        for label, change in variants:
            e = dict(genv_full)
            for k, v in change.items():
                if v is None:
                    e.pop(k, None)
                    if k in host_env:
                        e[k] = host_env[k]
                else:
                    e[k] = v
            before = {p.name for p in records()}
            res = subprocess.run(['xdg-open', reveal_target], env=e, user=USER, group=USER, extra_groups=[], capture_output=True, text=True, timeout=60)
            rec = wait_record(before, lambda x: bool(x['args']) and x['args'][-1] in (reveal_target, f'file://{reveal_target}'), 12) if res.returncode == 0 else None
            lines = res.stderr.splitlines()
            init_from_mount = sorted({l.split('calling init:', 1)[1].strip() for l in lines if 'calling init:' in l and mount in l})
            direct[label] = {'rc': res.returncode, 'recordReached': rec is not None, 'restored': sorted(change), 'loaderErrors': sorted({l.strip()[:300] for l in lines if loader_error.search(l)})[:8],
                             'librariesInitialisedFromMount': [x.rsplit('/', 1)[-1] for x in init_from_mount], 'stderrTail': '' if 'ld-debug' in label else res.stderr[-500:]}
        gio_raw = subprocess.run(['gio', 'help', 'open'], env=dict(genv_full), user=USER, group=USER, extra_groups=[], capture_output=True, text=True, timeout=60)
        direct['gio-help-open-gui-environment'] = {'rc': gio_raw.returncode, 'stderr': gio_raw.stderr[-500:]}
        r['directReproduction'] = direct

        # `stop()` waits for the traced process, and strace waits for every traced descendant including the detached daemon: bounded wait for the GUI process
        # itself, then detach strace (SIGTERM) and read the trace. The failed first attempt (strace wait, 60 s) is kept in dev1.
        gui.ctl('exit', 'control-exit')
        deadline = time.monotonic() + 40
        while pid_alive(gui_pid) and time.monotonic() < deadline:
            time.sleep(.3)
        r['guiExitedAfterControlExit'] = not pid_alive(gui_pid)
        gui.proc.terminate()
        try:
            gui.proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            gui.proc.kill()
        launches.clear()
        parsed = parse_trace(run.trace)
        r['straceEvidence'] = {'file': run.trace.name, 'execves': len(parsed[0]), 'bytes': run.trace.stat().st_size}

        def target_pred_for(act_name, rec):
            if rec is not None:
                last = rec['args'][-1]
                return lambda argv: argv[-1] in (last, last.replace('file://', '', 1))
            return None

        for a in actions:
            name, rec = a['name'], a['record']
            if name.startswith('reveal'):
                pred = lambda argv: argv[-1] in (reveal_target, f'file://{reveal_target}')
            elif name.startswith('Browser'):
                pred = lambda argv: argv[-1] == url_nonce
            elif 'no handler' in name:
                pred = lambda argv: argv[-1].split('/')[-1] == nohandler_name or argv[-1].endswith(nohandler_name)
            elif 'image' in name:
                pred = lambda argv: argv[-1].split('/')[-1] == image_name
            else:
                pred = lambda argv: len(argv) > 1 and bool(re.search(r'(?i)logs?', argv[-1])) and 'handoff' not in argv[-1]
            chain = analyse_chain(parsed, mount, pred)
            a['chain'] = chain
            ok_reach = (rec is not None) == a['expectReach']
            if a['expectReach']:
                run.check(f'A {name}: the real xdg-open chain delivered the product\'s target to the handler', rec is not None, {'args': rec['args'] if rec else None, 'driveResult': a['driveResult']})
                run.check(f'A {name}: xdg-open was started by the GUI (strace), exited 0, and the handler process it led to exited 0 (other chain members such as `which` probes may exit non-zero by design; all are recorded)',
                          chain is not None and chain['exitOfXdgOpen'] == 'exit 0' and any(p['exe'].endswith('/uc-recorder') and p['exit'] == 'exit 0' for p in chain['processes']),
                          None if chain is None else {'xdgOpenExit': chain['exitOfXdgOpen'], 'processes': chain['processes']})
                run.check(f'A {name}: no environment variable of the helper chain points into the AppImage mount (xdg-open as started by the GUI)',
                          chain is not None and not mount_vars(chain['xdgOpenEnv'], mount) and 'LD_LIBRARY_PATH' not in chain['xdgOpenEnv'], None if chain is None else chain['xdgOpenEnv'])
                run.check(f'A {name}: the handler process the chain started has no AppImage variable and maps no library from the AppImage mount',
                          rec is not None and not mount_vars(rec['env'], mount) and not [m for m in rec['maps'] if m.startswith(mount)],
                          None if rec is None else {'envPointingIntoMount': mount_vars(rec['env'], mount), 'mountLibs': [m for m in rec['maps'] if m.startswith(mount)]})
                run.check(f'A {name}: no host helper process of the chain opened a library from the AppImage mount',
                          chain is not None and not chain['mountLibsOpenedByHostProcesses'], None if chain is None else chain['mountLibsOpenedByHostProcesses'])
            else:
                run.check(f'A {name}: the handler was NOT reached (negative control: the harness can tell), while the product call itself returned ok',
                          rec is None and bool(a['driveResult']) and a['driveResult'].get('ok') is True,
                          {'driveResult': a['driveResult'], 'xdgOpenExit': None if chain is None else chain['exitOfXdgOpen']})
                run.check(f'A {name}: xdg-open was started and failed visibly in the trace (non-zero exit): the product cannot see this',
                          chain is not None and chain['exitOfXdgOpen'] not in (None, 'exit 0'), None if chain is None else chain['exitOfXdgOpen'])
        r['actions'] = [{k: v for k, v in a.items() if k != 'matcher'} for a in actions]
        raw, raw_dbg, fixed, fixed_dbg = (direct[k] for k in ('gui-environment', 'gui-environment-ld-debug', 'restore-LD_LIBRARY_PATH', 'restore-LD_LIBRARY_PATH-ld-debug'))
        r['rawEnvironmentDelivers'] = raw['rc'] == 0 and raw['recordReached']
        r['rawEnvironmentObservation'] = ('functional failure' if not r['rawEnvironmentDelivers'] else 'delivers, but see library origin')
        # D is the CAUSAL NEGATIVE CONTROL of the A checks, not an acceptance of the product: the GUI's own raw environment is what the product used to pass on, so it must
        # reproduce the mechanism (the loader initialises libraries of the mount in host helpers); the same command with only LD_LIBRARY_PATH restored must not.
        # Whether the raw environment still delivers is DISTRIBUTION specific (Ubuntu: same GLib version, delivers; Fedora 44 GLib 2.88: fails) and is recorded, not asserted.
        run.check('D negative control: under the GUI\'s raw environment the loader initialises libraries of the AppImage mount inside the host helper chain (mechanism present)',
                  bool(raw_dbg['librariesInitialisedFromMount']), {'libs': raw_dbg['librariesInitialisedFromMount'], 'loaderErrors': raw_dbg['loaderErrors'], 'rawDelivers': r['rawEnvironmentDelivers'], 'rawRc': raw['rc']})
        run.check('D causal control: the same command with ONLY LD_LIBRARY_PATH restored initialises no library of the mount and delivers the target',
                  not fixed_dbg['librariesInitialisedFromMount'] and fixed['rc'] == 0 and fixed['recordReached'], {'fixed': fixed, 'fixedDebug': fixed_dbg})
        r['passed'] = all(c['ok'] for c in r['checks'])
    except StopScenario:
        pass
    except Exception as e:
        r['error'] = repr(e)
        import traceback
        r['traceback'] = traceback.format_exc()
        print('ERROR', repr(e), flush=True)
    finally:
        for lc in launches:
            if lc.proc.poll() is None:
                lc.proc.terminate()
        for conn in sandbox.rglob('daemon.conn'):
            try:
                pid = json.loads(conn.read_text())['pid']
                if pid_alive(pid):
                    os.kill(pid, 15)
            except (OSError, ValueError, KeyError):
                pass
        time.sleep(1)
        xvfb.terminate()
        r['passed'] = bool(r['checks']) and all(c['ok'] for c in r['checks']) and 'error' not in r
        shutil.copytree(RECORDS, out / 'handler-records', dirs_exist_ok=True)
        (out / 'appimage-assertions.json').write_text(json.dumps(r, indent=2, default=str) + '\n')
    print(json.dumps({'passed': r['passed'], 'mode': r['mode']}))
    sys.exit(0 if r['passed'] else 1)


if __name__ == '__main__':
    main()
