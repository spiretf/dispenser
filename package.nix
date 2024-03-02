{
  stdenv,
  makeRustPlatform,
  libsodium,
  pkg-config,
  lib,
  rust-bin,
}: let
  inherit (lib.sources) sourceByRegex;
  rustPlatform = makeRustPlatform {
    cargo = rust-bin.stable.latest.minimal;
    rustc = rust-bin.stable.latest.minimal;
  };
  src = sourceByRegex ./. ["Cargo.*" "(src)(/.*)?"];
in
  rustPlatform.buildRustPackage rec {
    pname = "dispenser";
    version = "0.1.0";

    inherit src;

    buildInputs = [
      libsodium
    ];

    nativeBuildInputs = [
      pkg-config
    ];

    doCheck = false;

    cargoLock = {
      lockFile = ./Cargo.lock;
    };
  }
