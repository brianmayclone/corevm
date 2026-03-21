#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# VMM-Server Self-Extracting Installer
# ─────────────────────────────────────────────────────────────────────────────
# This file is a self-extracting archive. Run it to install vmm-server.
#
# Usage:
#   sudo ./vmm-server-installer.sh              # Interactive install
#   sudo ./vmm-server-installer.sh --uninstall  # Remove vmm-server
#
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

INSTALL_DIR="/opt/vmm-server"
CONFIG_DIR="/etc/vmm"
DATA_DIR="/var/lib/vmm"
SERVICE_NAME="vmm-server"

# ── Root check ─────────────────────────────────────────────────────────────
if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}Error: This installer must be run as root (sudo).${NC}"
    exit 1
fi

# ── Uninstall mode ─────────────────────────────────────────────────────────
if [ "$1" = "--uninstall" ]; then
    echo -e "${CYAN}=== Uninstalling vmm-server ===${NC}"
    systemctl stop "$SERVICE_NAME" 2>/dev/null || true
    systemctl disable "$SERVICE_NAME" 2>/dev/null || true
    rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
    systemctl daemon-reload 2>/dev/null || true
    rm -rf "$INSTALL_DIR"
    echo -e "${GREEN}vmm-server uninstalled.${NC}"
    echo -e "${YELLOW}Config preserved at $CONFIG_DIR and data at $DATA_DIR${NC}"
    echo -e "To remove everything: rm -rf $CONFIG_DIR $DATA_DIR"
    exit 0
fi

# ── Banner ─────────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║           VMM-Server Installer                   ║${NC}"
echo -e "${CYAN}║           CoreVM Web Management Server           ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""

# ── Find payload offset ───────────────────────────────────────────────────
ARCHIVE_MARKER="__ARCHIVE_BELOW__"
ARCHIVE_LINE=$(grep -an "^${ARCHIVE_MARKER}$" "$0" | tail -1 | cut -d: -f1)
if [ -z "$ARCHIVE_LINE" ]; then
    echo -e "${RED}Error: Cannot find archive payload in installer.${NC}"
    exit 1
fi

# ── Extract payload ────────────────────────────────────────────────────────
echo -e "${CYAN}Extracting files...${NC}"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT
tail -n +"$((ARCHIVE_LINE + 1))" "$0" | tar xz -C "$TMPDIR"

# ── Install binary ─────────────────────────────────────────────────────────
echo -e "${CYAN}Installing vmm-server to $INSTALL_DIR...${NC}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/vmm-server" "$INSTALL_DIR/vmm-server"
chmod 755 "$INSTALL_DIR/vmm-server"

# ── Install BIOS assets ───────────────────────────────────────────────────
if [ -d "$TMPDIR/assets" ]; then
    cp -r "$TMPDIR/assets" "$INSTALL_DIR/"
    echo -e "${GREEN}✓ BIOS assets installed${NC}"
fi

# ── Install UI ─────────────────────────────────────────────────────────────
if [ -d "$TMPDIR/ui" ]; then
    rm -rf "$INSTALL_DIR/ui"
    cp -r "$TMPDIR/ui" "$INSTALL_DIR/ui"
    echo -e "${GREEN}✓ Web UI installed${NC}"
fi

# ── Create data directories ───────────────────────────────────────────────
mkdir -p "$DATA_DIR/vms"
mkdir -p "$DATA_DIR/images"
mkdir -p "$DATA_DIR/isos"

# ── Create config ──────────────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_DIR/vmm-server.toml" ]; then
    # Generate a random JWT secret
    JWT_SECRET=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
    cat > "$CONFIG_DIR/vmm-server.toml" << EOF
[server]
bind = "0.0.0.0"
port = 8443

[auth]
jwt_secret = "$JWT_SECRET"
session_timeout_hours = 24

[storage]
default_pool = "$DATA_DIR/images"
iso_pool = "$DATA_DIR/isos"

[vms]
config_dir = "$DATA_DIR/vms"

[logging]
level = "info"
file = "/var/log/vmm-server.log"
EOF
    echo -e "${GREEN}✓ Config created at $CONFIG_DIR/vmm-server.toml${NC}"
else
    echo -e "${YELLOW}Config already exists at $CONFIG_DIR/vmm-server.toml — skipping${NC}"
fi

# ── Create systemd service ────────────────────────────────────────────────
cat > "/etc/systemd/system/${SERVICE_NAME}.service" << EOF
[Unit]
Description=VMM-Server — CoreVM Web Management Server
After=network.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/vmm-server --config $CONFIG_DIR/vmm-server.toml
WorkingDirectory=$INSTALL_DIR
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Security hardening
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$DATA_DIR /var/log
PrivateTmp=true
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
echo -e "${GREEN}✓ Systemd service created${NC}"

# ── Done ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Installation complete!                          ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Binary:    $INSTALL_DIR/vmm-server"
echo -e "  Config:    $CONFIG_DIR/vmm-server.toml"
echo -e "  Data:      $DATA_DIR/"
echo -e "  Service:   $SERVICE_NAME"
echo ""
echo -e "  ${CYAN}Start the service:${NC}"
echo -e "    sudo systemctl enable --now $SERVICE_NAME"
echo ""
echo -e "  ${CYAN}Check status:${NC}"
echo -e "    sudo systemctl status $SERVICE_NAME"
echo ""
echo -e "  ${CYAN}View logs:${NC}"
echo -e "    sudo journalctl -u $SERVICE_NAME -f"
echo ""
echo -e "  ${CYAN}Web UI:${NC}  http://$(hostname -I 2>/dev/null | awk '{print $1}' || echo 'localhost'):8443"
echo -e "  ${CYAN}Login:${NC}   admin / admin"
echo ""

exit 0
__ARCHIVE_BELOW__
