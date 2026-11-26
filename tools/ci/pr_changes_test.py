#!/usr/bin/env python3
"""Tests for PR check path triggers, job gating, and release metadata validation."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS_DIR = ROOT / ".github/workflows"
sys.path.insert(0, str(ROOT / "tools/ci"))

import pr_changes  # noqa: E402
import validate_release  # noqa: E402


def load_yaml(path: Path) -> dict:
    try:
        import yaml  # type: ignore
    except ImportError:
        out = subprocess.check_output(
            [
                "ruby",
                "-ryaml",
                "-rjson",
                "-e",
                "puts JSON.generate(YAML.load_file(ARGV[0], aliases: true))",
                str(path),
            ],
            text=True,
        )
        return json.loads(out)
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """Translate a GitHub Actions path filter glob to a regex."""
    i = 0
    n = len(pattern)
    out = ["^"]
    while i < n:
        ch = pattern[i]
        if ch == "*":
            if i + 1 < n and pattern[i + 1] == "*":
                if i + 2 < n and pattern[i + 2] == "/":
                    out.append("(?:.*/)?")
                    i += 3
                    continue
                out.append(".*")
                i += 2
                continue
            out.append("[^/]*")
            i += 1
        elif ch == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(ch))
            i += 1
    out.append("$")
    return re.compile("".join(out))


def matches_github_paths(patterns: list[str] | None, files: list[str]) -> bool:
    """Evaluate GitHub Actions `paths:` matching semantics across changed files."""
    if not patterns:
        return True
    for file_path in files:
        matched = False
        for raw in patterns:
            neg = raw.startswith("!")
            pat = raw[1:] if neg else raw
            if glob_to_regex(pat).match(file_path):
                matched = not neg
        if matched:
            return True
    return False


def pr_trigger_workflows(files: list[str]) -> set[str]:
    """Return workflow filenames that would trigger on an opened/synchronize PR."""
    triggered = set()
    for wf_path in sorted(WORKFLOWS_DIR.glob("*.yml")):
        data = load_yaml(wf_path)
        on_block = data.get("on") if "on" in data else data.get(True)
        if not isinstance(on_block, dict) or "pull_request" not in on_block:
            continue
        pr_cfg = on_block["pull_request"] or {}
        types = pr_cfg.get("types")
        if types and "synchronize" not in types and "opened" not in types:
            continue
        paths = pr_cfg.get("paths")
        if matches_github_paths(paths, files):
            triggered.add(wf_path.name)
    return triggered


class PrChangesClassifierTests(unittest.TestCase):
    def test_pure_docs_and_release_notes_trigger_no_go_jobs(self):
        self.assertEqual(
            pr_changes.classify_paths(
                [
                    "README.md",
                    "AGENTS.md",
                    "app/desktop/README.md",
                    "docs/architecture/releases.md",
                    "release/changelogs/desktop/v2.0.0-build.13.md",
                    "release/desktop-notes.json",
                    "release/products.toml",
                ]
            ),
            {"lint": False, "test": False, "headless": False, "desktop_e2e": False},
        )

    def test_pure_desktop_ui_skips_headless(self):
        self.assertEqual(
            pr_changes.classify_paths(
                [
                    "app/desktop/internal/desktopui/live.go",
                    "app/desktop/internal/i18n/locales.json",
                ]
            ),
            {"lint": True, "test": True, "headless": False, "desktop_e2e": True},
        )

    def test_desktop_e2e_harness_only_runs_desktop_e2e(self):
        self.assertEqual(
            pr_changes.classify_paths(
                [
                    "app/desktop/e2e/cases/quick-panel.txtar",
                    "app/desktop/e2e/CASES.md",
                    "app/desktop/e2e/run.py",
                ]
            ),
            {"lint": False, "test": False, "headless": False, "desktop_e2e": True},
        )

    def test_headless_e2e_only_skips_desktop_e2e(self):
        self.assertEqual(
            pr_changes.classify_paths(["e2e/headless/migration_tags_test.go"]),
            {"lint": True, "test": True, "headless": True, "desktop_e2e": False},
        )

    def test_cli_e2e_only_runs_lint_and_test(self):
        self.assertEqual(
            pr_changes.classify_paths(
                ["e2e/cli/testdata/history-auto-cleanup.txtar", "e2e/cli/CASES.md"]
            ),
            {"lint": True, "test": True, "headless": False, "desktop_e2e": False},
        )

    def test_lint_config_only_runs_lint(self):
        self.assertEqual(
            pr_changes.classify_paths([".golangci.yml"]),
            {"lint": True, "test": False, "headless": False, "desktop_e2e": False},
        )
        self.assertEqual(
            pr_changes.classify_paths(["./.golangci-lint-version"]),
            {"lint": True, "test": False, "headless": False, "desktop_e2e": False},
        )

    def test_workflow_file_runs_all_jobs(self):
        self.assertEqual(
            pr_changes.classify_paths([".github/workflows/pr-checks.yml"]),
            {"lint": True, "test": True, "headless": True, "desktop_e2e": True},
        )

    def test_service_and_core_changes_run_all_jobs(self):
        self.assertEqual(
            pr_changes.classify_paths(["app/service/cmd/unic/cli.go"]),
            {"lint": True, "test": True, "headless": True, "desktop_e2e": True},
        )
        self.assertEqual(
            pr_changes.classify_paths(["engine/options.go"]),
            {"lint": True, "test": True, "headless": True, "desktop_e2e": True},
        )

    def test_mobilemigration_internal_skips_headless_and_desktop_e2e(self):
        self.assertEqual(
            pr_changes.classify_paths(["internal/mobilemigration/migrate.go"]),
            {"lint": True, "test": True, "headless": False, "desktop_e2e": False},
        )

    def test_go_mod_runs_all_jobs(self):
        self.assertEqual(
            pr_changes.classify_paths(["go.mod"]),
            {"lint": True, "test": True, "headless": True, "desktop_e2e": True},
        )


class WorkflowTriggerMatrixTests(unittest.TestCase):
    def test_all_workflows_are_valid_yaml_and_pr_workflows_have_paths(self):
        for wf_path in sorted(WORKFLOWS_DIR.glob("*.yml")):
            data = load_yaml(wf_path)
            self.assertIsInstance(data, dict, wf_path.name)
            on_block = data.get("on") if "on" in data else data.get(True)
            if isinstance(on_block, dict) and "pull_request" in on_block:
                pr_cfg = on_block["pull_request"] or {}
                types = pr_cfg.get("types")
                if types == ["closed"]:
                    continue
                self.assertTrue(
                    pr_cfg.get("paths"),
                    f"{wf_path.name} triggers on pull_request without explicit paths",
                )

    def test_pr_2419_desktop_release_prep_only_runs_release_checks(self):
        files = [
            "release/changelogs/desktop/v2.0.0-build.13.md",
            "release/changelogs/desktop/v2.0.0-build.13.zh.md",
            "release/desktop-notes.json",
            "release/products.toml",
        ]
        self.assertEqual(pr_trigger_workflows(files), {"release-checks.yml"})

    def test_pr_2418_prepare_release_and_docs_runs_only_release_and_repo_docs(self):
        files = [
            ".github/workflows/prepare-release.yml",
            "docs/architecture/releases.md",
        ]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"release-checks.yml", "repo-docs-check.yml"},
        )

    def test_pr_2417_release_ios_workflow_runs_only_release_checks(self):
        self.assertEqual(
            pr_trigger_workflows([".github/workflows/release-ios.yml"]),
            {"release-checks.yml"},
        )

    def test_pr_2420_expo_config_and_release_ios_runs_mobile_and_release_checks(self):
        files = [".github/workflows/release-ios.yml", "app/expo/app.config.js"]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"check-expo.yml", "release-checks.yml"},
        )

    def test_pr_2416_mobile_release_tooling_runs_mobile_release_and_docs(self):
        files = [
            ".github/release-notes/android.md.tmpl",
            ".github/workflows/release-android.yml",
            ".github/workflows/release-ios.yml",
            "app/expo/plugins/withUnsignedAndroidRelease.js",
            "bindings/scripts/build-mobile.sh",
            "docs/architecture/releases.md",
            "e2e/release/mobile_delivery_test.py",
            "release/android-notes.json",
            "release/products.toml",
            "tools/release/android_release.py",
            "tools/release/ios_profiles.py",
            "tools/release/mobile.py",
        ]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"check-expo.yml", "release-checks.yml", "repo-docs-check.yml"},
        )

    def test_pr_2414_prepare_release_workflow_runs_only_release_and_docs(self):
        files = [
            ".github/prompts/prepare-release-changelog.codex.md",
            ".github/workflows/prepare-release.yml",
            ".github/workflows/release-on-merge.yml",
            "docs/architecture/releases.md",
            "e2e/release/prepare_release_test.py",
            "tools/release/prepare_release.py",
        ]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"release-checks.yml", "repo-docs-check.yml"},
        )

    def test_pr_2415_v1_migration_core_runs_go_cli_mobile_and_docs(self):
        files = [
            "docs/architecture/pipeline.md",
            "e2e/cli/testdata/history-auto-cleanup.txtar",
            "e2e/headless/migration_tags_test.go",
            "engine/options.go",
            "internal/localconfig/config.go",
            "internal/v1migration/migrate.go",
        ]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"pr-checks.yml", "build-cli.yml", "check-expo.yml", "repo-docs-check.yml"},
        )

    def test_pr_2413_desktop_quickpanel_and_localconfig(self):
        files = [
            "app/desktop/internal/desktopui/live_settings.go",
            "app/desktop/internal/quickpanel/placement.go",
            "docs/contracts/desktop-quick-panel-contract.md",
            "internal/localconfig/config.go",
        ]
        self.assertEqual(
            pr_trigger_workflows(files),
            {"pr-checks.yml", "build-cli.yml", "check-expo.yml", "repo-docs-check.yml"},
        )

    def test_pure_desktop_ui_only_runs_pr_checks(self):
        files = [
            "app/desktop/internal/desktopui/live.go",
            "app/desktop/boardui/button.go",
        ]
        self.assertEqual(pr_trigger_workflows(files), {"pr-checks.yml"})

    def test_pure_repo_docs_only_runs_repo_docs_check(self):
        self.assertEqual(
            pr_trigger_workflows(["README.md", "docs/architecture/overview.md"]),
            {"repo-docs-check.yml"},
        )

    def test_pure_docs_site_only_runs_docs_check(self):
        self.assertEqual(
            pr_trigger_workflows(["docs-site/content/docs/index.mdx"]),
            {"docs-check.yml"},
        )

    def test_pure_packaging_change_runs_package_acceptance(self):
        self.assertEqual(
            pr_trigger_workflows(["packaging/windows/installer.nsi"]),
            {"package-acceptance.yml"},
        )

    def test_pure_nix_change_runs_check_nix(self):
        self.assertEqual(
            pr_trigger_workflows(["nix/package.nix"]),
            {"check-nix.yml"},
        )

    def test_desktop_only_internal_does_not_trigger_mobile(self):
        self.assertEqual(
            pr_trigger_workflows(["internal/localapi/api.go"]),
            {"pr-checks.yml", "build-cli.yml"},
        )
        self.assertEqual(
            pr_trigger_workflows(["internal/desktopupdate/updater.go"]),
            {"pr-checks.yml", "build-cli.yml", "package-acceptance.yml"},
        )


class ReleaseMetadataValidationTests(unittest.TestCase):
    def test_current_repo_release_metadata_is_valid(self):
        validate_release.validate_repo(ROOT)


if __name__ == "__main__":
    unittest.main()
