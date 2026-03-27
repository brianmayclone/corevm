//! vmmctl config — manage server connections and contexts.

use clap::Subcommand;
use crate::Cli;
use crate::config::VmmctlConfig;
use crate::output;

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set or update a server connection
    SetServer {
        /// Server URL (e.g. https://192.168.1.100:8443)
        url: String,
        /// Context name (default: "default")
        #[arg(long, default_value = "default")]
        name: String,
        /// Accept self-signed certificates for this context
        #[arg(long)]
        insecure: bool,
    },
    /// Switch to a different context
    UseContext {
        /// Context name to switch to
        name: String,
    },
    /// List all configured contexts
    ListContexts,
    /// Remove a context
    RemoveContext {
        /// Context name to remove
        name: String,
    },
    /// Show current context details
    Current,
}

pub async fn execute(cli: &Cli, command: &ConfigCommands) -> Result<(), String> {
    match command {
        ConfigCommands::SetServer { url, name, insecure } => {
            let mut cfg = VmmctlConfig::load();
            cfg.set_context(name, url, *insecure);
            if cfg.contexts.len() == 1 {
                cfg.current_context = name.clone();
            }
            cfg.save()?;
            output::print_ok(&format!("Context '{}' set to {}", &name, &url), &cli.output);
        }

        ConfigCommands::UseContext { name } => {
            let mut cfg = VmmctlConfig::load();
            if cfg.get_context(&name).is_none() {
                return Err(format!("Context '{}' not found", name));
            }
            cfg.current_context = name.clone();
            cfg.save()?;
            output::print_ok(&format!("Switched to context '{}'", name), &cli.output);
        }

        ConfigCommands::ListContexts => {
            let cfg = VmmctlConfig::load();
            if cfg.contexts.is_empty() {
                println!("No contexts configured. Run: vmmctl config set-server <url>");
                return Ok(());
            }
            for ctx in &cfg.contexts {
                let current = if ctx.name == cfg.current_context { " *" } else { "" };
                let insecure_flag = if ctx.insecure { " (insecure)" } else { "" };
                println!("{}{}: {}{}", ctx.name, current, ctx.server, insecure_flag);
            }
        }

        ConfigCommands::RemoveContext { name } => {
            let mut cfg = VmmctlConfig::load();
            if cfg.remove_context(&name) {
                cfg.save()?;
                output::print_ok(&format!("Context '{}' removed", name), &cli.output);
            } else {
                return Err(format!("Context '{}' not found", name));
            }
        }

        ConfigCommands::Current => {
            let cfg = VmmctlConfig::load();
            match cfg.current() {
                Some(ctx) => {
                    output::println_status("Context", &ctx.name);
                    output::println_status("Server", &ctx.server);
                    output::println_status("Insecure", &ctx.insecure.to_string());
                    let has_token = VmmctlConfig::load_token(&ctx.name).is_some();
                    output::println_status("Logged in", &has_token.to_string());
                }
                None => println!("No context configured. Run: vmmctl config set-server <url>"),
            }
        }
    }
    Ok(())
}
