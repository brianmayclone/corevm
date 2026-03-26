#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# Build a .deb package for corevm-vmmanager (CoreVM Manager desktop app).
#
# Usage:
#   sudo -E env "PATH=$PATH" ./tools/build-deb-vmmanager.sh
#
# Output:
#   dist/corevm-vmmanager_<version>_amd64.deb
#
# Prerequisites:
#   - Rust toolchain (cargo +stable)
#   - dpkg-deb (usually pre-installed on Debian/Ubuntu)
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

# ── Must run as root (required for libcorevm/linux build) ─────────────────

if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root.${NC}"
    echo "Usage: sudo -E env \"PATH=\$PATH\" ./tools/build-deb-vmmanager.sh"
    exit 1
fi

# ── Prerequisites ─────────────────────────────────────────────────────────

# Ensure cargo is in PATH
for p in "$HOME/.cargo/bin" "/usr/local/bin"; do
    [ -d "$p" ] && export PATH="$p:$PATH"
done
if [ -n "${SUDO_USER:-}" ]; then
    SUDO_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
    [ -d "$SUDO_HOME/.cargo/bin" ] && export PATH="$SUDO_HOME/.cargo/bin:$PATH"
fi

if ! command -v cargo &>/dev/null; then
    echo -e "${RED}Error: cargo not found. Install the Rust toolchain first.${NC}"
    exit 1
fi

if ! command -v dpkg-deb &>/dev/null; then
    echo -e "${RED}Error: dpkg-deb not found. Install dpkg.${NC}"
    exit 1
fi

# ── Version ───────────────────────────────────────────────────────────────

VERSION=$(cat "$ROOT/VERSION")
ARCH="amd64"
PKG_NAME="corevm-vmmanager"
DEB_FILE="${PKG_NAME}_${VERSION}_${ARCH}.deb"

echo ""
echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  Building .deb package: ${PKG_NAME} v${VERSION}${NC}"
echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
echo ""

# ── Build binary ──────────────────────────────────────────────────────────

echo -e "${CYAN}Building corevm-vmmanager...${NC}"
cd "$ROOT/apps/vmmanager"
cargo clean -p libcorevm 2>/dev/null || true
cargo +stable build --release --features libcorevm/linux
echo -e "${GREEN}✓ Binary built${NC}"

# ── Locate binary ────────────────────────────────────────────────────────

BINARY=""
for candidate in \
    "$ROOT/target/release/corevm-vmmanager" \
    "$ROOT/target/x86_64-unknown-linux-gnu/release/corevm-vmmanager"; do
    if [ -f "$candidate" ]; then
        BINARY="$candidate"
        break
    fi
done

if [ -z "$BINARY" ]; then
    echo -e "${RED}Error: corevm-vmmanager binary not found after build.${NC}"
    exit 1
fi

# ── Prepare package tree ─────────────────────────────────────────────────

DIST="$ROOT/dist"
STAGING="$DIST/deb-staging"
rm -rf "$STAGING"

# Directory structure
mkdir -p "$STAGING/DEBIAN"
mkdir -p "$STAGING/usr/bin"
mkdir -p "$STAGING/usr/share/applications"
mkdir -p "$STAGING/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$STAGING/usr/share/doc/${PKG_NAME}"

# Binary (stripped for smaller package)
cp "$BINARY" "$STAGING/usr/bin/corevm-vmmanager"
strip "$STAGING/usr/bin/corevm-vmmanager" 2>/dev/null || true

# Desktop entry
cp "$ROOT/apps/vmmanager/linux/corevm-manager.desktop" \
   "$STAGING/usr/share/applications/corevm-manager.desktop"

# Icon – convert .ico to .png if ImageMagick is available, otherwise skip
ICON_SRC="$ROOT/apps/vmmanager/assets/icons/Icon.ico"
ICON_DST="$STAGING/usr/share/icons/hicolor/256x256/apps/corevm-manager.png"
if command -v convert &>/dev/null && [ -f "$ICON_SRC" ]; then
    convert "$ICON_SRC[0]" -resize 256x256 "$ICON_DST" 2>/dev/null || true
    echo -e "${GREEN}✓ Icon converted${NC}"
elif command -v magick &>/dev/null && [ -f "$ICON_SRC" ]; then
    magick "$ICON_SRC[0]" -resize 256x256 "$ICON_DST" 2>/dev/null || true
    echo -e "${GREEN}✓ Icon converted${NC}"
else
    echo -e "${CYAN}  (ImageMagick not found – .deb will ship without icon)${NC}"
fi

# ── Installed size ────────────────────────────────────────────────────────

INSTALLED_SIZE=$(du -sk "$STAGING" | cut -f1)

# ── DEBIAN/control ────────────────────────────────────────────────────────

cat > "$STAGING/DEBIAN/control" <<EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Section: system
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Maintainer: CoreVM Developers <noreply@corevm.dev>
Description: CoreVM Manager – Desktop GUI for managing x86 virtual machines
 CoreVM Manager is a native Linux desktop application for creating,
 configuring, and running x86 virtual machines using CoreVM's built-in
 software emulation engine with optional KVM hardware acceleration.
Homepage: https://github.com/cmoeller/corevm
EOF

# ── DEBIAN/postinst ───────────────────────────────────────────────────────

cat > "$STAGING/DEBIAN/postinst" <<'EOF'
#!/bin/bash
set -e
# Update desktop database so the .desktop file is picked up
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
# Update icon cache
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
fi
EOF
chmod 755 "$STAGING/DEBIAN/postinst"

# ── DEBIAN/postrm ─────────────────────────────────────────────────────────

cat > "$STAGING/DEBIAN/postrm" <<'EOF'
#!/bin/bash
set -e
if command -v update-desktop-database &>/dev/null; then
    update-desktop-database -q /usr/share/applications 2>/dev/null || true
fi
if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
fi
EOF
chmod 755 "$STAGING/DEBIAN/postrm"

# ── Copyright ─────────────────────────────────────────────────────────────

cat > "$STAGING/usr/share/doc/${PKG_NAME}/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: CoreVM Manager
Source: https://github.com/cmoeller/corevm

Files: *
Copyright: $(date +%Y) CoreVM Developers
License: Proprietary
EOF

# ── Build .deb ────────────────────────────────────────────────────────────

echo -e "${CYAN}Packaging .deb...${NC}"
mkdir -p "$DIST"
dpkg-deb --build --root-owner-group "$STAGING" "$DIST/$DEB_FILE"

# Cleanup staging
rm -rf "$STAGING"

# ── Summary ───────────────────────────────────────────────────────────────

SIZE=$(du -h "$DIST/$DEB_FILE" | cut -f1)
echo ""
echo -e "${GREEN}══════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  .deb package built successfully!${NC}"
echo -e "${GREEN}══════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Package: ${CYAN}$DIST/$DEB_FILE${NC} ($SIZE)"
echo ""
echo -e "  Install:   ${CYAN}sudo dpkg -i $DIST/$DEB_FILE${NC}"
echo -e "  Remove:    ${CYAN}sudo dpkg -r $PKG_NAME${NC}"
echo -e "  Purge:     ${CYAN}sudo dpkg -P $PKG_NAME${NC}"
echo ""
