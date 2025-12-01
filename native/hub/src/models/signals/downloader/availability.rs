use rinf::RustSignal;

#[derive(serde::Serialize, serde::Deserialize, RustSignal)]
pub(crate) struct DownloaderAvailabilityChanged {
    pub available: bool,
    pub initializing: bool,
    pub error: Option<String>,
    /// Optional ID of the currently configured downloader.json
    pub config_id: Option<String>,
    pub is_donation_configured: bool,
    /// True when no downloader.json exists and user needs to configure one.
    /// False when config exists (even if initialization is in progress or failed).
    pub needs_setup: bool,
}
