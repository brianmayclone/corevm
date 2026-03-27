//! vmmctl vm — virtual machine management.

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum VmCommands {
    /// List all VMs
    List,
    /// Show VM details
    Info {
        /// VM ID or name
        id: String,
    },
    /// Create a new VM
    Create {
        /// VM name
        #[arg(long)]
        name: String,
        /// RAM in MB
        #[arg(long, default_value = "2048")]
        ram: u32,
        /// Number of CPU cores
        #[arg(long, default_value = "2")]
        cpus: u32,
        /// Disk image path
        #[arg(long)]
        disk: Option<String>,
        /// ISO image path
        #[arg(long)]
        iso: Option<String>,
        /// Guest OS type
        #[arg(long, default_value = "other")]
        os: String,
        /// BIOS type (seabios, uefi, corevm)
        #[arg(long, default_value = "seabios")]
        bios: String,
        /// Network mode (usermode, bridge, disconnected)
        #[arg(long, default_value = "usermode")]
        net: String,
    },
    /// Start a VM
    Start {
        /// VM ID or name
        id: String,
    },
    /// Stop a VM (graceful shutdown)
    Stop {
        /// VM ID or name
        id: String,
    },
    /// Force-stop a VM
    ForceStop {
        /// VM ID or name
        id: String,
    },
    /// Delete a VM
    Delete {
        /// VM ID or name
        id: String,
    },
    /// Take a screenshot of a running VM
    Screenshot {
        /// VM ID or name
        id: String,
        /// Output file path
        #[arg(short, long, default_value = "screenshot.png")]
        file: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct VmSummary {
    pub id: String,
    pub name: String,
    pub state: String,
    pub guest_os: String,
    pub ram_mb: u32,
    pub cpu_cores: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VmDetail {
    pub id: String,
    pub name: String,
    pub state: String,
    pub config: serde_json::Value,
    pub owner_id: i64,
    pub resource_group_id: i64,
    pub created_at: String,
    #[serde(default)]
    pub disks: Vec<DiskInfo>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiskInfo {
    pub path: String,
    pub size_bytes: u64,
    pub used_bytes: u64,
}

pub async fn execute(cli: &Cli, command: &VmCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        VmCommands::List => {
            let vms: Vec<VmSummary> = client.get("/api/vms").await?;
            output::print_list(&vms, &cli.output, cli.no_header);
        }

        VmCommands::Info { id } => {
            let vm: VmDetail = client.get(&format!("/api/vms/{}", id)).await?;
            output::print_item(&vm, &cli.output);
        }

        VmCommands::Create { name, ram, cpus, disk, iso, os, bios, net } => {
            let mut disk_images: Vec<String> = Vec::new();
            if let Some(d) = disk {
                disk_images.push(d.clone());
            }

            let config = serde_json::json!({
                "uuid": "",
                "name": name,
                "guest_os": os,
                "guest_arch": "x64",
                "ram_mb": ram,
                "cpu_cores": cpus,
                "disk_images": disk_images,
                "iso_image": iso.as_deref().unwrap_or(""),
                "boot_order": if iso.is_some() { "cd" } else { "disk" },
                "bios_type": bios,
                "gpu_model": "stdvga",
                "vram_mb": 16,
                "nic_model": "e1000",
                "net_enabled": net != "disconnected",
                "net_mode": net,
                "net_host_nic": "",
                "mac_mode": "dynamic",
                "mac_address": "",
                "audio_enabled": true,
                "usb_tablet": false,
                "ram_alloc": "ondemand",
                "diagnostics": false,
                "disk_cache_mb": 0,
                "disk_cache_mode": "none",
            });

            let resp: serde_json::Value = client.post("/api/vms", &config).await?;
            let vm_id = resp.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let vm_name = resp.get("name").and_then(|v| v.as_str()).unwrap_or(name);
            output::print_ok(&format!("VM '{}' created (id: {})", vm_name, vm_id), &cli.output);
        }

        VmCommands::Start { id } => {
            let _: serde_json::Value = client.post_empty(&format!("/api/vms/{}/start", id)).await?;
            output::print_ok(&format!("VM '{}' started", id), &cli.output);
        }

        VmCommands::Stop { id } => {
            let _: serde_json::Value = client.post_empty(&format!("/api/vms/{}/stop", id)).await?;
            output::print_ok(&format!("VM '{}' stop requested", id), &cli.output);
        }

        VmCommands::ForceStop { id } => {
            let _: serde_json::Value = client.post_empty(&format!("/api/vms/{}/force-stop", id)).await?;
            output::print_ok(&format!("VM '{}' force-stopped", id), &cli.output);
        }

        VmCommands::Delete { id } => {
            let _: serde_json::Value = client.delete(&format!("/api/vms/{}", id)).await?;
            output::print_ok(&format!("VM '{}' deleted", id), &cli.output);
        }

        VmCommands::Screenshot { id, file } => {
            let data = client.get_bytes(&format!("/api/vms/{}/screenshot", id)).await?;
            std::fs::write(file, &data).map_err(|e| format!("Write error: {}", e))?;
            output::print_ok(&format!("Screenshot saved to '{}' ({} bytes)", file, data.len()), &cli.output);
        }
    }

    Ok(())
}
