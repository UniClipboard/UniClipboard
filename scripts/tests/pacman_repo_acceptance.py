#!/usr/bin/env python3
"""Exercise real libalpm transactions with disposable roots and GPG keyrings.

Without --package-dir, use tiny packages to verify installation, upgrade, removal,
version ordering, and signature rejection without compiling the desktop app.
"""
import argparse
import functools
import http.server
import os
from pathlib import Path
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[2]


def run(*args, env=None, success=True):
    result = subprocess.run([str(arg) for arg in args], env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if (result.returncode == 0) != success:
        raise RuntimeError(f'{args}: exit {result.returncode}\n{result.stdout}')
    return result.stdout


def fixture(directory, version, name='uniclipboard'):
    tree = directory / (name + version)
    (tree / 'usr/share/uniclipboard').mkdir(parents=True)
    (tree / 'usr/share/uniclipboard/version').write_text(version)
    (tree / '.PKGINFO').write_text(f'pkgname = {name}\npkgver = {version}\n'
                                 'pkgdesc = repository acceptance fixture\narch = x86_64\n'
                                 'size = 10\nconflict = uniclipboard-git\nconflict = uniclipboard-bin\n')
    package = directory / f'{name}-{version}-x86_64.pkg.tar.zst'
    run('bsdtar', '--zstd', '-cf', package, '-C', tree, '.PKGINFO', 'usr')
    return package


def main(package_dir):
    if os.geteuid() != 0:
        raise SystemExit('run in a disposable root container or unshare --user --map-root-user')
    with tempfile.TemporaryDirectory(prefix='uc-pacman-') as tmp:
        base = Path(tmp)
        home = base / 'gnupg'
        home.mkdir(mode=0o700)
        env = dict(os.environ, GNUPGHOME=str(home))
        run('gpg', '--batch', '--passphrase', '', '--quick-generate-key',
            'Repository acceptance <acceptance@example.invalid>', 'ed25519', 'sign', '0', env=env)
        fingerprint = next(line.split(':')[9] for line in run(
            'gpg', '--with-colons', '--list-keys', env=env).splitlines() if line.startswith('fpr:'))
        packages = sorted(package_dir.glob('*.pkg.tar.zst')) if package_dir else [
            fixture(base, '1.0.0-1'), fixture(base, '1.0.1-1')]
        assert len(packages) == (1 if package_dir else 2)
        web = base / 'web'
        web.mkdir()
        for index, package in enumerate(packages):
            run('python3', ROOT / 'scripts/ci/assemble-pacman-repo.py', package,
                web / str(index), '--arch', 'x86_64', '--fingerprint', fingerprint, env=env)
        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(web))
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            root = base / 'root'
            database = base / 'database'
            cache = base / 'cache'
            keyring = base / 'pacman-gnupg'
            for directory in (root, database, cache):
                directory.mkdir()
            run('pacman-key', '--gpgdir', keyring, '--init')
            run('pacman-key', '--gpgdir', keyring, '--add', web / '0/uniclipboard.asc')
            run('pacman-key', '--gpgdir', keyring, '--lsign-key', fingerprint)
            config = base / 'pacman.conf'
            def configure(index):
                config.write_text(f'[options]\nArchitecture = x86_64\nSigLevel = Required\n'
                                  f'[uniclipboard]\nServer = http://127.0.0.1:{server.server_port}/{index}/$arch\n')
            def pacman(*args, success=True):
                return run('pacman', '--config', config, '--root', root, '--dbpath', database,
                           '--cachedir', cache, '--gpgdir', keyring, '--logfile', base / 'pacman.log',
                           '--noconfirm', *args, success=success)
            configure(0)
            pacman('-Syy')
            # A real desktop package uses system dependencies; download-only validates the
            # signed bytes without pretending a dependency-free root can launch its GUI.
            if package_dir:
                pacman('-Swdd', 'uniclipboard')
            else:
                old = fixture(base, '0.9.0-1', 'uniclipboard-git')
                run('gpg', '--batch', '--local-user', fingerprint, '--detach-sign', old, env=env)
                pacman('-U', old)
                pacman('-S', 'uniclipboard', success=False)
                assert 'uniclipboard-git' in pacman('-Q')
                pacman('-R', 'uniclipboard-git')
                pacman('-S', 'uniclipboard')
                assert (root / 'usr/share/uniclipboard/version').read_text() == '1.0.0-1'
                pacman('-Qo', root / 'usr/share/uniclipboard/version')
                configure(1)
                pacman('-Syyu')
                assert (root / 'usr/share/uniclipboard/version').read_text() == '1.0.1-1'
                pacman('-R', 'uniclipboard')
                assert not (root / 'usr/share/uniclipboard/version').exists()
            # A database must not advertise an unavailable package.
            configure(0)
            for path in cache.iterdir():
                if path.is_file():
                    path.unlink()
            archive = next((web / '0/x86_64').glob('*.pkg.tar.zst'))
            hidden = archive.with_suffix('.hidden')
            archive.rename(hidden)
            pacman('-Syy')
            pacman('-Swdd', 'uniclipboard', success=False)
            hidden.rename(archive)
            # A valid signature by an untrusted key must also be rejected.
            run('gpg', '--batch', '--passphrase', '', '--quick-generate-key',
                'Untrusted signer <untrusted@example.invalid>', 'ed25519', 'sign', '0', env=env)
            rogue = [line.split(':')[9] for line in run('gpg', '--with-colons', '--list-keys',
                     'untrusted@example.invalid', env=env).splitlines() if line.startswith('fpr:')][0]
            signature = web / '0/x86_64/uniclipboard.db.sig'
            original_signature = signature.read_bytes()
            run('gpg', '--batch', '--yes', '--local-user', rogue, '--output', signature,
                '--detach-sign', web / '0/x86_64/uniclipboard.db', env=env)
            pacman('-Syy', success=False)
            signature.write_bytes(original_signature)
            # Corrupt the signed database and require the real sync operation to fail.
            (web / '0/x86_64/uniclipboard.db').write_bytes(b'invalid database')
            configure(0)
            pacman('-Syy', success=False)
            for path in cache.iterdir():
                if path.is_file():
                    path.unlink()
            # Restore the database alias, then corrupt the package after signing.
            arch = web / '0/x86_64'
            (arch / 'uniclipboard.db').write_bytes((arch / 'uniclipboard.db.tar.gz').read_bytes())
            next(arch.glob('*.pkg.tar.zst')).write_bytes(b'corrupt package')
            pacman('-Syy')
            pacman('-Swdd', 'uniclipboard', success=False)
            assert run('vercmp', '1.0.1-1', '1.0.0-1').strip() == '1'
            print('PASS: signed sync, package verification, tampering rejection' +
                  ('' if package_dir else ', install, AUR conflict, ownership, upgrade, removal'))
        finally:
            server.shutdown()
            run('gpgconf', '--kill', 'all', env=env)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package-dir', type=Path)
    main(parser.parse_args().package_dir)
