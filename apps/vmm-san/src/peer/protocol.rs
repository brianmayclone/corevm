//! Wire types for peer-to-peer data transfer.

use serde::{Serialize, Deserialize};

/// Request to transfer a file to a peer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileTransferRequest {
    pub volume_id: String,
    pub rel_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Request to pull a file from a peer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilePullRequest {
    pub volume_id: String,
    pub rel_path: String,
}

/// Request to delete a file replica on a peer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileDeleteRequest {
    pub volume_id: String,
    pub rel_path: String,
}

/// Request to verify file checksum on a peer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChecksumVerifyRequest {
    pub volume_id: String,
    pub rel_path: String,
}

/// Response to a checksum verification request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChecksumVerifyResponse {
    pub volume_id: String,
    pub rel_path: String,
    pub sha256: String,
    pub exists: bool,
}

/// Benchmark data payload (for throughput testing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchmarkPayload {
    pub from_node_id: String,
    pub timestamp_ns: u64,
    pub sequence: u32,
}
