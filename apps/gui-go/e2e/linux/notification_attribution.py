#!/usr/bin/env python3
"""Attribute a desktop notification of the e2e GUI to the desktop entry the shell chose, on a real GNOME session.

  notification_attribution.py --control FILE --evidence FILE --out DIR [--click-wait 20]

Needs the gtk3,e2e GUI already running with UC_GUI_GO_E2E_CONTROL_FILE=FILE and UC_GUI_GO_EVIDENCE=FILE (the app under test, started by the
driver). It monitors org.freedesktop.Notifications on the session bus, makes the app send one notification through the control verb
`invoke host_notification_send`, and records:
  - the Notify call (app_name, hints) as the daemon received it,
  - which desktop entry the shell created per-application notification settings for (dconf path .../notifications/application/<id>/),
  - every installed desktop entry that claims the app's StartupWMClass (a second claimant changes the attribution),
  - ActionInvoked / NotificationClosed when a person or the driver clicks the banner within --click-wait seconds.
It does not click. The click is the driver's real input; the app-side handling is NOT observable here and is recorded as unknown.
"""
import argparse
import json
import re
import subprocess
import time
from pathlib import Path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--control', type=Path, required=True)
    ap.add_argument('--evidence', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--wm-class', default='uniclipboard')
    ap.add_argument('--click-wait', type=int, default=20)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    raw = args.out / 'notification-monitor.json'
    mon = subprocess.Popen(['busctl', '--user', 'monitor', '--json=short'], stdout=raw.open('w'), stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    label = f'attr-{int(time.time())}'
    nid = 4242
    with args.control.open('a') as f:
        f.write('invoke %s host_notification_send %s\n' % (label, json.dumps({'options': {'id': nid, 'title': 'attribution test', 'body': 'click this banner'}})))
    deadline = time.monotonic() + args.click_wait
    while time.monotonic() < deadline:
        time.sleep(1)
    mon.terminate()
    mon.wait(5)
    notify, dconf_ids, actions = None, set(), []
    for line in raw.read_text().splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        text = json.dumps(m)
        if m.get('member') == 'Notify' and m.get('type') == 'method_call' and notify is None and 'attribution test' in text:
            notify = m.get('payload', {}).get('data')
        for found in re.findall(r'/org/gnome/desktop/notifications/application/([^/\\"]+)/', text):
            dconf_ids.add(found)
        if m.get('member') in ('ActionInvoked', 'NotificationClosed'):
            actions.append({'member': m['member'], 'data': m.get('payload', {}).get('data')})
    claimants = []
    for base in (Path.home() / '.local/share/applications', Path('/usr/share/applications')):
        for d in sorted(base.glob('*.desktop')):
            try:
                body = d.read_text(errors='replace')
            except OSError:
                continue
            if re.search(r'^StartupWMClass=%s$' % re.escape(args.wm_class), body, re.M):
                claimants.append(str(d))
    ack = None
    for line in args.evidence.read_text().splitlines():
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if r.get('step') == f'invoke-{label}':
            ack = r
    result = {'label': label, 'invokeAck': ack, 'notify': notify, 'shellAttributedDesktopIds': sorted(dconf_ids), 'wmClassClaimants': claimants,
              'actions': actions, 'appSideHandling': 'unknown: no observable app-side evidence step for notification clicks'}
    (args.out / 'notification-attribution.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
