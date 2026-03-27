//! Node lifecycle management — start, stop, restart vmm-san child processes.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::fs;

pub struct NodeHandle {
    pub index: usize,
    pub node_id: String,
    pub port: u16,
    pub peer_port: u16,
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub disk_paths: Vec<String>,
    pub log_path: PathBuf,
    child: Option<Child>,
}

impl NodeHandle {
    pub fn new(
        index: usize,
        base_port: u16,
        temp_dir: &Path,
    ) -> Self {
        let node_id = format!("node-{}", index);
        let port = base_port + index as u16;
        let peer_port = port + 100;
        let node_dir = temp_dir.join(format!("node-{}", index));
        let data_dir = node_dir.join("data");
        let fuse_dir = node_dir.join("fuse");
        let disk_0 = node_dir.join("disk-0");
        let disk_1 = node_dir.join("disk-1");

        // Create directories
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&fuse_dir).unwrap();
        fs::create_dir_all(&disk_0).unwrap();
        fs::create_dir_all(&disk_1).unwrap();

        let disk_paths = vec![
            disk_0.to_string_lossy().to_string(),
            disk_1.to_string_lossy().to_string(),
        ];

        let config_path = node_dir.join("vmm-san.toml");
        let log_path = node_dir.join("output.log");

        Self {
            index,
            node_id,
            port,
            peer_port,
            config_path,
            data_dir,
            disk_paths,
            log_path,
            child: None,
        }
    }

    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Write the TOML config file for this node.
    pub fn write_config(
        &self,
        _total_nodes: usize,
        _base_port: u16,
        witness_port: u16,
        peer_secret: &str,
    ) {
        let fuse_dir = self.data_dir.parent().unwrap().join("fuse");
        let config = format!(
            r#"[server]
bind = "127.0.0.1"
port = {}

[data]
data_dir = "{}"
fuse_root = "{}"

[peer]
port = {}
secret = "{}"

[cluster]
witness_url = "http://127.0.0.1:{}"

[benchmark]
enabled = false

[integrity]
enabled = false
repair_interval_secs = 9999
"#,
            self.port,
            self.data_dir.display(),
            fuse_dir.display(),
            self.peer_port,
            peer_secret,
            witness_port,
        );
        fs::write(&self.config_path, config).unwrap();
    }

    /// Start the vmm-san process.
    pub fn start(&mut self, vmm_san_binary: &Path) -> Result<(), String> {
        let log_file = fs::File::create(&self.log_path)
            .map_err(|e| format!("Cannot create log file: {}", e))?;
        let log_err = log_file.try_clone()
            .map_err(|e| format!("Cannot clone log file: {}", e))?;

        let child = Command::new(vmm_san_binary)
            .arg("--config")
            .arg(&self.config_path)
            .env("RUST_LOG", "vmm_san=trace")
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| format!("Cannot start node {}: {}", self.index, e))?;

        self.child = Some(child);
        Ok(())
    }

    /// Stop the vmm-san process (SIGTERM, then SIGKILL after 3s).
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Try graceful shutdown
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            // Wait up to 3 seconds
            for _ in 0..30 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // Force kill
            child.kill().ok();
            child.wait().ok();
        }
    }

    /// Check if the child process is still running (actually checks process status).
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited
                    self.child = None;
                    false
                }
                Ok(None) => true, // Still running
                Err(_) => false,
            }
        } else {
            false
        }
    }
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Find the vmm-san binary (built by cargo).
pub fn find_vmm_san_binary() -> PathBuf {
    // Try common locations
    let candidates = [
        PathBuf::from("target/debug/vmm-san"),
        PathBuf::from("../../target/debug/vmm-san"),
        PathBuf::from("/tmp/cargo-build-san/debug/vmm-san"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.canonicalize().unwrap_or_else(|_| c.clone());
        }
    }
    // Fall back to cargo build
    eprintln!("vmm-san binary not found. Building...");
    let status = Command::new("cargo")
        .args(["build", "-p", "vmm-san"])
        .status()
        .expect("Failed to run cargo build");
    if !status.success() {
        panic!("Failed to build vmm-san");
    }
    candidates[0].canonicalize().unwrap_or_else(|_| candidates[0].clone())
}
