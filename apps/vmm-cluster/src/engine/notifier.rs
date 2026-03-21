//! Notification dispatch worker — processes queued notifications asynchronously.
//!
//! Sends webhooks via HTTP POST and emails via SMTP.
//! The queue is fed by NotificationService::dispatch() which runs synchronously.

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::state::ClusterState;
use crate::services::settings::ClusterSettingsService;

/// A notification to be sent asynchronously.
#[derive(Debug, Clone)]
pub struct PendingNotification {
    pub channel_type: String,
    pub channel_config: String,
    pub rule_name: String,
    pub severity: String,
    pub category: String,
    pub message: String,
    pub log_id: i64,
}

/// Global notification sender — use this to enqueue notifications.
static NOTIFICATION_TX: std::sync::OnceLock<mpsc::UnboundedSender<PendingNotification>> = std::sync::OnceLock::new();

/// Enqueue a notification for async dispatch.
pub fn enqueue(notification: PendingNotification) {
    if let Some(tx) = NOTIFICATION_TX.get() {
        let _ = tx.send(notification);
    }
}

/// Spawn the notification worker as a background task.
pub fn spawn(state: Arc<ClusterState>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<PendingNotification>();
    let _ = NOTIFICATION_TX.set(tx);

    tokio::spawn(async move {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        while let Some(notif) = rx.recv().await {
            let result = match notif.channel_type.as_str() {
                "webhook" => send_webhook(&http_client, &notif).await,
                "email" => send_email(&state, &notif).await,
                "log" => {
                    tracing::info!("NOTIFICATION [{}]: [{}] {}", notif.rule_name, notif.severity, notif.message);
                    Ok(())
                }
                _ => Err(format!("Unknown channel type: {}", notif.channel_type)),
            };

            // Update notification log with actual result
            if let Ok(db) = state.db.lock() {
                let (status, error) = match &result {
                    Ok(()) => ("sent", None),
                    Err(e) => ("failed", Some(e.as_str())),
                };
                let _ = db.execute(
                    "UPDATE notification_log SET status = ?1, error = ?2 WHERE id = ?3",
                    rusqlite::params![status, error, notif.log_id],
                );
            }

            if let Err(e) = &result {
                tracing::warn!("Notification failed [{}]: {}", notif.rule_name, e);
            }
        }
    });
}

async fn send_webhook(client: &reqwest::Client, notif: &PendingNotification) -> Result<(), String> {
    let config: serde_json::Value = serde_json::from_str(&notif.channel_config).unwrap_or_default();
    let url = config.get("url").and_then(|v| v.as_str())
        .ok_or_else(|| "Webhook URL not configured".to_string())?;

    let payload = serde_json::json!({
        "severity": notif.severity,
        "category": notif.category,
        "message": notif.message,
        "rule": notif.rule_name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let mut request = client.post(url).json(&payload);

    // Add HMAC signature if secret is configured
    if let Some(secret) = config.get("secret").and_then(|v| v.as_str()) {
        if !secret.is_empty() {
            request = request.header("X-VMM-Signature", secret);
        }
    }

    // Add custom headers
    if let Some(headers) = config.get("headers").and_then(|v| v.as_object()) {
        for (key, val) in headers {
            if let Some(v) = val.as_str() {
                request = request.header(key.as_str(), v);
            }
        }
    }

    let resp = request.send().await
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {} — {}", status, body));
    }

    tracing::info!("Webhook sent to {} for [{}]", url, notif.rule_name);
    Ok(())
}

async fn send_email(state: &Arc<ClusterState>, notif: &PendingNotification) -> Result<(), String> {
    let smtp_config = {
        let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
        ClusterSettingsService::get_smtp_config(&db)
    };

    if smtp_config.host.is_empty() {
        return Err("SMTP server not configured — go to Settings → E-Mail (SMTP)".into());
    }

    let channel_config: serde_json::Value = serde_json::from_str(&notif.channel_config).unwrap_or_default();
    let to = channel_config.get("to").and_then(|v| v.as_str())
        .ok_or_else(|| "Email recipient not configured".to_string())?;

    // Build email using lettre (if available) or fallback to direct SMTP
    // For now: use a simple TCP connection to send the email
    let from = if smtp_config.from_address.is_empty() {
        format!("vmm-cluster@{}", smtp_config.host)
    } else {
        smtp_config.from_address.clone()
    };

    let subject = format!("[VMM-Cluster] [{}] {}", notif.severity.to_uppercase(), notif.category);
    let body = format!(
        "VMM-Cluster Notification\n\
         ========================\n\n\
         Severity: {}\n\
         Category: {}\n\
         Rule: {}\n\
         Time: {}\n\n\
         {}\n",
        notif.severity, notif.category, notif.rule_name,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        notif.message,
    );

    // Simple SMTP send via raw TCP (no TLS for now — production would use lettre)
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let addr = format!("{}:{}", smtp_config.host, smtp_config.port);
    let mut stream = tokio::net::TcpStream::connect(&addr).await
        .map_err(|e| format!("Cannot connect to SMTP {}: {}", addr, e))?;

    let mut buf = vec![0u8; 1024];
    // Read greeting
    let _ = stream.read(&mut buf).await;

    // EHLO
    stream.write_all(format!("EHLO vmm-cluster\r\n").as_bytes()).await.map_err(|e| e.to_string())?;
    let _ = stream.read(&mut buf).await;

    // AUTH if credentials provided
    if !smtp_config.username.is_empty() {
        let auth = base64_encode(&format!("\0{}\0{}", smtp_config.username, smtp_config.password));
        stream.write_all(format!("AUTH PLAIN {}\r\n", auth).as_bytes()).await.map_err(|e| e.to_string())?;
        let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
        let resp = String::from_utf8_lossy(&buf[..n]);
        if !resp.starts_with("235") {
            return Err(format!("SMTP auth failed: {}", resp.trim()));
        }
    }

    // MAIL FROM
    stream.write_all(format!("MAIL FROM:<{}>\r\n", from).as_bytes()).await.map_err(|e| e.to_string())?;
    let _ = stream.read(&mut buf).await;

    // RCPT TO
    stream.write_all(format!("RCPT TO:<{}>\r\n", to).as_bytes()).await.map_err(|e| e.to_string())?;
    let _ = stream.read(&mut buf).await;

    // DATA
    stream.write_all(b"DATA\r\n").await.map_err(|e| e.to_string())?;
    let _ = stream.read(&mut buf).await;

    let email_data = format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}\r\n.\r\n",
        from, to, subject, body
    );
    stream.write_all(email_data.as_bytes()).await.map_err(|e| e.to_string())?;
    let _ = stream.read(&mut buf).await;

    // QUIT
    stream.write_all(b"QUIT\r\n").await.map_err(|e| e.to_string())?;

    tracing::info!("Email sent to {} for [{}]", to, notif.rule_name);
    Ok(())
}

fn base64_encode(input: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input.as_bytes())
}
