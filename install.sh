#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# install.sh — Build archman, then optionally install it system-wide
#
# archman is a native Rust application (ratatui interface).
# Supports both local repository builds (./install.sh) and direct
# one-liner installation via curl (curl -fsSL .../install.sh | bash).
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

INSTALL_BIN="/usr/local/bin/archman"
REPO_URL="https://github.com/ankur3-101106/pacman-utils.git"
IS_TEMP_CLONE=0
AUTO_YES=0

for arg in "${@:-}"; do
    case "$arg" in
        -y|--yes) AUTO_YES=1 ;;
    esac
done

echo ""
echo "  ╔═════════════════════════════════════════╗"
echo "  ║      archman — Build & Install          ║"
echo "  ╚═════════════════════════════════════════╝"
echo ""

# Locate cargo (rustup installs to ~/.cargo/bin which may not be on PATH, or SUDO_USER's home)
CARGO=""
if command -v cargo &>/dev/null; then
    CARGO=cargo
elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    CARGO="$HOME/.cargo/bin/cargo"
elif [[ -n "${SUDO_USER:-}" && -x "$(eval echo ~$SUDO_USER)/.cargo/bin/cargo" ]]; then
    CARGO="$(eval echo ~$SUDO_USER)/.cargo/bin/cargo"
elif [[ -x "/usr/bin/cargo" ]]; then
    CARGO="/usr/bin/cargo"
fi

if [[ -z "$CARGO" ]]; then
    echo "  ERROR: cargo not found."
    echo "  Install the Rust toolchain first:"
    echo "    sudo pacman -S rust"
    echo "  or via rustup:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Determine source directory (local clone vs piped from curl)
if [[ -n "${BASH_SOURCE[0]:-}" && -f "$(dirname "${BASH_SOURCE[0]}")/Cargo.toml" ]]; then
    SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elif [[ -f "./Cargo.toml" ]] && grep -q 'name = "archman"' "./Cargo.toml" 2>/dev/null; then
    SRC_DIR="$(pwd)"
else
    echo "  Fetching source repository from GitHub..."
    if ! command -v git &>/dev/null; then
        echo "  ERROR: git not found. Please install git:"
        echo "    sudo pacman -S git"
        exit 1
    fi
    SRC_DIR="$(mktemp -d "${TMPDIR:-/tmp}/archman-build.XXXXXX")"
    IS_TEMP_CLONE=1
    trap 'if [[ "$IS_TEMP_CLONE" -eq 1 && -d "$SRC_DIR" ]]; then rm -rf "$SRC_DIR"; fi' EXIT INT TERM
    git clone --depth=1 "$REPO_URL" "$SRC_DIR" --quiet
fi

echo "  Building archman (release mode)..."
(cd "$SRC_DIR" && "$CARGO" build --release)

BIN="$SRC_DIR/target/release/archman"
if [[ ! -x "$BIN" ]]; then
    echo "  ERROR: build finished but no binary was produced at:"
    echo "    $BIN"
    exit 1
fi

SIZE="$(du -h "$BIN" | cut -f1)"
echo ""
echo "  ✔ Build complete: $BIN ($SIZE)"
echo ""

# Handle user input even when script is piped via curl ... | bash
reply="y"
if [[ "$AUTO_YES" -eq 1 ]]; then
    reply="y"
elif [[ -t 0 ]]; then
    read -rp "  Install to $INSTALL_BIN? [Y/n] " reply || reply="y"
elif [[ -c /dev/tty ]]; then
    read -rp "  Install to $INSTALL_BIN? [Y/n] " reply </dev/tty || reply="y"
fi

if [[ "$reply" =~ ^[Nn] ]]; then
    if [[ "$IS_TEMP_CLONE" -eq 1 ]]; then
        cp "$BIN" "./archman"
        chmod 755 "./archman"
        echo ""
        echo "  Skipped system install. Binary saved to current directory:"
        echo "    ./archman"
        echo ""
    else
        echo ""
        echo "  Skipped installation. Run the binary directly:"
        echo "    $BIN"
        echo ""
    fi
    exit 0
fi

echo ""
echo "  Installing to $INSTALL_BIN..."
if [[ $EUID -eq 0 ]]; then
    cp "$BIN" "$INSTALL_BIN"
    chmod 755 "$INSTALL_BIN"
else
    sudo cp "$BIN" "$INSTALL_BIN"
    sudo chmod 755 "$INSTALL_BIN"
fi

echo ""
echo "  ✔ archman installed successfully!"
echo ""
echo "  Run 'archman' to start."
echo ""
