#!/bin/bash
set -euo pipefail

VERSION="0.1.0"
ARCH=$(dpkg --print-architecture 2>/dev/null || echo "amd64")
PKG_NAME="remote-desktop"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/dist/linux"
DEB_DIR="${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}"

echo "Building release..."
cd "$PROJECT_DIR"
cargo build --release --bin rd-desktop

echo "Creating .deb package..."
rm -rf "${DEB_DIR}"
mkdir -p "${DEB_DIR}/DEBIAN"
mkdir -p "${DEB_DIR}/usr/bin"
mkdir -p "${DEB_DIR}/usr/share/applications"

# Single binary
cp target/release/rd-desktop "${DEB_DIR}/usr/bin/remote-desktop"

# Desktop entry
cat > "${DEB_DIR}/usr/share/applications/remote-desktop.desktop" << 'EOF'
[Desktop Entry]
Name=Remote Desktop
Comment=Access and control machines remotely
Exec=remote-desktop
Type=Application
Categories=Utility;RemoteAccess;
StartupNotify=true
EOF

# Control
cat > "${DEB_DIR}/DEBIAN/control" << EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: libvpx-dev, libx11-6, libopus0, libasound2
Maintainer: RemoteDesktop <noreply@github.com>
Homepage: https://github.com/vdhushetty/RemoteDesktop
Description: Remote desktop application
 One app to both control and be controlled remotely.
 VP9 video, input control, clipboard sync, file transfer, audio.
 Works over LAN and Internet via iroh NAT traversal.
EOF

cat > "${DEB_DIR}/DEBIAN/postinst" << 'EOF'
#!/bin/bash
echo ""
echo "  Remote Desktop installed!"
echo "  Run 'remote-desktop' from your app menu or terminal."
echo ""
EOF
chmod 755 "${DEB_DIR}/DEBIAN/postinst"

dpkg-deb --build "${DEB_DIR}" "${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb" 2>/dev/null || {
    echo "DEB structure at: ${DEB_DIR}/"
    exit 0
}

echo "Package: ${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
ls -lh "${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
