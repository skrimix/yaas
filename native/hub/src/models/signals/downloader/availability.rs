use rinf::RustSignal;

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub(crate) struct DownloaderAvailabilityChanged {
    pub available: bool,
    pub initializing: bool,
    pub error: Option<String>,
    /// Optional ID of the currently configured downloader.json
    pub config_id: Option<String>,
    pub is_donation_configured: bool,
}
