{
  lib,
  buildGoModule,
  wrapGAppsHook3,
  gtk3,
  libayatana-appindicator,
  glib,
  cairo,
  gdk-pixbuf,
  libxkbcommon,
  nix-update-script,
  src ? lib.cleanSource ../.,
  version ? (builtins.fromTOML (builtins.readFile ../release/products.toml)).products.desktop.version,
  buildNumber ? toString (builtins.fromTOML (builtins.readFile ../release/products.toml)).products.desktop.build_number,
  sourceRev ? "unknown",
  vendorHash ? "sha256-pIYoBwMFqCZ5taiq86k2BUdLvrBPLXML7C/yIV1escM=",
}:

let
  # Linux Desktop uses purego (dlopen) for GTK3, AppIndicator and Wayland
  # virtual-keyboard keymap loading. Embed DT_RUNPATH via Go's internal linker
  # (-r) so both the wrapped launcher and login-autostart launches of the
  # private unic-desktop binary resolve these libraries, and disable patchelf
  # shrink-rpath so the dlopen-only entries are preserved.
  runtimeLibs = [
    gtk3
    libayatana-appindicator
    glib
    cairo
    gdk-pixbuf
    libxkbcommon
  ];
  runtimeLibPath = lib.makeLibraryPath runtimeLibs;
in
buildGoModule {
  pname = "uniclipboard";
  inherit version src vendorHash;

  env.CGO_ENABLED = 0;

  subPackages = [
    "app/desktop/cmd/unic-desktop"
    "app/service/cmd/unic"
    "app/service/cmd/unicd"
  ];

  # Keep update_public_key unset so Nix / NixOS owns application upgrades.
  ldflags = [
    "-s"
    "-w"
    "-r ${runtimeLibPath}"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.Version=${version}"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.BuildNumber=${buildNumber}"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.Source=${sourceRev}"
    "-X github.com/uniclipboard/unic/internal/observability.ProductVersion=${version}"
    "-X main.desktopAppName=Unic"
    "-X github.com/egoist/mygo.packageIdentifier=UniClipboard"
    "-X github.com/egoist/mygo.production=1"
  ];

  # Integration tests require local sockets, display servers, or network fixtures.
  doCheck = false;

  nativeBuildInputs = [
    wrapGAppsHook3
  ];

  buildInputs = runtimeLibs;

  # Preserve Go's link-time -r RUNPATH entries (patchelf --shrink-rpath drops
  # dlopen-only paths) and wrap the $out/bin/uniclipboard symlink explicitly.
  dontPatchELF = true;
  dontWrapGApps = true;

  postInstall = ''
    mkdir -p $out/lib/uniclipboard/licenses
    mv $out/bin/unic-desktop $out/bin/unic $out/bin/unicd $out/lib/uniclipboard/
    ln -s unic $out/lib/uniclipboard/uniclip
    ln -s $out/lib/uniclipboard/unic-desktop $out/bin/uniclipboard

    printf '{"schema":1,"application_id":"app.uniclipboard.desktop","target":"linux-%s"}\n' \
      "$(go env GOARCH)" > $out/lib/uniclipboard/installation.json

    install -Dm644 LICENSE $out/lib/uniclipboard/LICENSE
    install -Dm644 app/desktop/boardui/assets/BOARDUI-MIT.txt \
      $out/lib/uniclipboard/licenses/BOARDUI-MIT.txt
    install -Dm644 app/desktop/boardui/assets/Inter-OFL.txt \
      $out/lib/uniclipboard/licenses/Inter-OFL.txt
    install -Dm644 app/desktop/boardui/assets/REMIX-LICENSE.txt \
      $out/lib/uniclipboard/licenses/REMIX-LICENSE.txt

    install -Dm644 app/desktop/internal/appicons/icon.png \
      $out/lib/uniclipboard/icon.png
    install -Dm644 app/desktop/internal/appicons/icon-linux-48.png \
      $out/lib/uniclipboard/icon-linux-48.png
    install -Dm644 app/desktop/internal/appicons/icon-linux.svg \
      $out/lib/uniclipboard/icon-linux.svg

    install -Dm644 packaging/linux/uniclipboard.desktop \
      $out/share/applications/app.uniclipboard.desktop.desktop
    install -Dm644 app/desktop/internal/appicons/icon-linux.svg \
      $out/share/icons/hicolor/scalable/apps/uniclipboard.svg
    install -Dm644 app/desktop/internal/appicons/icon-linux-48.png \
      $out/share/icons/hicolor/48x48/apps/uniclipboard.png
    install -Dm644 app/desktop/internal/appicons/icon.png \
      $out/share/icons/hicolor/512x512/apps/uniclipboard.png
  '';

  postFixup = ''
    wrapGApp $out/bin/uniclipboard \
      --prefix LD_LIBRARY_PATH : "${runtimeLibPath}"
  '';

  passthru.updateScript = nix-update-script { };

  meta = {
    description = "Cross-device encrypted peer-to-peer clipboard synchronization";
    homepage = "https://uniclipboard.app";
    changelog = "https://github.com/UniClipboard/UniClipboard/releases";
    license = lib.licenses.agpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "uniclipboard";
  };
}
