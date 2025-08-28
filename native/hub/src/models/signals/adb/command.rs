use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommand {
    LaunchApp(String),
    ForceStopApp(String),
    UninstallPackage(String),
    RefreshDevice,
    Reboot(RebootMode),
    SetProximitySensor(bool),
    SetGuardianPaused(bool),
    GetBatteryDump,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct AdbRequest {
    pub command: AdbCommand,
    /// Arbitrary identifier to correlate completion events with UI elements
    pub command_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommandType {
    LaunchApp,
    ForceStopApp,
    UninstallPackage,
    Reboot,
    ProximitySensorSet,
    GuardianPausedSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum RebootMode {
    Normal,
    Bootloader,
    Recovery,
    Fastboot,
    PowerOff,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct AdbCommandCompletedEvent {
    pub command_type: AdbCommandType,
    pub command_key: String,
    pub success: bool,
}
