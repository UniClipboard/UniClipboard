#!/usr/bin/env python3
"""Update wake + analytics E2E: the Wails SystemDidWake event drives the real scheduler, and the update
lifecycle analytics reach the real daemon.

Real pieces: the e2e GUI build (Wails event loop, scheduler, updater), a real daemon, a local signed feed that
counts requests. Injected pieces, stated plainly: the wake is NSWorkspaceDidWakeNotification posted on the
workspace notification center Wails observes (the machine never sleeps), so Wails' own observer ->
Mac.ApplicationDidWake -> Common.SystemDidWake chain delivers it to the scheduler. A real sleep/wake of the
host is NOT exercised.

Analytics evidence comes from the isolated debug daemon only: `update_diag` lines (written for every captured UI
event whatever the consent) show the GUI posted the event; StdoutSink lines (`uc_observability::analytics`, written
only after the consent gate) show the daemon would have sent it. The debug daemon has no PostHog sink, so nothing
leaves the machine.

Scenarios (cadence 120s, wake guard 10s):
  A fresh wake            right after a check: no feed request
  B stale wake            after the guard: exactly one request
  C fresh burst           5 wakes right after a check: no request
  D stale burst           5 wakes after the guard: exactly one request
  E cadence               the scheduled check still arrives on the (reset) timer, checked last
  F manual + tray         both still check, and count as "last check" for the wake guard
  G auto-check off        a stale wake makes no feed request and no analytics
  H analytics             check_performed manual / scheduled / failure kinds, notification_shown, download_bg
  I consent               usage analytics off: the GUI still posts, the daemon sink sees nothing
  J exit                  the GUI process is gone afterwards
"""
import argparse
import http.server
import json
import os
import platform
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from file_preview_run import PASSPHRASE, cli  # noqa: E402
from run import ROOT, isolated_env, pid_alive, read_steps  # noqa: E402

VERSION = '99.0.0-e2e'
VERSION2 = '99.0.1-e2e'
INTERVAL = 120
GUARD = 10
SOURCE_TARGET = 'update_diag'
SINK_TARGET = 'uc_observability::analytics'


class Feed:
    """A local update feed that counts manifest and artifact requests and can misbehave on demand."""

    def __init__(self, directory):
        self.manifests, self.artifacts, self.mode = [], [], 'ok'
        feed = self

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *a, **k):
                super().__init__(*a, directory=str(directory), **k)

            def log_message(self, *a):
                pass

            def do_GET(self):
                if self.path.endswith('.json'):
                    feed.manifests.append(time.monotonic())
                    if feed.mode == 'http500':
                        return self.send_error(500)
                    if feed.mode == 'badjson':
                        self.send_response(200)
                        self.end_headers()
                        return self.wfile.write(b'{not json')
                    if feed.mode == 'drop':
                        return self.connection.close()
                else:
                    feed.artifacts.append(time.monotonic())
                super().do_GET()

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = f'http://127.0.0.1:{self.server.server_address[1]}'


def daemon_lines(log_dir):
    rows = []
    for path in sorted(log_dir.glob('uniclipboard-daemon.json*')):
        for line in path.read_text(errors='replace').splitlines():
            try:
                rows.append(json.loads(line))
            except ValueError:
                pass
    return rows


def diag(rows, message):
    """update_diag lines: the GUI's events as the daemon received them."""
    return [r for r in rows if r.get('target') == SOURCE_TARGET and r.get('message') == message]


def sink(rows):
    """StdoutSink lines: events that passed the consent gate."""
    return [r for r in rows if r.get('target') == SINK_TARGET]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-wake-'))
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / 'update-wake-native.jsonl'
    evidence.write_text('')
    control = work / 'control.txt'
    control.write_text('')
    gui_log = out / 'update-wake-gui.log'
    feed_dir = work / 'feed'
    feed_dir.mkdir()
    artifact = feed_dir / 'update.app.tar.gz'
    artifact.write_bytes(b'not a real bundle: the scheduler never installs')
    subprocess.run(['go', 'run', './e2e/updatetool', str(artifact), str(feed_dir)], cwd=ROOT / 'apps/gui-go', check=True)
    feed = Feed(feed_dir)
    arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
    (feed_dir / 'good.json').write_text(json.dumps({
        'version': VERSION, 'notes': 'E2E wake notes', 'pub_date': '2026-10-06T00:00:00Z',
        'platforms': {f'darwin-{arch}-app': {'url': f'{feed.base}/update.app.tar.gz', 'signature': (feed_dir / 'good.sig.b64').read_text()}},
    }))
    path = str(ROOT / 'target/debug') + ':' + os.environ['PATH']
    cli_env = isolated_env(home, profile, {'PATH': path})
    gui_env = isolated_env(home, profile, {
        'PATH': path, 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1', 'UC_GUI_GO_EVIDENCE': str(evidence),
        'UC_GUI_GO_E2E_PHASE': 'wake', 'UC_GUI_GO_E2E_CONTROL_FILE': str(control), 'UC_GUI_GO_EXIT_MODE': 'full',
        'UC_UPDATE_PUBKEY': (feed_dir / 'pubkey.b64').read_text(), 'UC_UPDATE_ENDPOINT': f'{feed.base}/good.json',
        'UC_UPDATE_SCHEDULER_INTERVAL': f'{INTERVAL}s', 'UC_UPDATE_WAKE_MIN_RECHECK': f'{GUARD}s'})
    log_dir = Path(home) / 'Library/Logs' / ('app.uniclipboard.desktop-' + profile)
    results = {'home': home, 'profile': profile, 'version': VERSION, 'cadenceSeconds': INTERVAL, 'wakeGuardSeconds': GUARD, 'passed': False}
    proc = None

    def command(line, step, timeout=60):
        with control.open('a') as f:
            f.write(line + '\n')
        return wait_nth(step, timeout)

    def wait_nth(step, timeout):
        # the n-th command of a kind is answered by the n-th matching step
        want = sum(1 for _ in kinds if _ == step) + 1
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = [r for r in read_steps(evidence, 0) if r['step'] == step]
            if len(rows) >= want:
                return rows[want - 1]
            time.sleep(.1)
        raise RuntimeError(f'timeout waiting for {step} #{want}')

    kinds = []
    base = [0]  # feed requests before the measured window (the updater page makes a check of its own)

    def send(line, step, timeout=60, must_succeed=True):
        row = command(line, step, timeout)
        kinds.append(step)
        assert row['ok'] or not must_succeed, f'{line}: {row}'
        return row

    def gui_wakes():
        return gui_log.read_text(errors='replace').count('update scheduler: system wake')

    def gui_log_count(text):
        return gui_log.read_text(errors='replace').count(text)

    def settle(seconds):
        time.sleep(seconds)

    def expect_requests(n, within=0, hold=0):
        """Wait until the feed has seen n manifest requests, then (hold) confirm no further one arrives."""
        deadline = time.monotonic() + within
        while len(feed.manifests) - base[0] < n and time.monotonic() < deadline:
            time.sleep(.1)
        settle(hold)
        assert len(feed.manifests) - base[0] == n, f'expected {n} feed requests, saw {len(feed.manifests) - base[0]}'

    def wait_wakes(n):
        deadline = time.monotonic() + 10
        while gui_wakes() < n and time.monotonic() < deadline:
            time.sleep(.1)
        assert gui_wakes() >= n, f'Wails delivered {gui_wakes()} of {n} wake events to the listener'

    try:
        cli(cli_env, 'space', 'init', '--passphrase', PASSPHRASE, '--device-name', 'wake-a')
        for _ in range(3):
            if cli(cli_env, 'start', check=False).returncode == 0:
                break
            time.sleep(3)
        else:
            raise RuntimeError('daemon start failed three times')
        binary = ROOT / 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go'
        proc = subprocess.Popen([str(binary)], env=gui_env, stdout=gui_log.open('w'), stderr=subprocess.STDOUT)
        results['guiPid'] = proc.pid

        # The first scheduled check runs by itself once setup is complete and opens the updater window, whose
        # page checks once by itself; close it and measure from there.
        deadline = time.monotonic() + 90
        while not feed.manifests and time.monotonic() < deadline:
            time.sleep(.1)
        assert feed.manifests, 'no scheduled check after setup'
        time.sleep(3)
        send('close-updater', 'control-close-updater')
        base[0] = len(feed.manifests)
        results['startupRequests'] = base[0]
        t1 = feed.manifests[-1]

        # A: fresh wake
        send('wake 1', 'control-wake')
        wait_wakes(1)
        expect_requests(0, hold=3)
        assert gui_log_count('wake skipped') == 1
        results['A_freshWakeNoRequest'] = True

        # B: stale wake -> exactly one request
        time.sleep(max(0, t1 + GUARD + 1 - time.monotonic()))
        assert time.monotonic() - t1 < INTERVAL - 4, 'timing: the cadence timer would fire first'
        send('wake 1', 'control-wake')
        wait_wakes(2)
        expect_requests(1, within=10, hold=2)
        t2 = feed.manifests[base[0]]
        assert gui_log_count('wake after') == 1
        results['B_staleWakeOneRequest'] = True

        # C: fresh burst -> nothing
        send('wake 5', 'control-wake')
        wait_wakes(7)
        expect_requests(1, hold=3)
        results['C_freshBurstNoRequest'] = True

        # D: stale burst -> exactly one
        time.sleep(max(0, t2 + GUARD + 1.5 - time.monotonic()))
        assert time.monotonic() - t2 < INTERVAL - 8
        send('wake 5', 'control-wake')
        wait_wakes(12)
        expect_requests(2, within=10, hold=5)
        t3 = feed.manifests[base[0] + 1]
        results['D_staleBurstOneRequest'] = True

        # F: manual and tray checks still work and refresh the guard
        send('check', 'control-check')
        expect_requests(3, within=10, hold=1)
        send('tray-check', 'control-tray-check')
        expect_requests(4, within=10, hold=1)
        # The tray check re-opened the updater window (announced version: the user asked, so it opens silently)
        # and its page checks once by itself; close it and leave that request out of the window.
        time.sleep(6)
        send('close-updater', 'control-close-updater')
        base[0] = len(feed.manifests) - 4
        assert time.monotonic() - feed.manifests[-1] < GUARD - 1, 'timing: the last check is already stale'
        send('wake 1', 'control-wake')
        wait_wakes(13)
        expect_requests(4, hold=3)
        results['F_manualTrayCheckAndGuard'] = True

        # G: auto-check disabled -> a stale wake makes no request and no analytics
        send('setting autoCheckUpdate off', 'control-setting')
        before = len(diag(daemon_lines(log_dir), 'update check performed'))
        time.sleep(GUARD + 1.5)
        send('wake 1', 'control-wake')
        wait_wakes(14)
        expect_requests(4, hold=4)
        assert len(diag(daemon_lines(log_dir), 'update check performed')) == before
        send('setting autoCheckUpdate on', 'control-setting')
        results['G_autoCheckOffNoRequestNoAnalytics'] = True

        # H: failure kinds through the manual command (the wire value is what the daemon logged)
        failures = {}
        for mode, expect in (('http500', 'HttpError'), ('badjson', 'ParseError'), ('drop', 'Network')):
            feed.mode = mode
            n = len(feed.manifests)
            cmd_rows = len(diag(daemon_lines(log_dir), 'update check performed'))
            send('check', 'control-check', must_succeed=False)  # the command reports the failure
            deadline = time.monotonic() + 10
            while len(diag(daemon_lines(log_dir), 'update check performed')) == cmd_rows and time.monotonic() < deadline:
                time.sleep(.2)
            row = diag(daemon_lines(log_dir), 'update check performed')[-1]
            failures[mode] = {'outcome': row['outcome'], 'failure_kind': row['failure_kind'], 'source': row['source']}
            assert row['outcome'] == 'Failed' and expect in row['failure_kind'], (mode, row)
            assert len(feed.manifests) == n + 1
        feed.mode = 'ok'
        results['H_failureKinds'] = failures

        # H2a: the release is already downloaded and verified (the first scheduled check downloaded it):
        # auto-download is refused as a precondition, so no download event is reported, only the check.
        send('setting autoDownloadUpdate on', 'control-setting')
        time.sleep(GUARD + 1.5)
        mark = len(daemon_lines(log_dir))
        art0 = len(feed.artifacts)
        send('wake 1', 'control-wake')
        wait_wakes(15)
        settle(3)
        order = [(r['message'], r.get('action'), r.get('outcome'), r.get('source'))
                 for r in daemon_lines(log_dir)[mark:] if r.get('target') == SOURCE_TARGET]
        results['H2a_alreadyDownloadedEvents'] = order
        assert len(feed.artifacts) == art0, 'a verified release was downloaded again'
        assert [o[0] for o in order] == ['update check performed'] and order[0][3] == 'Scheduled', order

        # H2b: a newer release arrives; the scheduled iteration downloads it and reports started, succeeded, then
        # the check (side effects first). The announcement stays within the prompt cooldown, so no new
        # notification_shown is expected.
        (feed_dir / 'good.json').write_text(json.dumps({
            'version': VERSION2, 'notes': 'E2E wake notes 2', 'pub_date': '2026-10-07T00:00:00Z',
            'platforms': {f'darwin-{arch}-app': {'url': f'{feed.base}/update.app.tar.gz', 'signature': (feed_dir / 'good.sig.b64').read_text()}},
        }))
        time.sleep(GUARD + 1.5)
        mark = len(daemon_lines(log_dir))
        art0 = len(feed.artifacts)
        send('wake 1', 'control-wake')
        wait_wakes(16)
        deadline = time.monotonic() + 15
        while len(feed.artifacts) == art0 and time.monotonic() < deadline:
            time.sleep(.2)
        assert len(feed.artifacts) > art0, 'auto-download did not fetch the new artifact'
        t_h2 = feed.manifests[-1]
        settle(2)
        order = [(r['message'], r.get('action'), r.get('outcome'), r.get('source'))
                 for r in daemon_lines(log_dir)[mark:] if r.get('target') == SOURCE_TARGET]
        results['H2b_scheduledDownloadOrder'] = order
        assert [o[0] for o in order] == ['update action invoked', 'update action invoked', 'update check performed'], order
        assert [o[2] for o in order] == ['Started', 'Succeeded', 'Available'] and order[2][3] == 'Scheduled', order
        send('setting autoDownloadUpdate off', 'control-setting')

        # I: consent. Enabled: both the GUI post and the sink line; disabled: the GUI still posts, the sink sees nothing.
        rows = daemon_lines(log_dir)
        diag_on, sink_on = len(diag(rows, 'update check performed')), len(sink(rows))
        send('check', 'control-check')
        settle(2)
        rows = daemon_lines(log_dir)
        assert len(diag(rows, 'update check performed')) == diag_on + 1
        assert len(sink(rows)) > sink_on, 'consent on: the daemon sink produced no analytics line'
        sample = [r for r in sink(rows)][-1]
        results['I_sinkSample'] = sample
        send('setting usageAnalyticsEnabled off', 'control-setting')
        diag_off, sink_off = len(diag(rows, 'update check performed')), len(sink(daemon_lines(log_dir)))
        send('check', 'control-check')
        settle(2)
        rows = daemon_lines(log_dir)
        assert len(diag(rows, 'update check performed')) == diag_off + 1, 'the GUI did not post while consent is off'
        assert len(sink(rows)) == sink_off, 'consent off: the daemon sink still received an event'
        results['I_consentOffZeroSink'] = {'guiPosted': True, 'sinkLinesBefore': sink_off, 'sinkLinesAfter': len(sink(rows))}
        send('setting usageAnalyticsEnabled on', 'control-setting')

        rows = daemon_lines(log_dir)
        shown = diag(rows, 'update notification shown')
        assert len(shown) == 1, f'notification_shown reported {len(shown)} times (expected once: the first announcement)'
        results['H_notificationShownOnce'] = True
        results['analyticsLineTotals'] = {'update_diag': len([r for r in rows if r.get('target') == SOURCE_TARGET]), 'sink': len(sink(rows))}

        # E: the cadence timer was reset by the last wake check and still fires on schedule, with no help
        t_reset = t_h2
        n = len(feed.manifests)
        deadline = time.monotonic() + INTERVAL + 20
        while len(feed.manifests) == n and time.monotonic() < deadline:
            time.sleep(.5)
        assert len(feed.manifests) == n + 1, 'the scheduled check never arrived'
        gap = feed.manifests[n] - t_reset
        assert INTERVAL - 6 <= gap <= INTERVAL + 10, f'scheduled check came {gap:.1f}s after the last wake check'
        results['E_cadenceGapSeconds'] = round(gap, 1)

        # J: exit
        send('exit', 'control-exit')
        assert proc.wait(timeout=60) == 0
        results['J_guiExitedCode'] = 0
        results['passed'] = True
    finally:
        if proc and proc.poll() is None:
            if not results['passed']:
                # Keep the cause of a hang: SIGQUIT makes the Go runtime dump every goroutine into the GUI log.
                # Only the Popen child of this run is signalled, after checking the parent link.
                ppid = subprocess.run(['ps', '-o', 'ppid=', '-p', str(proc.pid)], capture_output=True, text=True).stdout.strip()
                if ppid == str(os.getpid()):
                    proc.send_signal(3)
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
        results['feedArtifactRequests'] = len(feed.artifacts)
        if log_dir.exists():
            for p in log_dir.glob('uniclipboard-daemon.json*'):
                (out / ('update-wake-daemon-' + p.name)).write_text(p.read_text(errors='replace'))
        (out / 'update-wake-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed'] and not results['guiPidAliveAfter']


if __name__ == '__main__':
    main()
