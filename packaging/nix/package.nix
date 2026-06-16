# nixpkgs derivation for UniClipboard — binary repackage of the upstream AppImage.
#
# Why a binary repackage instead of a source build:
#   UniClipboard is a Tauri app (Rust workspace + bun-built frontend + a vendored
#   iroh-blobs git dependency + a sidecar `uniclipd` daemon). A from-source build
#   under the Nix sandbox would need a fixed-output bun/node_modules derivation, a
#   cargoLock entry for the vendored git source, and two separate binaries — hard
#   to land and to keep green. Wrapping the official AppImage is an accepted
#   nixpkgs pattern for this class of app and is far easier to maintain. If a
#   reviewer asks for a source build, see ./README.md for the migration path.
#
# This file is the in-repo submission source (same convention as packaging/aur/).
# To ship it, copy it into nixpkgs at pkgs/by-name/un/uniclipboard/package.nix —
# see ./README.md for the full step-by-step.
{
  lib,
  appimageTools,
  fetchurl,
}:
let
  pname = "uniclipboard";
  version = "0.15.0";

  src = fetchurl {
    url = "https://github.com/UniClipboard/UniClipboard/releases/download/v${version}/UniClipboard_${version}_amd64.AppImage";
    # Placeholder. Replace with the real hash before submitting. Easiest path:
    # leave this as-is, run `nix-build`, and copy the "got:" hash Nix prints.
    # Or precompute:
    #   nix store prefetch-file --hash-type sha256 \
    #     https://github.com/UniClipboard/UniClipboard/releases/download/v0.15.0/UniClipboard_0.15.0_amd64.AppImage
    # The same value is published (minisign-signed) in the release SHA256SUMS.txt.
    hash = lib.fakeHash;
  };

  appimageContents = appimageTools.extractType2 { inherit pname version src; };
in
appimageTools.wrapType2 {
  inherit pname version src;

  # The AppImage bundles most of the GTK/WebKit stack, but the wrapped FHS env
  # may still miss a library at runtime. If the app fails to start with a
  # "cannot open shared object file" error, add the missing package here.
  extraPkgs = pkgs: with pkgs; [ ];

  extraInstallCommands = ''
    # Desktop entry + icons taken from the AppImage payload. File names follow
    # Tauri's AppImage layout (main binary name = "uniclipboard"). If the first
    # build fails here, run `ls ${appimageContents}` and adjust the paths.
    install -Dm444 ${appimageContents}/uniclipboard.desktop \
      $out/share/applications/uniclipboard.desktop
    substituteInPlace $out/share/applications/uniclipboard.desktop \
      --replace-warn 'Exec=AppRun --no-sandbox %U' 'Exec=uniclipboard %U' \
      --replace-warn 'Exec=AppRun' 'Exec=uniclipboard'
    cp -r ${appimageContents}/usr/share/icons $out/share/icons
  '';

  meta = {
    description = "Encrypted peer-to-peer clipboard sync between your devices";
    longDescription = ''
      UniClipboard syncs your clipboard securely across your devices on the local
      network. End-to-end encrypted and powered by iroh QUIC — no clouds, no
      accounts, no third-party servers, just direct peer-to-peer sync of text,
      images, and files.
    '';
    homepage = "https://uniclipboard.app";
    downloadPage = "https://github.com/UniClipboard/UniClipboard/releases";
    license = lib.licenses.agpl3Only;
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
    maintainers = with lib.maintainers; [ ]; # add your nixpkgs handle here
    mainProgram = "uniclipboard";
    platforms = [ "x86_64-linux" ];
  };
}
