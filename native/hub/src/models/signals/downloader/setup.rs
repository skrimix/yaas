use rinf::{DartSignal, RustSignal};

#[derive(serde::Serialize, serde::Deserialize, DartSignal)]
pub struct InstallDownloaderConfigRequest {
    pub source_path: String,
}

#[derive(serde::Serialize, serde::Deserialize, DartSignal)]
pub struct RetryDownloaderInitRequest {}

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub struct DownloaderConfigInstallResult {
    pub success: bool,
    pub error: Option<String>,
}
