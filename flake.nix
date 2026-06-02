# SPDX-License-Identifier: EUPL-1.2
# inputs are managed by tack, see ./.tack

{
  description = "low footprint, derive-first CLI parser";

  outputs =
    { self }:
    let
      pins = import ./.tack;
      inherit (pins) nixpkgs fenix;
      inherit (nixpkgs) lib;
      forAllSystems = lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          # nightly rustfmt only, our .rustfmt.toml uses unstable options.
          # everything else stays on nixpkgs.
          nightlyRustfmt = fenix.packages.${system}.latest.rustfmt;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.taplo
            ];
            env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };

          # `nix develop .#fmt` formats the tree on entry
          fmt = pkgs.mkShellNoCC {
            packages = [
              pkgs.cargo
              nightlyRustfmt
              pkgs.taplo
              pkgs.nixfmt
            ];
            shellHook = ''
              cargo fmt
              taplo fmt
              find . -name '*.nix' -not -path './target/*' -exec nixfmt {} +
              exit
            '';
          };
        }
      );
    };
}
