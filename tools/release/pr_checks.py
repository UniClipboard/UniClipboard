"""Select repository checks from the files changed by a pull request."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

from mobile import mobile_metadata
from prepare_release import changelog_paths, parse_release_branch, update_toml_section
from release import metadata


MANIFEST = "release/products.toml"
CHANGELOGS = "release/changelogs/"
RELEASE_NOTES = {"release/android-notes.json", "release/desktop-notes.json"}
INDEPENDENT_PATHS = ("app/expo/", "bindings/", "nix/", "packaging/")
SCOPES = {
    "cli": (
        ".github/workflows/build-cli.yml",
        "app/service/", "e2e/cli/", "e2e/headless/", "e2e/packaging/", "e2e/release/",
        "engine/", "internal/", "plugin/", "sourceapp/", "tools/package/",
        "tools/release/", "go.mod", "go.sum", "Makefile", "LICENSE", ".gitattributes", MANIFEST,
    ),
    "mobile": (
        ".github/workflows/check-expo.yml", "app/expo/", "bindings/",
        "engine/", "internal/", "plugin/", "sourceapp/", "go.mod", "go.sum",
        "tools/release/mobile.py", "tools/release/release.py", MANIFEST,
    ),
    "nix": (
        ".github/workflows/check-nix.yml", "flake.nix", "nix/", "go.mod",
        "go.sum", "packaging/linux/", "app/desktop/internal/appicons/",
        "app/desktop/boardui/assets/", MANIFEST,
    ),
    "package": (
        ".github/workflows/package-acceptance.yml", ".github/workflows/build-cli.yml",
        ".github/workflows/build-desktop.yml", ".github/workflows/build-desktop-test.yml",
        ".github/workflows/prepare-legacy-desktop-alpha.yml",
        ".github/workflows/release-cli.yml", ".github/workflows/release-desktop.yml",
        ".github/release-notes/", "tools/package/", "tools/release/", "packaging/",
        "e2e/packaging/", "internal/localpaths/", "internal/releaseinfo/", "release/",
        "app/desktop/cmd/unic-desktop/", "app/desktop/internal/appupdates/",
        "internal/desktopupdate/", "app/service/cmd/unicd/", "app/desktop/e2e/updateprobe/",
        "LICENSE", ".gitattributes",
    ),
}


def git(*args):
    return subprocess.check_output(["git", *args]).decode(errors="surrogateescape")


def is_documentation(path):
    if path.startswith(("docs/", "docs-site/")):
        return True
    return path.endswith(".md") and (
        path != "app/desktop/e2e/CASES.md"
        and not path.startswith((CHANGELOGS, ".github/prompts/", ".github/release-notes/"))
    )


def generated_release(paths, branch, base, head):
    if not branch or not paths or not Path(MANIFEST).is_file():
        return False
    try:
        release = parse_release_branch(branch)
    except ValueError:
        return False
    platform = release["platform"]
    version = release["version"]
    build_number = release["build_number"]
    changelogs = changelog_paths(platform, version, build_number)
    allowed = {MANIFEST, *map(str, changelogs)}
    if platform == "desktop":
        allowed.add("release/desktop-notes.json")
    if not paths <= allowed:
        return False

    # Require the same manifest bytes that Prepare Release would generate.
    expected = git("show", f"{base}:{MANIFEST}")
    products = ("android", "ios") if platform == "mobile" else (platform,)
    new_build = None if platform == "desktop" and release["channel"] == "stable" else build_number
    for product in products:
        expected = update_toml_section(expected, product, version, new_build)
    if expected != git("show", f"{head}:{MANIFEST}"):
        return False

    for product in products:
        if product in {"android", "ios"}:
            mobile_metadata(product, Path(MANIFEST))
        else:
            metadata(MANIFEST, product)
            if build_number > 65535:
                raise ValueError(f"{product} build number exceeds Windows limit 65535")

    subprocess.run(
        [sys.executable, str(Path(__file__).with_name("prepare_release.py")),
         "parse-branch", "--branch", branch, "--repo-root", ".", "--validate-repo"],
        check=True,
    )
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--release-branch", default="")
    args = parser.parse_args()

    changed = set(git("diff", "--name-only", "--no-renames", "-z", args.base, args.head, "--").split("\0"))
    changed.discard("")
    docs = {path for path in changed if is_documentation(path)}
    paths = changed - docs
    if generated_release(paths, args.release_branch, args.base, args.head):
        paths.clear()
    paths = {path for path in paths if path not in RELEASE_NOTES and not path.startswith(CHANGELOGS)}

    checks = {
        "docs": any(not p.startswith("docs-site/") for p in docs),
        "shared": any(path != "flake.nix" and not path.startswith(INDEPENDENT_PATHS) for path in paths),
    }
    for name, prefixes in SCOPES.items():
        files = {value for value in prefixes if not value.endswith("/")}
        directories = tuple(value for value in prefixes if value.endswith("/"))
        checks[name] = any(path in files or path.startswith(directories) for path in paths)

    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for check, enabled in checks.items():
            print(f"{check}={str(enabled).lower()}", file=output)


if __name__ == "__main__":
    main()
