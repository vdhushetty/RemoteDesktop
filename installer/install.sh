#!/bin/bash
set -euo pipefail

# Remote Desktop - One-line installer for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/vdhushetty/RemoteDesktop/main/installer/install.sh | bash

VERSION="0.1.0"
REPO="vdhushetty/RemoteDesktop"

echo ""
echo "  Remote Desktop Installer v${VERSION}"
echo "  =================================="
echo ""

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in x86_64) ARCH="x86_64" ;; aarch64|arm64) ARCH="aarch64" ;; *) echo "Unsupported: $ARCH"; exit 1 ;; esac
case "$OS" in linux) PLATFORM="linux" ;; darwin) PLATFORM="macos" ;; *) echo "Use install.ps1 for Windows"; exit 1 ;; esac

INSTALL_DIR="/usr/local/bin"
RELEASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}/RemoteDesktop-${PLATFORM}-${ARCH}"

# Try download, fall back to source build
if curl -fsSL -o /tmp/remote-desktop "$RELEASE_URL" 2>/dev/null; then
    echo "  Downloaded pre-built binary."
    sudo mv /tmp/remote-desktop "$INSTALL_DIR/remote-desktop"
    sudo chmod +x "$INSTALL_DIR/remote-desktop"
else
    echo "  Building from source (needs Rust + deps)..."

    if ! command -v cargo &>/dev/null; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    if [ "$PLATFORM" = "linux" ] && command -v apt-get &>/dev/null; then
        sudo apt-get update -qq
        sudo apt-get install -y -qq build-essential pkg-config libvpx-dev \
            libx11-dev libxcb1-dev libxcb-shm0-dev libopus-dev \
            protobuf-compiler libasound2-dev libclang-dev \
            libglib2.0-dev libgtk-3-dev libxdo-dev libxkbcommon-dev \
            libwayland-dev libpipewire-0.3-dev libdbus-1-dev libgbm-dev
    elif [ "$PLATFORM" = "macos" ] && command -v brew &>/dev/null; then
        brew install libvpx opus protobuf pkg-config
    fi

    TMPDIR=$(mktemp -d)
    git clone --depth=1 "https://github.com/${REPO}.git" "$TMPDIR/rd"
    cd "$TMPDIR/rd"
    cargo build --release --bin rd-desktop
    sudo cp target/release/rd-desktop "$INSTALL_DIR/remote-desktop"
    sudo chmod +x "$INSTALL_DIR/remote-desktop"
    rm -rf "$TMPDIR"
fi

# Linux desktop entry
if [ "$PLATFORM" = "linux" ]; then
    mkdir -p "$HOME/.local/share/applications"
    cat > "$HOME/.local/share/applications/remote-desktop.desktop" << 'EOF'
[Desktop Entry]
Name=Remote Desktop
Comment=Access and control machines remotely
Exec=remote-desktop
Type=Application
Categories=Utility;RemoteAccess;
StartupNotify=true
EOF
fi

echo ""
echo "  Installed: $INSTALL_DIR/remote-desktop"
echo "  Run: remote-desktop"
echo ""
