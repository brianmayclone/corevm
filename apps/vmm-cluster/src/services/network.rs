//! NetworkService — Software Defined Networking for the cluster.
//!
//! Manages virtual networks with integrated DHCP, DNS, and PXE services.
//! Networks are cluster-wide and automatically configured on all hosts.

use rusqlite::Connection;
use serde::Serialize;

pub struct NetworkService;

#[derive(Debug, Serialize, Clone)]
pub struct VirtualNetwork {
    pub id: i64,
    pub cluster_id: String,
    pub name: String,
    pub vlan_id: Option<i32>,
    pub subnet: String,
    pub gateway: String,
    pub dhcp_enabled: bool,
    pub dhcp_range_start: String,
    pub dhcp_range_end: String,
    pub dhcp_lease_secs: i64,
    pub dns_enabled: bool,
    pub dns_domain: String,
    pub dns_upstream: String,
    pub pxe_enabled: bool,
    pub pxe_boot_file: String,
    pub pxe_tftp_root: String,
    pub pxe_next_server: String,
    pub auto_register_dns: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct DhcpLease {
    pub id: i64,
    pub mac_address: String,
    pub ip_address: String,
    pub hostname: Option<String>,
    pub vm_id: Option<String>,
    pub lease_start: String,
    pub lease_end: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DnsRecord {
    pub id: i64,
    pub record_type: String,
    pub name: String,
    pub value: String,
    pub ttl: i64,
    pub auto_registered: bool,
}

impl NetworkService {
    // ── Virtual Networks ─────────────────────────────────────────────

    pub fn list_networks(db: &Connection, cluster_id: Option<&str>) -> Result<Vec<VirtualNetwork>, String> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(cid) = cluster_id {
            ("SELECT id, cluster_id, name, vlan_id, subnet, gateway, \
                    dhcp_enabled, dhcp_range_start, dhcp_range_end, dhcp_lease_secs, \
                    dns_enabled, dns_domain, dns_upstream, \
                    pxe_enabled, pxe_boot_file, pxe_tftp_root, pxe_next_server, \
                    auto_register_dns, created_at \
             FROM virtual_networks WHERE cluster_id = ?1 ORDER BY name".into(),
             vec![Box::new(cid.to_string())])
        } else {
            ("SELECT id, cluster_id, name, vlan_id, subnet, gateway, \
                    dhcp_enabled, dhcp_range_start, dhcp_range_end, dhcp_lease_secs, \
                    dns_enabled, dns_domain, dns_upstream, \
                    pxe_enabled, pxe_boot_file, pxe_tftp_root, pxe_next_server, \
                    auto_register_dns, created_at \
             FROM virtual_networks ORDER BY name".into(),
             vec![])
        };
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(VirtualNetwork {
                id: row.get(0)?, cluster_id: row.get(1)?, name: row.get(2)?,
                vlan_id: row.get(3)?, subnet: row.get(4)?, gateway: row.get(5)?,
                dhcp_enabled: row.get::<_, i32>(6)? != 0,
                dhcp_range_start: row.get(7)?, dhcp_range_end: row.get(8)?,
                dhcp_lease_secs: row.get(9)?,
                dns_enabled: row.get::<_, i32>(10)? != 0,
                dns_domain: row.get(11)?, dns_upstream: row.get(12)?,
                pxe_enabled: row.get::<_, i32>(13)? != 0,
                pxe_boot_file: row.get(14)?, pxe_tftp_root: row.get(15)?,
                pxe_next_server: row.get(16)?,
                auto_register_dns: row.get::<_, i32>(17)? != 0,
                created_at: row.get(18)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_network(db: &Connection, id: i64) -> Result<VirtualNetwork, String> {
        db.query_row(
            "SELECT id, cluster_id, name, vlan_id, subnet, gateway, \
                    dhcp_enabled, dhcp_range_start, dhcp_range_end, dhcp_lease_secs, \
                    dns_enabled, dns_domain, dns_upstream, \
                    pxe_enabled, pxe_boot_file, pxe_tftp_root, pxe_next_server, \
                    auto_register_dns, created_at \
             FROM virtual_networks WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok(VirtualNetwork {
                id: row.get(0)?, cluster_id: row.get(1)?, name: row.get(2)?,
                vlan_id: row.get(3)?, subnet: row.get(4)?, gateway: row.get(5)?,
                dhcp_enabled: row.get::<_, i32>(6)? != 0,
                dhcp_range_start: row.get(7)?, dhcp_range_end: row.get(8)?,
                dhcp_lease_secs: row.get(9)?,
                dns_enabled: row.get::<_, i32>(10)? != 0,
                dns_domain: row.get(11)?, dns_upstream: row.get(12)?,
                pxe_enabled: row.get::<_, i32>(13)? != 0,
                pxe_boot_file: row.get(14)?, pxe_tftp_root: row.get(15)?,
                pxe_next_server: row.get(16)?,
                auto_register_dns: row.get::<_, i32>(17)? != 0,
                created_at: row.get(18)?,
            }),
        ).map_err(|_| "Network not found".to_string())
    }

    pub fn create_network(db: &Connection, cluster_id: &str, name: &str, subnet: &str,
                          gateway: &str, vlan_id: Option<i32>) -> Result<i64, String> {
        db.execute(
            "INSERT INTO virtual_networks (cluster_id, name, subnet, gateway, vlan_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![cluster_id, name, subnet, gateway, vlan_id],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn update_network(db: &Connection, id: i64, updates: &serde_json::Value) -> Result<(), String> {
        // Dynamic update from JSON — apply each field that's present
        let fields = [
            ("name", "name"), ("subnet", "subnet"), ("gateway", "gateway"),
            ("dhcp_range_start", "dhcp_range_start"), ("dhcp_range_end", "dhcp_range_end"),
            ("dns_domain", "dns_domain"), ("dns_upstream", "dns_upstream"),
            ("pxe_boot_file", "pxe_boot_file"), ("pxe_tftp_root", "pxe_tftp_root"),
            ("pxe_next_server", "pxe_next_server"),
        ];
        for (json_key, col) in &fields {
            if let Some(val) = updates.get(json_key).and_then(|v| v.as_str()) {
                db.execute(&format!("UPDATE virtual_networks SET {} = ?1 WHERE id = ?2", col),
                    rusqlite::params![val, id]).map_err(|e| e.to_string())?;
            }
        }
        let bool_fields = [
            ("dhcp_enabled", "dhcp_enabled"), ("dns_enabled", "dns_enabled"),
            ("pxe_enabled", "pxe_enabled"), ("auto_register_dns", "auto_register_dns"),
        ];
        for (json_key, col) in &bool_fields {
            if let Some(val) = updates.get(json_key).and_then(|v| v.as_bool()) {
                db.execute(&format!("UPDATE virtual_networks SET {} = ?1 WHERE id = ?2", col),
                    rusqlite::params![val as i32, id]).map_err(|e| e.to_string())?;
            }
        }
        let int_fields = [("vlan_id", "vlan_id"), ("dhcp_lease_secs", "dhcp_lease_secs")];
        for (json_key, col) in &int_fields {
            if let Some(val) = updates.get(json_key).and_then(|v| v.as_i64()) {
                db.execute(&format!("UPDATE virtual_networks SET {} = ?1 WHERE id = ?2", col),
                    rusqlite::params![val, id]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn delete_network(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM virtual_networks WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── DHCP Leases ─────────────────────────────────────────────────

    pub fn list_leases(db: &Connection, network_id: i64) -> Result<Vec<DhcpLease>, String> {
        let mut stmt = db.prepare(
            "SELECT id, mac_address, ip_address, hostname, vm_id, lease_start, lease_end \
             FROM dhcp_leases WHERE network_service_id = ?1 ORDER BY ip_address"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![network_id], |row| {
            Ok(DhcpLease {
                id: row.get(0)?, mac_address: row.get(1)?, ip_address: row.get(2)?,
                hostname: row.get(3)?, vm_id: row.get(4)?,
                lease_start: row.get(5)?, lease_end: row.get(6)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── DNS Records ─────────────────────────────────────────────────

    pub fn list_dns_records(db: &Connection, network_id: i64) -> Result<Vec<DnsRecord>, String> {
        let mut stmt = db.prepare(
            "SELECT id, record_type, name, value, ttl, auto_registered \
             FROM dns_records WHERE network_service_id = ?1 ORDER BY name"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![network_id], |row| {
            Ok(DnsRecord {
                id: row.get(0)?, record_type: row.get(1)?, name: row.get(2)?,
                value: row.get(3)?, ttl: row.get(4)?,
                auto_registered: row.get::<_, i32>(5)? != 0,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Auto-register a DNS record for a VM (called when VM starts).
    pub fn auto_register_vm_dns(db: &Connection, network_id: i64, vm_name: &str, ip: &str, domain: &str) {
        let fqdn = format!("{}.{}", vm_name.to_lowercase().replace(' ', "-"), domain);
        let _ = db.execute(
            "INSERT OR REPLACE INTO dns_records (network_service_id, record_type, name, value, auto_registered) \
             VALUES (?1, 'A', ?2, ?3, 1)",
            rusqlite::params![network_id, &fqdn, ip],
        );
    }
}
