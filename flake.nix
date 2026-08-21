{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixpkgs-unstable";
    };
    jacquard = {
      url = "git+https://tangled.org/nonbinary.computer/jacquard?rev=714a5da87d012aca82f48522ea441ca2a49752c2"; # 0.12.1
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    {
      self,
      nixpkgs,
      jacquard,
      ...
    }:
    let
      forAllSystems =
        function:
        nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (
          system: function system nixpkgs.legacyPackages.${system} jacquard.packages.${system}
        );
    in
    {
      devShells = forAllSystems (
        system: pkgs: jacquard-pkgs: {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              rust-jemalloc-sys
              jacquard-pkgs.jacquard-lexgen

              # Nix
              nil
              nixd
            ];
            env = {
              RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
            };
          };
        }
      );
    };
}
