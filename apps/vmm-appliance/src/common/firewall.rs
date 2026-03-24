use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{bail, Context, Result};

pub struct FirewallConfig {
    pub ssh_port: u16,
    pub vmm_server_port: Option<u16>,
    pub vmm_cluster_port: Option<u16>,
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
