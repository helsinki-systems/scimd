{ pkgs ? import <nixpkgs> {} }: let
  unstable = pkgs.unstable or (import <unstable> { });
  inherit (unstable) rustPackages;
in pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    # Compiler and linker
    rustPackages.rustc
    clang_15
    llvmPackages.lld
    # Native dependencies
    pkg-config
    rustPackages.cargo
    # Utilities
    cargo-deny
    cargo-watch
    cargo-outdated
    cargo-machete
    cargo-expand
    rustPackages.clippy
    rustPackages.rustfmt
  ];
  buildInputs = with pkgs; [
    openssl
  ];

  RUST_SRC_PATH = "${rustPackages.rustPlatform.rustLibSrc}";
}
