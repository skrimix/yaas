use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

use crate::models::UpdateChannel;

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetAppUpdateStateRequest {}
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct CheckAppUpdateRequest {}
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DownloadAppUpdateRequest {
    pub candidate_id: String,
}
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct InstallAppUpdateRequest {
    pub candidate_id: String,
}
/// Cancels a check, download, or pending exit. An armed installation cannot be cancelled.
#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct CancelAppUpdateRequest {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub(crate) enum AppUpdatePhase {
    Idle,
    Checking,
    Available,
    UpToDate,
    Downloading,
    Ready,
    Preparing,
    AwaitingExit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, SignalPiece)]
pub(crate) enum AppUpdateErrorKind {
    Network,
    NoRelease,
    IncompleteRelease,
    InvalidMetadata,
    NoPackage,
    Integrity,
    Installation,
    InvalidRequest,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub(crate) struct AppUpdateRelease {
    pub candidate_id: String,
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
    pub error_kind: Option<AppUpdateErrorKind>,
    pub error: Option<String>,
}

/// Flutter should request a cancellable application exit using this candidate ID.
#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct AppUpdateExitRequested {
    pub candidate_id: String,
}
