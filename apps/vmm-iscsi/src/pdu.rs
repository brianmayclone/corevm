//! iSCSI PDU (Protocol Data Unit) parsing and serialization.
//! Implements RFC 3720 wire format for the minimal PDU set.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const OPCODE_NOP_OUT: u8 = 0x00;
pub const OPCODE_SCSI_CMD: u8 = 0x01;
pub const OPCODE_LOGIN_REQ: u8 = 0x03;
pub const OPCODE_TEXT_REQ: u8 = 0x04;
pub const OPCODE_DATA_OUT: u8 = 0x05;
pub const OPCODE_LOGOUT_REQ: u8 = 0x06;

pub const OPCODE_NOP_IN: u8 = 0x20;
pub const OPCODE_SCSI_RESP: u8 = 0x21;
pub const OPCODE_LOGIN_RESP: u8 = 0x23;
pub const OPCODE_TEXT_RESP: u8 = 0x24;
pub const OPCODE_DATA_IN: u8 = 0x25;
pub const OPCODE_LOGOUT_RESP: u8 = 0x26;

/// Basic Header Segment (BHS) — 48 bytes.
#[derive(Debug, Clone)]
pub struct Bhs {
    pub opcode: u8,
    pub flags: u8,
    pub data_segment_length: u32,
    pub lun: u64,
    pub initiator_task_tag: u32,
    pub specific: [u8; 28],
}

impl Bhs {
    pub const SIZE: usize = 48;

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        let opcode = buf[0] & 0x3F;
        let flags = buf[1];
        let data_segment_length =
            ((buf[5] as u32) << 16) | ((buf[6] as u32) << 8) | (buf[7] as u32);
        let lun = u64::from_be_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]);
        let initiator_task_tag = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
        let mut specific = [0u8; 28];
        specific.copy_from_slice(&buf[20..48]);
        Self { opcode, flags, data_segment_length, lun, initiator_task_tag, specific }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0] = self.opcode;
        buf[1] = self.flags;
        buf[5] = ((self.data_segment_length >> 16) & 0xFF) as u8;
        buf[6] = ((self.data_segment_length >> 8) & 0xFF) as u8;
        buf[7] = (self.data_segment_length & 0xFF) as u8;
        buf[8..16].copy_from_slice(&self.lun.to_be_bytes());
        buf[16..20].copy_from_slice(&self.initiator_task_tag.to_be_bytes());
        buf[20..48].copy_from_slice(&self.specific);
        buf
    }
}

#[derive(Debug, Clone)]
pub struct Pdu {
    pub bhs: Bhs,
    pub data: Vec<u8>,
}

impl Pdu {
    pub async fn read_from(stream: &mut TcpStream) -> Result<Self, String> {
        let mut hdr_buf = [0u8; Bhs::SIZE];
        stream.read_exact(&mut hdr_buf).await.map_err(|e| format!("read BHS: {}", e))?;
        let bhs = Bhs::from_bytes(&hdr_buf);
        let data_len = bhs.data_segment_length as usize;
        let padded_len = (data_len + 3) & !3;
        let mut data = vec![0u8; padded_len];
        if padded_len > 0 {
            stream.read_exact(&mut data).await.map_err(|e| format!("read data: {}", e))?;
        }
        data.truncate(data_len);
        Ok(Pdu { bhs, data })
    }

    pub async fn write_to(&self, stream: &mut TcpStream) -> Result<(), String> {
        stream.write_all(&self.bhs.to_bytes()).await.map_err(|e| format!("write BHS: {}", e))?;
        if !self.data.is_empty() {
            stream.write_all(&self.data).await.map_err(|e| format!("write data: {}", e))?;
            let pad = (4 - (self.data.len() % 4)) % 4;
            if pad > 0 {
                stream.write_all(&vec![0u8; pad]).await.map_err(|e| format!("write pad: {}", e))?;
            }
        }
        stream.flush().await.map_err(|e| format!("flush: {}", e))?;
        Ok(())
    }
}

pub fn login_response(tag: u32, status_class: u8, status_detail: u8, stat_sn: u32, exp_cmd_sn: u32, max_cmd_sn: u32, transit: bool, data: Vec<u8>) -> Pdu {
    let mut bhs = Bhs { opcode: OPCODE_LOGIN_RESP, flags: if transit { 0x87 } else { 0x04 }, data_segment_length: data.len() as u32, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    bhs.specific[16] = status_class;
    bhs.specific[17] = status_detail;
    Pdu { bhs, data }
}

pub fn text_response(tag: u32, stat_sn: u32, exp_cmd_sn: u32, max_cmd_sn: u32, data: Vec<u8>) -> Pdu {
    let mut bhs = Bhs { opcode: OPCODE_TEXT_RESP, flags: 0x80, data_segment_length: data.len() as u32, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    Pdu { bhs, data }
}

pub fn scsi_response(tag: u32, stat_sn: u32, exp_cmd_sn: u32, max_cmd_sn: u32, scsi_status: u8) -> Pdu {
    let mut bhs = Bhs { opcode: OPCODE_SCSI_RESP, flags: 0x80, data_segment_length: 0, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    bhs.specific[1] = scsi_status;
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    Pdu { bhs, data: vec![] }
}

pub fn data_in(tag: u32, stat_sn: u32, data_sn: u32, buffer_offset: u32, data: Vec<u8>, is_final: bool, scsi_status: Option<u8>) -> Pdu {
    let mut flags: u8 = 0;
    if is_final { flags |= 0x80; }
    if scsi_status.is_some() { flags |= 0x01; }
    let mut bhs = Bhs { opcode: OPCODE_DATA_IN, flags, data_segment_length: data.len() as u32, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    if let Some(s) = scsi_status { bhs.specific[1] = s; }
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[16..20].copy_from_slice(&data_sn.to_be_bytes());
    bhs.specific[20..24].copy_from_slice(&buffer_offset.to_be_bytes());
    Pdu { bhs, data }
}

pub fn nop_in(tag: u32, stat_sn: u32, exp_cmd_sn: u32, max_cmd_sn: u32) -> Pdu {
    let mut bhs = Bhs { opcode: OPCODE_NOP_IN, flags: 0x80, data_segment_length: 0, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    bhs.specific[0..4].copy_from_slice(&0xFFFFFFFFu32.to_be_bytes());
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    Pdu { bhs, data: vec![] }
}

pub fn logout_response(tag: u32, stat_sn: u32, exp_cmd_sn: u32, max_cmd_sn: u32) -> Pdu {
    let mut bhs = Bhs { opcode: OPCODE_LOGOUT_RESP, flags: 0x80, data_segment_length: 0, lun: 0, initiator_task_tag: tag, specific: [0u8; 28] };
    bhs.specific[4..8].copy_from_slice(&stat_sn.to_be_bytes());
    bhs.specific[8..12].copy_from_slice(&exp_cmd_sn.to_be_bytes());
    bhs.specific[12..16].copy_from_slice(&max_cmd_sn.to_be_bytes());
    Pdu { bhs, data: vec![] }
}

pub fn parse_text_params(data: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(data).split('\0').filter(|s| !s.is_empty()).filter_map(|s| {
        let mut parts = s.splitn(2, '=');
        Some((parts.next()?.to_string(), parts.next()?.to_string()))
    }).collect()
}

pub fn encode_text_params(params: &[(&str, &str)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (k, v) in params { data.extend_from_slice(format!("{}={}", k, v).as_bytes()); data.push(0); }
    data
}
