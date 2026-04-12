#!/bin/bash
set -euo pipefail

VERSION="0.1.0"
ARCH=$(dpkg --print-architecture 2>/dev/null || echo "amd64")
PKG_NAME="remote-desktop"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="${PROJECT_DIR}/dist/linux"
DEB_DIR="${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}"

echo "Building release binaries..."
cd "$PROJECT_DIR"
cargo build --release --bin rd-agent --bin rd-viewer

echo "Creating .deb package..."
rm -rf "${DEB_DIR}"

# Directory structure
mkdir -p "${DEB_DIR}/DEBIAN"
mkdir -p "${DEB_DIR}/usr/bin"
mkdir -p "${DEB_DIR}/usr/lib/systemd/user"
mkdir -p "${DEB_DIR}/usr/share/applications"

# Copy binaries
cp target/release/rd-agent "${DEB_DIR}/usr/bin/"
cp target/release/rd-viewer "${DEB_DIR}/usr/bin/"

# Systemd service (user-level)
cat > "${DEB_DIR}/usr/lib/systemd/user/rd-agent.service" << 'EOF'
[Unit]
Description=Remote Desktop Agent
After=graphical-session.target

[Service]
Type=simple
ExecStart=/usr/bin/rd-agent
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

# Desktop entries
cat > "${DEB_DIR}/usr/share/applications/rd-agent.desktop" << 'EOF'
[Desktop Entry]
Name=Remote Desktop Agent
Comment=Allow remote access to this machine
Exec=rd-agent
Type=Application
Categories=Utility;RemoteAccess;
StartupNotify=false
EOF

cat > "${DEB_DIR}/usr/share/applications/rd-viewer.desktop" << 'EOF'
[Desktop Entry]
Name=Remote Desktop Viewer
Comment=Connect to and control remote machines
Exec=rd-viewer
Type=Application
Categories=Utility;RemoteAccess;
StartupNotify=true
EOF

# DEBIAN control
cat > "${DEB_DIR}/DEBIAN/control" << EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: libvpx-dev, libx11-6, libopus0, libasound2
Maintainer: RemoteDesktop <noreply@github.com>
Homepage: https://github.com/vdhushetty/RemoteDesktop
Description: Cross-platform remote desktop application
 Native remote desktop with VP9 video, input control,
 clipboard sync, file transfer, and audio streaming.
 Works over LAN (mDNS discovery) and Internet (iroh NAT traversal).
EOF

# Post-install script
cat > "${DEB_DIR}/DEBIAN/postinst" << 'POSTINST'
#!/bin/bash
set -e
echo ""
echo "========================================"
echo "  Remote Desktop installed successfully!"
echo "========================================"
echo ""
echo "  To allow remote access to this machine:"
echo "    rd-agent"
echo ""
echo "  To auto-start on login:"
echo "    systemctl --user enable rd-agent"
echo "    systemctl --user start rd-agent"
echo ""
echo "  To connect to a remote machine:"
echo "    rd-viewer"
echo ""
POSTINST
chmod 755 "${DEB_DIR}/DEBIAN/postinst"

# Pre-remove script
cat > "${DEB_DIR}/DEBIAN/prerm" << 'PRERM'
#!/bin/bash
set -e
systemctl --user stop rd-agent 2>/dev/null || true
systemctl --user disable rd-agent 2>/dev/null || true
PRERM
chmod 755 "${DEB_DIR}/DEBIAN/prerm"

# Build .deb
dpkg-deb --build "${DEB_DIR}" "${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb" 2>/dev/null || {
    echo "dpkg-deb not available (not on Debian/Ubuntu)."
    echo "DEB structure created at: ${DEB_DIR}/"
    echo "Build on a Debian/Ubuntu system with: dpkg-deb --build ${DEB_DIR}"
    exit 0
}

echo ""
echo "Package created: ${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
ls -lh "${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
echo ""
echo "Install with: sudo dpkg -i ${BUILD_DIR}/${PKG_NAME}_${VERSION}_${ARCH}.deb"
