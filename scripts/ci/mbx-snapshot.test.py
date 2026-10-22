#!/usr/bin/env python3
"""Exercise snapshot boundaries and optionally the real pinned MBX importer."""

import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


snapshot = load("mbx-snapshot")
summary = load("summarize-mbx")


class SnapshotTests(unittest.TestCase):
    def test_bootstrap_trusts_only_the_source_checkout_and_preserves_git_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source checkout"
            source.mkdir()
            clean = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
            subprocess.run(["git", "init", "-q", str(source)], env=clean, check=True, capture_output=True)
            subprocess.run(["git", "-c", "user.name=Snapshot Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "fixture"], cwd=source, env=clean, check=True, capture_output=True)
            expected = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, env=clean, text=True).strip()
            forced = {**clean, "GIT_TEST_ASSUME_DIFFERENT_OWNER": "1"}
            rejected = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source, env=forced, capture_output=True)
            self.assertNotEqual(rejected.returncode, 0)
            tools = root / "tools"
            tools.mkdir()
            shutil.copyfile(Path(__file__).with_name("mbx-snapshot.sh"), tools / "mbx-snapshot.sh")
            (tools / "mbx-snapshot.py").write_text('import json, os, subprocess\nfrom pathlib import Path\nroot = os.environ["UC_MBX_SNAPSHOT_SOURCE_ROOT"]\nprint(json.dumps(dict(sha=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(), abbrev=subprocess.check_output(["git", "config", "core.abbrev"], cwd=root, text=True).strip(), trust=subprocess.check_output(["git", "config", "--get-all", "safe.directory"], cwd=root, text=True).strip())))\n')
            # Avoid SDK installation; the probe exercises the real shell and Git boundary.
            (tools / "boto3.py").write_text("")
            config = root / "engine-env"
            config.write_text("")
            env = {**forced, "UC_MBX_SNAPSHOT_SOURCE_ROOT": str(source), "RUNNER_TEMP": str(root),
                   "RUNNER_OS": "Windows" if os.name == "nt" else "Linux" if sys.platform == "linux" else "macOS",
                   "UC_ENGINE_MBX_ENV_FILE": str(config), "UC_ENGINE_MBX_STATS_DIR": str(root / "stats"),
                   "MBX_CACHE_DIR": str(root / "cache"), "PYTHONPATH": str(tools),
                   "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.abbrev", "GIT_CONFIG_VALUE_0": "9"}
            # Windows also exposes a WSL bash.exe; use the Git Bash installation.
            bash = str(Path(shutil.which("cygpath")).with_name("bash.exe")) if os.name == "nt" else "bash"
            probe = subprocess.run([bash, (tools / "mbx-snapshot.sh").as_posix()], env=env, capture_output=True, text=True)
            self.assertEqual(probe.returncode, 0, probe.stdout + probe.stderr)
            result = json.loads(probe.stdout)
            self.assertEqual(result["sha"], expected)
            self.assertEqual(result["abbrev"], "9")
            self.assertEqual(Path(result["trust"]).resolve(), source.resolve())

    def test_scene_keys_isolate_workloads_and_host_architectures(self):
        with patch.object(snapshot.subprocess, "check_output", return_value=b"compiler"), patch.object(snapshot.platform, "system", return_value="Windows"):
            with patch.object(snapshot.platform, "machine", return_value="AMD64"):
                x64 = snapshot.scene_key("quick-panel-windows")
                self.assertNotEqual(x64, snapshot.scene_key("e2e-macos"))
            with patch.object(snapshot.platform, "machine", return_value="ARM64"):
                self.assertNotEqual(x64, snapshot.scene_key("quick-panel-windows"))

    def test_release_seed_survives_local_version_and_channel_changes(self):
        manifest = b'[workspace.package]\nversion = "1.0.0"\n[workspace.dependencies]\nengine = { git = "https://example.invalid/engine", rev = "abc" }\n'
        lock = b'[[package]]\nname = "daemon"\nversion = "1.0.0"\ndependencies = ["helper 1.0.0", "external"]\n[[package]]\nname = "helper"\nversion = "1.0.0"\n[[package]]\nname = "external"\nversion = "2.0.0"\nsource = "registry+https://example.invalid/index"\nchecksum = "abc"\n'
        for name, raw in [("Cargo.toml", manifest), ("Cargo.lock", lock)]:
            self.assertEqual(snapshot.release_seed_input(name, raw), snapshot.release_seed_input(name, raw.replace(b'1.0.0', b'1.1.0-alpha.1')))
        self.assertNotEqual(snapshot.release_seed_input("Cargo.lock", lock), snapshot.release_seed_input("Cargo.lock", lock.replace(b'2.0.0', b'2.0.1')))
        self.assertNotEqual(snapshot.release_seed_input("Cargo.toml", manifest), snapshot.release_seed_input("Cargo.toml", manifest.replace(b'rev = "abc"', b'rev = "def"')))
        context = snapshot.release_context("x86_64-pc-windows-msvc", "daemon", "release", "")
        with patch.object(snapshot.subprocess, "check_output", return_value=b"compiler"), patch.dict(os.environ, CARGO_PROFILE_RELEASE_OPT_LEVEL="3"):
            with patch.dict(os.environ, APP_ENV="production"):
                baseline = snapshot.scene_key("release-sidecar", context)
            with patch.dict(os.environ, APP_ENV="alpha"):
                self.assertEqual(baseline, snapshot.scene_key("release-sidecar", context))
            for changed in [dict(context, component="helper"), dict(context, build_mode="test"), dict(context, target_mode="explicit"), dict(context, target="aarch64-pc-windows-msvc")]:
                self.assertNotEqual(baseline, snapshot.scene_key("release-sidecar", changed))
            with patch.dict(os.environ, CARGO_PROFILE_RELEASE_OPT_LEVEL="1"):
                self.assertNotEqual(baseline, snapshot.scene_key("release-sidecar", context))

    def test_release_seed_uses_pinned_transport_with_a_historical_source_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("Cargo.toml", "Cargo.lock", ".cargo/config.toml", "rust-toolchain.toml", "scripts/build-cache/engine-tools.sh", "scripts/stage-daemon.mjs", "scripts/ci/configure-build-mode.mjs", "scripts/ci/resolve-app-env.mjs"):
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(snapshot.ROOT / name, target)
            context = snapshot.release_context("x86_64-pc-windows-msvc", "daemon", "release", "")
            with patch.object(snapshot, "ROOT", root), patch.object(snapshot.subprocess, "check_output", return_value=b"compiler"):
                baseline = snapshot.scene_key("release-sidecar", context)
                (root / "scripts/ci/mbx-snapshot.py").write_text("historical transport is not executed")
                self.assertEqual(baseline, snapshot.scene_key("release-sidecar", context))

    def test_release_context_rejects_unsupported_or_inconsistent_targets(self):
        for context in [("../escape", "daemon", "release", ""),
                        ("x86_64-unknown-linux-gnu", "helper", "release", ""),
                        ("x86_64-pc-windows-msvc", "daemon", "debug", ""),
                        ("x86_64-pc-windows-msvc", "daemon", "release", "--target aarch64-pc-windows-msvc")]:
            with self.subTest(context=context), self.assertRaises(ValueError):
                snapshot.release_context(*context)

    def test_release_publisher_refuses_a_different_source_before_accessing_r2(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = dict(UC_ENGINE_MBX_STATS_DIR=str(root), GITHUB_ENV=str(root / "env"), GITHUB_SHA="a" * 40)
            argv = ["mbx-snapshot.py", "save", "--scene", "release-sidecar", "--target", "x86_64-pc-windows-msvc", "--component", "daemon"]
            with patch.dict(os.environ, env), patch.object(sys, "argv", argv), patch.object(snapshot.subprocess, "check_output", return_value="b" * 40), patch.object(snapshot, "client") as client:
                snapshot.main()
                client.assert_not_called()
            report = json.loads((root / "snapshot-save.json").read_text())
            self.assertEqual(report["status"], "error")
            self.assertEqual(report["error_type"], "ValueError")

    def test_release_cold_miss_configures_export_group_and_preserves_remote_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = dict(UC_ENGINE_MBX_STATS_DIR=str(root), GITHUB_ENV=str(root / "env"), RUNNER_TEMP=str(root))
            argv = ["mbx-snapshot.py", "restore", "--scene", "release-sidecar", "--target", "x86_64-pc-windows-msvc", "--component", "daemon", "--object-fallback", "false"]
            with patch.dict(os.environ, env), patch.object(sys, "argv", argv), patch.object(snapshot.subprocess, "check_output", return_value="a" * 40), patch.object(snapshot, "client"), patch.object(snapshot, "scene_key", return_value="scene"), patch.object(snapshot, "get_index", return_value=None):
                snapshot.main()
            self.assertIn("MBX_CACHE_EXPORT_GROUP=sidecar-x86_64-pc-windows-msvc-daemon-release-native", (root / "env").read_text())
            self.assertEqual(json.loads((root / "snapshot-restore.json").read_text())["status"], "miss")

    def test_index_cannot_escape_scene_or_exceed_limit(self):
        sha = "a" * 64
        valid = dict(version=1, sha256=sha, size=20, key=f"scene/{sha}.tar.gz")
        self.assertEqual(snapshot.validate_index(valid, "scene"), valid)
        for change in [dict(key="other/key"), dict(sha256="../escape"), dict(size=0),
                       dict(size=snapshot.MAX_ARCHIVE_BYTES + 1), dict(version=2)]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                snapshot.validate_index({**valid, **change}, "scene")

    def test_archive_rejects_links_unsafe_paths_and_unexpected_files(self):
        for name, kind in [("../escape", tarfile.REGTYPE), ("/escape", tarfile.REGTYPE),
                           ("cas/link", tarfile.SYMTYPE), ("cas/hardlink", tarfile.LNKTYPE),
                           ("target/debug/bin", tarfile.REGTYPE), ("cas\\escape", tarfile.REGTYPE),
                           ("cas/C:/escape", tarfile.REGTYPE), ("cas/blob:stream", tarfile.REGTYPE),
                           ("cas/alias.", tarfile.REGTYPE), ("cas/alias ", tarfile.REGTYPE), ("cas/CON.bin", tarfile.REGTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                archive = root / "bundle.tar.gz"
                with tarfile.open(archive, "w:gz") as output:
                    entry = tarfile.TarInfo(name)
                    entry.type = kind
                    output.addfile(entry)
                with self.assertRaises(ValueError):
                    snapshot.unpack(archive, root / "output")

    def test_archive_rejects_workspace_state(self):
        for data in [dict(attachments={"cargo-workspace-state-v1": {}}), dict(objects=[{}])]:
            with self.subTest(data=data), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                payload = json.dumps(data).encode()
                with tarfile.open(root / "bundle.tar.gz", "w:gz") as output:
                    entry = tarfile.TarInfo(snapshot.MANIFEST)
                    entry.size = len(payload)
                    output.addfile(entry, io.BytesIO(payload))
                with self.assertRaises(ValueError):
                    snapshot.unpack(root / "bundle.tar.gz", root / "output")

    def test_archive_rejects_duplicate_members_and_expansion_limit(self):
        for duplicate in (True, False):
            with self.subTest(duplicate=duplicate), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                with tarfile.open(root / "bundle.tar.gz", "w:gz") as output:
                    for index in range(2):
                        entry = tarfile.TarInfo("cas/blob" if duplicate else f"cas/{index}")
                        entry.size = 3
                        output.addfile(entry, io.BytesIO(b"abc"))
                with patch.object(snapshot, "MAX_EXPANDED_BYTES", 100 if duplicate else 2):
                    with self.assertRaises(ValueError):
                        snapshot.unpack(root / "bundle.tar.gz", root / "output")

    def test_miss_preserves_existing_cache_policy(self):
        report = dict(index_reads=0, archive_reads=0)
        with patch.object(snapshot, "get_index", return_value=None), patch.object(snapshot, "mbx") as mbx:
            snapshot.restore(None, "scene", Path("unused"), report, False)
        self.assertEqual(report, dict(index_reads=1, archive_reads=0, status="miss"))
        mbx.assert_not_called()

    def test_checksum_failure_never_imports(self):
        class FakeS3:
            def get_object(self, **kwargs):
                return dict(ContentLength=3, Body=io.BytesIO(b"bad"))

        report = dict(index_reads=0, archive_reads=0, downloaded_bytes=0)
        with tempfile.TemporaryDirectory() as directory, patch.object(snapshot, "get_index", return_value=dict(size=3, sha256="0" * 64, key="blob")), patch.object(snapshot, "mbx") as mbx:
            with self.assertRaises(ValueError):
                snapshot.restore(FakeS3(), "scene", Path(directory), report, False)
            mbx.assert_not_called()

    def test_reader_cannot_publish(self):
        with patch.dict(os.environ, dict(GITHUB_REF="refs/pull/1/merge", GITHUB_REF_PROTECTED="true", GITHUB_EVENT_NAME="pull_request")), patch.object(snapshot, "mbx") as mbx:
            with self.assertRaises(ValueError):
                snapshot.save(None, "scene", Path("unused"), {}, "e2e-macos")
            mbx.assert_not_called()

    def test_retention_only_deletes_expired_scene_objects(self):
        now = datetime.now(timezone.utc)

        class FakeS3:
            def get_paginator(self, operation):
                self.operation = operation
                return self

            def paginate(self, **kwargs):
                self.scope = kwargs
                return [{"Contents": [dict(Key="expired", LastModified=now - timedelta(days=15)),
                                      dict(Key="fresh", LastModified=now)]}]

            def delete_objects(self, **kwargs):
                self.deleted = kwargs["Delete"]["Objects"]
                return {}

        s3 = FakeS3()
        report = {}
        snapshot.prune_snapshots(s3, "e2e-macos", report)
        self.assertEqual(s3.scope["Prefix"], "desktop/mbx-snapshots/v1/e2e-macos/")
        self.assertEqual(s3.deleted, [{"Key": "expired"}])
        self.assertEqual(report["expired_objects"], 1)

    def test_summary_keeps_transport_bytes_separate_and_ignores_its_own_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "stats.json").write_text(json.dumps(dict(session_duration_ns=1, remote_action_lookups=2, remote_blob_requests=4, downloaded_bytes=100)))
            (root / "snapshot-restore.json").write_text(json.dumps(dict(downloaded_bytes=500, index_reads=1, archive_reads=1)))
            (root / "summary.json").write_text(json.dumps(dict(session_duration_ns=1, remote_blob_requests=900)))
            result = summary.summarize(root)
            self.assertEqual(result["logical_reads"], 6)
            self.assertEqual(result["sessions"], 1)
            self.assertEqual(result["downloaded_bytes"], 100)
            self.assertEqual(result["total_downloaded_bytes"], 600)
            self.assertEqual(result["total_logical_reads"], 8)


@unittest.skipUnless(os.environ.get("UC_MBX_TEST_BINARY"), "Set UC_MBX_TEST_BINARY for native MBX integration")
class NativeMbxTests(unittest.TestCase):
    def test_compaction_restore_execution_and_corruption(self):
        binary = os.environ["UC_MBX_TEST_BINARY"]
        if os.name == "nt":
            binary = subprocess.check_output(["cygpath", "-w", binary], text=True).strip()
        executable = "snapshot-proof.exe" if os.name == "nt" else "snapshot-proof"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "workspace"
            workspace.mkdir()
            (workspace / "src").mkdir()
            (workspace / "Cargo.toml").write_text('[package]\nname = "snapshot-proof"\nversion = "0.1.0"\nedition = "2021"\n[features]\nvariant = []\n')
            (workspace / "src/main.rs").write_text('fn main() { println!("{}", if cfg!(feature="variant") { "variant" } else { "snapshot-proof" }); }\n')
            environment = os.environ.copy()
            for name in list(environment):
                if name.startswith("MBX_REMOTE_") or name in ("RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"):
                    environment.pop(name)
            environment.update(MBX_CACHE_DIR=str(root / "producer"), MBX_CACHE_EXPORT_GROUP="e2e-macos", CARGO_INCREMENTAL="0", MBX_TARGET_VIEWS="0", MBX_TARGET_SEED="0", MBX_LEARNED_INCREMENTAL="0", MBX_DISPLAY="plain", MBX_STATS_REPORT=str(root / "native-stats.json"))

            def run(*args, cache=None):
                env = {**environment, **({"MBX_CACHE_DIR": str(cache)} if cache else {})}
                return subprocess.run([binary, *args], cwd=workspace, env=env, check=True, capture_output=True, text=True)

            run("build", "--offline")
            exported = root / "exported"
            run("cache", "export", "--group", "e2e-macos", "--format", "directory", str(exported))
            self.assertTrue(json.loads((exported / snapshot.MANIFEST).read_text()).get("attachments"))
            bundle = root / "bundle"
            with patch.object(snapshot, "mbx", side_effect=run):
                snapshot.compact_bundle(exported, bundle)
            manifest = json.loads((bundle / snapshot.MANIFEST).read_text())
            self.assertNotIn("attachments", manifest)
            self.assertNotIn("objects", manifest)
            self.assertEqual({file.name for file in bundle.iterdir()}, {snapshot.MANIFEST, "cas", "action-results"})
            # The native importer validates the action closure. Importing to an
            # empty target must not restore scheduler state or compiler outputs.
            wrapper_bundle = root / "wrapper-bundle"
            shutil.copytree(bundle, wrapper_bundle)
            tooling = root / "pipeline-tools"
            tooling.mkdir()
            shutil.copyfile(Path(snapshot.__file__), tooling / "mbx-snapshot.py")
            spec = importlib.util.spec_from_file_location("relocated_snapshot", tooling / "mbx-snapshot.py")
            relocated = importlib.util.module_from_spec(spec)
            with patch.dict(os.environ, UC_MBX_SNAPSHOT_SOURCE_ROOT=str(snapshot.ROOT)):
                spec.loader.exec_module(relocated)
            self.assertEqual(relocated.ROOT, snapshot.ROOT)
            relocated.mbx("cache", "import", str(wrapper_bundle), cache=root / "wrapper-consumer")
            self.assertTrue((root / "wrapper-consumer/actions/cas").is_dir())
            original_target = workspace / "target"
            original_target.rename(workspace / "producer-target")
            run("cache", "import", str(bundle), cache=root / "consumer")
            self.assertFalse(original_target.exists())
            run("build", "--offline", cache=root / "consumer")
            self.assertGreater(json.loads((root / "native-stats.json").read_text())["hits"], 0)
            output = subprocess.check_output([str(original_target / "debug" / executable)], text=True)
            self.assertEqual(output.strip(), "snapshot-proof")
            # Exercise the complete compressed transport with the native
            # exporter/importer, including the snapshot-only policy transition.
            class MemoryS3:
                def __init__(self):
                    self.objects = {}
                    self.publications = []

                def upload_file(self, file, bucket, key):
                    self.objects[key] = Path(file).read_bytes()
                    self.publications.append(key)

                def put_object(self, *, Bucket, Key, Body, **kwargs):
                    self.objects[Key] = Body
                    self.publications.append(Key)

                def get_object(self, *, Bucket, Key):
                    data = self.objects[Key]
                    return dict(Body=io.BytesIO(data), ContentLength=len(data))

            s3 = MemoryS3()
            transport = root / "transport"
            transport.mkdir()
            # Group export consumes pending receipts. Produce a fresh build
            # before testing another export from the same producer store.
            run("build", "--offline", "--target-dir", str(workspace / "transport-target"))
            config = root / "engine-env"
            config.write_text("# Test policy\n")
            with patch.dict(os.environ, dict(GITHUB_REF="refs/heads/main", GITHUB_REF_PROTECTED="true", GITHUB_EVENT_NAME="push", GITHUB_SHA="a" * 40, UC_ENGINE_MBX_ENV_FILE=str(config))), patch.object(snapshot, "mbx", side_effect=run):
                saved = {}
                snapshot.save(s3, "scene", transport, saved, "e2e-macos")
                self.assertEqual(saved["status"], "saved")
                self.assertEqual(s3.publications[-1], "scene/latest.json")
                environment["MBX_CACHE_DIR"] = str(root / "transport-consumer")
                restored = dict(index_reads=0, archive_reads=0, downloaded_bytes=0)
                restored_dir = root / "restored"
                restored_dir.mkdir()
                snapshot.restore(s3, "scene", restored_dir, restored, False)
                self.assertEqual(restored["status"], "hit")
                self.assertEqual(restored["index_reads"], 1)
                self.assertEqual(restored["archive_reads"], 1)
                self.assertIn("unset MBX_REMOTE_URL", config.read_text())
                if os.name != "nt":
                    self.assertEqual(config.stat().st_mode & 0o777, 0o600)
                self.assertTrue((root / "transport-consumer/actions/cas").is_dir())
                environment["MBX_CACHE_DIR"] = str(root / "producer")
            run("build", "--offline", "--features", "variant", cache=root / "consumer")
            self.assertEqual(subprocess.check_output([str(original_target / "debug" / executable)], text=True).strip(), "variant")
            source = workspace / "src/main.rs"
            source.write_text(source.read_text().replace('"snapshot-proof"', '"changed-source"'))
            run("build", "--offline", cache=root / "consumer")
            self.assertEqual(subprocess.check_output([str(original_target / "debug" / executable)], text=True).strip(), "changed-source")
            # A dependency seed may cross versions and channels, but executable
            # contents must always reflect Cargo's current full inputs.
            source.write_text('fn main() { println!("{}|{}", env!("CARGO_PKG_VERSION"), env!("APP_ENV")); }\n')
            environment.update(MBX_CACHE_EXPORT_GROUP="release-proof", APP_ENV="production")
            run("build", "--offline", "--locked", "--release")
            release_transport = root / "release-transport"
            release_transport.mkdir()
            release_export = release_transport / "exported"
            run("cache", "export", "--group", "release-proof", "--format", "directory", str(release_export))
            release_bundle = release_transport / "bundle"
            with patch.object(snapshot, "mbx", side_effect=run):
                snapshot.compact_bundle(release_export, release_bundle)
            original_target.rename(workspace / "release-producer-target")
            run("cache", "import", str(release_bundle), cache=root / "release-consumer")
            run("build", "--offline", "--locked", "--release", cache=root / "release-consumer")
            self.assertGreater(json.loads((root / "native-stats.json").read_text())["hits"], 0)
            release_executable = original_target / "release" / executable
            self.assertEqual(subprocess.check_output([str(release_executable)], text=True).strip(), "0.1.0|production")
            seeds = {name: snapshot.release_seed_input(name, (workspace / name).read_bytes()) for name in ("Cargo.toml", "Cargo.lock")}
            for name in seeds:
                path = workspace / name
                path.write_text(path.read_text().replace('0.1.0', '0.2.0'))
                self.assertEqual(seeds[name], snapshot.release_seed_input(name, path.read_bytes()))
            shutil.rmtree(original_target)
            run("build", "--offline", "--locked", "--release", cache=root / "release-consumer")
            self.assertEqual(subprocess.check_output([str(release_executable)], text=True).strip(), "0.2.0|production")
            environment["APP_ENV"] = "alpha"
            shutil.rmtree(original_target)
            run("build", "--offline", "--locked", "--release", cache=root / "release-consumer")
            self.assertEqual(subprocess.check_output([str(release_executable)], text=True).strip(), "0.2.0|alpha")
            environment["MBX_CACHE_EXPORT_GROUP"] = "e2e-macos"
            run("build", "--offline", cache=root / "consumer")
            # Corrupt a required object and confirm native import rejects it.
            exported = root / "corrupt-export"
            run("cache", "export", "--group", "e2e-macos", "--format", "directory", str(exported), cache=root / "consumer")
            leaf = next(file for file in (exported / "cas").rglob("*") if file.is_file())
            # Exports can hard-link read-only compiler objects. Replace the
            # fixture's link instead of modifying the producer's shared inode.
            leaf.chmod(0o600)
            leaf.unlink()
            leaf.write_bytes(b"corrupt")
            with self.assertRaises(subprocess.CalledProcessError):
                run("cache", "import", str(exported), cache=root / "rejected")


if __name__ == "__main__":
    unittest.main()
