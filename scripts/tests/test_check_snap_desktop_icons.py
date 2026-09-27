"""Exercise snap desktop-entry icon validation against extracted snap trees."""
import importlib.util
import pathlib
import struct
import tempfile
import unittest
import zlib

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "check-snap-desktop-icons.py"
SPEC = importlib.util.spec_from_file_location("snap_desktop_icons", SCRIPT)
icons = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(icons)


def png(width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IEND", b"")


class SnapDesktopIconChecks(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = pathlib.Path(directory.name)
        self.write("usr/share/icons/hicolor/256x256/apps/uniclipboard.png", png(256, 256))
        # Empty size directories staged by hicolor-icon-theme, as in the real snap.
        for size in ("96x96", "512x512"):
            (self.root / f"usr/share/icons/hicolor/{size}/apps").mkdir(parents=True)

    def write(self, relative, data):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def desktop(self, icon):
        self.write(
            "meta/gui/uniclipboard.desktop",
            f"[Desktop Entry]\nName=UniClipboard\nExec=uniclipboard %U\nIcon={icon}\n".encode(),
        )

    def test_resolvable_png_passes(self):
        self.desktop("${SNAP}/usr/share/icons/hicolor/256x256/apps/uniclipboard.png")
        icons.check_tree(self.root)

    def test_nonexistent_theme_guess_is_rejected(self):
        self.desktop("${SNAP}/usr/share/icons/hicolor/96x96/apps/uniclipboard.xpm")
        with self.assertRaisesRegex(RuntimeError, "does not exist"):
            icons.check_tree(self.root)

    def test_unresolved_theme_name_is_rejected(self):
        self.desktop("uniclipboard")
        with self.assertRaisesRegex(RuntimeError, r"not a \$\{SNAP\} path"):
            icons.check_tree(self.root)

    def test_path_escaping_snap_is_rejected(self):
        self.desktop("${SNAP}/../etc/uniclipboard.png")
        with self.assertRaisesRegex(RuntimeError, "escapes"):
            icons.check_tree(self.root)

    def test_content_not_matching_extension_is_rejected(self):
        self.write("usr/share/icons/hicolor/256x256/apps/uniclipboard.png", b"GIF89a")
        self.desktop("${SNAP}/usr/share/icons/hicolor/256x256/apps/uniclipboard.png")
        with self.assertRaisesRegex(RuntimeError, "not a valid PNG"):
            icons.check_tree(self.root)

    def test_png_size_not_matching_theme_directory_is_rejected(self):
        self.write("usr/share/icons/hicolor/256x256/apps/uniclipboard.png", png(128, 128))
        self.desktop("${SNAP}/usr/share/icons/hicolor/256x256/apps/uniclipboard.png")
        with self.assertRaisesRegex(RuntimeError, "128x128"):
            icons.check_tree(self.root)

    def test_snap_without_desktop_entries_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "No desktop entries"):
            icons.check_tree(self.root)


if __name__ == "__main__":
    unittest.main()
