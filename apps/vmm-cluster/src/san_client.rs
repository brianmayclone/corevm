//! HTTP client for proxying requests to vmm-san (CoreSAN) instances.
//!
//! The cluster uses this to forward vSAN operations to the correct SAN host.
//! Modeled after `node_client` but targets vmm-san's REST API (port 7443).

use reqwest::Client;
use rusqlite::Connection;
use serde_json::Value;
use std::time::Duration;

/// Client for a single vmm-san instance.
pub struct SanClient {
    http: Client,
    base_url: String,
}

/// Minimal SAN host info from the hosts table.
pub struct SanHost {
    pub host_id: String,
    pub hostname: String,
    pub san_address: String,
    pub san_node_id: String,
}

impl SanClient {
    pub fn new(san_address: &str) -> Self {
        let http = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build SAN HTTP client");

        Self {
            http,
            base_url: san_address.trim_end_matches('/').to_string(),
        }
    }

    // ── Generic raw methods ─────────────────────────────────────

    pub async fn raw_get(&self, path: &str) -> Result<Value, String> {
        self.get(path).await
    }

    pub async fn raw_post(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.post(path, body).await
    }

    pub async fn raw_post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.post(path, body).await
    }

    // ── Status ────────────────────────────────────────────────────

    pub async fn get_status(&self) -> Result<Value, String> {
        self.get("/api/status").await
    }

    // ── Network Configuration ────────────────────────────────────

    /// Push SAN network config from viSwitch uplink assignments.
    pub async fn update_network(&self, interfaces: &[String], teaming: &str, mtu: u32) -> Result<Value, String> {
        let body = serde_json::json!({
            "interfaces": interfaces,
            "teaming": teaming,
            "mtu": mtu,
        });
        self.put("/api/network/config", &body).await
    }

    // ── Volumes ───────────────────────────────────────────────────

    pub async fn list_volumes(&self) -> Result<Value, String> {
        self.get("/api/volumes").await
    }

    pub async fn create_volume(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/volumes", body).await
    }

    pub async fn get_volume(&self, id: &str) -> Result<Value, String> {
        self.get(&format!("/api/volumes/{}", id)).await
    }

    pub async fn update_volume(&self, id: &str, body: &Value) -> Result<Value, String> {
        self.put(&format!("/api/volumes/{}", id), body).await
    }

    pub async fn delete_volume(&self, id: &str) -> Result<Value, String> {
        self.delete(&format!("/api/volumes/{}", id)).await
    }

    // ── Backends ──────────────────────────────────────────────────

    pub async fn list_backends(&self, volume_id: &str) -> Result<Value, String> {
        self.get(&format!("/api/volumes/{}/backends", volume_id)).await
    }

    pub async fn add_backend(&self, volume_id: &str, body: &Value) -> Result<Value, String> {
        self.post(&format!("/api/volumes/{}/backends", volume_id), body).await
    }

    pub async fn remove_backend(&self, volume_id: &str, backend_id: &str) -> Result<Value, String> {
        self.delete(&format!("/api/volumes/{}/backends/{}", volume_id, backend_id)).await
    }

    // ── Peers ─────────────────────────────────────────────────────

    pub async fn list_peers(&self) -> Result<Value, String> {
        self.get("/api/peers").await
    }

    pub async fn join_peer(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/peers/join", body).await
    }

    // ── Disks ─────────────────────────────────────────────────────

    pub async fn list_disks(&self) -> Result<Value, String> {
        self.get("/api/disks").await
    }

    pub async fn claim_disk(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/disks/claim", body).await
    }

    pub async fn release_disk(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/disks/release", body).await
    }

    pub async fn reset_disk(&self, body: &Value) -> Result<Value, String> {
        self.post("/api/disks/reset", body).await
    }

    pub async fn allocate_disk(&self, volume_id: &str, body: &Value) -> Result<Value, String> {
        self.post(&format!("/api/volumes/{}/allocate-disk", volume_id), body).await
    }

    pub async fn disk_smart(&self, device_name: &str) -> Result<Value, String> {
        self.get(&format!("/api/disks/{}/smart", device_name)).await
    }

    // ── Volume File Operations ──────────────────────────────────────

    pub async fn browse_volume(&self, volume_id: &str, path: &str) -> Result<Value, String> {
        let encoded = if path.is_empty() { String::new() } else { format!("/{}", path) };
        self.get(&format!("/api/volumes/{}/browse{}", volume_id, encoded)).await
    }

    pub async fn chunk_map(&self, volume_id: &str) -> Result<Value, String> {
        self.get(&format!("/api/volumes/{}/chunk-map", volume_id)).await
    }

    pub async fn mkdir_volume(&self, volume_id: &str, body: &Value) -> Result<Value, String> {
        self.post(&format!("/api/volumes/{}/mkdir", volume_id), body).await
    }

    pub async fn delete_file(&self, volume_id: &str, path: &str) -> Result<Value, String> {
        self.delete(&format!("/api/volumes/{}/files/{}", volume_id, path)).await
    }

    pub async fn upload_file(&self, volume_id: &str, path: &str, data: Vec<u8>) -> Result<Value, String> {
        let url = format!("{}/api/volumes/{}/files/{}", self.base_url, volume_id, path);
        let resp = self.http.put(&url)
            .header("Content-Type", "application/octet-stream")
            .body(data)
            .send().await
            .map_err(|e| format!("SAN request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("SAN {} returned {}: {}", url, status, body));
        }

        resp.json().await
            .map_err(|e| format!("SAN response parse error ({}): {}", url, e))
    }

    // ── Benchmark ─────────────────────────────────────────────────

    pub async fn benchmark_matrix(&self) -> Result<Value, String> {
        self.get("/api/benchmark/matrix").await
    }

    pub async fn run_benchmark(&self) -> Result<Value, String> {
        self.post("/api/benchmark/run", &serde_json::json!({})).await
    }

    // ── Internal HTTP helpers ─────────────────────────────────────

    async fn get(&self, path: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.get(&url)
            .send().await
            .map_err(|e| format!("SAN request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("SAN {} returned {}: {}", url, status, body));
        }

        resp.json().await
            .map_err(|e| format!("SAN response parse error ({}): {}", url, e))
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.post(&url)
            .json(body)
            .send().await
            .map_err(|e| format!("SAN request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("SAN {} returned {}: {}", url, status, body));
        }

        resp.json().await
            .map_err(|e| format!("SAN response parse error ({}): {}", url, e))
    }

    async fn put(&self, path: &str, body: &Value) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.put(&url)
            .json(body)
            .send().await
            .map_err(|e| format!("SAN request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("SAN {} returned {}: {}", url, status, body));
        }

        resp.json().await
            .map_err(|e| format!("SAN response parse error ({}): {}", url, e))
    }

    async fn delete(&self, path: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.delete(&url)
            .send().await
            .map_err(|e| format!("SAN request failed ({}): {}", url, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("SAN {} returned {}: {}", url, status, body));
        }

        resp.json().await
            .map_err(|e| format!("SAN response parse error ({}): {}", url, e))
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Get all SAN-enabled hosts from the cluster database.
pub fn get_san_hosts(db: &Connection) -> Vec<SanHost> {
    let mut stmt = db.prepare(
        "SELECT id, hostname, san_address, san_node_id FROM hosts
         WHERE san_enabled = 1 AND san_address != '' AND status != 'offline'"
    ).unwrap();

    stmt.query_map([], |row| {
        Ok(SanHost {
            host_id: row.get(0)?,
            hostname: row.get(1)?,
            san_address: row.get(2)?,
            san_node_id: row.get(3)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect()
}

/// Get a specific SAN host by cluster host ID.
pub fn get_san_host_by_id(db: &Connection, host_id: &str) -> Option<SanHost> {
    db.query_row(
        "SELECT id, hostname, san_address, san_node_id FROM hosts
         WHERE id = ?1 AND san_enabled = 1 AND san_address != ''",
        rusqlite::params![host_id],
        |row| Ok(SanHost {
            host_id: row.get(0)?,
            hostname: row.get(1)?,
            san_address: row.get(2)?,
            san_node_id: row.get(3)?,
        }),
    ).ok()
}
