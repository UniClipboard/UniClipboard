{
  description = "UniClipboard: encrypted peer-to-peer clipboard sync";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs: rec {
        uniclipboard = pkgs.callPackage ./packaging/nix/package.nix { };
        default = uniclipboard;
      });
    };
}
