//! vmmctl: CoreVM CLI management tool.
//!
//! Remote server administration via REST API — like kubectl for CoreVM.
//! Supports multiple server contexts, JWT authentication, and scriptable output.

mod client;
mod config;
mod auth;
mod output;
mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vmmctl", version, about = "CoreVM CLI management tool")]
pub struct Cli {
    /// Output format
    #[arg(short, long, global = true, default_value = "table")]
    pub output: output::OutputFormat,

    /// Accept self-signed TLS certificates
    #[arg(long, global = true)]
    pub insecure: bool,

    /// Suppress table headers
    #[arg(long, global = true)]
    pub no_header: bool,

    /// Override server URL
    #[arg(short, long, global = true)]
    pub server: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Authenticate with a vmm-server
    Login(commands::login::LoginArgs),
    /// Show authentication status
    Auth(commands::auth_status::AuthStatusArgs),
    /// Manage server connections and contexts
    Config {
        #[command(subcommand)]
        command: commands::config_cmd::ConfigCommands,
    },
    /// Virtual machine management
    Vm {
        #[command(subcommand)]
        command: commands::vm::VmCommands,
    },
    /// System information and stats
    System {
        #[command(subcommand)]
        command: commands::system::SystemCommands,
    },
    /// Storage management (pools, disks, ISOs)
    Storage {
        #[command(subcommand)]
        command: commands::storage::StorageCommands,
    },
    /// Network management (interfaces, bridges)
    Network {
        #[command(subcommand)]
        command: commands::network::NetworkCommands,
    },
    /// User management (admin only)
    User {
        #[command(subcommand)]
        command: commands::user::UserCommands,
    },
    /// Cluster management
    Cluster {
        #[command(subcommand)]
        command: commands::cluster::ClusterCommands,
    },
    /// Control CLI/API access on the server
    ApiAccess {
        #[command(subcommand)]
        command: commands::api_access::ApiAccessCommands,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Login(args) => commands::login::execute(&cli, args).await,
        Commands::Auth(args) => commands::auth_status::execute(&cli, args).await,
        Commands::Config { command } => commands::config_cmd::execute(&cli, command).await,
        Commands::Vm { command } => commands::vm::execute(&cli, command).await,
        Commands::System { command } => commands::system::execute(&cli, command).await,
        Commands::Storage { command } => commands::storage::execute(&cli, command).await,
        Commands::Network { command } => commands::network::execute(&cli, command).await,
        Commands::User { command } => commands::user::execute(&cli, command).await,
        Commands::Cluster { command } => commands::cluster::execute(&cli, command).await,
        Commands::ApiAccess { command } => commands::api_access::execute(&cli, command).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
