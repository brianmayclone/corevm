use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApplianceRole {
    Server,
    Cluster,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplianceConfig {
    pub role: ApplianceRole,
    pub language: String,
    pub version: String,
}

impl ApplianceConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;
        toml::from_str(&content).context("Failed to parse appliance config TOML")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize appliance config")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write config to {:?}", path))
    }
}

pub fn write_vmm_server_config(
    target: &Path,
    port: u16,
    data_dir: &str,
    log_file: &str,
) -> Result<()> {
    let vmm_dir = target.join("etc/vmm");
    fs::create_dir_all(&vmm_dir).context("Failed to create /etc/vmm directory")?;

    let jwt_secret = generate_jwt_secret();

    let config = format!(
        "# CoreVM Server Configuration\n\n\
         [server]\n\
         bind = \"0.0.0.0\"\n\
         port = {port}\n\n\
         [auth]\n\
         jwt_secret = \"{jwt_secret}\"\n\n\
         [storage]\n\
         data_dir = \"{data_dir}\"\n\n\
         [vms]\n\
         default_storage = \"{data_dir}/vms\"\n\n\
         [logging]\n\
         log_file = \"{log_file}\"\n\
         level = \"info\"\n",
        port = port,
        jwt_secret = jwt_secret,
        data_dir = data_dir,
        log_file = log_file,
    );

    fs::write(vmm_dir.join("vmm-server.toml"), config)
        .context("Failed to write vmm-server.toml")
}

pub fn write_vmm_cluster_config(
    target: &Path,
    port: u16,
    data_dir: &str,
    log_file: &str,
) -> Result<()> {
    let vmm_dir = target.join("etc/vmm");
    fs::create_dir_all(&vmm_dir).context("Failed to create /etc/vmm directory")?;

    let jwt_secret = generate_jwt_secret();

    let config = format!(
        "# CoreVM Cluster Configuration\n\n\
         [cluster]\n\
         bind = \"0.0.0.0\"\n\
         port = {port}\n\n\
         [auth]\n\
         jwt_secret = \"{jwt_secret}\"\n\n\
         [storage]\n\
         data_dir = \"{data_dir}\"\n\n\
         [vms]\n\
         default_storage = \"{data_dir}/vms\"\n\n\
         [logging]\n\
         log_file = \"{log_file}\"\n\
         level = \"info\"\n",
        port = port,
        jwt_secret = jwt_secret,
        data_dir = data_dir,
        log_file = log_file,
    );

    fs::write(vmm_dir.join("vmm-cluster.toml"), config)
        .context("Failed to write vmm-cluster.toml")
}

pub fn write_default_config(target: &Path, role: &ApplianceRole) -> Result<()> {
    let vmm_dir = target.join("etc/vmm");
    fs::create_dir_all(&vmm_dir).context("Failed to create /etc/vmm directory")?;

    let config_path = vmm_dir.join("appliance.toml");
    let appliance_config = ApplianceConfig {
        role: role.clone(),
        language: "en_US".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    appliance_config.save(&config_path)?;

    match role {
        ApplianceRole::Server => {
            write_vmm_server_config(target, 8080, "/var/lib/vmm", "/var/log/vmm/vmm-server.log")?;
        }
        ApplianceRole::Cluster => {
            write_vmm_cluster_config(target, 8081, "/var/lib/vmm", "/var/log/vmm/vmm-cluster.log")?;
        }
    }

    Ok(())
}

fn generate_jwt_secret() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use system time and process ID as entropy sources
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();

    let mut hasher = DefaultHasher::new();
    now.hash(&mut hasher);
    pid.hash(&mut hasher);
    let h1 = hasher.finish();

    // Hash again with different seed for more bits
    let mut hasher2 = DefaultHasher::new();
    h1.hash(&mut hasher2);
    now.wrapping_add(1).hash(&mut hasher2);
    let h2 = hasher2.finish();

    let mut hasher3 = DefaultHasher::new();
    h2.hash(&mut hasher3);
    pid.wrapping_add(1).hash(&mut hasher3);
    let h3 = hasher3.finish();

    let mut hasher4 = DefaultHasher::new();
    h3.hash(&mut hasher4);
    h1.hash(&mut hasher4);
    let h4 = hasher4.finish();

    // Encode to base64-like alphanumeric string (32 chars)
    let raw = format!("{:016x}{:016x}", h1 ^ h2, h3 ^ h4);
    // Map hex to a broader character set for base64-like output
    raw.chars()
        .map(|c| match c {
            '0'..='9' => c,
            'a'..='f' => (b'A' + (c as u8 - b'a') * 4) as char,
            _ => c,
        })
        .take(32)
        .collect()
}
