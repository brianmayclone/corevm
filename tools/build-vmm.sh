#!/bin/bash
# Build (and optionally run) vmm-server + vmm-ui.
#
# Usage:
#   ./tools/build-vmm.sh          # Build only
#   ./tools/build-vmm.sh --run    # Build and run both

set -e
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m'

echo -e "${CYAN}=== Building vmm-server (Rust) ===${NC}"
cargo build --release -p vmm-server
echo -e "${GREEN}✓ vmm-server built${NC}"

echo -e "${CYAN}=== Building vmmctl (CLI) ===${NC}"
cargo build --release -p vmmctl
echo -e "${GREEN}✓ vmmctl built${NC}"

# Copy BIOS assets next to the binary
BIOS_SRC="$ROOT/apps/vmm-server/assets/bios"
BIOS_DST="$ROOT/target/release/assets/bios"
if [ -d "$BIOS_SRC" ]; then
    mkdir -p "$BIOS_DST"
    cp -u "$BIOS_SRC"/*.bin "$BIOS_DST/" 2>/dev/null || true
    echo -e "${GREEN}✓ BIOS files copied to target/release/assets/bios/${NC}"
fi

echo ""
echo -e "${CYAN}=== Building vmm-ui (React) ===${NC}"
(cd "$ROOT/apps/vmm-ui" && npm install --silent 2>/dev/null && npx vite build)
echo -e "${GREEN}✓ vmm-ui built → apps/vmm-ui/dist/${NC}"

if [ "$1" = "--run" ]; then
    echo ""
    echo -e "${CYAN}=== Starting vmm-server + vmm-ui ===${NC}"

    # Create default config if missing
    CONFIG="$ROOT/vmm-server.toml"
    if [ ! -f "$CONFIG" ]; then
        cat > "$CONFIG" << EOF
[server]
bind = "0.0.0.0"
port = 8443

[auth]
jwt_secret = "dev-secret-change-in-production"
session_timeout_hours = 24

[storage]
default_pool = "/tmp/vmm/images"
iso_pool = "/tmp/vmm/isos"

[vms]
config_dir = "/tmp/vmm/vms"

[logging]
level = "info"
EOF
        echo -e "Created default config: ${CONFIG}"
    fi

    # Start vmm-server from project root (so relative paths work)
    cd "$ROOT"
    echo -e "${CYAN}Starting vmm-server on :8443...${NC}"
    "$ROOT/target/release/vmm-server" --config "$CONFIG" &
    SERVER_PID=$!

    # Start vmm-ui dev server in a subshell (proxies API to :8443)
    echo -e "${CYAN}Starting vmm-ui dev server on :5173...${NC}"
    (cd "$ROOT/apps/vmm-ui" && npx vite --host) &
    UI_PID=$!

    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════${NC}"
    echo -e "${GREEN}  vmm-server : http://localhost:8443${NC}"
    echo -e "${GREEN}  vmm-ui     : http://localhost:5173${NC}"
    echo -e "${GREEN}  Login      : admin / admin${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════${NC}"
    echo ""
    echo "Press Ctrl+C to stop both."

    # Trap Ctrl+C to kill both
    trap "kill $SERVER_PID $UI_PID 2>/dev/null; exit 0" INT TERM
    wait
fi
