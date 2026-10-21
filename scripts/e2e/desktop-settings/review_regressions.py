#!/usr/bin/env python3
"""Real-daemon regressions for deferred migration warnings and cleanup failures."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import sys
import time

from adoption import Profile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--daemon', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    profiles = []
    result = {'passed': False, 'daemonSha256': hashlib.sha256(args.daemon.read_bytes()).hexdigest(),
              'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()}
    try:
        p = Profile(out, 'initial-warnings', args.daemon.resolve(), args.cli.resolve())
        profiles.append(p)
        p.legacy.write_text(json.dumps({'schema_version': 3, 'general': {'debug_mode': True, 'theme': 'invalid', 'startup_mode': 'invalid'}}))
        p.product.write_text(json.dumps({'version': 1, 'general': {'theme': 'dark'},
            'import': {'fieldSet': 0, 'userWritten': ['general.theme']}}))
        p.start()
        status = p.api('GET', '/diagnostics/debug')
        assert status['debugMode'] and status['effectiveLogProfile'] == 'debug', status
        p.stop()
        logs = (p.root / 'daemon-0.log').read_text()
        assert 'desktop_settings_import_invalid_field' in logs, 'initial invalid-field warning lost'
        assert 'desktop_settings_import_kept_user_value' in logs, 'initial kept-user-value warning lost'
        assert 'general.startupMode' in logs and 'general.theme' in logs
        result['deferredDiagnostics'] = True
        # Exercise actual process cleanup failure without touching unrelated processes.
        from adoption import finish_profiles
        killed = Profile(out, 'killed-daemon', args.daemon.resolve(), args.cli.resolve())
        stopped = Profile(out, 'stopped-daemon', args.daemon.resolve(), args.cli.resolve())
        survivor = Profile(out, 'surviving-daemon', args.daemon.resolve(), args.cli.resolve())
        profiles.extend([killed, stopped, survivor])
        killed.start(); stopped.start(); survivor.start()
        killed.process.kill(); killed.process.wait(timeout=10)
        stopped.process.send_signal(signal.SIGSTOP)
        cleanup_errors = finish_profiles([killed, stopped, survivor])
        assert len(cleanup_errors) == 2, cleanup_errors
        assert all((item.root / 'receipt.json').is_file() for item in [killed, stopped, survivor])
        assert all(item.process is None for item in [killed, stopped, survivor])
        result['cleanupErrors'] = cleanup_errors
        result['passed'] = True
    finally:
        errors = []
        for profile in profiles:
            try:
                profile.finish()
            except Exception as error:
                errors.append(str(error))
        result['finalCleanupErrors'] = errors
        (out / 'result.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
