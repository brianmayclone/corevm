//! Event reporter — sends events to the vmm-cluster for centralized logging.
//!
//! vmm-san detects disk failures, replication issues, quorum changes, etc.
//! This module provides a simple API to push these events to the cluster's
//! event ingress endpoint so they appear in the central event log and
//! trigger notifications/alarms.

use std::sync::Arc;
use crate::state::CoreSanState;

/// Report a single event to the cluster.
/// Falls back silently if no cluster is configured or unreachable.
pub async fn report_event(
    state: &CoreSanState,
    severity: &str,
    category: &str,
    message: &str,
    target_type: Option<&str>,
    target_id: Option<&str>,
) {
    // Find the cluster witness URL or any peer that might be the cluster
    let cluster_url = &state.config.cluster.witness_url;
    if cluster_url.is_empty() {
        return; // No cluster configured
    }

    let body = serde_json::json!({
        "severity": severity,
        "category": category,
        "message": message,
        "target_type": target_type,
        "target_id": target_id,
        "hostname": state.hostname,
        "host_id": state.node_id,
    });

    let url = format!("{}/api/events/ingest", cluster_url.trim_end_matches('/'));

    // Non-blocking, fire-and-forget — don't block the caller
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return,
    };

    match client.post(&url).json(&body).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::debug!("Event report to cluster returned {}", resp.status());
            }
        }
        Err(e) => {
            tracing::debug!("Event report to cluster failed: {}", e);
        }
    }
}

/// Convenience: report a critical event.
pub async fn critical(state: &CoreSanState, category: &str, message: &str, target_type: Option<&str>, target_id: Option<&str>) {
    report_event(state, "critical", category, message, target_type, target_id).await;
}

/// Convenience: report a warning event.
pub async fn warning(state: &CoreSanState, category: &str, message: &str, target_type: Option<&str>, target_id: Option<&str>) {
    report_event(state, "warning", category, message, target_type, target_id).await;
}

/// Convenience: report an info event.
pub async fn info(state: &CoreSanState, category: &str, message: &str, target_type: Option<&str>, target_id: Option<&str>) {
    report_event(state, "info", category, message, target_type, target_id).await;
}
