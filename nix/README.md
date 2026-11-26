# Nix / NixOS & nixpkgs Packaging

This directory contains the source-built Nix package definitions and declarative NixOS / Home Manager modules exported by the root [flake.nix](../flake.nix).

> Note: Prebuilt release-channel flake assets remain under `packaging/nix/` and are produced by `.github/workflows/release-nix.yml`.

## Quick start (Flakes)

Run Desktop (Linux) or headless CLI directly:

```sh
# Desktop application (Linux x86_64 / aarch64)
nix run github:UniClipboard/UniClipboard

# Headless CLI & daemon (Linux / macOS)
nix run github:UniClipboard/UniClipboard#unic -- --version
```

Install into your profile:

```sh
nix profile install github:UniClipboard/UniClipboard          # Desktop (Linux) or CLI (macOS)
nix profile install github:UniClipboard/UniClipboard#unic     # Headless CLI (unic, unicd, uniclip)
```

## NixOS configuration

Add Unic as a flake input and import `nixosModules.default`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    uniclipboard.url = "github:UniClipboard/UniClipboard";
  };

  outputs = { nixpkgs, uniclipboard, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        uniclipboard.nixosModules.default
        {
          # Desktop application (includes private bundled unic/unicd)
          programs.uniclipboard.enable = true;

          # Optional: standalone headless daemon on servers / CLI-only hosts
          # services.unicd.enable = true;
        }
      ];
    };
  };
}
```

## Home Manager configuration

Pass the `uniclipboard` flake input through `extraSpecialArgs` (for example in `home-manager.lib.homeManagerConfiguration`):

```nix
home-manager.lib.homeManagerConfiguration {
  pkgs = nixpkgs.legacyPackages.x86_64-linux;
  modules = [ ./home.nix ];
  extraSpecialArgs = { inherit uniclipboard; };
}
```

Then import `homeManagerModules.default` in `home.nix`:

```nix
{ uniclipboard, ... }:
{
  imports = [ uniclipboard.homeManagerModules.default ];

  # Desktop application with XDG autostart (Linux)
  services.uniclipboard = {
    enable = true;
    autostart = true;
  };

  # Or standalone headless CLI + systemd user service (systemd service is Linux-only;
  # on macOS this installs `unic`/`unicd`, and you can run `unicd` directly or let `unic` manage it on demand)
  # services.unicd.enable = true;
}
```

## Submitting to `NixOS/nixpkgs`

[package.nix](package.nix) (Desktop) and [unic.nix](unic.nix) (headless CLI) use `buildGoModule` with `CGO_ENABLED = 0` and can be adapted directly into `NixOS/nixpkgs` under `pkgs/by-name/un/uniclipboard/package.nix` (and optionally `pkgs/by-name/un/unic/package.nix`):

1. Replace the default `src` parameter with `fetchFromGitHub` pinned to a stable release tag and hash, and pin `version`, `buildNumber`, and `vendorHash`.
2. Add your maintainer handle to `meta.maintainers` (registered in `maintainers/maintainer-list.nix`).
3. Verify locally with `nix-build -A uniclipboard` and `./result/bin/uniclipboard --version --json`, plus interactive X11/Wayland verification (window, tray icon, clipboard, Quick Panel).
4. Open a PR against `NixOS/nixpkgs` `master` (e.g. commit message `uniclipboard: init at <version>`).

### Keeping `vendorHash` up to date

When `go.mod` or `go.sum` changes, update `vendorHash` in [package.nix](package.nix) and [unic.nix](unic.nix). Setting `vendorHash = lib.fakeHash;` and running `nix build .#unic` will print the expected SRI hash, which is also checked by `.github/workflows/check-nix.yml`.
