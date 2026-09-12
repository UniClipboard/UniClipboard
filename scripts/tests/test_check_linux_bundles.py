"""Exercise release rejection using package-tool responses and extracted ELF fixtures."""
import importlib.util
import pathlib
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check-linux-bundles.py"
SPEC = importlib.util.spec_from_file_location("linux_bundles", SCRIPT)
bundles = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundles)
LIBRARY = SCRIPT.parent.parent / "src-tauri/binaries/linux/libgtk-layer-shell.so.0"


@unittest.skipUnless(LIBRARY.is_file(), "Run prepare-linux-bundle.mjs first")
class BundleChecks(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = pathlib.Path(self.directory.name)
        for extension in ("deb", "rpm", "AppImage"):
            (self.root / f"panel.{extension}").touch()
        self.deb = "libc6 (>= 2.36), libgtk-layer-shell0 (>= 0.8), libgtk-3-0"
        self.rpm = "gtk3\ngtk-layer-shell\n"
        self.library = LIBRARY.read_bytes()
        original_output = bundles.output
        original_run = bundles.subprocess.run

        def output(*args):
            if args[0] == "dpkg-deb":
                return self.deb
            if args[0] == "rpm":
                return self.rpm
            return original_output(*args)

        def extract(args, **kwargs):
            if not str(args[0]).endswith(".AppImage"):
                return original_run(args, **kwargs)
            cwd = kwargs["cwd"]
            if self.library is not None:
                target = pathlib.Path(cwd) / "squashfs-root/usr/lib/libgtk-layer-shell.so.0"
                target.parent.mkdir(parents=True)
                target.write_bytes(self.library)

        self.enterContext(patch.object(bundles, "output", side_effect=output))
        self.enterContext(patch.object(bundles.subprocess, "run", side_effect=extract))

    def test_complete_packages_pass(self):
        bundles.check(self.root)

    def test_missing_deb_dependency_fails(self):
        self.deb = "libgtk-3-0, libgtk4-layer-shell0"
        with self.assertRaisesRegex(RuntimeError, "missing libgtk-layer-shell0"):
            bundles.check(self.root)

    def test_missing_rpm_dependency_fails(self):
        self.rpm = "gtk3\ngtk4-layer-shell\n"
        with self.assertRaisesRegex(RuntimeError, "missing gtk-layer-shell"):
            bundles.check(self.root)

    def test_missing_appimage_library_fails(self):
        self.library = None
        with self.assertRaisesRegex(RuntimeError, "missing or incorrect"):
            bundles.check(self.root)

    def test_wrong_appimage_architecture_fails(self):
        data = bytearray(self.library)
        data[18:20] = b"\x00\x00"
        self.library = data
        with self.assertRaisesRegex(RuntimeError, "architecture"):
            bundles.check(self.root)

    def test_missing_artifacts_fail(self):
        with self.assertRaisesRegex(RuntimeError, "No deb artifact"):
            bundles.check(self.root / "absent")


if __name__ == "__main__":
    unittest.main()
