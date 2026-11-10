#!/usr/bin/env python3
"""Build-set validation, npm packaging and immutable CLI publication."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
from urllib.error import HTTPError
from urllib.parse import quote, urlparse
from urllib.request import urlopen
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
NPM_BUILD = re.compile(r"(\d+)\.(\d+)\.(\d+)-build\.([1-9][0-9]*)\Z")
SHA256 = re.compile(r"[a-f0-9]{64}\Z")
MAX_FILE = 512 * 1024 * 1024
NODE_OS = {"darwin": "darwin", "linux": "linux", "windows": "win32"}
NODE_ARCH = {"amd64": "x64", "arm64": "arm64"}


def digest(path, algorithm="sha256"):
    h = hashlib.new(algorithm)
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def read_json(path):
    return json.loads(Path(path).read_text())


def save(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def run(args, **kwargs):
    result = subprocess.run(
        args, cwd=ROOT, check=True, text=True, stdout=subprocess.PIPE, **kwargs
    )
    return result.stdout.strip()


def metadata(path, product="cli"):
    path = Path(path)
    data = tomllib.loads(path.read_text())
    record = data["products"][product]
    if data["schema"] != 1 or not SEMVER.fullmatch(record["version"]):
        raise ValueError("invalid product version")
    number = record["build_number"]
    if type(number) is not int or not 1 <= number <= 999999999:
        raise ValueError("invalid product build number")
    if len(record["targets"]) != len(set(record["targets"])):
        raise ValueError("duplicate target")
    return record


def identity(record):
    return f"{record['version']}-build.{record['build_number']}"


def package_name(target):
    operating_system, arch = target.split("-")
    return f"@uniclipboard/cli-{NODE_OS[operating_system]}-{NODE_ARCH[arch]}"


def checked_file(directory, name, expected):
    if Path(name).name != name or not SHA256.fullmatch(expected):
        raise ValueError("invalid asset name or SHA-256")
    path = Path(directory) / name
    if path.is_symlink() or not path.is_file() or digest(path) != expected:
        raise ValueError(f"asset digest mismatch: {name}")
    return path


def pack(directory, output):
    packed = json.loads(
        run(
            [
                "npm",
                "pack",
                str(directory),
                "--ignore-scripts",
                "--json",
                "--pack-destination",
                str(output),
            ]
        )
    )[0]
    archive = Path(output) / packed["filename"]
    integrity = (
        "sha512-"
        + base64.b64encode(hashlib.sha512(archive.read_bytes()).digest()).decode()
    )
    if integrity != packed["integrity"]:
        raise ValueError("npm pack integrity mismatch")
    return {
        "name": packed["name"],
        "version": packed["version"],
        "file": archive.name,
        "integrity": integrity,
        "sha256": digest(archive),
    }


def assemble(args):
    record = metadata(args.metadata)
    version = identity(record)
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    inputs = Path(args.input)
    candidates = [read_json(p) for p in sorted(inputs.rglob("manifest-*.json"))]
    selected = {}
    for item in candidates:
        target = item["os"] + "-" + item["arch"]
        if target in selected:
            raise ValueError("duplicate target candidate: " + target)
        if (
            item.get("product"),
            item.get("product_version"),
            item.get("build_number"),
        ) != ("cli", record["version"], record["build_number"]):
            raise ValueError("candidate product/build identity mismatch")
        if item.get("metadata_sha256") != digest(args.metadata) or not item.get(
            "source_unchanged"
        ):
            raise ValueError("candidate source or metadata mismatch")
        if not all(
            item.get("acceptance", {}).get(key) is True
            for key in ("version_smoke", "alias_smoke", "native")
        ):
            raise ValueError("candidate lacks native version smoke receipt")
        selected[target] = item
    if set(selected) != set(record["targets"]) or not selected:
        raise ValueError("candidate target set is incomplete or unexpected")
    heads = {item["source_head"] for item in selected.values()}
    if len(heads) != 1:
        raise ValueError("candidate source SHAs disagree")
    packages = []
    with tempfile.TemporaryDirectory(prefix="unic-npm-") as work:
        work = Path(work)
        for target, item in sorted(selected.items()):
            operating_system, arch = target.split("-")
            suffix = ".exe" if operating_system == "windows" else ""
            archives = item["archives_sha256"]
            if len(archives) != 1:
                raise ValueError("CLI target must contain exactly one archive")
            archive_name, sha = next(iter(archives.items()))
            matches = [p for p in inputs.rglob(archive_name) if p.is_file()]
            if len(matches) != 1:
                raise ValueError("missing or ambiguous target archive")
            archive = checked_file(matches[0].parent, archive_name, sha)
            directory = work / target
            (directory / "bin").mkdir(parents=True)
            binaries = {}
            if operating_system == "windows":
                container = zipfile.ZipFile(archive)
                entries = {
                    name: container.getinfo(name) for name in container.namelist()
                }
                reader = container.open

                def size(entry):
                    return entry.file_size

                def regular(entry):
                    return (
                        not entry.is_dir()
                        and (entry.external_attr >> 16) & 0o170000 != 0o120000
                    )
            else:
                container = tarfile.open(archive)
                entries = {entry.name: entry for entry in container.getmembers()}

                def reader(name):
                    return container.extractfile(entries[name])

                def size(entry):
                    return entry.size

                def regular(entry):
                    return entry.isfile()

            with container:
                for name in ["unic" + suffix, "unicd" + suffix]:
                    entry = entries.get(name)
                    if (
                        entry is None
                        or not regular(entry)
                        or not 0 < size(entry) <= MAX_FILE
                    ):
                        raise ValueError("invalid binary archive member: " + name)
                    path = directory / "bin" / name
                    with reader(name) as source, path.open("wb") as destination:
                        remaining = size(entry)
                        while remaining:
                            block = source.read(min(remaining, 1024 * 1024))
                            if not block:
                                raise ValueError("truncated binary")
                            destination.write(block)
                            remaining -= len(block)
                    path.chmod(0o755)
                    if digest(path) != item["binaries_sha256"][name]:
                        raise ValueError("binary digest mismatch")
                    binaries[name] = digest(path)
            name = package_name(target)
            save(
                directory / "package.json",
                {
                    "name": name,
                    "version": version,
                    "license": "AGPL-3.0-only",
                    "os": [NODE_OS[operating_system]],
                    "cpu": [NODE_ARCH[arch]],
                    "files": ["bin"],
                    "preferUnplugged": True,
                    "repository": {
                        "type": "git",
                        "url": "git+https://github.com/UniClipboard/UniClipboard.git",
                    },
                    "unicRelease": {
                        "version": record["version"],
                        "buildNumber": record["build_number"],
                        "sourceSha": next(iter(heads)),
                    },
                },
            )
            (directory / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
            package = pack(directory, output)
            package.update(target=target, binaries_sha256=binaries)
            packages.append(package)
        main = work / "main"
        main.mkdir()
        launcher = """#!/usr/bin/env node
const {spawnSync} = require('node:child_process');
const path = require('node:path');
const name = `@uniclipboard/cli-${process.platform}-${process.arch}`;
let directory;
try { directory = path.dirname(require.resolve(name + '/package.json')); }
catch { console.error('unic: platform package missing: ' + name); process.exit(1); }
const executable = path.join(directory, 'bin', process.platform === 'win32' ? 'unic.exe' : 'unic');
const result = spawnSync(executable, process.argv.slice(2), {stdio:'inherit'});
if (result.error) { console.error('unic: ' + result.error.message); process.exit(1); }
if (result.signal) process.kill(process.pid, result.signal);
else process.exit(result.status === null ? 1 : result.status);
"""
        (main / "launcher.cjs").write_text(launcher)
        (main / "launcher.cjs").chmod(0o755)
        save(
            main / "package.json",
            {
                "name": "@uniclipboard/cli",
                "version": version,
                "license": "AGPL-3.0-only",
                "bin": {"unic": "launcher.cjs", "uniclip": "launcher.cjs"},
                "files": ["launcher.cjs"],
                "engines": {"node": ">=22.22.2"},
                "optionalDependencies": {p["name"]: version for p in packages},
                "repository": {
                    "type": "git",
                    "url": "git+https://github.com/UniClipboard/UniClipboard.git",
                },
                "unicRelease": {
                    "version": record["version"],
                    "buildNumber": record["build_number"],
                    "sourceSha": next(iter(heads)),
                },
            },
        )
        (main / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
        packages.append(pack(main, output))
    manifest = {
        "schema": 1,
        "product": "cli",
        "version": record["version"],
        "build_number": record["build_number"],
        "npm_version": version,
        "source_sha": next(iter(heads)),
        "source_clean": all(not item["source_dirty"] for item in selected.values()),
        "metadata_sha256": digest(args.metadata),
        "targets": sorted(selected),
        "packages": packages,
        "target_manifests": selected,
    }
    save(output / "release-manifest.json", manifest)
    print(output / "release-manifest.json")


def registry_url(value):
    url = urlparse(value)
    if (
        url.scheme == "https"
        and url.netloc == "registry.npmjs.org"
        and url.path in ("", "/")
    ):
        return value.rstrip("/"), False
    if (
        url.scheme == "http"
        and url.hostname in ("127.0.0.1", "::1")
        and not url.username
        and not url.password
        and url.path in ("", "/")
    ):
        return value.rstrip("/"), True
    raise ValueError("registry must be npmjs.org or a loopback acceptance fixture")


def registry_get(registry, name):
    try:
        with urlopen(registry + "/" + quote(name, safe="@"), timeout=30) as response:
            data = response.read(16 * 1024 * 1024 + 1)
            if len(data) > 16 * 1024 * 1024:
                raise ValueError("registry response too large")
            return json.loads(data)
    except HTTPError as error:
        if error.code == 404:
            return {"versions": {}, "dist-tags": {}}
        raise


def validate_manifest(path):
    manifest = read_json(path)
    if (
        manifest.get("schema") != 1
        or manifest.get("product") != "cli"
        or not SEMVER.fullmatch(manifest["version"])
    ):
        raise ValueError("invalid CLI manifest")
    if (
        type(manifest["build_number"]) is not int
        or not 1 <= manifest["build_number"] <= 999999999
    ):
        raise ValueError("invalid build number")
    if manifest["npm_version"] != identity(manifest):
        raise ValueError("npm identity mismatch")
    expected = {package_name(target) for target in manifest["targets"]} | {
        "@uniclipboard/cli"
    }
    packages = manifest["packages"]
    if {p["name"] for p in packages} != expected or len(packages) != len(expected):
        raise ValueError("manifest package set mismatch")
    if packages[-1]["name"] != "@uniclipboard/cli" or any(
        p["version"] != manifest["npm_version"] for p in packages
    ):
        raise ValueError("invalid package publication order or version")
    return manifest


def version_order(version):
    match = NPM_BUILD.fullmatch(version)
    if match:
        return tuple(int(v) for v in match.groups())
    if SEMVER.fullmatch(version):
        return tuple(int(v) for v in version.split(".")) + (0,)
    raise ValueError("unrecognized existing channel version; requires explicit review")


def distribute(args):
    registry, fixture = registry_url(args.registry)
    manifest = validate_manifest(args.manifest)
    receipt = {
        "product": "cli",
        "version": manifest["version"],
        "build_number": manifest["build_number"],
        "manifest_sha256": digest(args.manifest),
        "channel": args.channel,
        "registry": registry,
        "fixture": fixture,
        "operation": args.command,
        "status": "running",
        "packages": [],
    }
    tag = "alpha" if args.channel == "alpha" else "latest"
    try:
        if not fixture and not manifest["source_clean"]:
            raise ValueError("dirty candidates cannot be publicly released")
        if not fixture:
            npm_version = tuple(int(x) for x in run(["npm", "--version"]).split("."))
            if npm_version < (11, 21, 0) or (
                npm_version[0] == 12 and npm_version < (12, 2, 0)
            ):
                raise ValueError("npm dist-tag OIDC requires npm >=11.21 or >=12.2")
            if (
                not os.environ.get("ACTIONS_ID_TOKEN_REQUEST_URL")
                or os.environ.get("GITHUB_ACTIONS") != "true"
            ):
                raise ValueError("public release requires GitHub Actions OIDC")
        existing = {}
        for package in manifest["packages"]:
            data = registry_get(registry, package["name"])
            current = data.get("dist-tags", {}).get(tag)
            if current and version_order(current) > version_order(package["version"]):
                raise ValueError("refusing channel downgrade: " + package["name"])
            remote = data.get("versions", {}).get(package["version"])
            if (
                remote
                and remote.get("dist", {}).get("integrity") != package["integrity"]
            ):
                raise ValueError(
                    "published package integrity conflict: " + package["name"]
                )
            if args.command == "promote" and not remote:
                raise ValueError("promotion package missing: " + package["name"])
            if args.command == "publish":
                archive = checked_file(
                    Path(args.manifest).parent, package["file"], package["sha256"]
                )
                actual = (
                    "sha512-"
                    + base64.b64encode(
                        bytes.fromhex(digest(archive, "sha512"))
                    ).decode()
                )
                if actual != package["integrity"]:
                    raise ValueError("local npm integrity mismatch: " + package["name"])
                with tarfile.open(archive) as packed:
                    entry = packed.getmember("package/package.json")
                    if not entry.isfile() or entry.size > 65536:
                        raise ValueError("invalid npm package metadata")
                    payload = json.load(packed.extractfile(entry))
                if (payload["name"], payload["version"]) != (
                    package["name"],
                    package["version"],
                ):
                    raise ValueError("packed npm identity mismatch")
                if package["name"] == "@uniclipboard/cli":
                    expected = {
                        p["name"]: p["version"] for p in manifest["packages"][:-1]
                    }
                    if payload.get("optionalDependencies") != expected:
                        raise ValueError("packed platform dependency set mismatch")
            existing[package["name"]] = remote
        # Complete preflight for the entire set before any mutation.
        for package in manifest["packages"]:
            item = {
                "name": package["name"],
                "version": package["version"],
                "integrity": package["integrity"],
                "status": "pending",
            }
            receipt["packages"].append(item)
            save(args.receipt, receipt)
            if not existing[package["name"]]:
                command = [
                    "npm",
                    "publish",
                    str(Path(args.manifest).resolve().parent / package["file"]),
                    "--registry",
                    registry,
                    "--access",
                    "public",
                    "--ignore-scripts",
                    "--tag",
                    tag,
                ]
                if not fixture:
                    command.append("--provenance")
                run(command)
            remote = (
                registry_get(registry, package["name"])
                .get("versions", {})
                .get(package["version"])
            )
            if (
                not remote
                or remote.get("dist", {}).get("integrity") != package["integrity"]
            ):
                raise ValueError("package read-back failed: " + package["name"])
            run(
                [
                    "npm",
                    "dist-tag",
                    "add",
                    package["name"] + "@" + package["version"],
                    tag,
                    "--registry",
                    registry,
                ]
            )
            if (
                registry_get(registry, package["name"]).get("dist-tags", {}).get(tag)
                != package["version"]
            ):
                raise ValueError("channel read-back failed: " + package["name"])
            item["status"] = "available"
            save(args.receipt, receipt)
        receipt["status"] = "available"
    except Exception as error:
        receipt["status"] = "failed"
        receipt["error"] = str(error)
        raise
    finally:
        save(args.receipt, receipt)


def smoke(args):
    directory = Path(args.directory)
    paths = list(directory.glob("manifest-*.json"))
    if len(paths) != 1:
        raise ValueError("expected one target manifest")
    manifest = read_json(paths[0])
    archive_name, sha = next(iter(manifest["archives_sha256"].items()))
    archive = checked_file(directory, archive_name, sha)
    suffix = ".exe" if manifest["os"] == "windows" else ""
    with tempfile.TemporaryDirectory(prefix="unic-smoke-") as temporary:
        dest = Path(temporary)
        if suffix:
            with zipfile.ZipFile(archive) as z:
                for name in ["unic.exe", "unicd.exe"]:
                    (dest / name).write_bytes(z.read(name))
                shim = z.read("uniclip.cmd")
                if (
                    shim
                    != b'@echo off\r\n"%~dp0unic.exe" %*\r\nexit /b %errorlevel%\r\n'
                ):
                    raise ValueError("invalid Windows alias")
                (dest / "uniclip.cmd").write_bytes(shim)
        else:
            with tarfile.open(archive) as t:
                for name in ["unic", "unicd"]:
                    entry = t.getmember(name)
                    if not entry.isfile():
                        raise ValueError("invalid smoke member")
                    (dest / name).write_bytes(t.extractfile(entry).read())
                    (dest / name).chmod(0o755)
                alias = t.getmember("uniclip")
                if not alias.issym() or alias.linkname != "unic":
                    raise ValueError("invalid Unix alias")
                (dest / "uniclip").symlink_to("unic")
        for name in ["unic", "unicd", "uniclip"]:
            executable = dest / (name + suffix if name != "uniclip" else name)
            if (
                name != "uniclip"
                and digest(executable) != manifest["binaries_sha256"][executable.name]
            ):
                raise ValueError("smoke binary digest mismatch")
            command = [str(executable), "--version", "--json"]
            if name == "uniclip" and suffix:
                command = [
                    "cmd",
                    "/d",
                    "/c",
                    str(dest / "uniclip.cmd"),
                    "--version",
                    "--json",
                ]
            value = json.loads(run(command))
            if (
                value["product"],
                value["version"],
                int(value["build_number"]),
                value["source_sha"],
            ) != (
                "cli",
                manifest["product_version"],
                manifest["build_number"],
                manifest["source_head"],
            ):
                raise ValueError("native binary identity mismatch")
    manifest["acceptance"] = {
        "version_smoke": True,
        "alias_smoke": True,
        "native": True,
    }
    save(paths[0], manifest)


def matrix(args):
    record = metadata(args.metadata, args.product)
    runners = {
        "darwin-arm64": "macos-15",
        "darwin-amd64": "macos-15-intel",
        "linux-arm64": "ubuntu-24.04-arm",
        "linux-amd64": "ubuntu-24.04",
        "windows-amd64": "windows-2025",
    }
    if not record["targets"]:
        raise ValueError("product has no implemented native targets")
    print(
        json.dumps(
            {
                "include": [
                    {
                        "target": target,
                        "runner": runners[target],
                        "goos": target.split("-")[0],
                        "goarch": target.split("-")[1],
                    }
                    for target in record["targets"]
                ]
            }
        )
    )


def github_record(args):
    manifest = validate_manifest(args.manifest)
    if not manifest["source_clean"]:
        raise ValueError("public build record requires clean source")
    tag = f"cli/v{manifest['version']}-build.{manifest['build_number']}"
    repository = args.repository
    if repository != "UniClipboard/UniClipboard":
        raise ValueError("unexpected release repository")
    endpoint = "repos/" + repository + "/releases/tags/" + quote(tag, safe="")
    result = subprocess.run(
        ["gh", "api", endpoint], cwd=ROOT, text=True, capture_output=True
    )
    if result.returncode:
        if "HTTP 404" not in result.stderr:
            raise ValueError("cannot query GitHub build record: " + result.stderr)
        if args.finish:
            raise ValueError("staged build record missing")
        ref_endpoint = "repos/" + repository + "/git/ref/tags/" + quote(tag, safe="")
        tag_result = subprocess.run(
            ["gh", "api", ref_endpoint], cwd=ROOT, text=True, capture_output=True
        )
        if tag_result.returncode:
            if "HTTP 404" not in tag_result.stderr:
                raise ValueError("cannot query build tag: " + tag_result.stderr)
            run(
                [
                    "gh",
                    "api",
                    "--method",
                    "POST",
                    "repos/" + repository + "/git/refs",
                    "-f",
                    "ref=refs/tags/" + tag,
                    "-f",
                    "sha=" + manifest["source_sha"],
                ]
            )
        else:
            obj = json.loads(tag_result.stdout)["object"]
            if obj["type"] != "commit" or obj["sha"] != manifest["source_sha"]:
                raise ValueError("existing build tag source conflict")
        run(
            [
                "gh",
                "release",
                "create",
                tag,
                "--repo",
                repository,
                "--target",
                manifest["source_sha"],
                "--draft",
                "--verify-tag",
                "--prerelease",
                "--title",
                tag,
                "--notes",
                "Immutable CLI build. npm dist-tags own channel availability.",
            ]
        )
    # Check the actual Git tag, not GitHub Release target_commitish text.
    ref = json.loads(
        run(
            [
                "gh",
                "api",
                "repos/" + repository + "/git/ref/tags/" + quote(tag, safe=""),
            ]
        )
    )["object"]
    if ref["type"] != "commit" or ref["sha"] != manifest["source_sha"]:
        raise ValueError("existing build tag source conflict")
    directory = Path(args.manifest).resolve().parent
    names = ["release-manifest.json"] + [p["file"] for p in manifest["packages"]]
    existing = json.loads(
        run(["gh", "release", "view", tag, "--repo", repository, "--json", "assets"])
    )
    assets = {asset["name"] for asset in existing["assets"]}
    for name in names:
        path = directory / name
        if name in assets:
            with tempfile.TemporaryDirectory(prefix="unic-record-") as temporary:
                run(
                    [
                        "gh",
                        "release",
                        "download",
                        tag,
                        "--repo",
                        repository,
                        "--pattern",
                        name,
                        "--dir",
                        temporary,
                    ]
                )
                if digest(Path(temporary) / name) != digest(path):
                    raise ValueError("durable build asset conflict: " + name)
        elif args.finish:
            raise ValueError("durable build asset missing: " + name)
        else:
            run(["gh", "release", "upload", tag, str(path), "--repo", repository])
    if args.finish:
        receipt = read_json(args.receipt)
        if (
            receipt["status"] != "available"
            or receipt["manifest_sha256"] != digest(args.manifest)
            or receipt["fixture"]
        ):
            raise ValueError("publication receipt does not prove real availability")
        run(["gh", "release", "edit", tag, "--repo", repository, "--draft=false"])


def resume(args):
    """Recover a fully staged candidate before rebuilding or publishing."""
    record = metadata(args.metadata)
    tag = f"cli/v{record['version']}-build.{record['build_number']}"
    repository = "UniClipboard/UniClipboard"
    result = subprocess.run(
        ["gh", "api", "repos/" + repository + "/releases/tags/" + quote(tag, safe="")],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if result.returncode:
        if "HTTP 404" not in result.stderr:
            raise ValueError("cannot query staged candidate: " + result.stderr)
        staged = False
    else:
        output = Path(args.output).resolve()
        output.mkdir(parents=True, exist_ok=True)
        base = [
            "gh",
            "release",
            "download",
            tag,
            "--repo",
            repository,
            "--dir",
            str(output),
        ]
        run(base + ["--pattern", "release-manifest.json"])
        manifest = validate_manifest(output / "release-manifest.json")
        if (
            manifest["version"],
            manifest["build_number"],
            manifest["source_sha"],
            manifest["metadata_sha256"],
        ) != (
            record["version"],
            record["build_number"],
            run(["git", "rev-parse", "HEAD"]),
            digest(args.metadata),
        ):
            raise ValueError(
                "staged candidate conflicts with the selected source or metadata"
            )
        for package in manifest["packages"]:
            run(base + ["--pattern", package["file"]])
            checked_file(output, package["file"], package["sha256"])
        staged = True
    with Path(args.github_output).open("a") as output:
        output.write("staged=" + str(staged).lower() + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("metadata")
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    p.add_argument("--product", default="cli")
    p = sub.add_parser("matrix")
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    p.add_argument("--product", default="cli")
    p = sub.add_parser("record")
    p.add_argument("--manifest", required=True)
    p.add_argument("--repository", default="UniClipboard/UniClipboard")
    p.add_argument("--finish", action="store_true")
    p.add_argument("--receipt")
    p = sub.add_parser("resume")
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    p.add_argument("--output", required=True)
    p.add_argument("--github-output", required=True)
    p = sub.add_parser("smoke")
    p.add_argument("--directory", required=True)
    p = sub.add_parser("assemble")
    p.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    p.add_argument("--input", required=True)
    p.add_argument("--output", required=True)
    for name in ["publish", "promote"]:
        p = sub.add_parser(name)
        p.add_argument("--manifest", required=True)
        p.add_argument("--channel", choices=["alpha", "stable"], required=True)
        p.add_argument("--registry", default="https://registry.npmjs.org")
        p.add_argument("--receipt", required=True)
    args = parser.parse_args()
    if args.command == "metadata":
        print(json.dumps(metadata(args.metadata, args.product)))
    elif args.command == "matrix":
        matrix(args)
    elif args.command == "record":
        github_record(args)
    elif args.command == "resume":
        resume(args)
    elif args.command == "smoke":
        smoke(args)
    elif args.command == "assemble":
        assemble(args)
    else:
        distribute(args)


if __name__ == "__main__":
    main()
