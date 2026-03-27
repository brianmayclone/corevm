#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# VMM-Server Self-Extracting Installer
# ─────────────────────────────────────────────────────────────────────────────
# This file is a self-extracting archive. Run it to install vmm-server.
#
# Supported platforms:
#   - Native Linux (systemd, OpenRC, SysVinit, runit)
#   - WSL2 (with or without systemd)
#
# Automatically detects the init system and WSL2 environment, then installs
# the appropriate service configuration.
#
# Usage:
#   sudo ./vmm-server-installer.sh                      # Install (interactive)
#   sudo ./vmm-server-installer.sh --enable-cli-access   # Install with CLI/API access enabled
#   sudo ./vmm-server-installer.sh --enable-tls          # Install with self-signed TLS
#   sudo ./vmm-server-installer.sh --uninstall           # Remove vmm-server
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
LOG_DIR="/var/log"

# ── WSL2 detection ───────────────────────────────────────────────────────────
IS_WSL=false
WSL_SYSTEMD=false
if grep -qi microsoft /proc/version 2>/dev/null; then
    IS_WSL=true
fi

# ── Init system detection ────────────────────────────────────────────────────
detect_init_system() {
    if [ -d /run/systemd/system ] && command -v systemctl &>/dev/null; then
        # Verify systemd is actually functional (not just present)
        if systemctl is-system-running &>/dev/null || systemctl is-system-running 2>&1 | grep -qE "running|degraded|starting|initializing"; then
            if [ "$IS_WSL" = true ]; then
                WSL_SYSTEMD=true
            fi
            echo "systemd"
            return
        fi
    fi
    if command -v rc-service &>/dev/null && [ -d /etc/init.d ]; then
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

# ── Parse CLI flags ─────────────────────────────────────────────────────────
ENABLE_CLI_ACCESS=false
ENABLE_TLS=false
for arg in "$@"; do
    case "$arg" in
        --enable-cli-access) ENABLE_CLI_ACCESS=true ;;
        --enable-tls)        ENABLE_TLS=true ;;
        --uninstall)         ;; # handled below
        *)                   ;;
    esac
done

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
    esac
    # Always clean up fallback script (might exist from WSL without systemd)
    rm -f "$INSTALL_DIR/run.sh"
}

# ── Uninstall mode ───────────────────────────────────────────────────────────
if [ "$1" = "--uninstall" ]; then
    echo -e "${CYAN}=== Uninstalling vmm-server ===${NC}"
    echo -e "  Platform:    ${CYAN}$([ "$IS_WSL" = true ] && echo "WSL2" || echo "Native Linux")${NC}"
    echo -e "  Init system: ${CYAN}${INIT_SYSTEM}${NC}"
    stop_service
    remove_service_files
    rm -rf "$INSTALL_DIR"
    echo -e "${GREEN}vmm-server uninstalled.${NC}"
    echo -e "${YELLOW}Config preserved at $CONFIG_DIR and data at $DATA_DIR${NC}"
    echo -e "To remove everything: rm -rf $CONFIG_DIR $DATA_DIR"
    exit 0
fi

# ── Banner ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║           VMM-Server Installer                   ║${NC}"
echo -e "${CYAN}║           CoreVM Web Management Server           ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""
if [ "$IS_WSL" = true ]; then
    echo -e "  Platform:    ${CYAN}WSL2${NC}"
    if [ "$WSL_SYSTEMD" = true ]; then
        echo -e "  Init system: ${CYAN}systemd (WSL2 systemd mode)${NC}"
    else
        echo -e "  Init system: ${CYAN}${INIT_SYSTEM}${NC}"
        echo -e "  ${YELLOW}systemd not active — will install manual start script${NC}"
    fi
else
    echo -e "  Platform:    ${CYAN}Native Linux${NC}"
    echo -e "  Init system: ${CYAN}${INIT_SYSTEM}${NC}"
fi
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
echo -e "${CYAN}Installing vmm-server to $INSTALL_DIR...${NC}"
mkdir -p "$INSTALL_DIR"
cp "$TMPDIR/vmm-server" "$INSTALL_DIR/vmm-server"
chmod 755 "$INSTALL_DIR/vmm-server"

# Install vmmctl CLI tool (if included in payload)
if [ -f "$TMPDIR/vmmctl" ]; then
    cp "$TMPDIR/vmmctl" "$INSTALL_DIR/vmmctl"
    chmod 755 "$INSTALL_DIR/vmmctl"
    # Symlink to /usr/local/bin for system-wide access
    ln -sf "$INSTALL_DIR/vmmctl" /usr/local/bin/vmmctl
    echo -e "${GREEN}✓ vmmctl CLI tool installed${NC}"
fi

# ── Install BIOS assets ─────────────────────────────────────────────────────
if [ -d "$TMPDIR/assets" ]; then
    mkdir -p "$INSTALL_DIR/assets"
    cp -r "$TMPDIR/assets/"* "$INSTALL_DIR/assets/"
    echo -e "${GREEN}✓ BIOS assets installed${NC}"
fi

# ── Install UI ───────────────────────────────────────────────────────────────
if [ -d "$TMPDIR/ui" ]; then
    rm -rf "$INSTALL_DIR/ui"
    cp -r "$TMPDIR/ui" "$INSTALL_DIR/ui"
    echo -e "${GREEN}✓ Web UI installed${NC}"
fi

# ── Create data directories ─────────────────────────────────────────────────
mkdir -p "$DATA_DIR/vms"
mkdir -p "$DATA_DIR/images"
mkdir -p "$DATA_DIR/isos"
mkdir -p "$LOG_DIR"

# ── Create config ────────────────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"

# ── Generate self-signed TLS certificate if requested ───────────────────────
TLS_CERT_PATH=""
TLS_KEY_PATH=""
if [ "$ENABLE_TLS" = true ]; then
    TLS_CERT_PATH="$CONFIG_DIR/server.crt"
    TLS_KEY_PATH="$CONFIG_DIR/server.key"
    if [ ! -f "$TLS_CERT_PATH" ]; then
        HOSTNAME=$(hostname -f 2>/dev/null || hostname)
        HOST_IP=$(hostname -I 2>/dev/null | awk '{print $1}')
        openssl req -x509 -newkey rsa:4096 -keyout "$TLS_KEY_PATH" -out "$TLS_CERT_PATH" \
            -days 3650 -nodes -subj "/CN=$HOSTNAME" \
            -addext "subjectAltName=DNS:$HOSTNAME,DNS:localhost,IP:127.0.0.1${HOST_IP:+,IP:$HOST_IP}" \
            2>/dev/null
        chmod 600 "$TLS_KEY_PATH"
        echo -e "${GREEN}✓ Self-signed TLS certificate generated${NC}"
    else
        echo -e "${YELLOW}TLS certificate already exists — skipping${NC}"
    fi
fi

# ── Interactive CLI access prompt (if not specified via flag) ───────────────
if [ "$ENABLE_CLI_ACCESS" = false ] && [ -t 0 ] && [ "$1" != "--uninstall" ]; then
    echo -n -e "  ${CYAN}Enable CLI/API access? [Y/n]:${NC} "
    read -r CLI_ANSWER
    if [ -z "$CLI_ANSWER" ] || echo "$CLI_ANSWER" | grep -qi '^y'; then
        ENABLE_CLI_ACCESS=true
    fi
fi

if [ ! -f "$CONFIG_DIR/vmm-server.toml" ]; then
    JWT_SECRET=$(head -c 32 /dev/urandom | base64 | tr -d '=/+' | head -c 32)
    cat > "$CONFIG_DIR/vmm-server.toml" << EOF
[server]
bind = "0.0.0.0"
port = 8443
$([ -n "$TLS_CERT_PATH" ] && echo "tls_cert = \"$TLS_CERT_PATH\"")
$([ -n "$TLS_KEY_PATH" ] && echo "tls_key = \"$TLS_KEY_PATH\"")

[auth]
jwt_secret = "$JWT_SECRET"
session_timeout_hours = 24

[storage]
default_pool = "$DATA_DIR/images"
iso_pool = "$DATA_DIR/isos"

[vms]
config_dir = "$DATA_DIR/vms"

[api]
cli_access_enabled = $ENABLE_CLI_ACCESS

[logging]
level = "info"
file = "$LOG_DIR/vmm-server.log"
EOF
    echo -e "${GREEN}✓ Config created at $CONFIG_DIR/vmm-server.toml${NC}"
else
    echo -e "${YELLOW}Config already exists at $CONFIG_DIR/vmm-server.toml — skipping${NC}"
fi

# ── Install service ──────────────────────────────────────────────────────────
install_systemd() {
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

name="vmm-server"
description="VMM-Server — CoreVM Web Management Server"
command="INSTALL_DIR_PLACEHOLDER/vmm-server"
command_args="--config CONFIG_DIR_PLACEHOLDER/vmm-server.toml"
command_background=true
pidfile="/var/run/${RC_SVCNAME}.pid"
directory="INSTALL_DIR_PLACEHOLDER"
output_log="LOG_DIR_PLACEHOLDER/vmm-server.log"
error_log="LOG_DIR_PLACEHOLDER/vmm-server.log"

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
# Provides:          vmm-server
# Required-Start:    $network $remote_fs
# Required-Stop:     $network $remote_fs
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: VMM-Server — CoreVM Web Management Server
### END INIT INFO

DAEMON="INSTALL_DIR_PLACEHOLDER/vmm-server"
DAEMON_ARGS="--config CONFIG_DIR_PLACEHOLDER/vmm-server.toml"
PIDFILE="/var/run/vmm-server.pid"
LOGFILE="LOG_DIR_PLACEHOLDER/vmm-server.log"

case "$1" in
    start)
        echo "Starting vmm-server..."
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
            echo "vmm-server is already running."
            exit 0
        fi
        cd "INSTALL_DIR_PLACEHOLDER"
        nohup "$DAEMON" $DAEMON_ARGS >> "$LOGFILE" 2>&1 &
        echo $! > "$PIDFILE"
        echo "vmm-server started (PID $(cat "$PIDFILE"))."
        ;;
    stop)
        echo "Stopping vmm-server..."
        if [ -f "$PIDFILE" ]; then
            kill "$(cat "$PIDFILE")" 2>/dev/null || true
            rm -f "$PIDFILE"
            echo "vmm-server stopped."
        else
            echo "vmm-server is not running."
        fi
        ;;
    restart)
        "$0" stop
        sleep 1
        "$0" start
        ;;
    status)
        if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
            echo "vmm-server is running (PID $(cat "$PIDFILE"))."
        else
            echo "vmm-server is not running."
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
exec chpst -b vmm-server $INSTALL_DIR/vmm-server --config $CONFIG_DIR/vmm-server.toml 2>&1
EOF
    cat > "$SV_DIR/log/run" << EOF
#!/bin/sh
exec svlogd -tt $LOG_DIR/vmm-server/
EOF
    chmod 755 "$SV_DIR/run" "$SV_DIR/log/run"
    mkdir -p "$LOG_DIR/vmm-server"
    ln -sf "$SV_DIR" /var/service/ 2>/dev/null || \
    ln -sf "$SV_DIR" /service/ 2>/dev/null || true
    echo -e "${GREEN}✓ runit service created${NC}"
}

install_fallback_script() {
    cat > "$INSTALL_DIR/run.sh" << EOF
#!/bin/bash
# Start/stop script for vmm-server
# Usage: $INSTALL_DIR/run.sh {start|stop|restart|status}

PIDFILE="/var/run/vmm-server.pid"
LOGFILE="$LOG_DIR/vmm-server.log"

case "\$1" in
    start)
        echo "Starting vmm-server..."
        if [ -f "\$PIDFILE" ] && kill -0 "\$(cat "\$PIDFILE")" 2>/dev/null; then
            echo "vmm-server is already running."
            exit 0
        fi
        cd "$INSTALL_DIR"
        nohup "$INSTALL_DIR/vmm-server" --config "$CONFIG_DIR/vmm-server.toml" >> "\$LOGFILE" 2>&1 &
        echo \$! > "\$PIDFILE"
        echo "vmm-server started (PID \$(cat "\$PIDFILE"))."
        ;;
    stop)
        echo "Stopping vmm-server..."
        if [ -f "\$PIDFILE" ]; then
            kill "\$(cat "\$PIDFILE")" 2>/dev/null || true
            rm -f "\$PIDFILE"
            echo "vmm-server stopped."
        else
            echo "vmm-server is not running."
        fi
        ;;
    restart)
        "\$0" stop
        sleep 1
        "\$0" start
        ;;
    status)
        if [ -f "\$PIDFILE" ] && kill -0 "\$(cat "\$PIDFILE")" 2>/dev/null; then
            echo "vmm-server is running (PID \$(cat "\$PIDFILE"))."
        else
            echo "vmm-server is not running."
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
    echo -e "${GREEN}✓ Start/stop script created at $INSTALL_DIR/run.sh${NC}"
}

# On WSL2 without systemd, always use fallback script
if [ "$IS_WSL" = true ] && [ "$INIT_SYSTEM" != "systemd" ]; then
    install_fallback_script
else
    case "$INIT_SYSTEM" in
        systemd)  install_systemd ;;
        openrc)   install_openrc ;;
        sysvinit) install_sysvinit ;;
        runit)    install_runit ;;
        *)        install_fallback_script ;;
    esac
fi

# On WSL2 with systemd, also install the fallback script as convenience
if [ "$IS_WSL" = true ] && [ "$INIT_SYSTEM" = "systemd" ]; then
    install_fallback_script
fi

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Installation complete!                          ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Binary:       $INSTALL_DIR/vmm-server"
echo -e "  Config:       $CONFIG_DIR/vmm-server.toml"
echo -e "  Data:         $DATA_DIR/"
echo -e "  Init system:  $INIT_SYSTEM"
echo ""

# ── WSL2-specific hints ─────────────────────────────────────────────────────
if [ "$IS_WSL" = true ]; then
    WSL_IP=$(hostname -I 2>/dev/null | awk '{print $1}')
    # Try to get the Windows host IP (gateway from WSL's perspective)
    WIN_HOST_IP=$(ip route show default 2>/dev/null | awk '{print $3}' || echo "")

    if [ "$WSL_SYSTEMD" = true ]; then
        echo -e "  ${CYAN}Start (systemd):${NC}"
        echo -e "    sudo systemctl enable --now $SERVICE_NAME"
        echo -e ""
        echo -e "  ${CYAN}Alternative (manual):${NC}"
        echo -e "    sudo $INSTALL_DIR/run.sh start"
    else
        echo -e "  ${CYAN}Start:${NC}    sudo $INSTALL_DIR/run.sh start"
        echo -e "  ${CYAN}Status:${NC}   sudo $INSTALL_DIR/run.sh status"
        echo -e "  ${CYAN}Stop:${NC}     sudo $INSTALL_DIR/run.sh stop"
    fi

    echo ""
    echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-server.log"
    echo ""
    echo -e "  ${YELLOW}── WSL2 Notes ──────────────────────────────────────${NC}"
    echo ""
    echo -e "  The service listens on the WSL2 virtual network."
    echo -e "  From inside WSL2:    ${CYAN}http://localhost:8443${NC}"
    if [ -n "$WSL_IP" ]; then
        echo -e "  WSL2 IP:             ${CYAN}http://${WSL_IP}:8443${NC}"
    fi
    echo ""
    echo -e "  ${CYAN}Access from Windows:${NC}"
    echo -e "    Modern WSL2 (mirrored networking): http://localhost:8443"
    echo -e "    Classic WSL2 (NAT networking):     http://${WSL_IP:-<WSL_IP>}:8443"
    echo ""
    echo -e "  To enable mirrored networking (recommended), add to"
    echo -e "  ${CYAN}%USERPROFILE%\\.wslconfig${NC} on Windows:"
    echo -e ""
    echo -e "    [wsl2]"
    echo -e "    networkingMode=mirrored"
    echo ""
    echo -e "  Then restart WSL: ${CYAN}wsl --shutdown${NC}"
    echo ""
    if [ "$WSL_SYSTEMD" != true ]; then
        echo -e "  ${CYAN}Autostart on WSL launch:${NC}"
        echo -e "    Add to /etc/wsl.conf:"
        echo -e ""
        echo -e "      [boot]"
        echo -e "      command = $INSTALL_DIR/run.sh start"
        echo ""
    fi
else
    # Native Linux hints
    case "$INIT_SYSTEM" in
        systemd)
            echo -e "  ${CYAN}Start:${NC}    sudo systemctl enable --now $SERVICE_NAME"
            echo -e "  ${CYAN}Status:${NC}   sudo systemctl status $SERVICE_NAME"
            echo -e "  ${CYAN}Logs:${NC}     sudo journalctl -u $SERVICE_NAME -f"
            ;;
        openrc)
            echo -e "  ${CYAN}Start:${NC}    sudo rc-update add $SERVICE_NAME default && sudo rc-service $SERVICE_NAME start"
            echo -e "  ${CYAN}Status:${NC}   sudo rc-service $SERVICE_NAME status"
            echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-server.log"
            ;;
        sysvinit)
            echo -e "  ${CYAN}Start:${NC}    sudo /etc/init.d/$SERVICE_NAME start"
            echo -e "  ${CYAN}Status:${NC}   sudo /etc/init.d/$SERVICE_NAME status"
            echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-server.log"
            ;;
        runit)
            echo -e "  ${CYAN}Start:${NC}    sudo sv start $SERVICE_NAME"
            echo -e "  ${CYAN}Status:${NC}   sudo sv status $SERVICE_NAME"
            echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-server/"
            ;;
        *)
            echo -e "  ${YELLOW}No known init system detected.${NC}"
            echo -e "  ${CYAN}Start:${NC}    sudo $INSTALL_DIR/run.sh start"
            echo -e "  ${CYAN}Status:${NC}   sudo $INSTALL_DIR/run.sh status"
            echo -e "  ${CYAN}Logs:${NC}     tail -f $LOG_DIR/vmm-server.log"
            ;;
    esac
fi

echo ""
HOST_IP=$(hostname -I 2>/dev/null | awk '{print $1}' || echo 'localhost')
PROTO="http"
[ -n "$TLS_CERT_PATH" ] && PROTO="https"
echo -e "  ${CYAN}Web UI:${NC}  ${PROTO}://${HOST_IP}:8443"
echo -e "  ${CYAN}Login:${NC}   admin / admin"
if [ "$ENABLE_CLI_ACCESS" = true ]; then
    echo ""
    echo -e "  ${CYAN}── CLI Access ──────────────────────────────────────${NC}"
    echo -e "  CLI/API access is ${GREEN}enabled${NC}."
    echo -e "  Connect from a remote machine:"
    echo ""
    echo -e "    vmmctl config set-server ${PROTO}://${HOST_IP}:8443$([ "$ENABLE_TLS" = true ] && echo " --insecure")"
    echo -e "    vmmctl login"
fi
echo ""

exit 0
__ARCHIVE_BELOW__
