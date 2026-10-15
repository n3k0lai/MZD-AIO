{
  description = "mzd-tweaks: declarative tweak bundles for the Mazda Connect CMU";

  # One pinned input. flake.lock records its exact revision and content hash.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        default = mzd-tweaks;
        mzd-tweaks = pkgs.callPackage ./nix/package.nix { };
        mzd-tweaks-serial = pkgs.callPackage ./nix/package.nix { withSerial = true; };
        # Starting example: the CMU 2 check bundle, built reproducibly.
        example-bundle = self.lib.mkCmuBundle pkgs {
          src = ./examples;
          config = "cmu2-check.toml";
        };
      });

      lib.mkCmuBundle =
        pkgs:
        pkgs.callPackage ./nix/bundle.nix {
          mzd-tweaks = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };

      overlays.default = final: _prev: {
        mzd-tweaks = final.callPackage ./nix/package.nix { };
      };

      nixosModules.default = ./nix/module.nix;

      checks = forAllSystems (
        pkgs:
        let
          pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          # cargo test, including the generated run.sh under BusyBox.
          package = pkg;
          serial = self.packages.${pkgs.stdenv.hostPlatform.system}.mzd-tweaks-serial;
          clippy = pkg.overrideAttrs (old: {
            pname = "mzd-tweaks-clippy";
            nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.clippy ];
            buildPhase = "cargo clippy --offline --workspace --all-targets --all-features -- -D warnings";
            doCheck = false;
            installPhase = "touch $out";
          });
          fmt =
            pkgs.runCommand "mzd-tweaks-fmt"
              {
                nativeBuildInputs = [
                  pkgs.cargo
                  pkgs.rustfmt
                ];
              }
              ''
                cd ${./.} && cargo fmt --check && touch $out
              '';
          example-bundle = self.packages.${pkgs.stdenv.hostPlatform.system}.example-bundle;
        }
        // lib.optionalAttrs (pkgs.stdenv.hostPlatform.system == "x86_64-linux") {
          vm = import ./nix/tests/vm.nix { inherit pkgs; };
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
