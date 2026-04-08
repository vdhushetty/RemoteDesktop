#!/bin/bash
set -euo pipefail

# Build release binaries for the current platform
echo "Building release binaries..."

cargo build --release --bin rd-agent --bin rd-viewer --bin rd-relay

echo ""
echo "Release binaries:"
ls -lh target/release/rd-agent target/release/rd-viewer target/release/rd-relay 2>/dev/null || true

OS=$(uname -s)
ARCH=$(uname -m)
VERSION=$(grep 'version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

echo ""
echo "Platform: ${OS}-${ARCH}"
echo "Version: ${VERSION}"

# Create dist directory
DIST_DIR="dist/${OS}-${ARCH}"
mkdir -p "${DIST_DIR}"

cp target/release/rd-agent "${DIST_DIR}/"
cp target/release/rd-viewer "${DIST_DIR}/"
cp target/release/rd-relay "${DIST_DIR}/"

echo ""
echo "Binaries copied to ${DIST_DIR}/"
ls -lh "${DIST_DIR}/"
