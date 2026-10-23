"""Exercise GTK ABI rejection and real deb/rpm metadata with disposable ELF fixtures."""
import hashlib
import importlib.util
from pathlib import Path
import platform
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'apps/gui-go/e2e'))
import package_linux as package

spec = importlib.util.spec_from_file_location('nix_update', ROOT / 'packaging/nix/update.py')
nix_update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(nix_update)


@unittest.skipUnless(shutil.which('gcc') and shutil.which('readelf'), 'requires a native C toolchain')
class LinuxGTK4Packaging(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        source = self.root / 'main.c'
        source.write_text('int main(void) { return 0; }\n')
        for name in ('libgtk-4.so.1', 'libwebkitgtk-6.0.so.4', 'libgtk-3.so.0', 'libwebkit2gtk-4.1.so.0'):
            subprocess.run(['gcc', '-shared', '-fPIC', str(source), '-Wl,-soname,' + name,
                            '-o', str(self.root / name)], check=True, capture_output=True)
        self.source = source
        self.arch = {'aarch64': 'arm64', 'x86_64': 'amd64'}[platform.machine()]

    def binary(self, name, libraries):
        binary = self.root / name
        subprocess.run(['gcc', str(self.source), '-Wl,--no-as-needed', '-L' + str(self.root),
                        *['-l:' + name for name in libraries], '-o', str(binary)], check=True, capture_output=True)
        return binary

    def stage(self, binary, name='stage'):
        stage = self.root / name
        (stage / 'usr/bin').mkdir(parents=True)
        shutil.copy2(binary, stage / 'usr/bin/uniclipboard')
        shutil.copy2(binary, stage / 'usr/bin/uniclipd')
        return stage

    def test_stale_and_mixed_gtk_binaries_are_rejected(self):
        gtk4 = ['libgtk-4.so.1', 'libwebkitgtk-6.0.so.4']
        package.require_gtk4_binary(self.binary('valid', gtk4))
        for libraries in (['libgtk-3.so.0', 'libwebkit2gtk-4.1.so.0'], gtk4 + ['libgtk-3.so.0'], ['libgtk-4.so.1']):
            with self.subTest(libraries=libraries), self.assertRaises(SystemExit):
                package.require_gtk4_binary(self.binary('invalid', libraries))

    def test_helper_relocation_keeps_both_architecture_paths_the_same_length(self):
        for triplet in ('x86_64-linux-gnu', 'aarch64-linux-gnu'):
            with self.subTest(triplet=triplet):
                appdir = self.root / triplet
                libdir = appdir / 'usr/lib'
                libdir.mkdir(parents=True)
                library = libdir / 'libwebkitgtk-6.0.so.4'
                old = f'/usr/lib/{triplet}/webkitgtk-6.0'
                original = b'prefix\0' + old.encode() + b'/WebKitWebProcess\0suffix'
                library.write_bytes(original)
                result = package.relocate_webkit(appdir, Path(old))
                self.assertEqual(len(library.read_bytes()), len(original))
                self.assertNotIn(old.encode(), library.read_bytes())
                self.assertIn(result['to'].encode() + b'/WebKitWebProcess', library.read_bytes())

    @unittest.skipUnless(shutil.which('dpkg-deb'), 'requires dpkg-deb')
    def test_real_deb_and_nix_pin_require_verified_gtk4_payloads(self):
        binary = self.binary('gui', ['libgtk-4.so.1', 'libwebkitgtk-6.0.so.4'])
        stage = self.stage(binary)
        sums = []
        for arch in ('amd64', 'arm64'):
            out = self.root / arch
            out.mkdir()
            asset = f'UniClipboard_9.0.0_{arch}.deb'
            deb = package.build_deb(stage, out, '9.0.0', arch, asset)
            shutil.copy2(deb, self.root / asset)
            sums.append(f'{hashlib.sha256(deb.read_bytes()).hexdigest()}  {asset}\n')
            depends = subprocess.check_output(['dpkg-deb', '-f', str(deb), 'Depends'], text=True)
            self.assertIn('libgtk-4-1 (>= 4.10)', depends)
            self.assertIn('libwebkitgtk-6.0-4', depends)
            self.assertNotIn('layer-shell', depends)
        manifest = self.root / 'SHA256SUMS.txt'
        manifest.write_text(''.join(sums))
        pin = nix_update.verified_sources('9.0.0', manifest, self.root)
        self.assertEqual(pin['gtkMajor'], 4)
        self.assertEqual(set(pin['hashes']), {'aarch64-linux', 'x86_64-linux'})
        # A changed artifact cannot be accepted merely because its ABI is right.
        first = self.root / 'UniClipboard_9.0.0_amd64.deb'
        first.write_bytes(first.read_bytes() + b'changed')
        with self.assertRaisesRegex(SystemExit, 'SHA-256 differs'):
            nix_update.verified_sources('9.0.0', manifest, self.root)
        # Even a correctly hashed old GTK3 deb cannot be relabelled as GTK4.
        old = self.binary('old', ['libgtk-3.so.0', 'libwebkit2gtk-4.1.so.0'])
        out = self.root / 'old-deb'
        out.mkdir()
        deb = package.build_deb(self.stage(old, 'old-stage'), out, '9.0.0', 'amd64', first.name)
        shutil.copy2(deb, first)
        manifest.write_text(f'{hashlib.sha256(first.read_bytes()).hexdigest()}  {first.name}\n')
        with self.assertRaisesRegex(SystemExit, 'expected GTK4'):
            nix_update.verified_sources('9.0.0', manifest, self.root)

    @unittest.skipUnless(shutil.which('rpmbuild') and shutil.which('rpm'), 'requires RPM build tools')
    def test_real_rpm_declares_the_new_abi_and_matches_copr_contract(self):
        binary = self.binary('gui', ['libgtk-4.so.1', 'libwebkitgtk-6.0.so.4'])
        rpm = package.build_rpm(self.stage(binary), self.root, '9.0.0-alpha.1', self.arch, 'test.rpm')
        requirements = subprocess.check_output(['rpm', '-qpR', str(rpm)], text=True).splitlines()
        for requirement in package.RPM_REQUIRES.split(', '):
            self.assertIn(requirement, requirements)
        self.assertFalse(any('layer-shell' in n or 'libgtk-3' in n for n in requirements))
        # COPR must declare the same runtime capabilities as the upstream RPM.
        spec = (ROOT / 'packaging/uniclipboard.spec').read_text()
        copr_requires = [line.split(':', 1)[1].strip() for line in spec.splitlines() if line.startswith('Requires:')]
        self.assertEqual(set(copr_requires), set(package.RPM_REQUIRES.split(', ')))


class InstallerGTK4Compatibility(unittest.TestCase):
    def test_old_native_baselines_choose_snap_without_clobbering_the_requested_version(self):
        script = (ROOT / 'scripts/install.sh').read_text()
        start = script.index('needs_snap_gtk_runtime()')
        function = script[start:script.index('\n}', start) + 2]
        for distro, version, codename, expected in (
            ('ubuntu', '20.04', '', 0), ('ubuntu', '22.04', '', 0),
            ('linuxmint', '21.3', 'jammy', 0), ('debian', '12', '', 0),
            ('ubuntu', '24.04', '', 1), ('debian', '13', '', 1), ('arch', '', '', 1),
        ):
            with self.subTest(distro=distro, version=version):
                result = subprocess.run(['bash', '-c', function + '\nneeds_snap_gtk_runtime; rc=$?; '
                                         '[[ "$VERSION" == v9.0.0 ]] || exit 42; exit "$rc"'], env={
                    **os.environ, 'DISTRO_ID': distro, 'DISTRO_VERSION_ID': version,
                    'DISTRO_UBUNTU_CODENAME': codename, 'VERSION': 'v9.0.0',
                })
                self.assertEqual(result.returncode, expected)


if __name__ == '__main__':
    unittest.main()
