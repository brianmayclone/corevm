#!/bin/bash
# CoreVM Boot Splash — ESXi-style text boot screen
# Shows logo, progress bar, and scrolling kernel messages
# Exits when the target service (installer or DCUI) starts

set -u

WAIT_FOR="${1:-vmm-dcui.service}"

# Get terminal size (fallback to 80x25)
COLS=80
ROWS=25
if command -v tput >/dev/null 2>&1; then
    COLS=$(tput cols 2>/dev/null || echo 80)
    ROWS=$(tput lines 2>/dev/null || echo 25)
fi

# Hide cursor, clear screen
printf '\033[?25l'
printf '\033[2J'

# ── Logo ─────────────────────────────────────────────────────────────

draw_logo() {
    local logo_lines=(
        '    ____               __     __ __  __'
        '   / ___|___  _ __ ___ \ \   / /|  \/  |'
        '  | |   / _ \| '"'"'__/ _ \ \ \ / / | |\/| |'
        '  | |__| (_) | | |  __/  \ V /  | |  | |'
        '   \____\___/|_|  \___|   \_/   |_|  |_|'
    )

    local start_row=$(( (ROWS / 2) - 7 ))
    [ "$start_row" -lt 2 ] && start_row=2

    local row=$start_row
    for line in "${logo_lines[@]}"; do
        local len=${#line}
        local pad=$(( (COLS - len) / 2 ))
        [ "$pad" -lt 1 ] && pad=1
        printf '\033[%d;%dH\033[1;36m%s\033[0m' "$row" "$pad" "$line"
        row=$((row + 1))
    done

    # Version
    row=$((row + 1))
    local ver="CoreVM Appliance 1.0"
    local vlen=${#ver}
    local vpad=$(( (COLS - vlen) / 2 ))
    printf '\033[%d;%dH\033[1;37m%s\033[0m' "$row" "$vpad" "$ver"

    # Return next available row
    printf '%d' $((row + 3))
}

BAR_ROW=$(draw_logo)
BAR_WIDTH=50
BAR_START=$(( (COLS - BAR_WIDTH) / 2 ))
STATUS_ROW=$((BAR_ROW + 2))
MSG_ROW=$((ROWS - 1))

# ── Drawing functions ────────────────────────────────────────────────

draw_bar() {
    local pct=$1
    local filled=$(( (pct * BAR_WIDTH) / 100 ))
    local empty=$(( BAR_WIDTH - filled ))

    printf '\033[%d;%dH' "$BAR_ROW" "$BAR_START"

    # Filled (cyan bg)
    [ "$filled" -gt 0 ] && printf '\033[46m%*s\033[0m' "$filled" ''
    # Empty (dark gray bg)
    [ "$empty" -gt 0 ] && printf '\033[100m%*s\033[0m' "$empty" ''

    # Percentage centered below bar
    local label="${pct}%"
    local lpad=$(( (COLS - ${#label}) / 2 ))
    printf '\033[%d;%dH\033[0;37m%s\033[0m' "$((BAR_ROW + 1))" "$lpad" "$label"
}

draw_status() {
    local msg="$1"
    local maxlen=$((COLS - 4))
    [ ${#msg} -gt $maxlen ] && msg="${msg:0:$maxlen}"
    printf '\033[%d;1H\033[2K' "$STATUS_ROW"
    local spad=$(( (COLS - ${#msg}) / 2 ))
    [ "$spad" -lt 1 ] && spad=1
    printf '\033[%d;%dH\033[0;90m%s\033[0m' "$STATUS_ROW" "$spad" "$msg"
}

draw_kernel() {
    local msg="$1"
    local maxlen=$((COLS - 2))
    [ ${#msg} -gt $maxlen ] && msg="${msg:0:$maxlen}"
    printf '\033[%d;1H\033[2K' "$MSG_ROW"
    printf '\033[%d;1H\033[0;90m %s\033[0m' "$MSG_ROW" "$msg"
}

# ── Kernel message reader ────────────────────────────────────────────

DMESG_PID=""
DMESG_FIFO=""

start_dmesg() {
    if command -v dmesg >/dev/null 2>&1; then
        DMESG_FIFO=$(mktemp -u /tmp/corevm-dmesg.XXXXXX)
        mkfifo "$DMESG_FIFO" 2>/dev/null || return
        dmesg -w --notime 2>/dev/null > "$DMESG_FIFO" &
        DMESG_PID=$!
    fi
}

cleanup() {
    [ -n "$DMESG_PID" ] && kill "$DMESG_PID" 2>/dev/null
    [ -n "$DMESG_FIFO" ] && rm -f "$DMESG_FIFO"
    printf '\033[?25h'
    printf '\033[2J'
    printf '\033[H'
}
trap cleanup EXIT

start_dmesg

# ── Main loop ────────────────────────────────────────────────────────

progress=0
draw_bar 0
draw_status "Booting..."

while true; do
    # Check if target service is running
    if systemctl is-active "$WAIT_FOR" >/dev/null 2>&1; then
        draw_bar 100
        draw_status "Starting CoreVM..."
        sleep 1
        break
    fi

    # Progress based on systemd boot targets
    if systemctl is-active multi-user.target >/dev/null 2>&1; then
        [ "$progress" -lt 85 ] && progress=85 && draw_status "Starting services..."
    elif systemctl is-active network.target >/dev/null 2>&1; then
        [ "$progress" -lt 60 ] && progress=60 && draw_status "Network ready..."
    elif systemctl is-active basic.target >/dev/null 2>&1; then
        [ "$progress" -lt 35 ] && progress=35 && draw_status "System initializing..."
    elif systemctl is-active local-fs.target >/dev/null 2>&1; then
        [ "$progress" -lt 15 ] && progress=15 && draw_status "Filesystems mounted..."
    fi

    # Slow tick
    [ "$progress" -lt 90 ] && progress=$((progress + 1))
    draw_bar "$progress"

    # Read one kernel line (non-blocking)
    if [ -n "$DMESG_FIFO" ] && [ -p "$DMESG_FIFO" ]; then
        if read -t 0.2 kline < "$DMESG_FIFO" 2>/dev/null; then
            draw_kernel "$kline"
        else
            sleep 0.3
        fi
    else
        sleep 0.5
    fi
done
