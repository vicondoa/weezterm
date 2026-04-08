#!/usr/bin/env bash
# --- weezterm remote features ---
# Build WeezTerm for both Windows (native) and Linux (via WSL), then
# assemble a ready-to-test package with the auto-install-mux feature.
#
# Usage (from Git Bash / MSYS2 on Windows):
#   ./ci/build-cross.sh              # debug build
#   ./ci/build-cross.sh --release    # release build
#   WSL_DISTRO=Ubuntu-24.04 ./ci/build-cross.sh  # pick WSL distro
#
# Output:  target/cross-pkg/
#   ├── windows/          Windows binaries (wezterm.exe, wezterm-gui.exe, …)
#   └── linux-x86_64/     Linux binaries  (wezterm, wezterm-mux-server)
#
# Then in wezterm.lua:
#   ssh_domains = {{
#     name = "myhost",
#     remote_address = "myhost",
#     remote_install_binaries_dir = "C:/src/weezterm/target/cross-pkg/linux-x86_64",
#   }}
# --- end weezterm remote features ---
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

# ── Configuration ────────────────────────────────────────────────────
PROFILE="debug"
CARGO_FLAG=""
if [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    CARGO_FLAG="--release"
fi

WSL_DISTRO="${WSL_DISTRO:-Ubuntu-24.04}"
PKG_DIR="$ROOT_DIR/target/cross-pkg"
WIN_DIR="$PKG_DIR/windows"
LINUX_DIR="$PKG_DIR/linux-x86_64"

echo "══════════════════════════════════════════════════════════════"
echo "  WeezTerm cross-build  (profile=$PROFILE, wsl=$WSL_DISTRO)"
echo "══════════════════════════════════════════════════════════════"

# ── Step 1: Build Windows binaries ───────────────────────────────────
echo ""
echo "── [1/4] Building Windows binaries ──────────────────────────"
export PATH="/c/Strawberry/perl/bin:$HOME/.cargo/bin:$PATH"

cargo build $CARGO_FLAG \
    -p wezterm \
    -p wezterm-gui \
    -p wezterm-mux-server \
    -p strip-ansi-escapes

echo "   ✓ Windows build complete"

# ── Step 2: Build Linux binaries via WSL ─────────────────────────────
echo ""
echo "── [2/4] Building Linux binaries via WSL ($WSL_DISTRO) ──────"

# Convert the repo root to a WSL-accessible path
WIN_ROOT="$(cygpath -w "$ROOT_DIR")"
WSL_ROOT="/mnt/$(echo "$WIN_ROOT" | sed 's|\\|/|g' | sed 's|^\([A-Za-z]\):|/\L\1|')"
# Simplify: strip double slash that cygpath may produce
WSL_ROOT="$(echo "$WSL_ROOT" | sed 's|//|/|g')"

echo "   Repo in WSL: $WSL_ROOT"

# Install Rust in WSL if needed, then build
wsl.exe -d "$WSL_DISTRO" -- bash -lc "
set -euo pipefail
cd '$WSL_ROOT'

# Ensure basic build deps
if ! command -v cargo &>/dev/null; then
    echo '   Installing Rust in WSL...'
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source \"\$HOME/.cargo/env\"
fi
source \"\$HOME/.cargo/env\" 2>/dev/null || true

# Install system build deps if missing (for OpenSSL, etc.)
if ! dpkg -s libssl-dev &>/dev/null 2>&1; then
    echo '   Installing build dependencies in WSL...'
    sudo apt-get update -qq
    sudo apt-get install -y -qq build-essential pkg-config libssl-dev \
        libfontconfig1-dev libwayland-dev libx11-xcb-dev libxkbcommon-dev \
        libxkbcommon-x11-dev libxcb-ewmh-dev libxcb-icccm4-dev \
        libxcb-image0-dev libxcb-keysyms1-dev libxcb-render0-dev \
        libxcb-xkb-dev
fi

echo '   Building Linux binaries...'
cargo build $CARGO_FLAG -p wezterm -p wezterm-mux-server
echo '   ✓ Linux/WSL build complete'
"

# ── Step 3: Assemble package ─────────────────────────────────────────
echo ""
echo "── [3/4] Assembling cross-package ───────────────────────────"

rm -rf "$PKG_DIR"
mkdir -p "$WIN_DIR" "$LINUX_DIR"

# Windows binaries
cp "target/$PROFILE/weezterm.exe"           "$WIN_DIR/"
cp "target/$PROFILE/weezterm-gui.exe"       "$WIN_DIR/"
cp "target/$PROFILE/weezterm-mux-server.exe" "$WIN_DIR/"
cp "target/$PROFILE/strip-ansi-escapes.exe" "$WIN_DIR/"

# Optional Windows DLLs
cp assets/windows/conhost/conpty.dll     "$WIN_DIR/" 2>/dev/null || true
cp assets/windows/conhost/OpenConsole.exe "$WIN_DIR/" 2>/dev/null || true
cp assets/windows/angle/libEGL.dll       "$WIN_DIR/" 2>/dev/null || true
cp assets/windows/angle/libGLESv2.dll    "$WIN_DIR/" 2>/dev/null || true

# Linux binaries — read from WSL's build output
WSL_TARGET="$WSL_ROOT/target/$PROFILE"
wsl.exe -d "$WSL_DISTRO" -- bash -c "
    cp '$WSL_TARGET/weezterm'           '$WSL_ROOT/target/cross-pkg/linux-x86_64/'
    cp '$WSL_TARGET/weezterm-mux-server' '$WSL_ROOT/target/cross-pkg/linux-x86_64/'
    chmod +x '$WSL_ROOT/target/cross-pkg/linux-x86_64/'*
"

echo "   ✓ Package assembled"

# ── Step 4: Print summary ───────────────────────────────────────────
echo ""
echo "── [4/4] Summary ────────────────────────────────────────────"
echo ""
echo "  Package directory: $PKG_DIR"
echo ""
echo "  Windows binaries:"
ls -lh "$WIN_DIR"/*.exe 2>/dev/null | awk '{printf "    %-35s %s\n", $NF, $5}'
echo ""
echo "  Linux binaries:"
ls -lh "$LINUX_DIR"/* 2>/dev/null | awk '{printf "    %-35s %s\n", $NF, $5}'
echo ""
echo "══════════════════════════════════════════════════════════════"
echo "  Ready to test! Add to your wezterm.lua:"
echo ""
echo '  config.ssh_domains = {'
echo '    {'
echo '      name = "myhost",'
echo '      remote_address = "myhost",'
echo "      remote_install_binaries_dir = [[$(cygpath -w "$LINUX_DIR")]],"
echo '    },'
echo '  }'
echo ""
echo "  Or run the Windows GUI directly:"
echo "    $WIN_DIR/wezterm-gui.exe"
echo "══════════════════════════════════════════════════════════════"
