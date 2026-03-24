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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("vmm_config_test_{}", suffix));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_appliance_config_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("appliance.toml");

        let config = ApplianceConfig {
            role: ApplianceRole::Cluster,
            language: "de_DE".to_string(),
            version: "1.2.3".to_string(),
        };
        config.save(&path).unwrap();

        let loaded = ApplianceConfig::load(&path).unwrap();
        assert_eq!(loaded.role, ApplianceRole::Cluster);
        assert_eq!(loaded.language, "de_DE");
        assert_eq!(loaded.version, "1.2.3");
    }

    #[test]
    fn test_write_vmm_server_config_keys() {
        let dir = temp_dir("server_cfg");
        write_vmm_server_config(&dir, 9000, "/data", "/log/vmm.log").unwrap();

        let content = fs::read_to_string(dir.join("etc/vmm/vmm-server.toml")).unwrap();
        assert!(content.contains("[server]"), "missing [server] section");
        assert!(content.contains("port = 9000"), "missing port");
        assert!(content.contains("[auth]"), "missing [auth] section");
        assert!(content.contains("jwt_secret"), "missing jwt_secret");
        assert!(content.contains("[storage]"), "missing [storage] section");
        assert!(content.contains("data_dir = \"/data\""), "missing data_dir");
        assert!(content.contains("[logging]"), "missing [logging] section");
    }

    #[test]
    fn test_write_vmm_cluster_config_keys() {
        let dir = temp_dir("cluster_cfg");
        write_vmm_cluster_config(&dir, 9001, "/data", "/log/vmm.log").unwrap();

        let content = fs::read_to_string(dir.join("etc/vmm/vmm-cluster.toml")).unwrap();
        assert!(content.contains("[cluster]"), "missing [cluster] section");
        assert!(content.contains("port = 9001"), "missing port");
        assert!(content.contains("[auth]"), "missing [auth] section");
        assert!(content.contains("jwt_secret"), "missing jwt_secret");
        assert!(content.contains("[storage]"), "missing [storage] section");
    }

    #[test]
    fn test_write_default_config_server() {
        let dir = temp_dir("default_server");
        write_default_config(&dir, &ApplianceRole::Server).unwrap();

        let appliance = fs::read_to_string(dir.join("etc/vmm/appliance.toml")).unwrap();
        assert!(appliance.contains("Server"), "appliance.toml missing Server role");
        assert!(dir.join("etc/vmm/vmm-server.toml").exists(), "vmm-server.toml not created");
    }

    #[test]
    fn test_write_default_config_cluster() {
        let dir = temp_dir("default_cluster");
        write_default_config(&dir, &ApplianceRole::Cluster).unwrap();

        let appliance = fs::read_to_string(dir.join("etc/vmm/appliance.toml")).unwrap();
        assert!(appliance.contains("Cluster"), "appliance.toml missing Cluster role");
        assert!(dir.join("etc/vmm/vmm-cluster.toml").exists(), "vmm-cluster.toml not created");
    }
}

fn generate_jwt_secret() -> String {
    use std::io::Read;

    // Read 32 bytes from /dev/urandom for cryptographically secure randomness
    let mut bytes = [0u8; 32];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut bytes);
    } else {
        // Fallback: use system time + pid (not ideal, but better than nothing)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = ((now >> (i % 16 * 8)) ^ (std::process::id() as u128)) as u8;
        }
    }

    // Encode as hex (64 chars, take 32)
    bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..32].to_string()
}
