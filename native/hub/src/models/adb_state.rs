use forensic_adb::DeviceBrief;

#[derive(Debug, Clone)]
pub enum AdbState {
    ServerNotRunning,
    NoDevices,
    DevicesAvailable(Vec<DeviceBrief>),
    DeviceNotAuthorized,
    DeviceConnected,
}

impl Default for AdbState {
    fn default() -> Self {
        Self::ServerNotRunning
    }
}
