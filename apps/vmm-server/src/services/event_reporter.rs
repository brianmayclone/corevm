//! Event reporter — sends events to vmm-cluster for centralized logging.
//!
//! Used by vmm-server to report VM lifecycle events, hardware issues,
//! and system warnings to the cluster's event ingress.
//!
//! Categories: "vm", "server", "disk", "network"

use crate::state::AppState;

/// Fire-and-forget event report to the cluster.
/// Does nothing if the server is not managed by a cluster.
pub fn report(state: &AppState, severity: &str, category: &str, message: &str, target_type: Option<&str>, target_id: Option<&str>) {
    let cluster_url = {
        let managed = state.managed_config.lock().unwrap();
        match managed.as_ref() {
            Some(cfg) => cfg.cluster_url.clone(),
            None => return, // Not managed by a cluster
        }
    };

    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let sev = severity.to_string();
    let cat = category.to_string();
    let msg = message.to_string();
    let tt = target_type.map(|s| s.to_string());
    let ti = target_id.map(|s| s.to_string());

    tokio::spawn(async move {
        let url = format!("{}/api/events/ingest", cluster_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "severity": sev,
            "category": cat,
            "message": msg,
            "target_type": tt,
            "target_id": ti,
            "hostname": hostname,
        });

        let _ = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok()
            .and_then(|c| Some(c.post(&url).json(&body).send()));
    });
}

// Convenience functions
pub fn vm_event(state: &AppState, severity: &str, message: &str, vm_id: &str) {
    report(state, severity, "vm", message, Some("vm"), Some(vm_id));
}

pub fn server_event(state: &AppState, severity: &str, message: &str) {
    report(state, severity, "server", message, Some("host"), None);
}
