//! HTTP client for communicating with peer CoreSAN nodes.

use reqwest::Client;
use std::time::Duration;

use crate::auth::PEER_SECRET_HEADER;

/// CoreSAN peer client — talks to other vmm-san instances via REST API.
pub struct PeerClient {
    http: Client,
    secret: String,
}

impl PeerClient {
    pub fn new(secret: &str) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");
        Self { http, secret: secret.to_string() }
    }

    /// Send a heartbeat to a peer.
    pub async fn heartbeat(
        &self,
        peer_address: &str,
        node_id: &str,
        hostname: &str,
        uptime_secs: u64,
        our_address: &str,
        is_leader: bool,
    ) -> Result<(), String> {
        let url = format!("{}/api/peers/heartbeat", peer_address);
        self.http.post(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .json(&serde_json::json!({
                "node_id": node_id,
                "hostname": hostname,
                "uptime_secs": uptime_secs,
                "address": our_address,
                "is_leader": is_leader,
            }))
            .send().await
            .map_err(|e| format!("Heartbeat failed: {}", e))?;
        Ok(())
    }

    /// Push a file to a peer node.
    pub async fn push_file(
        &self,
        peer_address: &str,
        volume_id: &str,
        rel_path: &str,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let url = format!("{}/api/volumes/{}/files/{}", peer_address, volume_id, rel_path);
        let resp = self.http.put(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .body(data)
            .send().await
            .map_err(|e| format!("Push failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Push returned status {}", resp.status()))
        }
    }

    /// Pull a file from a peer node.
    pub async fn pull_file(
        &self,
        peer_address: &str,
        volume_id: &str,
        rel_path: &str,
    ) -> Result<Vec<u8>, String> {
        let url = format!("{}/api/volumes/{}/files/{}", peer_address, volume_id, rel_path);
        let resp = self.http.get(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .send().await
            .map_err(|e| format!("Pull failed: {}", e))?;

        if resp.status().is_success() {
            resp.bytes().await
                .map(|b| b.to_vec())
                .map_err(|e| format!("Pull read error: {}", e))
        } else {
            Err(format!("Pull returned status {}", resp.status()))
        }
    }

    /// Ping a peer for latency measurement.
    pub async fn ping(&self, peer_address: &str) -> Result<Duration, String> {
        let url = format!("{}/api/benchmark/ping", peer_address);
        let start = std::time::Instant::now();
        self.http.get(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .send().await
            .map_err(|e| format!("Ping failed: {}", e))?;
        Ok(start.elapsed())
    }

    /// Echo test for throughput measurement — send data and receive it back.
    pub async fn echo(&self, peer_address: &str, data: &[u8]) -> Result<(Duration, usize), String> {
        let url = format!("{}/api/benchmark/echo", peer_address);
        let start = std::time::Instant::now();
        let resp = self.http.post(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .body(data.to_vec())
            .send().await
            .map_err(|e| format!("Echo failed: {}", e))?;

        let bytes = resp.bytes().await
            .map_err(|e| format!("Echo read error: {}", e))?;
        Ok((start.elapsed(), bytes.len()))
    }

    /// Join a peer (announce ourselves).
    pub async fn announce(
        &self,
        peer_address: &str,
        node_id: &str,
        our_address: &str,
        hostname: &str,
        peer_port: u16,
    ) -> Result<(), String> {
        let url = format!("{}/api/peers/join", peer_address);
        let resp = self.http.post(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .json(&serde_json::json!({
                "node_id": node_id,
                "address": our_address,
                "hostname": hostname,
                "peer_port": peer_port,
                "secret": &self.secret,
            }))
            .send().await
            .map_err(|e| format!("Announce failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Announce returned status {}", resp.status()))
        }
    }

    /// Sync a volume definition to a peer (create if missing).
    pub async fn sync_volume(
        &self,
        peer_address: &str,
        volume: &serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{}/api/volumes/sync", peer_address);
        let resp = self.http.post(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .json(volume)
            .send().await
            .map_err(|e| format!("Volume sync failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Volume sync returned status {}", resp.status()))
        }
    }

    /// Ask vmm-cluster witness whether this node is allowed to write.
    pub async fn witness_check(witness_url: &str, node_id: &str) -> Result<bool, String> {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(3))
            .connect_timeout(std::time::Duration::from_secs(2))
            .build()
            .map_err(|e| format!("witness client build error: {}", e))?;

        let url = format!("{}/api/san/witness/{}", witness_url.trim_end_matches('/'), node_id);
        let resp = client.get(&url)
            .send().await
            .map_err(|e| format!("witness unreachable: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("witness returned {}", resp.status()));
        }

        let body: serde_json::Value = resp.json().await
            .map_err(|e| format!("witness response parse error: {}", e))?;

        Ok(body.get("allowed").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// Push a single chunk to a peer node.
    pub async fn push_chunk(
        &self,
        peer_address: &str,
        volume_id: &str,
        file_id: i64,
        chunk_index: u32,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let url = format!("{}/api/chunks/{}/{}/{}", peer_address, volume_id, file_id, chunk_index);
        let resp = self.http.put(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .body(data)
            .send().await
            .map_err(|e| format!("Chunk push failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Chunk push returned status {}", resp.status()))
        }
    }

    /// Pull a single chunk from a peer node.
    pub async fn pull_chunk(
        &self,
        peer_address: &str,
        volume_id: &str,
        file_id: i64,
        chunk_index: u32,
    ) -> Result<Vec<u8>, String> {
        let url = format!("{}/api/chunks/{}/{}/{}", peer_address, volume_id, file_id, chunk_index);
        let resp = self.http.get(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .send().await
            .map_err(|e| format!("Chunk pull failed: {}", e))?;

        if resp.status().is_success() {
            resp.bytes().await
                .map(|b| b.to_vec())
                .map_err(|e| format!("Chunk pull read error: {}", e))
        } else {
            Err(format!("Chunk pull returned status {}", resp.status()))
        }
    }

    /// Push file metadata to a peer (so they know the file exists and its chunk layout).
    pub async fn push_file_meta(
        &self,
        peer_address: &str,
        meta: &serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{}/api/file-meta/sync", peer_address);
        let resp = self.http.post(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .json(meta)
            .send().await
            .map_err(|e| format!("Meta sync failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Meta sync returned status {}", resp.status()))
        }
    }

    /// Notify a peer to delete a volume.
    pub async fn delete_volume(
        &self,
        peer_address: &str,
        volume_id: &str,
    ) -> Result<(), String> {
        let url = format!("{}/api/volumes/{}", peer_address, volume_id);
        let resp = self.http.delete(&url)
            .header(PEER_SECRET_HEADER, &self.secret)
            .send().await
            .map_err(|e| format!("Volume delete sync failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Volume delete sync returned status {}", resp.status()))
        }
    }
}
