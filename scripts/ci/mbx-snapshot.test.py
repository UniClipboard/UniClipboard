#!/usr/bin/env python3
"""Exercise snapshot boundaries and optionally the real pinned MBX importer."""

import importlib.util
import io
import json
import os
import shutil
import subprocess
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
    def test_scene_keys_isolate_workloads_and_host_architectures(self):
        with patch.object(snapshot.subprocess, "check_output", return_value=b"compiler"), patch.object(snapshot.platform, "system", return_value="Windows"):
            with patch.object(snapshot.platform, "machine", return_value="AMD64"):
                x64 = snapshot.scene_key("quick-panel-windows")
                self.assertNotEqual(x64, snapshot.scene_key("e2e-macos"))
            with patch.object(snapshot.platform, "machine", return_value="ARM64"):
                self.assertNotEqual(x64, snapshot.scene_key("quick-panel-windows"))

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

    def test_retention_only_deletes_expired_pilot_objects(self):
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
            snapshot.mbx("cache", "import", str(wrapper_bundle), cache=root / "wrapper-consumer")
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
            # Corrupt a required object and confirm native import rejects it.
            exported = root / "corrupt-export"
            run("cache", "export", "--group", "e2e-macos", "--format", "directory", str(exported), cache=root / "consumer")
            leaf = next(file for file in (exported / "cas").rglob("*") if file.is_file())
            leaf.write_bytes(b"corrupt")
            with self.assertRaises(subprocess.CalledProcessError):
                run("cache", "import", str(exported), cache=root / "rejected")


if __name__ == "__main__":
    unittest.main()
