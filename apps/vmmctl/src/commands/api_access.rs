//! vmmctl api-access — control CLI/API access on the server.

use clap::Subcommand;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum ApiAccessCommands {
    /// Show CLI/API access status
    Status,
    /// Enable CLI/API access
    Enable,
    /// Disable CLI/API access
    Disable,
}

pub async fn execute(cli: &Cli, command: &ApiAccessCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        ApiAccessCommands::Status => {
            let resp: serde_json::Value = client.get("/api/settings/api-access").await?;
            if matches!(cli.output, output::OutputFormat::Json) {
                output::print_item(&resp, &cli.output);
            } else {
                let enabled = resp.get("cli_access_enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                output::println_status("CLI/API access", if enabled { "enabled" } else { "disabled" });
                if let Some(ips) = resp.get("allowed_ips").and_then(|v| v.as_array()) {
                    if !ips.is_empty() {
                        let ip_list: Vec<&str> = ips.iter().filter_map(|v| v.as_str()).collect();
                        output::println_status("Allowed IPs", &ip_list.join(", "));
                    }
                }
            }
        }
        ApiAccessCommands::Enable => {
            let _: serde_json::Value = client.put("/api/settings/api-access",
                &serde_json::json!({"cli_access_enabled": true})).await?;
            output::print_ok("CLI/API access enabled", &cli.output);
        }
        ApiAccessCommands::Disable => {
            let _: serde_json::Value = client.put("/api/settings/api-access",
                &serde_json::json!({"cli_access_enabled": false})).await?;
            output::print_ok("CLI/API access disabled", &cli.output);
        }
    }

    Ok(())
}
