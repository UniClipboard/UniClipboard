#!/usr/bin/env python3
"""Stage immutable CLI inputs and construct native Desktop distribution packages."""

import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from urllib.parse import quote
from urllib.request import urlopen

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "cli_release", ROOT / "tools/release/release.py"
)
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)
APP_ID = "app.uniclipboard.desktop"
TARGETS = {
    os + "-" + arch
    for os in ["darwin", "linux", "windows"]
    for arch in ["amd64", "arm64"]
}
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


def run(args, **kwargs):
    return subprocess.run(
        [str(x) for x in args],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        **kwargs,
    ).stdout.strip()


def sha(path):
    return cli.digest(path)


def save(path, value):
    cli.save(path, value)


def sources():
    names = run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"]
    ).split("\0")
    return {
        name: sha(ROOT / name)
        for name in sorted(set(names))
        if name and (ROOT / name).is_file() and not (ROOT / name).is_symlink()
    }


def fresh_output(path):
    path = Path(path).resolve()
    if path.exists():
        raise ValueError("output already exists: " + str(path))
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def locked_cli(metadata, manifest_path, target, development):
    manifest = cli.validate_manifest(manifest_path)
    if (sha(manifest_path), manifest["version"], manifest["build_number"]) != (
        metadata.get("cli_manifest_sha256"),
        metadata.get("cli_version"),
        metadata.get("cli_build_number"),
    ):
        raise ValueError("Desktop CLI lock mismatch")
    if not development and manifest.get("source_clean") is not True:
        raise ValueError("locked CLI source is not clean")
    if target not in manifest["targets"] or len(manifest["targets"]) != len(
        set(manifest["targets"])
    ):
        raise ValueError("missing or duplicate CLI target")
    native = manifest["target_manifests"][target]
    if not all(
        native.get("acceptance", {}).get(k) is True
        for k in ["native", "version_smoke", "alias_smoke"]
    ):
        raise ValueError("locked CLI target lacks acceptance")
    package = next(p for p in manifest["packages"] if p.get("target") == target)
    path = cli.checked_file(
        Path(manifest_path).parent, package["file"], package["sha256"]
    )
    actual = (
        "sha512-" + base64.b64encode(bytes.fromhex(cli.digest(path, "sha512"))).decode()
    )
    if (
        package["integrity"] != actual
        or package["binaries_sha256"] != native["binaries_sha256"]
    ):
        raise ValueError("CLI package integrity or target mismatch")
    return manifest, package, path


def fetch_cli(args):
    record = cli.metadata(args.metadata, "desktop")
    expected = record.get("cli_manifest_sha256", "")
    if not cli.SHA256.fullmatch(expected) or expected == "0" * 64:
        raise ValueError("select a real accepted CLI lock before downloading")
    if args.allow_ci:
        sys.path.insert(0, str(ROOT / "tools/release"))
        import cli_candidate

        output = fresh_output(args.output)
        with tempfile.TemporaryDirectory(prefix=".unic-ci-cli-", dir=output.parent) as temporary:
            work = Path(temporary) / "candidate"
            cli_candidate.acquire(record.get("cli_artifact_run_id"), work, args.metadata, signed=True)
            locked_cli(record, work / "release-manifest.json", args.target, False)
            work.rename(output)
        print(output / "release-manifest.json")
        return
    tag = "cli/v" + record["cli_version"] + "-build." + str(record["cli_build_number"])
    base = (
        "https://github.com/UniClipboard/UniClipboard/releases/download/"
        + quote(tag, safe="")
        + "/"
    )
    output = fresh_output(args.output)
    with tempfile.TemporaryDirectory(
        prefix=".unic-cli-", dir=output.parent
    ) as temporary:
        work = Path(temporary)

        def download(name, limit):
            if Path(name).name != name:
                raise ValueError("unsafe CLI asset name")
            with urlopen(base + quote(name), timeout=60) as response:
                with (work / name).open("xb") as destination:
                    remaining = limit
                    while True:
                        block = response.read(min(1024 * 1024, remaining + 1))
                        if not block:
                            break
                        remaining -= len(block)
                        if remaining < 0:
                            raise ValueError("CLI download exceeds limit")
                        destination.write(block)
            return work / name

        manifest_path = download("release-manifest.json", 16 * 1024 * 1024)
        if sha(manifest_path) != expected:
            raise ValueError("downloaded CLI manifest digest mismatch")
        manifest = cli.validate_manifest(manifest_path)
        package = next(
            p for p in manifest["packages"] if p.get("target") == args.target
        )
        download(package["file"], cli.MAX_FILE)
        locked_cli(record, manifest_path, args.target, False)
        output.mkdir()
        for path in work.iterdir():
            path.rename(output / path.name)
    print(output / "release-manifest.json")


def sign(args):
    root, m, payload = validated_stage(args.stage)
    os_name = m["target"].split("-")[0]
    if m["development"] or not m["source_clean"]:
        raise ValueError("development/dirty stage cannot be signed for release")
    if os_name == "darwin" and platform.system() == "Darwin":
        for name in ["unic", "unicd"]:
            run(["codesign", "--verify", "--strict", payload / name])
            info = subprocess.run(
                ["codesign", "--display", "--verbose=4", str(payload / name)],
                capture_output=True,
                text=True,
                check=True,
            )
            if (
                "TeamIdentifier=not set" in info.stderr
                or "TeamIdentifier=" not in info.stderr
            ):
                raise ValueError(
                    "locked CLI needs a distribution signature before acceptance"
                )
        run(
            [
                "codesign",
                "--force",
                "--options",
                "runtime",
                "--timestamp",
                "--sign",
                args.identity,
                root / "UniClipboard.app",
            ]
        )
        run(["codesign", "--verify", "--deep", "--strict", root / "UniClipboard.app"])
    elif os_name == "windows" and platform.system() == "Windows":
        if not re.fullmatch(r"[a-fA-F0-9]{40}", args.identity):
            raise ValueError(
                "Windows signing identity must be a certificate thumbprint"
            )
        for name in ["unic.exe", "unicd.exe"]:
            run(["signtool", "verify", "/pa", payload / name])
        run(
            [
                "signtool",
                "sign",
                "/fd",
                "SHA256",
                "/td",
                "SHA256",
                "/tr",
                "http://timestamp.digicert.com",
                "/sha1",
                args.identity,
                payload / "unic-desktop.exe",
            ]
        )
        run(["signtool", "verify", "/pa", payload / "unic-desktop.exe"])
    else:
        raise ValueError("signing requires matching native macOS/Windows tools")
    if any(
        sha(payload / name) != expected
        for name, expected in m["cli"]["binaries_sha256"].items()
    ):
        raise ValueError("signing altered locked CLI bytes")
    m["files"] = {
        str(p.relative_to(root)): sha(p)
        for p in root.rglob("*")
        if p.is_file() and not p.is_symlink() and p.name != "manifest.json"
    }
    m["acceptance"]["signing"] = True
    m["acceptance"]["native_identity"] = False
    save(root / "manifest.json", m)
    print(root / "manifest.json")


def notarize_stage(args):
    root, m, _ = validated_stage(args.stage)
    if not m["target"].startswith("darwin-") or not m["acceptance"].get("signing"):
        raise ValueError("notarization requires a signed macOS stage")
    output = fresh_output(args.output)
    try:
        shutil.copytree(root, output, symlinks=True)
        with tempfile.TemporaryDirectory(prefix="unic-notary-") as temporary:
            archive = Path(temporary) / "notary.zip"
            bundle = output / "UniClipboard.app"
            run(["ditto", "-c", "-k", "--keepParent", bundle, archive])
            run([sys.executable, ROOT / "tools/package/signing.py", "notarize", "--file", archive, "--staple", bundle, "--receipt", output / "notarization.json"])
        run(["codesign", "--verify", "--deep", "--strict", bundle])
        m["files"] = {str(p.relative_to(output)): sha(p) for p in output.rglob("*") if p.is_file() and not p.is_symlink() and p != output / "manifest.json"}
        m["acceptance"]["notarization"] = True
        save(output / "manifest.json", m)
        validated_stage(output)
    except BaseException:
        if output.exists(): shutil.rmtree(output)
        raise
    print(output / "manifest.json")


def stage(args):
    target = args.target
    if target not in TARGETS:
        raise ValueError("unsupported Desktop target")
    record = cli.metadata(args.metadata, "desktop")
    if target not in record["targets"]:
        raise ValueError("target not declared in Desktop metadata")
    output = fresh_output(args.output)
    manifest, package, archive = locked_cli(
        record, args.cli_manifest, target, args.development
    )
    source = run(["git", "rev-parse", "HEAD"])
    dirty = bool(run(["git", "status", "--porcelain"]))
    if dirty and not args.development:
        raise ValueError("production Desktop requires clean source")
    initial = sources()
    metadata_sha = sha(args.metadata)
    os_name, arch = target.split("-")
    suffix = ".exe" if os_name == "windows" else ""
    with tempfile.TemporaryDirectory(prefix=".unic-stage-", dir=output.parent) as work:
        root = Path(work) / "stage"
        payload = root / (
            "UniClipboard.app/Contents/MacOS" if os_name == "darwin" else "payload"
        )
        payload.mkdir(parents=True)
        with tarfile.open(archive) as packed:
            for name in ["unic" + suffix, "unicd" + suffix]:
                matches = [
                    e for e in packed.getmembers() if e.name == "package/bin/" + name
                ]
                if (
                    len(matches) != 1
                    or not matches[0].isfile()
                    or not 0 < matches[0].size <= cli.MAX_FILE
                ):
                    raise ValueError("invalid locked CLI member: " + name)
                path = payload / name
                with packed.extractfile(matches[0]) as src, path.open("wb") as dest:
                    shutil.copyfileobj(src, dest)
                path.chmod(0o755)
                if sha(path) != package["binaries_sha256"][name]:
                    raise ValueError("locked executable digest mismatch")
        if suffix:
            (payload / "uniclip.cmd").write_bytes(
                b'@echo off\r\n"%~dp0unic.exe" %*\r\nexit /b %errorlevel%\r\n'
            )
        else:
            (payload / "uniclip").symlink_to("unic")
        linker = f"-X github.com/uniclipboard/unic/internal/releaseinfo.Version={record['version']} -X github.com/uniclipboard/unic/internal/releaseinfo.BuildNumber={record['build_number']} -X github.com/uniclipboard/unic/internal/releaseinfo.Source={source} -X main.desktopAppName=UniClipboard -X github.com/uniclipboard/unic/internal/observability.ProductVersion={record['version']}"
        update_key = record.get("update_public_key", "")
        if update_key:
            if len(base64.b64decode(update_key, validate=True)) != 32:
                raise ValueError("Desktop update_public_key must be a raw Ed25519 public key")
            linker += f" -X github.com/egoist/mygo.production=1 -X github.com/egoist/mygo.packageUpdateFeed=https://release.uniclipboard.app/v1/desktop/stable.json -X github.com/egoist/mygo.packageUpdateKey={update_key} -X github.com/uniclipboard/unic/app/desktop/internal/appupdates.PublicKey={update_key}"
        if os_name == "windows":
            linker += " -H windowsgui"
        env = dict(os.environ, GOOS=os_name, GOARCH=arch)
        if os_name != "darwin":
            env["CGO_ENABLED"] = "0"
        run(
            [
                "go",
                "build",
                "-trimpath",
                "-buildvcs=false",
                "-ldflags",
                linker,
                "-o",
                payload / ("unic-desktop" + suffix),
                "./app/desktop/cmd/unic-desktop",
            ],
            env=env,
        )
        resources = payload.parent / "Resources" if os_name == "darwin" else payload
        resources.mkdir(exist_ok=True)
        save(resources / "release-identity.json", {
            "product": "desktop", "version": record["version"],
            "build_number": record["build_number"], "target": target,
            "cli_manifest_sha256": sha(args.cli_manifest), "source_sha": source,
        })
        (resources / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
        if os_name == "darwin":
            info = {
                "CFBundleIdentifier": APP_ID,
                "CFBundleName": "UniClipboard",
                "CFBundleDisplayName": "UniClipboard",
                "CFBundleExecutable": "unic-desktop",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": record["version"],
                "CFBundleVersion": str(record["build_number"]),
                "LSMinimumSystemVersion": "13.0",
                "NSHighResolutionCapable": True,
                "NSPrincipalClass": "NSApplication",
            }
            (payload.parent / "Info.plist").write_bytes(plistlib.dumps(info))
        else:
            (payload / "installation.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "application_id": APP_ID,
                        "target": target,
                    }
                )
                + "\n"
            )
        if os_name != "darwin":
            (payload / ".mygo-files").write_text("".join(p.name + "\n" for p in sorted(payload.iterdir())))
        if (
            initial != sources()
            or source != run(["git", "rev-parse", "HEAD"])
            or metadata_sha != sha(args.metadata)
            or sha(archive) != package["sha256"]
        ):
            raise ValueError("source, metadata or CLI input changed during staging")
        files = {
            str(p.relative_to(root)): sha(p)
            for p in root.rglob("*")
            if p.is_file() and not p.is_symlink()
        }
        links = {
            str(p.relative_to(root)): os.readlink(p)
            for p in root.rglob("*")
            if p.is_symlink()
        }
        receipt = {
            "schema": 1,
            "product": "desktop",
            "version": record["version"],
            "build_number": record["build_number"],
            "target": target,
            "source_sha": source,
            "source_clean": not dirty,
            "source_files": initial,
            "metadata_sha256": metadata_sha,
            "development": args.development,
            "payload": str(payload.relative_to(root)),
            "files": files,
            "links": links,
            "update_public_key": record.get("update_public_key", ""),
            "cli": {
                "version": manifest["version"],
                "build_number": manifest["build_number"],
                "manifest_sha256": sha(args.cli_manifest),
                "source_sha": manifest["source_sha"],
                "binaries_sha256": package["binaries_sha256"],
            },
            "acceptance": {
                "native_identity": False,
                "package_lifecycle": False,
                "signing": False,
            },
        }
        save(root / "manifest.json", receipt)
        root.rename(output)
    print(output / "manifest.json")


def validated_stage(path):
    root = Path(path).resolve()
    m = cli.read_json(root / "manifest.json")
    if (
        m.get("schema") != 1
        or m.get("product") != "desktop"
        or m.get("target") not in TARGETS
    ):
        raise ValueError("invalid Desktop stage manifest")
    for name, expected in m["files"].items():
        path = root / name
        if (
            path.is_symlink()
            or not path.is_file()
            or not path.resolve().is_relative_to(root)
            or sha(path) != expected
        ):
            raise ValueError("stage file mismatch: " + name)
    for name, target in m["links"].items():
        path = root / name
        if (
            not path.is_symlink()
            or os.readlink(path) != target
            or not path.resolve().is_relative_to(root)
        ):
            raise ValueError("stage alias mismatch")
    actual = {
        str(p.relative_to(root))
        for p in root.rglob("*")
        if p.is_file() or p.is_symlink()
    }
    if actual != set(m["files"]) | set(m["links"]) | {"manifest.json"}:
        raise ValueError("unexpected stage files")
    payload = root / m["payload"]
    if not payload.resolve().is_relative_to(root):
        raise ValueError("invalid payload path")
    for name, expected in m["cli"]["binaries_sha256"].items():
        if Path(name).name != name or sha(payload / name) != expected:
            raise ValueError("staged CLI no longer matches the immutable lock")
    if not m.get("development") and (
        m.get("source_clean") is not True
        or not cli.SHA256.fullmatch(m["cli"]["manifest_sha256"])
        or m["cli"]["manifest_sha256"] == "0" * 64
    ):
        raise ValueError("production stage lacks clean source or a real CLI lock")
    return root, m, payload


def smoke(args):
    root, m, payload = validated_stage(args.stage)
    native = (
        {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[platform.system()]
        + "-"
        + {
            "arm64": "arm64",
            "aarch64": "arm64",
            "x86_64": "amd64",
            "AMD64": "amd64",
            "ARM64": "arm64",
        }[platform.machine()]
    )
    if m["target"] != native:
        raise ValueError("identity acceptance requires matching native target")
    suffix = ".exe" if native.startswith("windows") else ""
    with tempfile.TemporaryDirectory(prefix="unic-identity-") as home:
        env = dict(os.environ, UNIC_HOME=home, UNIC_TELEMETRY_ENABLED="false")
        for name in ["unic-desktop", "unic", "unicd"]:
            value = json.loads(
                run([payload / (name + suffix), "--version", "--json"], env=env)
            )
            version = m if name == "unic-desktop" else m["cli"]
            expected = (
                version["version"],
                str(version["build_number"]),
                version["source_sha"],
            )
            if (
                value["version"],
                value["build_number"],
                value["source_sha"],
            ) != expected:
                raise ValueError("native executable identity mismatch")
        if list(Path(home).iterdir()):
            raise ValueError("version inspection initialized user state")
    m["acceptance"]["native_identity"] = True
    save(root / "manifest.json", m)
    print(root / "manifest.json")


def package(args):
    root, m, payload = validated_stage(args.stage)
    kind = args.format
    os_name, arch = m["target"].split("-")
    if kind not in FORMATS[os_name]:
        raise ValueError("format does not match target")
    output = fresh_output(args.output)
    output.mkdir()
    base = f"uniclipboard-{m['version']}-build.{m['build_number']}-{m['target']}"
    artifacts = []
    try:
        if args.notarize and (
            os_name != "darwin" or m["acceptance"].get("signing") is not True
        ):
            raise ValueError("notarization requires a distribution-signed macOS stage")
        if kind == "update.tar.gz":
            file = output / (base + ".update.tar.gz")
            with tarfile.open(file, "w:gz", dereference=False) as t:
                for path in sorted(payload.iterdir()):
                    t.add(path, arcname=path.name)
        elif kind in ["deb", "rpm", "archlinux"]:
            contents = [
                {
                    "src": str(p),
                    "dst": "/usr/lib/uniclipboard/" + p.name,
                    "type": "symlink" if p.is_symlink() else "",
                }
                for p in sorted(payload.iterdir())
                if not p.is_symlink()
            ]
            contents += [
                {
                    "src": "unic",
                    "dst": "/usr/lib/uniclipboard/uniclip",
                    "type": "symlink",
                },
                {
                    "src": "/usr/lib/uniclipboard/unic-desktop",
                    "dst": "/usr/bin/uniclipboard",
                    "type": "symlink",
                },
                {
                    "src": str(ROOT / "packaging/linux/uniclipboard.desktop"),
                    "dst": "/usr/share/applications/" + APP_ID + ".desktop",
                },
                {
                    "src": str(ROOT / "packaging/linux/uniclipboard.svg"),
                    "dst": "/usr/share/icons/hicolor/scalable/apps/uniclipboard.svg",
                },
            ]
            dependencies = {
                "deb": ["libgtk-3-0 | libgtk-3-0t64", "libayatana-appindicator3-1"],
                "rpm": ["gtk3", "libappindicator-gtk3"],
                "archlinux": ["gtk3", "libappindicator-gtk3"],
            }
            config = {
                "name": "uniclipboard",
                "arch": arch,
                "platform": "linux",
                "version": m["version"],
                "release": str(m["build_number"]),
                "maintainer": "UniClipboard <support@uniclipboard.app>",
                "description": "Cross-device clipboard synchronization",
                "homepage": "https://uniclipboard.app",
                "license": "AGPL-3.0-only",
                "contents": contents,
                "depends": dependencies[kind],
            }
            file = output / (
                base + {"deb": ".deb", "rpm": ".rpm", "archlinux": ".pkg.tar.zst"}[kind]
            )
            config_path = output / "nfpm.json"
            config_path.write_text(json.dumps(config))
            run(
                [
                    args.nfpm,
                    "package",
                    "--config",
                    config_path,
                    "--packager",
                    kind,
                    "--target",
                    file,
                ]
            )
            config_path.unlink()
        elif kind == "portable.zip":
            file = output / (base + "-portable.zip")
            with zipfile.ZipFile(file, "w", zipfile.ZIP_DEFLATED) as z:
                for path in sorted(payload.iterdir()):
                    z.write(path, path.name)
                z.writestr("portable.dat", "")
                z.writestr(
                    "README.txt",
                    "UniClipboard portable mode stores v2 data in data/ beside these programs. Explicit UNIC_HOME overrides this location. Back up data/ before removal.\n",
                )
        elif kind == "setup.exe":
            # NSIS compiles the native installer; execution is accepted only on Windows.
            major, minor, patch = map(int, m["version"].split("."))
            if max(major, minor, patch, m["build_number"]) > 65535:
                raise ValueError("Windows native version component exceeds 65535")
            file = output / (base + "-setup.exe")
            script = (
                (ROOT / "packaging/windows/installer.nsi")
                .read_text()
                .replace("@PAYLOAD@", str(payload))
                .replace("@OUTPUT@", str(file))
                .replace("@VERSION@", m["version"])
                .replace("@BUILD@", str(m["build_number"]))
            )
            config = output / "installer.nsi"
            config.write_text(script)
            run(["makensis", config])
            config.unlink()
        elif kind in ["dmg", "app.tar.gz"]:
            if platform.system() != "Darwin":
                raise ValueError("macOS package construction requires macOS")
            bundle = root / "UniClipboard.app"
            if args.notarize:
                # Notarization/stapling mutate a copy, never accepted stage inputs.
                bundle_copy = output / "notary-input" / "UniClipboard.app"
                shutil.copytree(bundle, bundle_copy, symlinks=True)
                archive = output / "notary-input.zip"
                run(["ditto", "-c", "-k", "--keepParent", bundle_copy, archive])
                run(
                    [
                        sys.executable,
                        ROOT / "tools/package/signing.py",
                        "notarize",
                        "--file",
                        archive,
                        "--staple",
                        bundle_copy,
                        "--receipt",
                        output / "notarization.json",
                    ]
                )
                archive.unlink()
                bundle = bundle_copy
            file = output / (base + (".dmg" if kind == "dmg" else ".app.tar.gz"))
            if kind == "dmg":
                run(
                    [
                        "hdiutil",
                        "create",
                        "-quiet",
                        "-volname",
                        "UniClipboard",
                        "-srcfolder",
                        bundle,
                        "-format",
                        "UDZO",
                        file,
                    ]
                )
            else:
                with tarfile.open(file, "w:gz", dereference=False) as t:
                    t.add(bundle, arcname=bundle.name)
        elif kind in ["AppImage", "AppImage.tar.gz"]:
            if platform.system() != "Linux":
                raise ValueError(
                    "AppImage construction requires a matching Linux builder"
                )
            with tempfile.TemporaryDirectory(prefix="unic-appdir-") as temporary:
                app = Path(temporary) / "UniClipboard.AppDir"
                shutil.copytree(payload, app / "usr/lib/uniclipboard", symlinks=True)
                shutil.copy(ROOT / "packaging/linux/AppRun", app / "AppRun")
                (app / "AppRun").chmod(0o755)
                shutil.copy(
                    ROOT / "packaging/linux/uniclipboard.desktop",
                    app / "uniclipboard.desktop",
                )
                shutil.copy(
                    ROOT / "packaging/linux/uniclipboard.svg", app / "uniclipboard.svg"
                )
                env = dict(
                    os.environ, ARCH={"amd64": "x86_64", "arm64": "aarch64"}[arch]
                )
                image = output / (base + ".AppImage")
                if not args.appimage_runtime:
                    raise ValueError(
                        "AppImage requires a pinned --appimage-runtime file"
                    )
                runtime = Path(args.appimage_runtime).resolve()
                expected = cli.read_json(ROOT / "packaging/tools.json")[
                    "appimage-runtime"
                ]["assets"][arch]["sha256"]
                if sha(runtime) != expected:
                    raise ValueError("AppImage runtime digest mismatch")
                env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
                run(
                    [
                        args.appimagetool,
                        "--no-appstream",
                        "--runtime-file",
                        runtime,
                        app,
                        image,
                    ],
                    env=env,
                )
                image.chmod(0o755)
                file = image
                if kind == "AppImage.tar.gz":
                    file = output / (base + ".AppImage.tar.gz")
                    with tarfile.open(file, "w:gz") as t:
                        t.add(image, arcname=image.name)
                    image.unlink()
        if args.notarize:
            shutil.rmtree(output / "notary-input")
        update_signature = ""
        if kind in ["app.tar.gz", "update.tar.gz"] and os.environ.get("UNIC_UPDATE_PRIVATE_KEY"):
            signature = json.loads(run(["go", "run", "./tools/release/update-sign", "-file", file]))
            if signature["public_key"] != m.get("update_public_key") or signature["sha256"] != sha(file):
                raise ValueError("update signature disagrees with staged key or archive")
            update_signature = signature["signature"]
        artifacts.append(
            {
                "signature": update_signature,
                "format": kind,
                "file": file.name,
                "sha256": sha(file),
                "size": file.stat().st_size,
                "signed": bool(m["acceptance"].get("signing")),
                "notarized": bool(args.notarize or m["acceptance"].get("notarization")),
            }
        )
        validated_stage(root)
        save(
            output / "artifacts.json",
            {
                "schema": 1,
                "product": "desktop",
                "version": m["version"],
                "build_number": m["build_number"],
                "target": m["target"],
                "source_sha": m["source_sha"],
                "source_clean": m["source_clean"],
                "development": m["development"],
                "cli": m["cli"],
                "stage_manifest_sha256": sha(root / "manifest.json"),
                "acceptance": m["acceptance"],
                "artifacts": artifacts,
            },
        )
    except BaseException:
        shutil.rmtree(output)
        raise
    print(output / "artifacts.json")


def accept(args):
    directory = Path(args.packages).resolve()
    path = directory / "artifacts.json"
    m = cli.read_json(path)
    receipt = cli.read_json(args.receipt)
    required = {
        "installation",
        "reinstallation",
        "upgrade",
        "removal",
        "data_preservation",
        "private_cli_ownership",
        "temporary_service_shutdown",
    }
    if (
        receipt.get("status") != "passed"
        or receipt.get("target") != m["target"]
        or not required.issubset(set(receipt.get("assertions", [])))
    ):
        raise ValueError("lifecycle receipt lacks required observations")
    if not m["artifacts"]:
        raise ValueError("empty package set")
    for item in m["artifacts"]:
        cli.checked_file(directory, item["file"], item["sha256"])
        if item["format"] != receipt.get("format") or item["sha256"] not in receipt.get(
            "artifacts_sha256", []
        ):
            raise ValueError("lifecycle receipt does not bind the accepted package")
    m["acceptance"]["package_lifecycle"] = True
    m["acceptance"]["lifecycle_receipt_sha256"] = sha(args.receipt)
    m["acceptance"]["native_gui"] = receipt.get("gui", "not_run")
    save(path, m)
    print(path)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("accept")
    s.add_argument("--packages", required=True)
    s.add_argument("--receipt", required=True)
    s = sub.add_parser("fetch-cli")
    s.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    s.add_argument("--target", required=True, choices=sorted(TARGETS))
    s.add_argument("--output", required=True)
    s.add_argument("--allow-ci", action="store_true", help="Use the explicitly locked canonical CI candidate before public publication")
    s = sub.add_parser("sign")
    s.add_argument("--stage", required=True)
    s.add_argument("--identity", required=True)
    s = sub.add_parser("notarize-stage")
    s.add_argument("--stage", required=True)
    s.add_argument("--output", required=True)
    s = sub.add_parser("stage")
    s.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    s.add_argument("--cli-manifest", required=True)
    s.add_argument("--target", required=True)
    s.add_argument("--output", required=True)
    s.add_argument("--development", action="store_true")
    s = sub.add_parser("smoke")
    s.add_argument("--stage", required=True)
    s = sub.add_parser("package")
    s.add_argument("--stage", required=True)
    s.add_argument(
        "--format",
        required=True,
        choices=sorted({f for formats in FORMATS.values() for f in formats}),
    )
    s.add_argument("--output", required=True)
    s.add_argument("--nfpm", default="nfpm")
    s.add_argument("--notarize", action="store_true")
    s.add_argument("--appimagetool", default="appimagetool")
    s.add_argument("--appimage-runtime")
    args = p.parse_args()
    {
        "accept": accept,
        "fetch-cli": fetch_cli,
        "sign": sign,
        "stage": stage,
        "notarize-stage": notarize_stage,
        "smoke": smoke,
        "package": package,
    }[args.command](args)


if __name__ == "__main__":
    main()
