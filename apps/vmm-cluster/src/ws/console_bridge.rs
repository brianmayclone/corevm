//! Console WebSocket bridge — proxies console connections through the cluster.
//!
//! Client ↔ vmm-cluster ↔ vmm-server (node)
//! Bidirectional message bridging using tokio::select!

use axum::{
    extract::{State, Path, Query, ws::{WebSocket, WebSocketUpgrade, Message}},
    response::Response,
};
use axum::http::StatusCode;
use std::sync::Arc;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use crate::state::ClusterState;
use crate::auth::middleware::AppError;
use crate::auth::jwt;

#[derive(Deserialize)]
pub struct ConsoleQuery {
    pub token: String,
}

/// GET /ws/console/{vm_id}?token=... — WebSocket console bridge.
pub async fn handler(
    State(state): State<Arc<ClusterState>>,
    Path(vm_id): Path<String>,
    Query(q): Query<ConsoleQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    // Validate JWT
    jwt::validate_token(&q.token, &state.jwt_secret)
        .map_err(|e| AppError(StatusCode::UNAUTHORIZED, e))?;

    // Find which host has this VM
    let (host_address, agent_token) = {
        let db = state.db.lock()
            .map_err(|_| AppError(StatusCode::INTERNAL_SERVER_ERROR, "DB lock".into()))?;
        let vm = crate::services::vm::VmService::get(&db, &vm_id)
            .map_err(|e| AppError(StatusCode::NOT_FOUND, e))?;
        let host_id = vm.host_id
            .ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "VM has no host".into()))?;
        let token = crate::services::host::HostService::get_agent_token(&db, &host_id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let host = crate::services::host::HostService::get(&db, &host_id)
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        (host.address, token)
    };

    let vm_id_clone = vm_id.clone();
    Ok(ws.on_upgrade(move |socket| {
        bridge_console(socket, host_address, agent_token, vm_id_clone)
    }))
}

async fn bridge_console(mut client_ws: WebSocket, host_address: String, agent_token: String, vm_id: String) {
    // Connect to the node's WebSocket console
    let ws_url = format!("{}/ws/console/{}?token={}",
        host_address.replace("https://", "wss://").replace("http://", "ws://"),
        vm_id, agent_token);

    let node_ws = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            tracing::error!("Console bridge: Cannot connect to node: {}", e);
            // Send error message to client before closing
            let _ = client_ws.send(Message::Text(
                serde_json::json!({"error": format!("Cannot reach host: {}", e)}).to_string().into()
            )).await;
            let _ = client_ws.close().await;
            return;
        }
    };

    let (mut client_send, mut client_recv) = client_ws.split();
    let (mut node_send, mut node_recv) = node_ws.split();

    // Bridge messages bidirectionally
    tokio::select! {
        // Client → Node
        _ = async {
            while let Some(Ok(msg)) = client_recv.next().await {
                let tungstenite_msg = match msg {
                    Message::Text(t) => tokio_tungstenite::tungstenite::Message::Text(t.to_string().into()),
                    Message::Binary(b) => tokio_tungstenite::tungstenite::Message::Binary(b.to_vec().into()),
                    Message::Ping(p) => tokio_tungstenite::tungstenite::Message::Ping(p.to_vec().into()),
                    Message::Pong(p) => tokio_tungstenite::tungstenite::Message::Pong(p.to_vec().into()),
                    Message::Close(_) => break,
                };
                if node_send.send(tungstenite_msg).await.is_err() { break; }
            }
        } => {}
        // Node → Client
        _ = async {
            while let Some(Ok(msg)) = node_recv.next().await {
                let axum_msg = match msg {
                    tokio_tungstenite::tungstenite::Message::Text(t) => Message::Text(t.to_string().into()),
                    tokio_tungstenite::tungstenite::Message::Binary(b) => Message::Binary(b.to_vec().into()),
                    tokio_tungstenite::tungstenite::Message::Ping(p) => Message::Ping(p.to_vec().into()),
                    tokio_tungstenite::tungstenite::Message::Pong(p) => Message::Pong(p.to_vec().into()),
                    tokio_tungstenite::tungstenite::Message::Close(_) => break,
                    _ => continue,
                };
                if client_send.send(axum_msg).await.is_err() { break; }
            }
        } => {}
    }
}
