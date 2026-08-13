use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct CastingStatusChanged {
    /// True if Casting/Casting.exe exists in the app data directory (Windows only)
    pub installed: bool,
    /// Absolute path to Casting.exe when installed (Windows only)
    pub exe_path: Option<String>,
    /// Error string if operation failed
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetCastingStatusRequest {}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct DownloadCastingBundleRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct CastingDownloadProgress {
    /// Bytes received so far
    pub received: u64,
    /// Total bytes if known
    pub total: Option<u64>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct StartNativeCastingRequest {
    /// Enable device audio, muxed into the stream
    pub audio: bool,
    /// Requested capture frame rate (0 = device default, about 30)
    pub fps: u32,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct StopNativeCastingRequest {}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct GetNativeCastingStateRequest {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, SignalPiece)]
pub(crate) enum NativeCastingState {
    /// No casting session is running
    Idle,
    Starting,
    /// The device stream dropped and a recovery attempt is in progress
    Reconnecting,
    /// Paced playback is running and the stream is being served
    Streaming,
}

#[derive(Serialize, Deserialize, RustSignal, Clone)]
pub(crate) struct NativeCastingStateChanged {
    pub state: NativeCastingState,
    /// URL of the live Matroska HTTP stream; set once the session has started
    pub url: Option<String>,
    /// Error string if the session failed
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct NativeCastingStats {
    /// Measured input frame rate over the last second
    pub fps: f64,
    /// Frames currently held in the playback buffer
    pub buffered_frames: u32,
    /// Age of the oldest buffered frame in milliseconds
    pub buffer_age_ms: f64,
    /// Mean presentation latency over the last second in milliseconds
    pub latency_ms: f64,
}
