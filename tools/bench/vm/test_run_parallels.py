#!/usr/bin/env python3
"""Unit tests for tools/bench/vm/run_parallels.py."""

from __future__ import annotations

import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_parallels as rp  # noqa: E402


def make_minimal_pe(machine: int) -> bytes:
    pe_offset = 0x80
    buf = bytearray(pe_offset + 24)
    buf[0:2] = b"MZ"
    struct.pack_into("<I", buf, 0x3C, pe_offset)
    buf[pe_offset : pe_offset + 4] = b"PE\x00\x00"
    struct.pack_into("<H", buf, pe_offset + 4, machine)
    return bytes(buf)


class TestRunParallels(unittest.TestCase):
    def test_parse_vm_state(self) -> None:
        self.assertEqual(rp.parse_vm_state("Name: Windows 11\nState: suspended\nOS: win-11\n"), "suspended")
        self.assertEqual(rp.parse_vm_state("State: running\n"), "running")
        self.assertEqual(rp.parse_vm_state("State: stopped\n"), "stopped")
        with self.assertRaises(rp.BenchmarkError):
            rp.parse_vm_state("Name: Windows 11\n")

    def test_restore_vm_commands(self) -> None:
        self.assertEqual(rp.restore_vm_commands("Windows 11", "suspended"), [["prlctl", "suspend", "Windows 11"]])
        self.assertEqual(rp.restore_vm_commands("Windows 11", "stopped"), [["prlctl", "stop", "Windows 11"]])
        self.assertEqual(rp.restore_vm_commands("Windows 11", "running"), [])

    def test_pe_arch_and_binary_manifest_verification(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            stage = Path(tmp)
            for spec in rp.binary_specs():
                target = stage / spec["relpath"]
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(make_minimal_pe(0xAA64))
            manifest = rp.verify_staged_binaries(stage, "arm64")
            self.assertEqual(set(manifest.keys()), {"unic", "unicd", "unic-desktop", "unicd-bench-probe", "unic-desktop-e2e-probe", "unic-bench"})
            self.assertEqual(manifest["unic-desktop"]["arch"], "arm64")
            self.assertEqual(len(manifest["unic"]["sha256"]), 64)

            with self.assertRaises(rp.BenchmarkError):
                rp.verify_staged_binaries(stage, "amd64")

            # Corrupt PE header check
            bad = stage / "bin" / "unic-bench.exe"
            bad.write_bytes(b"not-a-pe")
            with self.assertRaises(rp.BenchmarkError):
                rp.inspect_pe_arch(bad)

    def test_build_go_command_flags(self) -> None:
        root = Path("/repo")
        stage = Path("/stage")
        by_key = {s["key"]: s for s in rp.binary_specs()}

        cmd_gui, env_gui, _ = rp.build_go_command(root, stage, "arm64", by_key["unic-desktop"])
        self.assertEqual(env_gui["CGO_ENABLED"], "0")
        self.assertEqual(env_gui["GOOS"], "windows")
        self.assertEqual(env_gui["GOARCH"], "arm64")
        self.assertIn("-ldflags=-s -w -H windowsgui", cmd_gui)
        self.assertNotIn("-tags", cmd_gui)

        cmd_probe, _, _ = rp.build_go_command(root, stage, "amd64", by_key["unic-desktop-e2e-probe"])
        self.assertIn("-ldflags=-s -w -H windowsgui", cmd_probe)
        self.assertIn("desktop_e2e,bench_probe", cmd_probe)

    def test_guest_args_and_prefix_cleanup_isolation(self) -> None:
        guest_root = r"C:\Users\mark\AppData\Local\Temp\unic-bench-20261127-abc"
        args = rp.build_guest_unic_bench_args(guest_root, "all", "both", "empty,standard", "default", 3)
        self.assertEqual(args[0], guest_root + r"\bin\unic-bench.exe")
        self.assertIn(guest_root + r"\out", args)
        self.assertIn(guest_root + r"\bin\prod\unic-desktop.exe", args)
        self.assertIn(guest_root + r"\bin\e2e-probe\unic-desktop.exe", args)
        self.assertIn("empty,standard", args)

        ps_cleanup = rp.build_guest_cleanup_ps(guest_root)
        # Must scope process termination strictly by ExecutablePath prefix and never wildcard Stop-Process -Name unic*
        self.assertIn("ExecutablePath.StartsWith($prefix", ps_cleanup)
        self.assertNotIn("Stop-Process -Name", ps_cleanup)
        self.assertIn("Remove-Item -LiteralPath $root -Recurse -Force", ps_cleanup)

    def test_host_path_to_mac_share(self) -> None:
        home = Path("/Users/mark")
        self.assertEqual(
            rp.host_path_to_mac_share(Path("/Users/mark/.cache/unic-bench/bin"), home_dir=home),
            r"\\Mac\Home\.cache\unic-bench\bin",
        )
        with self.assertRaises(rp.BenchmarkError):
            rp.host_path_to_mac_share(Path("/var/tmp/outside"), home_dir=home)


if __name__ == "__main__":
    unittest.main()
