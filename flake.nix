{
  description = "climax CLI/TUI workspace";

  # `tack init` populates ./.tack with your pins
  outputs =
    { self, ... }@args:
    let
      inputs = (import ./.tack) { overrides = args.tackOverrides or { }; };
      inherit (inputs) fenix nixpkgs;
      inherit (nixpkgs) lib;
      forAllSystems = lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      pkgsFor = system: nixpkgs.legacyPackages.${system} or (import nixpkgs { inherit system; });
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          # The workspace rustfmt config uses unstable options; keep only
          # rustfmt on nightly and leave the compiler/tooling on nixpkgs.
          nightlyRustfmt =
            if fenix.packages ? ${system} then fenix.packages.${system}.latest.rustfmt else pkgs.rustfmt;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              nixfmt
              rustc
              rust-analyzer
              rustfmt
              taplo
            ];
            env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };

          # `nix develop .#fmt` formats the workspace and exits.
          fmt = pkgs.mkShellNoCC {
            packages = [
              pkgs.cargo
              pkgs.nixfmt
              nightlyRustfmt
              pkgs.taplo
            ];
            shellHook = ''
              cargo fmt --all
              taplo fmt
              find . -name '*.nix' -not -path './target/*' -exec nixfmt {} +
              exit
            '';
          };
        }
      );
    };
}
