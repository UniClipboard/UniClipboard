#!/usr/bin/env python3
"""Build and execute actual Nix, Flatpak or strict Snap packages on disposable CI hosts."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import time
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "tools/package"))
import desktop  # noqa: E402 - repository tool path

p = argparse.ArgumentParser()
p.add_argument("--input", required=True)
p.add_argument("--kind", choices=["nix", "flatpak", "snap"], required=True)
p.add_argument("--output", default=str(ROOT / ".cache/sandbox-e2e"))
a = p.parse_args()
if os.environ.get("GITHUB_ACTIONS") != "true":
    p.error("Use only an ephemeral GitHub-hosted acceptance runner")
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
arch = {"aarch64": "arm64", "x86_64": "amd64"}[platform.machine()]
report = {
    "status": "running",
    "kind": a.kind,
    "target": "linux-" + arch,
    "checks": [],
    "scope": "native sandbox construction and executable/service identity; GUI and store submission not run",
    "commands": [],
}


def run(argv, **kwargs):
    r = subprocess.run(
        list(map(str, argv)),
        cwd=kwargs.pop("cwd", ROOT),
        text=True,
        capture_output=True,
        timeout=2400,
        **kwargs
    )
    report["commands"].append(
        {
            "argv": list(map(str, argv)),
            "exit_code": r.returncode,
            "output": (r.stdout + r.stderr)[-3000:],
        }
    )
    if r.returncode:
        raise RuntimeError((r.stdout + r.stderr)[-3000:])
    return r.stdout.strip()


try:
    report["harness_sha256"] = desktop.sha(Path(__file__))
    report["source_sha"] = run(
        ["git", "-c", "safe.directory=" + str(ROOT), "rev-parse", "HEAD"]
    )
    with tempfile.TemporaryDirectory(prefix="unic-sandbox-e2e-") as temporary:
        temp = Path(temporary)
        selected = None
        for path in Path(a.input).rglob("artifacts.json"):
            m = json.loads(path.read_text())
            if m["target"] == "linux-" + arch:
                for item in m["artifacts"]:
                    if item["format"] == "deb":
                        if selected is not None:
                            raise ValueError("duplicate DEB input")
                        selected = desktop.cli.checked_file(
                            path.parent, item["file"], item["sha256"]
                        ).resolve()
                        expected_cli = m["cli"]["binaries_sha256"]
                        report["input_sha256"] = item["sha256"]
                        report["package_source_sha"] = m["source_sha"]
        if selected is None:
            raise ValueError("missing native DEB input")
        definitions = temp / "definitions"
        run(
            [
                sys.executable,
                ROOT / "tools/package/channels.py",
                "--input",
                Path(a.input).resolve(),
                "--output",
                definitions,
                "--channel",
                a.kind,
                "--base-url",
                "https://release.uniclipboard.app/artifacts/desktop/acceptance/",
            ]
        )
        report["definitions_sha256"] = {
            str(f.relative_to(definitions)): desktop.sha(f)
            for f in definitions.rglob("*")
            if f.is_file()
        }
        identity = None
        if a.kind == "nix":
            expression = temp / "acceptance.nix"
            # Only source acquisition is overridden. Build and runtime hooks are unchanged.
            expression.write_text(
                "{ pkgs ? import <nixpkgs> {} }: (pkgs.callPackage "
                + str(definitions / "nix/package.nix")
                + " {}).overrideAttrs (_: { src = "
                + str(selected)
                + "; })\n"
            )
            result = temp / "result"
            run(["nix-build", expression, "--out-link", result])
            identity = json.loads(
                run([result / "bin/uniclipboard", "--version", "--json"])
            )
            private = result / "lib/uniclipboard"
            for name, digest in expected_cli.items():
                if desktop.sha(private / name) != digest:
                    raise ValueError("Nix changed the locked CLI pair")
            report["private_cli_sha256"] = expected_cli
            report["derived_desktop_sha256"] = desktop.sha(private / "unic-desktop")
            profile = temp / "profile"
            profile.mkdir()
            (profile / "config.toml").write_text(
                "[sync]\nenabled=false\n[telemetry]\nenabled=false\n"
            )
            run(
                [private / "unic", "status", "--json"],
                env=dict(os.environ, UNIC_HOME=str(profile)),
            )
            deadline = time.monotonic() + 15
            while (profile / "unic.sock").exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            if (profile / "unic.sock").exists():
                raise ValueError("Nix private service did not stop")
            report["checks"].append(
                "locked Nix CLI pair, real IPC and temporary shutdown verified"
            )
            report["checks"].append(
                "Nix native derivation built and executable wrapper ran"
            )
        elif a.kind == "flatpak":
            directory = definitions / "flatpak"
            manifest = directory / (desktop.APP_ID + ".json")
            config = json.loads(manifest.read_text())
            # Preserve the generated digest while acquiring the exact candidate locally.
            for source in config["modules"][0]["sources"]:
                if source.get("only-arches") == [
                    {"amd64": "x86_64", "arm64": "aarch64"}[arch]
                ]:
                    del source["url"]
                    source["path"] = str(selected)
            manifest.write_text(json.dumps(config))
            run(
                [
                    "flatpak-builder",
                    "--user",
                    "--install-deps-from=flathub",
                    "--force-clean",
                    "--repo=" + str(temp / "repo"),
                    temp / "build",
                    manifest,
                ]
            )
            run(
                [
                    "flatpak",
                    "--user",
                    "remote-add",
                    "--no-gpg-verify",
                    "unic-acceptance",
                    temp / "repo",
                ]
            )
            try:
                run(
                    [
                        "flatpak",
                        "--user",
                        "install",
                        "-y",
                        "unic-acceptance",
                        desktop.APP_ID,
                    ]
                )
                identity = json.loads(
                    run(["flatpak", "run", desktop.APP_ID, "--version", "--json"])
                )
                run(
                    [
                        "flatpak",
                        "run",
                        "--command=/app/lib/uniclipboard/unic",
                        desktop.APP_ID,
                        "--version",
                        "--json",
                    ]
                )
                profile = Path.home() / ".var/app" / desktop.APP_ID / "data/acceptance"
                profile.mkdir(parents=True)
                try:
                    (profile / "config.toml").write_text(
                        "[sync]\nenabled=false\n[telemetry]\nenabled=false\n"
                    )
                    run(
                        [
                            "flatpak",
                            "run",
                            "--env=UNIC_HOME=" + str(profile),
                            "--command=/app/lib/uniclipboard/unic",
                            desktop.APP_ID,
                            "status",
                            "--json",
                        ]
                    )
                    deadline = time.monotonic() + 15
                    while (
                        profile / "unic.sock"
                    ).exists() and time.monotonic() < deadline:
                        time.sleep(0.05)
                    if (profile / "unic.sock").exists():
                        raise ValueError("Flatpak private service did not stop")
                    report["checks"].append(
                        "real private CLI/service IPC and shutdown inside Flatpak"
                    )
                finally:
                    shutil.rmtree(profile)
                report["checks"].append(
                    "Flatpak bundle built, installed and ran inside bubblewrap"
                )
            finally:
                run(["flatpak", "--user", "uninstall", "-y", desktop.APP_ID])
                run(["flatpak", "--user", "remote-delete", "unic-acceptance"])
        else:
            directory = definitions / "snap" / arch
            config = json.loads((directory / "snapcraft.yaml").read_text())
            config["parts"]["uniclipboard"]["source"] = str(selected)
            (directory / "snapcraft.yaml").unlink()
            (directory / "snap").mkdir()
            (directory / "snap/snapcraft.yaml").write_text(json.dumps(config))
            run(["snapcraft", "pack", "--destructive-mode"], cwd=directory)
            files = list(directory.glob("*.snap"))
            if len(files) != 1:
                raise ValueError("expected one native Snap artifact")
            run(["sudo", "snap", "install", "--dangerous", files[0]])
            try:
                run(["sudo", "snap", "connect", "uniclipboard:network"])
                run(["sudo", "snap", "connect", "uniclipboard:network-bind"])
                identity = json.loads(
                    run(["snap", "run", "uniclipboard", "--version", "--json"])
                )
                script = """set -e
mkdir -p "$UNIC_HOME"
printf '[sync]\\nenabled=false\\n[telemetry]\\nenabled=false\\n' > "$UNIC_HOME/config.toml"
"$SNAP/usr/lib/uniclipboard/unic" status --json
for attempt in {1..150}; do
  if [ ! -e "$UNIC_HOME/unic.sock" ]; then exit 0; fi
  sleep 0.1
done
exit 1
"""
                run(["snap", "run", "--shell", "uniclipboard"], input=script)
                report["checks"].append(
                    "real private CLI/service IPC and shutdown under strict Snap"
                )
                report["checks"].append(
                    "strict Snap built, installed and ran under snapd confinement"
                )
                report["snap_sha256"] = desktop.sha(files[0])
            finally:
                run(["sudo", "snap", "remove", "--purge", "uniclipboard"])
        if (
            identity.get("product") != "desktop"
            or identity.get("source_sha") != report["package_source_sha"]
        ):
            raise ValueError("sandbox executable differs from accepted source")
        report["identity"] = identity
        report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error_type"] = type(error).__name__
    raise
finally:
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "SHA256SUMS").write_text(
        "".join(
            desktop.sha(f) + "  " + f.name + "\n"
            for f in sorted(out.iterdir())
            if f.name != "SHA256SUMS"
        )
    )
print(out / "report.json")
