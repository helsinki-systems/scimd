{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05-small";
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable-small";
  };

  outputs =
    {
      nixpkgs,
      nixpkgs-unstable,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        unstable = import nixpkgs-unstable { inherit system; };
        inherit (unstable) rustPackages;

        nativeBuildInputs = with pkgs; [
          # Compiler and linker
          rustPackages.rustc
          llvmPackages.clang
          llvmPackages.lld
          rustPackages.cargo
          # Utilities
          unstable.cargo-deny
          unstable.cargo-watch
          unstable.cargo-outdated
          unstable.cargo-machete
          unstable.cargo-expand
          rustPackages.clippy
          rustPackages.rustfmt
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs;

          RUST_SRC_PATH = "${rustPackages.rustPlatform.rustLibSrc}";
        };
      }
    );
}
