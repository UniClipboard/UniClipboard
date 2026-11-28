#!/usr/bin/env python3
"""Unit tests for macos_dmg whole-disk extraction, pre-mount indexing suppression, and detach retries."""

from __future__ import annotations

from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import macos_dmg  # noqa: E402


def _hdiutil_info_plist(image_path: Path, disk: str = "/dev/disk2") -> str:
    payload = {
        "images": [
            {
                "image-path": str(image_path.resolve()),
                "system-entities": [
                    {"content-hint": "Apple_partition_scheme", "dev-entry": disk},
                    {
                        "content-hint": "Apple_HFS",
                        "dev-entry": f"{disk}s1",
                        "mount-point": "/tmp/mnt",
                    },
                ],
            }
        ]
    }
    return plistlib.dumps(payload).decode("utf-8")


class MacosDmgDetachTests(unittest.TestCase):
    def test_extract_whole_disk_prefers_parent_device(self):
        entities = [
            {"content-hint": "GUID_partition_scheme", "dev-entry": "/dev/disk4"},
            {"content-hint": "Apple_HFS", "dev-entry": "/dev/disk4s1", "mount-point": "/tmp/m"},
        ]
        self.assertEqual(macos_dmg._extract_whole_disk(entities), "/dev/disk4")

    def test_extract_whole_disk_normalizes_partition_slice(self):
        entities = [
            {"content-hint": "Apple_HFS", "dev-entry": "/dev/disk7s1", "mount-point": "/tmp/m"},
        ]
        self.assertEqual(macos_dmg._extract_whole_disk(entities), "/dev/disk7")

    def test_detach_graceful_success_after_transient_busy(self):
        calls = []

        def fake_run(args, **kwargs):
            calls.append(args)
            if len(calls) < 3:
                return subprocess.CompletedProcess(args, 16, stdout="", stderr="Resource busy")
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

        with mock.patch.object(macos_dmg.subprocess, "run", side_effect=fake_run), mock.patch.object(
            macos_dmg.time, "sleep"
        ):
            macos_dmg._detach("/dev/disk2")

        self.assertEqual(len(calls), 3)
        self.assertTrue(all(c == ["hdiutil", "detach", "/dev/disk2"] for c in calls))

    def test_detach_force_uses_bound_diskutil_and_retries(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp) / "rw.dmg"
            img.write_bytes(b"")
            info_xml = _hdiutil_info_plist(img, "/dev/disk2")
            calls = []
            force_attempts = 0

            def fake_run(args, **kwargs):
                nonlocal force_attempts
                calls.append(args)
                if args[:2] == ["hdiutil", "info"]:
                    return subprocess.CompletedProcess(args, 0, stdout=info_xml, stderr="")
                if args == ["hdiutil", "detach", "/dev/disk2"]:
                    return subprocess.CompletedProcess(args, 16, stdout="", stderr="Resource busy")
                if args == ["diskutil", "unmountDisk", "force", "/dev/disk2"]:
                    return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
                if args == ["hdiutil", "detach", "-force", "/dev/disk2"]:
                    force_attempts += 1
                    if force_attempts < 3:
                        return subprocess.CompletedProcess(
                            args, 16, stdout="", stderr="hdiutil: couldn't eject \"disk2\" - Resource busy"
                        )
                    return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
                raise AssertionError(f"unexpected command: {args}")

            with mock.patch.object(macos_dmg.subprocess, "run", side_effect=fake_run), mock.patch.object(
                macos_dmg.time, "sleep"
            ):
                macos_dmg._detach("/dev/disk2", image_path=img)

            self.assertEqual(force_attempts, 3)
            self.assertIn(["diskutil", "unmountDisk", "force", "/dev/disk2"], calls)

    def test_detach_idempotent_when_already_detached(self):
        empty_info = plistlib.dumps({"images": []}).decode("utf-8")

        def fake_run(args, **kwargs):
            if args[:2] == ["hdiutil", "info"]:
                return subprocess.CompletedProcess(args, 0, stdout=empty_info, stderr="")
            return subprocess.CompletedProcess(
                args, 1, stdout="", stderr="hdiutil: detach failed - No such file or directory"
            )

        with mock.patch.object(macos_dmg.subprocess, "run", side_effect=fake_run), mock.patch.object(
            macos_dmg.time, "sleep"
        ):
            macos_dmg._detach("/dev/disk2")

    def test_detach_refuses_force_if_device_bound_to_different_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            ours = Path(tmp) / "ours.dmg"
            other = Path(tmp) / "other.dmg"
            ours.write_bytes(b"")
            other.write_bytes(b"")
            other_info = _hdiutil_info_plist(other, "/dev/disk2")
            seen_force = False

            def fake_run(args, **kwargs):
                nonlocal seen_force
                if args[:2] == ["hdiutil", "info"]:
                    return subprocess.CompletedProcess(args, 0, stdout=other_info, stderr="")
                if args[0] == "diskutil" or "-force" in args:
                    seen_force = True
                return subprocess.CompletedProcess(args, 16, stdout="", stderr="Resource busy")

            with mock.patch.object(macos_dmg.subprocess, "run", side_effect=fake_run), mock.patch.object(
                macos_dmg.time, "sleep"
            ):
                with self.assertRaisesRegex(RuntimeError, "not bound"):
                    macos_dmg._detach("/dev/disk2", image_path=ours)

            self.assertFalse(seen_force)

    def test_detach_raises_with_diagnostics_after_exhausting_force_retries(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp) / "rw.dmg"
            img.write_bytes(b"")
            info_xml = _hdiutil_info_plist(img, "/dev/disk2")

            def fake_run(args, **kwargs):
                if args[:2] == ["hdiutil", "info"]:
                    return subprocess.CompletedProcess(args, 0, stdout=info_xml, stderr="")
                if args[0] == "lsof":
                    return subprocess.CompletedProcess(args, 0, stdout="p1234\ncmds_stores\n", stderr="")
                if args[0] == "diskutil":
                    return subprocess.CompletedProcess(args, 1, stdout="", stderr="busy")
                return subprocess.CompletedProcess(
                    args, 16, stdout="", stderr="hdiutil: couldn't eject \"disk2\" - Resource busy"
                )

            with mock.patch.object(macos_dmg.subprocess, "run", side_effect=fake_run), mock.patch.object(
                macos_dmg.time, "sleep"
            ):
                with self.assertRaises(RuntimeError) as ctx:
                    macos_dmg._detach("/dev/disk2", image_path=img, mountpoint=Path(tmp) / "mnt")

            msg = str(ctx.exception)
            self.assertIn("exit 16", msg)
            self.assertIn("mds_stores(1234)", msg)

    def test_build_dmg_stages_fsevents_no_log_before_create(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bundle = root / "UniClipboard.app"
            bundle.mkdir()
            bg = root / "bg.tiff"
            bg.write_bytes(b"bg")
            icon = root / "icon.icns"
            icon.write_bytes(b"icns")
            out_dmg = root / "out.dmg"

            saw_fsevents_before_create = False
            detached_targets = []

            def fake_run(args, **kwargs):
                nonlocal saw_fsevents_before_create
                cmd = [str(x) for x in args]
                if cmd[:2] == ["hdiutil", "create"]:
                    src_idx = cmd.index("-srcfolder") + 1
                    staging = Path(cmd[src_idx])
                    saw_fsevents_before_create = (staging / ".fseventsd" / "no_log").is_file()
                    Path(cmd[-1]).write_bytes(b"rw")
                    return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
                if cmd[:2] == ["hdiutil", "attach"]:
                    rw_path = Path(cmd[-1])
                    mnt = Path(cmd[cmd.index("-mountpoint") + 1])
                    (mnt / ".background").mkdir(exist_ok=True)
                    (mnt / ".background" / "background.tiff").write_bytes(b"bg")
                    (mnt / ".VolumeIcon.icns").write_bytes(b"icns")
                    xml = _hdiutil_info_plist(rw_path, "/dev/disk9")
                    # attach -plist returns dict with system-entities at top level
                    entities = plistlib.loads(xml.encode())["images"][0]["system-entities"]
                    return subprocess.CompletedProcess(
                        cmd,
                        0,
                        stdout=plistlib.dumps({"system-entities": entities}).decode("utf-8"),
                        stderr="",
                    )
                if cmd[:2] == ["hdiutil", "convert"]:
                    out_dmg.write_bytes(b"ulfo")
                    return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")
                return subprocess.CompletedProcess(cmd, 0, stdout="", stderr="")

            def fake_detach(device, **kwargs):
                detached_targets.append((str(device), kwargs))

            with mock.patch.object(macos_dmg, "_run", side_effect=fake_run), mock.patch.object(
                macos_dmg, "_set_file_attr"
            ), mock.patch.object(
                macos_dmg, "create_hfs_alias", return_value=b"alias"
            ), mock.patch.object(
                macos_dmg, "create_bookmark", return_value=b"bookmark"
            ), mock.patch.object(
                macos_dmg, "_detach", side_effect=fake_detach
            ):
                macos_dmg.build_dmg(bundle, out_dmg, background=bg, volume_icon=icon)

            self.assertTrue(saw_fsevents_before_create)
            self.assertEqual(len(detached_targets), 1)
            self.assertEqual(detached_targets[0][0], "/dev/disk9")


if __name__ == "__main__":
    unittest.main()
