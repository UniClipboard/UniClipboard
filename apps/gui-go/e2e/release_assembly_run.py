#!/usr/bin/env python3
"""Offline acceptance of the release path between the downloaded workflow artifacts and the first external write.

Runs the SAME scripts the release workflow runs (`scripts/ci/release_gate.py`, `scripts/ci/assemble_release_assets.py`, the
collector, `updater-sign`, the manifest and registration generators) with `--mode fixture`, over a synthetic artifact tree laid
out like `download-artifact` output. Then a real Go consumer downloads all six platforms over local HTTP and verifies them.

What this is NOT: no package here is a real build, the updater key is a disposable one, the Windows "production signature" in
the evidence is a labelled fixture record, and nothing was signed by a production certificate. Production signing acceptance
stays blocked until the production Windows backend exists (see scope.json). No network service, tag, release, bucket or channel
is contacted: proxies point at a dead port and every token is removed from the child environment.

Failure model, each a negative case below: a platform missing; a file of another version; evidence from another commit or a
dirty one; a source record that is not the pinned SHA; the same name twice; test-mode / test-signed / unsigned Windows input;
stale buildinfo or a drifted Engine pin; a tampered or swapped signature after signing; the production public key being
bypassed with a throwaway key; missing production prerequisites.
"""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[3]
GUI = ROOT / 'apps/gui-go'
GATE = ROOT / 'scripts/ci/release_gate.py'
ASSEMBLE = ROOT / 'scripts/ci/assemble_release_assets.py'
RUN_ID = '1000000000'


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Run:
    def __init__(self, out):
        self.out = out
        self.checks = []

    def exec(self, label, cmd, *, env=None, cwd=ROOT, want_ok=True, contains=None):
        r = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env if env is not None else self.env, capture_output=True, text=True)
        text = r.stdout + r.stderr
        (self.out / 'logs').mkdir(exist_ok=True)
        (self.out / 'logs' / f'{label}.log').write_text(f'$ {" ".join(map(str, cmd))}\nexit {r.returncode}\n{text}')
        ok = (r.returncode == 0) == want_ok and (contains is None or contains in text)
        self.checks.append({'name': label, 'exit': r.returncode, 'expectedOk': want_ok, 'expectedText': contains, 'ok': ok})
        (self.out / 'checks.json').write_text(json.dumps(self.checks, indent=2) + '\n')
        if not ok:
            sys.exit(f'FAILED {label}: exit {r.returncode}, wanted {"success" if want_ok else "failure"}'
                     + (f' containing {contains!r}' if contains else '') + f'\n{text[-2000:]}')
        print(('PASS ' if ok else 'FAIL ') + label)
        return r

    def assert_(self, label, condition):
        self.checks.append({'name': label, 'ok': bool(condition)})
        (self.out / 'checks.json').write_text(json.dumps(self.checks, indent=2) + '\n')
        if not condition:
            sys.exit(f'FAILED {label}')
        print('PASS ' + label)


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data if isinstance(data, bytes) else data.encode())


def build_tree(tree, version, sha, *, windows_provider='signed'):
    """A synthetic `download-artifact` tree. Every payload says it is synthetic and differs per file."""
    def payload(name):
        return f'SYNTHETIC release-assembly fixture, not a build: {name}\n'.encode()

    for target, dmg in (('aarch64-apple-darwin', 'aarch64'), ('x86_64-apple-darwin', 'x64')):
        write(tree / f'macos-gui-{target}/UniClipboard.app.tar.gz', payload('app.tar.gz ' + target))
        write(tree / f'macos-gui-{target}/UniClipboard_{version}_{dmg}.dmg', payload('dmg ' + dmg))
        write(tree / f'macos-gui-evidence-{target}/provenance.json',
              json.dumps({'version': version, 'git': {'head': sha, 'dirty': False}, 'target': target}))
    for deb, rpm, appimage in (('amd64', 'x86_64', 'amd64'), ('arm64', 'aarch64', 'aarch64')):
        d = tree / f'linux-gui-{deb}'
        write(d / f'UniClipboard_{version}_{deb}.deb', payload('deb ' + deb))
        write(d / f'UniClipboard-{version}-1.{rpm}.rpm', payload('rpm ' + rpm))
        write(d / f'UniClipboard_{version}_{appimage}.AppImage.tar.gz', payload('appimage ' + appimage))
        write(tree / f'linux-gui-evidence-{deb}/package-manifest.json',
              json.dumps({'version': version, 'source': {'head': sha, 'dirty': False}, 'arch': deb}))
    for arch, win in (('amd64', 'x64'), ('arm64', 'arm64')):
        write(tree / f'windows-gui-{arch}-{RUN_ID}/UniClipboard_{version}_{win}-setup.exe', payload('setup ' + win))
        write(tree / f'windows-gui-{arch}-{RUN_ID}/UniClipboard_{version}_{win}-portable.zip', payload('portable ' + win))
        evidence = tree / f'windows-gui-evidence-{arch}-{RUN_ID}/windows-gui'
        write(evidence / 'shipped/package-manifest.json', json.dumps(
            {'version': version, 'source': {'head': sha, 'dirty': False}, 'arch': arch, 'signed': True,
             'signing': {'provider': windows_provider}, 'fixtureRecord': 'a labelled fixture, not an Authenticode signature'}))
        # The upgrade-acceptance package deliberately has another version and must not be judged as the release.
        write(evidence / 'newer/package-manifest.json', json.dumps(
            {'purpose': 'acceptance-newer-version', 'version': '99.0.0-acceptance', 'source': {'head': sha, 'dirty': False},
             'signed': True, 'signing': {'provider': 'selftest'}}))
        # The evidence artifact also carries a copy of the CLI archive (a real quirk the collector must ignore).
        write(evidence / 'cli-package/uniclipboard-cli-{}-x86_64-pc-windows-msvc.zip'.format(version), payload('evidence copy of cli'))
    write(tree / 'cli-x86_64-pc-windows-msvc' / f'uniclipboard-cli-{version}-x86_64-pc-windows-msvc.zip', payload('cli windows'))
    write(tree / f'cli-x86_64-unknown-linux-musl' / f'uniclipboard-cli-{version}-x86_64-unknown-linux-musl.tar.gz', payload('cli linux'))
    # Acceptance, install and legacy-upgrade artifacts of the real Linux run (names from CI run 37894007183) hold package manifests of
    # other purposes and versions; they are not release inputs and must not be judged as such.
    write(tree / 'linux-gui-acceptance-amd64/v1/pkg/package-manifest.json',
          json.dumps({'purpose': 'e2e-package', 'version': '0.0.1-acceptance', 'source': {'head': '1' * 40, 'dirty': True}}))
    write(tree / 'linux-gui-legacy-go-amd64/deb/legacy-go.deb', payload('legacy go deb'))
    write(tree / 'linux-gui-legacy-go-amd64/rpm-newer/legacy-go.rpm', payload('legacy go rpm'))
    # An intermediate SignPath input: same installer names before their final signature; never a release source.
    write(tree / f'signpath-stage2-amd64-{RUN_ID}/UniClipboard_{version}_x64-setup.exe', payload('UNSIGNED stage 2 input'))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True)
    args = p.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    run = Run(out)
    version = json.loads((GUI / 'app.json').read_text())['version']
    head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()
    other = '0' * 40

    # No network and no credentials for any child process.
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('TOKEN', 'SECRET', 'PASSWORD', 'TAURI_SIGNING', 'SIGN_BACKEND', 'CLOUDFLARE', 'FLARE', 'NPM'))}
    env.update(HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9', http_proxy='http://127.0.0.1:9',
               https_proxy='http://127.0.0.1:9', NO_PROXY='127.0.0.1', no_proxy='127.0.0.1', GOPROXY='off')  # modules are fetched by the two builds below, before the offline environment applies
    run.env = env

    with tempfile.TemporaryDirectory(prefix='uc-release-assembly-') as secret_dir:
        secret = Path(secret_dir)
        signer, probe = secret / 'signer', secret / 'probe'
        run.exec('build-signer', ['go', 'build', '-o', signer, './cmd/updater-sign'], cwd=GUI, env=dict(os.environ))
        run.exec('build-probe', ['go', 'build', '-o', probe, './e2e/signatureprobe'], cwd=GUI, env=dict(os.environ))
        run.exec('fixture-key', [probe, 'fixture-key', secret / 'key-dir'])
        key = (secret / 'key-dir/key').read_bytes()
        config = secret / 'key-dir/app.json'
        sign_env = dict(env, TAURI_SIGNING_PRIVATE_KEY=base64.b64encode(key).decode(), TAURI_SIGNING_PRIVATE_KEY_PASSWORD='disposable-e2e-password')

        notes, zh = out / 'notes.md', out / 'notes.zh.md'
        notes.write_text('Synthetic acceptance only')
        zh.write_text('仅用于合成验收')

        def source_record(path, mode='fixture', sha=head, ver=version):
            run.exec(path.stem, ['python3', '-I', GATE, 'source', '--version', ver, '--expect-sha', sha, '--mode', mode, '--out', path])

        record = out / 'source-record.json'
        source_record(record)
        recorded = json.loads(record.read_text())
        run.assert_('source record pins the commit, the version and the Engine rev', recorded['sourceSha'] == head
                    and recorded['version'] == version and len(recorded['enginePin']) == 40 and recorded['mode'] == 'fixture')

        def assemble(label, tree, work, *, base_url='http://127.0.0.1:1/assets', source_record_path=record, sha=head, mode='fixture',
                     extra=(), env_=sign_env, want_ok=True, contains=None, use_fixture_signer=True, ver=version):
            cmd = ['python3', '-I', ASSEMBLE, '--artifacts', tree, '--work', work, '--version', ver, '--channel', 'alpha',
                   '--source-sha', sha, '--source-record', source_record_path, '--base-url', base_url, '--notes-file', notes,
                   '--zh-notes-file', zh, '--registration-source', 'offline-acceptance:fixture', '--mode', mode]
            if use_fixture_signer:
                cmd += ['--signer', signer, '--app-config', config]
            return run.exec(label, cmd + list(extra), env=env_, want_ok=want_ok, contains=contains)

        # ---- positive: the whole chain, then a real consumer over HTTP ----
        base = out / 'cases'
        tree = base / 'positive-artifacts'
        build_tree(tree, version, head)
        server_root = base / 'positive'
        handler = type('H', (http.server.SimpleHTTPRequestHandler,), {
            '__init__': lambda self, *a, **k: http.server.SimpleHTTPRequestHandler.__init__(self, *a, directory=str(server_root), **k),
            'log_message': lambda self, *a: None})
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        url = f'http://127.0.0.1:{server.server_port}'
        try:
            assemble('positive-assemble', tree, server_root, base_url=f'{url}/release-assets')
            assets = server_root / 'release-assets'
            names = sorted(f.name for f in assets.iterdir())
            sigs = [n for n in names if n.endswith('.sig')]
            run.assert_('six updater payloads carry a signature', len(sigs) == 6)
            run.assert_('the SignPath stage-2 input and the evidence CLI copy were not collected',
                        sha256(assets / f'UniClipboard_{version}_x64-setup.exe') == sha256(tree / f'windows-gui-amd64-{RUN_ID}/UniClipboard_{version}_x64-setup.exe')
                        and sha256(assets / f'uniclipboard-cli-{version}-x86_64-pc-windows-msvc.zip') == sha256(tree / 'cli-x86_64-pc-windows-msvc' / f'uniclipboard-cli-{version}-x86_64-pc-windows-msvc.zip'))
            manifest = json.loads((server_root / 'evidence/manifest.json').read_text())
            run.assert_('manifest covers six platforms', sorted(manifest['platforms']) ==
                        ['darwin-aarch64', 'darwin-x86_64', 'linux-aarch64', 'linux-x86_64', 'windows-aarch64', 'windows-x86_64'])
            registration = json.loads((server_root / 'evidence/registration.json').read_text())
            run.assert_('registration lists six artifacts with sha256 and signature',
                        len(registration['artifacts']) == 6 and all(a['sha256'] and a['signature'] for a in registration['artifacts'])
                        and registration['version'] == version and registration['source'] == 'offline-acceptance:fixture')
            index = json.loads((server_root / 'evidence/assembly-index.json').read_text())
            run.assert_('index sha256 equals the bytes on disk and records the pinned source and Engine rev',
                        all(sha256(assets / n) == h for n, h in index['files'].items()) and index['sourceSha'] == head and index['enginePin'] == recorded['enginePin'])
            gate_index = json.loads((server_root / 'evidence/release-assets-index.json').read_text())
            run.assert_('every asset records the workflow artifact it came from', all(a['sourceArtifacts'] for a in gate_index['assets']))
            run.exec('consume-six-platforms-over-http', [probe, 'consume', config, f'{url}/evidence/manifest.json'])

            # ---- tamper after signing ----
            target = assets / f'UniClipboard_{version}_amd64.AppImage.tar.gz'
            original = target.read_bytes()
            target.write_bytes(original + b'tampered')
            run.exec('verify-refuses-tampered-bytes', [signer, '--app-config', config, '--artifacts-dir', assets, '--verify-only'],
                     want_ok=False)
            run.exec('consume-refuses-tampered-bytes', [probe, 'consume', config, f'{url}/evidence/manifest.json'], want_ok=False)
            target.write_bytes(original)
            sig_a = Path(str(target) + '.sig')
            sig_b = Path(str(assets / f'UniClipboard_{version}_aarch64.AppImage.tar.gz') + '.sig')
            saved = sig_a.read_bytes()
            sig_a.write_bytes(sig_b.read_bytes())
            run.exec('verify-refuses-swapped-signature', [signer, '--app-config', config, '--artifacts-dir', assets, '--verify-only'],
                     want_ok=False)
            sig_a.write_bytes(saved)
            run.exec('verify-accepts-restored-bytes', [signer, '--app-config', config, '--artifacts-dir', assets, '--verify-only'])
        finally:
            server.shutdown()
            server.server_close()

        # ---- negatives over the same chain ----
        def variant(name, mutate, *, contains, sha=head, record_path=record):
            tree = base / f'{name}-artifacts'
            build_tree(tree, version, head)
            mutate(tree)
            work = base / name
            assemble(f'negative-{name}', tree, work, sha=sha, source_record_path=record_path, want_ok=False, contains=contains)
            run.assert_(f'{name}: nothing was signed or registered', not (work / 'evidence/registration.json').exists()
                        and not list((work / 'release-assets').glob('*.sig')) if (work / 'release-assets').exists() else True)

        variant('missing-platform', lambda t: (t / f'linux-gui-arm64/UniClipboard_{version}_aarch64.AppImage.tar.gz').unlink(),
                contains='missing linux/aarch64 asset')
        variant('missing-windows-arm64-installer', lambda t: (t / f'windows-gui-arm64-{RUN_ID}/UniClipboard_{version}_arm64-setup.exe').unlink(),
                contains='missing windows/aarch64 asset')
        variant('wrong-version-file', lambda t: (t / f'linux-gui-amd64/UniClipboard_{version}_amd64.deb').rename(t / 'linux-gui-amd64/UniClipboard_9.9.9_amd64.deb'),
                contains='carries version 9.9.9')
        variant('wrong-version-cli', lambda t: (t / 'cli-x86_64-pc-windows-msvc' / f'uniclipboard-cli-{version}-x86_64-pc-windows-msvc.zip').rename(
            t / 'cli-x86_64-pc-windows-msvc/uniclipboard-cli-9.9.9-x86_64-pc-windows-msvc.zip'), contains='CLI archive')
        variant('duplicate-asset', lambda t: write(t / f'linux-gui-extra/UniClipboard_{version}_amd64.deb', b'a second deb of the same name'),
                contains='duplicate release asset')
        variant('evidence-from-other-commit', lambda t: write(t / 'linux-gui-evidence-amd64/package-manifest.json',
                json.dumps({'version': version, 'source': {'head': other, 'dirty': False}})), contains='not the pinned source')
        variant('evidence-from-dirty-checkout', lambda t: write(t / 'linux-gui-evidence-arm64/package-manifest.json',
                json.dumps({'version': version, 'source': {'head': head, 'dirty': True}})), contains='dirty checkout')
        variant('evidence-of-other-version', lambda t: write(t / 'macos-gui-evidence-x86_64-apple-darwin/provenance.json',
                json.dumps({'version': '9.9.9', 'git': {'head': head, 'dirty': False}})), contains='is version')
        variant('windows-test-signing-artifact', lambda t: write(t / f'windows-gui-amd64-{RUN_ID}-signpath-test/package-manifest.json', '{}'),
                contains='test-mode or test-signed build')
        variant('windows-selftest-artifact', lambda t: write(t / f'windows-gui-evidence-amd64-{RUN_ID}-signing-selftest/x.json', '{}'),
                contains='test-mode or test-signed build')
        variant('test-mode-build-artifact', lambda t: write(t / f'macos-gui-evidence-aarch64-apple-darwin-test/provenance.json', '{}'),
                contains='test-mode or test-signed build')
        for provider in ('signpath-test', 'selftest', 'unsigned', 'unspecified'):
            variant(f'windows-provider-{provider}', lambda t, p=provider: write(
                t / f'windows-gui-evidence-amd64-{RUN_ID}/windows-gui/shipped/package-manifest.json',
                json.dumps({'version': version, 'source': {'head': head, 'dirty': False}, 'signed': provider != 'unsigned',
                            'signing': {'provider': p}})), contains='requires a production signature')
        variant('source-record-for-another-sha', lambda t: None, contains='does not describe', sha=other)

        # ---- the source gate on a pinned commit ----
        run.exec('source-gate-wrong-version', ['python3', '-I', GATE, 'source', '--version', '9.9.9', '--mode', 'fixture', '--out', out / 'x.json'],
                 want_ok=False, contains='not the release version')
        run.exec('source-gate-wrong-sha', ['python3', '-I', GATE, 'source', '--version', version, '--expect-sha', other, '--mode', 'fixture', '--out', out / 'x.json'],
                 want_ok=False, contains='is not the pinned source')
        repo = secret / 'carriers'
        for rel in ('package.json', 'apps/gui-go/app.json', 'Cargo.toml', 'Cargo.lock', 'packages/desktop-host-go/buildinfo/buildinfo.go'):
            (repo / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / rel, repo / rel)
        git = lambda *a: subprocess.run(['git', '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', *a], cwd=repo, check=True, capture_output=True)
        git('init', '-q')
        git('add', '-A')
        git('commit', '-qm', 'carriers')
        run.exec('source-gate-copy-of-carriers-passes', ['python3', '-I', GATE, 'source', '--version', version, '--root', repo, '--out', out / 'copy.json'])
        buildinfo = repo / 'packages/desktop-host-go/buildinfo/buildinfo.go'
        buildinfo.write_text(buildinfo.read_text().replace(f'"{version}"', '"0.0.1-stale"', 1))
        git('commit', '-qam', 'stale buildinfo')
        run.exec('source-gate-stale-buildinfo', ['python3', '-I', GATE, 'source', '--version', version, '--root', repo, '--out', out / 'x.json'],
                 want_ok=False, contains='buildinfo.go')
        git('revert', '--no-edit', 'HEAD')
        lock = repo / 'Cargo.lock'
        text = lock.read_text()
        engine = recorded['enginePin']
        lock.write_text(text.replace(engine, '1' * 40))
        git('commit', '-qam', 'drifted lock')
        run.exec('source-gate-engine-pin-drift', ['python3', '-I', GATE, 'source', '--version', version, '--root', repo, '--out', out / 'x.json'],
                 want_ok=False, contains='does not lock uc-engine')
        git('revert', '--no-edit', 'HEAD')
        (repo / 'package.json').write_text((repo / 'package.json').read_text().replace(f'"{version}"', '"9.9.9"', 1))
        git('commit', '-qam', 'second bump of one carrier')
        run.exec('source-gate-single-carrier-differs', ['python3', '-I', GATE, 'source', '--version', version, '--root', repo, '--out', out / 'x.json'],
                 want_ok=False, contains='package.json')

        # ---- the same chain for a pre-release spelling, on a copy of the carriers that says 1.3.0-alpha.1 ----
        # The repository's own carriers hold whatever prepare-release last wrote; the pre-release file names, the rpm/deb/AppImage
        # patterns and the manifest are exercised here regardless. The copy is never written back.
        import re
        pre_version = '1.3.0-alpha.1'
        pre_repo = secret / 'prerelease-carriers'
        shutil.copytree(repo, pre_repo)
        git_pre = lambda *a: subprocess.run(['git', '-c', 'user.name=t', '-c', 'user.email=t@example.invalid', *a], cwd=pre_repo, check=True, capture_output=True, text=True)
        git_pre('reset', '-q', '--hard', 'HEAD~0')
        git_pre('checkout', '-q', git_pre('rev-list', '--max-parents=0', 'HEAD').stdout.strip(), '--', '.')
        for rel in ('package.json', 'apps/gui-go/app.json'):
            f = pre_repo / rel
            f.write_text(f.read_text().replace(f'"version": "{version}"', f'"version": "{pre_version}"', 1))
        cargo = pre_repo / 'Cargo.toml'
        cargo.write_text(re.sub(r'(\[workspace\.package\]\n(?:.*\n)*?version = ")[^"]+', lambda m: m.group(1) + pre_version, cargo.read_text(), count=1))
        info = pre_repo / 'packages/desktop-host-go/buildinfo/buildinfo.go'
        info.write_text(re.sub(r'PackageVersion = "[^"]+"', f'PackageVersion = "{pre_version}"', info.read_text()))
        lockf = pre_repo / 'Cargo.lock'
        lockf.write_text('[[package]]'.join([lockf.read_text().split('[[package]]')[0]] + [
            re.sub(r'^version = "[^"]+"', f'version = "{pre_version}"', b, count=1, flags=re.M) if 'source = ' not in b and f'version = "{version}"' in b else b
            for b in lockf.read_text().split('[[package]]')[1:]]))
        git_pre('add', '-A')
        git_pre('commit', '-qm', 'prepared 1.3.0-alpha.1 carriers (fixture copy)')
        pre_sha = git_pre('rev-parse', 'HEAD').stdout.strip()
        pre_record = out / 'source-record-prerelease.json'
        run.exec('prerelease-source-gate', ['python3', '-I', GATE, 'source', '--version', pre_version, '--root', pre_repo, '--expect-sha', pre_sha,
                                            '--mode', 'fixture', '--out', pre_record])
        # In release mode a dirty checkout is refused: files the workflow itself writes (artifacts/) make it dirty, so the
        # workflow runs this gate right after checkout, before install and download.
        run.exec('release-mode-source-gate-clean-checkout', ['python3', '-I', GATE, 'source', '--version', pre_version, '--root', pre_repo,
                                                             '--expect-sha', pre_sha, '--out', out / 'release-mode-clean.json'])
        write(pre_repo / 'artifacts/linux-gui-amd64/x', b'downloaded after the gate')
        run.exec('release-mode-source-gate-refuses-untracked-download', ['python3', '-I', GATE, 'source', '--version', pre_version, '--root', pre_repo,
                                                                         '--expect-sha', pre_sha, '--out', out / 'x.json'], want_ok=False,
                 contains='uncommitted changes')
        shutil.rmtree(pre_repo / 'artifacts')
        pre_tree = base / 'prerelease-artifacts'
        build_tree(pre_tree, pre_version, pre_sha)
        pre_root = base / 'prerelease'
        handler2 = type('H2', (http.server.SimpleHTTPRequestHandler,), {
            '__init__': lambda self, *a, **k: http.server.SimpleHTTPRequestHandler.__init__(self, *a, directory=str(pre_root), **k),
            'log_message': lambda self, *a: None})
        server2 = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler2)
        threading.Thread(target=server2.serve_forever, daemon=True).start()
        url2 = f'http://127.0.0.1:{server2.server_port}'
        try:
            assemble('prerelease-assemble', pre_tree, pre_root, base_url=f'{url2}/release-assets', source_record_path=pre_record, sha=pre_sha, ver=pre_version)
            reg = json.loads((pre_root / 'evidence/registration.json').read_text())
            run.assert_('pre-release registration: tag, prerelease flag and six artifacts', reg['tagName'] == f'v{pre_version}'
                        and reg['prerelease'] is True and len(reg['artifacts']) == 6)
            run.assert_('pre-release names are spelled X.Y.Z-alpha.N in every file name',
                        (pre_root / f'release-assets/UniClipboard_{pre_version}_amd64.deb').exists()
                        and (pre_root / f'release-assets/UniClipboard-{pre_version}-1.x86_64.rpm').exists())
            run.exec('prerelease-consume-six-platforms-over-http', [probe, 'consume', config, f'{url2}/evidence/manifest.json'])
        finally:
            server2.shutdown()
            server2.server_close()
        variant_tree = base / 'prerelease-wrong-sha-artifacts'
        build_tree(variant_tree, pre_version, head)
        assemble('negative-prerelease-evidence-from-another-commit', variant_tree, base / 'prerelease-wrong-sha', source_record_path=pre_record,
                 sha=pre_sha, ver=pre_version, want_ok=False, contains='not the pinned source')

        # ---- release mode keeps every strictness ----
        run.exec('release-mode-refuses-fixture-signer', ['python3', '-I', ASSEMBLE, '--artifacts', tree, '--work', base / 'rm1', '--version', version,
                 '--channel', 'alpha', '--source-sha', head, '--source-record', record, '--base-url', 'http://127.0.0.1:1', '--notes-file', notes,
                 '--zh-notes-file', zh, '--registration-source', 'x', '--mode', 'release', '--signer', signer], env=sign_env, want_ok=False,
                 contains='release mode signs with')
        run.exec('release-mode-refuses-fixture-source-record', ['python3', '-I', ASSEMBLE, '--artifacts', tree, '--work', base / 'rm2', '--version', version,
                 '--channel', 'alpha', '--source-sha', head, '--source-record', record, '--base-url', 'http://127.0.0.1:1', '--notes-file', notes,
                 '--zh-notes-file', zh, '--registration-source', 'x', '--mode', 'release'], env=sign_env, want_ok=False,
                 contains='does not describe')
        release_record = out / 'release-mode-record-handwritten-for-this-case.json'
        release_record.write_text(json.dumps(dict(recorded, mode='release')))
        run.exec('release-mode-rejects-throwaway-key-against-production-pubkey', ['python3', '-I', ASSEMBLE, '--artifacts', tree, '--work', base / 'rm3',
                 '--version', version, '--channel', 'alpha', '--source-sha', head, '--source-record', release_record, '--base-url', 'http://127.0.0.1:1',
                 '--notes-file', notes, '--zh-notes-file', zh, '--registration-source', 'x', '--mode', 'release'], env=sign_env, want_ok=False)
        run.assert_('that run signed nothing', not list((base / 'rm3/release-assets').glob('*.sig')))

        # ---- production prerequisites fail closed ----
        pre = lambda label, extra, ok, text=None: run.exec(label, ['python3', '-I', GATE, 'prerequisites'], env=dict(env, **extra), want_ok=ok, contains=text)
        pre('prerequisites-none', {}, False, 'WINDOWS_SIGN_BACKEND')
        pre('prerequisites-signpath-test-backend', {'WINDOWS_SIGN_BACKEND': 'signpath-test', 'TAURI_SIGNING_PRIVATE_KEY': 'k'}, False, 'WINDOWS_SIGN_BACKEND')
        pre('prerequisites-selftest-backend', {'WINDOWS_SIGN_BACKEND': 'selftest', 'TAURI_SIGNING_PRIVATE_KEY': 'k'}, False, 'WINDOWS_SIGN_BACKEND')
        pre('prerequisites-no-updater-key', {'WINDOWS_SIGN_BACKEND': 'azure'}, False, 'TAURI_SIGNING_PRIVATE_KEY')
        pre('prerequisites-presence-only-passes', {'WINDOWS_SIGN_BACKEND': 'azure', 'TAURI_SIGNING_PRIVATE_KEY': 'k'}, True)

        # ---- scope, provenance and index ----
        for f in out.rglob('*'):
            if f.is_file() and f.name == 'key':
                sys.exit('a private key file reached the evidence directory')
        tool = lambda *c: subprocess.run(c, capture_output=True, text=True).stdout.strip()
        (out / 'scope.json').write_text(json.dumps({
            'assertions': len(run.checks), 'sourceSha': head, 'version': version, 'enginePin': recorded['enginePin'],
            'tools': {'go': tool('go', 'version'), 'node': tool('node', '--version'), 'python': sys.version.split()[0]},
            'synthetic': True, 'realPackages': False, 'productionUpdaterKey': False, 'productionWindowsSigning': 'blocked: no production backend, not run',
            'externalWrites': 'none (proxies to a dead port, tokens removed, loopback HTTP only)',
            'rerun': f'python3 -I apps/gui-go/e2e/release_assembly_run.py --out <new empty dir>'}, indent=2, sort_keys=True) + '\n')
        shutil.rmtree(base / 'positive-artifacts', ignore_errors=False)
        (out / 'SHA256SUMS.txt').write_text(''.join(f'{sha256(f)}  {f.relative_to(out)}\n' for f in sorted(out.rglob('*')) if f.is_file()))
    print(f'{len(run.checks)} assertions passed')


if __name__ == '__main__':
    main()
