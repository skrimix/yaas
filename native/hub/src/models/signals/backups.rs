use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, SignalPiece)]
pub(crate) struct BackupEntry {
    pub path: String,
    pub name: String,
    /// Milliseconds since Unix epoch
    pub timestamp: u64,
    /// Total size of this backup directory in bytes
    pub total_size: u64,
    pub has_apk: bool,
    pub has_private_data: bool,
    pub has_shared_data: bool,
    pub has_obb: bool,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetBackupsRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct GetBackupsResponse {
    pub entries: Vec<BackupEntry>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DeleteBackupRequest {
    pub path: String,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct DeleteBackupResponse {
    pub path: String,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct BackupsChanged {}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetBackupsDirectoryRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct GetBackupsDirectoryResponse {
    pub path: String,
}
