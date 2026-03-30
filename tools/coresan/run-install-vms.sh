#!/bin/bash
# CoreSAN Install-VM Testbed
#
# Spins up N QEMU/KVM VMs booting the CoreVM appliance ISO for installation
# testing. Each VM gets:
#   - 1× 20 GB system disk
#   - 2× 40 GB data disks
#   - 6 vCPUs, 8 GB RAM, KVM acceleration
#   - Boot order: HDD first, then CD-ROM
#   - Shared L2 network (bridge or user-mode NAT with full connectivity)
#
# Usage:
#   ./tools/coresan/run-install-vms.sh                     # 1 VM (next free slot)
#   ./tools/coresan/run-install-vms.sh --count 3            # 3 VMs
#   ./tools/coresan/run-install-vms.sh --reset              # Recreate all disks
#   ./tools/coresan/run-install-vms.sh --count 2 --reset    # 2 VMs, fresh disks
#   ./tools/coresan/run-install-vms.sh --bridge br0         # Use existing bridge
#   ./tools/coresan/run-install-vms.sh --cleanup            # Remove all VM data
#
# Parallel: Run multiple times to add VMs at runtime. Each instance
# auto-detects the next free slot (tap device + IP address).

set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT="$(pwd)"

# ── Colors / helpers ─────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[install-vm]${NC} $*"; }
ok()    { echo -e "${GREEN}[install-vm]${NC} $*"; }
warn()  { echo -e "${YELLOW}[install-vm]${NC} $*"; }
fail()  { echo -e "${RED}[install-vm]${NC} $*"; exit 1; }

# ── Defaults ─────────────────────────────────────────────────────

VM_COUNT=1
RESET_DISKS=false
CLEANUP=false
BRIDGE=""
VM_DIR="$ROOT/.vms/coresan"
ISO=""
CPUS=6
RAM_MB=8192
DISK_SYS_SIZE="20G"
DISK_DATA_SIZE="40G"
SUBNET="192.168.100"        # Subnet for auto-created bridge
QEMU_BIN="qemu-system-x86_64"

# ── Parse args ───────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case $1 in
        --count|-c)
            VM_COUNT="$2"; shift 2 ;;
        --reset|-r)
            RESET_DISKS=true; shift ;;
        --cleanup)
            CLEANUP=true; shift ;;
        --bridge|-b)
            BRIDGE="$2"; shift 2 ;;
        --iso)
            ISO="$2"; shift 2 ;;
        --dir)
            VM_DIR="$2"; shift 2 ;;
        --help|-h)
            echo "CoreSAN Install-VM Testbed"
            echo ""
            echo "Usage:"
            echo "  $(basename "$0") [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -c, --count N       Number of VMs to start (default: 3)"
            echo "  -r, --reset         Delete and recreate all disk images"
            echo "  -b, --bridge NAME   Use an existing Linux bridge (e.g. br0)"
            echo "                      If omitted, a temporary bridge is created (requires root)"
            echo "      --iso PATH      Path to CoreVM ISO (default: auto-detect from dist/)"
            echo "      --dir PATH      VM data directory (default: .vms/coresan)"
            echo "      --cleanup       Remove all VM data and exit"
            echo "  -h, --help          Show this help"
            echo ""
            echo "Each VM gets: ${CPUS} vCPUs, $((RAM_MB / 1024)) GB RAM, 1×${DISK_SYS_SIZE} sys + 2×${DISK_DATA_SIZE} data"
            echo "Boot order: HDD → CD-ROM"
            echo "Network: bridged L2 — all guests + host can reach each other"
            exit 0 ;;
        *)
            fail "Unknown option: $1 (try --help)" ;;
    esac
done

# ── Pre-flight checks ───────────────────────────────────────────

command -v "$QEMU_BIN" >/dev/null 2>&1 || fail "qemu-system-x86_64 not found. Install qemu-system-x86."
command -v dnsmasq >/dev/null 2>&1 || fail "dnsmasq not found. Install dnsmasq (apt install dnsmasq / dnf install dnsmasq)."

if [[ ! -e /dev/kvm ]]; then
    fail "/dev/kvm not available. Ensure KVM is loaded (modprobe kvm_intel or kvm_amd)."
fi

# ── Cleanup mode ─────────────────────────────────────────────────

if $CLEANUP; then
    if [[ -d "$VM_DIR" ]]; then
        info "Removing VM data directory: $VM_DIR"
        rm -rf "$VM_DIR"
        ok "Cleaned up."
    else
        info "Nothing to clean up (directory does not exist)."
    fi
    exit 0
fi

# ── Locate ISO ───────────────────────────────────────────────────

if [[ -z "$ISO" ]]; then
    ISO=$(ls -t "$ROOT"/dist/corevm-appliance-*.iso 2>/dev/null | head -1)
    if [[ -z "$ISO" ]]; then
        fail "No CoreVM ISO found in dist/. Build one first or specify --iso PATH."
    fi
fi

[[ -f "$ISO" ]] || fail "ISO not found: $ISO"
info "Using ISO: $ISO"

# ── Prepare VM directory ────────────────────────────────────────

mkdir -p "$VM_DIR"

# ── Create / reset disk images ──────────────────────────────────

create_disks() {
    local idx=$1
    local vm_path="$VM_DIR/vm${idx}"
    mkdir -p "$vm_path"

    local sys_disk="$vm_path/system.qcow2"
    local data1_disk="$vm_path/data1.qcow2"
    local data2_disk="$vm_path/data2.qcow2"

    if $RESET_DISKS || [[ ! -f "$sys_disk" ]]; then
        info "VM ${idx}: creating system disk (${DISK_SYS_SIZE})"
        qemu-img create -f qcow2 "$sys_disk" "$DISK_SYS_SIZE" >/dev/null
    fi
    if $RESET_DISKS || [[ ! -f "$data1_disk" ]]; then
        info "VM ${idx}: creating data disk 1 (${DISK_DATA_SIZE})"
        qemu-img create -f qcow2 "$data1_disk" "$DISK_DATA_SIZE" >/dev/null
    fi
    if $RESET_DISKS || [[ ! -f "$data2_disk" ]]; then
        info "VM ${idx}: creating data disk 2 (${DISK_DATA_SIZE})"
        qemu-img create -f qcow2 "$data2_disk" "$DISK_DATA_SIZE" >/dev/null
    fi
}

# ── Find next free VM slots ──────────────────────────────────────
#
# Look at existing tap devices and VM dirs to find which slots are taken.
# This allows parallel invocations to add VMs without conflicts.

find_free_slots() {
    local needed=$1
    local slots=()
    for candidate in $(seq 1 99); do
        # Check if tap device or QEMU process already exists for this slot
        if ip link show "coresan-tap${candidate}" &>/dev/null 2>&1; then
            continue
        fi
        if pgrep -f "coresan-vm${candidate}" &>/dev/null 2>&1; then
            continue
        fi
        slots+=("$candidate")
        if [[ ${#slots[@]} -ge $needed ]]; then
            break
        fi
    done
    if [[ ${#slots[@]} -lt $needed ]]; then
        fail "Cannot find $needed free VM slots (checked 1-99)"
    fi
    echo "${slots[@]}"
}

VM_SLOTS=($(find_free_slots "$VM_COUNT"))
info "Using VM slots: ${VM_SLOTS[*]}"

for i in "${VM_SLOTS[@]}"; do
    create_disks "$i"
done
ok "Disk images ready."

# ── Network setup ────────────────────────────────────────────────
#
# Strategy:
#   1. If --bridge is given, use that existing bridge (tap devices, needs root).
#   2. Otherwise, create a temporary bridge "coresan-br0" with tap devices.
#
# In both cases each VM gets a tap interface on the bridge, providing
# full L2 connectivity between all guests and the host.

CREATED_BRIDGE=false
BRIDGE_NAME="${BRIDGE:-coresan-br0}"
TAP_DEVICES=()
QEMU_PIDS=()
DNSMASQ_PID=""
DNSMASQ_PIDFILE="$VM_DIR/dnsmasq.pid"
DNSMASQ_LEASEFILE="$VM_DIR/dnsmasq.leases"

cleanup_on_exit() {
    info "Shutting down VMs..."
    for pid in "${QEMU_PIDS[@]}"; do
        kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null || true
    done

    # Stop dnsmasq only if we started it (not if reused from another instance)
    if ! ${DNSMASQ_REUSED:-false}; then
        if [[ -n "$DNSMASQ_PID" ]] && kill -0 "$DNSMASQ_PID" 2>/dev/null; then
            kill "$DNSMASQ_PID" 2>/dev/null || true
            info "Stopped dnsmasq (PID $DNSMASQ_PID)"
        elif [[ -f "$DNSMASQ_PIDFILE" ]]; then
            kill "$(cat "$DNSMASQ_PIDFILE")" 2>/dev/null || true
            rm -f "$DNSMASQ_PIDFILE"
        fi
        rm -f "$DNSMASQ_LEASEFILE"
    fi

    for tap in "${TAP_DEVICES[@]}"; do
        ip link set "$tap" down 2>/dev/null || true
        ip link delete "$tap" 2>/dev/null || true
    done

    if $CREATED_BRIDGE; then
        iptables -t nat -D POSTROUTING -s "${SUBNET}.0/24" ! -o "$BRIDGE_NAME" -j MASQUERADE 2>/dev/null || true
        iptables -D FORWARD -i "$BRIDGE_NAME" -o "$BRIDGE_NAME" -j ACCEPT 2>/dev/null || true
        iptables -D INPUT -i "$BRIDGE_NAME" -s "${SUBNET}.0/24" -j ACCEPT 2>/dev/null || true
        iptables -D OUTPUT -o "$BRIDGE_NAME" -d "${SUBNET}.0/24" -j ACCEPT 2>/dev/null || true
        ip link set "$BRIDGE_NAME" down 2>/dev/null || true
        ip link delete "$BRIDGE_NAME" 2>/dev/null || true
        info "Removed bridge $BRIDGE_NAME and iptables rules"
    fi

    ok "Cleanup complete."
}

trap cleanup_on_exit EXIT INT TERM

setup_bridge_network() {
    if [[ $(id -u) -ne 0 ]]; then
        fail "Bridge networking requires root. Run with sudo or specify an existing bridge with --bridge."
    fi

    if [[ -z "$BRIDGE" ]]; then
        # Create temporary bridge
        if ! ip link show "$BRIDGE_NAME" &>/dev/null; then
            info "Creating bridge: $BRIDGE_NAME"
            ip link add name "$BRIDGE_NAME" type bridge
            ip addr add "${SUBNET}.1/24" dev "$BRIDGE_NAME"
            ip link set "$BRIDGE_NAME" up
            CREATED_BRIDGE=true

            # Enable IP forwarding and set up masquerading so guests can reach outside
            sysctl -q -w net.ipv4.ip_forward=1
            iptables -t nat -A POSTROUTING -s "${SUBNET}.0/24" ! -o "$BRIDGE_NAME" -j MASQUERADE
            iptables -A FORWARD -i "$BRIDGE_NAME" -o "$BRIDGE_NAME" -j ACCEPT
            # Allow all traffic between host and guests on the bridge
            iptables -A INPUT -i "$BRIDGE_NAME" -s "${SUBNET}.0/24" -j ACCEPT
            iptables -A OUTPUT -o "$BRIDGE_NAME" -d "${SUBNET}.0/24" -j ACCEPT
            ok "Bridge $BRIDGE_NAME up with ${SUBNET}.1/24"

            # Ensure host can reach its own bridge IP (hairpin / loopback)
            ip route replace "${SUBNET}.0/24" dev "$BRIDGE_NAME" src "${SUBNET}.1"
        else
            info "Bridge $BRIDGE_NAME already exists, reusing."
        fi
    else
        # Validate user-supplied bridge
        ip link show "$BRIDGE_NAME" &>/dev/null || fail "Bridge $BRIDGE_NAME does not exist."
        info "Using existing bridge: $BRIDGE_NAME"
    fi

    # Create tap devices for our slots only
    for i in "${VM_SLOTS[@]}"; do
        local tap="coresan-tap${i}"
        if ip link show "$tap" &>/dev/null; then
            ip link delete "$tap" 2>/dev/null || true
        fi
        ip tuntap add dev "$tap" mode tap
        ip link set "$tap" master "$BRIDGE_NAME"
        ip link set "$tap" up
        TAP_DEVICES+=("$tap")
    done
    ok "${#TAP_DEVICES[@]} tap devices created on $BRIDGE_NAME"
}

setup_bridge_network

# ── Deterministic MAC + IP per VM ────────────────────────────────
#
# VM 1 → MAC 52:54:00:c5:00:01  IP .101
# VM 2 → MAC 52:54:00:c5:00:02  IP .102
# …stable across restarts.

mac_for_vm() {
    printf "52:54:00:c5:00:%02x" "$1"
}

ip_for_vm() {
    echo "${SUBNET}.$((100 + $1))"
}

# ── Start DHCP server on bridge ──────────────────────────────────

DNSMASQ_REUSED=false

start_dhcp() {
    # Check if dnsmasq is already running on this bridge (from another instance)
    if [[ -f "$DNSMASQ_PIDFILE" ]] && kill -0 "$(cat "$DNSMASQ_PIDFILE")" 2>/dev/null; then
        ok "dnsmasq already running (PID $(cat "$DNSMASQ_PIDFILE")), reusing."
        DNSMASQ_PID=""
        DNSMASQ_REUSED=true
        return
    fi

    # Also check by process
    local existing_pid
    existing_pid=$(pgrep -f "dnsmasq.*${BRIDGE_NAME}" 2>/dev/null | head -1 || true)
    if [[ -n "$existing_pid" ]]; then
        ok "dnsmasq already running on $BRIDGE_NAME (PID $existing_pid), reusing."
        DNSMASQ_PID=""
        DNSMASQ_REUSED=true
        return
    fi

    # Build static DHCP host entries for ALL possible VMs (1-99)
    # This way any parallel instance can add VMs without restarting dnsmasq
    local dhcp_hosts=()
    for i in $(seq 1 99); do
        local mac ip
        mac=$(mac_for_vm "$i")
        ip=$(ip_for_vm "$i")
        dhcp_hosts+=("--dhcp-host=${mac},coresan-vm${i},${ip}")
    done

    local first_slot="${VM_SLOTS[0]}"
    local last_slot="${VM_SLOTS[${#VM_SLOTS[@]}-1]}"
    info "Starting dnsmasq DHCP on $BRIDGE_NAME (range: ${SUBNET}.101–${SUBNET}.199)"
    dnsmasq \
        --strict-order \
        --bind-interfaces \
        --interface="$BRIDGE_NAME" \
        --except-interface=lo \
        --dhcp-range="${SUBNET}.100,${SUBNET}.199,255.255.255.0,infinite" \
        --dhcp-option=option:router,"${SUBNET}.1" \
        --dhcp-option=option:dns-server,"${SUBNET}.1" \
        "${dhcp_hosts[@]}" \
        --dhcp-no-override \
        --no-resolv \
        --server=8.8.8.8 \
        --server=1.1.1.1 \
        --pid-file="$DNSMASQ_PIDFILE" \
        --dhcp-leasefile="$DNSMASQ_LEASEFILE" \
        --log-facility=- \
        --no-daemon &
    DNSMASQ_PID=$!
    sleep 0.5

    if kill -0 "$DNSMASQ_PID" 2>/dev/null; then
        ok "dnsmasq running (PID $DNSMASQ_PID)"
    else
        fail "dnsmasq failed to start — check if port 53/67 is already in use on $BRIDGE_NAME"
    fi
}

start_dhcp

# ── Launch VMs ───────────────────────────────────────────────────

info "Starting ${VM_COUNT} VM(s) (slots: ${VM_SLOTS[*]})..."
echo ""

tap_idx=0
for i in "${VM_SLOTS[@]}"; do
    vm_path="$VM_DIR/vm${i}"
    mac=$(mac_for_vm "$i")
    monitor_sock="$vm_path/monitor.sock"
    serial_log="$vm_path/serial.log"
    tap_dev="${TAP_DEVICES[$tap_idx]}"
    tap_idx=$((tap_idx + 1))

    vm_ip=$(ip_for_vm "$i")
    info "VM ${i}: ${vm_ip}  mac=${mac}  tap=${tap_dev}"

    $QEMU_BIN \
        -name "coresan-vm${i},process=coresan-vm${i}" \
        -enable-kvm \
        -cpu host \
        -smp "$CPUS" \
        -m "$RAM_MB" \
        -drive file="$vm_path/system.qcow2",format=qcow2,if=virtio,index=0 \
        -drive file="$vm_path/data1.qcow2",format=qcow2,if=virtio,index=1 \
        -drive file="$vm_path/data2.qcow2",format=qcow2,if=virtio,index=2 \
        -cdrom "$ISO" \
        -boot order=cdn \
        -netdev tap,id=net0,ifname="$tap_dev",script=no,downscript=no \
        -device virtio-net-pci,netdev=net0,mac="$mac" \
        -display gtk,window-close=off \
        -monitor unix:"$monitor_sock",server,nowait \
        -serial file:"$serial_log" \
        &

    QEMU_PIDS+=($!)
done

echo ""
ok "${VM_COUNT} VM(s) started with GUI windows."
echo ""
info "VM network (fixed DHCP assignments):"
echo "  Host:   ${SUBNET}.1"
for i in "${VM_SLOTS[@]}"; do
    echo "  VM ${i}:   $(ip_for_vm "$i")  ($(mac_for_vm "$i"))"
done
echo ""
info "Monitor sockets (use 'socat - UNIX-CONNECT:<path>'):"
for i in "${VM_SLOTS[@]}"; do
    echo "  VM ${i}: $VM_DIR/vm${i}/monitor.sock"
done
echo ""
info "Serial logs:"
for i in "${VM_SLOTS[@]}"; do
    echo "  VM ${i}: $VM_DIR/vm${i}/serial.log"
done
echo ""
info "Press Ctrl+C to stop all VMs and clean up."
echo ""

# ── Wait for all VMs ─────────────────────────────────────────────

wait_for_vms() {
    while true; do
        local alive=0
        for pid in "${QEMU_PIDS[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then
                alive=$((alive + 1))
            fi
        done
        if [[ $alive -eq 0 ]]; then
            info "All VMs have shut down."
            break
        fi
        sleep 2
    done
}

wait_for_vms
