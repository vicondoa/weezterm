{
  description = "WeezTerm terminal emulator with remote SSH extensions";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
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
    # --- weezterm remote features ---
    # Explicit, pinned raw sources for the two d2b git dependencies consumed
    # by config/src/d2b.rs via Cargo.toml. Neither is a flake we build against
    # (no nixpkgs to follow), so these are plain `flake = false` sources, same
    # as freetype2/harfbuzz/libpng/zlib above. Pinning them as flake inputs
    # gives flake.lock narHash authority over the exact revision instead of
    # leaving it to an unbound, unlocked `cargoLock.allowBuiltinFetchGit`
    # fetch. Keep the revs in sync with the `d2b-client-toolkit` git rev in
    # Cargo.toml and the canonical d2b source rev asserted in
    # config/src/d2b.rs.
    d2b = {
      url = "github:vicondoa/d2b/9dc902243cdd7aba7ef269988b96f0aae6e037da";
      flake = false;
    };
    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit/926de54e7320599c373524a10b65aaf13b6ff422";
      flake = false;
    };
    # --- end weezterm remote features ---
  };

  outputs = inputs: (import ./nix/flake.nix).outputs inputs;
}
