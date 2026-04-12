#!/bin/bash
set -euo pipefail

# Remote Desktop - One-line installer for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/vdhushetty/RemoteDesktop/main/installer/install.sh | bash

REPO="vdhushetty/RemoteDesktop"
VERSION="0.1.0"
INSTALL_DIR="/usr/local/bin"

echo ""
echo "========================================"
echo "  Remote Desktop Installer v${VERSION}"
echo "========================================"
echo ""

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux) PLATFORM="linux" ;;
    darwin) PLATFORM="macos" ;;
    *) echo "Unsupported OS: $OS (use install.ps1 for Windows)"; exit 1 ;;
esac

echo "Platform: ${PLATFORM}-${ARCH}"
echo ""

# Check for GitHub release
RELEASE_URL="https://github.com/${REPO}/releases/download/v${VERSION}"
AGENT_URL="${RELEASE_URL}/rd-agent-${PLATFORM}-${ARCH}"
VIEWER_URL="${RELEASE_URL}/rd-viewer-${PLATFORM}-${ARCH}"

# Try downloading from release, fall back to building from source
download_or_build() {
    echo "Checking for pre-built binaries..."
    if curl -fsSL -o /dev/null -w "%{http_code}" "$AGENT_URL" 2>/dev/null | grep -q "200"; then
        echo "Downloading pre-built binaries..."
        sudo curl -fsSL -o "${INSTALL_DIR}/rd-agent" "$AGENT_URL"
        sudo curl -fsSL -o "${INSTALL_DIR}/rd-viewer" "$VIEWER_URL"
        sudo chmod +x "${INSTALL_DIR}/rd-agent" "${INSTALL_DIR}/rd-viewer"
    else
        echo "No pre-built binaries found. Building from source..."
        build_from_source
    fi
}

build_from_source() {
    # Install Rust if needed
    if ! command -v cargo &>/dev/null; then
        echo "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    # Install dependencies
    if [ "$PLATFORM" = "linux" ]; then
        echo "Installing build dependencies..."
        if command -v apt-get &>/dev/null; then
            sudo apt-get update -qq
            sudo apt-get install -y -qq build-essential pkg-config libvpx-dev \
                libx11-dev libxcb1-dev libxcb-shm0-dev libopus-dev \
                protobuf-compiler libasound2-dev libclang-dev
        elif command -v dnf &>/dev/null; then
            sudo dnf install -y gcc pkg-config libvpx-devel libX11-devel \
                libxcb-devel opus-devel protobuf-compiler alsa-lib-devel clang-devel
        elif command -v pacman &>/dev/null; then
            sudo pacman -S --noconfirm base-devel pkg-config libvpx libx11 \
                libxcb opus protobuf alsa-lib clang
        fi
    elif [ "$PLATFORM" = "macos" ]; then
        if command -v brew &>/dev/null; then
            echo "Installing build dependencies..."
            brew install libvpx opus protobuf pkg-config
        fi
    fi

    # Clone and build
    TMPDIR=$(mktemp -d)
    echo "Cloning repository..."
    git clone --depth=1 "https://github.com/${REPO}.git" "$TMPDIR/RemoteDesktop"
    cd "$TMPDIR/RemoteDesktop"

    echo "Building (this may take a few minutes)..."
    cargo build --release --bin rd-agent --bin rd-viewer

    sudo cp target/release/rd-agent "${INSTALL_DIR}/"
    sudo cp target/release/rd-viewer "${INSTALL_DIR}/"
    sudo chmod +x "${INSTALL_DIR}/rd-agent" "${INSTALL_DIR}/rd-viewer"

    # Install systemd service on Linux
    if [ "$PLATFORM" = "linux" ] && command -v systemctl &>/dev/null; then
        mkdir -p "$HOME/.config/systemd/user"
        cp packaging/linux/rd-agent.service "$HOME/.config/systemd/user/"
        echo "Systemd service installed. Enable with: systemctl --user enable --now rd-agent"
    fi

    # Install desktop entries on Linux
    if [ "$PLATFORM" = "linux" ]; then
        mkdir -p "$HOME/.local/share/applications"
        cp packaging/linux/rd-agent.desktop "$HOME/.local/share/applications/"
        cp packaging/linux/rd-viewer.desktop "$HOME/.local/share/applications/"
    fi

    # Cleanup
    rm -rf "$TMPDIR"
}

download_or_build

echo ""
echo "========================================"
echo "  Installation complete!"
echo "========================================"
echo ""
echo "  To allow remote access to this machine:"
echo "    rd-agent"
echo ""
echo "  To connect to a remote machine:"
echo "    rd-viewer"
echo ""
echo "  The agent will print a Device ID that"
echo "  you enter in the viewer to connect."
echo ""
