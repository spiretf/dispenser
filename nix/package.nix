{ stdenv
, rustPlatform
, libsodium
, pkg-config
, lib
, rust-bin
,
}:
let
  inherit (lib.sources) sourceByRegex;
  inherit (builtins) fromTOML readFile;
  src = sourceByRegex ../. [ "Cargo.*" "(src)(/.*)?" ];
  cargoPackage = (fromTOML (readFile ../Cargo.toml)).package;
in
rustPlatform.buildRustPackage rec {
  pname = cargoPackage.name;
  inherit (cargoPackage) version;

  inherit src;

  buildInputs = [
    libsodium
  ];

  nativeBuildInputs = [
    pkg-config
  ];

  doCheck = false;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };
}
