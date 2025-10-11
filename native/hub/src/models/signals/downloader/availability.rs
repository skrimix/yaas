use rinf::RustSignal;

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub struct DownloaderAvailabilityChanged {
    pub available: bool,
    pub initializing: bool,
    pub error: Option<String>,
}
