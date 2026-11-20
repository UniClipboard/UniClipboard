#!/usr/bin/env python3
"""Deliver physical guest input while the canonical Desktop runner owns acceptance."""
import argparse
import hashlib
import json
import re
from pathlib import Path, PurePosixPath
import shlex
import subprocess
import time
import uuid


def main():
    adapter_sha256 = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', required=True)
    parser.add_argument('--guest', required=True, type=uuid.UUID)
    parser.add_argument('--workspace', required=True, type=PurePosixPath)
    parser.add_argument('--case', default='system-clipboard')
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    if not args.workspace.is_absolute() or '..' in args.workspace.parts or not re.fullmatch(r'[A-Za-z0-9_/.-]+', str(args.workspace)):
        parser.error('workspace must be an absolute owned guest directory')
    if not re.fullmatch(r'[a-z][a-z0-9-]*', args.case):
        parser.error('case must be a catalogue identifier')
    if args.host.startswith('-'):
        parser.error('host must be an SSH alias')
    if args.out.exists():
        parser.error('evidence directory must be new')
    args.out.mkdir(parents=True, mode=0o700)
    guest = str(args.guest).upper()
    control = args.workspace / ('control-' + uuid.uuid4().hex)

    def remote(command, **kwargs):
        return subprocess.run(['ssh', '-o', 'BatchMode=yes', args.host, shlex.join(command)],
                              check=True, text=True, capture_output=True, timeout=15, **kwargs).stdout

    # Verify the VM ID exists on this host before it can receive input.
    status = subprocess.run(['osascript'], input=f'tell application "UTM" to get status of virtual machine id "{guest}"',
                            text=True, check=True, capture_output=True, timeout=15).stdout.strip()
    if status != 'started':
        raise RuntimeError('requested UTM guest is not running')
    configuration = subprocess.run(['osascript'], input=f'tell application "UTM" to get configuration of virtual machine id "{guest}"',
                                   text=True, check=True, capture_output=True, timeout=15).stdout
    vm_addresses = set(re.findall(r'\baddress:([0-9a-fA-F]{2}(?::[0-9a-fA-F]{2}){5})', configuration.lower()))
    guest_addresses = set(remote(['sh', '-c', 'cat /sys/class/net/*/address']).lower().splitlines())
    verified_addresses = sorted(vm_addresses & guest_addresses)
    if not verified_addresses:
        raise RuntimeError('SSH guest network identity does not match the requested UTM VM')
    remote(['mkdir', '-m', '700', str(control)])
    values = dict(line.split('=', 1) for line in remote(['systemctl', '--user', 'show-environment']).splitlines() if '=' in line)
    allowed = ('DISPLAY', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'DBUS_SESSION_BUS_ADDRESS')
    environment = [name + '=' + values[name] for name in allowed if name in values]
    environment += ['PATH=' + str(args.workspace / 'go/bin') + ':/usr/local/bin:/usr/bin:/bin',
                    'GOCACHE=' + str(args.workspace / 'go-cache'),
                    'GOMODCACHE=' + str(args.workspace / 'go-mod'),
                    'GOPATH=' + str(args.workspace / 'gopath'), 'GOPROXY=off']
    command = ['env', *environment, 'make', '-C', str(args.workspace / 'source'), 'desktop-e2e',
               'CASE=' + args.case, 'GUEST=' + guest, 'CONTROL=' + str(control)]
    events, seen = [], set()
    process, code = None, 1
    try:
        with (args.out / 'canonical.log').open('w') as log:
            process = subprocess.Popen(['ssh', '-o', 'BatchMode=yes', args.host, shlex.join(command)], stdout=log, stderr=subprocess.STDOUT)
            while process.poll() is None:
                try:
                    request = json.loads(remote(['cat', str(control / 'request.json')]))
                except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError):
                    time.sleep(.3)
                    continue
                identity = request.get('id')
                if not identity or identity in seen:
                    time.sleep(.3)
                    continue
                seen.add(identity)  # Record before delivery. A timeout must never retry input.
                ok = False
                keys = {'Enter': '28, 156', 'Escape': '1, 129', 'Alt+F4': '56, 62, 190, 184', 'Ctrl+C': '29, 46, 174, 157',
                        'Ctrl+F': '29, 33, 161, 157', 'Ctrl+V': '29, 47, 175, 157', 'Ctrl+S': '29, 31, 159, 157'}
                if request.get('guest', '').upper() == guest and request.get('action') == 'key' and request.get('value') in keys:
                    codes = keys[request['value']]
                    script = f'tell application "UTM" to input scan code (virtual machine id "{guest}") codes {{{codes}}}'
                    try:
                        result = subprocess.run(['osascript'], input=script, text=True, capture_output=True, timeout=15)
                        ok = result.returncode == 0
                    except subprocess.TimeoutExpired:
                        pass  # Unknown delivery is acknowledged as failure and is never replayed.
                events.append(dict(at=time.time(), request=request, ok=ok))
                reply = json.dumps(dict(id=identity, ok=ok))
                try:
                    remote(['sh', '-c', 'umask 077; cat > ' + shlex.quote(str(control / 'ack.json'))], input=reply)
                except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                    events[-1]['acknowledgement'] = 'unknown'
            code = process.wait()
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        cleanup = 'PASS'
        try:
            remote(['rm', '-rf', '--', str(control)])
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            cleanup = 'FAIL'
        source_unchanged = hashlib.sha256(Path(__file__).read_bytes()).hexdigest() == adapter_sha256
        status = 'PASS' if code == 0 and cleanup == 'PASS' and source_unchanged else 'FAIL'
        (args.out / 'control.json').write_text(json.dumps(dict(status=status, guest=guest, events=events,
            verified_mac_addresses=verified_addresses, cleanup=cleanup,
            script_sha256=adapter_sha256, script_unchanged=source_unchanged), indent=2) + '\n')
    print((args.out / 'canonical.log').read_text())
    return 0 if status == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
