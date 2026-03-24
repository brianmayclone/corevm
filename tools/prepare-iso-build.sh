#!/bin/bash
set -euo pipefail

# prepare-iso-build.sh — Install all host dependencies needed by build-iso.sh
# Run this once on a fresh Debian/Ubuntu build machine before running build-iso.sh.
# Requires root (sudo).

echo "=== CoreVM Appliance ISO — Build Environment Setup ==="

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root (or with sudo)."
    exit 1
fi

# Detect distro
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO="${ID:-unknown}"
else
    DISTRO="unknown"
fi

case "$DISTRO" in
    debian|ubuntu|linuxmint|pop)
        echo "[1/4] Updating package lists..."
        apt-get update -qq

        echo "[2/4] Installing ISO build dependencies..."
        apt-get install -y --no-install-recommends \
            debootstrap \
            xorriso \
            isolinux \
            syslinux-utils \
            grub-efi-amd64-bin \
            grub-pc-bin \
            grub-common \
            mtools \
            squashfs-tools \
            dosfstools \
            e2fsprogs \
            parted \
            live-boot \
            ca-certificates \
            curl \
            tar \
            gzip

        echo "[3/4] Installing Rust toolchain (if not present)..."
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            echo "Rust installed. Source ~/.cargo/env or re-login to use."
        else
            echo "Rust already installed: $(cargo --version)"
        fi

        echo "[4/4] Installing Node.js 20+ (required for Vite build)..."
        NODE_MAJOR=0
        if command -v node >/dev/null 2>&1; then
            NODE_MAJOR=$(node --version | sed 's/v\([0-9]*\).*/\1/')
        fi
        if [ "$NODE_MAJOR" -lt 20 ]; then
            echo "Node.js ${NODE_MAJOR:-not found} is too old, installing Node.js 20..."
            curl -fsSL https://deb.nodesource.com/setup_20.x | bash -
            apt-get install -y nodejs
        else
            echo "Node.js already installed: $(node --version)"
        fi
        ;;
    *)
        echo "ERROR: Unsupported distribution '$DISTRO'."
        echo "This script supports Debian, Ubuntu, Linux Mint, and Pop!_OS."
        echo ""
        echo "You need to manually install these packages:"
        echo "  debootstrap xorriso isolinux syslinux-utils"
        echo "  grub-efi-amd64-bin grub-pc-bin grub-common"
        echo "  mtools squashfs-tools dosfstools e2fsprogs parted"
        echo "  live-boot ca-certificates curl tar gzip"
        echo "Plus: Rust toolchain (rustup.rs) and Node.js 20+"
        exit 1
        ;;
esac

echo ""
echo "=== Build environment ready ==="
echo "Verify with:"
echo "  debootstrap --version"
echo "  xorriso --version"
echo "  grub-mkimage --version"
echo "  mksquashfs -version"
echo "  cargo --version"
echo "  node --version"
echo ""
echo "Then run: sudo ./tools/build-iso.sh"
