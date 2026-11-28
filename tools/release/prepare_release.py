#!/usr/bin/env python3
"""Prepare multi-platform v2 release PRs and validate merge-time dispatch."""

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

sys.dont_write_bytecode = True
import mobile as mobile_release  # noqa: E402
import release as cli_release  # noqa: E402

ROOT = cli_release.ROOT
REPO = "UniClipboard/UniClipboard"
NPM = "https://registry.npmjs.org"
PLATFORMS = ("desktop", "cli", "mobile", "android", "ios")
CHANNELS = ("alpha", "stable")
BUILD_RE = r"[1-9][0-9]{0,8}"
BRANCH_RE = re.compile(
    rf"release/(?P<platform>{'|'.join(PLATFORMS)})-"
    rf"(?P<channel>{'|'.join(CHANNELS)})-"
    rf"v(?P<version>(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-"
    rf"build\.(?P<build>{BUILD_RE})\Z"
)
PLACEHOLDER_MARKERS = (
    "<!--",
    "Release notes are not available yet.",
    "Release notes pending.",
    "Pending release notes.",
    "No desktop installer artifacts found for this release.",
    "No CLI artifacts found for this release.",
)
MAX_DESKTOP_NOTE_CHARS = 4000
NOTES_CONFIG = {
    "desktop": {
        "path": "release/desktop-notes.json",
        "placeholder_en": "<!-- Replace with concise English release notes for Desktop users. -->",
        "placeholder_zh": "<!-- 替换为面向桌面端用户的简洁中文更新说明。 -->",
        "end_en": "All paired devices must use v2.",
        "end_zh": "所有配对设备必须使用 v2。",
    },
    "mobile": {
        "path": "release/android-notes.json",
        "placeholder_en": "<!-- Replace with concise English release notes for Android/Mobile users. -->",
        "placeholder_zh": "<!-- 替换为面向移动端用户的简洁中文更新说明。 -->",
        "end_en": "All paired devices must use v2/v3.",
        "end_zh": "所有配对设备需使用 v2/v3。",
    },
    "android": {
        "path": "release/android-notes.json",
        "placeholder_en": "<!-- Replace with concise English release notes for Android users. -->",
        "placeholder_zh": "<!-- 替换为面向 Android 用户的简洁中文更新说明。 -->",
        "end_en": "All paired devices must use v2/v3.",
        "end_zh": "所有配对设备需使用 v2/v3。",
    },
}


def parse_version(raw):
    value = raw.strip()
    if value.startswith("v"):
        value = value[1:]
    if not cli_release.SEMVER.fullmatch(value):
        raise ValueError(
            f"invalid canonical semver version {raw!r}; expected major.minor.patch"
        )
    return value


def semver_tuple(version):
    return tuple(int(part) for part in version.split("."))


def product_tag_match(tag, product):
    match = re.fullmatch(
        rf"{re.escape(product)}/v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))-build\.({BUILD_RE})",
        tag,
    )
    if not match:
        return None
    return match[1], int(match[2])


def stable_v1_tag_match(tag):
    match = re.fullmatch(
        r"v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))", tag
    )
    return match[1] if match else None


def run_git(args, cwd=ROOT):
    result = subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    )
    return result.stdout.strip()


def list_local_tags(cwd=ROOT):
    output = run_git(["tag", "-l"], cwd=cwd)
    return [line.strip() for line in output.splitlines() if line.strip()]


def fetch_github_releases(repo):
    output = cli_release.run(
        ["gh", "api", "--paginate", "--slurp", f"/repos/{repo}/releases?per_page=100"]
    )
    return [entry for page in json.loads(output) for entry in page]


def fetch_github_tags(repo, prefix):
    output = cli_release.run(
        [
            "gh",
            "api",
            "--paginate",
            "--slurp",
            f"/repos/{repo}/git/matching-refs/tags/{prefix}",
        ]
    )
    return [
        item["ref"].removeprefix("refs/tags/")
        for page in json.loads(output)
        for item in page
    ]


def fetch_npm_cli_builds(metadata_path, version):
    targets = cli_release.metadata(metadata_path, "cli")["targets"]
    names = ["@uniclipboard/cli", *[cli_release.package_name(t) for t in targets]]
    builds = []
    for name in names:
        data = cli_release.registry_get(NPM, name)
        for ident in data.get("versions", {}):
            matched = product_tag_match("cli/v" + ident, "cli")
            if matched and matched[0] == version:
                builds.append(matched[1])
    return builds


def load_inventory(args, platform, version, metadata_path):
    if args.offline_inventory:
        data = json.loads(Path(args.offline_inventory).read_text(encoding="utf-8"))
        return (
            list(data.get("tags", [])),
            list(data.get("releases", [])),
            list(data.get("npm_cli_builds", [])),
        )

    tags = list_local_tags(cwd=args.repo_root)
    releases = []
    npm_cli_builds = []
    if not args.local_only:
        releases = fetch_github_releases(args.repo)
        prefixes = {
            "desktop": [f"desktop/v{version}-build.", "joint-release/"],
            "cli": [f"cli/v{version}-build.", "joint-release/alpha/"],
            "mobile": ["android/v", "ios/v"],
            "android": ["android/v"],
            "ios": ["ios/v"],
        }[platform]
        tag_set = set(tags)
        for prefix in prefixes:
            tag_set.update(fetch_github_tags(args.repo, prefix))
        tags = sorted(tag_set)
        if platform == "cli":
            npm_cli_builds = fetch_npm_cli_builds(metadata_path, version)
    return tags, releases, npm_cli_builds


def reservation_builds(tags, version, product):
    prefix = f"joint-release/alpha/v{version}/"
    result = []
    for tag in tags:
        if not tag.startswith(prefix):
            continue
        match = re.fullmatch(
            rf"{re.escape(prefix)}[1-9][0-9]*-(0|{BUILD_RE})-({BUILD_RE})", tag
        )
        if not match:
            continue
        cli_build, desktop_build = int(match[1]), int(match[2])
        if product == "desktop":
            result.append(desktop_build)
        elif product == "cli" and cli_build > 0:
            result.append(cli_build)
    return result


def accepted_desktop_builds(tags, version):
    prefix = f"joint-release/accepted/v{version}-build."
    builds = []
    for tag in tags:
        if not tag.startswith(prefix):
            continue
        suffix = tag.removeprefix(prefix)
        if re.fullmatch(BUILD_RE, suffix):
            builds.append(int(suffix))
    return builds


def existing_product_builds(tags, releases, product, version=None):
    all_names = list(tags) + [r.get("tag_name", "") for r in releases]
    builds = []
    for name in all_names:
        matched = product_tag_match(name, product)
        if not matched:
            continue
        if version is None or matched[0] == version:
            builds.append(matched[1])
    return builds


def select_previous_tag(platform, channel, version, build_number, tags, releases, cwd):
    published_tags = {
        r["tag_name"]: r
        for r in releases
        if not r.get("draft", False) and r.get("tag_name")
    }
    products = (
        ("android", "ios") if platform == "mobile" else (platform,)
    )
    candidates = []
    seen = set()
    for tag in list(published_tags.keys()) + list(tags):
        if tag in seen:
            continue
        seen.add(tag)
        for product in products:
            matched = product_tag_match(tag, product)
            if not matched:
                continue
            rel = published_tags.get(tag)
            if rel is not None and rel.get("draft", False):
                continue
            if (
                platform == "desktop"
                and channel == "stable"
                and (rel is None or rel.get("prerelease", True))
            ):
                continue
            ver, bld = matched
            if (semver_tuple(ver), bld) < (semver_tuple(version), build_number):
                candidates.append((semver_tuple(ver), bld, tag, f"{ver}-build.{bld}"))

    if candidates:
        candidates.sort(key=lambda item: (item[0], item[1]))
        _, _, prev_tag, prev_ver = candidates[-1]
        return prev_tag, prev_ver

    if platform == "desktop" and channel == "stable":
        v1_candidates = []
        for tag in list(published_tags.keys()) + list(tags):
            v1_ver = stable_v1_tag_match(tag)
            if v1_ver and semver_tuple(v1_ver) < semver_tuple(version):
                v1_candidates.append((semver_tuple(v1_ver), tag, v1_ver))
        if v1_candidates:
            v1_candidates.sort(key=lambda item: item[0])
            _, prev_tag, prev_ver = v1_candidates[-1]
            return prev_tag, prev_ver

    root_commit = run_git(["rev-list", "--max-parents=0", "HEAD"], cwd=cwd).splitlines()[
        0
    ]
    return root_commit, "initial commit"


def resolve_version_and_build(platform, channel, requested_version, metadata_path, tags, releases, npm_cli_builds):
    data = tomllib.loads(Path(metadata_path).read_text(encoding="utf-8"))
    products = data["products"]

    if requested_version:
        version = parse_version(requested_version)
    else:
        default_product = "android" if platform == "mobile" else platform
        version = parse_version(products[default_product]["version"])

    needs_acceptance = False
    if platform == "desktop":
        record = products["desktop"]
        if channel == "stable":
            accepted = accepted_desktop_builds(tags, version)
            if accepted:
                build_number = max(accepted)
            else:
                needs_acceptance = True
                existing = existing_product_builds(tags, releases, "desktop", version)
                floor = record["build_number"] if record["version"] == version else 1
                build_number = max(existing, default=floor)
        else:
            existing = existing_product_builds(tags, releases, "desktop", version)
            reserved = reservation_builds(tags, version, "desktop")
            floor = record["build_number"] if record["version"] == version else 1
            build_number = max(
                max(existing, default=0) + 1,
                max(reserved, default=0) + 1,
                floor + (1 if record["version"] == version and not existing and not reserved else 0),
                1,
            )
    elif platform == "cli":
        record = products["cli"]
        existing = existing_product_builds(tags, releases, "cli", version)
        reserved = reservation_builds(tags, version, "cli")
        floor = record["build_number"] if record["version"] == version else 0
        highest = max(
            max(existing, default=0),
            max(reserved, default=0),
            max(npm_cli_builds, default=0),
            floor,
        )
        build_number = highest + 1
    elif platform == "mobile":
        android_rec = products["android"]
        ios_rec = products["ios"]
        existing = existing_product_builds(
            tags, releases, "android"
        ) + existing_product_builds(tags, releases, "ios")
        highest = max(
            android_rec["build_number"],
            ios_rec["build_number"],
            android_rec["legacy_build_floor"],
            ios_rec["legacy_build_floor"],
            max(existing, default=0),
        )
        build_number = highest + 1
    else:
        record = products[platform]
        existing = existing_product_builds(tags, releases, platform)
        highest = max(
            record["build_number"],
            record["legacy_build_floor"],
            max(existing, default=0),
        )
        build_number = highest + 1

    if not 1 <= build_number <= 999999999:
        raise ValueError(f"build number out of range: {build_number}")
    if platform in ("desktop", "cli") and build_number > 65535:
        raise ValueError(f"{platform} build number exceeds Windows limit 65535")

    return version, build_number, needs_acceptance


def changelog_paths(platform, version, build_number):
    base = Path("release") / "changelogs" / platform
    stem = f"v{version}-build.{build_number}"
    return base / f"{stem}.md", base / f"{stem}.zh.md"


def write_github_output(path, values):
    with Path(path).open("a", encoding="utf-8") as stream:
        for key, value in values.items():
            text = str(value)
            if "\n" in text:
                marker = f"__UNIC_OUTPUT_{key.upper()}__"
                while marker in text:
                    marker += "_"
                stream.write(f"{key}<<{marker}\n{text}\n{marker}\n")
            else:
                stream.write(f"{key}={text}\n")


def cmd_plan(args):
    metadata_path = Path(args.metadata)
    tags, releases, npm_cli_builds = load_inventory(
        args, args.platform, parse_version(args.version) if args.version else cli_release.metadata(metadata_path, "android" if args.platform == "mobile" else args.platform)["version"], metadata_path
    )
    version, build_number, needs_acceptance = resolve_version_and_build(
        args.platform,
        args.channel,
        args.version,
        metadata_path,
        tags,
        releases,
        npm_cli_builds,
    )
    prev_tag, prev_version = select_previous_tag(
        args.platform,
        args.channel,
        version,
        build_number,
        tags,
        releases,
        args.repo_root,
    )
    cl_en, cl_zh = changelog_paths(args.platform, version, build_number)
    identity = f"v{version}-build.{build_number}"
    branch = f"release/{args.platform}-{args.channel}-{identity}"

    plan = {
        "platform": args.platform,
        "channel": args.channel,
        "version": version,
        "build_number": build_number,
        "identity": identity,
        "branch": branch,
        "previous_tag": prev_tag,
        "previous_version": prev_version,
        "range_expression": f"{prev_tag}..HEAD",
        "changelog_en": cl_en.as_posix(),
        "changelog_zh": cl_zh.as_posix(),
        "desktop_notes": "release/desktop-notes.json"
        if args.platform == "desktop"
        else "",
        "android_notes": "release/android-notes.json"
        if args.platform in ("mobile", "android")
        else "",
        "needs_acceptance": "true" if needs_acceptance else "false",
    }

    if args.json_output:
        cli_release.save(args.json_output, plan)
    if args.github_output:
        write_github_output(args.github_output, plan)
    print(json.dumps(plan, indent=2, sort_keys=True))


def update_toml_section(text, product, version, build_number):
    header = f"[products.{product}]"
    lines = text.splitlines(keepends=False)
    start = None
    for idx, line in enumerate(lines):
        if line.strip() == header:
            start = idx
            break
    if start is None:
        raise ValueError(f"missing section {header}")

    end = len(lines)
    for idx in range(start + 1, len(lines)):
        stripped = lines[idx].strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            end = idx
            break

    found_version = False
    found_build = False
    for idx in range(start + 1, end):
        line = lines[idx]
        if re.match(r"^\s*version\s*=", line):
            lines[idx] = f'version = "{version}"'
            found_version = True
        elif build_number is not None and re.match(r"^\s*build_number\s*=", line):
            lines[idx] = f"build_number = {build_number}"
            found_build = True

    if not found_version:
        raise ValueError(f"missing version key in {header}")
    if build_number is not None and not found_build:
        raise ValueError(f"missing build_number key in {header}")

    return "\n".join(lines) + "\n"


def cmd_bump(args):
    version = parse_version(args.version)
    build_number = int(args.build_number)
    if not 1 <= build_number <= 999999999:
        raise ValueError("build_number must be in 1..999999999")

    metadata_path = Path(args.metadata)
    original = metadata_path.read_bytes().decode("utf-8").replace("\r\n", "\n")

    if args.platform == "desktop":
        updated = update_toml_section(
            original,
            "desktop",
            version,
            None if args.channel == "stable" else build_number,
        )
    elif args.platform == "cli":
        updated = update_toml_section(original, "cli", version, build_number)
    elif args.platform == "mobile":
        updated = update_toml_section(original, "android", version, build_number)
        updated = update_toml_section(updated, "ios", version, build_number)
    elif args.platform in ("android", "ios"):
        updated = update_toml_section(original, args.platform, version, build_number)
    else:
        raise ValueError(f"unsupported platform: {args.platform}")

    # Enforce strict LF bytes per release contract.
    metadata_path.write_bytes(updated.encode("utf-8"))

    # Re-validate all products after editing.
    for product in ("cli", "desktop"):
        cli_release.metadata(metadata_path, product)
    original_mobile_meta = mobile_release.METADATA
    try:
        mobile_release.METADATA = metadata_path
        for product in ("android", "ios"):
            mobile_release.mobile_metadata(product)
    finally:
        mobile_release.METADATA = original_mobile_meta


def english_skeleton(platform, version, build_number, release_date):
    return (
        f"## v{version}-build.{build_number} ({platform}) - {release_date}\n\n"
        "### Features\n\n"
        "<!-- Replace with user-facing additions or remove this section if not needed. -->\n\n"
        "### Fixes\n\n"
        "<!-- Replace with user-facing fixes or remove this section if not needed. -->\n"
    )


def chinese_skeleton(platform, version, build_number, release_date):
    return (
        f"## v{version}-build.{build_number} ({platform}) - {release_date}\n\n"
        "### Features\n\n"
        "<!-- 填写本次版本中用户能直接感受到的新增或改进；若没有可删除整个小节。 -->\n\n"
        "### Fixes\n\n"
        "<!-- 填写本次版本中修复的用户可感知问题；若没有可删除整个小节。 -->\n"
    )


def cmd_init_changelog(args):
    version = parse_version(args.version)
    build_number = int(args.build_number)
    release_date = args.date or dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d")
    root = Path(args.repo_root)
    en_rel, zh_rel = changelog_paths(args.platform, version, build_number)
    en_path = root / en_rel
    zh_path = root / zh_rel
    en_path.parent.mkdir(parents=True, exist_ok=True)
    if not en_path.exists():
        en_path.write_bytes(
            english_skeleton(args.platform, version, build_number, release_date).encode(
                "utf-8"
            )
        )
    if not zh_path.exists():
        zh_path.write_bytes(
            chinese_skeleton(args.platform, version, build_number, release_date).encode(
                "utf-8"
            )
        )
    notes_cfg = NOTES_CONFIG.get(args.platform)
    if notes_cfg:
        notes_path = root / notes_cfg["path"]
        notes_path.parent.mkdir(parents=True, exist_ok=True)
        notes_path.write_bytes(
            (
                json.dumps(
                    {
                        "en": notes_cfg["placeholder_en"],
                        "zh": notes_cfg["placeholder_zh"],
                    },
                    ensure_ascii=False,
                    indent=2,
                )
                + "\n"
            ).encode("utf-8")
        )


def assert_finalized_markdown(path, version, build_number, platform):
    if not path.is_file():
        raise ValueError(f"missing changelog file: {path}")
    content = path.read_text(encoding="utf-8")
    prefix = f"## v{version}-build.{build_number} ({platform}) - "
    if not content.startswith(prefix):
        raise ValueError(f"unexpected changelog heading in {path}; expected {prefix!r}")
    for marker in PLACEHOLDER_MARKERS:
        if marker in content:
            raise ValueError(
                f"changelog file {path} still contains unfinished placeholder {marker!r}"
            )
    body = content.split("\n", 1)[1].strip() if "\n" in content else ""
    if not body:
        raise ValueError(f"changelog file {path} has no content")


def assert_finalized_notes_json(path):
    if not path.is_file():
        raise ValueError(f"missing notes file: {path}")
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or set(data.keys()) != {"en", "zh"}:
        raise ValueError(f"{path} must contain exact keys 'en' and 'zh'")
    for lang in ("en", "zh"):
        text = data[lang]
        if not isinstance(text, str) or not text.strip():
            raise ValueError(f"{path} '{lang}' must be a non-empty string")
        if len(text) > MAX_DESKTOP_NOTE_CHARS:
            raise ValueError(
                f"{path} '{lang}' exceeds {MAX_DESKTOP_NOTE_CHARS} characters"
            )
        for marker in PLACEHOLDER_MARKERS:
            if marker in text:
                raise ValueError(
                    f"{path} '{lang}' contains forbidden marker {marker!r}"
                )


def assert_finalized_desktop_notes(path):
    assert_finalized_notes_json(path)


def cmd_validate_changelog(args):
    version = parse_version(args.version)
    build_number = int(args.build_number)
    root = Path(args.repo_root)
    en_rel, zh_rel = changelog_paths(args.platform, version, build_number)
    assert_finalized_markdown(root / en_rel, version, build_number, args.platform)
    assert_finalized_markdown(root / zh_rel, version, build_number, args.platform)
    notes_cfg = NOTES_CONFIG.get(args.platform)
    if notes_cfg:
        assert_finalized_notes_json(root / notes_cfg["path"])


def cmd_render_prompt(args):
    version = parse_version(args.version)
    build_number = int(args.build_number)
    template = Path(args.template).read_text(encoding="utf-8")
    en_rel, zh_rel = changelog_paths(args.platform, version, build_number)
    release_date = args.date or dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d")
    scope_hints = {
        "desktop": "Desktop app (macOS/Windows/Linux GUI, Quick Panel, tray, updater, sync UX, and bundled CLI/service behavior noticeable to desktop users).",
        "cli": "Standalone CLI and headless daemon (`unic`, `unicd`, `uniclip`, CLI flags, migration, npm package behavior).",
        "mobile": "Mobile apps for Android and iOS (`app/expo/`, mobile sync, Share/Keyboard extensions, mobile migration).",
        "android": "Android app (`app/expo/` Android behavior, mobile sync, Android migration and sharing).",
        "ios": "iOS app (`app/expo/` iOS behavior, Share Extension, Keyboard Extension, mobile sync, iOS migration).",
    }
    notes_cfg = NOTES_CONFIG.get(args.platform)
    replacements = {
        "{{PLATFORM}}": args.platform,
        "{{PLATFORM_SCOPE}}": scope_hints[args.platform],
        "{{CHANNEL}}": args.channel,
        "{{IS_PRERELEASE}}": "false" if args.channel == "stable" else "true",
        "{{VERSION}}": version,
        "{{BUILD_NUMBER}}": str(build_number),
        "{{IDENTITY}}": f"v{version}-build.{build_number}",
        "{{PREVIOUS_TAG}}": args.previous_tag,
        "{{PREVIOUS_VERSION}}": args.previous_version,
        "{{RANGE_EXPRESSION}}": args.range_expression,
        "{{RELEASE_DATE}}": release_date,
        "{{CHANGELOG_EN}}": en_rel.as_posix(),
        "{{CHANGELOG_ZH}}": zh_rel.as_posix(),
        "{{IS_DESKTOP}}": "true" if args.platform == "desktop" else "false",
        "{{DESKTOP_NOTES_PATH}}": "release/desktop-notes.json"
        if args.platform == "desktop"
        else "(not applicable for this platform)",
        "{{HAS_NOTES_JSON}}": "true" if notes_cfg else "false",
        "{{NOTES_JSON_PATH}}": notes_cfg["path"]
        if notes_cfg
        else "(not applicable for this platform)",
        "{{NOTES_END_EN}}": notes_cfg["end_en"] if notes_cfg else "",
        "{{NOTES_END_ZH}}": notes_cfg["end_zh"] if notes_cfg else "",
    }
    rendered = template
    for token, value in replacements.items():
        rendered = rendered.replace(token, value)

    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    if args.github_output:
        write_github_output(args.github_output, {"prompt": rendered})
    if not args.output and not args.github_output:
        sys.stdout.write(rendered)


def parse_release_branch(branch):
    match = BRANCH_RE.fullmatch(branch.strip())
    if not match:
        raise ValueError(
            f"invalid release branch {branch!r}; expected release/<platform>-<channel>-v<version>-build.<number>"
        )
    return {
        "platform": match.group("platform"),
        "channel": match.group("channel"),
        "version": match.group("version"),
        "build_number": int(match.group("build")),
        "identity": f"v{match.group('version')}-build.{match.group('build')}",
    }


def dispatch_plan(platform, channel):
    track = "production" if channel == "stable" else "internal"
    if platform == "desktop":
        return [{"workflow": "release-cli.yml", "mode": "joint", "channel": channel}]
    if platform == "cli":
        return [
            {"workflow": "release-cli.yml", "mode": "standalone", "channel": channel}
        ]
    if platform == "mobile":
        return [
            {"workflow": "release-android.yml", "channel": channel, "publish": True, "play_track": track},
            {"workflow": "release-ios.yml", "publish": True},
        ]
    if platform == "android":
        return [{"workflow": "release-android.yml", "channel": channel, "publish": True, "play_track": track}]
    if platform == "ios":
        return [{"workflow": "release-ios.yml", "publish": True}]
    raise ValueError(f"unsupported platform: {platform}")


def cmd_parse_branch(args):
    info = parse_release_branch(args.branch)
    root = Path(args.repo_root)
    if args.validate_repo:
        metadata_path = Path(args.metadata) if args.metadata else root / "release/products.toml"
        data = tomllib.loads(metadata_path.read_text(encoding="utf-8"))
        products = data["products"]
        platform = info["platform"]
        version = info["version"]
        build_number = info["build_number"]

        if platform == "desktop":
            if products["desktop"]["version"] != version:
                raise ValueError(
                    f"products.desktop.version ({products['desktop']['version']}) != branch version ({version})"
                )
            if (
                info["channel"] != "stable"
                and products["desktop"]["build_number"] != build_number
            ):
                raise ValueError(
                    f"products.desktop.build_number ({products['desktop']['build_number']}) != branch build ({build_number})"
                )
        elif platform == "cli":
            if (products["cli"]["version"], products["cli"]["build_number"]) != (
                version,
                build_number,
            ):
                raise ValueError("products.cli identity does not match release branch")
        elif platform == "mobile":
            for mobile_product in ("android", "ios"):
                if (
                    products[mobile_product]["version"],
                    products[mobile_product]["build_number"],
                ) != (version, build_number):
                    raise ValueError(
                        f"products.{mobile_product} identity does not match release branch"
                    )
        else:
            if (products[platform]["version"], products[platform]["build_number"]) != (
                version,
                build_number,
            ):
                raise ValueError(
                    f"products.{platform} identity does not match release branch"
                )

        en_rel, zh_rel = changelog_paths(platform, version, build_number)
        assert_finalized_markdown(root / en_rel, version, build_number, platform)
        assert_finalized_markdown(root / zh_rel, version, build_number, platform)
        notes_cfg = NOTES_CONFIG.get(platform)
        if notes_cfg:
            assert_finalized_notes_json(root / notes_cfg["path"])

    dispatches = dispatch_plan(info["platform"], info["channel"])
    track = "production" if info["channel"] == "stable" else "internal"
    result = {
        **info,
        "android_track": track,
        "workflows": ",".join(item["workflow"] for item in dispatches),
    }
    if args.github_output:
        write_github_output(args.github_output, result)
    print(json.dumps({**result, "dispatches": dispatches}, indent=2, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("plan")
    p.add_argument("--platform", choices=PLATFORMS, required=True)
    p.add_argument("--channel", choices=CHANNELS, required=True)
    p.add_argument("--version", default="")
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    p.add_argument("--repo", default=REPO)
    p.add_argument("--repo-root", default=str(ROOT))
    p.add_argument("--local-only", action="store_true")
    p.add_argument("--offline-inventory", default="")
    p.add_argument("--github-output", default="")
    p.add_argument("--json-output", default="")

    p = sub.add_parser("bump")
    p.add_argument("--platform", choices=PLATFORMS, required=True)
    p.add_argument("--channel", choices=CHANNELS, default="alpha")
    p.add_argument("--version", required=True)
    p.add_argument("--build-number", required=True)
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))

    p = sub.add_parser("init-changelog")
    p.add_argument("--platform", choices=PLATFORMS, required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--build-number", required=True)
    p.add_argument("--date", default="")
    p.add_argument("--repo-root", default=str(ROOT))

    p = sub.add_parser("validate-changelog")
    p.add_argument("--platform", choices=PLATFORMS, required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--build-number", required=True)
    p.add_argument("--repo-root", default=str(ROOT))

    p = sub.add_parser("render-prompt")
    p.add_argument("--template", default=str(ROOT / ".github/prompts/prepare-release-changelog.codex.md"))
    p.add_argument("--platform", choices=PLATFORMS, required=True)
    p.add_argument("--channel", choices=CHANNELS, required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--build-number", required=True)
    p.add_argument("--previous-tag", required=True)
    p.add_argument("--previous-version", required=True)
    p.add_argument("--range-expression", required=True)
    p.add_argument("--date", default="")
    p.add_argument("--output", default="")
    p.add_argument("--github-output", default="")

    p = sub.add_parser("parse-branch")
    p.add_argument("--branch", required=True)
    p.add_argument("--validate-repo", action="store_true")
    p.add_argument("--metadata", default="")
    p.add_argument("--repo-root", default=str(ROOT))
    p.add_argument("--github-output", default="")

    args = parser.parse_args()
    handlers = {
        "plan": cmd_plan,
        "bump": cmd_bump,
        "init-changelog": cmd_init_changelog,
        "validate-changelog": cmd_validate_changelog,
        "render-prompt": cmd_render_prompt,
        "parse-branch": cmd_parse_branch,
    }
    handlers[args.command](args)


if __name__ == "__main__":
    main()
