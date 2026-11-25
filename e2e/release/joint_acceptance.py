#!/usr/bin/env python3
"""Exercise joint release commands with disposable GitHub, npm and origin state."""

import argparse
import base64
from fnmatch import fnmatchcase
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import tomllib
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
VERSION = "2.0.0"
TARGETS = tomllib.loads((ROOT / "release/products.toml").read_text())["products"][
    "desktop"
]["targets"]
FORMATS = {
    "darwin": ["dmg", "app.tar.gz"],
    "linux": [
        "deb",
        "rpm",
        "archlinux",
        "AppImage",
        "AppImage.tar.gz",
        "update.tar.gz",
    ],
    "windows": ["setup.exe", "portable.zip", "update.tar.gz"],
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, sort_keys=True) + "\n")


def github_fixture(arguments):
    state_path = Path(os.environ["JOINT_GITHUB_FIXTURE"])
    state = json.loads(state_path.read_text())

    def missing():
        print("HTTP 404: fixture resource missing", file=sys.stderr)
        raise SystemExit(1)

    if arguments[:2] == ["release", "download"]:
        tag = arguments[2]
        record = state["releases"].get(tag)
        if record is None:
            missing()
        destination = Path(arguments[arguments.index("--dir") + 1])
        patterns = [
            arguments[i + 1] for i, arg in enumerate(arguments) if arg == "--pattern"
        ]
        for file in Path(record["directory"]).iterdir():
            if any(fnmatchcase(file.name, pattern) for pattern in patterns):
                shutil.copyfile(file, destination / file.name)
        return
    assert arguments[0] == "api", arguments
    _, _, endpoint = unquote(
        urlsplit(arguments[1] if arguments[1] != "--paginate" else arguments[-1]).path
    ).partition("/repos/UniClipboard/UniClipboard/")
    if "--input" in arguments:
        body = json.loads(Path(arguments[arguments.index("--input") + 1]).read_text())
        if endpoint == "git/tags":
            sha = digest(json.dumps(body).encode())[:40]
            state["annotations"][sha] = {
                "tag": body["tag"],
                "message": body["message"],
                "object": {"type": body["type"], "sha": body["object"]},
            }
            result = {"sha": sha}
        else:
            assert endpoint == "git/refs", endpoint
            tag = body["ref"].removeprefix("refs/tags/")
            assert tag not in state["refs"]
            state["refs"][tag] = {"type": "tag", "sha": body["sha"]}
            result = state["refs"][tag]
        write_json(state_path, state)
    elif endpoint.startswith("git/matching-refs/tags/"):
        prefix = endpoint.removeprefix("git/matching-refs/tags/")
        result = [
            [
                {"ref": "refs/tags/" + name}
                for name in state["refs"]
                if name.startswith(prefix)
            ]
        ]
    elif endpoint.startswith("git/ref/tags/"):
        tag = endpoint.removeprefix("git/ref/tags/")
        if tag not in state["refs"]:
            missing()
        result = {"object": state["refs"][tag]}
    elif endpoint.startswith("git/tags/"):
        result = state["annotations"][endpoint.rsplit("/", 1)[1]]
    elif endpoint.startswith("releases/tags/"):
        tag = endpoint.removeprefix("releases/tags/")
        if tag not in state["releases"]:
            missing()
        result = state["releases"][tag]
    elif endpoint == "contents/.github/release-notes/desktop.md.tmpl":
        result = {
            "encoding": "base64",
            "content": base64.b64encode(b"Original Alpha page template\n").decode(),
        }
    else:
        assert endpoint == "releases", endpoint
        result = [list(state["releases"].values())]
    print(json.dumps(result))


def acceptance(work, report):
    source = report["source_sha"]
    state_path = work / "github.json"
    write_json(state_path, {"refs": {}, "annotations": {}, "releases": {}})
    npm, origin = {}, {}
    output = work / "outputs"
    output.mkdir()
    gh = work / "gh"
    gh.write_text(
        "#!/bin/sh\nexec "
        + shlex.join(
            [sys.executable, str(Path(__file__).resolve()), "--github-fixture"]
        )
        + ' "$@"\n'
    )
    gh.chmod(0o755)

    class Services(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            path = unquote(urlsplit(self.path).path)
            value = npm.get(path[1:], origin.get(path))
            data = json.dumps(value).encode()
            self.send_response(200 if value is not None else 404)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    with ThreadingHTTPServer(("127.0.0.1", 0), Services) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_port}"
        launcher = (
            "import sys; sys.dont_write_bytecode = True; "
            "sys.path.insert(0, 'tools/release'); import joint_plan as p; "
            f"p.PUBLIC = p.NPM = {base!r}; p.main()"
        )
        env = dict(os.environ, JOINT_GITHUB_FIXTURE=str(state_path))
        env["PATH"] = str(work) + os.pathsep + env["PATH"]
        env.pop("GH_TOKEN", None)
        env.pop("GITHUB_TOKEN", None)

        def invoke(label, *args, error=None):
            result = subprocess.run(
                [sys.executable, "-c", launcher, *map(str, args)],
                cwd=ROOT,
                env=env,
                text=True,
                capture_output=True,
                timeout=90,
            )
            report["checks"].append(
                {
                    "name": label,
                    "arguments": [
                        str(arg).replace(str(work), "<fixture>") for arg in args
                    ],
                    "exit_code": result.returncode,
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                }
            )
            if error is None:
                assert result.returncode == 0, result.stderr
            else:
                assert result.returncode != 0 and error in result.stderr, result.stderr
            print("PASS " + label, flush=True)

        def plan(label, run_id, channel="alpha", version=VERSION, error=None):
            path = output / str(len(report["checks"]))
            invoke(
                label,
                "plan",
                "--channel",
                channel,
                "--version",
                version,
                "--run-id",
                run_id,
                "--source",
                source,
                "--output",
                path,
                "--github-output",
                work / "outputs.txt",
                error=error,
            )
            return path

        def planned(path):
            return json.loads((path / "plan.json").read_text())

        def save_release(tag, directory, draft=False):
            state = json.loads(state_path.read_text())
            if not draft:
                state["refs"][tag] = {"type": "commit", "sha": source}
            state["releases"][tag] = {
                "tag_name": tag,
                "draft": draft,
                "directory": str(directory),
                "assets": [
                    {"name": p.name, "digest": "sha256:" + digest(p.read_bytes())}
                    for p in directory.iterdir()
                ],
            }
            write_json(state_path, state)

        shared = {}
        for dirname in [
            "internal/localconfig",
            "internal/localapi",
            "internal/servicelifecycle",
            "internal/servicelaunch",
            "app/service/internal/server",
        ]:
            for file in (ROOT / dirname).glob("*.go"):
                shared[file.relative_to(ROOT).as_posix()] = digest(file.read_bytes())

        def publish_cli(build, incompatible=False, draft=False):
            directory = work / f"cli-{build}"
            directory.mkdir()
            version = f"{VERSION}-build.{build}"
            items = []
            native = {}
            for target in TARGETS:
                name = "@uniclipboard/cli-" + target.replace(
                    "windows", "win32"
                ).replace("amd64", "x64")
                hashes = dict(shared)
                if incompatible and target == "windows-arm64":
                    hashes["internal/servicelaunch/process_windows.go"] = "0" * 64
                native[target] = {
                    "acceptance": dict.fromkeys(
                        ["native", "version_smoke", "alias_smoke"], True
                    ),
                    "source_sha256": hashes,
                    "signed": target.startswith("darwin-"),
                }
                items.append(
                    {"name": name, "version": version, "integrity": "fixture-" + name}
                )
            items.append(
                {
                    "name": "@uniclipboard/cli",
                    "version": version,
                    "integrity": "fixture-main",
                }
            )
            manifest = directory / "release-manifest.json"
            write_json(
                manifest,
                {
                    "schema": 1,
                    "product": "cli",
                    "version": VERSION,
                    "build_number": build,
                    "npm_version": version,
                    "source_clean": True,
                    "source_sha": source,
                    "targets": TARGETS,
                    "target_manifests": native,
                    "packages": items,
                },
            )
            if not draft:
                for item in items:
                    npm.setdefault(item["name"], {"versions": {}})["versions"][
                        version
                    ] = {
                        "dist": {"integrity": item["integrity"]},
                    }
            save_release(f"cli/v{version}", directory, draft)
            return manifest

        def publish_desktop(build, cli_manifest, recorded=True):
            directory = work / f"desktop-{build}"
            directory.mkdir()
            files = []
            for target in TARGETS:
                platform = target.split("-")[0]
                for format_name in FORMATS[platform]:
                    file = directory / (target + "." + format_name)
                    file.write_text(f"fixture {build} {file.name}")
                    files.append(
                        {
                            "file": file.name,
                            "sha256": digest(file.read_bytes()),
                            "target": target,
                            "updater": format_name
                            == (
                                "app.tar.gz"
                                if platform == "darwin"
                                else "update.tar.gz"
                            ),
                        }
                    )
            manifest = directory / "release-manifest.json"
            write_json(
                manifest,
                {
                    "schema": 1,
                    "product": "desktop",
                    "version": VERSION,
                    "build_number": build,
                    "source_sha": source,
                    "source_clean": True,
                    "development": False,
                    "cli_manifest_sha256": digest(cli_manifest.read_bytes()),
                    "artifacts": files,
                },
            )
            manifest_sha = digest(manifest.read_bytes())
            origin[f"/artifacts/desktop/{VERSION}/build.{build}/publication.json"] = {
                "status": "available",
                "product": "desktop",
                "version": VERSION,
                "build_number": build,
                "manifest_sha256": manifest_sha,
                "fixture": False,
            }
            origin["/v1/desktop/alpha.json"] = {
                "product": "desktop",
                "version": VERSION,
                "buildNumber": str(build),
                "artifacts": [dict(file, platform=file["target"]) for file in files],
            }
            notes = directory / "desktop-notes.json"
            write_json(
                notes, {"en": f"Notes from Alpha {build}", "zh": f"Alpha {build}"}
            )
            (directory / "release-page.json").touch()
            (directory / "SHA256SUMS.txt").touch()
            save_release(f"desktop/v{VERSION}-build.{build}", directory)

            evidence = work / f"acceptance-{build}.json"
            write_json(
                evidence,
                {
                    platform: {
                        "manifest_sha256": manifest_sha,
                        "evidence": "https://example.com/native/" + platform,
                        "artifacts": [
                            {
                                **file,
                                "native_gui": "passed",
                                "package_lifecycle": "passed",
                            }
                            for file in files
                            if file["target"].startswith(platform + "-")
                        ],
                    }
                    for platform in FORMATS
                },
            )
            if not recorded:
                return evidence
            promotion = work / f"promotion-{build}.json"
            write_json(
                promotion,
                {
                    "status": "promoted",
                    "channel": "desktop/alpha",
                    "fixture": False,
                    "manifest_sha256": manifest_sha,
                },
            )
            invoke(
                f"mark Alpha build {build}",
                "mark-alpha",
                "--manifest",
                manifest,
                "--cli-manifest",
                cli_manifest,
                "--promotion",
                promotion,
            )
            return evidence

        first = plan(
            "new version ignores configured product version", 100, version="3.0.0"
        )
        assert planned(first)["cli_build"] == planned(first)["desktop_build"] == 1
        generated = tomllib.loads((first / "products.toml").read_text())
        assert all(
            product["version"] == "3.0.0" for product in generated["products"].values()
        )

        old_cli = publish_cli(10)
        publish_cli(13, draft=True)
        evidence_20 = publish_desktop(20, old_cli, recorded=False)
        first = plan("reuse compatible CLI and allocate the next Desktop identity", 200)
        assert (planned(first)["cli_build"], planned(first)["desktop_build"]) == (
            10,
            21,
        )
        assert planned(first)["reused_cli_sha256"] == digest(old_cli.read_bytes())

        publish_cli(11, incompatible=True)
        second = plan(
            "rebuild on Windows ARM64 shared source mismatch and skip occupied numbers",
            201,
        )
        assert (planned(second)["cli_build"], planned(second)["desktop_build"]) == (
            14,
            22,
        )
        assert planned(second)["reused_cli_sha256"] == ""
        rerun = plan("retry reuses its original source and identities", 200)
        assert (rerun / "plan.json").read_bytes() == (first / "plan.json").read_bytes()
        third = plan("new run cannot reuse an unmaterialized reservation", 202)
        assert (planned(third)["cli_build"], planned(third)["desktop_build"]) == (
            15,
            23,
        )
        npm["@uniclipboard/cli-win32-arm64"]["versions"][f"{VERSION}-build.29"] = {}
        partial = plan("an orphaned npm platform version also reserves its build", 203)
        assert planned(partial)["cli_build"] == 30

        plan(
            "Stable fails before mutation when no accepted Alpha exists",
            300,
            "stable",
            error="no accepted Alpha candidate",
        )
        invalid = work / "invalid-acceptance.json"
        receipt = json.loads(evidence_20.read_text())
        receipt["windows"]["artifacts"][0]["sha256"] = "0" * 64
        write_json(invalid, receipt)
        invoke(
            "reject mismatched native acceptance",
            "accept",
            "--build",
            f"{VERSION}-build.20",
            "--acceptance",
            invalid,
            error="invalid stable acceptance artifact",
        )
        invoke(
            "record three-platform acceptance for an Alpha already on the public feed",
            "accept",
            "--build",
            f"{VERSION}-build.20",
            "--acceptance",
            evidence_20,
        )
        evidence_24 = publish_desktop(24, old_cli)
        selected = plan(
            "Stable selects accepted Alpha and ignores newer unaccepted Alpha",
            301,
            "stable",
        )
        assert planned(selected)["accepted"]["build_number"] == 20

        invoke(
            "record acceptance for the newer Alpha",
            "accept",
            "--build",
            f"{VERSION}-build.24",
            "--acceptance",
            evidence_24,
        )
        retry = plan(
            "Stable retry keeps the original Alpha after new acceptance", 301, "stable"
        )
        assert (retry / "plan.json").read_bytes() == (
            selected / "plan.json"
        ).read_bytes()
        latest = plan("new Stable run selects newest accepted Alpha", 302, "stable")
        assert planned(latest)["accepted"]["build_number"] == 24
        destination = work / "stable-assets"
        invoke(
            "Stable fetches the original catalogue and notes",
            "fetch-stable",
            "--plan",
            latest / "plan.json",
            "--output",
            destination,
            "--cli-manifest",
            latest / "cli-manifest.json",
            "--acceptance",
            latest / "stable-acceptance.json",
        )
        assert (
            json.loads((destination / "desktop-notes.json").read_text())["en"]
            == "Notes from Alpha 24"
        )
        assert (
            destination / "release-template.md"
        ).read_text() == "Original Alpha page template\n"

        origin["/v1/desktop/stable.json"] = {"version": VERSION, "buildNumber": "25"}
        plan(
            "Stable rejects a Desktop downgrade before npm promotion",
            303,
            "stable",
            error="joint release cannot downgrade the Desktop channel",
        )
        server.shutdown()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, default=ROOT / ".cache/joint-release-e2e"
    )
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    report = {
        "status": "running",
        "checks": [],
        "scope": "Release command processes with disposable GitHub and loopback services",
        "source_sha": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
    }
    try:
        with tempfile.TemporaryDirectory(prefix="unic-joint-e2e-") as temporary:
            acceptance(Path(temporary), report)
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = str(error)
        raise
    finally:
        receipt = args.output / "report.json"
        write_json(receipt, report)
        (args.output / "SHA256SUMS").write_text(
            digest(receipt.read_bytes()) + "  report.json\n"
        )
        print(receipt, flush=True)


if __name__ == "__main__":
    if sys.argv[1:2] == ["--github-fixture"]:
        github_fixture(sys.argv[2:])
    else:
        main()
