use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: String,
    pub has_link: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub interface: String,
    pub dhcp: bool,
    pub address: Option<String>, // e.g. "192.168.1.50/24"
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub hostname: String,
}

pub fn detect_interfaces() -> Result<Vec<NetworkInterface>> {
    let net_dir = Path::new("/sys/class/net");
    let mut interfaces = Vec::new();

    let entries = fs::read_dir(net_dir).context("Failed to read /sys/class/net")?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }

        let mac_path = entry.path().join("address");
        let mac = fs::read_to_string(&mac_path)
            .unwrap_or_default()
            .trim()
            .to_string();

        let carrier_path = entry.path().join("carrier");
        let has_link = fs::read_to_string(&carrier_path)
            .unwrap_or_default()
            .trim()
            == "1";

        interfaces.push(NetworkInterface { name, mac, has_link });
    }

    Ok(interfaces)
}

pub fn write_networkd_config(target: &Path, config: &NetworkConfig) -> Result<()> {
    let network_dir = target.join("etc/systemd/network");
    fs::create_dir_all(&network_dir).context("Failed to create systemd/network dir")?;

    let mut content = format!(
        "[Match]\nName={}\n\n[Network]\nHostname={}\n",
        config.interface, config.hostname
    );

    if config.dhcp {
        content.push_str("DHCP=yes\n");
    } else {
        if let Some(addr) = &config.address {
            content.push_str(&format!("Address={}\n", addr));
        }
        if let Some(gw) = &config.gateway {
            content.push_str(&format!("Gateway={}\n", gw));
        }
        for dns in &config.dns {
            content.push_str(&format!("DNS={}\n", dns));
        }
    }

    fs::write(network_dir.join("10-management.network"), content)
        .context("Failed to write systemd network config")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("vmm_network_test_{}", suffix));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_networkd_config_dhcp() {
        let dir = temp_dir("dhcp");
        let config = NetworkConfig {
            interface: "eth0".to_string(),
            dhcp: true,
            address: None,
            gateway: None,
            dns: vec![],
            hostname: "corevm-node".to_string(),
        };
        write_networkd_config(&dir, &config).unwrap();

        let content = fs::read_to_string(
            dir.join("etc/systemd/network/10-management.network"),
        ).unwrap();
        assert!(content.contains("Name=eth0"), "missing interface name");
        assert!(content.contains("Hostname=corevm-node"), "missing hostname");
        assert!(content.contains("DHCP=yes"), "missing DHCP=yes");
        assert!(!content.contains("Address="), "unexpected Address line");
    }

    #[test]
    fn test_networkd_config_static() {
        let dir = temp_dir("static");
        let config = NetworkConfig {
            interface: "ens3".to_string(),
            dhcp: false,
            address: Some("10.0.0.5/24".to_string()),
            gateway: Some("10.0.0.1".to_string()),
            dns: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
            hostname: "corevm-static".to_string(),
        };
        write_networkd_config(&dir, &config).unwrap();

        let content = fs::read_to_string(
            dir.join("etc/systemd/network/10-management.network"),
        ).unwrap();
        assert!(content.contains("Name=ens3"), "missing interface name");
        assert!(content.contains("Hostname=corevm-static"), "missing hostname");
        assert!(!content.contains("DHCP=yes"), "unexpected DHCP=yes");
        assert!(content.contains("Address=10.0.0.5/24"), "missing Address");
        assert!(content.contains("Gateway=10.0.0.1"), "missing Gateway");
        assert!(content.contains("DNS=8.8.8.8"), "missing first DNS");
        assert!(content.contains("DNS=1.1.1.1"), "missing second DNS");
    }
}

pub fn apply_networkd_config() -> Result<()> {
    // Restart systemd-networkd to pick up new config
    let output = Command::new("systemctl")
        .args(["restart", "systemd-networkd"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("Failed to restart systemd-networkd")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("systemctl restart systemd-networkd failed: {}", stderr);
    }
    Ok(())
}

#[derive(Deserialize)]
struct IpAddrEntry {
    addr_info: Vec<IpAddrInfo>,
}

#[derive(Deserialize)]
struct IpAddrInfo {
    family: String,
    local: String,
}

pub fn read_current_ip(interface: &str) -> Result<Option<String>> {
    let output = Command::new("ip")
        .args(["-j", "addr", "show", interface])
        .output()
        .context("Failed to execute ip addr")?;

    if !output.status.success() {
        return Ok(None);
    }

    let entries: Vec<IpAddrEntry> =
        serde_json::from_slice(&output.stdout).context("Failed to parse ip addr JSON")?;

    for entry in entries {
        for info in entry.addr_info {
            if info.family == "inet" {
                return Ok(Some(info.local));
            }
        }
    }

    Ok(None)
}
