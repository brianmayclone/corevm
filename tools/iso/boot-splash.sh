#!/bin/bash
# CoreVM Boot Splash — ESXi-style text boot screen
# Shows logo, progress bar, and scrolling kernel messages
# Exits when the target service (installer or DCUI) starts

set -u

# Which service to wait for (passed as argument or default)
WAIT_FOR="${1:-vmm-dcui.service}"

# Terminal setup
export TERM=linux
COLS=$(tput cols 2>/dev/null || echo 80)
ROWS=$(tput lines 2>/dev/null || echo 25)

# Hide cursor, clear screen
printf '\033[?25l'
printf '\033[2J'

# Colors
C_RESET='\033[0m'
C_CYAN='\033[1;36m'
C_WHITE='\033[1;37m'
C_GRAY='\033[0;37m'
C_DARK='\033[0;90m'
C_BAR='\033[46m'     # cyan background
C_BARBG='\033[100m'  # dark gray background

# ── Draw static elements ─────────────────────────────────────────────

draw_logo() {
    local start_row=$(( (ROWS / 2) - 8 ))
    [ "$start_row" -lt 2 ] && start_row=2

    local logo=(
        "    ____               __     __ __  __"
        "   / ___|___  _ __ ___ \\ \\   / /|  \\/  |"
        "  | |   / _ \\| '__/ _ \\ \\ \\ / / | |\\/| |"
        "  | |__| (_) | | |  __/  \\ V /  | |  | |"
        "   \\____\\___/|_|  \\___|   \\_/   |_|  |_|"
    )

    local row=$start_row
    for line in "${logo[@]}"; do
        local pad=$(( (COLS - ${#line}) / 2 ))
        [ "$pad" -lt 0 ] && pad=0
        printf '\033[%d;%dH%b%s%b' "$row" "$pad" "$C_CYAN" "$line" "$C_RESET"
        row=$((row + 1))
    done

    # Version line
    row=$((row + 1))
    local ver="CoreVM Appliance 1.0"
    local vpad=$(( (COLS - ${#ver}) / 2 ))
    printf '\033[%d;%dH%b%s%b' "$row" "$vpad" "$C_WHITE" "$ver" "$C_RESET"

    # Return the row after logo for progress bar placement
    echo $((row + 2))
}

# Capture progress bar row
BAR_ROW=$(draw_logo)
BAR_WIDTH=50
BAR_START=$(( (COLS - BAR_WIDTH) / 2 ))
MSG_ROW=$((ROWS - 1))
STATUS_ROW=$((BAR_ROW + 2))

# ── Progress bar ─────────────────────────────────────────────────────

draw_progress() {
    local pct=$1
    local filled=$(( (pct * BAR_WIDTH) / 100 ))
    local empty=$(( BAR_WIDTH - filled ))

    printf '\033[%d;%dH' "$BAR_ROW" "$BAR_START"

    # Filled portion
    if [ "$filled" -gt 0 ]; then
        printf '%b' "$C_BAR"
        printf '%*s' "$filled" ''
        printf '%b' "$C_RESET"
    fi

    # Empty portion
    if [ "$empty" -gt 0 ]; then
        printf '%b' "$C_BARBG"
        printf '%*s' "$empty" ''
        printf '%b' "$C_RESET"
    fi

    # Percentage
    local pct_str="${pct}%"
    local pct_pad=$(( (COLS - ${#pct_str}) / 2 ))
    printf '\033[%d;%dH%b%s%b' "$((BAR_ROW + 1))" "$pct_pad" "$C_GRAY" "$pct_str" "$C_RESET"
}

draw_status() {
    local msg="$1"
    # Truncate to terminal width - 4
    local maxlen=$((COLS - 4))
    if [ ${#msg} -gt $maxlen ]; then
        msg="${msg:0:$maxlen}"
    fi
    # Clear line and print
    printf '\033[%d;1H\033[2K' "$STATUS_ROW"
    local spad=$(( (COLS - ${#msg}) / 2 ))
    [ "$spad" -lt 0 ] && spad=0
    printf '\033[%d;%dH%b%s%b' "$STATUS_ROW" "$spad" "$C_DARK" "$msg" "$C_RESET"
}

draw_kernel_line() {
    local msg="$1"
    # Truncate
    local maxlen=$((COLS - 2))
    if [ ${#msg} -gt $maxlen ]; then
        msg="${msg:0:$maxlen}"
    fi
    # Clear bottom line and print
    printf '\033[%d;1H\033[2K' "$MSG_ROW"
    printf '\033[%d;1H%b %s%b' "$MSG_ROW" "$C_DARK" "$msg" "$C_RESET"
}

# ── Main loop ────────────────────────────────────────────────────────

# Start reading kernel messages in background
DMESG_FIFO=$(mktemp -u /tmp/dmesg.XXXXXX)
mkfifo "$DMESG_FIFO"
dmesg --follow --notime 2>/dev/null > "$DMESG_FIFO" &
DMESG_PID=$!

cleanup() {
    kill "$DMESG_PID" 2>/dev/null
    rm -f "$DMESG_FIFO"
    printf '\033[?25h'  # show cursor
    printf '\033[2J'    # clear screen
}
trap cleanup EXIT

# Simulate progress based on systemd boot targets
progress=0
draw_progress 0
draw_status "Booting..."

while true; do
    # Check if target service is active
    if systemctl is-active "$WAIT_FOR" >/dev/null 2>&1; then
        draw_progress 100
        draw_status "Starting CoreVM..."
        sleep 1
        break
    fi

    # Update progress based on boot stage
    if systemctl is-active basic.target >/dev/null 2>&1 && [ "$progress" -lt 40 ]; then
        progress=40
        draw_status "Basic system initialized..."
    fi
    if systemctl is-active network.target >/dev/null 2>&1 && [ "$progress" -lt 60 ]; then
        progress=60
        draw_status "Network ready..."
    fi
    if systemctl is-active multi-user.target >/dev/null 2>&1 && [ "$progress" -lt 80 ]; then
        progress=80
        draw_status "Starting services..."
    fi

    # Slow increment to show activity
    if [ "$progress" -lt 90 ]; then
        progress=$((progress + 1))
    fi
    draw_progress "$progress"

    # Read kernel messages (non-blocking via timeout)
    if read -t 0.3 line < "$DMESG_FIFO" 2>/dev/null; then
        draw_kernel_line "$line"
    fi
done
