{ self ? null }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfgDesktop = config.services.uniclipboard;
  cfgDaemon = config.services.unicd;

  system = pkgs.stdenv.hostPlatform.system;
  flakePkgs = if self != null then (self.packages.${system} or { }) else { };

  defaultDesktopPackage = flakePkgs.uniclipboard or pkgs.uniclipboard;
  defaultCliPackage = flakePkgs.unic or pkgs.unic;

  daemonArgs =
    lib.optionals (cfgDaemon.profile != null) [
      "--profile"
      cfgDaemon.profile
    ]
    ++ cfgDaemon.extraArgs;
in
{
  options = {
    services.uniclipboard = {
      enable = lib.mkEnableOption "Unic (UniClipboard) desktop application";

      package = lib.mkOption {
        type = lib.types.package;
        default = defaultDesktopPackage;
        defaultText = lib.literalExpression "pkgs.uniclipboard";
        description = "The Unic desktop package to install.";
      };

      autostart = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether to register an XDG autostart desktop entry for Unic.";
      };
    };

    services.unicd = {
      enable = lib.mkEnableOption "Unic headless background sync daemon (unicd)";

      package = lib.mkOption {
        type = lib.types.package;
        default = defaultCliPackage;
        defaultText = lib.literalExpression "pkgs.unic";
        description = "The headless Unic CLI/daemon package to install and run.";
      };

      profile = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Optional profile name passed via `--profile`.";
      };

      extraArgs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "Extra command-line arguments passed to `unicd`.";
      };
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfgDesktop.enable {
      home.packages = [ cfgDesktop.package ];

      xdg.configFile."autostart/app.uniclipboard.desktop.desktop" = lib.mkIf (
        pkgs.stdenv.hostPlatform.isLinux && cfgDesktop.autostart
      ) {
        source = "${cfgDesktop.package}/share/applications/app.uniclipboard.desktop.desktop";
      };
    })

    (lib.mkIf cfgDaemon.enable {
      home.packages = [ cfgDaemon.package ];

      systemd.user.services.unicd = lib.mkIf pkgs.stdenv.hostPlatform.isLinux {
        Unit = {
          Description = "Unic clipboard sync daemon";
          After = [ "network-online.target" ];
          Wants = [ "network-online.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe' cfgDaemon.package "unicd"}${
            lib.optionalString (daemonArgs != [ ]) (" " + lib.escapeShellArgs daemonArgs)
          }";
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install = {
          WantedBy = [ "default.target" ];
        };
      };
    })
  ];
}
