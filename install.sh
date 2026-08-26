#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# install.sh — Build archman, then optionally install it system-wide
#
# archman is a native Rust application (ratatui interface). This script
# always compiles the release binary into target/release/, then asks
# whether to copy it to /usr/local/bin. Answering "no" (or just wanting
# a local build) leaves the ready-to-run binary in place.
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

INSTALL_BIN="/usr/local/bin/archman"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo ""
echo "  ╔═════════════════════════════════════════╗"
echo "  ║      archman — Build & Install          ║"
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

echo "  Building archman (release mode)..."
(cd "$SCRIPT_DIR" && "$CARGO" build --release)

BIN="$SCRIPT_DIR/target/release/archman"
if [[ ! -x "$BIN" ]]; then
    echo "  ERROR: build finished but no binary was produced at:"
    echo "    $BIN"
    exit 1
fi

SIZE="$(du -h "$BIN" | cut -f1)"
echo ""
echo "  ✔ Build complete: $BIN ($SIZE)"
echo ""

read -rp "  Install to $INSTALL_BIN? [Y/n] " reply
if [[ "$reply" =~ ^[Nn] ]]; then
    echo ""
    echo "  Skipped installation. Run the binary directly:"
    echo "    $BIN"
    echo ""
    exit 0
fi

echo ""
echo "  Installing..."
sudo cp "$BIN" "$INSTALL_BIN"
sudo chmod 755 "$INSTALL_BIN"

echo ""
echo "  ✔ archman installed successfully!"
echo ""
echo "  Run 'archman' to start."
echo ""
