use std::time::Duration;

use rinf::RustSignal;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RustPanic {
    pub message: String,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct Toast {
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
pub struct MediaConfigChanged {
    pub media_base_url: String,
    pub cache_dir: String,
}

impl Toast {
    pub fn send(title: String, description: String, error: bool, duration: Option<Duration>) {
        Toast { title, description, error, duration: duration.map(|d| d.as_millis() as u32) }
            .send_signal_to_dart();
    }
}
