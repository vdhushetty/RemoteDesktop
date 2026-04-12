#!/bin/bash
set -euo pipefail

VERSION="0.1.0"
APP_NAME="Remote Desktop"
DMG_NAME="RemoteDesktop-${VERSION}-macOS"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/dist/macos"

echo "Building release binary..."
cd "$PROJECT_DIR"
cargo build --release --bin rd-desktop

echo "Creating app bundle..."
rm -rf "${BUILD_DIR}"
BUNDLE="${BUILD_DIR}/${APP_NAME}.app"
mkdir -p "${BUNDLE}/Contents/MacOS"
mkdir -p "${BUNDLE}/Contents/Resources"

cp target/release/rd-desktop "${BUNDLE}/Contents/MacOS/"

cat > "${BUNDLE}/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key><string>com.remotedesktop.app</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleExecutable</key><string>rd-desktop</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSScreenCaptureUsageDescription</key><string>Remote Desktop needs screen capture to share your screen.</string>
    <key>NSAccessibilityUsageDescription</key><string>Remote Desktop needs accessibility to control mouse and keyboard.</string>
</dict>
</plist>
PLIST

echo "Creating DMG..."
DMG_TEMP="${BUILD_DIR}/dmg-temp"
rm -rf "${DMG_TEMP}"
mkdir -p "${DMG_TEMP}"
cp -R "${BUNDLE}" "${DMG_TEMP}/"
ln -s /Applications "${DMG_TEMP}/Applications"

hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${DMG_TEMP}" \
    -ov -format UDZO \
    "${BUILD_DIR}/${DMG_NAME}.dmg"

rm -rf "${DMG_TEMP}"
echo ""
echo "Done: ${BUILD_DIR}/${DMG_NAME}.dmg"
ls -lh "${BUILD_DIR}/${DMG_NAME}.dmg"
