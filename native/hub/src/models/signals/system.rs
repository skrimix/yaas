use rinf::RustSignal;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RustPanic {
    pub message: String,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct Toast {
    pub title: String,
    pub description: String,
    pub error: bool,
    pub duration: Option<u32>,
}

impl Toast {
    pub fn send(title: String, description: String, error: bool, duration: Option<u32>) {
        Toast { title, description, error, duration }.send_signal_to_dart();
    }
}
