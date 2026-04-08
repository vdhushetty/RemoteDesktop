#!/bin/bash
set -euo pipefail

# Package rd-viewer as a macOS .app bundle and .dmg
echo "Packaging macOS app..."

VERSION=$(grep 'version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
APP_NAME="Remote Desktop Viewer"
BUNDLE_DIR="dist/macos/${APP_NAME}.app"

# Build release first
cargo build --release --bin rd-viewer --bin rd-agent

# Create .app bundle structure
rm -rf "${BUNDLE_DIR}"
mkdir -p "${BUNDLE_DIR}/Contents/MacOS"
mkdir -p "${BUNDLE_DIR}/Contents/Resources"

# Copy binary
cp target/release/rd-viewer "${BUNDLE_DIR}/Contents/MacOS/"

# Copy Info.plist
cp packaging/macos/Info.plist "${BUNDLE_DIR}/Contents/"

# Copy icon if exists
if [ -f assets/icons/icon.icns ]; then
    cp assets/icons/icon.icns "${BUNDLE_DIR}/Contents/Resources/AppIcon.icns"
fi

echo "Created: ${BUNDLE_DIR}"
echo ""

# Also copy agent binary for distribution
AGENT_DIR="dist/macos"
cp target/release/rd-agent "${AGENT_DIR}/"
cp packaging/macos/com.remotedesktop.agent.plist "${AGENT_DIR}/"

echo "Agent binary and launchd plist copied to ${AGENT_DIR}/"
echo ""
echo "To install the agent as a service:"
echo "  sudo cp ${AGENT_DIR}/rd-agent /usr/local/bin/"
echo "  sudo cp ${AGENT_DIR}/com.remotedesktop.agent.plist /Library/LaunchDaemons/"
echo "  sudo launchctl load /Library/LaunchDaemons/com.remotedesktop.agent.plist"
