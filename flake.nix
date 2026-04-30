{
  description = "Porxie, an ATProto blob proxy for secure content delivery";

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

      packages = forAllSystems (
        system: pkgs: {
          porxie = pkgs.lib.warn "using the porxie flake directly is deprecated; use porxie from nixpkgs instead" pkgs.porxie;
          default = pkgs.lib.warn "using the porxie flake directly is deprecated; use porxie from nixpkgs instead" pkgs.porxie;
        }
      );

      nixosModules = {
        porxie = nixpkgs.lib.warn "using the porxie flake directly is deprecated; use porxie from nixpkgs instead" nixpkgs.nixosModules.porxie;
        default = nixpkgs.lib.warn "using the porxie flake directly is deprecated; use porxie from nixpkgs instead" nixpkgs.nixosModules.porxie;
      };
    };
}
