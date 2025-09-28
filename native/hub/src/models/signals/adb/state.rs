use rinf::RustSignal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, RustSignal, PartialEq)]
pub enum AdbState {
    ServerNotRunning,
    ServerStarting,
    NoDevices,
    DevicesAvailable(Vec<String>),
    DeviceUnauthorized { count: u32 },
    DeviceConnected { count: u32 },
}

impl Default for AdbState {
    fn default() -> Self {
        Self::ServerNotRunning
    }
}
