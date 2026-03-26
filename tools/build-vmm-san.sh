#!/bin/bash
# Build (and optionally run) vmm-san (CoreSAN).
#
# Usage:
#   ./tools/build-vmm-san.sh          # Build only (no sudo needed)
#   ./tools/build-vmm-san.sh --run    # Build and run (needs sudo for FUSE mount)
#
# If running with sudo, use:  sudo -E env "PATH=$PATH" ./tools/build-vmm-san.sh --run
# This preserves the Rust toolchain in PATH.
#
# CoreSAN runs as a standalone storage daemon on port 7443.
# It works independently — no vmm-server or vmm-cluster required.

set -e
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

# ── Ensure cargo is in PATH (when called via sudo) ────────────────────

if ! command -v cargo >/dev/null 2>&1; then
    # Try common Rust install locations
    for CARGO_HOME in "$HOME/.cargo" "/home/${SUDO_USER:-$USER}/.cargo" "/root/.cargo"; do
        if [ -f "$CARGO_HOME/env" ]; then
            . "$CARGO_HOME/env"
            break
        fi
    done
    if ! command -v cargo >/dev/null 2>&1; then
        echo -e "${RED}ERROR: cargo not found in PATH.${NC}"
        echo -e "If running with sudo, use: ${CYAN}sudo -E env \"PATH=\$PATH\" $0 $*${NC}"
        exit 1
    fi
fi

# ── Check FUSE3 ────────────────────────────────────────────────────────

if ! pkg-config --exists fuse3 2>/dev/null; then
    echo -e "${RED}ERROR: FUSE3 development headers not found.${NC}"
    echo -e "Run: ${CYAN}sudo ./tools/prepare-coresan.sh${NC}"
    exit 1
fi

# ── Build vmm-san ──────────────────────────────────────────────────────

echo -e "${CYAN}=== Building vmm-san / CoreSAN (Rust) ===${NC}"
cargo build --release -p vmm-san
echo -e "${GREEN}✓ vmm-san built${NC}"

# ── Run mode ───────────────────────────────────────────────────────────

if [ "$1" = "--run" ]; then
    echo ""
    echo -e "${CYAN}=== Starting CoreSAN ===${NC}"

    # Create config if missing
    SAN_CONFIG="$ROOT/vmm-san.toml"
    if [ ! -f "$SAN_CONFIG" ]; then
        cat > "$SAN_CONFIG" << 'EOF'
[server]
bind = "0.0.0.0"
port = 7443

[data]
data_dir = "/tmp/vmm-san"
fuse_root = "/tmp/vmm-san/mnt"

[peer]
port = 7444
secret = ""

[replication]
sync_mode = "async"

[benchmark]
enabled = true
interval_secs = 300
bandwidth_test_size_mb = 64

[integrity]
enabled = true
interval_secs = 3600
repair_interval_secs = 60

[logging]
level = "info"
EOF
        echo -e "Created default config: ${SAN_CONFIG}"
    fi

    # Create data directories
    mkdir -p /tmp/vmm-san /tmp/vmm-san/mnt

    # Unmount stale FUSE mounts from previous runs
    for mnt in /tmp/vmm-san/mnt/*/; do
        if mountpoint -q "$mnt" 2>/dev/null; then
            echo -e "${YELLOW}Unmounting stale FUSE: $mnt${NC}"
            fusermount3 -u "$mnt" 2>/dev/null || fusermount -u "$mnt" 2>/dev/null || true
        fi
    done

    # Start vmm-san
    cd "$ROOT"
    echo -e "${CYAN}Starting vmm-san on :7443...${NC}"
    "$ROOT/target/release/vmm-san" --config "$SAN_CONFIG" &
    SAN_PID=$!

    sleep 1

    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}  CoreSAN Running${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
    echo ""
    echo -e "  ${CYAN}REST API${NC}     : http://localhost:7443"
    echo -e "  ${CYAN}FUSE root${NC}    : /tmp/vmm-san/mnt"
    echo -e "  ${CYAN}Database${NC}     : /tmp/vmm-san/vmm-san.db"
    echo ""
    echo -e "  ${CYAN}Quick Start:${NC}"
    echo -e "    1. Create a volume:"
    echo -e "       curl -X POST http://localhost:7443/api/volumes \\"
    echo -e "         -H 'Content-Type: application/json' \\"
    echo -e "         -d '{\"name\": \"pool-a\", \"resilience_mode\": \"none\", \"replica_count\": 1}'"
    echo ""
    echo -e "    2. Add a backend (mountpoint):"
    echo -e "       mkdir -p /tmp/vmm-san/backend1"
    echo -e "       curl -X POST http://localhost:7443/api/volumes/<VOLUME_ID>/backends \\"
    echo -e "         -H 'Content-Type: application/json' \\"
    echo -e "         -d '{\"path\": \"/tmp/vmm-san/backend1\"}'"
    echo ""
    echo -e "    3. Write a file:"
    echo -e "       curl -X PUT http://localhost:7443/api/volumes/<VOLUME_ID>/files/test.txt \\"
    echo -e "         -d 'Hello CoreSAN!'"
    echo ""
    echo -e "    4. Check status:"
    echo -e "       curl http://localhost:7443/api/status | python3 -m json.tool"
    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
    echo ""
    echo "Press Ctrl+C to stop."

    trap "kill $SAN_PID 2>/dev/null; exit 0" INT TERM
    wait $SAN_PID
fi
