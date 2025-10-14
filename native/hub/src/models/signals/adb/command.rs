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
    /// Windows-only: Start Meta Quest Casting tool against the current device
    StartCasting,
    /// Connect to a specific device by its serial
    ConnectTo(String),
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct AdbRequest {
    pub command: AdbCommand,
    /// Arbitrary identifier to correlate completion events with UI elements
    pub command_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub enum AdbCommandKind {
    LaunchApp,
    ForceStopApp,
    UninstallPackage,
    Reboot,
    ProximitySensorSet,
    GuardianPausedSet,
    StartCasting,
    ConnectTo,
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
    pub command_type: AdbCommandKind,
    pub command_key: String,
    pub success: bool,
}
