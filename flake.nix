{
  description = "Unic (UniClipboard) — encrypted peer-to-peer clipboard sync";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      linuxSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      unixSystems = linuxSystems ++ [
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      products = (builtins.fromTOML (builtins.readFile ./release/products.toml)).products;
      sourceRev = self.shortRev or self.dirtyShortRev or "unknown";

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
        };

      mkPackages =
        pkgs:
        let
          buildGoModule = pkgs.buildGo127Module or pkgs.buildGoModule;
          unic = pkgs.callPackage ./nix/unic.nix {
            inherit buildGoModule sourceRev;
            src = self;
            version = products.cli.version;
            buildNumber = toString products.cli.build_number;
          };
          desktop = lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            uniclipboard = pkgs.callPackage ./nix/package.nix {
              inherit buildGoModule sourceRev;
              src = self;
              version = products.desktop.version;
              buildNumber = toString products.desktop.build_number;
            };
          };
        in
        desktop
        // {
          inherit unic;
          default = if pkgs.stdenv.hostPlatform.isLinux then desktop.uniclipboard else unic;
        };
    in
    {
      packages = lib.genAttrs unixSystems (system: mkPackages (pkgsFor system));

      overlays.default =
        final: _prev:
        let
          built = mkPackages final;
        in
        {
          inherit (built) unic;
        }
        // lib.optionalAttrs final.stdenv.hostPlatform.isLinux {
          inherit (built) uniclipboard;
        };

      nixosModules = {
        uniclipboard = import ./nix/nixos-module.nix { inherit self; };
        default = self.nixosModules.uniclipboard;
      };

      homeManagerModules = {
        uniclipboard = import ./nix/home-manager-module.nix { inherit self; };
        default = self.homeManagerModules.uniclipboard;
      };
    };
}
