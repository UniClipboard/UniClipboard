# UniClipboard for Nix: repackages the released .deb of the Go/Wails GUI host and its bundled `uniclipd` daemon.
#
# Why the .deb and not the AppImage or a source build:
# - The host branches on APPIMAGE/APPDIR (install kind, self-update, restart, autostart, portable data root). Wrapping the
#   AppImage would make a Nix install behave like a self-updating AppImage; the .deb has none of that.
# - The .deb links the system GTK4 / WebKitGTK 6.0 / libsoup 3, which nixpkgs provides, so no bundled libraries and no FHS sandbox.
# - A source build needs a fixed-output bun tree, a Go vendor hash, a cargoLock with git sources and the Engine build tools.
#
# The version and the per-architecture hashes live in ./sources.json; `./update.py` rewrites them from a release's SHA256SUMS.txt.
# This file is `callPackage`-able and needs nixpkgs 26.05 or newer (`libx11` as a top-level attribute).
{
  lib,
  stdenv,
  fetchurl,
  dpkg,
  autoPatchelfHook,
  wrapGAppsHook4,
  wrapGAppsHook3,
  adwaita-icon-theme,
  cairo,
  dbus,
  gdk-pixbuf,
  glib,
  glib-networking,
  gtk-layer-shell,
  gtk4,
  gtk3,
  libsoup_3,
  libx11,
  librsvg,
  webkitgtk_6_0,
  webkitgtk_4_1,
  xdg-utils,
  releaseSources ? builtins.fromJSON (builtins.readFile ./sources.json),
}:
let
  sources = releaseSources;
  # TODO: Remove this exact legacy release branch when sources.json first pins a verified GTK4 deb.
  # Existing published hashes cannot be relabelled as GTK4 artifacts before a new release exists.
  legacyGtk3 = sources.version == "1.2.0-alpha.7" && sources.gtkMajor == 3;
  gtkRuntime = if legacyGtk3 then gtk3 else gtk4;
  webkitRuntime = if legacyGtk3 then webkitgtk_4_1 else webkitgtk_6_0;
  gappsHook = if legacyGtk3 then wrapGAppsHook3 else wrapGAppsHook4;
  debArch =
    {
      x86_64-linux = "amd64";
      aarch64-linux = "arm64";
    }
    .${stdenv.hostPlatform.system} or (throw "uniclipboard: unsupported system ${stdenv.hostPlatform.system}");
in
assert legacyGtk3 || sources.gtkMajor == 4;
stdenv.mkDerivation (finalAttrs: {
  pname = "uniclipboard";
  inherit (sources) version;

  src = fetchurl {
    url = "https://github.com/UniClipboard/UniClipboard/releases/download/v${finalAttrs.version}/UniClipboard_${finalAttrs.version}_${debArch}.deb";
    hash = sources.hashes.${stdenv.hostPlatform.system};
  };

  nativeBuildInputs = [
    autoPatchelfHook
    dpkg
    gappsHook
  ];

  # Every DT_NEEDED entry of both binaries; autoPatchelfHook fails the build if one stays unresolved.
  buildInputs = [
    cairo
    dbus
    gdk-pixbuf
    glib
    gtkRuntime
    libsoup_3
    libx11
    stdenv.cc.cc.lib
    webkitRuntime
    # GTK and WebKit runtime data, collected by the GTK wrapper hook into the GUI wrapper.
    adwaita-icon-theme
    glib-networking
    librsvg
  ];

  # Only the pinned pre-migration release needs the GTK3 Layer Shell library. GTK4 uses ordinary windows.
  runtimeDependencies = lib.optionals legacyGtk3 [ (lib.getLib gtk-layer-shell) ];

  dontConfigure = true;
  dontBuild = true;
  # The GTK wrapper hook would wrap every file in bin/. `uniclipd` must keep its name: the host accepts a running daemon only if
  # /proc/<pid>/exe is called `uniclipd` (packages/desktop-host-go/daemonproc/pid.go), so only the GUI is wrapped, in postFixup.
  dontWrapGApps = true;

  unpackPhase = ''
    runHook preUnpack
    dpkg-deb -x $src .
    runHook postUnpack
  '';
  sourceRoot = ".";

  installPhase = ''
    runHook preInstall
    # The GUI finds its daemon as a sibling of its own executable, so both stay in bin/.
    install -Dm755 usr/bin/uniclipboard $out/bin/uniclipboard
    install -Dm755 usr/bin/uniclipd $out/bin/uniclipd
    mkdir -p $out/share
    cp -r usr/share/applications usr/share/icons $out/share/
    runHook postInstall
  '';

  postFixup = ''
    # xdg-open opens links and files; gsettings reads the desktop theme for the tray icon. Both come last in PATH so the
    # host's own tools win.
    wrapProgram $out/bin/uniclipboard \
      "''${gappsWrapperArgs[@]}" \
      --suffix PATH : ${lib.makeBinPath [ xdg-utils glib ]}
  '';

  passthru.gtkMajor = if legacyGtk3 then 3 else 4;
  passthru.releaseSources = sources;

  meta = {
    description = "Encrypted peer-to-peer clipboard sync between your devices";
    longDescription = ''
      UniClipboard syncs text, images and files between your devices over end-to-end encrypted direct connections.
      This package installs the released desktop app (`uniclipboard`) together with its background daemon (`uniclipd`).
    '';
    homepage = "https://uniclipboard.app";
    downloadPage = "https://github.com/UniClipboard/UniClipboard/releases";
    changelog = "https://github.com/UniClipboard/UniClipboard/releases/tag/v${finalAttrs.version}";
    license = lib.licenses.agpl3Only;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    mainProgram = "uniclipboard";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
})
