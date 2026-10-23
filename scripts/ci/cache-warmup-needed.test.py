#!/usr/bin/env python3
"""Exercise warmup selection against real Git revisions and Cargo metadata."""
import importlib.util
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("policy", Path(__file__).with_name("cache-warmup-needed.py"))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


class WarmupPolicyTests(unittest.TestCase):
    def test_dependency_and_profile_changes_are_not_version_only(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = Path.cwd()
            os.chdir(directory)
            try:
                def git(*args):
                    return subprocess.check_output(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", *args], text=True).strip()
                git("init", "-q")
                manifest = '[workspace.package]\nversion="1.0.0"\n[profile.release]\nlto="thin"\n'
                lock = '[[package]]\nname="daemon"\nversion="1.0.0"\n[[package]]\nname="dep"\nversion="2.0.0"\nsource="registry+https://example.invalid"\n'
                Path("Cargo.toml").write_text(manifest)
                Path("Cargo.lock").write_text(lock)
                git("add", "."); git("commit", "-qm", "baseline")
                before = git("rev-parse", "HEAD")
                def commit():
                    git("add", "."); git("commit", "-qm", "change")
                    return git("rev-parse", "HEAD")
                Path("Cargo.toml").write_text(manifest.replace('1.0.0', '1.1.0'))
                Path("Cargo.lock").write_text(lock.replace('1.0.0', '1.1.0'))
                self.assertFalse(policy.warmup_needed(before, commit()))
                Path("Cargo.lock").write_text(lock.replace('2.0.0', '2.1.0'))
                self.assertTrue(policy.warmup_needed(before, commit()))
                Path("Cargo.lock").write_text(lock)
                Path("Cargo.toml").write_text(manifest.replace('"thin"', 'false'))
                self.assertTrue(policy.warmup_needed(before, commit()))
                Path("Cargo.toml").write_text(manifest)
                Path("crates/new").mkdir(parents=True)
                Path("crates/new/Cargo.toml").write_text('[package]\nname="new"\nversion="1.0.0"\n')
                self.assertTrue(policy.warmup_needed(before, commit()))
            finally:
                os.chdir(previous)

    def test_initial_push_always_warms(self):
        self.assertTrue(policy.warmup_needed("0" * 40, "unused"))


if __name__ == "__main__":
    unittest.main()
