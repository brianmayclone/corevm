//! vmmctl storage — storage pool, disk image, and ISO management.

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum StorageCommands {
    /// Storage pool management
    Pool {
        #[command(subcommand)]
        command: PoolCommands,
    },
    /// Disk image management
    Disk {
        #[command(subcommand)]
        command: DiskCommands,
    },
    /// ISO image management
    Iso {
        #[command(subcommand)]
        command: IsoCommands,
    },
    /// Show aggregate storage statistics
    Stats,
}

#[derive(Subcommand)]
pub enum PoolCommands {
    /// List storage pools
    List,
    /// Create a storage pool
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value = "local")]
        pool_type: String,
    },
    /// Delete a storage pool
    Delete { id: i64 },
}

#[derive(Subcommand)]
pub enum DiskCommands {
    /// List disk images
    List,
    /// Create a disk image
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        size_gb: u64,
        #[arg(long)]
        pool_id: i64,
    },
    /// Delete a disk image
    Delete { id: i64 },
    /// Resize a disk image
    Resize {
        id: i64,
        #[arg(long)]
        size_gb: u64,
    },
}

#[derive(Subcommand)]
pub enum IsoCommands {
    /// List ISO images
    List,
    /// Upload an ISO file
    Upload {
        /// Path to the ISO file
        file: String,
    },
    /// Delete an ISO
    Delete { id: i64 },
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct StoragePool {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub pool_type: String,
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct DiskImage {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub size_gb: u64,
    pub pool_id: i64,
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct IsoImage {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub path: String,
    pub size_bytes: i64,
}

pub async fn execute(cli: &Cli, command: &StorageCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        StorageCommands::Pool { command } => match command {
            PoolCommands::List => {
                let pools: Vec<StoragePool> = client.get("/api/storage/pools").await?;
                output::print_list(&pools, &cli.output, cli.no_header);
            }
            PoolCommands::Create { name, path, pool_type } => {
                let resp: serde_json::Value = client.post("/api/storage/pools", &serde_json::json!({
                    "name": name, "path": path, "pool_type": pool_type
                })).await?;
                output::print_ok(&format!("Pool '{}' created (id: {})", &name,
                    resp.get("id").and_then(|v| v.as_i64()).unwrap_or(0)), &cli.output);
            }
            PoolCommands::Delete { id } => {
                let _: serde_json::Value = client.delete(&format!("/api/storage/pools/{}", &id)).await?;
                output::print_ok(&format!("Pool {} deleted", id), &cli.output);
            }
        },

        StorageCommands::Disk { command } => match command {
            DiskCommands::List => {
                let images: Vec<DiskImage> = client.get("/api/storage/images").await?;
                output::print_list(&images, &cli.output, cli.no_header);
            }
            DiskCommands::Create { name, size_gb, pool_id } => {
                let resp: serde_json::Value = client.post("/api/storage/images", &serde_json::json!({
                    "name": name, "size_gb": size_gb, "pool_id": pool_id
                })).await?;
                output::print_ok(&format!("Disk '{}' created ({})",
                    resp.get("name").and_then(|v| v.as_str()).unwrap_or(name),
                    resp.get("path").and_then(|v| v.as_str()).unwrap_or("")), &cli.output);
            }
            DiskCommands::Delete { id } => {
                let _: serde_json::Value = client.delete(&format!("/api/storage/images/{}", &id)).await?;
                output::print_ok(&format!("Disk {} deleted", &id), &cli.output);
            }
            DiskCommands::Resize { id, size_gb } => {
                let _: serde_json::Value = client.post(&format!("/api/storage/images/{}/resize", &id),
                    &serde_json::json!({"size_gb": size_gb})).await?;
                output::print_ok(&format!("Disk {} resized to {} GB", &id, &size_gb), &cli.output);
            }
        },

        StorageCommands::Iso { command } => match command {
            IsoCommands::List => {
                let isos: Vec<IsoImage> = client.get("/api/storage/isos").await?;
                output::print_list(&isos, &cli.output, cli.no_header);
            }
            IsoCommands::Upload { file } => {
                let resp: serde_json::Value = client.upload_file("/api/storage/isos/upload", file).await?;
                output::print_ok(&format!("ISO uploaded: {} ({} bytes)",
                    resp.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                    resp.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0)), &cli.output);
            }
            IsoCommands::Delete { id } => {
                let _: serde_json::Value = client.delete(&format!("/api/storage/isos/{}", &id)).await?;
                output::print_ok(&format!("ISO {} deleted", &id), &cli.output);
            }
        },

        StorageCommands::Stats => {
            let stats: serde_json::Value = client.get("/api/storage/stats").await?;
            output::print_item(&stats, &cli.output);
        }
    }

    Ok(())
}
