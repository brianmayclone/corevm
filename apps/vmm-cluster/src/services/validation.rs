//! Input validation — validates IP addresses, subnets, VLAN IDs, etc.

use std::net::Ipv4Addr;

/// Validate an IPv4 address string.
pub fn validate_ipv4(ip: &str) -> Result<Ipv4Addr, String> {
    ip.parse::<Ipv4Addr>()
        .map_err(|_| format!("'{}' is not a valid IPv4 address", ip))
}

/// Validate a CIDR subnet (e.g. "10.0.0.0/24").
pub fn validate_cidr(cidr: &str) -> Result<(Ipv4Addr, u8), String> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(format!("'{}' is not valid CIDR (expected x.x.x.x/y)", cidr));
    }
    let ip = validate_ipv4(parts[0])?;
    let prefix: u8 = parts[1].parse()
        .map_err(|_| format!("Invalid prefix length '{}' (must be 0-32)", parts[1]))?;
    if prefix > 32 {
        return Err(format!("Prefix length {} is out of range (0-32)", prefix));
    }
    Ok((ip, prefix))
}

/// Validate that an IP is within a given subnet.
pub fn validate_ip_in_subnet(ip: &str, cidr: &str) -> Result<(), String> {
    let addr = validate_ipv4(ip)?;
    let (net, prefix) = validate_cidr(cidr)?;
    let mask = if prefix == 0 { 0u32 } else { !0u32 << (32 - prefix) };
    let net_u32 = u32::from(net);
    let addr_u32 = u32::from(addr);
    if (addr_u32 & mask) != (net_u32 & mask) {
        return Err(format!("'{}' is not within subnet '{}'", ip, cidr));
    }
    Ok(())
}

/// Validate a VLAN ID (1-4094).
pub fn validate_vlan(vlan_id: i32) -> Result<(), String> {
    if vlan_id < 1 || vlan_id > 4094 {
        return Err(format!("VLAN ID {} is out of range (1-4094)", vlan_id));
    }
    Ok(())
}

/// Validate a DHCP range — checks IPs are valid, in subnet, properly ordered,
/// and don't include the gateway address.
pub fn validate_dhcp_range(start: &str, end: &str, cidr: &str, gateway: &str) -> Result<(), String> {
    let start_ip = validate_ipv4(start)?;
    let end_ip = validate_ipv4(end)?;
    let gw_ip = validate_ipv4(gateway)?;
    validate_ip_in_subnet(start, cidr)?;
    validate_ip_in_subnet(end, cidr)?;
    if u32::from(start_ip) >= u32::from(end_ip) {
        return Err(format!("DHCP range start ({}) must be less than end ({})", start, end));
    }
    // Gateway must not be within DHCP range
    let gw_u32 = u32::from(gw_ip);
    let start_u32 = u32::from(start_ip);
    let end_u32 = u32::from(end_ip);
    if gw_u32 >= start_u32 && gw_u32 <= end_u32 {
        return Err(format!("Gateway {} is within DHCP range {}-{} — this will cause conflicts", gateway, start, end));
    }
    Ok(())
}

/// Validate a MAC address format (XX:XX:XX:XX:XX:XX).
pub fn validate_mac(mac: &str) -> Result<(), String> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return Err(format!("'{}' is not a valid MAC address (expected XX:XX:XX:XX:XX:XX)", mac));
    }
    for part in &parts {
        if part.len() != 2 || u8::from_str_radix(part, 16).is_err() {
            return Err(format!("'{}' contains invalid hex octet '{}'", mac, part));
        }
    }
    Ok(())
}

/// Validate VM config basics.
pub fn validate_vm_config(name: &str, cpu_cores: u32, ram_mb: u32) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("VM name is required".into());
    }
    if cpu_cores == 0 || cpu_cores > 128 {
        return Err(format!("CPU cores must be 1-128 (got {})", cpu_cores));
    }
    if ram_mb < 32 || ram_mb > 1048576 {
        return Err(format!("RAM must be 32-1048576 MB (got {})", ram_mb));
    }
    Ok(())
}

/// Validate NFS mount source format (host:/path).
pub fn validate_nfs_source(source: &str) -> Result<(), String> {
    if !source.contains(':') || source.starts_with(':') {
        return Err(format!("NFS source '{}' must be in format 'host:/path'", source));
    }
    let parts: Vec<&str> = source.splitn(2, ':').collect();
    if parts.len() != 2 || parts[1].is_empty() || !parts[1].starts_with('/') {
        return Err(format!("NFS source '{}' must be in format 'host:/path'", source));
    }
    Ok(())
}
