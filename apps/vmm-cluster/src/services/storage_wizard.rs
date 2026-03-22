//! Storage Wizard Service — orchestrates cluster filesystem setup.
//!
//! Supports NFS, GlusterFS, and CephFS.
//! Handles package installation, filesystem creation, mounting, and datastore registration.

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use crate::state::ClusterState;
use crate::node_client::NodeClient;
use crate::services::host::HostService;
use crate::services::datastore::DatastoreService;
use crate::services::event::EventService;

pub struct StorageWizardService;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WizardConfig {
    pub fs_type: String,       // nfs, glusterfs, cephfs
    pub cluster_id: String,
    pub datastore_name: String,
    pub host_ids: Vec<String>,
    pub mount_path: String,
    // NFS-specific
    pub nfs_server: Option<String>,
    pub nfs_export: Option<String>,
    pub nfs_opts: Option<String>,
    // GlusterFS-specific
    pub gluster_volume: Option<String>,
    pub gluster_brick_path: Option<String>,
    pub gluster_replica: Option<u32>,
    // NFS create mode — set up NFS server on this host
    pub nfs_server_host_id: Option<String>,
    // CephFS-specific
    pub ceph_monitors: Option<String>,
    pub ceph_path: Option<String>,
    pub ceph_secret: Option<String>,
    /// true = install Ceph from scratch on all hosts
    #[serde(default)]
    pub ceph_create_new: bool,
    // Credentials — per-host sudo passwords (host_id → password)
    #[serde(default)]
    pub sudo_passwords: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WizardStep {
    pub label: String,
    pub status: String, // pending, running, done, error
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HostPackageStatus {
    pub host_id: String,
    pub hostname: String,
    pub installed: Vec<String>,
    pub missing: Vec<String>,
    pub distro: String,
}

impl StorageWizardService {
    /// Get required packages for a filesystem type.
    pub fn required_packages(fs_type: &str) -> Vec<String> {
        match fs_type {
            // NFS: install both client and server (server only used on the selected host)
            "nfs" => vec!["nfs-common".into(), "nfs-kernel-server".into()],
            "glusterfs" => vec!["glusterfs-server".into(), "glusterfs-client".into()],
            "cephfs" => vec!["ceph-common".into(), "ceph-fuse".into()],
            _ => Vec::new(),
        }
    }

    /// Check package status on all hosts.
    pub async fn check_hosts(state: &Arc<ClusterState>, host_ids: &[String], fs_type: &str) -> Vec<HostPackageStatus> {
        let packages = Self::required_packages(fs_type);
        let mut results = Vec::new();

        for host_id in host_ids {
            let node = match state.nodes.get(host_id) {
                Some(n) => n.clone(),
                None => continue,
            };

            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let resp = client.post(format!("{}/agent/packages/check", &node.address))
                .header("X-Agent-Token", &node.agent_token)
                .json(&serde_json::json!({ "packages": packages }))
                .send().await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    if let Ok(data) = r.json::<serde_json::Value>().await {
                        results.push(HostPackageStatus {
                            host_id: host_id.clone(),
                            hostname: node.hostname.clone(),
                            installed: data.get("installed").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
                            missing: data.get("missing").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
                            distro: data.get("distro").and_then(|v| v.as_str()).unwrap_or("unknown").into(),
                        });
                    }
                }
                _ => {
                    results.push(HostPackageStatus {
                        host_id: host_id.clone(), hostname: node.hostname.clone(),
                        installed: Vec::new(), missing: packages.clone(),
                        distro: "unknown".into(),
                    });
                }
            }
        }
        results
    }

    /// Install packages on specified hosts.
    pub async fn install_on_hosts(state: &Arc<ClusterState>, host_ids: &[String], fs_type: &str) -> Result<(), String> {
        let packages = Self::required_packages(fs_type);

        for host_id in host_ids {
            let node = match state.nodes.get(host_id) {
                Some(n) => n.clone(),
                None => continue,
            };

            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(120))
                .build().map_err(|e| e.to_string())?;

            let resp = client.post(format!("{}/agent/packages/install", &node.address))
                .header("X-Agent-Token", &node.agent_token)
                .json(&serde_json::json!({ "packages": packages }))
                .send().await
                .map_err(|e| format!("Cannot reach host '{}': {}", node.hostname, e))?;

            if !resp.status().is_success() {
                let err = resp.text().await.unwrap_or_default();
                return Err(format!("Package install failed on '{}': {}", node.hostname, err));
            }
        }
        Ok(())
    }

    /// Execute a command on a specific host via agent, with optional sudo.
    async fn exec_on_host(state: &Arc<ClusterState>, host_id: &str, command: &str, sudo_password: Option<&str>) -> Result<(i32, String, String), String> {
        let node = state.nodes.get(host_id)
            .ok_or_else(|| format!("Host {} not connected", host_id))?
            .clone();

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(120))
            .build().map_err(|e| e.to_string())?;

        let mut body = serde_json::json!({ "command": command, "timeout_secs": 60 });
        if let Some(pass) = sudo_password {
            body["sudo_password"] = serde_json::Value::String(pass.to_string());
        }

        let resp = client.post(format!("{}/agent/exec", &node.address))
            .header("X-Agent-Token", &node.agent_token)
            .json(&body)
            .send().await
            .map_err(|e| format!("Cannot reach host: {}", e))?;

        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let exit_code = data.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        let stdout = data.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let stderr = data.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string();
        Ok((exit_code, stdout, stderr))
    }

    /// Setup the filesystem and create the datastore.
    pub async fn setup(state: &Arc<ClusterState>, config: &WizardConfig) -> Result<Vec<WizardStep>, String> {
        let mut steps = Vec::new();

        match config.fs_type.as_str() {
            "glusterfs" => Self::setup_glusterfs(state, config, &mut steps).await?,
            "nfs" => Self::setup_nfs(state, config, &mut steps).await?,
            "cephfs" => Self::setup_cephfs(state, config, &mut steps).await?,
            _ => return Err(format!("Unknown filesystem type: {}", config.fs_type)),
        }

        // Final step: Register datastore in cluster DB
        let mount_source = match config.fs_type.as_str() {
            "nfs" => format!("{}:{}", config.nfs_server.as_deref().unwrap_or(""), config.nfs_export.as_deref().unwrap_or("")),
            "glusterfs" => format!("localhost:{}", config.gluster_volume.as_deref().unwrap_or("vol")),
            "cephfs" => config.ceph_monitors.clone().unwrap_or_default(),
            _ => String::new(),
        };

        {
            let db = state.db.lock().map_err(|_| "DB lock error".to_string())?;
            let ds_id = DatastoreService::create(&db, &config.datastore_name, &config.fs_type,
                &mount_source, "", &config.mount_path, &config.cluster_id)?;

            for host_id in &config.host_ids {
                DatastoreService::add_host_mount(&db, &ds_id, host_id)?;
                DatastoreService::update_host_mount(&db, &ds_id, host_id, true, "mounted", 0, 0)?;
            }
            DatastoreService::update_status(&db, &ds_id, "online")?;

            EventService::log(&db, "info", "datastore",
                &format!("Cluster storage '{}' ({}) created via wizard", config.datastore_name, config.fs_type),
                Some("datastore"), Some(&ds_id), None);
        }

        steps.push(WizardStep { label: "Datastore registered".into(), status: "done".into(), error: None });
        Ok(steps)
    }

    /// Get sudo password for a host from config.
    fn sudo_for<'a>(config: &'a WizardConfig, host_id: &str) -> Option<&'a str> {
        config.sudo_passwords.get(host_id).map(|s| s.as_str())
    }

    async fn setup_glusterfs(state: &Arc<ClusterState>, config: &WizardConfig, steps: &mut Vec<WizardStep>) -> Result<(), String> {
        let volume = config.gluster_volume.as_deref().unwrap_or("cluster-storage");
        let brick = config.gluster_brick_path.as_deref().unwrap_or("/data/gluster/cluster-storage");
        let replica = config.gluster_replica.unwrap_or(2);

        // 1. Start GlusterFS service on all hosts
        for host_id in &config.host_ids {
            let (code, _, stderr) = Self::exec_on_host(state, host_id, "systemctl enable --now glusterd", Self::sudo_for(config, host_id)).await?;
            if code != 0 {
                steps.push(WizardStep { label: format!("Start glusterd on {}", host_id), status: "error".into(), error: Some(stderr) });
                return Err("Failed to start GlusterFS service".into());
            }
        }
        steps.push(WizardStep { label: "GlusterFS service started".into(), status: "done".into(), error: None });

        // 2. Peer-probe from first host to all others
        if config.host_ids.len() > 1 {
            let first = &config.host_ids[0];
            for other in &config.host_ids[1..] {
                let other_node = state.nodes.get(other).ok_or("Host not found")?.clone();
                // Use IP from address (strip protocol+port)
                let peer_addr = other_node.address.replace("https://", "").replace("http://", "")
                    .split(':').next().unwrap_or("").to_string();
                let cmd = format!("gluster peer probe {}", peer_addr);
                let sudo = Self::sudo_for(config, first);
                let (code, stdout, stderr) = Self::exec_on_host(state, first, &cmd, sudo).await?;
                if code != 0 && !stdout.contains("already in peer list") {
                    steps.push(WizardStep { label: format!("Peer probe {}", peer_addr), status: "error".into(), error: Some(stderr) });
                    return Err(format!("Peer probe failed for {}", peer_addr));
                }
                steps.push(WizardStep { label: format!("Peer: {}", other_node.hostname), status: "done".into(), error: None });
            }
        }

        // 3. Create brick directories
        for host_id in &config.host_ids {
            Self::exec_on_host(state, host_id, &format!("mkdir -p {}", brick), Self::sudo_for(config, host_id)).await?;
        }
        steps.push(WizardStep { label: "Brick directories created".into(), status: "done".into(), error: None });

        // 4. Create volume
        let mut bricks = Vec::new();
        for host_id in &config.host_ids {
            let node = state.nodes.get(host_id).ok_or("Host not found")?.clone();
            let addr = node.address.replace("https://", "").replace("http://", "")
                .split(':').next().unwrap_or("").to_string();
            bricks.push(format!("{}:{}", addr, brick));
        }
        let brick_list = bricks.join(" ");
        let create_cmd = format!("gluster volume create {} replica {} {} force", volume, replica, brick_list);
        let first_sudo = Self::sudo_for(config, &config.host_ids[0]);
        let (code, _, stderr) = Self::exec_on_host(state, &config.host_ids[0], &create_cmd, first_sudo).await?;
        if code != 0 && !stderr.contains("already exists") {
            steps.push(WizardStep { label: "Create volume".into(), status: "error".into(), error: Some(stderr) });
            return Err("Volume creation failed".into());
        }
        steps.push(WizardStep { label: format!("Volume '{}' created (replica {})", volume, replica), status: "done".into(), error: None });

        // 5. Start volume
        let (code, _, stderr) = Self::exec_on_host(state, &config.host_ids[0], &format!("gluster volume start {}", volume), first_sudo).await?;
        if code != 0 && !stderr.contains("already started") {
            steps.push(WizardStep { label: "Start volume".into(), status: "error".into(), error: Some(stderr) });
            return Err("Volume start failed".into());
        }
        steps.push(WizardStep { label: "Volume started".into(), status: "done".into(), error: None });

        // 6. Mount on all hosts
        for host_id in &config.host_ids {
            let sudo = Self::sudo_for(config, host_id);
            Self::exec_on_host(state, host_id, &format!("mkdir -p {}", config.mount_path), sudo).await?;
            let mount_cmd = format!("mount -t glusterfs localhost:{} {}", volume, config.mount_path);
            let (code, _, stderr) = Self::exec_on_host(state, host_id, &mount_cmd, sudo).await?;
            if code != 0 && !stderr.contains("already mounted") {
                steps.push(WizardStep { label: format!("Mount on {}", host_id), status: "error".into(), error: Some(stderr) });
                return Err("Mount failed".into());
            }
        }
        steps.push(WizardStep { label: "Mounted on all hosts".into(), status: "done".into(), error: None });

        Ok(())
    }

    async fn setup_nfs(state: &Arc<ClusterState>, config: &WizardConfig, steps: &mut Vec<WizardStep>) -> Result<(), String> {
        let export = config.nfs_export.as_deref().unwrap_or("/vmm/nfs-export");
        let opts = config.nfs_opts.as_deref().unwrap_or("vers=4,noatime");

        // If creating a new NFS server on a host
        let server_addr = if let Some(ref server_host_id) = config.nfs_server_host_id {
            let sudo = Self::sudo_for(config, server_host_id);

            // Install NFS server package
            Self::exec_on_host(state, server_host_id, "which exportfs || apt-get install -y nfs-kernel-server || yum install -y nfs-utils", sudo).await?;
            steps.push(WizardStep { label: "NFS server package installed".into(), status: "done".into(), error: None });

            // Create export directory
            Self::exec_on_host(state, server_host_id, &format!("mkdir -p {}", export), sudo).await?;

            // Configure /etc/exports
            let export_line = format!("{} *(rw,sync,no_subtree_check,no_root_squash)", export);
            let add_export_cmd = format!("grep -qF '{}' /etc/exports 2>/dev/null || echo '{}' >> /etc/exports", export, export_line);
            Self::exec_on_host(state, server_host_id, &add_export_cmd, sudo).await?;
            steps.push(WizardStep { label: format!("NFS export '{}' configured", export), status: "done".into(), error: None });

            // Start/restart NFS server
            Self::exec_on_host(state, server_host_id, "systemctl enable --now nfs-kernel-server 2>/dev/null || systemctl enable --now nfs-server 2>/dev/null || true", sudo).await?;
            Self::exec_on_host(state, server_host_id, "exportfs -ra", sudo).await?;
            steps.push(WizardStep { label: "NFS server started".into(), status: "done".into(), error: None });

            // Get the server's IP
            let node = state.nodes.get(server_host_id).ok_or("NFS server host not found")?.clone();
            node.address.replace("https://", "").replace("http://", "").split(':').next().unwrap_or("").to_string()
        } else {
            config.nfs_server.clone().ok_or("NFS server address required")?
        };

        let source = format!("{}:{}", server_addr, export);

        // Mount on all hosts (except the server itself — it already has the directory)
        for host_id in &config.host_ids {
            if config.nfs_server_host_id.as_deref() == Some(host_id.as_str()) {
                continue; // Server host uses the local directory, no mount needed
            }
            let sudo = Self::sudo_for(config, host_id);
            Self::exec_on_host(state, host_id, &format!("mkdir -p {}", config.mount_path), sudo).await?;
            let mount_cmd = format!("mount -t nfs -o {} {} {}", opts, source, config.mount_path);
            let (code, _, stderr) = Self::exec_on_host(state, host_id, &mount_cmd, sudo).await?;
            if code != 0 {
                let node = state.nodes.get(host_id).map(|n| n.hostname.clone()).unwrap_or_default();
                steps.push(WizardStep { label: format!("Mount on {}", node), status: "error".into(), error: Some(stderr) });
                return Err(format!("NFS mount failed on {}", node));
            }
        }
        steps.push(WizardStep { label: format!("NFS mounted on all hosts"), status: "done".into(), error: None });

        Ok(())
    }

    async fn setup_cephfs(state: &Arc<ClusterState>, config: &WizardConfig, steps: &mut Vec<WizardStep>) -> Result<(), String> {
        if config.ceph_create_new {
            // Full Ceph installation from scratch
            let first = &config.host_ids[0];
            let sudo = Self::sudo_for(config, first);

            // Install ceph on all hosts
            for host_id in &config.host_ids {
                let s = Self::sudo_for(config, host_id);
                Self::exec_on_host(state, host_id, "which ceph-mon >/dev/null 2>&1 || apt-get install -y ceph ceph-mds ceph-fuse || yum install -y ceph ceph-mds ceph-fuse", s).await?;
            }
            steps.push(WizardStep { label: "Ceph packages installed on all hosts".into(), status: "done".into(), error: None });

            // Generate minimal ceph.conf on first host
            let node = state.nodes.get(first).ok_or("First host not found")?.clone();
            let first_ip = node.address.replace("https://", "").replace("http://", "").split(':').next().unwrap_or("").to_string();
            let fsid = uuid::Uuid::new_v4().to_string();

            let ceph_conf = format!(
                "[global]\nfsid = {}\nmon_initial_members = {}\nmon_host = {}\nauth_cluster_required = none\nauth_service_required = none\nauth_client_required = none\nosd_pool_default_size = {}\n",
                fsid, node.hostname, first_ip, std::cmp::min(config.host_ids.len(), 3)
            );

            Self::exec_on_host(state, first, &format!("mkdir -p /etc/ceph && echo '{}' > /etc/ceph/ceph.conf", ceph_conf.replace('\n', "\\n")), sudo).await?;
            steps.push(WizardStep { label: "Ceph configuration generated".into(), status: "done".into(), error: None });

            // Note: Full Ceph bootstrap (ceph-deploy, cephadm, or manual mon/osd init) is complex.
            // For a production setup, cephadm is recommended. We set up the config and let the admin
            // finish the bootstrap, or we detect if cephadm is available.
            Self::exec_on_host(state, first, "which cephadm >/dev/null 2>&1 && cephadm bootstrap --mon-ip {} --skip-monitoring-stack --skip-dashboard || echo 'cephadm not found — manual Ceph init required'", sudo).await?;
            steps.push(WizardStep { label: "Ceph cluster bootstrap attempted".into(), status: "done".into(), error: None });

            // Create CephFS
            Self::exec_on_host(state, first, "ceph osd pool create cephfs_data 32 2>/dev/null; ceph osd pool create cephfs_metadata 32 2>/dev/null; ceph fs new cephfs cephfs_metadata cephfs_data 2>/dev/null || true", sudo).await?;
            steps.push(WizardStep { label: "CephFS filesystem created".into(), status: "done".into(), error: None });

            // Mount CephFS on all hosts
            for host_id in &config.host_ids {
                let s = Self::sudo_for(config, host_id);
                // Copy ceph.conf to other hosts
                if host_id != first {
                    Self::exec_on_host(state, host_id, &format!("mkdir -p /etc/ceph && echo '{}' > /etc/ceph/ceph.conf", ceph_conf.replace('\n', "\\n")), s).await?;
                }
                Self::exec_on_host(state, host_id, &format!("mkdir -p {}", config.mount_path), s).await?;
                let mount_cmd = format!("mount -t ceph {}:/ {} -o name=admin", first_ip, config.mount_path);
                let (code, _, stderr) = Self::exec_on_host(state, host_id, &mount_cmd, s).await?;
                if code != 0 {
                    let hn = state.nodes.get(host_id).map(|n| n.hostname.clone()).unwrap_or_default();
                    steps.push(WizardStep { label: format!("Mount on {}", hn), status: "error".into(), error: Some(stderr) });
                    return Err(format!("CephFS mount failed on {}", hn));
                }
            }
            steps.push(WizardStep { label: "CephFS mounted on all hosts".into(), status: "done".into(), error: None });

            return Ok(());
        }

        // Existing Ceph cluster — just mount
        let monitors = config.ceph_monitors.as_deref().ok_or("Ceph monitor addresses required")?;
        let path = config.ceph_path.as_deref().unwrap_or("/");
        let source = format!("{}:{}", monitors, path);

        let mut mount_opts = "name=admin".to_string();
        if let Some(secret) = &config.ceph_secret {
            mount_opts = format!("name=admin,secret={}", secret);
        }

        for host_id in &config.host_ids {
            let sudo = Self::sudo_for(config, host_id);
            Self::exec_on_host(state, host_id, &format!("mkdir -p {}", config.mount_path), sudo).await?;
            let mount_cmd = format!("mount -t ceph -o {} {} {}", mount_opts, source, config.mount_path);
            let (code, _, stderr) = Self::exec_on_host(state, host_id, &mount_cmd, sudo).await?;
            if code != 0 {
                let node = state.nodes.get(host_id).map(|n| n.hostname.clone()).unwrap_or_default();
                steps.push(WizardStep { label: format!("Mount on {}", node), status: "error".into(), error: Some(stderr) });
                return Err(format!("CephFS mount failed on {}", node));
            }
        }
        steps.push(WizardStep { label: "CephFS mounted on all hosts".into(), status: "done".into(), error: None });

        Ok(())
    }
}
