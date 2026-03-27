//! TestContext — shared state and helper methods for CLI and scenarios.

use crate::cluster::{NodeHandle, find_vmm_san_binary};
use crate::witness::{self, WitnessHandle, WitnessMode};
use crate::partition::{self, OriginalAddresses};
use crate::db_init;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use tempfile::TempDir;

const BASE_PORT: u16 = 7442;
const WITNESS_PORT: u16 = 9443;
const PEER_SECRET: &str = "testbed-secret";

#[derive(Debug, Deserialize)]
pub struct NodeStatus {
    pub running: bool,
    pub node_id: String,
    pub quorum_status: String,
    pub is_leader: bool,
    pub peer_count: u32,
}

pub struct TestContext {
    pub nodes: Vec<NodeHandle>,
    pub witness: WitnessHandle,
    pub temp_dir: TempDir,
    pub http: Client,
    pub original_addresses: OriginalAddresses,
    vmm_san_binary: PathBuf,
}

impl TestContext {
    /// Create a new testbed with N nodes.
    pub async fn new(num_nodes: usize) -> Result<Self, String> {
        let temp_dir = TempDir::new()
            .map_err(|e| format!("Cannot create temp dir: {}", e))?;

        let vmm_san_binary = find_vmm_san_binary();
        tracing::info!("Using vmm-san binary: {}", vmm_san_binary.display());

        // Create nodes
        let mut nodes: Vec<NodeHandle> = (1..=num_nodes)
            .map(|i| NodeHandle::new(i, BASE_PORT, temp_dir.path()))
            .collect();

        // Write configs and init DBs
        for node in &nodes {
            node.write_config(num_nodes, BASE_PORT, WITNESS_PORT, PEER_SECRET);

            let db_path = node.data_dir.join("vmm-san.db");
            db_init::init_node_db(
                &db_path,
                &node.node_id,
                node.index,
                num_nodes,
                BASE_PORT,
                &node.disk_paths,
            )?;
        }

        // Start witness
        let witness = witness::spawn(WITNESS_PORT).await;

        // Start all nodes
        for node in &mut nodes {
            node.start(&vmm_san_binary)?;
            tracing::info!("Started node {} (port {})", node.index, node.port);
        }

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| format!("HTTP client: {}", e))?;

        Ok(Self {
            nodes,
            witness,
            temp_dir,
            http,
            original_addresses: OriginalAddresses::new(),
            vmm_san_binary,
        })
    }

    /// Wait for all running nodes to report healthy via /api/status.
    pub async fn wait_all_healthy(&mut self) -> Result<(), String> {
        let mut indices = Vec::new();
        for node in &mut self.nodes {
            if node.is_running() {
                indices.push(node.index);
            }
        }
        for idx in indices {
            self.wait_node_healthy(idx).await?;
        }
        Ok(())
    }

    pub async fn wait_node_healthy(&self, index: usize) -> Result<(), String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/status", node.address());
        for attempt in 0..60 {
            match self.http.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => {
                    if attempt % 10 == 0 && attempt > 0 {
                        tracing::debug!("Waiting for node {} to be healthy... ({}s)", index, attempt / 2);
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        Err(format!("Node {} did not become healthy within 30s", index))
    }

    /// Get status from a node.
    pub async fn get_status(&self, index: usize) -> Result<NodeStatus, String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/status", node.address());
        let resp = self.http.get(&url).send().await
            .map_err(|e| format!("status request to node {}: {}", index, e))?;
        resp.json::<NodeStatus>().await
            .map_err(|e| format!("status parse for node {}: {}", index, e))
    }

    pub async fn kill_node(&mut self, index: usize) {
        self.nodes[index - 1].stop();
        tracing::info!("Killed node {}", index);
    }

    pub async fn start_node(&mut self, index: usize) -> Result<(), String> {
        self.nodes[index - 1].start(&self.vmm_san_binary)?;
        tracing::info!("Started node {}", index);
        Ok(())
    }

    pub async fn partition(&mut self, group_a: &[usize], group_b: &[usize]) -> Result<(), String> {
        let node_info: Vec<(usize, u16, String)> = self.nodes.iter()
            .map(|n| (n.index, n.port, n.node_id.clone()))
            .collect();
        partition::apply_partition(
            &self.http, &node_info, group_a, group_b,
            PEER_SECRET, &mut self.original_addresses,
        ).await
    }

    pub async fn heal(&mut self) -> Result<(), String> {
        let node_info: Vec<(usize, u16, String)> = self.nodes.iter()
            .map(|n| (n.index, n.port, n.node_id.clone()))
            .collect();
        partition::heal_all(
            &self.http, &node_info, &mut self.original_addresses, PEER_SECRET,
        ).await
    }

    pub async fn write_file(&self, index: usize, vol: &str, path: &str, content: &[u8]) -> Result<u16, String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/volumes/{}/files/{}", node.address(), vol, path);
        let resp = self.http.put(&url)
            .body(content.to_vec())
            .send().await
            .map_err(|e| format!("write to node {}: {}", index, e))?;
        Ok(resp.status().as_u16())
    }

    pub async fn read_file(&self, index: usize, vol: &str, path: &str) -> Result<(u16, Vec<u8>), String> {
        let node = &self.nodes[index - 1];
        let url = format!("{}/api/volumes/{}/files/{}", node.address(), vol, path);
        let resp = self.http.get(&url)
            .send().await
            .map_err(|e| format!("read from node {}: {}", index, e))?;
        let status = resp.status().as_u16();
        let body = resp.bytes().await
            .map_err(|e| format!("read body from node {}: {}", index, e))?;
        Ok((status, body.to_vec()))
    }

    pub fn set_witness_mode(&self, mode: WitnessMode) {
        witness::set_mode(&self.witness, mode);
    }

    pub async fn wait_secs(&self, secs: u64) {
        tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
    }

    /// Read a node's log file.
    pub fn read_log(&self, index: usize) -> String {
        std::fs::read_to_string(&self.nodes[index - 1].log_path).unwrap_or_default()
    }

    pub fn shutdown(&mut self) {
        for node in &mut self.nodes {
            node.stop();
        }
        witness::shutdown(&self.witness);
        // Give OS a moment to release the port
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        self.shutdown();
    }
}
