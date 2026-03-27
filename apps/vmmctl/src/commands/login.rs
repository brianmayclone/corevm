//! vmmctl login — authenticate with a vmm-server.

use clap::Args;
use crate::Cli;
use crate::client::ApiClient;
use crate::config::VmmctlConfig;
use crate::auth::{LoginRequest, LoginResponse};

#[derive(Args)]
pub struct LoginArgs {
    /// Username
    #[arg(short, long)]
    pub username: Option<String>,

    /// Read password from stdin (for scripts)
    #[arg(long)]
    pub password_stdin: bool,
}

pub async fn execute(cli: &Cli, args: &LoginArgs) -> Result<(), String> {
    let cfg = VmmctlConfig::load();
    let server = cfg.resolve_server(cli.server.as_deref())?;
    let insecure = cfg.resolve_insecure(cli.insecure);

    let username = match &args.username {
        Some(u) => u.clone(),
        None => {
            eprint!("Username: ");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
            input.trim().to_string()
        }
    };

    let password = if args.password_stdin {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
        input.trim().to_string()
    } else {
        rpassword::prompt_password("Password: ").map_err(|e| e.to_string())?
    };

    let client = ApiClient::unauthenticated(&server, insecure)?;
    let resp: LoginResponse = client.post("/api/auth/login", &LoginRequest { username, password }).await?;

    // Store the token
    let context_name = cfg.current().map(|c| c.name.clone()).unwrap_or_else(|| "default".into());
    VmmctlConfig::store_token(&context_name, &resp.access_token)?;

    println!("Logged in as {} (role: {})", resp.user.username, resp.user.role);
    println!("Server: {}", server);
    Ok(())
}
