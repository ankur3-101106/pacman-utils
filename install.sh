#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# install.sh — Build archman and install it to /usr/local
#
# archman is now a native Rust application (ratatui interface). This
# script compiles the release binary and places it on your PATH.
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

INSTALL_BIN="/usr/local/bin/archman"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo ""
echo "  ╔═════════════════════════════════════════╗"
echo "  ║     archman — System-wide Installer     ║"
echo "  ╚═════════════════════════════════════════╝"
echo ""

# Locate cargo (rustup installs to ~/.cargo/bin which may not be on PATH)
if command -v cargo &>/dev/null; then
    CARGO=cargo
elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
else
    echo "  ERROR: cargo not found."
    echo "  Install the Rust toolchain first:"
    echo "    sudo pacman -S rust"
    echo "  or via rustup:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "  This will build archman (release mode) and install it to:"
echo "    Binary: $INSTALL_BIN"
echo ""
read -rp "  Continue? [Y/n] " reply
if [[ "$reply" =~ ^[Nn] ]]; then
    echo "  Installation cancelled."
    exit 0
fi

echo ""
echo "  Building..."
(cd "$SCRIPT_DIR" && "$CARGO" build --release)

echo "  Installing..."
sudo cp "$SCRIPT_DIR/target/release/archman" "$INSTALL_BIN"
sudo chmod 755 "$INSTALL_BIN"

echo ""
echo "  ✔ archman installed successfully!"
echo ""
echo "  Run 'archman' to start."
echo ""
