"""Exercise the actual Snap prime gate with complete and incomplete runtime trees."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

RECIPE = Path(__file__).resolve().parents[2] / 'snap/snapcraft.yaml'
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
        self.script = yaml.safe_load(RECIPE.read_text())['parts']['uniclipboard']['override-prime']

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
            'CRAFT_ARCH_TRIPLET': triplet,
        }, capture_output=True).returncode

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

    def test_old_staged_gtk_cannot_shadow_the_content_runtime(self):
        triplet = 'aarch64-linux-gnu'
        libdir = self.populate(triplet)
        (libdir / 'libgtk-4.so.1').write_bytes(b'old apt GTK')
        self.assertNotEqual(self.run_gate(triplet), 0)


if __name__ == '__main__':
    unittest.main()
