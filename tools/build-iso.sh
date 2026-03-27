#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$ROOT/dist/iso-build"

# Read version from VERSION file (single source of truth)
COREVM_VERSION=$(cat "$ROOT/VERSION" | tr -d '[:space:]')
BUILD_TIMESTAMP=$(date -u +%Y%m%dT%H%M%SZ)

# ISO output filename: include version, optionally append timestamp
# Override with COREVM_ISO_TIMESTAMP=1 to append build timestamp
if [ "${COREVM_ISO_TIMESTAMP:-0}" = "1" ]; then
    ISO_OUTPUT="$ROOT/dist/corevm-appliance-${COREVM_VERSION}-${BUILD_TIMESTAMP}.iso"
else
    ISO_OUTPUT="$ROOT/dist/corevm-appliance-${COREVM_VERSION}.iso"
fi
echo "Building CoreVM Appliance v${COREVM_VERSION} (${BUILD_TIMESTAMP})"

# ============================================================
# SAFETY: Verify paths are absolute and point inside the repo
# ============================================================
if [[ "$ROOT" != /* ]]; then
    echo "FATAL: ROOT is not an absolute path: $ROOT"
    exit 1
fi
if [[ "$BUILD_DIR" != "$ROOT/dist/"* ]]; then
    echo "FATAL: BUILD_DIR is not inside ROOT/dist: $BUILD_DIR"
    exit 1
fi
# Verify this is actually our repo
if [ ! -f "$ROOT/Cargo.toml" ] || [ ! -d "$ROOT/tools" ]; then
    echo "FATAL: ROOT does not look like the corevm repo: $ROOT"
    exit 1
fi

# Must run as root (debootstrap, chroot, mount all require it)
# Use: sudo -E env "PATH=$PATH" ./tools/build-iso.sh
# Add --reset for a full rebuild (otherwise reuses cached rootfs + live-root)
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: This script must be run as root."
    echo "Usage: sudo -E env \"PATH=\$PATH\" ./tools/build-iso.sh"
    exit 1
fi

# Ensure cargo/node are in PATH (may come from user's home dir)
for p in "$HOME/.cargo/bin" "/usr/local/bin"; do
    [ -d "$p" ] && export PATH="$p:$PATH"
done
# Also check SUDO_USER's home for cargo and node
if [ -n "${SUDO_USER:-}" ]; then
    SUDO_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
    [ -d "$SUDO_HOME/.cargo/bin" ] && export PATH="$SUDO_HOME/.cargo/bin:$PATH"
    [ -d "$SUDO_HOME/.nvm/versions" ] && {
        NODE_DIR=$(ls -d "$SUDO_HOME/.nvm/versions/node/"*/bin 2>/dev/null | tail -1)
        [ -n "$NODE_DIR" ] && export PATH="$NODE_DIR:$PATH"
    }
fi

# Check prerequisites
for cmd in debootstrap xorriso mksquashfs grub-mkimage mtools node npm; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: Missing required command: $cmd"; exit 1; }
done

echo "=== Building CoreVM Appliance ISO ==="
echo "ROOT:      $ROOT"
echo "BUILD_DIR: $BUILD_DIR"

# ── Safety: cleanup function to unmount chroot mounts on ANY exit ────────
# This prevents the catastrophic scenario where a failed build leaves
# /proc, /sys, /dev bind-mounted inside the build directory, causing
# a subsequent "rm -rf" to destroy the host filesystem.
cleanup_mounts() {
    echo "Cleaning up chroot mounts..."
    local rootfs="$BUILD_DIR/rootfs"
    umount "$rootfs/build" 2>/dev/null || true
    umount "$rootfs/dev"   2>/dev/null || true
    umount "$rootfs/sys"   2>/dev/null || true
    umount "$rootfs/proc"  2>/dev/null || true
    # Also lazy-unmount as fallback
    umount -l "$rootfs/build" 2>/dev/null || true
    umount -l "$rootfs/dev"   2>/dev/null || true
    umount -l "$rootfs/sys"   2>/dev/null || true
    umount -l "$rootfs/proc"  2>/dev/null || true
}
trap cleanup_mounts EXIT ERR

# --reset: full rebuild (delete cached rootfs + live-root)
# --clean: alias for --reset (backwards compat)
RESET=false
if [[ " $* " == *" --reset "* ]] || [[ " $* " == *" --clean "* ]]; then
    RESET=true
fi

if $RESET && [ -d "$BUILD_DIR" ]; then
    echo "Cleaning previous build directory (--reset requested)..."
    cleanup_mounts
    sleep 1
    # Safety check: refuse to rm if any mounts are still active
    if mount | grep -q "$BUILD_DIR/rootfs/proc"; then
        echo "FATAL: /proc still mounted in $BUILD_DIR/rootfs — refusing to rm -rf"
        echo "Manual fix: sudo umount $BUILD_DIR/rootfs/{proc,sys,dev,build}"
        exit 1
    fi
    rm -rf "$BUILD_DIR"
elif [ -d "$BUILD_DIR" ]; then
    echo "Reusing existing build directory (use --reset for full rebuild)"
    # Always ensure stale mounts from a previous crashed build are gone
    cleanup_mounts
fi
mkdir -p "$BUILD_DIR"

# Step 1: Build UI
echo "[1/9] Building UI..."
cd "$ROOT/apps/vmm-ui" && npm install --silent && npx vite build
cd "$ROOT"

# Step 2: Build installable root-FS
ROOTFS_DIR="$BUILD_DIR/rootfs"
if [ -d "$ROOTFS_DIR/usr" ]; then
    echo "[2/9] Reusing existing root filesystem (use --clean to rebuild)"
else
    echo "[2/9] Building root filesystem..."
    debootstrap --variant=minbase --include=\
linux-image-amd64,grub-pc,grub-efi-amd64-bin,systemd,systemd-sysv,dbus,\
openssh-server,openssl,chrony,parted,\
plymouth,plymouth-themes,\
e2fsprogs,dosfstools,iproute2,sudo,ca-certificates,\
util-linux,pciutils,nftables,locales,\
nfs-common,nfs-kernel-server,\
glusterfs-server,glusterfs-client,\
ceph-common,ceph-fuse,\
fuse3,\
curl,gcc,libc6-dev,pkg-config,libssl-dev,make \
    bookworm "$ROOTFS_DIR" http://deb.debian.org/debian
fi

# Step 3: Build Rust binaries inside chroot (ensures glibc compatibility)
echo "[3/9] Building Rust binaries in Debian 12 chroot..."
mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"

# Bind-mount the source code into the chroot
mkdir -p "$ROOTFS_DIR/build"
mount --bind "$ROOT" "$ROOTFS_DIR/build"

# Install Rust in chroot and build
chroot "$ROOTFS_DIR" bash -c '
    export HOME=/root

    # Install build-only dependencies for CoreSAN (FUSE headers needed by fuser crate)
    apt-get update -qq
    apt-get install -y --no-install-recommends libfuse3-dev 2>/dev/null || true

    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable 2>&1
    source /root/.cargo/env
    cd /build
    cargo build --release -p vmm-appliance -p vmm-server -p vmm-cluster -p vmm-san

    # Remove build-only dependencies (not needed at runtime, saves ~50MB)
    apt-get purge -y libfuse3-dev 2>/dev/null || true
    apt-get autoremove -y 2>/dev/null || true
    apt-get clean
'

# Unmount source and cleanup Rust toolchain from rootfs
umount "$ROOTFS_DIR/build" 2>/dev/null || umount -l "$ROOTFS_DIR/build" 2>/dev/null || true
rmdir "$ROOTFS_DIR/build" 2>/dev/null || true
chroot "$ROOTFS_DIR" bash -c 'rm -rf /root/.cargo /root/.rustup' 2>/dev/null || true
umount "$ROOTFS_DIR/dev"  2>/dev/null || umount -l "$ROOTFS_DIR/dev"  2>/dev/null || true
umount "$ROOTFS_DIR/sys"  2>/dev/null || umount -l "$ROOTFS_DIR/sys"  2>/dev/null || true
umount "$ROOTFS_DIR/proc" 2>/dev/null || umount -l "$ROOTFS_DIR/proc" 2>/dev/null || true

# Copy CoreVM binaries (now built for Debian 12 glibc)
mkdir -p "$ROOTFS_DIR/opt/vmm"
cp "$ROOT/target/release/vmm-appliance" "$ROOTFS_DIR/opt/vmm/"
cp "$ROOT/target/release/vmm-server" "$ROOTFS_DIR/opt/vmm/"
cp "$ROOT/target/release/vmm-cluster" "$ROOTFS_DIR/opt/vmm/"
cp "$ROOT/target/release/vmm-san" "$ROOTFS_DIR/opt/vmm/"
cp -r "$ROOT/apps/vmm-ui/dist" "$ROOTFS_DIR/opt/vmm/ui"
if [ -d "$ROOT/apps/vmm-server/assets/bios" ]; then
    cp -r "$ROOT/apps/vmm-server/assets/bios" "$ROOTFS_DIR/opt/vmm/bios"
fi

# Install Plymouth theme
PLYMOUTH_DIR="$ROOTFS_DIR/usr/share/plymouth/themes/corevm"
mkdir -p "$PLYMOUTH_DIR"
cp "$SCRIPT_DIR/iso/plymouth-theme/logo.png" "$PLYMOUTH_DIR/"
cp "$SCRIPT_DIR/iso/plymouth-theme/corevm.plymouth" "$PLYMOUTH_DIR/"
cp "$SCRIPT_DIR/iso/plymouth-theme/corevm.script" "$PLYMOUTH_DIR/"
mkdir -p "$ROOTFS_DIR/etc/plymouth"
cat > "$ROOTFS_DIR/etc/plymouth/plymouthd.conf" <<'PLYMOUTHCONF'
[Daemon]
Theme=corevm
ShowDelay=0
PLYMOUTHCONF

# Branding: os-release, issue, motd, hostname
cat > "$ROOTFS_DIR/etc/os-release" <<OSRELEASE
PRETTY_NAME="CoreVM Appliance ${COREVM_VERSION}"
NAME="CoreVM"
VERSION_ID="${COREVM_VERSION}"
VERSION="${COREVM_VERSION}"
BUILD_TIMESTAMP="${BUILD_TIMESTAMP}"
ID=corevm
ID_LIKE=debian
HOME_URL="https://corevm.io"
OSRELEASE

cat > "$ROOTFS_DIR/etc/issue" <<ISSUE

   ____               __     __ __  __
  / ___|___  _ __ ___ \\ \\   / /|  \\/  |
 | |   / _ \\| '__/ _ \\ \\ \\/ / | |\\/| |
 | |__| (_) | | |  __/  \\ V /  | |  | |
  \\____\\___/|_|  \\___|   \\_/   |_|  |_|

  CoreVM Appliance ${COREVM_VERSION} -- \\n \\l

ISSUE

cp "$ROOTFS_DIR/etc/issue" "$ROOTFS_DIR/etc/issue.net"

cat > "$ROOTFS_DIR/etc/motd" <<MOTD

  Welcome to CoreVM Appliance ${COREVM_VERSION}
  Manage this appliance via the DCUI on tty1 or the web UI.

MOTD

echo "corevm" > "$ROOTFS_DIR/etc/hostname"
cat > "$ROOTFS_DIR/etc/hosts" <<'HOSTS'
127.0.0.1	localhost
127.0.1.1	corevm
::1		localhost ip6-localhost ip6-loopback
HOSTS

# Copy systemd service files
mkdir -p "$ROOTFS_DIR/etc/systemd/system"

tee "$ROOTFS_DIR/etc/systemd/system/vmm-dcui.service" > /dev/null <<'DCUI_SVC'
[Unit]
Description=CoreVM DCUI
After=multi-user.target

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-appliance --mode dcui
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
Restart=always

[Install]
WantedBy=multi-user.target
DCUI_SVC

tee "$ROOTFS_DIR/etc/systemd/system/vmm-server.service" > /dev/null <<'SERVER_SVC'
[Unit]
Description=CoreVM Server
After=network.target

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
SERVER_SVC

tee "$ROOTFS_DIR/etc/systemd/system/vmm-cluster.service" > /dev/null <<'CLUSTER_SVC'
[Unit]
Description=CoreVM Cluster Controller
After=network.target

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-cluster
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
CLUSTER_SVC

tee "$ROOTFS_DIR/etc/systemd/system/vmm-san.service" > /dev/null <<'SAN_SVC'
[Unit]
Description=CoreSAN Software-Defined Storage
After=network.target
Before=vmm-server.service

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-san
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
SAN_SVC

# Configure FUSE for CoreSAN (allow_other needed for VM access to FUSE mounts)
echo "user_allow_other" > "$ROOTFS_DIR/etc/fuse.conf"
chmod 644 "$ROOTFS_DIR/etc/fuse.conf"

# Ensure fuse kernel module loads at boot
echo "fuse" >> "$ROOTFS_DIR/etc/modules-load.d/corevm.conf"

# Create default CoreSAN config
mkdir -p "$ROOTFS_DIR/etc/vmm"
tee "$ROOTFS_DIR/etc/vmm/vmm-san.toml" > /dev/null <<'SAN_CONF'
[server]
bind = "0.0.0.0"
port = 7443

[data]
data_dir = "/var/lib/vmm-san"
fuse_root = "/vmm/san"

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
log_file = "/var/log/vmm/vmm-san.log"
SAN_CONF

# Create CoreSAN data directories and log directory
mkdir -p "$ROOTFS_DIR/var/lib/vmm-san"
mkdir -p "$ROOTFS_DIR/var/log/vmm"
mkdir -p "$ROOTFS_DIR/vmm/san"

# Open UDP discovery port in firewall
# (will be picked up by nftables if the config supports it)

# Copy GRUB defaults and nftables config
cp "$SCRIPT_DIR/iso/grub-installed.cfg" "$ROOTFS_DIR/etc/default/grub"
cp "$SCRIPT_DIR/iso/nftables.conf" "$ROOTFS_DIR/etc/nftables.conf"

# Enable services (use --root= since systemd is not PID 1 in the chroot)
systemctl --root="$ROOTFS_DIR" enable vmm-dcui.service
systemctl --root="$ROOTFS_DIR" enable vmm-san.service
systemctl --root="$ROOTFS_DIR" enable nftables.service
systemctl --root="$ROOTFS_DIR" enable ssh.service

# systemd-networkd: enable only the core unit, skip wait-online (not in minbase)
ln -sf /usr/lib/systemd/system/systemd-networkd.service \
    "$ROOTFS_DIR/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
ln -sf /usr/lib/systemd/system/systemd-networkd.socket \
    "$ROOTFS_DIR/etc/systemd/system/sockets.target.wants/systemd-networkd.socket"

# Disable getty on tty1 (DCUI takes over)
systemctl --root="$ROOTFS_DIR" mask getty@tty1.service

# DNS: use static resolv.conf (systemd-resolved not available in Debian 12 minbase)
echo "nameserver 8.8.8.8" > "$ROOTFS_DIR/etc/resolv.conf"
echo "nameserver 1.1.1.1" >> "$ROOTFS_DIR/etc/resolv.conf"

# Build initramfs in chroot
mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"
chroot "$ROOTFS_DIR" update-initramfs -u
umount "$ROOTFS_DIR/dev" "$ROOTFS_DIR/sys" "$ROOTFS_DIR/proc"

# Remove build tools from rootfs (not needed on the appliance)
mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"
chroot "$ROOTFS_DIR" apt-get purge -y gcc libc6-dev pkg-config libssl-dev make cpp 2>/dev/null || true
chroot "$ROOTFS_DIR" apt-get autoremove -y 2>/dev/null || true
chroot "$ROOTFS_DIR" apt-get clean
umount "$ROOTFS_DIR/dev" "$ROOTFS_DIR/sys" "$ROOTFS_DIR/proc"

# Pack rootfs tarball
echo "[4/9] Packing rootfs tarball..."
tar czf "$BUILD_DIR/rootfs.tar.gz" -C "$ROOTFS_DIR" .

# Step 4: Build live environment
LIVE_DIR="$BUILD_DIR/live-root"
if [ -d "$LIVE_DIR/usr" ]; then
    echo "[5/9] Reusing existing live environment (use --reset to rebuild)"
else
    echo "[5/9] Building live environment..."
    debootstrap --variant=minbase --include=\
linux-image-amd64,live-boot,live-boot-initramfs-tools,\
initramfs-tools,systemd,systemd-sysv,udev,\
parted,e2fsprogs,dosfstools,tar,openssl,ncurses-base,iproute2,\
grub-pc,grub-efi-amd64-bin \
    bookworm "$LIVE_DIR" http://deb.debian.org/debian
fi

# Ensure squashfs module is loaded in initramfs
echo "squashfs" >> "$LIVE_DIR/etc/initramfs-tools/modules"

# Branding for live environment
cat > "$LIVE_DIR/etc/os-release" <<OSRELEASE
PRETTY_NAME="CoreVM Appliance Installer ${COREVM_VERSION}"
NAME="CoreVM"
VERSION_ID="${COREVM_VERSION}"
VERSION="${COREVM_VERSION}"
BUILD_TIMESTAMP="${BUILD_TIMESTAMP}"
ID=corevm
ID_LIKE=debian
OSRELEASE

cat > "$LIVE_DIR/etc/issue" <<ISSUE

   ____               __     __ __  __
  / ___|___  _ __ ___ \\ \\   / /|  \\/  |
 | |   / _ \\| '__/ _ \\ \\ \\/ / | |\\/| |
 | |__| (_) | | |  __/  \\ V /  | |  | |
  \\____\\___/|_|  \\___|   \\_/   |_|  |_|

  CoreVM Appliance Installer ${COREVM_VERSION} -- \\n \\l

ISSUE

echo "corevm-installer" > "$LIVE_DIR/etc/hostname"

# Set root password for debug access (auto-login on tty2+)
chroot "$LIVE_DIR" bash -c 'echo "root:corevm" | chpasswd'

# Enable auto-login on tty2 for debugging
mkdir -p "$LIVE_DIR/etc/systemd/system/getty@tty2.service.d"
cat > "$LIVE_DIR/etc/systemd/system/getty@tty2.service.d/autologin.conf" <<'AUTOLOGIN'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I $TERM
AUTOLOGIN

# Rebuild initramfs with live-boot and squashfs support
mount --bind /proc "$LIVE_DIR/proc"
mount --bind /sys "$LIVE_DIR/sys"
mount --bind /dev "$LIVE_DIR/dev"
chroot "$LIVE_DIR" update-initramfs -u
umount "$LIVE_DIR/dev" "$LIVE_DIR/sys" "$LIVE_DIR/proc"

# Copy installer binary + rootfs tarball + boot splash into live env
mkdir -p "$LIVE_DIR/opt/vmm"
cp "$ROOT/target/release/vmm-appliance" "$LIVE_DIR/opt/vmm/"
cp "$BUILD_DIR/rootfs.tar.gz" "$LIVE_DIR/opt/vmm/"
# Auto-start installer in live env
tee "$LIVE_DIR/etc/systemd/system/vmm-installer.service" > /dev/null <<'INSTALLER_SVC'
[Unit]
Description=CoreVM Installer
After=multi-user.target

[Service]
Type=simple
ExecStart=/opt/vmm/vmm-appliance --mode installer
StandardInput=tty
StandardOutput=tty
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes

[Install]
WantedBy=multi-user.target
INSTALLER_SVC

systemctl --root="$LIVE_DIR" enable vmm-installer.service
systemctl --root="$LIVE_DIR" mask getty@tty1.service

# Step 5: Assemble ISO
echo "[6/9] Creating squashfs..."
ISO_STAGING="$BUILD_DIR/iso-staging"
mkdir -p "$ISO_STAGING/live" "$ISO_STAGING/boot/grub" "$ISO_STAGING/isolinux"

# Copy kernel + initramfs from live env
cp "$LIVE_DIR/vmlinuz" "$ISO_STAGING/live/" 2>/dev/null || \
    cp "$LIVE_DIR/boot/vmlinuz-"* "$ISO_STAGING/live/vmlinuz"
cp "$LIVE_DIR/initrd.img" "$ISO_STAGING/live/" 2>/dev/null || \
    cp "$LIVE_DIR/boot/initrd.img-"* "$ISO_STAGING/live/initrd.img"

# Create squashfs
mksquashfs "$LIVE_DIR" "$ISO_STAGING/live/filesystem.squashfs" -comp xz -noappend

# Copy boot configs
cp "$SCRIPT_DIR/iso/grub.cfg" "$ISO_STAGING/boot/grub/"
cp "$SCRIPT_DIR/iso/isolinux.cfg" "$ISO_STAGING/isolinux/"

# Copy isolinux/syslinux boot files (search standard package paths first)
find_and_copy() {
    local filename="$1"
    local dest="$2"
    local found=""
    # Prefer standard isolinux/syslinux package paths to avoid picking up
    # incompatible versions from other software (e.g. VMware)
    for dir in /usr/lib/ISOLINUX /usr/lib/syslinux/modules/bios /usr/share/syslinux; do
        if [ -f "$dir/$filename" ]; then
            found="$dir/$filename"
            break
        fi
    done
    # Fallback: search /usr/lib and /usr/share, excluding efi and third-party paths
    if [ -z "$found" ]; then
        found=$(find /usr/lib /usr/share -name "$filename" \
            -not -path "*/efi*" -not -path "*/vmware/*" 2>/dev/null | head -1)
    fi
    if [ -n "$found" ]; then
        cp "$found" "$dest"
        echo "  Found $filename at $found"
    else
        echo "ERROR: Cannot find $filename — install isolinux and syslinux-common packages"
        exit 1
    fi
}

find_and_copy "isolinux.bin" "$ISO_STAGING/isolinux/"
find_and_copy "ldlinux.c32" "$ISO_STAGING/isolinux/"
find_and_copy "menu.c32" "$ISO_STAGING/isolinux/"
find_and_copy "libcom32.c32" "$ISO_STAGING/isolinux/"
find_and_copy "libutil.c32" "$ISO_STAGING/isolinux/"
find_and_copy "isohdpfx.bin" "$ISO_STAGING/isolinux/"

# Build EFI boot image
echo "[7/9] Building EFI boot image..."
mkdir -p "$ISO_STAGING/boot/grub/x86_64-efi"
if [ -d /usr/lib/grub/x86_64-efi ]; then
    cp /usr/lib/grub/x86_64-efi/*.mod "$ISO_STAGING/boot/grub/x86_64-efi/"
fi
grub-mkimage -o "$ISO_STAGING/boot/grub/bootx64.efi" \
    -p /boot/grub -O x86_64-efi \
    part_gpt part_msdos fat iso9660 normal boot linux search search_fs_uuid search_label configfile

# Create FAT image for EFI System Partition + copy into ISO tree
dd if=/dev/zero of="$ISO_STAGING/boot/grub/efi.img" bs=1M count=4 2>/dev/null
mkfs.vfat "$ISO_STAGING/boot/grub/efi.img" >/dev/null
mmd -i "$ISO_STAGING/boot/grub/efi.img" ::/EFI ::/EFI/BOOT
mcopy -i "$ISO_STAGING/boot/grub/efi.img" "$ISO_STAGING/boot/grub/bootx64.efi" ::/EFI/BOOT/BOOTX64.EFI

# Also place EFI directory in ISO filesystem (fixes xorriso warning)
mkdir -p "$ISO_STAGING/EFI/BOOT"
cp "$ISO_STAGING/boot/grub/bootx64.efi" "$ISO_STAGING/EFI/BOOT/BOOTX64.EFI"

echo "[8/9] Building ISO image..."
xorriso -as mkisofs \
    -o "$ISO_OUTPUT" \
    -isohybrid-mbr "$ISO_STAGING/isolinux/isohdpfx.bin" \
    -c isolinux/boot.cat \
    -b isolinux/isolinux.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    -eltorito-alt-boot \
    -e boot/grub/efi.img \
    -no-emul-boot -isohybrid-gpt-basdat \
    "$ISO_STAGING"

echo ""
echo "[9/9] Done!"
echo "ISO: $ISO_OUTPUT"
ls -lh "$ISO_OUTPUT"

# Cleanup temporary staging (keep rootfs + live-root cached for next build)
echo "Cleaning up staging directory..."
rm -rf "$ISO_STAGING"
rm -f "$BUILD_DIR/rootfs.tar.gz"
echo "Cached rootfs and live-root kept in $BUILD_DIR (use --reset to purge)"
