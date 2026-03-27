//! vmmctl auth — show authentication status.

use clap::Args;
use crate::Cli;
use crate::config::VmmctlConfig;
use crate::auth;
use crate::output;

#[derive(Args)]
pub struct AuthStatusArgs;

pub async fn execute(cli: &Cli, _args: &AuthStatusArgs) -> Result<(), String> {
    let cfg = VmmctlConfig::load();
    let ctx = cfg.current().ok_or("No context configured")?;
    let token = VmmctlConfig::load_token(&ctx.name);

    match token {
        Some(ref t) => {
            if let Some(claims) = auth::decode_jwt_payload(t) {
                let exp = chrono::DateTime::from_timestamp(claims.exp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "unknown".into());
                let now = chrono::Utc::now().timestamp();
                let expired = claims.exp < now;

                output::println_status("Context", &ctx.name);
                output::println_status("Server", &ctx.server);
                output::println_status("Username", &claims.username);
                output::println_status("Role", &claims.role);
                output::println_status("Token expires", &exp);
                if expired {
                    output::println_status("Status", "EXPIRED — run 'vmmctl login'");
                } else {
                    output::println_status("Status", "Active");
                }
            } else {
                output::println_status("Context", &ctx.name);
                output::println_status("Server", &ctx.server);
                output::println_status("Status", "Token present (cannot decode)");
            }
        }
        None => {
            output::println_status("Context", &ctx.name);
            output::println_status("Server", &ctx.server);
            output::println_status("Status", "Not logged in — run 'vmmctl login'");
        }
    }

    if matches!(cli.output, output::OutputFormat::Json) {
        let json = serde_json::json!({
            "context": ctx.name,
            "server": ctx.server,
            "logged_in": token.is_some(),
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    }

    Ok(())
}
