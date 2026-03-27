#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# Build self-extracting Linux installers for vmm-server and vmm-cluster.
#
# Usage:
#   ./tools/build-installers.sh              # Build both installers
#   ./tools/build-installers.sh --server     # Build vmm-server installer only
#   ./tools/build-installers.sh --cluster    # Build vmm-cluster installer only
#
# Output:
#   dist/vmm-server-installer.sh
#   dist/vmm-cluster-installer.sh
# ─────────────────────────────────────────────────────────────────────────────

set -e
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

DIST="$ROOT/dist"
STAGING="$ROOT/dist/staging"

BUILD_SERVER=false
BUILD_CLUSTER=false

case "$1" in
    --server)  BUILD_SERVER=true ;;
    --cluster) BUILD_CLUSTER=true ;;
    *)         BUILD_SERVER=true; BUILD_CLUSTER=true ;;
esac

# ── Prerequisites ──────────────────────────────────────────────────────────

echo -e "${CYAN}Checking prerequisites...${NC}"

if ! command -v cargo &>/dev/null; then
    echo -e "${RED}Error: cargo not found. Install the Rust toolchain first.${NC}"
    exit 1
fi

if ! command -v node &>/dev/null || ! command -v npm &>/dev/null; then
    echo -e "${RED}Error: node/npm not found. Install Node.js first.${NC}"
    exit 1
fi

mkdir -p "$DIST"

# ── Build UI (shared by both installers) ───────────────────────────────────

echo ""
echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}  Building vmm-ui (React)${NC}"
echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
(cd "$ROOT/apps/vmm-ui" && npm install --silent 2>/dev/null && npx vite build)
echo -e "${GREEN}✓ vmm-ui built${NC}"

# ── Build vmm-server installer ─────────────────────────────────────────────

if [ "$BUILD_SERVER" = true ]; then
    echo ""
    echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}  Building vmm-server${NC}"
    echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
    cargo build --release -p vmm-server -p vmmctl
    echo -e "${GREEN}✓ vmm-server + vmmctl built${NC}"

    echo -e "${CYAN}Packaging vmm-server installer...${NC}"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"

    # Binaries
    cp "$ROOT/target/release/vmm-server" "$STAGING/"
    cp "$ROOT/target/release/vmmctl" "$STAGING/"

    # BIOS assets
    BIOS_SRC="$ROOT/apps/vmm-server/assets/bios"
    if [ -d "$BIOS_SRC" ]; then
        mkdir -p "$STAGING/assets/bios"
        cp "$BIOS_SRC"/*.bin "$STAGING/assets/bios/" 2>/dev/null || true
    fi

    # UI
    if [ -d "$ROOT/apps/vmm-ui/dist" ]; then
        cp -r "$ROOT/apps/vmm-ui/dist" "$STAGING/ui"
    fi

    # Create tar.gz payload
    PAYLOAD="$DIST/vmm-server-payload.tar.gz"
    (cd "$STAGING" && tar czf "$PAYLOAD" .)

    # Combine header + payload into self-extracting installer
    INSTALLER="$DIST/vmm-server-installer.sh"
    cat "$ROOT/tools/installer-header-vmm-server.sh" "$PAYLOAD" > "$INSTALLER"
    chmod 755 "$INSTALLER"
    rm -f "$PAYLOAD"
    rm -rf "$STAGING"

    SIZE=$(du -h "$INSTALLER" | cut -f1)
    echo -e "${GREEN}✓ vmm-server installer: $INSTALLER ($SIZE)${NC}"
fi

# ── Build vmm-cluster installer ────────────────────────────────────────────

if [ "$BUILD_CLUSTER" = true ]; then
    echo ""
    echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}  Building vmm-cluster${NC}"
    echo -e "${CYAN}══════════════════════════════════════════════════════════${NC}"
    cargo build --release -p vmm-cluster
    echo -e "${GREEN}✓ vmm-cluster built${NC}"

    echo -e "${CYAN}Packaging vmm-cluster installer...${NC}"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"

    # Binary
    cp "$ROOT/target/release/vmm-cluster" "$STAGING/"

    # UI (required for vmm-cluster production)
    if [ -d "$ROOT/apps/vmm-ui/dist" ]; then
        cp -r "$ROOT/apps/vmm-ui/dist" "$STAGING/ui"
    else
        echo -e "${RED}Error: vmm-ui dist/ not found. UI build may have failed.${NC}"
        exit 1
    fi

    # Create tar.gz payload
    PAYLOAD="$DIST/vmm-cluster-payload.tar.gz"
    (cd "$STAGING" && tar czf "$PAYLOAD" .)

    # Combine header + payload into self-extracting installer
    INSTALLER="$DIST/vmm-cluster-installer.sh"
    cat "$ROOT/tools/installer-header-vmm-cluster.sh" "$PAYLOAD" > "$INSTALLER"
    chmod 755 "$INSTALLER"
    rm -f "$PAYLOAD"
    rm -rf "$STAGING"

    SIZE=$(du -h "$INSTALLER" | cut -f1)
    echo -e "${GREEN}✓ vmm-cluster installer: $INSTALLER ($SIZE)${NC}"
fi

# ── Summary ────────────────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}══════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Build complete!${NC}"
echo -e "${GREEN}══════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Output directory: ${CYAN}$DIST/${NC}"
echo ""
if [ "$BUILD_SERVER" = true ] && [ -f "$DIST/vmm-server-installer.sh" ]; then
    echo -e "  ${CYAN}vmm-server-installer.sh${NC}"
    echo -e "    Install: sudo ./dist/vmm-server-installer.sh"
    echo -e "    Remove:  sudo ./dist/vmm-server-installer.sh --uninstall"
    echo ""
fi
if [ "$BUILD_CLUSTER" = true ] && [ -f "$DIST/vmm-cluster-installer.sh" ]; then
    echo -e "  ${CYAN}vmm-cluster-installer.sh${NC}"
    echo -e "    Install: sudo ./dist/vmm-cluster-installer.sh"
    echo -e "    Remove:  sudo ./dist/vmm-cluster-installer.sh --uninstall"
    echo ""
fi
echo -e "  Copy the installer to a target machine and run with sudo."
echo ""
