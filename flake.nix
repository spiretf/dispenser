{
  inputs = {
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    nixpkgs.url = "nixpkgs/release-23.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.inputs.flake-utils.follows = "utils";
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    naersk,
    rust-overlay,
  }:
    utils.lib.eachDefaultSystem (system: let
      lib = nixpkgs.lib;
      overlays = [
        (import rust-overlay)
        (import ./overlay.nix)
      ];
      pkgs = (import nixpkgs) {
        inherit system overlays;
      };
      hostTarget = pkgs.hostPlatform.config;
      targets = ["x86_64-unknown-linux-musl" hostTarget];
      naerskForTarget = target: let
        toolchain = pkgs.rust-bin.stable.latest.default.override {targets = [target];};
      in
        pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };
      hostNaersk = naerskForTarget hostTarget;
      nearskOpt = {
        pname = "dispenser";

        inherit (pkgs.dispenser) src;

        nativeBuildInputs = with pkgs; [
          libsodium
          pkg-config
        ];
      };
    in rec {
      packages =
        (lib.attrsets.genAttrs targets (target: (naerskForTarget target).buildPackage nearskOpt))
        // rec {
          dispenser = pkgs.dispenser;
          check = hostNaersk.buildPackage (nearskOpt // {checkOnly = true;});
          test = hostNaersk.buildPackage (nearskOpt // {testOnly = true;});
          clippy = hostNaersk.buildPackage (nearskOpt // {clippyOnly = true;});
          dockerImage = pkgs.dockerTools.buildImage {
            name = "spiretf/dispenser";
            tag = "latest";
            copyToRoot = [dispenser];
            config = {
              Cmd = ["${dispenser}/bin/dispenser" "/config.toml"];
            };
          };
          default = dispenser;
        };

      devShells.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs;
          [
            rust-bin.stable.latest.default
            bacon
            skopeo
            cargo-edit
          ]
          ++ nearskOpt.nativeBuildInputs;
      };
    })
    // {
      overlays.default = import ./overlay.nix;
      nixosModules.default = {
        pkgs,
        config,
        lib,
        ...
      }: {
        imports = [./module.nix];
        config = lib.mkIf config.services.dispenser.enable {
          nixpkgs.overlays = [self.overlays.default];
          services.dispenser.package = lib.mkDefault pkgs.dispenser;
        };
      };
    };
}
