use rinf::RustSignal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, RustSignal, PartialEq)]
pub enum AdbState {
    ServerNotRunning,
    ServerStarting,
    NoDevices,
    DevicesAvailable(Vec<String>),
    DeviceUnauthorized,
    DeviceConnected,
}

impl Default for AdbState {
    fn default() -> Self {
        Self::ServerNotRunning
    }
}
