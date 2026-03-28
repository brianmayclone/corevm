//! HTTP client for communicating with vmm-server Agent API.
//!
//! The cluster uses this to send commands to nodes and poll their status.

pub mod types;

use crate::node_client::types::*;
use vmm_core::cluster::{HostStatus, ProvisionVmRequest, ProvisionVmResponse,
    MountDatastoreRequest, AgentResponse, SetupViSwitchRequest, TeardownViSwitchRequest};

/// Client for a single vmm-server node's Agent API.
pub struct NodeClient {
    http: reqwest::Client,
    base_url: String,
    agent_token: String,
}

impl NodeClient {
    pub fn new(base_url: &str, agent_token: &str) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            agent_token: agent_token.to_string(),
        })
    }

    /// GET /agent/status — full host status (heartbeat payload).
    pub async fn get_status(&self) -> Result<HostStatus, String> {
        let resp = self.http.get(format!("{}/agent/status", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Status request failed: {}", resp.status()));
        }

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/vms/provision — provision a VM on this node.
    pub async fn provision_vm(&self, req: &ProvisionVmRequest) -> Result<ProvisionVmResponse, String> {
        let resp = self.http.post(format!("{}/agent/vms/provision", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(format!("Provision failed: {}", err));
        }

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/vms/{id}/start
    pub async fn start_vm(&self, vm_id: &str) -> Result<AgentResponse, String> {
        self.post_agent(&format!("/agent/vms/{}/start", vm_id)).await
    }

    /// POST /agent/vms/{id}/stop
    pub async fn stop_vm(&self, vm_id: &str) -> Result<AgentResponse, String> {
        self.post_agent(&format!("/agent/vms/{}/stop", vm_id)).await
    }

    /// POST /agent/vms/{id}/force-stop
    pub async fn force_stop_vm(&self, vm_id: &str) -> Result<AgentResponse, String> {
        self.post_agent(&format!("/agent/vms/{}/force-stop", vm_id)).await
    }

    /// POST /agent/vms/{id}/destroy
    pub async fn destroy_vm(&self, vm_id: &str) -> Result<AgentResponse, String> {
        self.post_agent(&format!("/agent/vms/{}/destroy", vm_id)).await
    }

    /// POST /agent/storage/mount
    pub async fn mount_datastore(&self, req: &MountDatastoreRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/storage/mount", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/storage/unmount
    pub async fn unmount_datastore(&self, req: &vmm_core::cluster::UnmountDatastoreRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/storage/unmount", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/network/bridge/setup — Set up a bridge on this node.
    pub async fn setup_bridge(&self, req: &vmm_core::cluster::SetupBridgeRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/network/bridge/setup", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/network/bridge/teardown — Remove a bridge from this node.
    pub async fn teardown_bridge(&self, req: &vmm_core::cluster::TeardownBridgeRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/network/bridge/teardown", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/network/viswitch/setup — Set up a viSwitch on this node.
    pub async fn setup_viswitch(&self, req: &SetupViSwitchRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/network/viswitch/setup", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// POST /agent/network/viswitch/teardown — Remove a viSwitch from this node.
    pub async fn teardown_viswitch(&self, req: &TeardownViSwitchRequest) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}/agent/network/viswitch/teardown", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .json(req)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// GET /agent/network/interfaces — list network interfaces on this node.
    pub async fn get_network_interfaces(&self) -> Result<serde_json::Value, String> {
        let resp = self.http.get(format!("{}/agent/network/interfaces", &self.base_url))
            .header("X-Agent-Token", &self.agent_token)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Interface request failed: {}", resp.status()));
        }

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// GET /agent/logs — fetch service log files from this host.
    pub async fn get_logs(&self, service: Option<&str>, lines: Option<usize>) -> Result<serde_json::Value, String> {
        let mut url = format!("{}/agent/logs?", &self.base_url);
        if let Some(s) = service { url.push_str(&format!("service={}&", s)); }
        if let Some(n) = lines { url.push_str(&format!("lines={}", n)); }

        let resp = self.http.get(&url)
            .header("X-Agent-Token", &self.agent_token)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Log request failed: {}", resp.status()));
        }

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }

    /// Generic POST to an agent endpoint (no body).
    async fn post_agent(&self, path: &str) -> Result<AgentResponse, String> {
        let resp = self.http.post(format!("{}{}", &self.base_url, path))
            .header("X-Agent-Token", &self.agent_token)
            .send().await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Ok(AgentResponse::err(err));
        }

        resp.json().await.map_err(|e| format!("Invalid response: {}", e))
    }
}
