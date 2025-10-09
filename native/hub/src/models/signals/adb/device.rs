use rinf::{RustSignal, SignalPiece};
use serde::Serialize;

use crate::models::{
    InstalledPackage, SpaceInfo, vendor::quest_controller::HeadsetControllersInfo,
};

#[derive(Serialize, SignalPiece)]
pub struct AdbDevice {
    pub name: Option<String>,
    pub product: String,
    pub serial: String,
    pub true_serial: String,
    pub is_wireless: bool,
    pub battery_level: u8,
    pub controllers: HeadsetControllersInfo,
    pub space_info: SpaceInfo,
    pub installed_packages: Vec<InstalledPackage>,
}

#[derive(Serialize, RustSignal)]
pub struct DeviceChangedEvent {
    pub device: Option<AdbDevice>,
}

impl From<crate::adb::device::AdbDevice> for AdbDevice {
    fn from(device: crate::adb::device::AdbDevice) -> Self {
        AdbDevice {
            name: device.name,
            product: device.product,
            serial: device.serial,
            true_serial: device.true_serial,
            is_wireless: device.is_wireless,
            battery_level: device.battery_level,
            controllers: device.controllers,
            space_info: device.space_info,
            installed_packages: device.installed_packages,
        }
    }
}
