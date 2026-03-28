//! ViSwitchService — virtual switch management for the cluster.
//!
//! Manages viSwitches with configurable uplinks, teaming policies,
//! traffic types (vm, san), and VM port assignments.

use rusqlite::Connection;
use serde::Serialize;

pub struct ViSwitchService;

#[derive(Debug, Serialize, Clone)]
pub struct ViSwitch {
    pub id: i64,
    pub cluster_id: String,
    pub name: String,
    pub description: String,
    pub max_ports: i32,
    pub max_uplinks: i32,
    pub mtu: i32,
    pub uplink_policy: String,
    pub uplink_rules: String,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ViSwitchUplinkRow {
    pub id: i64,
    pub viswitch_id: i64,
    pub uplink_index: i32,
    pub uplink_type: String,
    pub physical_nic: String,
    pub network_id: Option<i64>,
    pub network_name: Option<String>,
    pub active: bool,
    pub traffic_types: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ViSwitchPort {
    pub id: i64,
    pub viswitch_id: i64,
    pub port_index: i32,
    pub vm_id: Option<String>,
    pub vm_name: Option<String>,
    pub vlan_id: Option<i32>,
    pub created_at: String,
}

impl ViSwitchService {
    // ── viSwitch CRUD ───────────────────────────────────────────────

    pub fn list_viswitches(db: &Connection, cluster_id: Option<&str>) -> Result<Vec<ViSwitch>, String> {
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(cid) = cluster_id {
            ("SELECT id, cluster_id, name, description, max_ports, max_uplinks, \
                    mtu, uplink_policy, uplink_rules, enabled, created_at \
             FROM viswitches WHERE cluster_id = ?1 ORDER BY name".into(),
             vec![Box::new(cid.to_string())])
        } else {
            ("SELECT id, cluster_id, name, description, max_ports, max_uplinks, \
                    mtu, uplink_policy, uplink_rules, enabled, created_at \
             FROM viswitches ORDER BY name".into(),
             vec![])
        };
        let mut stmt = db.prepare(&sql).map_err(|e| e.to_string())?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(ViSwitch {
                id: row.get(0)?, cluster_id: row.get(1)?, name: row.get(2)?,
                description: row.get(3)?, max_ports: row.get(4)?, max_uplinks: row.get(5)?,
                mtu: row.get(6)?, uplink_policy: row.get(7)?, uplink_rules: row.get(8)?,
                enabled: row.get::<_, i32>(9)? != 0, created_at: row.get(10)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_viswitch(db: &Connection, id: i64) -> Result<ViSwitch, String> {
        db.query_row(
            "SELECT id, cluster_id, name, description, max_ports, max_uplinks, \
                    mtu, uplink_policy, uplink_rules, enabled, created_at \
             FROM viswitches WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok(ViSwitch {
                id: row.get(0)?, cluster_id: row.get(1)?, name: row.get(2)?,
                description: row.get(3)?, max_ports: row.get(4)?, max_uplinks: row.get(5)?,
                mtu: row.get(6)?, uplink_policy: row.get(7)?, uplink_rules: row.get(8)?,
                enabled: row.get::<_, i32>(9)? != 0, created_at: row.get(10)?,
            }),
        ).map_err(|_| "viSwitch not found".to_string())
    }

    pub fn create_viswitch(
        db: &Connection, cluster_id: &str, name: &str, description: &str,
        max_ports: i32, max_uplinks: i32, mtu: i32, uplink_policy: &str,
    ) -> Result<i64, String> {
        db.execute(
            "INSERT INTO viswitches (cluster_id, name, description, max_ports, max_uplinks, mtu, uplink_policy) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![cluster_id, name, description, max_ports, max_uplinks, mtu, uplink_policy],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn update_viswitch(db: &Connection, id: i64, updates: &serde_json::Value) -> Result<(), String> {
        let str_fields = [
            ("name", "name"), ("description", "description"),
            ("uplink_policy", "uplink_policy"), ("uplink_rules", "uplink_rules"),
        ];
        for (json_key, col) in &str_fields {
            if let Some(val) = updates.get(json_key).and_then(|v| v.as_str()) {
                db.execute(&format!("UPDATE viswitches SET {} = ?1 WHERE id = ?2", col),
                    rusqlite::params![val, id]).map_err(|e| e.to_string())?;
            }
        }
        let int_fields = [
            ("max_ports", "max_ports"), ("max_uplinks", "max_uplinks"), ("mtu", "mtu"),
        ];
        for (json_key, col) in &int_fields {
            if let Some(val) = updates.get(json_key).and_then(|v| v.as_i64()) {
                db.execute(&format!("UPDATE viswitches SET {} = ?1 WHERE id = ?2", col),
                    rusqlite::params![val, id]).map_err(|e| e.to_string())?;
            }
        }
        if let Some(val) = updates.get("enabled").and_then(|v| v.as_bool()) {
            db.execute("UPDATE viswitches SET enabled = ?1 WHERE id = ?2",
                rusqlite::params![val as i32, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_viswitch(db: &Connection, id: i64) -> Result<(), String> {
        db.execute("DELETE FROM viswitches WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Uplinks ─────────────────────────────────────────────────────

    pub fn list_uplinks(db: &Connection, viswitch_id: i64) -> Result<Vec<ViSwitchUplinkRow>, String> {
        let mut stmt = db.prepare(
            "SELECT u.id, u.viswitch_id, u.uplink_index, u.uplink_type, u.physical_nic, \
                    u.network_id, n.name, u.active, u.traffic_types, u.created_at \
             FROM viswitch_uplinks u \
             LEFT JOIN virtual_networks n ON u.network_id = n.id \
             WHERE u.viswitch_id = ?1 ORDER BY u.uplink_index"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![viswitch_id], |row| {
            Ok(ViSwitchUplinkRow {
                id: row.get(0)?, viswitch_id: row.get(1)?, uplink_index: row.get(2)?,
                uplink_type: row.get(3)?, physical_nic: row.get(4)?,
                network_id: row.get(5)?, network_name: row.get(6)?,
                active: row.get::<_, i32>(7)? != 0, traffic_types: row.get(8)?,
                created_at: row.get(9)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn add_uplink(
        db: &Connection, viswitch_id: i64, uplink_type: &str, physical_nic: &str,
        network_id: Option<i64>, active: bool, traffic_types: &str,
    ) -> Result<i64, String> {
        // Auto-assign next free uplink_index
        let max_idx: i32 = db.query_row(
            "SELECT COALESCE(MAX(uplink_index), -1) FROM viswitch_uplinks WHERE viswitch_id = ?1",
            rusqlite::params![viswitch_id],
            |row| row.get(0),
        ).unwrap_or(-1);
        let next_idx = max_idx + 1;

        // Check max_uplinks limit
        let max_uplinks: i32 = db.query_row(
            "SELECT max_uplinks FROM viswitches WHERE id = ?1",
            rusqlite::params![viswitch_id],
            |row| row.get(0),
        ).map_err(|_| "viSwitch not found".to_string())?;
        if next_idx >= max_uplinks {
            return Err(format!("Maximum uplinks ({}) reached", max_uplinks));
        }

        db.execute(
            "INSERT INTO viswitch_uplinks (viswitch_id, uplink_index, uplink_type, physical_nic, network_id, active, traffic_types) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![viswitch_id, next_idx, uplink_type, physical_nic, network_id, active as i32, traffic_types],
        ).map_err(|e| e.to_string())?;
        Ok(db.last_insert_rowid())
    }

    pub fn remove_uplink(db: &Connection, uplink_id: i64) -> Result<(), String> {
        db.execute("DELETE FROM viswitch_uplinks WHERE id = ?1", rusqlite::params![uplink_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Ports (VM assignments) ──────────────────────────────────────

    pub fn list_ports(db: &Connection, viswitch_id: i64) -> Result<Vec<ViSwitchPort>, String> {
        let mut stmt = db.prepare(
            "SELECT p.id, p.viswitch_id, p.port_index, p.vm_id, v.name, p.vlan_id, p.created_at \
             FROM viswitch_ports p \
             LEFT JOIN vms v ON p.vm_id = v.id \
             WHERE p.viswitch_id = ?1 ORDER BY p.port_index"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![viswitch_id], |row| {
            Ok(ViSwitchPort {
                id: row.get(0)?, viswitch_id: row.get(1)?, port_index: row.get(2)?,
                vm_id: row.get(3)?, vm_name: row.get(4)?,
                vlan_id: row.get(5)?, created_at: row.get(6)?,
            })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Allocate the next free port for a VM. Returns the port_index.
    pub fn assign_port(db: &Connection, viswitch_id: i64, vm_id: &str, vlan_id: Option<i32>) -> Result<i32, String> {
        let max_ports: i32 = db.query_row(
            "SELECT max_ports FROM viswitches WHERE id = ?1",
            rusqlite::params![viswitch_id],
            |row| row.get(0),
        ).map_err(|_| "viSwitch not found".to_string())?;

        // Find the lowest free port index
        let used: Vec<i32> = {
            let mut stmt = db.prepare(
                "SELECT port_index FROM viswitch_ports WHERE viswitch_id = ?1 ORDER BY port_index"
            ).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(rusqlite::params![viswitch_id], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|r| r.ok()).collect()
        };

        let mut next_port = 0i32;
        for &idx in &used {
            if idx != next_port { break; }
            next_port += 1;
        }

        if next_port >= max_ports {
            return Err(format!("All {} ports in use", max_ports));
        }

        db.execute(
            "INSERT INTO viswitch_ports (viswitch_id, port_index, vm_id, vlan_id) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![viswitch_id, next_port, vm_id, vlan_id],
        ).map_err(|e| e.to_string())?;

        Ok(next_port)
    }

    /// Release a VM's port on a viSwitch.
    pub fn release_port(db: &Connection, viswitch_id: i64, vm_id: &str) -> Result<(), String> {
        db.execute(
            "DELETE FROM viswitch_ports WHERE viswitch_id = ?1 AND vm_id = ?2",
            rusqlite::params![viswitch_id, vm_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get the bridge name for a viSwitch (e.g. "vs42").
    pub fn bridge_name(viswitch_id: i64) -> String {
        format!("vs{}", viswitch_id)
    }
}
