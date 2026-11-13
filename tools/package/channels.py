#!/usr/bin/env python3
"""Render channel-native definitions from verified immutable Desktop artifacts."""

import argparse
import base64
from datetime import date
import json
from pathlib import Path
import re
import sys
from urllib.parse import quote, urlparse

sys.dont_write_bytecode = True
from desktop import APP_ID, cli, fresh_output, sha  # noqa: E402

CHANNELS = [
    "homebrew-tap",
    "homebrew-cask",
    "scoop",
    "chocolatey",
    "winget",
    "aur",
    "pacman",
    "copr",
    "snap",
    "nix",
    "flatpak",
    "npm",
]


def render(args):
    base = urlparse(args.base_url)
    if (
        base.scheme != "https"
        or not base.hostname
        or base.username
        or base.password
        or base.query
        or base.fragment
        or not re.fullmatch(
            r"https://[A-Za-z0-9.-]+(?::[0-9]+)?/[A-Za-z0-9._~%/-]*", args.base_url
        )
    ):
        raise ValueError("base URL must be a fixed HTTPS artifact directory")
    records = {}
    identity = None
    for path in sorted(Path(args.input).rglob("artifacts.json")):
        m = cli.read_json(path)
        if m.get("schema") != 1 or m.get("product") != "desktop":
            raise ValueError("invalid Desktop artifact record")
        key = (
            m["version"],
            m["build_number"],
            m["source_sha"],
            m["cli"]["manifest_sha256"],
        )
        if identity is not None and identity != key:
            raise ValueError("mixed Desktop or CLI identities")
        identity = key
        if not args.development and (m.get("development") or not m.get("source_clean")):
            raise ValueError("public recipes require clean production inputs")
        for artifact in m["artifacts"]:
            if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,254}", artifact["file"]):
                raise ValueError("unsafe artifact filename")
            slot = (m["target"], artifact["format"])
            if slot in records:
                raise ValueError("duplicate target/format")
            cli.checked_file(path.parent, artifact["file"], artifact["sha256"])
            records[slot] = dict(
                artifact,
                url=args.base_url.rstrip("/") + "/" + quote(artifact["file"], safe=""),
            )
    if identity is None:
        raise ValueError("no Desktop artifacts")
    version, build, source, lock = identity
    if (
        not cli.SEMVER.fullmatch(version)
        or type(build) is not int
        or not 1 <= build <= 999999999
        or not re.fullmatch("[a-f0-9]{40}", source)
    ):
        raise ValueError("invalid build identity")
    channels = CHANNELS if args.channel == "all" else [args.channel]
    # Construct in memory first. Missing inputs never leave a partly rendered set.
    files = {}

    def put(name, content):
        files[name] = (
            content
            if isinstance(content, str)
            else json.dumps(content, indent=2) + "\n"
        )

    def asset(target, kind):
        try:
            return records[(target, kind)]
        except KeyError:
            raise ValueError("missing input " + target + "/" + kind) from None

    for channel in channels:
        if channel in ["homebrew-tap", "homebrew-cask"]:
            arm = asset("darwin-arm64", "dmg")
            intel = asset("darwin-amd64", "dmg")
            if channel == "homebrew-tap":
                if not args.cli_manifest or sha(args.cli_manifest) != lock:
                    raise ValueError(
                        "Tap Formula requires the exact locked CLI manifest"
                    )
                manifest = cli.validate_manifest(args.cli_manifest)
                formula = f'class Uniclipboard < Formula\n  desc "Cross-device clipboard CLI"\n  homepage "https://uniclipboard.app"\n  version "{manifest["version"]}-build.{manifest["build_number"]}"\n  license "AGPL-3.0-only"\n'

                cli_base = (
                    "https://github.com/UniClipboard/UniClipboard/releases/download/"
                    + quote("cli/v" + manifest["npm_version"], safe="")
                    + "/"
                )
                for arch, block in [("arm64", "on_arm"), ("amd64", "on_intel")]:
                    package = next(
                        p
                        for p in manifest["packages"]
                        if p.get("target") == "darwin-" + arch
                    )
                    cli.checked_file(
                        Path(args.cli_manifest).parent,
                        package["file"],
                        package["sha256"],
                    )
                    formula += f'  {block} do\n    url "{cli_base + package["file"]}"\n    sha256 "{package["sha256"]}"\n  end\n'
                formula += '  def install\n    libexec.install "bin"\n    bin.install_symlink libexec/"bin/unic"\n    bin.install_symlink libexec/"bin/unicd"\n    bin.install_symlink libexec/"bin/unic" => "uniclip"\n  end\nend\n'
                put("homebrew-tap/Formula/uniclipboard.rb", formula)
            put(
                channel + "/uniclipboard.rb",
                f'''cask "uniclipboard" do
  version "{version},{build}"
  on_arm do
    url "{arm["url"]}"
    sha256 "{arm["sha256"]}"
  end
  on_intel do
    url "{intel["url"]}"
    sha256 "{intel["sha256"]}"
  end
  name "UniClipboard"
  desc "Cross-device clipboard synchronization"
  homepage "https://uniclipboard.app"
  depends_on macos: ">= :ventura"
  app "UniClipboard.app"
end
''',
            )
        elif channel == "scoop":
            architecture = {}
            for arch, label in [("amd64", "64bit"), ("arm64", "arm64")]:
                item = asset("windows-" + arch, "portable.zip")
                architecture[label] = {"url": item["url"], "hash": item["sha256"]}
            put(
                "scoop/uniclipboard.json",
                {
                    "version": f"{version}.{build}",
                    "description": "Cross-device clipboard synchronization",
                    "homepage": "https://uniclipboard.app",
                    "license": "AGPL-3.0-only",
                    "architecture": architecture,
                    "shortcuts": [["unic-desktop.exe", "UniClipboard"]],
                    "persist": "data",
                    "notes": "Portable v2 data stays in the persisted data directory. Standalone CLI commands are not replaced.",
                },
            )
        elif channel == "winget":
            installers = []
            for arch, label in [("amd64", "x64"), ("arm64", "arm64")]:
                item = asset("windows-" + arch, "setup.exe")
                installers.append(
                    {
                        "Architecture": label,
                        "InstallerUrl": item["url"],
                        "InstallerSha256": item["sha256"].upper(),
                    }
                )
            # JSON is valid YAML; native validation is still required before submission.
            common = {
                "PackageIdentifier": "UniClipboard.UniClipboard",
                "PackageVersion": f"{version}.{build}",
                "ManifestVersion": "1.9.0",
            }
            put(
                "winget/UniClipboard.UniClipboard.installer.yaml",
                dict(
                    common,
                    InstallerType="nullsoft",
                    InstallerSwitches={"Custom": "/MANAGED=winget"},
                    Scope="user",
                    Installers=installers,
                    ManifestType="installer",
                ),
            )
            put(
                "winget/UniClipboard.UniClipboard.yaml",
                dict(common, DefaultLocale="en-US", ManifestType="version"),
            )
            put(
                "winget/UniClipboard.UniClipboard.locale.en-US.yaml",
                dict(
                    common,
                    PackageLocale="en-US",
                    Publisher="UniClipboard",
                    PackageName="UniClipboard",
                    License="AGPL-3.0-only",
                    ShortDescription="Cross-device clipboard synchronization",
                    ManifestType="defaultLocale",
                ),
            )
        elif channel == "chocolatey":
            x64 = asset("windows-amd64", "setup.exe")
            arm = asset("windows-arm64", "setup.exe")
            put(
                "chocolatey/uniclipboard.nuspec",
                f"""<?xml version="1.0"?>
<package xmlns="http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd"><metadata><id>uniclipboard</id><version>{version}.{build}</version><title>UniClipboard</title><authors>UniClipboard</authors><description>Cross-device clipboard synchronization</description><projectUrl>https://uniclipboard.app</projectUrl><licenseUrl>https://github.com/UniClipboard/UniClipboard/blob/{source}/LICENSE</licenseUrl><requireLicenseAcceptance>false</requireLicenseAcceptance></metadata><files><file src="tools/**" target="tools"/></files></package>
""",
            )
            put(
                "chocolatey/tools/chocolateyInstall.ps1",
                f"""$ErrorActionPreference = 'Stop'
$url = '{x64["url"]}'
$checksum = '{x64["sha256"]}'
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {{
  $url = '{arm["url"]}'
  $checksum = '{arm["sha256"]}'
}}
Install-ChocolateyPackage -PackageName 'uniclipboard' -FileType 'exe' -SilentArgs '/S /MANAGED=chocolatey' -Url $url -Checksum $checksum -ChecksumType 'sha256' -ValidExitCodes @(0)
""",
            )
            put(
                "chocolatey/tools/chocolateyUninstall.ps1",
                """$ErrorActionPreference = 'Stop'
$exe = Join-Path $env:LOCALAPPDATA 'UniClipboard/uninstall.exe'
if (Test-Path $exe) {
  $process = Start-Process -FilePath $exe -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw 'UniClipboard uninstall failed; quit its owned programs and retry.' }
}
""",
            )
        elif channel == "nix":
            entries = []
            for arch, system in [("amd64", "x86_64-linux"), ("arm64", "aarch64-linux")]:
                item = asset("linux-" + arch, "deb")
                sri = (
                    "sha256-" + base64.b64encode(bytes.fromhex(item["sha256"])).decode()
                )
                entries.append(
                    f'  "{system}" = {{ url = "{item["url"]}"; hash = "{sri}"; }};'
                )
            put("nix/sources.nix", "{\n" + "\n".join(entries) + "\n}\n")
            put(
                "nix/package.nix",
                f'''{{ lib, stdenv, fetchurl, dpkg, autoPatchelfHook, wrapGAppsHook3, gtk3, libayatana-appindicator, glib, cairo, gdk-pixbuf }}:
let sources = import ./sources.nix; in
stdenv.mkDerivation {{
  pname = "uniclipboard";
  version = "{version}-build.{build}";
  src = fetchurl sources.${{stdenv.hostPlatform.system}};
  nativeBuildInputs = [ dpkg autoPatchelfHook wrapGAppsHook3 ];
  buildInputs = [ gtk3 libayatana-appindicator glib cairo gdk-pixbuf ];
  runtimeDependencies = [ gtk3 libayatana-appindicator glib cairo gdk-pixbuf ];
  dontUnpack = true;
  dontAutoPatchelf = true;
  dontWrapGApps = true;
  dontStrip = true;
  installPhase = ''
    mkdir -p $out
    dpkg-deb -x $src unpacked
    cp -r unpacked/usr/. $out/
    rm $out/bin/uniclipboard
    ln -s $out/lib/uniclipboard/unic-desktop $out/bin/uniclipboard
  '';
  postFixup = ''
    autoPatchelf $out/lib/uniclipboard/unic-desktop
    wrapGApp $out/bin/uniclipboard --prefix LD_LIBRARY_PATH : "${{lib.makeLibraryPath [ gtk3 libayatana-appindicator glib cairo gdk-pixbuf ]}}"
  '';
  meta = {{ description = "Cross-device clipboard synchronization"; homepage = "https://uniclipboard.app"; license = lib.licenses.agpl3Only; platforms = [ "x86_64-linux" "aarch64-linux" ]; mainProgram = "uniclipboard"; }};
}}
''',
            )
        elif channel == "copr":
            amd = asset("linux-amd64", "rpm")
            arm = asset("linux-arm64", "rpm")
            put(
                "copr/uniclipboard.spec",
                f"""Name: uniclipboard
Version: {version}
Release: {build}%{{?dist}}
Summary: Cross-device clipboard synchronization
License: AGPL-3.0-only
URL: https://uniclipboard.app
%ifarch x86_64
Source0: {amd["url"]}
%global payload_sha256 {amd["sha256"]}
%else
Source0: {arm["url"]}
%global payload_sha256 {arm["sha256"]}
%endif
ExclusiveArch: x86_64 aarch64
BuildRequires: rpm-build
BuildRequires: cpio
Requires: gtk3
Requires: libappindicator-gtk3
%global __os_install_post %{{nil}}
%global debug_package %{{nil}}
%description
Cross-device clipboard synchronization.
%prep
echo "%{{payload_sha256}}  %{{SOURCE0}}" | sha256sum -c -
%build
%install
mkdir -p %{{buildroot}}
cd %{{buildroot}}
rpm2cpio %{{SOURCE0}} | cpio -idm
%files
/usr/lib/uniclipboard
/usr/bin/uniclipboard
/usr/share/applications/{APP_ID}.desktop
/usr/share/icons/hicolor/scalable/apps/uniclipboard.svg
""",
            )
        elif channel in ["aur", "pacman"]:
            # Stable repository packages are already produced by nFPM. The -git
            # source path has separate provenance and retains its exact CLI lock.
            if channel == "pacman":
                item = asset("linux-amd64", "archlinux")
                put(
                    "pacman/snapshot.json",
                    {
                        "version": version,
                        "build_number": build,
                        "source_sha": source,
                        "package": item,
                        "signing": "required-before-publication",
                    },
                )
            else:
                items = {a: asset("linux-" + a, "deb") for a in ["amd64", "arm64"]}
                put(
                    "aur/uniclipboard-git/PKGBUILD",
                    (
                        Path(__file__).resolve().parents[2]
                        / "packaging/aur/uniclipboard-git/PKGBUILD"
                    ).read_text(),
                )
                put(
                    "aur/PKGBUILD",
                    f"""# Fixed binary release recipe; source -git builds require a separate source receipt.
pkgname=uniclipboard-bin
pkgver={version}
pkgrel={build}
pkgdesc='Cross-device clipboard synchronization'
arch=('x86_64' 'aarch64')
url='https://uniclipboard.app'
license=('AGPL-3.0-only')
depends=('gtk3' 'libappindicator-gtk3')
provides=('uniclipboard')
conflicts=('uniclipboard' 'uniclipboard-git')
source_x86_64=('uniclipboard.deb::{items["amd64"]["url"]}')
sha256sums_x86_64=('{items["amd64"]["sha256"]}')
source_aarch64=('uniclipboard.deb::{items["arm64"]["url"]}')
sha256sums_aarch64=('{items["arm64"]["sha256"]}')
package() {{
  cd "$srcdir"
  bsdtar -xf data.tar.* -C "$pkgdir"
}}
""",
                )
        elif channel == "flatpak":
            sources = []
            for arch, label in [("amd64", "x86_64"), ("arm64", "aarch64")]:
                item = asset("linux-" + arch, "deb")
                sources.append(
                    {
                        "type": "file",
                        "url": item["url"],
                        "sha256": item["sha256"],
                        "dest-filename": "uniclipboard.deb",
                        "only-arches": [label],
                    }
                )
            put(
                "flatpak/" + APP_ID + ".json",
                {
                    "app-id": APP_ID,
                    "runtime": "org.gnome.Platform",
                    "runtime-version": "50",
                    "sdk": "org.gnome.Sdk",
                    "command": "uniclipboard",
                    "rename-icon": "uniclipboard",
                    "finish-args": [
                        "--share=network",
                        "--share=ipc",
                        "--socket=wayland",
                        "--socket=fallback-x11",
                        "--device=dri",
                        "--talk-name=org.kde.StatusNotifierWatcher",
                    ],
                    "modules": [
                        {
                            "name": "uniclipboard",
                            "buildsystem": "simple",
                            "build-commands": [
                                "ar x uniclipboard.deb",
                                "tar xf data.tar.*",
                                "mkdir -p /app/bin /app/lib /app/share",
                                "cp -a usr/lib/uniclipboard /app/lib/",
                                "ln -s /app/lib/uniclipboard/unic-desktop /app/bin/uniclipboard",
                                "cp -a usr/share/. /app/share/",
                                "install -Dm644 "
                                + APP_ID
                                + ".metainfo.xml /app/share/metainfo/"
                                + APP_ID
                                + ".metainfo.xml",
                            ],
                            "sources": sources
                            + [{"type": "file", "path": APP_ID + ".metainfo.xml"}],
                        }
                    ],
                },
            )
            put(
                "flatpak/" + APP_ID + ".metainfo.xml",
                f'''<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application"><id>{APP_ID}</id><name>UniClipboard</name><summary>Cross-device clipboard synchronization</summary><metadata_license>CC0-1.0</metadata_license><project_license>AGPL-3.0-only</project_license><description><p>Synchronize clipboard history between paired devices.</p></description><launchable type="desktop-id">{APP_ID}.desktop</launchable><url type="homepage">https://uniclipboard.app</url><releases><release version="{version}.{build}" date="{date.today().isoformat()}"/></releases></component>
''',
            )
        elif channel == "snap":
            # JSON-form YAML is accepted by YAML parsers. Render per-architecture
            # definitions to keep each download and digest explicit.
            for arch in ["amd64", "arm64"]:
                item = asset("linux-" + arch, "deb")
                put(
                    "snap/" + arch + "/snapcraft.yaml",
                    {
                        "name": "uniclipboard",
                        "base": "core24",
                        "version": f"{version}-build.{build}",
                        "summary": "Cross-device clipboard synchronization",
                        "description": "Synchronize clipboard history between paired devices.",
                        "grade": "devel",
                        "confinement": "strict",
                        "platforms": {arch: None},
                        "apps": {
                            "uniclipboard": {
                                "command": "usr/lib/uniclipboard/unic-desktop",
                                "environment": {"UNIC_HOME": "$SNAP_USER_COMMON/unic"},
                                "extensions": ["gnome"],
                                "plugs": [
                                    "network",
                                    "network-bind",
                                    "desktop",
                                    "desktop-legacy",
                                    "unity7",
                                    "wayland",
                                    "x11",
                                    "opengl",
                                ],
                            }
                        },
                        "parts": {
                            "uniclipboard": {
                                "plugin": "dump",
                                "source": item["url"],
                                "source-checksum": "sha256/" + item["sha256"],
                                "source-type": "deb",
                                "stage-packages": [
                                    "libgtk-3-0t64",
                                    "libayatana-appindicator3-1",
                                ],
                            }
                        },
                    },
                )
        elif channel == "npm":
            put(
                "npm/locked-cli.json",
                {
                    "manifest_sha256": lock,
                    "authority": "existing CLI release workflow",
                    "desktop_is_not_npm_package": True,
                },
            )
    output = fresh_output(args.output)
    output.mkdir()
    for name, value in files.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
    cli.save(
        output / "channels.json",
        {
            "schema": 1,
            "product": "desktop",
            "version": version,
            "build_number": build,
            "source_sha": source,
            "cli_manifest_sha256": lock,
            "development": args.development,
            "channels": {
                name: {
                    "definition": "rendered",
                    "install": "pending",
                    "native": "pending",
                    "submission": "pending",
                    "approval": "pending",
                    "availability": "pending",
                }
                for name in channels
            },
            "files": {name: sha(output / name) for name in files},
        },
    )
    print(output / "channels.json")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input", required=True)
    p.add_argument("--base-url", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--channel", choices=["all"] + CHANNELS, default="all")
    p.add_argument("--development", action="store_true")
    p.add_argument("--cli-manifest")
    render(p.parse_args())


if __name__ == "__main__":
    main()
