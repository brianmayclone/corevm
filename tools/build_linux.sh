#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Must run as root
# Use: sudo -E env "PATH=$PATH" ./tools/build_linux.sh
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: This script must be run as root."
    echo "Usage: sudo -E env \"PATH=\$PATH\" ./tools/build_linux.sh"
    exit 1
fi

# Ensure cargo is in PATH (may come from user's home dir)
for p in "$HOME/.cargo/bin" "/usr/local/bin"; do
    [ -d "$p" ] && export PATH="$p:$PATH"
done
# Also check SUDO_USER's home for cargo
if [ -n "${SUDO_USER:-}" ]; then
    SUDO_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
    [ -d "$SUDO_HOME/.cargo/bin" ] && export PATH="$SUDO_HOME/.cargo/bin:$PATH"
fi

cd "$ROOT/apps/vmmanager"

# Always clean libcorevm artifacts to avoid stale builds
cargo clean -p libcorevm 2>/dev/null || true

if [ "${1:-}" = "--clean" ]; then
    cargo clean
    shift
    echo "Cleaned build artifacts."
fi

cargo +stable build --release --features libcorevm/linux
echo "Built: target/x86_64-unknown-linux-gnu/release/corevm-vmmanager"

if [ "${1:-}" = "--run" ]; then
    exec cargo +stable run --release --features libcorevm/linux -- "${@:2}"
fi
