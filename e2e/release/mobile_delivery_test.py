#!/usr/bin/env python3
"""Fixture E2E tests for Android APK/R2/FlareRelease/GitHub delivery and iOS ASC profile fetching."""

import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import plistlib
import stat
import subprocess
import sys
import tempfile
import threading
from urllib.parse import parse_qs, unquote, urlparse
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))
import ios_profiles  # noqa: E402
import mobile  # noqa: E402


def make_fake_apk(path: Path, abis=("arm64-v8a",)):
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr("AndroidManifest.xml", b"binary-manifest-placeholder")
        for abi in abis:
            zf.writestr(f"lib/{abi}/libunic.so", b"\x7fELF-placeholder-" + abi.encode())


def write_executable(path: Path, body: str):
    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def test_mobile_candidate_apk(temp: Path):
    meta = temp / "products.toml"
    meta.write_text(
        'schema = 1\n'
        '[products.android]\n'
        'version = "3.0.0"\n'
        'build_number = 205\n'
        'targets = ["android-apk-arm64", "android-aab"]\n'
        'application_id = "app.uniclipboard.android"\n'
        'legacy_build_floor = 204\n'
        '[products.ios]\n'
        'version = "3.0.0"\n'
        'build_number = 205\n'
        'targets = ["ios-arm64"]\n'
        'application_id = "app.uniclipboard.UniClipboard"\n'
        'legacy_build_floor = 204\n',
        encoding="utf-8",
    )
    aapt2 = temp / "fake-aapt2"
    write_executable(
        aapt2,
        "#!/usr/bin/env bash\n"
        "echo \"package: name='app.uniclipboard.android' versionCode='205' versionName='3.0.0' compileSdkVersion='36'\"\n",
    )
    legacy_sha = "fac61745dc0903786fb9ede62a962b399f7348f0bb6f899b8332667591033b9c"
    apksigner = temp / "fake-apksigner"
    write_executable(
        apksigner,
        "#!/usr/bin/env bash\n"
        "if [ \"$1\" = \"lineage\" ]; then\n"
        f"  echo 'Signer #1 in lineage certificate SHA-256 digest: {legacy_sha}'\n"
        "  echo 'Signer #2 in lineage certificate SHA-256 digest: 743578a9932abe04e37865a3d5e4c54fb7f7ede27b69c88e7603453993d0dc22'\n"
        "  exit 0\n"
        "fi\n"
        "echo 'Verifies'\n"
        "echo 'Verified using v2 scheme (APK Signature Scheme v2): true'\n"
        "echo 'Verified using v3 scheme (APK Signature Scheme v3): true'\n",
    )
    cand_dir = temp / "android-cand"
    cand_dir.mkdir()
    apk = cand_dir / "unic-android-3.0.0-205-arm64-v8a.apk"
    make_fake_apk(apk, abis=("arm64-v8a",))

    cand_json = cand_dir / "candidate.json"
    mobile.candidate(
        "android",
        apk,
        cand_json,
        aapt2=str(aapt2),
        apksigner=str(apksigner),
        architecture="arm64-v8a",
        require_v3_rotation_from=legacy_sha,
        metadata_path=meta,
        published_at="2026-11-26T08:00:00Z",
        source_sha="a" * 40,
        fixture=True,
    )
    data = json.loads(cand_json.read_text(encoding="utf-8"))
    assert data["product"] == "android"
    assert data["format"] == "apk"
    assert data["architecture"] == "arm64-v8a"
    assert data["version"] == "3.0.0"
    assert data["build_number"] == 205
    assert data["artifact"]["name"] == apk.name

    # Reject mismatched legacy rotation lineage root.
    try:
        mobile.candidate(
            "android",
            apk,
            temp / "bad-lineage.json",
            aapt2=str(aapt2),
            apksigner=str(apksigner),
            architecture="arm64-v8a",
            require_v3_rotation_from="0" * 64,
            metadata_path=meta,
            source_sha="a" * 40,
            fixture=True,
        )
    except ValueError as err:
        assert "lineage" in str(err)
    else:
        raise AssertionError("expected lineage mismatch failure")

    # Reject APK containing unexpected ABI.
    bad_apk = temp / "bad.apk"
    make_fake_apk(bad_apk, abis=("arm64-v8a", "x86_64"))
    try:
        mobile.candidate(
            "android",
            bad_apk,
            temp / "bad.json",
            aapt2=str(aapt2),
            apksigner=str(apksigner),
            architecture="arm64-v8a",
            metadata_path=meta,
            source_sha="a" * 40,
            fixture=True,
        )
    except ValueError as err:
        assert "ABIs" in str(err)
    else:
        raise AssertionError("expected ABI mismatch failure")

    return cand_json, apk


def test_android_delivery_end_to_end(temp: Path, cand_json: Path, apk: Path):
    notes = temp / "android-notes.json"
    notes.write_text(json.dumps({"en": "Alpha English notes", "zh": "Alpha 中文说明"}), encoding="utf-8")

    objects: dict[str, bytes] = {}
    registrations: list[dict] = []
    channels = {
        "android/alpha": {"internalRevision": 0, "currentVersion": None, "currentBuildNumber": None, "release": None},
        "android/stable": {"internalRevision": 0, "currentVersion": None, "currentBuildNumber": None, "release": None},
    }
    git_refs: dict[str, str] = {}
    releases: list[dict] = []
    assets: dict[int, dict] = {}
    next_id = [100]

    def alloc_id():
        next_id[0] += 1
        return next_id[0]

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def send_json(self, code, payload):
            raw = json.dumps(payload).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def send_bytes(self, code, raw):
            self.send_response(code)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def read_body(self):
            length = int(self.headers.get("Content-Length", "0"))
            return self.rfile.read(length)

        def do_GET(self):
            parsed = urlparse(self.path)
            path = unquote(parsed.path)
            # S3 path-style: /uniclipboard-releases/<key>
            if path.startswith("/uniclipboard-releases/"):
                key = path[len("/uniclipboard-releases/"):]
                if key not in objects:
                    return self.send_bytes(404, b"<Error><Code>NoSuchKey</Code></Error>")
                return self.send_bytes(200, objects[key])
            # Public R2 origin read-back
            if path.startswith("/artifacts/"):
                key = path.lstrip("/")
                if key not in objects:
                    return self.send_json(404, {"error": "Not found"})
                return self.send_bytes(200, objects[key])
            if path in ("/api/contract", "/contract"):
                if path.startswith("/api/") and (
                    self.headers.get("CF-Access-Client-Id") != "fixture-id"
                    or self.headers.get("CF-Access-Client-Secret") != "fixture-secret"
                ):
                    return self.send_json(403, {"error": "forbidden"})
                return self.send_json(200, {
                    "buildIdentity": "product-version-build-v1",
                    "products": ["desktop", "android", "ios"],
                    "channels": ["alpha", "stable"],
                })
            if path.startswith("/api/channels/"):
                channel_id = unquote(path[len("/api/channels/"):])
                st = channels[channel_id]
                return self.send_json(200, {
                    "data": {
                        "id": channel_id,
                        "internalRevision": st["internalRevision"],
                        "currentVersion": st["currentVersion"],
                        "currentBuildNumber": st["currentBuildNumber"],
                    }
                })
            if path.startswith("/v1/android/") and path.endswith(".json"):
                ch = path[len("/v1/android/"):-len(".json")]
                st = channels["android/" + ch]
                if not st["release"]:
                    return self.send_json(404, {"error": "Build not found"})
                return self.send_json(200, st["release"])
            # GitHub API fixture
            if path.startswith("/repos/UniClipboard/UniClipboard/git/ref/tags/"):
                tag = unquote(path[len("/repos/UniClipboard/UniClipboard/git/ref/tags/"):])
                if tag not in git_refs:
                    return self.send_json(404, {"message": "Not Found"})
                return self.send_json(200, {"object": {"type": "commit", "sha": git_refs[tag]}})
            if path == "/repos/UniClipboard/UniClipboard/releases":
                return self.send_json(200, releases)
            if path.startswith("/repos/UniClipboard/UniClipboard/releases/assets/"):
                aid = int(path.rsplit("/", 1)[1])
                return self.send_bytes(200, assets[aid]["bytes"])
            if path.startswith("/repos/UniClipboard/UniClipboard/releases/") and path.endswith("/assets"):
                rid = int(path.split("/")[-2])
                items = [
                    {k: v for k, v in a.items() if k != "bytes"}
                    for a in assets.values()
                    if a["release_id"] == rid
                ]
                return self.send_json(200, items)
            if path.startswith("/repos/UniClipboard/UniClipboard/releases/"):
                rid = int(path.rsplit("/", 1)[1])
                rel = next(r for r in releases if r["id"] == rid)
                return self.send_json(200, rel)
            return self.send_json(404, {"error": f"unhandled GET {path}"})

        def do_PUT(self):
            path = unquote(urlparse(self.path).path)
            if path.startswith("/uniclipboard-releases/"):
                key = path[len("/uniclipboard-releases/"):]
                body = self.read_body()
                if self.headers.get("If-None-Match") == "*" and key in objects:
                    return self.send_bytes(412, b"<Error><Code>PreconditionFailed</Code></Error>")
                objects[key] = body
                self.send_response(200)
                self.send_header("ETag", f'"{hashlib.md5(body).hexdigest()}"')
                self.end_headers()
                return
            self.send_json(404, {"error": f"unhandled PUT {path}"})

        def do_POST(self):
            parsed = urlparse(self.path)
            path = unquote(parsed.path)
            raw = self.read_body()
            if path == "/api/releases/register":
                payload = json.loads(raw)
                assert payload["product"] == "android"
                assert payload["buildNumber"] == "205"
                assert payload["prerelease"] is True
                assert payload["artifacts"][0]["r2Key"] in objects
                rid = f"rel-{len(registrations) + 1}"
                registrations.append({"id": rid, **payload})
                return self.send_json(200, {"data": {"id": rid, "status": "ready"}})
            if path.startswith("/api/channels/") and path.endswith("/promote"):
                channel_id = unquote(path[len("/api/channels/"):-len("/promote")])
                payload = json.loads(raw)
                st = channels[channel_id]
                assert payload["expectedRevisions"][channel_id] == st["internalRevision"]
                reg = next(r for r in registrations if r["id"] == payload["releaseId"])
                st["internalRevision"] += 1
                st["currentVersion"] = reg["version"]
                st["currentBuildNumber"] = reg["buildNumber"]
                st["release"] = {
                    "product": "android",
                    "version": reg["version"],
                    "buildNumber": reg["buildNumber"],
                    "channel": channel_id.split("/")[1],
                    "artifacts": reg["artifacts"],
                }
                return self.send_json(200, {"data": {"id": channel_id, "internalRevision": st["internalRevision"]}})
            if path == "/repos/UniClipboard/UniClipboard/git/refs":
                payload = json.loads(raw)
                tag = payload["ref"].removeprefix("refs/tags/")
                git_refs[tag] = payload["sha"]
                return self.send_json(201, {"ref": payload["ref"], "object": {"type": "commit", "sha": payload["sha"]}})
            if path == "/repos/UniClipboard/UniClipboard/releases":
                payload = json.loads(raw)
                rid = alloc_id()
                rel = {
                    "id": rid,
                    "tag_name": payload["tag_name"],
                    "name": payload["name"],
                    "body": payload["body"],
                    "draft": payload["draft"],
                    "prerelease": payload["prerelease"],
                    "html_url": f"http://127.0.0.1/releases/{rid}",
                }
                releases.append(rel)
                return self.send_json(201, rel)
            if path.startswith("/repos/UniClipboard/UniClipboard/releases/") and path.endswith("/assets"):
                rid = int(path.split("/")[-2])
                name = parse_qs(parsed.query)["name"][0]
                aid = alloc_id()
                assets[aid] = {
                    "id": aid,
                    "release_id": rid,
                    "name": name,
                    "size": len(raw),
                    "digest": "sha256:" + hashlib.sha256(raw).hexdigest(),
                    "bytes": raw,
                }
                return self.send_json(201, {k: v for k, v in assets[aid].items() if k != "bytes"})
            return self.send_json(404, {"error": f"unhandled POST {path}"})

        def do_PATCH(self):
            path = unquote(urlparse(self.path).path)
            raw = self.read_body()
            if path.startswith("/repos/UniClipboard/UniClipboard/releases/"):
                rid = int(path.rsplit("/", 1)[1])
                payload = json.loads(raw)
                rel = next(r for r in releases if r["id"] == rid)
                rel.update(payload)
                return self.send_json(200, rel)
            return self.send_json(404, {"error": f"unhandled PATCH {path}"})

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f"http://127.0.0.1:{port}"
    env = {
        **os.environ,
        "AWS_ACCESS_KEY_ID": "fixture-key",
        "AWS_SECRET_ACCESS_KEY": "fixture-secret",
        "CF_ACCESS_CLIENT_ID": "fixture-id",
        "CF_ACCESS_CLIENT_SECRET": "fixture-secret",
        "GH_TOKEN": "fixture-only",
        "GH_ENTERPRISE_TOKEN": "fixture-only",
    }
    try:
        gh_out = temp / "gh-page"
        pub_receipt = temp / "publication.json"
        pro_receipt = temp / "promotion.json"

        # 1. Stage GitHub draft
        subprocess.run(
            [
                sys.executable, str(ROOT / "tools/release/android_release.py"),
                "github", "draft",
                "--candidate", str(cand_json),
                "--notes", str(notes),
                "--output", str(gh_out),
                "--channel", "alpha",
                "--api-base", base,
                "--fixture",
            ],
            check=True,
            env=env,
        )
        assert releases[0]["draft"] is True
        assert releases[0]["prerelease"] is True

        # 2. Publish APK to R2 + register with FlareRelease
        subprocess.run(
            [
                sys.executable, str(ROOT / "tools/release/android_release.py"),
                "publish-r2",
                "--candidate", str(cand_json),
                "--notes", str(notes),
                "--endpoint", base,
                "--admin", base,
                "--public", base,
                "--channel", "alpha",
                "--receipt", str(pub_receipt),
                "--fixture",
            ],
            check=True,
            env=env,
        )
        pub = json.loads(pub_receipt.read_text())
        assert pub["status"] == "available"
        assert "artifacts/android/3.0.0/build.205/" + apk.name in objects
        assert "artifacts/android/3.0.0/build.205/publication.json" in objects

        # 3. Promote android/alpha and verify public feed read-back
        subprocess.run(
            [
                sys.executable, str(ROOT / "tools/release/android_release.py"),
                "promote",
                "--candidate", str(cand_json),
                "--publication", str(pub_receipt),
                "--admin", base,
                "--public", base,
                "--channel", "alpha",
                "--expected-revision", "auto",
                "--forward-only",
                "--receipt", str(pro_receipt),
                "--fixture",
            ],
            check=True,
            env=env,
        )
        assert json.loads(pro_receipt.read_text())["status"] == "promoted"

        # 4. Publish GitHub prerelease
        subprocess.run(
            [
                sys.executable, str(ROOT / "tools/release/android_release.py"),
                "github", "publish",
                "--candidate", str(cand_json),
                "--notes", str(notes),
                "--output", str(gh_out),
                "--channel", "alpha",
                "--publication", str(pub_receipt),
                "--promotion", str(pro_receipt),
                "--api-base", base,
                "--fixture",
            ],
            check=True,
            env=env,
        )
        assert releases[0]["draft"] is False
        assert releases[0]["prerelease"] is True
        assert {a["name"] for a in assets.values()} == {
            apk.name,
            "candidate.json",
            "android-notes.json",
            "release-page.json",
            "SHA256SUMS.txt",
        }
    finally:
        server.shutdown()


def test_ios_asc_profiles(temp: Path):
    # Generate a disposable P-256 private key using Ruby stdlib OpenSSL.
    key_path = temp / "AuthKey.p8"
    subprocess.run(
        [
            "ruby", "-ropenssl", "-e",
            f"k = OpenSSL::PKey::EC.generate('prime256v1'); File.write({str(key_path)!r}, k.private_to_pem)",
        ],
        check=True,
    )
    pem_text = key_path.read_text(encoding="utf-8")
    assert ios_profiles.decode_private_key(pem_text) == ios_profiles.decode_private_key(
        base64.b64encode(pem_text.encode("utf-8")).decode("ascii")
    )

    def make_profile_bytes(uuid: str, name: str, bundle: str, groups: list[str]) -> str:
        raw = plistlib.dumps({
            "UUID": uuid,
            "Name": name,
            "TeamIdentifier": ["8XG39X5CL8"],
            "Entitlements": {
                "application-identifier": "8XG39X5CL8." + bundle,
                "com.apple.security.application-groups": groups,
            },
        })
        return base64.b64encode(raw).decode("ascii")

    app = "app.uniclipboard.UniClipboard"
    existing_builds = []

    class ASCHandler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def send_json(self, code, payload):
            raw = json.dumps(payload).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_GET(self):
            auth = self.headers.get("Authorization", "")
            assert auth.startswith("Bearer ") and auth.count(".") == 2
            parsed = urlparse(self.path)
            if parsed.path == "/v1/profiles":
                return self.send_json(200, {
                    "included": [
                        {"type": "bundleIds", "id": "b-app", "attributes": {"identifier": app}},
                        {"type": "bundleIds", "id": "b-share", "attributes": {"identifier": app + ".Share"}},
                        {"type": "bundleIds", "id": "b-kbd", "attributes": {"identifier": app + ".Keyboard"}},
                    ],
                    "data": [
                        {
                            "id": "p-app",
                            "attributes": {
                                "name": "UniClipboard App Store",
                                "expirationDate": "2027-01-01T00:00:00Z",
                                "profileContent": make_profile_bytes(
                                    "11111111-1111-1111-1111-111111111111",
                                    "UniClipboard App Store",
                                    app,
                                    [f"group.{app}.incoming-share", f"group.{app}"],
                                ),
                            },
                            "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": "b-app"}}},
                        },
                        {
                            "id": "p-share",
                            "attributes": {
                                "name": "UniClipboard Share App Store",
                                "expirationDate": "2027-01-01T00:00:00Z",
                                "profileContent": make_profile_bytes(
                                    "22222222-2222-2222-2222-222222222222",
                                    "UniClipboard Share App Store",
                                    app + ".Share",
                                    [f"group.{app}.incoming-share"],
                                ),
                            },
                            "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": "b-share"}}},
                        },
                        {
                            "id": "p-kbd",
                            "attributes": {
                                "name": "UniClipboard Keyboard App Store",
                                "expirationDate": "2027-01-01T00:00:00Z",
                                "profileContent": make_profile_bytes(
                                    "33333333-3333-3333-3333-333333333333",
                                    "UniClipboard Keyboard App Store",
                                    app + ".Keyboard",
                                    [f"group.{app}.incoming-share"],
                                ),
                            },
                            "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": "b-kbd"}}},
                        },
                    ],
                })
            if parsed.path == "/v1/apps":
                return self.send_json(200, {"data": [{"id": "app-1", "attributes": {"bundleId": app}}]})
            if parsed.path == "/v1/builds":
                return self.send_json(200, {"data": existing_builds})
            return self.send_json(404, {"error": parsed.path})

    server = ThreadingHTTPServer(("127.0.0.1", 0), ASCHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base = f"http://127.0.0.1:{port}"
    dest = temp / "profiles-out"
    try:
        result = ios_profiles.fetch_asc_profiles(
            "KEY123",
            "ISSUER456",
            key_path,
            api_base=base,
            destinations=[dest],
            decode_fn=lambda p: plistlib.loads(p.read_bytes()),
        )
        assert result["team"] == "8XG39X5CL8"
        assert set(result["profiles"]) == {app, app + ".Share", app + ".Keyboard"}
        assert len(list(dest.glob("*.mobileprovision"))) == 3

        avail = ios_profiles.check_asc_build("KEY123", "ISSUER456", key_path, api_base=base)
        assert avail["available"] is True

        existing_builds.append({"id": "build-205"})
        try:
            ios_profiles.check_asc_build("KEY123", "ISSUER456", key_path, api_base=base)
        except ValueError as err:
            assert "already exists" in str(err)
        else:
            raise AssertionError("expected duplicate ASC build rejection")
    finally:
        server.shutdown()


def main():
    with tempfile.TemporaryDirectory(prefix="unic-mobile-release-e2e-") as temporary:
        temp = Path(temporary)
        cand_json, apk = test_mobile_candidate_apk(temp)
        test_android_delivery_end_to_end(temp, cand_json, apk)
        test_ios_asc_profiles(temp)
    print("PASS: mobile release fixture tests (Android APK/R2/FlareRelease/GitHub + iOS ASC profiles)")


if __name__ == "__main__":
    main()
