#!/bin/bash
set -euo pipefail

# prepare-coresan.sh — Install all build and runtime dependencies for CoreSAN (vmm-san)
# Run this once on a fresh Debian/Ubuntu build machine before building vmm-san.
# Requires root (sudo).
#
# This installs:
#   Build-time:  pkg-config, libfuse3-dev (FUSE 3 headers for fuser crate)
#   Runtime:     fuse3 (FUSE 3 userspace tools), libfuse3-3 (shared library)
#   Rust:        stable toolchain (if not present)

echo "=== CoreSAN (vmm-san) — Build Environment Setup ==="

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

        echo "[2/4] Installing CoreSAN build dependencies..."
        apt-get install -y --no-install-recommends \
            pkg-config \
            libfuse3-dev \
            fuse3 \
            build-essential \
            libssl-dev \
            ca-certificates \
            curl

        echo "[3/4] Installing Rust toolchain (if not present)..."
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            echo "Rust installed. Source ~/.cargo/env or re-login to use."
        else
            echo "Rust already installed: $(cargo --version)"
        fi

        echo "[4/4] Verifying FUSE3 installation..."
        if pkg-config --exists fuse3; then
            FUSE_VER=$(pkg-config --modversion fuse3)
            echo "FUSE3 found: version $FUSE_VER"
        else
            echo "ERROR: FUSE3 pkg-config not found after install!"
            echo "Try: dpkg -L libfuse3-dev | grep .pc"
            exit 1
        fi

        # Ensure fuse kernel module is available
        if ! lsmod | grep -q '^fuse'; then
            echo "Loading fuse kernel module..."
            modprobe fuse || echo "WARNING: Could not load fuse module (may need reboot or running in container)"
        fi
        ;;

    fedora|rhel|centos|rocky|alma)
        echo "[1/4] Installing CoreSAN build dependencies..."
        dnf install -y \
            pkg-config \
            fuse3-devel \
            fuse3 \
            gcc \
            openssl-devel \
            ca-certificates \
            curl

        echo "[2/4] Installing Rust toolchain (if not present)..."
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            echo "Rust installed. Source ~/.cargo/env or re-login to use."
        else
            echo "Rust already installed: $(cargo --version)"
        fi

        echo "[3/4] Verifying FUSE3 installation..."
        if pkg-config --exists fuse3; then
            FUSE_VER=$(pkg-config --modversion fuse3)
            echo "FUSE3 found: version $FUSE_VER"
        else
            echo "ERROR: FUSE3 pkg-config not found after install!"
            exit 1
        fi

        echo "[4/4] Loading fuse kernel module..."
        modprobe fuse 2>/dev/null || true
        ;;

    opensuse*|sles)
        echo "[1/4] Installing CoreSAN build dependencies..."
        zypper install -y \
            pkg-config \
            fuse3-devel \
            fuse3 \
            gcc \
            libopenssl-devel \
            ca-certificates \
            curl

        echo "[2/4] Installing Rust toolchain (if not present)..."
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            echo "Rust installed. Source ~/.cargo/env or re-login to use."
        else
            echo "Rust already installed: $(cargo --version)"
        fi

        echo "[3/4] Verifying FUSE3 installation..."
        if pkg-config --exists fuse3; then
            FUSE_VER=$(pkg-config --modversion fuse3)
            echo "FUSE3 found: version $FUSE_VER"
        else
            echo "ERROR: FUSE3 pkg-config not found after install!"
            exit 1
        fi

        echo "[4/4] Loading fuse kernel module..."
        modprobe fuse 2>/dev/null || true
        ;;

    arch|manjaro)
        echo "[1/4] Installing CoreSAN build dependencies..."
        pacman -S --noconfirm --needed \
            pkgconf \
            fuse3 \
            base-devel \
            openssl \
            ca-certificates \
            curl

        echo "[2/4] Installing Rust toolchain (if not present)..."
        if ! command -v cargo >/dev/null 2>&1; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
            echo "Rust installed. Source ~/.cargo/env or re-login to use."
        else
            echo "Rust already installed: $(cargo --version)"
        fi

        echo "[3/4] Verifying FUSE3 installation..."
        if pkg-config --exists fuse3; then
            FUSE_VER=$(pkg-config --modversion fuse3)
            echo "FUSE3 found: version $FUSE_VER"
        else
            echo "ERROR: FUSE3 pkg-config not found after install!"
            exit 1
        fi

        echo "[4/4] Loading fuse kernel module..."
        modprobe fuse 2>/dev/null || true
        ;;

    *)
        echo "ERROR: Unsupported distribution '$DISTRO'."
        echo "This script supports Debian/Ubuntu, Fedora/RHEL/Rocky, openSUSE, and Arch Linux."
        echo ""
        echo "You need to manually install these packages:"
        echo "  Build:   pkg-config, libfuse3-dev (or fuse3-devel), gcc, openssl-dev"
        echo "  Runtime: fuse3"
        echo "  Rust:    stable toolchain (https://rustup.rs)"
        echo ""
        echo "Verify with: pkg-config --modversion fuse3"
        exit 1
        ;;
esac

echo ""
echo "=== CoreSAN build environment ready ==="
echo ""
echo "Verify with:"
echo "  pkg-config --modversion fuse3"
echo "  cargo --version"
echo "  fusermount3 --version"
echo ""
echo "Build CoreSAN with:"
echo "  cargo build -p vmm-san --release"
echo ""
echo "Runtime requirements on target hosts:"
echo "  - fuse3 package installed"
echo "  - 'fuse' kernel module loaded"
echo "  - User in 'fuse' group (or running as root)"
echo "  - /etc/fuse.conf: user_allow_other enabled (for VM access)"
