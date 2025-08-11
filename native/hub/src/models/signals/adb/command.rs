use rinf::{DartSignal, RustSignal, SignalPiece};
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

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommandType {
    LaunchApp,
    ForceStopApp,
    UninstallPackage,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct AdbCommandCompletedEvent {
    pub command_type: AdbCommandType,
    pub package_name: String,
    pub success: bool,
}
