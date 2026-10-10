use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

use crate::models::UpdateChannel;

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetAppUpdateStateRequest {}
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct CheckAppUpdateRequest {}
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DownloadAppUpdateRequest {}
/// Cancels a check or download.
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct CancelAppUpdateRequest {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub(crate) enum AppUpdatePhase {
    Idle,
    Checking,
    Available,
    UpToDate,
    /// Downloading, verifying, and staging the package.
    Downloading,
    /// Staged; installed when the app exits with `AppShutdownRequest::install_update`.
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub(crate) struct AppUpdateRelease {
    pub version: String,
    pub build_number: u32,
    pub channel: UpdateChannel,
    pub commit: String,
    pub run_number: u64,
    pub run_attempt: u64,
    pub notes: String,
    pub release_url: String,
    pub package_size: u64,
}

/// A complete snapshot; request it when attaching a new consumer.
#[derive(Debug, Clone, Serialize, Deserialize, RustSignal)]
pub(crate) struct AppUpdateStateChanged {
    pub channel: UpdateChannel,
    pub phase: AppUpdatePhase,
    pub release: Option<AppUpdateRelease>,
    pub received_bytes: u64,
    pub installation_unavailable_reason: Option<String>,
    pub error: Option<String>,
}
