//! vmmctl cluster — cluster management commands.

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum ClusterCommands {
    /// Host management
    Host {
        #[command(subcommand)]
        command: HostCommands,
    },
    /// DRS (Distributed Resource Scheduler) management
    Drs {
        #[command(subcommand)]
        command: DrsCommands,
    },
    /// Migrate a VM to another host
    Migrate {
        /// VM ID
        #[arg(long)]
        vm: String,
        /// Target host ID
        #[arg(long)]
        to: String,
    },
}

#[derive(Subcommand)]
pub enum HostCommands {
    /// List cluster hosts
    List,
    /// Add a host to the cluster
    Add {
        /// Host URL (e.g. https://192.168.1.10:8443)
        url: String,
        /// Host display name
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove a host from the cluster
    Remove {
        /// Host ID
        id: String,
    },
    /// Enable/disable maintenance mode
    Maintenance {
        /// Host ID
        id: String,
        /// Enable or disable
        #[arg(long)]
        enable: bool,
    },
}

#[derive(Subcommand)]
pub enum DrsCommands {
    /// Show DRS status and recommendations
    Status,
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct ClusterHost {
    pub id: String,
    pub name: String,
    pub url: String,
    pub status: String,
    #[serde(default)]
    pub maintenance: bool,
    #[serde(default)]
    pub vm_count: u32,
}

pub async fn execute(cli: &Cli, command: &ClusterCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        ClusterCommands::Host { command } => match command {
            HostCommands::List => {
                let hosts: Vec<ClusterHost> = client.get("/api/hosts").await?;
                output::print_list(&hosts, &cli.output, cli.no_header);
            }
            HostCommands::Add { url, name } => {
                let host_name = name.as_deref().unwrap_or(url.as_str());
                let resp: serde_json::Value = client.post("/api/hosts", &serde_json::json!({
                    "url": url,
                    "name": host_name,
                })).await?;
                output::print_ok(&format!("Host added (id: {})",
                    resp.get("id").and_then(|v| v.as_str()).unwrap_or("?")), &cli.output);
            }
            HostCommands::Remove { id } => {
                let _: serde_json::Value = client.delete(&format!("/api/hosts/{}", id)).await?;
                output::print_ok(&format!("Host {} removed", id), &cli.output);
            }
            HostCommands::Maintenance { id, enable } => {
                let _: serde_json::Value = client.put(&format!("/api/hosts/{}/maintenance", &id),
                    &serde_json::json!({"maintenance": enable})).await?;
                let action = if *enable { "enabled" } else { "disabled" };
                output::print_ok(&format!("Maintenance mode {} for host {}", action, id), &cli.output);
            }
        },

        ClusterCommands::Drs { command } => match command {
            DrsCommands::Status => {
                let status: serde_json::Value = client.get("/api/drs/recommendations").await?;
                output::print_item(&status, &cli.output);
            }
        },

        ClusterCommands::Migrate { vm, to } => {
            let _: serde_json::Value = client.post("/api/migrate", &serde_json::json!({
                "vm_id": vm,
                "target_host_id": to,
            })).await?;
            output::print_ok(&format!("Migration of VM {} to host {} initiated", vm, to), &cli.output);
        }
    }

    Ok(())
}
