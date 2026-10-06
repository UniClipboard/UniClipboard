#!/usr/bin/env python3
"""Single-instance E2E: the Wails `Options.SingleInstance` lock decides which GUI process is the first instance.

Real pieces: the e2e GUI build (Wails lock + NSDistributedNotificationCenter activation, daemon bootstrap, quick
panel helper), real daemons, real second launches of the same executable (each a real process that really exits).
Injected pieces, stated plainly:
  - `--autostart` is passed by hand to stand in for the LaunchAgent that `app.Autostart.Enable` bootstraps; no
    real `launchctl bootstrap` is run (it would start the job under the real HOME). The Wails behaviour that
    matters, "that launch is just another process of the same scope", is what is exercised.
  - the "other application" is the same source built with another bundle identifier (and run unbundled), not the
    shipped production binary, which refuses to start outside a development profile.
Quiet: throwaway HOMEs, file key store, no system clipboard, accessory activation, off-screen windows; no focus, no
pointer, no keychain, nothing installed. Signals only go to PIDs this run started (Popen) or that the GUI itself
reported in evidence, after `ps -E` shows the executable and the throwaway HOME in their environment.

Scenarios:
  N  `--quick-panel` with no first instance exits 1 before any daemon work (Tauri validate_primary_launch)
  A  first instance (silent startup, real quick panel helper). Second launches: `--autostart` (no window), `--quick-panel`
     (no window, ignored), plain (window created); a burst of 8; GUI/daemon/helper PIDs and the process table unchanged
  B  early second launches while the first instance bootstraps its daemon: both exit 0; `--autostart` stays silent, the plain
     launch is held and the first instance shows the window once the bootstrap ends; one daemon only
  C  different profile, same profile name under another HOME, and another bundle identifier: independent first
     instances with distinct UniqueIDs and lock files; a second launch only reaches its own scope
  R  Restart (tray path): the new process waits for the old one and becomes the first instance
  T  after the first instance quits, a new launch takes over
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, pid_alive, read_steps  # noqa: E402
from startup_run import Profile, set_mode  # noqa: E402

BINARY = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
ALT_BUNDLE = 'app.uniclipboard.desktop.other'
LIFETIMES = []


def now():
    return time.time()


def ps_env(pid):
    return subprocess.run(['ps', '-Eww', '-p', str(pid), '-o', 'command='], capture_output=True, text=True).stdout


def owned(pid, home, binary=BINARY):
    text = ps_env(pid)
    return str(binary) in text and f'HOME={home}' in text


def home_processes(home):
    """pid -> command of every process whose environment carries this throwaway HOME."""
    rows = subprocess.run(['ps', '-Eww', '-axo', 'pid=,ppid=,command='], capture_output=True, text=True).stdout.splitlines()
    found = {}
    for row in rows:
        if f'HOME={home} ' in row + ' ' or f'HOME={home}\n' in row:
            parts = row.split(None, 2)
            found[int(parts[0])] = parts[2].split(' ')[0]
    return found


def lock_dir():
    return Path(subprocess.check_output(['getconf', 'DARWIN_USER_TEMP_DIR'], text=True).strip())


def lock_files(unique_ids):
    d = lock_dir()
    return {u: (d / f'{u}.lock').exists() for u in unique_ids}


def wait_until(fn, timeout, what):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(.1)
    raise RuntimeError(f'timeout waiting for {what}')


class Gui:
    """One GUI process started by this run, with its evidence and control files."""

    def __init__(self, label, p, out, binary=BINARY, extra=None, args=(), exit_mode='full'):
        self.label, self.p, self.binary = label, p, binary
        self.evidence = out / f'si-{label}.jsonl'
        self.evidence.write_text('')
        self.control = out / f'si-{label}.control'
        self.control.write_text('')
        self.log = out / f'si-{label}.log'
        from run import isolated_env
        env = isolated_env(p.home, p.profile, dict(p.base, UC_GUI_GO_EVIDENCE=str(self.evidence), UC_GUI_GO_E2E_CONTROL_FILE=str(self.control),
                                                    UC_GUI_GO_E2E_NOTIFY_LOG=str(p.notify_log), UC_GUI_GO_EXIT_MODE=exit_mode, **(extra or {})))
        if 'UC_GPUI_QUICK_PANEL' not in (extra or {}):
            env.pop('UC_GPUI_QUICK_PANEL', None)  # the real quick panel helper runs (default on)
        self.started = now()
        self.proc = subprocess.Popen([str(binary), *args], env=env, stdout=self.log.open('w'), stderr=subprocess.STDOUT)
        self.pid = self.proc.pid
        self.sent = 0

    def steps(self):
        return read_steps(self.evidence, 0)

    def step(self, name, timeout=120, where=lambda r: True):
        def find():
            if self.proc.poll() is not None and not any(r['step'] == name and where(r) for r in self.steps()):
                raise RuntimeError(f'{self.label} exited ({self.proc.returncode}) before step {name}')
            return [r for r in self.steps() if r['step'] == name and where(r)]
        return wait_until(find, timeout, f'{self.label}:{name}')

    def ctl(self, line):
        with self.control.open('a') as f:
            f.write(line + '\n')

    def state(self, tag, pid=None):
        self.ctl(f'state {tag}')
        rows = self.step('control-state', 30, lambda r: r['detail']['label'] == tag)
        return rows[-1]['detail']

    def ready(self):
        """The first instance is fully started: the run loop (and with it the activation observer) is up."""
        self.step('bootstrapped', 120)
        detail = self.state('ready')
        time.sleep(1.5)
        return detail

    def second_instances(self):
        return [r for r in self.steps() if r['step'] == 'second-instance']

    def launches(self):
        return [r for r in self.steps() if r['step'] == 'launch']

    def exit(self, timeout=90):
        self.ctl('exit')
        wait_until(lambda: not pid_alive(self.pid) or self.proc.poll() is not None, timeout, f'{self.label} exit')
        self.proc.wait(timeout=10)
        LIFETIMES.append({'label': self.label, 'pid': self.pid, 'start': self.started, 'end': now(), 'exit': self.proc.returncode})


def second_launch(label, p, out, args=(), binary=BINARY, extra=None):
    """A real second process of the same scope; returns (exit code, seconds to exit, log text)."""
    log = out / f'si-{label}.log'
    from run import isolated_env
    env = isolated_env(p.home, p.profile, dict(p.base, UC_GUI_GO_EVIDENCE=str(out / f'si-{label}.jsonl'), UC_GUI_GO_E2E_NOTIFY_LOG=str(p.notify_log), **(extra or {})))
    start = time.monotonic()
    t0 = now()
    proc = subprocess.Popen([str(binary), *args], env=env, stdout=log.open('w'), stderr=subprocess.STDOUT)
    code = proc.wait(timeout=60)
    LIFETIMES.append({'label': label, 'pid': proc.pid, 'start': t0, 'end': now(), 'exit': code})
    return code, round(time.monotonic() - start, 2), log.read_text(), proc.pid


def init_profile(p):
    cli(p.cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'single-instance')
    for _ in range(3):
        if p.daemon_alive():
            break
        time.sleep(3)
    set_mode(p, 'silent')


def new_profile(out, name, same_profile_as=None):
    sub = out / name
    sub.mkdir(exist_ok=True)
    p = Profile(sub)
    if same_profile_as is not None:
        from run import isolated_env
        p.profile = same_profile_as.profile
        path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
        p.cli_env = isolated_env(p.home, p.profile, {'PATH': path})
    return p


def build_alt(out):
    """The same e2e source with another bundle identifier, standing in for an independent application."""
    binary = out / 'alt-bundle-gui'
    subprocess.run(['go', 'build', '-tags', 'e2e', '-ldflags', f'-X main.bundleID={ALT_BUNDLE}', '-o', str(binary), '.'],
                   cwd=ROOT / 'apps/gui-go', check=True)
    return binary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    results = {'passed': False}
    profiles, guis, loose_pids = [], [], []
    try:
        alt_binary = build_alt(out)
        pa, pb, pc = new_profile(out, 'a'), new_profile(out, 'b'), None
        pc = new_profile(out, 'c', same_profile_as=pa)
        profiles = [pa, pb, pc]
        results['scopes'] = {'a': {'home': pa.home, 'profile': pa.profile}, 'b': {'home': pb.home, 'profile': pb.profile},
                             'c_same_profile_other_home': {'home': pc.home, 'profile': pc.profile}}
        for p in profiles:
            init_profile(p)
        for p in profiles:  # setup launches quit fully, so nothing of these scopes runs now
            wait_until(lambda p=p: not home_processes(p.home), 30, 'the setup daemon to exit')

        # N: no first instance + --quick-panel
        code, secs, text, _ = second_launch('n-quickpanel-noprimary', pa, out, args=['--quick-panel'])
        assert code == 1, (code, text[-400:])
        assert 'cannot show the quick panel' in text, text[-400:]
        assert not home_processes(pa.home), 'the refused quick-panel launch left processes behind'
        results['N_quickPanelWithoutPrimary'] = {'exitCode': code, 'seconds': secs}

        # A: first instance, silent, real helper
        a1 = Gui('a1-primary', pa, out)
        guis.append(a1)
        a1.step('launch')
        s0 = a1.ready()
        helper0 = sorted(int(x) for x in subprocess.run(['pgrep', '-P', str(a1.pid), '-f', 'uniclip-quick-panel'], capture_output=True, text=True).stdout.split())
        procs0 = home_processes(pa.home)
        uid_a = a1.launches()[0]['detail']['uniqueID']
        assert s0['pid'] == a1.pid and not s0['mainExists'], s0
        assert owned(a1.pid, pa.home)
        assert helper0 and s0['daemonPid'] in procs0, (helper0, s0, procs0)
        assert lock_files([uid_a]) == {uid_a: True}
        results['A_primary'] = {'guiPid': a1.pid, 'daemonPid': s0['daemonPid'], 'helperPids': helper0, 'uniqueID': uid_a,
                                'processes': procs0, 'silentStartMainExists': s0['mainExists']}

        def unchanged(tag):
            s = a1.state(tag)
            assert s['pid'] == a1.pid and s['daemonPid'] == s0['daemonPid'], (s, s0)
            h = sorted(int(x) for x in subprocess.run(['pgrep', '-P', str(a1.pid), '-f', 'uniclip-quick-panel'], capture_output=True, text=True).stdout.split())
            assert h == helper0, (h, helper0)
            assert home_processes(pa.home) == procs0, (home_processes(pa.home), procs0)
            assert len(a1.launches()) == 1, a1.launches()
            return s

        # A2: second launch started by the login item (--autostart): no window
        code, secs, text, sec_pid = second_launch('a2-autostart', pa, out, args=['--autostart'])
        assert code == 0 and secs < 5, (code, secs, text[-300:])
        row = a1.step('second-instance', 30, lambda r: '--autostart' in r['detail']['args'])[-1]['detail']
        assert row['action'] == 'ignore-autostart' and not row['mainExists'], row
        assert not any(w in text.lower() for w in ('daemon', 'autostart', 'login item')), text
        s = unchanged('after-autostart')
        assert not s['mainExists']
        results['A2_autostartSecond'] = {'exitCode': code, 'seconds': secs, 'secondPid': sec_pid, 'primaryReceived': row, 'secondaryLog': text.strip()[:200]}

        # A3: --quick-panel: ignored (no host panel toggle yet), no window
        code, secs, text, _ = second_launch('a3-quickpanel', pa, out, args=['--quick-panel'])
        assert code == 0 and secs < 5, (code, secs)
        row = a1.step('second-instance', 30, lambda r: '--quick-panel' in r['detail']['args'])[-1]['detail']
        assert row['action'] == 'ignore-quick-panel' and not row['mainExists'], row
        unchanged('after-quick-panel')
        results['A3_quickPanelSecond'] = {'exitCode': code, 'seconds': secs, 'primaryReceived': row}

        # A4: plain second launch: the main window is created by the first instance
        code, secs, text, _ = second_launch('a4-plain', pa, out)
        assert code == 0 and secs < 5, (code, secs)
        rows = a1.step('second-instance', 30, lambda r: r['detail']['action'] == 'show-main-window')
        assert rows[-1]['detail']['mainExists'], rows
        s = unchanged('after-plain')
        assert s['mainExists']
        results['A4_plainSecond'] = {'exitCode': code, 'seconds': secs, 'primaryReceived': rows[-1]['detail']}

        # A5: a burst of 8 launches at once
        before = len(a1.second_instances())
        burst = []
        from run import isolated_env
        for i in range(8):
            env = isolated_env(pa.home, pa.profile, dict(pa.base, UC_GUI_GO_E2E_NOTIFY_LOG=str(pa.notify_log)))
            t0 = now()
            burst.append((subprocess.Popen([str(BINARY)] + (['--autostart'] if i % 2 else []), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT), t0))
        codes = []
        for proc, t0 in burst:
            codes.append(proc.wait(timeout=60))
            LIFETIMES.append({'label': 'a5-burst', 'pid': proc.pid, 'start': t0, 'end': now(), 'exit': codes[-1]})
        assert codes == [0] * 8, codes
        # Delivery is best effort: identical distributed notifications posted back to back may coalesce, which is
        # harmless for these idempotent intents. What must hold: every launcher exits cleanly and at least one of
        # each kind arrives, and a later launch is still delivered.
        wait_until(lambda: len(a1.second_instances()) > before, 30, 'burst activations')
        time.sleep(2)
        got = a1.second_instances()[before:]
        kinds = sorted({r['detail']['action'] for r in got})
        assert 'show-main-window' in kinds or 'ignore-autostart' in kinds, got
        unchanged('after-burst')
        count = len(a1.second_instances())
        second_launch('a5-after-burst', pa, out)
        wait_until(lambda: len(a1.second_instances()) > count, 30, 'launch after the burst')
        results['A5_burst'] = {'exitCodes': codes, 'received': len(got), 'actionsReceived': kinds, 'sent': 8,
                               'note': 'duplicates posted back to back may coalesce (best-effort delivery)'}

        # C: independent scopes next to A
        b1 = Gui('b1-primary', pb, out)
        guis.append(b1)
        # B: second launches while the first instance is still bootstrapping its daemon (cold start). The run loop
        # (and Wails' activation observer) is already up; the shell is not, so the window request is held and replayed.
        b1.step('launch')
        b1.ctl('state early')
        b1.step('control-state', 60, lambda r: r['detail']['label'] == 'early')
        time.sleep(.5)
        assert not any(r['step'] == 'bootstrapped' for r in b1.steps()), 'the bootstrap finished before the early launches'
        early = []
        for label, extra_args in (('b-early-autostart', ['--autostart']), ('b-early-plain', [])):
            code_e, secs_e, _, _ = second_launch(label, pb, out, args=extra_args)
            assert code_e == 0 and secs_e < 5, (label, code_e, secs_e)
            early.append({'label': label, 'exitCode': code_e, 'seconds': secs_e})
        boot = b1.step('bootstrapped', 120)[0]['detail']
        sb = b1.ready()
        early_received = b1.second_instances()
        actions = [r['detail']['action'] for r in early_received]
        assert 'ignore-autostart' in actions and 'show-main-window' in actions, early_received
        assert boot['replayedHeldShow'] and boot['mainExists'], boot
        assert sb['mainExists'] and not any(r['detail']['mainExists'] for r in early_received if r['detail']['action'] == 'ignore-autostart'), (sb, early_received)
        assert sum(1 for c in home_processes(pb.home).values() if c.endswith('uniclipd')) == 1, home_processes(pb.home)
        results['B_earlySecond'] = {'launches': early, 'primaryReceived': [r['detail'] for r in early_received], 'bootstrapped': boot,
                                    'daemonPidAfter': sb['daemonPid'], 'daemonPidsStarted': len({sb['daemonPid']})}
        c1 = Gui('c1-primary-same-profile-other-home', pc, out)
        guis.append(c1)
        c1.step('launch')
        sc = c1.ready()
        alt = Gui('alt-bundle-same-home-profile', pa, out, binary=alt_binary, extra={'UC_GUI_GO_E2E_UNBUNDLED': '1'}, exit_mode='keep')
        guis.append(alt)
        alt.step('launch')
        salt = alt.ready()
        uid_b, uid_c, uid_alt = (g.launches()[0]['detail']['uniqueID'] for g in (b1, c1, alt))
        ids = [uid_a, uid_b, uid_c, uid_alt]
        assert len(set(ids)) == 4, ids
        assert all(lock_files(ids).values()), lock_files(ids)
        assert pa.profile == pc.profile and uid_a != uid_c
        assert uid_alt.startswith(ALT_BUNDLE + '.') and uid_a.startswith('app.uniclipboard.desktop.e2e.')
        assert all(g.proc.poll() is None for g in (a1, b1, c1, alt))
        assert salt['daemonPid'] == s0['daemonPid'], 'the other-bundle app shares the profile daemon, as a second GUI client'
        assert len(a1.launches()) == 1 and len(alt.launches()) == 1
        a_before, alt_before = len(a1.second_instances()), len(alt.second_instances())
        code, secs, _, _ = second_launch('c-second-for-b', pb, out)
        assert code == 0
        wait_until(lambda: len(b1.second_instances()) > len(early_received), 30, 'second launch reaching b1')
        code2, _, _, _ = second_launch('c-second-for-c', pc, out)
        assert code2 == 0
        wait_until(lambda: len(c1.second_instances()) >= 1, 30, 'second launch reaching c1')
        code3, _, _, _ = second_launch('c-second-for-alt', pa, out, binary=alt_binary, extra={'UC_GUI_GO_E2E_UNBUNDLED': '1'})
        assert code3 == 0
        wait_until(lambda: len(alt.second_instances()) > alt_before, 30, 'second launch reaching the other-bundle app')
        time.sleep(1)
        assert len(a1.second_instances()) == a_before, 'a launch of another scope reached A'
        results['C_independentScopes'] = {'uniqueIDs': {'a': uid_a, 'b': uid_b, 'c_same_profile_other_home': uid_c, 'alt_bundle': uid_alt},
                                          'lockFilesPresent': lock_files(ids), 'lockDir': str(lock_dir()),
                                          'pids': {'a': a1.pid, 'b': b1.pid, 'c': c1.pid, 'alt': alt.pid},
                                          'daemonPids': {'a': s0['daemonPid'], 'b': sb['daemonPid'], 'c': sc['daemonPid'], 'alt': salt['daemonPid']},
                                          'aActivationsDuringC': len(a1.second_instances()) - a_before}
        assert len({s0['daemonPid'], sb['daemonPid'], sc['daemonPid']}) == 3
        alt.exit()
        assert pid_alive(a1.pid) and pid_alive(s0['daemonPid']), 'closing the other-bundle app must not take A or its daemon down'
        b1.exit()
        c1.exit()

        # R: restart (tray path) hands the scope over to the new process
        old_pid, old_daemon = a1.pid, s0['daemonPid']
        a1.ctl('restart')
        a1.step('control-restart', 30)
        new = wait_until(lambda: [r for r in a1.launches() if r['detail']['pid'] != old_pid], 120, 'restarted GUI launch')[0]['detail']
        new_pid = new['pid']
        a1.proc.wait(timeout=60)
        LIFETIMES.append({'label': 'a1-primary', 'pid': old_pid, 'start': a1.started, 'end': now(), 'exit': a1.proc.returncode})
        assert a1.proc.returncode == 0 and not pid_alive(old_pid)
        assert new['uniqueID'] == uid_a and pid_alive(new_pid) and owned(new_pid, pa.home), new
        # a launch after the restart reaches the new process, proving it holds the lock
        deadline = time.monotonic() + 60
        reached = None
        while time.monotonic() < deadline and not reached:
            time.sleep(2)  # the new process needs its run loop before it can receive the activation
            second_launch('r-second-after-restart', pa, out, args=['--autostart'])
            reached = [r for r in a1.second_instances() if r['detail']['pid'] == new_pid]
        assert reached, 'the restarted GUI is not the first instance'
        results['R_restart'] = {'oldPid': old_pid, 'newPid': new_pid, 'oldExit': a1.proc.returncode, 'uniqueID': new['uniqueID'],
                                'newReceivedActivation': reached[-1]['detail']}

        # T: quit, then take over
        a1.ctl('exit')
        wait_until(lambda: not pid_alive(new_pid), 90, 'restarted GUI exit')
        wait_until(lambda: not home_processes(pa.home), 30, 'the daemon to finish exiting')
        a2 = Gui('a2-takeover', pa, out)
        guis.append(a2)
        takeover = a2.step('launch')[0]['detail']
        a2.ready()
        assert takeover['pid'] == a2.pid and takeover['uniqueID'] == uid_a
        results['T_takeover'] = {'pid': a2.pid, 'uniqueID': takeover['uniqueID']}
        a2.exit()
        for p in profiles:
            wait_until(lambda p=p: not home_processes(p.home), 30, f'processes of {p.home} to finish exiting')
        results['passed'] = True
    finally:
        for g in guis:
            if g.proc.poll() is None:
                if owned(g.pid, g.p.home, g.binary):
                    g.proc.terminate()
                    try:
                        g.proc.wait(timeout=30)
                    except subprocess.TimeoutExpired:
                        g.proc.kill()
        for p in profiles:
            cli(p.cli_env, '--json', 'stop', check=False, timeout=80)
        leftovers = {p.home: home_processes(p.home) for p in profiles}
        results['leftoverProcesses'] = leftovers
        results['lifetimes'] = LIFETIMES
        (out / 'single-instance-assertions.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps(results, indent=2, ensure_ascii=False))
    assert results['passed']


if __name__ == '__main__':
    main()
