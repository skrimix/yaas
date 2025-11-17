use std::time::Duration;

use rinf::RustSignal;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct RustPanic {
    pub message: String,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct Toast {
    /// Title of the toast
    pub title: String,
    /// Description of the toast
    pub description: String,
    /// Whether the toast is an error
    pub error: bool,
    /// Duration of the toast in milliseconds
    pub duration: Option<u32>,
}

/// Sent on startup or when media configuration changes.
#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct MediaConfigChanged {
    pub media_base_url: String,
    pub cache_dir: String,
}

/// Sent once on startup with build/version information.
#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct AppVersionInfo {
    /// Crate version (from Cargo.toml)
    pub backend_version: String,
    /// Rust profile (debug/release)
    pub profile: String,
    /// rustc version string
    pub rustc_version: String,
    /// RFC2822 UTC build time
    pub built_time_utc: String,
    /// Full git commit hash, if available
    pub git_commit_hash: Option<String>,
    /// Short git commit hash, if available
    pub git_commit_hash_short: Option<String>,
    /// Repo dirty flag, if available
    pub git_dirty: Option<bool>,
}

impl Toast {
    pub fn send(title: String, description: String, error: bool, duration: Option<Duration>) {
        let duration_ms = duration.map(|d| d.as_millis() as u32);
        debug!(
            title = title,
            description = description,
            error = error,
            duration_ms = duration_ms,
            "Sending toast"
        );
        Toast { title, description, error, duration: duration_ms }.send_signal_to_dart();
    }
}
