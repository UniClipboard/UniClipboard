{ self ? null }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfgDesktop = config.programs.uniclipboard;
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
    programs.uniclipboard = {
      enable = lib.mkEnableOption "Unic (UniClipboard) desktop application";

      package = lib.mkOption {
        type = lib.types.package;
        default = defaultDesktopPackage;
        defaultText = lib.literalExpression "pkgs.uniclipboard";
        description = "The Unic desktop package to install.";
      };
    };

    services.unicd = {
      enable = lib.mkEnableOption "Unic headless background sync daemon (unicd) as a systemd user service";

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
      environment.systemPackages = [ cfgDesktop.package ];
    })

    (lib.mkIf cfgDaemon.enable {
      environment.systemPackages = [ cfgDaemon.package ];

      systemd.user.services.unicd = {
        description = "Unic clipboard sync daemon";
        after = [ "network-online.target" ];
        wants = [ "network-online.target" ];
        wantedBy = [ "default.target" ];
        serviceConfig = {
          ExecStart = "${lib.getExe' cfgDaemon.package "unicd"}${
            lib.optionalString (daemonArgs != [ ]) (" " + lib.escapeShellArgs daemonArgs)
          }";
          Restart = "on-failure";
          RestartSec = 3;
        };
      };
    })
  ];
}
