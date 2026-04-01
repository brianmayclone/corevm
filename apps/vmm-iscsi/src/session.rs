use std::sync::Arc;
use tokio::net::TcpStream;
use vmm_core::san_mgmt::MgmtCommand;
use crate::AppState;
use crate::pdu::*;

pub async fn handle_connection(mut stream: TcpStream, state: Arc<AppState>) -> Result<(), String> {
    let pdu = Pdu::read_from(&mut stream).await?;
    if pdu.bhs.opcode != OPCODE_LOGIN_REQ {
        return Err(format!("expected Login, got 0x{:02X}", pdu.bhs.opcode));
    }

    let params = parse_text_params(&pdu.data);
    let initiator_name = params.iter().find(|(k,_)| k == "InitiatorName").map(|(_,v)| v.clone()).unwrap_or_default();
    let target_name = params.iter().find(|(k,_)| k == "TargetName").map(|(_,v)| v.clone()).unwrap_or_default();
    let session_type = params.iter().find(|(k,_)| k == "SessionType").map(|(_,v)| v.clone()).unwrap_or_else(|| "Normal".into());
    let is_discovery = session_type == "Discovery";

    tracing::info!("iSCSI login: initiator={} target={} type={}", initiator_name, target_name, session_type);

    let exp_cmd_sn = u32::from_be_bytes(pdu.bhs.specific[8..12].try_into().unwrap_or([0;4]));
    let mut stat_sn: u32 = 1;
    let max_cmd_sn: u32 = exp_cmd_sn + 32;
    let mut volume_id = String::new();

    if !is_discovery {
        let vol_name = target_name.rsplit(':').next().unwrap_or("");
        let resp = state.socket.mgmt_request(MgmtCommand::ResolveVolume, vol_name.as_bytes(), &[]).await?;
        if !resp.is_ok() {
            login_response(pdu.bhs.initiator_task_tag, 0x02, 0x00, stat_sn, exp_cmd_sn, max_cmd_sn, false, vec![]).write_to(&mut stream).await?;
            return Err(format!("volume '{}' not found", vol_name));
        }
        let meta = resp.body_json();
        volume_id = meta["id"].as_str().unwrap_or("").to_string();

        // Check ACL
        let acl_resp = state.socket.mgmt_request(MgmtCommand::ListIscsiAcls, volume_id.as_bytes(), &[]).await?;
        if acl_resp.is_ok() {
            let acls: Vec<serde_json::Value> = serde_json::from_slice(&acl_resp.body).unwrap_or_default();
            if !acls.is_empty() && !acls.iter().any(|a| a["initiator_iqn"].as_str() == Some(&initiator_name)) {
                login_response(pdu.bhs.initiator_task_tag, 0x02, 0x01, stat_sn, exp_cmd_sn, max_cmd_sn, false, vec![]).write_to(&mut stream).await?;
                return Err(format!("initiator '{}' not in ACL", initiator_name));
            }
        }
    }

    let resp_params = encode_text_params(&[
        ("HeaderDigest", "None"), ("DataDigest", "None"),
        ("MaxRecvDataSegmentLength", "65536"), ("MaxBurstLength", "262144"),
        ("FirstBurstLength", "65536"), ("DefaultTime2Wait", "2"),
        ("DefaultTime2Retain", "20"), ("MaxOutstandingR2T", "1"),
        ("InitialR2T", "Yes"), ("ImmediateData", "Yes"),
        ("MaxConnections", "1"), ("ErrorRecoveryLevel", "0"),
    ]);

    login_response(pdu.bhs.initiator_task_tag, 0x00, 0x00, stat_sn, exp_cmd_sn, max_cmd_sn, true, resp_params).write_to(&mut stream).await?;
    stat_sn += 1;

    tracing::info!("iSCSI login OK: initiator={} volume_id={}", initiator_name, volume_id);

    loop {
        let pdu = match Pdu::read_from(&mut stream).await { Ok(p) => p, Err(_) => break };

        match pdu.bhs.opcode {
            OPCODE_SCSI_CMD => {
                crate::scsi::handle_scsi_command(&pdu, &mut stat_sn, exp_cmd_sn, max_cmd_sn, &volume_id, &state, &mut stream).await?;
            }
            OPCODE_TEXT_REQ => {
                let data = crate::discovery::build_send_targets(&state.socket, &state.config.server.node_name).await.unwrap_or_default();
                text_response(pdu.bhs.initiator_task_tag, stat_sn, exp_cmd_sn, max_cmd_sn, data).write_to(&mut stream).await?;
                stat_sn += 1;
            }
            OPCODE_NOP_OUT => {
                nop_in(pdu.bhs.initiator_task_tag, stat_sn, exp_cmd_sn, max_cmd_sn).write_to(&mut stream).await?;
                stat_sn += 1;
            }
            OPCODE_LOGOUT_REQ => {
                logout_response(pdu.bhs.initiator_task_tag, stat_sn, exp_cmd_sn, max_cmd_sn).write_to(&mut stream).await?;
                tracing::info!("iSCSI logout: initiator={}", initiator_name);
                break;
            }
            other => { tracing::warn!("Unhandled opcode: 0x{:02X}", other); }
        }
    }
    Ok(())
}
