{
  description = "DMM - the Declarative Music Manager";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem(system:
      let
        fs = nixpkgs.lib.fileset;
        overlays = [ (import rust-overlay ) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        # tell crane to use this toolchain
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        # cf. https://crane.dev/API.html#libcleancargosource
        # src = craneLib.cleanCargoSource ./.;
        src = fs.toSource {
          root = ./.;
          fileset = fs.unions [
            ./assets/dmm.default.ron
            ./assets/gitignore
            ./assets/dmm.minimal.ron
            ./examples/sources/yt-dlp.ron
            ./assets/example-playlist.ron
            (fs.fromSource (craneLib.cleanCargoSource ./.))
          ];
        };
        # compile-time
        nativeBuildInputs = with pkgs; [ rustToolchain clang mold-wrapped pkg-config alsa-lib ];
        # runtime
        buildInputs = with pkgs; [ yt-dlp alsa-lib ]; # needed system libraries
        cargoArtifacts = craneLib.buildDepsOnly { inherit src buildInputs nativeBuildInputs; };
        bin = craneLib.buildPackage ({ inherit src buildInputs nativeBuildInputs cargoArtifacts; });
      in
      {
        packages = {
          # so bin can be spacifically built, or just by default
          inherit bin;
          default = bin;
        };
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          name = "dmm-dev";
          nativeBuildInputs = [
            pkgs.rust-analyzer-unwrapped
            pkgs.nodePackages.vscode-langservers-extracted
          ] ++ nativeBuildInputs;
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          shellHook = ''
          exec zsh
          '';
        };
      }
    );
}
