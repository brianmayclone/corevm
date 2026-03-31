mod config;
mod auth;
mod s3;
mod socket;

use config::S3GwConfig;
use std::sync::Arc;

pub struct AppState {
    pub config: S3GwConfig,
    pub socket: socket::SocketPool,
}

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .skip_while(|a| a != "--config")
        .nth(1)
        .unwrap_or_else(|| "/etc/vmm/s3gw.toml".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = S3GwConfig::load(std::path::Path::new(&config_path))
        .unwrap_or_else(|e| {
            eprintln!("Config error: {}", e);
            std::process::exit(1);
        });

    tracing::info!("vmm-s3gw starting on {}", config.server.listen);

    let listen_addr = config.server.listen.clone();
    let socket_pool = socket::SocketPool::new(&config.san);
    let state = Arc::new(AppState {
        config,
        socket: socket_pool,
    });

    let app = s3::router(state);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind {}: {}", listen_addr, e);
            std::process::exit(1);
        });

    tracing::info!("Listening on {}", listen_addr);
    axum::serve(listener, app).await.unwrap();
}
