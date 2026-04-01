//! Backend stats refresh engine — periodically updates free/total bytes for all backends.

use std::sync::Arc;
use tokio::time::{interval, Duration};
use crate::state::CoreSanState;
use crate::storage::backend;

/// Spawn the backend refresh engine as a background task.
pub fn spawn(state: Arc<CoreSanState>) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            refresh_all_backends(&state);
        }
    });
}

fn refresh_all_backends(state: &CoreSanState) {
    let backends: Vec<(String, String)> = {
        let db = state.db.read();
        let mut stmt = db.prepare(
            "SELECT id, path FROM backends WHERE node_id = ?1 AND status != 'offline'"
        ).unwrap();
        stmt.query_map(
            rusqlite::params![&state.node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap().filter_map(|r| r.ok()).collect()
    };

    let db = state.db.write();
    let now = chrono::Utc::now().to_rfc3339();

    for (id, path) in backends {
        let healthy = backend::is_healthy(&path);
        let (total, free) = backend::refresh_stats(&path);

        let status = if healthy { "online" } else { "degraded" };
        db.execute(
            "UPDATE backends SET total_bytes = ?1, free_bytes = ?2, status = ?3, last_check = ?4
             WHERE id = ?5",
            rusqlite::params![total, free, status, &now, &id],
        ).ok();
    }
}
