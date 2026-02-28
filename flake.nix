{
  description = "Nix flake for Porxie: an atproto blob proxy";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs, ...}: let
    forAllSystems = function:
      nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (
        system: (function system nixpkgs.legacyPackages.${system})
      );
  in {
    packages = forAllSystems (system: pkgs: {
      porxie = pkgs.callPackage ({ lib, rustPlatform }: let
        toml = (lib.importTOML ./Cargo.toml).package;
      in rustPlatform.buildRustPackage {
        pname = "porxie";
        inherit (toml) version;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.intersection (lib.fileset.fromSource (lib.sources.cleanSource ./.)) (
            lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./src
            ]
          );
        };

        cargoLock.lockFile = ./Cargo.lock;

        meta = {
          inherit (toml) homepage description;
          mainProgram = "porxie";
          license = lib.licenses.agpl3Only;
        };
      }) { };

      default = self.packages.${system}.porxie;
    });

    nixosModules = {
      porxie = ({ config, lib, pkgs, ... }: let
        inherit (lib) types mkOption;
        cfg = config.services.porxie;
      in {
        _class = "nixos";

        options.services.porxie = {
          enable = lib.mkEnableOption "Porxie";


          package = mkOption {
            type = types.package;
            default = self.packages.${pkgs.stdenv.hostPlatform.system}.porxie;
            defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.porxie";
            description = "The Porxie package to use";
          };

          user = mkOption {
            type = types.str;
            default = "porxie";
            description = "User under which Porxie runs";
          };

          group = mkOption {
            type = types.str;
            default = "porxie";
            description = "Group under which Porxie runs";
          };

          settings = mkOption {
            type = types.submodule {
              freeformType = types.attrsOf types.str;
            };
            default = {};
            description = ''
              Environment variables to set for the service.

              Refer to <https://codeberg.org/Blooym/porxie/src/branch/main#configuration> for available environment variables;
            '';
          };
        };

        config = lib.mkIf cfg.enable {
          users = {
            users.${cfg.user} = {
              isSystemUser = true;
              inherit (cfg) group;
            };
            groups.${cfg.group} = { };
          };

          systemd.services.porxie = {
            description = "Porxie atproto blob proxy";
            after = [ "network-online.target" ];
            wants = [ "network-online.target" ];
            wantedBy = [ "multi-user.target" ];

            serviceConfig = {
              User = cfg.user;
              Group = cfg.group;
              ExecStart = lib.getExe cfg.package;
              Environment = lib.mapAttrsToList (k: v: "${k}=${if builtins.isInt v then toString v else v}") cfg.settings;
              Restart = "on-failure";
              RestartSec = 5;

              CapabilityBoundingSet = "";
              AmbientCapabilities = "";
              NoNewPrivileges = true;
              ReadOnlyRootFilesystem = true;
              ProtectSystem = "strict";
              ProtectHome = true;
              PrivateTmp = true;
              PrivateDevices = true;
              ProtectKernelTunables = true;
              ProtectKernelModules = true;
              ProtectKernelLogs = true;
              ProtectControlGroups = true;
              ProtectClock = true;
              ProtectHostname = true;
              RestrictSUIDSGID = true;
              RestrictRealtime = true;
              LockPersonality = true;
              RestrictNamespaces = true;
            };
          };
        };
      });

      default = self.nixosModules.porxie;
    };
  };
}
