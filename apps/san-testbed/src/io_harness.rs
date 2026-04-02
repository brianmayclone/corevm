//! SAN disk I/O harness — UDS client with ground-truth verification.

use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const REQUEST_MAGIC: u32 = 0x53414E31; // "SAN1"
const RESPONSE_MAGIC: u32 = 0x53414E52; // "SANR"

const CMD_OPEN: u32 = 0;
const CMD_READ: u32 = 1;
const CMD_WRITE: u32 = 2;
const CMD_FLUSH: u32 = 3;
const CMD_CLOSE: u32 = 4;
#[allow(dead_code)]
const CMD_GETSIZE: u32 = 5;

const CHUNK_SIZE: u64 = 4 * 1024 * 1024; // 4 MB

/// Build a 32-byte SAN request header.
fn request_header(cmd: u32, file_id: u64, offset: u64, size: u32) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[0..4].copy_from_slice(&REQUEST_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&cmd.to_le_bytes());
    buf[8..16].copy_from_slice(&file_id.to_le_bytes());
    buf[16..24].copy_from_slice(&offset.to_le_bytes());
    buf[24..28].copy_from_slice(&size.to_le_bytes());
    // flags = 0
    buf
}

/// Parse a 16-byte SAN response header. Returns (status, data_size).
fn parse_response(buf: &[u8; 16]) -> (u32, u32) {
    let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    assert_eq!(magic, RESPONSE_MAGIC, "bad response magic");
    let status = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let size = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    (status, size)
}

/// Detailed mismatch information for verification failures.
pub struct VerifyError {
    pub offset: u64,
    pub expected: u8,
    pub actual: u8,
    pub chunk_index: u32,
    pub local_offset: u64,
    pub context_expected: Vec<u8>,
    pub context_actual: Vec<u8>,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DATA MISMATCH at byte offset {} (chunk {} + local_offset {}): expected 0x{:02X}, got 0x{:02X}\n\
             Expected (±32 bytes): {:02X?}\n\
             Actual   (±32 bytes): {:02X?}",
            self.offset, self.chunk_index, self.local_offset,
            self.expected, self.actual,
            self.context_expected, self.context_actual,
        )
    }
}

/// Performance statistics for a test run.
pub struct IoStats {
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub write_ops: u64,
    pub read_ops: u64,
    pub flush_ops: u64,
    pub write_duration: std::time::Duration,
    pub read_duration: std::time::Duration,
}

impl IoStats {
    fn new() -> Self {
        Self {
            bytes_written: 0,
            bytes_read: 0,
            write_ops: 0,
            read_ops: 0,
            flush_ops: 0,
            write_duration: std::time::Duration::ZERO,
            read_duration: std::time::Duration::ZERO,
        }
    }

    pub fn write_mbps(&self) -> f64 {
        let secs = self.write_duration.as_secs_f64();
        if secs > 0.0 { self.bytes_written as f64 / 1_048_576.0 / secs } else { 0.0 }
    }

    pub fn read_mbps(&self) -> f64 {
        let secs = self.read_duration.as_secs_f64();
        if secs > 0.0 { self.bytes_read as f64 / 1_048_576.0 / secs } else { 0.0 }
    }

    pub fn write_iops(&self) -> f64 {
        let secs = self.write_duration.as_secs_f64();
        if secs > 0.0 { self.write_ops as f64 / secs } else { 0.0 }
    }

    pub fn read_iops(&self) -> f64 {
        let secs = self.read_duration.as_secs_f64();
        if secs > 0.0 { self.read_ops as f64 / secs } else { 0.0 }
    }
}

/// I/O test harness: SAN UDS client + ground-truth buffer + verification.
pub struct IoHarness {
    stream: UnixStream,
    file_id: i64,
    pub ground_truth: Vec<u8>,
    pub stats: IoStats,
}

impl IoHarness {
    /// Connect to the vmm-san UDS and open a file.
    pub async fn open(socket_path: &str, rel_path: &str, file_size: u64) -> Result<Self, String> {
        let stream = UnixStream::connect(socket_path).await
            .map_err(|e| format!("UDS connect {}: {}", socket_path, e))?;

        let mut harness = Self {
            stream,
            file_id: 0,
            ground_truth: vec![0u8; file_size as usize],
            stats: IoStats::new(),
        };

        // Send CMD_OPEN with rel_path as payload
        let path_bytes = rel_path.as_bytes();
        let hdr = request_header(CMD_OPEN, 0, 0, path_bytes.len() as u32);
        harness.stream.write_all(&hdr).await.map_err(|e| format!("open write hdr: {}", e))?;
        harness.stream.write_all(path_bytes).await.map_err(|e| format!("open write path: {}", e))?;

        // Read response
        let mut resp_buf = [0u8; 16];
        harness.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("open read resp: {}", e))?;
        let (status, data_size) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_OPEN failed: status={}", status));
        }

        // Read file_id from response data
        if data_size >= 8 {
            let mut id_buf = [0u8; 8];
            harness.stream.read_exact(&mut id_buf).await.map_err(|e| format!("open read id: {}", e))?;
            harness.file_id = i64::from_le_bytes(id_buf);
            // Drain any extra response bytes
            if data_size > 8 {
                let mut discard = vec![0u8; (data_size - 8) as usize];
                harness.stream.read_exact(&mut discard).await.ok();
            }
        }

        tracing::debug!("IoHarness: opened '{}' file_id={}", rel_path, harness.file_id);
        Ok(harness)
    }

    /// Write data at offset. Updates ground truth.
    pub async fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), String> {
        let start = Instant::now();

        let hdr = request_header(CMD_WRITE, self.file_id as u64, offset, data.len() as u32);
        self.stream.write_all(&hdr).await.map_err(|e| format!("write hdr: {}", e))?;
        self.stream.write_all(data).await.map_err(|e| format!("write data: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("write resp: {}", e))?;
        let (status, _) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_WRITE failed: status={}", status));
        }

        let elapsed = start.elapsed();
        self.stats.bytes_written += data.len() as u64;
        self.stats.write_ops += 1;
        self.stats.write_duration += elapsed;

        // Update ground truth
        let end = offset as usize + data.len();
        if end > self.ground_truth.len() {
            self.ground_truth.resize(end, 0);
        }
        self.ground_truth[offset as usize..end].copy_from_slice(data);

        Ok(())
    }

    /// Read data at offset.
    pub async fn read(&mut self, offset: u64, size: u64) -> Result<Vec<u8>, String> {
        let start = Instant::now();

        let hdr = request_header(CMD_READ, self.file_id as u64, offset, size as u32);
        self.stream.write_all(&hdr).await.map_err(|e| format!("read hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("read resp: {}", e))?;
        let (status, data_size) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_READ failed: status={}", status));
        }

        let mut data = vec![0u8; data_size as usize];
        if data_size > 0 {
            self.stream.read_exact(&mut data).await.map_err(|e| format!("read data: {}", e))?;
        }

        let elapsed = start.elapsed();
        self.stats.bytes_read += data.len() as u64;
        self.stats.read_ops += 1;
        self.stats.read_duration += elapsed;

        Ok(data)
    }

    /// Flush all dirty cached chunks to disk.
    pub async fn flush(&mut self) -> Result<(), String> {
        let hdr = request_header(CMD_FLUSH, self.file_id as u64, 0, 0);
        self.stream.write_all(&hdr).await.map_err(|e| format!("flush hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("flush resp: {}", e))?;
        let (status, _) = parse_response(&resp_buf);
        if status != 0 {
            return Err(format!("CMD_FLUSH failed: status={}", status));
        }
        self.stats.flush_ops += 1;
        Ok(())
    }

    /// Close the file handle.
    pub async fn close(mut self) -> Result<(), String> {
        let hdr = request_header(CMD_CLOSE, self.file_id as u64, 0, 0);
        self.stream.write_all(&hdr).await.map_err(|e| format!("close hdr: {}", e))?;

        let mut resp_buf = [0u8; 16];
        self.stream.read_exact(&mut resp_buf).await.map_err(|e| format!("close resp: {}", e))?;
        Ok(())
    }

    /// Flush, then read the entire file and compare against ground truth.
    /// Returns Ok(()) if all bytes match, or Err with detailed mismatch info.
    pub async fn verify_all(&mut self) -> Result<(), String> {
        self.flush().await?;

        let read_block: u64 = 256 * 1024; // 256 KB read blocks
        let file_size = self.ground_truth.len() as u64;
        let mut offset: u64 = 0;

        while offset < file_size {
            let remaining = file_size - offset;
            let block = remaining.min(read_block);
            let actual = self.read(offset, block).await?;
            let expected = &self.ground_truth[offset as usize..(offset + block) as usize];

            if actual.len() != expected.len() {
                return Err(format!(
                    "Size mismatch at offset {}: expected {} bytes, got {}",
                    offset, expected.len(), actual.len()
                ));
            }

            // Find first mismatch
            for i in 0..actual.len() {
                if actual[i] != expected[i] {
                    let abs_offset = offset + i as u64;
                    let chunk_index = (abs_offset / CHUNK_SIZE) as u32;
                    let local_offset = abs_offset % CHUNK_SIZE;

                    // Extract context (±32 bytes)
                    let ctx_start = if i >= 32 { i - 32 } else { 0 };
                    let ctx_end = (i + 32).min(actual.len());

                    let err = VerifyError {
                        offset: abs_offset,
                        expected: expected[i],
                        actual: actual[i],
                        chunk_index,
                        local_offset,
                        context_expected: expected[ctx_start..ctx_end].to_vec(),
                        context_actual: actual[ctx_start..ctx_end].to_vec(),
                    };
                    return Err(err.to_string());
                }
            }

            offset += block;
        }

        Ok(())
    }

    /// Reset stats for a fresh measurement.
    pub fn reset_stats(&mut self) {
        self.stats = IoStats::new();
    }
}
