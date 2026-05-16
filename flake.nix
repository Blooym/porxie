{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
  };
  outputs =
    { self, nixpkgs, ... }:
    let
      forAllSystems =
        function:
        nixpkgs.lib.genAttrs nixpkgs.lib.systems.flakeExposed (
          system: function system nixpkgs.legacyPackages.${system}
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
              rust-jemalloc-sys
            ];
            env = {
              JEMALLOC_OVERRIDE = pkgs.rust-jemalloc-sys; # https://github.com/NixOS/nixpkgs/issues/370494
              RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
            };
          };
        }
      );
    };
}
