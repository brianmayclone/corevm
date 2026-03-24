use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;
use std::io::Write;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sysinfo::System;

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub model: String,
}

fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .with_context(|| format!("Failed to execute: {} {}", program, args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Command failed: {} {}\nstderr: {}", program, args.join(" "), stderr);
    }
    Ok(())
}

fn run_cmd_output(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute: {} {}", program, args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Command failed: {} {}\nstderr: {}", program, args.join(" "), stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
struct LsblkDevice {
    name: String,
    size: u64,
    model: Option<String>,
    #[serde(rename = "type")]
    dev_type: String,
}

pub fn detect_disks() -> Result<Vec<DiskInfo>> {
    let output = run_cmd_output("lsblk", &["-Jb", "-o", "NAME,SIZE,MODEL,TYPE"])
        .context("Failed to run lsblk")?;
    let parsed: LsblkOutput =
        serde_json::from_str(&output).context("Failed to parse lsblk JSON output")?;
    let disks = parsed
        .blockdevices
        .into_iter()
        .filter(|d| d.dev_type == "disk")
        .map(|d| DiskInfo {
            path: PathBuf::from(format!("/dev/{}", d.name)),
            size_bytes: d.size,
            model: d.model.unwrap_or_default().trim().to_string(),
        })
        .collect();
    Ok(disks)
}

fn part_path(disk: &Path, num: u32) -> String {
    let disk_str = disk.to_str().unwrap_or("");
    if disk_str.contains("nvme") || disk_str.contains("mmcblk") {
        format!("{}p{}", disk_str, num)
    } else {
        format!("{}{}", disk_str, num)
    }
}

pub fn partition_disk(disk: &Path, efi: bool) -> Result<()> {
    let disk_str = disk.to_str().context("Invalid disk path")?;
    let ram_gib = get_ram_bytes() / (1024 * 1024 * 1024);
    let swap_gib = std::cmp::min(ram_gib, 8).max(1);

    run_cmd("parted", &["-s", disk_str, "mklabel", "gpt"])
        .context("Failed to create GPT label")?;

    let mut offset_mib: u64 = 1;

    if efi {
        let end = offset_mib + 256;
        run_cmd("parted", &[
            "-s", disk_str,
            "mkpart", "EFI", "fat32",
            &format!("{}MiB", offset_mib),
            &format!("{}MiB", end),
        ])
        .context("Failed to create EFI partition")?;
        run_cmd("parted", &["-s", disk_str, "set", "1", "esp", "on"])
            .context("Failed to set ESP flag")?;
        offset_mib = end;
    }

    // boot (512 MiB)
    let boot_end = offset_mib + 512;
    run_cmd("parted", &[
        "-s", disk_str,
        "mkpart", "boot", "ext4",
        &format!("{}MiB", offset_mib),
        &format!("{}MiB", boot_end),
    ])
    .context("Failed to create boot partition")?;
    offset_mib = boot_end;

    // swap
    let swap_end = offset_mib + swap_gib * 1024;
    run_cmd("parted", &[
        "-s", disk_str,
        "mkpart", "swap", "linux-swap",
        &format!("{}MiB", offset_mib),
        &format!("{}MiB", swap_end),
    ])
    .context("Failed to create swap partition")?;
    offset_mib = swap_end;

    // root (50 GiB)
    let root_end = offset_mib + 50 * 1024;
    run_cmd("parted", &[
        "-s", disk_str,
        "mkpart", "root", "ext4",
        &format!("{}MiB", offset_mib),
        &format!("{}MiB", root_end),
    ])
    .context("Failed to create root partition")?;
    offset_mib = root_end;

    // data (rest)
    run_cmd("parted", &[
        "-s", disk_str,
        "mkpart", "data", "ext4",
        &format!("{}MiB", offset_mib),
        "100%",
    ])
    .context("Failed to create data partition")?;

    let _ = run_cmd("partprobe", &[disk_str]);
    let _ = run_cmd("udevadm", &["settle"]);

    Ok(())
}

pub fn format_partitions(disk: &Path, efi: bool) -> Result<()> {
    if efi {
        // 1=EFI, 2=boot, 3=swap, 4=root, 5=data
        run_cmd("mkfs.vfat", &["-F32", &part_path(disk, 1)])
            .context("Failed to format EFI partition")?;
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 2)])
            .context("Failed to format boot partition")?;
        run_cmd("mkswap", &[&part_path(disk, 3)]).context("Failed to create swap")?;
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 4)])
            .context("Failed to format root partition")?;
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 5)])
            .context("Failed to format data partition")?;
    } else {
        // 1=boot, 2=swap, 3=root, 4=data
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 1)])
            .context("Failed to format boot partition")?;
        run_cmd("mkswap", &[&part_path(disk, 2)]).context("Failed to create swap")?;
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 3)])
            .context("Failed to format root partition")?;
        run_cmd("mkfs.ext4", &["-F", &part_path(disk, 4)])
            .context("Failed to format data partition")?;
    }
    Ok(())
}

pub fn mount_target(disk: &Path, target: &Path, efi: bool) -> Result<()> {
    fs::create_dir_all(target).context("Failed to create target directory")?;

    let (root_part, boot_part, efi_part, data_part) = if efi {
        (
            part_path(disk, 4),
            part_path(disk, 2),
            Some(part_path(disk, 1)),
            part_path(disk, 5),
        )
    } else {
        (
            part_path(disk, 3),
            part_path(disk, 1),
            None,
            part_path(disk, 4),
        )
    };

    let target_str = target.to_str().context("Invalid target path")?;
    run_cmd("mount", &[&root_part, target_str]).context("Failed to mount root partition")?;

    let boot_dir = target.join("boot");
    fs::create_dir_all(&boot_dir).context("Failed to create /boot")?;
    run_cmd("mount", &[&boot_part, boot_dir.to_str().unwrap()])
        .context("Failed to mount boot partition")?;

    if let Some(efi_p) = efi_part {
        let efi_dir = target.join("boot/efi");
        fs::create_dir_all(&efi_dir).context("Failed to create /boot/efi")?;
        run_cmd("mount", &[&efi_p, efi_dir.to_str().unwrap()])
            .context("Failed to mount EFI partition")?;
    }

    let vmm_dir = target.join("var/lib/vmm");
    fs::create_dir_all(&vmm_dir).context("Failed to create /var/lib/vmm")?;
    run_cmd("mount", &[&data_part, vmm_dir.to_str().unwrap()])
        .context("Failed to mount data partition")?;

    Ok(())
}

pub fn unmount_target(target: &Path) -> Result<()> {
    let vmm_dir = target.join("var/lib/vmm");
    let _ = run_cmd("umount", &[vmm_dir.to_str().unwrap_or("")]);
    let efi_dir = target.join("boot/efi");
    if efi_dir.exists() {
        let _ = run_cmd("umount", &[efi_dir.to_str().unwrap_or("")]);
    }
    let boot_dir = target.join("boot");
    let _ = run_cmd("umount", &[boot_dir.to_str().unwrap_or("")]);
    run_cmd("umount", &[target.to_str().context("Invalid target path")?])
        .context("Failed to unmount root")?;
    Ok(())
}

pub fn extract_rootfs(tarball: &Path, target: &Path) -> Result<()> {
    run_cmd(
        "tar",
        &[
            "xzf",
            tarball.to_str().context("Invalid tarball path")?,
            "-C",
            target.to_str().context("Invalid target path")?,
        ],
    )
    .context("Failed to extract rootfs tarball")
}

pub fn install_grub(target: &Path, disk: &Path, efi: bool) -> Result<()> {
    let target_str = target.to_str().context("Invalid target path")?;
    let disk_str = disk.to_str().context("Invalid disk path")?;

    // Bind-mount /dev, /proc, /sys
    for src in &["/dev", "/proc", "/sys"] {
        let dst = target.join(src.trim_start_matches('/'));
        fs::create_dir_all(&dst).ok();
        run_cmd("mount", &["--bind", src, dst.to_str().unwrap()])
            .with_context(|| format!("Failed to bind-mount {}", src))?;
    }
    if efi {
        let efivarfs = target.join("sys/firmware/efi/efivars");
        fs::create_dir_all(&efivarfs).ok();
        let _ = run_cmd(
            "mount",
            &["-t", "efivarfs", "efivarfs", efivarfs.to_str().unwrap()],
        );
    }

    // Ensure /boot/grub exists (grub-install needs it to find the device)
    let grub_dir = target.join("boot/grub");
    fs::create_dir_all(&grub_dir).ok();

    let grub_result = if efi {
        run_cmd("chroot", &[
            target_str,
            "grub-install",
            "--target=x86_64-efi",
            "--efi-directory=/boot/efi",
            "--bootloader-id=CoreVM",
            disk_str,
        ])
        .context("Failed to install GRUB (EFI)")
    } else {
        run_cmd("chroot", &[target_str, "grub-install", disk_str])
            .context("Failed to install GRUB (BIOS)")
    };

    let mkconfig_result = run_cmd(
        "chroot",
        &[target_str, "grub-mkconfig", "-o", "/boot/grub/grub.cfg"],
    )
    .context("Failed to generate GRUB config");

    // Always unmount bind mounts
    if efi {
        let efivarfs = target.join("sys/firmware/efi/efivars");
        let _ = run_cmd("umount", &[efivarfs.to_str().unwrap_or("")]);
    }
    for src in &["sys", "proc", "dev"] {
        let dst = target.join(src);
        let _ = run_cmd("umount", &[dst.to_str().unwrap_or("")]);
    }

    grub_result?;
    mkconfig_result?;
    Ok(())
}

pub fn configure_fstab(target: &Path, disk: &Path, efi: bool) -> Result<()> {
    let get_uuid = |part: &str| -> Result<String> {
        let out = run_cmd_output("blkid", &["-s", "UUID", "-o", "value", part])
            .with_context(|| format!("Failed to get UUID for {}", part))?;
        Ok(out.trim().to_string())
    };

    let (root_part, boot_part, efi_opt, swap_part, data_part) = if efi {
        (
            part_path(disk, 4),
            part_path(disk, 2),
            Some(part_path(disk, 1)),
            part_path(disk, 3),
            part_path(disk, 5),
        )
    } else {
        (
            part_path(disk, 3),
            part_path(disk, 1),
            None,
            part_path(disk, 2),
            part_path(disk, 4),
        )
    };

    let root_uuid = get_uuid(&root_part)?;
    let boot_uuid = get_uuid(&boot_part)?;
    let swap_uuid = get_uuid(&swap_part)?;
    let data_uuid = get_uuid(&data_part)?;

    let mut fstab = String::from(
        "# /etc/fstab: static file system information.\n\
         # <file system>  <mount point>  <type>  <options>  <dump>  <pass>\n\n",
    );
    fstab.push_str(&format!(
        "UUID={}  /            ext4  defaults,errors=remount-ro  0  1\n",
        root_uuid
    ));
    fstab.push_str(&format!(
        "UUID={}  /boot        ext4  defaults                   0  2\n",
        boot_uuid
    ));

    if let Some(efi_p) = efi_opt {
        let efi_uuid = get_uuid(&efi_p)?;
        fstab.push_str(&format!(
            "UUID={}  /boot/efi    vfat  umask=0077                 0  1\n",
            efi_uuid
        ));
    }

    fstab.push_str(&format!(
        "UUID={}  none         swap  sw                         0  0\n",
        swap_uuid
    ));
    fstab.push_str(&format!(
        "UUID={}  /var/lib/vmm ext4  defaults                   0  2\n",
        data_uuid
    ));

    let fstab_path = target.join("etc/fstab");
    fs::create_dir_all(fstab_path.parent().unwrap()).ok();
    fs::write(&fstab_path, fstab).context("Failed to write /etc/fstab")
}

fn chpasswd(target: &Path, user: &str, password: &str) -> Result<()> {
    let target_str = target.to_str().context("Invalid target path")?;
    let input = format!("{}:{}", user, password);
    let mut child = Command::new("chroot")
        .args([target_str, "chpasswd"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn chpasswd")?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .context("Failed to write to chpasswd stdin")?;
    }
    let status = child.wait().context("Failed to wait for chpasswd")?;
    if !status.success() {
        bail!("chpasswd failed for user {}", user);
    }
    Ok(())
}

pub fn create_user(target: &Path, username: &str, password: &str, sudo: bool) -> Result<()> {
    let target_str = target.to_str().context("Invalid target path")?;
    run_cmd("chroot", &[target_str, "useradd", "-m", "-s", "/bin/bash", username])
        .with_context(|| format!("Failed to create user {}", username))?;
    chpasswd(target, username, password)?;
    if sudo {
        run_cmd("chroot", &[target_str, "usermod", "-aG", "sudo", username])
            .with_context(|| format!("Failed to add {} to sudo group", username))?;
    }
    Ok(())
}

pub fn set_root_password(target: &Path, password: &str) -> Result<()> {
    chpasswd(target, "root", password)
}

pub fn set_hostname(target: &Path, hostname: &str) -> Result<()> {
    let path = target.join("etc/hostname");
    fs::create_dir_all(path.parent().unwrap()).ok();
    fs::write(&path, format!("{}\n", hostname)).context("Failed to write /etc/hostname")
}

pub fn set_locale(target: &Path, locale: &str) -> Result<()> {
    let target_str = target.to_str().context("Invalid target path")?;

    let locale_default = target.join("etc/default/locale");
    fs::create_dir_all(locale_default.parent().unwrap()).ok();
    fs::write(&locale_default, format!("LANG={}\n", locale))
        .context("Failed to write /etc/default/locale")?;

    let locale_gen = target.join("etc/locale.gen");
    if locale_gen.exists() {
        let content = fs::read_to_string(&locale_gen).context("Failed to read locale.gen")?;
        let updated: String = content
            .lines()
            .map(|line| {
                let stripped = line.trim_start_matches("# ");
                if stripped.starts_with(locale) {
                    stripped.to_string()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&locale_gen, format!("{}\n", updated)).context("Failed to write locale.gen")?;
    }

    run_cmd("chroot", &[target_str, "locale-gen"]).context("Failed to run locale-gen")
}

pub fn configure_chrony(target: &Path, ntp_enabled: bool, ntp_server: &str) -> Result<()> {
    let chrony_dir = target.join("etc/chrony");
    fs::create_dir_all(&chrony_dir).context("Failed to create chrony config dir")?;

    let config = if ntp_enabled {
        format!(
            "# CoreVM chrony configuration\n\
             server {} iburst\n\n\
             driftfile /var/lib/chrony/drift\n\
             makestep 1.0 3\n\
             rtcsync\n\
             logdir /var/log/chrony\n",
            ntp_server
        )
    } else {
        "# CoreVM chrony configuration\n\
         # NTP disabled\n\n\
         driftfile /var/lib/chrony/drift\n\
         makestep 1.0 3\n\
         rtcsync\n\
         logdir /var/log/chrony\n"
            .to_string()
    };

    fs::write(chrony_dir.join("chrony.conf"), config).context("Failed to write chrony.conf")
}

pub fn set_timezone(target: &Path, timezone: &str) -> Result<()> {
    let target_str = target.to_str().context("Invalid target path")?;
    let tz_link = format!("/usr/share/zoneinfo/{}", timezone);
    run_cmd("chroot", &[target_str, "ln", "-sf", &tz_link, "/etc/localtime"])
        .context("Failed to set timezone symlink")?;
    let tz_path = target.join("etc/timezone");
    fs::write(&tz_path, format!("{}\n", timezone)).context("Failed to write /etc/timezone")
}

pub fn is_efi_booted() -> bool {
    Path::new("/sys/firmware/efi").exists()
}

pub fn reboot() -> Result<()> {
    Command::new("reboot")
        .status()
        .context("Failed to execute reboot")?;
    Ok(())
}

pub fn get_ram_bytes() -> u64 {
    let mut sys = System::new_all();
    sys.refresh_memory();
    sys.total_memory()
}
