use rinf::{DartSignal, SignalPiece};
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
