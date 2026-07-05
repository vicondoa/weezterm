{
  description = "WeezTerm terminal emulator with remote SSH extensions";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit/terminal-integration-toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Keep these in sync with nix/flake.nix; the root flake delegates
    # outputs there while providing a repository-root entry point.
    freetype2 = {
      url = "github:freetype/freetype/VER-2-13-3";
      flake = false;
    };
    harfbuzz = {
      url = "github:harfbuzz/harfbuzz/11.2.1";
      flake = false;
    };
    libpng = {
      url = "github:pnggroup/libpng/v1.6.44";
      flake = false;
    };
    zlib = {
      url = "github:madler/zlib/v1.3.1";
      flake = false;
    };
  };

  outputs = inputs: (import ./nix/flake.nix).outputs inputs;
}
