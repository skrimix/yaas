use rinf::{DartSignal, RustSignal};

#[derive(serde::Serialize, serde::Deserialize, DartSignal)]
pub(crate) struct InstallDownloaderConfigRequest {
    pub source_path: String,
}

#[derive(serde::Serialize, serde::Deserialize, DartSignal)]
pub(crate) struct RetryDownloaderInitRequest {}

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub(crate) struct DownloaderConfigInstallResult {
    pub success: bool,
    pub error: Option<String>,
}
