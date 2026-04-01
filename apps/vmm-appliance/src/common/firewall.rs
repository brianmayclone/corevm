use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{bail, Context, Result};

pub struct FirewallConfig {
    pub ssh_port: u16,
    pub vmm_server_port: Option<u16>,
    pub vmm_cluster_port: Option<u16>,
    pub vmm_san_port: Option<u16>,
    pub vmm_san_peer_port: Option<u16>,
    pub discovery_port: Option<u16>,
    pub vmm_s3gw_port: Option<u16>,
}

pub fn write_nftables_config(target: &Path, config: &FirewallConfig) -> Result<()> {
    let mut accept_rules = String::new();

    // SSH
    accept_rules.push_str(&format!(
        "        tcp dport {} accept\n",
        config.ssh_port
    ));

    // VMM server port
    if let Some(port) = config.vmm_server_port {
        accept_rules.push_str(&format!("        tcp dport {} accept\n", port));
    }

    // VMM cluster port
    if let Some(port) = config.vmm_cluster_port {
        accept_rules.push_str(&format!("        tcp dport {} accept\n", port));
    }

    // VMM SAN port
    if let Some(port) = config.vmm_san_port {
        accept_rules.push_str(&format!("        tcp dport {} accept\n", port));
    }

    // VMM SAN peer port
    if let Some(port) = config.vmm_san_peer_port {
        accept_rules.push_str(&format!("        tcp dport {} accept\n", port));
    }

    // VMM S3 Gateway port
    if let Some(port) = config.vmm_s3gw_port {
        accept_rules.push_str(&format!("        tcp dport {} accept\n", port));
    }

    // UDP discovery port
    if let Some(port) = config.discovery_port {
        accept_rules.push_str(&format!("        udp dport {} accept\n", port));
    }

    let nftables_conf = format!(
        "#!/usr/sbin/nft -f\n\
         # CoreVM nftables configuration\n\n\
         flush ruleset\n\n\
         table inet filter {{\n\
             chain input {{\n\
                 type filter hook input priority 0; policy drop;\n\n\
                 # Accept loopback\n\
                 iif lo accept\n\n\
                 # Accept established/related connections\n\
                 ct state established,related accept\n\n\
                 # Accept ICMP\n\
                 ip protocol icmp accept\n\
                 ip6 nexthdr icmpv6 accept\n\n\
                 # Accept configured ports\n\
         {}\
             }}\n\n\
             chain forward {{\n\
                 type filter hook forward priority 0; policy drop;\n\
             }}\n\n\
             chain output {{\n\
                 type filter hook output priority 0; policy accept;\n\
             }}\n\
         }}\n",
        accept_rules
    );

    fs::write(target.join("etc/nftables.conf"), nftables_conf)
        .context("Failed to write nftables.conf")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!("vmm_firewall_test_{}", suffix));
        fs::create_dir_all(dir.join("etc")).unwrap();
        dir
    }

    #[test]
    fn test_server_only_firewall() {
        let dir = temp_dir("server");
        let config = FirewallConfig {
            ssh_port: 22,
            vmm_server_port: Some(8443),
            vmm_cluster_port: None,
            vmm_san_port: Some(7443),
            vmm_san_peer_port: Some(7444),
            discovery_port: Some(7445),
            vmm_s3gw_port: Some(9000),
        };
        write_nftables_config(&dir, &config).unwrap();

        let content = fs::read_to_string(dir.join("etc/nftables.conf")).unwrap();
        assert!(content.contains("tcp dport 22 accept"), "missing ssh port");
        assert!(content.contains("tcp dport 8443 accept"), "missing server port");
        assert!(content.contains("tcp dport 7443 accept"), "missing san port");
        assert!(content.contains("tcp dport 7444 accept"), "missing san peer port");
        assert!(content.contains("udp dport 7445 accept"), "missing discovery port");
        assert!(content.contains("tcp dport 9000 accept"), "missing s3gw port");
        assert!(!content.contains("tcp dport 9443"), "unexpected cluster port");
    }

    #[test]
    fn test_cluster_firewall() {
        let dir = temp_dir("cluster");
        let config = FirewallConfig {
            ssh_port: 22,
            vmm_server_port: Some(8443),
            vmm_cluster_port: Some(9443),
            vmm_san_port: Some(7443),
            vmm_san_peer_port: Some(7444),
            discovery_port: Some(7445),
            vmm_s3gw_port: Some(9000),
        };
        write_nftables_config(&dir, &config).unwrap();

        let content = fs::read_to_string(dir.join("etc/nftables.conf")).unwrap();
        assert!(content.contains("tcp dport 22 accept"), "missing ssh port");
        assert!(content.contains("tcp dport 8443 accept"), "missing server port");
        assert!(content.contains("tcp dport 9443 accept"), "missing cluster port");
        assert!(content.contains("tcp dport 7443 accept"), "missing san port");
        assert!(content.contains("tcp dport 7444 accept"), "missing san peer port");
        assert!(content.contains("udp dport 7445 accept"), "missing discovery port");
        assert!(content.contains("tcp dport 9000 accept"), "missing s3gw port");
    }

    #[test]
    fn test_nftables_structure() {
        let dir = temp_dir("structure");
        let config = FirewallConfig {
            ssh_port: 2222,
            vmm_server_port: None,
            vmm_cluster_port: None,
            vmm_san_port: None,
            vmm_san_peer_port: None,
            discovery_port: None,
            vmm_s3gw_port: None,
        };
        write_nftables_config(&dir, &config).unwrap();

        let content = fs::read_to_string(dir.join("etc/nftables.conf")).unwrap();
        assert!(content.contains("flush ruleset"), "missing flush ruleset");
        assert!(content.contains("table inet filter"), "missing inet filter table");
        assert!(content.contains("policy drop"), "missing drop policy");
        assert!(content.contains("tcp dport 2222 accept"), "missing custom ssh port");
    }
}

pub fn apply_nftables() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["restart", "nftables"])
        .status()
        .context("Failed to execute systemctl restart nftables")?;
    if !status.success() {
        bail!("systemctl restart nftables failed");
    }
    Ok(())
}
