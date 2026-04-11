{
  description = "Nix flake for Porxie: an atproto blob proxy";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      forAllSystems =
        function:
        nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (
          system: (function system nixpkgs.legacyPackages.${system})
        );
    in
    {
      devShells = forAllSystems (
        system: pkgs: {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              bashInteractive
            ];
          };
        }
      );

      packages = forAllSystems (
        system: pkgs: {
          porxie = pkgs.callPackage (
            { lib, rustPlatform }:
            let
              toml = (lib.importTOML ./Cargo.toml).package;
            in
            rustPlatform.buildRustPackage {
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
            }
          ) { };

          default = self.packages.${system}.porxie;
        }
      );

      nixosModules = {
        porxie = (
          {
            config,
            lib,
            pkgs,
            ...
          }:
          let
            inherit (lib) types mkOption;
            cfg = config.services.porxie;
          in
          {
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
                default = [ ];
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
                        description = ''
                          Address to bind the server to.

                          Use the 'ip:' prefix for an IP address (e.g. 'ip:127.0.0.1:6314'), or on
                          UNIX systems, the 'unix:' prefix for a UNIX socket path
                          (e.g. 'unix:/run/porxie.sock').
                        '';
                      };

                      authToken = mkOption {
                        type = types.nullOr types.str;
                        default = null;
                        description = ''
                          Bearer token for authenticating admin requests.

                          When unset, all authenticated endpoints will reject requests with HTTP 401.
                          Should be set via {option}`environmentFiles` rather than directly.
                        '';
                      };
                    };

                    blob = {
                      allowedMimetypes = mkOption {
                        type = types.listOf types.str;
                        default = [ "image/*" ];
                        description = ''
                          Blob mimetypes that can be served.

                          Validation is done loosely via content inference. Further validation can be
                          done by a layer above this proxy, such as an image transformation service.
                          When inference fails, the blob's type falls back to
                          `application/octet-stream`. When that type is allowed, blobs failing
                          inference can still be served.
                        '';
                      };

                      maxSize = mkOption {
                        type = types.str;
                        default = "50mb";
                        description = ''
                          Maximum blob size that can be fetched and served.

                          Blobs that exceed this limit will return HTTP 413. Setting this too high can
                          exhaust process or system memory. The minimum value is 512kb.
                        '';
                      };

                      cacheHeader = mkOption {
                        type = types.str;
                        default = "public, max-age=604800, must-revalidate, immutable";
                        description = ''
                          The Cache-Control header value to send alongside blob responses.

                          This does not affect internal cache lifetimes, only how downstream clients
                          such as CDNs and browsers are instructed to cache responses. Intermediary
                          caches may need to be cleared manually for changes to take effect quickly.
                        '';
                      };

                      processingTimeout = mkOption {
                        type = types.str;
                        default = "1m";
                        description = ''
                          Maximum duration a blob can be processed by this server before aborting.
                        '';
                      };

                      httpTimeout = mkOption {
                        type = types.str;
                        default = "30s";
                        description = ''
                          Maximum duration before blob fetch requests are timed out.
                        '';
                      };

                      httpConnectTimeout = mkOption {
                        type = types.str;
                        default = "10s";
                        description = ''
                          Maximum duration before an attempted connection to a blob upstream is aborted.

                          This value should be lower than {option}`settings.blob.httpTimeout`.
                        '';
                      };
                    };

                    identity = {
                      plcUrl = mkOption {
                        type = types.str;
                        default = "https://plc.directory";
                        description = ''
                          URL of the PLC instance used for `did:plc` lookups.

                          Can typically be left as default unless using a custom or local development
                          setup.
                        '';
                      };

                      httpTimeout = mkOption {
                        type = types.str;
                        default = "10s";
                        description = ''
                          Maximum duration before identity resolution requests are timed out.
                        '';
                      };

                      httpConnectTimeout = mkOption {
                        type = types.str;
                        default = "8s";
                        description = ''
                          Maximum duration before a connection attempt to an identity upstream is aborted.

                          This value should be lower than {option}`settings.identity.httpTimeout`.
                        '';
                      };
                    };

                    cache = {
                      allocation = mkOption {
                        type = types.str;
                        default = "512mb";
                        description = ''
                          Total memory allocation for the internal cache.

                          Blobs are cached using an LFU policy. The most frequently requested blobs
                          are kept longest when the cache approaches its limit.

                          For production deployments, a CDN or caching layer in front of this server
                          is recommended for lower latency and better global availability.

                          Setting this too high can exhaust process or system memory. The minimum
                          value is 8mb.
                        '';
                      };

                      blobTti = mkOption {
                        type = types.str;
                        default = "7days";
                        description = ''
                          How long blobs can be idle in the cache before expiring.
                        '';
                      };

                      ownershipTtl = mkOption {
                        type = types.str;
                        default = "1day";
                        description = ''
                          How long blob ownership can be cached before expiring.
                        '';
                      };

                      policyTtl = mkOption {
                        type = types.str;
                        default = "1h";
                        description = ''
                          How long policy decisions can be cached before expiring.
                        '';
                      };

                      identityTtl = mkOption {
                        type = types.str;
                        default = "1h";
                        description = ''
                          How long identity lookups (DID resolution, etc) can be cached before expiring.
                        '';
                      };
                    };

                    policy = {
                      url = mkOption {
                        type = types.nullOr types.str;
                        default = null;
                        description = ''
                          Policy service URL that DID+CID pairs will be checked against.

                          Requests are sent as HTTP GET <url>/<did>/<cid>.

                          The service is expected to return HTTP 200 (OK) if permitted or HTTP 410
                          (GONE) if restricted.
                        '';
                      };

                      requestHeaders = mkOption {
                        type = types.listOf types.str;
                        default = [ ];
                        description = ''
                          Headers sent alongside all requests to the policy service.

                          Each header must be in the format "Name: value". When setting via
                          environment variable, headers are pipe-separated (|).

                          Should be set via {option}`environmentFiles` for sensitive values such as
                          API keys.
                        '';
                      };

                      failOpen = mkOption {
                        type = types.bool;
                        default = false;
                        description = ''
                          Allow requests to proceed if the policy service is unavailable or returns
                          an unexpected status code.

                          Warning: enabling this means restricted blobs may be served when the
                          policy service is unreachable.
                        '';
                      };

                      httpTimeout = mkOption {
                        type = types.str;
                        default = "30s";
                        description = ''
                          Maximum duration before policy service requests are timed out.
                        '';
                      };

                      httpConnectTimeout = mkOption {
                        type = types.str;
                        default = "10s";
                        description = ''
                          Maximum duration before an attempted connection to the policy service is aborted.

                          This value should be lower than {option}`settings.policy.httpTimeout`.
                        '';
                      };
                    };
                  };
                };

                default = { };
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

              systemd.services.porxie =
                let
                  knownKeys = [
                    "server"
                    "blob"
                    "identity"
                    "cache"
                    "policy"
                  ];
                  env =
                    (lib.filterAttrs (k: _: !(builtins.elem k knownKeys)) cfg.settings)
                    // lib.filterAttrs (_: v: v != null) {
                      PORXIE_SERVER_ADDRESS = cfg.settings.server.address;
                      PORXIE_SERVER_AUTH_TOKEN = cfg.settings.server.authToken;
                      PORXIE_BLOB_ALLOWED_MIMETYPES =
                        if cfg.settings.blob.allowedMimetypes != [ ] then
                          lib.concatStringsSep "," cfg.settings.blob.allowedMimetypes
                        else
                          null;
                      PORXIE_BLOB_MAX_SIZE = cfg.settings.blob.maxSize;
                      PORXIE_BLOB_CACHE_HEADER = cfg.settings.blob.cacheHeader;
                      PORXIE_BLOB_PROCESSING_TIMEOUT = cfg.settings.blob.processingTimeout;
                      PORXIE_BLOB_HTTP_TIMEOUT = cfg.settings.blob.httpTimeout;
                      PORXIE_BLOB_HTTP_CONNECT_TIMEOUT = cfg.settings.blob.httpConnectTimeout;
                      PORXIE_IDENTITY_PLC_URL = cfg.settings.identity.plcUrl;
                      PORXIE_IDENTITY_HTTP_TIMEOUT = cfg.settings.identity.httpTimeout;
                      PORXIE_IDENTITY_HTTP_CONNECT_TIMEOUT = cfg.settings.identity.httpConnectTimeout;
                      PORXIE_CACHE_ALLOCATION = cfg.settings.cache.allocation;
                      PORXIE_CACHE_BLOB_TTI = cfg.settings.cache.blobTti;
                      PORXIE_CACHE_OWNERSHIP_TTL = cfg.settings.cache.ownershipTtl;
                      PORXIE_CACHE_POLICY_TTL = cfg.settings.cache.policyTtl;
                      PORXIE_CACHE_IDENTITY_TTL = cfg.settings.cache.identityTtl;
                      PORXIE_POLICY_URL = cfg.settings.policy.url;
                      PORXIE_POLICY_REQUEST_HEADERS =
                        if cfg.settings.policy.requestHeaders != [ ] then
                          lib.concatStringsSep "|" cfg.settings.policy.requestHeaders
                        else
                          null;
                      PORXIE_POLICY_FAIL_OPEN = if cfg.settings.policy.failOpen then "true" else null;
                      PORXIE_POLICY_HTTP_TIMEOUT = cfg.settings.policy.httpTimeout;
                      PORXIE_POLICY_HTTP_CONNECT_TIMEOUT = cfg.settings.policy.httpConnectTimeout;
                    };
                in
                {
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
                    RestrictAddressFamilies = [
                      "AF_INET"
                      "AF_INET6"
                      "AF_UNIX"
                    ];
                    LockPersonality = true;
                    MemoryDenyWriteExecute = true;
                    RemoveIPC = true;
                    SystemCallArchitectures = "native";
                    SystemCallFilter = [
                      "@system-service"
                      "~@privileged"
                      "~@resources"
                    ];
                  };
                };
            };
          }
        );

        default = self.nixosModules.porxie;
      };
    };
}
