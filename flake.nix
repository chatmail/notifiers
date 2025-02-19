{
  description = "Notification service";
  inputs = {
    fenix.url = "github:nix-community/fenix";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, naersk, fenix }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      formatter = nixpkgs.lib.genAttrs supportedSystems (system:
        let
          pkgs = nixpkgs.legacyPackages."${system}";
        in
        pkgs.nixpkgs-fmt
      );

      packages = nixpkgs.lib.genAttrs supportedSystems (system:
        let
          pkgs = nixpkgs.legacyPackages."${system}";
          buildNotifiers = (arch:
            let
              crossSystem = "${arch}-unknown-linux-musl";
              pkgsCross = import nixpkgs ({
                system = system;
                crossSystem.config = crossSystem;
              });
              rustToolchain = with fenix.packages.${system};
                combine [
                  stable.rustc
                  stable.cargo
                  targets.${crossSystem}.stable.rust-std
                ];

              naersk' = pkgs.callPackage naersk {
                cargo = rustToolchain;
                rustc = rustToolchain;
              };
            in
            naersk'.buildPackage rec {
              pname = "notifiers";
              src = ./.;
              nativeBuildInputs = [
                pkgs.pkg-config
              ];

              TARGET_CC = "${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}cc";
              CARGO_BUILD_RUSTFLAGS = [ "-C" "linker=${TARGET_CC}" ];

              OPENSSL_STATIC = 1;
              OPENSSL_LIB_DIR = "${pkgsCross.pkgsStatic.openssl.out}/lib";
              OPENSSL_INCLUDE_DIR = "${pkgsCross.pkgsStatic.openssl.dev}/include";
              CARGO_BUILD_TARGET = crossSystem;
            }
          );
        in
        rec {
          default = x86_64;
          x86_64 = buildNotifiers "x86_64";
          aarch64 = buildNotifiers "aarch64";
        }
      );
    };
}
