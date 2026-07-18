#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# install.sh — Install archman to /usr/local
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

INSTALL_BIN="/usr/local/bin/archman"
INSTALL_LIB="/usr/local/lib/archman"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo ""
echo "  ╔═════════════════════════════════════════╗"
echo "  ║     archman — System-wide Installer     ║"
echo "  ╚═════════════════════════════════════════╝"
echo ""

# Check we're in the right directory
if [[ ! -f "$SCRIPT_DIR/archman" ]] || [[ ! -d "$SCRIPT_DIR/lib" ]]; then
    echo "ERROR: Run this script from the archman source directory."
    exit 1
fi

echo "  This will install archman to:"
echo "    Binary: $INSTALL_BIN"
echo "    Library: $INSTALL_LIB/"
echo ""

read -rp "  Continue? [Y/n] " reply
if [[ "$reply" =~ ^[Nn] ]]; then
    echo "  Installation cancelled."
    exit 0
fi

echo ""
echo "  Installing..."

# Install library files
sudo mkdir -p "$INSTALL_LIB"
sudo cp -r "$SCRIPT_DIR/lib/"* "$INSTALL_LIB/"
sudo chmod 644 "$INSTALL_LIB/"*.sh

# Install main executable
sudo cp "$SCRIPT_DIR/archman" "$INSTALL_BIN"
sudo chmod 755 "$INSTALL_BIN"

echo ""
echo "  ✔ archman installed successfully!"
echo ""
echo "  Run 'archman' to start."
echo ""

# Offer to install optional dependencies
if ! command -v gum &>/dev/null; then
    echo "  Optional: Install 'gum' for the best TUI experience."
    echo "    sudo pacman -S gum"
    echo ""
fi
