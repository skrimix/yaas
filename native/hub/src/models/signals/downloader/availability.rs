use rinf::RustSignal;

#[derive(Default, serde::Serialize, serde::Deserialize, RustSignal)]
pub(crate) struct DownloaderAvailabilityChanged {
    pub available: bool,
    pub initializing: bool,
    pub error: Option<String>,
    /// Optional ID of the active downloader config.
    pub config_id: Option<String>,
    pub is_donation_configured: bool,
    pub supports_remote_selection: bool,
    pub supports_bandwidth_limit: bool,
    pub supports_download_mode_selection: bool,
    /// True when no managed downloader configs exist and user needs to configure one.
    /// False when at least one managed config exists (even if initialization is in progress or failed).
    pub needs_setup: bool,
}
