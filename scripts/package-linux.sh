#!/bin/bash
set -euo pipefail

# Package for Linux (DEB + AppImage-ready structure)
echo "Packaging Linux..."

VERSION=$(grep 'version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
ARCH=$(uname -m)

cargo build --release --bin rd-agent --bin rd-viewer --bin rd-relay

# Create DEB-like directory structure
DEB_DIR="dist/linux/deb"
rm -rf "${DEB_DIR}"
mkdir -p "${DEB_DIR}/usr/bin"
mkdir -p "${DEB_DIR}/usr/lib/systemd/user"
mkdir -p "${DEB_DIR}/usr/share/applications"
mkdir -p "${DEB_DIR}/DEBIAN"

cp target/release/rd-agent "${DEB_DIR}/usr/bin/"
cp target/release/rd-viewer "${DEB_DIR}/usr/bin/"
cp packaging/linux/rd-agent.service "${DEB_DIR}/usr/lib/systemd/user/"
cp packaging/linux/rd-agent.desktop "${DEB_DIR}/usr/share/applications/"
cp packaging/linux/rd-viewer.desktop "${DEB_DIR}/usr/share/applications/"

# Create DEBIAN control file
cat > "${DEB_DIR}/DEBIAN/control" << EOF
Package: remote-desktop
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: $(dpkg --print-architecture 2>/dev/null || echo amd64)
Depends: libvpx-dev, libx11-dev
Maintainer: Remote Desktop Team
Description: Cross-platform remote desktop application
 A native remote desktop tool with screen capture, input control,
 clipboard sync, file transfer, and audio streaming.
 Supports both LAN and internet connections via NAT traversal.
EOF

# Create postinst script
cat > "${DEB_DIR}/DEBIAN/postinst" << 'EOF'
#!/bin/bash
echo "Remote Desktop installed successfully."
echo "To start the agent: systemctl --user enable --now rd-agent"
echo "To launch the viewer: rd-viewer"
EOF
chmod 755 "${DEB_DIR}/DEBIAN/postinst"

echo "DEB structure created at ${DEB_DIR}/"
echo "Build .deb with: dpkg-deb --build ${DEB_DIR} dist/linux/remote-desktop_${VERSION}.deb"

# Also create a portable tarball
TAR_DIR="dist/linux/portable"
mkdir -p "${TAR_DIR}"
cp target/release/rd-agent "${TAR_DIR}/"
cp target/release/rd-viewer "${TAR_DIR}/"
cp target/release/rd-relay "${TAR_DIR}/"
cp packaging/linux/rd-agent.service "${TAR_DIR}/"

echo ""
echo "Portable binaries at ${TAR_DIR}/"
