use crate::socket::SocketPool;
use vmm_core::san_mgmt::MgmtCommand;

pub const ALUA_ACTIVE_OPTIMIZED: u8 = 0x00;
pub const ALUA_ACTIVE_NON_OPTIMIZED: u8 = 0x01;
pub const ALUA_STANDBY: u8 = 0x02;
pub const ALUA_UNAVAILABLE: u8 = 0x03;

pub async fn report_target_port_groups(socket: &SocketPool, volume_id: &str) -> Result<Vec<u8>, String> {
    let resp = socket.mgmt_request(MgmtCommand::GetTargetPortGroups, volume_id.as_bytes(), &[]).await?;
    if !resp.is_ok() { return Err("GetTargetPortGroups failed".into()); }

    let tpgs: Vec<serde_json::Value> = serde_json::from_slice(&resp.body).unwrap_or_default();
    let mut data = vec![0u8; 4]; // return data length placeholder

    for (i, tpg) in tpgs.iter().enumerate() {
        let state_str = tpg["state"].as_str().unwrap_or("active_non_optimized");
        let alua_state = match state_str {
            "active_optimized" => ALUA_ACTIVE_OPTIMIZED,
            "active_non_optimized" => ALUA_ACTIVE_NON_OPTIMIZED,
            "standby" => ALUA_STANDBY,
            _ => ALUA_UNAVAILABLE,
        };
        let tpg_id = (i + 1) as u16;
        data.push(alua_state);
        data.push(0x8F); // supported states
        data.extend_from_slice(&tpg_id.to_be_bytes());
        data.push(0); data.push(0); data.push(0);
        data.push(1); // 1 target port
        data.extend_from_slice(&[0, 0]);
        let port_id = (i + 1) as u16;
        data.extend_from_slice(&port_id.to_be_bytes());
    }

    let len = (data.len() - 4) as u32;
    data[0..4].copy_from_slice(&len.to_be_bytes());
    Ok(data)
}
