//! WebSocket console — streams JPEG framebuffer frames to client,
//! receives keyboard/mouse input events.
//!
//! Protocol:
//!   Server → Client (binary): [0x01][width:u16LE][height:u16LE][jpeg...]
//!   Server → Client (binary): [0x03] = no change (keepalive)
//!   Client → Server (text/JSON): {"type":"key","code":28,"pressed":true}

use axum::{
    extract::{ws::{Message, WebSocket}, Path, State, WebSocketUpgrade, Query},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;

use crate::state::{AppState, VmState};
use crate::auth::jwt;
use crate::vm::framebuffer_encoder;
use crate::vm::input_translator::{self, ConsoleInput};

#[derive(serde::Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

/// GET /ws/console/:vm_id?token=<jwt> — WebSocket upgrade.
pub async fn handler(
    ws: WebSocketUpgrade,
    Path(vm_id): Path<String>,
    Query(query): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Validate JWT from query param
    let token = match query.token {
        Some(t) => t,
        None => {
            tracing::warn!("WebSocket console: missing token for VM {}", vm_id);
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    };
    if let Err(e) = jwt::validate_token(&token, &state.jwt_secret) {
        tracing::warn!("WebSocket console: invalid token for VM {}: {}", vm_id, e);
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }

    // Check VM exists and is running with a framebuffer
    let vm_info = state.vms.get(&vm_id).map(|v| (v.state, v.framebuffer.is_some()));
    match vm_info {
        Some((VmState::Running, true)) => {
            tracing::info!("WebSocket console: upgrading for VM {}", vm_id);
        }
        Some((st, has_fb)) => {
            tracing::warn!("WebSocket console: VM {} not ready (state={:?}, fb={})", vm_id, st, has_fb);
            return axum::http::StatusCode::CONFLICT.into_response();
        }
        None => {
            tracing::warn!("WebSocket console: VM {} not found", vm_id);
            return axum::http::StatusCode::NOT_FOUND.into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, vm_id, state))
        .into_response()
}

async fn handle_socket(socket: WebSocket, vm_id: String, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Get shared framebuffer and VM handle
    let (fb, vm_handle) = {
        let vm = match state.vms.get(&vm_id) {
            Some(v) => v,
            None => return,
        };
        let fb = match &vm.framebuffer {
            Some(f) => f.clone(),
            None => return,
        };
        let handle = vm.vm_handle.unwrap_or(0);
        (fb, handle)
    };

    let fb_for_send = fb.clone();
    let state_for_send = state.clone();
    let vm_id_for_send = vm_id.clone();

    // Frame sender task — sends JPEG frames at ~30fps
    let send_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(33));
        let mut prev_hash: u64 = 0;
        let quality = 65u8;

        loop {
            interval.tick().await;

            // Check VM is still running
            let still_running = state_for_send.vms.get(&vm_id_for_send)
                .map(|v| v.state == VmState::Running)
                .unwrap_or(false);
            if !still_running { break; }

            let frame = {
                let fb_lock = match fb_for_send.lock() {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                let jpeg = framebuffer_encoder::encode_frame(&fb_lock, &mut prev_hash, quality);
                jpeg.map(|data| (fb_lock.width, fb_lock.height, data))
            };

            match frame {
                Some((w, h, jpeg_data)) => {
                    // Binary frame: [0x01][width:u16LE][height:u16LE][jpeg...]
                    let mut msg = Vec::with_capacity(5 + jpeg_data.len());
                    msg.push(0x01);
                    msg.extend_from_slice(&(w as u16).to_le_bytes());
                    msg.extend_from_slice(&(h as u16).to_le_bytes());
                    msg.extend_from_slice(&jpeg_data);
                    if sender.send(Message::Binary(msg.into())).await.is_err() {
                        break;
                    }
                }
                None => {
                    // No change — send keepalive every ~1s (every 30th frame)
                    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                    if COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 30 == 0 {
                        if sender.send(Message::Binary(vec![0x03].into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    // Input receiver task — receives keyboard/mouse events from client
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(input) = serde_json::from_str::<ConsoleInput>(&text.to_string()) {
                        let (fw, fh) = {
                            let fb_lock = fb.lock().unwrap_or_else(|e| e.into_inner());
                            (fb_lock.width, fb_lock.height)
                        };
                        input_translator::inject_input(vm_handle, &input, fw, fh);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    tracing::debug!("Console WebSocket closed for VM {}", vm_id);
}
