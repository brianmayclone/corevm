//! S.M.A.R.T. monitor engine — periodically collects disk health data.
//!
//! Runs every 5 minutes (separate from the fast 5s disk_monitor to avoid
//! blocking hot-plug detection — SMART reads can take seconds per disk).

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::smart;

const SMART_INTERVAL_SECS: u64 = 300; // 5 minutes

pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        // Initial delay — let disk_monitor discover disks first
        tokio::time::sleep(Duration::from_secs(15)).await;

        // Run once immediately
        collect_smart_data(&state).await;

        let mut tick = interval(Duration::from_secs(SMART_INTERVAL_SECS));
        loop {
            tick.tick().await;
            collect_smart_data(&state).await;
        }
    });
}

async fn collect_smart_data(state: &CoreSanState) {
    // Get all device paths from discovered disks
    let device_paths: Vec<String> = {
        let db = state.db.lock().unwrap();
        let disks = crate::storage::disk::discover_disks(&db);
        disks.iter().map(|d| d.device.path.clone()).collect()
    };

    if device_paths.is_empty() {
        return;
    }

    // Read SMART data in a blocking task (smartctl is synchronous I/O)
    let paths = device_paths.clone();
    let smart_results = tokio::task::spawn_blocking(move || {
        smart::read_smart_all(&paths)
    }).await.unwrap_or_default();

    // Store results in DB
    let db = state.db.lock().unwrap();
    let mut ok = 0u32;
    let mut warnings = 0u32;

    for data in &smart_results {
        let health_int: Option<i32> = data.health_passed.map(|b| if b { 1 } else { 0 });

        log_err!(db.execute(
            "INSERT OR REPLACE INTO smart_data
             (device_path, supported, health_passed, transport, power_on_hours, temperature_c,
              reallocated_sectors, pending_sectors, uncorrectable_sectors, wear_leveling_pct,
              media_errors, percentage_used, model, serial, firmware, raw_json, collected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                &data.device_path,
                data.supported as i32,
                health_int,
                &data.transport,
                data.power_on_hours,
                data.temperature_celsius,
                data.reallocated_sectors,
                data.pending_sectors,
                data.uncorrectable_sectors,
                data.wear_leveling_pct,
                data.media_errors,
                data.percentage_used,
                &data.model,
                &data.serial,
                &data.firmware,
                &data.raw_json,
                &data.collected_at,
            ],
        ), "smart_monitor: INSERT smart_data");

        ok += 1;

        if data.has_warning() {
            warnings += 1;
            let msg = format!("SMART {} on {} ({}): health={:?} realloc={:?} pending={:?} temp={:?}°C",
                data.severity(), data.device_path, data.model,
                data.health_passed, data.reallocated_sectors,
                data.pending_sectors, data.temperature_celsius);
            tracing::warn!("{}", msg);

            // Report to cluster (fire-and-forget)
            let cluster_url = state.config.cluster.witness_url.clone();
            let hostname = state.hostname.clone();
            let node_id = state.node_id.clone();
            let sev = if data.severity() == "critical" { "critical" } else { "warning" };
            let msg_clone = msg.clone();
            let dev = data.device_path.clone();
            tokio::spawn(async move {
                if cluster_url.is_empty() { return; }
                let url = format!("{}/api/events/ingest", cluster_url.trim_end_matches('/'));
                let body = serde_json::json!({
                    "severity": sev, "category": "disk", "message": msg_clone,
                    "target_type": "disk", "target_id": dev, "hostname": hostname, "host_id": node_id,
                });
                let _ = reqwest::Client::builder()
                    .danger_accept_invalid_certs(true)
                    .timeout(std::time::Duration::from_secs(5))
                    .build().ok()
                    .map(|c| c.post(&url).json(&body).send());
            });
        }
    }

    if warnings > 0 {
        tracing::warn!("SMART monitor: {} disk(s) with warnings out of {} checked", warnings, ok);
    } else {
        tracing::info!("SMART monitor: {} disk(s) checked, all healthy", ok);
    }
}
