mod config;
mod socket;
pub mod pdu;
pub mod session;
pub mod scsi;
pub mod alua;
pub mod discovery;

use config::IscsiConfig;
use std::sync::Arc;

pub struct AppState {
    pub config: IscsiConfig,
    pub socket: socket::SocketPool,
}

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/iscsi.toml".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = IscsiConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| { eprintln!("Config error: {}", e); std::process::exit(1); });

    tracing::info!("vmm-iscsi starting on {}", config.server.listen);

    let listen_addr = config.server.listen.clone();
    let socket_pool = socket::SocketPool::new(&config.san);
    let state = Arc::new(AppState { config, socket: socket_pool });

    let listener = tokio::net::TcpListener::bind(&listen_addr).await
        .unwrap_or_else(|e| { eprintln!("Cannot bind {}: {}", listen_addr, e); std::process::exit(1); });

    tracing::info!("iSCSI target listening on {}", listen_addr);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tracing::debug!("iSCSI connection from {}", addr);
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = session::handle_connection(stream, state).await {
                        tracing::debug!("iSCSI session ended: {}", e);
                    }
                });
            }
            Err(e) => tracing::error!("Accept error: {}", e),
        }
    }
}
