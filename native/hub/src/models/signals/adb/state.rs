use rinf::RustSignal;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, RustSignal, PartialEq, Default)]
pub enum AdbState {
    #[default]
    ServerNotRunning,
    ServerStarting,
    NoDevices,
    DevicesAvailable(Vec<String>),
    DeviceUnauthorized,
    DeviceConnected,
    ServerStartFailed,
}
