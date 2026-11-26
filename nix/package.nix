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
  vendorHash ? "sha256-TeTSGCDctFIs4nb/0hkbMs+az+3ThqhsO+lFcU0QwvU=",
}:

let
  # Linux Desktop uses purego (dlopen) for GTK3, AppIndicator and Wayland
  # virtual-keyboard keymap loading, so runtime libraries must be exposed via
  # LD_LIBRARY_PATH rather than ELF DT_NEEDED entries.
  runtimeLibs = [
    gtk3
    libayatana-appindicator
    glib
    cairo
    gdk-pixbuf
    libxkbcommon
  ];
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

  # wrapGAppsHook3 skips symlinks in $out/bin; wrap $out/bin/uniclipboard explicitly in postFixup.
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
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibs}"
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
