{
  stdenv,
  rustPlatform,
  libsodium,
  pkg-config,
  lib,
}: let
  inherit (lib.sources) sourceByRegex;
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

    cargoLock = {
      lockFile = ./Cargo.lock;
    };
  }
