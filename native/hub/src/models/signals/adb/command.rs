use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub(crate) enum AdbCommand {
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
    /// Enable ADB over Wi‑Fi on the current device and connect to it
    EnableWirelessAdb,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub(crate) struct AdbRequest {
    pub command: AdbCommand,
    /// Arbitrary identifier to correlate completion events with UI elements
    pub command_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub(crate) enum AdbCommandKind {
    LaunchApp,
    ForceStopApp,
    UninstallPackage,
    Reboot,
    ProximitySensorSet,
    GuardianPausedSet,
    StartCasting,
    ConnectTo,
    WirelessAdbEnable,
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece)]
pub(crate) enum RebootMode {
    Normal,
    Bootloader,
    Recovery,
    Fastboot,
    PowerOff,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub(crate) struct AdbCommandCompletedEvent {
    pub command_type: AdbCommandKind,
    pub command_key: String,
    pub success: bool,
}
