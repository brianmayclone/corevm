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

# ── Ensure cargo is in PATH (when called via sudo) ────────────────────

if ! command -v cargo >/dev/null 2>&1; then
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

# ── Build vmm-s3gw (S3 Gateway) ──────────────────────────────────────────

echo ""
echo -e "${CYAN}=== Building vmm-s3gw / S3 Gateway (Rust) ===${NC}"
cargo build --release -p vmm-s3gw
echo -e "${GREEN}✓ vmm-s3gw built${NC}"

# ── Build vmm-ui ─────────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}=== Building vmm-ui (React) ===${NC}"
(cd "$ROOT/apps/vmm-ui" && npm install --silent 2>/dev/null && npx vite build)
echo -e "${GREEN}✓ vmm-ui built → apps/vmm-ui/dist/${NC}"

# ── Run mode ─────────────────────────────────────────────────────────────

# ── Handle --reset flag ───────────────────────────────────────────────────

for arg in "$@"; do
    if [ "$arg" = "--reset" ]; then
        echo -e "${YELLOW}=== Resetting all data ===${NC}"
        # Kill running services first
        pkill -f "vmm-s3gw" 2>/dev/null || true
        pkill -f "vmm-san" 2>/dev/null || true
        pkill -f "vmm-server.*config" 2>/dev/null || true
        pkill -f "vmm-cluster.*config" 2>/dev/null || true
        sleep 1
        # Force-unmount ALL FUSE mounts under /tmp/vmm-san
        if [ -d /tmp/vmm-san/mnt ]; then
            find /tmp/vmm-san/mnt -maxdepth 1 -mindepth 1 -type d 2>/dev/null | while read mnt; do
                fusermount3 -u "$mnt" 2>/dev/null || true
                umount -l "$mnt" 2>/dev/null || true
                umount -f "$mnt" 2>/dev/null || true
            done
        fi
        sleep 1
        # Remove data and configs
        rm -rf /tmp/vmm-cluster /tmp/vmm /tmp/vmm-san 2>/dev/null || true
        # If rm failed on FUSE dirs, lazy-unmount and retry
        if [ -d /tmp/vmm-san ]; then
            umount -l /tmp/vmm-san/mnt/* 2>/dev/null || true
            rm -rf /tmp/vmm-san 2>/dev/null || true
        fi
        rm -f "$ROOT/vmm-cluster.toml" "$ROOT/vmm-server.toml" "$ROOT/vmm-san.toml" "$ROOT/vmm-s3gw.toml"
        echo -e "${GREEN}All configs, data, and running services removed.${NC}"
    fi
done

if [[ " $* " == *" --run "* ]]; then
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

    # ── Create S3 Gateway config if missing ─────────────────────────

    S3GW_CONFIG="$ROOT/vmm-s3gw.toml"
    if [ ! -f "$S3GW_CONFIG" ]; then
        cat > "$S3GW_CONFIG" << 'EOF'
[server]
listen = "0.0.0.0:9000"
region = "us-east-1"

[san]
mgmt_socket = "/run/vmm-san/mgmt.sock"
object_socket_dir = "/run/vmm-san"

[logging]
level = "info"
EOF
        echo -e "Created S3 gateway config: ${S3GW_CONFIG}"
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

        # ── Start vmm-s3gw (S3 Gateway — depends on vmm-san sockets) ──

        echo -e "${CYAN}Starting vmm-s3gw (S3 Gateway) on :9000...${NC}"
        "$ROOT/target/release/vmm-s3gw" --config "$S3GW_CONFIG" &
        PIDS+=($!)
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
    echo -e "  ${CYAN}vmm-s3gw${NC}     : http://localhost:9000   (S3 gateway)"
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
