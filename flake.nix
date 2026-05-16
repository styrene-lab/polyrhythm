{
  description = "polyrhythm — Rust e-drum MIDI mapping and live drum-rig stability tooling";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "polyrhythm";
            version = cargoToml.package.version;
            src = pkgs.lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            meta = with pkgs.lib; {
              description = "Rust e-drum MIDI mapping and live drum-rig stability tooling";
              homepage = "https://github.com/styrene-lab/polyrhythm";
              license = licenses.mit;
              mainProgram = "td50-drumgizmo-hihat-mapper-rs";
            };
          };
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustc
              cargo
              clippy
              rustfmt
              rust-analyzer
              pkg-config
              alsa-lib
              qpwgraph
              python3
            ];

            shellHook = ''
              echo "polyrhythm dev environment ready ($(rustc --version))"
            '';
          };
        }
      );
    };
}
