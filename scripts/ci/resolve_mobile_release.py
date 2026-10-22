#!/usr/bin/env python3
"""Resolve optional release-note enrichment without blocking publication."""
import os
from pathlib import Path
import re
import subprocess

base = 'https://github.com/UniClipboard/UniClip/releases'
url = base + '/latest' if os.environ['CHANNEL'] == 'stable' else base
if os.environ['CHANNEL'] != 'stable':
    try:
        result = subprocess.run(
            ['gh', 'api', '-X', 'GET', 'repos/UniClipboard/UniClip/releases?per_page=100',
             '--jq', 'map(select(.prerelease == true and .draft == false)) | .[0].html_url // empty'],
            capture_output=True, text=True, timeout=45)
        candidate = result.stdout.strip()
        if result.returncode == 0 and re.fullmatch(re.escape(base) + r'/tag/[^\s]+', candidate):
            url = candidate
        else:
            print('::warning::Mobile preview lookup was unavailable or empty; using the mobile releases page.')
    except (OSError, subprocess.TimeoutExpired):
        print('::warning::Mobile preview lookup failed; using the mobile releases page.')
with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
    output.write(f'android_release_url={url}\n')
