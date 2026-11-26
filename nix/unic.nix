{
  lib,
  buildGoModule,
  nix-update-script,
  src ? lib.cleanSource ../.,
  version ? (builtins.fromTOML (builtins.readFile ../release/products.toml)).products.cli.version,
  buildNumber ? toString (builtins.fromTOML (builtins.readFile ../release/products.toml)).products.cli.build_number,
  sourceRev ? "unknown",
  vendorHash ? "sha256-pIYoBwMFqCZ5taiq86k2BUdLvrBPLXML7C/yIV1escM=",
}:

buildGoModule {
  pname = "unic";
  inherit version src vendorHash;

  env.CGO_ENABLED = 0;

  subPackages = [
    "app/service/cmd/unic"
    "app/service/cmd/unicd"
  ];

  ldflags = [
    "-s"
    "-w"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.Version=${version}"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.BuildNumber=${buildNumber}"
    "-X github.com/uniclipboard/unic/internal/releaseinfo.Source=${sourceRev}"
    "-X github.com/uniclipboard/unic/internal/observability.ProductVersion=${version}"
  ];

  doCheck = false;

  postInstall = ''
    ln -s unic $out/bin/uniclip
  '';

  passthru.updateScript = nix-update-script { };

  meta = {
    description = "Headless CLI and sync daemon for Unic (UniClipboard)";
    homepage = "https://uniclipboard.app";
    changelog = "https://github.com/UniClipboard/UniClipboard/releases";
    license = lib.licenses.agpl3Only;
    platforms = lib.platforms.unix;
    mainProgram = "unic";
  };
}
