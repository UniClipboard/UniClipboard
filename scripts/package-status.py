#!/usr/bin/env python3
"""Query published package metadata and render a shared README badge snapshot."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import gzip
from html import escape
import json
from pathlib import Path
import re
import subprocess
import tempfile
import unicodedata
from urllib.error import HTTPError
from urllib.parse import urljoin
from urllib.request import Request, urlopen
import xml.etree.ElementTree as ET


class NotPublished(Exception):
    """The registry explicitly reports that the package/channel is absent."""


def fetch(url, *, missing=False):
    request = Request(url, headers={
        'User-Agent': 'UniClipboard-package-status', 'Snap-Device-Series': '16',
    })
    try:
        with urlopen(request, timeout=25) as response:
            return response.read()
    except HTTPError as error:
        if missing and error.code == 404:
            raise NotPublished(url) from error
        raise


def document(url):
    return json.loads(fetch(url, missing=True))


def version_key(version):
    match = re.fullmatch(r'(\d+(?:\.\d+)*)(.*)', version)
    if not match:
        raise ValueError(f'Unrecognized version: {version}')
    numbers = tuple(int(part) for part in match[1].split('.'))
    suffix = match[2]
    tokens = tuple((1, int(part)) if part.isdigit() else (0, part)
                   for part in re.findall(r'\d+|[a-zA-Z]+', suffix))
    return numbers, not bool(suffix), tokens


def latest(versions):
    values = set(versions)
    if not values:
        raise NotPublished('No published version in this channel')
    return max(values, key=version_key)


def ruby_version(url):
    match = re.search(r'^\s*version "([^"]+)"', fetch(url, missing=True).decode(), re.M)
    if not match:
        raise ValueError('Published Homebrew definition has no version')
    return match[1]


def aur_version():
    # Git avoids the web challenge and RPC cache; never execute remote PKGBUILD code.
    with tempfile.TemporaryDirectory(prefix='uc-aur-status-') as directory:
        subprocess.run(['git', 'clone', '--quiet', '--depth', '1',
                        'https://aur.archlinux.org/uniclipboard-git.git', directory],
                       check=True, timeout=60, capture_output=True)
        source = Path(directory, '.SRCINFO').read_text()
    version = re.search(r'^\s*pkgver = (\S+)$', source, re.M)
    release = re.search(r'^\s*pkgrel = (\S+)$', source, re.M)
    if not version or not release:
        raise ValueError('AUR .SRCINFO is missing pkgver/pkgrel')
    return f'{version[1]}-{release[1]}'


def snap_version(risk):
    entries = document('https://api.snapcraft.io/v2/snaps/info/uniclipboard')['channel-map']
    versions = {arch: {entry['version'] for entry in entries
                      if entry['channel']['risk'] == risk
                      and entry['channel']['track'] == 'latest'
                      and entry['channel']['architecture'] == arch}
                for arch in ('amd64', 'arm64')}
    # A partial architecture rollout must not look complete.
    if versions['amd64'] != versions['arm64']:
        return ' / '.join(f'{arch}: {latest(values) if values else "未发布"}'
                          for arch, values in versions.items())
    return latest(versions['amd64'])


def winget_version():
    entries = document('https://api.github.com/repos/microsoft/winget-pkgs/contents/'
                       'manifests/u/UniClipboard/UniClipboard')
    return latest(entry['name'] for entry in entries if entry['type'] == 'dir')


def chocolatey_version():
    source = fetch("https://community.chocolatey.org/api/v2/FindPackagesById()?id='uniclipboard'")
    entries = ET.fromstring(source).findall('{http://www.w3.org/2005/Atom}entry')
    versions = []
    for entry in entries:
        properties = {node.tag.split('}')[-1]: node.text for node in entry.iter()}
        if properties.get('IsApproved') == 'true' and properties.get('Listed') != 'false':
            versions.append(properties['Version'])
    return latest(versions)


def copr_version(project):
    # Report the explicitly labelled Fedora 44 x86_64 repository, not build status.
    base = f'https://download.copr.fedorainfracloud.org/results/mkdir700/{project}/fedora-44-x86_64/'
    root = ET.fromstring(fetch(base + 'repodata/repomd.xml', missing=True))
    namespace = {'r': 'http://linux.duke.edu/metadata/repo'}
    primary = root.find("r:data[@type='primary']", namespace)
    location = primary.find('r:location', namespace).attrib['href']
    payload = fetch(urljoin(base, location))
    if location.endswith('.gz'):
        payload = gzip.decompress(payload)
    namespace = {'c': 'http://linux.duke.edu/metadata/common'}
    packages = ET.fromstring(payload).findall('c:package', namespace)
    versions = []
    for package in packages:
        if package.findtext('c:name', namespaces=namespace) == 'uniclipboard':
            version = package.find('c:version', namespace).attrib
            versions.append(f"{version['ver']}-{version['rel']}")
    return latest(versions)


def flathub_version():
    data = document('https://flathub.org/api/v2/appstream/app.uniclipboard.desktop')
    return latest(release['version'] for release in data['releases']
                  if release.get('type') == 'stable')


def nixpkgs_version():
    source = fetch('https://raw.githubusercontent.com/NixOS/nixpkgs/nixos-unstable/'
                   'pkgs/by-name/un/uniclipboard/package.nix', missing=True).decode()
    match = re.search(r'\bversion\s*=\s*"([^"]+)"', source)
    if not match:
        raise ValueError('nixpkgs package has no version')
    return match[1]


RAW = 'https://raw.githubusercontent.com/'
CHANNELS = {
    'aur': aur_version,
    'brew-gui': lambda: ruby_version(RAW + 'UniClipboard/homebrew-tap/HEAD/Casks/uniclipboard.rb'),
    'brew-cli': lambda: ruby_version(RAW + 'UniClipboard/homebrew-tap/HEAD/Formula/uniclipboard.rb'),
    'brew-official': lambda: ruby_version(RAW + 'Homebrew/homebrew-cask/HEAD/Casks/u/uniclipboard.rb'),
    'scoop': lambda: document(RAW + 'ScoopInstaller/Extras/HEAD/bucket/uniclipboard.json')['version'],
    'winget': winget_version,
    'chocolatey': chocolatey_version,
    'npm': lambda: document('https://registry.npmjs.org/@uniclipboard%2fcli')['dist-tags']['latest'],
    'npm-alpha': lambda: document('https://registry.npmjs.org/@uniclipboard%2fcli')['dist-tags'].get('alpha'),
    'snap': lambda: snap_version('stable'),
    'snap-edge': lambda: snap_version('edge'),
    'copr': lambda: copr_version('uniclipboard'),
    'copr-alpha': lambda: copr_version('uniclipboard-alpha'),
    'nix-flake': lambda: document(RAW + 'UniClipboard/UniClipboard/HEAD/packaging/nix/sources.json')['version'],
    'nixpkgs': nixpkgs_version,
    'flathub': flathub_version,
}


def probe(item):
    name, reader = item
    try:
        version = reader()
        if version is None:
            raise NotPublished('Channel has no version')
        if not isinstance(version, str) or not version.strip():
            raise ValueError('Registry returned an invalid version')
        return name, {'status': 'published', 'version': version}
    except NotPublished:
        return name, {'status': 'not-published', 'version': None}
    except Exception as error:
        # Replace stale successes with explicit errors; retain details in the snapshot.
        return name, {'status': 'unavailable', 'version': None,
                      'error': f'{type(error).__name__}: {error}'}


def badge(label, message, color):
    def width(text):
        return 12 + sum(13 if unicodedata.east_asian_width(c) in 'WF' else 7 for c in text)
    left, right = width(label), width(message)
    title = escape(f'{label}: {message}', quote=True)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{left + right}" height="22" role="img" aria-label="{title}">
<title>{title}</title><rect width="{left}" height="22" fill="#555"/><rect x="{left}" width="{right}" height="22" fill="{color}"/>
<g fill="#fff" text-anchor="middle" font-family="Verdana,DejaVu Sans,sans-serif" font-size="11"><text x="{left / 2}" y="15">{escape(label)}</text><text x="{left + right / 2}" y="15">{escape(message)}</text></g></svg>
'''


def collect(directory):
    checked_at = datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')
    with ThreadPoolExecutor(max_workers=8) as pool:
        channels = dict(pool.map(probe, CHANNELS.items()))
    directory.mkdir(parents=True, exist_ok=True)
    badges = directory / 'badges'
    badges.mkdir(exist_ok=True)
    for name, result in channels.items():
        message = result['version'] or {'not-published': '未发布', 'unavailable': '查询失败'}[result['status']]
        color = {'published': '#007ec6', 'not-published': '#777', 'unavailable': '#b33'}[result['status']]
        (badges / f'{name}.svg').write_text(badge('版本', message, color))
    (badges / 'checked-at.svg').write_text(badge('查询时间', checked_at, '#555'))
    (directory / 'status.json').write_text(json.dumps(
        {'checked_at': checked_at, 'channels': channels}, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(channels, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    collect(parser.parse_args().output)
