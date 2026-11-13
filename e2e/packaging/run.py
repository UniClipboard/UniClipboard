#!/usr/bin/env python3
"""Production packager acceptance with real Go/npm processes and no GUI startup."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument("--output", default=str(ROOT / ".cache/package-e2e"))
p.add_argument(
    "--format",
    action="append",
    choices=["deb", "rpm", "archlinux", "dmg", "app.tar.gz"],
)
p.add_argument("--lifecycle", action="store_true")
a = p.parse_args()
if a.lifecycle and ("deb" not in (a.format or []) or not Path("/.dockerenv").exists()):
    p.error("--lifecycle requires --format deb in a disposable container")
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "commands": [],
    "checks": [],
    "not_run": [
        "GUI",
        "public publication",
        "native installer replacement",
        "signing/notarization",
    ],
}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(args, ok=True, **kwargs):
    result = subprocess.run(
        [str(x) for x in args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        **kwargs,
    )
    report["commands"].append(
        {
            "argv": [str(x) for x in args],
            "exit_code": result.returncode,
            "output": result.stdout[-6000:],
        }
    )
    if (result.returncode == 0) != ok:
        raise RuntimeError(result.stdout[-6000:])
    return result.stdout


def check(name, condition):
    if not condition:
        raise AssertionError(name)
    report["checks"].append(name)


try:
    report["source_sha"] = run(["git", "rev-parse", "HEAD"]).strip()
    report["source_files"] = {
        str(f.relative_to(ROOT)): sha(f)
        for d in ["tools/package", "packaging", "e2e/packaging", "release"]
        for f in (ROOT / d).rglob("*")
        if f.is_file() and "__pycache__" not in str(f)
    }
    operating_system = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[
        platform.system()
    ]
    arch = {
        "arm64": "arm64",
        "aarch64": "arm64",
        "x86_64": "amd64",
        "AMD64": "amd64",
        "ARM64": "arm64",
    }[platform.machine()]
    target = operating_system + "-" + arch
    report["target"] = target
    with tempfile.TemporaryDirectory(prefix="unic-package-e2e-") as temporary:
        temp = Path(temporary).resolve()
        metadata = temp / "products.toml"

        def configure(lock="0" * 64, desktop_build=1):
            metadata.write_text(f'''schema = 1
[products.cli]
version = "2.0.0"
build_number = 1
targets = ["{target}"]
[products.desktop]
version = "2.0.0"
build_number = {desktop_build}
targets = ["{target}"]
cli_version = "2.0.0"
cli_build_number = 1
cli_manifest_sha256 = "{lock}"
''')

        configure()
        cli = temp / "cli"
        run(
            [
                "go",
                "run",
                "./tools/package",
                "-product",
                "cli",
                "-metadata",
                metadata,
                "-output",
                cli,
            ]
        )
        tool = ROOT / "tools/release/release.py"
        run([sys.executable, tool, "smoke", "--directory", cli])
        npm = temp / "npm"
        run(
            [
                sys.executable,
                tool,
                "assemble",
                "--metadata",
                metadata,
                "--input",
                cli,
                "--output",
                npm,
            ]
        )
        manifest = npm / "release-manifest.json"
        configure(sha(manifest))
        desktop = ROOT / "tools/package/desktop.py"
        stage = temp / "Desktop stage with spaces"
        base = [
            sys.executable,
            desktop,
            "stage",
            "--metadata",
            metadata,
            "--cli-manifest",
            manifest,
            "--target",
            target,
            "--output",
            stage,
        ]
        run(base + ["--development"])
        run([sys.executable, desktop, "smoke", "--stage", stage])
        receipt = json.loads((stage / "manifest.json").read_text())
        payload = stage / receipt["payload"]
        report["stage_manifest_sha256"] = sha(stage / "manifest.json")
        report["staged_source_files"] = receipt["source_files"]
        suffix = ".exe" if operating_system == "windows" else ""
        value = json.loads(
            run([payload / ("unic-desktop" + suffix), "--version", "--json"])
        )
        check(
            "desktop version identity",
            value["version"] == "2.0.0" and value["build_number"] == "1",
        )
        for name in ["unic", "unicd"]:
            check(
                "locked " + name + " bytes unchanged",
                sha(payload / (name + suffix))
                == receipt["cli"]["binaries_sha256"][name + suffix],
            )
        portable = temp / "Portable location with spaces"
        shutil.copytree(payload, portable, symlinks=True)
        (portable / "portable.dat").write_text("")
        portable_env = dict(
            os.environ,
            HOME=str(temp / "home"),
            USERPROFILE=str(temp / "home"),
            APPDATA=str(temp / "roaming"),
            LOCALAPPDATA=str(temp / "local"),
            XDG_CONFIG_HOME=str(temp / "config"),
        )
        portable_env.pop("UNIC_HOME", None)
        portable_env.pop("UNIC_SOCK", None)
        config_path = run(
            [portable / ("unic" + suffix), "config", "path"], env=portable_env
        ).strip()
        check("portable data root", Path(config_path) == portable / "data/config.toml")
        portable_env["UNIC_HOME"] = str(temp / "explicit")
        config_path = run(
            [portable / ("unic" + suffix), "config", "path"], env=portable_env
        ).strip()
        check(
            "explicit profile overrides portable",
            Path(config_path) == temp / "explicit/config.toml",
        )
        run(base + ["--development"], ok=False)
        check("existing stage rejected", True)
        configure()
        run(base[:-1] + [temp / "wrong lock"] + ["--development"], ok=False)
        check("wrong lock rejected", True)
        configure(sha(manifest))
        original = manifest.read_bytes()
        manifest.write_bytes(original + b" ")
        run(base[:-1] + [temp / "changed manifest"] + ["--development"], ok=False)
        check("changed manifest rejected", True)
        manifest.write_bytes(original)
        package = next(
            item
            for item in json.loads(original)["packages"]
            if item.get("target") == target
        )
        path = npm / package["file"]
        original_package = path.read_bytes()
        path.write_bytes(original_package + b"changed")
        run(base[:-1] + [temp / "changed package"] + ["--development"], ok=False)
        check("changed npm bytes rejected", True)
        path.write_bytes(original_package)
        for format_name in a.format or []:
            packages = temp / ("packages-" + format_name)
            run(
                [
                    sys.executable,
                    desktop,
                    "package",
                    "--stage",
                    stage,
                    "--format",
                    format_name,
                    "--output",
                    packages,
                ]
            )
            built = json.loads((packages / "artifacts.json").read_text())
            check(
                "package artifact hash: " + format_name,
                all(
                    sha(packages / x["file"]) == x["sha256"] for x in built["artifacts"]
                ),
            )
            if a.lifecycle and format_name == "deb":
                configure(sha(manifest), desktop_build=2)
                second = temp / "second-stage"
                run(base[:-1] + [second] + ["--development"])
                newer = temp / "newer-packages"
                run(
                    [
                        sys.executable,
                        desktop,
                        "package",
                        "--stage",
                        second,
                        "--format",
                        "deb",
                        "--output",
                        newer,
                    ]
                )
                lifecycle = out / "linux-lifecycle"
                first_file = packages / built["artifacts"][0]["file"]
                second_manifest = json.loads((newer / "artifacts.json").read_text())
                second_file = newer / second_manifest["artifacts"][0]["file"]
                run(
                    [
                        "sh",
                        "-x",
                        ROOT / "e2e/packaging/linux-lifecycle.sh",
                        first_file,
                        second_file,
                        lifecycle,
                    ]
                )
                run(
                    [
                        sys.executable,
                        desktop,
                        "accept",
                        "--packages",
                        packages,
                        "--receipt",
                        lifecycle / "report.json",
                    ]
                )
                check(
                    "lifecycle receipt attached to exact package",
                    json.loads((packages / "artifacts.json").read_text())["acceptance"][
                        "package_lifecycle"
                    ]
                    is True,
                )
                run(
                    [
                        sys.executable,
                        desktop,
                        "accept",
                        "--packages",
                        newer,
                        "--receipt",
                        lifecycle / "report.json",
                    ]
                )
                check(
                    "DEB install upgrade remove lifecycle",
                    json.loads((lifecycle / "report.json").read_text())["status"]
                    == "passed",
                )
        check(
            "source unchanged during acceptance",
            report["source_files"]
            == {name: sha(ROOT / name) for name in report["source_files"]},
        )
    report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "replay.sh").write_text(
        "#!/bin/sh\nset -eu\npython3 e2e/packaging/run.py --output .cache/package-e2e-replay"
        + ("".join(" --format " + name for name in (a.format or [])))
        + (" --lifecycle" if a.lifecycle else "")
        + "\n"
    )
    (out / "SHA256SUMS").write_text(
        sha(out / "report.json")
        + "  report.json\n"
        + sha(out / "replay.sh")
        + "  replay.sh\n"
    )
    print(out / "report.json")
