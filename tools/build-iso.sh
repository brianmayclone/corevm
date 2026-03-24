#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
BUILD_DIR="$ROOT/dist/iso-build"
ISO_OUTPUT="$ROOT/dist/corevm-appliance.iso"

# Must run as root (debootstrap, chroot, mount all require it)
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: This script must be run as root (./tools/build-iso.sh)"
    exit 1
fi

# Check prerequisites
for cmd in debootstrap xorriso mksquashfs grub-mkimage mtools cargo node npm; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: Missing required command: $cmd"; exit 1; }
done

echo "=== Building CoreVM Appliance ISO ==="
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# Step 0: Build all binaries
echo "[1/8] Building binaries..."
cd "$ROOT/apps/vmm-ui" && npm install --silent && npx vite build
cd "$ROOT"
cargo build --release -p vmm-appliance -p vmm-server -p vmm-cluster

# Step 1: Build installable root-FS tarball
echo "[2/8] Building root filesystem..."
ROOTFS_DIR="$BUILD_DIR/rootfs"
debootstrap --variant=minbase --include=\
linux-image-amd64,grub-pc,grub-efi-amd64-bin,systemd,\
openssh-server,openssl,chrony,parted,\
e2fsprogs,dosfstools,iproute2,sudo,ca-certificates,\
util-linux,pciutils,nftables,locales,\
nfs-common,nfs-kernel-server,\
glusterfs-server,glusterfs-client,\
ceph-common,ceph-fuse \
    bookworm "$ROOTFS_DIR" http://deb.debian.org/debian

# Copy CoreVM binaries
mkdir -p "$ROOTFS_DIR/opt/vmm"
cp "$ROOT/target/release/vmm-appliance" "$ROOTFS_DIR/opt/vmm/"
cp "$ROOT/target/release/vmm-server" "$ROOTFS_DIR/opt/vmm/"
cp "$ROOT/target/release/vmm-cluster" "$ROOTFS_DIR/opt/vmm/"
cp -r "$ROOT/apps/vmm-ui/dist" "$ROOTFS_DIR/opt/vmm/ui"
if [ -d "$ROOT/apps/vmm-server/assets/bios" ]; then
    cp -r "$ROOT/apps/vmm-server/assets/bios" "$ROOTFS_DIR/opt/vmm/bios"
fi

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

# Copy GRUB defaults and nftables config
cp "$SCRIPT_DIR/iso/grub-installed.cfg" "$ROOTFS_DIR/etc/default/grub"
cp "$SCRIPT_DIR/iso/nftables.conf" "$ROOTFS_DIR/etc/nftables.conf"

# Enable services (use --root= since systemd is not PID 1 in the chroot)
systemctl --root="$ROOTFS_DIR" enable vmm-dcui.service
systemctl --root="$ROOTFS_DIR" enable nftables.service
systemctl --root="$ROOTFS_DIR" enable systemd-networkd.service
systemctl --root="$ROOTFS_DIR" enable systemd-resolved.service
systemctl --root="$ROOTFS_DIR" enable ssh.service

# Disable getty on tty1 (DCUI takes over)
systemctl --root="$ROOTFS_DIR" mask getty@tty1.service

# Symlink resolv.conf for systemd-resolved
ln -sf /run/systemd/resolve/stub-resolv.conf "$ROOTFS_DIR/etc/resolv.conf"

# Build initramfs in chroot
mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"
chroot "$ROOTFS_DIR" update-initramfs -u
umount "$ROOTFS_DIR/dev" "$ROOTFS_DIR/sys" "$ROOTFS_DIR/proc"

# Pack rootfs tarball
echo "[3/8] Packing rootfs tarball..."
tar czf "$BUILD_DIR/rootfs.tar.gz" -C "$ROOTFS_DIR" .

# Step 2: Build live environment
echo "[4/8] Building live environment..."
LIVE_DIR="$BUILD_DIR/live-root"
debootstrap --variant=minbase --include=\
linux-image-amd64,live-boot,systemd \
    bookworm "$LIVE_DIR" http://deb.debian.org/debian

# Copy installer binary + rootfs tarball into live env
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

# Step 3: Assemble ISO
echo "[5/8] Creating squashfs..."
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

# Copy isolinux files
if [ -f /usr/lib/ISOLINUX/isolinux.bin ]; then
    cp /usr/lib/ISOLINUX/isolinux.bin "$ISO_STAGING/isolinux/"
fi
if [ -f /usr/lib/syslinux/modules/bios/ldlinux.c32 ]; then
    cp /usr/lib/syslinux/modules/bios/ldlinux.c32 "$ISO_STAGING/isolinux/"
fi

# Build EFI boot image
echo "[6/8] Building EFI boot image..."
mkdir -p "$ISO_STAGING/boot/grub/x86_64-efi"
if [ -d /usr/lib/grub/x86_64-efi ]; then
    cp /usr/lib/grub/x86_64-efi/*.mod "$ISO_STAGING/boot/grub/x86_64-efi/"
fi
grub-mkimage -o "$ISO_STAGING/boot/grub/bootx64.efi" \
    -p /boot/grub -O x86_64-efi \
    part_gpt part_msdos fat iso9660 normal boot linux search search_fs_uuid search_label configfile

# Create FAT image for EFI System Partition
dd if=/dev/zero of="$ISO_STAGING/boot/grub/efi.img" bs=1M count=4 2>/dev/null
mkfs.vfat "$ISO_STAGING/boot/grub/efi.img" >/dev/null
mmd -i "$ISO_STAGING/boot/grub/efi.img" ::/EFI ::/EFI/BOOT
mcopy -i "$ISO_STAGING/boot/grub/efi.img" "$ISO_STAGING/boot/grub/bootx64.efi" ::/EFI/BOOT/BOOTX64.EFI

echo "[7/8] Building ISO image..."
xorriso -as mkisofs \
    -o "$ISO_OUTPUT" \
    -isohybrid-mbr /usr/lib/ISOLINUX/isohdpfx.bin \
    -c isolinux/boot.cat \
    -b isolinux/isolinux.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    -eltorito-alt-boot \
    -e boot/grub/efi.img \
    -no-emul-boot -isohybrid-gpt-basdat \
    "$ISO_STAGING"

echo "[8/8] Done!"
echo "ISO: $ISO_OUTPUT"
ls -lh "$ISO_OUTPUT"

# Cleanup
echo "Cleaning up build directory..."
rm -rf "$BUILD_DIR"
