use rinf::{RustSignal, SignalPiece};
use serde::Serialize;

use crate::{
    adb,
    models::{InstalledPackage, SpaceInfo, vendor::quest_controller::HeadsetControllersInfo},
};

#[derive(Serialize, SignalPiece)]
pub(crate) struct AdbDevice {
    pub name: Option<String>,
    pub product: String,
    pub serial: String,
    pub true_serial: String,
    pub transport_id: String,
    pub is_wireless: bool,
    pub battery_level: u8,
    pub controllers: HeadsetControllersInfo,
    pub space_info: SpaceInfo,
    pub installed_packages: Vec<InstalledPackage>,
    /// Whether the Guardian system is currently paused on the device
    pub guardian_paused: Option<bool>,
}

#[derive(Serialize, RustSignal)]
pub(crate) struct DeviceChangedEvent {
    pub device: Option<AdbDevice>,
}

impl From<adb::device::AdbDevice> for AdbDevice {
    fn from(device: adb::device::AdbDevice) -> Self {
        AdbDevice {
            name: device.name,
            product: device.product,
            serial: device.serial,
            true_serial: device.true_serial,
            transport_id: device.transport_id,
            is_wireless: device.is_wireless,
            battery_level: device.battery_level,
            controllers: device.controllers,
            space_info: device.space_info,
            installed_packages: device.installed_packages,
            guardian_paused: device.guardian_paused,
        }
    }
}
