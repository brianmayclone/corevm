#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# VMM-Cluster Self-Extracting Installer
# ─────────────────────────────────────────────────────────────────────────────
# This file is a self-extracting archive. Run it to install vmm-cluster.
#
# Automatically detects the init system (systemd, OpenRC, SysVinit, runit)
# and installs the appropriate service configuration.
#
# Usage:
#   sudo ./vmm-cluster-installer.sh              # Install
#   sudo ./vmm-cluster-installer.sh --uninstall  # Remove vmm-cluster
#
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

INSTALL_DIR="/opt/vmm-cluster"
CONFIG_DIR="/etc/vmm"
DATA_DIR="/var/lib/vmm-cluster"
SERVICE_NAME="vmm-cluster"
LOG_DIR="/var/log"

# ── Init system detection ────────────────────────────────────────────────────
detect_init_system() {
    if [ -d /run/systemd/system ] && command -v systemctl &>/dev/null; then
        echo "systemd"
    elif command -v rc-service &>/dev/null && [ -d /etc/init.d ]; then
        echo "openrc"
    elif command -v sv &>/dev/null && [ -d /etc/sv ]; then
        echo "runit"
    elif [ -f /etc/inittab ] || command -v update-rc.d &>/dev/null || command -v chkconfig &>/dev/null; then
        echo "sysvinit"
    else
        echo "none"
    fi
}

INIT_SYSTEM=$(detect_init_system)

# ── Root check ───────────────────────────────────────────────────────────────
if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}Error: This installer must be run as root (sudo).${NC}"
    exit 1
fi

# ── Service management helpers ───────────────────────────────────────────────
stop_service() {
    case "$INIT_SYSTEM" in
        systemd)
            systemctl stop "$SERVICE_NAME" 2>/dev/null || true
            systemctl disable "$SERVICE_NAME" 2>/dev/null || true
            ;;
        openrc)
            rc-service "$SERVICE_NAME" stop 2>/dev/null || true
            rc-update del "$SERVICE_NAME" default 2>/dev/null || true
            ;;
        runit)
            sv stop "$SERVICE_NAME" 2>/dev/null || true
            rm -f "/var/service/$SERVICE_NAME"
            ;;
        sysvinit)
            "/etc/init.d/$SERVICE_NAME" stop 2>/dev/null || true
            if command -v update-rc.d &>/dev/null; then
                update-rc.d -f "$SERVICE_NAME" remove 2>/dev/null || true
            elif command -v chkconfig &>/dev/null; then
                chkconfig --del "$SERVICE_NAME" 2>/dev/null || true
            fi
            ;;
        *)
            if [ -f "/var/run/${SERVICE_NAME}.pid" ]; then
                kill "$(cat "/var/run/${SERVICE_NAME}.pid")" 2>/dev/null || true
                rm -f "/var/run/${SERVICE_NAME}.pid"
            fi
            ;;
    esac
}

remove_service_files() {
    case "$INIT_SYSTEM" in
        systemd)
            rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
            systemctl daemon-reload 2>/dev/null || true
            ;;
        openrc)
            rm -f "/etc/init.d/$SERVICE_NAME"
            ;;
        runit)
            rm -rf "/etc/sv/$SERVICE_NAME"
            rm -f "/var/service/$SERVICE_NAME"
            ;;
        sysvinit)
            rm -f "/etc/init.d/$SERVICE_NAME"
            ;;
        *)
            rm -f "$INSTALL_DIR/run.sh"
            ;;
    esac
}

# ── Uninstall mode ───────────────────────────────────────────────────────────
if [ "$1" = "--uninstall" ]; then
    echo -e "${CYAN}=== Uninstalling vmm-cluster ===${NC}"
    echo -e "Detected init system: ${CYAN}${INIT_SYSTEM}${NC}"
    stop_service
    remove_service_files
    rm -rf "$INSTALL_DIR"
    echo -e "${GREEN}vmm-cluster uninstalled.${NC}"
    echo -e "${YELLOW}Config preserved at $CONFIG_DIR and data at $DATA_DIR${NC}"
    echo -e "To remove everything: rm -rf $CONFIG_DIR $DATA_DIR"
    exit 0
fi

# ── Banner ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║           VMM-Cluster Installer                  ║${NC}"
echo -e "${CYAN}║           CoreVM Cluster Orchestration Server    ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Detected init system: ${CYAN}${INIT_SYSTEM}${NC}"
echo ""

# ── Find payload offset ─────────────────────────────────────────────────────
ARCHIVE_MARKER="__ARCHIVE_BELOW__"
ARCHIVE_LINE=$(grep -an "^${ARCHIVE_MARKER}$" "$0" | tail -1 | cut -d: -f1)
if [ -z "$ARCHIVE_LINE" ]; then
    echo -e "${RED}Error: Cannot find archive payload in installer.${NC}"
    exit 1
fi

# ── Extract payload ──────────────────────────────────────────────────────────
echo -e "${CYAN}Extracting files...${NC}"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT
tail -n +"$((ARCHIVE_LINE + 1))" "$0" | tar xz -C "$TMPDIR"

# ── Stop existing service if upgrading ───────────────────────────────────────
stop_service

# ── Install binary ───────────────────────────────────────────────────────────
echo -e "${CYAN}Installing vmm-cluster to $INSTALL_DIR...${NC}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/vmm-cluster" "$INSTALL_DIR/vmm-cluster"
chmod 755 "$INSTALL_DIR/vmm-cluster"

# ── Install UI ───────────────────────────────────────────────────────────────
if [ -d "$TMPDIR/ui" ]; then
    rm -rf "$INSTALL_DIR/ui"
    cp -r "$TMPDIR/ui" "$INSTALL_DIR/ui"
    echo -e "${GREEN}✓ Web UI installed${NC}"
fi

# ── Create data directory ────────────────────────────────────────────────────
mkdir -p "$DATA_DIR"
mkdir -p "$LOG_DIR"

# ── Create config ────────────────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"
if [ ! -f "$CONFIG_DIR/vmm-cluster.toml" ]; then
    JWT_SECRET=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
    cat > "$CONFIG_DIR/vmm-cluster.toml" << EOF
[server]
bind = "0.0.0.0"
port = 9443

[auth]
jwt_secret = "$JWT_SECRET"
session_timeout_hours = 24

[data]
data_dir = "$DATA_DIR"

[logging]
level = "info"
file = "$LOG_DIR/vmm-cluster.log"
EOF
    echo -e "${GREEN}✓ Config created at $CONFIG_DIR/vmm-cluster.toml${NC}"
else
    echo -e "${YELLOW}Config already exists at $CONFIG_DIR/vmm-cluster.toml — skipping${NC}"
fi

# ── Install service ──────────────────────────────────────────────────────────
install_systemd() {
    cat > "/etc/systemd/system/${SERVICE_NAME}.service" << EOF
[Unit]
Description=VMM-Cluster — CoreVM Cluster Orchestration Server
After=network.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/vmm-cluster --config $CONFIG_DIR/vmm-cluster.toml
WorkingDirectory=$INSTALL_DIR
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Security hardening
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$DATA_DIR $LOG_DIR
PrivateTmp=true
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    echo -e "${GREEN}✓ systemd service created${NC}"
}

install_openrc() {
    cat > "/etc/init.d/$SERVICE_NAME" << 'INITEOF'
#!/sbin/openrc-run

name="vmm-cluster"
description="VMM-Cluster — CoreVM Cluster Orchestration Server"
command="INSTALL_DIR_PLACEHOLDER/vmm-cluster"
command_args="--config CONFIG_DIR_PLACEHOLDER/vmm-cluster.toml"
command_background=true
pidfile="/var/run/${RC_SVCNAME}.pid"
directory="INSTALL_DIR_PLACEHOLDER"
output_log="LOG_DIR_PLACEHOLDER/vmm-cluster.log"
error_log="LOG_DIR_PLACEHOLDER/vmm-cluster.log"

depend() {
    need net
    after firewall
}
INITEOF
    sed -i "s|INSTALL_DIR_PLACEHOLDER|$INSTALL_DIR|g" "/etc/init.d/$SERVICE_NAME"
    sed -i "s|CONFIG_DIR_PLACEHOLDER|$CONFIG_DIR|g" "/etc/init.d/$SERVICE_NAME"
    sed -i "s|LOG_DIR_PLACEHOLDER|$LOG_DIR|g" "/etc/init.d/$SERVICE_NAME"
    chmod 755 "/etc/init.d/$SERVICE_NAME"
    echo -e "${GREEN}✓ OpenRC service created${NC}"
}

install_sysvinit() {
    cat > "/etc/init.d/$SERVICE_NAME" << 'INITEOF'
#!/bin/sh
### BEGIN INIT INFO
# Provides:          vmm-cluster
# Required-Start:    $network $remote_fs
# Required-Stop:     $network $remote_fs
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: VMM-Cluster — CoreVM Cluster Orchestration Server
### END INIT INFO

DAEMON="INSTALL_DIR_PLACEHOLDER/vmm-cluster"
DAEMON_ARGS="--config CONFIG_DIR_PLACEHOLDER/vmm-cluster.toml"
PIDFILE="/var/run/vmm-cluster.pid"
LOGFILE="LOG_DIR_PLACEHOLDER/vmm-cluster.log"

case "$1" in
    start)
        echo "Starting vmm-cluster..."
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
            echo "vmm-cluster is already running."
            exit 0
        fi
        cd "INSTALL_DIR_PLACEHOLDER"
        nohup "$DAEMON" $DAEMON_ARGS >> "$LOGFILE" 2>&1 &
        echo $! > "$PIDFILE"
        echo "vmm-cluster started (PID $(cat "$PIDFILE"))."
        ;;
    stop)
        echo "Stopping vmm-cluster..."
        if [ -f "$PIDFILE" ]; then
            kill "$(cat "$PIDFILE")" 2>/dev/null || true
            rm -f "$PIDFILE"
            echo "vmm-cluster stopped."
        else
            echo "vmm-cluster is not running."
        fi
        ;;
    restart)
        "$0" stop
        sleep 1
        "$0" start
        ;;
    status)
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
            echo "vmm-cluster is running (PID $(cat "$PIDFILE"))."
        else
            echo "vmm-cluster is not running."
            exit 1
        fi
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|status}"
        exit 1
        ;;
esac
INITEOF
    sed -i "s|INSTALL_DIR_PLACEHOLDER|$INSTALL_DIR|g" "/etc/init.d/$SERVICE_NAME"
    sed -i "s|CONFIG_DIR_PLACEHOLDER|$CONFIG_DIR|g" "/etc/init.d/$SERVICE_NAME"
    sed -i "s|LOG_DIR_PLACEHOLDER|$LOG_DIR|g" "/etc/init.d/$SERVICE_NAME"
    chmod 755 "/etc/init.d/$SERVICE_NAME"
    if command -v update-rc.d &>/dev/null; then
        update-rc.d "$SERVICE_NAME" defaults
    elif command -v chkconfig &>/dev/null; then
        chkconfig --add "$SERVICE_NAME"
    fi
    echo -e "${GREEN}✓ SysVinit service created${NC}"
}

install_runit() {
    SV_DIR="/etc/sv/$SERVICE_NAME"
    mkdir -p "$SV_DIR/log"
    cat > "$SV_DIR/run" << EOF
#!/bin/sh
exec chpst -b vmm-cluster $INSTALL_DIR/vmm-cluster --config $CONFIG_DIR/vmm-cluster.toml 2>&1
EOF
    cat > "$SV_DIR/log/run" << EOF
#!/bin/sh
exec svlogd -tt $LOG_DIR/vmm-cluster/
EOF
    chmod 755 "$SV_DIR/run" "$SV_DIR/log/run"
    mkdir -p "$LOG_DIR/vmm-cluster"
    ln -sf "$SV_DIR" /var/service/ 2>/dev/null || \
    ln -sf "$SV_DIR" /service/ 2>/dev/null || true
    echo -e "${GREEN}✓ runit service created${NC}"
}

install_fallback_script() {
    cat > "$INSTALL_DIR/run.sh" << EOF
#!/bin/bash
# Manual start/stop script for vmm-cluster
# Usage: $INSTALL_DIR/run.sh {start|stop|restart|status}

PIDFILE="/var/run/vmm-cluster.pid"
LOGFILE="$LOG_DIR/vmm-cluster.log"

case "\$1" in
    start)
        echo "Starting vmm-cluster..."
        if [ -f "\$PIDFILE" ] && kill -0 "\$(cat "\$PIDFILE")" 2>/dev/null; then
            echo "vmm-cluster is already running."
            exit 0
        fi
        cd "$INSTALL_DIR"
        nohup "$INSTALL_DIR/vmm-cluster" --config "$CONFIG_DIR/vmm-cluster.toml" >> "\$LOGFILE" 2>&1 &
        echo \$! > "\$PIDFILE"
        echo "vmm-cluster started (PID \$(cat "\$PIDFILE"))."
        ;;
    stop)
        echo "Stopping vmm-cluster..."
        if [ -f "\$PIDFILE" ]; then
            kill "\$(cat "\$PIDFILE")" 2>/dev/null || true
            rm -f "\$PIDFILE"
            echo "vmm-cluster stopped."
        else
            echo "vmm-cluster is not running."
        fi
        ;;
    restart)
        "\$0" stop
        sleep 1
        "\$0" start
        ;;
    status)
        if [ -f "\$PIDFILE" ] && kill -0 "\$(cat "\$PIDFILE")" 2>/dev/null; then
            echo "vmm-cluster is running (PID \$(cat "\$PIDFILE"))."
        else
            echo "vmm-cluster is not running."
            exit 1
        fi
        ;;
    *)
        echo "Usage: \$0 {start|stop|restart|status}"
        exit 1
        ;;
esac
EOF
    chmod 755 "$INSTALL_DIR/run.sh"
    echo -e "${GREEN}✓ Fallback start/stop script created at $INSTALL_DIR/run.sh${NC}"
}

case "$INIT_SYSTEM" in
    systemd)  install_systemd ;;
    openrc)   install_openrc ;;
    sysvinit) install_sysvinit ;;
    runit)    install_runit ;;
    *)        install_fallback_script ;;
esac

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Installation complete!                          ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Binary:       $INSTALL_DIR/vmm-cluster"
echo -e "  UI:           $INSTALL_DIR/ui/"
echo -e "  Config:       $CONFIG_DIR/vmm-cluster.toml"
echo -e "  Data:         $DATA_DIR/"
echo -e "  Init system:  $INIT_SYSTEM"
echo ""

case "$INIT_SYSTEM" in
    systemd)
        echo -e "  ${CYAN}Start:${NC}    sudo systemctl enable --now $SERVICE_NAME"
        echo -e "  ${CYAN}Status:${NC}   sudo systemctl status $SERVICE_NAME"
        echo -e "  ${CYAN}Logs:${NC}     sudo journalctl -u $SERVICE_NAME -f"
        ;;
    openrc)
        echo -e "  ${CYAN}Start:${NC}    sudo rc-update add $SERVICE_NAME default && sudo rc-service $SERVICE_NAME start"
        echo -e "  ${CYAN}Status:${NC}   sudo rc-service $SERVICE_NAME status"
        echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-cluster.log"
        ;;
    sysvinit)
        echo -e "  ${CYAN}Start:${NC}    sudo /etc/init.d/$SERVICE_NAME start"
        echo -e "  ${CYAN}Status:${NC}   sudo /etc/init.d/$SERVICE_NAME status"
        echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-cluster.log"
        ;;
    runit)
        echo -e "  ${CYAN}Start:${NC}    sudo sv start $SERVICE_NAME"
        echo -e "  ${CYAN}Status:${NC}   sudo sv status $SERVICE_NAME"
        echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-cluster/"
        ;;
    *)
        echo -e "  ${YELLOW}No known init system detected.${NC}"
        echo -e "  ${CYAN}Start:${NC}    sudo $INSTALL_DIR/run.sh start"
        echo -e "  ${CYAN}Status:${NC}   sudo $INSTALL_DIR/run.sh status"
        echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-cluster.log"
        ;;
esac

echo ""
echo -e "  ${CYAN}Web UI:${NC}  http://$(hostname -I 2>/dev/null | awk '{print $1}' || echo 'localhost'):9443"
echo -e "  ${CYAN}Login:${NC}   admin / admin"
echo ""

exit 0
__ARCHIVE_BELOW__
