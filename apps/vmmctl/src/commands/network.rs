//! vmmctl network — network interface and bridge management.

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum NetworkCommands {
    /// List host network interfaces
    List,
    /// Show network statistics
    Stats,
    /// Bridge management
    Bridge {
        #[command(subcommand)]
        command: BridgeCommands,
    },
}

#[derive(Subcommand)]
pub enum BridgeCommands {
    /// List bridges
    List,
    /// Create a bridge
    Create {
        /// Bridge name
        #[arg(long)]
        name: String,
        /// Network ID (for VXLAN)
        #[arg(long, default_value = "")]
        network_id: String,
    },
    /// Delete a bridge
    Delete {
        /// Bridge name
        name: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct NetworkInterface {
    pub name: String,
    pub kind: String,
    pub mac: String,
    #[tabled(display_with = "display_opt")]
    pub ipv4: Option<String>,
    pub mtu: u32,
    pub state: String,
}

fn display_opt(opt: &Option<String>) -> String {
    opt.as_deref().unwrap_or("-").to_string()
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct BridgeInfo {
    pub name: String,
    pub state: String,
    #[tabled(display_with = "display_members")]
    pub members: Vec<String>,
}

fn display_members(members: &Vec<String>) -> String {
    if members.is_empty() { "-".into() } else { members.join(", ") }
}

pub async fn execute(cli: &Cli, command: &NetworkCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        NetworkCommands::List => {
            let ifaces: Vec<NetworkInterface> = client.get("/api/network/interfaces").await?;
            output::print_list(&ifaces, &cli.output, cli.no_header);
        }

        NetworkCommands::Stats => {
            let stats: serde_json::Value = client.get("/api/network/stats").await?;
            output::print_item(&stats, &cli.output);
        }

        NetworkCommands::Bridge { command } => match command {
            BridgeCommands::List => {
                let bridges: Vec<BridgeInfo> = client.get("/api/network/bridges").await?;
                output::print_list(&bridges, &cli.output, cli.no_header);
            }
            BridgeCommands::Create { name, network_id } => {
                let net_id = if network_id.is_empty() { name } else { network_id };
                let _: serde_json::Value = client.post("/api/network/bridges", &serde_json::json!({
                    "bridge_name": name,
                    "network_id": net_id,
                })).await?;
                output::print_ok(&format!("Bridge '{}' created", name), &cli.output);
            }
            BridgeCommands::Delete { name } => {
                let _: serde_json::Value = client.delete(&format!("/api/network/bridges/{}", name)).await?;
                output::print_ok(&format!("Bridge '{}' deleted", name), &cli.output);
            }
        },
    }

    Ok(())
}
