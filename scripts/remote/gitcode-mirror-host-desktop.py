#!/usr/bin/env python3
"""Runs the GitCode mirror upload for Desktop installers on a host inside mainland China.

Why: from a GitHub runner the upload to GitCode crawls (the object storage is in
China), so CI logs in to a small host in Shanghai over SSH and this wrapper does
the transfer there. It is the forced command of that SSH key, so the key can do
nothing else. This is the Desktop counterpart of Mobile's gitcode-mirror-host.py,
sharing the same host, the same unprivileged user and the same security model,
generalized to one release's several platform installers instead of one APK.

Protocol (stdin): exactly one JSON line and nothing else.
  {"tag", "version", "prerelease", "source", "scriptSha256",
   "artifacts": [{"filename", "sha256"}, ...], "env": {...}}

No code ever arrives over the SSH session. The upload script is installed on this
host by a maintainer (root-owned, see scripts/remote/deploy-gitcode-mirror-host-desktop.sh);
CI only says which version it expects (scriptSha256), and the wrapper refuses to run
a different one. So the key can start the mirror of a named release's already-built
installers and nothing more.

The wrapper
  1. validates the request (plain tag/version, a short list of known installer
     names, digests, allow-listed settings),
  2. downloads each artifact from the public R2 address, whose route from China is
     fast (GitHub is not), and checks size and SHA-256 against what CI released,
  3. runs the installed upload script with Node once for the whole batch, secrets
     only in the child's environment,
  4. prints the provenance between ::provenance:: markers and exits with the
     script's status. Everything it created is removed.

Python 3.6 compatible (the host's system Python).
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

R2_BASE = os.environ.get("UNICLIP_MIRROR_R2_BASE", "https://release.uniclipboard.app")
NODE = os.environ.get("UNICLIP_MIRROR_NODE", os.path.expanduser("~/node/bin/node"))
SCRIPT = os.environ.get(
    "UNICLIP_MIRROR_SCRIPT", "/opt/uniclip-mirror/mirror-desktop-installers-to-gitcode.mjs"
)
TMP_ROOT = os.environ.get("UNICLIP_MIRROR_TMP") or None
MAX_ARTIFACTS = 8

ALLOWED_ENV = {
    "GITCODE_TOKEN",
    "GITCODE_OWNER",
    "GITCODE_REPO",
    "GITCODE_API_BASE",
    "GITCODE_TARGET_COMMITISH",
    "FLARE_RELEASE_ACCESS_CLIENT_ID",
    "FLARE_RELEASE_ACCESS_CLIENT_SECRET",
    "FLARE_RELEASE_ADMIN_URL",
}
TAG = re.compile(r"^v[0-9][0-9A-Za-z.\-]{0,63}$")
VERSION = re.compile(r"^[0-9][0-9A-Za-z.\-]{0,63}$")
# Only the Tauri-updater-selected installer per platform, matching what
# scripts/build-flare-release-registration.js actually registers with
# FlareRelease today: macOS .app.tar.gz, Linux .AppImage(.tar.gz), Windows
# .nsis.zip/.exe. Case-insensitive product-name prefix because the bundler
# emits "uniclipboard_" for the macOS/Linux archives and "UniClipboard_" for
# the Windows ones.
FILENAME = re.compile(
    r"^(?i:uniclipboard)_[0-9A-Za-z_.\-]{1,60}\.(app\.tar\.gz|AppImage(?:\.tar\.gz)?|nsis\.zip|exe)$"
)
SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")
SOURCE = re.compile(r"^[0-9A-Za-z:/._\-]{1,200}$")


def fail(message):
    sys.stdout.write("mirror host: %s\n" % message)
    sys.stdout.flush()
    sys.exit(2)


def read_request():
    raw = sys.stdin.buffer.read()
    header, _, rest = raw.partition(b"\n")
    try:
        request = json.loads(header.decode("utf-8"))
    except ValueError:
        fail("the first line must be a JSON request")
    if rest.strip():
        fail("unexpected data after the request: only the request line is accepted")
    return request


def validate(request):
    tag = request.get("tag", "")
    version = request.get("version", "")
    script_digest = request.get("scriptSha256", "")
    source = request.get("source", "github-actions")
    artifacts = request.get("artifacts", [])
    if not isinstance(tag, str) or not TAG.match(tag):
        fail("invalid tag")
    if not isinstance(version, str) or not VERSION.match(version):
        fail("invalid version")
    if not isinstance(script_digest, str) or not SHA256.match(script_digest):
        fail("scriptSha256 must be 64 hex characters")
    if not isinstance(source, str) or not SOURCE.match(source):
        fail("invalid source")
    if not isinstance(artifacts, list) or not (1 <= len(artifacts) <= MAX_ARTIFACTS):
        fail("artifacts must be a list of 1 to %d entries" % MAX_ARTIFACTS)
    seen = set()
    checked_artifacts = []
    for entry in artifacts:
        if not isinstance(entry, dict):
            fail("each artifact must be an object")
        filename = entry.get("filename", "")
        digest = entry.get("sha256", "")
        if not isinstance(filename, str) or not FILENAME.match(filename):
            fail("invalid artifact file name: %r" % filename)
        if filename in seen:
            fail("duplicate artifact file name: %s" % filename)
        seen.add(filename)
        if not isinstance(digest, str) or not SHA256.match(digest):
            fail("artifact %s: sha256 must be 64 hex characters" % filename)
        checked_artifacts.append({"filename": filename, "sha256": digest.lower()})
    env = request.get("env", {})
    if not isinstance(env, dict):
        fail("env must be an object")
    for key, value in env.items():
        if key not in ALLOWED_ENV:
            fail("environment variable %s is not allowed" % key)
        if not isinstance(value, str) or "\n" in value or "\x00" in value:
            fail("environment variable %s has an invalid value" % key)
    return (
        tag,
        version,
        script_digest.lower(),
        source,
        bool(request.get("prerelease")),
        checked_artifacts,
        env,
    )


def download(url, target):
    started = time.time()
    code = subprocess.call(
        [
            "curl", "-4", "--fail", "--silent", "--show-error", "--location",
            "--retry", "3", "--retry-delay", "5",
            "--max-time", "1500", "--output", target, url,
        ]
    )
    if code != 0:
        fail("downloading %s failed (curl exit status %d)" % (url, code))
    size = os.path.getsize(target)
    seconds = max(time.time() - started, 0.001)
    print("Downloaded %d bytes from R2 in %.0fs (%.0f KiB/s)" % (size, seconds, size / 1024.0 / seconds))
    sys.stdout.flush()


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    request = read_request()
    tag, version, script_digest, source, prerelease, artifacts, env = validate(request)
    if not os.path.isfile(SCRIPT):
        fail("the upload script is not installed on this host (%s)" % SCRIPT)
    installed = sha256_of(SCRIPT)
    if installed != script_digest:
        fail(
            "the installed upload script is out of date (installed %s, CI expects scriptSha256 %s); "
            "run deploy-gitcode-mirror-host-desktop.sh" % (installed, script_digest)
        )
    work = tempfile.mkdtemp(prefix="gitcode-mirror-desktop-", dir=TMP_ROOT)
    os.chmod(work, 0o700)
    try:
        for artifact in artifacts:
            target = os.path.join(work, artifact["filename"])
            download(
                "%s/artifacts/%s/%s" % (R2_BASE.rstrip("/"), tag, artifact["filename"]),
                target,
            )
            actual = sha256_of(target)
            if actual != artifact["sha256"]:
                fail(
                    "sha256 of %s is %s, expected %s"
                    % (artifact["filename"], actual, artifact["sha256"])
                )
        registration = {
            "version": version,
            "tagName": tag,
            "prerelease": prerelease,
            "artifacts": [{"filename": artifact["filename"]} for artifact in artifacts],
        }
        registration_path = os.path.join(work, "registration.json")
        with open(registration_path, "w") as handle:
            json.dump(registration, handle)
        provenance = os.path.join(work, "provenance.json")
        child_env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": os.environ.get("HOME", "")}
        child_env.update(env)
        status = subprocess.call(
            [
                NODE, SCRIPT,
                "--registration", registration_path,
                "--artifacts-dir", work,
                "--tag", tag,
                "--source", source,
                "--missing-config", "fail",
                "--provenance", provenance,
            ],
            env=child_env,
            cwd=work,
        )
        if os.path.exists(provenance):
            with open(provenance) as handle:
                sys.stdout.write("::provenance::%s\n" % json.dumps(json.load(handle)))
        sys.stdout.flush()
        sys.exit(status)
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
