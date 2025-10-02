use rinf::{RustSignal, SignalPiece};
use serde::Serialize;

use crate::models::{
    DeviceType, InstalledPackage, SpaceInfo, vendor::quest_controller::HeadsetControllersInfo,
};

#[derive(Serialize, SignalPiece)]
pub struct AdbDevice {
    pub name: String,
    pub product: String,
    pub device_type: DeviceType,
    pub serial: String,
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
            device_type: device.device_type,
            serial: device.serial,
            battery_level: device.battery_level,
            controllers: device.controllers,
            space_info: device.space_info,
            installed_packages: device.installed_packages,
        }
    }
}
