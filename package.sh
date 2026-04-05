#!/bin/bash
# Package Nokkvi for distribution/building
# This script creates a clean zip file with only the files needed to build the client

set -e

# Configuration
PACKAGE_NAME="nokkvi"
VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
COMMIT_SHORT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
COMMIT_FULL=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
COMMIT_DATE=$(git log -1 --format='%ci' 2>/dev/null || echo "unknown")
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
DIRTY=$(git diff --quiet 2>/dev/null && echo "clean" || echo "dirty")
OUTPUT_DIR="dist"
TEMP_DIR="${OUTPUT_DIR}/${PACKAGE_NAME}-${VERSION}"
ZIP_FILE="${OUTPUT_DIR}/${PACKAGE_NAME}-${VERSION}-${COMMIT_SHORT}.zip"

# Try to capture the local test server version (Navidrome) running on localhost
TEST_SERVER_VERSION=$(curl -sS "http://localhost:4533/rest/ping?u=nobody&p=nopass&v=1.16.1&c=nokkvi&f=json" 2>/dev/null | grep -oP '"serverVersion":"\K[^"]+' || echo "unknown")

echo "📦 Packaging ${PACKAGE_NAME} v${VERSION} (${COMMIT_SHORT}, ${DIRTY})"

# Clean up old builds
rm -rf "${OUTPUT_DIR}"
mkdir -p "${TEMP_DIR}"

echo "📋 Copying essential files..."

# Core source files
cp -r src "${TEMP_DIR}/"
cp Cargo.toml "${TEMP_DIR}/"
if [ -f Cargo.lock ]; then
    cp Cargo.lock "${TEMP_DIR}/"
fi

# Workspace members
echo "   - data workspace..."
cp -r data "${TEMP_DIR}/"



# Documentation
cp README.md "${TEMP_DIR}/"
cp BUILD.md "${TEMP_DIR}/"
if [ -d docs ] && [ "$(ls -A docs 2>/dev/null)" ]; then
    cp -r docs "${TEMP_DIR}/"
fi

# Configuration examples

cp -r themes "${TEMP_DIR}/"

# Build/format config
if [ -f rustfmt.toml ]; then
    cp rustfmt.toml "${TEMP_DIR}/"
fi

# Assets (fonts, icons, desktop entry)
cp -r assets "${TEMP_DIR}/"

# Install script (desktop entry + icon)
cp -p install.sh "${TEMP_DIR}/"

# Git files (for version control)
cp .gitignore "${TEMP_DIR}/"

# License (if exists)
if [ -f LICENSE ]; then
    cp LICENSE "${TEMP_DIR}/"
fi

# Build info (tracks which commit this package was built from)
cat > "${TEMP_DIR}/BUILD_INFO" <<EOF
package: ${PACKAGE_NAME}
version: ${VERSION}
commit:  ${COMMIT_FULL}
branch:  ${BRANCH}
date:    ${COMMIT_DATE}
status:  ${DIRTY}
built:   $(date -Iseconds)
test_server: ${TEST_SERVER_VERSION}
EOF

echo "🗜️  Creating zip archive..."
cd "${OUTPUT_DIR}"
zip -r "$(basename ${ZIP_FILE})" "$(basename ${TEMP_DIR})" -q

# Clean up temp directory
rm -rf "$(basename ${TEMP_DIR})"

cd ..
echo "✅ Package created: ${ZIP_FILE}"
echo "📊 Package size: $(du -h ${ZIP_FILE} | cut -f1)"
echo ""
echo "📝 Package contents:"
unzip -l "${ZIP_FILE}" | head -20
echo "..."
echo ""
echo "🎉 Done! Your friend can now:"
echo "   1. Extract the zip file: unzip $(basename ${ZIP_FILE})"
echo "   2. cd into the directory: cd ${PACKAGE_NAME}-${VERSION}"
echo "   3. Install dependencies (Arch): sudo pacman -S pipewire fontconfig pkg-config"
echo "   4. Build with: cargo build --release"
echo "   5. Binary will be at: target/release/nokkvi"
echo "   6. Install desktop entry + icon: ./install.sh"
