{
  description = "A GPU-accelerated cross-platform terminal emulator and multiplexer written by @wez and implemented in Rust";

  nixConfig = {
    extra-substituters = [ "https://vicondoa.github.io/weezterm" ];
    extra-trusted-public-keys = [ "vicondoa-weezterm:ngBOtTKVGlEGkoDHTpGQZFN/amcKwleX0XZH28HIM5s=" ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    d2b-toolkit = {
      url = "github:vicondoa/d2b-toolkit";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # NOTE: @2024-05 Nix flakes does not support getting git submodules of 'self'.
    # refs:
    # - https://discourse.nixos.org/t/get-nix-flake-to-include-git-submodule/30324
    # - https://github.com/NixOS/nix/pull/7862
    #
    # ... In the meantime we kinda duplicate the dependencies here then replace the submodules with
    # links to each repo in package sources.
    #
    # Try to use tags when possible to increase readability
    # (note: `git submodule status` in wezterm repo will show the `git describe` result for each
    # submodule, can help finding a tag if any)
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

  outputs =
    inputs@{ self, ... }:
    inputs.flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import inputs.rust-overlay) ];
        pkgs = import (inputs.nixpkgs) { inherit system overlays; };

        inherit (inputs.nixpkgs) lib;
        inherit (pkgs) stdenv;
        toolkitSource = inputs.d2b-toolkit.packages.${system}.default;

        nativeBuildInputs =
          with pkgs;
          [
            installShellFiles
            ncurses # tic for terminfo
            pkg-config
            python3
          ]
          ++ lib.optional stdenv.isDarwin perl;

        buildInputs =
          with pkgs;
          [
            fontconfig
            openssl
            zlib
          ]
          ++ lib.optionals stdenv.isLinux [
            libxkbcommon
            wayland

            libx11
            libxcb
            libxcb-util
            libxcb-image
            libxcb-keysyms
            libxcb-wm # contains xcb-ewmh among others
          ]
          ++ lib.optionals stdenv.isDarwin ([
            libiconv
          ]);

        libPath = lib.makeLibraryPath (
          with pkgs;
          [
            libxcb-image
            libGL
            wayland
            vulkan-loader
          ]
        );
        runtimeLibPath = lib.makeLibraryPath (
          with pkgs;
          [
            libxcb-image
            libGL
            libxkbcommon
            libx11
            libxcb
            libxcb-util
            wayland
            vulkan-loader
          ]
        );

        rustPlatform = pkgs.makeRustPlatform {
          cargo = pkgs.rust-bin.stable.latest.minimal;
          rustc = pkgs.rust-bin.stable.latest.minimal;
        };

        prebuiltManifest = builtins.fromJSON (builtins.readFile ./prebuilt.json);
        hasPrebuilt = prebuiltManifest.version != null
          && prebuiltManifest.binaries ? weezterm
          && system == prebuiltManifest.system;

        prebuiltPackage = pkgs.stdenv.mkDerivation {
          pname = "weezterm";
          version = prebuiltManifest.version;
          src = pkgs.fetchurl {
            inherit (prebuiltManifest.binaries.weezterm) url hash;
          };
          nativeBuildInputs = [ pkgs.autoPatchelfHook ];
          buildInputs = with pkgs; [
            stdenv.cc.cc.lib openssl fontconfig libGL libxkbcommon wayland
            vulkan-loader xorg.libxcb xorg.libX11 xorg.libXcursor
            xorg.libXrandr xorg.libXi xorg.xcbutil xorg.xcbutilimage
            xorg.xcbutilkeysyms xorg.xcbutilwm zlib
          ];
          sourceRoot = ".";
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            mkdir -p $out
            cp -a bin share $out/ 2>/dev/null || cp -a bin $out/
            cp -a etc $out/ 2>/dev/null || true
          '';
          meta.mainProgram = "weezterm";
        };

        sourcePackage = rustPlatform.buildRustPackage rec {
          inherit buildInputs nativeBuildInputs;

          # --- weezterm remote features ---
          name = "weezterm";
          # --- end weezterm remote features ---
          src = ./..;
          version = self.shortRev or "dev";

          cargoLock = {
            lockFile = ../Cargo.lock;
            allowBuiltinFetchGit = true;
          };

          prePatch = ''
            rm -rf deps/freetype/{freetype2,libpng,zlib} deps/harfbuzz/harfbuzz

            ln -s ${inputs.freetype2} deps/freetype/freetype2
            ln -s ${inputs.libpng} deps/freetype/libpng
            ln -s ${inputs.zlib} deps/freetype/zlib
            ln -s ${inputs.harfbuzz} deps/harfbuzz/harfbuzz
          '';

          postPatch = ''
            echo ${version} > .tag

            substituteInPlace mux/Cargo.toml \
              --replace-fail "../../d2b-toolkit/crates/d2b-client" \
                "${toolkitSource}/share/d2b-toolkit/crates/d2b-client" \
              --replace-fail "../../d2b-toolkit/crates/d2b-toolkit-core" \
                "${toolkitSource}/share/d2b-toolkit/crates/d2b-toolkit-core"

            # tests are failing with: Unable to exchange encryption keys
            rm -r wezterm-ssh/tests

            # hash does not work well with NixOS
            substituteInPlace assets/shell-integration/wezterm.sh \
              --replace-fail 'hash wezterm 2>/dev/null' 'command type -P weezterm &>/dev/null' \
              --replace-fail 'wezterm set-working-directory' 'weezterm set-working-directory' \
              --replace-fail 'hash base64 2>/dev/null' 'command type -P base64 &>/dev/null' \
              --replace-fail 'hash hostname 2>/dev/null' 'command type -P hostname &>/dev/null' \
              --replace-fail 'hash hostnamectl 2>/dev/null' 'command type -P hostnamectl &>/dev/null'

            # --- weezterm remote features ---
            substituteInPlace assets/shell-completion/bash \
              --replace-fail 'wezterm' 'weezterm'
            substituteInPlace assets/shell-completion/fish \
              --replace-fail 'wezterm' 'weezterm'
            substituteInPlace assets/shell-completion/zsh \
              --replace-fail 'wezterm' 'weezterm'
            substituteInPlace assets/wezterm-nautilus.py \
              --replace-fail "cmd = ['wezterm', 'start', '--cwd', path]" "cmd = ['weezterm', 'start', '--cwd', path]" \
              --replace-fail 'org.wezfurlong.wezterm' 'com.vicondoa.weezterm'
            # --- end weezterm remote features ---
          '';

          # Disable cargo-auditable until https://github.com/rust-secure-code/cargo-auditable/issues/124 is fixed
          auditable = false;

          preFixup =
            lib.optionalString stdenv.isLinux /* bash */ ''
              patchelf \
                --add-needed "${pkgs.libGL}/lib/libEGL.so.1" \
                --add-needed "${pkgs.vulkan-loader}/lib/libvulkan.so.1" \
                $out/bin/weezterm-gui
            ''
            + lib.optionalString stdenv.isDarwin /* bash */ ''
              mkdir -p "$out/Applications"
              OUT_APP="$out/Applications/WezTerm.app"
              cp -r assets/macos/WezTerm.app "$OUT_APP"
              rm $OUT_APP/*.dylib
              cp -r assets/shell-integration/* "$OUT_APP"
              substituteInPlace "$OUT_APP/Contents/Info.plist" \
                --replace-fail '<string>wezterm-gui</string>' '<string>weezterm-gui</string>'
              # macOS will only recognize our application bundle
              # if the binaries are inside of it. Move them there
              # and create symbolic links for them in bin/.
              mv $out/bin/{weezterm,weezterm-mux-server,weezterm-gui,strip-ansi-escapes} "$OUT_APP"
              ln -s "$OUT_APP"/{weezterm,weezterm-mux-server,weezterm-gui,strip-ansi-escapes} "$out/bin"
            '';

          postFixup = lib.optionalString stdenv.isLinux /* bash */ ''
            for bin in weezterm weezterm-mux-server weezterm-gui; do
              if [ -x "$out/bin/$bin" ]; then
                patchelf --set-rpath "${runtimeLibPath}:$(patchelf --print-rpath "$out/bin/$bin")" "$out/bin/$bin"
              fi
            done
          '';

          postInstall = ''
            mkdir -p $out/nix-support
            echo "${passthru.terminfo}" >> $out/nix-support/propagated-user-env-packages

            # --- weezterm remote features ---
            # Prefer weezterm-branded desktop/appdata/icon if available
            if [ -f assets/icon/weezterm/terminal.png ]; then
              install -Dm644 assets/icon/weezterm/terminal.png $out/share/icons/hicolor/128x128/apps/com.vicondoa.weezterm.png
            else
              install -Dm644 assets/icon/terminal.png $out/share/icons/hicolor/128x128/apps/org.wezfurlong.wezterm.png
            fi
            if [ -f assets/weezterm.desktop ]; then
              install -Dm644 assets/weezterm.desktop $out/share/applications/com.vicondoa.weezterm.desktop
            else
              install -Dm644 assets/wezterm.desktop $out/share/applications/org.wezfurlong.wezterm.desktop
            fi
            if [ -f assets/weezterm.appdata.xml ]; then
              install -Dm644 assets/weezterm.appdata.xml $out/share/metainfo/com.vicondoa.weezterm.appdata.xml
            else
              install -Dm644 assets/wezterm.appdata.xml $out/share/metainfo/org.wezfurlong.wezterm.appdata.xml
            fi
            # --- end weezterm remote features ---

            install -Dm644 assets/shell-integration/wezterm.sh -t $out/etc/profile.d
            installShellCompletion --cmd weezterm \
              --bash assets/shell-completion/bash \
              --fish assets/shell-completion/fish \
              --zsh assets/shell-completion/zsh

            install -Dm644 assets/wezterm-nautilus.py -t $out/share/nautilus-python/extensions
          '';

          passthru = {
            # the headless variant is useful when deploying wezterm's mux server on remote severs
            headless = rustPlatform.buildRustPackage {
              pname = "wezterm-headless";
              inherit
                version
                src
                postPatch
                cargoLock
                meta
                ;

              nativeBuildInputs = [ pkgs.pkg-config ];

              buildInputs = [ pkgs.openssl ];

              cargoBuildFlags = [
                "--package"
                "wezterm"
                "--package"
                "wezterm-mux-server"
              ];

              doCheck = false;

              postInstall = ''
                install -Dm644 assets/shell-integration/wezterm.sh -t $out/etc/profile.d
                install -Dm644 ${passthru.terminfo}/share/terminfo/w/wezterm -t $out/share/terminfo/w
              '';
            };

            terminfo =
              pkgs.runCommand "wezterm-terminfo"
                {
                  nativeBuildInputs = [ pkgs.ncurses ];
                }
                ''
                  mkdir -p $out/share/terminfo $out/nix-support
                  tic -x -o $out/share/terminfo ${src}/termwiz/data/wezterm.terminfo
                '';
          };

          # --- weezterm remote features ---
          meta.mainProgram = "weezterm";
        };
      in
      {
        packages.default = if hasPrebuilt then prebuiltPackage else sourcePackage;
        packages.source = sourcePackage;

        # --- weezterm remote features ---
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/weezterm";
        };
        # --- end weezterm remote features ---

        devShell = pkgs.mkShell {
          name = "wezterm-shell";
          inherit nativeBuildInputs;

          buildInputs =
            buildInputs
            ++ (with pkgs.rust-bin; [
              (stable.latest.minimal.override {
                extensions = [
                  "clippy"
                  "rust-src"
                ];
              })

              nightly.latest.rustfmt
              nightly.latest.rust-analyzer
            ]);

          LD_LIBRARY_PATH = libPath;
        };

        # --- weezterm remote features ---
        devShells.default = self.devShell.${system};

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
