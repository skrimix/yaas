use forensic_adb::DeviceState;
use rinf::{RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece, PartialEq, Eq)]
pub(crate) enum AdbBriefState {
    Offline,
    Bootloader,
    Device,
    Host,
    Recovery,
    NoPermissions,
    Sideload,
    Unauthorized,
    Authorizing,
    Unknown,
}

impl From<DeviceState> for AdbBriefState {
    fn from(state: DeviceState) -> Self {
        match state {
            DeviceState::Device => Self::Device,
            DeviceState::Offline => Self::Offline,
            DeviceState::Bootloader => Self::Bootloader,
            DeviceState::Host => Self::Host,
            DeviceState::Recovery => Self::Recovery,
            DeviceState::NoPermissions => Self::NoPermissions,
            DeviceState::Sideload => Self::Sideload,
            DeviceState::Unauthorized => Self::Unauthorized,
            DeviceState::Authorizing => Self::Authorizing,
            DeviceState::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, SignalPiece, PartialEq, Eq)]
pub(crate) struct AdbDeviceBrief {
    pub serial: String,
    pub is_wireless: bool,
    pub state: AdbBriefState,
    /// Optional friendly name if available (only for ready devices we can query)
    pub name: Option<String>,
    pub true_serial: Option<String>,
}

#[derive(Debug, Clone, Serialize, RustSignal, PartialEq)]
pub(crate) struct AdbDevicesList {
    pub value: Vec<AdbDeviceBrief>,
}
