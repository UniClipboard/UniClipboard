#!/usr/bin/env python3
"""Transport private, CAS-only MBX snapshots; MBX owns closure validation."""

import argparse
import hashlib
import json
import os
import platform
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import time
import tomllib
from datetime import datetime, timedelta, timezone
from pathlib import Path, PurePosixPath


BUCKET = "uniclipboard-build-cache"
PREFIX = "desktop/mbx-snapshots/v1"
MANIFEST = "mbx-cache-export-v1.json"
MAX_ARCHIVE_BYTES = 16 * 1024**3
MAX_EXPANDED_BYTES = 40 * 1024**3
MAX_MEMBERS = 200_000
WINDOWS_DEVICES = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} | {
    f"{prefix}{digit}" for prefix in ("COM", "LPT") for digit in "123456789¹²³"
}
ROOT = Path(os.environ.get("UC_MBX_SNAPSHOT_SOURCE_ROOT", Path(__file__).resolve().parents[2])).resolve()
SCENES = {
    "release-sidecar": "scripts/stage-daemon.mjs",
    "e2e-macos": ".github/actions/build-e2e-rust/action.yml",
    "pr-check-linux": ".github/actions/build-pr-rust/action.yml",
    "quick-panel-windows": ".github/actions/build-pr-rust/action.yml",
}


def digest_file(file):
    with open(file, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def release_context(target, component, build_mode, target_args):
    targets = {
        "aarch64-apple-darwin", "x86_64-apple-darwin",
        "aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc",
        "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu",
    }
    if (target not in targets or component not in ("daemon", "helper")
            or build_mode not in ("release", "test")
            or target_args not in ("", f"--target {target}")
            or (component == "helper" and "linux" in target)):
        raise ValueError("Invalid release snapshot context")
    target_mode = "explicit" if target_args else "native"
    return dict(target=target, component=component, build_mode=build_mode, target_mode=target_mode)


def export_group(scene, context=None):
    if scene != "release-sidecar":
        return scene
    if context is None:
        raise ValueError("Release snapshot requires a build context")
    return "sidecar-" + "-".join(context[field] for field in ("target", "component", "build_mode", "target_mode"))


def release_seed_input(name, raw):
    """Select a reusable dependency baseline; native MBX still validates full inputs."""
    if name not in ("Cargo.toml", "Cargo.lock"):
        return raw
    value = tomllib.loads(raw.decode())
    if name == "Cargo.toml":
        for package in (value.get("package", {}), value.get("workspace", {}).get("package", {})):
            if "version" in package:
                package["version"] = "<local-version>"
    else:
        packages = value.get("package", [])
        local_versions = {(package["name"], package["version"]) for package in packages if "source" not in package}
        for package in packages:
            if "source" not in package:
                package["version"] = "<local-version>"
            # Cargo adds a version to dependency references only when needed to disambiguate.
            package["dependencies"] = [
                f"{parts[0]} <local-version>" if len(parts := dependency.split()) == 2 and tuple(parts) in local_versions else dependency
                for dependency in package.get("dependencies", [])
            ]
    return json.dumps(value, sort_keys=True, separators=(",", ":"), default=str).encode()


def scene_key(scene, context=None):
    group = export_group(scene, context)
    digest = hashlib.sha256()
    names = ["Cargo.lock", "Cargo.toml", ".cargo/config.toml", "rust-toolchain.toml",
             "scripts/build-cache/engine-tools.sh", "scripts/ci/mbx-snapshot.py", SCENES[scene]]
    if scene == "release-sidecar":
        names.extend(("scripts/ci/configure-build-mode.mjs", "scripts/ci/resolve-app-env.mjs"))
        # Channels and local package versions are deliberately coarse seed inputs.
        # MBX owns artifact validity, including APP_ENV and CARGO_PKG_VERSION.
        flags = {key: value for key, value in os.environ.items()
                 if key.startswith("CARGO_PROFILE_") or key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")}
        digest.update(json.dumps(dict(context=context, flags=flags), sort_keys=True).encode())
    for name in names:
        # Release automation may run from RUNNER_TEMP against a historical source checkout.
        raw = (Path(__file__) if name == "scripts/ci/mbx-snapshot.py" else ROOT / name).read_bytes()
        if scene == "release-sidecar":
            raw = release_seed_input(name, raw)
        digest.update(name.encode() + b"\0" + raw + b"\0")
    if scene == "pr-check-linux" or (scene == "release-sidecar" and "linux" in context["target"]):
        digest.update((ROOT / ".github/docker/build-bookworm.Dockerfile").read_bytes())
        digest.update(Path("/etc/os-release").read_bytes())
    digest.update(subprocess.check_output(["rustc", "-vV"]))
    digest.update(f"{platform.system()}-{platform.machine()}-{group}".encode())
    return f"{PREFIX}/{group}/{digest.hexdigest()}"


def mbx(*args, cache=None):
    environment = os.environ.copy()
    if cache is not None:
        environment["MBX_CACHE_DIR"] = str(cache)
    # Windows CreateProcess searches System32 before PATH for bare executables;
    # its bash.exe is the WSL launcher, not the Git Bash running this workflow.
    bash = (subprocess.check_output(["cygpath", "-w", "/bin/bash"], text=True).strip()
            if os.name == "nt" else "bash")
    subprocess.run(
        [bash, "scripts/build-cache/mbx.sh", "--exec", "mbx", *args],
        cwd=ROOT, env=environment, check=True,
    )


def client():
    import boto3
    from botocore.config import Config

    return boto3.client(
        "s3", endpoint_url=os.environ["BUILD_CACHE_R2_ENDPOINT"],
        aws_access_key_id=os.environ["BUILD_CACHE_R2_ACCESS_KEY_ID"],
        aws_secret_access_key=os.environ["BUILD_CACHE_R2_SECRET_ACCESS_KEY"],
        region_name="auto",
        config=Config(connect_timeout=10, read_timeout=120,
                      s3={"addressing_style": "path"},
                      request_checksum_calculation="when_required",
                      response_checksum_validation="when_required",
                      retries={"total_max_attempts": 3, "mode": "standard"}),
    )


def get_index(s3, key):
    try:
        response = s3.get_object(Bucket=BUCKET, Key=f"{key}/latest.json")
    except Exception as error:
        from botocore.exceptions import ClientError

        if isinstance(error, ClientError) and error.response["Error"]["Code"] in ("NoSuchKey", "404"):
            return None
        raise
    with response["Body"] as body:
        raw = body.read(16 * 1024 + 1)
    if len(raw) > 16 * 1024:
        raise ValueError("Snapshot index exceeds limit")
    return validate_index(json.loads(raw), key)


def validate_index(value, key):
    sha = value.get("sha256", "")
    size = value.get("size", 0)
    if (value.get("version") != 1 or len(sha) != 64
            or any(c not in "0123456789abcdef" for c in sha)
            or not isinstance(size, int) or not 0 < size <= MAX_ARCHIVE_BYTES
            or value.get("key") != f"{key}/{sha}.tar.gz"):
        raise ValueError("Invalid snapshot index")
    return value


def unpack(archive, destination):
    """Accept only regular MBX bundle members, never target state or links."""
    total = 0
    seen = set()
    with tarfile.open(archive, "r:gz") as source:
        for entry in source:
            name = PurePosixPath(entry.name)
            if (name.is_absolute() or ".." in name.parts or entry.name in seen
                    or not name.parts or "\\" in entry.name
                    or any(any(c in part for c in ':<>"|?*') or part.endswith((".", " "))
                           or part.split(".")[0].upper() in WINDOWS_DEVICES
                           for part in name.parts)
                    or entry.size < 0 or not (entry.isfile() or entry.isdir())):
                raise ValueError("Unsafe snapshot member")
            seen.add(entry.name)
            if len(seen) > MAX_MEMBERS:
                raise ValueError("Too many snapshot members")
            if name.parts[0] not in (MANIFEST, "cas", "action-results"):
                raise ValueError("Unexpected snapshot member")
            total += entry.size
            if total > MAX_EXPANDED_BYTES:
                raise ValueError("Expanded snapshot exceeds limit")
            target = destination.joinpath(*name.parts)
            if entry.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(entry) as incoming, open(target, "xb") as output:
                    shutil.copyfileobj(incoming, output)
    manifest = json.loads((destination / MANIFEST).read_text())
    if manifest.get("attachments") or manifest.get("objects"):
        raise ValueError("Snapshot must not carry Cargo workspace state")


def compact_bundle(exported, destination):
    """Remove optional attachments, then let native import select the closure."""
    manifest = json.loads((exported / MANIFEST).read_text())
    if manifest.get("version") not in (1, 2):
        raise ValueError("Unsupported MBX export format")
    manifest.pop("attachments", None)
    manifest.pop("objects", None)
    manifest["version"] = 1
    encoded = json.dumps(manifest).encode()
    (exported / MANIFEST).write_bytes(encoded)
    isolated = destination.parent / "isolated-cache"
    isolated.mkdir()
    # Native import verifies every required blob before publishing it, discards
    # unreferenced attachment objects, and consumes the temporary directory.
    mbx("cache", "import", str(exported), cache=isolated)
    destination.mkdir()
    (destination / MANIFEST).write_bytes(encoded)
    for name in ("cas", "action-results"):
        source = isolated / "actions" / name
        if source.exists():
            shutil.move(str(source), destination / name)


def restore(s3, key, temporary, report, object_fallback):
    report["index_reads"] += 1
    index = get_index(s3, key)
    if index is None:
        report["status"] = "miss"
        return
    archive = temporary / "snapshot.tar.gz"
    report["archive_reads"] += 1
    response = s3.get_object(Bucket=BUCKET, Key=index["key"])
    if response["ContentLength"] != index["size"]:
        response["Body"].close()
        raise ValueError("Snapshot size mismatch")
    with response["Body"] as body, open(archive, "wb") as output:
        while chunk := body.read(1024 * 1024):
            report["downloaded_bytes"] += len(chunk)
            if report["downloaded_bytes"] > index["size"]:
                raise ValueError("Snapshot exceeds declared size")
            output.write(chunk)
    if archive.stat().st_size != index["size"] or digest_file(archive) != index["sha256"]:
        raise ValueError("Snapshot checksum mismatch")
    bundle = temporary / "bundle"
    bundle.mkdir()
    unpack(archive, bundle)
    mbx("cache", "import", str(bundle))
    report.update(status="hit", source_sha=index.get("source_sha"), object_fallback=object_fallback)
    if not object_fallback:
        # Keep the Engine identity immutable for snapshot publication. Native
        # Cargo commands load a separate policy containing only a source path.
        config = Path(os.environ.get("UC_MBX_SNAPSHOT_ENV_FILE", os.environ["UC_ENGINE_MBX_ENV_FILE"]))
        policy = f"source {shlex.quote(config.as_posix())}\n"
        policy += "unset BUILD_CACHE_R2_ENDPOINT BUILD_CACHE_R2_ACCESS_KEY_ID BUILD_CACHE_R2_SECRET_ACCESS_KEY\n"
        policy += "unset MBX_REMOTE_URL MBX_REMOTE_NAMESPACE MBX_REMOTE_S3_ENDPOINT MBX_REMOTE_S3_REGION MBX_REMOTE_MODE\n"
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n", prefix="mbx-local-policy-", dir=config.parent, delete=False) as output:
            replacement = Path(output.name)
            try:
                output.write(policy)
                output.close()
                with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as commands:
                    commands.write(f"UC_MBX_SNAPSHOT_ENV_FILE={config.as_posix()}\nUC_ENGINE_MBX_ENV_FILE={replacement.as_posix()}\n")
            except BaseException:
                output.close()
                replacement.unlink(missing_ok=True)
                raise


def save(s3, key, temporary, report, scene, *, source_sha=None):
    if (os.environ.get("GITHUB_REF") != "refs/heads/main"
            or os.environ.get("GITHUB_REF_PROTECTED") != "true"
            or os.environ.get("GITHUB_EVENT_NAME") not in ("push", "schedule", "workflow_dispatch")):
        raise ValueError("Snapshot publication requires trusted protected main")
    exported = temporary / "exported"
    mbx("cache", "export", "--group", scene, "--format", "directory", str(exported))
    bundle = temporary / "bundle"
    compact_bundle(exported, bundle)
    archive = temporary / "snapshot.tar.gz"
    with tarfile.open(archive, "w:gz", compresslevel=1) as output:
        for member in sorted(bundle.iterdir()):
            output.add(member, arcname=member.name)
    size = archive.stat().st_size
    if not 0 < size <= MAX_ARCHIVE_BYTES:
        raise ValueError("Snapshot archive exceeds limit")
    sha = digest_file(archive)
    index = {"version": 1, "sha256": sha, "size": size,
             "key": f"{key}/{sha}.tar.gz", "source_sha": source_sha or os.environ["GITHUB_SHA"]}
    # Publish the index last so a reader never sees a partially uploaded bundle.
    s3.upload_file(str(archive), BUCKET, index["key"])
    s3.put_object(Bucket=BUCKET, Key=f"{key}/latest.json",
                  Body=json.dumps(index).encode(), ContentType="application/json")
    report.update(status="saved", uploaded_bytes=size, source_sha=index["source_sha"])


def prune_snapshots(s3, scene, report):
    """Expire only this scene's old objects, independently of the MBX bucket."""
    cutoff = datetime.now(timezone.utc) - timedelta(days=14)
    paginator = s3.get_paginator("list_objects_v2")
    report.update(list_requests=0, expired_objects=0)
    for page in paginator.paginate(Bucket=BUCKET, Prefix=f"{PREFIX}/{scene}/"):
        report["list_requests"] += 1
        expired = [{"Key": item["Key"]} for item in page.get("Contents", [])
                   if item["LastModified"] < cutoff]
        if expired:
            response = s3.delete_objects(Bucket=BUCKET, Delete={"Objects": expired, "Quiet": True})
            if response.get("Errors"):
                raise RuntimeError("Snapshot retention deletion failed")
            report["expired_objects"] += len(expired)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("restore", "save"))
    parser.add_argument("--scene", choices=tuple(SCENES), default="e2e-macos")
    parser.add_argument("--object-fallback", choices=("true", "false"), default="false")
    parser.add_argument("--target", default="")
    parser.add_argument("--component", default="")
    parser.add_argument("--build-mode", default="release")
    parser.add_argument("--target-args", default="")
    args = parser.parse_args()
    evidence = Path(os.environ["UC_ENGINE_MBX_STATS_DIR"])
    evidence.mkdir(parents=True, exist_ok=True)
    report = {"operation": args.command, "scene": args.scene, "status": "pending", "index_reads": 0,
              "archive_reads": 0, "downloaded_bytes": 0}
    started = time.monotonic()
    try:
        context = None
        source_sha = None
        if args.scene == "release-sidecar":
            context = release_context(args.target, args.component, args.build_mode, args.target_args)
            source_sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
            if args.command == "save" and source_sha != os.environ.get("GITHUB_SHA"):
                raise ValueError("Release publisher must build the protected workflow source")
            report["context"] = context
        group = export_group(args.scene, context)
        if context is not None:
            # The restore step configures receipts even on a cold snapshot miss.
            with open(os.environ["GITHUB_ENV"], "a") as output:
                output.write(f"MBX_CACHE_EXPORT_GROUP={group}\n")
        with tempfile.TemporaryDirectory(prefix="mbx-snapshot-", dir=os.environ["RUNNER_TEMP"]) as directory:
            s3 = client()
            key = scene_key(args.scene, context)
            if args.command == "restore":
                restore(s3, key, Path(directory), report, args.object_fallback == "true")
            else:
                save(s3, key, Path(directory), report, group, source_sha=source_sha)
                try:
                    prune_snapshots(s3, group, report)
                except Exception as error:
                    report["retention_error_type"] = type(error).__name__
                    print(f"::warning::MBX snapshot retention failed ({type(error).__name__})")
    except Exception as error:
        # SDK exceptions can include signed URLs and endpoints. Keep those out
        # of evidence while making the failed optimization visible in CI.
        report.update(status="error", error_type=type(error).__name__)
        print(f"::warning::MBX snapshot {args.command} failed ({type(error).__name__}); ordinary cache policy retained")
    report["duration_seconds"] = round(time.monotonic() - started, 3)
    path = evidence / f"snapshot-{args.command}.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
