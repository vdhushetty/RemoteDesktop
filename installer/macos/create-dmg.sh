#!/bin/bash
set -euo pipefail

VERSION="0.1.0"
APP_NAME="Remote Desktop"
DMG_NAME="RemoteDesktop-${VERSION}-macOS"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/dist/macos"

echo "Building release binaries..."
cd "$PROJECT_DIR"
cargo build --release --bin rd-agent --bin rd-viewer

echo "Creating app bundles..."
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}"

# --- Viewer App Bundle ---
VIEWER_APP="${BUILD_DIR}/${APP_NAME} Viewer.app"
mkdir -p "${VIEWER_APP}/Contents/MacOS"
mkdir -p "${VIEWER_APP}/Contents/Resources"

cp target/release/rd-viewer "${VIEWER_APP}/Contents/MacOS/"
cat > "${VIEWER_APP}/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Remote Desktop Viewer</string>
    <key>CFBundleDisplayName</key><string>Remote Desktop Viewer</string>
    <key>CFBundleIdentifier</key><string>com.remotedesktop.viewer</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>rd-viewer</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# --- Agent App Bundle ---
AGENT_APP="${BUILD_DIR}/${APP_NAME} Agent.app"
mkdir -p "${AGENT_APP}/Contents/MacOS"
mkdir -p "${AGENT_APP}/Contents/Resources"

cp target/release/rd-agent "${AGENT_APP}/Contents/MacOS/"
cat > "${AGENT_APP}/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Remote Desktop Agent</string>
    <key>CFBundleDisplayName</key><string>Remote Desktop Agent</string>
    <key>CFBundleIdentifier</key><string>com.remotedesktop.agent</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>rd-agent</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSUIElement</key><true/>
    <key>NSScreenCaptureUsageDescription</key><string>Remote Desktop needs screen capture access.</string>
    <key>NSAccessibilityUsageDescription</key><string>Remote Desktop needs accessibility access for input control.</string>
</dict>
</plist>
PLIST

# --- Create DMG ---
echo "Creating DMG..."
DMG_TEMP="${BUILD_DIR}/dmg-temp"
rm -rf "${DMG_TEMP}"
mkdir -p "${DMG_TEMP}"

cp -R "${VIEWER_APP}" "${DMG_TEMP}/"
cp -R "${AGENT_APP}" "${DMG_TEMP}/"
ln -s /Applications "${DMG_TEMP}/Applications"

# Create a README in the DMG
cat > "${DMG_TEMP}/README.txt" << 'EOF'
Remote Desktop - Installation
=============================

1. Drag "Remote Desktop Agent" to Applications
   -> Run this on the machine you want to control remotely
   -> It will show a Device ID for connecting

2. Drag "Remote Desktop Viewer" to Applications
   -> Run this on the machine you're controlling FROM
   -> Enter the Device ID to connect

That's it! Works over LAN and Internet.
EOF

hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${DMG_TEMP}" \
    -ov -format UDZO \
    "${BUILD_DIR}/${DMG_NAME}.dmg"

rm -rf "${DMG_TEMP}"

echo ""
echo "DMG created: ${BUILD_DIR}/${DMG_NAME}.dmg"
ls -lh "${BUILD_DIR}/${DMG_NAME}.dmg"
