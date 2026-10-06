#!/usr/bin/env python3
"""App Nap compensation E2E: the macOS NSBackgroundActivityScheduler drives the real update scheduler in a
quiet, idle, Accessory GUI process.

Real pieces: the e2e GUI build (Wails event loop, scheduler, the cgo NSBackgroundActivityScheduler), a real
daemon, a local signed feed that counts requests. NOTHING is injected into the scheduler: no system-wake
posting, no control-file wake. Every wake in this run is a callback the operating system made on its own
background queue (the GUI log names the queue and the thread; `(system-did-wake)` lines must not appear).

What this proves: the system calls the activity back at (about) its interval while the process is idle and
windowless, each callback reaches the scheduler wake path, and the wake guard, auto-check switch and exit
cleanup behave. What it does NOT prove: that the process was actually in App Nap (no unprivileged API reports
it). The timer probe shows how late ordinary 1s Go sleeps run during the idle period; that is the effect of
timer coalescing, not the nap state. The production interval (6h) is not exercised: the e2e build shortens it
through UC_UPDATE_BACKGROUND_ACTIVITY_INTERVAL, which does not exist in the production build.

Phases (interval I, wake guard G < I, the scheduler's own cadence is 30 minutes and never fires):
  A idle          no commands at all; >= MIN_FIRES callbacks; every one is classified by the guard
  B auto-check    autoCheckUpdate off: a callback past the guard checks nothing; on again afterwards
  C guard         a manual check shortly before the next callback makes that callback skip
  D exit          exit stops the activity, no callback is logged afterwards, no process is left
Invariant for every callback (all phases): last-check age < G -> skipped, no feed request; otherwise -> exactly
one scheduled check (one feed request, none when auto-check is off).
"""
import argparse
import datetime
import json
import os
import platform
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, pid_alive, read_steps  # noqa: E402
from update_wake_run import Feed  # noqa: E402

VERSION = '99.0.0-e2e'
LOG_TS = re.compile(r'^(\d{4}/\d\d/\d\d \d\d:\d\d:\d\d) (.*)$')
CALLBACK = re.compile(r'background activity callback \(mainThread=(\w+), deferred=(\w+), queue=(.*)\)')
SKIPPED = re.compile(r'wake skipped, last check (\S+) ago \(background-activity\)')
CHECKING = re.compile(r'wake after (\S+), checking \(background-activity\)')


def parse_log(path):
    rows = []
    for line in path.read_text(errors='replace').splitlines():
        m = LOG_TS.match(line)
        if m:
            rows.append((datetime.datetime.strptime(m.group(1), '%Y/%m/%d %H:%M:%S').replace(tzinfo=datetime.timezone.utc).timestamp(), m.group(2)))
    return rows


def parse_duration(text):
    total = 0.0
    for value, unit in re.findall(r'([\d.]+)(h|m|s)', text):
        total += float(value) * {'h': 3600, 'm': 60, 's': 1}[unit]
    return total


def callbacks(rows):
    """Callbacks with the scheduler's decision that followed each (the next scheduler line)."""
    out = []
    for i, (ts, msg) in enumerate(rows):
        m = CALLBACK.search(msg)
        if not m:
            continue
        item = {'at': ts, 'mainThread': m.group(1) == 'true', 'deferred': m.group(2) == 'true', 'queue': m.group(3), 'decision': None}
        if not item['deferred']:
            for _, nxt in rows[i + 1:i + 6]:
                s, c = SKIPPED.search(nxt), CHECKING.search(nxt)
                if s or c:
                    item['decision'] = 'skipped' if s else 'checking'
                    item['sinceSeconds'] = parse_duration((s or c).group(1))
                    break
        out.append(item)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--interval', type=int, default=90, help='background activity interval, seconds')
    parser.add_argument('--guard', type=int, default=60, help='wake guard (min seconds since the last check), seconds')
    parser.add_argument('--min-fires', type=int, default=4, help='callbacks phase A must observe')
    parser.add_argument('--timeout-factor', type=float, default=4, help='phase A waits this many intervals per callback at most')
    args = parser.parse_args()
    interval, guard = args.interval, args.guard
    assert guard < interval, 'the guard must be shorter than the interval or every callback would skip'
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-appnap-'))
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'app-nap-native.jsonl'
    evidence.write_text('')
    control = work / 'control.txt'
    control.write_text('')
    gui_log = out / 'app-nap-gui.log'
    feed_dir = work / 'feed'
    feed_dir.mkdir()
    artifact = feed_dir / 'update.app.tar.gz'
    artifact.write_bytes(b'not a real bundle: the scheduler never installs')
    subprocess.run(['go', 'run', './e2e/updatetool', str(artifact), str(feed_dir)], cwd=ROOT / 'apps/gui-go', check=True)
    feed = Feed(feed_dir)
    arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    (feed_dir / 'good.json').write_text(json.dumps({
        'version': VERSION, 'notes': 'E2E app nap notes', 'pub_date': '2026-10-06T00:00:00Z',
        'platforms': {f'darwin-{arch}-app': {'url': f'{feed.base}/update.app.tar.gz', 'signature': (feed_dir / 'good.sig.b64').read_text()}},
    }))
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {
        'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
        'UC_GUI_GO_E2E_PHASE': 'wake', 'UC_GUI_GO_E2E_CONTROL_FILE': str(control), 'UC_GUI_GO_EXIT_MODE': 'full',
        'UC_GUI_GO_E2E_TIMER_PROBE': '1', 'TZ': 'UTC',  # the log timestamps are read back as UTC
        'UC_UPDATE_PUBKEY': (feed_dir / 'pubkey.b64').read_text(), 'UC_UPDATE_ENDPOINT': f'{feed.base}/good.json',
        'UC_UPDATE_SCHEDULER_INTERVAL': '1800s', 'UC_UPDATE_WAKE_MIN_RECHECK': f'{guard}s',
        'UC_UPDATE_BACKGROUND_ACTIVITY_INTERVAL': f'{interval}s'})
    log_dir = Path(home) / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile)
    mono_to_wall = time.time() - time.monotonic()
    results = {'home': home, 'profile': profile, 'activityIntervalSeconds': interval, 'wakeGuardSeconds': guard,
               'schedulerCadenceSeconds': 1800, 'passed': False}
    proc = None
    kinds = []

    def send(line, step, timeout=60):
        want = kinds.count(step) + 1
        with control.open('a') as f:
            f.write(line + '\n')
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(evidence, 0) if r['step'] == step]
            if len(rows) >= want:
                kinds.append(step)
                assert rows[want - 1]['ok'], f'{line}: {rows[want - 1]}'
                return rows[want - 1]
            time.sleep(.1)
        raise RuntimeError(f'timeout waiting for {step}')

    def fires():
        return callbacks(parse_log(gui_log))

    def wait_fires(n, timeout):
        deadline = time.monotonic() + timeout
        while len(fires()) < n and time.monotonic() < deadline:
            time.sleep(1)
        got = fires()
        assert len(got) >= n, f'only {len(got)} of {n} system callbacks arrived within {timeout:.0f}s'
        return got

    def wall(mono):
        return mono + mono_to_wall

    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'appnap-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=gui_log.open('w'), stderr=subprocess.STDOUT)
        results['guiPid'] = proc.pid
        results['guiStartedAt'] = time.time()
        # the first scheduled check opens the updater window, whose page checks once more; close it, then stay idle
        deadline = time.monotonic() + 90
        while not feed.manifests and time.monotonic() < deadline:
            time.sleep(.1)
        assert feed.manifests, 'no scheduled check after setup'
        time.sleep(3)
        send('close-updater', 'control-close-updater')
        base = len(feed.manifests)
        results['startupFeedRequests'] = base
        # the activity is scheduled when the scheduler starts; its first firing is one interval later
        text = gui_log.read_text(errors='replace')
        assert 'background activity scheduled every' in text, 'the background activity was never scheduled'
        results['idleStartedAt'] = time.time()

        # A: idle, no commands
        got = wait_fires(args.min_fires, interval * args.timeout_factor * args.min_fires)
        time.sleep(4)  # let the last decision and its request land
        got = fires()
        results['A_callbacks'] = got
        assert not any(c['mainThread'] for c in got), 'a callback ran on the main thread'
        assert 'update scheduler: system wake' not in gui_log.read_text(errors='replace'), 'a system wake was delivered: not an isolated run'
        for c in got:
            if c['deferred']:
                continue
            assert c['decision'], f'callback with no scheduler decision: {c}'
            assert (c['decision'] == 'skipped') == (c['sinceSeconds'] < guard), f'guard invariant broken: {c}'
        checked = [c for c in got if c['decision'] == 'checking']
        requests = len(feed.manifests) - base
        results['A_feedRequests'] = requests
        results['A_checkingCallbacks'] = len(checked)
        assert requests == len(checked), f'{requests} feed requests for {len(checked)} checking callbacks'
        assert checked, 'no callback passed the guard'
        gaps = [round(b['at'] - a['at'], 1) for a, b in zip(got, got[1:])]
        results['A_callbackGapsSeconds'] = gaps
        first_gap = got[0]['at'] - results['idleStartedAt']
        results['A_firstCallbackAfterIdleStartSeconds'] = round(first_gap, 1)
        results['A_feedRequestWallTimes'] = [round(wall(m), 1) for m in feed.manifests[base:]]

        # B: auto-check off -> a callback past the guard checks nothing
        send('setting autoCheckUpdate off', 'control-setting')
        n_cb, n_req = len(fires()), len(feed.manifests)
        off_at = time.time()
        got = wait_fires(n_cb + 1, interval * 4)
        time.sleep(4)
        # the callback may have been skipped by the guard (the setting call happens soon after a check); wait for one that passes it
        deadline = time.monotonic() + interval * 4
        while not [c for c in fires()[n_cb:] if c['decision'] == 'checking'] and time.monotonic() < deadline:
            time.sleep(2)
        new = fires()[n_cb:]
        results['B_callbacksWhileOff'] = new
        assert [c for c in new if c['decision'] == 'checking'], 'no callback passed the guard while auto-check was off'
        assert len(feed.manifests) == n_req, 'a feed request was made while auto-check was off'
        results['B_autoCheckOff'] = {'offAt': off_at, 'onAt': time.time(), 'feedRequestsBefore': n_req, 'feedRequestsAfter': len(feed.manifests),
                                     'checkingCallbacksInWindow': len([c for c in new if c['decision'] == 'checking']),
                                     'note': 'decision=checking is only the wake guard result; the switch is read inside the check, which is idle'}
        send('setting autoCheckUpdate on', 'control-setting')

        # C: a manual check ~guard-10s before the next callback makes that callback skip
        n_cb = len(fires())
        last = fires()[-1]['at']
        target = last + interval - (guard - 15)  # the manual check lands guard-15s before the expected firing
        time.sleep(max(0, target - time.time()))
        n_req = len(feed.manifests)
        send('check', 'control-check')
        assert len(feed.manifests) == n_req + 1
        t_check = time.time()
        got = wait_fires(n_cb + 1, interval * 3)
        time.sleep(4)
        nxt = fires()[n_cb]
        results['C_manualCheckAt'] = round(t_check, 1)
        results['C_nextCallback'] = nxt
        results['C_callbackAfterManualCheckSeconds'] = round(nxt['at'] - t_check, 1)
        expect = 'skipped' if nxt['at'] - t_check < guard - 2 else None
        if expect:
            assert nxt['decision'] == 'skipped', nxt
            assert len(feed.manifests) == n_req + 1, 'a callback inside the guard window made a feed request'
            results['C_guardSkipObserved'] = True
        else:
            results['C_guardSkipObserved'] = False  # the system fired late: not a guard scenario, the invariant still holds
        for c in fires():
            if not c['deferred']:
                assert (c['decision'] == 'skipped') == (c['sinceSeconds'] < guard), c

        # timer probe (idle effect of App Nap coalescing)
        probes = [m for _, m in parse_log(gui_log) if m.startswith('timer probe:')]
        results['timerProbeWindows'] = probes

        # D: exit
        stopped_before = 'background activity stopped' in gui_log.read_text(errors='replace')
        assert not stopped_before
        send('exit', 'control-exit')
        assert proc.wait(timeout=60) == 0
        results['D_guiExitedCode'] = 0
        text = gui_log.read_text(errors='replace')
        assert 'background activity stopped' in text, 'shutdown did not invalidate the activity'
        results['D_activityInvalidatedLogged'] = True
        exit_at = max(ts for ts, m in parse_log(gui_log) if 'background activity stopped' in m)
        results['D_callbacksAfterStop'] = len([c for c in fires() if c['at'] > exit_at + 1])
        assert results['D_callbacksAfterStop'] == 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            if not results['passed']:
                ppid = subprocess.run(['ps', '-o', 'ppid=', '-p', str(proc.pid)], capture_output=True, text=True).stdout.strip()
                if ppid == str(os.getpid()):
                    proc.send_signal(3)  # SIGQUIT: the Go runtime dumps goroutines into the log
                    try:
                        proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        pass
        if proc and proc.poll() is None:
            proc.terminate()
            proc.wait(timeout=20)
        feed.server.shutdown()
        if proc:
            results['guiPidAliveAfter'] = pid_alive(proc.pid)
        stop = cli(cli_env, '--json', 'stop', check=False, timeout=80)
        results['cleanupCLIExit'] = stop.returncode
        results['feedManifestRequests'] = len(feed.manifests)
        if log_dir.exists():
            for p in log_dir.glob('uniclipboard-daemon.json*'):
                (out / ('app-nap-daemon-' + p.name)).write_text(p.read_text(errors='replace'))
        results['verdict'] = {
            'proven': 'the operating system itself calls the NSBackgroundActivityScheduler back (xpc activity queue, not the main thread) in an idle windowless Accessory process, each callback reaches the one existing wake path, guard and auto-check switch apply, exit invalidates it',
            'notProven': 'that the process ever entered App Nap or that its timers were suspended: the 1s timer probe showed only a few ms of lateness, so passed=true is NOT a claim that a suspension was compensated',
            'nextAcceptance': 'a long idle run (tens of minutes, display asleep or another app frontmost, ideally on battery) where the timer probe shows real lateness or Activity Monitor App Nap column reads Yes for this PID, with the callbacks still arriving; then the 6h production interval without the e2e seam',
        }
        (out / 'app-nap-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed'] and not results['guiPidAliveAfter']


if __name__ == '__main__':
    main()
