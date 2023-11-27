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
    let
      overlays = [ (import rust-overlay ) ];
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system overlays;
      };
      rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      # tell crane to use this toolchain
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
      # cf. https://crane.dev/API.html#libcleancargosource
      src = craneLib.cleanCargoSource ./.;
      # compile-time
      nativeBuildInputs = with pkgs; [ rustToolchain pkg-config yt-dlp ];
      # runtime
      buildInputs = with pkgs; [ yt-dlp ]; # needed system libraries
      cargoArtifacts = craneLib.buildDepsOnly { inherit src buildInputs nativeBuildInputs; };
      bin = craneLib.buildPackage ({ inherit src buildInputs nativeBuildInputs cargoArtifacts; });
    in
    {
      packages.${system} = {
        # so bin can be spacifically built, or just by default
        inherit bin;
        default = bin;
      };
      devShells.${system}.default = pkgs.mkShell {
        inherit buildInputs;
        nativeBuildInputs = [
          pkgs.rust-analyzer-unwrapped
          pkgs.nodePackages.vscode-langservers-extracted
        ] ++ nativeBuildInputs;
        RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        shellHook = ''
        exec zsh
        '';
      };
    };
}
