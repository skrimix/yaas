use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommand {
    LaunchApp(String),
    ForceStopApp(String),
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct AdbRequest {
    pub command: AdbCommand,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct AdbResponse {
    pub command: AdbCommand,
    pub success: bool,
    pub message: String,
}
