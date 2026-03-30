//! SAN Disk I/O client — connects to vmm-san's disk server via Unix Domain Socket.
//!
//! Provides pread/pwrite operations that go directly to vmm-san over UDS.
//! Used by vmm-server to serve VM disk I/O without FUSE.
//!
//! This module is ADDITIVE — does NOT change existing fd-based I/O.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

// Inline protocol constants (same as vmm_core::san_disk but without the dependency)
const REQUEST_MAGIC: u32 = 0x53414E31;

// Command codes matching vmm_core::san_disk::SanCommand
const CMD_OPEN: u32 = 0;
const CMD_READ: u32 = 1;
const CMD_WRITE: u32 = 2;
const CMD_FLUSH: u32 = 3;
const CMD_CLOSE: u32 = 4;
const CMD_GETSIZE: u32 = 5;

fn socket_path(volume_id: &str) -> String {
    format!("/run/vmm-san/{}.sock", volume_id)
}

#[repr(C)]
struct SanRequestHeader {
    magic: u32, cmd: u32, file_id: u64, offset: u64, size: u32, flags: u32,
}
impl SanRequestHeader {
    fn to_bytes(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0..4].copy_from_slice(&self.magic.to_le_bytes());
        b[4..8].copy_from_slice(&self.cmd.to_le_bytes());
        b[8..16].copy_from_slice(&self.file_id.to_le_bytes());
        b[16..24].copy_from_slice(&self.offset.to_le_bytes());
        b[24..28].copy_from_slice(&self.size.to_le_bytes());
        b[28..32].copy_from_slice(&self.flags.to_le_bytes());
        b
    }
}

struct SanResponseHeader { _magic: u32, status: u32, size: u32, _reserved: u32 }
impl SanResponseHeader {
    const SIZE: usize = 16;
    fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            _magic: u32::from_le_bytes([b[0],b[1],b[2],b[3]]),
            status: u32::from_le_bytes([b[4],b[5],b[6],b[7]]),
            size: u32::from_le_bytes([b[8],b[9],b[10],b[11]]),
            _reserved: u32::from_le_bytes([b[12],b[13],b[14],b[15]]),
        }
    }
    fn is_ok(&self) -> bool { self.status == 0 }
}

/// Connection to a SAN disk — provides pread/pwrite over UDS.
pub struct SanDiskConnection {
    stream: Mutex<UnixStream>,
    pub file_id: u64,
    pub disk_size: u64,
    pub volume_id: String,
    pub rel_path: String,
}

impl SanDiskConnection {
    /// Open a SAN disk file. Connects to vmm-san, acquires write lease, returns connection.
    pub fn open(volume_id: &str, rel_path: &str) -> Result<Self, String> {
        let sock_path = socket_path(volume_id);

        let mut stream = UnixStream::connect(&sock_path)
            .map_err(|e| format!("Cannot connect to SAN at {}: {}", sock_path, e))?;

        stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
        stream.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();

        // OPEN
        let path_bytes = rel_path.as_bytes();
        let req = SanRequestHeader {
            magic: REQUEST_MAGIC, cmd: CMD_OPEN,
            file_id: 0, offset: 0, size: path_bytes.len() as u32, flags: 0,
        };
        stream.write_all(&req.to_bytes()).map_err(|e| format!("Send OPEN: {}", e))?;
        stream.write_all(path_bytes).map_err(|e| format!("Send path: {}", e))?;

        let resp = read_response(&mut stream)?;
        if !resp.is_ok() {
            return Err(format!("SAN OPEN failed: status={}", resp.status));
        }
        let mut fid_buf = [0u8; 8];
        stream.read_exact(&mut fid_buf).map_err(|e| format!("Read file_id: {}", e))?;
        let file_id = u64::from_le_bytes(fid_buf);

        // GETSIZE
        let req = SanRequestHeader {
            magic: REQUEST_MAGIC, cmd: CMD_GETSIZE,
            file_id, offset: 0, size: 0, flags: 0,
        };
        stream.write_all(&req.to_bytes()).map_err(|e| format!("Send GETSIZE: {}", e))?;
        let resp = read_response(&mut stream)?;
        let disk_size = if resp.is_ok() && resp.size == 8 {
            let mut buf = [0u8; 8];
            stream.read_exact(&mut buf).map_err(|e| format!("Read size: {}", e))?;
            u64::from_le_bytes(buf)
        } else {
            return Err("Cannot get disk size".into());
        };

        Ok(SanDiskConnection {
            stream: Mutex::new(stream),
            file_id, disk_size,
            volume_id: volume_id.to_string(),
            rel_path: rel_path.to_string(),
        })
    }

    /// Read data at offset — equivalent to pread(fd, buf, size, offset).
    pub fn pread(&self, buf: &mut [u8], offset: u64) -> Result<usize, String> {
        let mut stream = self.stream.lock().map_err(|_| "Lock error")?;

        let req = SanRequestHeader {
            magic: REQUEST_MAGIC, cmd: CMD_READ,
            file_id: self.file_id, offset, size: buf.len() as u32, flags: 0,
        };
        stream.write_all(&req.to_bytes()).map_err(|e| format!("Send READ: {}", e))?;

        let resp = read_response_from(&mut *stream)?;
        if !resp.is_ok() {
            return Err(format!("SAN READ failed: status={}", resp.status));
        }

        let to_read = (resp.size as usize).min(buf.len());
        stream.read_exact(&mut buf[..to_read]).map_err(|e| format!("Read data: {}", e))?;
        Ok(to_read)
    }

    /// Write data at offset — equivalent to pwrite(fd, buf, size, offset).
    pub fn pwrite(&self, buf: &[u8], offset: u64) -> Result<(), String> {
        let mut stream = self.stream.lock().map_err(|_| "Lock error")?;

        let req = SanRequestHeader {
            magic: REQUEST_MAGIC, cmd: CMD_WRITE,
            file_id: self.file_id, offset, size: buf.len() as u32, flags: 0,
        };
        stream.write_all(&req.to_bytes()).map_err(|e| format!("Send WRITE: {}", e))?;
        stream.write_all(buf).map_err(|e| format!("Send data: {}", e))?;

        let resp = read_response_from(&mut *stream)?;
        if !resp.is_ok() {
            return Err(format!("SAN WRITE failed: status={}", resp.status));
        }
        Ok(())
    }

    /// Flush cached data to disk.
    pub fn flush(&self) -> Result<(), String> {
        let mut stream = self.stream.lock().map_err(|_| "Lock error")?;

        let req = SanRequestHeader {
            magic: REQUEST_MAGIC, cmd: CMD_FLUSH,
            file_id: self.file_id, offset: 0, size: 0, flags: 0,
        };
        stream.write_all(&req.to_bytes()).map_err(|e| format!("Send FLUSH: {}", e))?;
        let _ = read_response_from(&mut *stream);
        Ok(())
    }
}

impl Drop for SanDiskConnection {
    fn drop(&mut self) {
        if let Ok(mut stream) = self.stream.lock() {
            let req = SanRequestHeader {
                magic: REQUEST_MAGIC, cmd: CMD_CLOSE,
                file_id: self.file_id, offset: 0, size: 0, flags: 0,
            };
            stream.write_all(&req.to_bytes()).ok();
            let mut resp_buf = [0u8; SanResponseHeader::SIZE];
            stream.read_exact(&mut resp_buf).ok();
        }
    }
}

fn read_response(stream: &mut UnixStream) -> Result<SanResponseHeader, String> {
    read_response_from(stream)
}

fn read_response_from(stream: &mut dyn Read) -> Result<SanResponseHeader, String> {
    let mut buf = [0u8; SanResponseHeader::SIZE];
    stream.read_exact(&mut buf).map_err(|e| format!("Read response: {}", e))?;
    Ok(SanResponseHeader::from_bytes(&buf))
}

/// Implement DiskIoBackend so SanDiskConnection can be used directly by AHCI.
impl crate::devices::ahci::DiskIoBackend for SanDiskConnection {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        self.pread(buf, offset)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> std::io::Result<()> {
        self.pwrite(buf, offset)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }

    fn flush(&self) -> std::io::Result<()> {
        SanDiskConnection::flush(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}
