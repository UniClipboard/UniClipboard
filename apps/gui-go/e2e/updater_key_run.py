#!/usr/bin/env python3
"""Production updater key path: what the release build trusts, proven without touching any real release source.

The trust anchor of a release build is the minisign key that `build.sh` injects from the Tauri updater config via
`-X main.updaterPublicKey=...`. This script shows, end to end:
  1. the production-tagged binary built by `build.sh manual` carries exactly the Tauri config key, has no e2e control
     plane and no environment override strings (supply-chain contract of the artifact);
  2. the Tauri config key parses with the app's own `update.ParsePublicKey` (the Tauri value format);
  3. the Go default feed URLs match the Rust shell's production endpoints (the shipped Tauri code ignores the
     `endpoints` of tauri.conf.json and uses `default_updater_endpoints`);
  4. behavior of the real updater code in e2e-tagged builds linked with different keys, against a LOCAL feed only
     (nothing here contacts the release hosts, and the production binary is never run):
       - the production key rejects an artifact signed by another key (download fails with a signature error);
       - an empty key disables updates and no request reaches the feed (fail closed);
       - positive control: a build linked with the throwaway key accepts a feed signed by it and rejects another key's.
Not proven (no private key, no release access): that the production key verifies a real signed release.
"""
import argparse
import base64
import http.server
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import ROOT, isolated_env, read_steps  # noqa: E402

GUI = ROOT / 'apps/gui-go'
VERSION = '99.0.0-e2e'
TAURI_CONF = ROOT / 'apps/gui/src-tauri/tauri.conf.json'


class Feed:
    def __init__(self, directory):
        self.hits = []
        outer = self

        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *a, **k):
                super().__init__(*a, directory=str(directory), **k)

            def log_message(self, *a):
                pass

            def do_GET(self):
                outer.hits.append(self.path)
                super().do_GET()

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = f'http://127.0.0.1:{self.server.server_address[1]}'


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw).stdout


def production_binary_checks(conf_key):
    run([str(GUI / 'build.sh'), 'manual'], cwd=ROOT)
    binary = ROOT / 'target/gui-go/UniClipboardGo-binary'
    info = run(['go', 'version', '-m', str(binary)])
    flags = re.search(r'-ldflags=(.*)', info).group(1)
    tags = re.search(r'-tags=(\S+)', info).group(1)
    assert f'-X main.updaterPublicKey={conf_key}' in flags, 'the production binary was not linked with the Tauri config key'
    assert tags == 'production', tags
    blob = binary.read_bytes()
    forbidden = [m for m in (b'EvidenceService', b'UC_UPDATE_ENDPOINT', b'UC_UPDATE_PUBKEY', b'UC_GUI_GO_E2E', b'update-verify') if m in blob]
    assert not forbidden, f'test or override strings in the production binary: {forbidden}'
    return {'tags': tags, 'linkedKeyEqualsTauriConfig': True, 'forbiddenStringsFound': forbidden}


def endpoint_contract():
    rust = (ROOT / 'crates/uc-tauri/src/commands/updater.rs').read_text()
    go = (GUI / 'internal/update/update.go').read_text()
    rust_urls = re.findall(r'format!\("(https://[^"]*?)\{channel_str\}\.json"\)', rust)
    go_urls = re.findall(r'fmt\.Sprintf\("(https://[^"]*?)%s\.json", channel\)', go)
    assert rust_urls and rust_urls == go_urls, (rust_urls, go_urls)
    return {'rust': rust_urls, 'go': go_urls}


def build_variant(work, name, key):
    """A copy of the e2e bundle whose executable is linked with the given updater key."""
    bundle = work / name / 'UniClipboardGoE2E.app'
    shutil.copytree(ROOT / 'target/gui-go/UniClipboardGoE2E.app', bundle, symlinks=True)
    subprocess.run(['go', 'build', '-tags', 'e2e', '-ldflags', f'-X main.updaterPublicKey={key} -X main.productName=UniClipboard',
                    '-o', str(bundle / 'Contents/MacOS/gui-go'), '.'], cwd=GUI, check=True)
    subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(bundle)], check=True, capture_output=True)
    return bundle / 'Contents/MacOS/gui-go'


def launch(binary, out, label, endpoint):
    home = tempfile.mkdtemp(prefix='uc-gui-go-')
    profile = 'gui-go-' + os.path.basename(home)
    evidence = out / f'updater-key-{label}-native.jsonl'
    evidence.write_text('')
    env = isolated_env(home, profile, {'PATH': str(ROOT / 'target/debug') + ':' + os.environ['PATH'], 'UC_GPUI_QUICK_PANEL': '0', 'UC_GUI_GO_ISOLATED': '1',
                                      'UC_GUI_GO_EVIDENCE': str(evidence), 'UC_GUI_GO_E2E_PHASE': 'key-path-verify', 'UC_GUI_GO_EXIT_MODE': 'full',
                                      'UC_UPDATE_ENDPOINT': endpoint})
    proc = subprocess.Popen([str(binary)], env=env, stdout=(out / f'updater-key-{label}-gui.log').open('w'), stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            rows = read_steps(evidence, 0)
            for row in rows:
                if row['step'] == 'driver-error':
                    raise RuntimeError(f"driver error: {row.get('detail')}")
                if row['step'] == 'update-verify':
                    proc.wait(timeout=60)
                    return row['detail']
            if proc.poll() is not None:
                raise RuntimeError(f'GUI exited early for {label}')
            time.sleep(.2)
        raise RuntimeError(f'timeout for {label}')
    finally:
        if proc.poll() is None:
            proc.terminate()
        subprocess.run([str(ROOT / 'target/gui-go/uniclip'), '--json', 'stop'], env=env, capture_output=True, timeout=80)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    conf_key = json.load(open(TAURI_CONF))['plugins']['updater']['pubkey']
    assert conf_key, 'the Tauri updater public key is not configured'
    results = {'passed': False, 'tauriConfigKeyConfigured': True}
    work = Path(tempfile.mkdtemp(prefix='uc-gui-go-updater-key-'))
    feed_dir = work / 'feed'
    feed_dir.mkdir()
    try:
        comment = base64.b64decode(conf_key).decode().splitlines()[0]
        parsed = run(['go', 'run', './e2e/updatetool', 'parsekey', conf_key], cwd=GUI).strip()
        results['tauriConfigKey'] = {'comment': comment, 'parsedKeyID': parsed}
        results['endpointContract'] = endpoint_contract()
        # Local feed signed by a throwaway key; the artifact content is irrelevant, only its signature matters.
        (work / 'artifact.tar.gz').write_bytes(b'not a real bundle: verification fails or passes before any install')
        shutil.copy(work / 'artifact.tar.gz', feed_dir / 'update.app.tar.gz')
        run(['go', 'run', './e2e/updatetool', str(feed_dir / 'update.app.tar.gz'), str(feed_dir)], cwd=GUI)
        throwaway = (feed_dir / 'pubkey.b64').read_text()
        feed = Feed(feed_dir)
        arch = {'arm64': 'aarch64', 'x86_64': 'x86_64'}[platform.machine()]
        for name, sig in (('good', 'good.sig.b64'), ('bad', 'bad.sig.b64')):
            (feed_dir / f'{name}.json').write_text(json.dumps({
                'version': VERSION, 'notes': 'E2E', 'pub_date': '2026-10-06T00:00:00Z',
                'platforms': {f'darwin-{arch}-app': {'url': f'{feed.base}/update.app.tar.gz', 'signature': (feed_dir / sig).read_text()}}}))
        variants = {'production-key': conf_key, 'empty-key': '', 'throwaway-key': throwaway}
        binaries = {name: build_variant(work, name, key) for name, key in variants.items()}
        runs = {}
        d = launch(binaries['production-key'], out, 'production-key', f'{feed.base}/good.json')
        assert d['bakedKeyID'] == parsed and d.get('meta') and 'signature' in d.get('downloadError', '') and not d.get('downloaded'), d
        runs['productionKeyRejectsForeignSignature'] = d
        before = len(feed.hits)
        d = launch(binaries['empty-key'], out, 'empty-key', f'{feed.base}/good.json')
        assert not d['bakedKeyConfigured'] and 'updates are disabled' in d.get('checkError', ''), d
        assert len(feed.hits) == before, 'an unconfigured key still contacted the feed'
        runs['emptyKeyFailsClosed'] = d
        d = launch(binaries['throwaway-key'], out, 'throwaway-good', f'{feed.base}/good.json')
        assert d.get('downloaded') is True and not d.get('downloadError'), d
        runs['controlAcceptsOwnSignature'] = d
        d = launch(binaries['throwaway-key'], out, 'throwaway-bad', f'{feed.base}/bad.json')
        assert 'signature' in d.get('downloadError', '') and not d.get('downloaded'), d
        runs['controlRejectsOtherKey'] = d
        results['runs'] = runs
        # Last: `build.sh manual` rebuilds frontend/dist without the e2e driver, so restore the e2e build afterwards.
        results['productionBinary'] = production_binary_checks(conf_key)
        run([str(GUI / 'build.sh'), 'e2e'], cwd=ROOT)
        results['passed'] = True
    finally:
        shutil.rmtree(work, ignore_errors=True)
        (out / 'updater-key-assertions.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results, indent=2))
    assert results['passed']


if __name__ == '__main__':
    main()
