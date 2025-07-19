use rinf::{DartSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommand {
    LaunchApp(String),
    ForceStopApp(String),
    UninstallPackage(String),
    RefreshDevice,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct AdbRequest {
    pub command: AdbCommand,
}
