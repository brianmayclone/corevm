use std::sync::Arc;
use tokio::net::TcpStream;
use vmm_core::san_iscsi::IscsiCommand;
use crate::AppState;
use crate::pdu::*;

const SCSI_STATUS_GOOD: u8 = 0x00;
const SCSI_STATUS_CHECK_CONDITION: u8 = 0x02;

pub async fn handle_scsi_command(
    pdu: &Pdu, stat_sn: &mut u32, exp_cmd_sn: u32, max_cmd_sn: u32,
    volume_id: &str, state: &Arc<AppState>, stream: &mut TcpStream,
) -> Result<(), String> {
    let cdb = &pdu.bhs.specific[12..28];
    let opcode = cdb[0];
    let tag = pdu.bhs.initiator_task_tag;

    match opcode {
        0x00 => { // TEST UNIT READY
            scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD).write_to(stream).await?;
            *stat_sn += 1;
        }
        0x12 => { // INQUIRY
            let evpd = cdb[1] & 0x01;
            let page_code = cdb[2];
            let alloc_len = u16::from_be_bytes([cdb[3], cdb[4]]) as usize;
            let resp_data = if evpd == 0 { build_standard_inquiry() }
            else { match page_code { 0x00 => build_vpd_supported_pages(), 0x80 => build_vpd_unit_serial(volume_id), 0x83 => build_vpd_device_id(volume_id), _ => vec![0x70,0,0x05,0,0,0,0,0x0A,0,0,0,0,0x20,0,0,0,0,0] } };
            let len = resp_data.len().min(alloc_len);
            data_in(tag, *stat_sn, 0, 0, resp_data[..len].to_vec(), true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            *stat_sn += 1;
        }
        0x25 => { // READ CAPACITY(10)
            let cap = get_capacity(volume_id, state).await;
            let blocks = (cap.0 / cap.1).saturating_sub(1);
            let last_lba = if blocks > 0xFFFFFFFF { 0xFFFFFFFF_u32 } else { blocks as u32 };
            let mut d = vec![0u8; 8];
            d[0..4].copy_from_slice(&last_lba.to_be_bytes());
            d[4..8].copy_from_slice(&(cap.1 as u32).to_be_bytes());
            data_in(tag, *stat_sn, 0, 0, d, true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            *stat_sn += 1;
        }
        0x9E => { // SERVICE ACTION IN (READ CAPACITY 16 when SA=0x10)
            let sa = cdb[1] & 0x1F;
            if sa == 0x10 {
                let cap = get_capacity(volume_id, state).await;
                let last_lba = (cap.0 / cap.1).saturating_sub(1);
                let mut d = vec![0u8; 32];
                d[0..8].copy_from_slice(&last_lba.to_be_bytes());
                d[8..12].copy_from_slice(&(cap.1 as u32).to_be_bytes());
                let alloc = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]) as usize;
                let len = d.len().min(alloc);
                data_in(tag, *stat_sn, 0, 0, d[..len].to_vec(), true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            } else {
                scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?;
            }
            *stat_sn += 1;
        }
        0x1A | 0x5A => { // MODE SENSE 6/10
            let d = if opcode == 0x1A { vec![3,0,0,0] } else { vec![0,6,0,0,0,0,0,0] };
            data_in(tag, *stat_sn, 0, 0, d, true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            *stat_sn += 1;
        }
        0xA0 => { // REPORT LUNS
            let mut d = vec![0u8; 16];
            d[0..4].copy_from_slice(&8u32.to_be_bytes());
            data_in(tag, *stat_sn, 0, 0, d, true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            *stat_sn += 1;
        }
        0x28 => { // READ(10)
            let lba = u32::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5]]) as u64;
            let xfer = u16::from_be_bytes([cdb[7], cdb[8]]) as u32;
            let bytes = xfer * 512;
            match state.socket.block_request(volume_id, IscsiCommand::ReadBlocks, lba, bytes, &[]).await {
                Ok(r) if r.is_ok() => data_in(tag, *stat_sn, 0, 0, r.data, true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?,
                _ => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?,
            }
            *stat_sn += 1;
        }
        0x88 => { // READ(16)
            let lba = u64::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5], cdb[6], cdb[7], cdb[8], cdb[9]]);
            let xfer = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]);
            let bytes = xfer * 512;
            match state.socket.block_request(volume_id, IscsiCommand::ReadBlocks, lba, bytes, &[]).await {
                Ok(r) if r.is_ok() => data_in(tag, *stat_sn, 0, 0, r.data, true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?,
                _ => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?,
            }
            *stat_sn += 1;
        }
        0x2A => { // WRITE(10)
            let lba = u32::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5]]) as u64;
            let xfer = u16::from_be_bytes([cdb[7], cdb[8]]) as u32;
            let bytes = xfer * 512;
            let write_data = collect_write_data(pdu, stream, bytes as usize).await?;
            match state.socket.block_request(volume_id, IscsiCommand::WriteBlocks, lba, bytes, &write_data).await {
                Ok(r) if r.is_ok() => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD).write_to(stream).await?,
                _ => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?,
            }
            *stat_sn += 1;
        }
        0x8A => { // WRITE(16)
            let lba = u64::from_be_bytes([cdb[2], cdb[3], cdb[4], cdb[5], cdb[6], cdb[7], cdb[8], cdb[9]]);
            let xfer = u32::from_be_bytes([cdb[10], cdb[11], cdb[12], cdb[13]]);
            let bytes = xfer * 512;
            let write_data = collect_write_data(pdu, stream, bytes as usize).await?;
            match state.socket.block_request(volume_id, IscsiCommand::WriteBlocks, lba, bytes, &write_data).await {
                Ok(r) if r.is_ok() => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_GOOD).write_to(stream).await?,
                _ => scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?,
            }
            *stat_sn += 1;
        }
        0xA3 => { // MAINTENANCE IN (REPORT TARGET PORT GROUPS when SA=0x0A)
            let sa = cdb[1] & 0x1F;
            if sa == 0x0A {
                let d = crate::alua::report_target_port_groups(&state.socket, volume_id).await.unwrap_or_default();
                let alloc = u32::from_be_bytes([cdb[6], cdb[7], cdb[8], cdb[9]]) as usize;
                let len = d.len().min(alloc);
                data_in(tag, *stat_sn, 0, 0, d[..len].to_vec(), true, Some(SCSI_STATUS_GOOD)).write_to(stream).await?;
            } else {
                scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?;
            }
            *stat_sn += 1;
        }
        _ => {
            tracing::warn!("Unsupported SCSI opcode: 0x{:02X}", opcode);
            scsi_response(tag, *stat_sn, exp_cmd_sn, max_cmd_sn, SCSI_STATUS_CHECK_CONDITION).write_to(stream).await?;
            *stat_sn += 1;
        }
    }
    Ok(())
}

async fn collect_write_data(pdu: &Pdu, stream: &mut TcpStream, expected: usize) -> Result<Vec<u8>, String> {
    if pdu.data.len() >= expected { return Ok(pdu.data[..expected].to_vec()); }
    let mut buf = pdu.data.clone();
    while buf.len() < expected {
        let p = Pdu::read_from(stream).await?;
        if p.bhs.opcode != OPCODE_DATA_OUT { break; }
        buf.extend_from_slice(&p.data);
    }
    Ok(buf)
}

async fn get_capacity(volume_id: &str, state: &Arc<AppState>) -> (u64, u64) {
    match state.socket.block_request(volume_id, IscsiCommand::GetCapacity, 0, 0, &[]).await {
        Ok(r) if r.is_ok() => {
            let j: serde_json::Value = serde_json::from_slice(&r.data).unwrap_or_default();
            (j["size_bytes"].as_u64().unwrap_or(0), j["block_size"].as_u64().unwrap_or(512))
        }
        _ => (0, 512),
    }
}

fn build_standard_inquiry() -> Vec<u8> {
    let mut d = vec![0u8; 96];
    d[0] = 0x00; d[2] = 0x06; d[3] = 0x02; d[4] = 91; d[5] = 0x10;
    d[8..16].copy_from_slice(b"CoreVM  ");
    d[16..32].copy_from_slice(b"CoreSAN         ");
    d[32..36].copy_from_slice(b"0001");
    d
}

fn build_vpd_supported_pages() -> Vec<u8> { vec![0x00, 0x00, 0x00, 0x03, 0x00, 0x80, 0x83] }

fn build_vpd_unit_serial(volume_id: &str) -> Vec<u8> {
    let s = volume_id.as_bytes();
    let mut d = vec![0x00, 0x80, 0x00, s.len() as u8];
    d.extend_from_slice(s);
    d
}

fn build_vpd_device_id(volume_id: &str) -> Vec<u8> {
    let naa = format!("naa.6001405{}", &volume_id.replace('-', ""));
    let naa = &naa.as_bytes()[..naa.len().min(28)];
    let mut d = vec![0x00, 0x83, 0x00, 0x00];
    d.extend_from_slice(&[0x01, 0x03, 0x00, naa.len() as u8]);
    d.extend_from_slice(naa);
    let tpg: u16 = 1;
    d.extend_from_slice(&[0x01, 0x05, 0x00, 0x04, 0x00, 0x00]);
    d.extend_from_slice(&tpg.to_be_bytes());
    let pl = (d.len() - 4) as u16;
    d[2..4].copy_from_slice(&pl.to_be_bytes());
    d
}
