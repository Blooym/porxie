{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixpkgs-unstable";
    };
    jacquard = {
      url = "git+https://tangled.org/nonbinary.computer/jacquard?rev=dd2e2bbf6bcbfd5e9cf1727bddb828a3f0038802"; # 0.11
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
              nixsd
            ];
            env = {
              RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
            };
          };
        }
      );
    };
}
