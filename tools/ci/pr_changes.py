#!/usr/bin/env python3
"""Classify changed files for PR checks job gating."""

from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

ALL_TRUE = {
    "lint": True,
    "test": True,
    "headless": True,
    "desktop_e2e": True,
}

# Files that require all PR check jobs when changed.
GLOBAL_ALL_FILES = {
    "go.mod",
    "go.sum",
    "Makefile",
    ".github/workflows/pr-checks.yml",
}

DESKTOP_E2E_WORKFLOW_FILES = {
    ".github/workflows/desktop-e2e-nightly.yml",
}

LINT_ONLY_FILES = {
    ".golangci.yml",
    ".golangci-lint-version",
}

GO_ROOT_DIRS = (
    "engine/",
    "internal/",
    "plugin/",
    "plugins/",
    "sourceapp/",
    "app/service/",
    "app/desktop/",
    "e2e/",
    "tools/docscheck/",
    "tools/release/update-sign/",
)

# Directories under e2e/ that contain only Python/PowerShell/packaging scripts
# and are not part of the root Go module test/lint set.
NON_GO_E2E_DIRS = (
    "e2e/packaging/",
    "e2e/release/",
)

# Directories under app/desktop/e2e/ or files that are Desktop E2E harness/cases
# rather than Go source code (no Go lint/test needed if only these change).
DESKTOP_E2E_NON_GO_FILES = {
    "app/desktop/e2e/run.py",
    "app/desktop/e2e/timing.py",
    "app/desktop/e2e/timings.json",
    "app/desktop/e2e/CASES.md",
}

HEADLESS_DIRS = (
    "engine/",
    "internal/",
    "plugin/",
    "sourceapp/",
    "app/service/",
    "e2e/headless/",
)

HEADLESS_EXCLUDED_INTERNAL = (
    "internal/mobilemigration/",
    "internal/waylandwire/",
)

DESKTOP_E2E_DIRS = (
    "engine/",
    "internal/",
    "plugin/",
    "sourceapp/",
    "app/desktop/",
    "app/service/",
)

DESKTOP_E2E_EXCLUDED_INTERNAL = (
    "internal/mobilemigration/",
    "internal/relayclient/",
    "internal/relaycrypto/",
    "internal/relayserver/",
    "internal/rendezvous/",
    "internal/tcnet/",
    "internal/testboard/",
    "internal/webtransfer/",
    "internal/wire/",
)


def extra_desktop_e2e_cases(paths: list[str], repo_root: Path = ROOT) -> list[str]:
    """Return sorted isolated Desktop E2E case IDs whose .txtar files changed."""
    prefix = "app/desktop/e2e/cases/"
    found: set[str] = set()
    for raw in paths:
        path = raw
        while path.startswith("./"):
            path = path[2:]
        if not path.startswith(prefix) or not path.endswith(".txtar"):
            continue
        case_file = repo_root / path
        if not case_file.is_file():
            continue
        text = case_file.read_text(encoding="utf-8")
        if "# Environment: disposable-guest\n" in text or "# Environment: owned-window-guest\n" in text:
            continue
        found.add(case_file.stem)
    return sorted(found)


def is_ignored_doc(path: str) -> bool:
    name = PurePosixPath(path).name
    if name == "AGENTS.md":
        return True
    if name.startswith("README") and name.endswith(".md"):
        return True
    return False


def is_root_go_module_file(path: str) -> bool:
    if path.startswith("tools/package/"):
        return path.endswith(".go")
    if any(path.startswith(d) for d in NON_GO_E2E_DIRS):
        return False
    if path in DESKTOP_E2E_NON_GO_FILES or path.startswith("app/desktop/e2e/cases/"):
        return False
    if path.startswith("app/desktop/e2e/") and path.endswith((".py", ".sh", ".ps1", ".json", ".txtar")):
        return False
    return any(path.startswith(d) for d in GO_ROOT_DIRS)


def classify_paths(paths: list[str]) -> dict[str, bool]:
    """Return which jobs in pr-checks.yml should run for the given changed paths."""
    result = {
        "lint": False,
        "test": False,
        "headless": False,
        "desktop_e2e": False,
    }
    for raw in paths:
        path = raw
        while path.startswith("./"):
            path = path[2:]
        if not path or is_ignored_doc(path):
            continue

        if path in GLOBAL_ALL_FILES:
            return dict(ALL_TRUE)

        if path in LINT_ONLY_FILES:
            result["lint"] = True
            continue

        if path in DESKTOP_E2E_WORKFLOW_FILES:
            result["desktop_e2e"] = True
            continue

        if path.startswith("tools/ci/"):
            # CI helper scripts are verified directly in the changes job; no Go/E2E build needed.
            continue

        if is_root_go_module_file(path):
            result["lint"] = True
            result["test"] = True

        if any(path.startswith(d) for d in HEADLESS_DIRS) and not any(
            path.startswith(ex) for ex in HEADLESS_EXCLUDED_INTERNAL
        ):
            result["headless"] = True

        if any(path.startswith(d) for d in DESKTOP_E2E_DIRS) and not any(
            path.startswith(ex) for ex in DESKTOP_E2E_EXCLUDED_INTERNAL
        ):
            result["desktop_e2e"] = True

    return result


def git_changed_files(base: str, head: str) -> list[str]:
    out = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z", base, head],
        text=True,
    )
    return [entry for entry in out.split("\0") if entry]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="Base git ref/SHA for git diff")
    parser.add_argument("--head", default="HEAD", help="Head git ref/SHA for git diff")
    parser.add_argument("--all", action="store_true", help="Force all checks enabled")
    parser.add_argument("--github-output", help="Path to $GITHUB_OUTPUT")
    parser.add_argument("paths", nargs="*", help="Explicit changed file paths")
    args = parser.parse_args()

    extra_cases: list[str] = []
    if args.all:
        flags = dict(ALL_TRUE)
    elif args.paths:
        flags = classify_paths(args.paths)
        extra_cases = extra_desktop_e2e_cases(args.paths)
    elif args.base:
        try:
            changed = git_changed_files(args.base, args.head)
        except subprocess.CalledProcessError as exc:
            print(
                f"warning: git diff {args.base}..{args.head} failed ({exc}); running all checks",
                file=sys.stderr,
            )
            flags = dict(ALL_TRUE)
        else:
            print(f"Classified {len(changed)} changed file(s):")
            for item in changed:
                print(f"  - {item}")
            flags = classify_paths(changed)
            extra_cases = extra_desktop_e2e_cases(changed)
    else:
        parser.error("provide --all, --base, or explicit paths")

    lines = [f"{key}={'true' if value else 'false'}" for key, value in flags.items()]
    lines.append(f"desktop_e2e_extra_cases={','.join(extra_cases)}")
    for line in lines:
        print(line)
    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as fh:
            for line in lines:
                fh.write(line + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
