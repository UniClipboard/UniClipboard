#!/usr/bin/env python3
"""Exercise the mobile-link command through its process and GitHub output boundary."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[2]
results = []
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    gh = tmp / 'gh'
    gh.write_text('#!/bin/sh\nprintf "%s" "$FAKE_URL"\nexit "$FAKE_EXIT"\n')
    gh.chmod(0o755)
    for channel, url, code, expected in [
        ('stable', '', 1, 'https://github.com/UniClipboard/UniClip/releases/latest'),
        ('alpha', '', 0, 'https://github.com/UniClipboard/UniClip/releases'),
        ('beta', '', 1, 'https://github.com/UniClipboard/UniClip/releases'),
        ('rc', 'https://github.com/UniClipboard/UniClip/releases/tag/v2.1.0', 0,
         'https://github.com/UniClipboard/UniClip/releases/tag/v2.1.0'),
        ('alpha', 'https://example.com/invalid\noutput=injected', 0,
         'https://github.com/UniClipboard/UniClip/releases'),
    ]:
        output = tmp / 'output'
        output.write_text('')
        env = dict(os.environ, PATH=f'{tmp}:{os.environ["PATH"]}', CHANNEL=channel,
                   FAKE_URL=url, FAKE_EXIT=str(code), GITHUB_OUTPUT=str(output))
        result = subprocess.run([sys.executable, str(root / 'scripts/ci/resolve_mobile_release.py')],
                                env=env, capture_output=True, text=True)
        passed = result.returncode == 0 and output.read_text() == f'android_release_url={expected}\n'
        results.append({'channel': channel, 'lookupExit': code, 'passed': passed})
print(json.dumps({'cases': results, 'passed': all(r['passed'] for r in results)}, indent=2))
sys.exit(0 if all(r['passed'] for r in results) else 1)
