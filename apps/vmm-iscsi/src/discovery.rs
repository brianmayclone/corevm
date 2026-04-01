use crate::socket::SocketPool;
use vmm_core::san_mgmt::MgmtCommand;

pub async fn build_send_targets(socket: &SocketPool, node_name: &str) -> Result<Vec<u8>, String> {
    let resp = socket.mgmt_request(MgmtCommand::ListIscsiVolumes, &[], &[]).await?;
    if !resp.is_ok() { return Err("ListIscsiVolumes failed".into()); }

    let volumes: Vec<serde_json::Value> = serde_json::from_slice(&resp.body).unwrap_or_default();
    let mut data = Vec::new();
    for vol in &volumes {
        let name = vol["name"].as_str().unwrap_or("unknown");
        data.extend_from_slice(format!("TargetName={}:{}", node_name, name).as_bytes());
        data.push(0);
        data.extend_from_slice(b"TargetAddress=0.0.0.0:3260,1");
        data.push(0);
    }
    Ok(data)
}
