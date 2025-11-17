use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) struct DownloadEntry {
    pub path: String,
    pub name: String,
    /// Milliseconds since Unix epoch
    pub timestamp: u64,
    /// Total size of this directory in bytes
    pub total_size: u64,
    /// Optional package metadata
    pub package_name: Option<String>,
    pub version_code: Option<u32>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetDownloadsRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct GetDownloadsResponse {
    pub entries: Vec<DownloadEntry>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct DownloadsChanged {}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetDownloadsDirectoryRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct GetDownloadsDirectoryResponse {
    pub path: String,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DeleteDownloadRequest {
    pub path: String,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct DeleteDownloadResponse {
    pub path: String,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DeleteAllDownloadsRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct DeleteAllDownloadsResponse {
    pub removed: u32,
    pub skipped: u32,
    pub error: Option<String>,
}
