//! S.M.A.R.T. disk health monitoring — reads smartctl data and parses it.
//!
//! Supports SATA/SAS (classic SMART attributes) and NVMe (nvme health log).
//! Virtual disks (virtio) return `supported: false`.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

/// Summary of SMART health — embedded in DiscoveredDisk API responses.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SmartSummary {
    pub supported: bool,
    pub health_passed: Option<bool>,
    pub temperature_celsius: Option<i32>,
    pub power_on_hours: Option<u64>,
    pub reallocated_sectors: Option<u64>,
    pub wear_leveling_pct: Option<u8>,
}

/// Full SMART data — returned by the detail endpoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SmartData {
    pub device_path: String,
    pub supported: bool,
    pub health_passed: Option<bool>,
    pub transport: String,
    pub power_on_hours: Option<u64>,
    pub temperature_celsius: Option<i32>,
    pub reallocated_sectors: Option<u64>,
    pub pending_sectors: Option<u64>,
    pub uncorrectable_sectors: Option<u64>,
    pub wear_leveling_pct: Option<u8>,
    pub media_errors: Option<u64>,
    pub percentage_used: Option<u8>,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub collected_at: String,
    pub raw_json: Option<String>,
}

impl SmartData {
    pub fn unsupported(device_path: &str) -> Self {
        SmartData {
            device_path: device_path.to_string(),
            supported: false,
            collected_at: chrono::Utc::now().to_rfc3339(),
            ..Default::default()
        }
    }

    pub fn to_summary(&self) -> SmartSummary {
        SmartSummary {
            supported: self.supported,
            health_passed: self.health_passed,
            temperature_celsius: self.temperature_celsius,
            power_on_hours: self.power_on_hours,
            reallocated_sectors: self.reallocated_sectors,
            wear_leveling_pct: self.wear_leveling_pct,
        }
    }

    /// Returns true if this disk has any warning condition.
    pub fn has_warning(&self) -> bool {
        if !self.supported { return false; }
        self.health_passed == Some(false)
            || self.reallocated_sectors.unwrap_or(0) > 0
            || self.pending_sectors.unwrap_or(0) > 0
            || self.uncorrectable_sectors.unwrap_or(0) > 0
            || self.temperature_celsius.unwrap_or(0) > 60
            || self.media_errors.unwrap_or(0) > 0
    }

    /// Severity level: "critical", "warning", or "ok".
    pub fn severity(&self) -> &'static str {
        if !self.supported { return "unknown"; }
        if self.health_passed == Some(false) || self.uncorrectable_sectors.unwrap_or(0) > 0 {
            return "critical";
        }
        if self.reallocated_sectors.unwrap_or(0) > 0
            || self.pending_sectors.unwrap_or(0) > 0
            || self.temperature_celsius.unwrap_or(0) > 55
            || self.media_errors.unwrap_or(0) > 0 {
            return "warning";
        }
        "ok"
    }
}

/// Read SMART data from a single device using smartctl.
/// Returns SmartData with supported=false if the device doesn't support SMART.
pub fn read_smart(device_path: &str) -> SmartData {
    // Skip virtual disks — they never support SMART
    let dev_name = device_path.rsplit('/').next().unwrap_or("");
    if dev_name.starts_with("vd") || dev_name.starts_with("loop") || dev_name.starts_with("ram") {
        return SmartData::unsupported(device_path);
    }

    // Execute smartctl with JSON output and 10s timeout
    let output = match Command::new("smartctl")
        .args(["-a", "-j", "--nocheck=standby", device_path])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::debug!("smartctl not available for {}: {}", device_path, e);
            return SmartData::unsupported(device_path);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    // smartctl exit codes: bit 0 = command parse error, bit 1 = device open failed
    // bit 2 = SMART/ATA command failed. Codes 1-2 mean "no SMART support".
    let exit = output.status.code().unwrap_or(1);
    if exit & 0x03 != 0 && stdout.len() < 10 {
        return SmartData::unsupported(device_path);
    }

    // Parse JSON
    let json: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(j) => j,
        Err(_) => return SmartData::unsupported(device_path),
    };

    let now = chrono::Utc::now().to_rfc3339();

    // Check if SMART is supported
    let smart_supported = json["smart_support"]["is_available"].as_bool().unwrap_or(false);
    if !smart_supported {
        return SmartData {
            device_path: device_path.to_string(),
            supported: false,
            collected_at: now,
            raw_json: Some(stdout.to_string()),
            ..Default::default()
        };
    }

    // Health assessment
    let health_passed = json["smart_status"]["passed"].as_bool();

    // Device info
    let model = json["model_name"].as_str().unwrap_or("").to_string();
    let serial = json["serial_number"].as_str().unwrap_or("").to_string();
    let firmware = json["firmware_version"].as_str().unwrap_or("").to_string();

    // Detect transport
    let transport = json["device"]["type"].as_str()
        .or_else(|| json["device"]["protocol"].as_str())
        .unwrap_or("unknown").to_string();

    let is_nvme = transport == "nvme" || json.get("nvme_smart_health_information_log").is_some();

    if is_nvme {
        parse_nvme_smart(device_path, &json, health_passed, &model, &serial, &firmware, &now, &stdout)
    } else {
        parse_ata_smart(device_path, &json, health_passed, &model, &serial, &firmware, &transport, &now, &stdout)
    }
}

/// Parse NVMe SMART health log.
fn parse_nvme_smart(
    device_path: &str, json: &serde_json::Value,
    health_passed: Option<bool>, model: &str, serial: &str, firmware: &str,
    now: &str, raw: &str,
) -> SmartData {
    let log = &json["nvme_smart_health_information_log"];

    SmartData {
        device_path: device_path.to_string(),
        supported: true,
        health_passed,
        transport: "nvme".to_string(),
        power_on_hours: log["power_on_hours"].as_u64(),
        temperature_celsius: log["temperature"].as_i64().map(|t| t as i32),
        media_errors: log["media_errors"].as_u64(),
        percentage_used: log["percentage_used"].as_u64().map(|p| p as u8),
        model: model.to_string(),
        serial: serial.to_string(),
        firmware: firmware.to_string(),
        collected_at: now.to_string(),
        raw_json: Some(raw.to_string()),
        ..Default::default()
    }
}

/// Parse ATA/SATA/SAS SMART attributes by ID.
fn parse_ata_smart(
    device_path: &str, json: &serde_json::Value,
    health_passed: Option<bool>, model: &str, serial: &str, firmware: &str,
    transport: &str, now: &str, raw: &str,
) -> SmartData {
    let attrs = json["ata_smart_attributes"]["table"].as_array();

    let find_attr = |id: u64| -> Option<u64> {
        attrs?.iter().find(|a| a["id"].as_u64() == Some(id))
            .and_then(|a| a["raw"]["value"].as_u64())
    };

    // Temperature: try attribute 194, then 190, then the temperature field
    let temp = find_attr(194).or_else(|| find_attr(190))
        .map(|t| (t & 0xFF) as i32) // raw value often has extra bytes
        .or_else(|| json["temperature"]["current"].as_i64().map(|t| t as i32));

    SmartData {
        device_path: device_path.to_string(),
        supported: true,
        health_passed,
        transport: transport.to_string(),
        power_on_hours: find_attr(9).or_else(|| json["power_on_time"]["hours"].as_u64()),
        temperature_celsius: temp,
        reallocated_sectors: find_attr(5),
        pending_sectors: find_attr(197),
        uncorrectable_sectors: find_attr(198),
        wear_leveling_pct: find_attr(177).map(|v| v as u8)
            .or_else(|| find_attr(231).map(|v| v as u8)), // SSD Life Left
        model: model.to_string(),
        serial: serial.to_string(),
        firmware: firmware.to_string(),
        collected_at: now.to_string(),
        raw_json: Some(raw.to_string()),
        ..Default::default()
    }
}

/// Read SMART data from all given devices.
pub fn read_smart_all(device_paths: &[String]) -> Vec<SmartData> {
    device_paths.iter().map(|p| read_smart(p)).collect()
}
