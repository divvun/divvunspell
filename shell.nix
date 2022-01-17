let
  fenix = import (fetchTarball "https://github.com/nix-community/fenix/archive/main.tar.gz") { };
  pkgs = import <nixpkgs> {};
in
  pkgs.mkShell {
    buildInputs = [
      (fenix.complete.withComponents [
        "cargo"
        "clippy"
        "rust-src"
        "rustc"
        "rustfmt"
      ])
      pkgs.openssl
      pkgs.pkg-config 
      pkgs.libtorch-bin
    ]; 
  }
