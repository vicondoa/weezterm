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
    # --- weezterm remote features ---
    # Keep these two in sync with the root flake.nix; see the comment there
    # for why they are pinned as `flake = false` inputs instead of relying on
    # `cargoLock.allowBuiltinFetchGit`.
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

  outputs =
    inputs@{ self, ... }:
    inputs.flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import inputs.rust-overlay) ];
        pkgs = import (inputs.nixpkgs) { inherit system overlays; };

        inherit (inputs.nixpkgs) lib;
        inherit (pkgs) stdenv;

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
          lib.optionals stdenv.isLinux (
            with pkgs;
            [
              libxcb-image
              libGL
              wayland
              vulkan-loader
            ]
          )
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
          cargo = pkgs.rust-bin.stable."1.96.0".minimal;
          rustc = pkgs.rust-bin.stable."1.96.0".minimal;
        };

        # --- weezterm remote features ---
        waylandTitleTools = with pkgs; [
          coreutils
          findutils
          grim
          imagemagick
          jq
          niri
          gnugrep
          gnused
          shellcheck
          tesseract
          weston
        ];
        waylandTitleLibPath = lib.makeLibraryPath [ pkgs.mesa ] + ":${runtimeLibPath}";
        waylandTitleEglVendor = "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
        waylandTitleTest =
          if stdenv.isLinux then
            pkgs.writeShellApplication {
              name = "niri-title-test";
              runtimeInputs = waylandTitleTools;
              text = ''
                export WEEZTERM_TITLE_REPO_ROOT="''${WEEZTERM_TITLE_REPO_ROOT:-$PWD}"
                export WEEZTERM_TITLE_NIRI_CONFIG=${../tests/wayland-title/niri.kdl}
                export LD_LIBRARY_PATH="${waylandTitleLibPath}:''${LD_LIBRARY_PATH:-}"
                export __EGL_VENDOR_LIBRARY_FILENAMES="${waylandTitleEglVendor}"
                export LIBGL_DRIVERS_PATH="${pkgs.mesa}/lib/dri"
                exec ${../tests/wayland-title/run.sh} "$@"
              '';
            }
          else
            null;
        # --- end weezterm remote features ---

        prebuiltManifest = builtins.fromJSON (builtins.readFile ./prebuilt.json);
        hasPrebuilt =
          prebuiltManifest.version != null
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
            stdenv.cc.cc.lib
            openssl
            fontconfig
            libGL
            libxkbcommon
            wayland
            vulkan-loader
            xorg.libxcb
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.xcbutil
            xorg.xcbutilimage
            xorg.xcbutilkeysyms
            xorg.xcbutilwm
            zlib
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
          # --- weezterm remote features ---
          version = "0.7.2";

          cargoLock = {
            lockFile = ../Cargo.lock;
            # --- weezterm remote features ---
            # `allowBuiltinFetchGit` remains only as the fallback path for the
            # other git dependencies in Cargo.lock (xcb-imdkit, finl_unicode)
            # that this fix does not pin. The four d2b/d2b-toolkit crates
            # below get an explicit `outputHashes` entry instead, so their
            # vendoring goes through the hermetic `fetchgit` fixed-output
            # derivation path rather than the impure, unlocked
            # `builtins.fetchGit` fallback. The hash is the pinned `d2b` /
            # `d2b-toolkit` flake input's own `narHash`, which is byte-for-
            # byte identical to what `fetchgit` (default `leaveDotGit =
            # false`) computes for the same url+rev -- verified empirically
            # before wiring this in. This makes flake.lock the narHash
            # authority for these revisions instead of an ad-hoc unbound
            # fetch, without needing a second, divergent content fetch.
            outputHashes = {
              "d2b-client-2.0.0" = inputs.d2b.narHash;
              "d2b-contracts-2.0.0" = inputs.d2b.narHash;
              "d2b-session-2.0.0" = inputs.d2b.narHash;
              "d2b-client-toolkit-2.0.0" = inputs.d2b-toolkit.narHash;
            };
            allowBuiltinFetchGit = true;
            # --- end weezterm remote features ---
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
        # Hermetic lint/format coverage for `nix build .#source`. Both reuse
        # `sourcePackage`'s src/patch/cargoLock (so the same vendored,
        # hash-pinned d2b/d2b-toolkit sources are linted, not a second
        # fetch), but replace buildPhase/installPhase so neither runs the
        # full install (icons, completions, terminfo, etc.) -- they only
        # need to fail the build on a format or lint violation.
        checks = {
          cargo-fmt = sourcePackage.overrideAttrs (old: {
            name = "wezterm-cargo-fmt-check";
            nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.rust-bin.nightly."2026-06-06".rustfmt ];
            doCheck = false;
            # No binaries get installed, so there is nothing for the
            # inherited preFixup/postFixup patchelf steps to act on.
            dontFixup = true;
            buildPhase = ''
              runHook preBuild
              cargo fmt --all -- --check
              runHook postBuild
            '';
            installPhase = "touch $out";
          });

          cargo-clippy = sourcePackage.overrideAttrs (old: {
            name = "wezterm-cargo-clippy-check";
            nativeBuildInputs = old.nativeBuildInputs ++ [
              (pkgs.rust-bin.stable."1.96.0".minimal.override { extensions = [ "clippy" ]; })
              pkgs.jq
            ];
            doCheck = false;
            dontFixup = true;
            buildPhase = ''
              runHook preBuild
              # The vendored upstream wezterm/config tree carries pre-existing
              # clippy findings this seam does not own and must not "fix" as
              # a side effect (see AGENTS.md: never reformat/rewrite upstream
              # files). A blanket `--workspace -D warnings` run fails on code
              # this seam never touched (even without `-D warnings`, some
              # upstream files trip deny-by-default lints). Run clippy for
              # just the `config` crate with `--no-deps` (so upstream
              # workspace deps like wezterm-char-props aren't linted
              # either), capture full diagnostics, and hard-fail only on
              # findings inside config/src/d2b.rs -- the file this seam
              # wholly owns and maintains.
              cargo clippy -p config --all-targets --no-deps --offline --message-format=json > clippy-config.json || true
              if jq -e '
                  select(.reason == "compiler-message")
                  | .message.spans[]?
                  | select(.file_name == "config/src/d2b.rs")
                ' clippy-config.json > /dev/null
              then
                echo "clippy findings in config/src/d2b.rs:" >&2
                jq -r '
                    select(.reason == "compiler-message")
                    | select(.message.spans[]?.file_name == "config/src/d2b.rs")
                    | .message.rendered
                  ' clippy-config.json >&2
                exit 1
              fi
              runHook postBuild
            '';
            installPhase = "touch $out";
          });
        };
        # --- end weezterm remote features ---

        # --- weezterm remote features ---
        apps = {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/weezterm";
          };
        }
        // lib.optionalAttrs stdenv.isLinux {
          niri-title-test = {
            type = "app";
            program = "${waylandTitleTest}/bin/niri-title-test";
          };
        };
        # --- end weezterm remote features ---

        devShell = pkgs.mkShell {
          name = "wezterm-shell";
          inherit nativeBuildInputs;

          buildInputs =
            buildInputs
            ++ (with pkgs.rust-bin; [
              (stable."1.96.0".minimal.override {
                extensions = [
                  "clippy"
                  "rust-src"
                ];
              })
              nightly."2026-06-06".rustfmt
              nightly."2026-06-06".rust-analyzer
            ])
            # --- weezterm remote features ---
            # `make precommit`/`make test` need `cargo nextest`; there is no
            # rustup in this shell to `cargo install` it on demand, so pin it
            # from nixpkgs alongside the rest of the toolchain.
            ++ [ pkgs.cargo-nextest ];
          # --- end weezterm remote features ---

          LD_LIBRARY_PATH = libPath;
        };

        # --- weezterm remote features ---
        devShells = {
          default = self.devShell.${system};
        }
        // lib.optionalAttrs stdenv.isLinux {
          wayland-title = pkgs.mkShell {
            packages = waylandTitleTools;
            LD_LIBRARY_PATH = waylandTitleLibPath;
            __EGL_VENDOR_LIBRARY_FILENAMES = waylandTitleEglVendor;
            LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
          };
        };
        # --- end weezterm remote features ---

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
