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
          maintainers = with lib.maintainers; [ "Blooym" ];
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
            description = "The Porxie package to use.";
          };

          user = mkOption {
            type = types.str;
            default = "porxie";
            description = "User that Porxie runs under.";
          };

          group = mkOption {
            type = types.str;
            default = "porxie";
            description = "Group that Porxie runs under.";
          };

          environmentFiles = mkOption {
            type = types.listOf types.path;
            default = [];
            description = ''
              Files to load environment variables from. Use for secrets such as
              {env}`PORXIE_SERVER_AUTH_TOKEN` and {env}`PORXIE_POLICY_REQUEST_HEADERS`.
            '';
          };

          settings = mkOption {
            type = types.submodule {
              freeformType = types.attrsOf types.str;

              options = {
                server = {
                  address = mkOption {
                    type = types.str;
                    default = "ip:127.0.0.1:6314";
                  };

                  authToken = mkOption {
                    type = types.nullOr types.str;
                    default = null;
                  };
                };

                blob = {
                  allowedMimetypes = mkOption {
                    type = types.listOf types.str;
                    default = [ "image/*" ];
                  };

                  maxSize = mkOption {
                    type = types.str;
                    default = "50mb";
                  };

                  cacheHeader = mkOption {
                    type = types.str;
                    default = "public, max-age=604800, must-revalidate, immutable";
                  };

                  processingTimeout = mkOption {
                    type = types.str;
                    default = "1m";
                  };

                  httpTimeout = mkOption {
                    type = types.str;
                    default = "30s";
                  };

                  httpConnectTimeout = mkOption {
                    type = types.str;
                    default = "10s";
                  };
                };

                identity = {
                  plcUrl = mkOption {
                    type = types.str;
                    default = "https://plc.directory";
                  };

                  httpTimeout = mkOption {
                    type = types.str;
                    default = "10s";
                  };

                  httpConnectTimeout = mkOption {
                    type = types.str;
                    default = "8s";
                  };
                };

                cache = {
                  allocation = mkOption {
                    type = types.str;
                    default = "512mb";
                  };

                  blobTti = mkOption {
                    type = types.str;
                    default = "7days";
                  };

                  ownershipTtl = mkOption {
                    type = types.str;
                    default = "1day";
                  };

                  policyTtl = mkOption {
                    type = types.str;
                    default = "1h";
                  };
                };

                policy = {
                  url = mkOption {
                    type = types.nullOr types.str;
                    default = null;
                  };

                  requestHeaders = mkOption {
                    type = types.listOf types.str;
                    default = [];
                  };

                  failOpen = mkOption {
                    type = types.bool;
                    default = false;
                  };

                  httpTimeout = mkOption {
                    type = types.str;
                    default = "30s";
                  };

                  httpConnectTimeout = mkOption {
                    type = types.str;
                    default = "10s";
                  };
                };
              };
            };

            default = {};
            description = ''
              Configuration for Porxie. Refer to
              <https://codeberg.org/Blooym/porxie/src/branch/main/README.md#configuration>
              for further guidance.

              Secrets such as {option}`settings.server.authToken` should be set via
              {option}`environmentFiles`.
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

          systemd.services.porxie = let
            knownKeys = [ "server" "blob" "identity" "cache" "policy" ];
            env = (lib.filterAttrs (k: _: !(builtins.elem k knownKeys)) cfg.settings) // lib.filterAttrs (_: v: v != null) {
              PORXIE_SERVER_ADDRESS            = cfg.settings.server.address;
              PORXIE_SERVER_AUTH_TOKEN         = cfg.settings.server.authToken;
              PORXIE_BLOB_ALLOWED_MIMETYPES    = if cfg.settings.blob.allowedMimetypes != [] then lib.concatStringsSep "," cfg.settings.blob.allowedMimetypes else null;
              PORXIE_BLOB_MAX_SIZE             = cfg.settings.blob.maxSize;
              PORXIE_BLOB_CACHE_HEADER         = cfg.settings.blob.cacheHeader;
              PORXIE_BLOB_PROCESSING_TIMEOUT   = cfg.settings.blob.processingTimeout;
              PORXIE_BLOB_HTTP_TIMEOUT         = cfg.settings.blob.httpTimeout;
              PORXIE_BLOB_HTTP_CONNECT_TIMEOUT = cfg.settings.blob.httpConnectTimeout;
              PORXIE_IDENTITY_PLC_URL          = cfg.settings.identity.plcUrl;
              PORXIE_IDENTITY_HTTP_TIMEOUT     = cfg.settings.identity.httpTimeout;
              PORXIE_IDENTITY_HTTP_CONNECT_TIMEOUT = cfg.settings.identity.httpConnectTimeout;
              PORXIE_CACHE_ALLOCATION          = cfg.settings.cache.allocation;
              PORXIE_CACHE_BLOB_TTI            = cfg.settings.cache.blobTti;
              PORXIE_CACHE_OWNERSHIP_TTL       = cfg.settings.cache.ownershipTtl;
              PORXIE_CACHE_POLICY_TTL          = cfg.settings.cache.policyTtl;
              PORXIE_POLICY_URL                = cfg.settings.policy.url;
              PORXIE_POLICY_REQUEST_HEADERS    = if cfg.settings.policy.requestHeaders != [] then lib.concatStringsSep "|" cfg.settings.policy.requestHeaders else null;
              PORXIE_POLICY_FAIL_OPEN          = if cfg.settings.policy.failOpen then "true" else null;
              PORXIE_POLICY_HTTP_TIMEOUT       = cfg.settings.policy.httpTimeout;
              PORXIE_POLICY_HTTP_CONNECT_TIMEOUT = cfg.settings.policy.httpConnectTimeout;
            };
          in {
            description = "Porxie atproto blob proxy";
            after = [ "network-online.target" ];
            wants = [ "network-online.target" ];
            wantedBy = [ "multi-user.target" ];

            serviceConfig = {
              User = cfg.user;
              Group = cfg.group;
              ExecStart = lib.getExe cfg.package;
              Environment = lib.mapAttrsToList (k: v: "${k}=${v}") env;
              EnvironmentFile = cfg.environmentFiles;
              Restart = "on-failure";
              RestartSec = 5;

              RuntimeDirectory = "porxie";
              RuntimeDirectoryMode = "0750";

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
              RestrictNamespaces = true;
              RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
              LockPersonality = true;
              MemoryDenyWriteExecute = true;
              RemoveIPC = true;
              SystemCallArchitectures = "native";
              SystemCallFilter = [ "@system-service" "~@privileged" "~@resources" ];
            };
          };
        };
      });

      default = self.nixosModules.porxie;
    };
  };
}
