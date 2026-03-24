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

pub fn apply_networkd_config() -> Result<()> {
    let status = Command::new("networkctl")
        .arg("reload")
        .status()
        .context("Failed to execute networkctl reload")?;
    if !status.success() {
        bail!("networkctl reload failed");
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
