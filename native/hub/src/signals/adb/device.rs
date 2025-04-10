use rinf::{RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, SignalPiece)]
pub enum DeviceType {
    Unknown,
    Quest,
    Quest2,
    Quest3,
    Quest3S,
    QuestPro,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub enum ControllerStatus {
    Unknown,
    Active,
    Disabled,
    Searching,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct ControllerInfo {
    pub battery_level: Option<u8>,
    pub status: ControllerStatus,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct ControllersInfo {
    pub left: Option<ControllerInfo>,
    pub right: Option<ControllerInfo>,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct SpaceInfo {
    pub total: u64,
    pub available: u64,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct AppSize {
    pub app: u64,
    pub data: u64,
    pub cache: u64,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct InstalledPackage {
    pub uid: u64,
    pub system: bool,
    pub package_name: String,
    pub version_code: u64,
    pub version_name: String,
    pub label: String,
    pub launchable: bool,
    pub vr: bool,
    pub size: AppSize,
}

#[derive(Serialize, Deserialize, SignalPiece)]
pub struct AdbDevice {
    pub name: String,
    pub product: String,
    pub device_type: DeviceType,
    pub serial: String,
    pub battery_level: u8,
    pub controllers: ControllersInfo,
    pub space_info: SpaceInfo,
    pub installed_packages: Vec<InstalledPackage>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct DeviceChangedEvent {
    pub device: Option<AdbDevice>,
}
