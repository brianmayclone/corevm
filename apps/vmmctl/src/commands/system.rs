//! vmmctl system — system information and statistics.

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum SystemCommands {
    /// Show server information
    Info,
    /// Show system statistics
    Stats,
    /// Show recent activity / audit log
    Activity {
        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub hostname: String,
    pub hw_virtualization: bool,
    pub cpu_count: usize,
    pub total_ram_mb: u64,
    pub free_ram_mb: u64,
    pub total_disk_bytes: u64,
    pub free_disk_bytes: u64,
    pub mode: String,
    pub cluster_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DashboardStats {
    pub total_vms: usize,
    pub running_vms: usize,
    pub stopped_vms: usize,
    pub cpu_count: usize,
    pub total_ram_mb: u64,
    pub used_ram_mb: u64,
    pub total_disk_bytes: u64,
    pub used_disk_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct AuditEntry {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub resource_type: String,
    #[serde(default)]
    pub resource_id: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub created_at: String,
}

pub async fn execute(cli: &Cli, command: &SystemCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        SystemCommands::Info => {
            let info: SystemInfo = client.get("/api/system/info").await?;
            output::print_item(&info, &cli.output);
        }

        SystemCommands::Stats => {
            let stats: DashboardStats = client.get("/api/system/stats").await?;
            if matches!(cli.output, output::OutputFormat::Json) {
                output::print_item(&stats, &cli.output);
            } else {
                output::println_status("Total VMs", &stats.total_vms.to_string());
                output::println_status("Running", &stats.running_vms.to_string());
                output::println_status("Stopped", &stats.stopped_vms.to_string());
                output::println_status("CPUs", &stats.cpu_count.to_string());
                output::println_status("RAM total", &format!("{} MB", stats.total_ram_mb));
                output::println_status("RAM used", &format!("{} MB", stats.used_ram_mb));
                let disk_total_gb = stats.total_disk_bytes / (1024 * 1024 * 1024);
                let disk_used_gb = stats.used_disk_bytes / (1024 * 1024 * 1024);
                output::println_status("Disk total", &format!("{} GB", disk_total_gb));
                output::println_status("Disk used", &format!("{} GB", disk_used_gb));
            }
        }

        SystemCommands::Activity { limit } => {
            let entries: Vec<AuditEntry> = client.get(&format!("/api/system/activity?limit={}", limit)).await?;
            output::print_list(&entries, &cli.output, cli.no_header);
        }
    }

    Ok(())
}
