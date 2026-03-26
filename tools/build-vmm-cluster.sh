#!/bin/bash
# Build (and optionally run) vmm-cluster + vmm-server (as agent) + vmm-ui.
#
# Usage:
#   ./tools/build-vmm-cluster.sh          # Build only
#   ./tools/build-vmm-cluster.sh --run    # Build and run all components
#
# This starts the full vSphere-like stack:
#   - vmm-cluster on :9443 (central authority)
#   - vmm-server  on :8443 (host agent — can be registered with the cluster)
#   - vmm-ui      on :5173 (dev server, proxied to vmm-cluster)
#
# The UI connects to vmm-cluster by default. You can also open :8443 directly
# to see the standalone vmm-server (or the "Managed by Cluster" page after registration).

set -e
cd "$(dirname "$0")/.."
ROOT="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

# ── Build vmm-cluster ────────────────────────────────────────────────────

echo -e "${CYAN}=== Building vmm-cluster (Rust) ===${NC}"
cargo build --release -p vmm-cluster
echo -e "${GREEN}✓ vmm-cluster built${NC}"

# ── Build vmm-server ─────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}=== Building vmm-server (Rust) ===${NC}"
cargo build --release -p vmm-server
echo -e "${GREEN}✓ vmm-server built${NC}"

# ── Build vmm-san (CoreSAN) ─────────────────────────────────────────────

echo ""
echo -e "${CYAN}=== Building vmm-san / CoreSAN (Rust) ===${NC}"
if pkg-config --exists fuse3 2>/dev/null; then
    cargo build --release -p vmm-san
    echo -e "${GREEN}✓ vmm-san built${NC}"
    HAS_SAN=1
else
    echo -e "${YELLOW}⚠ FUSE3 not found — skipping vmm-san (run tools/prepare-coresan.sh)${NC}"
    HAS_SAN=0
fi

# Copy BIOS assets next to the binary
BIOS_SRC="$ROOT/apps/vmm-server/assets/bios"
BIOS_DST="$ROOT/target/release/assets/bios"
if [ -d "$BIOS_SRC" ]; then
    mkdir -p "$BIOS_DST"
    cp -u "$BIOS_SRC"/*.bin "$BIOS_DST/" 2>/dev/null || true
    echo -e "${GREEN}✓ BIOS files copied${NC}"
fi

# ── Build vmm-ui ─────────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}=== Building vmm-ui (React) ===${NC}"
(cd "$ROOT/apps/vmm-ui" && npm install --silent 2>/dev/null && npx vite build)
echo -e "${GREEN}✓ vmm-ui built → apps/vmm-ui/dist/${NC}"

# ── Run mode ─────────────────────────────────────────────────────────────

if [ "$1" = "--run" ]; then
    echo ""
    echo -e "${CYAN}=== Starting VMM-Cluster Stack ===${NC}"

    PIDS=()

    cleanup() {
        echo ""
        echo -e "${YELLOW}Stopping all services...${NC}"
        for pid in "${PIDS[@]}"; do
            kill "$pid" 2>/dev/null || true
        done
        wait "${PIDS[@]}" 2>/dev/null || true
        echo -e "${GREEN}All services stopped.${NC}"
        exit 0
    }
    trap cleanup INT TERM

    # ── Create vmm-cluster config if missing ─────────────────────────

    CLUSTER_CONFIG="$ROOT/vmm-cluster.toml"
    if [ ! -f "$CLUSTER_CONFIG" ]; then
        cat > "$CLUSTER_CONFIG" << 'EOF'
[server]
bind = "0.0.0.0"
port = 9443

[auth]
jwt_secret = "cluster-dev-secret-change-in-production"
session_timeout_hours = 24

[data]
data_dir = "/tmp/vmm-cluster"

[logging]
level = "info"
EOF
        echo -e "Created cluster config: ${CLUSTER_CONFIG}"
    fi

    # ── Create vmm-server config if missing ──────────────────────────

    SERVER_CONFIG="$ROOT/vmm-server.toml"
    if [ ! -f "$SERVER_CONFIG" ]; then
        cat > "$SERVER_CONFIG" << 'EOF'
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
        echo -e "Created server config: ${SERVER_CONFIG}"
    fi

    # ── Create CoreSAN config if missing ──────────────────────────

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
        echo -e "Created CoreSAN config: ${SAN_CONFIG}"
    fi

    # ── Create data directories ──────────────────────────────────────

    mkdir -p /tmp/vmm-cluster /tmp/vmm/images /tmp/vmm/isos /tmp/vmm/vms /tmp/vmm-san /tmp/vmm-san/mnt

    # ── Start CoreSAN (before vmm-server, so storage is ready) ───────

    cd "$ROOT"
    if [ "${HAS_SAN:-0}" = "1" ]; then
        echo -e "${CYAN}Starting vmm-san (CoreSAN) on :7443...${NC}"
        "$ROOT/target/release/vmm-san" --config "$SAN_CONFIG" &
        PIDS+=($!)
        sleep 1
    fi

    # ── Start vmm-cluster ────────────────────────────────────────────

    echo -e "${CYAN}Starting vmm-cluster on :9443...${NC}"
    "$ROOT/target/release/vmm-cluster" --config "$CLUSTER_CONFIG" &
    PIDS+=($!)

    # Give cluster a moment to initialize
    sleep 1

    # ── Start vmm-server ─────────────────────────────────────────────

    echo -e "${CYAN}Starting vmm-server on :8443...${NC}"
    "$ROOT/target/release/vmm-server" --config "$SERVER_CONFIG" &
    PIDS+=($!)

    # ── Start vmm-ui (proxied to cluster) ────────────────────────────
    # Override the proxy target to point to vmm-cluster instead of vmm-server

    echo -e "${CYAN}Starting vmm-ui dev server on :5173 → proxied to vmm-cluster :9443...${NC}"
    (cd "$ROOT/apps/vmm-ui" && VITE_API_TARGET="http://localhost:9443" npx vite --host) &
    PIDS+=($!)

    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}  VMM-Cluster Stack Running${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
    if [ "${HAS_SAN:-0}" = "1" ]; then
    echo -e "  ${CYAN}vmm-san${NC}      : http://localhost:7443   (CoreSAN storage)"
    fi
    echo -e "  ${CYAN}vmm-cluster${NC}  : http://localhost:9443   (central authority)"
    echo -e "  ${CYAN}vmm-server${NC}   : http://localhost:8443   (host agent)"
    echo -e "  ${CYAN}vmm-ui${NC}       : http://localhost:5173   (web UI → cluster)"
    echo ""
    echo -e "  ${YELLOW}Login${NC}        : admin / admin"
    echo ""
    echo -e "  ${CYAN}Quick Start:${NC}"
    echo -e "    1. Open http://localhost:5173 → login as admin/admin"
    echo -e "    2. Create a Cluster: Cluster → Clusters → New Cluster"
    echo -e "    3. Add the local vmm-server as host:"
    echo -e "       Cluster → Hosts → Add Host"
    echo -e "       Address: http://localhost:8443"
    echo -e "       Admin: admin / admin"
    echo ""
    echo -e "  After registration, http://localhost:8443 will show"
    echo -e "  \"Managed by VMM-Cluster\" and redirect to the cluster UI."
    echo ""
    echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo "Press Ctrl+C to stop all services."

    # Wait for any child to exit
    wait "${PIDS[@]}"
fi
