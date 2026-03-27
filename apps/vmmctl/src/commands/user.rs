//! vmmctl user — user management (admin only).

use clap::Subcommand;
use serde::{Serialize, Deserialize};
use tabled::Tabled;
use crate::Cli;
use crate::client::ApiClient;
use crate::output;

#[derive(Subcommand)]
pub enum UserCommands {
    /// List all users
    List,
    /// Create a new user
    Create {
        /// Username
        #[arg(long)]
        username: String,
        /// Password (omit to prompt)
        #[arg(long)]
        password: Option<String>,
        /// Role: admin, operator, viewer
        #[arg(long, default_value = "operator")]
        role: String,
    },
    /// Delete a user
    Delete {
        /// User ID
        id: i64,
    },
    /// Change a user's password
    Password {
        /// User ID
        id: i64,
    },
}

#[derive(Debug, Deserialize, Serialize, Tabled)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
}

pub async fn execute(cli: &Cli, command: &UserCommands) -> Result<(), String> {
    let client = ApiClient::from_cli(cli)?;

    match command {
        UserCommands::List => {
            let users: Vec<User> = client.get("/api/users").await?;
            output::print_list(&users, &cli.output, cli.no_header);
        }

        UserCommands::Create { username, password, role } => {
            let pass = match password {
                Some(p) => p.clone(),
                None => rpassword::prompt_password("Password: ").map_err(|e| e.to_string())?,
            };
            let resp: serde_json::Value = client.post("/api/users", &serde_json::json!({
                "username": username, "password": pass, "role": role
            })).await?;
            output::print_ok(&format!("User '{}' created (id: {}, role: {})", &username,
                resp.get("id").and_then(|v| v.as_i64()).unwrap_or(0), &role), &cli.output);
        }

        UserCommands::Delete { id } => {
            let _: serde_json::Value = client.delete(&format!("/api/users/{}", &id)).await?;
            output::print_ok(&format!("User {} deleted", &id), &cli.output);
        }

        UserCommands::Password { id } => {
            let password = rpassword::prompt_password("New password: ").map_err(|e| e.to_string())?;
            let confirm = rpassword::prompt_password("Confirm password: ").map_err(|e| e.to_string())?;
            if password != confirm {
                return Err("Passwords do not match".into());
            }
            let _: serde_json::Value = client.put(&format!("/api/users/{}/password", &id),
                &serde_json::json!({"password": password})).await?;
            output::print_ok(&format!("Password for user {} updated", &id), &cli.output);
        }
    }

    Ok(())
}
