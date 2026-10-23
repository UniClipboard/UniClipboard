"""Exercise the actual Snap prime gate with complete and incomplete runtime trees."""
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import unittest

import yaml

RECIPE = Path(__file__).resolve().parents[2] / 'snap/snapcraft.yaml'
ROOT = RECIPE.parents[1]
ASSETS = (
    'libwebkitgtk-6.0.so.4',
    'libjavascriptcoregtk-6.0.so.1',
    'webkitgtk-6.0/WebKitWebProcess',
    'webkitgtk-6.0/WebKitNetworkProcess',
    'webkitgtk-6.0/injected-bundle/libwebkitgtkinjectedbundle.so',
)


class SnapGTK4PrimeGate(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        craftctl = self.bin / 'craftctl'
        craftctl.write_text('#!/bin/sh\nexit 0\n')
        craftctl.chmod(0o755)
        self.platform = self.root / 'platform'
        self.platform.mkdir()
        self.script = yaml.safe_load(RECIPE.read_text())['parts']['platform-cleanup']['override-prime']
        self.script = self.script.replace('/snap/gnome-46-2404/current', str(self.platform))

    def populate(self, triplet):
        libdir = self.root / 'prime/usr/lib' / triplet
        for relative in ASSETS:
            path = libdir / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fixture')
            if path.name in ('WebKitWebProcess', 'WebKitNetworkProcess'):
                path.chmod(0o755)
        return libdir

    def run_gate(self, triplet):
        return subprocess.run(['bash', '-c', self.script], env={
            **os.environ,
            'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
            'CRAFT_PRIME': str(self.root / 'prime'),
            'CRAFT_PROJECT_DIR': str(ROOT),
            'CRAFT_ARCH_TRIPLET_BUILD_FOR': triplet,
        }, capture_output=True).returncode

    def test_declared_instance_scope_drives_the_build_and_rejects_a_mismatched_ui_slot(self):
        recipe = yaml.safe_load(RECIPE.read_text())
        script = recipe['parts']['uniclipboard']['override-build']
        start = script.index('INSTANCE_SCOPE=')
        end = script.index('(cd apps/gui-go &&', start)
        command = script[start:end] + '\nprintf "%s\\n" "$INSTANCE_SCOPE"'
        self.assertIn('-X main.singleInstanceScope=${INSTANCE_SCOPE}', script)
        for slot in ('gui-instance', 'gui-ui-instance'):
            self.assertIn(slot, recipe['apps']['uniclipboard']['slots'])
            self.assertEqual(recipe['slots'][slot]['bus'], 'session')
            self.assertEqual(recipe['slots'][slot]['interface'], 'dbus')
        fixture = self.root / 'snap/snapcraft.yaml'
        fixture.parent.mkdir()
        fixture.write_text(yaml.safe_dump(recipe))
        result = subprocess.run(['bash', '-eu', '-c', command], cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        scope = result.stdout.strip()
        self.assertEqual(recipe['slots']['gui-instance']['name'], scope + '.SingleInstance')
        self.assertEqual(recipe['slots']['gui-ui-instance']['name'], scope + '.ui.SingleInstance')
        recipe['slots']['gui-ui-instance']['name'] = 'invalid.fixture.SingleInstance'
        fixture.write_text(yaml.safe_dump(recipe))
        result = subprocess.run(['bash', '-eu', '-c', command], cwd=self.root, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)

    def test_complete_runtime_for_both_architectures(self):
        for triplet in ('x86_64-linux-gnu', 'aarch64-linux-gnu'):
            with self.subTest(triplet=triplet):
                self.populate(triplet)
                self.assertEqual(self.run_gate(triplet), 0)

    def test_every_missing_asset_is_rejected(self):
        triplet = 'aarch64-linux-gnu'
        for relative in ASSETS:
            with self.subTest(asset=relative):
                libdir = self.populate(triplet)
                (libdir / relative).unlink()
                self.assertNotEqual(self.run_gate(triplet), 0)

    def test_nonexecutable_web_process_is_rejected(self):
        triplet = 'x86_64-linux-gnu'
        libdir = self.populate(triplet)
        (libdir / 'webkitgtk-6.0/WebKitWebProcess').chmod(0o644)
        self.assertNotEqual(self.run_gate(triplet), 0)

    def test_old_staged_gtk_is_removed_in_favor_of_the_content_runtime(self):
        triplet = 'aarch64-linux-gnu'
        libdir = self.populate(triplet)
        (libdir / 'libgtk-4.so.1').write_bytes(b'old apt GTK')
        platform_lib = self.platform / libdir.relative_to(self.root / 'prime')
        platform_lib.mkdir(parents=True)
        (platform_lib / 'libgtk-4.so.1').write_bytes(b'platform GTK')
        self.assertEqual(self.run_gate(triplet), 0)
        self.assertFalse((libdir / 'libgtk-4.so.1').exists())

    @unittest.skipUnless(shutil.which('gcc') and shutil.which('readelf'), 'requires native ELF tools')
    def test_platform_soname_ownership_resolves_real_harfbuzz_symbol_conflict(self):
        triplet = 'aarch64-linux-gnu'
        app = self.populate(triplet)
        platform = self.platform / app.relative_to(self.root / 'prime')
        platform.mkdir(parents=True)
        def build(name, source, *arguments):
            code = self.root / (name + '.c')
            code.write_text(source)
            subprocess.run(['gcc', str(code), '-o', str(self.root / name), *arguments], check=True)

        old = app / 'libharfbuzz.so.0.80000'
        new = platform / 'libharfbuzz.so.0.120000'
        build('old', 'void hb_legacy(void) {}', '-shared', '-fPIC', '-Wl,-soname,libharfbuzz.so.0')
        shutil.move(self.root / 'old', old)
        (app / 'libharfbuzz.so.0').symlink_to(old.name)
        build('new', 'void hb_free(void) {}', '-shared', '-fPIC', '-Wl,-soname,libharfbuzz.so.0')
        shutil.move(self.root / 'new', new)
        (platform / 'libharfbuzz.so.0').symlink_to(new.name)
        build('subset', 'void hb_free(void); void subset(void) { hb_free(); }',
              '-shared', '-fPIC', '-Wl,-soname,libharfbuzz-subset.so.0',
              '-L' + str(platform), '-l:libharfbuzz.so.0')
        shutil.move(self.root / 'subset', platform / 'libharfbuzz-subset.so.0')
        build('probe', 'void subset(void); int main(void) { subset(); return 0; }',
              '-L' + str(platform), '-l:libharfbuzz-subset.so.0', '-l:libharfbuzz.so.0')
        env = {**os.environ, 'LD_LIBRARY_PATH': str(app) + os.pathsep + str(platform)}
        broken = subprocess.run([str(self.root / 'probe')], env=env, capture_output=True)
        self.assertNotEqual(broken.returncode, 0)
        self.assertIn(b'hb_free', broken.stderr)
        self.assertEqual(self.run_gate(triplet), 0)
        self.assertFalse(old.exists())
        self.assertFalse((app / 'libharfbuzz.so.0').is_symlink())
        self.assertEqual(subprocess.run([str(self.root / 'probe')], env=env).returncode, 0)


if __name__ == '__main__':
    unittest.main()
