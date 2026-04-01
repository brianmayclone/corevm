use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use vmm_core::san_mgmt::*;
use vmm_core::san_iscsi::*;

pub struct MgmtResponse {
    pub status: u32,
    pub body: Vec<u8>,
}

impl MgmtResponse {
    pub fn is_ok(&self) -> bool { self.status == MgmtStatus::Ok as u32 }

    pub fn body_json(&self) -> serde_json::Value {
        if self.body.is_empty() { serde_json::Value::Null }
        else { serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null) }
    }
}

pub struct BlockResponse {
    pub status: u32,
    pub data: Vec<u8>,
}

impl BlockResponse {
    pub fn is_ok(&self) -> bool { self.status == IscsiStatus::Ok as u32 }
}

pub struct SocketPool {
    mgmt_path: String,
    block_socket_dir: String,
    mgmt_conn: Mutex<Option<UnixStream>>,
    blk_conns: Mutex<HashMap<String, UnixStream>>,
}

impl SocketPool {
    pub fn new(config: &crate::config::SanSection) -> Self {
        Self {
            mgmt_path: config.mgmt_socket.clone(),
            block_socket_dir: config.block_socket_dir.clone(),
            mgmt_conn: Mutex::new(None),
            blk_conns: Mutex::new(HashMap::new()),
        }
    }

    pub async fn mgmt_request(
        &self, cmd: MgmtCommand, key: &[u8], body: &[u8],
    ) -> Result<MgmtResponse, String> {
        let mut guard = self.mgmt_conn.lock().await;
        if guard.is_none() {
            let stream = UnixStream::connect(&self.mgmt_path).await
                .map_err(|e| format!("connect {}: {}", self.mgmt_path, e))?;
            *guard = Some(stream);
        }
        let result = Self::do_mgmt_request(guard.as_mut().unwrap(), cmd, key, body).await;
        if result.is_err() { *guard = None; }
        result
    }

    async fn do_mgmt_request(
        stream: &mut UnixStream, cmd: MgmtCommand, key: &[u8], body: &[u8],
    ) -> Result<MgmtResponse, String> {
        let header = MgmtRequestHeader::new(cmd, key.len() as u32, body.len() as u64);
        stream.write_all(&header.to_bytes()).await.map_err(|e| format!("write header: {}", e))?;
        if !key.is_empty() { stream.write_all(key).await.map_err(|e| format!("write key: {}", e))?; }
        if !body.is_empty() { stream.write_all(body).await.map_err(|e| format!("write body: {}", e))?; }
        stream.flush().await.map_err(|e| format!("flush: {}", e))?;

        let mut resp_buf = [0u8; MgmtResponseHeader::SIZE];
        stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp header: {}", e))?;
        let resp_header = MgmtResponseHeader::from_bytes(&resp_buf);
        if resp_header.magic != MGMT_RESPONSE_MAGIC { return Err("invalid mgmt response magic".into()); }

        let mut metadata = vec![0u8; resp_header.metadata_len as usize];
        if !metadata.is_empty() { stream.read_exact(&mut metadata).await.map_err(|e| format!("read metadata: {}", e))?; }
        let mut resp_body = vec![0u8; resp_header.body_len as usize];
        if !resp_body.is_empty() { stream.read_exact(&mut resp_body).await.map_err(|e| format!("read body: {}", e))?; }

        Ok(MgmtResponse { status: resp_header.status, body: resp_body })
    }

    pub async fn block_request(
        &self, volume_id: &str, cmd: IscsiCommand, lba: u64, length: u32, data: &[u8],
    ) -> Result<BlockResponse, String> {
        let mut guard = self.blk_conns.lock().await;
        let sock_path = format!("{}/blk-{}.sock", self.block_socket_dir, volume_id);

        if !guard.contains_key(volume_id) {
            let stream = UnixStream::connect(&sock_path).await
                .map_err(|e| format!("connect {}: {}", sock_path, e))?;
            guard.insert(volume_id.to_string(), stream);
        }

        let stream = guard.get_mut(volume_id).unwrap();
        let result = Self::do_block_request(stream, cmd, lba, length, data).await;
        if result.is_err() { guard.remove(volume_id); }
        result
    }

    async fn do_block_request(
        stream: &mut UnixStream, cmd: IscsiCommand, lba: u64, length: u32, data: &[u8],
    ) -> Result<BlockResponse, String> {
        let header = IscsiRequestHeader::new(cmd, lba, length);
        stream.write_all(&header.to_bytes()).await.map_err(|e| format!("write header: {}", e))?;
        if !data.is_empty() { stream.write_all(data).await.map_err(|e| format!("write data: {}", e))?; }
        stream.flush().await.map_err(|e| format!("flush: {}", e))?;

        let mut resp_buf = [0u8; IscsiResponseHeader::SIZE];
        stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp header: {}", e))?;
        let resp_header = IscsiResponseHeader::from_bytes(&resp_buf);
        if resp_header.magic != ISCSI_RESPONSE_MAGIC { return Err("invalid block response magic".into()); }

        let mut resp_data = vec![0u8; resp_header.length as usize];
        if !resp_data.is_empty() { stream.read_exact(&mut resp_data).await.map_err(|e| format!("read data: {}", e))?; }

        Ok(BlockResponse { status: resp_header.status, data: resp_data })
    }
}
