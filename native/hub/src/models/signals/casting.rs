use rinf::{DartSignal, RustSignal};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, RustSignal)]
pub struct CastingStatusChanged {
    /// True if Casting/Casting.exe exists in the app data directory (Windows only)
    pub installed: bool,
    /// Absolute path to Casting.exe when installed (Windows only)
    pub exe_path: Option<String>,
    /// Error string if operation failed
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetCastingStatusRequest {}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct DownloadCastingBundleRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct CastingDownloadProgress {
    /// Bytes received so far
    pub received: u64,
    /// Total bytes if known
    pub total: Option<u64>,
}
