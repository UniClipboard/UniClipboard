#!/usr/bin/env python3
"""Unit and workflow-step tests for prepare-release and release-on-merge."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))
import prepare_release  # noqa: E402

PREPARE_WORKFLOW = ROOT / ".github/workflows/prepare-release.yml"
MERGE_WORKFLOW = ROOT / ".github/workflows/release-on-merge.yml"
RELEASE_CLI_WORKFLOW = ROOT / ".github/workflows/release-cli.yml"
RELEASE_DESKTOP_WORKFLOW = ROOT / ".github/workflows/release-desktop.yml"
LEGACY_ALPHA_WORKFLOW = ROOT / ".github/workflows/prepare-legacy-desktop-alpha.yml"
PROMPT_TEMPLATE = ROOT / ".github/prompts/prepare-release-changelog.codex.md"


def load_yaml(path):
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


def workflow_on(workflow):
    for key in ("on", True, "true"):
        if key in workflow:
            return workflow[key]
    raise KeyError("workflow is missing 'on' trigger block")


def workflow_step(workflow, job, step_name):
    for step in workflow["jobs"][job]["steps"]:
        if step.get("name") == step_name:
            return step
    raise AssertionError(f"missing step {job}/{step_name}")


def run_bash(cwd, script, env):
    full_env = {**os.environ, **env}
    # Prevent host BASH_ENV hooks from prepending real binaries ahead of fake test shims.
    full_env.pop("BASH_ENV", None)
    full_env.pop("ENV", None)
    return subprocess.run(
        ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script],
        cwd=cwd,
        env=full_env,
        text=True,
        capture_output=True,
        check=True,
    )


def run_git(cwd, *args):
    return subprocess.check_output(
        ["git", *args], cwd=cwd, text=True, stderr=subprocess.PIPE
    ).strip()


FIXTURE_PRODUCTS_TOML = """schema = 1

[products.cli]
version = "2.0.0"
build_number = 7
targets = ["darwin-arm64", "darwin-amd64", "linux-arm64", "linux-amd64", "windows-amd64", "windows-arm64"]

[products.desktop]
version = "2.0.0"
build_number = 8
targets = ["darwin-arm64", "darwin-amd64", "linux-arm64", "linux-amd64", "windows-amd64", "windows-arm64"]
cli_version = "2.0.0"
cli_build_number = 3
cli_manifest_sha256 = "e09cf4ed4ca6c357c585d11660ca64583d565901af64e15c5a6a8ca8e46fbdc4"

cli_artifact_run_id = 45138690798
# Configure the durable raw Ed25519 public key before enabling in-app updates.
update_public_key = "4/pGKRfV8KGneAnMEci+DnpdrnoFLM5uVhDq0Kp/pvk="

[products.android]
version = "3.0.0"
build_number = 205
targets = ["android-apk-arm64", "android-aab"]
application_id = "app.uniclipboard.android"
legacy_build_floor = 204

[products.ios]
version = "3.0.0"
build_number = 205
targets = ["ios-arm64"]
application_id = "app.uniclipboard.UniClipboard"
legacy_build_floor = 204
"""


class PrepareReleaseScriptTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="unic-prepare-release-test-"))
        self.repo = self.tmp / "repo"
        self.repo.mkdir()
        run_git(self.repo, "init", "-b", "main")
        run_git(self.repo, "config", "user.name", "Release Test")
        run_git(self.repo, "config", "user.email", "release@example.test")
        (self.repo / "release").mkdir(parents=True)
        (self.repo / "release/products.toml").write_text(
            FIXTURE_PRODUCTS_TOML, encoding="utf-8", newline="\n"
        )
        shutil.copy2(
            ROOT / "release/desktop-notes.json", self.repo / "release/desktop-notes.json"
        )
        run_git(self.repo, "add", ".")
        run_git(self.repo, "commit", "-m", "initial")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write_inventory(self, data):
        path = self.tmp / "inventory.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        return path

    def run_cli(self, *args):
        return subprocess.check_output(
            [sys.executable, str(ROOT / "tools/release/prepare_release.py"), *args],
            cwd=self.repo,
            text=True,
        )

    def test_plan_desktop_alpha_respects_tags_and_reservations(self):
        inv = self.write_inventory(
            {
                "tags": [
                    "desktop/v2.0.0-build.11",
                    "joint-release/alpha/v2.0.0/45810962595-11-12",
                ],
                "releases": [
                    {
                        "tag_name": "desktop/v2.0.0-build.11",
                        "draft": False,
                        "prerelease": True,
                    }
                ],
                "npm_cli_builds": [],
            }
        )
        out = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "desktop",
                "--channel",
                "alpha",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(inv),
            )
        )
        self.assertEqual(out["version"], "2.0.0")
        self.assertEqual(out["build_number"], 13)
        self.assertEqual(out["branch"], "release/desktop-alpha-v2.0.0-build.13")
        self.assertEqual(out["previous_tag"], "desktop/v2.0.0-build.11")
        self.assertEqual(out["desktop_notes"], "release/desktop-notes.json")
        self.assertEqual(out["needs_acceptance"], "false")

    def test_plan_desktop_stable_picks_highest_accepted_or_warns(self):
        inv_accepted = self.write_inventory(
            {
                "tags": [
                    "v1.6.2",
                    "desktop/v2.0.0-build.11",
                    "joint-release/accepted/v2.0.0-build.10",
                    "joint-release/accepted/v2.0.0-build.11",
                ],
                "releases": [
                    {"tag_name": "v1.6.2", "draft": False, "prerelease": False},
                    {
                        "tag_name": "desktop/v2.0.0-build.11",
                        "draft": False,
                        "prerelease": True,
                    },
                ],
            }
        )
        out = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "desktop",
                "--channel",
                "stable",
                "--version",
                "v2.0.0",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(inv_accepted),
            )
        )
        self.assertEqual(out["build_number"], 11)
        self.assertEqual(out["branch"], "release/desktop-stable-v2.0.0-build.11")
        self.assertEqual(out["previous_tag"], "v1.6.2")
        self.assertEqual(out["needs_acceptance"], "false")

        inv_unaccepted = self.write_inventory(
            {
                "tags": ["desktop/v2.0.0-build.11"],
                "releases": [],
            }
        )
        out2 = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "desktop",
                "--channel",
                "stable",
                "--version",
                "2.0.0",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(inv_unaccepted),
            )
        )
        self.assertEqual(out2["needs_acceptance"], "true")

    def test_plan_cli_respects_npm_builds_and_new_version(self):
        inv = self.write_inventory(
            {
                "tags": ["cli/v2.0.0-build.10"],
                "releases": [
                    {
                        "tag_name": "cli/v2.0.0-build.10",
                        "draft": False,
                        "prerelease": True,
                    }
                ],
                "npm_cli_builds": [9, 10, 11],
            }
        )
        out = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "cli",
                "--channel",
                "alpha",
                "--version",
                "2.0.0",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(inv),
            )
        )
        self.assertEqual(out["build_number"], 12)
        self.assertEqual(out["previous_tag"], "cli/v2.0.0-build.10")
        self.assertEqual(out["desktop_notes"], "")

        out_new = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "cli",
                "--channel",
                "stable",
                "--version",
                "2.1.0",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(self.write_inventory({"tags": [], "releases": [], "npm_cli_builds": []})),
            )
        )
        self.assertEqual(out_new["version"], "2.1.0")
        self.assertEqual(out_new["build_number"], 1)

    def test_plan_mobile_advances_monotonically_across_versions(self):
        inv = self.write_inventory(
            {
                "tags": ["android/v2.5.0-build.205"],
                "releases": [],
                "npm_cli_builds": [],
            }
        )
        out = json.loads(
            self.run_cli(
                "plan",
                "--platform",
                "mobile",
                "--channel",
                "alpha",
                "--version",
                "2.6.0",
                "--metadata",
                str(self.repo / "release/products.toml"),
                "--repo-root",
                str(self.repo),
                "--offline-inventory",
                str(inv),
            )
        )
        self.assertEqual(out["version"], "2.6.0")
        self.assertEqual(out["build_number"], 206)
        self.assertEqual(out["branch"], "release/mobile-alpha-v2.6.0-build.206")
        self.assertEqual(out["previous_tag"], "android/v2.5.0-build.205")

    def test_bump_preserves_lf_comments_and_unrelated_sections(self):
        meta_path = self.repo / "release/products.toml"
        shutil.copy2(ROOT / "release/products.toml", meta_path)
        before = tomllib.loads(meta_path.read_text(encoding="utf-8"))
        next_mobile_build = (
            max(
                before["products"]["android"]["build_number"],
                before["products"]["ios"]["build_number"],
            )
            + 1
        )
        self.run_cli(
            "bump",
            "--platform",
            "mobile",
            "--channel",
            "alpha",
            "--version",
            "2.6.0",
            "--build-number",
            str(next_mobile_build),
            "--metadata",
            str(meta_path),
        )
        raw = meta_path.read_bytes()
        self.assertNotIn(b"\r\n", raw)
        self.assertIn(b"# Configure the durable raw Ed25519 public key", raw)
        parsed = tomllib.loads(raw.decode("utf-8"))
        self.assertEqual(parsed["products"]["android"]["version"], "2.6.0")
        self.assertEqual(parsed["products"]["android"]["build_number"], next_mobile_build)
        self.assertEqual(parsed["products"]["ios"]["version"], "2.6.0")
        self.assertEqual(parsed["products"]["ios"]["build_number"], next_mobile_build)
        # Unrelated sections untouched.
        self.assertEqual(parsed["products"]["desktop"], before["products"]["desktop"])
        self.assertEqual(parsed["products"]["cli"], before["products"]["cli"])

    def test_changelog_init_render_validate_and_parse_branch(self):
        self.run_cli(
            "bump",
            "--platform",
            "desktop",
            "--channel",
            "alpha",
            "--version",
            "2.0.0",
            "--build-number",
            "13",
            "--metadata",
            str(self.repo / "release/products.toml"),
        )
        self.run_cli(
            "init-changelog",
            "--platform",
            "desktop",
            "--version",
            "2.0.0",
            "--build-number",
            "13",
            "--date",
            "2026-11-26",
            "--repo-root",
            str(self.repo),
        )
        # Skeleton must fail validation until placeholders are replaced.
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_cli(
                "validate-changelog",
                "--platform",
                "desktop",
                "--version",
                "2.0.0",
                "--build-number",
                "13",
                "--repo-root",
                str(self.repo),
            )

        # Render prompt and verify all tokens were substituted.
        prompt_out = self.tmp / "prompt.md"
        self.run_cli(
            "render-prompt",
            "--template",
            str(PROMPT_TEMPLATE),
            "--platform",
            "desktop",
            "--channel",
            "alpha",
            "--version",
            "2.0.0",
            "--build-number",
            "13",
            "--previous-tag",
            "desktop/v2.0.0-build.11",
            "--previous-version",
            "2.0.0-build.11",
            "--range-expression",
            "desktop/v2.0.0-build.11..HEAD",
            "--date",
            "2026-11-26",
            "--output",
            str(prompt_out),
        )
        prompt_text = prompt_out.read_text(encoding="utf-8")
        self.assertNotIn("{{", prompt_text)
        self.assertIn("release/desktop-notes.json", prompt_text)
        self.assertIn("desktop/v2.0.0-build.11..HEAD", prompt_text)

        # Finalize changelog files.
        en_file = self.repo / "release/changelogs/desktop/v2.0.0-build.13.md"
        zh_file = self.repo / "release/changelogs/desktop/v2.0.0-build.13.zh.md"
        en_file.write_text(
            "## v2.0.0-build.13 (desktop) - 2026-11-26\n\n### Features\n\n- Show pairing QR codes (#2403)\n",
            encoding="utf-8",
        )
        zh_file.write_text(
            "## v2.0.0-build.13 (desktop) - 2026-11-26\n\n### Features\n\n- 支持展示配对二维码 (#2403)\n",
            encoding="utf-8",
        )
        notes_file = self.repo / "release/desktop-notes.json"
        # Forbidden <!-- delimiter in desktop-notes.json must fail.
        notes_file.write_text(
            json.dumps({"en": "Hello <!-- zh -->", "zh": "你好"}), encoding="utf-8"
        )
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_cli(
                "validate-changelog",
                "--platform",
                "desktop",
                "--version",
                "2.0.0",
                "--build-number",
                "13",
                "--repo-root",
                str(self.repo),
            )

        notes_file.write_text(
            json.dumps(
                {
                    "en": "Pairing now displays QR codes for faster setup. All paired devices must use v2.",
                    "zh": "配对界面现已支持二维码展示。所有配对设备必须使用 v2。",
                },
                ensure_ascii=False,
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        self.run_cli(
            "validate-changelog",
            "--platform",
            "desktop",
            "--version",
            "2.0.0",
            "--build-number",
            "13",
            "--repo-root",
            str(self.repo),
        )

        parsed = json.loads(
            self.run_cli(
                "parse-branch",
                "--branch",
                "release/desktop-alpha-v2.0.0-build.13",
                "--validate-repo",
                "--repo-root",
                str(self.repo),
            )
        )
        self.assertEqual(parsed["platform"], "desktop")
        self.assertEqual(parsed["channel"], "alpha")
        self.assertEqual(parsed["version"], "2.0.0")
        self.assertEqual(parsed["build_number"], 13)
        self.assertEqual(parsed["workflows"], "release-cli.yml")


class WorkflowExecutionTests(unittest.TestCase):
    def setUp(self):
        self.prepare_wf = load_yaml(PREPARE_WORKFLOW)
        self.merge_wf = load_yaml(MERGE_WORKFLOW)
        self.tmp = Path(tempfile.mkdtemp(prefix="unic-wf-test-"))

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def make_sandbox(self, branch):
        origin = self.tmp / "origin.git"
        repo = self.tmp / "work"
        run_git(self.tmp, "init", "--bare", str(origin))
        run_git(self.tmp, "clone", str(origin), str(repo))
        run_git(repo, "config", "user.name", "Release Test")
        run_git(repo, "config", "user.email", "release@example.test")
        (repo / "release").mkdir(parents=True)
        shutil.copy2(ROOT / "release/products.toml", repo / "release/products.toml")
        shutil.copy2(
            ROOT / "release/desktop-notes.json", repo / "release/desktop-notes.json"
        )
        run_git(repo, "add", ".")
        run_git(repo, "commit", "-m", "initial")
        base_sha = run_git(repo, "rev-parse", "HEAD")
        run_git(repo, "push", "origin", "HEAD:main")
        run_git(repo, "checkout", "-b", branch)
        (repo / "release/changelogs/desktop").mkdir(parents=True)
        (repo / "release/changelogs/desktop/v2.0.0-build.13.md").write_text(
            "## v2.0.0-build.13 (desktop) - 2026-11-26\n\n### Features\n\n- Add feature (#1)\n",
            encoding="utf-8",
        )
        (repo / "release/changelogs/desktop/v2.0.0-build.13.zh.md").write_text(
            "## v2.0.0-build.13 (desktop) - 2026-11-26\n\n### Features\n\n- 新增功能 (#1)\n",
            encoding="utf-8",
        )
        run_git(repo, "add", ".")
        run_git(repo, "commit", "-m", "prepare release")
        return origin, repo, base_sha

    def test_workflows_declare_expected_triggers_and_codex_reuse(self):
        # PyYAML parses bare `on` as boolean True; Ruby Psych + JSON stringifies it as "true".
        on_cfg = workflow_on(self.prepare_wf)
        inputs = on_cfg["workflow_dispatch"]["inputs"]
        self.assertEqual(set(inputs.keys()), {"platform", "channel", "version", "base_branch"})
        self.assertEqual(inputs["platform"]["options"], ["desktop", "cli", "mobile", "android", "ios"])
        self.assertEqual(inputs["channel"]["options"], ["alpha", "stable"])

        codex_step = workflow_step(self.prepare_wf, "changelog", "Codex generate changelog")
        self.assertEqual(codex_step["uses"], "openai/codex-action@v1")
        self.assertIn("CODEX_API_ENDPOINT", codex_step["with"]["responses-api-endpoint"])
        self.assertIn("CODEX_API_KEY", codex_step["with"]["openai-api-key"])

        merge_on = workflow_on(self.merge_wf)
        self.assertEqual(merge_on["pull_request"]["types"], ["closed"])
        self.assertEqual(merge_on["pull_request"]["branches"], ["main"])

    def test_checkpoint_bundle_push_idempotency_and_pr_creation(self):
        branch = "release/desktop-alpha-v2.0.0-build.13"
        origin, repo, base_sha = self.make_sandbox(branch)
        runner_temp = self.tmp / "runner-temp"
        runner_temp.mkdir()

        # Save checkpoint in prepare, restore in changelog.
        run_bash(
            repo,
            workflow_step(self.prepare_wf, "prepare", "Save release checkpoint")["run"],
            {"BASE_SHA": base_sha, "RUNNER_TEMP": str(runner_temp)},
        )
        ckpt_dir = runner_temp / "release-checkpoint"
        ckpt_dir.mkdir()
        shutil.copy2(runner_temp / "release.bundle", ckpt_dir / "release.bundle")

        next_repo = self.tmp / "changelog-work"
        run_git(self.tmp, "clone", str(origin), str(next_repo))
        run_git(next_repo, "checkout", base_sha)
        run_bash(
            next_repo,
            workflow_step(self.prepare_wf, "changelog", "Restore release branch")["run"],
            {"BRANCH": branch, "RUNNER_TEMP": str(runner_temp)},
        )
        self.assertEqual(run_git(next_repo, "rev-parse", "HEAD"), run_git(repo, "rev-parse", "HEAD"))
        self.assertEqual(run_git(next_repo, "branch", "--show-current"), branch)

        # Idempotent push succeeds twice, rejects diverged remote.
        push_script = workflow_step(self.prepare_wf, "publish", "Push release branch")["run"]
        run_bash(next_repo, push_script, {"BRANCH": branch})
        run_bash(next_repo, push_script, {"BRANCH": branch})

        (next_repo / "manual.txt").write_text("manual edit\n", encoding="utf-8")
        run_git(next_repo, "add", "manual.txt")
        run_git(next_repo, "commit", "-m", "manual")
        run_git(next_repo, "push", "origin", branch)
        run_git(next_repo, "reset", "--hard", "HEAD~1")
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            run_bash(next_repo, push_script, {"BRANCH": branch})
        self.assertIn("refusing to overwrite", ctx.exception.stderr)

        # Idempotent PR creation with fake gh.
        fake_bin = self.tmp / "bin"
        fake_bin.mkdir()
        gh_path = fake_bin / "gh"
        gh_path.write_text(
            """#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  'pr list')
    if [ -f "$RUNNER_TEMP/pr-created" ]; then cat "$RUNNER_TEMP/pr-created"; fi
    ;;
  'pr create')
    test ! -f "$RUNNER_TEMP/pr-created"
    grep -q 'Release desktop v2.0.0-build.13 (alpha)' "$RUNNER_TEMP/release-pr-body.md"
    grep -q 'Add feature (#1)' "$RUNNER_TEMP/release-pr-body.md"
    echo 'https://github.com/UniClipboard/UniClipboard/pull/9999' | tee "$RUNNER_TEMP/pr-created"
    ;;
  *) exit 1 ;;
esac
""",
            encoding="utf-8",
        )
        gh_path.chmod(0o755)
        gh_env_file = self.tmp / "github-env"
        gh_out_file = self.tmp / "github-out"
        pr_script = workflow_step(self.prepare_wf, "publish", "Create pull request")["run"]
        pr_env = {
            "BASE_BRANCH": "main",
            "PLATFORM": "desktop",
            "CHANNEL": "alpha",
            "VERSION": "2.0.0",
            "BUILD_NUMBER": "13",
            "IDENTITY": "v2.0.0-build.13",
            "BRANCH": branch,
            "CHANGELOG_EN": "release/changelogs/desktop/v2.0.0-build.13.md",
            "CHANGELOG_ZH": "release/changelogs/desktop/v2.0.0-build.13.zh.md",
            "DESKTOP_NOTES": "release/desktop-notes.json",
            "PREVIOUS_TAG": "desktop/v2.0.0-build.11",
            "NEEDS_ACCEPTANCE": "false",
            "RUNNER_TEMP": str(runner_temp),
            "GITHUB_ENV": str(gh_env_file),
            "GITHUB_OUTPUT": str(gh_out_file),
            "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
        }
        run_bash(next_repo, pr_script, pr_env)
        run_bash(next_repo, pr_script, pr_env)
        self.assertEqual(
            gh_env_file.read_text(encoding="utf-8").strip().splitlines(),
            [
                "PR_URL=https://github.com/UniClipboard/UniClipboard/pull/9999",
                "PR_URL=https://github.com/UniClipboard/UniClipboard/pull/9999",
            ],
        )

    def test_release_on_merge_dispatch_routing_all_platforms(self):
        fake_bin = self.tmp / "dispatch-bin"
        fake_bin.mkdir()
        log_file = self.tmp / "gh-calls.log"
        gh_path = fake_bin / "gh"
        gh_path.write_text(
            f"""#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >> {json.dumps(str(log_file))}
""",
            encoding="utf-8",
        )
        gh_path.chmod(0o755)
        dispatch_script = workflow_step(
            self.merge_wf, "dispatch", "Dispatch build and release workflow"
        )["run"]

        cases = [
            (
                {"PLATFORM": "desktop", "CHANNEL": "alpha", "VERSION": "2.0.0", "ANDROID_TRACK": "internal"},
                [
                    "workflow run release-cli.yml --repo example/repo --ref main -f channel=alpha -f version=2.0.0 -f expected_source=deadbeef"
                ],
            ),
            (
                {"PLATFORM": "desktop", "CHANNEL": "stable", "VERSION": "2.0.0", "ANDROID_TRACK": "production"},
                [
                    "workflow run release-cli.yml --repo example/repo --ref main -f channel=stable -f version=2.0.0 -f expected_source=deadbeef"
                ],
            ),
            (
                {"PLATFORM": "cli", "CHANNEL": "alpha", "VERSION": "2.0.0", "ANDROID_TRACK": "internal"},
                [
                    "workflow run release-cli.yml --repo example/repo --ref main -f channel=alpha -f expected_source=deadbeef"
                ],
            ),
            (
                {"PLATFORM": "mobile", "CHANNEL": "alpha", "VERSION": "2.5.0", "ANDROID_TRACK": "internal"},
                [
                    "workflow run release-android.yml --repo example/repo --ref main -f publish=true -f track=internal",
                    "workflow run release-ios.yml --repo example/repo --ref main -f publish=true",
                ],
            ),
            (
                {"PLATFORM": "mobile", "CHANNEL": "stable", "VERSION": "2.5.0", "ANDROID_TRACK": "production"},
                [
                    "workflow run release-android.yml --repo example/repo --ref main -f publish=true -f track=production",
                    "workflow run release-ios.yml --repo example/repo --ref main -f publish=true",
                ],
            ),
            (
                {"PLATFORM": "android", "CHANNEL": "stable", "VERSION": "2.5.0", "ANDROID_TRACK": "production"},
                [
                    "workflow run release-android.yml --repo example/repo --ref main -f publish=true -f track=production"
                ],
            ),
            (
                {"PLATFORM": "ios", "CHANNEL": "alpha", "VERSION": "2.5.0", "ANDROID_TRACK": "internal"},
                [
                    "workflow run release-ios.yml --repo example/repo --ref main -f publish=true"
                ],
            ),
        ]

        for extra_env, expected_calls in cases:
            if log_file.exists():
                log_file.unlink()
            run_bash(
                ROOT,
                dispatch_script,
                {
                    "GITHUB_REPOSITORY": "example/repo",
                    "BASE_REF": "main",
                    "MERGE_SHA": "deadbeef",
                    "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
                    **extra_env,
                },
            )
            actual = log_file.read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(actual, expected_calls)


class ReleaseSourceResolutionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="unic-release-source-test-"))
        self.release_cli_wf = load_yaml(RELEASE_CLI_WORKFLOW)
        self.release_desktop_wf = load_yaml(RELEASE_DESKTOP_WORKFLOW)
        self.legacy_alpha_wf = load_yaml(LEGACY_ALPHA_WORKFLOW)
        self.resolve_script = workflow_step(
            self.release_cli_wf, "plan", "Resolve and verify release source"
        )["run"]

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def make_fake_gh(self, body_json, exit_code=0):
        fake_bin = self.tmp / f"gh-bin-{abs(hash((body_json, exit_code)))}"
        fake_bin.mkdir(exist_ok=True)
        log_file = fake_bin / "gh.log"
        gh_path = fake_bin / "gh"
        gh_path.write_text(
            f"""#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >> {json.dumps(str(log_file))}
if [ {exit_code} -ne 0 ]; then
  printf '%s\\n' {json.dumps(body_json)} >&2
  exit {exit_code}
fi
printf '%s\\n' {json.dumps(body_json)}
""",
            encoding="utf-8",
        )
        gh_path.chmod(0o755)
        return fake_bin, log_file

    def run_resolve(self, head_sha, expected_source, fake_bin):
        out_file = self.tmp / "github-output"
        if out_file.exists():
            out_file.unlink()
        full_env = {
            **os.environ,
            "GITHUB_REPOSITORY": "UniClipboard/UniClipboard",
            "GITHUB_SHA": head_sha,
            "EXPECTED_SOURCE": expected_source,
            "GITHUB_OUTPUT": str(out_file),
            "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
        }
        full_env.pop("BASH_ENV", None)
        full_env.pop("ENV", None)
        proc = subprocess.run(
            ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", self.resolve_script],
            cwd=ROOT,
            env=full_env,
            text=True,
            capture_output=True,
        )
        outputs = {}
        if out_file.exists():
            for line in out_file.read_text(encoding="utf-8").splitlines():
                if "=" in line:
                    k, v = line.split("=", 1)
                    outputs[k] = v
        return proc, outputs

    def test_empty_and_identical_expected_source_skip_compare_api(self):
        head = "0eac2a06e234f8f54b89055dacd26f9787adb0b4"
        fake_bin, log_file = self.make_fake_gh('{"status":"diverged","behind_by":1}')
        for expected in ("", head):
            proc, outputs = self.run_resolve(head, expected, fake_bin)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            self.assertEqual(outputs["source_sha"], head)
            self.assertFalse(log_file.exists())

    def test_ancestor_expected_source_succeeds_after_concurrent_merge(self):
        merge_sha = "2012ae20bbf2c75eb9ef177f7634e4aeead5992a"
        head_sha = "0eac2a06e234f8f54b89055dacd26f9787adb0b4"
        fake_bin, log_file = self.make_fake_gh(
            json.dumps({"status": "ahead", "ahead_by": 1, "behind_by": 0})
        )
        proc, outputs = self.run_resolve(head_sha, merge_sha, fake_bin)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(outputs["source_sha"], merge_sha)
        self.assertEqual(
            log_file.read_text(encoding="utf-8").strip(),
            f"api repos/UniClipboard/UniClipboard/compare/{merge_sha}...{head_sha}",
        )

    def test_non_ancestor_and_malformed_expected_source_rejected(self):
        merge_sha = "2012ae20bbf2c75eb9ef177f7634e4aeead5992a"
        head_sha = "0eac2a06e234f8f54b89055dacd26f9787adb0b4"

        for bad_sha in ("not-a-sha", "2012ae20b", "../main", "2012AE20BBF2C75EB9EF177F7634E4AEEAD5992A"):
            fake_bin, log_file = self.make_fake_gh("{}")
            proc, _ = self.run_resolve(head_sha, bad_sha, fake_bin)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("40-character lowercase hex SHA", proc.stderr)
            self.assertFalse(log_file.exists())

        for payload in (
            {"status": "diverged", "ahead_by": 1, "behind_by": 1},
            {"status": "behind", "ahead_by": 0, "behind_by": 2},
        ):
            fake_bin, _ = self.make_fake_gh(json.dumps(payload))
            proc, _ = self.run_resolve(head_sha, merge_sha, fake_bin)
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("not an ancestor", proc.stderr)

    def test_release_workflows_propagate_resolved_source_sha(self):
        jobs = self.release_cli_wf["jobs"]
        self.assertEqual(
            jobs["plan"]["outputs"]["source_sha"], "${{ steps.source.outputs.source_sha }}"
        )
        for job_name in ("build", "desktop_build", "desktop_publish", "legacy_alpha", "desktop_promote"):
            self.assertEqual(
                jobs[job_name]["with"]["source_sha"], "${{ needs.plan.outputs.source_sha }}"
            )
            self.assertIn("plan", jobs[job_name]["needs"])
        desktop_on = workflow_on(self.release_desktop_wf)
        legacy_on = workflow_on(self.legacy_alpha_wf)
        self.assertIn("source_sha", desktop_on["workflow_call"]["inputs"])
        self.assertIn("source_sha", legacy_on["workflow_call"]["inputs"])

    def test_desktop_release_acquire_accepts_ancestor_and_rejects_diverged(self):
        import hashlib
        import desktop_release  # noqa: E402

        merge_sha = "2012ae20bbf2c75eb9ef177f7634e4aeead5992a"
        head_sha = "0eac2a06e234f8f54b89055dacd26f9787adb0b4"

        def make_artifacts_tree(root_dir, recorded_sha):
            for target in desktop_release.TARGETS:
                os_name = target.split("-")[0]
                tdir = root_dir / f"desktop-{target}"
                tdir.mkdir(parents=True, exist_ok=True)
                items = []
                for fmt in desktop_release.FORMATS[os_name]:
                    fname = f"pkg-{target}.{fmt}"
                    data = f"{target}:{fmt}".encode()
                    (tdir / fname).write_bytes(data)
                    items.append(
                        {
                            "format": fmt,
                            "file": fname,
                            "sha256": hashlib.sha256(data).hexdigest(),
                        }
                    )
                (tdir / "artifacts.json").write_text(
                    json.dumps(
                        {
                            "schema": 1,
                            "product": "desktop",
                            "target": target,
                            "source_sha": recorded_sha,
                            "source_clean": True,
                            "development": False,
                            "artifacts": items,
                        }
                    ),
                    encoding="utf-8",
                )

        def make_acquire_gh(compare_payload):
            fake_bin = self.tmp / f"acq-gh-{abs(hash(json.dumps(compare_payload, sort_keys=True)))}"
            fake_bin.mkdir(exist_ok=True)
            gh_path = fake_bin / "gh"
            gh_path.write_text(
                f"""#!/usr/bin/env python3
import json, sys
args = sys.argv[1:]
endpoint = args[1]
if endpoint.endswith('/actions/runs/45829686375'):
    print(json.dumps({{"id": 45829686375, "workflow_id": 77, "head_sha": {head_sha!r}, "created_at": "2026-11-26T13:29:49Z", "repository": {{"full_name": "UniClipboard/UniClipboard"}}}}))
elif endpoint.endswith('/actions/workflows/release-cli.yml'):
    print(json.dumps({{"id": 77}}))
elif endpoint.endswith('/actions/workflows/build-desktop.yml'):
    print(json.dumps({{"id": 88}}))
elif '/compare/' in endpoint:
    print(json.dumps({compare_payload!r}))
else:
    sys.exit(f"unexpected gh call: {{args}}")
""",
                encoding="utf-8",
            )
            gh_path.chmod(0o755)
            return fake_bin

        # Ancestor source SHA succeeds and records merge_sha in acquisition.json.
        in_dir = self.tmp / "in-ok"
        out_dir = self.tmp / "out-ok"
        make_artifacts_tree(in_dir, merge_sha)
        fake_bin = make_acquire_gh({"status": "ahead", "ahead_by": 1, "behind_by": 0})
        env = {
            **os.environ,
            "GITHUB_RUN_ID": "45829686375",
            "GITHUB_SHA": head_sha,
            "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
        }
        proc = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/release/desktop_release.py"),
                "acquire",
                "--run-id",
                "45829686375",
                "--current-artifacts",
                str(in_dir),
                "--source-sha",
                merge_sha,
                "--output",
                str(out_dir),
            ],
            cwd=ROOT,
            env=env,
            text=True,
            capture_output=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)
        acq = json.loads((out_dir / "acquisition.json").read_text(encoding="utf-8"))
        self.assertEqual(acq["source_sha"], merge_sha)

        # Diverged source SHA is rejected.
        in_bad = self.tmp / "in-diverged"
        out_bad = self.tmp / "out-diverged"
        make_artifacts_tree(in_bad, merge_sha)
        fake_bad = make_acquire_gh({"status": "diverged", "ahead_by": 1, "behind_by": 1})
        env_bad = {**env, "PATH": f"{fake_bad}{os.pathsep}{os.environ['PATH']}"}
        proc = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/release/desktop_release.py"),
                "acquire",
                "--run-id",
                "45829686375",
                "--current-artifacts",
                str(in_bad),
                "--source-sha",
                merge_sha,
                "--output",
                str(out_bad),
            ],
            cwd=ROOT,
            env=env_bad,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("not an ancestor", proc.stderr)

        # Artifact recorded with wrong source SHA (e.g. head_sha instead of merge_sha) is rejected.
        in_mismatch = self.tmp / "in-mismatch"
        out_mismatch = self.tmp / "out-mismatch"
        make_artifacts_tree(in_mismatch, head_sha)
        proc = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/release/desktop_release.py"),
                "acquire",
                "--run-id",
                "45829686375",
                "--current-artifacts",
                str(in_mismatch),
                "--source-sha",
                merge_sha,
                "--output",
                str(out_mismatch),
            ],
            cwd=ROOT,
            env=env,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("candidate source disagrees", proc.stderr)

        # --source-sha without --current-artifacts is rejected.
        proc = subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/release/desktop_release.py"),
                "acquire",
                "--run-id",
                "45829686375",
                "--source-sha",
                merge_sha,
                "--output",
                str(self.tmp / "out-no-current"),
            ],
            cwd=ROOT,
            env=env,
            text=True,
            capture_output=True,
        )
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("--source-sha is supported only with --current-artifacts", proc.stderr)


if __name__ == "__main__":
    unittest.main()
