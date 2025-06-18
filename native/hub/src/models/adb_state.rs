use rinf::RustSignal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, RustSignal)]
pub enum AdbState {
    ServerNotRunning,
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
