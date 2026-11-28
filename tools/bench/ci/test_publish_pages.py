#!/usr/bin/env python3
"""Unit and git-isolation tests for tools/bench/ci/publish_pages.py."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import publish_pages  # noqa: E402


class PublishPagesTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _seed_pages_root(self, pages_dir: Path) -> dict[str, bytes]:
        pages_dir.mkdir(parents=True, exist_ok=True)
        root_files = {
            ".nojekyll": b"",
            "alpha.json": b'{"channel":"alpha","version":"2.0.0-alpha.1"}\n',
            "stable.json": b'{"channel":"stable","version":"2.0.0"}\n',
            "bench_other/keep.txt": b"untouched sibling directory\n",
        }
        for rel, content in root_files.items():
            target = pages_dir / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        return root_files

    def _write_html(self, name: str, body: str) -> Path:
        path = self.root / name
        path.write_text(f"<!doctype html><html><body>{body}</body></html>\n", encoding="utf-8")
        return path

    def test_first_publish_preserves_root_files(self) -> None:
        pages_dir = self.root / "pages"
        root_files = self._seed_pages_root(pages_dir)
        dashboard = self._write_html("dash1.html", "<h1>Run 100</h1>")
        summary_out = self.root / "summary.md"

        manifest = publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=dashboard,
            run_id="100",
            timestamp="2026-11-28T01:00:00Z",
            commit="abcdef1234567890",
            branch="main",
            event="schedule",
            run_url="https://github.com/UniClipboard/UniClipboard/actions/runs/100",
            targets="darwin-arm64,windows-amd64",
            max_runs=30,
            summary_out=summary_out,
        )

        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(manifest["latest_run_id"], "100")
        self.assertEqual(len(manifest["runs"]), 1)
        self.assertEqual(manifest["runs"][0]["targets"], ["darwin-arm64", "windows-amd64"])

        self.assertEqual((pages_dir / "bench/index.html").read_text(encoding="utf-8"), dashboard.read_text(encoding="utf-8"))
        self.assertEqual(
            (pages_dir / "bench/runs/100/index.html").read_text(encoding="utf-8"),
            dashboard.read_text(encoding="utf-8"),
        )
        history_html = (pages_dir / "bench/history.html").read_text(encoding="utf-8")
        self.assertIn("default-src 'none'", history_html)
        self.assertIn("runs/100/index.html", history_html)
        self.assertIn("darwin-arm64, windows-amd64", history_html)

        summary_text = summary_out.read_text(encoding="utf-8")
        self.assertIn("`100`", summary_text)
        self.assertIn("bench/index.html", summary_text)

        # Sibling files outside bench/ must stay byte-for-byte intact.
        for rel, expected in root_files.items():
            self.assertEqual((pages_dir / rel).read_bytes(), expected)
        top_entries = {p.name for p in pages_dir.iterdir()}
        self.assertEqual(top_entries, {".nojekyll", "alpha.json", "stable.json", "bench_other", "bench"})

    def test_upsert_sorting_and_pruning(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)

        for idx, (rid, ts, marker) in enumerate(
            [
                ("run-1", "2026-11-25T19:00:00Z", "first"),
                ("run-2", "2026-11-26T19:00:00Z", "second"),
                ("run-3", "2026-11-27T19:00:00Z", "third"),
            ],
            start=1,
        ):
            html_file = self._write_html(f"d{idx}.html", f"<p>{marker}</p>")
            publish_pages.publish_pages(
                pages_dir=pages_dir,
                dashboard_html=html_file,
                run_id=rid,
                timestamp=ts,
                commit=f"deadbeef{idx}",
                branch="main",
                event="schedule",
                max_runs=2,
            )

        manifest = json.loads((pages_dir / "bench/manifest.json").read_text(encoding="utf-8"))
        self.assertEqual([r["run_id"] for r in manifest["runs"]], ["run-3", "run-2"])
        self.assertEqual(manifest["latest_run_id"], "run-3")
        self.assertFalse((pages_dir / "bench/runs/run-1").exists())
        self.assertTrue((pages_dir / "bench/runs/run-2/index.html").is_file())
        self.assertTrue((pages_dir / "bench/runs/run-3/index.html").is_file())
        self.assertIn("<p>third</p>", (pages_dir / "bench/index.html").read_text(encoding="utf-8"))

        # Idempotent upsert of existing run-2 with new content/timestamp preserves no duplicates.
        html_updated = self._write_html("d2-updated.html", "<p>second-updated</p>")
        manifest2 = publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=html_updated,
            run_id="run-2",
            timestamp="2026-11-28T19:00:00Z",
            commit="deadbeef2b",
            branch="main",
            event="workflow_dispatch",
            max_runs=2,
        )
        self.assertEqual([r["run_id"] for r in manifest2["runs"]], ["run-2", "run-3"])
        self.assertEqual(manifest2["latest_run_id"], "run-2")
        self.assertIn("<p>second-updated</p>", (pages_dir / "bench/index.html").read_text(encoding="utf-8"))
        self.assertIn("<p>second-updated</p>", (pages_dir / "bench/runs/run-2/index.html").read_text(encoding="utf-8"))

    def test_fail_closed_validation_preserves_existing_bench(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)
        valid_html = self._write_html("valid.html", "<p>baseline</p>")

        publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=valid_html,
            run_id="10",
            timestamp="2026-11-28T00:00:00Z",
            max_runs=5,
        )
        orig_index = (pages_dir / "bench/index.html").read_bytes()
        orig_manifest = (pages_dir / "bench/manifest.json").read_bytes()

        missing_html = self.root / "missing.html"
        empty_html = self.root / "empty.html"
        empty_html.write_text("   \n\t ", encoding="utf-8")

        bad_cases = [
            {"dashboard_html": missing_html, "run_id": "11"},
            {"dashboard_html": empty_html, "run_id": "11"},
            {"dashboard_html": valid_html, "run_id": "../evil"},
            {"dashboard_html": valid_html, "run_id": ".."},
            {"dashboard_html": valid_html, "run_id": "a/b"},
            {"dashboard_html": valid_html, "run_id": "11", "run_url": "javascript:alert(1)"},
            {"dashboard_html": valid_html, "run_id": "11", "run_url": "file:///etc/passwd"},
            {"dashboard_html": valid_html, "run_id": "11", "max_runs": 0},
            {"dashboard_html": valid_html, "run_id": "11", "timestamp": "not-a-timestamp"},
        ]

        for kwargs in bad_cases:
            with self.subTest(kwargs=kwargs):
                params = {
                    "pages_dir": pages_dir,
                    "timestamp": "2026-11-28T01:00:00Z",
                    "max_runs": 5,
                }
                params.update(kwargs)
                with self.assertRaises(publish_pages.PublishError):
                    publish_pages.publish_pages(**params)
                self.assertEqual((pages_dir / "bench/index.html").read_bytes(), orig_index)
                self.assertEqual((pages_dir / "bench/manifest.json").read_bytes(), orig_manifest)

    def test_history_html_escapes_dynamic_fields(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)
        valid_html = self._write_html("valid.html", "<p>ok</p>")

        publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=valid_html,
            run_id="safe-run_1",
            timestamp="2026-11-28T02:00:00Z",
            commit='abc<script>alert(1)</script>',
            branch='feat/"><img src=x onerror=alert(1)>',
            event='schedule&"\'',
            run_url='https://github.com/UniClipboard/UniClipboard/actions/runs/1?a=1&b="x"',
            targets='darwin-arm64,<svg/onload=alert(1)>',
        )

        history_html = (pages_dir / "bench/history.html").read_text(encoding="utf-8")
        self.assertNotIn("<script>alert(1)</script>", history_html)
        self.assertNotIn("<img src=x", history_html)
        self.assertNotIn("<svg/onload", history_html)
        self.assertIn("abc&lt;script&gt;a", history_html)
        self.assertIn("&quot;&gt;&lt;img src=x onerror=alert(1)&gt;", history_html)
        self.assertIn("&lt;svg/onload=alert(1)&gt;", history_html)
        self.assertIn("script-src 'none'", history_html)

    def test_symlink_escape_fails_closed(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)
        outside = self.root / "outside"
        outside.mkdir()
        try:
            os.symlink(outside, pages_dir / "bench")
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable on this platform/filesystem")

        valid_html = self._write_html("valid.html", "<p>ok</p>")
        with self.assertRaises(publish_pages.PublishError):
            publish_pages.publish_pages(
                pages_dir=pages_dir,
                dashboard_html=valid_html,
                run_id="42",
                timestamp="2026-11-28T02:00:00Z",
            )
        self.assertEqual(list(outside.iterdir()), [])

    def test_git_bare_repo_gh_pages_isolation(self) -> None:
        bare = self.root / "remote.git"
        seed = self.root / "seed"
        work = self.root / "work"

        def git(cwd: Path, *args: str) -> str:
            res = subprocess.run(
                ["git", *args],
                cwd=cwd,
                check=True,
                text=True,
                capture_output=True,
            )
            return res.stdout

        subprocess.run(["git", "init", "--bare", str(bare)], check=True, capture_output=True)
        subprocess.run(["git", "clone", str(bare), str(seed)], check=True, capture_output=True)
        git(seed, "config", "user.name", "Bench Test")
        git(seed, "config", "user.email", "bench@example.invalid")
        git(seed, "checkout", "--orphan", "gh-pages")
        (seed / ".nojekyll").write_bytes(b"")
        (seed / "alpha.json").write_text('{"version":"alpha"}\n', encoding="utf-8")
        (seed / "stable.json").write_text('{"version":"stable"}\n', encoding="utf-8")
        git(seed, "add", ".nojekyll", "alpha.json", "stable.json")
        git(seed, "commit", "-m", "seed gh-pages")
        git(seed, "push", "origin", "HEAD:gh-pages")

        subprocess.run(["git", "clone", "--branch", "gh-pages", str(bare), str(work)], check=True, capture_output=True)
        git(work, "config", "user.name", "github-actions[bot]")
        git(work, "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")

        dash1 = self._write_html("dash1.html", "<h1>Nightly 1</h1>")
        proc = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).resolve().parent / "publish_pages.py"),
                "--pages-dir",
                str(work),
                "--dashboard-html",
                str(dash1),
                "--run-id",
                "501",
                "--timestamp",
                "2026-11-28T19:00:00Z",
                "--commit",
                "1234567890abcdef",
                "--branch",
                "main",
                "--event",
                "schedule",
                "--targets",
                "darwin-arm64,windows-amd64",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        self.assertEqual(proc.returncode, 0)

        dirty_paths = [p for p in git(work, "status", "--porcelain", "-z").split("\0") if p]
        for entry in dirty_paths:
            rel = entry[3:]
            self.assertTrue(rel.startswith("bench/"), f"unexpected dirty path: {entry!r}")

        git(work, "add", "-A", "--", "bench/")
        staged = [p for p in git(work, "diff", "--cached", "--name-only", "-z").split("\0") if p]
        self.assertGreater(len(staged), 0)
        self.assertTrue(all(p.startswith("bench/") for p in staged), staged)

        git(work, "commit", "-m", "chore(bench): publish 501")
        git(work, "push", "origin", "HEAD:gh-pages")

        changed = git(work, "diff", "--name-only", "HEAD~1", "HEAD").splitlines()
        self.assertTrue(all(p.startswith("bench/") for p in changed), changed)
        self.assertEqual((work / "alpha.json").read_text(encoding="utf-8"), '{"version":"alpha"}\n')
        self.assertEqual((work / "stable.json").read_text(encoding="utf-8"), '{"version":"stable"}\n')
        self.assertEqual((work / ".nojekyll").read_bytes(), b"")

    def test_summary_out_inside_pages_dir_fails_closed(self) -> None:
        pages_dir = self.root / "pages"
        root_files = self._seed_pages_root(pages_dir)
        valid_html = self._write_html("valid.html", "<p>ok</p>")

        with self.assertRaises(publish_pages.PublishError):
            publish_pages.publish_pages(
                pages_dir=pages_dir,
                dashboard_html=valid_html,
                run_id="10",
                timestamp="2026-11-28T19:00:00Z",
                summary_out=pages_dir / "stable.json",
            )
        self.assertEqual((pages_dir / "stable.json").read_bytes(), root_files["stable.json"])
        self.assertFalse((pages_dir / "bench").exists())

        symlink_summary = self.root / "summary-link.md"
        dir_link = self.root / "dir-link"
        try:
            os.symlink(pages_dir / "alpha.json", symlink_summary)
            os.symlink(pages_dir, dir_link)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable on this platform/filesystem")

        for bad_summary in (symlink_summary, dir_link / "nested/summary.md"):
            with self.subTest(bad_summary=bad_summary):
                with self.assertRaises(publish_pages.PublishError):
                    publish_pages.publish_pages(
                        pages_dir=pages_dir,
                        dashboard_html=valid_html,
                        run_id="10",
                        timestamp="2026-11-28T19:00:00Z",
                        summary_out=bad_summary,
                    )
                self.assertEqual((pages_dir / "alpha.json").read_bytes(), root_files["alpha.json"])
                self.assertFalse((pages_dir / "bench").exists())

    def test_invalid_existing_manifest_schema_or_missing_runs_fails_closed(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)
        baseline_html = self._write_html("base.html", "<p>baseline</p>")
        next_html = self._write_html("next.html", "<p>next</p>")

        publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=baseline_html,
            run_id="11",
            timestamp="2026-11-28T19:00:00Z",
        )
        manifest_path = pages_dir / "bench/manifest.json"
        baseline_index = (pages_dir / "bench/index.html").read_bytes()

        bad_manifests = [
            '{"schema_version": 999, "runs": []}',
            '{"schema_version": true, "runs": []}',
            '{"schema_version": 1}',
            '{"schema_version": 1, "runs": [{"run_id": "11"}]}',
            '{"schema_version": 1, "runs": [{"run_id": "11", "timestamp": "2026-11-28T19:00:00Z", "targets": [123]}]}',
        ]
        for payload in bad_manifests:
            with self.subTest(payload=payload):
                manifest_path.write_text(payload + "\n", encoding="utf-8")
                with self.assertRaises(publish_pages.PublishError):
                    publish_pages.publish_pages(
                        pages_dir=pages_dir,
                        dashboard_html=next_html,
                        run_id="12",
                        timestamp="2026-11-28T19:01:00Z",
                    )
                self.assertEqual((pages_dir / "bench/index.html").read_bytes(), baseline_index)
                self.assertFalse((pages_dir / "bench/runs/12").exists())

    def test_retained_latest_symlink_escape_fails_closed(self) -> None:
        pages_dir = self.root / "pages"
        self._seed_pages_root(pages_dir)
        newer_html = self._write_html("newer.html", "<p>newer</p>")
        older_html = self._write_html("older.html", "<p>older</p>")

        publish_pages.publish_pages(
            pages_dir=pages_dir,
            dashboard_html=newer_html,
            run_id="newer",
            timestamp="2026-11-28T19:02:00Z",
        )
        orig_index = (pages_dir / "bench/index.html").read_bytes()

        outside = self.root / "outside-secret.html"
        outside.write_text("TOP SECRET OUTSIDE CONTENT", encoding="utf-8")
        latest_run_file = pages_dir / "bench/runs/newer/index.html"
        try:
            latest_run_file.unlink()
            os.symlink(outside, latest_run_file)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable on this platform/filesystem")

        with self.assertRaises(publish_pages.PublishError):
            publish_pages.publish_pages(
                pages_dir=pages_dir,
                dashboard_html=older_html,
                run_id="older",
                timestamp="2026-11-28T19:01:00Z",
            )
        self.assertEqual((pages_dir / "bench/index.html").read_bytes(), orig_index)
        self.assertFalse((pages_dir / "bench/runs/older").exists())

    def test_git_bare_repo_concurrent_push_recovery(self) -> None:
        bare = self.root / "remote.git"
        seed = self.root / "seed"
        left = self.root / "left"
        right = self.root / "right"

        def git(cwd: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                ["git", *args],
                cwd=cwd,
                check=check,
                text=True,
                capture_output=True,
            )

        subprocess.run(["git", "init", "--bare", "--initial-branch=gh-pages", str(bare)], check=True, capture_output=True)
        subprocess.run(["git", "clone", str(bare), str(seed)], check=True, capture_output=True)
        git(seed, "config", "user.name", "Bench Test")
        git(seed, "config", "user.email", "bench@example.invalid")
        root_files = self._seed_pages_root(seed)
        dash_seed = self._write_html("dash-seed.html", "<h1>Run 100</h1>")
        publish_pages.publish_pages(
            pages_dir=seed,
            dashboard_html=dash_seed,
            run_id="100",
            timestamp="2026-11-28T19:00:00Z",
        )
        git(seed, "add", "--all")
        git(seed, "commit", "-m", "seed gh-pages")
        git(seed, "push", "origin", "HEAD:gh-pages")

        for repo in (left, right):
            subprocess.run(["git", "clone", "--branch", "gh-pages", str(bare), str(repo)], check=True, capture_output=True)
            git(repo, "config", "user.name", "github-actions[bot]")
            git(repo, "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")

        dash_right = self._write_html("dash-right.html", "<h1>Run 201</h1>")
        dash_left = self._write_html("dash-left.html", "<h1>Run 200</h1>")
        summary_out = self.root / "publish-summary.md"

        # Winner pushes run 201 first.
        publish_pages.publish_pages(
            pages_dir=right,
            dashboard_html=dash_right,
            run_id="201",
            timestamp="2026-11-28T19:02:00Z",
            targets="darwin-arm64",
        )
        git(right, "add", "-A", "--", "bench/")
        git(right, "commit", "-m", "chore(bench): publish 201")
        git(right, "push", "origin", "HEAD:gh-pages")

        # Concurrent run 200 hits non-fast-forward rejection on attempt 1, resets to origin/gh-pages, and succeeds cleanly on attempt 2.
        pushed = False
        for attempt in (1, 2, 3):
            publish_pages.publish_pages(
                pages_dir=left,
                dashboard_html=dash_left,
                run_id="200",
                timestamp="2026-11-28T19:01:00Z",
                targets="windows-amd64",
                summary_out=summary_out,
            )
            git(left, "add", "-A", "--", "bench/")
            git(left, "commit", "-m", "chore(bench): publish 200")
            res = git(left, "push", "origin", "HEAD:gh-pages", check=False)
            if res.returncode == 0:
                pushed = True
                break
            git(left, "fetch", "origin", "gh-pages")
            git(left, "reset", "--hard", "origin/gh-pages")
            git(left, "clean", "-fd", "--", "bench")

        self.assertTrue(pushed)
        remote_manifest = json.loads(git(bare, "show", "refs/heads/gh-pages:bench/manifest.json").stdout)
        self.assertEqual([r["run_id"] for r in remote_manifest["runs"]], ["201", "200", "100"])
        self.assertEqual(remote_manifest["latest_run_id"], "201")
        remote_history = git(bare, "show", "refs/heads/gh-pages:bench/history.html").stdout
        self.assertIn("runs/201/index.html", remote_history)
        self.assertIn("runs/200/index.html", remote_history)
        remote_index = git(bare, "show", "refs/heads/gh-pages:bench/index.html").stdout
        self.assertIn("Run 201", remote_index)
        for rel, expected in root_files.items():
            self.assertEqual((left / rel).read_bytes(), expected)


if __name__ == "__main__":
    unittest.main()
