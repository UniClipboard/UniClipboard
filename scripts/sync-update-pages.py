#!/usr/bin/env python3
"""Snapshot authoritative channel responses and verify the Pages fallback.

This tool never pushes. The workflow owns the gh-pages commit and preserves all
unrelated files. An empty primary channel means the fallback file must be absent.
"""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.error
import urllib.request

# FlareRelease currently exposes exactly these desktop manifest routes.
CHANNELS = ('stable', 'alpha')
PRIMARY = 'https://release.uniclipboard.app'
FALLBACK = 'https://uniclipboard.github.io/UniClipboard'


def fetch(base, channel, authoritative=False):
    request = urllib.request.Request(f'{base}/{channel}.json', headers={
        'Accept': 'application/json', 'Cache-Control': 'no-cache',
        'User-Agent': 'UniClipboard-Update-Publisher/1.0'})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            status = response.status
            raw = response.read(4 * 1024 * 1024 + 1)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            if authoritative and json.loads(error.read()) != {'error': 'Release not found'}:
                raise ValueError(f'{channel}: primary route missing, not an empty channel')
            return 404, None, b''
        raise
    if status == 204:
        return status, None, b''
    if status != 200 or len(raw) > 4 * 1024 * 1024:
        raise ValueError(f'{channel}: invalid response status/size')
    data = json.loads(raw)
    if not isinstance(data, dict) or not isinstance(data.get('platforms'), dict) or not data.get('version'):
        raise ValueError(f'{channel}: invalid feed')
    if not data['platforms']:
        raise ValueError(f'{channel}: feed contains no update platforms')
    for key, entry in data['platforms'].items():
        if not entry.get('url', '').startswith('https://') or not entry.get('signature'):
            raise ValueError(f'{channel}: unsigned or non-HTTPS artifact {key}')
    return status, data, raw


def snapshot(out, primary):
    out.mkdir(parents=True, exist_ok=False)
    states = {}
    for channel in CHANNELS:
        status, _, raw = fetch(primary, channel, authoritative=True)
        states[channel] = {'status': status, 'sha256': hashlib.sha256(raw).hexdigest()}
        if status == 200:
            (out / (channel + '.json')).write_bytes(raw)
    (out / 'snapshot.json').write_text(json.dumps(states, indent=2) + '\n')


def check_snapshot(directory, primary, fallback=None):
    states = json.loads((directory / 'snapshot.json').read_text())
    for channel in CHANNELS:
        status, live, _ = fetch(primary, channel, authoritative=True)
        expected_status = states[channel]['status']
        expected = None
        if expected_status == 200:
            raw = (directory / (channel + '.json')).read_bytes()
            if hashlib.sha256(raw).hexdigest() != states[channel]['sha256']:
                raise ValueError(f'{channel}: snapshot changed on disk')
            expected = json.loads(raw)
        if status != expected_status or live != expected:
            raise ValueError(f'{channel}: primary changed since snapshot; take a fresh snapshot')
        if fallback:
            fallback_status, actual, _ = fetch(fallback, channel)
            if expected_status in (204, 404):
                if fallback_status not in (204, 404):
                    raise ValueError(f'{channel}: fallback still advertises a withdrawn channel')
            elif fallback_status != 200 or actual != expected:
                raise ValueError(f'{channel}: fallback differs from primary')
        print(f'{channel}: {status}, consistent')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('snapshot', 'check'))
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--primary', default=PRIMARY)
    parser.add_argument('--fallback', default=None)
    args = parser.parse_args()
    if args.mode == 'snapshot':
        snapshot(args.directory, args.primary)
    else:
        check_snapshot(args.directory, args.primary, args.fallback)
